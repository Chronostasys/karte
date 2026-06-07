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
use crate::pass::register_allocation::types::{AllocationTargetInfo, RegisterAllocationResult};
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

    /// 解析被溢出到栈的虚拟寄存器操作数
    ///
    /// 寄存器分配后，虚拟寄存器可能被溢出到栈帧上的 slot。
    /// 此方法检查操作数是否为已溢出的虚拟寄存器，
    /// 如果是，则从栈帧加载其值到临时寄存器，返回临时寄存器操作数。
    /// 如果不是溢出寄存器（已由 rewrite_registers 替换为物理寄存器），
    /// 则原样返回。
    fn resolve_spilled_operand(
        &self,
        op: &Operand,
        function: &LirFunction,
        analyses: &AnalysisManager,
        span: &Span,
        temp_reg: Register,
        instructions: &mut Vec<Instruction>,
    ) -> Operand {
        match op {
            Operand::Register { id: reg_id @ Register::Virtual(_) } => {
                if let Some(ra) = analyses.get_result::<RegisterAllocationResult>("register-allocation") {
                    if let Some(target_info) = ra.allocation_map.get(reg_id) {
                        match target_info {
                            AllocationTargetInfo::Spill(slot_id) => {
                                if let Some(&fp_offset) = function.spill_slot_offsets.get(slot_id) {
                                    instructions.push(Instruction::Load64 {
                                        dst: temp_reg,
                                        addr: Register::Physical(self.calling_convention.frame_pointer),
                                        offset: fp_offset,
                                        span: *span,
                                    });
                                    return Operand::Register { id: temp_reg };
                                }
                            }
                            AllocationTargetInfo::Register(_) => {
                                // 已被 rewrite_registers 替换为物理寄存器，无需处理
                            }
                        }
                    }
                }
                op.clone()
            }
            _ => op.clone(),
        }
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
    ///
    /// 策略：保守保存所有 caller-saved 寄存器，确保正确性。
    /// 精确的 liveness 分析需要完整的数据流分析（考虑循环、条件跳转等），
    /// 目前不实现。
    #[allow(dead_code)]
    fn get_live_caller_saved_registers_precise(
        &self,
        instruction_index: usize,
        function: &LirFunction,
        caller_saved_convention: &HashSet<u8>,
    ) -> HashSet<u8> {
        let mut live_regs: HashSet<u8> = HashSet::new();
        for instr in function.instructions.iter().skip(instruction_index + 1) {
            if matches!(instr, Instruction::Return { .. } | Instruction::Jump { .. }) {
                break;
            }
            for reg in instr.get_used_registers() {
                if let Register::Physical(phys) = reg {
                    live_regs.insert(phys);
                }
            }
        }
        caller_saved_convention.intersection(&live_regs).copied().collect()
    }

    /// 保守策略：保存所有 caller-saved 寄存器
    fn get_live_caller_saved_registers_at(
        &self,
        _instruction_index: usize,
        _function: &LirFunction,
        caller_saved_convention: &HashSet<u8>,
    ) -> HashSet<u8> {
        caller_saved_convention.clone()
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
    ///
    /// 参数传递机制：
    /// - 前 register_passing_limit 个参数通过物理寄存器传递
    ///   （argument_registers + overflow_argument_registers）
    /// - 超过 limit 的参数通过虚拟栈传递
    ///
    /// 虚拟栈布局（callee 入口时，从 vm_sp 由低到高）：
    ///   [return_address]       ← vm_sp
    ///   [stack_param_0]        ← vm_sp + 16
    ///   [stack_param_1]        ← vm_sp + 32
    ///   ...
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

        let register_limit = self.calling_convention.register_passing_limit();
        let overflow_regs = self.calling_convention.overflow_argument_registers();
        let num_stack_params = arg_operands.len().saturating_sub(register_limit);

        // 获取需要保存的调用者保存寄存器
        let live_caller_saved =
            self.get_live_caller_saved_registers_at(instruction_index, function, &self.calling_convention.caller_saved);
        let mut caller_saved: Vec<_> = live_caller_saved.into_iter().collect();
        
        // 溢出参数使用 callee-saved 寄存器传递，需要在调用前保存/恢复
        let overflow_arg_count = arg_operands.len().saturating_sub(self.calling_convention.argument_registers.len());
        for i in 0..overflow_arg_count.min(overflow_regs.len()) {
            if !caller_saved.contains(&overflow_regs[i]) {
                caller_saved.push(overflow_regs[i]);
            }
        }
        
        caller_saved.sort();

        log::debug!(
            "函数调用：保存 {} 个寄存器, {} 个寄存器参数, {} 个栈参数 f: {}",
            caller_saved.len(),
            arg_operands.len().min(register_limit),
            num_stack_params,
            function.name
        );

        // 保存caller-saved寄存器到栈，使用StorePair优化
        self.save_registers_to_stack(&caller_saved, span, instructions);

        // === 参数传递 ===
        // 使用虚拟栈作为中间存储，避免寄存器覆盖（如 mov #p2, #p4; mov #p3, #p2）
        // 
        // 步骤：
        // 1. 将栈传参数逆序压入虚拟栈（callee 从 vm_sp+16 开始顺序读取）
        // 2. 将寄存器传参数顺序压入虚拟栈
        // 3. 从虚拟栈逆序弹出寄存器传参数到物理寄存器
        // 4. 栈传参数留在虚拟栈上（位于返回地址之上）

        // 使用 RAX（返回值寄存器）作为临时寄存器，避免与参数寄存器冲突
        // RAX 是 caller-saved（已在上方保存到虚拟栈），且不是参数寄存器
        let temp_operand_reg = Register::Physical(self.calling_convention.return_register);

        // 步骤 1：逆序压入栈传参数
        // 这样 callee 可以从 vm_sp+16 开始顺序读取
        for i in (0..num_stack_params).rev() {
            let op = &arg_operands[register_limit + i];
            let resolved_op = self.resolve_spilled_operand(
                op, function, analyses, span, temp_operand_reg, instructions,
            );
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
                src: resolved_op,
                span: *span,
            });
        }

        // 步骤 2：顺序压入寄存器传参数
        for i in 0..arg_operands.len().min(register_limit) {
            let op = &arg_operands[i];
            let resolved_op = self.resolve_spilled_operand(
                op, function, analyses, span, temp_operand_reg, instructions,
            );
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
                src: resolved_op,
                span: *span,
            });
        }

        // 步骤 3：逆序弹出寄存器传参数到物理寄存器
        let arg_reg_count = self.calling_convention.argument_registers.len();
        for i in (0..arg_operands.len().min(register_limit)).rev() {
            let phys_reg = if i < arg_reg_count {
                self.calling_convention.argument_registers[i]
            } else {
                overflow_regs[i - arg_reg_count]
            };
            instructions.push(Instruction::Load64 {
                dst: Register::Physical(phys_reg),
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

        // 此时虚拟栈上只剩下栈传参数（顺序排列，第一个在栈顶）

        // 压入返回地址（在栈传参数之上）
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

        // 清理栈传参数占用的虚拟栈空间
        if num_stack_params > 0 {
            instructions.push(Instruction::Add {
                dst: self.stack_pointer_reg,
                src1: Operand::Register {
                    id: self.stack_pointer_reg,
                },
                src2: Operand::Immediate { value: (num_stack_params * 16) as i64 },
                span: *span,
            });
        }

        // 将返回值暂存到 vm_sp 下方的安全位置（不改变 vm_sp）
        let return_reg = self.calling_convention.return_register;
        instructions.push(Instruction::Store64 {
            addr: self.stack_pointer_reg,
            offset: -8,
            src: Operand::Register {
                id: Register::Physical(return_reg),
            },
            span: *span,
        });

        // 恢复caller-saved寄存器
        self.restore_registers_from_stack(&caller_saved, span, instructions);

        // 处理返回值：从暂存位置加载到 result 寄存器
        if let Some(result_reg) = result {
            let total_size = caller_saved.len() * 8;
            let aligned_size = self.align_stack_size(total_size);
            let restore_offset = -(aligned_size as i64) - 8;
            instructions.push(Instruction::Load64 {
                dst: *result_reg,
                addr: self.stack_pointer_reg,
                offset: restore_offset,
                span: *span,
            });
        }

        Ok(())
    }

    /// 降级CallIndirect指令
    ///
    /// 与 Call 相同的参数传递机制，额外处理函数指针。
    /// effect_resume_temp (RAX) 用于保存函数指针。
    /// RAX 是 caller-saved 寄存器（已被保存），且不是参数寄存器，
    /// 因此在参数弹出过程中不会被覆盖，无需通过虚拟栈暂存。
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

        let register_limit = self.calling_convention.register_passing_limit();
        let overflow_regs = self.calling_convention.overflow_argument_registers();
        let num_stack_params = arg_operands.len().saturating_sub(register_limit);

        // 获取需要保存的调用者保存寄存器
        let live_caller_saved =
            self.get_live_caller_saved_registers_at(instruction_index, function, &self.calling_convention.caller_saved);
        let mut caller_saved: Vec<_> = live_caller_saved.into_iter().collect();
        
        // 溢出参数使用 callee-saved 寄存器传递，需要在调用前保存/恢复
        let overflow_arg_count = arg_operands.len().saturating_sub(self.calling_convention.argument_registers.len());
        for i in 0..overflow_arg_count.min(overflow_regs.len()) {
            if !caller_saved.contains(&overflow_regs[i]) {
                caller_saved.push(overflow_regs[i]);
            }
        }
        
        caller_saved.sort();

        log::debug!(
            "间接函数调用：保存 {} 个寄存器, {} 个寄存器参数, {} 个栈参数",
            caller_saved.len(),
            arg_operands.len().min(register_limit),
            num_stack_params,
        );

        // 保存caller-saved寄存器到栈
        self.save_registers_to_stack(&caller_saved, span, instructions);

        // 将函数地址移动到临时寄存器（RAX = effect_resume_temp）
        // RAX 是 caller-saved，已在上方保存到虚拟栈。
        // RAX 不是参数寄存器（argument_registers 和 overflow_regs 都不包含 RAX），
        // 因此后续的参数弹出不会覆盖 RAX 中的函数指针。
        let temp_func_reg = self.effect_resume_temp_register();
        instructions.push(Instruction::Move {
            dst: temp_func_reg,
            src: Operand::Register {
                id: *function_register,
            },
            span: *span,
        });

        // === 参数传递 ===
        // 与 Call 相同的机制：
        // 1. 逆序压入栈传参数
        // 2. 顺序压入寄存器传参数
        // 3. 逆序弹出寄存器传参数到物理寄存器
        // 4. 栈传参数留在虚拟栈上

        // 使用 RAX（返回值寄存器）作为临时寄存器，避免与参数寄存器冲突
        // RAX 是 caller-saved（已在上方保存到虚拟栈），且不是参数寄存器
        let temp_operand_reg = Register::Physical(self.calling_convention.return_register);

        // 步骤 1：逆序压入栈传参数
        for i in (0..num_stack_params).rev() {
            let op = &arg_operands[register_limit + i];
            let resolved_op = self.resolve_spilled_operand(
                op, function, analyses, span, temp_operand_reg, instructions,
            );
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
                src: resolved_op,
                span: *span,
            });
        }

        // 步骤 2：顺序压入寄存器传参数
        for i in 0..arg_operands.len().min(register_limit) {
            let op = &arg_operands[i];
            let resolved_op = self.resolve_spilled_operand(
                op, function, analyses, span, temp_operand_reg, instructions,
            );
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
                src: resolved_op,
                span: *span,
            });
        }

        // 步骤 3：逆序弹出寄存器传参数到物理寄存器
        let arg_reg_count = self.calling_convention.argument_registers.len();
        for i in (0..arg_operands.len().min(register_limit)).rev() {
            let phys_reg = if i < arg_reg_count {
                self.calling_convention.argument_registers[i]
            } else {
                overflow_regs[i - arg_reg_count]
            };
            instructions.push(Instruction::Load64 {
                dst: Register::Physical(phys_reg),
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

        // 栈传参数留在虚拟栈上，RAX 仍持有函数指针（未被参数弹出覆盖）

        // 压入返回地址（在栈传参数之上）
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

        // 间接跳转（通过 RAX 中的函数指针）
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

        // 清理栈传参数占用的虚拟栈空间
        if num_stack_params > 0 {
            instructions.push(Instruction::Add {
                dst: self.stack_pointer_reg,
                src1: Operand::Register {
                    id: self.stack_pointer_reg,
                },
                src2: Operand::Immediate { value: (num_stack_params * 16) as i64 },
                span: *span,
            });
        }

        // 将返回值暂存到 vm_sp 下方的安全位置
        let return_reg = self.calling_convention.return_register;
        instructions.push(Instruction::Store64 {
            addr: self.stack_pointer_reg,
            offset: -8,
            src: Operand::Register {
                id: Register::Physical(return_reg),
            },
            span: *span,
        });

        // 恢复caller-saved寄存器
        self.restore_registers_from_stack(&caller_saved, span, instructions);

        // 处理返回值
        if let Some(result_reg) = result {
            let total_size = caller_saved.len() * 8;
            let aligned_size = self.align_stack_size(total_size);
            let restore_offset = -(aligned_size as i64) - 8;
            instructions.push(Instruction::Load64 {
                dst: *result_reg,
                addr: self.stack_pointer_reg,
                offset: restore_offset,
                span: *span,
            });
        }

        Ok(())
    }

    /// 降级Phi指令
    ///
    /// Phi 指令不应该出现在指令降级阶段，说明 SSA 降级不完整。
    /// 直接 panic 以便暴露问题，而非静默使用第一个 incoming 值导致语义错误。
    fn lower_phi(
        &mut self,
        _dst: &Register,
        incoming: &[(crate::LabelId, Operand)],
        _span: &Span,
        _instructions: &mut Vec<Instruction>,
    ) -> crate::Result<()> {
        panic!(
            "Phi 指令出现在降级阶段，这表明 SSA 降级不完整。Phi 目标: {:?}, incoming 数量: {}",
            _dst, incoming.len()
        );
    }

    /// 消除冗余Move指令
    /// 
    /// 根因：多个编译pass（phi elimination、transformation等）在前驱块末尾插入
    /// Move指令。block layout 合并块后，这些 Move 在物理序列中出现在后继块的
    /// Load64（从vm_fp栈帧加载）之后，覆盖了正确的值。
    /// 
    /// 策略：扫描指令序列，找到 Load64(addr=vm_fp) 后被同一基本块内后续 Move
    /// 覆盖同一 dst 寄存器的模式。如果 Load64 和 Move 之间没有指令使用 Load64
    /// 的值，则 Move 是冗余的，予以消除。
    fn eliminate_redundant_moves(instructions: &mut Vec<Instruction>) {
        if instructions.is_empty() {
            return;
        }

        let has_phi = instructions.iter().any(|i| matches!(i, Instruction::Phi { .. }));
        if !has_phi {
            return;
        }

        let len = instructions.len();
        let mut remove_set = std::collections::HashSet::new();

        // vm_fp 通常是 Physical(11)
        const VM_FP: u8 = 11;

        // 扫描所有指令，寻找 Load64(vm_fp) 后紧跟冗余 Move 的模式
        let mut i = 0;
        while i < len - 1 {
            // 检测模式：Load64 dst=Rd, addr=VM_FP, offset=N 紧跟 Move dst=Rd, src=Register
            if let Instruction::Load64 { dst: Register::Physical(dst_phys), addr: Register::Physical(addr_phys), .. } = &instructions[i] {
                if *addr_phys == VM_FP {
                    // 检查下一条指令是否是覆盖同一寄存器的 dummy/phi Move
                    if let Instruction::Move { dst: Register::Physical(move_dst), src: Operand::Register { .. }, span } = &instructions[i + 1] {
                        let is_dummy = span.start == 0 && span.end == 0;
                        let is_phi = span.start == usize::MAX && span.end == usize::MAX;
                        if (is_dummy || is_phi) && *move_dst == *dst_phys {
                            // Load64 的值未被中间指令使用（它们紧邻，所以一定没有被使用）
                            remove_set.insert(i + 1);
                        }
                    }
                }
            }
            i += 1;
        }

        if !remove_set.is_empty() {
            log::debug!("消除了 {} 条冗余 Move 指令（覆盖 Load64 from vm_fp）", remove_set.len());
            let new: Vec<_> = instructions.drain(..)
                .enumerate()
                .filter(|(i, _)| !remove_set.contains(i))
                .map(|(_, instr)| instr)
                .collect();
            *instructions = new;
        }
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
        // 从 AnalysisManager 获取目标架构的调用约定（支持 cross-compile）
        self.calling_convention = analyses.get_calling_convention();
        self.stack_pointer_reg = Register::Physical(self.calling_convention.stack_pointer);
        self.frame_pointer_reg = Register::Physical(self.calling_convention.frame_pointer);

        let mut new_instructions = Vec::new();
        let mut changed = false;
        let instructions_to_process = std::mem::take(&mut function.instructions);

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

                        // 仅在 main 函数初始化 effect 栈指针
                        if function.name == "main" || function.name == "main::main" || function.name.ends_with("::__script_entry__") {
                            new_instructions.push(Instruction::Move {
                                dst: self.effect_stack_register(),
                                src: Operand::Immediate { value: 0 },
                                span: Span::dummy(),
                            });
                        }

                        // === 加载栈传递参数 ===
                        // 对于超过 register_passing_limit 的参数，caller 将它们留在
                        // 虚拟栈上（位于返回地址之上）。此时 LIR prologue 已设置好
                        // vm_fp 并分配了帧空间，可以计算栈参数的偏移。
                        //
                        // 虚拟栈布局（从低到高）：
                        //   vm_sp → [frame space]                ← frame_size 字节
                        //            [callee-saved N-1]           ← N 个 × 16 字节
                        //            ...
                        //            [callee-saved 0]
                        //            [old_fp, old_sp]             ← 16 字节
                        //            [return_address]             ← 16 字节
                        //   vm_fp = vm_sp + frame_size
                        //            [stack_param_0]              ← vm_fp + (N+2)*16
                        //            [stack_param_1]              ← vm_fp + (N+2)*16 + 16
                        //            ...
                        //
                        // 栈参数 i 从 vm_fp 的偏移：base_offset + 16 * i
                        // 其中 base_offset = (N_callee + 2) * 16

                        let register_limit = self.calling_convention.register_passing_limit();
                        let num_stack_params = function.parameter_registers.len().saturating_sub(register_limit);

                        if num_stack_params > 0 {
                            // 计算 callee-saved 寄存器使用数量
                            // 与 JIT prologue 的 get_callee_saved_registers() 保持一致
                            let n_callee = function.get_used_regs().iter()
                                .filter(|&&r| {
                                    self.calling_convention.is_callee_saved(r)
                                        && r != self.calling_convention.stack_pointer
                                        && r != self.calling_convention.frame_pointer
                                })
                                .count();

                            let base_offset = ((n_callee + 2) * 16) as i64;
                            let fp_reg = self.frame_pointer_reg;
                            // 使用返回值寄存器（RAX）作为临时寄存器，避免覆盖参数寄存器
                            // RAX 不是参数寄存器，在函数入口处是空闲的
                            let temp_reg = Register::Physical(self.calling_convention.return_register);

                            // 从虚拟栈加载每个栈传参数，写入对应的 spill slot
                            if let Some(ra) = analyses.get_result::<RegisterAllocationResult>("register-allocation") {
                                for i in 0..num_stack_params {
                                    let param_idx = register_limit + i;
                                    let param_reg = function.parameter_registers[param_idx];

                                    if let Some(AllocationTargetInfo::Spill(slot_id)) = ra.allocation_map.get(&param_reg) {
                                        if let Some(&fp_offset) = function.spill_slot_offsets.get(slot_id) {
                                            let stack_offset = base_offset + (16 * i) as i64;

                                            // 从虚拟栈加载到临时寄存器
                                            new_instructions.push(Instruction::Load64 {
                                                dst: temp_reg,
                                                addr: fp_reg,
                                                offset: stack_offset,
                                                span: Span::dummy(),
                                            });
                                            // 从临时寄存器写入 spill slot
                                            new_instructions.push(Instruction::Store64 {
                                                addr: fp_reg,
                                                offset: fp_offset,
                                                src: Operand::Register { id: temp_reg },
                                                span: Span::dummy(),
                                            });
                                        }
                                    }
                                }
                            }
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
            // 消除冗余Move：phi elimination在BlockLayout重排后产生的冗余Move覆盖问题
            Self::eliminate_redundant_moves(&mut new_instructions);

            function.instructions = new_instructions;
            PassResult::Changed
        } else {
            // 🔧 修复：将 mem::take 取走的指令放回，即使没有改变
            function.instructions = new_instructions;
            PassResult::Unchanged
        }
    }
}
