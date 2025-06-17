use super::{FunctionPass, AnalysisManager, PassResult};
use crate::{LirFunction, Instruction, RegisterId, Operand};
use std::collections::HashMap;

/// 死代码消除 Pass
/// 
/// 移除未使用的指令和寄存器定义
#[derive(Debug)]
pub struct DeadCodeElimination;

impl DeadCodeElimination {
    pub fn new() -> Self {
        Self
    }
    
    /// 运行死代码消除
    fn eliminate_dead_code(&self, function: &mut LirFunction) -> bool {
        let mut changed = false;
        let mut worklist = Vec::new();
        let mut live_instructions = std::collections::HashSet::new();
        
        // 标记所有有副作用的指令为活跃
        for (i, instruction) in function.instructions.iter().enumerate() {
            if self.has_side_effects(instruction) {
                live_instructions.insert(i);
                worklist.push(i);
            }
        }
        
        // 从活跃指令开始，标记所有依赖的指令
        while let Some(instruction_index) = worklist.pop() {
            // 获取这条指令使用的寄存器
            let used_registers = self.get_used_registers(&function.instructions[instruction_index]);
            
            // 找到定义这些寄存器的指令
            for used_reg in used_registers {
                for (i, instr) in function.instructions.iter().enumerate() {
                    if i < instruction_index && self.defines_register(instr, &used_reg) {
                        if live_instructions.insert(i) {
                            worklist.push(i);
                        }
                        break; // 找到最近的定义即可
                    }
                }
            }
        }
        
        // 移除未标记为活跃的指令
        let mut new_instructions = Vec::new();
        for (i, instruction) in function.instructions.iter().enumerate() {
            if live_instructions.contains(&i) {
                new_instructions.push(instruction.clone());
            } else {
                changed = true;
            }
        }
        
        function.instructions = new_instructions;
        changed
    }
    
    /// 检查指令是否有副作用
    fn has_side_effects(&self, instruction: &Instruction) -> bool {
        match instruction {
            Instruction::Store64 { .. } |
            Instruction::Call { .. } |
            Instruction::Return { .. } |
            Instruction::Jump { .. } |
            Instruction::JumpEqual { .. } |
            Instruction::JumpNotEqual { .. } |
            Instruction::JumpLess { .. } |
            Instruction::JumpLessEqual { .. } |
            Instruction::JumpGreater { .. } |
            Instruction::JumpGreaterEqual { .. } |
            Instruction::Compare { .. } |
            Instruction::Label { .. } => true, // 标签指令不能被移除，因为它们是跳转目标
            _ => false,
        }
    }
    
    /// 获取指令使用的寄存器
    fn get_used_registers(&self, instruction: &Instruction) -> Vec<RegisterId> {
        let mut used = Vec::new();
        
        match instruction {
            Instruction::Move { src, .. } => {
                self.add_operand_registers(src, &mut used);
            }
            Instruction::Add { src1, src2, .. } |
            Instruction::Sub { src1, src2, .. } |
            Instruction::Mul { src1, src2, .. } |
            Instruction::Div { src1, src2, .. } => {
                self.add_operand_registers(src1, &mut used);
                self.add_operand_registers(src2, &mut used);
            }
            Instruction::Compare { src1, src2, .. } => {
                self.add_operand_registers(src1, &mut used);
                self.add_operand_registers(src2, &mut used);
            }
            Instruction::Load64 { addr, .. } => {
                used.push(*addr);
            }
            Instruction::Store64 { addr, src, .. } => {
                used.push(*addr);
                self.add_operand_registers(src, &mut used);
            }
            Instruction::Call { args, .. } => {
                used.extend_from_slice(args);
            }
            Instruction::Return { value, .. } => {
                if let Some(reg) = value {
                    used.push(*reg);
                }
            }
            _ => {}
        }
        
        used
    }
    
    /// 添加操作数中的寄存器
    fn add_operand_registers(&self, operand: &Operand, registers: &mut Vec<RegisterId>) {
        match operand {
            Operand::Register { id } => registers.push(*id),
            Operand::Memory { base, .. } => registers.push(*base),
            _ => {}
        }
    }
    
    /// 检查指令是否定义了指定寄存器
    fn defines_register(&self, instruction: &Instruction, register: &RegisterId) -> bool {
        match instruction {
            Instruction::Move { dst, .. } |
            Instruction::Add { dst, .. } |
            Instruction::Sub { dst, .. } |
            Instruction::Mul { dst, .. } |
            Instruction::Div { dst, .. } |
            Instruction::Load64 { dst, .. } |
            Instruction::Alloc { dst, .. } => dst == register,
            Instruction::Call { result: Some(dst), .. } => dst == register,
            _ => false,
        }
    }
}

impl FunctionPass for DeadCodeElimination {
    fn name(&self) -> &str {
        "dce"
    }
    
    fn run_on_function(&mut self, function: &mut LirFunction, _analyses: &mut AnalysisManager) -> PassResult {
        if self.eliminate_dead_code(function) {
            PassResult::Changed
        } else {
            PassResult::Unchanged
        }
    }
}

/// 常量折叠 Pass
/// 
/// 计算编译时可确定的常量表达式
#[derive(Debug)]
pub struct ConstantFolding;

impl ConstantFolding {
    pub fn new() -> Self {
        Self
    }
    
    /// 运行常量折叠
    fn fold_constants(&self, function: &mut LirFunction) -> bool {
        let mut changed = false;
        let mut constant_values = HashMap::new();
        
        for instruction in function.instructions.iter_mut() {
            match instruction {
                Instruction::Move { dst, src: Operand::Immediate { value }, .. } => {
                    // 记录常量定义
                    constant_values.insert(*dst, *value);
                }
                Instruction::Add { dst, src1, src2, span } => {
                    let dst_reg = *dst;
                    let span_copy = *span;
                    if let (Some(&val1), Some(&val2)) = (
                        self.get_constant_value(src1, &constant_values),
                        self.get_constant_value(src2, &constant_values)
                    ) {
                        let result = val1 + val2;
                        // 折叠加法
                        *instruction = Instruction::Move {
                            dst: dst_reg,
                            src: Operand::Immediate { value: result },
                            span: span_copy,
                        };
                        constant_values.insert(dst_reg, result);
                        changed = true;
                    }
                }
                Instruction::Sub { dst, src1, src2, span } => {
                    let dst_reg = *dst;
                    let span_copy = *span;
                    if let (Some(&val1), Some(&val2)) = (
                        self.get_constant_value(src1, &constant_values),
                        self.get_constant_value(src2, &constant_values)
                    ) {
                        let result = val1 - val2;
                        // 折叠减法
                        *instruction = Instruction::Move {
                            dst: dst_reg,
                            src: Operand::Immediate { value: result },
                            span: span_copy,
                        };
                        constant_values.insert(dst_reg, result);
                        changed = true;
                    }
                }
                Instruction::Mul { dst, src1, src2, span } => {
                    let dst_reg = *dst;
                    let span_copy = *span;
                    if let (Some(&val1), Some(&val2)) = (
                        self.get_constant_value(src1, &constant_values),
                        self.get_constant_value(src2, &constant_values)
                    ) {
                        let result = val1 * val2;
                        // 折叠乘法
                        *instruction = Instruction::Move {
                            dst: dst_reg,
                            src: Operand::Immediate { value: result },
                            span: span_copy,
                        };
                        constant_values.insert(dst_reg, result);
                        changed = true;
                    }
                }
                _ => {
                    // 其他指令可能使常量值失效
                    if let Some(defined_reg) = self.get_defined_register(instruction) {
                        constant_values.remove(&defined_reg);
                    }
                }
            }
        }
        
        changed
    }
    
    /// 获取操作数的常量值
    fn get_constant_value<'a>(&self, operand: &'a Operand, constants: &'a HashMap<RegisterId, i64>) -> Option<&'a i64> {
        match operand {
            Operand::Immediate { value } => Some(value),
            Operand::Register { id } => constants.get(id),
            _ => None,
        }
    }
    
    /// 获取指令定义的寄存器
    fn get_defined_register(&self, instruction: &Instruction) -> Option<RegisterId> {
        match instruction {
            Instruction::Move { dst, .. } |
            Instruction::Add { dst, .. } |
            Instruction::Sub { dst, .. } |
            Instruction::Mul { dst, .. } |
            Instruction::Div { dst, .. } |
            Instruction::Load64 { dst, .. } |
            Instruction::Alloc { dst, .. } => Some(*dst),
            Instruction::Call { result: Some(dst), .. } => Some(*dst),
            _ => None,
        }
    }
}

impl FunctionPass for ConstantFolding {
    fn name(&self) -> &str {
        "const-fold"
    }
    
    fn run_on_function(&mut self, function: &mut LirFunction, _analyses: &mut AnalysisManager) -> PassResult {
        if self.fold_constants(function) {
            PassResult::Changed
        } else {
            PassResult::Unchanged
        }
    }
} 