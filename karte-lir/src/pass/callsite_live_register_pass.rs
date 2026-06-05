//! 调用位置活跃寄存器标注 Pass
//!
//! 此 Pass 负责分析所有调用指令（Call, CallIndirect, Safepoint, Alloc, Free, Retain, Release），
//! 计算每个调用位置的活跃寄存器，并将信息标注到 instruction_metadata。

use crate::pass::lifetime_analysis_pass::LifetimeAnalysisResult;
use crate::pass::{AnalysisManager, FunctionPass, PassResult};
use crate::{Instruction, InstructionMetadata, LirFunction, LiveRegisterInfo, Register};

/// 调用位置活跃寄存器标注 Pass
///
/// 功能：
/// 1. 识别所有调用指令（Call, CallIndirect, Safepoint, Alloc, Free, Retain, Release）
/// 2. 分析每个调用位置的活跃寄存器
/// 3. 将活跃寄存器信息标注到 instruction_metadata
///
/// 依赖：
/// - lifetime-analysis：需要寄存器生命周期信息
pub struct CallsiteLiveRegisterPass;

impl CallsiteLiveRegisterPass {
    pub fn new() -> Self {
        Self
    }

    /// 判断指令是否是调用指令
    fn is_call_instruction(instruction: &Instruction) -> bool {
        matches!(
            instruction,
            Instruction::Call { .. }
                | Instruction::CallIndirect { .. }
                | Instruction::Safepoint { .. }
                | Instruction::Alloc { .. }
                | Instruction::Free { .. }
                | Instruction::Retain { .. }
                | Instruction::Release { .. }
                | Instruction::StringConcat { .. }
                | Instruction::StringEqual { .. }
                | Instruction::StringCharAt { .. }
                | Instruction::StringSubstring { .. }
                | Instruction::StringContains { .. }
                | Instruction::SplitCount { .. }
                | Instruction::Trim { .. }
                | Instruction::CharToString { .. }
                | Instruction::ToString { .. }
                | Instruction::PrintString { .. }
                | Instruction::PrintNumber { .. }
                | Instruction::PrintBool { .. }
                | Instruction::Panic { .. }
        )
    }

    /// 计算指定位置的活跃寄存器
    fn compute_live_registers(
        instruction_index: usize,
        lifetime_result: &LifetimeAnalysisResult,
    ) -> LiveRegisterInfo {
        let mut live_registers = Vec::new();

        // 遍历所有寄存器生命周期
        for lifetime in &lifetime_result.lifetimes {
            // 检查寄存器是否在当前位置活跃
            if instruction_index >= lifetime.start && instruction_index <= lifetime.end {
                match lifetime.register {
                    Register::Physical(phys_reg) => {
                        live_registers.push(Register::Physical(phys_reg));
                    }
                    Register::Virtual(_) => {
                        // JIT 编译前应该已经完成寄存器分配，不应出现虚拟寄存器
                        log::warn!(
                            "⚠️  在调用指令标注时发现虚拟寄存器: {:?}",
                            lifetime.register
                        );
                    }
                }
            }
        }

        LiveRegisterInfo { live_registers }
    }
}

impl FunctionPass for CallsiteLiveRegisterPass {
    fn name(&self) -> &str {
        "callsite-live-register"
    }

    fn description(&self) -> &str {
        "标注调用位置的活跃寄存器信息"
    }

    fn required_analyses(&self) -> Vec<&'static str> {
        vec!["lifetime-analysis"]
    }

    fn invalidated_analyses(&self) -> Vec<&'static str> {
        vec![] // 不修改指令，不失效任何分析
    }

    fn run_on_function(
        &mut self,
        function: &mut LirFunction,
        analyses: &mut AnalysisManager,
    ) -> PassResult {
        log::info!("🏷️  运行调用位置活跃寄存器标注 Pass: {}", function.name);

        // 获取生命周期分析结果
        let lifetime_result =
            match analyses.get_result::<LifetimeAnalysisResult>("lifetime-analysis") {
                Some(result) => result,
                None => return PassResult::Failed("需要 lifetime-analysis 结果".to_string()),
            };

        let mut annotated_count = 0;

        // 遍历所有指令，标注调用指令
        for (index, instruction) in function.instructions.iter().enumerate() {
            if Self::is_call_instruction(instruction) {
                let live_info = Self::compute_live_registers(index, lifetime_result);

                log::debug!(
                    "🏷️  标注调用指令 [{}]: {} 个活跃寄存器",
                    index,
                    live_info.live_registers.len(),
                );

                // 插入或更新 metadata
                function
                    .instruction_metadata
                    .entry(index)
                    .or_insert_with(|| InstructionMetadata {
                        live_register_info: None,
                    })
                    .live_register_info = Some(live_info);

                annotated_count += 1;
            }
        }

        if annotated_count > 0 {
            log::info!(
                "✅ 调用位置活跃寄存器标注完成：标注了 {} 个调用指令",
                annotated_count
            );
            PassResult::Changed
        } else {
            PassResult::Unchanged
        }
    }
}

impl Default for CallsiteLiveRegisterPass {
    fn default() -> Self {
        Self::new()
    }
}
