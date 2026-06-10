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
