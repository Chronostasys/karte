use super::{AnalysisManager, AnalysisPass, AnalysisResult};
use crate::{Instruction, LabelId, LirFunction, Operand, Register};
use log::{debug, warn};
use std::any::Any;
use std::collections::{HashMap, HashSet};

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
#[derive(Debug, Clone)]
pub struct ControlFlowGraph {
    /// 所有基本块
    pub nodes: Vec<ControlFlowNode>,
    /// 入口块
    pub entry_block: usize,
    /// 出口块
    pub exit_blocks: Vec<usize>,
    /// 标签到块ID的映射
    pub label_to_block: HashMap<LabelId, usize>,
    /// block_id 到数组索引的映射
    id_to_index: HashMap<usize, usize>,
}

impl ControlFlowGraph {
    /// 根据 block_id 获取节点
    ///
    /// 注意：不要直接使用 `cfg.nodes[block_id]`，因为 block_id 可能不等于数组索引
    /// （特别是在 block 重排之后）
    pub fn get_node_by_id(&self, block_id: usize) -> Option<&ControlFlowNode> {
        self.id_to_index
            .get(&block_id)
            .and_then(|&idx| self.nodes.get(idx))
    }

    /// 根据 block_id 获取节点的可变引用
    pub fn get_node_by_id_mut(&mut self, block_id: usize) -> Option<&mut ControlFlowNode> {
        self.id_to_index
            .get(&block_id)
            .copied()
            .and_then(|idx| self.nodes.get_mut(idx))
    }

    /// 构建 id_to_index ��射
    fn build_id_to_index_map(&mut self) {
        self.id_to_index.clear();
        for (idx, node) in self.nodes.iter().enumerate() {
            self.id_to_index.insert(node.block_id, idx);
        }
    }
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
    pub definitions: HashMap<Register, Vec<usize>>,
    /// 每个寄存器的使用位置 (寄存器ID -> 指令位置列表)
    pub uses: HashMap<Register, Vec<usize>>,
    /// 每个指令定义的寄存器
    pub instruction_defs: HashMap<usize, Vec<Register>>,
    /// 每个指令使用的寄存器
    pub instruction_uses: HashMap<usize, Vec<Register>>,
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
    pub live_in: HashMap<usize, HashSet<Register>>,
    /// 每个基本块出口处的活跃变量
    pub live_out: HashMap<usize, HashSet<Register>>,
    /// 每个指令位置的活跃变量
    pub live_at_instruction: HashMap<usize, HashSet<Register>>,
}

impl AnalysisResult for LivenessAnalysis {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// 控制流图分析 Pass
#[derive(Debug)]
pub struct ControlFlowAnalysis;

impl Default for ControlFlowAnalysis {
    fn default() -> Self {
        Self::new()
    }
}

impl ControlFlowAnalysis {
    pub fn new() -> Self {
        Self
    }

    /// 构建控制流图
    fn build_cfg(&self, function: &LirFunction) -> crate::Result<ControlFlowGraph> {
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
        debug!("lir: \n{:?}", function);
        debug!("🔍 控制流图分析结果: {:?}", nodes);

        // 构建 id_to_index 映射
        let mut id_to_index = HashMap::new();
        for (idx, node) in nodes.iter().enumerate() {
            id_to_index.insert(node.block_id, idx);
        }

        Ok(ControlFlowGraph {
            nodes,
            entry_block,
            exit_blocks,
            label_to_block,
            id_to_index,
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
                Instruction::Jump { .. }
                | Instruction::JumpEqual { .. }
                | Instruction::JumpNotEqual { .. }
                | Instruction::JumpLess { .. }
                | Instruction::JumpLessEqual { .. }
                | Instruction::JumpGreater { .. }
                | Instruction::JumpGreaterEqual { .. } => {
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
    ) -> crate::Result<()> {
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
                Instruction::JumpEqual { target, .. }
                | Instruction::JumpNotEqual { target, .. }
                | Instruction::JumpLess { target, .. }
                | Instruction::JumpLessEqual { target, .. }
                | Instruction::JumpGreater { target, .. }
                | Instruction::JumpGreaterEqual { target, .. } => {
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
                    if i < function.instructions.len()
                        && matches!(function.instructions[i], Instruction::Return { .. })
                    {
                        exit_blocks.push(block_id);
                        break;
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

    fn description(&self) -> &str {
        "控制流图分析 - 构建基本块和控制流信息"
    }

    fn analyze_function(
        &mut self,
        function: &LirFunction,
        _analyses: &AnalysisManager,
    ) -> crate::Result<Box<dyn AnalysisResult>> {
        let cfg = self.build_cfg(function)?;
        Ok(Box::new(cfg))
    }
}

/// 定义-使用链分析 Pass
#[derive(Debug)]
pub struct DefUseAnalysis;

impl Default for DefUseAnalysis {
    fn default() -> Self {
        Self::new()
    }
}

impl DefUseAnalysis {
    pub fn new() -> Self {
        Self
    }

    /// 构建定义-使用链
    fn build_def_use_chains(&self, function: &LirFunction) -> crate::Result<DefUseChains> {
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
                definitions
                    .entry(def_reg)
                    .or_insert_with(Vec::new)
                    .push(instruction_index);
            }

            // 记录每个寄存器的使用位置
            for use_reg in used {
                uses.entry(use_reg)
                    .or_insert_with(Vec::new)
                    .push(instruction_index);
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
    fn analyze_instruction(&self, instruction: &Instruction) -> (Vec<Register>, Vec<Register>) {
        let mut defs = Vec::new();
        let mut uses = Vec::new();

        match instruction {
            Instruction::Move { dst, src, .. } => {
                defs.push(*dst);
                self.analyze_operand_uses(src, &mut uses);
            }
            Instruction::Add {
                dst, src1, src2, ..
            }
            | Instruction::Sub {
                dst, src1, src2, ..
            }
            | Instruction::Mul {
                dst, src1, src2, ..
            }
            | Instruction::Div {
                dst, src1, src2, ..
            } => {
                defs.push(*dst);
                self.analyze_operand_uses(src1, &mut uses);
                self.analyze_operand_uses(src2, &mut uses);
            }
            Instruction::Compare { src1, src2, .. } => {
                self.analyze_operand_uses(src1, &mut uses);
                self.analyze_operand_uses(src2, &mut uses);
            }
            Instruction::CompareSet { dst, src1, src2, .. } => {
                defs.push(*dst);
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
            Instruction::CallIndirect {
                function_register,
                args,
                result,
                ..
            } => {
                // 函数寄存器被使用
                uses.push(*function_register);
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
                // 🔧 修复：Alloc 指令定义 dst 寄存器
                // 在降级后的 LIR 中，Alloc 明确定义了目标寄存器
                defs.push(*dst);
            }
            Instruction::StructAlloc { dst, .. } => {
                defs.push(*dst);
            }
            Instruction::StructFieldLoad {
                dst, struct_addr, ..
            } => {
                defs.push(*dst);
                uses.push(*struct_addr);
            }
            Instruction::StructFieldStore {
                struct_addr, src, ..
            } => {
                uses.push(*struct_addr);
                self.analyze_operand_uses(src, &mut uses);
            }
            Instruction::Phi { dst, incoming, .. } => {
                defs.push(*dst);
                // Phi指令使用来自不同前驱块的寄存器
                for (_, operand) in incoming {
                    self.analyze_operand_uses(operand, &mut uses);
                }
            }
            // 跳转指令和标签不涉及寄存器定义/使用
            Instruction::Jump { .. }
            | Instruction::JumpEqual { .. }
            | Instruction::JumpNotEqual { .. }
            | Instruction::JumpLess { .. }
            | Instruction::JumpLessEqual { .. }
            | Instruction::JumpGreater { .. }
            | Instruction::JumpGreaterEqual { .. }
            | Instruction::Label { .. }
            | Instruction::Nop { .. } => {
                // 这些指令不涉及寄存器
            }
            // 未知指令类型的默认处理
            _ => {
                warn!("警告: DefUseAnalysis遇到未知指令类型: {:?}", instruction);
                // 不返回任何定义或使用
            }
        }

        (defs, uses)
    }

    /// 分析操作数的使用
    fn analyze_operand_uses(&self, operand: &Operand, uses: &mut Vec<Register>) {
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
            Operand::Label { .. } => {
                // 标签不使用寄存器
            }
            // 处理其他可能的操作数类型
            _ => {
                // 未知操作数类型，暂时不处理
            }
        }
    }
}

impl AnalysisPass for DefUseAnalysis {
    fn name(&self) -> &str {
        "def-use"
    }

    fn description(&self) -> &str {
        "定义-使用链分析 - 跟踪每个寄存器的定义和使用位置"
    }

    fn analyze_function(
        &mut self,
        function: &LirFunction,
        _analyses: &AnalysisManager,
    ) -> crate::Result<Box<dyn AnalysisResult>> {
        let def_use = self.build_def_use_chains(function)?;
        Ok(Box::new(def_use))
    }
}

/// 活跃变量分析 Pass
///
/// 实现标准的向后数据流分析算法，正确处理 Phi 指令的特殊语义。
///
/// ## 算法
/// 使用迭代数据流方程求解：
/// - live_out[B] = ∪(live_in[S] for S in successors(B))
/// - live_in[B] = use[B] ∪ (live_out[B] - def[B])
///
/// ## Phi 指令处理
/// Phi 指令 `dst = φ(val1 from pred1, val2 from pred2, ...)` 的语义：
/// - dst 在当前块入口被定义
/// - val1 在 pred1 块出口被使用
/// - val2 在 pred2 块出口被使用
#[derive(Debug)]
pub struct LivenessAnalysisPass;

impl Default for LivenessAnalysisPass {
    fn default() -> Self {
        Self::new()
    }
}

impl LivenessAnalysisPass {
    pub fn new() -> Self {
        Self
    }

    /// 执行活跃度分析
    fn analyze_liveness(
        &self,
        function: &LirFunction,
        cfg: &ControlFlowGraph,
        def_use: &DefUseChains,
    ) -> crate::Result<LivenessAnalysis> {
        debug!("🔍 开始活跃度分析");

        // 第一阶段：计算块级的 use 和 def 集合
        let (block_use, block_def) = self.compute_block_use_def(function, cfg, def_use)?;

        // 第二阶段：迭代求解活跃度方程
        let (live_in, live_out) =
            self.compute_block_liveness(function, cfg, &block_use, &block_def)?;

        // 第三阶段：计算指令级活跃度
        let live_at_instruction =
            self.compute_instruction_liveness(function, cfg, def_use, &live_out)?;

        debug!("✅ 活跃度分析完成");
        Ok(LivenessAnalysis {
            live_in,
            live_out,
            live_at_instruction,
        })
    }

    /// 计算每个块的 use 和 def 集合
    fn compute_block_use_def(
        &self,
        function: &LirFunction,
        cfg: &ControlFlowGraph,
        def_use: &DefUseChains,
    ) -> crate::Result<
        (
            HashMap<usize, HashSet<Register>>,
            HashMap<usize, HashSet<Register>>,
        ),
    > {
        let mut block_use = HashMap::new();
        let mut block_def = HashMap::new();

        for node in &cfg.nodes {
            let mut use_set = HashSet::new();
            let mut def_set = HashSet::new();

            let (start, end) = node.instruction_range;
            for instr_idx in start..end {
                if instr_idx >= function.instructions.len() {
                    continue;
                }

                let instruction = &function.instructions[instr_idx];

                // Phi 指令的特殊处理
                if let Instruction::Phi { dst, .. } = instruction {
                    // Phi 的 dst 在块入口就被定义
                    def_set.insert(*dst);
                    // Phi 的使用在后继块的 live_out 计算时处理
                    continue;
                }

                // 普通指令：先记录使用（在定义之前的使用）
                if let Some(uses) = def_use.instruction_uses.get(&instr_idx) {
                    for &reg in uses {
                        if !def_set.contains(&reg) {
                            use_set.insert(reg);
                        }
                    }
                }

                // 然后记录定义
                if let Some(defs) = def_use.instruction_defs.get(&instr_idx) {
                    for &reg in defs {
                        def_set.insert(reg);
                    }
                }
            }

            block_use.insert(node.block_id, use_set);
            block_def.insert(node.block_id, def_set);
        }

        Ok((block_use, block_def))
    }

    /// 迭代求解块级活跃度
    fn compute_block_liveness(
        &self,
        function: &LirFunction,
        cfg: &ControlFlowGraph,
        block_use: &HashMap<usize, HashSet<Register>>,
        block_def: &HashMap<usize, HashSet<Register>>,
    ) -> crate::Result<
        (
            HashMap<usize, HashSet<Register>>,
            HashMap<usize, HashSet<Register>>,
        ),
    > {
        let mut live_in: HashMap<usize, HashSet<Register>> = HashMap::new();
        let mut live_out: HashMap<usize, HashSet<Register>> = HashMap::new();

        // 初始化
        for node in &cfg.nodes {
            live_in.insert(node.block_id, HashSet::new());
            live_out.insert(node.block_id, HashSet::new());
        }

        // 迭代直到不动点
        let mut changed = true;
        let mut iteration = 0;
        while changed {
            changed = false;
            iteration += 1;

            // 逆后序遍历以加速收敛
            for node in cfg.nodes.iter().rev() {
                let block_id = node.block_id;

                // 计算 live_out：从后继块传播
                let mut new_live_out = HashSet::new();
                for &successor_id in &node.successors {
                    if let Some(succ_live_in) = live_in.get(&successor_id) {
                        // 标准传播：后继的 live_in
                        new_live_out.extend(succ_live_in.iter().copied());
                    }

                    // Phi 指令的特殊处理：
                    // 如果后继块 S 有 Phi 指令，检查从当前块来的操作数
                    if let Some(successor_node) =
                        cfg.nodes.iter().find(|n| n.block_id == successor_id)
                    {
                        self.add_phi_uses_from_predecessor(
                            function,
                            &mut new_live_out,
                            successor_node,
                            node,
                            cfg,
                        );
                    }
                }

                // 计算 live_in = use[B] ∪ (live_out[B] - def[B])
                let mut new_live_in = block_use.get(&block_id).cloned().unwrap_or_default();
                let def_set = block_def.get(&block_id).cloned().unwrap_or_default();
                let live_out_minus_def: HashSet<_> =
                    new_live_out.difference(&def_set).copied().collect();
                new_live_in.extend(live_out_minus_def);

                // 检查是否有变化
                if live_out.get(&block_id) != Some(&new_live_out) {
                    live_out.insert(block_id, new_live_out);
                    changed = true;
                }
                if live_in.get(&block_id) != Some(&new_live_in) {
                    live_in.insert(block_id, new_live_in);
                    changed = true;
                }
            }
        }

        debug!("活跃度分析迭���次数: {}", iteration);
        Ok((live_in, live_out))
    }

    /// 添加从 Phi 指令来的使用
    fn add_phi_uses_from_predecessor(
        &self,
        function: &LirFunction,
        live_out: &mut HashSet<Register>,
        successor_node: &ControlFlowNode,
        current_node: &ControlFlowNode,
        cfg: &ControlFlowGraph,
    ) {
        let (start, end) = successor_node.instruction_range;

        // 扫描后继块的 Phi 指令
        for instr_idx in start..end {
            if instr_idx >= function.instructions.len() {
                break;
            }

            let instruction = &function.instructions[instr_idx];

            // 只处理 Phi 指令
            if let Instruction::Phi { incoming, .. } = instruction {
                // 查找从当前块来的操作数
                if let Some(current_label) = current_node.label {
                    if let Some((_, operand)) =
                        incoming.iter().find(|(label, _)| *label == current_label)
                    {
                        // 将该操作数添加到当前块的 live_out
                        if let Operand::Register { id } = operand {
                            live_out.insert(*id);
                        }
                    }
                }
            } else {
                // Phi 指令应该在块的开始，一旦遇到非 Phi 指令就停止
                break;
            }
        }
    }

    /// 计算指令级活跃度
    fn compute_instruction_liveness(
        &self,
        function: &LirFunction,
        cfg: &ControlFlowGraph,
        def_use: &DefUseChains,
        live_out: &HashMap<usize, HashSet<Register>>,
    ) -> crate::Result<HashMap<usize, HashSet<Register>>> {
        let mut live_at_instruction = HashMap::new();

        for node in &cfg.nodes {
            let (start, end) = node.instruction_range;
            let mut current_live = live_out.get(&node.block_id).cloned().unwrap_or_default();

            // 向后扫描块中的指令
            for instr_idx in (start..end).rev() {
                if instr_idx >= function.instructions.len() {
                    continue;
                }

                // 记录指令后的活跃集合
                live_at_instruction.insert(instr_idx, current_live.clone());

                let instruction = &function.instructions[instr_idx];

                // Phi 指令的特殊处理
                if let Instruction::Phi { dst, .. } = instruction {
                    // Phi 的定义会使 dst 不再活跃
                    current_live.remove(dst);
                    // Phi 的使用已在块级处理，这里不处理
                    continue;
                }

                // 普通指令：先移除定义
                if let Some(defs) = def_use.instruction_defs.get(&instr_idx) {
                    for &reg in defs {
                        current_live.remove(&reg);
                    }
                }

                // 再添加使用
                if let Some(uses) = def_use.instruction_uses.get(&instr_idx) {
                    for &reg in uses {
                        current_live.insert(reg);
                    }
                }
            }
        }

        Ok(live_at_instruction)
    }
}

impl AnalysisPass for LivenessAnalysisPass {
    fn name(&self) -> &str {
        "liveness"
    }

    fn description(&self) -> &str {
        "活跃变量分析 - 计算每个程序点的活跃变量集合（支持 Phi 指令）"
    }

    fn analyze_function(
        &mut self,
        function: &LirFunction,
        analyses: &AnalysisManager,
    ) -> crate::Result<Box<dyn AnalysisResult>> {
        // 获取 CFG 分析结果
        let cfg = analyses
            .get_result::<ControlFlowGraph>("cfg")
            .ok_or_else(|| "需要先运行 CFG 分析".to_string())?;

        // 获取 DefUse 分析结果
        let def_use = analyses
            .get_result::<DefUseChains>("def-use")
            .ok_or_else(|| "需要先运行 DefUse 分析".to_string())?;

        // 执行活跃度分析
        let liveness = self.analyze_liveness(function, cfg, def_use)?;

        Ok(Box::new(liveness))
    }

    fn required_analyses(&self) -> Vec<&'static str> {
        vec!["cfg", "def-use"]
    }
}
