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
    fn test_parse_effect_perform() {
        let (tokens, _) = tokenize("perform Eff(42)");
        let (expr, diagnostics) = parse(&tokens);

        assert!(diagnostics.is_empty());
        assert!(expr.is_some());

        if let Some(Expr::EffectPerform { .. }) = expr {
            // ok
        } else {
            panic!("Expected EffectPerform expression");
        }
    }

    #[test]
    fn test_parse_effect_resume() {
        let (tokens, _) = tokenize("resume(7)");
        let (expr, diagnostics) = parse(&tokens);

        assert!(diagnostics.is_empty());
        assert!(expr.is_some());

        if let Some(Expr::EffectResume { .. }) = expr {
            // ok
        } else {
            panic!("Expected EffectResume expression");
        }
    }

    #[test]
    fn test_parse_effect_handle() {
        let (tokens, _) = tokenize("handle Eff(x) { x } in 1");
        let (expr, diagnostics) = parse(&tokens);

        assert!(diagnostics.is_empty());
        assert!(expr.is_some());

        if let Some(Expr::EffectHandle { .. }) = expr {
            // ok
        } else {
            panic!("Expected EffectHandle expression");
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
    fn test_module_and_import_metadata() {
        let source = r#"
module demo.core
import std.runtime
import std.array as array
import utils::{add as sum, Subtractor}

fn main() -> i32 {
    array::len([1, 2, 3]) + sum(1, 2)
}
"#;
        let (tokens, _) = tokenize(source);
        let (program, diagnostics) = parse_program_with_metadata(&tokens, ParserMode::Project);

        assert!(diagnostics.is_empty(), "diagnostics: {:?}", diagnostics);
        let program = program.expect("program should parse");
        let module = program.module.expect("module declaration missing");
        assert_eq!(module.name, "demo.core");
        assert_eq!(program.imports.len(), 3);

        let array_import = &program.imports[1];
        assert_eq!(array_import.path.join("."), "std.array");
        assert_eq!(array_import.alias.as_deref(), Some("array"));
        assert!(matches!(
            array_import.specifier,
            ImportSpecifier::EntireModule
        ));

        let utils_import = program
            .imports
            .iter()
            .find(|imp| imp.path.join(".") == "utils")
            .expect("utils import missing");
        match &utils_import.specifier {
            ImportSpecifier::Symbols(symbols) => {
                assert_eq!(symbols.len(), 2);
                assert_eq!(symbols[0].name, "add");
                assert_eq!(symbols[0].alias.as_deref(), Some("sum"));
                assert_eq!(symbols[1].name, "Subtractor");
                assert!(symbols[1].alias.is_none());
            }
            _ => panic!("expected selective import"),
        }
    }

    #[test]
    fn test_parse_array_literal_and_index() {
        let (tokens, _) = tokenize("let arr = [1, 2]; arr[0]");
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
            if let Some(Statement::Let { value, .. }) = statements.first() {
                assert!(
                    matches!(value, Expr::ArrayLiteral { elements, .. } if elements.len() == 2)
                );
            } else {
                panic!("Expected let binding");
            }

            if let Some(expr) = final_expr {
                assert!(matches!(expr.as_ref(), Expr::Index { .. }));
            } else {
                panic!("Expected final expression");
            }
        } else {
            panic!("Expected block expression");
        }
    }

    #[test]
    fn test_parse_array_len() {
        let (tokens, _) = tokenize("let arr = [1]; len(arr)");
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
            if let Some(expr) = final_expr {
                assert!(matches!(expr.as_ref(), Expr::ArrayLen { .. }));
            } else {
                panic!("Expected final expression");
            }
        } else {
            panic!("Expected block expression");
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
            assert_eq!(params[0].name, "x");
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
            assert_eq!(params[0].name, "x");
            assert_eq!(params[1].name, "y");
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
        use crate::execute_from_string;

        let input = "let x = 10; x + 5";
        let result = execute_from_string(input).unwrap();
        assert_eq!(result, 15);
    }

    #[test]
    fn test_parse_comparison_equal() {
        let (tokens, _) = tokenize("5 == 3");
        let (expr, diagnostics) = parse(&tokens);

        assert!(diagnostics.is_empty());
        assert!(expr.is_some());

        if let Some(Expr::BinaryOp {
            left, op, right, ..
        }) = expr
        {
            assert!(matches!(left.as_ref(), Expr::Number { value: 5, .. }));
            assert_eq!(op, BinaryOperator::Equal);
            assert!(matches!(right.as_ref(), Expr::Number { value: 3, .. }));
        } else {
            panic!("Expected binary operation for ==");
        }
    }

    #[test]
    fn test_parse_comparison_greater_equal() {
        let (tokens, _) = tokenize("10 >= 7");
        let (expr, diagnostics) = parse(&tokens);

        assert!(diagnostics.is_empty());
        assert!(expr.is_some());

        if let Some(Expr::BinaryOp {
            left, op, right, ..
        }) = expr
        {
            assert!(matches!(left.as_ref(), Expr::Number { value: 10, .. }));
            assert_eq!(op, BinaryOperator::GreaterEqual);
            assert!(matches!(right.as_ref(), Expr::Number { value: 7, .. }));
        } else {
            panic!("Expected binary operation for >=");
        }
    }

    #[test]
    fn test_parse_comparison_less_equal() {
        let (tokens, _) = tokenize("3 <= 8");
        let (expr, diagnostics) = parse(&tokens);

        assert!(diagnostics.is_empty());
        assert!(expr.is_some());

        if let Some(Expr::BinaryOp {
            left, op, right, ..
        }) = expr
        {
            assert!(matches!(left.as_ref(), Expr::Number { value: 3, .. }));
            assert_eq!(op, BinaryOperator::LessEqual);
            assert!(matches!(right.as_ref(), Expr::Number { value: 8, .. }));
        } else {
            panic!("Expected binary operation for <=");
        }
    }

    #[test]
    fn test_parse_comparison_precedence() {
        let (tokens, _) = tokenize("1 + 2 == 3");
        let (expr, diagnostics) = parse(&tokens);

        assert!(diagnostics.is_empty());
        assert!(expr.is_some());

        // Should parse as (1 + 2) == 3
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
            assert_eq!(op, BinaryOperator::Equal);
            assert!(matches!(right.as_ref(), Expr::Number { value: 3, .. }));
        } else {
            panic!("Expected binary operation with correct precedence for comparison");
        }
    }

    #[test]
    fn test_parse_comparison_chain() {
        let (tokens, _) = tokenize("5 >= 3 == true");
        let (expr, diagnostics) = parse(&tokens);

        assert!(diagnostics.is_empty());
        assert!(expr.is_some());

        // Should parse as (5 >= 3) == true
        if let Some(Expr::BinaryOp {
            left, op, right, ..
        }) = expr
        {
            assert!(matches!(
                left.as_ref(),
                Expr::BinaryOp {
                    op: BinaryOperator::GreaterEqual,
                    ..
                }
            ));
            assert_eq!(op, BinaryOperator::Equal);
            assert!(matches!(right.as_ref(), Expr::Boolean { value: true, .. }));
        } else {
            panic!("Expected chained comparison operation");
        }
    }

    #[test]
    fn test_parse_comparison_greater() {
        let (tokens, _) = tokenize("10 > 7");
        let (expr, diagnostics) = parse(&tokens);

        assert!(diagnostics.is_empty());
        assert!(expr.is_some());

        if let Some(Expr::BinaryOp {
            left, op, right, ..
        }) = expr
        {
            assert!(matches!(left.as_ref(), Expr::Number { value: 10, .. }));
            assert_eq!(op, BinaryOperator::Greater);
            assert!(matches!(right.as_ref(), Expr::Number { value: 7, .. }));
        } else {
            panic!("Expected binary operation for >");
        }
    }

    #[test]
    fn test_parse_comparison_less() {
        let (tokens, _) = tokenize("3 < 8");
        let (expr, diagnostics) = parse(&tokens);

        assert!(diagnostics.is_empty());
        assert!(expr.is_some());

        if let Some(Expr::BinaryOp {
            left, op, right, ..
        }) = expr
        {
            assert!(matches!(left.as_ref(), Expr::Number { value: 3, .. }));
            assert_eq!(op, BinaryOperator::Less);
            assert!(matches!(right.as_ref(), Expr::Number { value: 8, .. }));
        } else {
            panic!("Expected binary operation for <");
        }
    }
}
