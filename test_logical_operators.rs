use karte_lexer::tokenize;
use karte_parser::parse;
use karte_hir::type_check;
use karte_codegen::evaluate;

#[cfg(test)]
mod logical_operators_tests {
    use super::*;

    fn test_logical_expression(input: &str) -> String {
        let (tokens, _) = tokenize(input);
        let (expr_opt, _) = parse(&tokens);
        
        if let Some(expr) = expr_opt {
            let (_, _) = type_check(&expr);
            match evaluate(&expr) {
                Ok(value) => format!("{}", value),
                Err(e) => format!("Error: {}", e),
            }
        } else {
            "Parse error".to_string()
        }
    }

    #[test]
    fn test_logical_and_basic() {
        // 基本逻辑与运算
        assert_eq!(test_logical_expression("true && true"), "True");
        assert_eq!(test_logical_expression("true && false"), "False");
        assert_eq!(test_logical_expression("false && true"), "False");
        assert_eq!(test_logical_expression("false && false"), "False");
    }

    #[test]
    fn test_logical_or_basic() {
        // 基本逻辑或运算
        assert_eq!(test_logical_expression("true || true"), "True");
        assert_eq!(test_logical_expression("true || false"), "True");
        assert_eq!(test_logical_expression("false || true"), "True");
        assert_eq!(test_logical_expression("false || false"), "False");
    }

    #[test]
    fn test_logical_not_basic() {
        // 基本逻辑非运算
        assert_eq!(test_logical_expression("!true"), "False");
        assert_eq!(test_logical_expression("!false"), "True");
        assert_eq!(test_logical_expression("!!true"), "True");
        assert_eq!(test_logical_expression("!!false"), "False");
    }

    #[test]
    fn test_logical_precedence() {
        // 测试运算符优先级
        assert_eq!(test_logical_expression("!true || false"), "False");
        assert_eq!(test_logical_expression("!(true || false)"), "False");
        assert_eq!(test_logical_expression("true && false || true"), "True");
        assert_eq!(test_logical_expression("true && (false || true)"), "True");
        assert_eq!(test_logical_expression("false || true && false"), "False");
        assert_eq!(test_logical_expression("(false || true) && false"), "False");
    }

    #[test]
    fn test_complex_logical_expressions() {
        // 复杂的逻辑表达式
        assert_eq!(test_logical_expression("!(!true && false)"), "True");
        assert_eq!(test_logical_expression("true && true && true"), "True");
        assert_eq!(test_logical_expression("true && true && false"), "False");
        assert_eq!(test_logical_expression("false || false || true"), "True");
        assert_eq!(test_logical_expression("false || false || false"), "False");
    }

    #[test]
    fn test_logical_with_comparisons() {
        // 逻辑运算符与比较运算符的组合
        assert_eq!(test_logical_expression("(1 == 1) && (2 > 1)"), "True");
        assert_eq!(test_logical_expression("(1 == 2) || (2 > 1)"), "True");
        assert_eq!(test_logical_expression("!(1 == 1)"), "False");
        assert_eq!(test_logical_expression("(1 < 2) && (3 >= 3)"), "True");
    }

    #[test]
    fn test_short_circuit_evaluation() {
        // 测试短路求值 - 这些表达式在短路求值下应该能正常工作
        // 注意：由于我们的简单解释器，这里主要测试基本功能
        assert_eq!(test_logical_expression("false && true"), "False");
        assert_eq!(test_logical_expression("true || false"), "True");
    }
} 