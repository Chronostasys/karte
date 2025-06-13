use karte_diagnostics::Span;
use karte_hir::{type_check, BinaryOperator, Expr, Parameter, Statement};

fn main() {
    let span = Span::new(0, 10);

    println!("=== 高级类型推断示例 ===\n");

    // 示例1: Identity函数 - 泛型函数
    // lambda (x) -> x
    let identity = Expr::Lambda {
        params: vec![Parameter::simple("x".to_string())],
        body: Box::new(Expr::Identifier {
            name: "x".to_string(),
            span,
        }),
        span,
    };

    let (id_type, id_diagnostics) = type_check(&identity);
    println!("1. Identity函数类型: {}", id_type);
    println!("   诊断: {} 个", id_diagnostics.len());

    // 示例2: 函数组合
    // lambda (f) -> lambda (g) -> lambda (x) -> f(g(x))
    let compose = Expr::Lambda {
        params: vec![Parameter::simple("f".to_string())],
        body: Box::new(Expr::Lambda {
            params: vec![Parameter::simple("g".to_string())],
            body: Box::new(Expr::Lambda {
                params: vec![Parameter::simple("x".to_string())],
                body: Box::new(Expr::FunctionCall {
                    function: Box::new(Expr::Identifier {
                        name: "f".to_string(),
                        span,
                    }),
                    args: vec![Expr::FunctionCall {
                        function: Box::new(Expr::Identifier {
                            name: "g".to_string(),
                            span,
                        }),
                        args: vec![Expr::Identifier {
                            name: "x".to_string(),
                            span,
                        }],
                        span,
                    }],
                    span,
                }),
                span,
            }),
            span,
        }),
        span,
    };

    let (comp_type, comp_diagnostics) = type_check(&compose);
    println!("2. 函数组合类型: {}", comp_type);
    println!("   诊断: {} 个", comp_diagnostics.len());

    // 示例3: 使用let绑定的复杂表达式
    let complex_block = Expr::Block {
        statements: vec![
            Statement::Let {
                name: "add_one".to_string(),
                value: Expr::Lambda {
                    params: vec![Parameter::simple("n".to_string())],
                    body: Box::new(Expr::BinaryOp {
                        left: Box::new(Expr::Identifier {
                            name: "n".to_string(),
                            span,
                        }),
                        op: BinaryOperator::Add,
                        right: Box::new(Expr::Number { value: 1, span }),
                        span,
                    }),
                    span,
                },
                span,
            },
            Statement::Let {
                name: "double".to_string(),
                value: Expr::Lambda {
                    params: vec![Parameter::simple("x".to_string())],
                    body: Box::new(Expr::BinaryOp {
                        left: Box::new(Expr::Identifier {
                            name: "x".to_string(),
                            span,
                        }),
                        op: BinaryOperator::Multiply,
                        right: Box::new(Expr::Number { value: 2, span }),
                        span,
                    }),
                    span,
                },
                span,
            },
        ],
        final_expr: Some(Box::new(Expr::Lambda {
            params: vec![Parameter::simple("y".to_string())],
            body: Box::new(Expr::FunctionCall {
                function: Box::new(Expr::Identifier {
                    name: "add_one".to_string(),
                    span,
                }),
                args: vec![Expr::FunctionCall {
                    function: Box::new(Expr::Identifier {
                        name: "double".to_string(),
                        span,
                    }),
                    args: vec![Expr::Identifier {
                        name: "y".to_string(),
                        span,
                    }],
                    span,
                }],
                span,
            }),
            span,
        })),
        span,
    };

    let (block_type, block_diagnostics) = type_check(&complex_block);
    println!("3. 复杂块表达式类型: {}", block_type);
    println!("   诊断: {} 个", block_diagnostics.len());

    // 示例4: 错误情况 - 类型不匹配
    let error_expr = Expr::FunctionCall {
        function: Box::new(Expr::Number { value: 42, span }),
        args: vec![Expr::Number { value: 1, span }],
        span,
    };

    let (error_type, error_diagnostics) = type_check(&error_expr);
    println!("4. 错误表达式类型: {}", error_type);
    println!("   诊断: {} 个", error_diagnostics.len());
    if error_diagnostics.has_errors() {
        println!("   错误详情:\n{}", error_diagnostics);
    }

    println!("\n=== 类型推断系统现在支持: ===");
    println!("✓ 多态lambda函数");
    println!("✓ 高阶函数类型推断");
    println!("✓ 类型变量和统一化");
    println!("✓ 复杂的函数组合");
    println!("✓ 约束求解");
    println!("✓ 错误恢复和诊断");
}
