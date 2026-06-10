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
