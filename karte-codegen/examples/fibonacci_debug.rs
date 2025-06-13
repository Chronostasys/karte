//! 简单的斐波那契调试程序
//! 
//! 验证基本的斐波那契计算逻辑

use karte_codegen::lir_interpreter::{execute_with_debug, analyze_register_usage};
use karte_lir::*;
use karte_diagnostics::Span;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== 斐波那契调试程序 ===\n");
    
    let n = 5; // 简单测试：计算第5个斐波那契数（应该是5）
    println!("计算第{}个斐波那契数", n);
    
    let lir_program = create_simple_fibonacci(n);
    
    // 先分析寄存器使用情况
    println!("\n寄存器使用分析：");
    let analysis = analyze_register_usage(&lir_program)?;
    analysis.print_analysis();
    
    println!("\n执行调试模式：");
    let result = execute_with_debug(&lir_program, true)?;
    println!("\n结果: {}", result);
    
    let expected = fibonacci_reference(n);
    println!("期望: {}", expected);
    
    if result == expected {
        println!("✅ 计算正确！");
    } else {
        println!("❌ 计算错误！");
    }
    
    Ok(())
}

/// 创建简单的斐波那契程序（使用最少寄存器）
fn create_simple_fibonacci(n: i64) -> LirProgram {
    let mut program = LirProgram::new();
    let mut main_fn = LirFunction::new("main".to_string());
    
    if n <= 1 {
        let result_reg = main_fn.new_register();
        let entry_label = main_fn.new_label();
        
        main_fn.add_instruction(Instruction::Label { 
            id: entry_label, 
            span: Span::dummy() 
        });
        
        main_fn.add_instruction(Instruction::Move { 
            dst: result_reg, 
            src: Operand::Immediate { value: n }, 
            span: Span::dummy() 
        });
        
        main_fn.add_instruction(Instruction::Return { 
            value: Some(result_reg), 
            span: Span::dummy() 
        });
        
        program.add_function(main_fn);
        program.set_main("main".to_string());
        return program;
    }
    
    // 只使用必要的寄存器
    let n_reg = main_fn.new_register();     // 存储n
    let a_reg = main_fn.new_register();     // fib(i-2)
    let b_reg = main_fn.new_register();     // fib(i-1)
    let i_reg = main_fn.new_register();     // 循环计数器
    let temp_reg = main_fn.new_register();  // 临时存储
    
    let entry_label = main_fn.new_label();
    let loop_start = main_fn.new_label();
    let loop_end = main_fn.new_label();
    
    // 程序开始
    main_fn.add_instruction(Instruction::Label { 
        id: entry_label, 
        span: Span::dummy() 
    });
    
    // 初始化
    main_fn.add_instruction(Instruction::Move { 
        dst: n_reg, 
        src: Operand::Immediate { value: n }, 
        span: Span::dummy() 
    });
    
    main_fn.add_instruction(Instruction::Move { 
        dst: a_reg, 
        src: Operand::Immediate { value: 0 }, 
        span: Span::dummy() 
    });
    
    main_fn.add_instruction(Instruction::Move { 
        dst: b_reg, 
        src: Operand::Immediate { value: 1 }, 
        span: Span::dummy() 
    });
    
    main_fn.add_instruction(Instruction::Move { 
        dst: i_reg, 
        src: Operand::Immediate { value: 2 }, 
        span: Span::dummy() 
    });
    
    // 循环开始
    main_fn.add_instruction(Instruction::Label { 
        id: loop_start, 
        span: Span::dummy() 
    });
    
    // 检查循环条件：i > n 则跳出
    main_fn.add_instruction(Instruction::Compare { 
        src1: Operand::Register { id: i_reg }, 
        src2: Operand::Register { id: n_reg }, 
        span: Span::dummy() 
    });
    
    main_fn.add_instruction(Instruction::JumpGreater { 
        target: loop_end, 
        span: Span::dummy() 
    });
    
    // 计算下一个斐波那契数
    main_fn.add_instruction(Instruction::Add { 
        dst: temp_reg, 
        src1: Operand::Register { id: a_reg }, 
        src2: Operand::Register { id: b_reg }, 
        span: Span::dummy() 
    });
    
    // 更新 a = b, b = temp
    main_fn.add_instruction(Instruction::Move { 
        dst: a_reg, 
        src: Operand::Register { id: b_reg }, 
        span: Span::dummy() 
    });
    
    main_fn.add_instruction(Instruction::Move { 
        dst: b_reg, 
        src: Operand::Register { id: temp_reg }, 
        span: Span::dummy() 
    });
    
    // i = i + 1
    main_fn.add_instruction(Instruction::Add { 
        dst: i_reg, 
        src1: Operand::Register { id: i_reg }, 
        src2: Operand::Immediate { value: 1 }, 
        span: Span::dummy() 
    });
    
    // 继续循环
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
        value: Some(b_reg), 
        span: Span::dummy() 
    });
    
    program.add_function(main_fn);
    program.set_main("main".to_string());
    program
}

/// 参考斐波那契实现
fn fibonacci_reference(n: i64) -> i64 {
    if n <= 1 {
        n
    } else {
        let mut a = 0;
        let mut b = 1;
        for _ in 2..=n {
            let temp = a + b;
            a = b;
            b = temp;
        }
        b
    }
} 