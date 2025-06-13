use karte_lir::*;
use karte_diagnostics::Span;
use karte_codegen::lir_interpreter::execute_with_debug;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== 简单寄存器溢出测试 ===");
    
    let program = create_simple_spill_test();
    
    println!("执行简单的寄存器溢出测试程序...");
    let result = execute_with_debug(&program, true)?;
    
    println!("结果: {}", result);
    println!("期望结果: 30 (1+2+3+4+5+6+7+8)");
    
    if result == 36 {
        println!("✅ 测试通过！");
    } else {
        println!("❌ 测试失败！");
    }
    
    Ok(())
}

/// 创建简单的溢出测试：计算1+2+3+4+5+6+7+8
fn create_simple_spill_test() -> LirProgram {
    let mut program = LirProgram::new();
    let mut main_fn = LirFunction::new("main".to_string());
    
    // 创建超过8个寄存器来强制溢出
    let mut regs = Vec::new();
    for _ in 0..12 {  // 12个寄存器，超过8个的限制
        regs.push(main_fn.new_register());
    }
    
    let entry_label = main_fn.new_label();
    
    main_fn.add_instruction(Instruction::Label { 
        id: entry_label, 
        span: Span::dummy() 
    });
    
    // 初始化前8个寄存器为1-8
    for i in 0..8 {
        main_fn.add_instruction(Instruction::Move { 
            dst: regs[i], 
            src: Operand::Immediate { value: (i + 1) as i64 }, 
            span: Span::dummy() 
        });
    }
    
    // 初始化后4个寄存器为0
    for i in 8..12 {
        main_fn.add_instruction(Instruction::Move { 
            dst: regs[i], 
            src: Operand::Immediate { value: 0 }, 
            span: Span::dummy() 
        });
    }
    
    // 计算 reg[8] = reg[0] + reg[1]  (1+2=3)
    main_fn.add_instruction(Instruction::Add { 
        dst: regs[8], 
        src1: Operand::Register { id: regs[0] }, 
        src2: Operand::Register { id: regs[1] }, 
        span: Span::dummy() 
    });
    
    // 计算 reg[9] = reg[2] + reg[3]  (3+4=7)
    main_fn.add_instruction(Instruction::Add { 
        dst: regs[9], 
        src1: Operand::Register { id: regs[2] }, 
        src2: Operand::Register { id: regs[3] }, 
        span: Span::dummy() 
    });
    
    // 计算 reg[10] = reg[4] + reg[5]  (5+6=11)
    main_fn.add_instruction(Instruction::Add { 
        dst: regs[10], 
        src1: Operand::Register { id: regs[4] }, 
        src2: Operand::Register { id: regs[5] }, 
        span: Span::dummy() 
    });
    
    // 计算 reg[11] = reg[6] + reg[7]  (7+8=15)
    main_fn.add_instruction(Instruction::Add { 
        dst: regs[11], 
        src1: Operand::Register { id: regs[6] }, 
        src2: Operand::Register { id: regs[7] }, 
        span: Span::dummy() 
    });
    
    // 计算最终结果：reg[0] = reg[8] + reg[9] + reg[10] + reg[11]
    main_fn.add_instruction(Instruction::Add { 
        dst: regs[0], 
        src1: Operand::Register { id: regs[8] }, 
        src2: Operand::Register { id: regs[9] }, 
        span: Span::dummy() 
    });
    
    main_fn.add_instruction(Instruction::Add { 
        dst: regs[1], 
        src1: Operand::Register { id: regs[10] }, 
        src2: Operand::Register { id: regs[11] }, 
        span: Span::dummy() 
    });
    
    main_fn.add_instruction(Instruction::Add { 
        dst: regs[2], 
        src1: Operand::Register { id: regs[0] }, 
        src2: Operand::Register { id: regs[1] }, 
        span: Span::dummy() 
    });
    
    main_fn.add_instruction(Instruction::Return { 
        value: Some(regs[2]), 
        span: Span::dummy() 
    });
    
    program.add_function(main_fn);
    program.set_main("main".to_string());
    program
} 