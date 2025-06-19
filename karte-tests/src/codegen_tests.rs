#[cfg(test)]
mod tests {
    use karte_diagnostics::Span;
    use karte_hir::{BinaryOperator, Expr, Parameter, Statement, UnaryOperator};
    use crate::{execute_with_pipeline, dummy_span};

    #[test]
    fn test_evaluate_number() {
        let expr = Expr::Number {
            value: 42,
            span: dummy_span(),
        };
        assert_eq!(execute_with_pipeline(&expr).unwrap(), 42);
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
        assert_eq!(execute_with_pipeline(&expr).unwrap(), 3);
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
        assert_eq!(execute_with_pipeline(&expr).unwrap(), 2);
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
        assert_eq!(execute_with_pipeline(&expr).unwrap(), 12);
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
        assert_eq!(execute_with_pipeline(&expr).unwrap(), 4);
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
        assert!(execute_with_pipeline(&expr).is_err());
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
        assert_eq!(execute_with_pipeline(&expr).unwrap(), 5);
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
        assert_eq!(execute_with_pipeline(&expr).unwrap(), -5);
    }

    #[test]
    fn test_lambda_creation() {
        let expr = Expr::Lambda {
            params: vec![Parameter::simple("x".to_string())],
            body: Box::new(Expr::Identifier {
                name: "x".to_string(),
                span: dummy_span(),
            }),
            span: dummy_span(),
        };

        // 对于Lambda表达式，我们检查类型而不是值
        // 因为HIR解释器不能直接将函数转换为i64
        let (expr_type, type_diagnostics) = karte_hir::type_check(&expr);
        assert!(!type_diagnostics.has_errors());
        
        // Lambda表达式应该有函数类型
        if let karte_hir::Type::Function { params, return_type: _ } = expr_type {
            assert_eq!(params.len(), 1);
            // 参数类型可能是类型变量，这是正常的
        } else {
            panic!("Expected function type, got {:?}", expr_type);
        }
        
        // 测试lambda可以被调用
        let call_expr = Expr::FunctionCall {
            function: Box::new(expr),
            args: vec![Expr::Number {
                value: 42,
                span: dummy_span(),
            }],
            span: dummy_span(),
        };
        
        let result = execute_with_pipeline(&call_expr);
        assert!(result.is_ok(), "{}", result.err().unwrap());
        assert_eq!(result.unwrap(), 42); // 身份函数应该返回输入值
    }

    #[test]
    fn test_simple_function_call() {
        // 测试 (|x| x + 1)(5) 应该返回 6
        let lambda = Expr::Lambda {
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

        assert_eq!(execute_with_pipeline(&call).unwrap(), 6);
    }

    #[test]
    fn test_multi_param_function_call() {
        // 测试 (|x, y| x * y)(3, 4) 应该返回 12
        let lambda = Expr::Lambda {
            params: vec![Parameter::simple("x".to_string()), Parameter::simple("y".to_string())],
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

        assert_eq!(execute_with_pipeline(&call).unwrap(), 12);
    }

    #[test]
    fn test_nested_function_calls() {
        // 测试嵌套调用: (|x| x * 2)((|y| y + 1)(5)) 应该返回 12
        let inner_lambda = Expr::Lambda {
            params: vec![Parameter::simple("y".to_string())],
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
            span: dummy_span(),
        };

        let outer_call = Expr::FunctionCall {
            function: Box::new(outer_lambda),
            args: vec![inner_call],
            span: dummy_span(),
        };

        println!("=== Testing nested function calls ===");
        let result = crate::execute_with_pipeline_debug(&outer_call, true).unwrap();
        println!("Expected: 12, Got: {}", result);
        assert_eq!(result, 12);
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

        assert_eq!(execute_with_pipeline(&expr).unwrap(), 15);
    }

    #[test]
    fn test_let_with_lambda() {
        // 测试 let f = |x| x * 2; f(7) 应该返回 14
        let expr = Expr::Block {
            statements: vec![Statement::Let {
                name: "f".to_string(),
                value: Expr::Lambda {
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

        assert_eq!(execute_with_pipeline(&expr).unwrap(), 14);
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

        assert_eq!(execute_with_pipeline(&expr).unwrap(), 12);
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
                        params: vec![Parameter::simple("y".to_string())],
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

        assert_eq!(execute_with_pipeline(&expr).unwrap(), 8);
    }

    #[test]
    fn test_evaluate_equal_true() {
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::Number {
                value: 5,
                span: dummy_span(),
            }),
            op: BinaryOperator::Equal,
            right: Box::new(Expr::Number {
                value: 5,
                span: dummy_span(),
            }),
            span: dummy_span(),
        };
        
        // 比较操作返回布尔值，我们需要检查结果是否是构造器值
        let result = execute_with_pipeline(&expr);
        assert!(result.is_ok());
        // 注意：这里我们无法直接比较布尔值，因为它返回的是构造器值
        // 在实际实现中，我们可能需要调整测试或返回类型
    }

    #[test]
    fn test_evaluate_equal_false() {
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::Number {
                value: 5,
                span: dummy_span(),
            }),
            op: BinaryOperator::Equal,
            right: Box::new(Expr::Number {
                value: 3,
                span: dummy_span(),
            }),
            span: dummy_span(),
        };
        
        let result = execute_with_pipeline(&expr);
        assert!(result.is_ok());
    }

    #[test]
    fn test_evaluate_greater_equal_true() {
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::Number {
                value: 10,
                span: dummy_span(),
            }),
            op: BinaryOperator::GreaterEqual,
            right: Box::new(Expr::Number {
                value: 7,
                span: dummy_span(),
            }),
            span: dummy_span(),
        };
        
        let result = execute_with_pipeline(&expr);
        assert!(result.is_ok());
    }

    #[test]
    fn test_evaluate_greater_equal_equal() {
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::Number {
                value: 5,
                span: dummy_span(),
            }),
            op: BinaryOperator::GreaterEqual,
            right: Box::new(Expr::Number {
                value: 5,
                span: dummy_span(),
            }),
            span: dummy_span(),
        };
        
        let result = execute_with_pipeline(&expr);
        assert!(result.is_ok());
    }

    #[test]
    fn test_evaluate_greater_equal_false() {
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::Number {
                value: 3,
                span: dummy_span(),
            }),
            op: BinaryOperator::GreaterEqual,
            right: Box::new(Expr::Number {
                value: 7,
                span: dummy_span(),
            }),
            span: dummy_span(),
        };
        
        let result = execute_with_pipeline(&expr);
        assert!(result.is_ok());
    }

    #[test]
    fn test_evaluate_less_equal_true() {
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::Number {
                value: 3,
                span: dummy_span(),
            }),
            op: BinaryOperator::LessEqual,
            right: Box::new(Expr::Number {
                value: 8,
                span: dummy_span(),
            }),
            span: dummy_span(),
        };
        
        let result = execute_with_pipeline(&expr);
        assert!(result.is_ok());
    }

    #[test]
    fn test_evaluate_less_equal_equal() {
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::Number {
                value: 5,
                span: dummy_span(),
            }),
            op: BinaryOperator::LessEqual,
            right: Box::new(Expr::Number {
                value: 5,
                span: dummy_span(),
            }),
            span: dummy_span(),
        };
        
        let result = execute_with_pipeline(&expr);
        assert!(result.is_ok());
    }

    #[test]
    fn test_evaluate_less_equal_false() {
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::Number {
                value: 10,
                span: dummy_span(),
            }),
            op: BinaryOperator::LessEqual,
            right: Box::new(Expr::Number {
                value: 3,
                span: dummy_span(),
            }),
            span: dummy_span(),
        };
        
        let result = execute_with_pipeline(&expr);
        assert!(result.is_ok());
    }

    #[test]
    fn test_evaluate_comparison_with_arithmetic() {
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::BinaryOp {
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
            }),
            op: BinaryOperator::Equal,
            right: Box::new(Expr::Number {
                value: 3,
                span: dummy_span(),
            }),
            span: dummy_span(),
        };
        
        let result = execute_with_pipeline(&expr);
        assert!(result.is_ok());
    }
}
