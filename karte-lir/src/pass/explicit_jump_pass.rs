//! 显式跳转Pass
//!
//! 该Pass将所有隐式的fall-through跳转转换为显式的Jump指令。
//!
//! ## 目标
//! 在基本块重排之前，确保所有控制流都是显式的，避免重排后隐式跳转指向错误的块。
//!
//! ## 算法
//! 1. 遍历CFG中的每个基本块
//! 2. 检查块的最后一条指令是否是terminator（Jump/Branch/Return/Switch）
//! 3. 如果不是terminator且有successor，插入显式Jump指令
//!
//! ## 执行时机
//! 必须在BlockLayoutPass之前运行，确保重排不会破坏隐式控制流。

use super::{AnalysisManager, FunctionPass, PassResult};
use crate::pass::analysis::ControlFlowGraph;
use crate::{Instruction, LabelId, LirFunction};
use log::{debug, info};

/// 显式跳转Pass
#[derive(Debug)]
pub struct ExplicitJumpPass;

impl ExplicitJumpPass {
    pub fn new() -> Self {
        Self
    }

    /// 检查指令是否是terminator
    fn is_terminator(inst: &Instruction) -> bool {
        matches!(
            inst,
            Instruction::Jump { .. }
                | Instruction::JumpEqual { .. }
                | Instruction::JumpNotEqual { .. }
                | Instruction::JumpGreater { .. }
                | Instruction::JumpGreaterEqual { .. }
                | Instruction::JumpLess { .. }
                | Instruction::JumpLessEqual { .. }
                | Instruction::Return { .. }
        )
    }

    /// 显式化所有隐式跳转
    fn make_jumps_explicit(
        &self,
        function: &LirFunction,
        cfg: &ControlFlowGraph,
    ) -> Result<Vec<Instruction>, String> {
        info!("🔄 开始显式化隐式跳转");

        let mut new_instructions = Vec::new();
        let mut modifications = 0;

        for (block_idx, node) in cfg.nodes.iter().enumerate() {
            let (start, end) = node.instruction_range;

            // 复制块中的所有指令
            for i in start..end {
                if i >= function.instructions.len() {
                    continue;
                }
                new_instructions.push(function.instructions[i].clone());
            }

            // 检查块的最后一条指令
            if end > start {
                let last_inst_idx = end - 1;
                if last_inst_idx < function.instructions.len() {
                    let last_inst = &function.instructions[last_inst_idx];

                    // 如果最后一条指令不是terminator，且有successor
                    if !Self::is_terminator(last_inst) && !node.successors.is_empty() {
                        // 找到下一个应该跳转到的块
                        // 按照原始顺序，应该跳转到第一个successor
                        let target_block_id = node.successors[0];
                        // 注意：使用 get_node_by_id 而不是 nodes[block_id]，因为 block_id 可能不等于数组索引
                        let target_node = match cfg.get_node_by_id(target_block_id) {
                            Some(node) => node,
                            None => {
                                return Err(format!(
                                    "无法找到目标块 {} 的节点信息",
                                    target_block_id
                                ));
                            }
                        };
                        let target_label_idx = target_node.instruction_range.0;

                        // 获取目标label
                        if let Instruction::Label {
                            id: target_label, ..
                        } = &function.instructions[target_label_idx]
                        {
                            debug!(
                                "📌 块 {} 添加显式跳转到块 {} (Label {:?})",
                                block_idx, target_block_id, target_label
                            );

                            // 插入显式Jump指令
                            use karte_diagnostics::Span;
                            new_instructions.push(Instruction::Jump {
                                target: *target_label,
                                span: Span::dummy(),
                            });

                            modifications += 1;
                        } else {
                            return Err(format!("块 {} 的第一条指令不是Label", target_block_id));
                        }
                    }
                }
            }
        }

        if modifications > 0 {
            info!(
                "✅ 显式跳转Pass完成: 添加了 {} 个显式Jump指令",
                modifications
            );
        } else {
            info!("✅ 显式跳转Pass完成: 所有跳转已经是显式的");
        }

        Ok(new_instructions)
    }
}

impl Default for ExplicitJumpPass {
    fn default() -> Self {
        Self::new()
    }
}

impl FunctionPass for ExplicitJumpPass {
    fn name(&self) -> &str {
        "explicit-jump"
    }

    fn description(&self) -> &str {
        "显式跳转 - 将隐式fall-through转换为显式Jump指令"
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
                return PassResult::Failed("ExplicitJumpPass 需要先运行 CFG 分析".to_string());
            }
        };

        // 执行显式化
        match self.make_jumps_explicit(function, cfg) {
            Ok(new_instructions) => {
                // 检查是否有改变
                if new_instructions.len() == function.instructions.len() {
                    debug!("所有跳转已经是显式的，无需改变");
                    return PassResult::Unchanged;
                }

                // 替换指令序列
                function.instructions = new_instructions;

                // 使无效化CFG分析（因为添加了新指令）
                analyses.clear();

                PassResult::Changed
            }
            Err(e) => PassResult::Failed(format!("显式跳转Pass失败: {}", e)),
        }
    }

    fn required_analyses(&self) -> Vec<&'static str> {
        vec!["cfg"]
    }

    fn invalidated_analyses(&self) -> Vec<&'static str> {
        // 添加了新指令，CFG需要重新分析
        vec!["cfg"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pass::analysis::ControlFlowAnalysis;
    use crate::pass::AnalysisPass;
    use crate::{Operand, Register};
    use karte_diagnostics::Span;

    #[test]
    fn test_explicit_jump_simple() {
        // 创建一个有隐式fall-through的函数
        let mut function = LirFunction::new("test".to_string());
        function.instructions = vec![
            // Block A - 没有显式跳转，会fall-through到Block B
            Instruction::Label {
                id: LabelId(1),
                span: Span::dummy(),
            },
            Instruction::Move {
                dst: Register::Virtual(1),
                src: Operand::Immediate { value: 10 },
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
        ];

        // 运行CFG分析
        let mut analyses = AnalysisManager::new();
        let mut cfg_pass = ControlFlowAnalysis::new();
        let cfg_result = cfg_pass
            .analyze_function(&function, &analyses)
            .expect("CFG analysis failed");
        analyses.store_result("cfg".to_string(), cfg_result);

        // 运行显式跳转Pass
        let mut explicit_jump_pass = ExplicitJumpPass::new();
        let result = explicit_jump_pass.run_on_function(&mut function, &mut analyses);

        assert!(matches!(result, PassResult::Changed));

        // 验证添加了Jump指令
        assert_eq!(function.instructions.len(), 5); // 原来4条，添加1条Jump

        // 检查第3条指令是Jump
        if let Instruction::Jump { target, .. } = &function.instructions[2] {
            assert_eq!(*target, LabelId(2));
        } else {
            panic!("Expected Jump instruction");
        }
    }

    #[test]
    fn test_explicit_jump_already_explicit() {
        // 创建一个已经有显式跳转的函数
        let mut function = LirFunction::new("test".to_string());
        function.instructions = vec![
            // Block A - 已有显式跳转
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
            Instruction::Return {
                value: Some(Register::Virtual(1)),
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

        // 运行显式跳转Pass
        let mut explicit_jump_pass = ExplicitJumpPass::new();
        let result = explicit_jump_pass.run_on_function(&mut function, &mut analyses);

        // 应该返回Unchanged
        assert!(matches!(result, PassResult::Unchanged));
        assert_eq!(function.instructions.len(), 5); // 没有添加新指令
    }
}
