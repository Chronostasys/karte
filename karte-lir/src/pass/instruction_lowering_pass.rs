//! 通用指令降级Pass
//!
//! 负责降级非effect相关的指令，包括：
//! - Alloc
//! - Load64/Store64
//! - StructAlloc/StructFieldLoad/StructFieldStore
//! - Call/CallIndirect
//! - Return
//! - Phi

use crate::pass::lifetime_analysis_pass::LifetimeAnalysisResult;
use crate::pass::register_allocation::RegisterAllocationResult;
use crate::pass::{AnalysisManager, FunctionPass, PassResult};
use crate::{AllocationType, Instruction, LirFunction, Operand, Register, StructTypeId};
use karte_common::calling_convention::{CallingConvention, CC};
use karte_diagnostics::Span;
use std::collections::HashSet;

/// 通用指令降级Pass
#[derive(Debug)]
pub struct InstructionLoweringPass {
    /// 调用约定
    calling_convention: CallingConvention,
    /// 栈指针寄存器
    stack_pointer_reg: Register,
    /// 帧指针寄存器
    frame_pointer_reg: Register,
    /// 是否已插入提前返回
    inserted_early_return: bool,
    /// 下一个可用的虚拟寄存器ID
    next_register: usize,
}

impl InstructionLoweringPass {
    pub fn new() -> Self {
        let calling_convention = CallingConvention::standard();
        Self {
            stack_pointer_reg: Register::Physical(calling_convention.stack_pointer),
            frame_pointer_reg: Register::Physical(calling_convention.frame_pointer),
            calling_convention,
            inserted_early_return: false,
            next_register: 40,
        }
    }

    /// 创建带有栈对齐配置的InstructionLoweringPass
    pub fn new_with_alignment(stack_alignment: usize) -> Self {
        let mut calling_convention = CallingConvention::standard();
        calling_convention.stack_alignment = stack_alignment;

        Self {
            stack_pointer_reg: Register::Physical(calling_convention.stack_pointer),
            frame_pointer_reg: Register::Physical(calling_convention.frame_pointer),
            calling_convention,
            inserted_early_return: false,
            next_register: 40,
        }
    }

    /// 将大小对齐到栈边界
    fn align_stack_size(&self, size: usize) -> usize {
        (size + self.calling_convention.stack_alignment - 1)
            & !(self.calling_convention.stack_alignment - 1)
    }

    /// 使用StorePair优化保存寄存器到栈
    fn save_registers_to_stack(
        &self,
        registers: &[u8],
        span: &Span,
        instructions: &mut Vec<Instruction>,
    ) {
        // 计算总大小并对齐
        let total_size = registers.len() * 8;
        let aligned_size = self.align_stack_size(total_size);

        // 先使用StorePair优化连续的寄存器对
        let mut i = 0;
        while i + 1 < registers.len() {
            let offset = -((i + 2) as i64 * 8);
            instructions.push(Instruction::StorePair {
                addr: self.stack_pointer_reg,
                offset,
                src1: Register::Physical(registers[i]),
                src2: Register::Physical(registers[i + 1]),
                span: *span,
            });
            i += 2;
        }

        // 处理剩余的单个寄存器
        if registers.len() % 2 == 1 {
            let last_reg = registers[registers.len() - 1];
            let offset = -(((registers.len()) * 8) as i64);
            instructions.push(Instruction::Store64 {
                addr: self.stack_pointer_reg,
                offset,
                src: Operand::Register {
                    id: Register::Physical(last_reg),
                },
                span: *span,
            });
        }
        // 调整栈指针
        instructions.push(Instruction::Sub {
            dst: self.stack_pointer_reg,
            src1: Operand::Register {
                id: self.stack_pointer_reg,
            },
            src2: Operand::Immediate {
                value: aligned_size as i64,
            },
            span: *span,
        });
    }

    /// 使用LoadPair优化从栈恢复寄存器
    fn restore_registers_from_stack(
        &self,
        registers: &[u8],
        span: &Span,
        instructions: &mut Vec<Instruction>,
    ) {
        // 计算总大小并对齐
        let total_size = registers.len() * 8;
        let aligned_size = self.align_stack_size(total_size);
        let padding = aligned_size - total_size;
        // 调整栈指针
        instructions.push(Instruction::Add {
            dst: self.stack_pointer_reg,
            src1: Operand::Register {
                id: self.stack_pointer_reg,
            },
            src2: Operand::Immediate {
                value: aligned_size as i64,
            },
            span: *span,
        });
        // 使用LoadPair恢复寄存器对
        let mut i = 0;
        while i + 1 < registers.len() {
            let offset = -((i + 2) as i64 * 8);
            instructions.push(Instruction::LoadPair {
                dst1: Register::Physical(registers[i]),
                dst2: Register::Physical(registers[i + 1]),
                addr: self.stack_pointer_reg,
                offset,
                span: *span,
            });
            i += 2;
        }

        // 处理剩余的单个寄存器
        if registers.len() % 2 == 1 {
            let last_reg = registers[registers.len() - 1];
            let offset = -(((registers.len()) * 8) as i64);
            instructions.push(Instruction::Load64 {
                dst: Register::Physical(last_reg),
                addr: self.stack_pointer_reg,
                offset,
                span: *span,
            });
        }
    }

    /// 创建新的虚拟寄存器
    fn new_register(&mut self) -> Register {
        self.next_register += 1;
        Register::Virtual(self.next_register)
    }

    /// 获取effect栈指针寄存器
    fn effect_stack_register(&self) -> Register {
        Register::Physical(self.calling_convention.effect_stack_pointer)
    }

    /// 获取effect payload寄存器
    fn effect_payload_register(&self) -> Register {
        Register::Physical(self.calling_convention.effect_payload_register)
    }

    /// 获取effect resume临时寄存器
    fn effect_resume_temp_register(&self) -> Register {
        Register::Physical(self.calling_convention.effect_resume_temp)
    }

    /// 获取指定指令位置需要保存的调用者保存寄存器
    fn get_live_caller_saved_registers_at(
        &self,
        _instruction_index: usize,
        _analyses: &AnalysisManager,
    ) -> HashSet<u8> {
        // 保守策略：保存所有 caller-saved 寄存器（排除返回值寄存器 RAX）
        //
        // 原因：register allocator 会把跨调用存活的变量分配到 caller-saved 寄存器，
        // 但 lifetime analysis 不能正确识别所有需要保存的寄存器。
        // 保守策略确保正确性，代价是多保存几个寄存器。
        self.calling_convention
            .caller_saved
            .iter()
            .filter(|&&reg| reg != self.calling_convention.return_register)
            .cloned()
            .collect()
    }

    /// 降级Alloc指令
    fn lower_alloc(
        &mut self,
        dst: &Register,
        size: usize,
        alignment: usize,
        allocation_type: &AllocationType,
        span: &Span,
        instructions: &mut Vec<Instruction>,
        function: &mut LirFunction,
    ) -> crate::Result<()> {
        match allocation_type {
            AllocationType::Stack => {
                // 栈分配：SP = SP - size; dst = SP
                let stack_pointer = function.get_stack_pointer_register();

                instructions.push(Instruction::Sub {
                    dst: stack_pointer,
                    src1: Operand::Register { id: stack_pointer },
                    src2: Operand::Immediate { value: size as i64 },
                    span: *span,
                });

                instructions.push(Instruction::Move {
                    dst: *dst,
                    src: Operand::Register { id: stack_pointer },
                    span: *span,
                });
            }
            AllocationType::Heap | AllocationType::Static => {
                // 堆分配和静态分配保持原始指令
                instructions.push(Instruction::Alloc {
                    dst: *dst,
                    size,
                    alignment,
                    allocation_type: allocation_type.clone(),
                    span: *span,
                });
            }
        }
        Ok(())
    }

    /// 降级Load64指令
    fn lower_load64(
        &mut self,
        dst: &Register,
        addr: &Register,
        offset: i64,
        span: &Span,
        instructions: &mut Vec<Instruction>,
    ) -> crate::Result<()> {
        instructions.push(Instruction::Load64 {
            dst: *dst,
            addr: *addr,
            offset,
            span: *span,
        });
        Ok(())
    }

    /// 降级Store64指令
    fn lower_store64(
        &mut self,
        addr: &Register,
        offset: i64,
        src: &Operand,
        span: &Span,
        instructions: &mut Vec<Instruction>,
    ) -> crate::Result<()> {
        instructions.push(Instruction::Store64 {
            addr: *addr,
            offset,
            src: src.clone(),
            span: *span,
        });
        Ok(())
    }

    /// 降级StructAlloc指令
    fn lower_struct_alloc(
        &mut self,
        dst: &Register,
        struct_type: &StructTypeId,
        allocation_type: &AllocationType,
        span: &Span,
        instructions: &mut Vec<Instruction>,
        function: &mut LirFunction,
    ) -> crate::Result<()> {
        let struct_layout = function
            .struct_types
            .get(struct_type)
            .ok_or_else(|| format!("Unknown struct type: {:?}", struct_type))?;

        self.lower_alloc(
            dst,
            struct_layout.total_size,
            struct_layout.alignment,
            allocation_type,
            span,
            instructions,
            function,
        )
    }

    /// 降级StructFieldLoad指令
    fn lower_struct_field_load(
        &mut self,
        dst: &Register,
        struct_addr: &Register,
        field_offset: usize,
        span: &Span,
        instructions: &mut Vec<Instruction>,
    ) -> crate::Result<()> {
        self.lower_load64(dst, struct_addr, field_offset as i64, span, instructions)
    }

    /// 降级StructFieldStore指令
    fn lower_struct_field_store(
        &mut self,
        struct_addr: &Register,
        field_offset: usize,
        src: &Operand,
        span: &Span,
        instructions: &mut Vec<Instruction>,
    ) -> crate::Result<()> {
        self.lower_store64(struct_addr, field_offset as i64, src, span, instructions)
    }

    /// 降级Call指令
    fn lower_call(
        &mut self,
        target: crate::LabelId,
        arg_operands: &[Operand],
        result: &Option<Register>,
        span: &Span,
        instructions: &mut Vec<Instruction>,
        function: &mut LirFunction,
        instruction_index: usize,
        analyses: &AnalysisManager,
    ) -> crate::Result<()> {
        let return_label = function.new_label();

        // 获取需要保存的调用者保存寄存器
        let live_caller_saved =
            self.get_live_caller_saved_registers_at(instruction_index, analyses);
        let mut caller_saved: Vec<_> = live_caller_saved.into_iter().collect();
        caller_saved.sort();

        log::debug!(
            "函数调用优化：需要保存 {} 个调用者保存寄存器: {:?} f: {}",
            caller_saved.len(),
            caller_saved,
            function.name
        );

        // 保存caller-saved寄存器到栈，使用StorePair优化
        self.save_registers_to_stack(&caller_saved, span, instructions);

        // 参数传递：使用栈作为中间存储避免寄存器覆盖
        for op in arg_operands.iter() {
            instructions.push(Instruction::Sub {
                dst: self.stack_pointer_reg,
                src1: Operand::Register {
                    id: self.stack_pointer_reg,
                },
                src2: Operand::Immediate { value: 16 },
                span: *span,
            });
            instructions.push(Instruction::Store64 {
                addr: self.stack_pointer_reg,
                offset: 0,
                src: op.clone(),
                span: *span,
            });
        }

        // 从栈加载到参数寄存器（逆序）
        for i in (0..arg_operands.len()).rev() {
            if let Some(phys_reg) = self.calling_convention.argument_registers.get(i) {
                instructions.push(Instruction::Load64 {
                    dst: Register::Physical(*phys_reg),
                    addr: self.stack_pointer_reg,
                    offset: 0,
                    span: *span,
                });
                instructions.push(Instruction::Add {
                    dst: self.stack_pointer_reg,
                    src1: Operand::Register {
                        id: self.stack_pointer_reg,
                    },
                    src2: Operand::Immediate { value: 16 },
                    span: *span,
                });
            }
        }

        // 压入返回地址
        instructions.push(Instruction::Sub {
            dst: self.stack_pointer_reg,
            src1: Operand::Register {
                id: self.stack_pointer_reg,
            },
            src2: Operand::Immediate { value: 16 },
            span: *span,
        });
        instructions.push(Instruction::Store64 {
            addr: self.stack_pointer_reg,
            offset: 0,
            src: Operand::Label { id: return_label },
            span: *span,
        });

        // 跳转到目标函数
        instructions.push(Instruction::Jump {
            target,
            span: *span,
        });

        // 返回标签
        instructions.push(Instruction::Label {
            id: return_label,
            span: *span,
        });

        // 弹出返回地址
        instructions.push(Instruction::Add {
            dst: self.stack_pointer_reg,
            src1: Operand::Register {
                id: self.stack_pointer_reg,
            },
            src2: Operand::Immediate { value: 16 },
            span: *span,
        });

        // 恢复caller-saved寄存器，使用LoadPair优化
        self.restore_registers_from_stack(&caller_saved, span, instructions);

        // 处理返回值
        if let Some(result_reg) = result {
            instructions.push(Instruction::Move {
                dst: *result_reg,
                src: Operand::Register {
                    id: Register::Physical(self.calling_convention.return_register),
                },
                span: *span,
            });
        }

        Ok(())
    }

    /// 降级CallIndirect指令
    fn lower_call_indirect(
        &mut self,
        function_register: &Register,
        arg_operands: &[Operand],
        result: &Option<Register>,
        span: &Span,
        instructions: &mut Vec<Instruction>,
        function: &mut LirFunction,
        instruction_index: usize,
        analyses: &AnalysisManager,
    ) -> crate::Result<()> {
        let return_label = function.new_label();

        // 获取需要保存的调用者保存寄存器
        let live_caller_saved =
            self.get_live_caller_saved_registers_at(instruction_index, analyses);
        let mut caller_saved: Vec<_> = live_caller_saved.into_iter().collect();
        caller_saved.sort();

        log::debug!(
            "间接函数调用优化：需要保存 {} 个调用者保存寄存器: {:?}",
            caller_saved.len(),
            caller_saved
        );

        // 保存caller-saved寄存器到栈，使用StorePair优化
        self.save_registers_to_stack(&caller_saved, span, instructions);

        // 将函数地址移动到临时寄存器
        let temp_func_reg = self.effect_resume_temp_register();
        instructions.push(Instruction::Move {
            dst: temp_func_reg,
            src: Operand::Register {
                id: *function_register,
            },
            span: *span,
        });

        // 参数传递：使用栈作为中间存储避免寄存器覆盖
        for op in arg_operands.iter() {
            instructions.push(Instruction::Sub {
                dst: self.stack_pointer_reg,
                src1: Operand::Register {
                    id: self.stack_pointer_reg,
                },
                src2: Operand::Immediate { value: 16 },
                span: *span,
            });
            instructions.push(Instruction::Store64 {
                addr: self.stack_pointer_reg,
                offset: 0,
                src: op.clone(),
                span: *span,
            });
        }

        // 从栈加载到参数寄存器（逆序）
        for i in (0..arg_operands.len()).rev() {
            if let Some(phys_reg) = self.calling_convention.argument_registers.get(i) {
                instructions.push(Instruction::Load64 {
                    dst: Register::Physical(*phys_reg),
                    addr: self.stack_pointer_reg,
                    offset: 0,
                    span: *span,
                });
                instructions.push(Instruction::Add {
                    dst: self.stack_pointer_reg,
                    src1: Operand::Register {
                        id: self.stack_pointer_reg,
                    },
                    src2: Operand::Immediate { value: 16 },
                    span: *span,
                });
            }
        }

        // 压入返回地址
        instructions.push(Instruction::Sub {
            dst: self.stack_pointer_reg,
            src1: Operand::Register {
                id: self.stack_pointer_reg,
            },
            src2: Operand::Immediate { value: 16 },
            span: *span,
        });
        instructions.push(Instruction::Store64 {
            addr: self.stack_pointer_reg,
            offset: 0,
            src: Operand::Label { id: return_label },
            span: *span,
        });

        // 间接跳转
        instructions.push(Instruction::JumpIndirect {
            function_register: temp_func_reg,
            span: *span,
        });

        // 返回标签
        instructions.push(Instruction::Label {
            id: return_label,
            span: *span,
        });

        // 弹出返回地址
        instructions.push(Instruction::Add {
            dst: self.stack_pointer_reg,
            src1: Operand::Register {
                id: self.stack_pointer_reg,
            },
            src2: Operand::Immediate { value: 16 },
            span: *span,
        });

        // 恢复caller-saved寄存器，使用LoadPair优化
        self.restore_registers_from_stack(&caller_saved, span, instructions);

        // 处理返回值
        if let Some(result_reg) = result {
            instructions.push(Instruction::Move {
                dst: *result_reg,
                src: Operand::Register {
                    id: Register::Physical(self.calling_convention.return_register),
                },
                span: *span,
            });
        }

        Ok(())
    }

    /// 降级Phi指令
    fn lower_phi(
        &mut self,
        dst: &Register,
        incoming: &[(crate::LabelId, Operand)],
        span: &Span,
        instructions: &mut Vec<Instruction>,
    ) -> crate::Result<()> {
        println!("🔧 专业降级φ指令: dst={:?}, incoming={:?}", dst, incoming);

        if incoming.is_empty() {
            return Err("φ指令没有incoming值".into());
        }

        println!("⚠️  警告：φ指令出现在降级阶段，这表明SSA降级不完整");

        // 选择第一个incoming值作为fallback
        let (source_block, ref operand) = incoming[0];
        println!(
            "🔧 使用fallback策略，选择来自块 {:?} 的值: {:?}",
            source_block, operand
        );

        instructions.push(Instruction::Move {
            dst: *dst,
            src: operand.clone(),
            span: *span,
        });

        Ok(())
    }
}

impl Default for InstructionLoweringPass {
    fn default() -> Self {
        Self::new()
    }
}

impl FunctionPass for InstructionLoweringPass {
    fn name(&self) -> &str {
        "instruction-lowering"
    }

    fn description(&self) -> &str {
        "指令降级 - 将高级LIR指令降级为基础指令集"
    }

    fn required_analyses(&self) -> Vec<&'static str> {
        // 指令降级需要生命周期分析的结果来优化寄存器保存
        vec!["lifetime-analysis"]
    }

    fn invalidated_analyses(&self) -> Vec<&'static str> {
        // 指令降级会插入大量额外指令（序言、参数传递、寄存器保存等），
        // 这会改变指令索引，使得基于指令索引的生命周期分析失效
        vec!["lifetime-analysis", "cfg", "def-use", "liveness"]
    }

    fn run_on_function(
        &mut self,
        function: &mut LirFunction,
        analyses: &mut AnalysisManager,
    ) -> PassResult {
        let mut new_instructions = Vec::new();
        let mut changed = false;
        let instructions_to_process = function.instructions.clone();

        // 进入函数前重置状态
        self.inserted_early_return = false;

        // 在看到第一个 Label 后，立即发射函数序言与 effect 栈初始化
        let mut prologue_emitted = false;

        for (index, instruction) in instructions_to_process.iter().enumerate() {
            match instruction {
                Instruction::Label { id, span } => {
                    new_instructions.push(Instruction::Label {
                        id: *id,
                        span: *span,
                    });

                    if !prologue_emitted {
                        prologue_emitted = true;
                        // 序言：fp = sp
                        new_instructions.push(Instruction::Move {
                            dst: self.frame_pointer_reg,
                            src: Operand::Register {
                                id: self.stack_pointer_reg,
                            },
                            span: Span::dummy(),
                        });

                        // 预留栈帧
                        debug_assert!(function.stack_frame_size % 16 == 0);
                        if function.stack_frame_size > 0 {
                            new_instructions.push(Instruction::Sub {
                                dst: self.stack_pointer_reg,
                                src1: Operand::Register {
                                    id: self.stack_pointer_reg,
                                },
                                src2: Operand::Immediate {
                                    value: function.stack_frame_size as i64,
                                },
                                span: Span::dummy(),
                            });
                        }

                        // 仅在 main 函数初始化 effect 栈
                        if function.name == "main" {
                            new_instructions.push(Instruction::Move {
                                dst: self.effect_stack_register(),
                                src: Operand::Immediate { value: 0 },
                                span: Span::dummy(),
                            });
                        }
                    }
                }

                Instruction::Alloc {
                    dst,
                    size,
                    alignment,
                    allocation_type,
                    span,
                } => {
                    if let Err(e) = self.lower_alloc(
                        dst,
                        *size,
                        *alignment,
                        allocation_type,
                        span,
                        &mut new_instructions,
                        function,
                    ) {
                        return PassResult::Failed(format!(
                            "Failed to lower Alloc instruction: {}",
                            e
                        ));
                    }
                    changed = true;
                }

                Instruction::Load64 {
                    dst,
                    addr,
                    offset,
                    span,
                } => {
                    if let Err(e) =
                        self.lower_load64(dst, addr, *offset, span, &mut new_instructions)
                    {
                        return PassResult::Failed(format!(
                            "Failed to lower Load64 instruction: {}",
                            e
                        ));
                    }
                    changed = true;
                }

                Instruction::Store64 {
                    addr,
                    offset,
                    src,
                    span,
                } => {
                    if let Err(e) =
                        self.lower_store64(addr, *offset, src, span, &mut new_instructions)
                    {
                        return PassResult::Failed(format!(
                            "Failed to lower Store64 instruction: {}",
                            e
                        ));
                    }
                    changed = true;
                }

                Instruction::StructAlloc {
                    dst,
                    struct_type,
                    allocation_type,
                    span,
                } => {
                    if let Err(e) = self.lower_struct_alloc(
                        dst,
                        struct_type,
                        allocation_type,
                        span,
                        &mut new_instructions,
                        function,
                    ) {
                        return PassResult::Failed(format!(
                            "Failed to lower StructAlloc instruction: {}",
                            e
                        ));
                    }
                    changed = true;
                }

                Instruction::StructFieldLoad {
                    dst,
                    struct_addr,
                    field_offset,
                    span,
                } => {
                    if let Err(e) = self.lower_struct_field_load(
                        dst,
                        struct_addr,
                        *field_offset,
                        span,
                        &mut new_instructions,
                    ) {
                        return PassResult::Failed(format!(
                            "Failed to lower StructFieldLoad instruction: {}",
                            e
                        ));
                    }
                    changed = true;
                }

                Instruction::StructFieldStore {
                    struct_addr,
                    field_offset,
                    src,
                    span,
                } => {
                    if let Err(e) = self.lower_struct_field_store(
                        struct_addr,
                        *field_offset,
                        src,
                        span,
                        &mut new_instructions,
                    ) {
                        return PassResult::Failed(format!(
                            "Failed to lower StructFieldStore instruction: {}",
                            e
                        ));
                    }
                    changed = true;
                }

                Instruction::Call {
                    target,
                    args: _,
                    arg_operands,
                    result,
                    span,
                } => {
                    if let Err(e) = self.lower_call(
                        *target,
                        arg_operands,
                        result,
                        span,
                        &mut new_instructions,
                        function,
                        index,
                        analyses,
                    ) {
                        return PassResult::Failed(format!(
                            "Failed to lower Call instruction: {}",
                            e
                        ));
                    }
                    changed = true;
                }

                Instruction::CallIndirect {
                    function_register,
                    args: _,
                    arg_operands,
                    result,
                    span,
                } => {
                    if let Err(e) = self.lower_call_indirect(
                        function_register,
                        arg_operands,
                        result,
                        span,
                        &mut new_instructions,
                        function,
                        index,
                        analyses,
                    ) {
                        return PassResult::Failed(format!(
                            "Failed to lower CallIndirect instruction: {}",
                            e
                        ));
                    }
                    changed = true;
                }

                Instruction::Return { .. } => {
                    if self.inserted_early_return {
                        // 跳过重复的尾声与Return
                        new_instructions.push(Instruction::Nop {
                            span: Span::dummy(),
                        });
                    } else {
                        // 恢复栈帧
                        if function.stack_frame_size > 0 {
                            new_instructions.push(Instruction::Add {
                                dst: self.stack_pointer_reg,
                                src1: Operand::Register {
                                    id: self.stack_pointer_reg,
                                },
                                src2: Operand::Immediate {
                                    value: function.stack_frame_size as i64,
                                },
                                span: Span::dummy(),
                            });
                        }
                        new_instructions.push(instruction.clone());
                    }
                }

                Instruction::Phi {
                    dst,
                    incoming,
                    span,
                } => {
                    if let Err(e) = self.lower_phi(dst, incoming, span, &mut new_instructions) {
                        return PassResult::Failed(format!(
                            "Failed to lower Phi instruction: {}",
                            e
                        ));
                    }
                    changed = true;
                }

                _ => {
                    // effect相关指令已经被EffectLoweringPass处理，直接保留
                    new_instructions.push(instruction.clone());
                }
            }
        }

        if changed {
            function.instructions = new_instructions;
            PassResult::Changed
        } else {
            PassResult::Unchanged
        }
    }
}
