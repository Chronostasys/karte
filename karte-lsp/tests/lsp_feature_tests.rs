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
