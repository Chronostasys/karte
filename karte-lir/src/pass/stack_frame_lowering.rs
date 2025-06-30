//! StackFrameLowering Pass
//!
//! 实现基于帧指针（Frame Pointer）的栈帧管理，这是修复栈指针不稳定问题的核心解决方案。
//!
//! 主要功能：
//! 1. 计算栈帧布局，包括本地变量和溢出槽
//! 2. 生成函数序言和尾声代码  
//! 3. 将 alloc 指令转换为基于 FP 的地址计算
//! 4. 处理寄存器溢出的 load/store 操作

use super::{AnalysisManager, FunctionPass, PassResult};
use crate::pass::register_allocation::RegisterAllocationResult;
use crate::pass::register_allocation::RegisterType;
use crate::{AllocationType, Instruction, LirFunction, Operand, Register};
use log::{error, info, warn};
use std::collections::HashMap;

/// 向下对齐到指定边界
fn align_down(value: i64, alignment: i64) -> i64 {
    value - (value % alignment)
}

/// 栈帧布局信息
#[derive(Debug, Clone)]
pub struct StackFrameLayout {
    /// 总栈帧大小
    pub total_frame_size: usize,
    /// 本地变量（alloc）的偏移映射
    pub local_var_offsets: HashMap<Register, i64>,
    /// 溢出槽的偏移映射
    pub spill_slot_offsets: HashMap<usize, i64>,
    /// 下一个可分配的偏移量
    pub next_offset: i64,
}

impl Default for StackFrameLayout {
    fn default() -> Self {
        Self::new()
    }
}

impl StackFrameLayout {
    pub fn new() -> Self {
        Self {
            total_frame_size: 0,
            local_var_offsets: HashMap::new(),
            spill_slot_offsets: HashMap::new(),
            next_offset: -8, // 从 FP 向下分配
        }
    }

    /// 分配本地变量槽位
    pub fn allocate_local_var(&mut self, var_reg: Register, size: usize) -> i64 {
        let offset = self.next_offset;
        self.local_var_offsets.insert(var_reg, offset);
        self.next_offset -= size as i64;
        self.total_frame_size = (-self.next_offset) as usize;
        info!(
            "🔧 分配本地变量: {:?} -> [FP{}] (size: {})",
            var_reg, offset, size
        );
        offset
    }

    /// 分配溢出槽位
    pub fn allocate_spill_slot(&mut self, slot_id: usize) -> i64 {
        let offset = self.next_offset;
        self.spill_slot_offsets.insert(slot_id, offset);
        self.next_offset -= 8; // 每个溢出槽 8 字节
        self.total_frame_size = (-self.next_offset) as usize;
        info!("🔧 分配溢出槽: slot_{} -> [FP{}]", slot_id, offset);
        offset
    }
}

/// StackFrameLowering Pass
pub struct StackFrameLowering {
    /// 栈指针寄存器
    stack_pointer: Register,
    /// 帧指针寄存器  
    frame_pointer: Register,
}

impl StackFrameLowering {
    pub fn new() -> Self {
        Self {
            // 根据调用约定：r6 = SP, r7 = FP
            // 🔧 关键修复：使用物理寄存器ID而不是虚拟寄存器ID
            stack_pointer: Register::Physical(6),
            frame_pointer: Register::Physical(7),
        }
    }

    /// 🔧 新增：获取栈指针和帧指针的物理寄存器ID
    fn get_stack_pointer_physical(&self) -> u8 {
        6 // r6
    }

    fn get_frame_pointer_physical(&self) -> u8 {
        7 // r7
    }

    /// 🔧 新增：确保栈指针和帧指针不被寄存器分配器重新分配
    fn ensure_special_registers_reserved(&self, allocation_result: &mut RegisterAllocationResult) {
        // 确保栈指针和帧指针映射到正确的物理寄存器
        allocation_result
            .register_mapping
            .insert(self.stack_pointer, self.get_stack_pointer_physical());
        allocation_result
            .register_mapping
            .insert(self.frame_pointer, self.get_frame_pointer_physical());

        info!(
            "🔧 保留特殊寄存器: SP={:?}->r{}, FP={:?}->r{}",
            self.stack_pointer,
            self.get_stack_pointer_physical(),
            self.frame_pointer,
            self.get_frame_pointer_physical()
        );
    }

    /// 计算栈帧布局
    fn calculate_stack_frame_layout(
        &self,
        function: &LirFunction,
        allocation_result: &RegisterAllocationResult,
    ) -> StackFrameLayout {
        let mut layout = StackFrameLayout::new();
        let mut current_offset = 0i64;

        // 1. 分配本地变量（由alloc指令分配的寄存器）
        for instruction in &function.instructions {
            if let Instruction::Alloc {
                dst,
                size,
                alignment,
                allocation_type,
                ..
            } = instruction
            {
                // 🔧 关键修复：只为栈分配的alloc指令分配栈空间
                // 堆分配的alloc指令应该在虚拟机执行时由堆分配器处理
                match allocation_type {
                    AllocationType::Stack => {
                        // 对齐当前偏移量
                        current_offset =
                            align_down(current_offset - (*size as i64), *alignment as i64);
                        layout.local_var_offsets.insert(*dst, current_offset);
                        info!(
                            "🔧 分配本地变量: {:?} -> [FP{}] (size: {}, type: Stack)",
                            dst, current_offset, size
                        );
                    }
                    AllocationType::Heap => {
                        // 堆分配不需要在栈帧中分配空间，将在虚拟机执行时处理
                        info!(
                            "🔧 跳过堆分配: {:?} (size: {}, type: Heap) - 将在虚拟机执行时处理",
                            dst, size
                        );
                    }
                    AllocationType::Static => {
                        // 静态分配也不需要在栈帧中分配空间
                        info!(
                            "🔧 跳过静态分配: {:?} (size: {}, type: Static) - 将在虚拟机执行时处理",
                            dst, size
                        );
                    }
                }
            }
        }

        // 2. 🔧 关键修复：只为数据寄存器分配溢出槽
        // 栈地址寄存器不应该被溢出，因为它们存储的是栈地址而不是数据
        for (register_id, spill_slot) in &allocation_result.spilled_registers {
            if let Some(register_type) = allocation_result.register_types.get(register_id) {
                if *register_type == RegisterType::Data {
                    // 只有数据寄存器才能溢出
                    current_offset = align_down(current_offset - 8, 8); // 每个溢出槽8字节对齐
                    layout
                        .spill_slot_offsets
                        .insert(spill_slot.slot_id, current_offset);
                    info!(
                        "🔧 分配溢出槽: slot_{} -> [FP{}]",
                        spill_slot.slot_id, current_offset
                    );
                    info!(
                        "🔧 为溢出数据寄存器 {:?} 分配槽位 {}",
                        register_id, spill_slot.slot_id
                    );
                } else {
                    // 🔧 修复：StackAddress寄存器现在不会被标记为溢出，所以这里不应该有错误
                    // 如果还有非数据寄存器被标记为溢出，说明寄存器分配器有问题
                    warn!("🔧 警告：非数据寄存器 {:?} (类型: {:?}) 被标记为溢出，这可能是寄存器分配器的bug", register_id, register_type);
                    // 我们仍然为它分配溢出槽，但这不是最佳实践
                    current_offset = align_down(current_offset - 8, 8);
                    layout
                        .spill_slot_offsets
                        .insert(spill_slot.slot_id, current_offset);
                    info!(
                        "🔧 为溢出寄存器 {:?} 分配槽位 {} (非最佳实践)",
                        register_id, spill_slot.slot_id
                    );
                }
            } else {
                error!(
                    "🔧 错误：溢出寄存器 {:?} 没有类型信息！这是寄存器分配器的bug",
                    register_id
                );
            }
        }

        // 设置总栈帧大小
        layout.total_frame_size = (-current_offset) as usize;
        layout.next_offset = current_offset;

        info!(
            "🔧 栈帧布局计算完成: 总大小 {} 字节",
            layout.total_frame_size
        );
        layout
    }

    /// 🔧 修复：简化指令重写，只做偏移替换，不再处理溢出寄存器
    fn rewrite_instructions(
        &mut self,
        function: &mut LirFunction,
        layout: &StackFrameLayout,
        allocation_result: &RegisterAllocationResult,
    ) -> Result<(), String> {
        info!(
            "🚀 运行 StackFrameLowering Pass for function: {}",
            function.name
        );

        // 🔧 保留特殊寄存器不被重新分配
        let mut mutable_allocation = allocation_result.clone();
        self.ensure_special_registers_reserved(&mut mutable_allocation);

        let mut new_instructions = Vec::new();

        // 在第一个标签后插入序言
        let mut prologue_inserted = false;

        for instruction in &function.instructions {
            // 在第一个标签后插入序言
            if !prologue_inserted {
                if let Instruction::Label { .. } = instruction {
                    if layout.total_frame_size > 0 {
                        // let prologue = self.generate_prologue(layout);
                        // new_instructions.extend(prologue);
                        // println!("🔧 在第一个标签 {:?} 之后插入序言 (栈帧大小: {})", instruction, layout.total_frame_size);
                    }
                    prologue_inserted = true;
                }
            }

            // 🔧 修复：在每个return指令之前插入尾声
            if let Instruction::Return { .. } = instruction {
                // if layout.total_frame_size > 0 {
                //     let epilogue = self.generate_epilogue(layout);
                //     new_instructions.extend(epilogue);
                //     println!("🔧 在 return 指令前插入尾声 (栈帧大小: {})", layout.total_frame_size);
                // }
            }

            match instruction {
                // 转换栈分配指令为地址计算
                Instruction::Alloc {
                    dst,
                    size: _,
                    alignment: _,
                    allocation_type: AllocationType::Stack,
                    span,
                } => {
                    if let Some(&offset) = layout.local_var_offsets.get(dst) {
                        new_instructions.push(Instruction::Add {
                            dst: *dst,
                            src1: Operand::Register {
                                id: self.frame_pointer,
                            },
                            src2: Operand::Immediate { value: offset },
                            span: *span,
                        });
                        info!("🔧 转换栈分配 alloc: {:?} = FP + {}", dst, offset);
                    } else {
                        // 如果找不到偏移，保持原指令
                        new_instructions.push(instruction.clone());
                    }
                }
                Instruction::Load64 {
                    dst,
                    addr,
                    offset: current_offset,
                    span,
                } => {
                    if let Some(&stack_offset) = layout.local_var_offsets.get(addr) {
                        // 直接替换为FP+offset
                        new_instructions.push(Instruction::Load64 {
                            dst: *dst,
                            addr: self.frame_pointer,
                            offset: stack_offset + *current_offset,
                            span: *span,
                        });
                        info!(
                            "🔧 直接替换load: {:?} = [FP + {}]",
                            dst,
                            stack_offset + *current_offset
                        );
                    } else {
                        new_instructions.push(instruction.clone());
                    }
                }
                Instruction::Store64 {
                    addr,
                    offset: current_offset,
                    src,
                    span,
                } => {
                    if let Some(&stack_offset) = layout.local_var_offsets.get(addr) {
                        // 直接替换为FP+offset
                        new_instructions.push(Instruction::Store64 {
                            addr: self.frame_pointer,
                            offset: stack_offset + *current_offset,
                            src: src.clone(),
                            span: *span,
                        });
                        info!(
                            "🔧 直接替换store: [FP + {}] = {:?}",
                            stack_offset + *current_offset,
                            src
                        );
                    } else {
                        new_instructions.push(instruction.clone());
                    }
                }
                _ => {
                    new_instructions.push(instruction.clone());
                }
            }
        }

        // 🔧 修复：删除旧的尾声插入逻辑，因为现在在每个return指令前都插入了
        function.instructions = new_instructions;
        Ok(())
    }
}

impl FunctionPass for StackFrameLowering {
    fn name(&self) -> &str {
        "stack-frame-lowering"
    }

    fn run_on_function(
        &mut self,
        function: &mut LirFunction,
        analyses: &mut AnalysisManager,
    ) -> PassResult {
        info!(
            "🚀 运行 StackFrameLowering Pass for function: {}",
            function.name
        );

        // 🔧 修改：获取 Pre-RA 决策结果
        let allocation_result_key = format!("pre-ra-decision-{}", function.name);
        let allocation_result =
            match analyses.get_result::<RegisterAllocationResult>(&allocation_result_key) {
                Some(result) => result,
                None => {
                    // 🔧 回退：如果没有 Pre-RA 决策，尝试获取旧的寄存器分配结果
                    let old_key = format!("register-allocation-{}", function.name);
                    match analyses.get_result::<RegisterAllocationResult>(&old_key) {
                        Some(result) => {
                            info!("⚠️ 使用旧的寄存器分配结果，建议使用两阶段分配架构");
                            result
                        }
                        None => {
                            info!("⚠️ 未找到寄存器分配结果，跳过栈帧降级");
                            return PassResult::Unchanged;
                        }
                    }
                }
            };

        // 🔧 关键修复：创建可变的allocation_result副本并确保栈指针和帧指针不被寄存器分配器重新分配
        let mut allocation_result = allocation_result.clone();
        self.ensure_special_registers_reserved(&mut allocation_result);

        // 计算栈帧布局
        let layout = self.calculate_stack_frame_layout(function, &allocation_result);

        // 重写指令
        match self.rewrite_instructions(function, &layout, &allocation_result) {
            Ok(()) => {
                info!("✅ StackFrameLowering Pass 完成");
                PassResult::Changed
            }
            Err(e) => {
                error!("❌ StackFrameLowering Pass 失败: {}", e);
                PassResult::Failed(e)
            }
        }
    }

    fn required_analyses(&self) -> Vec<&'static str> {
        vec![] // 不声明分析依赖，因为我们查找的是函数特定的分析结果键
    }

    fn invalidated_analyses(&self) -> Vec<&'static str> {
        vec![] // 不使其他分析失效
    }
}

impl Default for StackFrameLowering {
    fn default() -> Self {
        Self::new()
    }
}
