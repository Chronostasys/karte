pub mod analysis;
pub mod block_layout_pass;
pub mod callsite_live_register_pass;
pub mod copy_propagation;
pub mod memory_optimization;
pub mod effect_lowering_pass;
pub mod explicit_jump_pass;
pub mod instruction_lowering_pass;
pub mod instruction_transformer;
pub mod lifetime_analysis_pass;
pub mod memory2reg;
pub mod pass_manager;
pub mod pass_registry;
pub mod peephole_optimization_pass;
pub mod phi_elimination;
pub mod pipeline_invariants;
pub mod register_allocation;
pub mod ssa_construction;
pub mod stack_frame_layout;
pub mod transformation;
pub mod utils;

pub use analysis::*;
pub use block_layout_pass::*;
pub use callsite_live_register_pass::*;
pub use effect_lowering_pass::*;
pub use explicit_jump_pass::*;
pub use instruction_lowering_pass::*;
pub use instruction_transformer::*;
pub use lifetime_analysis_pass::*;
pub use memory2reg::*;
pub use pass_manager::*;
pub use pass_registry::{PassRegistry, PipelinePreset};
pub use peephole_optimization_pass::*;
pub use phi_elimination::*;
pub use pipeline_invariants::*;
pub use register_allocation::*;
pub use ssa_construction::*;
pub use stack_frame_layout::*;
pub use transformation::*;
pub use utils::*;

use crate::{LirFunction, LirProgram};
use std::any::Any;
use std::collections::HashMap;

/// Pass 执行结果
#[derive(Debug, Clone)]
pub enum PassResult {
    /// Pass 成功执行，程序未修改
    Unchanged,
    /// Pass 成功执行，程序被修改
    Changed,
    /// Pass 执行失败
    Failed(String),
}

/// 分析结果的通用接口
pub trait AnalysisResult: std::fmt::Debug + Send + Sync {
    fn as_any(&self) -> &dyn Any;
}

/// 函数级别的 Pass
pub trait FunctionPass: Send + Sync {
    /// Pass 名称
    fn name(&self) -> &str;

    /// Pass 描述信息
    fn description(&self) -> &str {
        "无描述"
    }

    /// 执行 Pass
    fn run_on_function(
        &mut self,
        function: &mut LirFunction,
        analyses: &mut AnalysisManager,
    ) -> PassResult;

    /// 获取需要的分析信息
    fn required_analyses(&self) -> Vec<&'static str> {
        Vec::new()
    }

    /// 获取会使无效的分析信息
    fn invalidated_analyses(&self) -> Vec<&'static str> {
        Vec::new()
    }
}

/// 程序级别的 Pass
pub trait ProgramPass: Send + Sync {
    /// Pass 名称
    fn name(&self) -> &str;

    /// Pass 描述信息
    fn description(&self) -> &str {
        "无描述"
    }

    /// 执行 Pass
    fn run_on_program(
        &mut self,
        program: &mut LirProgram,
        analyses: &mut AnalysisManager,
    ) -> PassResult;

    /// 获取需要的分析信息
    fn required_analyses(&self) -> Vec<&'static str> {
        Vec::new()
    }

    /// 获取会使无效的分析信息
    fn invalidated_analyses(&self) -> Vec<&'static str> {
        Vec::new()
    }
}

/// 分析 Pass（只读，不修改程序）
pub trait AnalysisPass: Send + Sync {
    /// Pass 名称
    fn name(&self) -> &str;

    /// Pass 描述信息
    fn description(&self) -> &str {
        "无描述"
    }

    /// 在函数上运行分析
    fn analyze_function(
        &mut self,
        function: &LirFunction,
        analyses: &AnalysisManager,
    ) -> crate::Result<Box<dyn AnalysisResult>>;

    /// 获取需要的分析信息
    fn required_analyses(&self) -> Vec<&'static str> {
        Vec::new()
    }
}

/// 分析管理器
#[derive(Debug)]
pub struct AnalysisManager {
    /// 存储分析结果
    pub results: HashMap<String, Box<dyn AnalysisResult>>,
    /// 目标架构的调用约定（由 PassManager 在运行前注入）
    calling_convention: Option<karte_common::calling_convention::CallingConvention>,
}

impl AnalysisManager {
    pub fn new() -> Self {
        Self {
            results: HashMap::new(),
            calling_convention: None,
        }
    }

    /// 存储目标架构的调用约定
    pub fn store_calling_convention(&mut self, cc: karte_common::calling_convention::CallingConvention) {
        self.calling_convention = Some(cc);
    }

    /// 获取目标架构的调用约定
    /// 如果没有设置，回退到 CallingConvention::standard()（编译主机架构）
    pub fn get_calling_convention(&self) -> karte_common::calling_convention::CallingConvention {
        self.calling_convention.clone()
            .unwrap_or_else(karte_common::calling_convention::CallingConvention::standard)
    }

    /// 存储分析结果
    pub fn store_result(&mut self, name: String, result: Box<dyn AnalysisResult>) {
        self.results.insert(name, result);
    }

    /// 获取分析结果
    pub fn get_result<T: AnalysisResult + 'static>(&self, name: &str) -> Option<&T> {
        self.results
            .get(name)
            .and_then(|result| result.as_any().downcast_ref::<T>())
    }

    /// 移除分析结果
    pub fn invalidate(&mut self, name: &str) {
        self.results.remove(name);
    }

    /// 移除多个分析结果
    pub fn invalidate_all(&mut self, names: &[&str]) {
        for name in names {
            self.invalidate(name);
        }
    }

    /// 清空所有分析结果
    pub fn clear(&mut self) {
        self.results.clear();
    }
}

impl Default for AnalysisManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Pass 执行统计信息
#[derive(Debug, Clone)]
pub struct PassStats {
    /// Pass 名称
    pub name: String,
    /// 执行时间（毫秒）
    pub execution_time_ms: u64,
    /// 执行结果
    pub result: PassResult,
    /// 处理的函数数量
    pub functions_processed: usize,
}

impl PassStats {
    pub fn new(name: String) -> Self {
        Self {
            name,
            execution_time_ms: 0,
            result: PassResult::Unchanged,
            functions_processed: 0,
        }
    }
}
