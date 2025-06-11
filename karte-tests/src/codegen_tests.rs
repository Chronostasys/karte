#[cfg(test)]
mod tests {
    use karte_codegen::*;
    use karte_diagnostics::Span;
    use karte_hir::{BinaryOperator, Expr, Statement, UnaryOperator};

    fn dummy_span() -> Span {
        Span::new(0, 1)
    }

    #[test]
    fn test_evaluate_number() {
        let expr = Expr::Number {
            value: 42,
            span: dummy_span(),
        };
        assert_eq!(evaluate(&expr).unwrap(), Value::Number(42));
    }

    #[test]
    fn test_evaluate_addition() {
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
        assert_eq!(evaluate(&expr).unwrap(), Value::Number(3));
    }

    #[test]
    fn test_evaluate_subtraction() {
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::Number {
                value: 5,
                span: dummy_span(),
            }),
            op: BinaryOperator::Subtract,
            right: Box::new(Expr::Number {
                value: 3,
                span: dummy_span(),
            }),
            span: dummy_span(),
        };
        assert_eq!(evaluate(&expr).unwrap(), Value::Number(2));
    }

    #[test]
    fn test_evaluate_multiplication() {
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::Number {
                value: 3,
                span: dummy_span(),
            }),
            op: BinaryOperator::Multiply,
            right: Box::new(Expr::Number {
                value: 4,
                span: dummy_span(),
            }),
            span: dummy_span(),
        };
        assert_eq!(evaluate(&expr).unwrap(), Value::Number(12));
    }

    #[test]
    fn test_evaluate_division() {
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::Number {
                value: 8,
                span: dummy_span(),
            }),
            op: BinaryOperator::Divide,
            right: Box::new(Expr::Number {
                value: 2,
                span: dummy_span(),
            }),
            span: dummy_span(),
        };
        assert_eq!(evaluate(&expr).unwrap(), Value::Number(4));
    }

    #[test]
    fn test_evaluate_division_by_zero() {
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::Number {
                value: 8,
                span: dummy_span(),
            }),
            op: BinaryOperator::Divide,
            right: Box::new(Expr::Number {
                value: 0,
                span: dummy_span(),
            }),
            span: dummy_span(),
        };
        assert!(evaluate(&expr).is_err());
    }

    #[test]
    fn test_evaluate_unary_plus() {
        let expr = Expr::UnaryOp {
            op: UnaryOperator::Plus,
            operand: Box::new(Expr::Number {
                value: 5,
                span: dummy_span(),
            }),
            span: dummy_span(),
        };
        assert_eq!(evaluate(&expr).unwrap(), Value::Number(5));
    }

    #[test]
    fn test_evaluate_unary_minus() {
        let expr = Expr::UnaryOp {
            op: UnaryOperator::Minus,
            operand: Box::new(Expr::Number {
                value: 5,
                span: dummy_span(),
            }),
            span: dummy_span(),
        };
        assert_eq!(evaluate(&expr).unwrap(), Value::Number(-5));
    }

    #[test]
    fn test_lambda_creation() {
        let expr = Expr::Lambda {
            params: vec!["x".to_string()],
            body: Box::new(Expr::Identifier {
                name: "x".to_string(),
                span: dummy_span(),
            }),
            span: dummy_span(),
        };

        if let Value::Function { params, .. } = evaluate(&expr).unwrap() {
            assert_eq!(params, vec!["x"]);
        } else {
            panic!("Expected function value");
        }
    }

    #[test]
    fn test_simple_function_call() {
        // 测试 (|x| x + 1)(5) 应该返回 6
        let lambda = Expr::Lambda {
            params: vec!["x".to_string()],
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
            span: dummy_span(),
        };

        let call = Expr::FunctionCall {
            function: Box::new(lambda),
            args: vec![Expr::Number {
                value: 5,
                span: dummy_span(),
            }],
            span: dummy_span(),
        };

        assert_eq!(evaluate(&call).unwrap(), Value::Number(6));
    }

    #[test]
    fn test_multi_param_function_call() {
        // 测试 (|x, y| x * y)(3, 4) 应该返回 12
        let lambda = Expr::Lambda {
            params: vec!["x".to_string(), "y".to_string()],
            body: Box::new(Expr::BinaryOp {
                left: Box::new(Expr::Identifier {
                    name: "x".to_string(),
                    span: dummy_span(),
                }),
                op: BinaryOperator::Multiply,
                right: Box::new(Expr::Identifier {
                    name: "y".to_string(),
                    span: dummy_span(),
                }),
                span: dummy_span(),
            }),
            span: dummy_span(),
        };

        let call = Expr::FunctionCall {
            function: Box::new(lambda),
            args: vec![
                Expr::Number {
                    value: 3,
                    span: dummy_span(),
                },
                Expr::Number {
                    value: 4,
                    span: dummy_span(),
                },
            ],
            span: dummy_span(),
        };

        assert_eq!(evaluate(&call).unwrap(), Value::Number(12));
    }

    #[test]
    fn test_nested_function_calls() {
        // 测试嵌套调用: (|x| x * 2)((|y| y + 1)(5)) 应该返回 12
        let inner_lambda = Expr::Lambda {
            params: vec!["y".to_string()],
            body: Box::new(Expr::BinaryOp {
                left: Box::new(Expr::Identifier {
                    name: "y".to_string(),
                    span: dummy_span(),
                }),
                op: BinaryOperator::Add,
                right: Box::new(Expr::Number {
                    value: 1,
                    span: dummy_span(),
                }),
                span: dummy_span(),
            }),
            span: dummy_span(),
        };

        let inner_call = Expr::FunctionCall {
            function: Box::new(inner_lambda),
            args: vec![Expr::Number {
                value: 5,
                span: dummy_span(),
            }],
            span: dummy_span(),
        };

        let outer_lambda = Expr::Lambda {
            params: vec!["x".to_string()],
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
            span: dummy_span(),
        };

        let outer_call = Expr::FunctionCall {
            function: Box::new(outer_lambda),
            args: vec![inner_call],
            span: dummy_span(),
        };

        assert_eq!(evaluate(&outer_call).unwrap(), Value::Number(12));
    }

    #[test]
    fn test_simple_let_statement() {
        // 测试 let x = 5; x + 10 应该返回 15
        let expr = Expr::Block {
            statements: vec![Statement::Let {
                name: "x".to_string(),
                value: Expr::Number {
                    value: 5,
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
                    value: 10,
                    span: dummy_span(),
                }),
                span: dummy_span(),
            })),
            span: dummy_span(),
        };

        assert_eq!(evaluate(&expr).unwrap(), Value::Number(15));
    }

    #[test]
    fn test_let_with_lambda() {
        // 测试 let f = |x| x * 2; f(7) 应该返回 14
        let expr = Expr::Block {
            statements: vec![Statement::Let {
                name: "f".to_string(),
                value: Expr::Lambda {
                    params: vec!["x".to_string()],
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
                    span: dummy_span(),
                },
                span: dummy_span(),
            }],
            final_expr: Some(Box::new(Expr::FunctionCall {
                function: Box::new(Expr::Identifier {
                    name: "f".to_string(),
                    span: dummy_span(),
                }),
                args: vec![Expr::Number {
                    value: 7,
                    span: dummy_span(),
                }],
                span: dummy_span(),
            })),
            span: dummy_span(),
        };

        assert_eq!(evaluate(&expr).unwrap(), Value::Number(14));
    }

    #[test]
    fn test_multiple_let_statements() {
        // 测试 let x = 3; let y = 4; x * y 应该返回 12
        let expr = Expr::Block {
            statements: vec![
                Statement::Let {
                    name: "x".to_string(),
                    value: Expr::Number {
                        value: 3,
                        span: dummy_span(),
                    },
                    span: dummy_span(),
                },
                Statement::Let {
                    name: "y".to_string(),
                    value: Expr::Number {
                        value: 4,
                        span: dummy_span(),
                    },
                    span: dummy_span(),
                },
            ],
            final_expr: Some(Box::new(Expr::BinaryOp {
                left: Box::new(Expr::Identifier {
                    name: "x".to_string(),
                    span: dummy_span(),
                }),
                op: BinaryOperator::Multiply,
                right: Box::new(Expr::Identifier {
                    name: "y".to_string(),
                    span: dummy_span(),
                }),
                span: dummy_span(),
            })),
            span: dummy_span(),
        };

        assert_eq!(evaluate(&expr).unwrap(), Value::Number(12));
    }

    #[test]
    fn test_let_with_closure() {
        // 测试闭包捕获: let x = 5; let f = |y| x + y; f(3) 应该返回 8
        let expr = Expr::Block {
            statements: vec![
                Statement::Let {
                    name: "x".to_string(),
                    value: Expr::Number {
                        value: 5,
                        span: dummy_span(),
                    },
                    span: dummy_span(),
                },
                Statement::Let {
                    name: "f".to_string(),
                    value: Expr::Lambda {
                        params: vec!["y".to_string()],
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
                        span: dummy_span(),
                    },
                    span: dummy_span(),
                },
            ],
            final_expr: Some(Box::new(Expr::FunctionCall {
                function: Box::new(Expr::Identifier {
                    name: "f".to_string(),
                    span: dummy_span(),
                }),
                args: vec![Expr::Number {
                    value: 3,
                    span: dummy_span(),
                }],
                span: dummy_span(),
            })),
            span: dummy_span(),
        };

        assert_eq!(evaluate(&expr).unwrap(), Value::Number(8));
    }
}
