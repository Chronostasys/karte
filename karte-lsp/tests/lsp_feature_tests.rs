use karte_lsp::compiler_bridge::CompilerBridge;
use tower_lsp::lsp_types::Position;

#[test]
fn test_analyze_simple_function() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number { 42 }";
    let diagnostics = bridge.analyze(source);
    assert!(diagnostics.is_empty(), "Simple function should not have errors");
}

#[test]
fn test_analyze_struct_definition() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Point { x: number, y: number }\nfn main() -> number { 0 }";
    let diagnostics = bridge.analyze(source);
    assert!(diagnostics.is_empty(), "Struct definition should not have errors");
}

#[test]
fn test_analyze_enum_definition() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Color { Red, Green, Blue }\nfn main() -> number { 0 }";
    let diagnostics = bridge.analyze(source);
    assert!(diagnostics.is_empty(), "Enum definition should not have errors");
}

#[test]
fn test_analyze_type_error() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number { \"hello\" }";
    let diagnostics = bridge.analyze(source);
    assert!(!diagnostics.is_empty(), "Type mismatch should have errors");
}

#[test]
fn test_document_symbols() {
    let mut bridge = CompilerBridge::new();
    let source = "fn foo() -> number { 1 }\nfn bar() -> number { 2 }";
    let _diagnostics = bridge.analyze(source);
    let symbols = bridge.get_document_symbols();
    assert!(symbols.iter().any(|s| s.name == "foo"), "Should find 'foo'");
    assert!(symbols.iter().any(|s| s.name == "bar"), "Should find 'bar'");
}

#[test]
fn test_hover_function() {
    let mut bridge = CompilerBridge::new();
    let source = "fn add(a: number, b: number) -> number { a + b }";
    let _diagnostics = bridge.analyze(source);
    let hover = bridge.get_hover_info(Position { line: 0, character: 3 });
    assert!(hover.is_some(), "Should have hover info for 'add'");
    if let Some((text, _)) = hover {
        assert!(text.contains("add"), "Hover should contain function name");
    }
}

#[test]
fn test_completions_not_empty() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number { let x = 42;  }";
    let _diagnostics = bridge.analyze(source);
    let completions = bridge.get_completions(Position { line: 0, character: 30 });
    assert!(!completions.is_empty(), "Should have completions");
}

#[test]
fn test_identifier_types() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number { let x = 42; x }";
    let _diagnostics = bridge.analyze(source);
    // x 应该有类型信息（通过 hover 验证）
    let hover = bridge.get_hover_info(Position { line: 0, character: 26 });
    assert!(hover.is_some(), "Should have type info for 'x'");
}

#[test]
fn test_analyze_match_expression() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet x = 1;\nmatch x {\n1 => 10,\n_ => 20\n}\n}";
    let diagnostics = bridge.analyze(source);
    assert!(diagnostics.is_empty(), "Match expression should not have errors");
}

#[test]
fn test_analyze_option_match() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet x = Some(42);\nmatch x {\nSome(v) => v,\nNone => 0\n}\n}";
    let diagnostics = bridge.analyze(source);
    assert!(diagnostics.is_empty(), "Option match should not have errors");
}
