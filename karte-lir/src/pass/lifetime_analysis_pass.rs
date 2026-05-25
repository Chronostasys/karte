//! 基于CFG的专业生命周期分析Pass
//!
//! 该Pass实现了标准的数据流分析算法来计算寄存器生命周期：
//! 1. 依赖控制流图（CFG）
//! 2. 依赖定义-使用链（DefUse）
//! 3. 依赖活跃度分析（Liveness）
//! 4. 基于活跃度信息精确计算寄存器生命周期
//! 5. 生成寄存器分配所需的生命周期和类型信息

use crate::pass::analysis::{ControlFlowGraph, DefUseChains, LivenessAnalysis};
use crate::pass::register_allocation::{LifetimeAnalyzer, RegisterLifetime, RegisterType};
use crate::pass::{AnalysisManager, AnalysisPass, AnalysisResult};
use crate::{LirFunction, Register};
use karte_common::calling_convention::CallingConvention;
use log::{debug, info};
use std::any::Any;
use std::collections::HashMap;

/// 生命周期分析结果
#[derive(Debug)]
pub struct LifetimeAnalysisResult {
    /// 寄存器生命周期信息
    pub lifetimes: Vec<RegisterLifetime>,
    /// 寄存器类型映射
    pub register_types: HashMap<Register, RegisterType>,
}

impl LifetimeAnalysisResult {
    pub fn new(
        lifetimes: Vec<RegisterLifetime>,
        register_types: HashMap<Register, RegisterType>,
    ) -> Self {
        Self {
            lifetimes,
            register_types,
        }
    }
}

impl AnalysisResult for LifetimeAnalysisResult {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// 基于CFG的生命周期分析Pass
///
/// 该Pass实现了编译器教材中的经典算法：
/// 1. CFG构建：识别基本块和控制流边
/// 2. Def-Use分析：构建定义-使用链
/// 3. 活跃变量分析：向后数据流方程求解
/// 4. 生命周期计算：基于活跃度信息精确计算区间
#[derive(Debug)]
pub struct LifetimeAnalysisPass {
    calling_convention: CallingConvention,
}

impl LifetimeAnalysisPass {
    pub fn new() -> Self {
        Self {
            calling_convention: CallingConvention::standard(),
        }
    }
}

impl Default for LifetimeAnalysisPass {
    fn default() -> Self {
        Self::new()
    }
}

impl AnalysisPass for LifetimeAnalysisPass {
    fn name(&self) -> &str {
        "lifetime-analysis"
    }

    fn description(&self) -> &str {
        "基于CFG的生命周期分析 - 使用数据流分析精确计算寄存器生命周期"
    }

    fn analyze_function(
        &mut self,
        function: &LirFunction,
        analyses: &AnalysisManager,
    ) -> crate::Result<Box<dyn AnalysisResult>> {
        info!("🔍 开始基于CFG的生命周期分析: {}", function.name);

        // 🔧 强制要求所有依赖分析，确保使用最精确的生命周期计算
        let cfg = analyses
            .get_result::<ControlFlowGraph>("cfg")
            .ok_or_else(|| {
                format!(
                    "函数 {} 的 CFG 分析结果不可用，无法进行精确的生命周期分析",
                    function.name
                )
            })?;

        let def_use = analyses
            .get_result::<DefUseChains>("def-use")
            .ok_or_else(|| {
                format!(
                    "函数 {} 的 DefUse 分析结果不可用，无法进行精确的生命周期分析",
                    function.name
                )
            })?;

        let liveness = analyses
            .get_result::<LivenessAnalysis>("liveness")
            .ok_or_else(|| {
                format!(
                    "函数 {} 的活跃度分析结果不可用，无法进行精确的生命周期分析",
                    function.name
                )
            })?;

        debug!("✅ 使用精确的活跃度分析（基于 CFG + DefUse + Liveness）");
        let analyzer = LifetimeAnalyzer::new(self.calling_convention.clone());
        let (lifetimes, register_types) =
            analyzer.analyze_with_liveness(function, cfg, def_use, liveness);

        info!(
            "✅ 生命周期分析完成: {} 个寄存器, {} 种类型",
            lifetimes.len(),
            register_types.len()
        );

        // 打印详细的生命周期信息（调试模式）
        for lifetime in &lifetimes {
            debug!(
                "  寄存器 {:?}: [{}, {}], uses={:?}, type={:?}",
                lifetime.register,
                lifetime.start,
                lifetime.end,
                lifetime.uses.len(),
                lifetime.register_type
            );
        }

        Ok(Box::new(LifetimeAnalysisResult::new(
            lifetimes,
            register_types,
        )))
    }

    fn required_analyses(&self) -> Vec<&'static str> {
        // 🔧 强制要求所有依赖，确保生命周期分析的精确度
        // 这对于后续的寄存器保存优化至关重要
        vec!["cfg", "def-use", "liveness"]
    }
}
