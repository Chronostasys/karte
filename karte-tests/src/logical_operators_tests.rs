#[cfg(test)]
mod logical_operators_tests {
    use karte_hir::Type;
    use karte_lexer::tokenize;
    use karte_parser::{parse, parse_with_type_check};

    use crate::{execute_from_string, execute_with_pipeline_debug};

    #[test]
    fn test_logical_and_basic() {
        let input = "match true && false {
            true -> 1,
            false -> 0,
        }";
        // 先测试单个boolean值 - 使用调试模式
        let (tokens, _) = tokenize("true");
        let (expr, _) = parse(&tokens);
        let true_result = execute_with_pipeline_debug(&expr.unwrap(), true).unwrap();

        let (tokens, _) = tokenize("false");
        let (expr, _) = parse(&tokens);
        let false_result = execute_with_pipeline_debug(&expr.unwrap(), true).unwrap();

        println!("Debug: true={}, false={}", true_result, false_result);

        let result = execute_from_string(input).unwrap();
        println!("Debug: true && false = {}", result);
        assert_eq!(result, 0); // false as i64
    }

    #[test]
    fn test_logical_or_basic() {
        let input = "match true || false {
            true -> 1,
            false -> 0,
        }";

        // 使用调试模式查看执行过程
        let (tokens, _) = tokenize(input);
        let (expr, _) = parse(&tokens);
        let debug_result = execute_with_pipeline_debug(&expr.unwrap(), true).unwrap();
        println!("Debug result for 'true || false': {}", debug_result);

        let result = execute_from_string(input).unwrap();
        assert_eq!(result, 1); // true as i64
    }

    #[test]
    fn test_logical_not_basic() {
        let input = "match !true {
            true -> 1,
            false -> 0,
        }";
        let result = execute_from_string(input).unwrap();
        assert_eq!(result, 0); // false as i64
    }

    #[test]
    fn test_logical_not_false() {
        let input = "match !false {
            true -> 1,
            false -> 0,
        }";
        let result = execute_from_string(input).unwrap();
        assert_eq!(result, 1); // true as i64
    }

    #[test]
    fn test_logical_and_short_circuit_false() {
        // 当第一个操作数为false时，应该短路求值
        let input = "match false && true {
            true -> 1,
            false -> 0,
        }";
        let result = execute_from_string(input).unwrap();
        assert_eq!(result, 0); // false as i64
    }

    #[test]
    fn test_logical_and_short_circuit_true() {
        // 当第一个操作数为true时，返回第二个操作数的值
        let input = "match true && false {
            true -> 1,
            false -> 0,
        }";
        let result = execute_from_string(input).unwrap();
        assert_eq!(result, 0); // false as i64
    }

    #[test]
    fn test_logical_or_short_circuit_true() {
        // 当第一个操作数为true时，应该短路求值
        let input = "match true || false {
            true -> 1,
            false -> 0,
        }";
        let result = execute_from_string(input).unwrap();
        assert_eq!(result, 1); // true as i64
    }

    #[test]
    fn test_logical_or_short_circuit_false() {
        // 当第一个操作数为false时，返回第二个操作数的值
        let input = "match false || true {
            true -> 1,
            false -> 0,
        }";
        let result = execute_from_string(input).unwrap();
        assert_eq!(result, 1); // true as i64
    }

    #[test]
    fn test_logical_operator_precedence() {
        // 测试操作符优先级: ! > && > ||
        let input = "match !false || true && false {
            true -> 1,
            false -> 0,
        }";
        // 应该解析为: (!false) || (true && false)
        // = true || false = true
        let result = execute_from_string(input).unwrap();
        assert_eq!(result, 1); // true as i64
    }
    #[test]
    fn test_logical_and_precedence_over_or() {
        // && 优先级高于 ||
        let input = "match true || false && false {
            true -> 1,
            false -> 0,
        }";
        // 应该解析为: true || (false && false)
        // = true || false = true
        let result = execute_from_string(input).unwrap();
        assert_eq!(result, 1); // true as i64
    }

    #[test]
    fn test_logical_not_precedence() {
        // ! 优先级最高
        let input = "match !true && false {
            true -> 1,
            false -> 0,
        }";
        // 应该解析为: (!true) && false
        // = false && false = false
        let result = execute_from_string(input).unwrap();
        assert_eq!(result, 0); // false as i64
    }

    #[test]
    fn test_complex_logical_expression() {
        let input = "match (true && false) || (false || true) {
            true -> 1,
            false -> 0,
        }";
        // = false || true = true
        let result = execute_from_string(input).unwrap();
        assert_eq!(result, 1); // true as i64
    }

    #[test]
    fn test_nested_logical_operations() {
        let input = "match true && (false || true) && !false {
            true -> 1,
            false -> 0,
        }";
        // = true && true && true = true
        let result = execute_from_string(input).unwrap();
        assert_eq!(result, 1); // true as i64
    }

    #[test]
    fn test_logical_with_variables() {
        let input = "let a = true; let b = false; match a && !b {
            true -> 1,
            false -> 0,
        }";
        // = true && true = true
        let result = execute_from_string(input).unwrap();
        assert_eq!(result, 1); // true as i64
    }

    #[test]
    fn test_logical_type_checking() {
        let input = "true && false";
        let (tokens, _) = tokenize(input);
        let (result, diagnostics) = parse_with_type_check(&tokens);

        assert!(diagnostics.is_empty());
        assert!(result.is_some());

        let result = result.unwrap();
        assert_eq!(result.result_type, Type::bool());
    }

    #[test]
    fn test_logical_or_type_checking() {
        let input = "true || false";
        let (tokens, _) = tokenize(input);
        let (result, diagnostics) = parse_with_type_check(&tokens);

        assert!(diagnostics.is_empty());
        assert!(result.is_some());

        let result = result.unwrap();
        assert_eq!(result.result_type, Type::bool());
    }

    #[test]
    fn test_logical_not_type_checking() {
        let input = "!true";
        let (tokens, _) = tokenize(input);
        let (result, diagnostics) = parse_with_type_check(&tokens);

        assert!(diagnostics.is_empty());
        assert!(result.is_some());

        let result = result.unwrap();
        assert_eq!(result.result_type, Type::bool());
    }

    #[test]
    fn test_logical_and_all_true() {
        let input = "match true && true && true {
            true -> 1,
            false -> 0,
        }";
        let result = execute_from_string(input).unwrap();
        assert_eq!(result, 1); // true as i64
    }

    #[test]
    fn test_logical_or_all_false() {
        let input = "match false || false || false {
            true -> 1,
            false -> 0,
        }";
        let result = execute_from_string(input).unwrap();
        assert_eq!(result, 0); // false as i64
    }

    #[test]
    fn test_mixed_logical_operations() {
        let input = "match true && true || false && true {
            true -> 1,
            false -> 0,
        }";
        // = (true && true) || (false && true)
        // = true || false = true
        let result = execute_from_string(input).unwrap();
        assert_eq!(result, 1); // true as i64
    }

    #[test]
    fn test_double_negation() {
        let input = "match !!true {
            true -> 1,
            false -> 0,
        }";
        let result = execute_from_string(input).unwrap();
        assert_eq!(result, 1); // true as i64
    }

    #[test]
    fn test_triple_negation() {
        let input = "match !!!false {
            true -> 1,
            false -> 0,
        }";
        let result = execute_from_string(input).unwrap();
        assert_eq!(result, 1); // true as i64
    }

    #[test]
    fn test_logical_with_comparison() {
        // use std::io::Write;
        // let mut builder = env_logger::Builder::new();
        // builder.filter_level(log::LevelFilter::Info);
        // builder.format(|buf: &mut env_logger::fmt::Formatter, record: &log::Record| {
        //     writeln!(
        //         buf,
        //         "[{}] {}",
        //         record.level(),
        //         record.args()
        //     )
        // });
        // builder.init();
        let input = "match 5 > 3 && 2 < 4 {
            true -> 1,
            false -> 0,
        }";
        let result = execute_from_string(input).unwrap();
        assert_eq!(result, 1); // true as i64
    }

    #[test]
    fn test_logical_with_arithmetic() {
        let input = "match (1 + 1) == 2 && (3 * 2) == 6 {
            true -> 1,
            false -> 0,
        }";
        let result = execute_from_string(input).unwrap();
        assert_eq!(result, 1); // true as i64
    }
}
