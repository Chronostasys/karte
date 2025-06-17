//! LIR (Low-Level Intermediate Representation) 解释器
//!
//! 这个模块提供了一个专业的LIR解释器实现，具有以下特性：
//! - 固定32个通用寄存器的架构
//! - 线性扫描寄存器分配算法
//! - 模块化的虚拟机设计
//! - 完整的内存管理
//! - 专业的指令执行引擎

use crate::vm::{InstructionExecutor, NUM_REGISTERS, ProfessionalVMManager, ProfessionalExecutor};
use crate::vm;
use karte_lir::LirProgram;

/// 执行LIR程序的主要接口
/// 
/// # 参数
/// - `program`: 要执行的LIR程序
/// - `debug`: 是否启用调试输出
/// 
/// # 返回值
/// 返回程序的执行结果，通常是main函数的返回值
/// 
/// # 错误
/// 如果程序执行过程中发生错误，返回错误信息
pub fn execute(program: &LirProgram) -> Result<i64, String> {
    execute_professional(program, true)
}

/// 执行LIR程序并启用调试模式
/// 
/// # 参数
/// - `program`: 要执行的LIR程序
/// - `debug`: 是否启用调试输出
/// 
/// # 返回值
/// 返回程序的执行结果，通常是main函数的返回值
/// 
/// # 错误
/// 如果程序执行过程中发生错误，返回错误信息
pub fn execute_with_debug(program: &LirProgram, debug: bool) -> Result<i64, String> {
    let mut executor = InstructionExecutor::new(debug);
    
    if debug {
        println!("=== 开始执行LIR程序 ===");
        println!("虚拟机配置:");
        println!("  - 通用寄存器数量: {}", NUM_REGISTERS);
        println!("  - 内存大小: {} bytes", vm::MEMORY_SIZE);
        println!("  - 栈大小: {} entries", vm::STACK_SIZE);
        println!();
    }
    
    let result = executor.execute_program(program)?;
    
    if debug {
        println!("=== 程序执行完成 ===");
        println!("返回值: {}", result);
        println!();
        
        // 打印最终的虚拟机状态
        executor.get_vm().print_state();
        executor.get_memory().print_memory_state();
    }
    
    Ok(result)
}

/// 使用专业执行器执行LIR程序
/// 
/// 这个函数使用新的 ProfessionalExecutor，提供：
/// - 更好的寄存器分配
/// - 专业的栈管理
/// - 规范的调用约定
/// 
/// # 参数
/// - `program`: 要执行的LIR程序
/// - `debug`: 是否启用调试输出
/// 
/// # 返回值
/// 返回程序的执行结果，通常是main函数的返回值
/// 
/// # 错误
/// 如果程序执行过程中发生错误，返回错误信息
pub fn execute_professional(program: &LirProgram, debug: bool) -> Result<i64, String> {
    let mut executor = ProfessionalExecutor::new(debug);
    
    if debug {
        println!("=== 使用专业执行器执行LIR程序 ===");
        println!("专业虚拟机配置:");
        println!("  - 通用寄存器数量: {}", NUM_REGISTERS);
        println!("  - 内存大小: {} bytes", vm::MEMORY_SIZE);
        println!("  - 栈大小: {} entries", vm::STACK_SIZE);
        println!("  - 调用约定: System V ABI inspired");
        println!("  - 寄存器分配: Linear Scan with Spilling");
        println!();
    }
    
    let result = executor.execute_program(program)?;
    
    if debug {
        println!("=== 专业执行器程序执行完成 ===");
        println!("返回值: {}", result);
        println!();
        
        // 打印最终的虚拟机状态
        executor.get_vm().print_state();
        executor.get_memory().print_memory_state();
        executor.get_stack_manager().print_state();
    }
    
    Ok(result)
}

/// 使用专业虚拟机管理器执行LIR程序
/// 
/// 这提供了更高级的接口，包装了专业执行器
pub fn execute_with_professional_vm(program: &LirProgram, debug: bool) -> Result<i64, String> {
    let mut vm_manager = ProfessionalVMManager::new(debug)?;
    vm_manager.execute_program(program)
}

/// 分析程序的寄存器使用情况
/// 
/// 这个函数可以用来分析程序的寄存器压力，帮助优化寄存器分配
pub fn analyze_register_usage(program: &LirProgram) -> Result<RegisterUsageAnalysis, String> {
    let mut total_virtual_registers = 0;
    let mut max_register_pressure = 0;
    let mut function_stats = Vec::new();
    
    for (func_name, function) in &program.functions {
        let mut allocator = vm::register_allocator::RegisterAllocator::new();
        allocator.analyze_lifetimes(function);
        
        let stats = allocator.get_allocation_stats();
        total_virtual_registers += stats.total_virtual_registers;
        max_register_pressure = max_register_pressure.max(stats.register_pressure);
        
        function_stats.push(FunctionRegisterStats {
            function_name: func_name.clone(),
            virtual_registers: stats.total_virtual_registers,
            register_pressure: stats.register_pressure,
            can_allocate: stats.register_pressure <= NUM_REGISTERS,
        });
    }
    
    Ok(RegisterUsageAnalysis {
        total_virtual_registers,
        max_register_pressure,
        available_physical_registers: NUM_REGISTERS,
        function_stats,
        can_allocate_all: max_register_pressure <= NUM_REGISTERS,
    })
}

/// 寄存器使用分析结果
#[derive(Debug, Clone)]
pub struct RegisterUsageAnalysis {
    /// 程序中虚拟寄存器的总数
    pub total_virtual_registers: usize,
    /// 最大寄存器压力
    pub max_register_pressure: usize,
    /// 可用的物理寄存器数量
    pub available_physical_registers: usize,
    /// 各函数的寄存器统计
    pub function_stats: Vec<FunctionRegisterStats>,
    /// 是否可以为所有寄存器分配物理寄存器
    pub can_allocate_all: bool,
}

/// 单个函数的寄存器统计
#[derive(Debug, Clone)]
pub struct FunctionRegisterStats {
    /// 函数名称
    pub function_name: String,
    /// 虚拟寄存器数量
    pub virtual_registers: usize,
    /// 寄存器压力
    pub register_pressure: usize,
    /// 是否可以分配
    pub can_allocate: bool,
}

impl RegisterUsageAnalysis {
    /// 打印分析结果
    pub fn print_analysis(&self) {
        println!("=== 寄存器使用分析 ===");
        println!("总虚拟寄存器数量: {}", self.total_virtual_registers);
        println!("最大寄存器压力: {}", self.max_register_pressure);
        println!("可用物理寄存器: {}", self.available_physical_registers);
        println!("可以分配所有寄存器: {}", if self.can_allocate_all { "是" } else { "否" });
        println!();
        
        println!("各函数统计:");
        for stats in &self.function_stats {
            println!("  函数 '{}': {} 虚拟寄存器, 压力 {}, 可分配: {}", 
                stats.function_name, 
                stats.virtual_registers, 
                stats.register_pressure,
                if stats.can_allocate { "是" } else { "否" }
            );
        }
        
        if !self.can_allocate_all {
            println!();
            println!("警告: 某些函数的寄存器压力超过了可用的物理寄存器数量!");
            println!("可能需要实现寄存器溢出(spilling)功能。");
        }
    }
}

/// 验证程序是否可以在当前虚拟机上执行
pub fn validate_program(program: &LirProgram) -> Result<(), String> {
    // 检查是否有主函数
    if program.main_function.is_none() {
        return Err("程序没有定义主函数".to_string());
    }
    
    let main_fn_name = program.main_function.as_ref().unwrap();
    if !program.functions.contains_key(main_fn_name) {
        return Err(format!("找不到主函数: {}", main_fn_name));
    }
    
    // 分析寄存器使用情况
    let analysis = analyze_register_usage(program)?;
    if !analysis.can_allocate_all {
        return Err("程序的寄存器压力超过了虚拟机的能力".to_string());
    }
    
    // 验证各函数的结构
    for (func_name, function) in &program.functions {
        if function.instructions.is_empty() {
            return Err(format!("函数 '{}' 没有指令", func_name));
        }
        
        // 检查是否有入口标签
        let has_entry_label = function.instructions.iter().any(|instr| {
            matches!(instr, karte_lir::Instruction::Label { .. })
        });
        
        if !has_entry_label {
            return Err(format!("函数 '{}' 没有入口标签", func_name));
        }
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use karte_lir::*;
    use karte_diagnostics::Span;
    
    /// 创建一个简单的测试程序
    fn create_test_program() -> LirProgram {
        let mut program = LirProgram::new();
        let mut main_fn = LirFunction::new("main".to_string());
        
        // 创建一个简单的程序：计算 2 + 3
        let r1 = main_fn.new_register();
        let r2 = main_fn.new_register();
        let r3 = main_fn.new_register();
        let entry_label = main_fn.new_label();
        
        main_fn.add_instruction(Instruction::Label { 
            id: entry_label, 
            span: Span::dummy() 
        });
        main_fn.add_instruction(Instruction::Move { 
            dst: r1, 
            src: Operand::Immediate { value: 2 }, 
            span: Span::dummy() 
        });
        main_fn.add_instruction(Instruction::Move { 
            dst: r2, 
            src: Operand::Immediate { value: 3 }, 
            span: Span::dummy() 
        });
        main_fn.add_instruction(Instruction::Add { 
            dst: r3, 
            src1: Operand::Register { id: r1 }, 
            src2: Operand::Register { id: r2 }, 
            span: Span::dummy() 
        });
        main_fn.add_instruction(Instruction::Return { 
            value: Some(r3), 
            span: Span::dummy() 
        });
        
        program.add_function(main_fn);
        program.set_main("main".to_string());
        program
    }
    
    #[test]
    fn test_simple_execution() {
        let program = create_test_program();
        let result = execute(&program).expect("程序执行失败");
        assert_eq!(result, 5);
    }
    
    #[test]
    fn test_program_validation() {
        let program = create_test_program();
        validate_program(&program).expect("程序验证失败");
    }
    
    #[test]
    fn test_register_analysis() {
        let program = create_test_program();
        let analysis = analyze_register_usage(&program).expect("寄存器分析失败");
        
        assert!(analysis.can_allocate_all);
        assert_eq!(analysis.function_stats.len(), 1);
        assert_eq!(analysis.function_stats[0].function_name, "main");
    }
} 