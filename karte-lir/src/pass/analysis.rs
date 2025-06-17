use super::{AnalysisPass, AnalysisResult, AnalysisManager};
use crate::{LirFunction, Instruction, RegisterId, LabelId, Operand};
use std::collections::{HashMap, HashSet, VecDeque};
use std::any::Any;

/// 控制流图节点
#[derive(Debug, Clone)]
pub struct ControlFlowNode {
    /// 基本块ID
    pub block_id: usize,
    /// 指令范围 [start, end)
    pub instruction_range: (usize, usize),
    /// 前驱节点
    pub predecessors: Vec<usize>,
    /// 后继节点
    pub successors: Vec<usize>,
    /// 标签（如果有）
    pub label: Option<LabelId>,
}

/// 控制流图分析结果
#[derive(Debug)]
pub struct ControlFlowGraph {
    /// 所有基本块
    pub nodes: Vec<ControlFlowNode>,
    /// 入口块
    pub entry_block: usize,
    /// 出口块
    pub exit_blocks: Vec<usize>,
    /// 标签到块ID的映射
    pub label_to_block: HashMap<LabelId, usize>,
}

impl AnalysisResult for ControlFlowGraph {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// 定义-使用链分析结果
#[derive(Debug)]
pub struct DefUseChains {
    /// 每个寄存器的定义位置 (寄存器ID -> 指令位置列表)
    pub definitions: HashMap<RegisterId, Vec<usize>>,
    /// 每个寄存器的使用位置 (寄存器ID -> 指令位置列表)
    pub uses: HashMap<RegisterId, Vec<usize>>,
    /// 每个指令定义的寄存器
    pub instruction_defs: HashMap<usize, Vec<RegisterId>>,
    /// 每个指令使用的寄存器
    pub instruction_uses: HashMap<usize, Vec<RegisterId>>,
}

impl AnalysisResult for DefUseChains {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// 活跃变量分析结果
#[derive(Debug)]
pub struct LivenessAnalysis {
    /// 每个基本块入口处的活跃变量
    pub live_in: HashMap<usize, HashSet<RegisterId>>,
    /// 每个基本块出口处的活跃变量
    pub live_out: HashMap<usize, HashSet<RegisterId>>,
    /// 每个指令位置的活跃变量
    pub live_at_instruction: HashMap<usize, HashSet<RegisterId>>,
}

impl AnalysisResult for LivenessAnalysis {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// 控制流图分析 Pass
#[derive(Debug)]
pub struct ControlFlowAnalysis;

impl ControlFlowAnalysis {
    pub fn new() -> Self {
        Self
    }
    
    /// 构建控制流图
    fn build_cfg(&self, function: &LirFunction) -> Result<ControlFlowGraph, String> {
        let mut nodes = Vec::new();
        let mut label_to_block = HashMap::new();
        
        // 第一阶段：识别基本块边界
        let block_starts = self.find_basic_block_starts(function);
        
        // 第二阶段：创建基本块节点
        for (i, &start) in block_starts.iter().enumerate() {
            let end = if i + 1 < block_starts.len() {
                block_starts[i + 1]
            } else {
                function.instructions.len()
            };
            
            let mut node = ControlFlowNode {
                block_id: i,
                instruction_range: (start, end),
                predecessors: Vec::new(),
                successors: Vec::new(),
                label: None,
            };
            
            // 检查是否有标签
            if start < function.instructions.len() {
                if let Instruction::Label { id, .. } = &function.instructions[start] {
                    node.label = Some(*id);
                    label_to_block.insert(*id, i);
                }
            }
            
            nodes.push(node);
        }
        
        // 第三阶段：建立控制流边
        self.build_control_flow_edges(&mut nodes, function, &label_to_block)?;
        
        // 确定入口和出口块
        let entry_block = 0; // 第一个块是入口
        let exit_blocks = self.find_exit_blocks(&nodes, function);
        
        Ok(ControlFlowGraph {
            nodes,
            entry_block,
            exit_blocks,
            label_to_block,
        })
    }
    
    /// 找到基本块的起始位置
    fn find_basic_block_starts(&self, function: &LirFunction) -> Vec<usize> {
        let mut starts = vec![0]; // 函数开始总是一个基本块
        let mut is_leader = vec![false; function.instructions.len()];
        is_leader[0] = true;
        
        for (i, instruction) in function.instructions.iter().enumerate() {
            match instruction {
                // 标签是基本块的开始
                Instruction::Label { .. } => {
                    is_leader[i] = true;
                }
                // 跳转指令的目标是基本块的开始
                Instruction::Jump { target, .. } |
                Instruction::JumpEqual { target, .. } |
                Instruction::JumpNotEqual { target, .. } |
                Instruction::JumpLess { target, .. } |
                Instruction::JumpLessEqual { target, .. } |
                Instruction::JumpGreater { target, .. } |
                Instruction::JumpGreaterEqual { target, .. } => {
                    // 标记跳转目标为基本块开始（稍后处理）
                    // 跳转指令的下一条指令也是基本块开始
                    if i + 1 < function.instructions.len() {
                        is_leader[i + 1] = true;
                    }
                }
                // 返回指令的下一条指令是基本块开始
                Instruction::Return { .. } => {
                    if i + 1 < function.instructions.len() {
                        is_leader[i + 1] = true;
                    }
                }
                _ => {}
            }
        }
        
        // 收集所有基本块开始位置
        for (i, &is_start) in is_leader.iter().enumerate() {
            if is_start && i > 0 {
                starts.push(i);
            }
        }
        
        starts.sort();
        starts.dedup();
        starts
    }
    
    /// 建立控制流边
    fn build_control_flow_edges(
        &self,
        nodes: &mut [ControlFlowNode],
        function: &LirFunction,
        label_to_block: &HashMap<LabelId, usize>,
    ) -> Result<(), String> {
        let nodes_len = nodes.len();
        
        for (block_id, node) in nodes.iter_mut().enumerate() {
            let (start, end) = node.instruction_range;
            
            if start >= end {
                continue;
            }
            
            // 查看基本块的最后一条指令
            let last_instruction_idx = end - 1;
            if last_instruction_idx >= function.instructions.len() {
                continue;
            }
            
            let last_instruction = &function.instructions[last_instruction_idx];
            
            match last_instruction {
                // 无条件跳转
                Instruction::Jump { target, .. } => {
                    if let Some(&target_block) = label_to_block.get(target) {
                        node.successors.push(target_block);
                    }
                }
                // 条件跳转
                Instruction::JumpEqual { target, .. } |
                Instruction::JumpNotEqual { target, .. } |
                Instruction::JumpLess { target, .. } |
                Instruction::JumpLessEqual { target, .. } |
                Instruction::JumpGreater { target, .. } |
                Instruction::JumpGreaterEqual { target, .. } => {
                    // 跳转目标
                    if let Some(&target_block) = label_to_block.get(target) {
                        node.successors.push(target_block);
                    }
                    // 顺序执行到下一个基本块
                    if block_id + 1 < nodes_len {
                        node.successors.push(block_id + 1);
                    }
                }
                // 返回指令没有后继
                Instruction::Return { .. } => {
                    // 没有后继
                }
                // 其他指令：顺序执行到下一个基本块
                _ => {
                    if block_id + 1 < nodes_len {
                        node.successors.push(block_id + 1);
                    }
                }
            }
        }
        
        // 建立前驱关系
        for block_id in 0..nodes_len {
            let successors = nodes[block_id].successors.clone();
            for &successor in &successors {
                if successor < nodes_len {
                    nodes[successor].predecessors.push(block_id);
                }
            }
        }
        
        Ok(())
    }
    
    /// 找到出口块
    fn find_exit_blocks(&self, nodes: &[ControlFlowNode], function: &LirFunction) -> Vec<usize> {
        let mut exit_blocks = Vec::new();
        
        for (block_id, node) in nodes.iter().enumerate() {
            // 没有后继的块是出口块
            if node.successors.is_empty() {
                exit_blocks.push(block_id);
            } else {
                // 或者包含返回指令的块
                let (start, end) = node.instruction_range;
                for i in start..end {
                    if i < function.instructions.len() {
                        if matches!(function.instructions[i], Instruction::Return { .. }) {
                            exit_blocks.push(block_id);
                            break;
                        }
                    }
                }
            }
        }
        
        exit_blocks
    }
}

impl AnalysisPass for ControlFlowAnalysis {
    fn name(&self) -> &str {
        "cfg"
    }
    
    fn analyze_function(&mut self, function: &LirFunction, _analyses: &AnalysisManager) -> Result<Box<dyn AnalysisResult>, String> {
        let cfg = self.build_cfg(function)?;
        Ok(Box::new(cfg))
    }
}

/// 定义-使用链分析 Pass
#[derive(Debug)]
pub struct DefUseAnalysis;

impl DefUseAnalysis {
    pub fn new() -> Self {
        Self
    }
    
    /// 构建定义-使用链
    fn build_def_use_chains(&self, function: &LirFunction) -> Result<DefUseChains, String> {
        let mut definitions = HashMap::new();
        let mut uses = HashMap::new();
        let mut instruction_defs = HashMap::new();
        let mut instruction_uses = HashMap::new();
        
        for (instruction_index, instruction) in function.instructions.iter().enumerate() {
            let (defs, used) = self.analyze_instruction(instruction);
            
            // 记录指令的定义和使用
            instruction_defs.insert(instruction_index, defs.clone());
            instruction_uses.insert(instruction_index, used.clone());
            
            // 记录每个寄存器的定义位置
            for def_reg in defs {
                definitions.entry(def_reg).or_insert_with(Vec::new).push(instruction_index);
            }
            
            // 记录每个寄存器的使用位置
            for use_reg in used {
                uses.entry(use_reg).or_insert_with(Vec::new).push(instruction_index);
            }
        }
        
        Ok(DefUseChains {
            definitions,
            uses,
            instruction_defs,
            instruction_uses,
        })
    }
    
    /// 分析单条指令的定义和使用
    fn analyze_instruction(&self, instruction: &Instruction) -> (Vec<RegisterId>, Vec<RegisterId>) {
        let mut defs = Vec::new();
        let mut uses = Vec::new();
        
        match instruction {
            Instruction::Move { dst, src, .. } => {
                defs.push(*dst);
                self.analyze_operand_uses(src, &mut uses);
            }
            Instruction::Add { dst, src1, src2, .. } |
            Instruction::Sub { dst, src1, src2, .. } |
            Instruction::Mul { dst, src1, src2, .. } |
            Instruction::Div { dst, src1, src2, .. } => {
                defs.push(*dst);
                self.analyze_operand_uses(src1, &mut uses);
                self.analyze_operand_uses(src2, &mut uses);
            }
            Instruction::Compare { src1, src2, .. } => {
                self.analyze_operand_uses(src1, &mut uses);
                self.analyze_operand_uses(src2, &mut uses);
            }
            Instruction::Load64 { dst, addr, .. } => {
                defs.push(*dst);
                uses.push(*addr);
            }
            Instruction::Store64 { addr, src, .. } => {
                uses.push(*addr);
                self.analyze_operand_uses(src, &mut uses);
            }
            Instruction::Call { args, result, .. } => {
                // 参数寄存器被使用
                for arg in args {
                    uses.push(*arg);
                }
                // 结果寄存器被定义
                if let Some(result_reg) = result {
                    defs.push(*result_reg);
                }
            }
            Instruction::Return { value, .. } => {
                if let Some(ret_reg) = value {
                    uses.push(*ret_reg);
                }
            }
            Instruction::Alloc { dst, .. } => {
                defs.push(*dst);
            }
            // 其他指令类型...
            _ => {}
        }
        
        (defs, uses)
    }
    
    /// 分析操作数的使用
    fn analyze_operand_uses(&self, operand: &Operand, uses: &mut Vec<RegisterId>) {
        match operand {
            Operand::Register { id } => {
                uses.push(*id);
            }
            Operand::Memory { base, .. } => {
                uses.push(*base);
            }
            Operand::Immediate { .. } => {
                // 立即数不使用寄存器
            }
            _ => {}
        }
    }
}

impl AnalysisPass for DefUseAnalysis {
    fn name(&self) -> &str {
        "def-use"
    }
    
    fn analyze_function(&mut self, function: &LirFunction, _analyses: &AnalysisManager) -> Result<Box<dyn AnalysisResult>, String> {
        let def_use = self.build_def_use_chains(function)?;
        Ok(Box::new(def_use))
    }
} 