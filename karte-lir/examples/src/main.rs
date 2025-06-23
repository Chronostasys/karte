use karte_lir::{LirProgram, LirFunction, Instruction, Register, Operand, LabelId};
use karte_diagnostics::Span;

fn main() {
    // 创建一个简单的测试程序
    let mut program = LirProgram::new();
    
    // 创建主函数
    let mut main_fn = LirFunction::new("main".to_string());
    
    // 添加一些测试指令
    main_fn.add_instruction(Instruction::Move {
        dst: Register::Virtual(1),
        src: Operand::Immediate { value: 42 },
        span: Span::new(0, 0),
    });
    
    // 添加一个Call指令（模拟函数调用）
    main_fn.add_instruction(Instruction::Call {
        target: LabelId(1), // 假设目标函数标签
        args: vec![], // 空的参数寄存器列表
        arg_operands: vec![
            Operand::Register { id: Register::Virtual(1) }, // 参数1
            Operand::Immediate { value: 10 }, // 参数2
        ],
        result: Some(Register::Virtual(2)), // 返回值寄存器
        span: Span::new(0, 0),
    });
    
    // 添加一个CallIndirect指令（模拟间接调用）
    main_fn.add_instruction(Instruction::CallIndirect {
        function_register: Register::Virtual(3), // 函数指针寄存器
        args: vec![], // 空的参数寄存器列表
        arg_operands: vec![
            Operand::Register { id: Register::Virtual(2) }, // 参数1
        ],
        result: Some(Register::Virtual(4)), // 返回值寄存器
        span: Span::new(0, 0),
    });
    
    // 添加返回指令
    main_fn.add_instruction(Instruction::Return {
        value: Some(Register::Virtual(4)),
        span: Span::new(0, 0),
    });
    
    // 将函数添加到程序
    program.add_function(main_fn);
    program.set_main("main".to_string());
    
    // 打印原始LIR
    println!("=== 原始LIR ===");
    println!("{}", program);
    
    // 应用指令降级
    match karte_lir::lower_instructions::lower_program_instructions(&mut program) {
        Ok(()) => {
            println!("\n=== 降级后的LIR ===");
            println!("{}", program);
        }
        Err(e) => {
            println!("降级失败: {}", e);
        }
    }
} 