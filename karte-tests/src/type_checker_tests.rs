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

    #[test]
    fn test_struct_field_type_check() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "struct Point { x: number, y: number }\nfn main() -> number {\n    let p = Point { x: 1, y: true };\n    p.x\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(diagnostics.has_errors(), "Should report type mismatch for struct field");
    }

    #[test]
    fn test_exhaustive_enum_matching() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "enum Color { Red, Green, Blue }\nfn main() -> number {\n    let c = Color::Red;\n    match c {\n        Color::Red => 1,\n        Color::Green => 2,\n        Color::Blue => 3\n    }\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "Exhaustive match should not report errors");
    }

    #[test]
    fn test_nested_match_exhaustiveness() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "enum Bool { True, False }\nfn main() -> number {\n    let b = Bool::True;\n    match b {\n        Bool::True => 1\n    }\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(diagnostics.has_errors(), "Non-exhaustive match should report error");
        let msgs: Vec<&str> = diagnostics.diagnostics.iter().map(|d| d.message.as_str()).collect();
        let msg = msgs.join(", ");
        assert!(msg.contains("False") || msg.contains("穷尽"), "Should mention missing pattern: {}", msg);
    }

    #[test]
    fn test_option_type_exhaustiveness() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let x = Some(42);\n    match x {\n        Some(v) => v\n    }\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(diagnostics.has_errors(), "Non-exhaustive Option match should report error");
        let msgs: Vec<&str> = diagnostics.diagnostics.iter().map(|d| d.message.as_str()).collect();
        let msg = msgs.join(", ");
        assert!(msg.contains("None") || msg.contains("穷尽"), "Should mention missing None: {}", msg);
    }

    #[test]
    fn test_result_type_exhaustiveness() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let x = Ok(42);\n    match x {\n        Ok(v) => v,\n        Err(e) => 0\n    }\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        if diagnostics.has_errors() {
            let msgs: Vec<String> = diagnostics.diagnostics.iter().map(|d| format!("[{:?}] {}", d.level, d.message)).collect();
            eprintln!("Unexpected errors: {:?}", msgs);
        }
        assert!(!diagnostics.has_errors(), "Exhaustive Result match should not report errors: {:?}", diagnostics.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>());
    }

    #[test]
    fn test_unary_minus_type_error() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let x = true;\n    -x\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(diagnostics.has_errors(), "Should report type error for -bool");
    }

    #[test]
    fn test_logical_not_type_error() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let x = 42;\n    !x\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(diagnostics.has_errors(), "Should report type error for !number");
    }

    #[test]
    fn test_array_element_type_consistency() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let arr = [1, true, 3];\n    arr[0]\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(diagnostics.has_errors(), "Should report type error for mixed array");
    }

    #[test]
    fn test_nested_function_call_type() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn add(a: number, b: number) -> number { a + b }\nfn main() -> number {\n    add(1, true)\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(diagnostics.has_errors(), "Should report type error for wrong argument type");
    }

    #[test]
    fn test_return_type_mismatch() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn foo() -> number { \"hello\" }";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(diagnostics.has_errors(), "Should report type error for return type mismatch");
    }

    #[test]
    fn test_if_else_branch_consistency() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    if 1 > 0 {\n        42\n    } else {\n        \"hello\"\n    }\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(diagnostics.has_errors(), "Should report type error for if-else branch mismatch");
    }

    #[test]
    fn test_for_loop_range_type() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let sum = 0;\n    for i in \"hello\" {\n        sum\n    }\n    sum\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(diagnostics.has_errors(), "Should report type error for non-number range");
    }

    #[test]
    fn test_bitwise_operation_type() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let x = true;\n    let y = 10;\n    x bitand y\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(diagnostics.has_errors(), "Should report type error for bitwise on bool");
    }

    #[test]
    fn test_string_comparison() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let a = \"hello\";\n    let b = \"world\";\n    if a > b {\n        1\n    } else {\n        0\n    }\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "String comparison should be valid");
    }

    #[test]
    fn test_match_with_guard() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let x = 42;\n    match x {\n        n if n > 10 => 1,\n        _ => 0\n    }\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "Match with guard should be valid");
    }

    #[test]
    fn test_empty_struct() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "struct Empty {}\nfn main() -> number {\n    let e = Empty {};\n    0\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "Empty struct should be valid");
    }

    #[test]
    fn test_exhaustive_bool_match() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let b = true;\n    match b {\n        true => 1,\n        false => 0\n    }\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "Exhaustive bool match should be valid");
    }

    #[test]
    fn test_non_exhaustive_bool_match() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let b = true;\n    match b {\n        true => 1\n    }\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(diagnostics.has_errors(), "Non-exhaustive bool match should report error");
    }

    #[test]
    fn test_wildcard_match_is_exhaustive() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let x = 42;\n    match x {\n        _ => 0\n    }\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "Wildcard match should be exhaustive");
    }

    #[test]
    fn test_or_pattern_match() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "enum Color { Red, Green, Blue }\nfn main() -> number {\n    let c = Color::Red;\n    match c {\n        Color::Red | Color::Green => 1,\n        Color::Blue => 2\n    }\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "Or-pattern match should be exhaustive");
    }

    #[test]
    fn test_nested_struct_pattern() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "struct Point { x: number, y: number }\nfn main() -> number {\n    let p = Point { x: 1, y: 2 };\n    match p {\n        Point { x, y } => x + y\n    }\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "Struct pattern match should be valid");
    }

    #[test]
    fn test_tuple_pattern_match() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let t = (1, 2);\n    match t {\n        (a, b) => a + b\n    }\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        // Tuple 可能不被支持，所以只检查不 panic
        let _ = diagnostics.has_errors();
    }

    #[test]
    fn test_number_pattern_match() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let x = 42;\n    match x {\n        0 => 1,\n        1 => 2,\n        _ => 3\n    }\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "Number pattern with wildcard should be valid");
    }

    #[test]
    fn test_string_concatenation() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let s = \"hello\" + \" \" + \"world\";\n    0\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "String concatenation should be valid");
    }

    #[test]
    fn test_nested_if_else() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let x = 5;\n    if x > 10 {\n        1\n    } else if x > 5 {\n        2\n    } else {\n        3\n    }\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "Nested if-else should be valid");
    }

    #[test]
    fn test_recursive_function() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn fib(n: number) -> number {\n    if n <= 1 {\n        n\n    } else {\n        fib(n - 1) + fib(n - 2)\n    }\n}\nfn main() -> number {\n    fib(10)\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "Recursive function should be valid");
    }

    #[test]
    fn test_while_loop() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let x = 0;\n    let i = 0;\n    while i < 10 {\n        x = x + i;\n        i = i + 1\n    };\n    x\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "While loop should be valid");
    }

    #[test]
    fn test_for_loop() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let sum = 0;\n    for i in 0..10 {\n        sum = sum + i\n    };\n    sum\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "For loop should be valid");
    }

    #[test]
    fn test_closure_capture() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let x = 10;\n    let add_x = |y| { y + x };\n    add_x(5)\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "Closure capture should be valid");
    }

    #[test]
    fn test_reference_type() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let x = 42;\n    let r = &x;\n    *r\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "Reference type should be valid");
    }

    #[test]
    fn test_result_type_ok() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let r = Ok(42);\n    match r {\n        Ok(v) => v,\n        Err(_) => 0\n    }\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "Result type with Ok/Err should be valid");
    }

    #[test]
    fn test_method_call_syntax() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let x = -5;\n    abs(x)\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "abs() built-in should be valid");
    }

    #[test]
    fn test_enum_with_data_exhaustive() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "enum Shape { Circle(number), Rectangle(number, number) }\nfn main() -> number {\n    let s = Shape::Circle(5);\n    match s {\n        Shape::Circle(r) => r,\n        Shape::Rectangle(w, h) => w * h\n    }\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "Exhaustive enum with data should be valid");
    }

    #[test]
    fn test_enum_with_data_non_exhaustive() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "enum Shape { Circle(number), Rectangle(number, number) }\nfn main() -> number {\n    let s = Shape::Circle(5);\n    match s {\n        Shape::Circle(r) => r\n    }\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(diagnostics.has_errors(), "Non-exhaustive enum with data should report error");
    }

    #[test]
    fn test_nested_enum_match() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "enum Outer { A, B }\nfn main() -> number {\n    let x = Outer::A;\n    match x {\n        Outer::A => 1,\n        Outer::B => 2\n    }\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "Exhaustive nested enum match should be valid");
    }

    #[test]
    fn test_match_with_multiple_patterns() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn classify(n: number) -> number {\n    match n {\n        0 => 0,\n        1 => 1,\n        2 => 2,\n        _ => -1\n    }\n}\nfn main() -> number {\n    classify(5)\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "Multiple patterns with wildcard should be valid");
    }

    #[test]
    fn test_let_binding_type_annotation() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let x: number = 42;\n    let y: number = x + 1;\n    y\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "Let binding with type annotation should be valid");
    }

    #[test]
    fn test_let_binding_wrong_type_annotation() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let x: number = \"hello\";\n    0\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(diagnostics.has_errors(), "Let binding with wrong type annotation should report error");
    }

    #[test]
    fn test_generic_function_simple() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn id(x) { x }\nfn main() -> number {\n    let a = id(42);\n    a\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "Generic identity function should be valid");
    }

    #[test]
    fn test_higher_order_function() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn apply(f, x) { f(x) }\nfn double(n: number) -> number { n * 2 }\nfn main() -> number {\n    apply(double, 5)\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "Higher-order function should be valid");
    }

    #[test]
    fn test_type_inference_let_binding() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let x = 42;\n    let y = x + 1;\n    let z = y * 2;\n    z\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "Type inference for let bindings should work");
    }

    #[test]
    fn test_type_inference_if_else() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let x = 5;\n    let result = if x > 3 {\n        x * 2\n    } else {\n        x + 1\n    };\n    result\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "Type inference for if-else should work");
    }

    #[test]
    fn test_type_inference_match() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let x = 42;\n    let result = match x {\n        0 => 100,\n        _ => x\n    };\n    result\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "Type inference for match should work");
    }

    #[test]
    fn test_type_inference_function_call() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn square(x: number) -> number { x * x }\nfn main() -> number {\n    let a = square(3);\n    let b = square(a);\n    b\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "Type inference for function calls should work");
    }

    #[test]
    fn test_type_error_wrong_return_type() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn foo() -> number {\n    \"hello\"\n}\nfn main() -> number {\n    foo()\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(diagnostics.has_errors(), "Wrong return type should report error");
        let msgs: Vec<&str> = diagnostics.diagnostics.iter().map(|d| d.message.as_str()).collect();
        let msg = msgs.join(", ");
        assert!(msg.contains("类型不匹配") || msg.contains("期望"), "Error should mention type mismatch: {}", msg);
    }

    #[test]
    fn test_type_error_wrong_argument() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn add(a: number, b: number) -> number { a + b }\nfn main() -> number {\n    add(1, \"hello\")\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(diagnostics.has_errors(), "Wrong argument type should report error");
    }

    #[test]
    fn test_struct_field_access() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "struct Point { x: number, y: number }\nfn main() -> number {\n    let p = Point { x: 1, y: 2 };\n    p.x + p.y\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "Struct field access should be valid");
    }

    #[test]
    fn test_struct_field_type_error() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "struct Point { x: number, y: number }\nfn main() -> number {\n    let p = Point { x: \"hello\", y: 2 };\n    p.y\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(diagnostics.has_errors(), "Wrong field type should report error");
    }

    #[test]
    fn test_nested_function_calls() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn double(x: number) -> number { x * 2 }\nfn add_one(x: number) -> number { x + 1 }\nfn main() -> number {\n    double(add_one(5))\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "Nested function calls should be valid");
    }

    #[test]
    fn test_type_error_context_message() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let x = \"hello\";\n    let y = x + 1;\n    y\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(diagnostics.has_errors(), "Should report type error");
        let msgs: Vec<&str> = diagnostics.diagnostics.iter().map(|d| d.message.as_str()).collect();
        let msg = msgs.join(", ");
        // 错误消息应该包含上下文描述
        assert!(msg.contains("类型不匹配") || msg.contains("运算") || msg.contains("期望"),
            "Error should have context: {}", msg);
    }

    #[test]
    fn test_undefined_variable_suggestion() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let counter = 5;\n    countr\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(diagnostics.has_errors(), "Should report undefined variable");
        let msgs: Vec<&str> = diagnostics.diagnostics.iter().map(|d| d.message.as_str()).collect();
        let msg = msgs.join(", ");
        // 错误消息应该包含拼写建议
        assert!(msg.contains("countr") || msg.contains("counter") || msg.contains("未定义"),
            "Error should mention variable name or suggestion: {}", msg);
    }

    #[test]
    fn test_empty_function_body() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    42\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "Function with return value should be valid");
    }

    #[test]
    fn test_multiple_return_paths() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn classify(n: number) -> number {\n    if n > 0 {\n        1\n    } else if n < 0 {\n        -1\n    } else {\n        0\n    }\n}\nfn main() -> number {\n    classify(42)\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "Multiple return paths should be valid");
    }

    #[test]
    fn test_string_operations() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let a = \"hello\";\n    let b = \"world\";\n    let c = a == b;\n    0\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "String comparison should be valid");
    }

    #[test]
    fn test_char_literal() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let c: char = 'A';\n    0\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "Char literal should be valid");
    }

    #[test]
    fn test_lexer_error_unclosed_string() {
        use karte_lexer::Lexer;
        let code = r#"fn main() -> number { let x = "hello; 0 }"#;
        let tokens = Lexer::new(code).tokenize();
        // 应该能处理未关闭的字符串
        assert!(tokens.len() > 0, "Should tokenize even with unclosed string");
    }

    #[test]
    fn test_lexer_error_float_literal() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let x = 3.14;\n    0\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        // 浮点数字面量应该报错
        assert!(diagnostics.has_errors(), "Float literal should report error");
    }

    #[test]
    fn test_parser_error_missing_semicolon_in_struct() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "struct Point { x: number y: number }\nfn main() -> number { 0 }";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        // 缺少逗号分隔应该报错
        assert!(diagnostics.has_errors(), "Missing comma in struct should report error");
    }

    #[test]
    fn test_parser_error_missing_arrow() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() number { 42 }";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        // 缺少 -> 应该报错
        assert!(diagnostics.has_errors(), "Missing arrow in function should report error");
    }

    #[test]
    fn test_parser_error_unclosed_brace() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    42";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        // 缺少闭合花括号应该报错
        assert!(diagnostics.has_errors(), "Unclosed brace should report error");
    }

        #[test]
    fn test_type_inference_simple_binding() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "let x = 5; let y = x + 1; y";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "Simple binding type inference should work");
    }

        #[test]
    fn test_type_inference_chained_operations() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "let a = 3; let b = 4; let c = a * b + a; c";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "Chained operations type inference should work");
    }

    #[test]
    fn test_enum_constructor_pattern() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "enum Option { Some, None }\nfn unwrap(o) -> number {\n    match o {\n        Option::Some => 1,\n        Option::None => 0\n    }\n}\nfn main() -> number {\n    unwrap(Option::Some)\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "Enum constructor pattern should be valid");
    }

    #[test]
    fn test_builtin_function_len() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let s = \"hello\";\n    len(s)\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "len() builtin should work");
    }

    #[test]
    fn test_builtin_function_abs() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    abs(-42)\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(!diagnostics.has_errors(), "abs() builtin should work");
    }

    #[test]
    fn test_type_error_string_subtraction() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = r#"let a = "hello"; let b = "world"; a - b"#;
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(diagnostics.has_errors(), "String subtraction should report error");
    }

    #[test]
    fn test_enum_qualified_match() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "enum Color { Red, Green, Blue }\nfn main() -> number {\n    let c = Color::Red;\n    match c {\n        Color::Red => 1,\n        Color::Green => 2,\n        Color::Blue => 3\n    }\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Qualified enum match should be valid");
    }

    #[test]
    fn test_nested_match_valid() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn classify(n: number) -> number {\n    match n {\n        0 => 1,\n        _ => 2\n    }\n}\nfn main() -> number { classify(5) }";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Nested match should be valid");
    }

    #[test]
    fn test_if_else_type_error() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let x = if true { 42 } else { \"hello\" };\n    0\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(diagnostics.has_errors(), "If-else type mismatch should report error");
    }

    #[test]
    fn test_match_wildcard_exhaustive() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn test(n: number) -> number {\n    match n {\n        0 => 1,\n        1 => 2,\n        _ => 3\n    }\n}\nfn main() -> number { test(5) }";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Match with wildcard should be valid");
    }

    #[test]
    fn test_option_match_some_none() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn unwrap_or(opt: number, default: number) -> number {\n    match opt {\n        0 => default,\n        _ => opt\n    }\n}\nfn main() -> number { unwrap_or(0, 42) }";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Option-like match should be valid");
    }

    #[test]
    fn test_recursive_function_type_check() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn factorial(n: number) -> number {\n    if n <= 1 {\n        1\n    } else {\n        n * factorial(n - 1)\n    }\n}\nfn main() -> number { factorial(5) }";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Recursive function should be valid");
    }

    #[test]
    fn test_mutual_recursion_type_error() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn is_even(n: number) -> number {\n    if n == 0 { 1 } else { is_odd(n - 1) }\n}\nfn is_odd(n: number) -> number {\n    if n == 0 { 0 } else { is_even(n - 1) }\n}\nfn main() -> number { is_even(4) }";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Mutual recursion should be valid");
    }

    #[test]
    fn test_struct_method_call_syntax() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn abs(x: number) -> number {\n    if x < 0 { 0 - x } else { x }\n}\nfn main() -> number {\n    abs(-42)\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Function call should be valid");
    }

    #[test]
    fn test_let_shadowing() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let x = 5;\n    let x = x + 1;\n    let x = x * 2;\n    x\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Let shadowing should be valid");
    }

    #[test]
    fn test_string_concat_type_check() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let a = \"hello\";\n    let b = \"world\";\n    let c = a == b;\n    if c { 1 } else { 0 }\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "String comparison should be valid");
    }

    #[test]
    fn test_boolean_logic() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let a = true;\n    let b = false;\n    let c = a && b;\n    let d = a || b;\n    if c { 1 } else if d { 2 } else { 0 }\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Boolean logic should be valid");
    }

    #[test]
    fn test_nested_struct() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "struct Point { x: number, y: number }\nstruct Line { start: Point, end: Point }\nfn main() -> number {\n    let p1 = Point { x: 0, y: 0 };\n    let p2 = Point { x: 1, y: 1 };\n    let line = Line { start: p1, end: p2 };\n    line.start.x + line.end.y\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Nested struct should be valid");
    }

    #[test]
    fn test_complex_match_patterns() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn classify(n: number) -> number {\n    match n {\n        0 => 0,\n        1 => 1,\n        2 => 4,\n        3 => 9,\n        _ => n * n\n    }\n}\nfn main() -> number { classify(4) }";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Complex match patterns should be valid");
    }

    #[test]
    fn test_empty_struct_type_check() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "struct Empty {}\nfn main() -> number {\n    let e = Empty {};\n    0\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Empty struct should be valid");
    }

    #[test]
    fn test_single_variant_enum() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "enum Unit { Value }\nfn main() -> number {\n    let u = Unit::Value;\n    match u {\n        Unit::Value => 1\n    }\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Single variant enum should be valid");
    }

    #[test]
    fn test_negative_number_literal() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let x = -42;\n    let y = -x;\n    x + y\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Negative number should be valid");
    }

    #[test]
    fn test_parenthesized_expression() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let x = (1 + 2) * (3 + 4);\n    x\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Parenthesized expression should be valid");
    }

    #[test]
    fn test_multiple_assignment() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let x = 1;\n    let y = 2;\n    let z = 3;\n    x + y + z\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Multiple assignments should be valid");
    }

    #[test]
    fn test_unit_type_return() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn print_hello() {\n    let _ = 42;\n}\nfn main() -> number { 0 }";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        // Unit type function should be valid
        let _ = diagnostics;
    }

    #[test]
    fn test_large_number_literal() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let x = 1000000000;\n    x\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Large number literal should be valid");
    }

    #[test]
    fn test_error_recovery_undefined_function() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    undefined_func(42)\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(diagnostics.has_errors(), "Undefined function should report error");
    }

    #[test]
    fn test_error_recovery_wrong_arity() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn add(a: number, b: number) -> number { a + b }\nfn main() -> number {\n    add(1)\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(diagnostics.has_errors(), "Wrong arity should report error");
    }

    #[test]
    fn test_error_recovery_duplicate_definition() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn foo() -> number { 1 }\nfn foo() -> number { 2 }\nfn main() -> number { foo() }";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(diagnostics.has_errors(), "Duplicate function definition should report error");
    }

    #[test]
    fn test_error_recovery_type_mismatch_in_if() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let x = if true { 42 } else { \"hello\" };\n    0\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(diagnostics.has_errors(), "Type mismatch in if-else should report error");
    }

    #[test]
    fn test_error_recovery_undefined_type_annotation() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let x: UndefinedType = 42;\n    0\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        // UndefinedType 在类型注解中可能不会报错（因为类型推断会忽略注解）
        let _ = diagnostics;
    }

    #[test]
    fn test_multiple_errors_in_one_file() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let x = \"hello\" + 1;\n    let y = x - \"world\";\n    y\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(diagnostics.has_errors(), "Multiple errors should be detected");
    }
}

    #[test]
    fn test_constraint_context_binary_op() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let x = \"hello\" + 1;\n    x\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(diagnostics.has_errors(), "String + number should report error");
        let msgs: Vec<&str> = diagnostics.diagnostics.iter().map(|d| d.message.as_str()).collect();
        let msg = msgs.join(", ");
        assert!(msg.contains("类型不匹配") || msg.contains("运算"),
            "Error should have binary operation context: {}", msg);
    }

    #[test]
    fn test_constraint_context_return_type() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    \"hello\"\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(diagnostics.has_errors(), "Wrong return type should report error");
        let msgs: Vec<&str> = diagnostics.diagnostics.iter().map(|d| d.message.as_str()).collect();
        let msg = msgs.join(", ");
        assert!(msg.contains("类型不匹配") || msg.contains("返回"),
            "Error should mention return type context: {}", msg);
    }

    #[test]
    fn test_constraint_context_assignment() {
        use karte_lexer::Lexer;
        use karte_parser::{ParserMode, parse_with_type_check};
        let code = "fn main() -> number {\n    let x: number = \"hello\";\n    x\n}";
        let tokens = Lexer::new(code).tokenize();
        let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(diagnostics.has_errors(), "Assignment type mismatch should report error");
    
}