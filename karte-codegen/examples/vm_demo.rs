//! LIR解释器优化演示程序
//! 
//! 这个程序展示了优化后的LIR解释器的功能，包括：
//! - 固定32个寄存器的架构
//! - 线性扫描寄存器分配算法
//! - 寄存器使用分析
//! - 专业的虚拟机状态管理

use karte_codegen::lir_interpreter::{execute_with_debug, analyze_register_usage, validate_program};
use karte_lir::*;
use karte_diagnostics::Span;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Karte LIR 解释器优化演示 ===\n");
    
    // 创建一个更复杂的测试程序
    let program = create_complex_program();
    
    // 1. 验证程序
    println!("1. 程序验证：");
    match validate_program(&program) {
        Ok(()) => println!("   ✅ 程序验证通过"),
        Err(e) => {
            println!("   ❌ 程序验证失败: {}", e);
            return Ok(());
        }
    }
    
    // 2. 寄存器使用分析
    println!("\n2. 寄存器使用分析：");
    let analysis = analyze_register_usage(&program)?;
    analysis.print_analysis();
    
    // 3. 执行程序（带调试信息）
    println!("\n3. 执行程序（调试模式）：");
    let result = execute_with_debug(&program, true)?;
    
    println!("\n=== 执行完成 ===");
    println!("最终结果: {}", result);
    
    Ok(())
}

/// 创建一个演示程序
/// 这个程序计算 (10 + 5) * 3 - 8 = 37
fn create_complex_program() -> LirProgram {
    let mut program = LirProgram::new();
    let mut main_fn = LirFunction::new("main".to_string());
    
    // 创建寄存器来展示寄存器分配
    let reg_10 = main_fn.new_register();      // 存储常数10
    let reg_5 = main_fn.new_register();       // 存储常数5
    let reg_3 = main_fn.new_register();       // 存储常数3
    let reg_8 = main_fn.new_register();       // 存储常数8
    let reg_temp1 = main_fn.new_register();   // 存储 10 + 5 = 15
    let reg_temp2 = main_fn.new_register();   // 存储 15 * 3 = 45
    let reg_result = main_fn.new_register();  // 存储 45 - 8 = 37
    
    // 创建标签
    let entry_label = main_fn.new_label();
    
    // 程序开始
    main_fn.add_instruction(Instruction::Label { 
        id: entry_label, 
        span: Span::dummy() 
    });
    
    // 加载常数
    main_fn.add_instruction(Instruction::Move { 
        dst: reg_10, 
        src: Operand::Immediate { value: 10 }, 
        span: Span::dummy() 
    });
    
    main_fn.add_instruction(Instruction::Move { 
        dst: reg_5, 
        src: Operand::Immediate { value: 5 }, 
        span: Span::dummy() 
    });
    
    main_fn.add_instruction(Instruction::Move { 
        dst: reg_3, 
        src: Operand::Immediate { value: 3 }, 
        span: Span::dummy() 
    });
    
    main_fn.add_instruction(Instruction::Move { 
        dst: reg_8, 
        src: Operand::Immediate { value: 8 }, 
        span: Span::dummy() 
    });
    
    // 计算 10 + 5
    main_fn.add_instruction(Instruction::Add { 
        dst: reg_temp1, 
        src1: Operand::Register { id: reg_10 }, 
        src2: Operand::Register { id: reg_5 }, 
        span: Span::dummy() 
    });
    
    // 计算 (10 + 5) * 3
    main_fn.add_instruction(Instruction::Mul { 
        dst: reg_temp2, 
        src1: Operand::Register { id: reg_temp1 }, 
        src2: Operand::Register { id: reg_3 }, 
        span: Span::dummy() 
    });
    
    // 计算 ((10 + 5) * 3) - 8
    main_fn.add_instruction(Instruction::Sub { 
        dst: reg_result, 
        src1: Operand::Register { id: reg_temp2 }, 
        src2: Operand::Register { id: reg_8 }, 
        span: Span::dummy() 
    });
    
    // 返回结果
    main_fn.add_instruction(Instruction::Return { 
        value: Some(reg_result), 
        span: Span::dummy() 
    });
    
    program.add_function(main_fn);
    program.set_main("main".to_string());
    program
} 