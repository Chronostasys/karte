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
            .contains("Undefined variable: x"));
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
            span: dummy_span(),
        };

        let (result_type, diagnostics) = type_check(&expr);

        assert!(diagnostics.is_empty());
        assert_eq!(
            result_type,
            Type::Function {
                params: vec![Type::Number],
                return_type: Box::new(Type::Number),
            }
        );
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
            params: vec![Parameter::simple("x".to_string()), Parameter::simple("y".to_string())],
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
            .any(|d| d.message.contains("Arity mismatch")));
    }

    #[test]
    fn test_type_check_let_statement() {
        let expr = Expr::Block {
            statements: vec![Statement::Let {
                name: "x".to_string(),
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
            .any(|d| d.message.contains("Cannot call")));
    }
}
