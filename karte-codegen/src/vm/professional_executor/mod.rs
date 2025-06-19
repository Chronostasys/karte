//! 专业虚拟机执行引擎模块
//! 
//! 这个模块提供了重新设计的执行引擎，具有以下特点：
//! - 简化的执行流程，专注于正确性
//! - 与传统执行器兼容的接口
//! - 更好的模块化设计
//! - 专业的调用约定和栈管理

pub mod execution_engine;
pub mod instruction_processor;
pub mod program_manager;
pub mod heap_allocator;

pub use execution_engine::*;
pub use instruction_processor::*;
pub use program_manager::*;
pub use heap_allocator::*;

use super::{VirtualMachine, MemoryManager, CallingConvention, StackManager};
use karte_lir::{LirProgram, LabelId, Instruction};
use std::collections::HashMap;

/// 专业执行器的核心结构
/// 
/// 这个结构负责协调各个组件，提供统一的执行接口
#[derive(Debug)]
pub struct ProfessionalExecutor {
    /// 执行引擎
    pub execution_engine: ExecutionEngine,
    /// 指令处理器
    pub instruction_processor: InstructionProcessor,
    /// 程序管理器
    pub program_manager: ProgramManager,
    /// 调试模式
    pub debug_mode: bool,
}

impl ProfessionalExecutor {
    /// 创建新的专业执行器
    pub fn new(debug_mode: bool) -> Self {
        Self {
            execution_engine: ExecutionEngine::new(debug_mode),
            instruction_processor: InstructionProcessor::new(),
            program_manager: ProgramManager::new(),
            debug_mode,
        }
    }

    /// 执行LIR程序
    /// 
    /// 这是专业执行器的主要接口，提供与传统执行器兼容的功能
    pub fn execute_program(&mut self, program: &LirProgram) -> Result<i64, String> {
        if self.debug_mode {
            println!("=== 专业执行器启动 ===");
        }

        // 1. 加载程序
        self.program_manager.load_program(program)?;
        
        // 2. 初始化执行环境
        self.execution_engine.initialize(&self.program_manager)?;
        
        // 3. 执行程序
        let result = self.run_main_function()?;
        
        if self.debug_mode {
            println!("=== 专业执行器完成，结果: {} ===", result);
        }
        
        Ok(result)
    }

    /// 运行主函数
    fn run_main_function(&mut self) -> Result<i64, String> {
        // 获取主函数信息
        let main_info = self.program_manager.get_main_function_info()?;
        
        // 设置程序计数器到主函数入口
        self.execution_engine.set_pc(main_info.entry_pc);
        
        // 执行指令循环
        loop {
            // 获取当前指令
            let instruction = self.program_manager.get_instruction_at_pc(
                self.execution_engine.get_pc()
            )?;
            
            if self.debug_mode {
                println!("PC: {}, 执行: {}", self.execution_engine.get_pc(), instruction);
            }
            
            // 处理指令
            let result = self.instruction_processor.process_instruction(
                &instruction,
                &mut self.execution_engine,
                &self.program_manager
            )?;
            
            // 检查是否需要退出
            match result {
                InstructionResult::Continue => {
                    // 继续执行下一条指令
                    self.execution_engine.advance_pc();
                }
                InstructionResult::Jump(target_pc) => {
                    // 跳转到目标地址
                    self.execution_engine.set_pc(target_pc);
                }
                InstructionResult::Return(value) => {
                    // 函数返回 - 检查是否有调用栈需要恢复
                    if let Some(call_frame) = self.execution_engine.pop_call_frame()? {
                        // 恢复调用栈：返回到调用点
                        if let Some(result_reg) = call_frame.result_register {
                            // 设置返回值到结果寄存器
                            self.execution_engine.set_register(&result_reg, value)?;
                        }
                        
                        // 设置返回地址
                        self.execution_engine.set_pc(call_frame.return_pc);
                        
                        if self.debug_mode {
                            println!("返回到调用点: PC={}, 返回值={}", call_frame.return_pc, value);
                        }
                    } else {
                        // 这是主函数的返回，程序结束
                        return Ok(value);
                    }
                }
                InstructionResult::Exit(code) => {
                    // 程序退出
                    return Ok(code);
                }
            }
            
            // 防止无限循环
            if self.execution_engine.get_pc() >= self.program_manager.instruction_count() {
                break;
            }
        }
        
        // 默认返回0
        Ok(0)
    }

    /// 获取虚拟机状态（用于调试和测试）
    pub fn get_vm(&self) -> &VirtualMachine {
        self.execution_engine.get_vm()
    }

    /// 获取内存管理器
    pub fn get_memory(&self) -> &MemoryManager {
        self.execution_engine.get_memory()
    }

    /// 获取栈管理器
    pub fn get_stack_manager(&self) -> &StackManager {
        self.execution_engine.get_stack_manager()
    }
}

impl Default for ProfessionalExecutor {
    fn default() -> Self {
        Self::new(false)
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