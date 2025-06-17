//! 寄存器溢出管理
//! 
//! 负责处理寄存器溢出到内存的操作，包括：
//! - 溢出代码生成
//! - 栈槽分配
//! - 加载/存储指令插入

use super::super::calling_convention::{CallingConvention, PhysicalRegister};
use super::super::stack_manager::{StackManager, StackOperation};
use karte_lir::{Instruction, LirFunction, RegisterId, Operand};
use karte_diagnostics::Span;
use std::collections::HashMap;

/// 溢出槽信息
#[derive(Debug, Clone)]
pub struct SpillSlot {
    /// 槽ID
    pub slot_id: usize,
    /// 栈偏移量
    pub stack_offset: i64,
    /// 被溢出的虚拟寄存器
    pub virtual_register: RegisterId,
    /// 大小（字节）
    pub size: usize,
}

/// 溢出管理器
#[derive(Debug)]
pub struct SpillManager {
    /// 调用约定
    calling_convention: CallingConvention,
    /// 溢出槽映射
    spill_slots: HashMap<RegisterId, SpillSlot>,
    /// 下一个可用的槽ID
    next_slot_id: usize,
    /// 溢出计数器
    spill_count: usize,
}

impl SpillManager {
    /// 创建新的溢出管理器
    pub fn new(calling_convention: CallingConvention) -> Self {
        Self {
            calling_convention,
            spill_slots: HashMap::new(),
            next_slot_id: 0,
            spill_count: 0,
        }
    }

    /// 处理寄存器溢出
    pub fn handle_spills(&mut self, function: &mut LirFunction, spilled_registers: &[RegisterId]) -> Result<(), String> {
        if spilled_registers.is_empty() {
            return Ok(());
        }

        // 1. 为溢出的寄存器分配栈槽
        self.allocate_spill_slots(function, spilled_registers)?;

        // 2. 在适当的位置插入溢出和重载指令
        self.insert_spill_code(function)?;

        // 3. 更新函数的栈帧大小
        self.update_stack_frame_size(function);

        Ok(())
    }

    /// 为溢出的寄存器分配栈槽
    fn allocate_spill_slots(&mut self, function: &mut LirFunction, spilled_registers: &[RegisterId]) -> Result<(), String> {
        for &reg in spilled_registers {
            if !self.spill_slots.contains_key(&reg) {
                let slot_id = self.next_slot_id;
                self.next_slot_id += 1;

                // 分配栈槽空间（8字节对齐）
                let slot_size = 8;
                let stack_offset = function.reserve_stack_space(slot_size, 8);

                let spill_slot = SpillSlot {
                    slot_id,
                    stack_offset: stack_offset as i64,
                    virtual_register: reg,
                    size: slot_size,
                };

                self.spill_slots.insert(reg, spill_slot);
                self.spill_count += 1;
            }
        }

        Ok(())
    }

    /// 插入溢出和重载代码
    fn insert_spill_code(&self, function: &mut LirFunction) -> Result<(), String> {
        let mut new_instructions = Vec::new();
        let sp_reg = self.calling_convention.stack_pointer;

        for (i, instruction) in function.instructions.iter().enumerate() {
            // 处理指令中使用的溢出寄存器
            let (modified_instruction, pre_loads, post_stores) = self.process_instruction(instruction, sp_reg)?;

            // 添加预加载指令
            new_instructions.extend(pre_loads);

            // 添加修改后的指令
            new_instructions.push(modified_instruction);

            // 添加后存储指令
            new_instructions.extend(post_stores);
        }

        function.instructions = new_instructions;
        Ok(())
    }

    /// 处理单条指令的溢出
    fn process_instruction(&self, instruction: &Instruction, sp_reg: PhysicalRegister) -> Result<(Instruction, Vec<Instruction>, Vec<Instruction>), String> {
        let mut pre_loads = Vec::new();
        let mut post_stores = Vec::new();
        let mut temp_reg_counter = 0;

        // 分析指令中使用的寄存器
        let (def_regs, use_regs) = self.analyze_register_usage(instruction);

        // 为使用的溢出寄存器生成加载指令
        let mut reg_mapping = HashMap::new();
        for &reg in &use_regs {
            if let Some(spill_slot) = self.spill_slots.get(&reg) {
                // 使用临时寄存器
                let temp_reg = self.get_temp_register(temp_reg_counter);
                temp_reg_counter += 1;

                // 生成加载指令：load temp_reg, sp + offset
                let load_instruction = Instruction::Load64 {
                    dst: RegisterId(temp_reg as usize),
                    addr: RegisterId(sp_reg as usize),
                    offset: spill_slot.stack_offset,
                    span: instruction.get_span(),
                };
                pre_loads.push(load_instruction);

                reg_mapping.insert(reg, RegisterId(temp_reg as usize));
            }
        }

        // 为定义的溢出寄存器生成存储指令
        for &reg in &def_regs {
            if let Some(spill_slot) = self.spill_slots.get(&reg) {
                let temp_reg = if let Some(&mapped_reg) = reg_mapping.get(&reg) {
                    mapped_reg
                } else {
                    let temp_reg = self.get_temp_register(temp_reg_counter);
                    temp_reg_counter += 1;
                    let temp_reg_id = RegisterId(temp_reg as usize);
                    reg_mapping.insert(reg, temp_reg_id);
                    temp_reg_id
                };

                // 生成存储指令：store sp + offset, temp_reg
                let store_instruction = Instruction::Store64 {
                    addr: RegisterId(sp_reg as usize),
                    offset: spill_slot.stack_offset,
                    src: Operand::Register { id: temp_reg },
                    span: instruction.get_span(),
                };
                post_stores.push(store_instruction);
            }
        }

        // 重写指令，将溢出寄存器替换为临时寄存器
        let modified_instruction = self.rewrite_instruction(instruction, &reg_mapping)?;

        Ok((modified_instruction, pre_loads, post_stores))
    }

    /// 分析指令的寄存器使用情况
    fn analyze_register_usage(&self, instruction: &Instruction) -> (Vec<RegisterId>, Vec<RegisterId>) {
        let mut def_regs = Vec::new();
        let mut use_regs = Vec::new();

        match instruction {
            Instruction::Move { dst, src, .. } => {
                def_regs.push(*dst);
                if let Operand::Register { id } = src {
                    use_regs.push(*id);
                }
            }

            Instruction::Add { dst, src1, src2, .. } |
            Instruction::Sub { dst, src1, src2, .. } |
            Instruction::Mul { dst, src1, src2, .. } |
            Instruction::Div { dst, src1, src2, .. } => {
                def_regs.push(*dst);
                if let Operand::Register { id } = src1 {
                    use_regs.push(*id);
                }
                if let Operand::Register { id } = src2 {
                    use_regs.push(*id);
                }
            }

            Instruction::Compare { src1, src2, .. } => {
                if let Operand::Register { id } = src1 {
                    use_regs.push(*id);
                }
                if let Operand::Register { id } = src2 {
                    use_regs.push(*id);
                }
            }

            Instruction::Call { args, result, .. } => {
                use_regs.extend_from_slice(args);
                if let Some(reg) = result {
                    def_regs.push(*reg);
                }
            }

            Instruction::Return { value, .. } => {
                if let Some(reg) = value {
                    use_regs.push(*reg);
                }
            }

            _ => {} // 其他指令暂时不处理
        }

        (def_regs, use_regs)
    }

    /// 重写指令，替换溢出寄存器
    fn rewrite_instruction(&self, instruction: &Instruction, reg_mapping: &HashMap<RegisterId, RegisterId>) -> Result<Instruction, String> {
        let rewrite_reg = |reg: RegisterId| -> RegisterId {
            reg_mapping.get(&reg).copied().unwrap_or(reg)
        };

        let rewrite_operand = |operand: &Operand| -> Operand {
            match operand {
                Operand::Register { id } => Operand::Register { id: rewrite_reg(*id) },
                _ => operand.clone(),
            }
        };

        let rewritten = match instruction {
            Instruction::Move { dst, src, span } => {
                Instruction::Move {
                    dst: rewrite_reg(*dst),
                    src: rewrite_operand(src),
                    span: *span,
                }
            }

            Instruction::Add { dst, src1, src2, span } => {
                Instruction::Add {
                    dst: rewrite_reg(*dst),
                    src1: rewrite_operand(src1),
                    src2: rewrite_operand(src2),
                    span: *span,
                }
            }

            Instruction::Sub { dst, src1, src2, span } => {
                Instruction::Sub {
                    dst: rewrite_reg(*dst),
                    src1: rewrite_operand(src1),
                    src2: rewrite_operand(src2),
                    span: *span,
                }
            }

            Instruction::Mul { dst, src1, src2, span } => {
                Instruction::Mul {
                    dst: rewrite_reg(*dst),
                    src1: rewrite_operand(src1),
                    src2: rewrite_operand(src2),
                    span: *span,
                }
            }

            Instruction::Div { dst, src1, src2, span } => {
                Instruction::Div {
                    dst: rewrite_reg(*dst),
                    src1: rewrite_operand(src1),
                    src2: rewrite_operand(src2),
                    span: *span,
                }
            }

            Instruction::Compare { src1, src2, span } => {
                Instruction::Compare {
                    src1: rewrite_operand(src1),
                    src2: rewrite_operand(src2),
                    span: *span,
                }
            }

            Instruction::Call { target, args, result, span } => {
                let rewritten_args = args.iter().map(|&reg| rewrite_reg(reg)).collect();
                let rewritten_result = result.map(|reg| rewrite_reg(reg));
                
                Instruction::Call {
                    target: *target,
                    args: rewritten_args,
                    result: rewritten_result,
                    span: *span,
                }
            }

            Instruction::Return { value, span } => {
                Instruction::Return {
                    value: value.map(|reg| rewrite_reg(reg)),
                    span: *span,
                }
            }

            // 其他指令直接克隆
            _ => instruction.clone(),
        };

        Ok(rewritten)
    }

    /// 获取临时寄存器
    fn get_temp_register(&self, index: usize) -> PhysicalRegister {
        // 使用调用约定中的临时寄存器
        let temp_registers = &self.calling_convention.temp_registers;
        if index < temp_registers.len() {
            temp_registers[index]
        } else {
            // 如果临时寄存器不够，使用可分配寄存器
            let allocatable = self.calling_convention.get_allocatable_registers();
            allocatable[index % allocatable.len()]
        }
    }

    /// 更新函数的栈帧大小
    fn update_stack_frame_size(&self, function: &mut LirFunction) {
        let total_spill_size: usize = self.spill_slots.values()
            .map(|slot| slot.size)
            .sum();
        
        function.stack_frame_size += total_spill_size;
    }

    /// 获取溢出计数
    pub fn get_spill_count(&self) -> usize {
        self.spill_count
    }

    /// 获取指定寄存器的溢出槽
    pub fn get_spill_slot(&self, register: &RegisterId) -> Option<&SpillSlot> {
        self.spill_slots.get(register)
    }

    /// 清除溢出信息
    pub fn clear(&mut self) {
        self.spill_slots.clear();
        self.next_slot_id = 0;
        self.spill_count = 0;
    }
}

/// 为Instruction添加get_span方法的扩展trait
trait InstructionSpanExtractor {
    fn get_span(&self) -> Span;
}

impl InstructionSpanExtractor for Instruction {
    fn get_span(&self) -> Span {
        match self {
            Instruction::Move { span, .. } => *span,
            Instruction::Add { span, .. } => *span,
            Instruction::Sub { span, .. } => *span,
            Instruction::Mul { span, .. } => *span,
            Instruction::Div { span, .. } => *span,
            Instruction::Compare { span, .. } => *span,
            Instruction::Jump { span, .. } => *span,
            Instruction::JumpEqual { span, .. } => *span,
            Instruction::JumpNotEqual { span, .. } => *span,
            Instruction::JumpGreater { span, .. } => *span,
            Instruction::JumpGreaterEqual { span, .. } => *span,
            Instruction::JumpLess { span, .. } => *span,
            Instruction::JumpLessEqual { span, .. } => *span,
            Instruction::Call { span, .. } => *span,
            Instruction::CallIndirect { span, .. } => *span,
            Instruction::Return { span, .. } => *span,
            Instruction::Label { span, .. } => *span,
            Instruction::Nop { span, .. } => *span,
            _ => Span::dummy(), // 其他指令使用dummy span
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use karte_lir::LirFunction;
    use karte_diagnostics::Span;

    #[test]
    fn test_spill_slot_allocation() {
        let cc = CallingConvention::standard();
        let mut spill_manager = SpillManager::new(cc);
        let mut function = LirFunction::new("test".to_string());

        let spilled_regs = vec![RegisterId(1), RegisterId(2)];
        spill_manager.allocate_spill_slots(&mut function, &spilled_regs).unwrap();

        assert_eq!(spill_manager.spill_slots.len(), 2);
        assert!(spill_manager.spill_slots.contains_key(&RegisterId(1)));
        assert!(spill_manager.spill_slots.contains_key(&RegisterId(2)));
    }

    #[test]
    fn test_register_usage_analysis() {
        let cc = CallingConvention::standard();
        let spill_manager = SpillManager::new(cc);

        let instruction = Instruction::Add {
            dst: RegisterId(1),
            src1: Operand::Register { id: RegisterId(2) },
            src2: Operand::Register { id: RegisterId(3) },
            span: Span::dummy(),
        };

        let (def_regs, use_regs) = spill_manager.analyze_register_usage(&instruction);

        assert_eq!(def_regs, vec![RegisterId(1)]);
        assert_eq!(use_regs, vec![RegisterId(2), RegisterId(3)]);
    }
} 