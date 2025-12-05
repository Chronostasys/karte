//! 本地指针不逃逸测试
//!
//! 验证Phase 1优化：当指针只在本地使用时，不应该标记为逃逸

use karte_escape_analysis::{EscapeAnalyzer, EscapeState};
use karte_mir::{BasicBlock, BasicBlockId, MirFunction, MirProgram, Statement, Terminator, Value};
use std::collections::BTreeMap;

/// 辅助函数：创建简单的MIR函数
fn create_test_function(name: &str) -> MirFunction {
    let entry_block = BasicBlockId(0);
    MirFunction {
        name: name.to_string(),
        params: Vec::new(),
        param_types: Vec::new(),
        return_type: None,
        basic_blocks: BTreeMap::new(),
        entry_block,
        next_block_id: 1,
        next_temp_id: 0,
    }
}

#[test]
fn test_local_pointer_no_escape() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init()
        .ok();

    // fn foo() -> number {
    //     let x = 42;
    //     let ptr = &x;
    //     let y = *ptr + 10;
    //     y  // 返回值，不是指针
    // }
    // 期望：x 和 ptr 都是 NoEscape（栈分配）

    let mut analyzer = EscapeAnalyzer::new();
    let mut program = MirProgram::new();

    let mut function = create_test_function("foo");
    let mut bb0 = BasicBlock::new(BasicBlockId(0));

    // let x = 42
    bb0.add_statement(Statement::Assign {
        target: Value::Variable {
            name: "x".to_string(),
            ty: None,
        },
        source: Value::Number {
            value: 42,
            ty: None,
        },
        span: Default::default(),
    });

    // let ptr = &x
    bb0.add_statement(Statement::Assign {
        target: Value::Variable {
            name: "ptr".to_string(),
            ty: None,
        },
        source: Value::Reference {
            value: Box::new(Value::Variable {
                name: "x".to_string(),
                ty: None,
            }),
            ty: None,
        },
        span: Default::default(),
    });

    // let temp = *ptr
    bb0.add_statement(Statement::Dereference {
        target: Value::Variable {
            name: "temp".to_string(),
            ty: None,
        },
        reference: Value::Variable {
            name: "ptr".to_string(),
            ty: None,
        },
        span: Default::default(),
    });

    // let y = temp + 10
    bb0.add_statement(Statement::BinaryOp {
        target: Value::Variable {
            name: "y".to_string(),
            ty: None,
        },
        op: karte_mir::BinaryOperator::Add,
        left: Value::Variable {
            name: "temp".to_string(),
            ty: None,
        },
        right: Value::Number {
            value: 10,
            ty: None,
        },
        span: Default::default(),
    });

    // return y
    bb0.set_terminator(Terminator::Return {
        value: Some(Value::Variable {
            name: "y".to_string(),
            ty: None,
        }),
        span: Default::default(),
    });

    function.basic_blocks.insert(BasicBlockId(0), bb0);
    program.functions.insert("foo".to_string(), function);

    // 运行分析
    analyzer.analyze_program(&program).unwrap();
    analyzer.print_results();

    // 验证：x 应该是 NoEscape（指针只在本地使用）
    let x_id = analyzer.get_var_id_by_name("x").unwrap();
    let x_info = analyzer.get_escape_info(&x_id).unwrap();
    assert_eq!(
        x_info.escape_state,
        EscapeState::NoEscape,
        "x should be NoEscape since ptr is only used locally"
    );

    // 验证：ptr 也应该是 NoEscape
    let ptr_id = analyzer.get_var_id_by_name("ptr").unwrap();
    let ptr_info = analyzer.get_escape_info(&ptr_id).unwrap();
    assert_eq!(
        ptr_info.escape_state,
        EscapeState::NoEscape,
        "ptr should be NoEscape since it's only dereferenced locally"
    );
}

#[test]
fn test_pointer_return_escapes() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init()
        .ok();

    // fn bar() -> &number {
    //     let x = 42;
    //     &x  // 指针逃逸
    // }
    // 期望：x 是 ReturnEscape（堆分配）

    let mut analyzer = EscapeAnalyzer::new();
    let mut program = MirProgram::new();

    let mut function = create_test_function("bar");
    let mut bb0 = BasicBlock::new(BasicBlockId(0));

    // let x = 42
    bb0.add_statement(Statement::Assign {
        target: Value::Variable {
            name: "x".to_string(),
            ty: None,
        },
        source: Value::Number {
            value: 42,
            ty: None,
        },
        span: Default::default(),
    });

    // return &x
    bb0.set_terminator(Terminator::Return {
        value: Some(Value::Reference {
            value: Box::new(Value::Variable {
                name: "x".to_string(),
                ty: None,
            }),
            ty: None,
        }),
        span: Default::default(),
    });

    function.basic_blocks.insert(BasicBlockId(0), bb0);
    program.functions.insert("bar".to_string(), function);

    // 运行分析
    analyzer.analyze_program(&program).unwrap();
    analyzer.print_results();

    // 验证：x 应该是 ReturnEscape（指针通过返回值逃逸）
    let x_id = analyzer.get_var_id_by_name("x").unwrap();
    let x_info = analyzer.get_escape_info(&x_id).unwrap();
    assert_eq!(
        x_info.escape_state,
        EscapeState::ReturnEscape,
        "x should be ReturnEscape since &x is returned"
    );
}

#[test]
fn test_multiple_local_pointers() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init()
        .ok();

    // fn test() -> number {
    //     let a = 1;
    //     let b = 2;
    //     let ptr1 = &a;
    //     let ptr2 = &b;
    //     *ptr1 + *ptr2
    // }
    // 期望：a, b, ptr1, ptr2 都是 NoEscape

    let mut analyzer = EscapeAnalyzer::new();
    let mut program = MirProgram::new();

    let mut function = create_test_function("test");
    let mut bb0 = BasicBlock::new(BasicBlockId(0));

    // let a = 1
    bb0.add_statement(Statement::Assign {
        target: Value::Variable {
            name: "a".to_string(),
            ty: None,
        },
        source: Value::Number {
            value: 1,
            ty: None,
        },
        span: Default::default(),
    });

    // let b = 2
    bb0.add_statement(Statement::Assign {
        target: Value::Variable {
            name: "b".to_string(),
            ty: None,
        },
        source: Value::Number {
            value: 2,
            ty: None,
        },
        span: Default::default(),
    });

    // let ptr1 = &a
    bb0.add_statement(Statement::Assign {
        target: Value::Variable {
            name: "ptr1".to_string(),
            ty: None,
        },
        source: Value::Reference {
            value: Box::new(Value::Variable {
                name: "a".to_string(),
                ty: None,
            }),
            ty: None,
        },
        span: Default::default(),
    });

    // let ptr2 = &b
    bb0.add_statement(Statement::Assign {
        target: Value::Variable {
            name: "ptr2".to_string(),
            ty: None,
        },
        source: Value::Reference {
            value: Box::new(Value::Variable {
                name: "b".to_string(),
                ty: None,
            }),
            ty: None,
        },
        span: Default::default(),
    });

    // let temp1 = *ptr1
    bb0.add_statement(Statement::Dereference {
        target: Value::Variable {
            name: "temp1".to_string(),
            ty: None,
        },
        reference: Value::Variable {
            name: "ptr1".to_string(),
            ty: None,
        },
        span: Default::default(),
    });

    // let temp2 = *ptr2
    bb0.add_statement(Statement::Dereference {
        target: Value::Variable {
            name: "temp2".to_string(),
            ty: None,
        },
        reference: Value::Variable {
            name: "ptr2".to_string(),
            ty: None,
        },
        span: Default::default(),
    });

    // let result = temp1 + temp2
    bb0.add_statement(Statement::BinaryOp {
        target: Value::Variable {
            name: "result".to_string(),
            ty: None,
        },
        op: karte_mir::BinaryOperator::Add,
        left: Value::Variable {
            name: "temp1".to_string(),
            ty: None,
        },
        right: Value::Variable {
            name: "temp2".to_string(),
            ty: None,
        },
        span: Default::default(),
    });

    // return result
    bb0.set_terminator(Terminator::Return {
        value: Some(Value::Variable {
            name: "result".to_string(),
            ty: None,
        }),
        span: Default::default(),
    });

    function.basic_blocks.insert(BasicBlockId(0), bb0);
    program.functions.insert("test".to_string(), function);

    // 运行分析
    analyzer.analyze_program(&program).unwrap();
    analyzer.print_results();

    // 验证：所有变量都是 NoEscape
    for var_name in &["a", "b", "ptr1", "ptr2"] {
        let var_id = analyzer.get_var_id_by_name(var_name).unwrap();
        let var_info = analyzer.get_escape_info(&var_id).unwrap();
        assert_eq!(
            var_info.escape_state,
            EscapeState::NoEscape,
            "{} should be NoEscape",
            var_name
        );
    }
}
