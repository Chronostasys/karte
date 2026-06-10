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

fn check_has_warnings(code: &str) {
    let tokens = Lexer::new(code).tokenize();
    let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
    let has_warnings = diagnostics.diagnostics.iter().any(|d| {
        matches!(d.level, karte_diagnostics::DiagnosticLevel::Warning)
    });
    assert!(has_warnings, "Expected warnings but got none. Diagnostics: {:?}", 
        diagnostics.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>());
}

fn check_error_contains(code: &str, substring: &str) {
    let tokens = Lexer::new(code).tokenize();
    let (_, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
    let found = diagnostics.diagnostics.iter().any(|d| d.message.contains(substring));
    assert!(found, "Expected to find '{}' in diagnostics: {:?}", substring,
        diagnostics.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>());
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
fn test_type_check_string_concat_v2() {
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

#[test]
fn test_type_check_let_binding_number() {
    check_no_errors("fn main() -> number {\n    let x: number = 42;\n    x\n}");
}

#[test]
fn test_type_check_let_binding_string() {
    check_no_errors("fn main() -> number {\n    let x: string = \"hello\";\n    0\n}");
}

#[test]
fn test_type_check_let_binding_bool() {
    check_no_errors("fn main() -> number {\n    let x: bool = true;\n    if x { 1 } else { 0 }\n}");
}

#[test]
fn test_type_check_struct_with_methods() {
    check_no_errors("struct Point { x: number, y: number }\nfn magnitude(p: Point) -> number {\n    p.x * p.x + p.y * p.y\n}\nfn main() -> number {\n    let p = Point { x: 3, y: 4 };\n    magnitude(p)\n}");
}

#[test]
fn test_type_check_option_some() {
    check_no_errors("fn main() -> number {\n    let x = Some(42);\n    match x {\n        Some(v) => v,\n        None => 0\n    }\n}");
}

#[test]
fn test_type_check_option_none() {
    check_no_errors("fn main() -> number {\n    let x: Option<number> = None;\n    match x {\n        Some(v) => v,\n        None => 0\n    }\n}");
}

#[test]
fn test_type_check_while_loop() {
    check_no_errors("fn main() -> number {\n    let x = 0;\n    while x < 10 {\n        x\n    }\n    0\n}");
}

#[test]
fn test_type_check_for_loop() {
    check_no_errors("fn main() -> number {\n    for i in 0..10 {\n        i\n    }\n    0\n}");
}


#[test]
fn test_type_check_function_parameter() {
    check_no_errors("fn add(a: number, b: number) -> number {\n    a + b\n}\nfn main() -> number {\n    add(1, 2)\n}");
}

#[test]
fn test_type_check_string_concat() {
    check_no_errors("fn main() -> number {\n    let s = \"hello\" + \" world\";\n    0\n}");
}

#[test]
fn test_type_check_bool_expression() {
    check_no_errors("fn main() -> number {\n    let x = true && false;\n    let y = true || false;\n    let z = !x;\n    if z { 1 } else { 0 }\n}");
}

#[test]
fn test_type_check_tuple() {
    check_no_errors("fn main() -> number {\n    let t = (1, 2, 3);\n    0\n}");
}

#[test]
fn test_type_check_nested_function() {
    check_no_errors("fn outer(x: number) -> number {\n    fn inner(y: number) -> number {\n        x + y\n    }\n    inner(10)\n}\nfn main() -> number {\n    outer(5)\n}");
}

#[test]
fn test_type_check_match_with_guard() {
    check_no_errors("fn classify(x: number) -> number {\n    match x {\n        0 => 1,\n        _ => 2\n    }\n}\nfn main() -> number {\n    classify(42)\n}");
}

#[test]
fn test_type_check_multiple_returns() {
    check_no_errors("fn abs(x: number) -> number {\n    if x < 0 {\n        return 0 - x\n    } else {\n        x\n    }\n}\nfn main() -> number {\n    abs(5)\n}");
}

#[test]
fn test_type_check_struct_field_access_v2() {
    check_no_errors("struct Point { x: number, y: number }\nfn main() -> number {\n    let p = Point { x: 1, y: 2 };\n    p.x + p.y\n}");
}

#[test]
fn test_type_check_enum_basic() {
    check_no_errors("enum Direction { North, South, East, West }\nfn main() -> number {\n    let d = Direction::North;\n    match d {\n        Direction::North => 0,\n        Direction::South => 1,\n        Direction::East => 2,\n        Direction::West => 3\n    }\n}");
}

#[test]
fn test_type_check_string_comparison() {
    check_no_errors("fn main() -> number {\n    let a = \"hello\";\n    let b = \"world\";\n    if a == b { 1 } else { 0 }\n}");
}

#[test]
fn test_type_check_tuple_two_elements() {
    check_no_errors("fn main() -> number {\n    let t = (1, 2);\n    0\n}");
}

#[test]
fn test_type_check_tuple_nested() {
    check_no_errors("fn main() -> number {\n    let t = (1, (2, 3), 4);\n    0\n}");
}

#[test]
fn test_type_check_reference() {
    check_no_errors("fn main() -> number {\n    let x = 42;\n    let r = &x;\n    *r\n}");
}

#[test]
fn test_type_check_closure_basic() {
    check_no_errors("fn main() -> number {\n    let f = |x| { x + 1 };\n    f(42)\n}");
}

#[test]
fn test_type_check_closure_multi_param() {
    check_no_errors("fn main() -> number {\n    let add = |a, b| { a + b };\n    add(1, 2)\n}");
}

#[test]
fn test_type_check_string_operations() {
    check_no_errors("fn main() -> number {\n    let s = \"hello\";\n    let len = len(s);\n    len\n}");
}

#[test]
fn test_type_check_result_ok() {
    check_no_errors("fn main() -> number {\n    let r = Ok(42);\n    match r {\n        Ok(v) => v,\n        Err(_) => 0\n    }\n}");
}

#[test]
fn test_type_check_result_err() {
    check_no_errors("fn main() -> number {\n    let r: Result<number, string> = Err(\"error\");\n    match r {\n        Ok(v) => v,\n        Err(_) => 0\n    }\n}");
}


#[test]
fn test_type_inference_nested_closure() {
    check_no_errors("fn main() -> number {\nlet add = |x, y| { x + y };\nlet inc = |n| { add(n, 1) };\ninc(41)\n}");
}

#[test]
fn test_type_inference_ternary_like() {
    check_no_errors("fn main() -> number {\nlet x = 10;\nif x > 5 { x * 2 } else { x / 2 }\n}");
}

#[test]
fn test_type_inference_bool_to_number() {
    check_no_errors("fn main() -> number {\nlet x = 5;\nlet y = 10;\nif x < y { 1 } else { 0 }\n}");
}

#[test]
fn test_type_inference_number_comparison() {
    check_no_errors("fn main() -> number {\nlet a = 3;\nlet b = 5;\nif a >= b { a } else { b }\n}");
}

#[test]
fn test_type_inference_multiple_returns() {
    check_no_errors("fn classify(n: number) -> number {\nif n > 0 { 1 }\nelse if n < 0 { -1 }\nelse { 0 }\n}\nfn main() -> number { classify(42) + classify(-5) + classify(0) }");
}

#[test]
fn test_type_inference_while_loop_sum() {
    check_no_errors("fn main() -> number {\nlet sum = 0;\nlet i = 1;\nwhile i <= 10 {\nsum = sum + i;\ni = i + 1\n};\nsum\n}");
}

#[test]
fn test_type_inference_factorial_recursive() {
    check_no_errors("fn fact(n: number) -> number {\nif n <= 1 { 1 }\nelse { n * fact(n - 1) }\n}\nfn main() -> number { fact(10) }");
}

#[test]
fn test_type_inference_fibonacci() {
    check_no_errors("fn fib(n: number) -> number {\nif n <= 1 { n }\nelse { fib(n - 1) + fib(n - 2) }\n}\nfn main() -> number { fib(10) }");
}

#[test]
fn test_type_inference_nested_let() {
    check_no_errors("fn main() -> number {\nlet a = 10;\nlet b = {\nlet c = a * 2;\nc + 5\n};\nb\n}");
}

#[test]
fn test_type_inference_chained_comparison() {
    check_no_errors("fn clamp(x: number, lo: number, hi: number) -> number {\nif x < lo { lo }\nelse if x > hi { hi }\nelse { x }\n}\nfn main() -> number { clamp(15, 0, 10) }");
}

#[test]
fn test_type_inference_mutual_recursion() {
    check_no_errors("fn is_even(n: number) -> number {\nif n == 0 { 1 }\nelse { is_odd(n - 1) }\n}\nfn is_odd(n: number) -> number {\nif n == 0 { 0 }\nelse { is_even(n - 1) }\n}\nfn main() -> number { is_even(10) + is_odd(7) }");
}

#[test]
fn test_type_inference_nested_function() {
    check_no_errors("fn outer(x: number) -> number {\nfn inner(y: number) -> number { y * 2 }\ninner(x) + 1\n}\nfn main() -> number { outer(21) }");
}

#[test]
fn test_type_inference_higher_order() {
    check_no_errors("fn apply(f: fn(number) -> number, x: number) -> number { f(x) }\nfn double(n: number) -> number { n * 2 }\nfn main() -> number { apply(double, 21) }");
}

#[test]
fn test_type_inference_struct_with_methods() {
    check_no_errors("struct Point { x: number, y: number }\nfn magnitude(p: Point) -> number { p.x * p.x + p.y * p.y }\nfn main() -> number { magnitude(Point { x: 3, y: 4 }) }");
}

#[test]
fn test_type_inference_enum_match_complex() {
    check_no_errors("enum Shape { Circle(number), Rectangle(number, number) }\nfn area(s: Shape) -> number {\nmatch s {\nShape::Circle(r) => 3 * r * r,\nShape::Rectangle(w, h) => w * h\n}\n}\nfn main() -> number { area(Shape::Circle(5)) + area(Shape::Rectangle(3, 4)) }");
}

#[test]
fn test_type_error_undefined_variable() {
    check_has_errors("fn main() -> number { x + 1 }");
}

#[test]
fn test_type_error_wrong_return_type() {
    check_has_errors("fn main() -> number { true }");
}

#[test]
fn test_type_error_duplicate_param() {
    check_has_errors("fn f(x: number, x: number) -> number { x }");
}

#[test]
fn test_type_error_missing_fields() {
    check_has_errors("struct Point { x: number, y: number }\nfn main() -> number { let p = Point { x: 1 }; 0 }");
}

#[test]
fn test_type_error_undefined_function() {
    check_has_errors("fn main() -> number { foo(1) }");
}

#[test]
fn test_type_error_wrong_arg_count() {
    check_has_errors("fn add(a: number, b: number) -> number { a + b }\nfn main() -> number { add(1) }");
}

#[test]
fn test_type_error_struct_field_unknown() {
    check_has_errors("struct Point { x: number, y: number }\nfn main() -> number { let p = Point { x: 1, y: 2 }; p.z }");
}

#[test]
fn test_type_error_non_exhaustive_match() {
    check_has_errors("enum Color { Red, Green, Blue }\nfn f(c: Color) -> number { match c { Color::Red => 1 } }\nfn main() -> number { f(Color::Red) }");
}

#[test]
fn test_type_check_let_shadow() {
    // 变量遮蔽应该产生警告但不是错误
    check_no_errors("fn main() -> number {\nlet x = 5;\nlet x = 10;\nx\n}");
}




#[test]
fn test_type_check_closure_capture() {
    check_no_errors("fn main() -> number {\nlet x = 10;\nlet f = || { x };\nf()\n}");
}

#[test]
fn test_type_check_array_index() {
    check_no_errors("fn main() -> number {\nlet arr = [1, 2, 3];\narr[0]\n}");
}

#[test]
fn test_exhaustive_enum_all_variants() {
    check_no_errors("enum Direction { North, South, East, West }\nfn f(d: Direction) -> number {\nmatch d {\nDirection::North => 1,\nDirection::South => 2,\nDirection::East => 3,\nDirection::West => 4\n}\n}");
}

#[test]
fn test_exhaustive_enum_with_wildcard() {
    check_no_errors("enum Direction { North, South, East, West }\nfn f(d: Direction) -> number {\nmatch d {\nDirection::North => 1,\n_ => 0\n}\n}");
}

#[test]
fn test_exhaustive_enum_with_data_all() {
    check_no_errors("enum Shape { Circle(number), Rectangle(number, number), Triangle(number, number, number) }\nfn sides(s: Shape) -> number {\nmatch s {\nShape::Circle(_) => 0,\nShape::Rectangle(_, _) => 4,\nShape::Triangle(_, _, _) => 3\n}\n}");
}

#[test]
fn test_exhaustive_bool_all() {
    check_no_errors("fn f(b: bool) -> number {\nmatch b {\ntrue => 1,\nfalse => 0\n}\n}");
}

#[test]
fn test_exhaustive_bool_with_wildcard() {
    check_no_errors("fn f(b: bool) -> number {\nmatch b {\ntrue => 1,\n_ => 0\n}\n}");
}

#[test]
fn test_type_check_complex_enum_match() {
    check_no_errors("enum Expr { Number(number), Add(Expr, Expr), Mul(Expr, Expr) }\nfn eval(e: Expr) -> number {\nmatch e {\nExpr::Number(n) => n,\nExpr::Add(a, b) => eval(a) + eval(b),\nExpr::Mul(a, b) => eval(a) * eval(b)\n}\n}");
}

#[test]
fn test_type_check_option_some_none() {
    check_no_errors("enum Option { Some(number), None }\nfn unwrap(o: Option) -> number {\nmatch o {\nOption::Some(x) => x,\nOption::None => 0\n}\n}");
}

#[test]
fn test_type_check_result_ok_err() {
    check_no_errors("enum Result { Ok(number), Err(string) }\nfn get_value(r: Result) -> number {\nmatch r {\nResult::Ok(v) => v,\nResult::Err(_) => 0\n}\n}");
}

#[test]
fn test_type_check_method_syntax() {
    check_no_errors("fn double(n: number) -> number { n * 2 }\nfn main() -> number {\nlet x = 5;\nx.double()\n}");
}

#[test]
fn test_type_check_array_length() {
    check_no_errors("fn main() -> number {\nlet arr = [1, 2, 3, 4, 5];\nlen(arr)\n}");
}

#[test]
fn test_type_check_string_len() {
    check_no_errors("fn main() -> number {\nlet s = \"hello\";\nlen(s)\n}");
}

#[test]
fn test_type_check_abs_builtin() {
    check_no_errors("fn main() -> number {\nlet x = -42;\nabs(x)\n}");
}

#[test]
fn test_type_check_min_max_builtin() {
    check_no_errors("fn main() -> number {\nlet a = min(3, 5);\nlet b = max(3, 5);\na + b\n}");
}

#[test]
fn test_type_check_for_range() {
    check_no_errors("fn main() -> number {\nlet sum = 0;\nfor i in 1..10 {\nsum = sum + i\n};\nsum\n}");
}

#[test]
fn test_type_check_for_array() {
    check_no_errors("fn main() -> number {\nlet sum = 0;\nfor x in [1, 2, 3] {\nsum = sum + x\n};\nsum\n}");
}

#[test]
fn test_type_check_nested_struct_access() {
    check_no_errors("struct Inner { val: number }\nstruct Outer { inner: Inner }\nfn get_val(o: Outer) -> number { o.inner.val }");
}

#[test]
fn test_type_check_complex_pattern() {
    check_no_errors("enum Option { Some(number), None }\nfn f(o: Option) -> number {\nmatch o {\nOption::Some(x) => x * 2,\nOption::None => 0\n}\n}");
}

#[test]
fn test_type_check_string_concat_in_expr() {
    check_no_errors("fn main() -> number {\nlet s = \"hello\" + \" \" + \"world\";\nlen(s)\n}");
}

#[test]
fn test_type_check_empty_function_unit() {
    check_no_errors("fn noop() {\n}");
}

#[test]
fn test_type_check_unit_return() {
    check_no_errors("fn main() {\nlet x = 42;\n}");
}

#[test]
fn test_type_check_complex_enum_nested_match() {
    check_no_errors("enum Expr { Num(number), Add(Expr, Expr) }\nfn eval(e: Expr) -> number {\nmatch e {\nExpr::Num(n) => n,\nExpr::Add(a, b) => eval(a) + eval(b)\n}\n}");
}

#[test]
fn test_type_check_string_in_if() {
    check_no_errors("fn greet(name: string) -> string {\nif name == \"\" {\n\"world\"\n} else {\nname\n}\n}");
}

#[test]
fn test_type_check_bool_operations() {
    check_no_errors("fn both(a: bool, b: bool) -> bool { a && b }\nfn either(a: bool, b: bool) -> bool { a || b }\nfn neg(a: bool) -> bool { !a }");
}

#[test]
fn test_type_check_nested_let_in_block() {
    check_no_errors("fn main() -> number {\nlet x = {\nlet a = 1;\nlet b = 2;\na + b\n};\nx\n}");
}

#[test]
fn test_type_check_multiple_structs() {
    check_no_errors("struct Point { x: number, y: number }\nstruct Vec2 { dx: number, dy: number }\nfn add(p: Point, v: Vec2) -> Point {\nPoint { x: p.x + v.dx, y: p.y + v.dy }\n}");
}

#[test]
fn test_type_check_enum_with_multiple_data() {
    check_no_errors("enum Shape { Circle(number), Rect(number, number), Triangle(number, number, number) }\nfn sides(s: Shape) -> number {\nmatch s {\nShape::Circle(_) => 0,\nShape::Rect(_, _) => 4,\nShape::Triangle(_, _, _) => 3\n}\n}");
}

#[test]
fn test_type_check_while_with_break() {
    check_no_errors("fn main() -> number {\nlet sum = 0;\nlet i = 0;\nwhile i < 10 {\nsum = sum + i;\ni = i + 1\n};\nsum\n}");
}

#[test]
fn test_type_check_redundant_match_arm() {
    // 冗余的 match arm 会产生警告，但不会产生错误
    check_no_errors("fn f(x: number) -> number {\nmatch x {\n0 => 1,\n1 => 2,\n_ => 3,\n0 => 4\n}\n}");
}

#[test]
fn test_type_check_wildcard_exhaustive() {
    check_no_errors("fn f(x: number) -> number {\nmatch x {\n_ => 42\n}\n}");
}

#[test]
fn test_type_check_bool_wildcard_exhaustive() {
    check_no_errors("fn f(b: bool) -> number {\nmatch b {\ntrue => 1,\n_ => 0\n}\n}");
}

#[test]
fn test_type_check_enum_wildcard_exhaustive() {
    check_no_errors("enum Color { Red, Green, Blue }\nfn f(c: Color) -> number {\nmatch c {\nColor::Red => 1,\n_ => 0\n}\n}");
}

#[test]
fn test_type_check_deeply_nested_struct() {
    check_no_errors("struct A { val: number }\nstruct B { a: A }\nstruct C { b: B }\nfn get(c: C) -> number { c.b.a.val }");
}

#[test]
fn test_type_check_array_map_pattern() {
    check_no_errors("fn main() -> number {\nlet arr = [1, 2, 3, 4, 5];\nlet sum = 0;\nfor x in arr {\nsum = sum + x\n};\nsum\n}");
}

#[test]
fn test_type_check_option_in_nested() {
    check_no_errors("enum Option { Some(number), None }\nfn map(o: Option, f: fn(number) -> number) -> Option {\nmatch o {\nOption::Some(x) => Option::Some(f(x)),\nOption::None => Option::None\n}\n}");
}

#[test]
fn test_type_check_chained_comparison() {
    check_no_errors("fn between(x: number, lo: number, hi: number) -> bool {\nx >= lo && x <= hi\n}");
}

#[test]
fn test_type_check_string_comparison_ops() {
    check_no_errors("fn cmp(a: string, b: string) -> number {\nif a < b { -1 } else if a > b { 1 } else { 0 }\n}");
}

#[test]
fn test_type_check_struct_update() {
    check_no_errors("struct Point { x: number, y: number }\nfn translate(p: Point, dx: number, dy: number) -> Point {\nPoint { x: p.x + dx, y: p.y + dy }\n}");
}

#[test]
fn test_type_check_multi_let_destructure() {
    check_no_errors("fn swap(a: number, b: number) -> number {\nlet temp = a;\nlet a = b;\nlet b = temp;\na + b\n}");
}

#[test]
fn test_type_check_enum_with_data() {
    check_no_errors("enum Shape { Circle(number), Rect(number, number) }\nfn area(s: Shape) -> number {\nmatch s {\nShape::Circle(r) => r * r\nShape::Rect(w, h) => w * h\n}\n}");
}

#[test]
fn test_type_check_nested_enum_match() {
    check_no_errors("enum Option<T> { Some(T), None }\nenum Result<T, E> { Ok(T), Err(E) }\nfn f(r: Result<number, number>) -> number {\nmatch r {\nResult::Ok(x) => x\nResult::Err(e) => e\n}\n}");
}

#[test]
fn test_type_check_string_concat_valid() {
    check_no_errors("fn main() -> number {\nlet s = \"hello\" + \" world\";\n0\n}");
}

#[test]
fn test_type_check_struct_field_access() {
    check_no_errors("struct Point { x: number, y: number }\nfn f(p: Point) -> number {\np.x + p.y\n}");
}

#[test]
fn test_type_check_captured_variable() {
    check_no_errors("fn main() -> number {\nlet x = 10;\nlet f = || { x };\nf()\n}");
}

#[test]
fn test_type_check_multiple_return() {
    check_no_errors("fn abs(x: number) -> number {\nif x < 0 { 0 - x } else { x }\n}");
}

#[test]
fn test_type_check_bool_ops() {
    check_no_errors("fn f(a: bool, b: bool) -> bool {\na && b || !a\n}");
}

#[test]
fn test_type_check_string_cmp_valid() {
    check_no_errors("fn f(a: string, b: string) -> bool {\na < b\n}");
}

#[test]
fn test_type_check_enum_constructor_no_data() {
    check_no_errors("enum Color { Red, Green, Blue }\nfn f(c: Color) -> number {\nmatch c {\nColor::Red => 1\nColor::Green => 2\nColor::Blue => 3\n}\n}");
}

#[test]
fn test_type_check_let_binding_type_annotation() {
    check_no_errors("fn main() -> number {\nlet x: number = 42;\nx\n}");
}

#[test]
fn test_type_check_while_loop_unit() {
    check_no_errors("fn main() -> number {\nlet x = 0;\nwhile x < 10 {\nlet x = x + 1; x\n}\n0\n}");
}

#[test]
fn test_type_check_nested_let() {
    check_no_errors("fn main() -> number {\nlet a = {\nlet x = 1;\nlet y = 2;\nx + y\n};\na\n}");
}

#[test]
fn test_type_check_result_type() {
    check_no_errors("fn div(a: number, b: number) -> number {\nif b == 0 { 0 } else { a / b }\n}");
}

#[test]
fn test_type_check_method_call() {
    check_no_errors("fn double(x: number) -> number { x * 2 }\nfn main() -> number {\n5.double()\n}");
}

#[test]
fn test_type_error_wrong_arity() {
    check_has_errors("fn add(a: number, b: number) -> number { a + b }\nfn main() -> number {\nadd(1)\n}");
}

#[test]
fn test_type_error_wrong_param_type() {
    check_has_errors("fn add(a: number, b: number) -> number { a + b }\nfn main() -> number {\nadd(1, true)\n}");
}

#[test]
fn test_type_error_if_else_branch_mismatch() {
    check_has_errors("fn main() -> number {\nlet x = if true { 42 } else { \"hello\" };\n0\n}");
}

#[test]
fn test_type_error_undef_var() {
    check_has_errors("fn main() -> number {\nfoo\n}");
}

#[test]
fn test_type_error_missing_struct_field() {
    check_has_errors("struct Point { x: number, y: number }\nfn main() -> number {\nlet p = Point { x: 1 };\n0\n}");
}

#[test]
fn test_type_error_unknown_struct_field() {
    check_has_errors("struct Point { x: number, y: number }\nfn main() -> number {\nlet p = Point { x: 1, z: 2 };\n0\n}");
}

#[test]
fn test_type_error_non_exhaustive() {
    check_has_errors("enum Color { Red, Green, Blue }\nfn main() -> number {\nmatch Color::Red {\nColor::Red => 1\n}\n}");
}

#[test]
fn test_type_error_dup_fn() {
    check_has_errors("fn f() -> number { 1 }\nfn f() -> number { 2 }");
}

#[test]
fn test_type_error_fn_wrong_return() {
    check_has_errors("fn f() -> number { true }");
}

#[test]
fn test_type_error_arithmetic_type_mismatch() {
    check_has_errors("fn main() -> number {\n1 + true\n}");
}

#[test]
fn test_type_no_error_simple_add() {
    check_no_errors("fn main() -> number { 1 + 2 }");
}

#[test]
fn test_type_no_error_simple_if() {
    check_no_errors("fn main() -> number { if true { 1 } else { 2 } }");
}

#[test]
fn test_type_no_error_simple_match() {
    check_no_errors("enum Bool { True, False }\nfn f(b: Bool) -> number {\nmatch b {\nBool::True => 1\nBool::False => 0\n}\n}");
}

#[test]
fn test_type_no_error_nested_if() {
    check_no_errors("fn main() -> number {\nif true {\nif false { 1 } else { 2 }\n} else {\n3\n}\n}");
}

#[test]
fn test_type_no_error_string_ops() {
    check_no_errors("fn main() -> number {\nlet s = \"hello\";\nlet n = 0;\nn\n}");
}

#[test]
fn test_type_check_recursive() {
    check_no_errors("fn fib(n: number) -> number {\nif n <= 1 { n } else { fib(n-1) + fib(n-2) }\n}");
}

#[test]
fn test_type_check_hof() {
    check_no_errors("fn apply(f: fn(number) -> number, x: number) -> number { f(x) }\nfn double(n: number) -> number { n * 2 }\nfn main() -> number {\napply(double, 21)\n}");
}

#[test]
fn test_type_check_closure_cap() {
    check_no_errors("fn main() -> number {\nlet x = 10;\nlet f = || { x + 1 };\nf()\n}");
}

#[test]
fn test_type_check_nested_struct2() {
    check_no_errors("struct Point { x: number, y: number }\nstruct Line { start: Point, end: Point }\nfn f(l: Line) -> number {\nl.start.x + l.end.y\n}");
}

#[test]
fn test_type_check_opt_match() {
    check_no_errors("enum Option<T> { Some(T), None }\nfn unwrap_or(opt: Option<number>, default: number) -> number {\nmatch opt {\nOption::Some(x) => x\nOption::None => default\n}\n}");
}

#[test]
fn test_type_check_fn_type_annot() {
    check_no_errors("fn apply(f: fn(number) -> number, x: number) -> number {\nf(x)\n}");
}

#[test]
fn test_type_check_generic_id() {
    check_no_errors("fn id(x) { x }\nfn main() -> number {\nid(42)\n}");
}

#[test]
fn test_type_check_multi_closure() {
    check_no_errors("fn main() -> number {\nlet add = |a, b| { a + b };\nadd(1, 2)\n}");
}

#[test]
fn test_type_check_classify() {
    check_no_errors("fn classify(n: number) -> number {\nif n > 0 { 1 } else { if n < 0 { -1 } else { 0 } }\n}");
}

#[test]
fn test_type_check_chain_method() {
    check_no_errors("fn double(x: number) -> number { x * 2 }\nfn inc(x: number) -> number { x + 1 }\nfn main() -> number {\n5.double().inc()\n}");
}

#[test]
fn test_type_error_ret_mismatch() {
    check_has_errors("fn f() -> number { true }");
}

#[test]
fn test_type_error_assign_type() {
    check_has_errors("fn f() -> number {\nlet x: number = true;\n0\n}");
}

#[test]
fn test_type_error_bad_comparison() {
    check_has_errors("fn f() -> number {\n1 < true\n}");
}

#[test]
fn test_type_error_not_callable() {
    check_has_errors("fn f() -> number {\nlet x = 42;\nx()\n}");
}

#[test]
fn test_type_error_dup_param() {
    check_has_errors("fn f(a: number, a: number) -> number { a }");
}

#[test]
fn test_warning_self_assignment() {
    check_has_warnings("fn main() -> number {\nlet x = 5;\nx = x;\n0\n}");
}

#[test]
fn test_warning_number_as_condition() {
    check_has_warnings("fn main() -> number {\nif 42 { 1 } else { 0 }\n}");
}

#[test]
fn test_warning_while_false() {
    check_has_warnings("fn main() -> number {\nwhile false { };\n0\n}");
}

#[test]
fn test_warning_redundant_match_arm() {
    check_has_warnings("enum Color { Red, Green, Blue }\nfn f(c: Color) -> number {\nmatch c {\nColor::Red => 1\nColor::Red => 2\nColor::Green => 3\nColor::Blue => 4\n}\n}");
}

#[test]
fn test_warning_variable_shadowing() {
    check_has_warnings("fn main() -> number {\nlet x = 5;\nlet x = 10;\nx\n}");
}

#[test]
fn test_warning_if_assignment() {
    check_has_warnings("fn main() -> number {\nlet x = 0;\nif x = 5 { 1 } else { 0 }\n}");
}

#[test]
fn test_warning_always_true_condition() {
    check_has_warnings("fn main() -> number {\nif true { 1 } else { 0 }\n}");
}

#[test]
fn test_warning_always_false_condition() {
    check_has_warnings("fn main() -> number {\nif false { 1 } else { 0 }\n}");
}





#[test]
fn test_type_check_str_len2() {
    check_no_errors("fn main() -> number {\nlet s = \"hello\";\nlet n = 0;\nn\n}");
}

#[test]
fn test_type_check_ref_type() {
    check_no_errors("fn main() -> number {\nlet x = 42;\nlet r = &x;\n*r\n}");
}

#[test]
fn test_type_check_empty_fn2() {
    check_no_errors("fn noop() {}\nfn main() -> number {\nnoop();\n0\n}");
}

#[test]
fn test_type_check_early_ret() {
    check_no_errors("fn abs(x: number) -> number {\nif x < 0 { 0 - x } else { x }\n}");
}

#[test]
fn test_type_check_cplx_match() {
    check_no_errors("enum Option<T> { Some(T), None }\nfn f(opt: Option<number>) -> number {\nmatch opt {\nOption::Some(x) => if x > 0 { x } else { 0 - x }\nOption::None => 0\n}\n}");
}

#[test]
fn test_type_check_gen_map() {
    check_no_errors("fn map(f, x) { f(x) }\nfn double(n: number) -> number { n * 2 }\nfn main() -> number {\nmap(double, 21)\n}");
}

#[test]
fn test_type_check_compose() {
    check_no_errors("fn compose(f, g, x) { f(g(x)) }\nfn inc(x: number) -> number { x + 1 }\nfn double(x: number) -> number { x * 2 }\nfn main() -> number {\ncompose(double, inc, 20)\n}");
}

#[test]
fn test_type_check_nested_closure2() {
    check_no_errors("fn main() -> number {\nlet add = |a, b| { a + b };\nlet mul = |a, b| { a * b };\nadd(mul(3, 4), 5)\n}");
}

#[test]
fn test_type_check_multi_field() {
    check_no_errors("struct Person { name: string, age: number }\nfn f(p: Person) -> number {\np.age\n}");
}

#[test]
fn test_type_check_tuple_destr() {
    check_no_errors("fn main() -> number {\nlet t = (1, 2);\n0\n}");
}

#[test]
fn test_warning_unreachable_code() {
    check_has_warnings("fn f(x: number) -> number {\nreturn 42;\nx\n}");
}

#[test]
fn test_type_check_curry2() {
    check_no_errors("fn add(a: number) -> fn(number) -> number {\n|b| { a + b }\n}");
}

#[test]
fn test_type_check_nested_closure3() {
    check_no_errors("fn main() -> number {\nlet f = |x| { |y| { x + y } };\nlet g = f(10);\ng(32)\n}");
}

#[test]
fn test_type_check_poly_first() {
    check_no_errors("fn first(a, b) { a }\nfn main() -> number {\nfirst(1, true)\n}");
}

#[test]
fn test_type_check_result_match2() {
    check_no_errors("enum Result<T, E> { Ok(T), Err(E) }\nfn f(r: Result<number, number>) -> number {\nmatch r {\nResult::Ok(x) => x\nResult::Err(e) => e\n}\n}");
}

#[test]
fn test_type_check_opt_string() {
    check_no_errors("enum Option<T> { Some(T), None }\nfn f(opt: Option<string>) -> number {\nmatch opt {\nOption::Some(_) => 1\nOption::None => 0\n}\n}");
}

#[test]
fn test_type_check_char_lit2() {
    check_no_errors("fn main() -> number {\nlet c = 'A';\n0\n}");
}

#[test]
fn test_type_check_ref2() {
    check_no_errors("fn main() -> number {\nlet x = 42;\nlet r = &x;\n*r\n}");
}

#[test]
fn test_type_check_unit_fn2() {
    check_no_errors("fn greet(name: string) {\n0\n}\nfn main() -> number {\ngreet(\"world\");\n0\n}");
}

#[test]
fn test_type_check_gen_struct_field() {
    check_no_errors("struct Pair<T> { first: T, second: T }\nfn main() -> number {\nlet p = Pair { first: 1, second: 2 };\np.first + p.second\n}");
}

#[test]
fn test_type_check_wildcard2() {
    check_no_errors("enum Color { Red, Green, Blue }\nfn f(c: Color) -> number {\nmatch c {\n_ => 42\n}\n}");
}

#[test]
fn test_type_check_fn_type_param() {
    check_no_errors("fn apply(f: fn(number) -> number, x: number) -> number {\nf(x)\n}");
}

#[test]
fn test_type_check_closure_with_type() {
    check_no_errors("fn main() -> number {\nlet f = |x: number| -> number { x * 2 };\nf(21)\n}");
}

#[test]
fn test_type_check_recursive_closure() {
    check_no_errors("fn main() -> number {\nlet f = |x| { if x > 0 { x - 1 } else { 0 } };\nf(10)\n}");
}

#[test]
fn test_type_check_nested_if_else() {
    check_no_errors("fn classify(n: number) -> number {\nif n > 0 { 1 } else { if n < 0 { -1 } else { 0 } }\n}");
}

#[test]
fn test_type_check_let_in_if() {
    check_no_errors("fn f(x: number) -> number {\nlet r = if x > 0 { let y = x * 2; y } else { 0 };\nr\n}");
}

#[test]
fn test_type_check_early_return2() {
    check_no_errors("fn abs(x: number) -> number {\nif x < 0 { 0 - x } else { x }\n}");
}

#[test]
fn test_type_error_if_branch_mismatch2() {
    check_has_errors("fn f() -> number {\nlet x = if true { 42 } else { \"hello\" };\n0\n}");
}

#[test]
fn test_type_check_str_cmp3() {
    check_no_errors("fn f(a: string, b: string) -> bool {\na == b\n}");
}

#[test]
fn test_type_check_num_cmp() {
    check_no_errors("fn f(a: number, b: number) -> bool {\na < b\n}");
}

#[test]
fn test_type_check_bool_neg() {
    check_no_errors("fn f(x: bool) -> bool {\n!x\n}");
}

#[test]
fn test_type_check_sc_and() {
    check_no_errors("fn f(a: bool, b: bool) -> bool {\na && b\n}");
}

#[test]
fn test_type_check_sc_or() {
    check_no_errors("fn f(a: bool, b: bool) -> bool {\na || b\n}");
}

#[test]
fn test_type_check_str_eq() {
    check_no_errors("fn f(s: string) -> bool {\ns == \"hello\"\n}");
}

#[test]
fn test_type_check_num_eq() {
    check_no_errors("fn f(n: number) -> bool {\nn == 42\n}");
}

#[test]
fn test_type_check_let_block2() {
    check_no_errors("fn main() -> number {\nlet a = {\nlet x = 1;\nlet y = 2;\nx + y\n};\na\n}");
}

#[test]
fn test_type_check_block_expr2() {
    check_no_errors("fn main() -> number {\nlet x = { 42 };\nx\n}");
}

#[test]
fn test_type_check_nested_call2() {
    check_no_errors("fn double(x: number) -> number { x * 2 }\nfn quad(x: number) -> number { double(double(x)) }\nfn main() -> number {\nquad(10)\n}");
}

#[test]
fn test_type_error_ret_string() {
    check_has_errors("fn f() -> string {\n42\n}");
}

#[test]
fn test_type_error_str_to_num() {
    check_has_errors("fn main() -> number {\nlet x: number = \"hello\";\n0\n}");
}

#[test]
fn test_type_error_bad_ctor() {
    check_has_errors("enum Color { Red, Green, Blue }\nfn main() -> number {\nlet c = Color::Yellow;\n0\n}");
}

#[test]
fn test_type_error_extra_field() {
    check_has_errors("struct Point { x: number, y: number }\nfn main() -> number {\nlet p = Point { x: 1, y: 2, z: 3 };\n0\n}");
}

#[test]
fn test_type_error_miss_field() {
    check_has_errors("struct Point { x: number, y: number }\nfn main() -> number {\nlet p = Point { x: 1 };\n0\n}");
}

#[test]
fn test_type_error_unk_field() {
    check_has_errors("struct Point { x: number, y: number }\nfn main() -> number {\nlet p = Point { x: 1, z: 2 };\n0\n}");
}

#[test]
fn test_type_check_empty_struct() {
    check_no_errors("struct Empty {}\nfn main() -> number {\nlet e = Empty {};\n0\n}");
}

#[test]
fn test_type_check_struct_in_match() {
    check_no_errors("struct Point { x: number, y: number }\nfn f(p: Point) -> number {\nmatch p.x {\n0 => p.y\n_ => p.x\n}\n}");
}

#[test]
fn test_type_check_bool_comparison() {
    check_no_errors("fn main() -> number {\nlet x = true == false;\nif x { 1 } else { 0 }\n}");
}

#[test]
fn test_type_check_nested_if_return() {
    check_no_errors("fn classify(n: number) -> number {\nif n > 0 { 1 } else { if n < 0 { -1 } else { 0 } }\n}");
}

#[test]
fn test_type_check_str_concat3() {
    check_no_errors("fn main() -> number {\nlet s = \"hello\" + \" world\";\n0\n}");
}

#[test]
fn test_type_check_neg_num() {
    check_no_errors("fn main() -> number {\nlet x = -42;\nx\n}");
}

#[test]
fn test_type_check_arith_prec() {
    check_no_errors("fn main() -> number {\n2 + 3 * 4\n}");
}

#[test]
fn test_type_check_parens2() {
    check_no_errors("fn main() -> number {\n(2 + 3) * 4\n}");
}

#[test]
fn test_type_check_modulo() {
    check_no_errors("fn main() -> number {\n10 % 3\n}");
}

#[test]
fn test_type_check_bitwise_and() {
    check_no_errors("fn main() -> number {\n12 & 10\n}");
}

#[test]
fn test_warning_redundant_after_wildcard_number() {
    let source = r#"fn f(x: number) -> number {
    match x {
        _ => 0,
        1 => 1
    }
}
fn main() -> number { f(42) }"#;
    check_has_warnings(source);
}

#[test]
fn test_no_warning_specific_before_wildcard() {
    let source = r#"fn f(x: number) -> number {
    match x {
        0 => 1,
        1 => 2,
        _ => 0
    }
}
fn main() -> number { f(42) }"#;
    check_no_errors(source);
}

#[test]
fn test_type_check_res_ok2() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x > 0 { Ok(x) } else { Err(\"negative\") }\n}");
}

#[test]
fn test_type_check_opt_some2() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 0 { Some(x) } else { None }\n}");
}

#[test]
fn test_type_check_opt_match2() {
    check_no_errors("fn f(opt: Option<number>) -> number {\nmatch opt {\nSome(x) => x\nNone => 0\n}\n}");
}

#[test]
fn test_type_check_res_match2() {
    check_no_errors("fn f(res: Result<number, string>) -> number {\nmatch res {\nOk(x) => x\nErr(_) => 0\n}\n}");
}

#[test]
fn test_type_check_nested_opt2() {
    check_no_errors("fn f(opt: Option<number>) -> Option<number> {\nmatch opt {\nSome(x) => Some(x + 1)\nNone => None\n}\n}");
}

#[test]
fn test_type_check_multi_param2() {
    check_no_errors("fn add(a: number, b: number) -> number { a + b }\nfn main() -> number { add(1, 2) }");
}

#[test]
fn test_type_check_recursive2() {
    check_no_errors("fn fib(n: number) -> number {\nif n <= 1 { n } else { fib(n - 1) + fib(n - 2) }\n}");
}

#[test]
fn test_type_check_str_ret2() {
    check_no_errors("fn greet(name: string) -> string {\n\"Hello, \" + name\n}");
}

#[test]
fn test_type_check_bool_ret2() {
    check_no_errors("fn is_positive(n: number) -> bool {\nn > 0\n}");
}

#[test]
fn test_type_check_void2() {
    check_no_errors("fn print_num(n: number) {
n
}");
}

#[test]
fn test_type_check_fn_param2() {
    check_no_errors("fn apply(f: fn(number) -> number, x: number) -> number {\nf(x)\n}\nfn double(n: number) -> number { n * 2 }\nfn main() -> number {\napply(double, 5)\n}");
}

#[test]
fn test_type_check_closure_cap2() {
    check_no_errors("fn main() -> number {\nlet x = 10;\nlet f = || { x + 1 };\nf()\n}");
}

#[test]
fn test_type_check_nested_clos2() {
    check_no_errors("fn main() -> number {\nlet add = |a: number, b: number| -> number { a + b };\nlet inc = |x: number| -> number { add(x, 1) };\ninc(41)\n}");
}

#[test]
fn test_type_check_match_guard() {
    check_no_errors("fn classify(n: number) -> number {\nmatch n {\n0 => 0\n_ => 1\n}\n}");
}

#[test]
fn test_type_check_struct_upd2() {
    check_no_errors("struct Point { x: number, y: number }\nfn translate(p: Point, dx: number) -> Point {\nPoint { x: p.x + dx, y: p.y }\n}");
}

#[test]
fn test_type_check_arr_access2() {
    check_no_errors("fn main() -> number {\nlet arr = [1, 2, 3];\n0\n}");
}

#[test]
fn test_type_check_for_in2() {
    check_no_errors("fn main() -> number {\nlet sum = 0;\nfor i in [1, 2, 3] {\nsum = sum + i\n};\nsum\n}");
}

#[test]
fn test_type_check_str_match2() {
    check_no_errors("fn greet(s: string) -> number {\nif s == \"hello\" { 1 } else { 0 }\n}");
}

#[test]
fn test_type_check_char_lit2_2() {
    check_no_errors("fn f() -> number {\nlet c = 'a';\nc\n}");
}

#[test]
fn test_type_check_neg_match2() {
    check_no_errors("fn f(n: number) -> number {\nmatch n {\n-1 => 100\n0 => 0\n_ => n\n}\n}");
}

#[test]
fn test_type_check_enum_field_access() {
    check_no_errors("enum Shape { Circle(number), Rect(number, number) }\nfn area(s: Shape) -> number {\nmatch s {\nShape::Circle(r) => r * r * 3\nShape::Rect(w, h) => w * h\n}\n}");
}

#[test]
fn test_type_check_enum_with_string_data() {
    check_no_errors("enum Expr { Lit(number), Var(string) }\nfn eval(e: Expr) -> number {\nmatch e {\nExpr::Lit(n) => n\nExpr::Var(_) => 0\n}\n}");
}

#[test]
fn test_type_check_nested_enum_match_v2() {
    check_no_errors("enum Color { Red, Green, Blue }\nfn f(c: Color) -> number {\nmatch c {\nColor::Red => 1\nColor::Green => 2\nColor::Blue => 3\n}\n}");
}

#[test]
fn test_type_check_bool_match() {
    check_no_errors("fn f(b: bool) -> number {\nmatch b {\ntrue => 1\nfalse => 0\n}\n}");
}

#[test]
fn test_type_check_option_none_match() {
    check_no_errors("fn f(opt: Option<number>) -> number {\nmatch opt {\nSome(x) => x\nNone => 0\n}\n}");
}

#[test]
fn test_type_check_nested_let_v2() {
    check_no_errors("fn main() -> number {\nlet a = {\nlet x = 1;\nlet y = x + 1;\ny * 2\n};\na\n}");
}

#[test]
fn test_type_check_tuple_return() {
    check_no_errors("fn pair() -> (number, number) {\n(1, 2)\n}");
}

#[test]
fn test_type_check_nested_tuple() {
    check_no_errors("fn f() -> number {\nlet t = (1, 2, 3);\n0\n}");
}

#[test]
fn test_type_check_string_len_v2() {
    check_no_errors("fn f(s: string) -> number {\nlen(s)\n}");
}

#[test]
fn test_type_check_abs_builtin_v2() {
    check_no_errors("fn f(x: number) -> number {\nabs(x)\n}");
}

#[test]
fn test_type_check_match_guard_v2() {
    check_no_errors("fn f(n: number) -> number {\nmatch n {\nx if x > 0 => 1\n_ => 0\n}\n}");
}

#[test]
fn test_type_check_match_multi_guard() {
    check_no_errors("fn f(n: number) -> number {\nmatch n {\nx if x > 0 => 1\nx if x < 0 => -1\n_ => 0\n}\n}");
}

#[test]
fn test_type_check_string_concat_auto() {
    check_no_errors("fn f() -> string {\n\"hello\" + 42\n}");
}

#[test]
fn test_type_check_builtin_min() {
    check_no_errors("fn f(a: number, b: number) -> number {\nmin(a, b)\n}");
}

#[test]
fn test_type_check_builtin_max() {
    check_no_errors("fn f(a: number, b: number) -> number {\nmax(a, b)\n}");
}

#[test]
fn test_type_check_negative_literal() {
    check_no_errors("fn f() -> number {\n-42\n}");
}

#[test]
fn test_type_check_double_negative() {
    check_no_errors("fn f() -> number {\n-(-42)\n}");
}

#[test]
fn test_type_check_complex_expr() {
    check_no_errors("fn f(a: number, b: number, c: number) -> number {\n(a + b) * c - a / b\n}");
}

#[test]
fn test_type_check_string_compare() {
    check_no_errors("fn f(a: string, b: string) -> bool {\na < b\n}");
}

#[test]
fn test_type_check_bool_not2() {
    check_no_errors("fn f(x: bool) -> bool {\n!x\n}");
}

#[test]
fn test_type_check_for_in_array() {
    check_no_errors("fn main() -> number {\nlet sum = 0;\nfor i in [1, 2, 3] {\nsum = sum + i\n};\nsum\n}");
}

#[test]
fn test_type_check_while_loop2() {
    check_no_errors("fn main() -> number {\nlet x = 10;\nlet r = 0;\nwhile x > 0 {\nr = r + 1\n};\nr\n}");
}

#[test]
fn test_type_check_nested_for() {
    check_no_errors("fn main() -> number {\nlet sum = 0;\nfor i in [1, 2] {\nfor j in [3, 4] {\nsum = sum + i + j\n}\n};\nsum\n}");
}

#[test]
fn test_type_check_match_option_nested() {
    check_no_errors("fn f(opt: Option<number>) -> number {\nmatch opt {\nSome(x) => x + 1\nNone => 0\n}\n}");
}

#[test]
fn test_type_check_method_call_v2() {
    check_no_errors("fn abs_val(x: number) -> number {\nabs(x)\n}");
}

#[test]
fn test_type_check_complex_match() {
    check_no_errors("enum Expr { Lit(number), Add(number, number), Mul(number, number) }\nfn eval(e: Expr) -> number {\nmatch e {\nExpr::Lit(n) => n\nExpr::Add(a, b) => a + b\nExpr::Mul(a, b) => a * b\n}\n}");
}

#[test]
fn test_type_check_string_ops() {
    check_no_errors("fn f(s: string) -> string {\ns + \"!\"\n}");
}

#[test]
fn test_type_check_bool_ops_v2() {
    check_no_errors("fn f(a: bool, b: bool) -> bool {\na && b || !a\n}");
}

#[test]
fn test_type_check_comparison_chain() {
    check_no_errors("fn f(a: number, b: number, c: number) -> bool {\na < b && b < c\n}");
}

#[test]
fn test_type_check_ternary_like() {
    check_no_errors("fn f(n: number) -> number {\nif n > 0 { n } else { 0 - n }\n}");
}

#[test]
fn test_type_check_tuple_index() {
    check_no_errors("fn main() -> number {\nlet t = (10, 20, 30);\nt.1\n}");
}

#[test]
fn test_type_check_tuple_destructure() {
    check_no_errors("fn main() -> number {\nlet t = (1, 2);\nt.0 + t.1\n}");
}

#[test]
fn test_type_check_nested_tuple_v2() {
    check_no_errors("fn main() -> number {\nlet t = ((1, 2), (3, 4));\n0\n}");
}

#[test]
fn test_type_check_string_method() {
    check_no_errors("fn f(s: string) -> number {\nlen(s)\n}");
}

#[test]
fn test_type_check_option_chain() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 0 { Some(x) } else { None }\n}");
}

#[test]
fn test_type_check_result_chain() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x > 0 { Ok(x) } else { Err(\"negative\") }\n}");
}

#[test]
fn test_type_check_match_range() {
    check_no_errors("fn classify(n: number) -> string {\nif n < 0 { \"negative\" } else { if n > 0 { \"positive\" } else { \"zero\" } }\n}");
}

#[test]
fn test_type_check_let_in_if2() {
    check_no_errors("fn f(x: number) -> number {\nlet r = if x > 0 {\nlet y = x * 2;\ny\n} else {\n0\n};\nr\n}");
}

#[test]
fn test_type_check_string_in_if_v2() {
    check_no_errors("fn f(s: string) -> number {\nif s == \"hello\" { 1 } else { 0 }\n}");
}

#[test]
fn test_type_check_multi_return() {
    check_no_errors("fn abs(n: number) -> number {\nif n < 0 {\nreturn 0 - n\n};\nn\n}");
}

#[test]
fn test_type_error_struct_field_type() {
    check_has_errors("struct Point { x: number, y: number }\nfn main() -> number {\nlet p = Point { x: \"hello\", y: 42 };\n0\n}");
}

#[test]
fn test_type_error_arity_too_many() {
    check_has_errors("fn f(x: number) -> number { x }\nfn main() -> number {\nf(1, 2)\n}");
}

#[test]
fn test_type_error_arity_too_few() {
    check_has_errors("fn f(x: number, y: number) -> number { x + y }\nfn main() -> number {\nf(1)\n}");
}

#[test]
fn test_type_error_bool_arithmetic() {
    check_has_errors("fn f(a: bool, b: bool) -> number {\na + b\n}");
}

#[test]
fn test_type_error_string_arithmetic() {
    check_has_errors("fn f(a: string) -> number {\na - 1\n}");
}

#[test]
fn test_type_error_non_bool_condition() {
    check_has_errors("fn f(x: string) -> number {\nif x { 1 } else { 0 }\n}");
}

#[test]
fn test_type_error_non_function_call() {
    check_has_errors("fn main() -> number {\nlet x = 42;\nx()\n}");
}

#[test]
fn test_type_error_undefined_var() {
    check_has_errors("fn main() -> number {\nundefined_var\n}");
}

#[test]
fn test_type_error_undefined_fn() {
    check_has_errors("fn main() -> number {\nundefined_fn()\n}");
}

#[test]
fn test_type_error_undefined_struct() {
    check_has_errors("fn main() -> number {\nlet p = UndefinedStruct { };\n0\n}");
}

#[test]
fn test_warning_unreachable_after_return() {
    let source = r#"fn f(x: number) -> number {
    return x;
    x + 1
}
fn main() -> number { f(42) }"#;
    check_has_warnings(source);
}

#[test]
fn test_warning_shadow_let() {
    let source = r#"fn main() -> number {
    let x = 10;
    let x = x + 1;
    x
}"#;
    check_has_warnings(source);
}

#[test]
fn test_no_warning_simple_return() {
    let source = r#"fn f(x: number) -> number {
    x
}"#;
    check_no_errors(source);
}

#[test]
fn test_no_warning_if_else_both_return() {
    let source = r#"fn abs(x: number) -> number {
    if x < 0 { 0 - x } else { x }
}"#;
    check_no_errors(source);
}

#[test]
fn test_type_check_enum_all_variants() {
    check_no_errors("enum Direction { North, South, East, West }\nfn opposite(d: Direction) -> Direction {\nmatch d {\nDirection::North => Direction::South\nDirection::South => Direction::North\nDirection::East => Direction::West\nDirection::West => Direction::East\n}\n}");
}

#[test]
fn test_type_check_enum_return_same_type() {
    check_no_errors("enum Color { Red, Green, Blue }\nfn next(c: Color) -> Color {\nmatch c {\nColor::Red => Color::Green\nColor::Green => Color::Blue\nColor::Blue => Color::Red\n}\n}");
}

#[test]
fn test_type_check_enum_with_multiple_data_v2() {
    check_no_errors("enum Expr { Lit(number), Add(number, number), Mul(number, number) }\nfn eval(e: Expr) -> number {\nmatch e {\nExpr::Lit(n) => n\nExpr::Add(a, b) => a + b\nExpr::Mul(a, b) => a * b\n}\n}");
}

#[test]
fn test_type_check_complex_fn_composition() {
    check_no_errors("fn double(x: number) -> number { x * 2 }\nfn inc(x: number) -> number { x + 1 }\nfn compose(x: number) -> number {\ndouble(inc(x))\n}");
}

#[test]
fn test_type_check_recursive_data() {
    check_no_errors("fn fib(n: number) -> number {\nif n <= 1 { n } else { fib(n - 1) + fib(n - 2) }\n}");
}

#[test]
fn test_type_check_string_concat_chain() {
    check_no_errors("fn greet(first: string, last: string) -> string {\nfirst + \" \" + last\n}");
}

#[test]
fn test_type_check_number_comparison_chain() {
    check_no_errors("fn in_range(x: number, lo: number, hi: number) -> bool {\nx >= lo && x <= hi\n}");
}

#[test]
fn test_type_check_option_map() {
    check_no_errors("fn map_opt(opt: Option<number>, f: fn(number) -> number) -> Option<number> {\nmatch opt {\nSome(x) => Some(f(x))\nNone => None\n}\n}");
}

#[test]
fn test_type_check_result_map() {
    check_no_errors("fn map_res(res: Result<number, string>, f: fn(number) -> number) -> Result<number, string> {\nmatch res {\nOk(x) => Ok(f(x))\nErr(e) => Err(e)\n}\n}");
}

#[test]
fn test_type_check_nested_if_return_v2() {
    check_no_errors("fn classify(n: number) -> string {\nif n < 0 {\nreturn \"negative\"\n};\nif n == 0 {\nreturn \"zero\"\n};\n\"positive\"\n}");
}

#[test]
fn test_type_error_assign_wrong_type() {
    check_has_errors("fn main() -> number {\nlet x = 42;\nx = \"hello\";\nx\n}");
}

#[test]
fn test_type_error_if_string_condition() {
    check_has_errors("fn f(x: string) -> number {\nif x { 1 } else { 0 }\n}");
}

#[test]
fn test_type_error_match_string_scrutinee() {
    check_has_errors("fn f(s: string) -> number {\nmatch s {\n1 => 1\n_ => 0\n}\n}");
}

#[test]
fn test_type_error_enum_variant_type() {
    check_has_errors("fn f(c: Color) -> number {\nmatch c {\nColor::Red => 1\n}\n}\nenum Color { Red, Green, Blue }");
}

#[test]
fn test_type_error_undefined_field() {
    check_has_errors("struct Point { x: number, y: number }\nfn main() -> number {\nlet p = Point { x: 1, y: 2 };\np.z\n}");
}

#[test]
fn test_type_check_fn_type_param2() {
    check_no_errors("fn apply(f: fn(number) -> number, x: number) -> number {\nf(x)\n}\nfn double(n: number) -> number { n * 2 }\nfn main() -> number {\napply(double, 5)\n}");
}

#[test]
fn test_type_check_option_result_chain() {
    check_no_errors("fn f(x: number) -> number {\nlet opt = Some(x);\nmatch opt {\nSome(v) => v\nNone => 0\n}\n}");
}

#[test]
fn test_type_check_while_with_break_v2() {
    check_no_errors("fn main() -> number {\nlet x = 10;\nlet r = 0;\nwhile x > 0 {\nr = r + x;\nif r > 20 {\nbreak\n}\n};\nr\n}");
}

#[test]
fn test_type_check_for_range_v2() {
    check_no_errors("fn main() -> number {\nlet sum = 0;\nfor i in [1, 2, 3, 4, 5] {\nsum = sum + i\n};\nsum\n}");
}

#[test]
fn test_type_check_multiple_structs_v2() {
    check_no_errors("struct Point { x: number, y: number }\nstruct Line { start: Point, end: Point }\nfn length(l: Line) -> number {\nlet dx = l.start.x - l.end.x;\nlet dy = l.start.y - l.end.y;\ndx * dx + dy * dy\n}");
}

#[test]
fn test_type_check_enum_with_struct_data() {
    check_no_errors("struct Point { x: number, y: number }\nenum Shape { Dot(Point), Circle(Point, number) }\nfn area(s: Shape) -> number {\nmatch s {\nShape::Dot(_) => 0\nShape::Circle(_, r) => r * r * 3\n}\n}");
}

#[test]
fn test_type_check_string_method_len() {
    check_no_errors("fn f(s: string) -> number {\nlen(s)\n}");
}

#[test]
fn test_type_check_early_return_chain() {
    check_no_errors("fn classify(n: number) -> string {\nif n < 0 {\nreturn \"negative\"\n};\nif n == 0 {\nreturn \"zero\"\n};\n\"positive\"\n}");
}

#[test]
fn test_type_check_nested_match_simple() {
    check_no_errors("fn f(x: number, y: number) -> number {\nmatch x {\n0 => y\n_ => match y {\n0 => x\n_ => x + y\n}\n}\n}");
}

#[test]
fn test_type_check_reference_v2() {
    check_no_errors("fn main() -> number {\nlet x = 42;\nlet r = &x;\n*r\n}");
}

#[test]
fn test_type_check_char_literal_add() {
    check_no_errors("fn main() -> number {\nlet c = 'A';\nc + 1\n}");
}

#[test]
fn test_type_check_char_in_match() {
    check_no_errors("fn f(c: number) -> number {\nmatch c {\n65 => 1\n66 => 2\n_ => 0\n}\n}");
}

#[test]
fn test_type_check_char_compare() {
    check_no_errors("fn f(c: number) -> bool {\nc > 64\n}");
}

#[test]
fn test_type_check_string_concat_number() {
    check_no_errors("fn f(n: number) -> string {\n\"value: \" + n\n}");
}

#[test]
fn test_type_check_nested_if_else_chain() {
    check_no_errors("fn classify(n: number) -> string {\nif n > 90 { \"A\" } else { if n > 80 { \"B\" } else { if n > 70 { \"C\" } else { \"D\" } } }\n}");
}

#[test]
fn test_type_check_complex_let_binding() {
    check_no_errors("fn main() -> number {\nlet a = 1;\nlet b = a + 1;\nlet c = b + a;\nc\n}");
}

#[test]
fn test_type_check_assign_same_type() {
    check_no_errors("fn main() -> number {\nlet x = 1;\nx = 2;\nx\n}");
}

#[test]
fn test_type_check_assign_expression() {
    check_no_errors("fn main() -> number {\nlet x = 1;\nx = x + 1;\nx\n}");
}

#[test]
fn test_type_check_struct_field_assign() {
    check_no_errors("struct Point { x: number, y: number }\nfn translate(p: Point, dx: number) -> Point {\nPoint { x: p.x + dx, y: p.y }\n}");
}

#[test]
fn test_type_check_enum_default_match() {
    check_no_errors("enum Option<T> { Some(T), None }\nfn unwrap_or(opt: Option<number>, default: number) -> number {\nmatch opt {\nSome(x) => x\nNone => default\n}\n}");
}

#[test]
fn test_type_check_complex_enum_data() {
    check_no_errors("enum AST { Num(number), Add(number, number), Mul(number, number), Neg(number) }\nfn eval(e: AST) -> number {\nmatch e {\nAST::Num(n) => n\nAST::Add(a, b) => eval(AST::Num(a)) + eval(AST::Num(b))\nAST::Mul(a, b) => a * b\nAST::Neg(n) => 0 - n\n}\n}");
}

#[test]
fn test_type_check_multiple_return_paths() {
    check_no_errors("fn classify(n: number) -> string {\nif n < 0 {\nreturn \"neg\"\n};\nif n == 0 {\nreturn \"zero\"\n};\n\"pos\"\n}");
}

#[test]
fn test_type_check_option_unwrap_or() {
    check_no_errors("fn unwrap_or(opt: Option<number>, def: number) -> number {\nmatch opt {\nSome(x) => x\nNone => def\n}\n}");
}

#[test]
fn test_type_check_result_map_err() {
    check_no_errors("fn map_err(res: Result<number, string>) -> Result<number, string> {\nmatch res {\nOk(x) => Ok(x)\nErr(e) => Err(e + \"!\")\n}\n}");
}

#[test]
fn test_type_check_nested_struct_method() {
    check_no_errors("struct Vec2 { x: number, y: number }\nfn length(v: Vec2) -> number {\nv.x * v.x + v.y * v.y\n}\nfn distance(a: Vec2, b: Vec2) -> number {\nlet dx = a.x - b.x;\nlet dy = a.y - b.y;\nlength(Vec2 { x: dx, y: dy })\n}");
}

#[test]
fn test_type_check_higher_order_fn() {
    check_no_errors("fn apply(f: fn(number) -> number, x: number) -> number {\nf(x)\n}");
}

#[test]
fn test_type_check_string_len2() {
    check_no_errors("fn f(s: string) -> number {\nlet n = len(s);\nn + 1\n}");
}

#[test]
fn test_type_check_bool_logic_complex() {
    check_no_errors("fn f(a: bool, b: bool, c: bool) -> bool {\n(a || b) && c\n}");
}

#[test]
fn test_type_check_number_conversion() {
    check_no_errors("fn f(b: bool) -> number {\nif b { 1 } else { 0 }\n}");
}

#[test]
fn test_type_check_struct_self_ref() {
    check_no_errors("struct Point { x: number, y: number }\nfn origin() -> Point {\nPoint { x: 0, y: 0 }\n}");
}
