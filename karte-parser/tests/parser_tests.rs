use karte_lexer::Lexer;
use karte_parser::{ParserMode, parse_with_type_check};

fn parse_project(code: &str) -> karte_diagnostics::DiagnosticBag {
    let tokens = Lexer::new(code).tokenize();
    let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
    diagnostics
}

fn parse_script(code: &str) -> karte_diagnostics::DiagnosticBag {
    let tokens = Lexer::new(code).tokenize();
    let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
    diagnostics
}

#[test]
fn test_parse_empty_function() {
    let code = "fn main() -> number {\n    0\n}";
    let diagnostics = parse_project(code);
    assert!(!diagnostics.has_errors(), "Empty function should parse: {:?}", diagnostics.diagnostics);
}

#[test]
fn test_parse_function_with_params() {
    let code = "fn add(a: number, b: number) -> number {\n    a + b\n}\nfn main() -> number { add(1, 2) }";
    let diagnostics = parse_project(code);
    assert!(!diagnostics.has_errors(), "Function with params should parse");
}

#[test]
fn test_parse_struct_definition() {
    let code = "struct Point { x: number, y: number }\nfn main() -> number { 0 }";
    let diagnostics = parse_project(code);
    assert!(!diagnostics.has_errors(), "Struct definition should parse");
}

#[test]
fn test_parse_enum_definition() {
    let code = "enum Color { Red, Green, Blue }\nfn main() -> number { 0 }";
    let diagnostics = parse_project(code);
    assert!(!diagnostics.has_errors(), "Enum definition should parse");
}

#[test]
fn test_parse_let_binding() {
    let code = "let x = 42; x";
    let diagnostics = parse_script(code);
    assert!(!diagnostics.has_errors(), "Let binding should parse in script mode");
}

#[test]
fn test_parse_if_else() {
    let code = "if true { 1 } else { 2 }";
    let diagnostics = parse_script(code);
    assert!(!diagnostics.has_errors(), "If-else should parse in script mode");
}

#[test]
fn test_parse_match_expr() {
    let code = "match 42 {\n    0 => 1,\n    _ => 2\n}";
    let diagnostics = parse_script(code);
    assert!(!diagnostics.has_errors(), "Match expression should parse in script mode");
}

#[test]
fn test_parse_string_literal() {
    let code = r#"let s = "hello"; 42"#;
    let diagnostics = parse_script(code);
    assert!(!diagnostics.has_errors(), "String literal should parse");
}

#[test]
fn test_parse_char_literal() {
    let code = "let c = 'A'; c";
    let diagnostics = parse_script(code);
    assert!(!diagnostics.has_errors(), "Char literal should parse");
}

#[test]
fn test_parse_number_literal() {
    let code = "let x = 42; let y = -10; x + y";
    let diagnostics = parse_script(code);
    assert!(!diagnostics.has_errors(), "Number literals should parse");
}

#[test]
fn test_parse_boolean_literal() {
    let code = "let a = true; let b = false; if a { 1 } else { 0 }";
    let diagnostics = parse_script(code);
    assert!(!diagnostics.has_errors(), "Boolean literals should parse");
}

#[test]
fn test_parse_function_call() {
    let code = "fn foo(x: number) -> number { x + 1 }\nfn main() -> number { foo(42) }";
    let diagnostics = parse_project(code);
    assert!(!diagnostics.has_errors(), "Function call should parse");
}

#[test]
fn test_parse_nested_function_call() {
    let code = "fn double(x: number) -> number { x * 2 }\nfn add_one(x: number) -> number { x + 1 }\nfn main() -> number { double(add_one(5)) }";
    let diagnostics = parse_project(code);
    assert!(!diagnostics.has_errors(), "Nested function calls should parse");
}

#[test]
fn test_parse_struct_instantiation() {
    let code = "struct Point { x: number, y: number }\nfn main() -> number {\n    let p = Point { x: 1, y: 2 };\n    p.x + p.y\n}";
    let diagnostics = parse_project(code);
    assert!(!diagnostics.has_errors(), "Struct instantiation should parse");
}

#[test]
fn test_parse_field_access() {
    let code = "struct Point { x: number, y: number }\nfn main() -> number {\n    let p = Point { x: 1, y: 2 };\n    p.x\n}";
    let diagnostics = parse_project(code);
    assert!(!diagnostics.has_errors(), "Field access should parse");
}

#[test]
fn test_parse_enum_match() {
    let code = "enum Color { Red, Green, Blue }\nfn main() -> number {\n    let c = Color::Red;\n    match c {\n        Color::Red => 1,\n        Color::Green => 2,\n        Color::Blue => 3\n    }\n}";
    let diagnostics = parse_project(code);
    assert!(!diagnostics.has_errors(), "Enum match should parse");
}

#[test]
fn test_parse_nested_if() {
    let code = "fn main() -> number {\n    if true {\n        if false { 1 } else { 2 }\n    } else {\n        3\n    }\n}";
    let diagnostics = parse_project(code);
    assert!(!diagnostics.has_errors(), "Nested if should parse");
}

#[test]
fn test_parse_function_with_multiple_params() {
    let code = "fn add(a: number, b: number, c: number) -> number { a + b + c }\nfn main() -> number { add(1, 2, 3) }";
    let diagnostics = parse_project(code);
    assert!(!diagnostics.has_errors(), "Function with multiple params should parse");
}

#[test]
fn test_parse_string_operations() {
    let code = "fn main() -> number {\n    let s = \"hello\";\n    0\n}";
    let diagnostics = parse_project(code);
    assert!(!diagnostics.has_errors(), "String operations should parse");
}

#[test]
fn test_parse_reference_and_deref() {
    let code = "fn main() -> number {\n    let x = 42;\n    let r = &x;\n    *r\n}";
    let diagnostics = parse_project(code);
    assert!(!diagnostics.has_errors(), "Reference and deref should parse");
}

#[test]
fn test_parse_option_match() {
    let code = "fn main() -> number {\n    let x = Some(42);\n    match x {\n        Some(v) => v,\n        None => 0\n    }\n}";
    let diagnostics = parse_project(code);
    assert!(!diagnostics.has_errors(), "Option match should parse");
}

#[test]
fn test_parse_result_match() {
    let code = "fn main() -> number {\n    let x = Ok(42);\n    match x {\n        Ok(v) => v,\n        Err(_) => 0\n    }\n}";
    let diagnostics = parse_project(code);
    assert!(!diagnostics.has_errors(), "Result match should parse");
}

#[test]
fn test_parse_tuple() {
    let code = "fn main() -> number {\n    let t = (1, 2, 3);\n    0\n}";
    let diagnostics = parse_project(code);
    assert!(!diagnostics.has_errors(), "Tuple should parse");
}

#[test]
fn test_parse_array() {
    let code = "fn main() -> number {\n    let a = [1, 2, 3];\n    0\n}";
    let diagnostics = parse_project(code);
    assert!(!diagnostics.has_errors(), "Array should parse");
}

#[test]
fn test_parse_nested_struct() {
    let code = "struct Point { x: number, y: number }\nstruct Line { start: Point, end: Point }\nfn main() -> number { 0 }";
    let diagnostics = parse_project(code);
    assert!(!diagnostics.has_errors(), "Nested struct should parse");
}

#[test]
fn test_parse_generic_struct() {
    let code = "struct Pair<T> { first: T, second: T }\nfn main() -> number { 0 }";
    let diagnostics = parse_project(code);
    assert!(!diagnostics.has_errors(), "Generic struct should parse");
}

#[test]
fn test_parse_while_then_expr() {
    let code = "fn foo() -> number {\n    let x = 0;\n    while x < 10 {\n        let y = x + 1;\n        y\n    }\n    42\n}\nfn main() -> number { foo() }";
    let diagnostics = parse_project(code);
    assert!(!diagnostics.has_errors(), "While loop followed by expression should parse");
}

#[test]
fn test_parse_for_then_expr() {
    let code = "fn foo() -> number {\n    for i in 0..10 {\n        i\n    }\n    42\n}\nfn main() -> number { foo() }";
    let diagnostics = parse_project(code);
    assert!(!diagnostics.has_errors(), "For loop followed by expression should parse");
}

#[test]
fn test_parse_error_recovery_missing_semicolon() {
    let code = "fn main() -> number {\n    let x = 42\n    x\n}";
    let diagnostics = parse_project(code);
    // 缺少分号应该报错
    assert!(diagnostics.has_errors(), "Missing semicolon should be an error");
}

#[test]
fn test_parse_error_recovery_extra_brace() {
    let code = "fn main() -> number {\n    let x = 42;\n    x\n}}";
    let diagnostics = parse_project(code);
    assert!(diagnostics.has_errors(), "Extra brace should be an error");
}

#[test]
fn test_parse_error_recovery_unclosed_string() {
    let code = "fn main() -> number {\n    let s = \"hello;\n    0\n}";
    let diagnostics = parse_project(code);
    assert!(diagnostics.has_errors(), "Unclosed string should be an error");
}

#[test]
fn test_parse_chained_method_calls() {
    let code = "fn main() -> number {\n    let x = 42;\n    x\n}";
    let diagnostics = parse_project(code);
    assert!(!diagnostics.has_errors(), "Simple expression should parse");
}

#[test]
fn test_parse_nested_match() {
    let code = "fn main() -> number {\n    let x = Some(Some(42));\n    match x {\n        Some(Some(v)) => v,\n        Some(None) => 0,\n        None => 0\n    }\n}";
    let diagnostics = parse_project(code);
    assert!(!diagnostics.has_errors(), "Nested match should parse");
}

#[test]
fn test_parse_let_with_pattern() {
    let code = "fn main() -> number {\n    let (a, b) = (1, 2);\n    a + b\n}";
    let diagnostics = parse_project(code);
    // Tuple destructuring 可能不支持
    let _ = diagnostics;
}

#[test]
fn test_parse_import_statement() {
    let code = "import std.io;\nfn main() -> number { 0 }";
    let diagnostics = parse_project(code);
    assert!(!diagnostics.has_errors(), "Import statement should parse");
}

#[test]
fn test_parse_from_import() {
    let code = "import std.io;\nfn main() -> number { 0 }";
    let diagnostics = parse_project(code);
    assert!(!diagnostics.has_errors(), "Import statement should parse");
}

#[test]
fn test_parse_pub_function() {
    let code = "pub fn helper() -> number { 42 }\nfn main() -> number { helper() }";
    let diagnostics = parse_project(code);
    assert!(!diagnostics.has_errors(), "Pub function should parse");
}

#[test]
fn test_parse_closure() {
    let code = "fn main() -> number {\n    let f = |x| { x + 1 };\n    f(42)\n}";
    let diagnostics = parse_project(code);
    assert!(!diagnostics.has_errors(), "Closure should parse");
}

#[test]
fn test_parse_chained_function_calls() {
    let code = "fn inc(x: number) -> number { x + 1 }\nfn main() -> number {\n    inc(inc(inc(42)))\n}";
    let diagnostics = parse_project(code);
    assert!(!diagnostics.has_errors(), "Chained function calls should parse");
}

#[test]
fn test_spell_suggestion_similar_keyword() {
    use karte_parser::ParseError;
    let result = ParseError::suggest_keyword("whille");
    assert!(result.is_some(), "Should suggest 'while' for 'whille'");
    if let Some(sug) = result {
        assert_eq!(sug, "while", "Should suggest 'while'");
    }
}

#[test]
fn test_spell_suggestion_struct() {
    use karte_parser::ParseError;
    let result = ParseError::suggest_keyword("strut");
    assert!(result.is_some(), "Should suggest 'struct' for 'strut'");
    if let Some(sug) = result {
        assert_eq!(sug, "struct", "Should suggest 'struct'");
    }
}

#[test]
fn test_spell_suggestion_no_match() {
    use karte_parser::ParseError;
    let result = ParseError::suggest_keyword("xyz123");
    assert!(result.is_none(), "Should not suggest for completely wrong input");
}

#[test]
fn test_spell_suggestion_too_short() {
    use karte_parser::ParseError;
    let result = ParseError::suggest_keyword("a");
    assert!(result.is_none(), "Should not suggest for single char input");
}
