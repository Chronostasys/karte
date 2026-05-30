//! 基本块布局优化Pass
//!
//! 该Pass重排LIR函数中基本块的物理顺序，使其与CFG执行顺序一致。
//!
//! ## 目标
//! 1. 解决生命周期区间表示问题（物理顺序与执行顺序不一致导致的错误）
//! 2. 提高指令缓存命中率（热路径连续布局）
//! 3. 优化分支预测（减少taken branches，利用fall-through）
//! 4. 减少无条件跳转指令
//!
//! ## 算法
//! 基于CFG的拓扑遍历，优先选择：
//! 1. 循环内的基本块保持连续
//! 2. 热路径（forward edge）优先fall-through
//! 3. 异常/冷路径放到函数末尾
//!
//! ## 执行时机
//! 必须在寄存器分配之前运行，确保生命周期分析的正确性。

use super::{AnalysisManager, AnalysisPass, FunctionPass, PassResult};
use crate::pass::analysis::ControlFlowGraph;
use crate::{Instruction, LabelId, LirFunction, Operand};
use log::{debug, info};
use std::collections::{HashMap, HashSet, VecDeque};

/// 基本块布局优化Pass
#[derive(Debug)]
pub struct BlockLayoutPass;

impl BlockLayoutPass {
    pub fn new() -> Self {
        Self
    }

    /// 重排基本块，使物理顺序与CFG执行顺序一致，并合并可合并的块
    ///
    /// 算法：
    /// 1. 从入口块开始DFS遍历CFG
    /// 2. 优先选择forward edge（非回边）
    /// 3. 循环块连续放置
    /// 4. 识别并合并可合并的块对
    /// 5. 重建指令序列
    fn reorder_blocks(
        &self,
        function: &LirFunction,
        cfg: &ControlFlowGraph,
    ) -> crate::Result<Vec<Instruction>> {
        info!("🔄 开始基本块布局优化");

        // 1. 确定新的块顺序
        let new_order = self.compute_optimal_order(cfg)?;

        debug!("📋 新块顺序: {:?}", new_order);

        // 2. 识别可合并的块对
        let mergeable_pairs = self.find_mergeable_blocks(cfg, &new_order);
        debug!("🔗 可合并的块对: {:?}", mergeable_pairs);

        // 3. 收集所有被跳转指令引用的标签
        let referenced_labels = self.collect_referenced_labels(function);
        debug!("📍 被引用的标签数量: {}", referenced_labels.len());

        // 4. 计算合并后将被移除的跳转目标标签
        let mut labels_to_remove: HashSet<LabelId> = HashSet::new();
        for &(pred_id, succ_id) in &mergeable_pairs {
            if let Some(succ_node) = cfg.get_node_by_id(succ_id) {
                if let Some(label) = succ_node.label {
                    // 检查这个标签是否只被前驱块的跳转引用
                    let pred_node = cfg.get_node_by_id(pred_id);
                    if let Some(pred_node) = pred_node {
                        let (_, pred_end) = pred_node.instruction_range;
                        // 计算有多少个跳转指令引用这个标签
                        let ref_count = self.count_label_references(function, label);
                        // 如果只有一个引用（就是前驱块的跳转），可以移除
                        if ref_count == 1 {
                            labels_to_remove.insert(label);
                        }
                    }
                }
            }
        }

        // 5. 构建合并映射：被合并的块 -> 合并到的目标块
        let mut merged_into: HashMap<usize, usize> = HashMap::new();
        for &(pred, succ) in &mergeable_pairs {
            merged_into.insert(succ, pred);
        }

        // 6. 按新顺序提取指令，同时执行合并
        let mut new_instructions = Vec::new();
        let mut label_remapping = HashMap::new();
        let mut processed_blocks = HashSet::new();

        for &block_id in &new_order {
            // 跳过已被合并到其他块的块
            if merged_into.contains_key(&block_id) {
                debug!("⏭️ 跳过已合并的块 {}", block_id);
                continue;
            }

            if processed_blocks.contains(&block_id) {
                continue;
            }

            // 收集当前块及其所有被合并的后续块
            let blocks_to_emit = self.collect_merged_chain(block_id, &mergeable_pairs, cfg);

            for (idx, &current_block_id) in blocks_to_emit.iter().enumerate() {
                processed_blocks.insert(current_block_id);

                let node = cfg
                    .get_node_by_id(current_block_id)
                    .ok_or_else(|| format!("无法找到块 {} 的节点信息", current_block_id))?;
                let (start, end) = node.instruction_range;

                debug!(
                    "📦 处理块 {} (指令 {}-{}){}",
                    current_block_id,
                    start,
                    end,
                    if idx > 0 { " [已合并]" } else { "" }
                );

                // 提取块中的指令
                for i in start..end {
                    if i >= function.instructions.len() {
                        continue;
                    }

                    let inst = &function.instructions[i];

                    // 对于合并链中的非首块，只有在标签可以安全移除时才跳过
                    if idx > 0 {
                        if let Instruction::Label { id, .. } = inst {
                            if labels_to_remove.contains(id) {
                                debug!("🗑️ 移除合并块的标签 L{}", id.0);
                                continue;
                            }
                            // 标签仍被其他地方引用，保留它
                        }
                    }

                    // 检查是否是到下一个合并块的跳转，如果是且目标标签可移除则跳过
                    if idx + 1 < blocks_to_emit.len() {
                        let next_block = blocks_to_emit[idx + 1];
                        if let Some(next_node) = cfg.get_node_by_id(next_block) {
                            if let Some(next_label) = next_node.label {
                                if self.is_jump_to_label(inst, next_label)
                                    && labels_to_remove.contains(&next_label)
                                {
                                    debug!("🗑️ 移除到合并块的跳转指令");
                                    continue;
                                }
                            }
                        }
                    }

                    // 记录Label映射（旧物理位置 -> 新物理位置）
                    if let Instruction::Label { id, .. } = inst {
                        label_remapping.insert(*id, new_instructions.len());
                    }

                    new_instructions.push(inst.clone());
                }
            }
        }

        // 7. 统计优化效果
        let original_count = function.instructions.len();
        let removed_count = original_count - new_instructions.len();

        if removed_count > 0 {
            info!(
                "✅ 基本块布局优化完成: {} 个块重排, 合并 {} 对块, 移除 {} 条指令",
                new_order.len(),
                mergeable_pairs.len(),
                removed_count
            );
        } else {
            info!(
                "✅ 基本块布局优化完成: {} 个块重排为顺序 {:?}",
                new_order.len(),
                new_order
            );
        }

        Ok(new_instructions)
    }

    /// 收集所有被指令引用的标签（包括跳转、调用、效应处理器、Phi节点、操作数等）
    fn collect_referenced_labels(&self, function: &LirFunction) -> HashSet<LabelId> {
        let mut labels = HashSet::new();
        for inst in &function.instructions {
            // 从操作数中收集标签引用
            self.collect_labels_from_operands(inst, &mut labels);

            match inst {
                // 跳转指令
                Instruction::Jump { target, .. }
                | Instruction::JumpEqual { target, .. }
                | Instruction::JumpNotEqual { target, .. }
                | Instruction::JumpLess { target, .. }
                | Instruction::JumpLessEqual { target, .. }
                | Instruction::JumpGreater { target, .. }
                | Instruction::JumpGreaterEqual { target, .. } => {
                    labels.insert(*target);
                }
                // 函数调用
                Instruction::Call { target, .. } => {
                    labels.insert(*target);
                }
                // 效应处理器
                Instruction::EffectPushHandler { handler_label, .. } => {
                    labels.insert(*handler_label);
                }
                // Phi 节点引用前驱块标签
                Instruction::Phi { incoming, .. } => {
                    for (label, _) in incoming {
                        labels.insert(*label);
                    }
                }
                _ => {}
            }
        }
        labels
    }

    /// 从指令的操作数中收集标签引用
    fn collect_labels_from_operands(&self, inst: &Instruction, labels: &mut HashSet<LabelId>) {
        // 获取指令中所有的操作数
        let operands = self.get_instruction_operands(inst);
        for op in operands {
            if let Operand::Label { id } = op {
                labels.insert(*id);
            }
        }
    }

    /// 获取指令中所有的操作数
    fn get_instruction_operands<'a>(&self, inst: &'a Instruction) -> Vec<&'a Operand> {
        let mut operands = Vec::new();
        match inst {
            Instruction::Move { src, .. } => {
                operands.push(src);
            }
            Instruction::Add { src1, src2, .. }
            | Instruction::Sub { src1, src2, .. }
            | Instruction::Mul { src1, src2, .. }
            | Instruction::Div { src1, src2, .. }
            | Instruction::Mod { src1, src2, .. }
            | Instruction::Compare { src1, src2, .. }
            | Instruction::CompareSet { src1, src2, .. } => {
                operands.push(src1);
                operands.push(src2);
            }
            Instruction::Store64 { src, .. } => {
                operands.push(src);
            }
            Instruction::EffectPushHandler { tag, .. } => {
                operands.push(tag);
            }
            Instruction::EffectPerform { tag, payload, .. } => {
                operands.push(tag);
                operands.push(payload);
            }
            Instruction::EffectResume { value, .. } => {
                operands.push(value);
            }
            Instruction::Phi { incoming, .. } => {
                for (_, op) in incoming {
                    operands.push(op);
                }
            }
            Instruction::Call { arg_operands, .. }
            | Instruction::CallIndirect { arg_operands, .. } => {
                for op in arg_operands {
                    operands.push(op);
                }
            }
            _ => {}
        }
        operands
    }

    /// 计算一个标签被引用的次数（包括所有类型的引用）
    fn count_label_references(&self, function: &LirFunction, label: LabelId) -> usize {
        function
            .instructions
            .iter()
            .filter(|inst| {
                // 检查操作数中的标签引用
                let operands = self.get_instruction_operands(inst);
                for op in operands {
                    if let Operand::Label { id } = op {
                        if *id == label {
                            return true;
                        }
                    }
                }

                match inst {
                    // 跳转指令
                    Instruction::Jump { target, .. }
                    | Instruction::JumpEqual { target, .. }
                    | Instruction::JumpNotEqual { target, .. }
                    | Instruction::JumpLess { target, .. }
                    | Instruction::JumpLessEqual { target, .. }
                    | Instruction::JumpGreater { target, .. }
                    | Instruction::JumpGreaterEqual { target, .. } => *target == label,
                    // 函数调用
                    Instruction::Call { target, .. } => *target == label,
                    // 效应处理器
                    Instruction::EffectPushHandler { handler_label, .. } => *handler_label == label,
                    // Phi 节点
                    Instruction::Phi { incoming, .. } => incoming.iter().any(|(l, _)| *l == label),
                    _ => false,
                }
            })
            .count()
    }

    /// 检查指令是否是跳转到指定标签
    fn is_jump_to_label(&self, inst: &Instruction, label: LabelId) -> bool {
        match inst {
            Instruction::Jump { target, .. } => *target == label,
            _ => false,
        }
    }

    /// 识别可合并的块对
    ///
    /// 合并条件：
    /// 1. 块A只有一个后继（块B）
    /// 2. 块B只有一个前驱（块A）
    /// 3. 在新顺序中块B紧跟块A，或者块A以无条件跳转到块B结束
    fn find_mergeable_blocks(
        &self,
        cfg: &ControlFlowGraph,
        new_order: &[usize],
    ) -> Vec<(usize, usize)> {
        let mut mergeable = Vec::new();

        // 创建顺序映射：block_id -> 在new_order中的位置
        let order_map: HashMap<usize, usize> = new_order
            .iter()
            .enumerate()
            .map(|(idx, &block_id)| (block_id, idx))
            .collect();

        for &block_id in new_order {
            let node = match cfg.get_node_by_id(block_id) {
                Some(n) => n,
                None => continue,
            };

            // 条件1：块A只有一个后继
            if node.successors.len() != 1 {
                continue;
            }

            let successor_id = node.successors[0];

            // 跳过自循环
            if successor_id == block_id {
                continue;
            }

            let successor = match cfg.get_node_by_id(successor_id) {
                Some(n) => n,
                None => continue,
            };

            // 条件2：块B只有一个前驱
            if successor.predecessors.len() != 1 {
                continue;
            }

            // 条件3：在新顺序中块B紧跟块A
            let block_pos = order_map.get(&block_id);
            let succ_pos = order_map.get(&successor_id);

            if let (Some(&pos_a), Some(&pos_b)) = (block_pos, succ_pos) {
                if pos_b == pos_a + 1 {
                    debug!(
                        "🔗 发现可合并块对: {} -> {} (连续布局)",
                        block_id, successor_id
                    );
                    mergeable.push((block_id, successor_id));
                }
            }
        }

        mergeable
    }

    /// 收集从指定块开始的合并链
    ///
    /// 例如：如果 A->B 和 B->C 都是可合并的，则返回 [A, B, C]
    fn collect_merged_chain(
        &self,
        start_block: usize,
        mergeable_pairs: &[(usize, usize)],
        _cfg: &ControlFlowGraph,
    ) -> Vec<usize> {
        let mut chain = vec![start_block];
        let mut current = start_block;

        // 构建快速查找映射
        let merge_map: HashMap<usize, usize> = mergeable_pairs
            .iter()
            .map(|&(pred, succ)| (pred, succ))
            .collect();

        // 沿着合并链收集所有块
        while let Some(&next) = merge_map.get(&current) {
            chain.push(next);
            current = next;
        }

        chain
    }

    /// 计算最优的块顺序
    ///
    /// 使用改进的DFS遍历：
    /// 1. 从入口块开始
    /// 2. 优先访问未访问的successor（forward edge）
    /// 3. 循环体块连续放置
    /// 4. 回边延迟处理
    fn compute_optimal_order(&self, cfg: &ControlFlowGraph) -> crate::Result<Vec<usize>> {
        let mut visited = HashSet::new();
        let mut order = Vec::new();
        let mut worklist = VecDeque::new();

        // 识别回边（用于循环检测）
        let back_edges = self.find_back_edges(cfg);

        // 从入口块开始
        worklist.push_back(cfg.entry_block);

        while let Some(block_id) = worklist.pop_front() {
            if visited.contains(&block_id) {
                continue;
            }

            visited.insert(block_id);
            order.push(block_id);

            // 注意：使用 get_node_by_id 而不是 nodes[block_id]
            let node = match cfg.get_node_by_id(block_id) {
                Some(n) => n,
                None => continue, // 跳过找不到的块
            };

            // 优先处理非回边的successor
            for &succ in &node.successors {
                if !visited.contains(&succ) && !back_edges.contains(&(block_id, succ)) {
                    // 非回边的successor优先访问（fall-through候选）
                    worklist.push_front(succ);
                }
            }

            // 回边的successor延迟处理
            for &succ in &node.successors {
                if !visited.contains(&succ) && back_edges.contains(&(block_id, succ)) {
                    worklist.push_back(succ);
                }
            }
        }

        // 处理未访问的块（不可达代码）
        for node in &cfg.nodes {
            if !visited.contains(&node.block_id) {
                debug!("⚠️ 发现不可达块: {}", node.block_id);
                order.push(node.block_id);
            }
        }

        Ok(order)
    }

    /// 识别CFG中的回边（循环的特征）
    ///
    /// 回边定义：从节点u到节点v的边，其中v在DFS树中是u的祖先
    fn find_back_edges(&self, cfg: &ControlFlowGraph) -> HashSet<(usize, usize)> {
        let mut back_edges = HashSet::new();
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();

        fn dfs(
            node_id: usize,
            cfg: &ControlFlowGraph,
            visited: &mut HashSet<usize>,
            rec_stack: &mut HashSet<usize>,
            back_edges: &mut HashSet<(usize, usize)>,
        ) {
            visited.insert(node_id);
            rec_stack.insert(node_id);

            // 注意：使用 get_node_by_id 而不是 nodes[node_id]
            let node = match cfg.get_node_by_id(node_id) {
                Some(n) => n,
                None => {
                    rec_stack.remove(&node_id);
                    return;
                }
            };
            let successors = node.successors.clone();
            for succ in successors {
                if !visited.contains(&succ) {
                    dfs(succ, cfg, visited, rec_stack, back_edges);
                } else if rec_stack.contains(&succ) {
                    // succ在递归栈中，说明这是回边
                    back_edges.insert((node_id, succ));
                }
            }

            rec_stack.remove(&node_id);
        }

        dfs(
            cfg.entry_block,
            cfg,
            &mut visited,
            &mut rec_stack,
            &mut back_edges,
        );

        debug!("🔄 识别到 {} 条回边: {:?}", back_edges.len(), back_edges);
        back_edges
    }
}

impl Default for BlockLayoutPass {
    fn default() -> Self {
        Self::new()
    }
}

impl FunctionPass for BlockLayoutPass {
    fn name(&self) -> &str {
        "block-layout"
    }

    fn description(&self) -> &str {
        "基本块布局优化 - 重排块使物理顺序与CFG执行顺序一致，并合并连续的单后继-单前驱块对"
    }

    fn run_on_function(
        &mut self,
        function: &mut LirFunction,
        analyses: &mut AnalysisManager,
    ) -> PassResult {
        // 获取CFG分析结果
        let cfg = match analyses.get_result::<ControlFlowGraph>("cfg") {
            Some(cfg) => cfg,
            None => {
                return PassResult::Failed("BlockLayoutPass 需要先运行 CFG 分析".to_string());
            }
        };

        // 如果只有一个块，无需重排
        if cfg.nodes.len() <= 1 {
            debug!("只有一个基本块，跳过布局优化");
            return PassResult::Unchanged;
        }

        // 执行块重排
        match self.reorder_blocks(function, cfg) {
            Ok(new_instructions) => {
                // 检查是否真的改变了顺序
                if new_instructions == function.instructions {
                    debug!("块顺序已经是最优的，无需改变");
                    return PassResult::Unchanged;
                }

                // 替换指令序列
                function.instructions = new_instructions;

                PassResult::Changed
            }
            Err(e) => PassResult::Failed(format!("块布局优化失败: {}", e)),
        }
    }

    fn required_analyses(&self) -> Vec<&'static str> {
        vec!["cfg"]
    }

    fn invalidated_analyses(&self) -> Vec<&'static str> {
        // 块重排后，所有基于指令索引的分析都需要重新运行
        vec!["def-use", "liveness", "lifetime-analysis", "cfg"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pass::analysis::ControlFlowAnalysis;
    use crate::{Operand, Register};
    use karte_diagnostics::Span;

    #[test]
    fn test_block_layout_simple() {
        // 创建一个简单的乱序函数：A -> C -> B
        // 块布局优化会重新排列块顺序并可能合并连续的块
        let mut function = LirFunction::new("test".to_string());
        function.instructions = vec![
            // Block A
            Instruction::Label {
                id: LabelId(1),
                span: Span::dummy(),
            },
            Instruction::Move {
                dst: Register::Virtual(1),
                src: Operand::Immediate { value: 10 },
                span: Span::dummy(),
            },
            Instruction::Jump {
                target: LabelId(3),
                span: Span::dummy(),
            },
            // Block B
            Instruction::Label {
                id: LabelId(2),
                span: Span::dummy(),
            },
            Instruction::Return {
                value: Some(Register::Virtual(1)),
                span: Span::dummy(),
            },
            // Block C
            Instruction::Label {
                id: LabelId(3),
                span: Span::dummy(),
            },
            Instruction::Add {
                dst: Register::Virtual(2),
                src1: Operand::Register {
                    id: Register::Virtual(1),
                },
                src2: Operand::Immediate { value: 5 },
                span: Span::dummy(),
            },
            Instruction::Jump {
                target: LabelId(2),
                span: Span::dummy(),
            },
        ];

        // 运行CFG分析
        let mut analyses = AnalysisManager::new();
        let mut cfg_pass = ControlFlowAnalysis::new();
        let cfg_result = cfg_pass
            .analyze_function(&function, &analyses)
            .expect("CFG analysis failed");
        analyses.store_result("cfg".to_string(), cfg_result);

        // 运行块布局优化
        let mut layout_pass = BlockLayoutPass::new();
        let result = layout_pass.run_on_function(&mut function, &mut analyses);

        assert!(matches!(result, PassResult::Changed));

        // 验证优化结果：
        // 块布局优化会重排列块并合并连续的块
        // 可能的结果：
        // 1. 如果所有块被合并：只剩下第一个标签
        // 2. 如果保留部分标签：顺序应该是 A -> C -> B
        let labels: Vec<LabelId> = function
            .instructions
            .iter()
            .filter_map(|inst| {
                if let Instruction::Label { id, .. } = inst {
                    Some(*id)
                } else {
                    None
                }
            })
            .collect();

        // 验证至少保留了入口标签
        assert!(!labels.is_empty(), "应该至少保留入口标签");
        assert_eq!(labels[0], LabelId(1), "第一个标签应该是入口块 A");

        // 验证返回指令仍然存在
        let has_return = function
            .instructions
            .iter()
            .any(|inst| matches!(inst, Instruction::Return { .. }));
        assert!(has_return, "应该保留返回指令");
    }

    #[test]
    fn test_block_merge_simple() {
        // 创建三个连续块：A -> B -> C（均为单后继-单前驱）
        // 预期：B 和 C 的标签被移除，A->B 和 B->C 的跳转被移除
        let mut function = LirFunction::new("test_merge".to_string());
        function.instructions = vec![
            // Block A
            Instruction::Label {
                id: LabelId(1),
                span: Span::dummy(),
            },
            Instruction::Move {
                dst: Register::Virtual(1),
                src: Operand::Immediate { value: 10 },
                span: Span::dummy(),
            },
            Instruction::Jump {
                target: LabelId(2),
                span: Span::dummy(),
            },
            // Block B
            Instruction::Label {
                id: LabelId(2),
                span: Span::dummy(),
            },
            Instruction::Add {
                dst: Register::Virtual(2),
                src1: Operand::Register {
                    id: Register::Virtual(1),
                },
                src2: Operand::Immediate { value: 5 },
                span: Span::dummy(),
            },
            Instruction::Jump {
                target: LabelId(3),
                span: Span::dummy(),
            },
            // Block C
            Instruction::Label {
                id: LabelId(3),
                span: Span::dummy(),
            },
            Instruction::Return {
                value: Some(Register::Virtual(2)),
                span: Span::dummy(),
            },
        ];

        let original_count = function.instructions.len();

        // 运行CFG分析
        let mut analyses = AnalysisManager::new();
        let mut cfg_pass = ControlFlowAnalysis::new();
        let cfg_result = cfg_pass
            .analyze_function(&function, &analyses)
            .expect("CFG analysis failed");
        analyses.store_result("cfg".to_string(), cfg_result);

        // 运行块布局优化
        let mut layout_pass = BlockLayoutPass::new();
        let result = layout_pass.run_on_function(&mut function, &mut analyses);

        assert!(matches!(result, PassResult::Changed));

        // 验证指令数减少（移除了 2 个标签 + 2 个跳转 = 4 条指令）
        assert_eq!(function.instructions.len(), original_count - 4);

        // 验证只剩下一个标签（Block A 的标签）
        let labels: Vec<LabelId> = function
            .instructions
            .iter()
            .filter_map(|inst| {
                if let Instruction::Label { id, .. } = inst {
                    Some(*id)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(labels, vec![LabelId(1)]);

        // 验证没有跳转指令了
        let jumps: Vec<&Instruction> = function
            .instructions
            .iter()
            .filter(|inst| matches!(inst, Instruction::Jump { .. }))
            .collect();
        assert!(jumps.is_empty());
    }

    #[test]
    fn test_block_merge_with_branch() {
        // 创建带条件分支的情况：
        //   A (条件跳转)
        //  / \
        // B   C
        //  \ /
        //   D (有多个前驱，不能被合并)
        let mut function = LirFunction::new("test_branch".to_string());
        function.instructions = vec![
            // Block A
            Instruction::Label {
                id: LabelId(1),
                span: Span::dummy(),
            },
            Instruction::Move {
                dst: Register::Virtual(1),
                src: Operand::Immediate { value: 10 },
                span: Span::dummy(),
            },
            Instruction::Compare {
                src1: Operand::Register {
                    id: Register::Virtual(1),
                },
                src2: Operand::Immediate { value: 10 },
                span: Span::dummy(),
            },
            Instruction::JumpEqual {
                target: LabelId(3),
                span: Span::dummy(),
            },
            // Block B (fall-through from A)
            Instruction::Label {
                id: LabelId(2),
                span: Span::dummy(),
            },
            Instruction::Move {
                dst: Register::Virtual(2),
                src: Operand::Immediate { value: 1 },
                span: Span::dummy(),
            },
            Instruction::Jump {
                target: LabelId(4),
                span: Span::dummy(),
            },
            // Block C (jump target from A)
            Instruction::Label {
                id: LabelId(3),
                span: Span::dummy(),
            },
            Instruction::Move {
                dst: Register::Virtual(2),
                src: Operand::Immediate { value: 2 },
                span: Span::dummy(),
            },
            Instruction::Jump {
                target: LabelId(4),
                span: Span::dummy(),
            },
            // Block D (有两个前驱 B 和 C，不能被合并)
            Instruction::Label {
                id: LabelId(4),
                span: Span::dummy(),
            },
            Instruction::Return {
                value: Some(Register::Virtual(2)),
                span: Span::dummy(),
            },
        ];

        // 运行CFG分析
        let mut analyses = AnalysisManager::new();
        let mut cfg_pass = ControlFlowAnalysis::new();
        let cfg_result = cfg_pass
            .analyze_function(&function, &analyses)
            .expect("CFG analysis failed");
        analyses.store_result("cfg".to_string(), cfg_result);

        // 运行块布局优化
        let mut layout_pass = BlockLayoutPass::new();
        let _result = layout_pass.run_on_function(&mut function, &mut analyses);

        // 验证 Block D 的标签仍然存在（因为有多个前驱）
        let labels: Vec<LabelId> = function
            .instructions
            .iter()
            .filter_map(|inst| {
                if let Instruction::Label { id, .. } = inst {
                    Some(*id)
                } else {
                    None
                }
            })
            .collect();

        // D (LabelId(4)) 必须保留，因为它有多个前驱
        assert!(labels.contains(&LabelId(4)));
    }

    #[test]
    fn test_block_merge_chain() {
        // 测试合并链：A -> B -> C -> D（四个连续块）
        let mut function = LirFunction::new("test_chain".to_string());
        function.instructions = vec![
            // Block A
            Instruction::Label {
                id: LabelId(1),
                span: Span::dummy(),
            },
            Instruction::Move {
                dst: Register::Virtual(1),
                src: Operand::Immediate { value: 1 },
                span: Span::dummy(),
            },
            Instruction::Jump {
                target: LabelId(2),
                span: Span::dummy(),
            },
            // Block B
            Instruction::Label {
                id: LabelId(2),
                span: Span::dummy(),
            },
            Instruction::Move {
                dst: Register::Virtual(2),
                src: Operand::Immediate { value: 2 },
                span: Span::dummy(),
            },
            Instruction::Jump {
                target: LabelId(3),
                span: Span::dummy(),
            },
            // Block C
            Instruction::Label {
                id: LabelId(3),
                span: Span::dummy(),
            },
            Instruction::Move {
                dst: Register::Virtual(3),
                src: Operand::Immediate { value: 3 },
                span: Span::dummy(),
            },
            Instruction::Jump {
                target: LabelId(4),
                span: Span::dummy(),
            },
            // Block D
            Instruction::Label {
                id: LabelId(4),
                span: Span::dummy(),
            },
            Instruction::Return {
                value: Some(Register::Virtual(3)),
                span: Span::dummy(),
            },
        ];

        let original_count = function.instructions.len();

        // 运行CFG分析
        let mut analyses = AnalysisManager::new();
        let mut cfg_pass = ControlFlowAnalysis::new();
        let cfg_result = cfg_pass
            .analyze_function(&function, &analyses)
            .expect("CFG analysis failed");
        analyses.store_result("cfg".to_string(), cfg_result);

        // 运行块布局优化
        let mut layout_pass = BlockLayoutPass::new();
        let result = layout_pass.run_on_function(&mut function, &mut analyses);

        assert!(matches!(result, PassResult::Changed));

        // 验证：4 个块合并成 1 个
        // 移除了 3 个标签 + 3 个跳转 = 6 条指令
        assert_eq!(function.instructions.len(), original_count - 6);

        // 只剩下第一个块的标签
        let labels: Vec<LabelId> = function
            .instructions
            .iter()
            .filter_map(|inst| {
                if let Instruction::Label { id, .. } = inst {
                    Some(*id)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(labels, vec![LabelId(1)]);
    }
}
