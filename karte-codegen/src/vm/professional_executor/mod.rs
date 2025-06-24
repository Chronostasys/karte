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
pub mod program_manager;
pub mod jit;

pub use execution_engine::*;
pub use heap_allocator::*;
pub use instruction_processor::*;
pub use program_manager::*;
pub use jit::*;

use super::{MemoryManager, StackManager, VirtualMachine};
use karte_lir::{LabelId, LirProgram};
use jit::{JitManager, TargetArchitecture};

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
    /// JIT管理器（可选）
    pub jit_manager: Option<JitManager>,
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
            jit_manager: None, // 默认不启用JIT
            debug_mode,
        }
    }

    /// 创建启用JIT的专业执行器
    pub fn new_with_jit(debug_mode: bool) -> Result<Self, String> {
        let jit_manager = JitManager::new(TargetArchitecture::default(), debug_mode)?;
        
        Ok(Self {
            execution_engine: ExecutionEngine::new(debug_mode),
            instruction_processor: InstructionProcessor::new(),
            program_manager: ProgramManager::new(),
            jit_manager: Some(jit_manager),
            debug_mode,
        })
    }

    /// 启用JIT功能
    pub fn enable_jit(&mut self) -> Result<(), String> {
        if self.jit_manager.is_none() {
            let jit_manager = JitManager::new(TargetArchitecture::default(), self.debug_mode)?;
            self.jit_manager = Some(jit_manager);
        }
        Ok(())
    }

    /// 禁用JIT功能
    pub fn disable_jit(&mut self) {
        self.jit_manager = None;
    }

    /// 检查是否启用了JIT
    pub fn is_jit_enabled(&self) -> bool {
        self.jit_manager.is_some()
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
            let instruction = self
                .program_manager
                .get_instruction_at_pc(self.execution_engine.get_pc())?;

            if self.debug_mode {
                println!(
                    "PC: {}, 执行: {}",
                    self.execution_engine.get_pc(),
                    instruction
                );
            }

            // 处理指令
            let result = self.instruction_processor.process_instruction(
                instruction,
                &mut self.execution_engine,
                &self.program_manager,
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
                            println!(
                                "返回到调用点: PC={}, 返回值={}",
                                call_frame.return_pc, value
                            );
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

    /// 使用JIT编译并执行程序（如果启用）
    pub fn execute_with_jit(&mut self, program: &LirProgram) -> Result<i64, String> {
        if let Some(ref mut jit_manager) = self.jit_manager {
            if self.debug_mode {
                println!("=== 使用JIT编译执行 ===");
            }

            // 尝试编译程序
            let compiled_program = jit_manager.compile_program(program)?;
            
            if self.debug_mode {
                let stats = jit_manager.get_code_cache().get_statistics();
                stats.print();
                
                // 🔧 新增：打印寄存器映射信息
                let compiler = jit_manager.get_compiler();
                println!("寄存器映射: {:?}", compiler.get_register_mapping());
            }

            // 🔧 新增：真正执行JIT编译后的机器码
            if let Some(main_function_name) = &compiled_program.main_function {
                if let Some(compiled_function) = compiled_program.functions.get(main_function_name) {
                    if self.debug_mode {
                        println!("找到主函数: {}", main_function_name);
                    }
                    return self.execute_compiled_function(compiled_function);
                }
            }

            // 如果没有找到主函数，回退到解释器
            if self.debug_mode {
                println!("未找到主函数，回退到解释器执行");
            }
        }

        // 回退到常规执行
        self.execute_program(program)
    }

    /// 🔧 新增：执行编译后的机器码函数
    fn execute_compiled_function(&self, compiled_function: &CompiledFunction) -> Result<i64, String> {
        if true {
            println!("=== 执行JIT编译的机器码 ===");
            println!("函数名: {}", compiled_function.name);
            println!("机器码大小: {} 字节", compiled_function.code_size());
            println!("入口点偏移: {} 字节", compiled_function.entry_point);
            
            // 打印机器码（十六进制）
            println!("机器码内容:");
            let code = compiled_function.machine_code();
            for (i, chunk) in code.chunks(16).enumerate() {
                let hex_part: String = chunk.iter()
                    .map(|b| format!("{:02x}", b))
                    .collect::<Vec<_>>()
                    .join(" ");
                let ascii_part: String = chunk.iter()
                    .map(|&b| if b.is_ascii_graphic() { b as char } else { '.' })
                    .collect();
                println!("  {:04x}: {:<48} |{}|", i * 16, hex_part, ascii_part);
            }
            println!();
        }

        // 获取机器码
        let code = compiled_function.machine_code();
        if code.is_empty() {
            return Err("编译后的机器码为空".to_string());
        }

        // 🔧 新增：安全检查
        if compiled_function.entry_point >= code.len() {
            return Err(format!(
                "入口点偏移 {} 超出机器码范围 {}",
                compiled_function.entry_point,
                code.len()
            ));
        }

        // 分配可执行内存并执行
        match unsafe { self.allocate_and_execute_code(code, compiled_function.entry_point) } {
            Ok(result) => {
                println!("JIT执行成功，返回值: {}", result);
                Ok(result)
            }
            Err(err) => {
                if self.debug_mode {
                    println!("JIT执行失败: {}，回退到解释器", err);
                }
                Err(err)
            }
        }
    }

    /// 🔧 新增：分配可执行内存并执行机器码（unsafe）
    unsafe fn allocate_and_execute_code(&self, code: &[u8], entry_point: usize) -> Result<i64, String> {
        #[cfg(target_os = "windows")]
        {
            use windows_sys::Win32::System::Memory::{
                VirtualAlloc, MEM_COMMIT, MEM_RESERVE, PAGE_READWRITE, PAGE_EXECUTE_READ, MEM_RELEASE
            };
            use windows_sys::Win32::Foundation::BOOL;

            // 分配内存
            let size = code.len();
            let addr = VirtualAlloc(
                std::ptr::null_mut(),
                size,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            );

            if addr.is_null() {
                return Err("无法分配内存".to_string());
            }

            // 拷贝机器码
            std::ptr::copy_nonoverlapping(code.as_ptr(), addr as *mut u8, size);

            // 设置内存为可执行
            let mut old_protect = 0u32;
            let result = unsafe { 
                windows_sys::Win32::System::Memory::VirtualProtect(
                    addr, 
                    size, 
                    PAGE_EXECUTE_READ, 
                    &mut old_protect
                )
            };
            if result == 0 {
                // 释放内存
                // unsafe { windows_sys::Win32::System::Memory::VirtualFree(addr, 0, MEM_RELEASE) };
                return Err("无法设置内存为可执行".to_string());
            }

            // 🔧 修复：为JIT函数提供正确的执行环境
            let result = self.execute_jit_function_with_context(addr, entry_point);

            // 释放内存
            // unsafe { windows_sys::Win32::System::Memory::VirtualFree(addr, 0, MEM_RELEASE) };

            if self.debug_mode {
                println!("JIT函数执行完成，返回值: {}", result);
            }

            Ok(result)
        }

        #[cfg(not(target_os = "windows"))]
        {
            use std::os::unix::io::AsRawFd;
            use std::fs::File;
            use std::io::Write;

            // Unix系统使用mmap
            let size = code.len();
            
            // 创建临时文件用于mmap
            let temp_file = tempfile::tempfile().map_err(|e| format!("创建临时文件失败: {}", e))?;
            temp_file.set_len(size as u64).map_err(|e| format!("设置文件大小失败: {}", e))?;
            
            // 写入机器码
            let mut file = File::from(temp_file);
            file.write_all(code).map_err(|e| format!("写入机器码失败: {}", e))?;
            file.flush().map_err(|e| format!("刷新文件失败: {}", e))?;

            // 使用mmap映射为可执行内存
            let fd = file.as_raw_fd();
            let addr = libc::mmap(
                std::ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC,
                libc::MAP_PRIVATE,
                fd,
                0,
            );

            if addr == libc::MAP_FAILED {
                return Err("mmap失败".to_string());
            }

            // 🔧 修复：为JIT函数提供正确的执行环境
            let result = self.execute_jit_function_with_context(addr, entry_point);

            // 释放内存
            libc::munmap(addr, size);

            if self.debug_mode {
                println!("JIT函数执行完成，返回值: {}", result);
            }

            Ok(result)
        }
    }

    /// 🔧 新增：为JIT函数提供正确的执行环境
    unsafe fn execute_jit_function_with_context(&self, code_addr: *mut std::ffi::c_void, entry_point: usize) -> i64 {
        // 分配虚拟栈空间
        let stack_size = 4096;
        let mut stack = vec![0u8; stack_size].into_boxed_slice();
        let stack_ptr = stack.as_mut_ptr() as u64;
        
        // 🔧 修复：正确设置虚拟栈的初始状态
        // r6应该指向栈顶（栈底+栈大小），r7应该指向栈底
        // 这样LIR的栈帧管理指令能正确工作
        let stack_top = stack_ptr + stack_size as u64;
        let stack_bottom = stack_ptr;
        
        type JitFunction = unsafe extern "C" fn(u64, u64) -> i64;
        let function_ptr = (code_addr as usize + entry_point) as *const ();
        let function: JitFunction = std::mem::transmute(function_ptr);
        
        // 调用JIT函数，传递虚拟栈指针
        // 第一个参数(r6): 栈顶地址，第二个参数(r7): 栈底地址
        let result = function(stack_top, stack_bottom);
        std::mem::forget(stack); // 避免提前释放
        result
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
