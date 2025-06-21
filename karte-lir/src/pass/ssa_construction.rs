//! SSA 构造和 Mem2Reg 优化
//! 
//! 实现静态单赋值形式的构造，包括：
//! 1. 支配边界计算
//! 2. Phi 节点插入
//! 3. 变量重命名
//! 4. 内存到寄存器的提升

use super::{FunctionPass, AnalysisManager, PassResult, AnalysisResult};
use crate::{LirFunction, Instruction, RegisterId, Operand, AllocationType};
use karte_diagnostics::Span;
use karte_mir::BasicBlockId;
use std::collections::{HashMap, HashSet, VecDeque};
use std::any::Any;

/// SSA 构造分析结果
#[derive(Debug, Clone)]
pub struct SsaConstructionResult {
    /// 值的 SSA 版本映射
    pub value_versions: HashMap<String, usize>,
    /// 寄存器的定义使用链
    pub def_use_chains: HashMap<RegisterId, DefUseChain>,
    /// Phi 节点信息
    pub phi_nodes: HashMap<RegisterId, PhiNode>,
    /// 支配边界信息
    pub dominance_frontiers: HashMap<usize, HashSet<usize>>,
}

/// 定义使用链
#[derive(Debug, Clone)]
pub struct DefUseChain {
    /// 定义点（指令索引）
    pub definitions: Vec<usize>,
    /// 使用点（指令索引）
    pub uses: Vec<usize>,
    /// 当前活跃版本
    pub current_version: usize,
}

/// Phi 节点信息
#[derive(Debug, Clone)]
pub struct PhiNode {
    /// 结果寄存器
    pub result: RegisterId,
    /// 输入操作数 (块ID, 寄存器)
    pub inputs: Vec<(usize, RegisterId)>,
    /// 插入位置（基本块ID）
    pub block_id: usize,
}

impl AnalysisResult for SsaConstructionResult {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// SSA 构造 Pass
#[derive(Debug)]
pub struct SsaConstructionPass {
    /// 当前函数的控制流图
    control_flow_graph: Vec<BasicBlockInfo>,
    /// 支配关系
    dominance_info: DominanceInfo,
    /// 标签到基本块的映射
    label_to_block: HashMap<crate::LabelId, usize>,
}

/// 基本块信息
#[derive(Debug, Clone)]
pub struct BasicBlockInfo {
    pub id: usize,
    pub predecessors: Vec<usize>,
    pub successors: Vec<usize>,
    pub instruction_range: (usize, usize),
}

/// 支配关系信息
#[derive(Debug, Clone)]
pub struct DominanceInfo {
    /// 支配关系：每个块的支配者
    pub dominators: HashMap<usize, HashSet<usize>>,
    /// 直接支配者
    pub immediate_dominators: HashMap<usize, usize>,
    /// 支配边界
    pub dominance_frontiers: HashMap<usize, HashSet<usize>>,
}

impl SsaConstructionPass {
    pub fn new() -> Self {
        Self {
            control_flow_graph: Vec::new(),
            dominance_info: DominanceInfo {
                dominators: HashMap::new(),
                immediate_dominators: HashMap::new(),
                dominance_frontiers: HashMap::new(),
            },
            label_to_block: HashMap::new(),
        }
    }

    /// 构建控制流图
    fn build_control_flow_graph(&mut self, function: &LirFunction) -> Result<(), String> {
        self.control_flow_graph.clear();
        
        // 识别基本块边界
        let mut block_starts = vec![0]; // 函数开始是第一个基本块
        let mut leaders = HashSet::new();
        leaders.insert((None,0));
        
        // 扫描指令，找到基本块的领导指令
        for (i, instruction) in function.instructions.iter().enumerate() {
            match instruction {
                Instruction::Label { id,.. } => {
                    leaders.insert((Some(*id), i));
                }
                Instruction::Jump { .. } |
                Instruction::JumpEqual { .. } |
                Instruction::JumpNotEqual { .. } |
                Instruction::Return { .. } => {
                    // 跳转指令后的指令是新基本块的开始
                    if i + 1 < function.instructions.len() {
                        leaders.insert((None, i + 1));
                    }
                }
                _ => {}
            }
        }
        
        // 收集并排序基本块开始位置
        let mut block_starts: Vec<(Option<crate::LabelId>, usize)>  = leaders.into_iter().collect::<Vec<(Option<_>,usize)>>();
        block_starts.sort_by_key(|(_, i)| *i);
            
        // 创建基本块信息
        for (idx, &(label, start)) in block_starts.iter().enumerate() {
            let end = if idx + 1 < block_starts.len() {
                block_starts[idx + 1].1
            } else {
                function.instructions.len()
            };
            
            if let Some(label) = label {
                self.label_to_block.insert(label, idx);
            }
            
            let block_info = BasicBlockInfo {
                id: idx,
                predecessors: Vec::new(),
                successors: Vec::new(),
                instruction_range: (start, end),
            };
            
            self.control_flow_graph.push(block_info);
        }
        
        // 构建前驱后继关系
        self.build_cfg_edges(function)?;
        
        Ok(())
    }
    
    /// 构建控制流图的边
    fn build_cfg_edges(&mut self, function: &LirFunction) -> Result<(), String> {
        for block_idx in 0..self.control_flow_graph.len() {
            let (start, end) = self.control_flow_graph[block_idx].instruction_range;
            
            // 查看基本块的最后一条指令
            if start < end && end > 0 {
                let last_instruction = &function.instructions[end - 1];
                
                match last_instruction {
                    Instruction::Jump { target, .. } => {
                        // 无条件跳转
                        if let Some(target_block) = self.find_block_by_label(*target) {
                            self.control_flow_graph[block_idx].successors.push(target_block);
                            self.control_flow_graph[target_block].predecessors.push(block_idx);
                        }
                    }
                    Instruction::JumpEqual { target, .. } |
                    Instruction::JumpNotEqual { target, .. } => {
                        // 条件跳转：有两个后继（跳转目标和顺序执行）
                        if let Some(target_block) = self.find_block_by_label(*target) {
                            self.control_flow_graph[block_idx].successors.push(target_block);
                            self.control_flow_graph[target_block].predecessors.push(block_idx);
                        }
                        
                        // 顺序执行到下一个基本块
                        if block_idx + 1 < self.control_flow_graph.len() {
                            let next_block = block_idx + 1;
                            self.control_flow_graph[block_idx].successors.push(next_block);
                            self.control_flow_graph[next_block].predecessors.push(block_idx);
                        }
                    }
                    Instruction::Return { .. } => {
                        // 返回指令没有后继
                    }
                    _ => {
                        // 其他指令：顺序执行到下一个基本块
                        if block_idx + 1 < self.control_flow_graph.len() {
                            let next_block = block_idx + 1;
                            self.control_flow_graph[block_idx].successors.push(next_block);
                            self.control_flow_graph[next_block].predecessors.push(block_idx);
                        }
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// 根据标签找到基本块
    fn find_block_by_label(&self, label: crate::LabelId) -> Option<usize> {
        self.label_to_block.get(&label).cloned()
    }
    
    /// 计算支配关系
    fn compute_dominance(&mut self) -> Result<(), String> {
        let num_blocks = self.control_flow_graph.len();
        if num_blocks == 0 {
            return Ok(());
        }
        
        // 初始化支配集合
        let mut dom: Vec<HashSet<usize>> = vec![HashSet::new(); num_blocks];
        
        // 入口块只被自己支配
        dom[0].insert(0);
        
        // 其他块初始时被所有块支配
        for i in 1..num_blocks {
            for j in 0..num_blocks {
                dom[i].insert(j);
            }
        }
        
        // 迭代直到不动点
        let mut changed = true;
        while changed {
            changed = false;
            
            for i in 1..num_blocks {
                let mut new_dom = HashSet::new();
                new_dom.insert(i); // 每个块支配自己
                
                // 计算所有前驱的交集
                let predecessors = &self.control_flow_graph[i].predecessors;
                if !predecessors.is_empty() {
                    let mut intersection = dom[predecessors[0]].clone();
                    for &pred in &predecessors[1..] {
                        intersection = intersection.intersection(&dom[pred]).cloned().collect();
                    }
                    new_dom.extend(intersection);
                }
                
                if new_dom != dom[i] {
                    dom[i] = new_dom;
                    changed = true;
                }
            }
        }
        
        // 存储支配信息
        for (i, dominators) in dom.into_iter().enumerate() {
            self.dominance_info.dominators.insert(i, dominators);
        }
        
        // 计算直接支配者
        self.compute_immediate_dominators()?;
        
        // 计算支配边界
        self.compute_dominance_frontiers()?;
        
        Ok(())
    }
    
    /// 计算直接支配者
    fn compute_immediate_dominators(&mut self) -> Result<(), String> {
        for (block_id, dominators) in &self.dominance_info.dominators {
            if *block_id == 0 {
                continue; // 入口块没有直接支配者
            }
            
            let mut candidates: Vec<usize> = dominators.iter().cloned().collect();
            candidates.retain(|&d| d != *block_id); // 移除自己
            
            // 直接支配者是不被其他支配者支配的支配者
            for &candidate in &candidates {
                let is_immediate = candidates.iter().all(|&other| {
                    other == candidate || 
                    !self.dominance_info.dominators.get(&other)
                        .map_or(false, |doms| doms.contains(&candidate))
                });
                
                if is_immediate {
                    self.dominance_info.immediate_dominators.insert(*block_id, candidate);
                    break;
                }
            }
        }
        
        Ok(())
    }
    
    /// 计算支配边界
    fn compute_dominance_frontiers(&mut self) -> Result<(), String> {
        for block in &self.control_flow_graph {
            let mut frontier = HashSet::new();
            
            // 对于每个前驱
            for &pred in &block.predecessors {
                let mut runner = pred;
                
                // 沿着支配树向上走，直到到达块的直接支配者
                while !self.dominance_info.dominators
                    .get(&runner)
                    .map_or(false, |doms| doms.contains(&block.id)) || runner == block.id {
                    
                    frontier.insert(block.id);
                    
                    if let Some(&idom) = self.dominance_info.immediate_dominators.get(&runner) {
                        runner = idom;
                    } else {
                        break;
                    }
                }
            }
            
            self.dominance_info.dominance_frontiers.insert(block.id, frontier);
        }
        
        Ok(())
    }
    
    /// 插入 Phi 节点
    fn insert_phi_nodes(&mut self, function: &mut LirFunction) -> Result<HashMap<RegisterId, PhiNode>, String> {
        let mut phi_nodes = HashMap::new();
        
        // 分析变量定义
        let variable_defs = self.analyze_variable_definitions(function)?;
        
        // 为每个变量插入 Phi 节点
        for (register, def_blocks) in variable_defs {
            let mut work_list: VecDeque<usize> = def_blocks.iter().cloned().collect();
            let mut has_phi = HashSet::new();
            
            while let Some(block) = work_list.pop_front() {
                if let Some(frontier) = self.dominance_info.dominance_frontiers.get(&block) {
                    for &df_block in frontier {
                        if !has_phi.contains(&df_block) && self.needs_phi_node(register, df_block) {
                            // 在支配边界插入 Phi 节点
                            let phi_node = PhiNode {
                                result: register,
                                inputs: Vec::new(), // 稍后填充
                                block_id: df_block,
                            };
                            
                            phi_nodes.insert(register, phi_node);
                            has_phi.insert(df_block);
                            
                            // 如果这个块之前没有定义这个变量，现在有了 Phi 定义
                            if !def_blocks.contains(&df_block) {
                                work_list.push_back(df_block);
                            }
                        }
                    }
                }
            }
        }
        
        Ok(phi_nodes)
    }
    
    /// 分析变量定义
    fn analyze_variable_definitions(&self, function: &LirFunction) -> Result<HashMap<RegisterId, HashSet<usize>>, String> {
        let mut defs = HashMap::new();
        
        for (block_idx, block) in self.control_flow_graph.iter().enumerate() {
            let (start, end) = block.instruction_range;
            
            for i in start..end {
                if i < function.instructions.len() {
                    let instruction = &function.instructions[i];
                    
                    // 收集定义的寄存器
                    if let Some(def_reg) = self.get_defined_register(instruction) {
                        defs.entry(def_reg).or_insert_with(HashSet::new).insert(block_idx);
                    }
                }
            }
        }
        
        Ok(defs)
    }
    
    /// 获取指令定义的寄存器
    fn get_defined_register(&self, instruction: &Instruction) -> Option<RegisterId> {
        match instruction {
            Instruction::Move { dst, .. } |
            Instruction::Add { dst, .. } |
            Instruction::Sub { dst, .. } |
            Instruction::Mul { dst, .. } |
            Instruction::Div { dst, .. } |
            Instruction::Load64 { dst, .. } => Some(*dst),
            Instruction::Call { result: Some(dst), .. } => Some(*dst),
            Instruction::Alloc { dst, .. } |
            Instruction::StructAlloc { dst, .. } |
            Instruction::StructFieldLoad { dst, .. } |
            Instruction::StructFieldAddr { dst, .. } => Some(*dst),
            Instruction::CallIndirect { result: Some(dst), .. } => Some(*dst),
            Instruction::Phi { dst, .. } => Some(*dst),
            _ => None,
        }
    }
    
    /// 判断是否需要在指定块插入 Phi 节点
    fn needs_phi_node(&self, _register: RegisterId, block_id: usize) -> bool {
        // 简化实现：如果块有多个前驱，就可能需要 Phi 节点
        self.control_flow_graph.get(block_id)
            .map_or(false, |block| block.predecessors.len() > 1)
    }
    
    /// 执行变量重命名
    fn rename_variables(&mut self, function: &mut LirFunction, phi_nodes: &HashMap<RegisterId, PhiNode>) -> Result<HashMap<String, usize>, String> {
        let mut value_versions = HashMap::new();
        let mut version_counters = HashMap::new();
        let mut version_stacks: HashMap<RegisterId, Vec<usize>> = HashMap::new();
        
        // 从入口块开始重命名
        self.rename_block(0, function, phi_nodes, &mut value_versions, &mut version_counters, &mut version_stacks)?;
        
        Ok(value_versions)
    }
    
    /// 重命名指定基本块中的变量
    fn rename_block(
        &self,
        block_id: usize,
        function: &mut LirFunction,
        phi_nodes: &HashMap<RegisterId, PhiNode>,
        value_versions: &mut HashMap<String, usize>,
        version_counters: &mut HashMap<RegisterId, usize>,
        version_stacks: &mut HashMap<RegisterId, Vec<usize>>,
    ) -> Result<(), String> {
        let block = &self.control_flow_graph[block_id];
        let (start, end) = block.instruction_range;
        
        // 重命名块中的指令
        for i in start..end {
            if i < function.instructions.len() {
                // 这里需要实际的重命名逻辑
                // 简化实现：记录版本信息
                if let Some(def_reg) = self.get_defined_register(&function.instructions[i]) {
                    let version = version_counters.entry(def_reg).or_insert(0);
                    *version += 1;
                    
                    version_stacks.entry(def_reg).or_insert_with(Vec::new).push(*version);
                    value_versions.insert(format!("r{}", def_reg.0), *version);
                }
            }
        }
        
        // 递归处理支配的子块
        for &successor in &block.successors {
            // 简化实现：只处理直接后继
            if let Some(&idom) = self.dominance_info.immediate_dominators.get(&successor) {
                if idom == block_id {
                    self.rename_block(successor, function, phi_nodes, value_versions, version_counters, version_stacks)?;
                }
            }
        }
        
        Ok(())
    }
}

impl FunctionPass for SsaConstructionPass {
    fn name(&self) -> &str {
        "ssa-construction"
    }
    
    fn run_on_function(&mut self, function: &mut LirFunction, analyses: &mut AnalysisManager) -> PassResult {
        // 1. 构建控制流图
        if let Err(e) = self.build_control_flow_graph(function) {
            println!("=== SSA构造失败：控制流图构建错误 ===");
            println!("{}", e);
            return PassResult::Unchanged;
        }
        
        // 2. 计算支配关系
        if let Err(e) = self.compute_dominance() {
            println!("=== SSA构造失败：支配关系计算错误 ===");
            println!("{}", e);
            return PassResult::Unchanged;
        }
        
        // 3. 插入 Phi 节点
        let phi_nodes = match self.insert_phi_nodes(function) {
            Ok(nodes) => nodes,
            Err(e) => {
                println!("=== SSA构造失败：Phi节点插入错误 ===");
                println!("{}", e);
                return PassResult::Unchanged;
            }
        };
        
        // 4. 变量重命名
        let value_versions = match self.rename_variables(function, &phi_nodes) {
            Ok(versions) => versions,
            Err(e) => {
                println!("=== SSA构造失败：变量重命名错误 ===");
                println!("{}", e);
                return PassResult::Unchanged;
            }
        };
        
        println!("=== SSA构造完成 ===");
        println!("函数: {}", function.name);
        println!("基本块数量: {}", self.control_flow_graph.len());
        println!("Phi节点数量: {}", phi_nodes.len());
        println!("值版本数量: {}", value_versions.len());
        
        // 存储分析结果
        let result = SsaConstructionResult {
            value_versions,
            def_use_chains: HashMap::new(), // 简化实现
            phi_nodes,
            dominance_frontiers: self.dominance_info.dominance_frontiers.clone(),
        };
        
        analyses.store_result(self.name().to_string(), Box::new(result));
        
        PassResult::Changed
    }
    
    fn invalidated_analyses(&self) -> Vec<&'static str> {
        vec!["def-use", "cfg"] // SSA 构造会改变控制流和定义使用关系
    }
} 