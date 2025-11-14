use karte_mir::*;
use karte_ir_codec::{IrDisplay, IrParse};

#[test]
fn test_parse_mir_function_standalone() {
    let input = "MirFunction
    name: test
    params: []
    blocks: {}";
    
    println!("Parsing MirFunction from: {}", input);
    let result = MirFunction::parse_ir(input);
    println!("Result: {:?}", result);
    assert!(result.is_ok(), "Failed to parse: {:?}", result);
}

#[test]
fn test_parse_mir_function_with_leading_space() {
    let input = " MirFunction
    name: test
    params: []
    blocks: {}";
    
    println!("Parsing MirFunction with leading space from: {}", input);
    let result = MirFunction::parse_ir(input);
    println!("Result: {:?}", result);
    assert!(result.is_ok(), "Failed to parse: {:?}", result);
}

#[test]
fn test_parse_hashmap_single_function() {
    let input = "{
    main: MirFunction
        name: main
        params: []
        blocks: {}
    }";
    
    println!("Parsing HashMap<String, MirFunction> from: {}", input);
    let result = std::collections::HashMap::<String, MirFunction>::parse_ir(input);
    println!("Result: {:?}", result);
    assert!(result.is_ok(), "Failed to parse: {:?}", result);
}

#[test]
fn test_parse_hashmap_with_real_blocks() {
    use karte_diagnostics::Span;
    use std::collections::HashMap;

    // 构造一个真实的 MirFunction，并通过 to_ir_string 获取最新格式
    let mut func = MirFunction::new("main".to_string(), vec![]);
    {
        let entry = BasicBlockId(0);
        let block = func
            .basic_blocks
            .get_mut(&entry)
            .expect("entry block should exist");

        block.statements.push(Statement::Assign {
            target: Value::Temp { id: TempId(1) },
            source: Value::Number { value: 2 },
            span: Span::default(),
        });
        block.statements.push(Statement::Assign {
            target: Value::Temp { id: TempId(2) },
            source: Value::Temp { id: TempId(1) },
            span: Span::default(),
        });
        block.statements.push(Statement::Assign {
            target: Value::Temp { id: TempId(3) },
            source: Value::Number { value: 3 },
            span: Span::default(),
        });
        block.statements.push(Statement::BinaryOp {
            target: Value::Temp { id: TempId(0) },
            left: Value::Temp { id: TempId(2) },
            op: BinaryOperator::Multiply,
            right: Value::Temp { id: TempId(3) },
            span: Span::default(),
        });
        block.terminator = Some(Terminator::Return {
            value: Some(Value::Temp { id: TempId(0) }),
            span: Span::default(),
        });
    }

    let mut map = HashMap::new();
    map.insert("main".to_string(), func);
    let input = IrDisplay::to_ir_string(&map);

    println!(
        "Parsing HashMap<String, MirFunction> with real blocks from:\n{}",
        input
    );
    let result = HashMap::<String, MirFunction>::parse_ir(&input);
    println!("Result: {:?}", result);
    assert!(result.is_ok(), "Failed to parse: {:?}", result);
}
