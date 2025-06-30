//! 专业虚拟机执行引擎模块
//!
//! 这个模块提供了重新设计的执行引擎，具有以下特点：
//! - 简化的执行流程，专注于正确性
//! - 与传统执行器兼容的接口
//! - 更好的模块化设计
//! - 专业的调用约定和栈管理

pub mod execution_engine;
pub mod heap_allocator;
pub mod instruction_processor;
pub mod jit;
pub mod program_manager;

pub use execution_engine::*;
pub use heap_allocator::*;
pub use instruction_processor::*;
pub use jit::*;
pub use program_manager::*;

use karte_lir::{LabelId, LirProgram};
use log::info;

/// 执行统计信息
#[derive(Debug, Clone)]
pub struct ExecutionStats {
    /// 执行的指令数
    pub instructions_executed: u64,
    /// 执行的函数数
    pub functions_executed: u64,
    /// JIT编译次数
    pub jit_compilations: u64,
    /// 执行时间（毫秒）
    pub execution_time_ms: u64,
}

/// 专业执行器
///
/// 高性能的LIR程序执行器，支持多种执行模式：
/// - 解释执行：直接解释LIR指令
/// - JIT编译：将LIR指令编译为原生机器码
/// - 混合执行：根据热点分析选择最优执行方式
#[derive(Debug)]
pub struct ProfessionalExecutor {
    /// 执行引擎
    execution_engine: ExecutionEngine,
    /// 调试模式
    debug_mode: bool,
}

impl ProfessionalExecutor {
    /// 创建新的专业执行器
    pub fn new(debug_mode: bool) -> Result<Self, String> {
        Ok(Self {
            execution_engine: ExecutionEngine::new(debug_mode),
            debug_mode,
        })
    }

    /// 创建支持JIT的专业执行器
    pub fn new_with_jit(debug_mode: bool) -> Result<Self, String> {
        Ok(Self {
            execution_engine: ExecutionEngine::new(debug_mode),
            debug_mode,
        })
    }

    /// 执行程序（解释执行）
    pub fn execute(&mut self, program: &LirProgram) -> Result<i64, String> {
        if self.debug_mode {
            info!("专业执行器: 开始解释执行程序");
        }

        // TODO: 实现解释执行逻辑
        // 当前使用JIT执行作为临时解决方案
        self.execution_engine.compile_and_execute_with_jit(program)
    }

    /// 使用JIT编译并执行程序（新架构）
    pub fn execute_with_jit(&mut self, program: &LirProgram) -> Result<i64, String> {
        if self.debug_mode {
            info!("专业执行器: 开始JIT编译并执行程序 (连续内存架构)");
        }

        self.execution_engine.compile_and_execute_with_jit(program)
    }

    /// 获取执行统计信息
    pub fn get_execution_stats(&self) -> ExecutionStats {
        ExecutionStats {
            instructions_executed: 0, // TODO: 实现统计
            functions_executed: 0,
            jit_compilations: 0,
            execution_time_ms: 0,
        }
    }

    /// 获取虚拟机引用（兼容性方法）
    pub fn get_vm(&self) -> &crate::vm::VirtualMachine {
        self.execution_engine.get_vm()
    }

    /// 获取内存管理器引用（兼容性方法）
    pub fn get_memory(&self) -> &crate::vm::MemoryManager {
        self.execution_engine.get_memory()
    }

    /// 获取栈管理器引用（兼容性方法）
    pub fn get_stack_manager(&self) -> &crate::vm::StackManager {
        self.execution_engine.get_stack_manager()
    }
}

impl Default for ProfessionalExecutor {
    fn default() -> Self {
        Self::new(false).unwrap()
    }
}

/// 指令执行结果
#[derive(Debug, Clone, PartialEq)]
pub enum InstructionResult {
    /// 继续执行下一条指令
    Continue,
    /// 跳转到指定地址
    Jump(usize),
    /// 函数返回，带返回值
    Return(i64),
    /// 程序退出，带退出码
    Exit(i64),
}

/// 主函数信息
#[derive(Debug, Clone)]
pub struct MainFunctionInfo {
    /// 函数名称
    pub name: String,
    /// 入口程序计数器
    pub entry_pc: usize,
    /// 函数标签ID
    pub entry_label: LabelId,
}
