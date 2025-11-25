//! 执行引擎
//!
//! 负责管理虚拟机状态、内存和栈，提供基本的执行环境

use super::{HeapAllocator, ProgramManager};
use crate::vm::{CallingConvention, ComparisonFlags, MemoryManager, StackManager, VirtualMachine};
use karte_lir::{LirProgram, Operand, Register};
use log::{info, warn};
use std::collections::HashMap;

// JIT相关imports
use super::jit::{AArch64Compiler, JitCompiler, JitMemoryManager, X86Compiler};

/// 调用栈帧
#[derive(Debug, Clone)]
pub struct CallFrame {
    /// 返回地址（程序计数器）
    pub return_pc: usize,
    /// 结果寄存器（可选）
    pub result_register: Option<Register>,
}

/// 执行引擎
///
/// 管理虚拟机的核心状态，包括寄存器、内存和栈
#[derive(Debug)]
pub struct ExecutionEngine {
    /// 虚拟机状态
    vm: VirtualMachine,
    /// 内存管理器
    memory: MemoryManager,
    /// 栈管理器
    stack_manager: StackManager,
    /// 调用约定
    calling_convention: CallingConvention,
    /// 调试模式
    debug_mode: bool,
    /// 调用栈
    call_stack: Vec<CallFrame>,
    /// 堆分配器
    heap_allocator: HeapAllocator,
    /// 虚拟栈（用于JIT执行）
    virtual_stack: Vec<i64>,
}

impl ExecutionEngine {
    /// 创建新的执行引擎
    pub fn new(debug_mode: bool) -> Self {
        let calling_convention = CallingConvention::standard();
        let stack_base = 1024 * 1024; // 1MB 栈基址

        // 堆从内存的前512KB开始
        let heap_start = 0x1000; // 4KB开始，避免NULL指针区域
        let heap_size = 512 * 1024; // 512KB堆空间

        Self {
            vm: VirtualMachine::new(),
            memory: MemoryManager::new(),
            stack_manager: StackManager::new(calling_convention.clone(), stack_base as i64),
            calling_convention,
            debug_mode,
            call_stack: Vec::new(),
            heap_allocator: HeapAllocator::new(heap_start, heap_size),
            virtual_stack: vec![0; 8192], // 64KB虚拟栈空间 (8192 * 8字节)
        }
    }

    /// 初始化执行环境
    pub fn initialize(&mut self, _program_manager: &ProgramManager) -> Result<(), String> {
        // 重置虚拟机状态
        self.vm.reset();
        self.memory.reset();

        // 初始化栈指针和帧指针（使用物理寄存器）
        // 栈从内存的高地址向低地址增长
        // 使用更安全的栈基址：从内存中间开始，留出足够的栈空间
        let stack_base = (super::super::MEMORY_SIZE / 2) as i64; // 使用内存中间作为栈基址
        self.vm
            .set_physical_register(self.calling_convention.stack_pointer, stack_base)?;
        self.vm
            .set_physical_register(self.calling_convention.frame_pointer, stack_base)?;
        self.vm
            .set_physical_register(self.calling_convention.return_address, 0)?;

        // 🔧 修复：现在寄存器分配已经在LIR Pass中完成，execution engine不需要做任何映射
        // 所有LIR指令中的寄存器ID现在直接对应物理寄存器ID
        // 建立1:1的映射关系
        for reg_id in 0..8 {
            self.vm
                .register_mapping
                .insert(karte_lir::Register::Physical(reg_id), reg_id);
        }

        if self.debug_mode {
            println!("执行引擎初始化完成");
            println!("  栈基址: {}", stack_base);
            println!("  内存大小: {} bytes", super::super::MEMORY_SIZE);
            println!("  调用约定: {:?}", self.calling_convention);
            println!("  寄存器映射: 1:1 直接映射 (RegisterId(i) -> r[i])");
            println!(
                "  物理寄存器r{}(SP): {}",
                self.calling_convention.stack_pointer, stack_base
            );
            println!(
                "  物理寄存器r{}(FP): {}",
                self.calling_convention.frame_pointer, stack_base
            );
        }

        Ok(())
    }

    /// 获取程序计数器
    pub fn get_pc(&self) -> usize {
        self.vm.pc
    }

    /// 设置程序计数器
    pub fn set_pc(&mut self, pc: usize) {
        self.vm.pc = pc;
    }

    /// 前进程序计数器
    pub fn advance_pc(&mut self) {
        self.vm.pc += 1;
    }

    /// 设置虚拟寄存器的值
    pub fn set_register(&mut self, reg: &Register, value: i64) -> Result<(), String> {
        if self.debug_mode {
            println!(
                "设置寄存器 {:?} = {}, 映射状态: {:?}",
                reg,
                value,
                self.vm.register_mapping.get(reg)
            );
        }
        self.vm.set_virtual_register(reg, value)
    }

    /// 获取虚拟寄存器的值
    pub fn get_register(&self, reg: &Register) -> Result<i64, String> {
        let value = self.vm.get_virtual_register(reg)?;
        if self.debug_mode {
            println!(
                "获取寄存器 {:?} = {}, 映射状态: {:?}",
                reg,
                value,
                self.vm.register_mapping.get(reg)
            );
        }
        Ok(value)
    }

    /// 获取操作数的值
    pub fn get_operand_value(&self, operand: &Operand) -> Result<i64, String> {
        match operand {
            Operand::Register { id } => self.get_register(id),
            Operand::Immediate { value } => Ok(*value),
            Operand::Memory { base, offset } => {
                let base_addr = self.get_register(base)?;
                let addr = (base_addr + offset) as usize;
                if addr < self.vm.memory.len() {
                    Ok(self.vm.memory[addr])
                } else {
                    Err("Memory access out of bounds".to_string())
                }
            }
            Operand::Label { id } => Ok(id.0 as i64),
            _ => Err(format!("Unsupported operand type: {:?}", operand)),
        }
    }

    /// 比较两个值并设置标志位
    pub fn compare(&mut self, val1: i64, val2: i64) {
        self.vm.compare(val1, val2);
    }

    /// 检查比较条件
    pub fn check_equal(&self) -> bool {
        self.vm.flags == ComparisonFlags::Equal
    }

    /// 检查小于条件
    pub fn check_less(&self) -> bool {
        self.vm.flags == ComparisonFlags::Less
    }

    /// 检查大于条件
    pub fn check_greater(&self) -> bool {
        self.vm.flags == ComparisonFlags::Greater
    }

    /// 检查小于等于条件
    pub fn check_less_equal(&self) -> bool {
        self.vm.flags == ComparisonFlags::Less || self.vm.flags == ComparisonFlags::Equal
    }

    /// 检查大于等于条件
    pub fn check_greater_equal(&self) -> bool {
        self.vm.flags == ComparisonFlags::Greater || self.vm.flags == ComparisonFlags::Equal
    }

    /// 设置返回值寄存器
    pub fn set_return_value(&mut self, value: i64) -> Result<(), String> {
        self.vm
            .set_physical_register(self.calling_convention.return_register, value)
    }

    /// 获取返回值寄存器的值
    pub fn get_return_value(&self) -> Result<i64, String> {
        self.vm
            .get_physical_register(self.calling_convention.return_register)
    }

    /// 存储值到内存
    pub fn store_memory(&mut self, addr: usize, value: i64) -> Result<(), String> {
        if addr < self.vm.memory.len() {
            self.vm.memory[addr] = value;
            Ok(())
        } else {
            Err("Memory store out of bounds".to_string())
        }
    }

    /// 从内存加载值
    pub fn load_memory(&self, addr: usize) -> Result<i64, String> {
        if addr < self.vm.memory.len() {
            Ok(self.vm.memory[addr])
        } else {
            Err("Memory load out of bounds".to_string())
        }
    }

    /// 获取虚拟机引用（用于调试）
    pub fn get_vm(&self) -> &VirtualMachine {
        &self.vm
    }

    /// 获取内存管理器引用
    pub fn get_memory(&self) -> &MemoryManager {
        &self.memory
    }

    /// 获取栈管理器引用
    pub fn get_stack_manager(&self) -> &StackManager {
        &self.stack_manager
    }

    /// 获取调用约定
    pub fn get_calling_convention(&self) -> &CallingConvention {
        &self.calling_convention
    }

    /// 推送调用栈帧
    pub fn push_call_frame(
        &mut self,
        return_pc: usize,
        result_register: Option<Register>,
    ) -> Result<(), String> {
        let frame = CallFrame {
            return_pc,
            result_register,
        };

        self.call_stack.push(frame);

        if self.debug_mode {
            println!(
                "推送调用栈帧: 返回PC={}, 结果寄存器={:?}",
                return_pc, result_register
            );
        }

        Ok(())
    }

    /// 弹出调用栈帧
    pub fn pop_call_frame(&mut self) -> Result<Option<CallFrame>, String> {
        let frame = self.call_stack.pop();

        if self.debug_mode {
            if let Some(ref f) = frame {
                println!(
                    "弹出调用栈帧: 返回PC={}, 结果寄存器={:?}",
                    f.return_pc, f.result_register
                );
            } else {
                println!("调用栈为空，无法弹出栈帧");
            }
        }

        Ok(frame)
    }

    /// 分配闭包环境
    pub fn allocate_closure_env(&mut self, field_count: usize) -> Result<i64, String> {
        let addr = self.heap_allocator.allocate_closure_env(field_count)?;
        Ok(addr as i64)
    }

    /// 分配堆内存
    pub fn allocate_heap(&mut self, size: usize, object_type: &str) -> Result<i64, String> {
        let addr = match object_type {
            "closure_env" => {
                // 计算字段数量（每个字段8字节，减去8字节头部）
                let field_count = if size > 8 { (size - 8) / 8 } else { 0 };
                self.heap_allocator.allocate_closure_env(field_count)?
            }
            "struct" => {
                let field_count = if size > 8 { (size - 8) / 8 } else { 0 };
                self.heap_allocator
                    .allocate_struct(object_type.to_string(), field_count, size)?
            }
            _ => self.heap_allocator.allocate_raw(size)?,
        };
        Ok(addr as i64)
    }

    /// 存储值到堆地址
    pub fn store_heap(&mut self, address: i64, offset: usize, value: i64) -> Result<(), String> {
        let heap_addr = address as usize + offset;
        if heap_addr < self.vm.memory.len() {
            self.vm.memory[heap_addr] = value;
            Ok(())
        } else {
            Err(format!("堆地址越界: 0x{:x}", heap_addr))
        }
    }

    /// 从堆地址加载值
    pub fn load_heap(&self, address: i64, offset: usize) -> Result<i64, String> {
        let heap_addr = address as usize + offset;
        if heap_addr < self.vm.memory.len() {
            Ok(self.vm.memory[heap_addr])
        } else {
            Err(format!("堆地址越界: 0x{:x}", heap_addr))
        }
    }

    /// 获取堆分配器的引用
    pub fn get_heap_allocator(&self) -> &HeapAllocator {
        &self.heap_allocator
    }

    /// 获取堆分配器的可变引用
    pub fn get_heap_allocator_mut(&mut self) -> &mut HeapAllocator {
        &mut self.heap_allocator
    }

    /// 打印执行状态（调试用）
    pub fn print_state(&self) {
        if self.debug_mode {
            println!("=== 执行引擎状态 ===");
            println!("PC: {}", self.vm.pc);
            println!("调用栈深度: {}", self.call_stack.len());
            self.vm.print_state();
            self.memory.print_memory_state();
            self.stack_manager.print_state();

            println!("堆分配统计:");
            let heap_stats = self.heap_allocator.get_allocation_stats();
            println!("  分配对象数: {}", heap_stats.active_objects);
            println!("  总分配字节: {}", heap_stats.total_allocated);
            println!("  峰值使用: {}", heap_stats.peak_usage);
            println!("  堆利用率: {:.1}%", heap_stats.heap_utilization);
        }
    }

    /// 编译并执行程序（JIT版本）
    pub fn compile_and_execute_with_jit(&mut self, program: &LirProgram) -> Result<i64, String> {
        info!("专业执行器: 开始JIT编译并执行程序 (连续内存架构)");
        info!("JIT执行器: 开始编译程序 (使用连续内存架构)");

        // 🔧 新架构：使用连续内存分配
        let mut memory_manager = JitMemoryManager::new(self.debug_mode);
        memory_manager.initialize()?; // 预分配连续内存段

        let mut compiled_functions = HashMap::new();
        let mut global_label_map = HashMap::new();

        info!("JIT编译: 第一轮编译所有函数到连续内存空间");

        // 第一轮：编译所有函数到连续内存空间（不修补跳转）
        for (function_name, function) in &program.functions {
            info!("编译函数: {}", function_name);

            // 根据当前架构选择编译器
            let compiled_function = if cfg!(target_arch = "aarch64") {
                let mut compiler = AArch64Compiler::new(self.debug_mode)?;
                compiler.compile_function(function, program)?
            } else if cfg!(target_arch = "x86_64") {
                let mut compiler = X86Compiler::new(self.debug_mode)?;
                compiler.compile_function(function, program)?
            } else {
                return Err("不支持的目标架构".to_string());
            };

            // 🔧 新架构：使用连续内存分配
            let executable_memory = memory_manager
                .allocate_function_memory(function_name, compiled_function.machine_code())?;

            info!(
                "函数 '{}' 分配到连续内存: 偏移=0x{:X}, 大小={}",
                function_name,
                executable_memory.offset(),
                executable_memory.size()
            );

            // 收集全局标签地址（函数地址）
            global_label_map.insert(
                format!("func_{}", function_name),
                executable_memory.address() as *const u8,
            );

            // 收集函数内部标签地址
            for (label_name, label_offset) in compiled_function.labels.iter() {
                let label_address =
                    (executable_memory.address() as usize + label_offset) as *const u8;
                global_label_map.insert(label_name.clone(), label_address);

                if self.debug_mode {
                    log::debug!(
                        "🔧 收集标签: {} -> 地址: 0x{:016X} (函数基址: 0x{:016X} + 偏移: {})",
                        label_name,
                        label_address as usize,
                        executable_memory.address() as usize,
                        label_offset
                    );
                }
            }

            compiled_functions.insert(
                function_name.clone(),
                (compiled_function, executable_memory),
            );
        }

        info!("JIT编译: 第二轮重新编译函数并修补跳转地址");

        // 第二轮：重新编译函数并修补跳转地址
        for (function_name, function) in &program.functions {
            info!("重新编译函数并修补跳转: {}", function_name);

            // 获取已分配的内存地址
            let (old_compiled_function, executable_memory) =
                compiled_functions.get(function_name).unwrap();

            // 使用全局标签表重新编译
            let compiled_function_with_patches = if cfg!(target_arch = "aarch64") {
                let mut compiler = AArch64Compiler::new(self.debug_mode)?;
                compiler.compile_function_with_global_labels(
                    function,
                    program,
                    &global_label_map,
                )?
            } else if cfg!(target_arch = "x86_64") {
                let mut compiler = X86Compiler::new(self.debug_mode)?;
                compiler.compile_function_with_global_labels(
                    function,
                    program,
                    &global_label_map,
                )?
            } else {
                return Err("不支持的目标架构".to_string());
            };

            let src_bytes = compiled_function_with_patches.machine_code();

            // 检查修补后的代码大小是否超出预分配内存
            if src_bytes.len() > executable_memory.size() {
                // 如果超出，重新分配更大的内存块
                warn!(
                    "函数 '{}' 修补后代码大小({})超出原分配({}), 重新分配内存",
                    function_name,
                    src_bytes.len(),
                    executable_memory.size()
                );

                let new_executable_memory = memory_manager
                    .allocate_function_memory(&format!("{}_patched", function_name), src_bytes)?;

                info!(
                    "函数 '{}' 重新分配内存: 地址=0x{:016X}, 大小={}",
                    function_name,
                    new_executable_memory.address() as usize,
                    new_executable_memory.size()
                );

                // 更新编译函数映射
                compiled_functions.insert(
                    function_name.clone(),
                    (compiled_function_with_patches, new_executable_memory),
                );
            } else {
                // 将修补后的机器码写入已分配的内存
                unsafe {
                    let dest_ptr = executable_memory.address() as *mut u8;

                    // 确保内存可写
                    memory_manager.temporarily_make_writable(function_name)?;

                    // 复制修补后的机器码
                    std::ptr::copy_nonoverlapping(src_bytes.as_ptr(), dest_ptr, src_bytes.len());

                    // 恢复为可执行
                    memory_manager.make_executable_again(function_name)?;

                    info!(
                        "函数 '{}' 跳转修补完成: 地址=0x{:016X}, 大小={}",
                        function_name,
                        executable_memory.address() as usize,
                        src_bytes.len()
                    );
                }
            }
        }

        // 🔧 新架构：所有函数现在都在连续地址空间中，已完成跳转修补
        info!("JIT编译: 函数间跳转修补完成，准备执行");

        // 获取main函数并执行
        let main_function_name = program.main_function.as_deref().unwrap_or("main");
        if let Some((_, executable_memory)) = compiled_functions.get(main_function_name) {
            info!("执行main函数: 地址={:p}", executable_memory.address());

            // 设置虚拟机参数
            if self.virtual_stack.is_empty() {
                return Err("虚拟栈未初始化".to_string());
            }

            let element_size = std::mem::size_of::<i64>();
                    if self.virtual_stack.is_empty() {
                        return Err("虚拟栈未初始化".to_string());
                    }

                    let element_size = std::mem::size_of::<i64>();
                    let stack_bottom = self.virtual_stack.as_ptr() as usize;
                    let stack_top_uninitialized = stack_bottom + self.virtual_stack.len() * element_size;

            // 在进入JIT主函数前，预先压入宿主返回哨兵（0），避免回退时读取未初始化的栈空间
                    let initial_sp = stack_top_uninitialized - element_size;
            unsafe {
                        self.virtual_stack.as_mut_ptr().add(self.virtual_stack.len() - 1).write(0);
            }

            // 创建函数指针并调用
            unsafe {
                let main_fn: extern "C" fn(usize, usize) -> i64 =
                    executable_memory.as_function_ptr()?;
                let result = main_fn(initial_sp, stack_bottom);

                info!("JIT执行完成，返回值: {}", result);
                Ok(result)
            }
        } else {
            Err("未找到main函数".to_string())
        }
    }
}
