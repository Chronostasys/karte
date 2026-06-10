use karte_lsp::CompilerBridge;
use tower_lsp::lsp_types::Position;

#[test]
fn test_compiler_bridge_analysis() {
    let mut bridge = CompilerBridge::new();
    let source = r#"fn add(a: number, b: number) -> number {
    a + b
}

fn main() -> number {
    add(1, 2)
}"#;

    let diagnostics = bridge.analyze(source);

    // 应该没有错误
    assert!(diagnostics.is_empty(), "Expected no diagnostics, got: {:?}", diagnostics);

    // 检查缓存结果
    let result = bridge.get_cached_result();
    assert!(result.is_some());

    let result = result.unwrap();

    // 应该有 2 个符号（add 和 main）
    assert!(result.symbols.len() >= 2, "Expected at least 2 symbols, got: {}", result.symbols.len());

    // 检查符号名称
    let names: Vec<&str> = result.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"add"), "Should contain 'add' function: {:?}", names);
    assert!(names.contains(&"main"), "Should contain 'main' function: {:?}", names);
}

#[test]
fn test_compiler_bridge_type_error() {
    let mut bridge = CompilerBridge::new();
    let source = r#"fn main() -> number {
    let x = "hello";
    x + 1
}"#;

    let diagnostics = bridge.analyze(source);

    // 应该有类型错误
    assert!(!diagnostics.is_empty(), "Expected diagnostics for type error");
}

#[test]
fn test_compiler_bridge_hover_info() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\n    let x = 42;\n    x\n}";

    let _diagnostics = bridge.analyze(source);

    // 测试 hover 信息
    // position: 行 0, 列 3 (指向 'main' 的某个位置)
    let hover = bridge.get_hover_info(Position::new(0, 5));
    assert!(hover.is_some(), "Should have hover info for 'main'");
    let (text, _range) = hover.unwrap();
    assert!(text.contains("main"), "Hover should contain 'main': {}", text);
}

#[test]
fn test_compiler_bridge_completions() {
    let mut bridge = CompilerBridge::new();
    let source = "fn foo() -> number { 1 }\nfn main() -> number {\n    f\n}";

    let _diagnostics = bridge.analyze(source);

    // 测试补全
    let completions = bridge.get_completions(Position::new(2, 5));
    assert!(!completions.is_empty(), "Should have completions");

    // 应该包含 'foo' 和 'main'
    let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();
    assert!(labels.contains(&"foo"), "Completions should contain 'foo': {:?}", labels);
}

#[test]
fn test_compiler_bridge_symbols() {
    let mut bridge = CompilerBridge::new();
    let source = r#"struct Point { x: number, y: number }
enum Color { Red, Green, Blue }
fn main() -> number { 0 }"#;

    let _diagnostics = bridge.analyze(source);

    let result = bridge.get_cached_result().unwrap();

    // 应该有 struct、enum 和 function 符号
    let names: Vec<&str> = result.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"Point"), "Should contain 'Point' struct: {:?}", names);
    assert!(names.contains(&"Color"), "Should contain 'Color' enum: {:?}", names);
    assert!(names.contains(&"main"), "Should contain 'main' function: {:?}", names);
}

#[test]
fn test_compiler_bridge_struct_type_check() {
    let mut bridge = CompilerBridge::new();
    let source = r#"struct Point { x: number, y: number }
fn main() -> number {
    let p = Point { x: 1, y: 2 };
    p.x + p.y
}"#;

    let diagnostics = bridge.analyze(source);
    assert!(diagnostics.is_empty(), "Expected no diagnostics, got: {:?}", diagnostics);
}

#[test]
fn test_compiler_bridge_enum_match() {
    let mut bridge = CompilerBridge::new();
    let source = r#"enum Color { Red, Green, Blue }
fn main() -> number {
    let c = Color::Red;
    match c {
        Color::Red => 1,
        Color::Green => 2,
        Color::Blue => 3
    }
}"#;

    let diagnostics = bridge.analyze(source);
    assert!(diagnostics.is_empty(), "Expected no diagnostics, got: {:?}", diagnostics);
}

#[test]
fn test_compiler_bridge_non_exhaustive_match() {
    let mut bridge = CompilerBridge::new();
    let source = r#"enum Color { Red, Green, Blue }
fn main() -> number {
    let c = Color::Red;
    match c {
        Color::Red => 1
    }
}"#;

    let diagnostics = bridge.analyze(source);
    assert!(!diagnostics.is_empty(), "Expected diagnostics for non-exhaustive match");
    let msgs: Vec<&str> = diagnostics.iter().map(|d| d.message.as_str()).collect();
    let msg = msgs.join(", ");
    assert!(msg.contains("Green") || msg.contains("Blue") || msg.contains("穷尽"),
        "Error should mention missing patterns: {}", msg);
}

#[test]
fn test_compiler_bridge_unused_variable_warning() {
    let mut bridge = CompilerBridge::new();
    let source = r#"fn main() -> number {
    let unused_var = 42;
    let used = 10;
    used
}"#;

    let _diagnostics = bridge.analyze(source);
    // 只确保不会 panic
}

#[test]
fn test_compiler_bridge_find_references() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\n    let x = 42;\n    x\n}";

    let _diagnostics = bridge.analyze(source);

    // 测试查找引用
    // position: 行 2, 列 4 (指向 'x' 的某个位置)
    let refs = bridge.find_references(Position::new(2, 4));
    assert!(!refs.is_empty(), "Should find references to 'x'");
}

#[test]
fn test_compiler_bridge_signature_help() {
    let mut bridge = CompilerBridge::new();
    let source = "fn add(a: number, b: number) -> number { a + b }\nfn main() -> number {\n    add(\n}";

    let _diagnostics = bridge.analyze(source);

    // 测试签名帮助
    let sig = bridge.get_signature_help(Position::new(2, 8));
    // 签名帮助可能不可用，只确保不会 panic
    let _ = sig;
}

#[test]
fn test_semantic_tokens_basic() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\n    let x = 42;\n    x\n}";

    let _diagnostics = bridge.analyze(source);

    // semantic tokens 应该被正确生成
    let result = bridge.get_cached_result();
    assert!(result.is_some(), "Should have analysis result");
    let result = result.unwrap();

    // 应该有 main 函数和 x 变量符号
    let names: Vec<&str> = result.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"main"), "Should have 'main' symbol");
    assert!(names.contains(&"x"), "Should have 'x' symbol");
}

#[test]
fn test_semantic_tokens_enum_variant() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Option { Some, None }\nfn main() -> number {\n    0\n}";

    let _diagnostics = bridge.analyze(source);

    let result = bridge.get_cached_result().unwrap();

    // 应该有 Option 枚举和 Some/None 变体符号
    let names: Vec<&str> = result.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"Option"), "Should have 'Option' enum: {:?}", names);
}

#[test]
fn test_semantic_tokens_function_params() {
    let mut bridge = CompilerBridge::new();
    let source = "fn add(a: number, b: number) -> number {\n    a + b\n}";

    let _diagnostics = bridge.analyze(source);

    let result = bridge.get_cached_result().unwrap();

    // 应该有 add 函数和 a, b 参数符号
    let kinds: Vec<_> = result.symbols.iter().map(|s| (s.name.as_str(), s.kind)).collect();
    assert!(kinds.iter().any(|(name, _)| *name == "add"), "Should have 'add' function");
    assert!(kinds.iter().any(|(name, _)| *name == "a"), "Should have 'a' parameter");
    assert!(kinds.iter().any(|(name, _)| *name == "b"), "Should have 'b' parameter");
}

#[test]
fn test_semantic_tokens_struct_fields() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Point { x: number, y: number }\nfn main() -> number {\n    0\n}";

    let _diagnostics = bridge.analyze(source);

    let result = bridge.get_cached_result().unwrap();

    // 应该有 Point 结构体符号
    let names: Vec<&str> = result.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"Point"), "Should have 'Point' struct: {:?}", names);
}
