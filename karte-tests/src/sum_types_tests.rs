#[cfg(test)]
use karte_codegen::{evaluate, Value};
#[cfg(test)]
use karte_hir::{type_check, Type};
#[cfg(test)]
use karte_lexer::tokenize;
#[cfg(test)]
use karte_parser::parse;

#[cfg(test)]
fn test_evaluate(input: &str) -> Result<Value, String> {
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
    evaluate(&expr)
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
        let result = test_evaluate("true").unwrap();
        assert_eq!(result, Value::Constructor {
            name: "True".to_string(),
            value: None,
        });

        let result = test_evaluate("false").unwrap();
        assert_eq!(result, Value::Constructor {
            name: "False".to_string(),
            value: None,
        });
    }

    #[test]
    fn test_constructor_without_args() {
        let result = test_evaluate("None").unwrap();
        assert_eq!(result, Value::Constructor {
            name: "None".to_string(),
            value: None,
        });
    }

    #[test]
    fn test_constructor_with_args() {
        let result = test_evaluate("Some(42)").unwrap();
        assert_eq!(result, Value::Constructor {
            name: "Some".to_string(),
            value: Some(Box::new(Value::Number(42))),
        });
    }

    #[test]
    fn test_simple_match() {
        let result = test_evaluate("match true { true -> 1, false -> 0 }").unwrap();
        assert_eq!(result, Value::Number(1));

        let result = test_evaluate("match false { true -> 1, false -> 0 }").unwrap();
        assert_eq!(result, Value::Number(0));
    }

    #[test]
    fn test_match_with_variable_binding() {
        let result = test_evaluate("match Some(42) { Some(x) -> x, None -> 0 }").unwrap();
        assert_eq!(result, Value::Number(42));

        let result = test_evaluate("match None { Some(x) -> x, None -> 999 }").unwrap();
        assert_eq!(result, Value::Number(999));
    }

    #[test]
    fn test_match_with_wildcard() {
        let result = test_evaluate("match Some(42) { _ -> 123 }").unwrap();
        assert_eq!(result, Value::Number(123));
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
        assert_eq!(result, Value::Number(52));
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
            Type::Sum { name, .. } if name == "Option" => {},
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
        assert_eq!(result, Value::Number(43));
    }
}
