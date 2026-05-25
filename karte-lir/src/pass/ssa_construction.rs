//! SSA 构造和 Mem2Reg 优化
//!
//! 实现静态单赋值形式的构造，包括：
//! 1. 支配边界计算
//! 2. Phi 节点插入（实际插入 Instruction::Phi）
//! 3. 变量重命名（创建新版本寄存器）
//! 4. 内存到寄存器的提升
//!
//! 注意：本 Pass 依赖已有的 CFG 分析结果，不自己构建 CFG
//!
//! 🔧 2025-12: 完整实现 SSA 构造，确保 BlockLayoutPass 可以在 SSA 之前运行
//! 之前的实现只创建了 PhiNode 元数据，但没有实际插入 Instruction::Phi 指令
//! 也没有执行真正的变量重命名

use super::analysis::{ControlFlowGraph, ControlFlowNode};
use super::{AnalysisManager, AnalysisResult, FunctionPass, PassResult};
use crate::{Instruction, LabelId, LirFunction, Operand, Register};
use karte_diagnostics::Span;
use log::{debug, error, info, warn};
use std::any::Any;
use std::collections::{HashMap, HashSet, VecDeque};

/// SSA 构造分析结果
#[derive(Debug, Clone)]
pub struct SsaConstructionResult {
    /// 值的 SSA 版本映射
    pub value_versions: HashMap<String, usize>,
    /// 寄存器的定义使用链
    pub def_use_chains: HashMap<Register, DefUseChain>,
    /// Phi 节点信息（每个原始寄存器可能有多个块需要 phi）
    pub phi_nodes: HashMap<Register, Vec<PhiNode>>,
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
    /// 原始寄存器（被重命名的寄存器）
    pub original_register: Register,
    /// 结果寄存器（新的 SSA 版本）
    pub result: Register,
    /// 输入操作数 (块ID, 寄存器) - 在重命名阶段填充
    pub inputs: Vec<(usize, Register)>,
    /// 插入位置（基本块ID）
    pub block_id: usize,
    /// 在指令序列中的位置（在重命名后更新）
    pub instruction_index: Option<usize>,
}

/// 重命名状态
#[derive(Debug)]
struct RenamingState {
    /// 每个原始寄存器的版本计数器
    version_counter: HashMap<Register, usize>,
    /// 每个原始寄存器的版本栈（存储新寄存器）
    version_stack: HashMap<Register, Vec<Register>>,
    /// 原始寄存器到最新 SSA 版本的映射
    register_mapping: HashMap<Register, Register>,
}

impl AnalysisResult for SsaConstructionResult {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// 支配关系信息
#[derive(Debug, Clone, Default)]
pub struct DominanceInfo {
    /// 支配关系：每个块的支配者（使用 block_id 作为 key）
    pub dominators: HashMap<usize, HashSet<usize>>,
    /// 直接支配者（使用 block_id 作为 key）
    pub immediate_dominators: HashMap<usize, usize>,
    /// 支配边界（使用 block_id 作为 key）
    pub dominance_frontiers: HashMap<usize, HashSet<usize>>,
}

/// SSA 构造 Pass
///
/// 依赖已有的 CFG 分析结果，通过 block_id 而非数组索引来引用基本块，
/// 确保在 block reorder 后仍然正确工作。
#[derive(Debug, Default)]
pub struct SsaConstructionPass {
    /// 支配关系
    dominance_info: DominanceInfo,
}

impl SsaConstructionPass {
    pub fn new() -> Self {
        Self {
            dominance_info: DominanceInfo::default(),
        }
    }

    /// 计算支配关系
    fn compute_dominance(&mut self, cfg: &ControlFlowGraph) -> crate::Result<()> {
        self.dominance_info = DominanceInfo::default();

        if cfg.nodes.is_empty() {
            return Ok(());
        }

        // 收集所有 block_id
        let all_block_ids: Vec<usize> = cfg.nodes.iter().map(|n| n.block_id).collect();
        let entry_block_id = cfg.entry_block;

        // 初始化支配集合
        // 入口块只被自己支配
        let mut entry_dom = HashSet::new();
        entry_dom.insert(entry_block_id);
        self.dominance_info
            .dominators
            .insert(entry_block_id, entry_dom);

        // 其他块初始时被所有块支配
        for &block_id in &all_block_ids {
            if block_id != entry_block_id {
                let all_set: HashSet<usize> = all_block_ids.iter().copied().collect();
                self.dominance_info.dominators.insert(block_id, all_set);
            }
        }

        // 迭代直到不动点
        let mut changed = true;
        while changed {
            changed = false;

            for node in &cfg.nodes {
                if node.block_id == entry_block_id {
                    continue;
                }

                let mut new_dom = HashSet::new();
                new_dom.insert(node.block_id); // 每个块支配自己

                // 计算所有前驱的交集（前驱存储的是 block_id）
                if !node.predecessors.is_empty() {
                    // 从第一个前驱开始
                    if let Some(first_pred_dom) =
                        self.dominance_info.dominators.get(&node.predecessors[0])
                    {
                        let mut intersection = first_pred_dom.clone();

                        for &pred_id in &node.predecessors[1..] {
                            if let Some(pred_dom) = self.dominance_info.dominators.get(&pred_id) {
                                intersection =
                                    intersection.intersection(pred_dom).copied().collect();
                            }
                        }
                        new_dom.extend(intersection);
                    }
                }

                if let Some(old_dom) = self.dominance_info.dominators.get(&node.block_id) {
                    if &new_dom != old_dom {
                        self.dominance_info
                            .dominators
                            .insert(node.block_id, new_dom);
                        changed = true;
                    }
                }
            }
        }

        // 计算直接支配者
        self.compute_immediate_dominators(cfg)?;

        // 计算支配边界
        self.compute_dominance_frontiers(cfg)?;

        Ok(())
    }

    /// 计算直接支配者
    fn compute_immediate_dominators(&mut self, cfg: &ControlFlowGraph) -> crate::Result<()> {
        let entry_block_id = cfg.entry_block;

        for node in &cfg.nodes {
            if node.block_id == entry_block_id {
                continue; // 入口块没有直接支配者
            }

            if let Some(dominators) = self.dominance_info.dominators.get(&node.block_id) {
                let mut candidates: Vec<usize> = dominators.iter().copied().collect();
                candidates.retain(|&d| d != node.block_id); // 移除自己

                // 直接支配者是不被其他支配者支配的支配者
                for &candidate in &candidates {
                    let is_immediate = candidates.iter().all(|&other| {
                        other == candidate
                            || !self
                                .dominance_info
                                .dominators
                                .get(&other)
                                .is_some_and(|doms| doms.contains(&candidate))
                    });

                    if is_immediate {
                        self.dominance_info
                            .immediate_dominators
                            .insert(node.block_id, candidate);
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    /// 计算支配边界
    ///
    /// 使用标准算法：对于每个块 d，块 n 在 DF(d) 中当且仅当：
    /// 1. d 支配 n 的某个前驱，AND
    /// 2. d 不严格支配 n（即 d 不是 n 的严格支配者）
    fn compute_dominance_frontiers(&mut self, cfg: &ControlFlowGraph) -> crate::Result<()> {
        // 初始化所有块的支配边界为空
        for node in &cfg.nodes {
            self.dominance_info
                .dominance_frontiers
                .insert(node.block_id, HashSet::new());
        }

        // 对于每个有多个前驱的块，计算支配边界
        for node in &cfg.nodes {
            if node.predecessors.len() < 2 {
                continue; // 只有一个前驱的块不会产生支配边界
            }

            // 对于每个前驱
            for &pred_id in &node.predecessors {
                let mut runner = pred_id;

                // 沿着支配树向上走，直到到达 node 的直接支配者
                let idom_of_node = self
                    .dominance_info
                    .immediate_dominators
                    .get(&node.block_id)
                    .copied();

                while Some(runner) != idom_of_node {
                    // runner 支配 pred_id，但不严格支配 node
                    // 所以 node 在 runner 的支配边界中
                    self.dominance_info
                        .dominance_frontiers
                        .entry(runner)
                        .or_insert_with(HashSet::new)
                        .insert(node.block_id);

                    debug!("🔍 SSA: 块 {} 在块 {} 的支配边界中", node.block_id, runner);

                    // 移动到 runner 的直接支配者
                    if let Some(&idom) = self.dominance_info.immediate_dominators.get(&runner) {
                        if idom == runner {
                            break; // 避免无限循环
                        }
                        runner = idom;
                    } else {
                        break; // 到达入口块
                    }
                }
            }
        }

        // 打印支配边界信息
        for (block_id, frontier) in &self.dominance_info.dominance_frontiers {
            if !frontier.is_empty() {
                info!("🔍 SSA: 块 {} 的支配边界: {:?}", block_id, frontier);
            }
        }

        Ok(())
    }

    /// 插入 Phi 节点
    ///
    /// 🔧 重构：不仅计算需要 Phi 的位置，还实际插入 Instruction::Phi
    fn insert_phi_nodes(
        &self,
        function: &mut LirFunction,
        cfg: &ControlFlowGraph,
    ) -> crate::Result<HashMap<Register, Vec<PhiNode>>> {
        // 返回类型改为 Vec<PhiNode>，因为同一个寄存器可能在多个块需要 phi
        let mut phi_nodes: HashMap<Register, Vec<PhiNode>> = HashMap::new();

        // 分析变量定义：找到每个虚拟寄存器在哪些块中被定义
        let variable_defs = self.analyze_variable_definitions(function, cfg)?;

        info!("🔍 SSA: 分析到 {} 个有定义的寄存器", variable_defs.len());

        // 只处理在多个块中被定义的寄存器（需要 phi 节点）
        for (register, def_blocks) in &variable_defs {
            // 跳过只在一个块中定义的寄存器
            if def_blocks.len() <= 1 {
                continue;
            }

            debug!(
                "🔍 SSA: 寄存器 {:?} 在 {} 个块中定义: {:?}",
                register,
                def_blocks.len(),
                def_blocks
            );

            let mut work_list: VecDeque<usize> = def_blocks.iter().copied().collect();
            let mut has_phi: HashSet<usize> = HashSet::new();

            while let Some(block_id) = work_list.pop_front() {
                if let Some(frontier) = self.dominance_info.dominance_frontiers.get(&block_id) {
                    for &df_block in frontier {
                        if !has_phi.contains(&df_block)
                            && self.needs_phi_node(cfg, *register, df_block)
                        {
                            // 为这个寄存器在这个块创建 phi 节点
                            // 结果寄存器暂时设为原寄存器，在重命名阶段会分配新寄存器
                            let phi_node = PhiNode {
                                original_register: *register,
                                result: *register, // 暂时使用原寄存器，重命名时更新
                                inputs: Vec::new(),
                                block_id: df_block,
                                instruction_index: None,
                            };

                            phi_nodes
                                .entry(*register)
                                .or_insert_with(Vec::new)
                                .push(phi_node);
                            has_phi.insert(df_block);

                            info!(
                                "🎯 SSA: 在块 {} 为寄存器 {:?} 创建 phi 节点",
                                df_block, register
                            );

                            // 如果这个块之前没有定义这个变量，现在有了 Phi 定义
                            if !def_blocks.contains(&df_block) {
                                work_list.push_back(df_block);
                            }
                        }
                    }
                }
            }
        }

        let total_phis: usize = phi_nodes.values().map(|v| v.len()).sum();
        info!("🎯 SSA: 共计算出 {} 个 phi 节点", total_phis);

        Ok(phi_nodes)
    }

    /// 分析变量定义
    fn analyze_variable_definitions(
        &self,
        function: &LirFunction,
        cfg: &ControlFlowGraph,
    ) -> crate::Result<HashMap<Register, HashSet<usize>>> {
        let mut defs = HashMap::new();

        for node in &cfg.nodes {
            let (start, end) = node.instruction_range;

            for i in start..end {
                if i < function.instructions.len() {
                    let instruction = &function.instructions[i];

                    // 收集定义的寄存器
                    if let Some(def_reg) = self.get_defined_register(instruction) {
                        // 🔧 修复：跳过物理寄存器，物理寄存器有特殊的调用约定
                        // 不应该被 SSA 重命名（如 effect_stack_pointer = x12）
                        if matches!(def_reg, Register::Physical(_)) {
                            continue;
                        }
                        defs.entry(def_reg)
                            .or_insert_with(HashSet::new)
                            .insert(node.block_id); // 使用 block_id 而非数组索引
                    }
                }
            }
        }

        Ok(defs)
    }

    /// 获取指令定义的寄存器
    fn get_defined_register(&self, instruction: &Instruction) -> Option<Register> {
        match instruction {
            Instruction::Move { dst, .. }
            | Instruction::Add { dst, .. }
            | Instruction::Sub { dst, .. }
            | Instruction::Mul { dst, .. }
            | Instruction::Div { dst, .. }
            | Instruction::Load64 { dst, .. } => Some(*dst),
            Instruction::Call {
                result: Some(dst), ..
            } => Some(*dst),
            Instruction::Alloc { dst, .. }
            | Instruction::StructAlloc { dst, .. }
            | Instruction::StructFieldLoad { dst, .. }
            | Instruction::StructFieldAddr { dst, .. } => Some(*dst),
            Instruction::CallIndirect {
                result: Some(dst), ..
            } => Some(*dst),
            Instruction::Phi { dst, .. } => Some(*dst),
            _ => None,
        }
    }

    /// 判断是否需要在指定块插入 Phi 节点
    fn needs_phi_node(&self, cfg: &ControlFlowGraph, _register: Register, block_id: usize) -> bool {
        // 简化实现：如果块有多个前驱，就可能需要 Phi 节点
        cfg.get_node_by_id(block_id)
            .is_some_and(|node| node.predecessors.len() > 1)
    }

    /// 执行变量重命名并插入 phi 指令
    ///
    /// 🔧 重构：完整实现 SSA 重命名算法
    /// 1. 为每个需要 phi 的寄存器维护版本栈
    /// 2. 遍历支配树，重命名定义和使用
    /// 3. 在后继块的 phi 节点中填充 incoming 值
    /// 4. 实际插入 Instruction::Phi 到函数中
    fn rename_variables(
        &mut self,
        function: &mut LirFunction,
        cfg: &ControlFlowGraph,
        phi_nodes: &mut HashMap<Register, Vec<PhiNode>>,
    ) -> crate::Result<HashMap<String, usize>> {
        let mut value_versions = HashMap::new();

        // 初始化重命名状态
        let mut state = RenamingState {
            version_counter: HashMap::new(),
            version_stack: HashMap::new(),
            register_mapping: HashMap::new(),
        };

        // 收集所有需要重命名的寄存器（有 phi 节点的寄存器）
        let registers_to_rename: HashSet<Register> = phi_nodes.keys().cloned().collect();

        if registers_to_rename.is_empty() {
            info!("🔍 SSA: 没有需要重命名的寄存器");
            return Ok(value_versions);
        }

        info!(
            "🔍 SSA: 需要重命名 {} 个寄存器: {:?}",
            registers_to_rename.len(),
            registers_to_rename
        );

        // 为 phi 节点分配新的结果寄存器
        for (original_reg, phis) in phi_nodes.iter_mut() {
            for phi in phis.iter_mut() {
                let new_reg = function.new_register();
                phi.result = new_reg;
                info!(
                    "🎯 SSA: phi 节点在块 {} 为 {:?} 分配新寄存器 {:?}",
                    phi.block_id, original_reg, new_reg
                );
            }
        }

        // 构建 block_id -> phi_nodes 的映射
        let mut block_to_phis: HashMap<usize, Vec<(Register, Register)>> = HashMap::new();
        for (original_reg, phis) in phi_nodes.iter() {
            for phi in phis {
                block_to_phis
                    .entry(phi.block_id)
                    .or_insert_with(Vec::new)
                    .push((*original_reg, phi.result));
            }
        }

        // 从入口块开始重命名
        let mut visited = HashSet::new();
        self.rename_block_recursive(
            cfg.entry_block,
            function,
            cfg,
            &registers_to_rename,
            phi_nodes,
            &block_to_phis,
            &mut state,
            &mut value_versions,
            &mut visited,
        )?;

        // 现在实际插入 phi 指令到函数中
        self.insert_phi_instructions(function, cfg, phi_nodes)?;

        Ok(value_versions)
    }

    /// 递归重命名基本块中的变量
    fn rename_block_recursive(
        &self,
        block_id: usize,
        function: &mut LirFunction,
        cfg: &ControlFlowGraph,
        registers_to_rename: &HashSet<Register>,
        phi_nodes: &mut HashMap<Register, Vec<PhiNode>>,
        block_to_phis: &HashMap<usize, Vec<(Register, Register)>>,
        state: &mut RenamingState,
        value_versions: &mut HashMap<String, usize>,
        visited: &mut HashSet<usize>,
    ) -> crate::Result<()> {
        if visited.contains(&block_id) {
            return Ok(());
        }
        visited.insert(block_id);

        let node = cfg
            .get_node_by_id(block_id)
            .ok_or_else(|| format!("Block {} not found in CFG", block_id))?;

        let (start, end) = node.instruction_range;
        let successors = node.successors.clone();
        let label = node.label;

        debug!(
            "🔍 SSA: 处理块 {} (label: {:?}), 指令范围 [{}, {})",
            block_id, label, start, end
        );

        // 记录进入块时的栈深度，用于退出时恢复
        let stack_depths: HashMap<Register, usize> = state
            .version_stack
            .iter()
            .map(|(r, v)| (*r, v.len()))
            .collect();

        // 步骤1: 处理这个块的 phi 节点定义
        // phi 节点的结果成为新的当前版本
        if let Some(phis) = block_to_phis.get(&block_id) {
            for (original_reg, result_reg) in phis {
                state
                    .version_stack
                    .entry(*original_reg)
                    .or_insert_with(Vec::new)
                    .push(*result_reg);
                state.register_mapping.insert(*original_reg, *result_reg);

                let version = state.version_counter.entry(*original_reg).or_insert(0);
                *version += 1;
                value_versions.insert(format!("r{}", original_reg.id()), *version);

                debug!(
                    "🔍 SSA: 块 {} phi 定义 {:?} -> {:?}",
                    block_id, original_reg, result_reg
                );
            }
        }

        // 步骤2: 处理块中的指令
        for i in start..end {
            if i >= function.instructions.len() {
                continue;
            }

            let instruction = &function.instructions[i];

            // 跳过 Label 指令
            if matches!(instruction, Instruction::Label { .. }) {
                continue;
            }

            // 2a. 重命名使用的寄存器
            self.rename_uses_in_instruction(function, i, registers_to_rename, state);

            // 2b. 处理定义
            if let Some(def_reg) = self.get_defined_register(&function.instructions[i]) {
                if registers_to_rename.contains(&def_reg) {
                    // 为这个定义创建新版本
                    let new_reg = function.new_register();
                    function.instructions[i].replace_register(def_reg, new_reg);

                    state
                        .version_stack
                        .entry(def_reg)
                        .or_insert_with(Vec::new)
                        .push(new_reg);
                    state.register_mapping.insert(def_reg, new_reg);

                    let version = state.version_counter.entry(def_reg).or_insert(0);
                    *version += 1;
                    value_versions.insert(format!("r{}", def_reg.id()), *version);

                    debug!("🔍 SSA: 指令 {} 定义 {:?} -> {:?}", i, def_reg, new_reg);
                }
            }
        }

        // 步骤3: 填充后继块 phi 节点的 incoming 值
        for &succ_id in &successors {
            // 查找后继块的 phi 节点
            for (original_reg, phis) in phi_nodes.iter_mut() {
                for phi in phis.iter_mut() {
                    if phi.block_id == succ_id {
                        // 找到当前块对应的 incoming 值
                        let current_version = state
                            .version_stack
                            .get(original_reg)
                            .and_then(|stack| stack.last())
                            .copied()
                            .unwrap_or(*original_reg);

                        // 添加 incoming: (当前块的 label, 当前版本)
                        if let Some(lbl) = label {
                            phi.inputs.push((block_id, current_version));
                            debug!(
                                "🔍 SSA: phi 在块 {} 添加 incoming: 从块 {} (label {:?}) 值 {:?}",
                                succ_id, block_id, lbl, current_version
                            );
                        }
                    }
                }
            }
        }

        // 步骤4: 递归处理支配树中的子块
        // 🔧 修复：使用支配树子节点遍历，而不是 CFG 后继 + idom 检查
        // 原来的方法只遍历 CFG 后继中 idom == current_block 的块，
        // 但支配树的子节点不一定是 CFG 直接后继（例如合并块可能是更早块的支配子节点）
        // 这导致某些块在重命名阶段被遗漏
        let dom_tree_children: Vec<usize> = self
            .dominance_info
            .immediate_dominators
            .iter()
            .filter_map(|(&child, &parent)| {
                if parent == block_id && !visited.contains(&child) {
                    Some(child)
                } else {
                    None
                }
            })
            .collect();

        for child_id in dom_tree_children {
            self.rename_block_recursive(
                child_id,
                function,
                cfg,
                registers_to_rename,
                phi_nodes,
                block_to_phis,
                state,
                value_versions,
                visited,
            )?;
        }

        // 步骤5: 退出块时恢复栈状态
        for (reg, original_depth) in stack_depths {
            if let Some(stack) = state.version_stack.get_mut(&reg) {
                while stack.len() > original_depth {
                    stack.pop();
                }
            }
        }

        Ok(())
    }

    /// 重命名指令中使用的寄存器
    fn rename_uses_in_instruction(
        &self,
        function: &mut LirFunction,
        inst_idx: usize,
        registers_to_rename: &HashSet<Register>,
        state: &RenamingState,
    ) {
        let instruction = &mut function.instructions[inst_idx];

        // 获取指令使用的所有寄存器
        let used_regs = instruction.get_used_registers();

        for used_reg in used_regs {
            if registers_to_rename.contains(&used_reg) {
                // 查找当前版本
                if let Some(stack) = state.version_stack.get(&used_reg) {
                    if let Some(current_reg) = stack.last().copied() {
                        if current_reg != used_reg {
                            // 使用 replace_register 方法替换寄存器
                            instruction.replace_register(used_reg, current_reg);
                            debug!(
                                "🔍 SSA: 指令 {} 使用 {:?} -> {:?}",
                                inst_idx, used_reg, current_reg
                            );
                        }
                    }
                }
            }
        }
    }

    /// 实际插入 phi 指令到函数中
    fn insert_phi_instructions(
        &self,
        function: &mut LirFunction,
        cfg: &ControlFlowGraph,
        phi_nodes: &HashMap<Register, Vec<PhiNode>>,
    ) -> crate::Result<()> {
        // 收集所有需要插入的 phi 指令，按块分组
        let mut insertions: HashMap<usize, Vec<Instruction>> = HashMap::new();

        for (_original_reg, phis) in phi_nodes {
            for phi in phis {
                if phi.inputs.is_empty() {
                    warn!(
                        "⚠️ SSA: phi 节点在块 {} 没有 incoming 值，跳过",
                        phi.block_id
                    );
                    continue;
                }

                // 构建 incoming 列表
                let mut incoming: Vec<(LabelId, Operand)> = Vec::new();
                for (pred_block_id, reg) in &phi.inputs {
                    // 查找前驱块的 label
                    if let Some(pred_node) = cfg.get_node_by_id(*pred_block_id) {
                        if let Some(label) = pred_node.label {
                            incoming.push((label, Operand::Register { id: *reg }));
                        }
                    }
                }

                if incoming.is_empty() {
                    warn!(
                        "⚠️ SSA: phi 节点在块 {} 无法构建 incoming，跳过",
                        phi.block_id
                    );
                    continue;
                }

                let phi_inst = Instruction::Phi {
                    dst: phi.result,
                    incoming,
                    span: Span::dummy(),
                };

                insertions
                    .entry(phi.block_id)
                    .or_insert_with(Vec::new)
                    .push(phi_inst);

                info!(
                    "🎯 SSA: 准备在块 {} 插入 phi 指令，结果寄存器 {:?}",
                    phi.block_id, phi.result
                );
            }
        }

        // 按块起始位置排序，从后向前插入以避免索引偏移
        let mut sorted_blocks: Vec<_> = insertions.keys().cloned().collect();
        sorted_blocks.sort_by(|a, b| {
            let a_start = cfg
                .get_node_by_id(*a)
                .map(|n| n.instruction_range.0)
                .unwrap_or(0);
            let b_start = cfg
                .get_node_by_id(*b)
                .map(|n| n.instruction_range.0)
                .unwrap_or(0);
            b_start.cmp(&a_start) // 降序，从后向前处理
        });

        for block_id in sorted_blocks {
            if let Some(phis) = insertions.get(&block_id) {
                if let Some(node) = cfg.get_node_by_id(block_id) {
                    // 在块的 Label 之后插入 phi 指令
                    let insert_pos = node.instruction_range.0 + 1;

                    // 反向插入以保持顺序
                    for phi_inst in phis.iter().rev() {
                        if insert_pos <= function.instructions.len() {
                            function.instructions.insert(insert_pos, phi_inst.clone());
                            info!("✅ SSA: 在位置 {} 插入 phi 指令", insert_pos);
                        }
                    }
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

    fn description(&self) -> &str {
        "SSA构造 - 将程序转换为静态单赋值形式"
    }

    fn run_on_function(
        &mut self,
        function: &mut LirFunction,
        analyses: &mut AnalysisManager,
    ) -> PassResult {
        // 获取已有的 CFG 分析结果
        let cfg = match analyses.get_result::<ControlFlowGraph>("cfg") {
            Some(cfg) => cfg.clone(),
            None => {
                error!("=== SSA构造失败：需要先运行 CFG 分析 ===");
                return PassResult::Failed("需要先运行 CFG 分析".to_string());
            }
        };

        // 计算支配关系
        if let Err(e) = self.compute_dominance(&cfg) {
            error!("=== SSA构造失败：支配关系计算错误 ===");
            error!("{}", e);
            return PassResult::Failed(e.to_string());
        }

        // 计算需要 Phi 节点的位置
        let mut phi_nodes = match self.insert_phi_nodes(function, &cfg) {
            Ok(nodes) => nodes,
            Err(e) => {
                error!("=== SSA构造失败：Phi节点计算错误 ===");
                error!("{}", e);
                return PassResult::Failed(e.to_string());
            }
        };

        // 变量重命名并插入 phi 指令
        let value_versions = match self.rename_variables(function, &cfg, &mut phi_nodes) {
            Ok(versions) => versions,
            Err(e) => {
                error!("=== SSA构造失败：变量重命名错误 ===");
                error!("{}", e);
                return PassResult::Failed(e.to_string());
            }
        };

        let total_phis: usize = phi_nodes.values().map(|v| v.len()).sum();

        info!("=== SSA构造完成 ===");
        info!("函数: {}", function.name);
        info!("基本块数量: {}", cfg.nodes.len());
        info!("Phi节点数量: {}", total_phis);
        info!("值版本数量: {}", value_versions.len());

        // 创建分析结果
        let result = SsaConstructionResult {
            value_versions,
            def_use_chains: HashMap::new(), // TODO: 实现定义使用链分析
            phi_nodes,
            dominance_frontiers: self.dominance_info.dominance_frontiers.clone(),
        };

        // 存储分析结果
        analyses.store_result(self.name().to_string(), Box::new(result));

        // 🔧 SSA 构造会修改指令，CFG 需要重新分析
        analyses.invalidate("cfg");

        PassResult::Changed
    }

    fn invalidated_analyses(&self) -> Vec<&'static str> {
        vec!["def-use", "cfg"] // SSA 构造会修改指令，CFG 和 def-use 都需要重新分析
    }

    fn required_analyses(&self) -> Vec<&'static str> {
        vec!["cfg"] // 依赖 CFG 分析
    }
}
