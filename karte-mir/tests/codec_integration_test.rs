use karte_diagnostics::Span as DiagSpan;
use karte_ir_codec::{IrDisplay, IrParse};
/// IR Codec 集成测试
///
/// 这个测试演示了如何在实际的 IR 类型上使用 IrCodec 系统
use karte_ir_derive::IrCodec;
use karte_mir::{Statement as MirStatement, TempId as MirTempId, Value as MirValue};

// 定义一些简单的 IR 类型用于测试

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, IrCodec)]
pub struct BasicBlockId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, IrCodec, Default)]
pub struct TempId(pub usize);

#[derive(Debug, Clone, PartialEq, IrCodec)]
pub enum BinaryOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
    Equal,
    NotEqual,
}

#[derive(Debug, Clone, PartialEq, IrCodec)]
pub enum UnaryOperator {
    Plus,
    Minus,
    Not,
}

#[derive(Debug, Clone, PartialEq, IrCodec, Default)]
pub enum Value {
    Variable {
        name: String,
    },
    Number {
        value: i64,
    },
    Boolean {
        value: bool,
    },
    #[default]
    Unit,
    Temp {
        id: TempId,
    },
    Constructor {
        name: String,
        arg: Option<Box<Value>>,
    },
    Function {
        name: String,
    },
    Closure {
        function_name: String,
        captured_values: Vec<Value>,
    },
    Reference {
        value: Box<Value>,
    },
}

#[derive(Debug, Clone, PartialEq, IrCodec)]
pub enum SimpleStatement {
    Assign {
        target: Value,
        source: Value,
    },
    BinaryOp {
        target: Value,
        left: Value,
        op: BinaryOperator,
        right: Value,
    },
    UnaryOp {
        target: Value,
        op: UnaryOperator,
        operand: Value,
    },
}

// ============================================================================
// 测试
// ============================================================================

#[test]
fn test_basic_block_id_roundtrip() {
    let id = BasicBlockId(42);
    let text = id.to_ir_string();
    assert_eq!(text, "BasicBlockId(42)");

    let parsed = BasicBlockId::parse_ir(&text).unwrap();
    assert_eq!(parsed, id);
}

#[test]
fn test_temp_id_roundtrip() {
    let id = TempId(10);
    let text = id.to_ir_string();
    assert_eq!(text, "TempId(10)");

    let parsed = TempId::parse_ir(&text).unwrap();
    assert_eq!(parsed, id);
}

#[test]
fn test_binary_operator_roundtrip() {
    let operators = vec![
        BinaryOperator::Add,
        BinaryOperator::Subtract,
        BinaryOperator::Multiply,
        BinaryOperator::Divide,
        BinaryOperator::Equal,
        BinaryOperator::NotEqual,
    ];

    for op in operators {
        let text = op.to_ir_string();
        let parsed = BinaryOperator::parse_ir(&text).unwrap();
        assert_eq!(parsed, op);
    }
}

#[test]
fn test_value_variable() {
    let value = Value::Variable {
        name: "x".to_string(),
    };
    let text = value.to_ir_string();
    assert_eq!(text, "Variable(x)");

    let parsed = Value::parse_ir(&text).unwrap();
    assert_eq!(parsed, value);
}

#[test]
fn test_value_number() {
    let value = Value::Number { value: 42 };
    let text = value.to_ir_string();
    assert_eq!(text, "Number(42)");

    let parsed = Value::parse_ir(&text).unwrap();
    assert_eq!(parsed, value);
}

#[test]
fn test_value_boolean() {
    let value_true = Value::Boolean { value: true };
    let text_true = value_true.to_ir_string();
    assert_eq!(text_true, "Boolean(true)");

    let value_false = Value::Boolean { value: false };
    let text_false = value_false.to_ir_string();
    assert_eq!(text_false, "Boolean(false)");

    let parsed_true = Value::parse_ir(&text_true).unwrap();
    assert_eq!(parsed_true, value_true);

    let parsed_false = Value::parse_ir(&text_false).unwrap();
    assert_eq!(parsed_false, value_false);
}

#[test]
fn test_value_unit() {
    let value = Value::Unit;
    let text = value.to_ir_string();
    assert_eq!(text, "Unit");

    let parsed = Value::parse_ir(&text).unwrap();
    assert_eq!(parsed, value);
}

#[test]
fn test_value_temp() {
    let value = Value::Temp { id: TempId(5) };
    let text = value.to_ir_string();
    assert_eq!(text, "Temp(TempId(5))");

    let parsed = Value::parse_ir(&text).unwrap();
    assert_eq!(parsed, value);
}

#[test]
fn test_value_constructor_without_arg() {
    let value = Value::Constructor {
        name: "None".to_string(),
        arg: None,
    };
    let text = value.to_ir_string();
    // 格式应该类似 "Constructor { name = None, arg = none }"

    let parsed = Value::parse_ir(&text).unwrap();
    assert_eq!(parsed, value);
}

#[test]
fn test_value_constructor_with_arg() {
    let value = Value::Constructor {
        name: "Some".to_string(),
        arg: Some(Box::new(Value::Number { value: 42 })),
    };
    let text = value.to_ir_string();

    let parsed = Value::parse_ir(&text).unwrap();
    assert_eq!(parsed, value);
}

#[test]
fn test_value_function() {
    let value = Value::Function {
        name: "factorial".to_string(),
    };
    let text = value.to_ir_string();
    assert_eq!(text, "Function(factorial)");

    let parsed = Value::parse_ir(&text).unwrap();
    assert_eq!(parsed, value);
}

#[test]
fn test_value_closure() {
    let value = Value::Closure {
        function_name: "add_x".to_string(),
        captured_values: vec![
            Value::Variable {
                name: "x".to_string(),
            },
            Value::Number { value: 10 },
        ],
    };
    let text = value.to_ir_string();

    let parsed = Value::parse_ir(&text).unwrap();
    assert_eq!(parsed, value);
}

#[test]
fn test_value_reference() {
    let value = Value::Reference {
        value: Box::new(Value::Variable {
            name: "x".to_string(),
        }),
    };
    let text = value.to_ir_string();
    assert_eq!(text, "Reference(Variable(x))");

    let parsed = Value::parse_ir(&text).unwrap();
    assert_eq!(parsed, value);
}

#[test]
fn test_statement_assign() {
    let stmt = SimpleStatement::Assign {
        target: Value::Variable {
            name: "x".to_string(),
        },
        source: Value::Number { value: 42 },
    };
    let text = stmt.to_ir_string();

    let parsed = SimpleStatement::parse_ir(&text).unwrap();
    assert_eq!(parsed, stmt);
}

#[test]
fn test_statement_binary_op() {
    let stmt = SimpleStatement::BinaryOp {
        target: Value::Temp { id: TempId(0) },
        left: Value::Variable {
            name: "x".to_string(),
        },
        op: BinaryOperator::Add,
        right: Value::Number { value: 1 },
    };
    let text = stmt.to_ir_string();

    let parsed = SimpleStatement::parse_ir(&text).unwrap();
    assert_eq!(parsed, stmt);
}

#[test]
fn test_statement_unary_op() {
    let stmt = SimpleStatement::UnaryOp {
        target: Value::Temp { id: TempId(0) },
        op: UnaryOperator::Minus,
        operand: Value::Variable {
            name: "x".to_string(),
        },
    };
    let text = stmt.to_ir_string();

    let parsed = SimpleStatement::parse_ir(&text).unwrap();
    assert_eq!(parsed, stmt);
}

#[test]
fn test_complex_nested_value() {
    let value = Value::Constructor {
        name: "Pair".to_string(),
        arg: Some(Box::new(Value::Closure {
            function_name: "lambda".to_string(),
            captured_values: vec![
                Value::Number { value: 1 },
                Value::Boolean { value: true },
                Value::Variable {
                    name: "x".to_string(),
                },
            ],
        })),
    };

    let text = value.to_ir_string();
    let parsed = Value::parse_ir(&text).unwrap();
    assert_eq!(parsed, value);
}

#[test]
fn test_mir_statement_call_roundtrip() {
    let stmt = MirStatement::Call {
        target: None,
        function: MirValue::Temp { id: MirTempId(4) },
        args: vec![MirValue::Temp { id: MirTempId(5) }, MirValue::Temp { id: MirTempId(3) }],
        span: DiagSpan::default(),
    };

    let text = stmt.to_ir_string();
    assert!(
        text.trim().starts_with("call "),
        "call display should start with 'call ': {}",
        text
    );

    let parsed = MirStatement::parse_ir(&text).unwrap();
    assert_eq!(parsed, stmt);
}

#[test]
fn test_mir_statement_call_parse_from_text() {
    let raw = "call function: %4, args: [%5, %3]";
    let parsed = MirStatement::parse_ir(raw).expect("call text should parse");

    let expected = MirStatement::Call {
        target: None,
        function: MirValue::Temp { id: MirTempId(4) },
        args: vec![
            MirValue::Temp { id: MirTempId(5) },
            MirValue::Temp { id: MirTempId(3) },
        ],
        span: DiagSpan::default(),
    };

    assert_eq!(parsed, expected);
}
