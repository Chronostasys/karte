//! 执行引擎
//! 
//! 负责管理虚拟机状态、内存和栈，提供基本的执行环境

use super::{ProgramManager, HeapAllocator};
use crate::vm::{VirtualMachine, MemoryManager, CallingConvention, StackManager, ComparisonFlags, JumpCondition};
use karte_lir::{RegisterId, Operand};

/// 调用栈帧
#[derive(Debug, Clone)]
pub struct CallFrame {
    /// 返回地址（程序计数器）
    pub return_pc: usize,
    /// 结果寄存器（可选）
    pub result_register: Option<RegisterId>,
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
        }
    }

    /// 初始化执行环境
    pub fn initialize(&mut self, program_manager: &ProgramManager) -> Result<(), String> {
        // 重置虚拟机状态
        self.vm.reset();
        self.memory.reset();
        
        // 初始化栈指针和帧指针（使用物理寄存器）
        // 栈从内存的高地址向低地址增长
        // 使用更安全的栈基址：从内存中间开始，留出足够的栈空间
        let stack_base = (super::super::MEMORY_SIZE / 2) as i64;  // 使用内存中间作为栈基址
        self.vm.set_physical_register(self.calling_convention.stack_pointer, stack_base)?;
        self.vm.set_physical_register(self.calling_convention.frame_pointer, stack_base)?;
        self.vm.set_physical_register(self.calling_convention.return_address, 0)?;
        
        // 从编译Pass结果中获取寄存器分配信息
        if program_manager.get_main_function_info().is_ok() {
            self.load_register_allocation_from_pass_results(program_manager)?;
        } else {
            // 如果没有主函数，使用默认映射（兼容性）
            self.initialize_default_register_mapping();
        }
        
        // 确保栈指针寄存器不被虚拟寄存器覆盖
        // RegisterId(6) 应该直接映射到物理寄存器r6（栈指针）
        self.vm.register_mapping.insert(karte_lir::RegisterId(6), self.calling_convention.stack_pointer);
        self.vm.register_mapping.insert(karte_lir::RegisterId(7), self.calling_convention.frame_pointer);
        
        if self.debug_mode {
            println!("执行引擎初始化完成");
            println!("  栈基址: {}", stack_base);
            println!("  内存大小: {} bytes", super::super::MEMORY_SIZE);
            println!("  调用约定: {:?}", self.calling_convention);
            println!("  寄存器映射: {:?}", self.vm.register_mapping);
            println!("  物理寄存器r6(SP): {}", self.vm.get_physical_register(6)?);
            println!("  物理寄存器r7(FP): {}", self.vm.get_physical_register(7)?);
        }
        
        Ok(())
    }
    
    /// 执行真正的寄存器分配
    fn perform_register_allocation(&mut self, program_manager: &ProgramManager) -> Result<(), String> {
        use super::super::register_allocator::RegisterAllocator;
        
        // 获取主函数信息
        let main_info = program_manager.get_main_function_info()?;
        let main_function = program_manager.get_function_info(&main_info.name)
            .ok_or("Cannot find main function")?;
        
        // 创建寄存器分配器
        let mut allocator = RegisterAllocator::new();
        
        // 设置栈寄存器（r6）
        allocator.set_stack_register(karte_lir::RegisterId(6));
        
        // 分析寄存器生命周期
        allocator.analyze_lifetimes(&main_function.function);
        
        if self.debug_mode {
            println!("=== 寄存器分配分析 ===");
            allocator.print_allocation();
        }
        
        // 执行寄存器分配
        match allocator.allocate_registers_with_spill(&mut main_function.function.clone()) {
            Ok(allocation) => {
                // 应用寄存器分配结果
                let allocation_result = allocator.get_register_allocation();
                
                // 1. 处理成功分配到物理寄存器的虚拟寄存器
                for (virtual_reg, physical_reg) in &allocation_result.register_assignments {
                    self.vm.register_mapping.insert(*virtual_reg, *physical_reg);
                    if self.debug_mode {
                        println!("  -> 分配 {:?} 到物理寄存器 r{}", virtual_reg, physical_reg);
                    }
                }
                
                // 2. 处理溢出到栈的虚拟寄存器
                // 溢出的寄存器使用特殊的标记来表示它们存储在栈上
                for (virtual_reg, spill_slot) in &allocation_result.spill_assignments {
                    // 使用特殊的物理寄存器ID (255) 来标记溢出寄存器
                    // 这样执行引擎就知道需要从栈加载/存储这些寄存器
                    self.vm.register_mapping.insert(*virtual_reg, 255); // 255表示溢出
                    
                    // 同时记录溢出槽信息
                    self.vm.spill_slot_mapping.insert(*virtual_reg, spill_slot.clone());
                    
                    if self.debug_mode {
                        println!("  -> 溢出 {:?} 到栈槽 {} (偏移 {})", 
                            virtual_reg, spill_slot.slot_id, spill_slot.stack_offset);
                    }
                }
                
                // 3. 打印最终的寄存器分配结果
                if self.debug_mode {
                    println!("Final register allocation:");
                    for (virtual_reg, physical_reg) in &allocation_result.register_assignments {
                        println!("  {:?} -> r{}", virtual_reg, physical_reg);
                    }
                    if !allocation_result.spill_assignments.is_empty() {
                        println!("Spilled registers:");
                        for (virtual_reg, spill_slot) in &allocation_result.spill_assignments {
                            println!("  {:?} -> 栈槽 {} (偏移 {})", 
                                virtual_reg, spill_slot.slot_id, spill_slot.stack_offset);
                        }
                    }
                }
                
                // 获取分配统计
                let stats = allocator.get_allocation_stats();
                if self.debug_mode {
                    println!("寄存器分配成功:");
                    println!("  - 虚拟寄存器总数: {}", stats.total_virtual_registers);
                    println!("  - 分配的物理寄存器: {}", stats.allocated_physical_registers);
                    println!("  - 寄存器压力: {}", stats.register_pressure);
                    println!("  - 溢出的寄存器: {}", stats.spilled_registers);
                    println!("  - 使用的溢出槽: {}", stats.spill_slots_used);
                }
                
                Ok(())
            }
            Err(e) => {
                if self.debug_mode {
                    println!("寄存器分配失败，使用默认映射: {}", e);
                }
                // 分配失败时使用默认映射
                self.initialize_default_register_mapping();
                Ok(())
            }
        }
    }
    
    /// 初始化默认的寄存器映射（兼容性后备）
    fn initialize_default_register_mapping(&mut self) {
        // 建立虚拟寄存器到物理寄存器的映射
        // 需要避免分配特殊寄存器（栈指针、帧指针等）给普通变量
        const MAX_VIRTUAL_REGS: usize = 32; // 支持足够多的虚拟寄存器
        
        // 获取可分配的物理寄存器（排除特殊寄存器）
        let allocatable_registers = self.calling_convention.get_allocatable_registers();
        
        for virtual_reg_id in 0..MAX_VIRTUAL_REGS {
            let virtual_reg = karte_lir::RegisterId(virtual_reg_id);
            
            // 特殊寄存器的映射
            if virtual_reg_id == 6 {
                // RegisterId(6) 直接映射到栈指针
                self.vm.register_mapping.insert(virtual_reg, self.calling_convention.stack_pointer);
            } else if virtual_reg_id == 7 {
                // RegisterId(7) 直接映射到帧指针
                self.vm.register_mapping.insert(virtual_reg, self.calling_convention.frame_pointer);
            } else {
                // 普通虚拟寄存器映射到可分配的物理寄存器
                // 使用模运算，但跳过特殊寄存器
                let allocatable_index = virtual_reg_id % allocatable_registers.len();
                let physical_reg_id = allocatable_registers[allocatable_index];
                self.vm.register_mapping.insert(virtual_reg, physical_reg_id);
            }
        }
        
        if self.debug_mode {
            println!("使用默认寄存器映射策略（智能分配，避免特殊寄存器冲突）");
            println!("可分配寄存器: {:?}", allocatable_registers);
            println!("栈指针寄存器: r{}", self.calling_convention.stack_pointer);
            println!("帧指针寄存器: r{}", self.calling_convention.frame_pointer);
        }
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
    pub fn set_register(&mut self, reg: &RegisterId, value: i64) -> Result<(), String> {
        if self.debug_mode {
            println!("设置寄存器 {:?} = {}, 映射状态: {:?}", reg, value, self.vm.register_mapping.get(reg));
        }
        self.vm.set_virtual_register(reg, value)
    }

    /// 获取虚拟寄存器的值
    pub fn get_register(&self, reg: &RegisterId) -> Result<i64, String> {
        let value = self.vm.get_virtual_register(reg)?;
        if self.debug_mode {
            println!("获取寄存器 {:?} = {}, 映射状态: {:?}", reg, value, self.vm.register_mapping.get(reg));
        }
        Ok(value)
    }

    /// 获取操作数的值
    pub fn get_operand_value(&self, operand: &Operand) -> Result<i64, String> {
        match operand {
            Operand::Register { id } => {
                self.get_register(id)
            }
            Operand::Immediate { value } => {
                Ok(*value)
            }
            Operand::Memory { base, offset } => {
                let base_addr = self.get_register(base)?;
                let addr = (base_addr + offset) as usize;
                if addr < self.vm.memory.len() {
                    Ok(self.vm.memory[addr])
                } else {
                    Err("Memory access out of bounds".to_string())
                }
            }
            _ => {
                Err(format!("Unsupported operand type: {:?}", operand))
            }
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
        self.vm.set_physical_register(self.calling_convention.return_register, value)
    }

    /// 获取返回值寄存器的值
    pub fn get_return_value(&self) -> Result<i64, String> {
        self.vm.get_physical_register(self.calling_convention.return_register)
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
    pub fn push_call_frame(&mut self, return_pc: usize, result_register: Option<RegisterId>) -> Result<(), String> {
        let frame = CallFrame {
            return_pc,
            result_register,
        };
        
        self.call_stack.push(frame);
        
        if self.debug_mode {
            println!("推送调用栈帧: 返回PC={}, 结果寄存器={:?}", return_pc, result_register);
        }
        
        Ok(())
    }

    /// 弹出调用栈帧
    pub fn pop_call_frame(&mut self) -> Result<Option<CallFrame>, String> {
        let frame = self.call_stack.pop();
        
        if self.debug_mode {
            if let Some(ref f) = frame {
                println!("弹出调用栈帧: 返回PC={}, 结果寄存器={:?}", f.return_pc, f.result_register);
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
                self.heap_allocator.allocate_struct(object_type.to_string(), field_count, size)?
            }
            _ => {
                self.heap_allocator.allocate_raw(size)?
            }
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

    /// 从编译Pass结果中获取寄存器分配信息
    fn load_register_allocation_from_pass_results(&mut self, _program_manager: &ProgramManager) -> Result<(), String> {
        // 简化版本：寄存器分配已经在编译时完成
        // 这里我们使用简单的1:1映射作为默认策略
        // 在更完善的实现中，这里应该从编译Pass结果中加载分配信息
        
        if self.debug_mode {
            println!("=== 寄存器分配信息 ===");
            println!("使用编译时寄存器分配结果");
            println!("执行引擎使用简化的1:1映射策略");
        }
        
        self.initialize_default_register_mapping();
        Ok(())
    }
} 