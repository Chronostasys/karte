//! JIT编译器模块
//!
//! 提供即时编译功能，支持将LIR指令编译为原生机器码
//! 设计目标：
//! 1. 模块化：支持多种目标架构（x86、ARM等）
//! 2. 缓存：编译后的代码可以重复使用
//! 3. 安全：执行生成的机器码时保证内存安全
//! 4. 性能：减少编译开销，提高执行效率

pub mod aarch64_compiler;
pub mod code_buffer;
pub mod code_cache;
pub mod compiler_trait;
pub mod execution_mode;
pub mod ffi;
pub mod memory_manager;
pub mod runtime;
pub mod x86_compiler;

pub use aarch64_compiler::*;
pub use code_buffer::*;
pub use code_cache::*;
pub use compiler_trait::*;
pub use execution_mode::*;
pub use ffi::*;
pub use memory_manager::*;
pub use runtime::*;
pub use x86_compiler::*;

use karte_lir::{LirFunction, LirProgram};
use std::collections::HashMap;

/// JIT管理器
///
/// 负责协调JIT编译和执行的高级接口
#[derive(Debug)]
pub struct JitManager {
    /// 目标架构编译器
    compiler: Box<dyn JitCompiler>,
    /// 代码缓存
    code_cache: CodeCache,
    /// JIT内存管理器
    pub memory_manager: crate::vm::professional_executor::jit::memory_manager::JitMemoryManager,
    /// 执行模式
    execution_mode: ExecutionMode,
    /// 调试模式
    debug_mode: bool,
}

impl JitManager {
    /// 创建新的JIT管理器
    pub fn new(target_arch: TargetArchitecture, debug_mode: bool) -> Result<Self, String> {
        let compiler: Box<dyn JitCompiler> = match target_arch {
            TargetArchitecture::X86_64 => Box::new(X86Compiler::new(debug_mode)?),
            TargetArchitecture::AArch64 => Box::new(AArch64Compiler::new(debug_mode)?),
            // 未来可以添加其他架构
            // TargetArchitecture::ARM64 => Box::new(ArmCompiler::new(debug_mode)?),
        };

        Ok(Self {
            compiler,
            code_cache: CodeCache::new(),
            memory_manager:
                crate::vm::professional_executor::jit::memory_manager::JitMemoryManager::new(
                    debug_mode,
                ),
            execution_mode: ExecutionMode::Hybrid, // 默认混合模式
            debug_mode: true,
        })
    }

    /// 设置执行模式
    pub fn set_execution_mode(&mut self, mode: ExecutionMode) {
        self.execution_mode = mode;
    }

    /// 编译函数
    pub fn compile_function(
        &mut self,
        function: &LirFunction,
        program: &LirProgram,
    ) -> Result<CompiledFunction, String> {
        // 检查缓存
        if let Some(cached) = self.code_cache.get(&function.name) {
            if self.debug_mode {
                log::debug!("JIT: 从缓存获取函数 '{}'", function.name);
            }
            return Ok(cached.clone());
        }

        if self.debug_mode {
            log::debug!("JIT: 编译函数 '{}'", function.name);
        }

        // 编译函数
        let compiled = self.compiler.compile_function(function, program)?;

        // 缓存结果
        self.code_cache
            .insert(function.name.clone(), compiled.clone())?;

        if self.debug_mode {
            log::debug!(
                "JIT: 函数 '{}' 编译完成，机器码大小: {} 字节",
                function.name,
                compiled.code_size()
            );
        }

        Ok(compiled)
    }

    /// 检查是否应该使用JIT执行
    pub fn should_use_jit(&self, function_name: &str) -> bool {
        match self.execution_mode {
            ExecutionMode::JitOnly => true,
            ExecutionMode::Hybrid => {
                // 简单的启发式：如果函数已经编译过，使用JIT
                self.code_cache.contains(function_name)
            }
        }
    }

    /// 获取编译器引用
    pub fn get_compiler(&self) -> &dyn JitCompiler {
        self.compiler.as_ref()
    }

    /// 获取代码缓存引用
    pub fn get_code_cache(&self) -> &CodeCache {
        &self.code_cache
    }

    /// 清理缓存
    pub fn clear_cache(&mut self) {
        self.code_cache.clear();
        if self.debug_mode {
            log::debug!("JIT: 缓存已清理");
        }
    }
}

/// 编译后的程序
#[derive(Debug, Clone)]
pub struct CompiledProgram {
    /// 编译后的函数
    pub functions: HashMap<String, CompiledFunction>,
}

/// 目标架构
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetArchitecture {
    /// x86-64架构
    X86_64,
    /// AArch64架构
    AArch64,
    // 未来可以添加其他架构
    // ARM64,
    // RISC_V,
}

impl Default for TargetArchitecture {
    fn default() -> Self {
        // 根据编译目标选择默认架构
        #[cfg(target_arch = "x86_64")]
        return TargetArchitecture::X86_64;

        #[cfg(target_arch = "aarch64")]
        return TargetArchitecture::AArch64;

        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        compile_error!("不支持的目标架构");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use karte_diagnostics::Span;
    use karte_lir::{Instruction, Operand, Register};

    /// 创建一个简单的测试函数
    fn create_test_function() -> LirFunction {
        let mut function = LirFunction::new("test_add".to_string());

        // add r0, r1, r2
        function.instructions.push(Instruction::Add {
            dst: Register::Virtual(0),
            src1: Operand::Register {
                id: Register::Virtual(1),
            },
            src2: Operand::Register {
                id: Register::Virtual(2),
            },
            span: Span::dummy(),
        });

        // return r0
        function.instructions.push(Instruction::Return {
            value: Some(Register::Virtual(0)),
            span: Span::dummy(),
        });

        function
    }

    #[test]
    fn test_jit_manager_creation() {
        let result = JitManager::new(TargetArchitecture::X86_64, true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_function_compilation() {
        let mut jit = JitManager::new(TargetArchitecture::X86_64, true).unwrap();
        let function = create_test_function();
        let program = LirProgram::new();

        let result = jit.compile_function(&function, &program);
        assert!(result.is_ok());

        let compiled = result.unwrap();
        assert_eq!(compiled.name, "test_add");
        assert!(compiled.code_size() > 0);
    }

    #[test]
    fn test_code_cache() {
        let mut jit = JitManager::new(TargetArchitecture::X86_64, false).unwrap();
        let function = create_test_function();
        let program = LirProgram::new();

        // 第一次编译
        let compiled1 = jit.compile_function(&function, &program).unwrap();

        // 第二次应该从缓存获取
        let compiled2 = jit.compile_function(&function, &program).unwrap();

        assert_eq!(compiled1.name, compiled2.name);
        assert_eq!(compiled1.code_size(), compiled2.code_size());
    }

    #[test]
    fn test_execution_modes() {
        let mut jit = JitManager::new(TargetArchitecture::X86_64, false).unwrap();

        // 测试不同执行模式
        jit.set_execution_mode(ExecutionMode::JitOnly);
        assert!(jit.should_use_jit("test_function"));

        jit.set_execution_mode(ExecutionMode::Hybrid);
        assert!(!jit.should_use_jit("test_function")); // 未编译过
    }
}
