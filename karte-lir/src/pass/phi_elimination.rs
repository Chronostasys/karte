//! φ指令消除Pass
//!
//! 实现专业的φ指令降级，将SSA形式的φ指令转换为普通的mov指令。
//! 这个pass应该在Memory2Reg之后、指令降级之前运行。

use super::analysis::ControlFlowGraph;
use super::instruction_transformer::IndexInstructionTransformer;
use super::{AnalysisManager, FunctionPass, PassResult};
use crate::{Instruction, LirFunction};
use log::{debug, error, info};

/// φ指令消除Pass
#[derive(Debug)]
pub struct PhiEliminationPass;

impl Default for PhiEliminationPass {
    fn default() -> Self {
        Self::new()
    }
}

impl PhiEliminationPass {
    pub fn new() -> Self {
        Self
    }

    /// 消除φ指令
    fn eliminate_phi_instructions(
        &self,
        function: &mut LirFunction,
        cfg: &ControlFlowGraph,
    ) -> Result<(), String> {
        let mut transformer = IndexInstructionTransformer::new();

        // 扫描所有φ指令
        for (i, instruction) in function.instructions.iter().enumerate() {
            if let Instruction::Phi {
                dst,
                incoming,
                span,
            } = instruction
            {
                debug!("🔧 处理φ指令 {}: dst={:?}, incoming={:?}", i, dst, incoming);

                // 标记φ指令为需要移除
                transformer.remove(i);

                // 为每个incoming值在对应的前驱块末尾插入mov指令
                for (source_label, operand) in incoming {
                    // 使用CFG分析结果找到对应的基本块
                    if let Some(&source_block_id) = cfg.label_to_block.get(source_label) {
                        if let Some(source_block) = cfg.nodes.get(source_block_id) {
                            // 在源基本块的末尾插入mov指令
                            let insert_position = self.find_insertion_point(function, source_block);
                            let move_instruction = Instruction::Move {
                                dst: *dst,
                                src: operand.clone(),
                                span: *span,
                            };
                            transformer.insert(insert_position, move_instruction);
                            debug!(
                                "🔧 在位置 {} 插入 mov {:?}, {:?}",
                                insert_position, dst, operand
                            );
                        }
                    }
                }
            }
        }
        // 应用所有变换
        let (_, _, _, _) = transformer.apply_to_function(function);
        Ok(())
    }

    /// 找到在基本块末尾插入指令的位置
    fn find_insertion_point(
        &self,
        function: &LirFunction,
        block: &super::analysis::ControlFlowNode,
    ) -> usize {
        let (start, end) = block.instruction_range;
        let mut last_non_control = end;
        let mut control_flow_pos = end;

        // 从后向前扫描，找到最后一个非控制流指令的位置
        for i in (start..end).rev() {
            if i >= function.instructions.len() {
                continue;
            }
            match &function.instructions[i] {
                Instruction::Jump { .. }
                | Instruction::JumpEqual { .. }
                | Instruction::JumpNotEqual { .. }
                | Instruction::JumpLess { .. }
                | Instruction::JumpLessEqual { .. }
                | Instruction::JumpGreater { .. }
                | Instruction::JumpGreaterEqual { .. }
                | Instruction::Return { .. } => {
                    control_flow_pos = i;
                }
                _ => {
                    last_non_control = i + 1;
                    break;
                }
            }
        }

        // 在最后一个非控制流指令之后、控制流指令之前插入
        if last_non_control <= control_flow_pos {
            last_non_control
        } else {
            control_flow_pos
        }
    }
}

impl FunctionPass for PhiEliminationPass {
    fn name(&self) -> &str {
        "phi-elimination"
    }

    fn run_on_function(
        &mut self,
        function: &mut LirFunction,
        analyses: &mut AnalysisManager,
    ) -> PassResult {
        info!("🔧 运行φ指令消除Pass");
        // 获取CFG分析结果
        let cfg = match analyses.get_result::<ControlFlowGraph>("cfg") {
            Some(cfg) => cfg,
            None => {
                error!("❌ φ指令消除失败：没有CFG分析结果");
                return PassResult::Failed("Missing CFG analysis".to_string());
            }
        };
        match self.eliminate_phi_instructions(function, cfg) {
            Ok(()) => {
                info!("✅ φ指令消除完成");
                PassResult::Changed
            }
            Err(e) => {
                error!("❌ φ指令消除失败: {}", e);
                PassResult::Failed(e)
            }
        }
    }
    fn required_analyses(&self) -> Vec<&'static str> {
        vec!["cfg"]
    }
    fn invalidated_analyses(&self) -> Vec<&'static str> {
        vec!["def-use", "cfg"]
    }
}
