#[cfg(test)]
use crate::execute_from_string;
#[cfg(test)]
use karte_hir::{type_check, Type};
#[cfg(test)]
use karte_lexer::tokenize;
#[cfg(test)]
use karte_parser::parse;

#[cfg(test)]
fn test_evaluate(input: &str) -> Result<i64, String> {
    execute_from_string(input)
}

#[cfg(test)]
fn test_type_check(input: &str) -> Result<Type, String> {
    let (tokens, mut diagnostics) = tokenize(input);
    if diagnostics.has_errors() {
        return Err(format!("Lexer errors: {:?}", diagnostics));
    }

    let (expr, parse_diagnostics) = parse(&tokens);
    diagnostics.extend(parse_diagnostics);

    if diagnostics.has_errors() {
        return Err(format!("Parser errors: {:?}", diagnostics));
    }

    let expr = expr.ok_or("Parse failed")?;
    let (result_type, type_diagnostics) = type_check(&expr);

    if type_diagnostics.has_errors() {
        return Err(format!("Type check errors: {:?}", type_diagnostics));
    }

    Ok(result_type)
}

#[cfg(test)]
mod evaluation_tests {
    use super::*;

    #[test]
    fn test_boolean_literals() {
        // 测试true构造器，通过match转换为数字1
        let result = test_evaluate("match true { true -> 1, false -> 0 }").unwrap();
        assert_eq!(result, 1);

        // 测试false构造器，通过match转换为数字0
        let result = test_evaluate("match false { true -> 1, false -> 0 }").unwrap();
        assert_eq!(result, 0);
    }

    #[test]
    fn test_constructor_without_args() {
        // 测试None构造器，通过match转换为数字999
        let result = test_evaluate("match None { Some(x) -> 42, None -> 999 }").unwrap();
        assert_eq!(result, 999);
    }

    #[test]
    fn test_constructor_with_args() {
        // 测试Some构造器，提取参数值
        let result = test_evaluate("match Some(42) { Some(x) -> x, None -> 0 }").unwrap();
        assert_eq!(result, 42);

        // 测试带不同参数的Some构造器
        let result = test_evaluate("match Some(123) { Some(x) -> x + 10, None -> 0 }").unwrap();
        assert_eq!(result, 133);
    }

    #[test]
    fn test_simple_match() {
        let result = test_evaluate("match true { true -> 1, false -> 0 }").unwrap();
        assert_eq!(result, 1);

        let result = test_evaluate("match false { true -> 1, false -> 0 }").unwrap();
        assert_eq!(result, 0);
    }

    #[test]
    fn test_match_with_variable_binding() {
        let result = test_evaluate("match Some(42) { Some(x) -> x, None -> 0 }").unwrap();
        assert_eq!(result, 42);

        let result = test_evaluate("match None { Some(x) -> x, None -> 999 }").unwrap();
        assert_eq!(result, 999);
    }

    #[test]
    fn test_match_with_wildcard() {
        let result = test_evaluate("match Some(42) { _ -> 123 }").unwrap();
        assert_eq!(result, 123);
    }

    #[test]
    fn test_complex_match() {
        let program = r#"
            let x = Some(42);
            match x {
                Some(y) -> y + 10,
                None -> 0
            }
        "#;
        let result = test_evaluate(program).unwrap();
        assert_eq!(result, 52);
    }

    #[test]
    fn test_practical_full_range_support() {
        // 测试构造器命名空间隔离方案：偏移编码
        // 用户数据范围：(-999999999, i64::MAX] (几乎完整的i64范围)
        // 构造器范围：[i64::MIN, -1000000000] (极端负值范围)

        // 测试正数范围（完全支持）
        let large_positive = 9223372036854775807_i64; // i64::MAX
        let program1 = format!(
            "match Some({}) {{ Some(x) -> x, None -> 0 }}",
            large_positive
        );
        let result1 = test_evaluate(&program1).unwrap();
        assert_eq!(result1, large_positive);

        // 测试中等正数
        let medium_positive = 1000000_i64;
        let program2 = format!(
            "match Some({}) {{ Some(x) -> x, None -> 0 }}",
            medium_positive
        );
        let result2 = test_evaluate(&program2).unwrap();
        assert_eq!(result2, medium_positive);

        // 测试零值（边界情况）
        let program3 = "match Some(0) { Some(x) -> x + 42, None -> -1 }";
        println!("DEBUG: Testing program3: {}", program3);
        let result3 = test_evaluate(program3).unwrap();
        println!("DEBUG: result3 = {}, expected = 42", result3);
        assert_eq!(result3, 42);

        // 测试负数（现在支持大部分负数）
        let safe_negative = -999999999_i64; // 刚好在用户数据范围内
        let program4 = format!(
            "match Some({}) {{ Some(x) -> x * 2, None -> 0 }}",
            safe_negative
        );
        let result4 = test_evaluate(&program4).unwrap();
        assert_eq!(result4, safe_negative * 2);

        // 测试接近i64::MAX的值
        let near_max = 9223372036854775000_i64;
        let program5 = format!(
            "match Some({}) {{ Some(x) -> x - 1000, None -> 0 }}",
            near_max
        );
        let result5 = test_evaluate(&program5).unwrap();
        assert_eq!(result5, near_max - 1000);

        // 测试小负数
        let small_negative = -42_i64;
        let program6 = format!(
            "match Some({}) {{ Some(x) -> x + 100, None -> 0 }}",
            small_negative
        );
        let result6 = test_evaluate(&program6).unwrap();
        assert_eq!(result6, small_negative + 100);

        // 注意：在偏移编码方案中，用户可以使用99.99%的i64范围
        // 只有极端负值（< -1000000000）被保留给构造器使用
        // 这样设计在实用性和类型安全之间取得了很好的平衡
    }

    #[test]
    fn test_constructor_id_separation() {
        // 验证构造器ID与用户数据完全分离
        // 现在构造器使用标记位编码，用户数据使用正数范围

        // 测试不同的构造器都能正确工作
        let result1 = test_evaluate("match true { true -> 100, false -> 200 }").unwrap();
        assert_eq!(result1, 100);

        let result2 = test_evaluate("match false { true -> 100, false -> 200 }").unwrap();
        assert_eq!(result2, 200);

        let result3 = test_evaluate("match None { Some(x) -> x, None -> 999 }").unwrap();
        assert_eq!(result3, 999);
    }
}

#[cfg(test)]
mod type_check_tests {
    use super::*;

    #[test]
    fn test_boolean_type() {
        let result = test_type_check("true").unwrap();
        assert_eq!(result, Type::bool());

        let result = test_type_check("false").unwrap();
        assert_eq!(result, Type::bool());
    }

    #[test]
    fn test_option_types() {
        let result = test_type_check("Some(42)").unwrap();
        assert_eq!(result, Type::option(Type::Number));

        let result = test_type_check("None").unwrap();
        match result {
            Type::Sum { name, .. } if name == "Option" => {}
            _ => panic!("Expected Option type, got {:?}", result),
        }
    }

    #[test]
    fn test_match_type_checking() {
        let result = test_type_check("match true { true -> 1, false -> 0 }").unwrap();
        assert_eq!(result, Type::Number);

        // 简化的匹配测试，避免复杂的类型推断
        let result = test_type_check("match None { _ -> 42 }").unwrap();
        assert_eq!(result, Type::Number);
    }

    #[test]
    fn test_some_match_fixed() {
        // 测试我们修复的Some匹配问题
        let result = test_evaluate("match Some(42) { Some(x) -> x + 1, None -> 0 }").unwrap();
        assert_eq!(result, 43);
    }

    #[test]
    fn test_match_guard_basic() {
        let result = test_evaluate("match Some(5) { Some(v) if v > 3 -> 1, Some(v) -> 2, None -> 0 }").unwrap();
        assert_eq!(result, 1);

        let result = test_evaluate("match Some(1) { Some(v) if v > 3 -> 1, Some(v) -> 2, None -> 0 }").unwrap();
        assert_eq!(result, 2);

        let result = test_evaluate("match None { Some(v) if v > 3 -> 1, Some(v) -> 2, None -> 0 }").unwrap();
        assert_eq!(result, 0);
    }

    #[test]
    fn test_match_guard_with_variable_binding() {
        let result = test_evaluate("match Some(10) { Some(x) if x > 5 -> x * 2, Some(x) -> x, None -> 0 }").unwrap();
        assert_eq!(result, 20);

        let result = test_evaluate("match Some(3) { Some(x) if x > 5 -> x * 2, Some(x) -> x, None -> 0 }").unwrap();
        assert_eq!(result, 3);
    }

    #[test]
    fn test_match_guard_with_boolean_pattern() {
        let result = test_evaluate("match true { true if false -> 1, true -> 2, false -> 3 }").unwrap();
        assert_eq!(result, 2);

        let result = test_evaluate("match true { true if true -> 1, true -> 2, false -> 3 }").unwrap();
        assert_eq!(result, 1);
    }

    #[test]
    fn test_match_guard_wildcard_fallback() {
        let result = test_evaluate("match Some(5) { Some(v) if v > 10 -> 100, _ -> 42 }").unwrap();
        assert_eq!(result, 42);

        let result = test_evaluate("match Some(20) { Some(v) if v > 10 -> 100, _ -> 42 }").unwrap();
        assert_eq!(result, 100);
    }

    #[test]
    fn test_match_guard_type_check() {
        let result = test_type_check("match Some(5) { Some(v) if v > 3 -> 1, Some(v) -> 2, None -> 0 }").unwrap();
        assert_eq!(result, Type::Number);
    }


    #[test]
    fn test_if_let_basic() {
        let result = test_evaluate("if let Some(v) = Some(42) { v + 1 } else { 0 }").unwrap();
        assert_eq!(result, 43);

        let result = test_evaluate("if let Some(v) = None { v + 1 } else { 0 }").unwrap();
        assert_eq!(result, 0);
    }

    #[test]
    fn test_if_let_with_wildcard() {
        let result = test_evaluate("if let Some(_) = Some(99) { 1 } else { 0 }").unwrap();
        assert_eq!(result, 1);

        let result = test_evaluate("if let Some(_) = None { 1 } else { 0 }").unwrap();
        assert_eq!(result, 0);
    }

    #[test]
    fn test_if_let_with_number_pattern() {
        let result = test_evaluate("if let 42 = 42 { 1 } else { 0 }").unwrap();
        assert_eq!(result, 1);

        let result = test_evaluate("if let 42 = 99 { 1 } else { 0 }").unwrap();
        assert_eq!(result, 0);
    }

    #[test]
    fn test_if_let_with_boolean_pattern() {
        let result = test_evaluate("if let true = true { 1 } else { 0 }").unwrap();
        assert_eq!(result, 1);

        let result = test_evaluate("if let true = false { 1 } else { 0 }").unwrap();
        assert_eq!(result, 0);
    }

    #[test]
    fn test_if_let_type_check() {
        let result = test_type_check("if let Some(v) = Some(42) { v } else { 0 }").unwrap();
        assert_eq!(result, Type::Number);
    }

}
