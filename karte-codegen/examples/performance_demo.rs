//! LIR解释器性能演示
//! 
//! 对比启用和不启用调试模式的执行性能

use karte_codegen::lir_interpreter::{execute, execute_with_debug, analyze_register_usage};
use karte_lir::*;
use karte_diagnostics::Span;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Karte LIR 解释器性能演示 ===\n");
    
    let program = create_demo_program();
    
    // 寄存器使用分析
    println!("1. 寄存器使用分析：");
    let analysis = analyze_register_usage(&program)?;
    analysis.print_analysis();
    
    // 性能测试 - 无调试模式
    println!("\n2. 性能测试：");
    let start = Instant::now();
    let result1 = execute(&program)?;
    let duration1 = start.elapsed();
    println!("   无调试模式: 结果 = {}, 耗时 = {:?}", result1, duration1);
    
    // 性能测试 - 调试模式
    let start = Instant::now();
    let result2 = execute_with_debug(&program, true)?;
    let duration2 = start.elapsed();
    println!("   调试模式: 结果 = {}, 耗时 = {:?}", result2, duration2);
    
    // 性能对比
    println!("\n3. 性能对比：");
    if duration2.as_nanos() > 0 {
        let speedup = duration2.as_nanos() as f64 / duration1.as_nanos() as f64;
        println!("   调试模式比无调试模式慢 {:.2}x", speedup);
    }
    
    println!("\n=== 演示完成 ===");
    println!("✅ 虚拟机具有固定32个寄存器");
    println!("✅ 实现了线性扫描寄存器分配算法");
    println!("✅ 模块化的虚拟机架构");
    println!("✅ 专业的调试和性能分析功能");
    
    Ok(())
}

/// 创建一个计算密集的演示程序
/// 计算 1 + 2 + 3 + ... + 100 = 5050
fn create_demo_program() -> LirProgram {
    let mut program = LirProgram::new();
    let mut main_fn = LirFunction::new("main".to_string());
    
    // 创建寄存器
    let sum_reg = main_fn.new_register();      // 累计和
    let counter_reg = main_fn.new_register();  // 计数器
    let limit_reg = main_fn.new_register();    // 上限100
    let one_reg = main_fn.new_register();      // 常数1
    
    // 创建标签
    let entry_label = main_fn.new_label();
    let loop_start = main_fn.new_label();
    let loop_end = main_fn.new_label();
    
    // 程序开始
    main_fn.add_instruction(Instruction::Label { 
        id: entry_label, 
        span: Span::dummy() 
    });
    
    // 初始化 sum = 0
    main_fn.add_instruction(Instruction::Move { 
        dst: sum_reg, 
        src: Operand::Immediate { value: 0 }, 
        span: Span::dummy() 
    });
    
    // 初始化 counter = 1
    main_fn.add_instruction(Instruction::Move { 
        dst: counter_reg, 
        src: Operand::Immediate { value: 1 }, 
        span: Span::dummy() 
    });
    
    // 初始化 limit = 100
    main_fn.add_instruction(Instruction::Move { 
        dst: limit_reg, 
        src: Operand::Immediate { value: 100 }, 
        span: Span::dummy() 
    });
    
    // 初始化 one = 1
    main_fn.add_instruction(Instruction::Move { 
        dst: one_reg, 
        src: Operand::Immediate { value: 1 }, 
        span: Span::dummy() 
    });
    
    // 循环开始
    main_fn.add_instruction(Instruction::Label { 
        id: loop_start, 
        span: Span::dummy() 
    });
    
    // 检查 counter > limit
    main_fn.add_instruction(Instruction::Compare { 
        src1: Operand::Register { id: counter_reg }, 
        src2: Operand::Register { id: limit_reg }, 
        span: Span::dummy() 
    });
    
    main_fn.add_instruction(Instruction::JumpGreater { 
        target: loop_end, 
        span: Span::dummy() 
    });
    
    // sum = sum + counter
    main_fn.add_instruction(Instruction::Add { 
        dst: sum_reg, 
        src1: Operand::Register { id: sum_reg }, 
        src2: Operand::Register { id: counter_reg }, 
        span: Span::dummy() 
    });
    
    // counter = counter + 1
    main_fn.add_instruction(Instruction::Add { 
        dst: counter_reg, 
        src1: Operand::Register { id: counter_reg }, 
        src2: Operand::Register { id: one_reg }, 
        span: Span::dummy() 
    });
    
    // 跳回循环开始
    main_fn.add_instruction(Instruction::Jump { 
        target: loop_start, 
        span: Span::dummy() 
    });
    
    // 循环结束
    main_fn.add_instruction(Instruction::Label { 
        id: loop_end, 
        span: Span::dummy() 
    });
    
    // 返回结果
    main_fn.add_instruction(Instruction::Return { 
        value: Some(sum_reg), 
        span: Span::dummy() 
    });
    
    program.add_function(main_fn);
    program.set_main("main".to_string());
    program
} 