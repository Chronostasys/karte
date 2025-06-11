#[cfg(test)]
mod tests {
    use karte_lexer::tokenize;
    use karte_parser::*;

    #[test]
    fn test_parse_simple_number() {
        let (tokens, _) = tokenize("42");
        let (expr, diagnostics) = parse(&tokens);

        assert!(diagnostics.is_empty());
        assert!(expr.is_some());

        if let Some(Expr::Number { value, .. }) = expr {
            assert_eq!(value, 42);
        } else {
            panic!("Expected number expression");
        }
    }

    #[test]
    fn test_parse_addition() {
        let (tokens, _) = tokenize("1 + 2");
        let (expr, diagnostics) = parse(&tokens);

        assert!(diagnostics.is_empty());
        assert!(expr.is_some());

        if let Some(Expr::BinaryOp {
            left, op, right, ..
        }) = expr
        {
            assert!(matches!(left.as_ref(), Expr::Number { value: 1, .. }));
            assert_eq!(op, BinaryOperator::Add);
            assert!(matches!(right.as_ref(), Expr::Number { value: 2, .. }));
        } else {
            panic!("Expected binary operation");
        }
    }

    #[test]
    fn test_parse_precedence() {
        let (tokens, _) = tokenize("1 + 2 * 3");
        let (expr, diagnostics) = parse(&tokens);

        assert!(diagnostics.is_empty());
        assert!(expr.is_some());

        // Should parse as 1 + (2 * 3)
        if let Some(Expr::BinaryOp {
            left, op, right, ..
        }) = expr
        {
            assert!(matches!(left.as_ref(), Expr::Number { value: 1, .. }));
            assert_eq!(op, BinaryOperator::Add);
            assert!(matches!(
                right.as_ref(),
                Expr::BinaryOp {
                    op: BinaryOperator::Multiply,
                    ..
                }
            ));
        } else {
            panic!("Expected binary operation with correct precedence");
        }
    }

    #[test]
    fn test_parse_parentheses() {
        let (tokens, _) = tokenize("(1 + 2) * 3");
        let (expr, diagnostics) = parse(&tokens);

        assert!(diagnostics.is_empty());
        assert!(expr.is_some());

        // Should parse as (1 + 2) * 3
        if let Some(Expr::BinaryOp {
            left, op, right, ..
        }) = expr
        {
            assert!(matches!(
                left.as_ref(),
                Expr::BinaryOp {
                    op: BinaryOperator::Add,
                    ..
                }
            ));
            assert_eq!(op, BinaryOperator::Multiply);
            assert!(matches!(right.as_ref(), Expr::Number { value: 3, .. }));
        } else {
            panic!("Expected binary operation with parentheses");
        }
    }

    #[test]
    fn test_parse_unary() {
        let (tokens, _) = tokenize("-42");
        let (expr, diagnostics) = parse(&tokens);

        assert!(diagnostics.is_empty());
        assert!(expr.is_some());

        if let Some(Expr::UnaryOp { op, operand, .. }) = expr {
            assert_eq!(op, UnaryOperator::Minus);
            assert!(matches!(operand.as_ref(), Expr::Number { value: 42, .. }));
        } else {
            panic!("Expected unary operation");
        }
    }

    #[test]
    fn test_parse_identifier() {
        let (tokens, _) = tokenize("x");
        let (expr, diagnostics) = parse(&tokens);

        assert!(diagnostics.is_empty());
        assert!(expr.is_some());

        if let Some(Expr::Identifier { name, .. }) = expr {
            assert_eq!(name, "x");
        } else {
            panic!("Expected identifier");
        }
    }

    #[test]
    fn test_parse_function_call() {
        let (tokens, _) = tokenize("add(1, 2)");
        let (expr, diagnostics) = parse(&tokens);

        assert!(diagnostics.is_empty());
        assert!(expr.is_some());

        if let Some(Expr::FunctionCall { function, args, .. }) = expr {
            assert!(matches!(function.as_ref(), Expr::Identifier { name, .. } if name == "add"));
            assert_eq!(args.len(), 2);
            assert!(matches!(args[0], Expr::Number { value: 1, .. }));
            assert!(matches!(args[1], Expr::Number { value: 2, .. }));
        } else {
            panic!("Expected function call");
        }
    }

    #[test]
    fn test_parse_lambda_simple() {
        let (tokens, _) = tokenize("|x| x + 1");
        let (expr, diagnostics) = parse(&tokens);

        assert!(diagnostics.is_empty());
        assert!(expr.is_some());

        if let Some(Expr::Lambda { params, body, .. }) = expr {
            assert_eq!(params.len(), 1);
            assert_eq!(params[0], "x");
            assert!(matches!(body.as_ref(), Expr::BinaryOp { .. }));
        } else {
            panic!("Expected lambda expression");
        }
    }

    #[test]
    fn test_parse_lambda_multiple_params() {
        let (tokens, _) = tokenize("|x, y| x * y");
        let (expr, diagnostics) = parse(&tokens);

        assert!(diagnostics.is_empty());
        assert!(expr.is_some());

        if let Some(Expr::Lambda { params, body, .. }) = expr {
            assert_eq!(params.len(), 2);
            assert_eq!(params[0], "x");
            assert_eq!(params[1], "y");
            assert!(matches!(
                body.as_ref(),
                Expr::BinaryOp {
                    op: BinaryOperator::Multiply,
                    ..
                }
            ));
        } else {
            panic!("Expected lambda expression with multiple parameters");
        }
    }

    #[test]
    fn test_parse_lambda_no_params() {
        let (tokens, _) = tokenize("|| 42");
        let (expr, diagnostics) = parse(&tokens);

        assert!(diagnostics.is_empty());
        assert!(expr.is_some());

        if let Some(Expr::Lambda { params, body, .. }) = expr {
            assert_eq!(params.len(), 0);
            assert!(matches!(body.as_ref(), Expr::Number { value: 42, .. }));
        } else {
            panic!("Expected lambda expression with no parameters");
        }
    }

    #[test]
    fn test_parse_and_evaluate_integration() {
        // 集成测试：从source code到执行结果
        use karte_codegen::evaluate;

        // 测试简单lambda调用
        let (tokens, _) = tokenize("(|x| x + 1)(5)");
        let (expr, diagnostics) = parse(&tokens);

        assert!(diagnostics.is_empty());
        assert!(expr.is_some());

        if let Some(expr) = expr {
            let result = evaluate(&expr).unwrap();
            if let karte_codegen::Value::Number(n) = result {
                assert_eq!(n, 6);
            } else {
                panic!("Expected number result");
            }
        }
    }

    #[test]
    fn test_parse_let_statement() {
        let (tokens, _) = tokenize("let x = 5; x + 1");
        let (expr, diagnostics) = parse(&tokens);

        assert!(diagnostics.is_empty());
        assert!(expr.is_some());

        if let Some(Expr::Block {
            statements,
            final_expr,
            ..
        }) = expr
        {
            assert_eq!(statements.len(), 1);
            if let Statement::Let { name, value, .. } = &statements[0] {
                assert_eq!(name, "x");
                if let Expr::Number { value: 5, .. } = value {
                    // 正确
                } else {
                    panic!("Expected number 5 in let value");
                }
            } else {
                panic!("Expected Let statement");
            }

            if let Some(expr) = final_expr {
                if let Expr::BinaryOp { .. } = expr.as_ref() {
                    // 正确
                } else {
                    panic!("Expected binary operation in final expr");
                }
            } else {
                panic!("Expected final expression");
            }
        } else {
            panic!("Expected Block expression");
        }
    }

    #[test]
    fn test_parse_let_with_lambda() {
        let (tokens, _) = tokenize("let f = |x| x * 2; f(5)");
        let (expr, diagnostics) = parse(&tokens);

        assert!(diagnostics.is_empty());
        assert!(expr.is_some());

        if let Some(Expr::Block {
            statements,
            final_expr,
            ..
        }) = expr
        {
            assert_eq!(statements.len(), 1);
            if let Statement::Let { name, value, .. } = &statements[0] {
                assert_eq!(name, "f");
                if let Expr::Lambda { .. } = value {
                    // 正确
                } else {
                    panic!("Expected lambda in let value");
                }
            } else {
                panic!("Expected Let statement");
            }

            if let Some(expr) = final_expr {
                if let Expr::FunctionCall { .. } = expr.as_ref() {
                    // 正确
                } else {
                    panic!("Expected function call in final expr");
                }
            } else {
                panic!("Expected final expression");
            }
        } else {
            panic!("Expected Block expression");
        }
    }

    #[test]
    fn test_parse_and_evaluate_let() {
        use karte_codegen::evaluate;

        let (tokens, _) = tokenize("let x = 10; x + 5");
        let (expr, diagnostics) = parse(&tokens);

        assert!(diagnostics.is_empty());
        assert!(expr.is_some());

        if let Some(expr) = expr {
            let result = evaluate(&expr).unwrap();
            if let karte_codegen::Value::Number(n) = result {
                assert_eq!(n, 15);
            } else {
                panic!("Expected number result");
            }
        }
    }
}
