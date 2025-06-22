
use karte_hir::{type_check, Expr};
use karte_lexer::tokenize;
use karte_parser::{parse, parse_with_type_check};
use crate::execute_from_string;

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_simple_arithmetic() {
        let input = "1 + 2 * 3";

        // 词法分析
        let (tokens, lex_diagnostics) = tokenize(input);
        assert!(lex_diagnostics.is_empty());

        // 语法分析和类型检查
        let (result, parse_diagnostics) = parse_with_type_check(&tokens);
        assert!(parse_diagnostics.is_empty());
        assert!(result.is_some());

        let result = result.unwrap();
        assert_eq!(result.result_type, karte_hir::Type::Number);

        // 代码生成 - 使用新的执行流程
        let value = execute_from_string(input).unwrap();
        assert_eq!(value, 7);
    }

    #[test]
    fn test_variable_binding() {
        let input = "let x = 5; x + 10";

        // 词法分析
        let (tokens, lex_diagnostics) = tokenize(input);
        assert!(lex_diagnostics.is_empty());

        // 语法分析和类型检查
        let (result, parse_diagnostics) = parse_with_type_check(&tokens);
        assert!(parse_diagnostics.is_empty());
        assert!(result.is_some());

        let result = result.unwrap();
        assert_eq!(result.result_type, karte_hir::Type::Number);

        // 代码生成 - 使用新的执行流程
        let value = execute_from_string(input).unwrap();
        assert_eq!(value, 15);
    }

    #[test]
    fn test_lambda_and_function_call() {
        let input = "let f = |x| x * 2; f(7)";

        // 词法分析
        let (tokens, lex_diagnostics) = tokenize(input);
        assert!(lex_diagnostics.is_empty());

        // 语法分析和类型检查
        let (result, parse_diagnostics) = parse_with_type_check(&tokens);
        assert!(parse_diagnostics.is_empty());
        assert!(result.is_some());

        let result = result.unwrap();
        assert_eq!(result.result_type, karte_hir::Type::Number);

        // 代码生成 - 使用新的执行流程
        let value = execute_from_string(input).unwrap();
        assert_eq!(value, 14);
    }

    #[test]
    fn test_complex_expression() {
        let input = "(|a, b| a * b)(3,4)";

        // 词法分析
        let (tokens, lex_diagnostics) = tokenize(input);
        assert!(lex_diagnostics.is_empty());

        // 语法分析和类型检查
        let (result, parse_diagnostics) = parse_with_type_check(&tokens);
        assert!(parse_diagnostics.is_empty());
        assert!(result.is_some());

        let result = result.unwrap();
        assert_eq!(result.result_type, karte_hir::Type::Number);

        // 代码生成 - 使用新的执行流程
        let value = execute_from_string(input).unwrap();
        assert_eq!(value, 12);
    }

    #[test]
    fn test_type_check_error_type_mismatch() {
        let input = "let a = |a|a; let b = |d|-d(||1);b(a)";
        let (tokens, lex_diagnostics) = tokenize(input);
        assert!(lex_diagnostics.is_empty());

        let (result, parse_diagnostics) = parse_with_type_check(&tokens);
        assert!(parse_diagnostics.has_errors());
        assert!(result.is_some());

        let error = parse_diagnostics
            .diagnostics
            .iter()
            .find(|d| d.level == karte_diagnostics::DiagnosticLevel::Error);
        assert!(error.is_some());
        assert_eq!(error.unwrap().message, "Type mismatch: expected fn(fn() -> number) -> number, found fn(fn() -> number) -> fn() -> number");
    }
}
