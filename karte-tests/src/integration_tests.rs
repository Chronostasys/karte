#[cfg(test)]
mod tests {
    use karte_parser::*;

    use karte_codegen::*;
    use karte_lexer::*;

    #[test]
    fn test_integration_simple() {
        let (tokens, _) = tokenize("42");
        let (expr, _) = parse(&tokens);
        assert!(expr.is_some());

        let result = evaluate(&expr.unwrap()).unwrap();
        assert_eq!(result, Value::Number(42));
    }

    #[test]
    fn test_integration_complex() {
        let (tokens, _) = tokenize("(1 + 2) * 3 - 4");
        let (expr, _) = parse(&tokens);
        assert!(expr.is_some());

        let result = evaluate(&expr.unwrap()).unwrap();
        assert_eq!(result, Value::Number(5)); // (1 + 2) * 3 - 4 = 9 - 4 = 5
    }
    #[test]
    fn test_full_pipeline_simple() {
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

        // 代码生成
        let value = evaluate(&result.expr).unwrap();
        assert_eq!(value, Value::Number(15));
    }

    #[test]
    fn test_full_pipeline_lambda() {
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

        // 代码生成
        let value = evaluate(&result.expr).unwrap();
        assert_eq!(value, Value::Number(14));
    }

    #[test]
    fn test_full_pipeline_error_undefined_variable() {
        let input = "let x = 5; y + 10";

        // 词法分析
        let (tokens, lex_diagnostics) = tokenize(input);
        assert!(lex_diagnostics.is_empty());

        // 语法分析和类型检查 - 应该有错误
        let (result, parse_diagnostics) = parse_with_type_check(&tokens);
        assert!(parse_diagnostics.has_errors());
        assert!(result.is_some());

        // 验证错误信息
        assert!(parse_diagnostics
            .diagnostics
            .iter()
            .any(|d| d.message.contains("Undefined variable: y")));
    }

    #[test]
    fn test_full_pipeline_error_type_mismatch() {
        let input = "5(10)";

        // 词法分析
        let (tokens, lex_diagnostics) = tokenize(input);
        assert!(lex_diagnostics.is_empty());

        // 语法分析和类型检查 - 应该有错误
        let (result, parse_diagnostics) = parse_with_type_check(&tokens);
        assert!(parse_diagnostics.has_errors());
        assert!(result.is_some());

        // 验证错误信息
        assert!(parse_diagnostics
            .diagnostics
            .iter()
            .any(|d| d.message.contains("Cannot call value of type number")));
    }

    #[test]
    fn test_full_pipeline_complex() {
        let input = "let x = 3; let y = 4; let multiply = |a, b| a * b; multiply(x, y)";

        // 词法分析
        let (tokens, lex_diagnostics) = tokenize(input);
        assert!(lex_diagnostics.is_empty());

        // 语法分析和类型检查
        let (result, parse_diagnostics) = parse_with_type_check(&tokens);
        assert!(parse_diagnostics.is_empty());
        assert!(result.is_some());

        let result = result.unwrap();
        assert_eq!(result.result_type, karte_hir::Type::Number);

        // 代码生成
        let value = evaluate(&result.expr).unwrap();
        assert_eq!(value, Value::Number(12));
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
