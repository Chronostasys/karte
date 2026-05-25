//! LIR (Low-Level Intermediate Representation) 解释器
//!
//! 这个模块提供了一个专业的LIR解释器实现，具有以下特性：
//! - 固定32个通用寄存器的架构
//! - 线性扫描寄存器分配算法
//! - 模块化的虚拟机设计
//! - 完整的内存管理
//! - 专业的指令执行引擎

use crate::vm;
use crate::vm::{ProfessionalExecutor, ProfessionalVMManager, NUM_REGISTERS};
use karte_lir::LirProgram;
use log::{info, warn};

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
pub fn execute(program: &LirProgram) -> crate::Result<i64> {
    execute_professional(program, false)
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
pub fn execute_with_debug(program: &LirProgram, debug: bool) -> crate::Result<i64> {
    execute_professional(program, debug)
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
pub fn execute_professional(program: &LirProgram, debug: bool) -> crate::Result<i64> {
    let mut executor = ProfessionalExecutor::new(debug)?;

    if debug {
        info!("=== 使用专业执行器执行LIR程序 ===");
        info!("专业虚拟机配置:");
        info!("  - 通用寄存器数量: {}", NUM_REGISTERS);
        info!("  - 内存大小: {} bytes", vm::MEMORY_SIZE);
        info!("  - 栈大小: {} entries", vm::STACK_SIZE);
        info!("  - 调用约定: System V ABI inspired");
        info!("  - 寄存器分配: Linear Scan with Spilling");
    }

    let result = executor.execute(program)?;

    if debug {
        info!("=== 专业执行器程序执行完成 ===");
        info!("返回值: {}", result);

        // 打印最终的虚拟机状态
        let stats = executor.get_execution_stats();
        info!("执行统计: {:?}", stats);
    }

    Ok(result)
}

/// 使用专业虚拟机管理器执行LIR程序
///
/// 这提供了更高级的接口，包装了专业执行器
pub fn execute_with_professional_vm(program: &LirProgram, debug: bool) -> crate::Result<i64> {
    let mut vm_manager = ProfessionalVMManager::new(debug)?;
    vm_manager.execute_program(program)
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
        info!("=== 寄存器使用分析 ===");
        info!("总虚拟寄存器数量: {}", self.total_virtual_registers);
        info!("最大寄存器压力: {}", self.max_register_pressure);
        info!("可用物理寄存器: {}", self.available_physical_registers);
        info!(
            "可以分配所有寄存器: {}",
            if self.can_allocate_all { "是" } else { "否" }
        );

        info!("各函数统计:");
        for stats in &self.function_stats {
            info!(
                "  函数 '{}': {} 虚拟寄存器, 压力 {}, 可分配: {}",
                stats.function_name,
                stats.virtual_registers,
                stats.register_pressure,
                if stats.can_allocate { "是" } else { "否" }
            );
        }

        if !self.can_allocate_all {
            warn!("警告: 某些函数的寄存器压力超过了可用的物理寄存器数量!");
            warn!("可能需要实现寄存器溢出(spilling)功能。");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use karte_diagnostics::Span;
    use karte_lir::*;

    /// 创建一个简单的测试程序
    fn create_test_program() -> LirProgram {
        let mut program = LirProgram::new();
        let mut main_fn = LirFunction::new("main".to_string());

        // 创建一个简单的程序：计算 2 + 3
        let r1 = main_fn.new_register().as_physical();
        let r2 = main_fn.new_register().as_physical();
        let r3 = main_fn.new_register().as_physical();
        let entry_label = main_fn.new_label();

        main_fn.add_instruction(Instruction::Label {
            id: entry_label,
            span: Span::dummy(),
        });
        main_fn.add_instruction(Instruction::Move {
            dst: r1,
            src: Operand::Immediate { value: 2 },
            span: Span::dummy(),
        });
        main_fn.add_instruction(Instruction::Move {
            dst: r2,
            src: Operand::Immediate { value: 3 },
            span: Span::dummy(),
        });
        main_fn.add_instruction(Instruction::Add {
            dst: r3,
            src1: Operand::Register { id: r1 },
            src2: Operand::Register { id: r2 },
            span: Span::dummy(),
        });
        main_fn.add_instruction(Instruction::Return {
            value: Some(r3),
            span: Span::dummy(),
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
}
