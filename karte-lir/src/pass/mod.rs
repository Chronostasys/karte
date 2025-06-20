pub mod pass_manager;
pub mod analysis;
pub mod transformation;
pub mod memory2reg;
pub mod register_allocation;
pub mod ssa_construction;
pub mod phi_elimination;
pub mod stack_frame_lowering;
pub mod instruction_transformer;

pub use pass_manager::*;
pub use analysis::*;
pub use transformation::*;
pub use memory2reg::*;
pub use register_allocation::*;
pub use stack_frame_lowering::*;
pub use instruction_transformer::*;

use crate::{LirProgram, LirFunction};
use std::collections::HashMap;
use std::any::Any;

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
    
    /// 执行 Pass
    fn run_on_function(&mut self, function: &mut LirFunction, analyses: &mut AnalysisManager) -> PassResult;
    
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
    
    /// 执行 Pass
    fn run_on_program(&mut self, program: &mut LirProgram, analyses: &mut AnalysisManager) -> PassResult;
    
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
    
    /// 在函数上运行分析
    fn analyze_function(&mut self, function: &LirFunction, analyses: &AnalysisManager) -> Result<Box<dyn AnalysisResult>, String>;
    
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
}

impl AnalysisManager {
    pub fn new() -> Self {
        Self {
            results: HashMap::new(),
        }
    }
    
    /// 存储分析结果
    pub fn store_result(&mut self, name: String, result: Box<dyn AnalysisResult>) {
        self.results.insert(name, result);
    }
    
    /// 获取分析结果
    pub fn get_result<T: AnalysisResult + 'static>(&self, name: &str) -> Option<&T> {
        self.results.get(name)
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