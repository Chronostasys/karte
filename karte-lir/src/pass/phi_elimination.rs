//! φ指令消除Pass
//! 
//! 实现专业的φ指令降级，将SSA形式的φ指令转换为普通的mov指令。
//! 这个pass应该在Memory2Reg之后、指令降级之前运行。

use super::{FunctionPass, AnalysisManager, PassResult};
use crate::{LirFunction, Instruction, RegisterId, Operand, LabelId};
use karte_diagnostics::Span;
use std::collections::{HashMap, HashSet, VecDeque};

/// φ指令消除Pass
#[derive(Debug)]
pub struct PhiEliminationPass;

impl PhiEliminationPass {
    pub fn new() -> Self {
        Self
    }
    
    /// 构建控制流图信息
    fn build_cfg_info(&self, function: &LirFunction) -> ControlFlowInfo {
        let mut cfg = ControlFlowInfo::new();
        let mut current_block_start = 0;
        let mut current_block_id = 0;
        
        // 扫描指令，识别基本块
        for (i, instruction) in function.instructions.iter().enumerate() {
            match instruction {
                Instruction::Label { id, .. } => {
                    // 结束前一个基本块
                    if i > current_block_start {
                        cfg.add_block(current_block_id, current_block_start, i);
                        current_block_id += 1;
                    }
                    
                    // 开始新的基本块
                    cfg.label_to_block.insert(*id, current_block_id);
                    current_block_start = i;
                }
                Instruction::Jump { .. } | 
                Instruction::JumpEqual { .. } | 
                Instruction::JumpNotEqual { .. } |
                Instruction::JumpLess { .. } |
                Instruction::JumpLessEqual { .. } |
                Instruction::JumpGreater { .. } |
                Instruction::JumpGreaterEqual { .. } |
                Instruction::Return { .. } => {
                    // 跳转指令结束当前基本块
                    cfg.add_block(current_block_id, current_block_start, i + 1);
                    current_block_id += 1;
                    current_block_start = i + 1;
                }
                _ => {}
            }
        }
        
        // 处理最后一个基本块
        if current_block_start < function.instructions.len() {
            cfg.add_block(current_block_id, current_block_start, function.instructions.len());
        }
        
        // 构建前驱后继关系
        self.build_predecessors_successors(&mut cfg, function);
        
        cfg
    }
    
    /// 构建前驱后继关系
    fn build_predecessors_successors(&self, cfg: &mut ControlFlowInfo, function: &LirFunction) {
        let block_ids: Vec<usize> = cfg.blocks.keys().cloned().collect();
        
        for block_id in block_ids {
            let (start, end) = {
                let block = &cfg.blocks[&block_id];
                (block.start, block.end)
            };
            
            if start >= end || end == 0 {
                continue;
            }
            
            // 查看基本块的最后一条指令
            let last_instruction = &function.instructions[end - 1];
            
            match last_instruction {
                Instruction::Jump { target, .. } => {
                    // 无条件跳转
                    if let Some(&target_block) = cfg.label_to_block.get(target) {
                        cfg.add_edge(block_id, target_block);
                    }
                }
                Instruction::JumpEqual { target, .. } |
                Instruction::JumpNotEqual { target, .. } |
                Instruction::JumpLess { target, .. } |
                Instruction::JumpLessEqual { target, .. } |
                Instruction::JumpGreater { target, .. } |
                Instruction::JumpGreaterEqual { target, .. } => {
                    // 条件跳转：两个后继
                    if let Some(&target_block) = cfg.label_to_block.get(target) {
                        cfg.add_edge(block_id, target_block);
                    }
                    // 顺序执行到下一个基本块
                    if block_id + 1 < cfg.blocks.len() {
                        cfg.add_edge(block_id, block_id + 1);
                    }
                }
                Instruction::Return { .. } => {
                    // 返回指令没有后继
                }
                _ => {
                    // 其他指令：顺序执行到下一个基本块
                    if block_id + 1 < cfg.blocks.len() {
                        cfg.add_edge(block_id, block_id + 1);
                    }
                }
            }
        }
    }
    
    /// 消除φ指令
    fn eliminate_phi_instructions(&self, function: &mut LirFunction) -> Result<(), String> {
        let cfg = self.build_cfg_info(function);
        let mut instructions_to_remove = Vec::new();
        let mut instructions_to_insert = Vec::new();
        
        // 扫描所有φ指令
        for (i, instruction) in function.instructions.iter().enumerate() {
            if let Instruction::Phi { dst, incoming, span } = instruction {
                println!("🔧 处理φ指令 {}: dst={:?}, incoming={:?}", i, dst, incoming);
                
                // 标记φ指令为需要移除
                instructions_to_remove.push(i);
                
                // 为每个incoming值在对应的前驱块末尾插入mov指令
                for (source_label, operand) in incoming {
                    if let Some(&source_block_id) = cfg.label_to_block.get(source_label) {
                        if let Some(source_block) = cfg.blocks.get(&source_block_id) {
                            // 在源基本块的末尾插入mov指令
                            let insert_position = self.find_insertion_point(function, source_block);
                            
                            let move_instruction = Instruction::Move {
                                dst: *dst,
                                src: operand.clone(),
                                span: *span,
                            };
                            
                            instructions_to_insert.push((insert_position, move_instruction));
                            println!("🔧 在位置 {} 插入 mov {:?}, {:?}", insert_position, dst, operand);
                        }
                    }
                }
            }
        }
        
        // 应用修改
        self.apply_modifications(function, instructions_to_remove, instructions_to_insert);
        
        Ok(())
    }
    
    /// 找到在基本块末尾插入指令的位置
    fn find_insertion_point(&self, function: &LirFunction, block: &BasicBlock) -> usize {
        // 在跳转指令之前插入
        for i in (block.start..block.end).rev() {
            match &function.instructions[i] {
                Instruction::Jump { .. } | 
                Instruction::JumpEqual { .. } | 
                Instruction::JumpNotEqual { .. } |
                Instruction::JumpLess { .. } |
                Instruction::JumpLessEqual { .. } |
                Instruction::JumpGreater { .. } |
                Instruction::JumpGreaterEqual { .. } |
                Instruction::Return { .. } => {
                    return i; // 在跳转指令之前插入
                }
                _ => {}
            }
        }
        
        // 如果没有跳转指令，在基本块末尾插入
        block.end
    }
    
    /// 应用指令修改
    fn apply_modifications(
        &self,
        function: &mut LirFunction,
        mut instructions_to_remove: Vec<usize>,
        mut instructions_to_insert: Vec<(usize, Instruction)>,
    ) {
        // 按位置排序
        instructions_to_remove.sort_by(|a, b| b.cmp(a)); // 逆序，从后往前删除
        instructions_to_insert.sort_by(|a, b| b.0.cmp(&a.0)); // 逆序，从后往前插入
        
        // 先插入新指令
        for (pos, instruction) in instructions_to_insert {
            function.instructions.insert(pos, instruction);
        }
        
        // 再删除φ指令（由于插入可能改变了位置，需要重新计算）
        // 简化处理：重新扫描并删除所有φ指令
        function.instructions.retain(|instruction| {
            !matches!(instruction, Instruction::Phi { .. })
        });
    }
}

impl FunctionPass for PhiEliminationPass {
    fn name(&self) -> &str {
        "phi-elimination"
    }
    
    fn run_on_function(&mut self, function: &mut LirFunction, _analyses: &mut AnalysisManager) -> PassResult {
        println!("🔧 运行φ指令消除Pass");
        
        match self.eliminate_phi_instructions(function) {
            Ok(()) => {
                println!("✅ φ指令消除完成");
                PassResult::Changed
            }
            Err(e) => {
                println!("❌ φ指令消除失败: {}", e);
                PassResult::Failed(e)
            }
        }
    }
}

/// 控制流图信息
#[derive(Debug)]
struct ControlFlowInfo {
    /// 基本块信息
    blocks: HashMap<usize, BasicBlock>,
    /// 标签到基本块的映射
    label_to_block: HashMap<LabelId, usize>,
    /// 前驱关系
    predecessors: HashMap<usize, Vec<usize>>,
    /// 后继关系
    successors: HashMap<usize, Vec<usize>>,
}

#[derive(Debug)]
struct BasicBlock {
    id: usize,
    start: usize,
    end: usize,
}

impl ControlFlowInfo {
    fn new() -> Self {
        Self {
            blocks: HashMap::new(),
            label_to_block: HashMap::new(),
            predecessors: HashMap::new(),
            successors: HashMap::new(),
        }
    }
    
    fn add_block(&mut self, id: usize, start: usize, end: usize) {
        self.blocks.insert(id, BasicBlock { id, start, end });
        self.predecessors.insert(id, Vec::new());
        self.successors.insert(id, Vec::new());
    }
    
    fn add_edge(&mut self, from: usize, to: usize) {
        self.successors.entry(from).or_insert_with(Vec::new).push(to);
        self.predecessors.entry(to).or_insert_with(Vec::new).push(from);
    }
} 