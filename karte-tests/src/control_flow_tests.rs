//! 控制流测试 - 测试if和while表达式

use karte_codegen::{evaluate, Value};
use karte_hir::type_checker::type_check;
use karte_lexer::tokenize;
use karte_parser::parse;

#[cfg(test)]
mod if_expression_tests {
    use super::*;

    #[test]
    fn test_if_true_simple() {
        let input = "if true then 42 else 0";
        let (tokens, _) = tokenize(input);
        let (ast, _) = parse(&tokens);
        assert!(ast.is_some());
        
        let result = evaluate(ast.as_ref().unwrap()).unwrap();
        assert_eq!(result, Value::Number(42));
    }

    #[test]
    fn test_if_false_simple() {
        let input = "if false then 42 else 0";
        let (tokens, _) = tokenize(input);
        let (ast, _) = parse(&tokens);
        assert!(ast.is_some());
        
        let result = evaluate(ast.as_ref().unwrap()).unwrap();
        assert_eq!(result, Value::Number(0));
    }

    #[test]
    fn test_if_with_computation() {
        let input = "if true then 1 + 2 else 3 * 4";
        let (tokens, _) = tokenize(input);
        let (ast, _) = parse(&tokens);
        assert!(ast.is_some());
        
        let result = evaluate(ast.as_ref().unwrap()).unwrap();
        assert_eq!(result, Value::Number(3));
    }

    #[test]
    fn test_if_without_else() {
        let input = "if false then 42";
        let (tokens, _) = tokenize(input);
        let (ast, _) = parse(&tokens);
        assert!(ast.is_some());
        
        let result = evaluate(ast.as_ref().unwrap()).unwrap();
        assert_eq!(result, Value::Unit);
    }

    #[test]
    fn test_if_nested() {
        let input = "if true then (if false then 1 else 2) else 3";
        let (tokens, _) = tokenize(input);
        let (ast, _) = parse(&tokens);
        assert!(ast.is_some());
        
        let result = evaluate(ast.as_ref().unwrap()).unwrap();
        assert_eq!(result, Value::Number(2));
    }

    #[test]
    fn test_if_with_variables() {
        let input = "let x = 5; if true then x else 0";
        let (tokens, _) = tokenize(input);
        let (ast, _) = parse(&tokens);
        assert!(ast.is_some());
        
        let result = evaluate(ast.as_ref().unwrap()).unwrap();
        assert_eq!(result, Value::Number(5));
    }
}

#[cfg(test)]
mod while_expression_tests {
    use super::*;

    #[test]
    fn test_while_false_never_executes() {
        let input = "while false do 42";
        let (tokens, _) = tokenize(input);
        let (ast, _) = parse(&tokens);
        assert!(ast.is_some());
        
        let result = evaluate(ast.as_ref().unwrap()).unwrap();
        assert_eq!(result, Value::Unit);
    }

    #[test]
    fn test_while_with_variable() {
        // 由于我们的while循环没有副作用机制（变量不可变），
        // 这个测试主要验证while表达式的基本功能
        let input = "let x = 0; while false do x";
        let (tokens, _) = tokenize(input);
        let (ast, _) = parse(&tokens);
        assert!(ast.is_some());
        
        let result = evaluate(ast.as_ref().unwrap()).unwrap();
        assert_eq!(result, Value::Unit);
    }

    #[test]
    fn test_while_in_block() {
        let input = "{ let x = 1; while false do x; x }";
        let (tokens, _) = tokenize(input);
        let (ast, _) = parse(&tokens);
        assert!(ast.is_some());
        
        let result = evaluate(ast.as_ref().unwrap()).unwrap();
        assert_eq!(result, Value::Number(1));
    }
}

#[cfg(test)]
mod type_checking_tests {
    use super::*;
    use karte_hir::types::Type;

    #[test]
    fn test_if_expression_type_checking() {
        let input = "if true then 42 else 0";
        let (tokens, _) = tokenize(input);
        let (ast, _) = parse(&tokens);
        assert!(ast.is_some());
        
        let (result_type, diagnostics) = type_check(ast.as_ref().unwrap());
        assert!(!diagnostics.has_errors());
        assert_eq!(result_type, Type::Number);
    }

    #[test]
    fn test_if_without_else_type() {
        let input = "if true then 42";
        let (tokens, _) = tokenize(input);
        let (ast, _) = parse(&tokens);
        assert!(ast.is_some());
        
        let (result_type, diagnostics) = type_check(ast.as_ref().unwrap());
        assert!(!diagnostics.has_errors());
        assert_eq!(result_type, Type::Unit);
    }

    #[test]
    fn test_while_expression_type() {
        let input = "while true do 42";
        let (tokens, _) = tokenize(input);
        let (ast, _) = parse(&tokens);
        assert!(ast.is_some());
        
        let (result_type, diagnostics) = type_check(ast.as_ref().unwrap());
        assert!(!diagnostics.has_errors());
        assert_eq!(result_type, Type::Unit);
    }

    #[test]
    fn test_if_condition_must_be_boolean() {
        let input = "if 42 then 1 else 0";
        let (tokens, _) = tokenize(input);
        let (ast, _) = parse(&tokens);
        assert!(ast.is_some());
        
        let (_, diagnostics) = type_check(ast.as_ref().unwrap());
        assert!(diagnostics.has_errors());
    }

    #[test]
    fn test_while_condition_must_be_boolean() {
        let input = "while 42 do 1";
        let (tokens, _) = tokenize(input);
        let (ast, _) = parse(&tokens);
        assert!(ast.is_some());
        
        let (_, diagnostics) = type_check(ast.as_ref().unwrap());
        assert!(diagnostics.has_errors());
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_if_and_match_equivalence() {
        // 测试if和match在某些情况下的等价性
        let if_input = "if true then 1 else 0";
        let match_input = "match true { true -> 1, false -> 0 }";
        
        let (if_tokens, _) = tokenize(if_input);
        let (if_ast, _) = parse(&if_tokens);
        
        let (match_tokens, _) = tokenize(match_input);
        let (match_ast, _) = parse(&match_tokens);
        
        let if_result = evaluate(if_ast.as_ref().unwrap()).unwrap();
        let match_result = evaluate(match_ast.as_ref().unwrap()).unwrap();
        
        assert_eq!(if_result, match_result);
    }

    #[test]
    fn test_complex_control_flow() {
        let input = "if true then (match Some(42) { Some(x) -> x, None -> 0 }) else 99";
        let (tokens, _) = tokenize(input);
        let (ast, _) = parse(&tokens);
        assert!(ast.is_some());
        
        let result = evaluate(ast.as_ref().unwrap()).unwrap();
        assert_eq!(result, Value::Number(42));
    }

    #[test]
    fn test_nested_control_flow() {
        let input = "if true then (if false then 1 else 2) else 3";
        let (tokens, _) = tokenize(input);
        let (ast, _) = parse(&tokens);
        assert!(ast.is_some());
        
        let result = evaluate(ast.as_ref().unwrap()).unwrap();
        assert_eq!(result, Value::Number(2));
    }
} 