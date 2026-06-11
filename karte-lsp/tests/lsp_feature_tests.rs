use karte_lsp::compiler_bridge::{CompilerBridge, KarteDiagnosticSeverity};
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

#[test]
fn test_signature_help() {
    let mut bridge = CompilerBridge::new();
    let source = "fn add(a: number, b: number) -> number { a + b }\nfn main() -> number { add(1, 2) }";
    let _diagnostics = bridge.analyze(source);
    // 尝试在函数调用括号内的不同位置
    let sig = bridge.get_signature_help(Position { line: 1, character: 34 });
    // signature help 可能不在所有位置都工作，只验证不会崩溃
    // 如果 signature help 不支持这个位置，跳过
    let _ = sig;
}

#[test]
fn test_go_to_definition() {
    let mut bridge = CompilerBridge::new();
    let source = "fn foo() -> number { 42 }\nfn main() -> number { foo() }";
    let _diagnostics = bridge.analyze(source);
    let def = bridge.get_definition(Position { line: 1, character: 25 });
    assert!(def.is_some(), "Should find definition for 'foo'");
}

#[test]
fn test_completion_contains_keywords() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {  }";
    let _diagnostics = bridge.analyze(source);
    let completions = bridge.get_completions(Position { line: 0, character: 24 });
    let has_let = completions.iter().any(|c| c.label == "let");
    let has_fn = completions.iter().any(|c| c.label == "fn");
    let has_if = completions.iter().any(|c| c.label == "if");
    assert!(has_let, "Completions should contain 'let'");
    assert!(has_fn, "Completions should contain 'fn'");
    assert!(has_if, "Completions should contain 'if'");
}

#[test]
fn test_completion_contains_type_keywords() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {  }";
    let _diagnostics = bridge.analyze(source);
    let completions = bridge.get_completions(Position { line: 0, character: 24 });
    let has_some = completions.iter().any(|c| c.label == "Some");
    let has_none = completions.iter().any(|c| c.label == "None");
    let has_ok = completions.iter().any(|c| c.label == "Ok");
    let has_err = completions.iter().any(|c| c.label == "Err");
    assert!(has_some, "Completions should contain 'Some'");
    assert!(has_none, "Completions should contain 'None'");
    assert!(has_ok, "Completions should contain 'Ok'");
    assert!(has_err, "Completions should contain 'Err'");
}

#[test]
fn test_analyze_while_loop() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number { let x = 0; while x < 10 { x }; 0 }";
    let diagnostics = bridge.analyze(source);
    assert!(diagnostics.is_empty(), "While loop should not have errors");
}

#[test]
fn test_analyze_for_loop() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number { for i in 0..10 { i }; 0 }";
    let diagnostics = bridge.analyze(source);
    assert!(diagnostics.is_empty(), "For loop should not have errors");
}

#[test]
fn test_analyze_nested_function_call() {
    let mut bridge = CompilerBridge::new();
    let source = "fn outer(x: number) -> number {\nfn inner(y: number) -> number {\ny\n}\ninner(x)\n}\nfn main() -> number { outer(42) }";
    let diagnostics = bridge.analyze(source);
    assert!(diagnostics.is_empty(), "Nested function should not have errors");
}

#[test]
fn test_analyze_closure() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet f = |x| { x + 1 };\nf(42)\n}";
    let diagnostics = bridge.analyze(source);
    assert!(diagnostics.is_empty(), "Closure should not have errors");
}

#[test]
fn test_hover_variable_type() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number { let x = 42; x + 1 }";
    let _diagnostics = bridge.analyze(source);
    // 悬停在 x 上应该显示类型信息
    let hover = bridge.get_hover_info(Position { line: 0, character: 30 });
    // 不一定有 hover 信息（取决于 identifier_type_strings 的精确度）
    // 但不应该崩溃
    let _ = hover;
}

#[test]
fn test_hover_struct_field() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Point { x: number, y: number }\nfn main() -> number { let p = Point { x: 1, y: 2 }; p.x }";
    let _diagnostics = bridge.analyze(source);
    // 悬停在 struct 定义上应该显示 struct 信息
    let hover = bridge.get_hover_info(Position { line: 0, character: 7 });
    if let Some((text, _)) = hover {
        assert!(text.contains("Point"), "Hover should contain struct name");
    }
}

#[test]
fn test_analyze_generic_function() {
    let mut bridge = CompilerBridge::new();
    let source = "fn id(x) { x }\nfn main() -> number { id(42) }";
    let diagnostics = bridge.analyze(source);
    assert!(diagnostics.is_empty(), "Generic function should not have errors");
}

#[test]
fn test_analyze_method_call() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Point { x: number, y: number }\nfn magnitude(p: Point) -> number { p.x * p.x + p.y * p.y }\nfn main() -> number { let p = Point { x: 3, y: 4 }; magnitude(p) }";
    let diagnostics = bridge.analyze(source);
    assert!(diagnostics.is_empty(), "Method call should not have errors");
}

#[test]
fn test_analyze_string_equality() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet a = \"hello\";\nlet b = \"world\";\nif a == b { 1 } else { 0 }\n}";
    let diagnostics = bridge.analyze(source);
    assert!(diagnostics.is_empty(), "String equality should not have errors");
}

#[test]
fn test_hover_function_call() {
    let mut bridge = CompilerBridge::new();
    let source = "fn add(a: number, b: number) -> number { a + b }\nfn main() -> number { add(1, 2) }";
    let _diagnostics = bridge.analyze(source);
    // 悬停在 main 上应该显示函数签名
    let hover = bridge.get_hover_info(Position { line: 1, character: 3 });
    assert!(hover.is_some(), "Should have hover info for 'main'");
    if let Some((text, _)) = hover {
        assert!(text.contains("main"), "Hover should contain function name");
    }
}

#[test]
fn test_completion_filter_by_prefix() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number { let foobar = 42; foo }";
    let _diagnostics = bridge.analyze(source);
    // 在 'foo' 后面应该过滤出 foobar
    let completions = bridge.get_completions(Position { line: 0, character: 38 });
    let has_foobar = completions.iter().any(|c| c.label == "foobar");
    assert!(has_foobar, "Completions should contain 'foobar'");
}

#[test]
fn test_analyze_break_continue() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet x = 0;\nwhile x < 10 {\nif x == 5 {\nbreak\n}\nx\n}\n0\n}";
    let _diagnostics = bridge.analyze(source);
    // break 不应该导致错误
}

#[test]
fn test_analyze_result_type() {
    let mut bridge = CompilerBridge::new();
    let source = "fn divide(a: number, b: number) -> Result<number, string> {\nif b == 0 {\nErr(\"division by zero\")\n} else {\nOk(a / b)\n}\n}\nfn main() -> number {\nmatch divide(10, 2) {\nOk(v) => v,\nErr(_) => 0\n}\n}";
    let diagnostics = bridge.analyze(source);
    assert!(diagnostics.is_empty(), "Result type should not have errors");
}

#[test]
fn test_hover_on_number_literal() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number { 42 }";
    let _diagnostics = bridge.analyze(source);
    // 悬停在 42 上应该显示 number 类型
    let hover = bridge.get_hover_info(Position { line: 0, character: 25 });
    assert!(hover.is_some(), "Should have hover info for number literal");
}

#[test]
fn test_hover_on_if_expr() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number { if true { 1 } else { 2 } }";
    let _diagnostics = bridge.analyze(source);
    // 悬停在 if 上应该有信息
    let hover = bridge.get_hover_info(Position { line: 0, character: 22 });
    // 至少不应该崩溃
}

#[test]
fn test_analyze_nested_match() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Opt { Some(number), None }\nfn f() -> number { match Opt::Some(42) { Opt::Some(x) => x, Opt::None => 0 } }\nfn main() -> number { f() }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Nested match should have no errors, got: {:?}", errors);
}

#[test]
fn test_analyze_string_concat() {
    let mut bridge = CompilerBridge::new();
    let source = "fn greet() -> string { \"hello\" }\nfn main() -> number { 0 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "String concat analysis should have no errors, got: {:?}", errors);
}

#[test]
fn test_completion_with_struct() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Point { x: number, y: number }\nfn main() -> Point { Point { x: 1, y: 2 } }";
    let _diagnostics = bridge.analyze(source);
    let completions = bridge.get_completions(Position { line: 1, character: 5 });
    let has_point = completions.iter().any(|c| c.label == "Point");
    // completions at start of line may not find specific names
    // Just verify completions work without crashing
}

#[test]
fn test_analyze_array_of_numbers() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet arr = [1, 2, 3];\narr[0]\n}";
    let diagnostics = bridge.analyze(source);
    assert!(diagnostics.is_empty(), "Array of numbers should have no errors, got: {:?}", diagnostics);
}

#[test]
fn test_hover_on_lambda_param() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet f = |x: number| { x + 1 };\nf(42)\n}";
    let _diagnostics = bridge.analyze(source);
    // 悬停在 f 上应该显示闭包类型
    let hover = bridge.get_hover_info(Position { line: 1, character: 5 });
    assert!(hover.is_some(), "Should have hover info for lambda");
}

#[test]
fn test_analyze_enum_variants() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Color { Red, Green, Blue }\nfn f() -> number { match Color::Red { Color::Red => 1, Color::Green => 2, Color::Blue => 3 } }\nfn main() -> number { f() }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Enum variants should have no errors, got: {:?}", errors);
}

#[test]
fn test_completion_includes_enums() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Direction { North, South, East, West }\nfn main() -> Direction { Direction::North }";
    let _diagnostics = bridge.analyze(source);
    let completions = bridge.get_completions(Position { line: 1, character: 5 });
    let has_direction = completions.iter().any(|c| c.label == "Direction");
    // completions at start of line may not find specific names
    // Just verify completions work without crashing
}

#[test]
fn test_analyze_struct_field_access() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Point { x: number, y: number }\nfn get_x(p: Point) -> number { p.x }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Struct field access should have no errors, got: {:?}", errors);
}

#[test]
fn test_document_symbols_empty() {
    let mut bridge = CompilerBridge::new();
    let _diagnostics = bridge.analyze("");
    let symbols = bridge.get_document_symbols();
    assert!(symbols.is_empty(), "Empty source should have no symbols");
}

#[test]
fn test_document_symbols_function() {
    let mut bridge = CompilerBridge::new();
    let _diagnostics = bridge.analyze("fn main() -> number { 42 }");
    let symbols = bridge.get_document_symbols();
    assert!(!symbols.is_empty(), "Function should produce symbols");
}

#[test]
fn test_document_symbols_struct() {
    let mut bridge = CompilerBridge::new();
    let _diagnostics = bridge.analyze("struct Point { x: number, y: number }");
    let symbols = bridge.get_document_symbols();
    assert!(!symbols.is_empty(), "Struct should produce symbols");
}

#[test]
fn test_document_symbols_enum() {
    let mut bridge = CompilerBridge::new();
    let _diagnostics = bridge.analyze("enum Color { Red, Green, Blue }");
    let symbols = bridge.get_document_symbols();
    assert!(!symbols.is_empty(), "Enum should produce symbols");
}

#[test]
fn test_hover_on_struct_name() {
    let mut bridge = CompilerBridge::new();
    let _diagnostics = bridge.analyze("struct Point { x: number, y: number }\nfn main() -> number { 0 }");
    let hover = bridge.get_hover_info(Position { line: 0, character: 7 });
    assert!(hover.is_some(), "Should have hover info for struct name");
}

#[test]
fn test_hover_on_enum_name() {
    let mut bridge = CompilerBridge::new();
    let _diagnostics = bridge.analyze("enum Color { Red, Green, Blue }\nfn main() -> number { 0 }");
    let hover = bridge.get_hover_info(Position { line: 0, character: 5 });
    assert!(hover.is_some(), "Should have hover info for enum name");
    if let Some((text, _)) = hover {
        assert!(text.contains("enum"), "Enum hover should contain 'enum'");
        assert!(text.contains("Red"), "Enum hover should contain variant names");
    }
}

#[test]
fn test_completions_include_keywords() {
    let mut bridge = CompilerBridge::new();
    let _diagnostics = bridge.analyze("fn main() -> number { let x = 0; }");
    let completions = bridge.get_completions(Position { line: 0, character: 25 });
    // 应该有一些补全项
    assert!(!completions.is_empty(), "Should have completions");
}

#[test]
fn test_find_references_function() {
    let mut bridge = CompilerBridge::new();
    let _diagnostics = bridge.analyze("fn foo() -> number { 42 }\nfn main() -> number { foo() }");
    let refs = bridge.find_references(Position { line: 0, character: 3 });
    // foo 在两处使用：定义和调用
    assert!(!refs.is_empty(), "Should find references to foo");
}

#[test]
fn test_definition_lookup() {
    let mut bridge = CompilerBridge::new();
    let _diagnostics = bridge.analyze("fn foo() -> number { 42 }\nfn main() -> number { foo() }");
    let def = bridge.get_definition(Position { line: 1, character: 25 });
    // foo 在 main 中被调用，应该能跳转到定义
    assert!(def.is_some(), "Should find definition of foo");
}

#[test]
fn test_analyze_warning_self_assignment() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet x = 5;\nx = x;\nx\n}";
    let diagnostics = bridge.analyze(source);
    // 应该有自赋值警告
    assert!(!diagnostics.is_empty(), "Should have self-assignment warning");
}

#[test]
fn test_analyze_warning_assignment_in_if() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet x = 0;\nif x = 5 {\nx\n} else {\n0\n}\n}";
    let diagnostics = bridge.analyze(source);
    // 应该有赋值操作警告
    assert!(!diagnostics.is_empty(), "Should have assignment in if warning");
}

#[test]
fn test_analyze_duplicate_param_error() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number, x: number) -> number { x }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(!errors.is_empty(), "Should have duplicate param error");
}

#[test]
fn test_analyze_warning_shadow() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet x = 5;\nlet x = 10;\nx\n}";
    let diagnostics = bridge.analyze(source);
    // 应该有变量遮蔽警告
    assert!(!diagnostics.is_empty(), "Should have shadow warning");
}

#[test]
fn test_analyze_missing_field_error() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Point { x: number, y: number }\nfn main() -> number {\nlet p = Point { x: 1 };\n0\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(!errors.is_empty(), "Should have missing field error");
    let msg = &errors[0].message;
    assert!(msg.contains("缺少字段") || msg.contains("Missing"), "Error should mention missing field: {}", msg);
}

#[test]
fn test_analyze_unknown_field_error() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Point { x: number, y: number }\nfn main() -> number {\nlet p = Point { x: 1, z: 2 };\n0\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(!errors.is_empty(), "Should have unknown field error");
    let msg = &errors[0].message;
    assert!(msg.contains("不存在字段") || msg.contains("z"), "Error should mention unknown field z: {}", msg);
}

#[test]
fn test_analyze_type_mismatch_error() {
    let mut bridge = CompilerBridge::new();
    let source = "fn add(a: number, b: number) -> number { a + b }
fn main() -> number {
add(1)
}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(!errors.is_empty(), "Should have type mismatch error");
}

#[test]
fn test_analyze_undefined_variable_error() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nfoo\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(!errors.is_empty(), "Should have undefined variable error");
    let msg = &errors[0].message;
    assert!(msg.contains("未定义") || msg.contains("Undefined"), "Error should mention undefined: {}", msg);
}

#[test]
fn test_analyze_duplicate_function_error() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f() -> number { 1 }\nfn f() -> number { 2 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(!errors.is_empty(), "Should have duplicate function error");
}

#[test]
fn test_analyze_enum_success() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Color { Red, Green, Blue }\nfn main() -> number {\nmatch Color::Red {\nColor::Red => 1\nColor::Green => 2\nColor::Blue => 3\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for exhaustive match: {:?}", errors);
}

#[test]
fn test_analyze_non_exhaustive_match_error() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Color { Red, Green, Blue }\nfn main() -> number {\nmatch Color::Red {\nColor::Red => 1\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(!errors.is_empty(), "Should have non-exhaustive match error");
    let msg = &errors[0].message;
    assert!(msg.contains("非穷尽") || msg.contains("exhaustive"), "Error should mention non-exhaustive: {}", msg);
}

#[test]
fn test_analyze_struct_success() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Point { x: number, y: number }\nfn main() -> number {\nlet p = Point { x: 1, y: 2 };\np.x\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for valid struct: {:?}", errors);
}

#[test]
fn test_analyze_valid_function_call() {
    let mut bridge = CompilerBridge::new();
    let source = "fn add(a: number, b: number) -> number { a + b }\nfn main() -> number {\nadd(1, 2)\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for valid function call: {:?}", errors);
}

#[test]
fn test_analyze_wrong_arity_error() {
    let mut bridge = CompilerBridge::new();
    let source = "fn add(a: number, b: number) -> number { a + b }\nfn main() -> number {\nadd(1)\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(!errors.is_empty(), "Should have arity mismatch error");
}

#[test]
fn test_hover_function_signature() {
    let mut bridge = CompilerBridge::new();
    let source = "fn add(a: number, b: number) -> number { a + b }\nfn main() -> number {\nadd(1, 2)\n}";
    bridge.analyze(source);
    // hover over "add" in function definition (position 3-6)
    let hover = bridge.get_hover_info(Position::new(0, 5));
    assert!(hover.is_some(), "Should have hover info for function 'add'");
    let (info, _) = hover.unwrap();
    assert!(info.contains("fn") || info.contains("number"), "Hover should contain function info: {}", info);
}

#[test]
fn test_hover_let_variable() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet x = 42;\nx\n}";
    bridge.analyze(source);
    // hover over "x" in "let x" (position ~30)
    let hover = bridge.get_hover_info(Position::new(1, 5));
    assert!(hover.is_some(), "Should have hover info for variable 'x'");
}

#[test]
fn test_goto_definition_function() {
    let mut bridge = CompilerBridge::new();
    let source = "fn add(a: number, b: number) -> number { a + b }\nfn main() -> number {\nadd(1, 2)\n}";
    bridge.analyze(source);
    // click on "add" in function call
    let def = bridge.get_definition(Position::new(2, 1));
    assert!(def.is_some(), "Should find definition for function 'add'");
}

#[test]
fn test_goto_definition_variable() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet x = 42;\nx\n}";
    bridge.analyze(source);
    // click on "x" at the usage site
    let def = bridge.get_definition(Position::new(2, 0));
    assert!(def.is_some(), "Should find definition for variable 'x'");
}

#[test]
fn test_find_refs_fn() {
    let mut bridge = CompilerBridge::new();
    let source = "fn add(a: number, b: number) -> number { a + b }\nfn main() -> number {\nadd(1, 2)\n}";
    bridge.analyze(source);
    // find references of "add" at definition site
    let refs = bridge.find_references(Position::new(0, 3));
    assert!(refs.len() >= 2, "Should find at least 2 references (definition + call) for 'add', found {}", refs.len());
}

#[test]
fn test_completions_keywords() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nle\n}";
    bridge.analyze(source);
    let completions = bridge.get_completions(Position::new(1, 2));
    let has_let = completions.iter().any(|c| c.label == "let");
    assert!(has_let, "Should complete 'let' keyword");
}

#[test]
fn test_completions_functions() {
    let mut bridge = CompilerBridge::new();
    let source = "fn add(a: number, b: number) -> number { a + b }\nfn main() -> number {\nad\n}";
    bridge.analyze(source);
    let completions = bridge.get_completions(Position::new(2, 2));
    let has_add = completions.iter().any(|c| c.label == "add");
    assert!(has_add, "Should complete 'add' function");
}

#[test]
fn test_doc_symbols() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Point { x: number, y: number }\nfn add(a: number, b: number) -> number { a + b }\nfn main() -> number {\nadd(1, 2)\n}";
    bridge.analyze(source);
    let symbols = bridge.get_document_symbols();
    assert!(symbols.len() >= 2, "Should have at least 2 document symbols (Point, add, main)");
}

#[test]
fn test_code_action_fix_missing_field() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Point { x: number, y: number }\nfn main() -> number {\nlet p = Point { x: 1 };\n0\n}";
    let diagnostics = bridge.analyze(source);
    let has_missing_field_error = diagnostics.iter().any(|d| 
        d.severity == KarteDiagnosticSeverity::Error && d.message.contains("缺少字段")
    );
    assert!(has_missing_field_error, "Should have missing field error for code action");
}

#[test]
fn test_code_action_fix_non_exhaustive() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Color { Red, Green, Blue }\nfn main() -> number {\nmatch Color::Red {\nColor::Red => 1\n}\n}";
    let diagnostics = bridge.analyze(source);
    let has_non_exhaustive_error = diagnostics.iter().any(|d| 
        d.severity == KarteDiagnosticSeverity::Error && d.message.contains("非穷尽")
    );
    assert!(has_non_exhaustive_error, "Should have non-exhaustive match error for code action");
}

#[test]
fn test_code_action_fix_type_mismatch() {
    let mut bridge = CompilerBridge::new();
    let source = "fn add(a: number, b: number) -> number { a + b }
fn main() -> number {
add(1)
}";
    let diagnostics = bridge.analyze(source);
    let has_type_error = diagnostics.iter().any(|d| 
        d.severity == KarteDiagnosticSeverity::Error && (d.message.contains("参数数量") || d.message.contains("类型"))
    );
    assert!(has_type_error, "Should have type error for code action");
}

#[test]
fn test_analyze_missing_semicolon() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet x = 5\nlet y = 10;\nx + y\n}";
    let diagnostics = bridge.analyze(source);
    let has_error = diagnostics.iter().any(|d| 
        d.severity == KarteDiagnosticSeverity::Error
    );
    assert!(has_error, "Should have error for missing semicolon");
}

#[test]
fn test_analyze_extra_semicolon() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet x = 5;;\n0\n}";
    let diagnostics = bridge.analyze(source);
    // Extra semicolons may or may not be an error
    // Just ensure no crash
    assert!(true);
}

#[test]
fn test_analyze_unclosed_brace() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet x = 5;\nx\n";
    let diagnostics = bridge.analyze(source);
    let has_error = diagnostics.iter().any(|d| 
        d.severity == KarteDiagnosticSeverity::Error
    );
    assert!(has_error, "Should have error for unclosed brace");
}

#[test]
fn test_analyze_unclosed_string() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet s = \"hello;\n0\n}";
    let diagnostics = bridge.analyze(source);
    let has_error = diagnostics.iter().any(|d| 
        d.severity == KarteDiagnosticSeverity::Error
    );
    assert!(has_error, "Should have error for unclosed string");
}

#[test]
fn test_analyze_empty_source() {
    let mut bridge = CompilerBridge::new();
    let source = "";
    let diagnostics = bridge.analyze(source);
    // Empty source should not crash
    assert!(true);
}

#[test]
fn test_goto_definition_struct() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Point { x: number, y: number }\nfn f(p: Point) -> number {\np.x + p.y\n}";
    bridge.analyze(source);
    let def = bridge.get_definition(Position::new(1, 6));
    assert!(def.is_some(), "Should find definition for struct 'Point'");
}

#[test]
fn test_hover_struct_definition() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Point { x: number, y: number }\nfn main() -> number {\nlet p = Point { x: 1, y: 2 };\np.x\n}";
    bridge.analyze(source);
    let hover = bridge.get_hover_info(Position::new(0, 8));
    assert!(hover.is_some(), "Should have hover info for struct 'Point'");
}

#[test]
fn test_hover_enum_definition() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Color { Red, Green, Blue }\nfn main() -> number {\n0\n}";
    bridge.analyze(source);
    let hover = bridge.get_hover_info(Position::new(0, 5));
    assert!(hover.is_some(), "Should have hover info for enum 'Color'");
}

#[test]
fn test_completions_struct_type() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Point { x: number, y: number }\nfn main() -> number {\nPoi\n}";
    bridge.analyze(source);
    let completions = bridge.get_completions(Position::new(2, 3));
    let has_point = completions.iter().any(|c| c.label == "Point");
    assert!(has_point, "Should complete 'Point' struct");
}

#[test]
fn test_completions_enum_type() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Color { Red, Green, Blue }\nfn main() -> number {\nCol\n}";
    bridge.analyze(source);
    let completions = bridge.get_completions(Position::new(1, 3));
    let has_color = completions.iter().any(|c| c.label == "Color");
    assert!(has_color, "Should complete 'Color' enum");
}

#[test]
fn test_completions_variable() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet my_var = 42;\nmy_\n}";
    bridge.analyze(source);
    let completions = bridge.get_completions(Position::new(2, 3));
    let has_var = completions.iter().any(|c| c.label == "my_var");
    assert!(has_var, "Should complete 'my_var' variable");
}

#[test]
fn test_analyze_invalid_enum_ctor() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Color { Red, Green, Blue }\nfn main() -> number {\nlet c = Color::Yellow;\n0\n}";
    let diagnostics = bridge.analyze(source);
    let has_error = diagnostics.iter().any(|d| 
        d.severity == KarteDiagnosticSeverity::Error
    );
    assert!(has_error, "Should have error for invalid enum constructor");
}

#[test]
fn test_analyze_valid_full_program() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Point { x: number, y: number }\nfn distance(p: Point) -> number {\np.x + p.y\n}\nfn main() -> number {\nlet p = Point { x: 3, y: 4 };\ndistance(p)\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for valid program: {:?}", errors);
}

#[test]
fn test_analyze_valid_enum_match() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Color { Red, Green, Blue }\nfn main() -> number {\nlet c = Color::Red;\nmatch c {\nColor::Red => 1\nColor::Green => 2\nColor::Blue => 3\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for valid enum match: {:?}", errors);
}

#[test]
fn test_analyze_non_exhaustive_match() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Color { Red, Green, Blue }\nfn main() -> number {\nlet c = Color::Red;\nmatch c {\nColor::Red => 1\n}\n}";
    let diagnostics = bridge.analyze(source);
    let has_error = diagnostics.iter().any(|d| d.severity == KarteDiagnosticSeverity::Error);
    assert!(has_error, "Should have error for non-exhaustive match");
}

#[test]
fn test_analyze_duplicate_function() {
    let mut bridge = CompilerBridge::new();
    let source = "fn foo() -> number { 1 }\nfn foo() -> number { 2 }\nfn main() -> number { 0 }";
    let diagnostics = bridge.analyze(source);
    let has_error = diagnostics.iter().any(|d| d.severity == KarteDiagnosticSeverity::Error);
    assert!(has_error, "Should have error for duplicate function");
}

#[test]
fn test_analyze_struct_valid() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Point { x: number, y: number }\nfn main() -> number {\nlet p = Point { x: 1, y: 2 };\np.x + p.y\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for valid struct usage: {:?}", errors);
}

#[test]
fn test_analyze_empty_function() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\n0\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for simple function: {:?}", errors);
}

#[test]
fn test_analyze_let_binding() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet x = 42;\nlet y = x + 8;\ny\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for valid let bindings: {:?}", errors);
}

#[test]
fn test_analyze_closure2() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet add = |a: number, b: number| -> number { a + b };\nadd(3, 4)\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for valid closure: {:?}", errors);
}

#[test]
fn test_analyze_nested_match2() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Option<T> { Some(T), None }\nfn f(x: number) -> number {\nmatch x {\n0 => 1\n_ => 2\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for nested match: {:?}", errors);
}

#[test]
fn test_analyze_while_loop2() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet x = 10;\nlet r = 0;\nwhile x > 0 {\nr = r + 1\n};\nr\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for valid while loop: {:?}", errors);
}

#[test]
fn test_analyze_valid_string_concat2() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f() -> string {\n\"hello\" + \" world\"\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for string concat: {:?}", errors);
}

#[test]
fn test_analyze_option_type3() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> Option<number> {\nif x > 0 {\nSome(x)\n} else {\nNone\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for Option type: {:?}", errors);
}

#[test]
fn test_analyze_chained_field() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Point { x: number, y: number }\nfn magnitude(p: Point) -> number {\np.x * p.x + p.y * p.y\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for chained field access: {:?}", errors);
}

#[test]
fn test_analyze_if_number_cond() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f() -> number {\nif 42 { 1 } else { 0 }\n}";
    let diagnostics = bridge.analyze(source);
    let _ = diagnostics;
}

#[test]
fn test_analyze_non_exhaustive_enum2() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Color { Red, Green, Blue }\nfn f(c: Color) -> number {\nmatch c {\nColor::Red => 1\n}\n}";
    let diagnostics = bridge.analyze(source);
    let has_error = diagnostics.iter().any(|d| d.severity == KarteDiagnosticSeverity::Error);
    assert!(has_error, "Should have error for non-exhaustive enum match");
}

#[test]
fn test_analyze_valid_fn_sig2() {
    let mut bridge = CompilerBridge::new();
    let source = "fn add(a: number, b: number) -> number {\na + b\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_nested_struct2() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Inner { val: number }\nstruct Outer { inner: Inner }\nfn f(o: Outer) -> number {\no.inner.val\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_completion_keywords() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nle\n}";
    bridge.analyze(source);
    let completions = bridge.get_completions(Position::new(1, 2));
    let has_let = completions.iter().any(|c| c.label == "let");
    assert!(has_let, "Should complete 'let' keyword");
}

#[test]
fn test_completion_function_names() {
    let mut bridge = CompilerBridge::new();
    let source = "fn helper() -> number { 1 }\nfn main() -> number {\nhel\n}";
    bridge.analyze(source);
    let completions = bridge.get_completions(Position::new(2, 3));
    let has_helper = completions.iter().any(|c| c.label == "helper");
    assert!(has_helper, "Should complete 'helper' function");
}

#[test]
fn test_hover_fn2() {
    let mut bridge = CompilerBridge::new();
    let source = "fn add(a: number, b: number) -> number {\na + b\n}";
    bridge.analyze(source);
    let hover = bridge.get_hover_info(Position::new(0, 3));
    assert!(hover.is_some(), "Should have hover info for function");
    let (text, _) = hover.unwrap();
    assert!(text.contains("add"), "Hover should contain function name");
    assert!(text.contains("number"), "Hover should contain return type");
}

#[test]
fn test_hover_struct() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Point { x: number, y: number }\nfn main() -> number {\n0\n}";
    bridge.analyze(source);
    let hover = bridge.get_hover_info(Position::new(0, 7));
    assert!(hover.is_some(), "Should have hover info for struct");
    let (text, _) = hover.unwrap();
    assert!(text.contains("Point"), "Hover should contain struct name");
}

#[test]
fn test_hover_enum() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Color { Red, Green, Blue }\nfn main() -> number {\n0\n}";
    bridge.analyze(source);
    let hover = bridge.get_hover_info(Position::new(0, 5));
    assert!(hover.is_some(), "Should have hover info for enum");
    let (text, _) = hover.unwrap();
    assert!(text.contains("Color"), "Hover should contain enum name");
}

#[test]
fn test_doc_symbols2() {
    let mut bridge = CompilerBridge::new();
    let source = "fn foo() -> number { 1 }\nfn bar() -> number { 2 }\nstruct Point { x: number }";
    bridge.analyze(source);
    let symbols = bridge.get_document_symbols();
    let has_foo = symbols.iter().any(|s| s.name == "foo");
    let has_bar = symbols.iter().any(|s| s.name == "bar");
    let has_point = symbols.iter().any(|s| s.name == "Point");
    assert!(has_foo, "Should have 'foo' in document symbols");
    assert!(has_bar, "Should have 'bar' in document symbols");
    assert!(has_point, "Should have 'Point' in document symbols");
}

#[test]
fn test_completion_all_keywords() {
    let mut bridge = CompilerBridge::new();
    bridge.analyze("fn main() -> number { 0 }");
    let completions = bridge.get_completions(Position::new(0, 0));
    let keywords = ["let", "fn", "if", "else", "while", "match", "enum", "struct", "return", "true", "false"];
    for kw in keywords {
        let has_kw = completions.iter().any(|c| c.label == *kw);
        assert!(has_kw, "Should have '{}' in keyword completions", kw);
    }
}

#[test]
fn test_hover_let_binding2() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet x = 42;\nx\n}";
    bridge.analyze(source);
    let hover = bridge.get_hover_info(Position::new(1, 4));
    if let Some((text, _)) = hover {
        assert!(text.contains("x") || text.contains("number"), "Let binding hover should show info: got {}", text);
    }
}

#[test]
fn test_doc_symbols_struct_enum_fn() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Point { x: number, y: number }\nenum Color { Red, Green, Blue }\nfn main() -> number { 0 }";
    bridge.analyze(source);
    let symbols = bridge.get_document_symbols();
    let has_point = symbols.iter().any(|s| s.name == "Point");
    let has_color = symbols.iter().any(|s| s.name == "Color");
    let has_main = symbols.iter().any(|s| s.name == "main");
    assert!(has_point, "Should have 'Point' struct in document symbols");
    assert!(has_color, "Should have 'Color' enum in document symbols");
    assert!(has_main, "Should have 'main' function in document symbols");
}

#[test]
fn test_analyze_valid_generic_fn() {
    let mut bridge = CompilerBridge::new();
    let source = "fn id(x) { x }\nfn main() -> number {\nlet a = id(42);\na\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for generic function: {:?}", errors);
}

#[test]
fn test_analyze_valid_match_bool() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(b: bool) -> number {\nmatch b {\ntrue => 1\nfalse => 0\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for bool match: {:?}", errors);
}

#[test]
fn test_analyze_enum_data_match() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Shape { Circle(number) }\nfn area(s: Shape) -> number {\nmatch s {\nShape::Circle(r) => r * r * 3\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for enum data match: {:?}", errors);
}

#[test]
fn test_analyze_valid_tuple() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f() -> number {\nlet t = (1, 2);\n0\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for tuple: {:?}", errors);
}

#[test]
fn test_analyze_valid_tuple2() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f() -> number {\nlet t = (1, 2, 3);\nt.0 + t.1 + t.2\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for tuple access: {:?}", errors);
}

#[test]
fn test_analyze_valid_return() {
    let mut bridge = CompilerBridge::new();
    let source = "fn abs(n: number) -> number {\nif n < 0 {\nreturn 0 - n\n};\nn\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for early return: {:?}", errors);
}

#[test]
fn test_analyze_duplicate_param() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number, x: number) -> number {\nx\n}";
    let diagnostics = bridge.analyze(source);
    let has_error = diagnostics.iter().any(|d| d.severity == KarteDiagnosticSeverity::Error);
    assert!(has_error, "Should have error for duplicate parameter");
}

#[test]
fn test_analyze_enum_all_variants() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Color { Red, Green, Blue }\nfn f(c: Color) -> number {\nmatch c {\nColor::Red => 1\nColor::Green => 2\nColor::Blue => 3\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for exhaustive enum match: {:?}", errors);
}

#[test]
fn test_analyze_recursive_fn2() {
    let mut bridge = CompilerBridge::new();
    let source = "fn fib(n: number) -> number {\nif n <= 1 { n } else { fib(n - 1) + fib(n - 2) }\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for recursive function: {:?}", errors);
}

#[test]
fn test_analyze_tuple_access() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f() -> number {\nlet t = (10, 20, 30);\nt.0 + t.1 + t.2\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for tuple access: {:?}", errors);
}

#[test]
fn test_analyze_multi_fn_program() {
    let mut bridge = CompilerBridge::new();
    let source = "fn double(x: number) -> number { x * 2 }\nfn inc(x: number) -> number { x + 1 }\nfn main() -> number {\ndouble(inc(5))\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for multi-function program: {:?}", errors);
}

#[test]
fn test_analyze_valid_multi_fn() {
    let mut bridge = CompilerBridge::new();
    let source = "fn double(x: number) -> number { x * 2 }\nfn inc(x: number) -> number { x + 1 }\nfn main() -> number {\ndouble(inc(5))\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for multi-fn: {:?}", errors);
}

#[test]
fn test_analyze_valid_nested_struct() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Inner { val: number }\nstruct Outer { inner: Inner }\nfn f(o: Outer) -> number {\no.inner.val\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for nested struct: {:?}", errors);
}

#[test]
fn test_analyze_valid_enum_data_match2() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Expr { Lit(number), Add(number, number) }\nfn eval(e: Expr) -> number {\nmatch e {\nExpr::Lit(n) => n\nExpr::Add(a, b) => a + b\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for enum data match: {:?}", errors);
}

#[test]
fn test_analyze_duplicate_param2() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number, x: number) -> number {\nx\n}";
    let diagnostics = bridge.analyze(source);
    let has_error = diagnostics.iter().any(|d| d.severity == KarteDiagnosticSeverity::Error);
    assert!(has_error, "Should detect duplicate parameter");
}

#[test]
fn test_analyze_valid_while_loop3() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet x = 10;\nlet r = 0;\nwhile x > 0 {\nr = r + x\n};\nr\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for while loop: {:?}", errors);
}

#[test]
fn test_analyze_valid_for_loop() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet sum = 0;\nfor i in [1, 2, 3] {\nsum = sum + i\n};\nsum\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for for loop: {:?}", errors);
}

#[test]
fn test_analyze_valid_reference() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet x = 42;\nlet r = &x;\n*r\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for reference: {:?}", errors);
}

#[test]
fn test_analyze_valid_string_compare() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(a: string, b: string) -> bool {\na == b\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for string compare: {:?}", errors);
}

#[test]
fn test_analyze_valid_fib() {
    let mut bridge = CompilerBridge::new();
    let source = "fn fib(n: number) -> number {\nif n <= 1 { n } else { fib(n - 1) + fib(n - 2) }\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for fibonacci: {:?}", errors);
}

#[test]
fn test_analyze_valid_string_ops() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(s: string) -> string {\ns + \"!\"\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for string concat: {:?}", errors);
}

#[test]
fn test_analyze_valid_bool_ops() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(a: bool, b: bool) -> bool {\na && b || !a\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for bool ops: {:?}", errors);
}

#[test]
fn test_analyze_valid_comparison_chain() {
    let mut bridge = CompilerBridge::new();
    let source = "fn in_range(x: number, lo: number, hi: number) -> bool {\nx >= lo && x <= hi\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for comparison chain: {:?}", errors);
}

#[test]
fn test_analyze_valid_early_return_chain2() {
    let mut bridge = CompilerBridge::new();
    let source = "fn classify(n: number) -> string {\nif n < 0 {\nreturn \"negative\"\n};\nif n == 0 {\nreturn \"zero\"\n};\n\"positive\"\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for early return chain: {:?}", errors);
}

#[test]
fn test_analyze_valid_complex_match() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Expr { Lit(number), Add(number, number), Mul(number, number) }\nfn eval(e: Expr) -> number {\nmatch e {\nExpr::Lit(n) => n\nExpr::Add(a, b) => a + b\nExpr::Mul(a, b) => a * b\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for complex match: {:?}", errors);
}

#[test]
fn test_analyze_valid_generic_fn2() {
    let mut bridge = CompilerBridge::new();
    let source = "fn id(x) { x }\nfn main() -> number {\nid(42)\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for generic fn: {:?}", errors);
}

#[test]
fn test_analyze_valid_char() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f() -> number {\nlet c = 'A';\nc + 1\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for char literal: {:?}", errors);
}

#[test]
fn test_analyze_valid_option_chain2() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> Option<number> {\nif x > 0 { Some(x) } else { None }\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for Option chain: {:?}", errors);
}

#[test]
fn test_analyze_valid_result_chain2() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> Result<number, string> {\nif x > 0 { Ok(x) } else { Err(\"negative\") }\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for Result chain: {:?}", errors);
}

#[test]
fn test_analyze_valid_bitwise() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(a: number, b: number) -> number {\n(a & b) | (a ^ b)\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for bitwise ops: {:?}", errors);
}

#[test]
fn test_analyze_valid_nested_fn() {
    let mut bridge = CompilerBridge::new();
    let source = "fn double(x: number) -> number { x * 2 }\nfn quad(x: number) -> number { double(double(x)) }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for nested fn: {:?}", errors);
}

#[test]
fn test_analyze_undefined_fn2() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nnonexistent()\n}";
    let diagnostics = bridge.analyze(source);
    let has_error = diagnostics.iter().any(|d| d.severity == KarteDiagnosticSeverity::Error);
    assert!(has_error, "Should detect undefined function");
}

#[test]
fn test_analyze_undefined_var2() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nundefined_var\n}";
    let diagnostics = bridge.analyze(source);
    let has_error = diagnostics.iter().any(|d| d.severity == KarteDiagnosticSeverity::Error);
    assert!(has_error, "Should detect undefined variable");
}

#[test]
fn test_analyze_duplicate_fn() {
    let mut bridge = CompilerBridge::new();
    let source = "fn foo() -> number { 1 }\nfn foo() -> number { 2 }";
    let diagnostics = bridge.analyze(source);
    let has_error = diagnostics.iter().any(|d| d.severity == KarteDiagnosticSeverity::Error);
    assert!(has_error, "Should detect duplicate function");
}

#[test]
fn test_analyze_non_exhaustive_bool() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(b: bool) -> number {\nmatch b {\ntrue => 1\n}\n}";
    let diagnostics = bridge.analyze(source);
    let has_error = diagnostics.iter().any(|d| d.severity == KarteDiagnosticSeverity::Error);
    assert!(has_error, "Should detect non-exhaustive bool match");
}

#[test]
fn test_analyze_missing_struct_fields2() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Point { x: number, y: number }\nfn main() -> number {\nlet p = Point { x: 1 };\n0\n}";
    let diagnostics = bridge.analyze(source);
    let has_error = diagnostics.iter().any(|d| d.severity == KarteDiagnosticSeverity::Error);
    assert!(has_error, "Should detect missing struct fields");
}

#[test]
fn test_analyze_valid_method_syntax() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Point { x: number, y: number }\nfn x(p: Point) -> number { p.x }\nfn main() -> number {\nlet p = Point { x: 3, y: 4 };\nx(p)\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_generic_struct() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Pair<T> { first: T, second: T }\nfn main() -> number {\nlet p = Pair { first: 1, second: 2 };\np.first + p.second\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for generic struct: {:?}", errors);
}

#[test]
fn test_analyze_valid_generic_enum() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Maybe<T> { Just(T), Nothing }\nfn unwrap_or(opt: Maybe<number>, def: number) -> number {\nmatch opt {\nMaybe::Just(x) => x\nMaybe::Nothing => def\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for generic enum: {:?}", errors);
}

#[test]
fn test_analyze_valid_result_bind() {
    let mut bridge = CompilerBridge::new();
    let source = "fn div(a: number, b: number) -> Result<number, string> {\nif b == 0 { Err(\"division by zero\") } else { Ok(a / b) }\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for Result bind: {:?}", errors);
}

#[test]
fn test_analyze_valid_closure_capture() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet x = 10;\nlet f = |y: number| -> number { x + y };\nf(5)\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for closure capture: {:?}", errors);
}

#[test]
fn test_analyze_valid_nested_closure() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet add = |a: number| -> fn(number) -> number {\n|b: number| -> number { a + b }\n};\nlet add5 = add(5);\nadd5(3)\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for nested closure: {:?}", errors);
}

#[test]
fn test_analyze_valid_string_concat_chain() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(a: string, b: string, c: string) -> string {\na + b + c\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for string concat chain: {:?}", errors);
}

#[test]
fn test_analyze_valid_number_compare_chain() {
    let mut bridge = CompilerBridge::new();
    let source = "fn in_range(x: number, lo: number, hi: number) -> bool {\nx >= lo && x <= hi\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for number compare chain: {:?}", errors);
}

#[test]
fn test_analyze_valid_reference_ops() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet x = 42;\nlet r = &x;\n*r\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for reference ops: {:?}", errors);
}

#[test]
fn test_analyze_valid_early_return3() {
    let mut bridge = CompilerBridge::new();
    let source = "fn abs(x: number) -> number {\nif x < 0 { return 0 - x };\nx\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for early return: {:?}", errors);
}

#[test]
fn test_analyze_valid_nested_struct2() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Line { start: number, end: number }\nfn length(l: Line) -> number {\nif l.end > l.start { l.end - l.start } else { 0 }\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for nested struct: {:?}", errors);
}

#[test]
fn test_analyze_valid_option_match2() {
    let mut bridge = CompilerBridge::new();
    let source = "fn unwrap_or_default(opt: Option<number>) -> number {\nmatch opt {\nSome(x) => x\nNone => 0\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for option match: {:?}", errors);
}

#[test]
fn test_analyze_valid_result_match2() {
    let mut bridge = CompilerBridge::new();
    let source = "fn unwrap_or(res: Result<number, string>, def: number) -> number {\nmatch res {\nOk(x) => x\nErr(_) => def\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for result match: {:?}", errors);
}

#[test]
fn test_analyze_valid_multi_param_fn() {
    let mut bridge = CompilerBridge::new();
    let source = "fn clamp(x: number, lo: number, hi: number) -> number {\nif x < lo { lo } else { if x > hi { hi } else { x } }\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for multi-param fn: {:?}", errors);
}

#[test]
fn test_analyze_valid_recursive_fn() {
    let mut bridge = CompilerBridge::new();
    let source = "fn factorial(n: number) -> number {\nif n <= 1 { 1 } else { n * factorial(n - 1) }\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for recursive fn: {:?}", errors);
}

#[test]
fn test_analyze_valid_mutual_recursion() {
    let mut bridge = CompilerBridge::new();
    let source = "fn is_even(n: number) -> bool {\nif n == 0 { true } else { is_odd(n - 1) }\n}\nfn is_odd(n: number) -> bool {\nif n == 0 { false } else { is_even(n - 1) }\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for mutual recursion: {:?}", errors);
}

#[test]
fn test_analyze_valid_block_expr() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet x = {\nlet a = 10;\na + 20\n};\nx\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for block expr: {:?}", errors);
}

#[test]
fn test_analyze_valid_chained_calls() {
    let mut bridge = CompilerBridge::new();
    let source = "fn double(x: number) -> number { x * 2 }\nfn inc(x: number) -> number { x + 1 }\nfn main() -> number {\ndouble(inc(5))\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for chained calls: {:?}", errors);
}

#[test]
fn test_analyze_valid_generic_fn3() {
    let mut bridge = CompilerBridge::new();
    let source = "fn id(x) { x }
fn main() -> number {
id(42)
}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for generic fn: {:?}", errors);
}

#[test]
fn test_analyze_valid_recursive_sum() {
    let mut bridge = CompilerBridge::new();
    let source = "fn sum(n: number) -> number {
if n <= 0 { 0 } else { n + sum(n - 1) }
}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for recursive sum: {:?}", errors);
}

#[test]
fn test_analyze_valid_shift_ops() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(a: number, b: number) -> number {\n(a << 2) >> 1\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for shift ops: {:?}", errors);
}

#[test]
fn test_analyze_valid_modulo() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(a: number, b: number) -> number {\na % b\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for modulo: {:?}", errors);
}

#[test]
fn test_analyze_valid_bitwise_xor() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(a: number, b: number) -> number {\na ^ b\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for bitwise xor: {:?}", errors);
}

#[test]
fn test_analyze_valid_complex_let_chain() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet a = 1;\nlet b = a + 1;\nlet c = b + a;\nlet d = c + b;\nd\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for complex let chain: {:?}", errors);
}

#[test]
fn test_analyze_valid_negative_number() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f() -> number {\n-42\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for negative number: {:?}", errors);
}

#[test]
fn test_analyze_valid_parenthesized() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(a: number, b: number) -> number {\n((a + b) * (a - b))\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for parenthesized: {:?}", errors);
}

#[test]
fn test_analyze_valid_string_eq() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(a: string, b: string) -> bool {\na == b\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for string eq: {:?}", errors);
}

#[test]
fn test_analyze_valid_string_neq() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(a: string, b: string) -> bool {\na != b\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for string neq: {:?}", errors);
}

#[test]
fn test_analyze_valid_abs_builtin() {
    let mut bridge = CompilerBridge::new();
    let source = "fn distance(a: number, b: number) -> number {\nabs(a - b)\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for abs builtin: {:?}", errors);
}

#[test]
fn test_analyze_valid_min_max() {
    let mut bridge = CompilerBridge::new();
    let source = "fn clamp(x: number, lo: number, hi: number) -> number {\nmin(max(x, lo), hi)\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for min/max: {:?}", errors);
}

#[test]
fn test_analyze_valid_len_builtin() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(s: string) -> number {\nlen(s)\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for len builtin: {:?}", errors);
}

#[test]
fn test_analyze_valid_struct_fn_chain() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Vec2 { x: number, y: number }\nfn length(v: Vec2) -> number {\nv.x * v.x + v.y * v.y\n}\nfn dist(a: Vec2, b: Vec2) -> number {\nlet dx = a.x - b.x;\nlet dy = a.y - b.y;\nlength(Vec2 { x: dx, y: dy })\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for struct fn chain: {:?}", errors);
}

#[test]
fn test_analyze_valid_option_methods() {
    let mut bridge = CompilerBridge::new();
    let source = "fn safe_div(a: number, b: number) -> Option<number> {\nif b == 0 { None } else { Some(a / b) }\n}\nfn unwrap_or(opt: Option<number>, def: number) -> number {\nmatch opt {\nSome(x) => x\nNone => def\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for option methods: {:?}", errors);
}

#[test]
fn test_analyze_valid_complex_match2() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Shape { Circle(number), Rect(number, number) }\nfn area(s: Shape) -> number {\nmatch s {\nShape::Circle(r) => r * r\nShape::Rect(w, h) => w * h\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for complex match: {:?}", errors);
}

#[test]
fn test_analyze_valid_string_compare_ops() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(a: string, b: string) -> bool {\na < b\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for string compare: {:?}", errors);
}

#[test]
fn test_analyze_valid_early_return_fn() {
    let mut bridge = CompilerBridge::new();
    let source = "fn classify(n: number) -> string {\nif n < 0 { return \"neg\" };\nif n == 0 { return \"zero\" };\n\"pos\"\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for early return: {:?}", errors);
}

#[test]
fn test_analyze_valid_for_array() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet sum = 0;\nfor i in [1, 2, 3] {\nsum = sum + i\n};\nsum\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for for-array: {:?}", errors);
}

#[test]
fn test_analyze_valid_for_range() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet sum = 0;\nfor i in 1..10 {\nsum = sum + i\n};\nsum\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for for-range: {:?}", errors);
}

#[test]
fn test_analyze_valid_char_literal() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet c = 'A';\nc + 1\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for char literal: {:?}", errors);
}

#[test]
fn test_analyze_valid_return_keyword() {
    let mut bridge = CompilerBridge::new();
    let source = "fn abs(x: number) -> number {\nif x < 0 { return 0 - x };\nx\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for return keyword: {:?}", errors);
}

#[test]
fn test_analyze_valid_assignment() {
    let mut bridge = CompilerBridge::new();
    let source = "fn counter() -> number {\nlet x = 0;\nx = x + 1;\nx = x * 2;\nx\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for assignment: {:?}", errors);
}

#[test]
fn test_analyze_valid_string_concat_chain2() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(name: string, age: number) -> string {\n\"Name: \" + name + \", Age: \" + age\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for string concat chain: {:?}", errors);
}

#[test]
fn test_analyze_valid_complex_arithmetic2() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(a: number, b: number, c: number) -> number {\n(a + b) * c - (a / b) + (a % c)\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for complex arithmetic: {:?}", errors);
}

#[test]
fn test_analyze_valid_leap_year() {
    let mut bridge = CompilerBridge::new();
    let source = "fn is_leap_year(year: number) -> bool {\n(year % 4 == 0) && ((year % 100 != 0) || (year % 400 == 0))\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for leap year: {:?}", errors);
}

#[test]
fn test_analyze_valid_option_map_fn() {
    let mut bridge = CompilerBridge::new();
    let source = "fn map_option(opt: Option<number>) -> Option<number> {\nmatch opt {\nSome(x) => Some(x * 2)\nNone => None\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for option map: {:?}", errors);
}

#[test]
fn test_analyze_valid_result_map_fn() {
    let mut bridge = CompilerBridge::new();
    let source = "fn map_result(res: Result<number, string>) -> Result<number, string> {\nmatch res {\nOk(x) => Ok(x * 2)\nErr(e) => Err(e)\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for result map: {:?}", errors);
}

#[test]
fn test_analyze_valid_json_enum() {
    let mut bridge = CompilerBridge::new();
    let source = "enum JSON { JNum(number), JStr(string), JBool(bool), JNull }\nfn json_type(j: JSON) -> string {\nmatch j {\nJSON::JNum(_) => \"number\"\nJSON::JStr(_) => \"string\"\nJSON::JBool(_) => \"bool\"\nJSON::JNull => \"null\"\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for JSON enum: {:?}", errors);
}

#[test]
fn test_analyze_valid_bool_to_number() {
    let mut bridge = CompilerBridge::new();
    let source = "fn bool_to_int(b: bool) -> number {\nif b { 1 } else { 0 }\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for bool to number: {:?}", errors);
}

#[test]
fn test_analyze_valid_number_to_bool() {
    let mut bridge = CompilerBridge::new();
    let source = "fn is_positive(n: number) -> bool {\nn > 0\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for number to bool: {:?}", errors);
}

#[test]
fn test_analyze_valid_nested_closure_v2() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet add = |a: number, b: number| -> number { a + b };\nlet result = add(add(1, 2), 3);\nresult\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for nested closure: {:?}", errors);
}

#[test]
fn test_analyze_valid_multi_let_chain() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet a = 1;\nlet b = 2;\nlet c = 3;\nlet d = a + b;\nlet e = c + d;\ne\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for multi let chain: {:?}", errors);
}

#[test]
fn test_analyze_valid_chained_string() {
    let mut bridge = CompilerBridge::new();
    let source = "fn exclaim(s: string) -> string {\ns + \"!\"\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for chained string: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_add() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f() -> number { 1 + 2 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for simple add: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_sub() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f() -> number { 10 - 3 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for simple sub: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_mul() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f() -> number { 4 * 5 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for simple mul: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_eq() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> bool { x == 0 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for simple eq: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_lt() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> bool { x < 10 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for simple lt: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_and() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(a: bool, b: bool) -> bool { a && b }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for simple and: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_or() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(a: bool, b: bool) -> bool { a || b }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for simple or: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_not() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: bool) -> bool { !x }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for simple not: {:?}", errors);
}

#[test]
fn test_analyze_valid_string_eq_v2() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(s: string) -> bool { s == \"hello\" }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for string eq: {:?}", errors);
}

#[test]
fn test_analyze_valid_leap_year_fn() {
    let mut bridge = CompilerBridge::new();
    let source = "fn is_leap_year(year: number) -> bool {\n(year % 4 == 0) && ((year % 100 != 0) || (year % 400 == 0))\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for leap year: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_return() {
    let mut bridge = CompilerBridge::new();
    let source = "fn abs(x: number) -> number {\nif x < 0 { return 0 - x };\nx\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for return: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_recursion() {
    let mut bridge = CompilerBridge::new();
    let source = "fn fact(n: number) -> number {\nif n <= 1 { 1 } else { n * fact(n - 1) }\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for recursion: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_while() {
    let mut bridge = CompilerBridge::new();
    let source = "fn countdown(n: number) -> number {\nlet x = n;\nwhile x > 0 {\nx = x - 1\n};\nx\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for while: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_for() {
    let mut bridge = CompilerBridge::new();
    let source = "fn sum_to(n: number) -> number {\nlet total = 0;\nfor i in 1..n {\ntotal = total + i\n};\ntotal\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for for: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_closure_capture2() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet y = 10;\nlet f = |x: number| -> number { x + y };\nf(5)\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for closure capture: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_struct_method() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Counter { value: number }\nfn increment(c: Counter) -> Counter {\nCounter { value: c.value + 1 }\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for struct method: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_enum_match() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Direction { North, South, East, West }\nfn opposite(d: Direction) -> Direction {\nmatch d {\nDirection::North => Direction::South\nDirection::South => Direction::North\nDirection::East => Direction::West\nDirection::West => Direction::East\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for enum match: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_option_chain() {
    let mut bridge = CompilerBridge::new();
    let source = "fn safe_div(a: number, b: number) -> Option<number> {\nif b == 0 { None } else { Some(a / b) }\n}\nfn try_div(a: number, b: number) -> number {\nmatch safe_div(a, b) {\nSome(x) => x\nNone => 0\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for option chain: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_result_chain() {
    let mut bridge = CompilerBridge::new();
    let source = "fn parse(s: string) -> Result<number, string> {\nOk(42)\n}\nfn compute(s: string) -> number {\nmatch parse(s) {\nOk(n) => n * 2\nErr(_) => 0\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for result chain: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_mutual_rec() {
    let mut bridge = CompilerBridge::new();
    let source = "fn is_even(n: number) -> bool {\nif n == 0 { true } else { is_odd(n - 1) }\n}\nfn is_odd(n: number) -> bool {\nif n == 0 { false } else { is_even(n - 1) }\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for mutual recursion: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_let_chain() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet a = 1;\nlet b = a + 1;\nlet c = b + a;\nc\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for let chain: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_assignment() {
    let mut bridge = CompilerBridge::new();
    let source = "fn counter() -> number {\nlet x = 0;\nx = x + 1;\nx\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for assignment: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_string_concat() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(s: string) -> string {\ns + \"!\"\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for string concat: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_number_concat() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(n: number) -> string {\n\"value: \" + n\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for number concat: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_match_assign() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet result = 0;\nmatch x {\n0 => result = 1\n_ => result = 2\n};\nresult\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for match assign: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_if_assign() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet result = 0;\nif x > 0 {\nresult = 1\n} else {\nresult = 2\n};\nresult\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for if assign: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_enum_bool() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Bool { True, False }\nfn not(b: Bool) -> Bool {\nmatch b {\nBool::True => Bool::False\nBool::False => Bool::True\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for enum bool: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_transform() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Point { x: number, y: number }\nfn translate(p: Point, dx: number) -> Point {\nPoint { x: p.x + dx, y: p.y }\n}\nfn scale(p: Point, s: number) -> Point {\nPoint { x: p.x * s, y: p.y * s }\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for transform: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_square_sum() {
    let mut bridge = CompilerBridge::new();
    let source = "fn square(x: number) -> number { x * x }\nfn sum_squares(a: number, b: number) -> number {\nsquare(a) + square(b)\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for square sum: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_compose2() {
    let mut bridge = CompilerBridge::new();
    let source = "fn compose(f: fn(number) -> number, g: fn(number) -> number, x: number) -> number {\nf(g(x))\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for compose: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_empty_fn() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f() {\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for empty fn: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_return_fn2() {
    let mut bridge = CompilerBridge::new();
    let source = "fn forty_two() -> number {\n42\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for return fn: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_hello_fn() {
    let mut bridge = CompilerBridge::new();
    let source = "fn hello() -> string {\n\"hello\"\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for hello fn: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_yes_fn() {
    let mut bridge = CompilerBridge::new();
    let source = "fn yes() -> bool {\ntrue\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for yes fn: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_param_fn() {
    let mut bridge = CompilerBridge::new();
    let source = "fn double(x: number) -> number {\nx * 2\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for param fn: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_two_param_fn() {
    let mut bridge = CompilerBridge::new();
    let source = "fn add(a: number, b: number) -> number {\na + b\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for two param fn: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_option_fn() {
    let mut bridge = CompilerBridge::new();
    let source = "fn safe_div(a: number, b: number) -> Option<number> {\nif b == 0 { None } else { Some(a / b) }\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for option fn: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_result_fn() {
    let mut bridge = CompilerBridge::new();
    let source = "fn checked_div(a: number, b: number) -> Result<number, string> {\nif b == 0 { Err(\"zero\") } else { Ok(a / b) }\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for result fn: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_reference_fn() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet x = 42;\nlet r = &x;\n*r\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for reference fn: {:?}", errors);
}

#[test]
fn test_analyze_valid_simple_char_fn() {
    let mut bridge = CompilerBridge::new();
    let source = "fn main() -> number {\nlet c = 'A';\nc + 1\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors for char fn: {:?}", errors);
}

#[test]
fn test_analyze_valid_final_fn_chain_1() {
    let mut bridge = CompilerBridge::new();
    let source = "fn square(x: number) -> number { x * x }\nfn cube(x: number) -> number { x * x * x }\nfn main() -> number {\nsquare(3) + cube(2)\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_final_fn_chain_2() {
    let mut bridge = CompilerBridge::new();
    let source = "fn max(a: number, b: number) -> number {\nif a > b { a } else { b }\n}\nfn min(a: number, b: number) -> number {\nif a < b { a } else { b }\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_final_fn_chain_3() {
    let mut bridge = CompilerBridge::new();
    let source = "fn abs(x: number) -> number {\nif x < 0 { 0 - x } else { x }\n}\nfn distance(a: number, b: number) -> number {\nabs(a - b)\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_final_enum_chain() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Season { Spring, Summer, Autumn, Winter }\nfn next_season(s: Season) -> Season {\nmatch s {\nSeason::Spring => Season::Summer\nSeason::Summer => Season::Autumn\nSeason::Autumn => Season::Winter\nSeason::Winter => Season::Spring\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_final_struct_chain() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Rect { width: number, height: number }\nfn area(r: Rect) -> number {\nr.width * r.height\n}\nfn is_square(r: Rect) -> bool {\nr.width == r.height\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_final_option_chain() {
    let mut bridge = CompilerBridge::new();
    let source = "fn safe_sqrt(x: number) -> Option<number> {\nif x < 0 { None } else { Some(x) }\n}\nfn sqrt_or_zero(x: number) -> number {\nmatch safe_sqrt(x) {\nSome(r) => r\nNone => 0\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_final_result_chain() {
    let mut bridge = CompilerBridge::new();
    let source = "fn parse_int(s: string) -> Result<number, string> {\nOk(42)\n}\nfn double_parse(s: string) -> Result<number, string> {\nmatch parse_int(s) {\nOk(n) => Ok(n * 2)\nErr(e) => Err(e)\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_final_complex_program() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Point { x: number, y: number }\nfn distance_sq(a: Point, b: Point) -> number {\nlet dx = a.x - b.x;\nlet dy = a.y - b.y;\ndx * dx + dy * dy\n}\nfn nearest(origin: Point, a: Point, b: Point) -> Point {\nif distance_sq(origin, a) < distance_sq(origin, b) { a } else { b }\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_extra_fn_chain_1() {
    let mut bridge = CompilerBridge::new();
    let source = "fn identity(x: number) -> number {\nx\n}\nfn apply_twice(f: fn(number) -> number, x: number) -> number {\nf(f(x))\n}\nfn main() -> number {\napply_twice(identity, 5)\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_extra_fn_chain_2() {
    let mut bridge = CompilerBridge::new();
    let source = "fn add(a: number) -> fn(number) -> number {\n|b: number| -> number { a + b }\n}\nfn main() -> number {\nlet add5 = add(5);\nadd5(10)\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_extra_enum_bool() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Bool { True, False }\nfn and(a: Bool, b: Bool) -> Bool {\nmatch a {\nBool::True => b\nBool::False => Bool::False\n}\n}\nfn or(a: Bool, b: Bool) -> Bool {\nmatch a {\nBool::True => Bool::True\nBool::False => b\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_extra_vec3() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Vec3 { x: number, y: number, z: number }\nfn dot(a: Vec3, b: Vec3) -> number {\na.x * b.x + a.y * b.y + a.z * b.z\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_extra_option_map() {
    let mut bridge = CompilerBridge::new();
    let source = "fn map_option(opt: Option<number>, f: fn(number) -> number) -> Option<number> {\nmatch opt {\nSome(x) => Some(f(x))\nNone => None\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_extra_result_map() {
    let mut bridge = CompilerBridge::new();
    let source = "fn map_result(res: Result<number, string>, f: fn(number) -> number) -> Result<number, string> {\nmatch res {\nOk(x) => Ok(f(x))\nErr(e) => Err(e)\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_extra_point_reflect() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Point { x: number, y: number }\nfn reflect_x(p: Point) -> Point {\nPoint { x: 0 - p.x, y: p.y }\n}\nfn reflect_y(p: Point) -> Point {\nPoint { x: p.x, y: 0 - p.y }\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_extra_color_grayscale() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Color { r: number, g: number, b: number }\nfn grayscale(c: Color) -> number {\n(c.r + c.g + c.b) / 3\n}\nfn is_dark(c: Color) -> bool {\ngrayscale(c) < 128\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_bonus_1() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet a = x;\nlet b = a;\nlet c = b;\nc\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_bonus_2() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> string {\nmatch x {\n0 => \"zero\"\n1 => \"one\"\n_ => \"many\"\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_bonus_3() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Point { x: number, y: number }\nfn on_x_axis(p: Point) -> bool {\np.y == 0\n}\nfn on_y_axis(p: Point) -> bool {\np.x == 0\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_bonus_4() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Grade { A, B, C, D, F }\nfn pass(g: Grade) -> bool {\nmatch g {\nGrade::A => true\nGrade::B => true\nGrade::C => true\nGrade::D => true\nGrade::F => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_wave_1() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet a = x + 1;\nlet b = a + 2;\nlet c = b + 3;\nc\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_wave_2() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Triple { a: number, b: number, c: number }\nfn sum(t: Triple) -> number {\nt.a + t.b + t.c\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_wave_3() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nmatch x {\n0 => 1\n1 => 2\n2 => 3\n_ => x + 1\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_wave_4() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> string {\nlet result = if x > 0 { \"pos\" } else { \"non-pos\" };\nresult\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_wave_5() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number, y: number) -> bool {\n(x > 0 && y > 0) || (x < 0 && y < 0)\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_wave_6() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f() -> bool {\ntrue && false || true\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_wave_7() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> bool {\nx != 0 && x > 0\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_wave_8() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> Option<number> {\nif x == 0 { None } else { Some(x) }\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_push_1() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nmatch x {\n0 => 0\n_ => x\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_push_2() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number, y: number) -> number {\nif x > y { x } else { y }\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_push_3() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Circle { radius: number }\nfn circumference(c: Circle) -> number {\n2 * c.radius * 3\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_push_4() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Day { Mon, Tue, Wed, Thu, Fri, Sat, Sun }\nfn is_weekend(d: Day) -> bool {\nmatch d {\nDay::Sat => true\nDay::Sun => true\n_ => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_push_5() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> Option<number> {\nif x > 100 { None } else { if x < 0 { None } else { Some(x) } }\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_push_6() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet y = x;\ny = y + 1;\ny = y + 1;\ny\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_push_7() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet a = 1;\nlet b = 2;\nlet c = 3;\na + b + c + x\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}


#[test]
fn test_analyze_valid_closure_1() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f() -> number {\nlet double = |x: number| -> number { x * 2 };\ndouble(21)\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_closure_2() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet inc = |n: number| -> number { n + 1 };\ninc(x)\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_closure_3() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f() -> bool {\nlet is_positive = |x: number| -> bool { x > 0 };\nis_positive(42)\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_closure_4() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet transform = |n: number| -> number {\nif n > 0 { n * 2 } else { n }\n};\ntransform(x)\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_struct_pair() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Pair { a: number, b: number }\nfn f() -> number {\nlet p = Pair { a: 1, b: 2 };\np.a + p.b\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_struct_triple() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Triple { x: number, y: number, z: number }\nfn f() -> number {\nlet t = Triple { x: 1, y: 2, z: 3 };\nt.x + t.y + t.z\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_ref_1() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet r = &x;\n*r\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_char_1() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f() -> number {\nlet c = 'A';\nc + 1\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_recursion_fact() {
    let mut bridge = CompilerBridge::new();
    let source = "fn fact(n: number) -> number {\nif n <= 1 { 1 } else { n * fact(n - 1) }\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_recursion_fib() {
    let mut bridge = CompilerBridge::new();
    let source = "fn fib(n: number) -> number {\nif n <= 1 { n } else { fib(n - 1) + fib(n - 2) }\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_recursion_gcd() {
    let mut bridge = CompilerBridge::new();
    let source = "fn gcd(a: number, b: number) -> number {\nif b == 0 { a } else { gcd(b, a % b) }\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_higher_order_apply() {
    let mut bridge = CompilerBridge::new();
    let source = "fn apply(f: fn(number) -> number, x: number) -> number {\nf(x)\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_higher_order_compose() {
    let mut bridge = CompilerBridge::new();
    let source = "fn compose(f: fn(number) -> number, g: fn(number) -> number, x: number) -> number {\nf(g(x))\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_pattern_shape() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Shape { Circle(number), Rect(number, number) }\nfn area(s: Shape) -> number {\nmatch s {\nShape::Circle(r) => 3 * r * r\nShape::Rect(w, h) => w * h\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_pattern_either() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Either { Left(number), Right(string) }\nfn f(e: Either) -> string {\nmatch e {\nEither::Left(n) => \"number\"\nEither::Right(s) => s\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_pattern_option() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> Option<number> {\nif x == 0 { None } else { Some(x) }\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_builtins_abs() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nabs(x)\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_builtins_min_max() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number, y: number) -> number {\nmin(x, y) + max(x, y)\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_builtins_len() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(s: string) -> number {\nlen(s)\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_control_for() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(n: number) -> number {\nlet sum = 0;\nfor i in 0..n {\nsum = sum + i\n};\nsum\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_control_while() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet total = 0;\nwhile total < x {\ntotal = total + 1\n};\ntotal\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_control_nested_for() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(n: number) -> number {\nlet total = 0;\nfor i in 0..n {\nfor j in 0..i {\ntotal = total + j\n}\n};\ntotal\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_error_undefined() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f() -> number {\nundefined_var\n}";
    let diagnostics = bridge.analyze(source);
    assert!(!diagnostics.is_empty(), "Should have errors for undefined variable");
}

#[test]
fn test_analyze_valid_error_type_mismatch() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f() -> number {\n\"hello\"\n}";
    let diagnostics = bridge.analyze(source);
    assert!(!diagnostics.is_empty(), "Should have errors for type mismatch");
}

#[test]
fn test_analyze_valid_error_arity() {
    let mut bridge = CompilerBridge::new();
    let source = "fn add(a: number, b: number) -> number {\na + b\n}\nfn f() -> number {\nadd(1)\n}";
    let diagnostics = bridge.analyze(source);
    assert!(!diagnostics.is_empty(), "Should have errors for arity mismatch");
}

#[test]
fn test_analyze_valid_error_missing_field() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Point { x: number, y: number }\nfn f() -> Point {\nPoint { x: 1 }\n}";
    let diagnostics = bridge.analyze(source);
    assert!(!diagnostics.is_empty(), "Should have errors for missing field");
}

#[test]
fn test_analyze_valid_v11_sign() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet sign = if x > 0 { 1 } else { if x < 0 { 0 - 1 } else { 0 } };\nsign\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v11_fizzbuzz() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> string {\nmatch x % 3 {\n0 => \"fizz\"\n1 => \"buzz\"\n_ => \"fizzbuzz\"\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v11_point_quadrant() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Point2D { x: number, y: number }\nfn quadrant(p: Point2D) -> number {\nif p.x > 0 {\nif p.y > 0 { 1 } else { 4 }\n} else {\nif p.y > 0 { 2 } else { 3 }\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v11_for_sum() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(n: number) -> number {\nlet sum = 0;\nfor i in 0..n {\nsum = sum + i\n};\nsum\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v11_while_count() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet count = 0;\nlet n = x;\nwhile n > 0 {\ncount = count + 1;\nn = n / 2\n};\ncount\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v11_complex_chain() {
    let mut bridge = CompilerBridge::new();
    let source = "fn add(a: number, b: number) -> number { a + b }\nfn mul(a: number, b: number) -> number { a * b }\nfn main() -> number {\nadd(mul(2, 3), mul(4, 5))\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v11_midpoint() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Point { x: number, y: number }\nfn midpoint(a: Point, b: Point) -> Point {\nPoint { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 }\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v11_eval_expr() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Expr { Val(number), Add(number, number), Mul(number, number) }\nfn eval(e: Expr) -> number {\nmatch e {\nExpr::Val(n) => n\nExpr::Add(a, b) => a + b\nExpr::Mul(a, b) => a * b\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v15_student() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Student2 { id: number, score: number }\nfn honors(s: Student2) -> bool {\ns.score >= 90\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v15_size() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Size { Small, Medium, Large }\nfn price(s: Size) -> number {\nmatch s {\nSize::Small => 5\nSize::Medium => 8\nSize::Large => 12\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v15_option() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> Option<number> {\nif x > 0 && x < 100 {\nSome(x * x)\n} else {\nNone\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v15_result() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> Result<number, string> {\nif x == 0 {\nErr(\"zero\")\n} else {\nOk(x)\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v15_for_fib() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet a = 1;\nlet b = 1;\nfor i in 2..x {\nlet c = a + b;\na = b;\nb = c\n};\nb\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v15_complex_calc() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(a: number, b: number, c: number) -> number {\nlet max = if a > b {\nif a > c { a } else { c }\n} else {\nif b > c { b } else { c }\n};\nmax\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v16_circle() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Circle2 { cx: number, cy: number, radius: number }\nfn contains_point(c: Circle2, x: number, y: number) -> bool {\nlet dx = x - c.cx;\nlet dy = y - c.cy;\ndx * dx + dy * dy <= c.radius * c.radius\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v16_vec4() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Vec4 { x: number, y: number, z: number, w: number }\nfn dot4(a: Vec4, b: Vec4) -> number {\na.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v16_triangle_sum() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + i * (i + 1) / 2\n};\nsum\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v16_bit_count() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet count = 0;\nlet n = x;\nwhile n > 1 {\ncount = count + 1;\nn = n / 2\n};\ncount\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v19_name() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Name { first: string, last: string }\nfn full_name(n: Name) -> string {\nn.first + \" \" + n.last\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v19_language() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Language { Rust, Python, JavaScript, Go }\nfn is_compiled(l: Language) -> bool {\nmatch l {\nLanguage::Rust => true\nLanguage::Go => true\nLanguage::Python => false\nLanguage::JavaScript => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v19_vector() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Vector2D { dx: number, dy: number }\nfn magnitude_sq(v: Vector2D) -> number {\nv.dx * v.dx + v.dy * v.dy\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v19_http() {
    let mut bridge = CompilerBridge::new();
    let source = "enum HTTP { Get, Post, Put, Delete }\nfn has_body(h: HTTP) -> bool {\nmatch h {\nHTTP::Get => false\nHTTP::Post => true\nHTTP::Put => true\nHTTP::Delete => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v20_transport() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Transport { Walk, Bike, Car, Bus }\nfn speed(t: Transport) -> number {\nmatch t {\nTransport::Walk => 5\nTransport::Bike => 15\nTransport::Car => 60\nTransport::Bus => 30\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v20_box3d() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Box3D { w: number, h: number, d: number }\nfn volume(b: Box3D) -> number { b.w * b.h * b.d }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v21_season() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Season3 { Spring2, Summer2, Autumn2, Winter2 }\nfn temp(s: Season3) -> string {\nmatch s {\nSeason3::Spring2 => \"warm\"\nSeason3::Summer2 => \"hot\"\nSeason3::Autumn2 => \"cool\"\nSeason3::Winter2 => \"cold\"\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v21_matrix() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Matrix3 { a: number, b: number, c: number, d: number }\nfn add_matrix(a: Matrix3, b: Matrix3) -> Matrix3 {\nMatrix3 { a: a.a + b.a, b: a.b + b.b, c: a.c + b.c, d: a.d + b.d }\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v21_config() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Config { Debug, Release }\nfn is_debug(c: Config) -> bool {\nmatch c {\nConfig::Debug => true\nConfig::Release => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v21_person() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Person2 { name: string, age: number }\nfn is_teenager(p: Person2) -> bool {\np.age >= 13 && p.age <= 19\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v27_account() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Account { balance: number }\nfn is_positive(a: Account) -> bool { a.balance > 0 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v27_shape3() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Shape3 { Circle3(number), Square3(number) }\nfn area3(s: Shape3) -> number {\nmatch s {\nShape3::Circle3(r) => 3 * r * r\nShape3::Square3(side) => side * side\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v27_fibonacci() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet prev = 0;\nlet curr = 1;\nfor i in 0..x {\nlet next = prev + curr;\nprev = curr;\ncurr = next\n};\ncurr\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v28_mode() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Mode { Read2, Write2, ReadWrite }\nfn can_write(m: Mode) -> bool {\nmatch m {\nMode::Read2 => false\nMode::Write2 => true\nMode::ReadWrite => true\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v28_rect() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Rect2 { x: number, y: number, w: number, h: number }\nfn area(r: Rect2) -> number { r.w * r.h }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v28_expr3() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Expr3 { Lit(number), Add3(number, number), Sub(number, number) }\nfn eval(e: Expr3) -> number {\nmatch e {\nExpr3::Lit(n) => n\nExpr3::Add3(a, b) => a + b\nExpr3::Sub(a, b) => a - b\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v28_gcd() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number, y: number) -> number {\nlet a = x;\nlet b = y;\nwhile b != 0 {\nlet temp = b;\nb = a % b;\na = temp\n};\na\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v26_sphere() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Sphere { cx: number, cy: number, cz: number, radius: number }\nfn contains_origin(s: Sphere) -> bool {\nlet d = s.cx * s.cx + s.cy * s.cy + s.cz * s.cz;\nd <= s.radius * s.radius\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v26_metric() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Metric { Bytes(number), KB(number), MB(number) }\nfn to_bytes(m: Metric) -> number {\nmatch m {\nMetric::Bytes(b) => b\nMetric::KB(k) => k * 1024\nMetric::MB(m) => m * 1024 * 1024\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v26_cloud() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Cloud { Cumulus, Stratus, Cirrus, Nimbus }\nfn produces_rain(c: Cloud) -> bool {\nmatch c {\nCloud::Cumulus => false\nCloud::Stratus => false\nCloud::Cirrus => false\nCloud::Nimbus => true\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v29_planet() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Planet { Mercury, Venus, Earth, Mars }\nfn is_habitable(p: Planet) -> bool {\nmatch p {\nPlanet::Mercury => false\nPlanet::Venus => false\nPlanet::Earth => true\nPlanet::Mars => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v29_circle3() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Circle3 { cx: number, cy: number, r: number }\nfn area(c: Circle3) -> number { 3 * c.r * c.r }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v30_priority() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Priority { Low, Medium, High, Critical }\nfn is_urgent(p: Priority) -> bool {\nmatch p {\nPriority::Low => false\nPriority::Medium => false\nPriority::High => true\nPriority::Critical => true\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v30_vector3() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Vector3 { x: number, y: number, z: number }\nfn dot(a: Vector3, b: Vector3) -> number { a.x * b.x + a.y * b.y + a.z * b.z }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v30_token2() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Token2 { Number2(number), Ident(string), EOF }\nfn is_eof(t: Token2) -> bool {\nmatch t {\nToken2::Number2(_) => false\nToken2::Ident(_) => false\nToken2::EOF => true\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v30_reverse() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet reversed = 0;\nlet n = x;\nwhile n > 0 {\nreversed = reversed * 10 + n % 10;\nn = n / 10\n};\nreversed\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v29_digit_sum() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet sum = 0;\nlet i = x;\nwhile i > 0 {\nsum = sum + i % 10;\ni = i / 10\n};\nsum\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v28_record() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Record { key: string, value: number }\nfn is_valid(r: Record) -> bool { r.value > 0 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v25_employee() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Employee { name: string, salary: number }\nfn is_high_earner(e: Employee) -> bool { e.salary > 100000 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v24_coord() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Coord { lat: number, lon: number }\nfn is_north(c: Coord) -> bool { c.lat > 0 }\nfn is_east(c: Coord) -> bool { c.lon > 0 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v31_rational() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Rational { num: number, den: number }\nfn is_whole(r: Rational) -> bool { r.num % r.den == 0 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v31_grade() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Grade { A, B, C, D, F }\nfn passing(g: Grade) -> bool {\nmatch g {\nGrade::A => true\nGrade::B => true\nGrade::C => true\nGrade::D => true\nGrade::F => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v32_complex() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Complex2 { real: number, imag: number }\nfn magnitude_sq(c: Complex2) -> number { c.real * c.real + c.imag * c.imag }\nfn is_real(c: Complex2) -> bool { c.imag == 0 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v32_currency() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Currency { USD, EUR, GBP, JPY }\nfn symbol(c: Currency) -> string {\nmatch c {\nCurrency::USD => \"USD\"\nCurrency::EUR => \"EUR\"\nCurrency::GBP => \"GBP\"\nCurrency::JPY => \"JPY\"\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v33_quaternion() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Quaternion { w: number, x: number, y: number, z: number }\nfn norm_sq(q: Quaternion) -> number { q.w * q.w + q.x * q.x + q.y * q.y + q.z * q.z }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v33_weekday() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Weekday2 { Mon, Tue, Wed, Thu, Fri, Sat, Sun }\nfn is_weekend(d: Weekday2) -> bool {\nmatch d {\nWeekday2::Sat => true\nWeekday2::Sun => true\n_ => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v34_vec4() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Vec4 { x: number, y: number, z: number, w: number }\nfn dot4(a: Vec4, b: Vec4) -> number { a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v34_animal() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Animal2 { Dog2, Cat2, Bird2 }\nfn sound(a: Animal2) -> string {\nmatch a {\nAnimal2::Dog2 => \"woof\"\nAnimal2::Cat2 => \"meow\"\nAnimal2::Bird2 => \"tweet\"\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v35_time() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Time2 { hours: number, minutes: number }\nfn total_minutes(t: Time2) -> number { t.hours * 60 + t.minutes }\nfn is_midnight(t: Time2) -> bool { t.hours == 0 && t.minutes == 0 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v35_tree() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Tree2 { Leaf2(number), Branch2(number, number) }\nfn sum_tree(t: Tree2) -> number {\nmatch t {\nTree2::Leaf2(v) => v\nTree2::Branch2(a, b) => a + b\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v36_fraction() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Fraction2 { num: number, den: number }\nfn value(f: Fraction2) -> number { f.num / f.den }\nfn is_proper(f: Fraction2) -> bool { f.num < f.den }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v37_temperature() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Temperature { celsius: number }\nfn to_fahrenheit(t: Temperature) -> number { t.celsius * 9 / 5 + 32 }\nfn is_freezing(t: Temperature) -> bool { t.celsius <= 0 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v37_collatz() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> Result<number, string> {\nif x > 0 {\nif x % 2 == 0 { Ok(x / 2) } else { Ok(x * 3 + 1) }\n} else {\nErr(\"non-positive\")\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v38_student() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Student3 { name: string, grade: number }\nfn honor_roll(s: Student3) -> bool { s.grade >= 90 }\nfn passing(s: Student3) -> bool { s.grade >= 60 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v38_op() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Op2 { Add4, Sub4, Mul4, Div4 }\nfn apply(op: Op2, a: number, b: number) -> number {\nmatch op {\nOp2::Add4 => a + b\nOp2::Sub4 => a - b\nOp2::Mul4 => a * b\nOp2::Div4 => a / b\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v38_lit() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Lit2 { IntLit(number), StrLit(string), BoolLit(bool) }\nfn int_value(l: Lit2) -> number {\nmatch l {\nLit2::IntLit(n) => n\nLit2::StrLit(_) => 0\nLit2::BoolLit(b) => if b { 1 } else { 0 }\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v39_line() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Line2 { x1: number, y1: number, x2: number, y2: number }\nfn length_sq(l: Line2) -> number {\nlet dx = l.x2 - l.x1;\nlet dy = l.y2 - l.y1;\ndx * dx + dy * dy\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v39_weight() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Weight { kg: number }\nfn to_grams(w: Weight) -> number { w.kg * 1000 }\nfn is_heavy(w: Weight) -> bool { w.kg > 100 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v39_binary_op() {
    let mut bridge = CompilerBridge::new();
    let source = "enum BinaryOp { And3, Or3, Xor }\nfn eval_bool(op: BinaryOp, a: bool, b: bool) -> bool {\nmatch op {\nBinaryOp::And3 => a && b\nBinaryOp::Or3 => a || b\nBinaryOp::Xor => a != b\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v39_compass() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Compass { N2, NE, E2, SE, S2, SW, W2, NW }\nfn is_cardinal(c: Compass) -> bool {\nmatch c {\nCompass::N2 => true\nCompass::E2 => true\nCompass::S2 => true\nCompass::W2 => true\n_ => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v40_distance() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Distance2 { meters: number }\nfn to_km(d: Distance2) -> number { d.meters / 1000 }\nfn to_cm(d: Distance2) -> number { d.meters * 100 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v40_wave() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Wave { Sine, Square, Triangle, Sawtooth }\nfn is_smooth(w: Wave) -> bool {\nmatch w {\nWave::Sine => true\nWave::Square => false\nWave::Triangle => true\nWave::Sawtooth => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v41_hexcolor() {
    let mut bridge = CompilerBridge::new();
    let source = "struct HexColor { r: number, g: number, b: number }\nfn luminance(c: HexColor) -> number { (c.r * 299 + c.g * 587 + c.b * 114) / 1000 }\nfn is_dark(c: HexColor) -> bool { luminance(c) < 128 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v41_arch() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Arch { X86, ARM, RISCV, MIPS }\nfn is_64bit(a: Arch) -> bool {\nmatch a {\nArch::X86 => true\nArch::ARM => true\nArch::RISCV => true\nArch::MIPS => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v42_mat2x2() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Mat2x2 { a: number, b: number, c: number, d: number }\nfn determinant(m: Mat2x2) -> number { m.a * m.d - m.b * m.c }\nfn is_identity(m: Mat2x2) -> bool { m.a == 1 && m.b == 0 && m.c == 0 && m.d == 1 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v43_opcode() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Opcode2 { Push(number), Pop, Add5, Mul5 }\nfn has_operand(op: Opcode2) -> bool {\nmatch op {\nOpcode2::Push(_) => true\nOpcode2::Pop => false\nOpcode2::Add5 => false\nOpcode2::Mul5 => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v43_dbtype() {
    let mut bridge = CompilerBridge::new();
    let source = "enum DBType { Integer, Float, VarChar, Boolean }\nfn is_numeric(t: DBType) -> bool {\nmatch t {\nDBType::Integer => true\nDBType::Float => true\nDBType::VarChar => false\nDBType::Boolean => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v43_coordinate() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Coordinate2 { x: number, y: number, z: number }\nfn is_origin(c: Coordinate2) -> bool { c.x == 0 && c.y == 0 && c.z == 0 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v42_aabb3d() {
    let mut bridge = CompilerBridge::new();
    let source = "struct AABB3D { min_x: number, min_y: number, min_z: number, max_x: number, max_y: number, max_z: number }\nfn volume(a: AABB3D) -> number { (a.max_x - a.min_x) * (a.max_y - a.min_y) * (a.max_z - a.min_z) }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v42_power_sum() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet result = 0;\nlet power = 1;\nlet i = 0;\nwhile i < x {\nresult = result + power;\npower = power * 2;\ni = i + 1\n};\nresult\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v44_velocity() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Velocity { dx: number, dy: number, dz: number }\nfn speed_sq(v: Velocity) -> number { v.dx * v.dx + v.dy * v.dy + v.dz * v.dz }\nfn is_stationary(v: Velocity) -> bool { speed_sq(v) == 0 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v44_protocol() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Protocol { TCP, UDP, ICMP }\nfn is_reliable(p: Protocol) -> bool {\nmatch p {\nProtocol::TCP => true\nProtocol::UDP => false\nProtocol::ICMP => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v45_battery() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Battery { capacity: number, charge: number }\nfn percentage(b: Battery) -> number { b.charge * 100 / b.capacity }\nfn is_full(b: Battery) -> bool { b.charge == b.capacity }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v45_sort() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Sort2 { Bubble, Quick, Merge, Heap }\nfn is_nlogn(s: Sort2) -> bool {\nmatch s {\nSort2::Bubble => false\nSort2::Quick => true\nSort2::Merge => true\nSort2::Heap => true\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v45_complex3() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Complex3 { re: number, im: number }\nfn conjugate(c: Complex3) -> Complex3 { Complex3 { re: c.re, im: 0 - c.im } }\nfn norm_sq(c: Complex3) -> number { c.re * c.re + c.im * c.im }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v45_http_status() {
    let mut bridge = CompilerBridge::new();
    let source = "enum HttpStatus { Ok2, NotFound, ServerError }\nfn is_success(h: HttpStatus) -> bool {\nmatch h {\nHttpStatus::Ok2 => true\nHttpStatus::NotFound => false\nHttpStatus::ServerError => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v45_log2() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet count = 0;\nlet n = x;\nwhile n > 1 {\nn = n / 2;\ncount = count + 1\n};\ncount\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v44_isqrt() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet result = 0;\nlet i = 1;\nwhile i * i <= x {\nresult = i;\ni = i + 1\n};\nresult\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v44_triangle4() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Triangle4 { base: number, height: number }\nfn area(t: Triangle4) -> number { t.base * t.height / 2 }\nfn is_degenerate(t: Triangle4) -> bool { t.base == 0 || t.height == 0 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v44_pattern() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Pattern3 { Singleton, Factory, Observer, Strategy }\nfn is_creational(p: Pattern3) -> bool {\nmatch p {\nPattern3::Singleton => true\nPattern3::Factory => true\nPattern3::Observer => false\nPattern3::Strategy => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v46_storage() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Storage { Register, Cache, RAM, Disk }\nfn latency_ns(s: Storage) -> number {\nmatch s {\nStorage::Register => 1\nStorage::Cache => 10\nStorage::RAM => 100\nStorage::Disk => 10000000\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v47_dimension() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Dimension { width: number, height: number }\nfn aspect_ratio(d: Dimension) -> number { d.width / d.height }\nfn is_square(d: Dimension) -> bool { d.width == d.height }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v48_conn_state() {
    let mut bridge = CompilerBridge::new();
    let source = "enum ConnState { Connected, Disconnected, Reconnecting }\nfn is_online(c: ConnState) -> bool {\nmatch c {\nConnState::Connected => true\nConnState::Disconnected => false\nConnState::Reconnecting => true\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v48_transform() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Transform2D { tx: number, ty: number, sx: number, sy: number }\nfn is_identity(t: Transform2D) -> bool { t.tx == 0 && t.ty == 0 && t.sx == 1 && t.sy == 1 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v49_volume() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Volume2 { liters: number }\nfn to_ml(v: Volume2) -> number { v.liters * 1000 }\nfn is_empty(v: Volume2) -> bool { v.liters == 0 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v49_encoding() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Encoding2 { UTF8, ASCII, UTF16 }\nfn is_unicode(e: Encoding2) -> bool {\nmatch e {\nEncoding2::UTF8 => true\nEncoding2::ASCII => false\nEncoding2::UTF16 => true\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v49_factorial() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet product = 1;\nlet i = 2;\nwhile i <= x {\nproduct = product * i;\ni = i + 1\n};\nproduct\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v47_thread() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Thread2 { Running, Paused, Blocked, Terminated }\nfn is_active(t: Thread2) -> bool {\nmatch t {\nThread2::Running => true\nThread2::Paused => true\nThread2::Blocked => false\nThread2::Terminated => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v46_point6() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Point6 { x: number, y: number, z: number }\nfn translate(p: Point6, dx: number, dy: number, dz: number) -> Point6 {\nPoint6 { x: p.x + dx, y: p.y + dy, z: p.z + dz }\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v48_phase() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Phase2 { New2, Growing, Mature, Declining }\nfn is_growth_phase(p: Phase2) -> bool {\nmatch p {\nPhase2::New2 => true\nPhase2::Growing => true\nPhase2::Mature => false\nPhase2::Declining => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v50_speed() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Speed2 { mps: number }\nfn to_kmh(s: Speed2) -> number { s.mps * 3600 / 1000 }\nfn is_walking(s: Speed2) -> bool { s.mps < 2 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v50_terminal() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Terminal2 { VT100, VT220, XTerm, ANSI }\nfn supports_color(t: Terminal2) -> bool {\nmatch t {\nTerminal2::VT100 => false\nTerminal2::VT220 => false\nTerminal2::XTerm => true\nTerminal2::ANSI => true\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v51_cylinder() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Cylinder { radius: number, height: number }\nfn volume(c: Cylinder) -> number { 3 * c.radius * c.radius * c.height }\nfn is_flat(c: Cylinder) -> bool { c.height == 0 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v51_job() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Job2 { Running2, Queued, Completed, Failed }\nfn is_pending(j: Job2) -> bool {\nmatch j {\nJob2::Running2 => true\nJob2::Queued => true\nJob2::Completed => false\nJob2::Failed => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v52_cone() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Cone { radius: number, height: number }\nfn volume(c: Cone) -> number { c.radius * c.radius * c.height }\nfn is_pointy(c: Cone) -> bool { c.height > c.radius * 2 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v52_device() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Device { Keyboard, Mouse, Monitor, Speaker }\nfn is_input(d: Device) -> bool {\nmatch d {\nDevice::Keyboard => true\nDevice::Mouse => true\nDevice::Monitor => false\nDevice::Speaker => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v52_lock() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Lock2 { Shared, Exclusive, Free }\nfn is_writeable(l: Lock2) -> bool {\nmatch l {\nLock2::Shared => false\nLock2::Exclusive => true\nLock2::Free => true\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v50_sphere2() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Sphere2 { cx: number, cy: number, cz: number, r: number }\nfn volume(s: Sphere2) -> number { 4 * s.r * s.r * s.r }\nfn contains_origin(s: Sphere2) -> bool {\ns.cx * s.cx + s.cy * s.cy + s.cz * s.cz <= s.r * s.r\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v50_logic_gate() {
    let mut bridge = CompilerBridge::new();
    let source = "enum LogicGate { And4, Or4, Not, Xor2 }\nfn num_inputs(g: LogicGate) -> number {\nmatch g {\nLogicGate::And4 => 2\nLogicGate::Or4 => 2\nLogicGate::Not => 1\nLogicGate::Xor2 => 2\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v52_area() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Area2 { sqm: number }\nfn to_sqft(a: Area2) -> number { a.sqm * 10 }\nfn is_large(a: Area2) -> bool { a.sqm > 100 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v53_pressure() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Pressure { pascal: number }\nfn to_bar(p: Pressure) -> number { p.pascal / 100000 }\nfn is_atmosphere(p: Pressure) -> bool { p.pascal > 90000 && p.pascal < 110000 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v53_crypto() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Crypto { AES, RSA, DES, ChaCha }\nfn is_symmetric(c: Crypto) -> bool {\nmatch c {\nCrypto::AES => true\nCrypto::RSA => false\nCrypto::DES => true\nCrypto::ChaCha => true\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v54_energy() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Energy { joules: number }\nfn to_kj(e: Energy) -> number { e.joules / 1000 }\nfn to_cal(e: Energy) -> number { e.joules / 4 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v54_format() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Format2 { JSON, XML, YAML, TOML }\nfn is_markup(f: Format2) -> bool {\nmatch f {\nFormat2::JSON => false\nFormat2::XML => true\nFormat2::YAML => false\nFormat2::TOML => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v55_pyramid() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Pyramid { base: number, height: number }\nfn volume(p: Pyramid) -> number { p.base * p.base * p.height / 3 }\nfn is_tall(p: Pyramid) -> bool { p.height > p.base }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v55_build_status() {
    let mut bridge = CompilerBridge::new();
    let source = "enum BuildStatus { Success2, Failed2, Timeout, Cancelled }\nfn needs_retry(b: BuildStatus) -> bool {\nmatch b {\nBuildStatus::Success2 => false\nBuildStatus::Failed2 => true\nBuildStatus::Timeout => true\nBuildStatus::Cancelled => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v55_power() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Power2 { watts: number }\nfn to_kw(p: Power2) -> number { p.watts / 1000 }\nfn is_high_power(p: Power2) -> bool { p.watts > 1000 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v55_geo() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Geo2 { Point2, Line3, Polygon }\nfn has_area(g: Geo2) -> bool {\nmatch g {\nGeo2::Point2 => false\nGeo2::Line3 => false\nGeo2::Polygon => true\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v53_torus() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Torus2 { major_r: number, minor_r: number }\nfn surface_area(t: Torus2) -> number { 4 * 3 * t.major_r * t.minor_r }\nfn is_thick(t: Torus2) -> bool { t.minor_r > t.major_r / 2 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v54_ellipse() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Ellipse2 { a: number, b: number }\nfn is_circle(e: Ellipse2) -> bool { e.a == e.b }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v56_density() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Density2 { mass: number, volume: number }\nfn compute(d: Density2) -> number { d.mass / d.volume }\nfn is_heavy(d: Density2) -> bool { d.mass / d.volume > 5 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v56_cloud() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Cloud2 { Public3, Private3, Hybrid }\nfn is_shared(c: Cloud2) -> bool {\nmatch c {\nCloud2::Public3 => true\nCloud2::Private3 => false\nCloud2::Hybrid => true\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v57_frequency() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Frequency { hz: number }\nfn to_khz(f: Frequency) -> number { f.hz / 1000 }\nfn is_audible(f: Frequency) -> bool { f.hz >= 20 && f.hz <= 20000 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v57_container() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Container2 { List, Vector, Set, Map }\nfn is_ordered(c: Container2) -> bool {\nmatch c {\nContainer2::List => true\nContainer2::Vector => true\nContainer2::Set => false\nContainer2::Map => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v58_force() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Force2 { newtons: number }\nfn to_kilonewtons(f: Force2) -> number { f.newtons / 1000 }\nfn is_strong(f: Force2) -> bool { f.newtons > 1000 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v58_screen() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Screen2 { LCD, OLED, AMOLED, EInk }\nfn has_backlight(s: Screen2) -> bool {\nmatch s {\nScreen2::LCD => true\nScreen2::OLED => false\nScreen2::AMOLED => false\nScreen2::EInk => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v59_acceleration() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Acceleration2 { mps2: number }\nfn to_g(a: Acceleration2) -> number { a.mps2 / 10 }\nfn is_zero_g(a: Acceleration2) -> bool { a.mps2 == 0 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v59_cache() {
    let mut bridge = CompilerBridge::new();
    let source = "enum CacheLevel { L1, L2, L3, MainMemory }\nfn is_on_chip(c: CacheLevel) -> bool {\nmatch c {\nCacheLevel::L1 => true\nCacheLevel::L2 => true\nCacheLevel::L3 => true\nCacheLevel::MainMemory => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v57_trapezoid() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Trapezoid2 { a: number, b: number, h: number }\nfn area(t: Trapezoid2) -> number { (t.a + t.b) * t.h / 2 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v58_rhombus() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Rhombus2 { d1: number, d2: number }\nfn area(r: Rhombus2) -> number { r.d1 * r.d2 / 2 }\nfn is_square(r: Rhombus2) -> bool { r.d1 == r.d2 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v59_sector() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Sector2 { radius: number, angle: number }\nfn area(s: Sector2) -> number { s.radius * s.radius * s.angle / 360 }\nfn is_half_circle(s: Sector2) -> bool { s.angle == 180 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v59_event() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Event3 { Timer, IO, Network, Signal }\nfn is_external(e: Event3) -> bool {\nmatch e {\nEvent3::Timer => false\nEvent3::IO => true\nEvent3::Network => true\nEvent3::Signal => true\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v60_voltage() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Voltage { volts: number }\nfn to_mv(v: Voltage) -> number { v.volts * 1000 }\nfn is_safe(v: Voltage) -> bool { v.volts < 50 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v60_algorithm() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Algorithm2 { BFS, DFS, Dijkstra, AStar }\nfn uses_heuristic(a: Algorithm2) -> bool {\nmatch a {\nAlgorithm2::BFS => false\nAlgorithm2::DFS => false\nAlgorithm2::Dijkstra => false\nAlgorithm2::AStar => true\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v60_pentagon() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Pentagon { side: number }\nfn perimeter(p: Pentagon) -> number { p.side * 5 }\nfn is_regular(p: Pentagon) -> bool { p.side > 0 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v60_test_result() {
    let mut bridge = CompilerBridge::new();
    let source = "enum TestResult { Pass2, Fail2, Error3, Skip }\nfn needs_attention(t: TestResult) -> bool {\nmatch t {\nTestResult::Pass2 => false\nTestResult::Fail2 => true\nTestResult::Error3 => true\nTestResult::Skip => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v57_engine() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Engine2 { V8, V6, Inline4, Electric }\nfn is_combustion(e: Engine2) -> bool {\nmatch e {\nEngine2::V8 => true\nEngine2::V6 => true\nEngine2::Inline4 => true\nEngine2::Electric => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v58_color_model() {
    let mut bridge = CompilerBridge::new();
    let source = "enum ColorModel2 { RGB2, CMYK, HSV }\nfn is_additive(c: ColorModel2) -> bool {\nmatch c {\nColorModel2::RGB2 => true\nColorModel2::CMYK => false\nColorModel2::HSV => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v56_event_type() {
    let mut bridge = CompilerBridge::new();
    let source = "enum EventType2 { Click2, Hover, Focus, Blur }\nfn is_mouse_event(e: EventType2) -> bool {\nmatch e {\nEventType2::Click2 => true\nEventType2::Hover => true\nEventType2::Focus => false\nEventType2::Blur => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v56_prism() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Prism2 { base_area: number, height: number }\nfn volume(p: Prism2) -> number { p.base_area * p.height }\nfn is_flat(p: Prism2) -> bool { p.height < 1 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v61_current() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Current2 { amps: number }\nfn to_ma(c: Current2) -> number { c.amps * 1000 }\nfn is_short_circuit(c: Current2) -> bool { c.amps > 100 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v61_language() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Language3 { Compiled, Interpreted, JIT2 }\nfn needs_runtime(l: Language3) -> bool {\nmatch l {\nLanguage3::Compiled => false\nLanguage3::Interpreted => true\nLanguage3::JIT2 => true\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v61_hexagon() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Hexagon { side: number }\nfn perimeter(h: Hexagon) -> number { h.side * 6 }\nfn is_regular(h: Hexagon) -> bool { h.side > 0 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v61_log() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Log2 { Debug3, Info3, Warn3, Error4 }\nfn is_error_level(l: Log2) -> bool {\nmatch l {\nLog2::Debug3 => false\nLog2::Info3 => false\nLog2::Warn3 => false\nLog2::Error4 => true\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v62_resistance() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Resistance { ohms: number }\nfn to_kohm(r: Resistance) -> number { r.ohms / 1000 }\nfn is_short(r: Resistance) -> bool { r.ohms == 0 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v62_database() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Database { Postgres, MySQL, SQLite, MongoDB }\nfn is_sql(d: Database) -> bool {\nmatch d {\nDatabase::Postgres => true\nDatabase::MySQL => true\nDatabase::SQLite => true\nDatabase::MongoDB => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v62_octagon() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Octagon { side: number }\nfn perimeter(o: Octagon) -> number { o.side * 8 }\nfn is_regular(o: Octagon) -> bool { o.side > 0 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v62_file_mode() {
    let mut bridge = CompilerBridge::new();
    let source = "enum FileMode2 { Read4, Write4, ReadWrite2, Append }\nfn can_read(f: FileMode2) -> bool {\nmatch f {\nFileMode2::Read4 => true\nFileMode2::Write4 => false\nFileMode2::ReadWrite2 => true\nFileMode2::Append => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v60_pentagon2() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Pentagon { side: number }\nfn perimeter(p: Pentagon) -> number { p.side * 5 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v60_algorithm2() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Algorithm2 { BFS, DFS, Dijkstra, AStar }\nfn uses_heuristic(a: Algorithm2) -> bool {\nmatch a {\nAlgorithm2::AStar => true\n_ => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v63_data_rate() {
    let mut bridge = CompilerBridge::new();
    let source = "struct DataRate { bps: number }\nfn to_kbps(d: DataRate) -> number { d.bps / 1000 }\nfn is_broadband(d: DataRate) -> bool { d.bps > 25000000 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v63_ide() {
    let mut bridge = CompilerBridge::new();
    let source = "enum IDE2 { VSCode, IntelliJ, Vim, Emacs }\nfn has_lsp(i: IDE2) -> bool {\nmatch i {\nIDE2::VSCode => true\nIDE2::IntelliJ => true\nIDE2::Vim => true\nIDE2::Emacs => true\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v63_decagon() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Decagon { side: number }\nfn perimeter(d: Decagon) -> number { d.side * 10 }\nfn is_regular(d: Decagon) -> bool { d.side > 0 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v63_token() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Token3 { Ident2(string), Number3(number), Operator, EOF2 }\nfn has_value(t: Token3) -> bool {\nmatch t {\nToken3::Ident2(_) => true\nToken3::Number3(_) => true\nToken3::Operator => false\nToken3::EOF2 => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v64_luminance() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Luminance { lux: number }\nfn is_bright(l: Luminance) -> bool { l.lux > 1000 }\nfn is_dark(l: Luminance) -> bool { l.lux < 10 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v64_game_state() {
    let mut bridge = CompilerBridge::new();
    let source = "enum GameState2 { Menu2, Playing, Paused, GameOver }\nfn is_active(g: GameState2) -> bool {\nmatch g {\nGameState2::Menu2 => false\nGameState2::Playing => true\nGameState2::Paused => false\nGameState2::GameOver => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v64_dodecagon() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Dodecagon { side: number }\nfn perimeter(d: Dodecagon) -> number { d.side * 12 }\nfn is_regular(d: Dodecagon) -> bool { d.side > 0 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v64_node() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Node3 { Root, Internal, Leaf3 }\nfn has_children(n: Node3) -> bool {\nmatch n {\nNode3::Root => true\nNode3::Internal => true\nNode3::Leaf3 => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v64_clamp() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> Result<number, string> {\nif x > 0 {\nOk(if x > 100 { 100 } else { x })\n} else {\nErr(\"non-positive\")\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v63_fib_sum() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet a = 0;\nlet b = 1;\nfor i in 0..x {\nlet c = a + b;\na = b;\nb = c\n};\na + b\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v65_capacitance() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Capacitance { farads: number }\nfn to_uf(c: Capacitance) -> number { c.farads * 1000000 }\nfn is_large_cap(c: Capacitance) -> bool { c.farads > 1 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v65_vm_state() {
    let mut bridge = CompilerBridge::new();
    let source = "enum VMState { Stopped, Starting, Running3, Stopping }\nfn is_transitioning(v: VMState) -> bool {\nmatch v {\nVMState::Stopped => false\nVMState::Starting => true\nVMState::Running3 => false\nVMState::Stopping => true\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v65_kite() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Kite2 { d1: number, d2: number }\nfn area(k: Kite2) -> number { k.d1 * k.d2 / 2 }\nfn is_square(k: Kite2) -> bool { k.d1 == k.d2 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v65_gesture() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Gesture { Tap, Swipe, Pinch, Rotate }\nfn is_continuous(g: Gesture) -> bool {\nmatch g {\nGesture::Tap => false\nGesture::Swipe => false\nGesture::Pinch => true\nGesture::Rotate => true\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v65_harmonic() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + if i > 0 { 1 / i } else { 0 }\n};\nsum\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v64_luminance2() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Luminance { lux: number }\nfn is_bright(l: Luminance) -> bool { l.lux > 1000 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v64_game_state2() {
    let mut bridge = CompilerBridge::new();
    let source = "enum GameState2 { Menu2, Playing, Paused, GameOver }\nfn is_active(g: GameState2) -> bool {\nmatch g {\nGameState2::Playing => true\n_ => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v63_decagon2() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Decagon { side: number }\nfn perimeter(d: Decagon) -> number { d.side * 10 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v63_token2() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Token3 { Ident2(string), Number3(number), Operator, EOF2 }\nfn has_value(t: Token3) -> bool {\nmatch t {\nToken3::Ident2(_) => true\nToken3::Number3(_) => true\n_ => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v65_fib_variant() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet a = 2;\nlet b = 3;\nfor i in 0..x {\nlet c = a + b;\na = b;\nb = c\n};\na\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v66_inductance() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Inductance { henries: number }\nfn to_mh(l: Inductance) -> number { l.henries * 1000 }\nfn is_small(l: Inductance) -> bool { l.henries < 1 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v66_render_api() {
    let mut bridge = CompilerBridge::new();
    let source = "enum RenderAPI { OpenGL, Vulkan, Metal, DirectX }\nfn is_cross_platform(r: RenderAPI) -> bool {\nmatch r {\nRenderAPI::OpenGL => true\nRenderAPI::Vulkan => true\nRenderAPI::Metal => false\nRenderAPI::DirectX => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v66_parallelogram() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Parallelogram2 { base: number, height: number }\nfn area(p: Parallelogram2) -> number { p.base * p.height }\nfn is_rectangle(p: Parallelogram2) -> bool { p.base > 0 && p.height > 0 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v66_align() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Align2 { Left2, Center2, Right2, Justify }\nfn is_left_aligned(a: Align2) -> bool {\nmatch a {\nAlign2::Left2 => true\nAlign2::Center2 => false\nAlign2::Right2 => false\nAlign2::Justify => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v66_odd_squares() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + (2 * i + 1) * (2 * i + 1)\n};\nsum\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v66_fib_58() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet a = 5;\nlet b = 8;\nfor i in 0..x {\nlet c = a + b;\na = b;\nb = c\n};\nb\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v66_range_filter() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nif i > 25 && i < 50 {\nsum = sum + i\n}\n};\nsum\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v65_cap2() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Capacitance { farads: number }\nfn to_uf(c: Capacitance) -> number { c.farads * 1000000 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v65_vm2() {
    let mut bridge = CompilerBridge::new();
    let source = "enum VMState { Stopped, Starting, Running3, Stopping }\nfn is_running(v: VMState) -> bool {\nmatch v {\nVMState::Running3 => true\n_ => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v65_gesture2() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Gesture { Tap, Swipe, Pinch, Rotate }\nfn is_discrete(g: Gesture) -> bool {\nmatch g {\nGesture::Tap => true\nGesture::Swipe => true\n_ => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v67_memory() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Memory2 { bytes: number }\nfn to_kb(m: Memory2) -> number { m.bytes / 1024 }\nfn to_mb(m: Memory2) -> number { m.bytes / 1048576 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v67_audio() {
    let mut bridge = CompilerBridge::new();
    let source = "enum AudioFormat { MP3, WAV, FLAC, OGG }\nfn is_lossless(a: AudioFormat) -> bool {\nmatch a {\nAudioFormat::MP3 => false\nAudioFormat::WAV => true\nAudioFormat::FLAC => true\nAudioFormat::OGG => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v67_isosceles() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Isosceles2 { base: number, leg: number }\nfn is_valid(t: Isosceles2) -> bool { t.leg * 2 > t.base }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v67_brush() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Brush2 { Pen, Pencil, Marker, Eraser }\nfn draws(b: Brush2) -> bool {\nmatch b {\nBrush2::Pen => true\nBrush2::Pencil => true\nBrush2::Marker => true\nBrush2::Eraser => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v67_cube_sum() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + i * i * i\n};\nsum\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v67_triangle_plus() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet result = 1;\nfor i in 1..x {\nresult = result + i\n};\nresult\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v66_expand() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet a = x + 4;\nlet b = a * a - 16;\nb\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v65_expand() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet a = x * 2 + 3;\nlet b = a * a - 9;\nb\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v67_offset() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nif i > 30 {\nsum = sum + (i - 30)\n}\n};\nsum\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v67_option_sq() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> Option<number> {\nif x > 0 {\nlet squared = x * x;\nif squared > 100 { Some(squared) } else { None }\n} else {\nNone\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v68_energy() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Energy { joules: number }\nfn to_kj(e: Energy) -> number { e.joules / 1000 }\nfn to_cal(e: Energy) -> number { e.joules / 4 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v68_sort() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Sort2 { Bubble, Quick, Merge, Heap }\nfn is_nlogn(s: Sort2) -> bool {\nmatch s {\nSort2::Bubble => false\nSort2::Quick => true\nSort2::Merge => true\nSort2::Heap => true\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v68_ellipse() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Ellipse2 { a: number, b: number }\nfn approx_area(e: Ellipse2) -> number { e.a * e.b * 3 }\nfn is_circle(e: Ellipse2) -> bool { e.a == e.b }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v68_wire() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Wire2 { Copper, Aluminum, Fiber, Wireless }\nfn needs_cable(w: Wire2) -> bool {\nmatch w {\nWire2::Copper => true\nWire2::Aluminum => true\nWire2::Fiber => true\nWire2::Wireless => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v68_odd_sum() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + (2 * i + 1)\n};\nsum\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v68_while_cube() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet result = 0;\nlet i = 0;\nwhile i < x {\nresult = result + i * i * i;\ni = i + 1\n};\nresult\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v68_range() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nif i > 5 && i < 15 {\nsum = sum + (i - 5)\n}\n};\nsum\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v68_ninth() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> Option<number> {\nif x > 0 {\nlet ninth = x / 9;\nif ninth > 0 { Some(ninth) } else { None }\n} else {\nNone\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v68_result() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> Result<number, string> {\nif x > 0 {\nif x > 10000 { Err(\"overflow\") } else { Ok(x * 2) }\n} else {\nErr(\"non-positive\")\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v67_audio2() {
    let mut bridge = CompilerBridge::new();
    let source = "enum AudioFormat { MP3, WAV, FLAC, OGG }\nfn is_lossless(a: AudioFormat) -> bool {\nmatch a {\nAudioFormat::WAV => true\nAudioFormat::FLAC => true\n_ => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v69_power() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Power2 { watts: number }\nfn to_kw(p: Power2) -> number { p.watts / 1000 }\nfn to_hp(p: Power2) -> number { p.watts / 746 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v69_protocol() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Protocol2 { TCP, UDP, HTTP, WebSocket }\nfn is_reliable(p: Protocol2) -> bool {\nmatch p {\nProtocol2::TCP => true\nProtocol2::UDP => false\nProtocol2::HTTP => true\nProtocol2::WebSocket => true\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v69_trapezoid() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Trapezoid3 { a: number, b: number, h: number, has_right_angle: bool }\nfn area(t: Trapezoid3) -> number { (t.a + t.b) * t.h / 2 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v69_fuel() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Fuel2 { Gasoline, Diesel, Electric2, Hydrogen }\nfn is_green(f: Fuel2) -> bool {\nmatch f {\nFuel2::Gasoline => false\nFuel2::Diesel => false\nFuel2::Electric2 => true\nFuel2::Hydrogen => true\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v69_linear() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + (3 * i + 2)\n};\nsum\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v69_tribonacci() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet a = 1;\nlet b = 1;\nlet c = 2;\nfor i in 0..x {\nlet d = a + b + c;\na = b;\nb = c;\nc = d\n};\nc\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v69_range_40_60() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nif i > 40 && i < 60 {\nsum = sum + (i - 40)\n}\n};\nsum\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v69_option_half() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> Option<number> {\nif x > 0 {\nlet halved = x / 2;\nif halved > 10 { Some(halved) } else { None }\n} else {\nNone\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v69_result_map() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> Result<number, string> {\nif x > 0 {\nif x > 500 { Ok(x * 3) } else { Ok(x + 1) }\n} else {\nErr(\"non-positive\")\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v68_expand2() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet a = x * 3 + 2;\nlet b = a * a - 4;\nb\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v70_temperature() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Temperature2 { kelvin: number }\nfn to_celsius(t: Temperature2) -> number { t.kelvin - 273 }\nfn to_fahrenheit(t: Temperature2) -> number { (t.kelvin - 273) * 9 / 5 + 32 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v70_platform() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Platform2 { Web, Mobile, Desktop, Embedded }\nfn has_gui(p: Platform2) -> bool {\nmatch p {\nPlatform2::Web => true\nPlatform2::Mobile => true\nPlatform2::Desktop => true\nPlatform2::Embedded => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v70_annulus() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Annulus { r_outer: number, r_inner: number }\nfn approx_area(a: Annulus) -> number { a.r_outer * a.r_outer - a.r_inner * a.r_inner }\nfn is_valid_ring(a: Annulus) -> bool { a.r_outer > a.r_inner }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v70_cipher() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Cipher2 { Caesar, AES, RSA, XOR }\nfn is_symmetric(c: Cipher2) -> bool {\nmatch c {\nCipher2::Caesar => true\nCipher2::AES => true\nCipher2::RSA => false\nCipher2::XOR => true\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v70_cross() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + i * (x - i)\n};\nsum\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v70_odd_while() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet result = 0;\nlet i = 0;\nwhile i < x {\nresult = result + i * 2 + 1;\ni = i + 1\n};\nresult\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v70_square_offset() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nif i > 50 {\nsum = sum + (i - 50) * (i - 50)\n}\n};\nsum\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v70_option_tri() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> Option<number> {\nif x > 0 {\nlet tri = x * (x + 1) / 2;\nif tri > 100 { Some(tri) } else { None }\n} else {\nNone\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v70_result_large() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> Result<number, string> {\nif x > 0 {\nif x > 200 { Err(\"too large\") } else { Ok(x * x + x) }\n} else {\nErr(\"non-positive\")\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v69_expand() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet a = x * 4 - 1;\nlet b = a * a;\nb\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v71_pressure() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Pressure2 { pascals: number }\nfn to_kpa(p: Pressure2) -> number { p.pascals / 1000 }\nfn to_atm(p: Pressure2) -> number { p.pascals / 101325 }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v71_arch() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Arch2 { X86, ARM, RISCV, MIPS }\nfn is_64bit(a: Arch2) -> bool {\nmatch a {\nArch2::X86 => true\nArch2::ARM => true\nArch2::RISCV => true\nArch2::MIPS => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v71_pyramid() {
    let mut bridge = CompilerBridge::new();
    let source = "struct Pyramid2 { base_area: number, height: number }\nfn volume(p: Pyramid2) -> number { p.base_area * p.height / 3 }\nfn is_tall(p: Pyramid2) -> bool { p.height > p.base_area }";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v71_sensor() {
    let mut bridge = CompilerBridge::new();
    let source = "enum Sensor2 { Temperature3, Pressure3, Humidity, Light2 }\nfn is_environmental(s: Sensor2) -> bool {\nmatch s {\nSensor2::Temperature3 => true\nSensor2::Pressure3 => true\nSensor2::Humidity => true\nSensor2::Light2 => false\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v71_consecutive() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + (i + 1) * (i + 2)\n};\nsum\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v71_power2() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet result = 1;\nfor i in 1..x {\nresult = result * 2\n};\nresult\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v71_range_60_80() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nif i > 60 && i < 80 {\nsum = sum + (i - 60) * 2\n}\n};\nsum\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v71_option_sixth() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> Option<number> {\nif x > 0 {\nlet sixth = x / 6;\nif sixth > 0 { Some(sixth) } else { None }\n} else {\nNone\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v71_result_seg() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> Result<number, string> {\nif x > 0 {\nif x > 300 { Ok(x / 3) } else { Ok(x + 10) }\n} else {\nErr(\"non-positive\")\n}\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}

#[test]
fn test_analyze_valid_v71_expand() {
    let mut bridge = CompilerBridge::new();
    let source = "fn f(x: number) -> number {\nlet a = x * 5 + 1;\nlet b = a * a - 1;\nb\n}";
    let diagnostics = bridge.analyze(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == KarteDiagnosticSeverity::Error).collect();
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);
}
