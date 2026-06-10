use karte_lexer::Lexer;
use karte_parser::{ParserMode, parse_with_type_check};

fn check_no_errors(code: &str) {
    let tokens = Lexer::new(code).tokenize();
    let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
    assert!(
        !diagnostics.has_errors(),
        "Expected no errors, but got: {:?}",
        diagnostics.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

fn check_has_errors(code: &str) {
    let tokens = Lexer::new(code).tokenize();
    let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
    assert!(diagnostics.has_errors(), "Expected errors but got none");
}

#[test]
fn test_type_inference_let_binding() {
    check_no_errors("fn main() -> number {\n    let x = 42;\n    let y = x + 1;\n    y\n}");
}

#[test]
fn test_type_inference_function_call() {
    check_no_errors("fn double(x: number) -> number { x * 2 }\nfn main() -> number { double(21) }");
}

#[test]
fn test_type_inference_nested_calls() {
    check_no_errors("fn inc(x: number) -> number { x + 1 }\nfn main() -> number { inc(inc(inc(42))) }");
}

#[test]
fn test_type_check_recursive_function() {
    check_no_errors("fn fib(n: number) -> number { if n <= 1 { n } else { fib(n - 1) + fib(n - 2) } }\nfn main() -> number { fib(10) }");
}

#[test]
fn test_type_check_struct_field_access() {
    check_no_errors("struct Point { x: number, y: number }\nfn main() -> number {\n    let p = Point { x: 1, y: 2 };\n    p.x + p.y\n}");
}

#[test]
fn test_type_check_string_concat() {
    check_no_errors("fn main() -> number {\n    let s = \"hello\" + \" world\";\n    0\n}");
}

#[test]
fn test_type_check_early_return() {
    check_no_errors("fn abs(x: number) -> number {\n    let result = if x < 0 { 0 - x } else { x };\n    result\n}\nfn main() -> number { abs(42) }");
}

#[test]
fn test_type_check_wrong_return_type() {
    check_has_errors("fn main() -> number {\n    \"hello\"\n}");
}

#[test]
fn test_type_check_undefined_variable() {
    check_has_errors("fn main() -> number {\n    undefined_var\n}");
}

#[test]
fn test_type_check_duplicate_function() {
    check_has_errors("fn foo() -> number { 1 }\nfn foo() -> number { 2 }\nfn main() -> number { foo() }");
}

#[test]
fn test_type_check_if_else_type_match() {
    check_no_errors("fn main() -> number {\n    let x = if true { 1 } else { 2 };\n    x\n}");
}

#[test]
fn test_type_check_if_else_type_mismatch() {
    check_has_errors("fn main() -> number {\n    let x = if true { 1 } else { \"hello\" };\n    x\n}");
}

#[test]
fn test_type_check_match_bool_exhaustive() {
    check_no_errors("fn main() -> number {\n    let x = true;\n    match x {\n        true => 1,\n        false => 0\n    }\n}");
}

#[test]
fn test_type_check_match_bool_non_exhaustive() {
    check_has_errors("fn main() -> number {\n    let x = true;\n    match x {\n        true => 1\n    }\n}");
}

#[test]
fn test_type_check_match_option() {
    check_no_errors("fn main() -> number {\n    let x = Some(42);\n    match x {\n        Some(v) => v,\n        None => 0\n    }\n}");
}

#[test]
fn test_type_check_match_enum_exhaustive() {
    check_no_errors("enum Color { Red, Green, Blue }\nfn main() -> number {\n    let c = Color::Red;\n    match c {\n        Color::Red => 1,\n        Color::Green => 2,\n        Color::Blue => 3\n    }\n}");
}

#[test]
fn test_type_check_match_enum_non_exhaustive() {
    check_has_errors("enum Color { Red, Green, Blue }\nfn main() -> number {\n    let c = Color::Red;\n    match c {\n        Color::Red => 1,\n        Color::Green => 2\n    }\n}");
}

#[test]
fn test_type_check_reference_and_deref() {
    check_no_errors("fn main() -> number {\n    let x = 42;\n    let r = &x;\n    *r\n}");
}

#[test]
fn test_type_check_generic_function() {
    check_no_errors("fn id(x) { x }\nfn main() -> number {\n    let a = id(42);\n    a\n}");
}

#[test]
fn test_type_check_higher_order_function() {
    check_no_errors("fn apply(f: fn(number) -> number, x: number) -> number { f(x) }\nfn inc(n: number) -> number { n + 1 }\nfn main() -> number { apply(inc, 42) }");
}

#[test]
fn test_type_check_struct_constructor() {
    check_no_errors("struct Point { x: number, y: number }\nfn main() -> number {\n    let p = Point { x: 1, y: 2 };\n    p.x\n}");
}

#[test]
fn test_type_check_closure() {
    check_no_errors("fn main() -> number {\n    let f = |x| { x + 1 };\n    f(42)\n}");
}
