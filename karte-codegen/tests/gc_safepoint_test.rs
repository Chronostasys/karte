//! GC 安全点测试
//!
//! 验证 GC 安全点机制是否正常工作

use karte_codegen::vm::professional_executor::{ExecutionEngine, ProgramManager};
use karte_diagnostics::Span;
use karte_lir::{Instruction, LabelId, LirFunction, LirProgram, Operand, Register};
use std::collections::HashMap;

#[test]
fn test_safepoint_instruction_compilation() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .is_test(true)
        .try_init()
        .ok();

    // 创建一个包含安全点指令的简单函数
    let mut main_function = LirFunction::new("main".to_string());

    main_function.instructions = vec![
        // label_main:
        Instruction::Label {
            id: LabelId(1),
            span: Span::default(),
        },
        // mov r0, #42
        Instruction::Move {
            dst: Register::Physical(0),
            src: Operand::Immediate { value: 42 },
            span: Span::default(),
        },
        // safepoint
        Instruction::Safepoint {
            span: Span::default(),
        },
        // return r0
        Instruction::Return {
            value: Some(Register::Physical(0)),
            span: Span::default(),
        },
    ];

    main_function.used_regs = vec![0];

    let mut functions = HashMap::new();
    functions.insert("main".to_string(), main_function);

    let program = LirProgram {
        functions,
        main_function: Some("main".to_string()),
        global_struct_types: HashMap::new(),
        global_variables: HashMap::new(),
    };

    // 创建执行引擎
    let mut engine = ExecutionEngine::new(false);
    let program_manager = ProgramManager::new();

    // 初始化
    engine.initialize(&program_manager).expect("初始化失败");

    // 执行程序（包含安全点）
    let result = engine.compile_and_execute_with_jit(&program);

    assert!(result.is_ok(), "执行失败: {:?}", result.err());
    assert_eq!(result.unwrap(), 42, "返回值应该是42");

    println!("✅ GC 安全点指令测试通过");
}

#[test]
fn test_multiple_safepoints_in_loop() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .is_test(true)
        .try_init()
        .ok();

    // 创建一个包含循环和多个安全点的函数
    let mut main_function = LirFunction::new("main".to_string());

    main_function.instructions = vec![
        // label_main:
        Instruction::Label {
            id: LabelId(1),
            span: Span::default(),
        },
        // mov r0, #0  ; 计数器
        Instruction::Move {
            dst: Register::Physical(0),
            src: Operand::Immediate { value: 0 },
            span: Span::default(),
        },
        // mov r1, #10 ; 限制
        Instruction::Move {
            dst: Register::Physical(1),
            src: Operand::Immediate { value: 10 },
            span: Span::default(),
        },
        // label_loop:
        Instruction::Label {
            id: LabelId(2),
            span: Span::default(),
        },
        // safepoint (循环回边安全点)
        Instruction::Safepoint {
            span: Span::default(),
        },
        // add r0, r0, #1
        Instruction::Add {
            dst: Register::Physical(0),
            src1: Operand::Register {
                id: Register::Physical(0),
            },
            src2: Operand::Immediate { value: 1 },
            span: Span::default(),
        },
        // compare r0, r1
        Instruction::Compare {
            src1: Operand::Register {
                id: Register::Physical(0),
            },
            src2: Operand::Register {
                id: Register::Physical(1),
            },
            span: Span::default(),
        },
        // jl label_loop
        Instruction::JumpLess {
            target: LabelId(2),
            span: Span::default(),
        },
        // return r0
        Instruction::Return {
            value: Some(Register::Physical(0)),
            span: Span::default(),
        },
    ];

    main_function.used_regs = vec![0, 1];

    let mut functions = HashMap::new();
    functions.insert("main".to_string(), main_function);

    let program = LirProgram {
        functions,
        main_function: Some("main".to_string()),
        global_struct_types: HashMap::new(),
        global_variables: HashMap::new(),
    };

    // 创建执行引擎
    let mut engine = ExecutionEngine::new(false);
    let program_manager = ProgramManager::new();

    // 初始化
    engine.initialize(&program_manager).expect("初始化失败");

    // 执行程序（循环中包含安全点）
    let result = engine.compile_and_execute_with_jit(&program);

    assert!(result.is_ok(), "执行失败: {:?}", result.err());
    assert_eq!(result.unwrap(), 10, "返回值应该是10");

    println!("✅ 循环中多个安全点测试通过");
}
