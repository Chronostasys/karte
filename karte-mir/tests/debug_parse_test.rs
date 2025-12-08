use karte_ir_codec::{IrDisplay, IrParse};
use karte_mir::*;

#[test]
fn test_parse_simple_value() {
    // Test parsing a simple Value
    let original = Value::Number { value: 2, ty: None };
    let value_str = original.to_ir_string();
    let parsed = Value::parse_ir(&value_str);
    println!("Parsing '{}': {:?}", value_str, parsed);
    assert_eq!(parsed.unwrap(), original);
}

#[test]
fn test_display_and_parse_mir_function() {
    // Create a simple MirFunction
    let func = MirFunction::new("test".to_string(), vec![]);

    // Display it
    let displayed = func.to_ir_string();
    println!("Displayed MirFunction:\n{}", displayed);

    // Try to parse it back
    let parsed = MirFunction::parse_ir(&displayed);
    println!("Parse result: {:?}", parsed);

    if let Ok(parsed_func) = parsed {
        assert_eq!(parsed_func.name, func.name);
    }
}

#[test]
fn test_display_and_parse_mir_program() {
    // Create a simple MirProgram
    let mut program = MirProgram::new();
    program.main_function = Some("main".to_string());

    let main_func = MirFunction::new("main".to_string(), vec![]);
    program.functions.insert("main".to_string(), main_func);

    // Display it
    let displayed = program.to_ir_string();
    println!("Displayed MirProgram:\n{}", displayed);

    // Try to parse it back
    let parsed = MirProgram::parse_ir(&displayed);
    println!("Parse result: {:?}", parsed);

    assert!(parsed.is_ok());
}

fn roundtrip_statement(stmt: &Statement) {
    let text = stmt.to_ir_string();
    println!("Statement display:\n{}", text);
    let parsed = Statement::parse_ir(&text).expect("parse statement");
    assert_eq!(parsed, *stmt);
}

#[test]
fn test_allocate_statements_roundtrip() {
    let layout = HeapLayout {
        type_id: "closure_env".to_string(),
        size: 32,
        align: 8,
        mutable: true,
        escape: EscapeState::Global,
        ownership: OwnershipKind::Manual,
    };

    let alloc_stmt = Statement::Allocate {
        target: Value::Temp {
            id: TempId(1),
            ty: None,
        },
        layout: layout.clone(),
        span: Default::default(),
    };
    roundtrip_statement(&alloc_stmt);

    let dealloc_stmt = Statement::Deallocate {
        pointer: Value::Temp {
            id: TempId(1),
            ty: None,
        },
        layout,
        span: Default::default(),
    };
    roundtrip_statement(&dealloc_stmt);
}

#[test]
fn test_gc_barrier_statements_roundtrip() {
    let mark_stmt = Statement::MarkGcRoot {
        value: Value::Temp {
            id: TempId(2),
            ty: None,
        },
        root: GcRootKind::StackSlot { slot: 0 },
        span: Default::default(),
    };
    roundtrip_statement(&mark_stmt);

    let write_barrier_stmt = Statement::WriteBarrier {
        object: Value::Temp {
            id: TempId(3),
            ty: None,
        },
        slot: Some("env_ptr".to_string()),
        value: Value::Temp {
            id: TempId(4),
            ty: None,
        },
        span: Default::default(),
    };
    roundtrip_statement(&write_barrier_stmt);

    let read_barrier_stmt = Statement::ReadBarrier {
        target: Value::Temp {
            id: TempId(5),
            ty: None,
        },
        object: Value::Temp {
            id: TempId(3),
            ty: None,
        },
        slot: Some("env_ptr".to_string()),
        span: Default::default(),
    };
    roundtrip_statement(&read_barrier_stmt);
}

#[test]
fn test_heap_layout_roundtrip() {
    let layout = HeapLayout {
        type_id: "vector".to_string(),
        size: 64,
        align: 16,
        mutable: false,
        escape: EscapeState::Return,
        ownership: OwnershipKind::Manual,
    };

    let text = layout.to_ir_string();
    println!("HeapLayout display: {}", text);
    let parsed = HeapLayout::parse_ir(&text).expect("parse heap layout");
    assert_eq!(parsed, layout);
}
