use super::{FunctionPass, AnalysisManager, PassResult, AnalysisResult};
use super::analysis::{ControlFlowGraph, DefUseChains};
use crate::{LirFunction, Instruction, RegisterId, Operand, AllocationType};
use karte_diagnostics::Span;
use std::collections::{HashMap, HashSet, VecDeque};
use std::any::Any;

/// 栈槽信息
#[derive(Debug, Clone)]
pub struct StackSlot {
    /// 分配指令的位置
    pub alloc_instruction: usize,
    /// 分配的寄存器（保存栈地址）
    pub address_register: RegisterId,
    /// 栈槽大小
    pub size: usize,
    /// 是否可以提升为寄存器
    pub promotable: bool,
    /// 加载指令位置列表
    pub loads: Vec<usize>,
    /// 存储指令位置列表  
    pub stores: Vec<usize>,
}

/// Memory2Reg 分析结果
#[derive(Debug, Clone)]
pub struct Memory2RegAnalysis {
    /// 栈槽信息
    pub stack_slots: HashMap<RegisterId, StackSlot>,
    /// 可提升的栈槽
    pub promotable_slots: Vec<RegisterId>,
}

impl AnalysisResult for Memory2RegAnalysis {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Memory2Reg Pass
/// 
/// 将合适的栈分配提升为寄存器分配，减少内存访问
#[derive(Debug)]
pub struct Memory2RegPass;

impl Memory2RegPass {
    pub fn new() -> Self {
        Self
    }
    
    /// 分析栈槽使用模式
    fn analyze_stack_slots(&self, function: &LirFunction) -> Memory2RegAnalysis {
        let mut stack_slots = HashMap::new();
        let mut promotable_slots = Vec::new();
        
        // 第一阶段：识别栈分配
        for (i, instruction) in function.instructions.iter().enumerate() {
            if let Instruction::Alloc { dst, size, allocation_type: AllocationType::Stack, .. } = instruction {
                let slot = StackSlot {
                    alloc_instruction: i,
                    address_register: *dst,
                    size: *size,
                    promotable: true, // 初始假设可提升
                    loads: Vec::new(),
                    stores: Vec::new(),
                };
                stack_slots.insert(*dst, slot);
            }
        }
        
        // 第二阶段：分析每个栈槽的使用
        for (i, instruction) in function.instructions.iter().enumerate() {
            match instruction {
                Instruction::Load64 { dst: _, addr, offset, .. } => {
                    if *offset == 0 {
                        if let Some(slot) = stack_slots.get_mut(addr) {
                            slot.loads.push(i);
                        }
                    } else {
                        // 有偏移的访问，标记为不可提升
                        if let Some(slot) = stack_slots.get_mut(addr) {
                            slot.promotable = false;
                        }
                    }
                }
                Instruction::Store64 { addr, offset, src: _, .. } => {
                    if *offset == 0 {
                        if let Some(slot) = stack_slots.get_mut(addr) {
                            slot.stores.push(i);
                        }
                    } else {
                        // 有偏移的访问，标记为不可提升
                        if let Some(slot) = stack_slots.get_mut(addr) {
                            slot.promotable = false;
                        }
                    }
                }
                Instruction::Move { dst: _, src, .. } => {
                    // 检查是否将栈地址传递给其他地方
                    if let Operand::Register { id } = src {
                        if stack_slots.contains_key(id) {
                            if let Some(slot) = stack_slots.get_mut(id) {
                                slot.promotable = false; // 地址被传递，不安全提升
                            }
                        }
                    }
                }
                Instruction::Call { args, .. } => {
                    // 检查栈地址是否作为参数传递
                    for arg in args {
                        if let Some(slot) = stack_slots.get_mut(arg) {
                            slot.promotable = false; // 可能被函数修改
                        }
                    }
                }
                _ => {}
            }
        }
        
        // 收集可提升的栈槽
        for (addr_reg, slot) in &stack_slots {
            if slot.promotable {
                // 只要栈槽是可提升的就考虑优化
                // 即使只有store没有load，也可能是常量定义，应该被优化
                if !slot.stores.is_empty() {
                    promotable_slots.push(*addr_reg);
                }
            }
        }
        
        Memory2RegAnalysis {
            stack_slots,
            promotable_slots,
        }
    }
    
    /// 执行 Memory2Reg 变换
    fn transform_function(&self, function: &mut LirFunction, analysis: &Memory2RegAnalysis) -> bool {
        // 收集所有栈槽的变换操作
        let mut all_instructions_to_remove = Vec::new();
        let mut all_instructions_to_modify = Vec::new();
        
        for &addr_reg in &analysis.promotable_slots {
            if let Some(slot) = analysis.stack_slots.get(&addr_reg) {
                let (mut remove_ops, mut modify_ops) = self.collect_transform_operations(function, slot);
                all_instructions_to_remove.append(&mut remove_ops);
                all_instructions_to_modify.append(&mut modify_ops);
            }
        }
        
        // 一次性应用所有变换，避免索引冲突
        self.apply_all_transforms(function, all_instructions_to_remove, all_instructions_to_modify)
    }
    
    /// 收集单个栈槽的变换操作（不实际修改）
    fn collect_transform_operations(&self, function: &LirFunction, slot: &StackSlot) -> (Vec<usize>, Vec<(usize, Instruction)>) {
        let mut instructions_to_remove = Vec::new();
        let mut instructions_to_modify = Vec::new();
        
        // 只处理简单的情况：单一存储，多次加载
        if slot.stores.len() != 1 {
            // 如果有多个store或者没有store，不优化
            return (instructions_to_remove, instructions_to_modify);
        }
        
        // 检查唯一的store操作
        let store_pos = slot.stores[0];
        if store_pos >= function.instructions.len() {
            return (instructions_to_remove, instructions_to_modify);
        }
        
        if let Instruction::Store64 { src, .. } = &function.instructions[store_pos] {
            // 只处理立即数存储，不处理寄存器存储（可能是计算结果）
            match src {
                Operand::Immediate { .. } => {
                    // 安全：这是一个常量存储，可以优化
                    
                    // 将所有load指令替换为直接使用立即数
                    for &load_pos in &slot.loads {
                        if load_pos < function.instructions.len() {
                            if let Instruction::Load64 { dst, .. } = &function.instructions[load_pos] {
                                // 替换load为mov
                                instructions_to_modify.push((load_pos, Instruction::Move {
                                    dst: *dst,
                                    src: src.clone(),
                                    span: function.instructions[load_pos].get_span(),
                                }));
                            }
                        }
                    }
                    
                    // 移除store指令和alloc指令
                    instructions_to_remove.push(store_pos);
                    instructions_to_remove.push(slot.alloc_instruction);
                }
                Operand::Register { .. } => {
                    // 不安全：这可能是计算结果，不优化
                    // 保持原有的栈操作
                }
                _ => {
                    // 其他情况也不优化
                }
            }
        }
        
        (instructions_to_remove, instructions_to_modify)
    }
    
    /// 一次性应用所有变换操作
    fn apply_all_transforms(&self, function: &mut LirFunction, 
                          instructions_to_remove: Vec<usize>, 
                          instructions_to_modify: Vec<(usize, Instruction)>) -> bool {
        if instructions_to_remove.is_empty() && instructions_to_modify.is_empty() {
            return false;
        }
        
        // 去重并排序
        let mut unique_removes: Vec<usize> = instructions_to_remove.into_iter().collect();
        unique_removes.sort();
        unique_removes.dedup();
        
        let mut unique_modifies = instructions_to_modify;
        unique_modifies.sort_by(|a, b| a.0.cmp(&b.0));
        unique_modifies.dedup_by(|a, b| a.0 == b.0);
        
        // 先处理修改操作（索引不变）
        for (pos, new_instruction) in unique_modifies.iter().rev() {
            if *pos < function.instructions.len() {
                function.instructions[*pos] = new_instruction.clone();
            }
        }
        
        // 然后处理移除操作（从后往前，避免索引变化）
        for &pos in unique_removes.iter().rev() {
            if pos < function.instructions.len() {
                function.instructions.remove(pos);
            }
        }
        
        true
    }
}

impl FunctionPass for Memory2RegPass {
    fn name(&self) -> &str {
        "mem2reg"
    }
    
    fn run_on_function(&mut self, function: &mut LirFunction, analyses: &mut AnalysisManager) -> PassResult {
        // 运行分析
        let analysis = self.analyze_stack_slots(function);
        
        if analysis.promotable_slots.is_empty() {
            return PassResult::Unchanged;
        }
        
        // 存储分析结果
        analyses.store_result(self.name().to_string(), Box::new(analysis.clone()));
        
        // 执行变换
        if self.transform_function(function, &analysis) {
            PassResult::Changed
        } else {
            PassResult::Unchanged
        }
    }
    
    fn invalidated_analyses(&self) -> Vec<&'static str> {
        vec!["def-use", "cfg"] // Memory2Reg 会改变控制流和定义使用关系
    }
}

// 为指令添加获取span的辅助方法
trait InstructionExt {
    fn get_span(&self) -> Span;
}

impl InstructionExt for Instruction {
    fn get_span(&self) -> Span {
        match self {
            Instruction::Move { span, .. } |
            Instruction::Add { span, .. } |
            Instruction::Sub { span, .. } |
            Instruction::Mul { span, .. } |
            Instruction::Div { span, .. } |
            Instruction::Compare { span, .. } |
            Instruction::Jump { span, .. } |
            Instruction::JumpEqual { span, .. } |
            Instruction::JumpNotEqual { span, .. } |
            Instruction::JumpLess { span, .. } |
            Instruction::JumpLessEqual { span, .. } |
            Instruction::JumpGreater { span, .. } |
            Instruction::JumpGreaterEqual { span, .. } |
            Instruction::Call { span, .. } |
            Instruction::Return { span, .. } |
            Instruction::Label { span, .. } |
            Instruction::Nop { span, .. } |
            Instruction::Load64 { span, .. } |
            Instruction::Store64 { span, .. } |
            Instruction::Alloc { span, .. } => *span,
            _ => Span::dummy(),
        }
    }
} 