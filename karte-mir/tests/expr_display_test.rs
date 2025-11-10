use karte_ir_codec::IrDisplay;
use karte_ir_derive::IrCodec;
use karte_mir::*;

// 定义一个简单的表达式类型来演示 token 和 args 功能
#[derive(Debug, Clone, PartialEq, IrCodec)]
enum SimpleExpr {
    Number {
        value: i64,
    },
    #[ir_codec(token = "+")]
    Add {
        #[ir_codec(args)]
        left: Box<SimpleExpr>,
        #[ir_codec(args)]
        right: Box<SimpleExpr>,
    },

    #[ir_codec(token = "*")]
    Mul {
        #[ir_codec(args)]
        left: Box<SimpleExpr>,
        #[ir_codec(args)]
        right: Box<SimpleExpr>,
    },

    #[ir_codec(token = "-")]
    Neg {
        #[ir_codec(args)]
        operand: Box<SimpleExpr>,
    },

    Variable {
        name: String,
    },
}

impl Default for SimpleExpr {
    fn default() -> Self {
        SimpleExpr::Number { value: 0 }
    }
}

#[test]
fn test_infix_expression() {
    // 1 + 2
    let expr = SimpleExpr::Add {
        left: Box::new(SimpleExpr::Number { value: 1 }),
        right: Box::new(SimpleExpr::Number { value: 2 }),
    };

    let output = expr.to_ir_string();
    println!("\n=== 中缀表达式 ===");
    println!("1 + 2 => {}", output);

    assert!(output.contains("1"));
    assert!(output.contains("+"));
    assert!(output.contains("2"));
}

#[test]
fn test_nested_expression() {
    // (x + 1) * 2
    let expr = SimpleExpr::Mul {
        left: Box::new(SimpleExpr::Add {
            left: Box::new(SimpleExpr::Variable {
                name: "x".to_string(),
            }),
            right: Box::new(SimpleExpr::Number { value: 1 }),
        }),
        right: Box::new(SimpleExpr::Number { value: 2 }),
    };

    let output = expr.to_ir_string();
    println!("\n=== 嵌套表达式 ===");
    println!("(x + 1) * 2 => {}", output);

    assert!(output.contains("x"));
    assert!(output.contains("+"));
    assert!(output.contains("*"));
}

#[test]
fn test_prefix_expression() {
    // -5
    let expr = SimpleExpr::Neg {
        operand: Box::new(SimpleExpr::Number { value: 5 }),
    };

    let output = expr.to_ir_string();
    println!("\n=== 前缀表达式 ===");
    println!("-5 => {}", output);

    assert!(output.contains("-"));
    assert!(output.contains("5"));
}

#[test]
fn test_binary_operator_with_token() {
    // 测试 MIR 的 BinaryOperator
    let op = BinaryOperator::Add;
    let output = op.to_ir_string();
    assert_eq!(output, "+");
}
