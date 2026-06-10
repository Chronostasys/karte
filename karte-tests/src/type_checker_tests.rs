#[cfg(test)]
mod tests {
    use crate::dummy_span;
    use karte_hir::*;

    #[test]
    fn test_type_check_number() {
        let expr = Expr::Number {
            value: 42,
            span: dummy_span(),
        };
        let (result_type, diagnostics) = type_check(&expr);

        assert!(diagnostics.is_empty());
        assert_eq!(result_type, Type::Number);
    }

    #[test]
    fn test_type_check_binary_op() {
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::Number {
                value: 1,
                span: dummy_span(),
            }),
            op: BinaryOperator::Add,
            right: Box::new(Expr::Number {
                value: 2,
                span: dummy_span(),
            }),
            span: dummy_span(),
        };

        let (result_type, diagnostics) = type_check(&expr);

        assert!(diagnostics.is_empty());
        assert_eq!(result_type, Type::Number);
    }

    #[test]
    fn test_type_check_undefined_variable() {
        let expr = Expr::Identifier {
            name: "x".to_string(),
            span: dummy_span(),
        };

        let (result_type, diagnostics) = type_check(&expr);

        assert!(diagnostics.has_errors());
        assert_eq!(result_type, Type::Unknown);

        // 检查错误信息
        assert_eq!(diagnostics.diagnostics.len(), 1);
        assert!(diagnostics.diagnostics[0]
            .message
            .contains("未定义的变量: x"));
    }

    #[test]
    fn test_type_check_lambda() {
        let expr = Expr::Lambda {
            params: vec![Parameter::simple("x".to_string())],
            body: Box::new(Expr::BinaryOp {
                left: Box::new(Expr::Identifier {
                    name: "x".to_string(),
                    span: dummy_span(),
                }),
                op: BinaryOperator::Add,
                right: Box::new(Expr::Number {
                    value: 1,
                    span: dummy_span(),
                }),
                span: dummy_span(),
            }),
            return_type: None,
            inferred_type: None,
            span: dummy_span(),
        };

        let (result_type, diagnostics) = type_check(&expr);

        assert!(diagnostics.is_empty());
        // Expect a closure returning Number; parameter may still be a type variable
        match result_type {
            Type::Closure {
                params,
                return_type,
            } => {
                assert_eq!(params.len(), 1);
                // param may be inferred to Type::Number or still a Type::Var during inference
                match &params[0] {
                    Type::Number | Type::Var(_) => {}
                    other => panic!("Expected param Number or Var, got {:?}", other),
                }
                assert_eq!(*return_type, Type::Number);
            }
            other => panic!("Expected Closure, got {:?}", other),
        }
    }

    #[test]
    fn test_type_check_function_call() {
        let lambda = Expr::Lambda {
            params: vec![Parameter::simple("x".to_string())],
            body: Box::new(Expr::BinaryOp {
                left: Box::new(Expr::Identifier {
                    name: "x".to_string(),
                    span: dummy_span(),
                }),
                op: BinaryOperator::Multiply,
                right: Box::new(Expr::Number {
                    value: 2,
                    span: dummy_span(),
                }),
                span: dummy_span(),
            }),
            return_type: None,
            inferred_type: None,
            span: dummy_span(),
        };

        let expr = Expr::FunctionCall {
            function: Box::new(lambda),
            args: vec![Expr::Number {
                value: 5,
                span: dummy_span(),
            }],
            span: dummy_span(),
        };

        let (result_type, diagnostics) = type_check(&expr);

        assert!(diagnostics.is_empty());
        assert_eq!(result_type, Type::Number);
    }

    #[test]
    fn test_type_check_arity_mismatch() {
        let lambda = Expr::Lambda {
            params: vec![
                Parameter::simple("x".to_string()),
                Parameter::simple("y".to_string()),
            ],
            body: Box::new(Expr::BinaryOp {
                left: Box::new(Expr::Identifier {
                    name: "x".to_string(),
                    span: dummy_span(),
                }),
                op: BinaryOperator::Add,
                right: Box::new(Expr::Identifier {
                    name: "y".to_string(),
                    span: dummy_span(),
                }),
                span: dummy_span(),
            }),
            return_type: None,
            inferred_type: None,
            span: dummy_span(),
        };

        let expr = Expr::FunctionCall {
            function: Box::new(lambda),
            args: vec![Expr::Number {
                value: 5,
                span: dummy_span(),
            }], // 只有一个参数，但函数需要两个
            span: dummy_span(),
        };

        let (result_type, diagnostics) = type_check(&expr);

        assert!(diagnostics.has_errors());
        assert_eq!(result_type, Type::Number); // 返回类型仍然正确，但有错误

        // 检查错误信息
        assert!(diagnostics
            .diagnostics
            .iter()
            .any(|d| d.message.contains("参数数量不匹配")));
    }

    #[test]
    fn test_type_check_let_statement() {
        let expr = Expr::Block {
            statements: vec![Statement::Let {
            pattern: None,
                name: "x".to_string(),
                type_annotation: None,
                value: Expr::Number {
                    value: 42,
                    span: dummy_span(),
                },
                span: dummy_span(),
            }],
            final_expr: Some(Box::new(Expr::BinaryOp {
                left: Box::new(Expr::Identifier {
                    name: "x".to_string(),
                    span: dummy_span(),
                }),
                op: BinaryOperator::Add,
                right: Box::new(Expr::Number {
                    value: 8,
                    span: dummy_span(),
                }),
                span: dummy_span(),
            })),
            span: dummy_span(),
        };

        let (result_type, diagnostics) = type_check(&expr);

        assert!(diagnostics.is_empty());
        assert_eq!(result_type, Type::Number);
    }

    #[test]
    fn test_type_check_not_callable() {
        let expr = Expr::FunctionCall {
            function: Box::new(Expr::Number {
                value: 42,
                span: dummy_span(),
            }),
            args: vec![Expr::Number {
                value: 5,
                span: dummy_span(),
            }],
            span: dummy_span(),
        };

        let (result_type, diagnostics) = type_check(&expr);

        assert!(diagnostics.has_errors());
        assert_eq!(result_type, Type::Unknown);

        // 检查错误信息
        assert!(diagnostics
            .diagnostics
            .iter()
            .any(|d| d.message.contains("无法调用")));
    }

    // ========================================
    // 类型标注检查测试
    // ========================================

    #[test]
    fn test_function_return_type_annotation_mismatch() {
        // fn foo() -> number { true }
        // 声明返回 number，但实际返回 bool，应该报错
        let expr = Expr::Block {
            statements: vec![Statement::FunctionDef {
                name: "foo".to_string(),
                params: vec![],
                return_type: Some(Type::Number),
                body: Expr::Boolean {
                    value: true,
                    span: dummy_span(),
                },
                is_pub: false,
                span: dummy_span(),
            }],
            final_expr: None,
            span: dummy_span(),
        };

        let (_, diagnostics) = type_check(&expr);

        // 应该有类型不匹配错误
        assert!(diagnostics.has_errors());
        assert!(diagnostics
            .diagnostics
            .iter()
            .any(|d| d.message.contains("类型不匹配")));
    }

    #[test]
    fn test_function_param_type_annotation_enforced() {
        // fn foo(x: number) -> number { x + 1 }
        // 这应该成功
        let expr = Expr::Block {
            statements: vec![Statement::FunctionDef {
                name: "foo".to_string(),
                params: vec![Parameter {
                    name: "x".to_string(),
                    type_annotation: Some(Type::Number),
                    span: dummy_span(),
                }],
                return_type: Some(Type::Number),
                body: Expr::BinaryOp {
                    left: Box::new(Expr::Identifier {
                        name: "x".to_string(),
                        span: dummy_span(),
                    }),
                    op: BinaryOperator::Add,
                    right: Box::new(Expr::Number {
                        value: 1,
                        span: dummy_span(),
                    }),
                    span: dummy_span(),
                },
                is_pub: false,
                span: dummy_span(),
            }],
            final_expr: None,
            span: dummy_span(),
        };

        let (_, diagnostics) = type_check(&expr);

        // 不应该有错误
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_function_unknown_type_annotation_compatible() {
        // fn foo(x: Unknown) -> number { 42 }
        // Type::Unknown 作为类型标注，type checker 不会报错（Unknown 与任何类型兼容）
        let expr = Expr::Block {
            statements: vec![Statement::FunctionDef {
                name: "foo".to_string(),
                params: vec![Parameter {
                    name: "x".to_string(),
                    type_annotation: Some(Type::Unknown),
                    span: dummy_span(),
                }],
                return_type: Some(Type::Number),
                body: Expr::Number {
                    value: 42,
                    span: dummy_span(),
                },
                is_pub: false,
                span: dummy_span(),
            }],
            final_expr: None,
            span: dummy_span(),
        };

        let (_, diagnostics) = type_check(&expr);

        // Type::Unknown 不报错，因为 Unknown 与任何类型兼容
        assert!(!diagnostics.has_errors());
    }

    #[test]
    fn test_function_return_type_correct() {
        // fn foo() -> number { 42 }
        // 声明返回 number，实际也返回 number，应该成功
        let expr = Expr::Block {
            statements: vec![Statement::FunctionDef {
                name: "foo".to_string(),
                params: vec![],
                return_type: Some(Type::Number),
                body: Expr::Number {
                    value: 42,
                    span: dummy_span(),
                },
                is_pub: false,
                span: dummy_span(),
            }],
            final_expr: None,
            span: dummy_span(),
        };

        let (_, diagnostics) = type_check(&expr);

        // 不应该有错误
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_function_multiple_params_type_annotations() {
        // fn add(x: number, y: number) -> number { x + y }
        let expr = Expr::Block {
            statements: vec![Statement::FunctionDef {
                name: "add".to_string(),
                params: vec![
                    Parameter {
                        name: "x".to_string(),
                        type_annotation: Some(Type::Number),
                        span: dummy_span(),
                    },
                    Parameter {
                        name: "y".to_string(),
                        type_annotation: Some(Type::Number),
                        span: dummy_span(),
                    },
                ],
                return_type: Some(Type::Number),
                body: Expr::BinaryOp {
                    left: Box::new(Expr::Identifier {
                        name: "x".to_string(),
                        span: dummy_span(),
                    }),
                    op: BinaryOperator::Add,
                    right: Box::new(Expr::Identifier {
                        name: "y".to_string(),
                        span: dummy_span(),
                    }),
                    span: dummy_span(),
                },
                is_pub: false,
                span: dummy_span(),
            }],
            final_expr: None,
            span: dummy_span(),
        };

        let (_, diagnostics) = type_check(&expr);

        // 不应该有错误
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_function_return_type_mismatch_complex() {
        // fn foo(x: number) -> number {
        //     if x > 0 { true } else { false }
        // }
        // 声明返回 number，但实际返回 bool
        let expr = Expr::Block {
            statements: vec![Statement::FunctionDef {
                name: "foo".to_string(),
                params: vec![Parameter {
                    name: "x".to_string(),
                    type_annotation: Some(Type::Number),
                    span: dummy_span(),
                }],
                return_type: Some(Type::Number),
                body: Expr::If {
                    condition: Box::new(Expr::BinaryOp {
                        left: Box::new(Expr::Identifier {
                            name: "x".to_string(),
                            span: dummy_span(),
                        }),
                        op: BinaryOperator::Greater,
                        right: Box::new(Expr::Number {
                            value: 0,
                            span: dummy_span(),
                        }),
                        span: dummy_span(),
                    }),
                    then_branch: Box::new(Expr::Boolean {
                        value: true,
                        span: dummy_span(),
                    }),
                    else_branch: Some(Box::new(Expr::Boolean {
                        value: false,
                        span: dummy_span(),
                    })),
                    span: dummy_span(),
                },
                is_pub: false,
                span: dummy_span(),
            }],
            final_expr: None,
            span: dummy_span(),
        };

        let (_, diagnostics) = type_check(&expr);

        // 应该有类型不匹配错误
        assert!(diagnostics.has_errors());
        assert!(diagnostics
            .diagnostics
            .iter()
            .any(|d| d.message.contains("类型不匹配")));
    }

    #[test]
    fn test_function_no_annotation_infers_correctly() {
        // fn foo(x) { x + 1 }
        // 没有类型标注，应该能正确推断
        let expr = Expr::Block {
            statements: vec![Statement::FunctionDef {
                name: "foo".to_string(),
                params: vec![Parameter::simple("x".to_string())],
                return_type: None,
                body: Expr::BinaryOp {
                    left: Box::new(Expr::Identifier {
                        name: "x".to_string(),
                        span: dummy_span(),
                    }),
                    op: BinaryOperator::Add,
                    right: Box::new(Expr::Number {
                        value: 1,
                        span: dummy_span(),
                    }),
                    span: dummy_span(),
                },
                is_pub: false,
                span: dummy_span(),
            }],
            final_expr: None,
            span: dummy_span(),
        };

        let (_, diagnostics) = type_check(&expr);

        // 不应该有错误
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_exhaustiveness_bool_match_complete() {
        use crate::dummy_span;
        use karte_hir::*;

        let expr = Expr::Match {
            expr: Box::new(Expr::Boolean {
                value: true,
                span: dummy_span(),
            }),
            arms: vec![
                MatchArm {
                    pattern: Pattern::Boolean {
                        value: true,
                        span: dummy_span(),
                    },
                    guard: None,
                    body: Expr::Number {
                        value: 1,
                        span: dummy_span(),
                    },
                    span: dummy_span(),
                },
                MatchArm {
                    pattern: Pattern::Boolean {
                        value: false,
                        span: dummy_span(),
                    },
                    guard: None,
                    body: Expr::Number {
                        value: 0,
                        span: dummy_span(),
                    },
                    span: dummy_span(),
                },
            ],
            span: dummy_span(),
        };

        let (_, diagnostics) = type_check(&expr);
        // bool 穷尽，不应该有 NonExhaustiveMatch 错误
        assert!(diagnostics
            .diagnostics
            .iter()
            .all(|d| !d.message.contains("非穷尽 match")));
    }

    #[test]
    fn test_exhaustiveness_bool_match_incomplete() {
        use crate::dummy_span;
        use karte_hir::*;

        let expr = Expr::Match {
            expr: Box::new(Expr::Boolean {
                value: true,
                span: dummy_span(),
            }),
            arms: vec![MatchArm {
                pattern: Pattern::Boolean {
                    value: true,
                    span: dummy_span(),
                },
                guard: None,
                body: Expr::Number {
                    value: 1,
                    span: dummy_span(),
                },
                span: dummy_span(),
            }],
            span: dummy_span(),
        };

        let (_, diagnostics) = type_check(&expr);
        // bool 非穷尽（缺少 false），应该报错
        assert!(diagnostics
            .diagnostics
            .iter()
            .any(|d| d.message.contains("非穷尽 match")));
    }

    #[test]
    fn test_exhaustiveness_wildcard_always_exhaustive() {
        use crate::dummy_span;
        use karte_hir::*;

        let expr = Expr::Match {
            expr: Box::new(Expr::Boolean {
                value: true,
                span: dummy_span(),
            }),
            arms: vec![MatchArm {
                pattern: Pattern::Wildcard {
                    span: dummy_span(),
                },
                guard: None,
                body: Expr::Number {
                    value: 1,
                    span: dummy_span(),
                },
                span: dummy_span(),
            }],
            span: dummy_span(),
        };

        let (_, diagnostics) = type_check(&expr);
        // wildcard 总是穷尽
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_exhaustiveness_option_some_none() {
        use karte_lexer::Lexer;
        use karte_parser::{Parser, ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let x = Some(42);\n    match x {\n        Some(n) => n,\n        None => 0\n    }\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "Unexpected errors: {:?}", diagnostics);
    }

    #[test]
    fn test_exhaustiveness_option_missing_none() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let x = Some(42);\n    match x {\n        Some(n) => n\n    }\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(diagnostics.has_errors(), "Should report non-exhaustive match");
        let msg: Vec<&str> = diagnostics.diagnostics.iter().map(|d| d.message.as_str()).collect();
        let msg = msg.join(", ");
        assert!(msg.contains("None") || msg.contains("穷尽"), "Error should mention missing None: {}", msg);
    }

    #[test]
    fn test_exhaustiveness_bool_true_false() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let b = true;\n    match b {\n        true => 1,\n        false => 0\n    }\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "Unexpected errors: {:?}", diagnostics);
    }

    #[test]
    fn test_exhaustiveness_bool_wildcard() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let b = true;\n    match b {\n        true => 1,\n        _ => 0\n    }\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "Unexpected errors: {:?}", diagnostics);
    }

    #[test]
    fn test_exhaustiveness_enum_all_variants() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "enum Color { Red, Green, Blue }\nfn main() -> number {\n    let c = Color::Red;\n    match c {\n        Color::Red => 1,\n        Color::Green => 2,\n        Color::Blue => 3\n    }\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "Unexpected errors: {:?}", diagnostics);
    }

    #[test]
    fn test_exhaustiveness_enum_missing_variant() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "enum Color { Red, Green, Blue }\nfn main() -> number {\n    let c = Color::Red;\n    match c {\n        Color::Red => 1,\n        Color::Green => 2\n    }\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(diagnostics.has_errors(), "Should report non-exhaustive match");
        let msg: Vec<&str> = diagnostics.diagnostics.iter().map(|d| d.message.as_str()).collect();
        let msg = msg.join(", ");
        assert!(msg.contains("Blue") || msg.contains("穷尽"), "Error should mention missing Blue: {}", msg);
    }

    #[test]
    fn test_type_error_string_plus_number() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let x = \"hello\";\n    x + 1\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(diagnostics.has_errors(), "Should report type error");
    }

    #[test]
    fn test_type_error_if_else_mismatch() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let x = 1;\n    if x > 0 {\n        \"positive\"\n    } else {\n        0\n    }\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(diagnostics.has_errors(), "Should report type error for if-else mismatch");
    }

    #[test]
    fn test_type_error_undefined_variable_suggestion() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let count = 5;\n    counr\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(diagnostics.has_errors(), "Should report undefined variable");
        let msg: Vec<&str> = diagnostics.diagnostics.iter().map(|d| d.message.as_str()).collect();
        let msg = msg.join(", ");
        assert!(msg.contains("counr") || msg.contains("count"), "Error should mention variable name: {}", msg);
    }

    #[test]
    fn test_type_error_duplicate_function() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn foo() -> number { 1 }\nfn foo() -> number { 2 }\nfn main() -> number { foo() }";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(diagnostics.has_errors(), "Should report duplicate function");
    }
}
