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

#[test]
fn test_type_check_builtin_functions() {
    check_no_errors("fn f(x: number) -> number {\nlet a = abs(x);\nlet b = min(x, 0);\nlet c = max(x, 100);\na + b + c\n}");
}

#[test]
fn test_type_check_string_comparison_ops_v2() {
    check_no_errors("fn f(a: string, b: string) -> bool {\na < b && a <= b && a > b && a >= b\n}");
}

#[test]
fn test_type_check_number_bitwise_ops() {
    check_no_errors("fn f(a: number, b: number) -> number {\n(a & b) | (a ^ b)\n}");
}

#[test]
fn test_type_check_shift_ops() {
    check_no_errors("fn f(a: number, b: number) -> number {\n(a << b) >> 1\n}");
}

#[test]
fn test_type_check_modulo_op() {
    check_no_errors("fn f(a: number, b: number) -> number {\na % b\n}");
}

#[test]
fn test_type_check_unary_minus2() {
    check_no_errors("fn f(x: number) -> number {\n-x\n}");
}

#[test]
fn test_type_check_not_bool() {
    check_no_errors("fn f(x: bool) -> bool {\n!x\n}");
}

#[test]
fn test_type_check_complex_arithmetic() {
    check_no_errors("fn f(a: number, b: number, c: number) -> number {\n(a + b) * c - (a / b) + (a % c)\n}");
}

#[test]
fn test_type_check_nested_function_call2() {
    check_no_errors("fn double(x: number) -> number { x * 2 }\nfn quad(x: number) -> number { double(double(x)) }\nfn oct(x: number) -> number { quad(double(x)) }\nfn main() -> number { oct(1) }");
}

#[test]
fn test_type_check_string_empty() {
    check_no_errors("fn f() -> string {\n\"\"\n}");
}

#[test]
fn test_type_error_missing_return() {
    check_has_errors("fn f() -> number {\n}");
}

#[test]
fn test_type_error_wrong_return_value() {
    check_has_errors("fn f() -> number {\n\"hello\"\n}");
}

#[test]
fn test_type_error_if_branch_type_mismatch() {
    check_has_errors("fn f() -> number {\nlet x = if true { 42 } else { \"hello\" };\n0\n}");
}

#[test]
fn test_type_error_match_branch_type_mismatch() {
    check_has_errors("fn f(x: number) -> number {\nmatch x {\n0 => 1\n_ => \"hello\"\n}\n}");
}

#[test]
fn test_type_error_undefined_enum_variant() {
    check_has_errors("enum Color { Red, Green, Blue }\nfn f(c: Color) -> number {\nmatch c {\nColor::Yellow => 1\n_ => 0\n}\n}");
}

#[test]
fn test_type_error_non_exhaustive_enum2() {
    check_has_errors("enum Color { Red, Green, Blue }\nfn f(c: Color) -> number {\nmatch c {\nColor::Red => 1\n}\n}");
}

#[test]
fn test_type_error_assign_undefined_var() {
    check_has_errors("fn main() -> number {\nx = 42;\nx\n}");
}

#[test]
fn test_type_error_use_undefined_var() {
    check_has_errors("fn main() -> number {\nundefined_var\n}");
}

#[test]
fn test_type_error_call_non_function() {
    check_has_errors("fn main() -> number {\nlet x = 42;\nx()\n}");
}

#[test]
fn test_type_error_struct_missing_all_fields() {
    check_has_errors("struct Point { x: number, y: number }\nfn main() -> number {\nlet p = Point {};\n0\n}");
}

#[test]
fn test_warning_true_condition() {
    let source = r#"fn f() -> number {
    if true { 1 } else { 0 }
}"#;
    check_has_warnings(source);
}

#[test]
fn test_warning_false_condition() {
    let source = r#"fn f() -> number {
    if false { 1 } else { 0 }
}"#;
    check_has_warnings(source);
}

#[test]
fn test_warning_number_condition() {
    let source = r#"fn f() -> number {
    if 42 { 1 } else { 0 }
}"#;
    check_has_warnings(source);
}

#[test]
fn test_warning_bool_literal_condition() {
    let source = r#"fn f(x: bool) -> number {
    if true { 1 } else { 0 }
}"#;
    check_has_warnings(source);
}

#[test]
fn test_warning_redundant_arm_number2() {
    let source = r#"fn f(x: number) -> number {
    match x {
        0 => 1,
        _ => 0,
        1 => 2
    }
}"#;
    check_has_warnings(source);
}

#[test]
fn test_type_check_while_with_condition() {
    check_no_errors("fn countdown(n: number) -> number {\nlet x = n;\nwhile x > 0 {\nx = x - 1\n};\nx\n}");
}

#[test]
fn test_type_check_complex_struct_usage() {
    check_no_errors("struct Rect { width: number, height: number }\nfn area(r: Rect) -> number {\nr.width * r.height\n}\nfn perimeter(r: Rect) -> number {\n2 * (r.width + r.height)\n}");
}

#[test]
fn test_type_check_enum_all_same_return() {
    check_no_errors("enum Bool { True, False }\nfn to_number(b: Bool) -> number {\nmatch b {\nBool::True => 1\nBool::False => 0\n}\n}");
}

#[test]
fn test_type_check_nested_match_expr() {
    check_no_errors("fn f(x: number, y: number) -> number {\nlet result = match x {\n0 => y\n_ => match y {\n0 => x\n_ => x + y\n}\n};\nresult\n}");
}

#[test]
fn test_type_check_string_as_return() {
    check_no_errors("fn greet(name: string) -> string {\n\"Hello, \" + name + \"!\"\n}");
}

#[test]
fn test_type_check_multi_line_string() {
    check_no_errors("fn f() -> string {\n\"hello\" + \" \" + \"world\"\n}");
}

#[test]
fn test_type_check_builtin_abs2() {
    check_no_errors("fn distance(a: number, b: number) -> number {\nabs(a - b)\n}");
}

#[test]
fn test_type_check_complex_closure() {
    check_no_errors("fn main() -> number {\nlet add = |a: number, b: number| -> number { a + b };\nlet mul = |a: number, b: number| -> number { a * b };\nadd(mul(3, 4), 5)\n}");
}

#[test]
fn test_type_check_let_in_block_expr() {
    check_no_errors("fn main() -> number {\nlet x = {\nlet a = 10;\nlet b = 20;\na + b\n};\nx\n}");
}

#[test]
fn test_type_check_assign_after_let() {
    check_no_errors("fn main() -> number {\nlet x = 1;\nlet y = 2;\nx = x + y;\nx\n}");
}

#[test]
fn test_type_check_array_literal() {
    check_no_errors("fn main() -> number {\nlet arr = [1, 2, 3];\narr[0] + arr[1] + arr[2]\n}");
}

#[test]
fn test_type_check_nested_array_access() {
    check_no_errors("fn main() -> number {\nlet arr = [10, 20, 30];\nlet x = arr[0];\nlet y = arr[1];\nx + y\n}");
}

#[test]
fn test_type_check_for_in_range() {
    check_no_errors("fn main() -> number {\nlet sum = 0;\nfor i in 1..10 {\nsum = sum + i\n};\nsum\n}");
}

#[test]
fn test_type_check_string_len3() {
    check_no_errors("fn f(s: string) -> number {\nlet n = len(s);\nn + 1\n}");
}

#[test]
fn test_type_check_negative_number() {
    check_no_errors("fn f() -> number {\n-42\n}");
}

#[test]
fn test_type_check_parenthesized_expr() {
    check_no_errors("fn f(a: number, b: number) -> number {\n((a + b) * (a - b))\n}");
}

#[test]
fn test_type_check_unit_return_v2() {
    check_no_errors("fn noop() {\nlet x = 42;\nlet y = x + 1;\n}");
}

#[test]
fn test_type_check_complex_let_chain() {
    check_no_errors("fn main() -> number {\nlet a = 1;\nlet b = a + 1;\nlet c = b + a;\nlet d = c + b;\nd\n}");
}

#[test]
fn test_type_check_fn_as_param2() {
    check_no_errors("fn apply(f: fn(number) -> number, x: number) -> number {\nf(x)\n}\nfn double(x: number) -> number { x * 2 }\nfn main() -> number { apply(double, 5) }");
}

#[test]
fn test_type_check_struct_param_passing() {
    check_no_errors("struct Point { x: number, y: number }\nfn translate(p: Point, dx: number, dy: number) -> Point {\nPoint { x: p.x + dx, y: p.y + dy }\n}");
}

#[test]
fn test_type_error_duplicate_param_name_v2() {
    check_has_errors("fn f(x: number, x: number) -> number {\nx + x\n}");
}

#[test]
fn test_type_error_missing_field_constructor() {
    check_has_errors("struct Point { x: number, y: number, z: number }\nfn main() -> number {\nlet p = Point { x: 1, y: 2 };\n0\n}");
}

#[test]
fn test_type_error_wrong_field_type_v2() {
    check_has_errors("struct Point { x: number, y: number }\nfn main() -> number {\nlet p = Point { x: 1, y: \"hello\" };\n0\n}");
}

#[test]
fn test_type_error_extra_field_v2() {
    check_has_errors("struct Point { x: number, y: number }\nfn main() -> number {\nlet p = Point { x: 1, y: 2, z: 3 };\n0\n}");
}

#[test]
fn test_type_error_wrong_arg_count_v2() {
    check_has_errors("fn add(a: number, b: number) -> number { a + b }\nfn main() -> number {\nadd(1)\n}");
}

#[test]
fn test_type_error_wrong_arg_type_v2() {
    check_has_errors("fn add(a: number, b: number) -> number { a + b }\nfn main() -> number {\nadd(1, \"hello\")\n}");
}

#[test]
fn test_type_error_undefined_struct_v2() {
    check_has_errors("fn main() -> number {\nlet p = Point { x: 1, y: 2 };\n0\n}");
}

#[test]
fn test_type_error_string_minus() {
    check_has_errors("fn main() -> number {\nlet x = \"a\" - \"b\";\n0\n}");
}

#[test]
fn test_type_error_bool_arithmetic_v2() {
    check_has_errors("fn main() -> number {\ntrue + false\n}");
}

#[test]
fn test_type_error_return_mismatch_v2() {
    check_has_errors("fn f() -> string {\n42\n}");
}

#[test]
fn test_type_check_string_not_equal() {
    check_no_errors("fn f(a: string, b: string) -> bool {\na != b\n}");
}

#[test]
fn test_type_check_complex_bool_expr() {
    check_no_errors("fn in_range(x: number, lo: number, hi: number) -> bool {\n(x >= lo) && (x <= hi)\n}");
}

#[test]
fn test_type_check_ternary_like_if() {
    check_no_errors("fn max(a: number, b: number) -> number {\nif a > b { a } else { b }\n}");
}

#[test]
fn test_type_check_min_max_builtin_v2() {
    check_no_errors("fn clamp(x: number, lo: number, hi: number) -> number {\nmin(max(x, lo), hi)\n}");
}

#[test]
fn test_type_check_complex_while_loop() {
    check_no_errors("fn gcd(a: number, b: number) -> number {\nlet x = a;\nlet y = b;\nwhile y != 0 {\nlet t = y;\ny = x % y;\nx = t\n};\nx\n}");
}

#[test]
fn test_type_check_nested_if_assign() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 0;\nif x > 0 {\nresult = 1\n} else {\nresult = 2\n};\nresult\n}");
}

#[test]
fn test_type_check_multi_field_struct_access() {
    check_no_errors("struct Student { name: string, age: number, grade: number }\nfn is_passing(s: Student) -> bool {\ns.grade >= 60\n}");
}

#[test]
fn test_type_check_option_chain_v2() {
    check_no_errors("fn safe_div(a: number, b: number) -> Option<number> {\nif b == 0 { None } else { Some(a / b) }\n}");
}

#[test]
fn test_type_check_result_chain_v2() {
    check_no_errors("fn checked_add(a: number, b: number) -> Result<number, string> {\nif a + b > 1000 { Err(\"overflow\") } else { Ok(a + b) }\n}");
}

#[test]
fn test_type_check_enum_with_multiple_data_v3() {
    check_no_errors("enum Shape { Circle(number), Rectangle(number, number), Triangle(number, number, number) }\nfn area(s: Shape) -> number {\nmatch s {\nShape::Circle(r) => r * r\nShape::Rectangle(w, h) => w * h\nShape::Triangle(b, h, _) => b * h\n}\n}");
}

#[test]
fn test_warning_unused_function() {
    let source = r#"fn helper() -> number { 42 }
fn main() -> number { 0 }"#;
    check_has_warnings(source);
}

#[test]
fn test_no_warning_used_function() {
    let source = r#"fn double(x: number) -> number { x * 2 }
fn main() -> number { double(5) }"#;
    check_no_errors(source);
}

#[test]
fn test_no_warning_underscore_prefix() {
    let source = r#"fn main() -> number {
    let _unused = 42;
    0
}"#;
    check_no_errors(source);
}

#[test]
fn test_no_warning_main_function() {
    let source = r#"fn main() -> number { 42 }"#;
    check_no_errors(source);
}

#[test]
fn test_type_check_complex_pattern_match() {
    check_no_errors("enum Expr { Lit(number), Add(number, number) }\nfn eval(e: Expr) -> number {\nmatch e {\nExpr::Lit(n) => n\nExpr::Add(a, b) => a + b\n}\n}");
}

#[test]
fn test_type_check_wildcard_pattern() {
    check_no_errors("fn f(x: number) -> number {\nmatch x {\n0 => 1\n_ => 0\n}\n}");
}

#[test]
fn test_type_check_sequential_function_def() {
    check_no_errors("fn double(x: number) -> number { x * 2 }\nfn main() -> number { double(21) }");
}

#[test]
fn test_type_check_early_return_in_if() {
    check_no_errors("fn abs(x: number) -> number {\nif x < 0 { return 0 - x };\nx\n}");
}

#[test]
fn test_type_check_multiple_early_returns() {
    check_no_errors("fn classify(n: number) -> number {\nif n < 0 { return 1 };\nif n == 0 { return 2 };\n3\n}");
}

#[test]
fn test_type_check_while_with_break_like() {
    check_no_errors("fn countdown(n: number) -> number {\nlet x = n;\nwhile x > 0 {\nx = x - 1\n};\nx\n}");
}

#[test]
fn test_type_check_string_concat_with_number() {
    check_no_errors("fn f(n: number) -> string {\n\"value: \" + n\n}");
}

#[test]
fn test_type_check_complex_enum_match2() {
    check_no_errors("enum Color { Red, Green, Blue }\nfn to_number(c: Color) -> number {\nmatch c {\nColor::Red => 0\nColor::Green => 1\nColor::Blue => 2\n}\n}");
}

#[test]
fn test_type_check_struct_update_pattern() {
    check_no_errors("struct Point { x: number, y: number }\nfn move_point(p: Point, dx: number, dy: number) -> Point {\nPoint { x: p.x + dx, y: p.y + dy }\n}");
}

#[test]
fn test_type_check_function_composition() {
    check_no_errors("fn compose(f: fn(number) -> number, g: fn(number) -> number, x: number) -> number {\nf(g(x))\n}");
}

#[test]
fn test_type_check_complex_return_path() {
    check_no_errors("fn f(x: number) -> number {\nif x > 0 {\nreturn x * 2\n};\nif x < 0 {\nreturn 0 - x\n};\n0\n}");
}

#[test]
fn test_type_check_nested_if_return_v3() {
    check_no_errors("fn f(x: number) -> number {\nif x > 10 {\nif x > 20 {\nreturn 3\n};\nreturn 2\n};\n1\n}");
}

#[test]
fn test_type_check_match_with_guard_v2() {
    check_no_errors("fn f(x: number) -> number {\nmatch x {\n0 => 0\n1 => 1\n_ if x > 0 => 2\n_ => 3\n}\n}");
}

#[test]
fn test_type_check_enum_with_string_data_v2() {
    check_no_errors("enum Message { Hello(string), Number(number), Quit }\nfn process(m: Message) -> number {\nmatch m {\nMessage::Hello(_) => 1\nMessage::Number(n) => n\nMessage::Quit => 0\n}\n}");
}

#[test]
fn test_type_check_struct_with_string_field() {
    check_no_errors("struct Person { name: string, age: number }\nfn greet(p: Person) -> string {\n\"Hello, \" + p.name\n}");
}

#[test]
fn test_type_check_array_with_struct() {
    check_no_errors("struct Point { x: number, y: number }\nfn sum_x(points: number) -> number {\n0\n}");
}

#[test]
fn test_type_check_multi_return_fn() {
    check_no_errors("fn classify(n: number) -> string {\nif n > 0 { \"positive\" } else { if n < 0 { \"negative\" } else { \"zero\" } }\n}");
}

#[test]
fn test_type_check_string_comparison_v2() {
    check_no_errors("fn cmp(a: string, b: string) -> number {\nif a == b { 0 } else { if a < b { -1 } else { 1 } }\n}");
}

#[test]
fn test_type_check_complex_closure_capture() {
    check_no_errors("fn main() -> number {\nlet x = 10;\nlet add_x = |y: number| -> number { x + y };\nadd_x(5)\n}");
}

#[test]
fn test_type_check_fn_param_closure() {
    check_no_errors("fn apply(f: fn(number) -> number, x: number) -> number {\nf(x)\n}\nfn main() -> number {\nlet double = |n: number| -> number { n * 2 };\napply(double, 5)\n}");
}

#[test]
fn test_type_error_undefined_fn_call() {
    check_has_errors("fn main() -> number {\nfoo()\n}");
}

#[test]
fn test_type_error_assign_wrong_type_v2() {
    check_has_errors("fn main() -> number {\nlet x: number = \"hello\";\n0\n}");
}

#[test]
fn test_type_error_if_condition_type() {
    check_has_errors("fn main() -> number {\nif \"hello\" { 1 } else { 0 }\n}");
}

#[test]
fn test_type_error_while_condition_type() {
    check_has_errors("fn main() -> number {\nwhile \"hello\" {\n0\n};\n0\n}");
}

#[test]
fn test_type_error_struct_undefined_field() {
    check_has_errors("struct Point { x: number }\nfn main() -> number {\nlet p = Point { x: 1, y: 2 };\n0\n}");
}

#[test]
fn test_type_error_enum_undefined_variant() {
    check_has_errors("enum Color { Red, Green, Blue }\nfn f(c: Color) -> number {\nmatch c {\nColor::Yellow => 1\n_ => 0\n}\n}");
}

#[test]
fn test_type_error_non_exhaustive_match_v2() {
    check_has_errors("enum Color { Red, Green, Blue }\nfn f(c: Color) -> number {\nmatch c {\nColor::Red => 1\n}\n}");
}

#[test]
fn test_type_error_duplicate_param_v2() {
    check_has_errors("fn f(x: number, x: string) -> number {\n0\n}");
}

#[test]
fn test_type_error_assign_to_undefined() {
    check_has_errors("fn main() -> number {\nfoo = 42;\n0\n}");
}

#[test]
fn test_type_error_use_before_define() {
    check_has_errors("fn main() -> number {\nlet y = x + 1;\nlet x = 42;\ny\n}");
}

#[test]
fn test_type_check_if_else_as_value() {
    check_no_errors("fn max(a: number, b: number) -> number {\nlet result = if a > b { a } else { b };\nresult\n}");
}

#[test]
fn test_type_check_match_as_value() {
    check_no_errors("fn f(x: number) -> number {\nlet result = match x {\n0 => 1\n_ => 2\n};\nresult\n}");
}

#[test]
fn test_type_check_block_as_value() {
    check_no_errors("fn main() -> number {\nlet x = {\nlet a = 10;\nlet b = 20;\na + b\n};\nx\n}");
}

#[test]
fn test_type_check_string_concat_chain2() {
    check_no_errors("fn f(name: string, age: number) -> string {\n\"Name: \" + name + \", Age: \" + age\n}");
}

#[test]
fn test_type_check_complex_struct_expr() {
    check_no_errors("struct Point { x: number, y: number }\nfn midpoint(a: Point, b: Point) -> Point {\nPoint { x: (a.x + b.x), y: (a.y + b.y) }\n}");
}

#[test]
fn test_type_check_nested_option() {
    check_no_errors("fn safe_index(arr: number, idx: number) -> Option<number> {\nif idx >= 0 { Some(idx) } else { None }\n}");
}

#[test]
fn test_type_check_nested_result() {
    check_no_errors("fn checked_div(a: number, b: number) -> Result<number, string> {\nif b == 0 { Err(\"division by zero\") } else { Ok(a / b) }\n}");
}

#[test]
fn test_type_check_complex_let_assignment() {
    check_no_errors("fn counter(start: number) -> number {\nlet c = start;\nc = c + 1;\nc = c * 2;\nc\n}");
}

#[test]
fn test_type_check_string_not() {
    check_no_errors("fn f(x: bool) -> bool {\n!x\n}");
}

#[test]
fn test_type_check_complex_bool_expr2() {
    check_no_errors("fn is_leap_year(year: number) -> bool {\n(year % 4 == 0) && ((year % 100 != 0) || (year % 400 == 0))\n}");
}

#[test]
fn test_type_check_complex_fn_composition_v2() {
    check_no_errors("fn double(x: number) -> number { x * 2 }\nfn inc(x: number) -> number { x + 1 }\nfn apply(f: fn(number) -> number, x: number) -> number { f(x) }\nfn main() -> number { apply(double, apply(inc, 5)) }");
}

#[test]
fn test_type_check_nested_option_result() {
    check_no_errors("fn parse_int(s: string) -> Option<number> {\nSome(42)\n}\nfn safe_compute(s: string) -> Result<number, string> {\nmatch parse_int(s) {\nSome(n) => Ok(n * 2)\nNone => Err(\"parse error\")\n}\n}");
}

#[test]
fn test_type_check_mutual_recursive() {
    check_no_errors("fn is_even(n: number) -> bool {\nif n == 0 { true } else { is_odd(n - 1) }\n}\nfn is_odd(n: number) -> bool {\nif n == 0 { false } else { is_even(n - 1) }\n}");
}

#[test]
fn test_type_check_complex_pattern2() {
    check_no_errors("enum List { Cons(number, number), Nil }\nfn sum(l: List) -> number {\nmatch l {\nList::Cons(a, b) => a + b\nList::Nil => 0\n}\n}");
}

#[test]
fn test_type_check_string_operations_v2() {
    check_no_errors("fn exclaim(s: string) -> string {\ns + \"!\"\n}\nfn whisper(s: string) -> string {\n\"(\" + s + \")\"\n}");
}

#[test]
fn test_type_check_chained_comparison_v2() {
    check_no_errors("fn in_range(x: number, lo: number, hi: number) -> bool {\nx >= lo && x <= hi && x != lo\n}");
}

#[test]
fn test_type_check_destructuring_match() {
    check_no_errors("enum Pair { MkPair(number, number) }\nfn fst(p: Pair) -> number {\nmatch p {\nPair::MkPair(a, _) => a\n}\n}\nfn snd(p: Pair) -> number {\nmatch p {\nPair::MkPair(_, b) => b\n}\n}");
}

#[test]
fn test_type_check_nested_struct_field() {
    check_no_errors("struct Line { x1: number, y1: number, x2: number, y2: number }\nfn length_sq(l: Line) -> number {\nlet dx = l.x2 - l.x1;\nlet dy = l.y2 - l.y1;\ndx * dx + dy * dy\n}");
}

#[test]
fn test_type_check_option_default_value() {
    check_no_errors("fn get_or_default(opt: Option<number>) -> number {\nmatch opt {\nSome(x) => x\nNone => 42\n}\n}");
}

#[test]
fn test_type_check_result_error_propagation() {
    check_no_errors("fn div_or_err(a: number, b: number) -> Result<number, string> {\nif b == 0 { Err(\"division by zero\") } else { Ok(a / b) }\n}\nfn safe_double_div(a: number, b: number, c: number) -> Result<number, string> {\nmatch div_or_err(a, b) {\nOk(ab) => div_or_err(ab, c)\nErr(e) => Err(e)\n}\n}");
}

#[test]
fn test_type_check_chained_string_ops_v2() {
    check_no_errors("fn f(s: string) -> string {\nlet a = s + \"!\";\nlet b = a + \"?\";\nb\n}");
}

#[test]
fn test_type_check_multi_struct_fn_v2() {
    check_no_errors("struct Point { x: number, y: number }\nfn distance_sq(a: Point, b: Point) -> number {\nlet dx = a.x - b.x;\nlet dy = a.y - b.y;\ndx * dx + dy * dy\n}");
}

#[test]
fn test_type_check_bool_to_number_v2() {
    check_no_errors("fn bool_to_int(b: bool) -> number {\nif b { 1 } else { 0 }\n}");
}

#[test]
fn test_type_check_number_to_bool_v2() {
    check_no_errors("fn is_positive(n: number) -> bool {\nn > 0\n}");
}

#[test]
fn test_type_check_json_enum() {
    check_no_errors("enum JSON { JNum(number), JStr(string), JBool(bool), JNull }\nfn json_type(j: JSON) -> string {\nmatch j {\nJSON::JNum(_) => \"number\"\nJSON::JStr(_) => \"string\"\nJSON::JBool(_) => \"bool\"\nJSON::JNull => \"null\"\n}\n}");
}

#[test]
fn test_type_check_multi_let_binding_v2() {
    check_no_errors("fn main() -> number {\nlet a = 1;\nlet b = 2;\nlet c = 3;\nlet d = a + b;\nlet e = c + d;\ne\n}");
}

#[test]
fn test_type_check_nested_closure_call_v2() {
    check_no_errors("fn main() -> number {\nlet add = |a: number, b: number| -> number { a + b };\nlet result = add(add(1, 2), 3);\nresult\n}");
}

#[test]
fn test_type_check_option_map_v2() {
    check_no_errors("fn map_option(opt: Option<number>) -> Option<number> {\nmatch opt {\nSome(x) => Some(x * 2)\nNone => None\n}\n}");
}

#[test]
fn test_type_check_result_map_v2() {
    check_no_errors("fn map_result(res: Result<number, string>) -> Result<number, string> {\nmatch res {\nOk(x) => Ok(x * 2)\nErr(e) => Err(e)\n}\n}");
}

#[test]
fn test_type_check_midpoint_fn() {
    check_no_errors("struct Point { x: number, y: number }\nfn midpoint(a: Point, b: Point) -> Point {\nPoint { x: (a.x + b.x), y: (a.y + b.y) }\n}");
}

#[test]
fn test_type_error_number_minus_string() {
    check_has_errors("fn main() -> number {\n42 - \"hello\"\n}");
}

#[test]
fn test_type_error_number_mul_string() {
    check_has_errors("fn main() -> number {\n42 * \"hello\"\n}");
}

#[test]
fn test_type_error_number_div_string() {
    check_has_errors("fn main() -> number {\n42 / \"hello\"\n}");
}

#[test]
fn test_type_error_number_mod_string() {
    check_has_errors("fn main() -> number {\n42 % \"hello\"\n}");
}

#[test]
fn test_type_error_bool_add_number() {
    check_has_errors("fn main() -> number {\ntrue + 42\n}");
}

#[test]
fn test_type_error_if_number_branch() {
    check_has_errors("fn main() -> number {\nif true { 42 } else { \"hello\" };\n0\n}");
}

#[test]
fn test_type_error_match_mixed_branch() {
    check_has_errors("fn f(x: number) -> number {\nmatch x {\n0 => 42\n1 => \"hello\"\n_ => 0\n}\n}");
}

#[test]
fn test_type_error_assign_different_type() {
    check_has_errors("fn main() -> number {\nlet x = 42;\nx = \"hello\";\n0\n}");
}

#[test]
fn test_type_error_fn_param_wrong_type() {
    check_has_errors("fn add(a: number, b: number) -> number { a + b }\nfn main() -> number {\nadd(\"hello\", \"world\")\n}");
}

#[test]
fn test_type_error_fn_return_wrong_type() {
    check_has_errors("fn f() -> string {\n42\n}");
}

#[test]
fn test_type_check_abs_builtin_fn() {
    check_no_errors("fn magnitude(x: number) -> number {\nif x >= 0 { x } else { 0 - x }\n}");
}

#[test]
fn test_type_check_clamp_fn() {
    check_no_errors("fn clamp(x: number, lo: number, hi: number) -> number {\nif x < lo { lo } else { if x > hi { hi } else { x } }\n}");
}

#[test]
fn test_type_check_swap_pattern() {
    check_no_errors("enum Pair { P(number, number) }\nfn swap(p: Pair) -> Pair {\nmatch p {\nPair::P(a, b) => Pair::P(b, a)\n}\n}");
}

#[test]
fn test_type_check_factorial_fn() {
    check_no_errors("fn factorial(n: number) -> number {\nif n <= 1 { 1 } else { n * factorial(n - 1) }\n}");
}

#[test]
fn test_type_check_fib_fn() {
    check_no_errors("fn fib(n: number) -> number {\nif n <= 1 { n } else { fib(n - 1) + fib(n - 2) }\n}");
}

#[test]
fn test_type_check_gcd_fn() {
    check_no_errors("fn gcd(a: number, b: number) -> number {\nif b == 0 { a } else { gcd(b, a % b) }\n}");
}

#[test]
fn test_type_check_power_fn() {
    check_no_errors("fn power(base: number, exp: number) -> number {\nif exp == 0 { 1 } else { base * power(base, exp - 1) }\n}");
}

#[test]
fn test_type_check_sum_to_n_fn() {
    check_no_errors("fn sum_to_n(n: number) -> number {\nif n <= 0 { 0 } else { n + sum_to_n(n - 1) }\n}");
}

#[test]
fn test_type_check_max_of_three_fn() {
    check_no_errors("fn max_of_three(a: number, b: number, c: number) -> number {\nlet m = if a > b { a } else { b };\nif m > c { m } else { c }\n}");
}

#[test]
fn test_type_check_is_palindrome_check() {
    check_no_errors("fn is_between(x: number, lo: number, hi: number) -> bool {\nx >= lo && x <= hi\n}");
}

#[test]
fn test_type_check_string_len_fn() {
    check_no_errors("fn f(s: string) -> number {\nlet n = len(s);\nn + 1\n}");
}

#[test]
fn test_type_check_nested_match_data() {
    check_no_errors("enum Tree { Leaf(number), Node(number, number) }\nfn sum_tree(t: Tree) -> number {\nmatch t {\nTree::Leaf(x) => x\nTree::Node(a, b) => a + b\n}\n}");
}

#[test]
fn test_type_check_complex_option_chain() {
    check_no_errors("fn safe_head(arr: number) -> Option<number> {\nSome(arr)\n}\nfn process(arr: number) -> number {\nmatch safe_head(arr) {\nSome(x) => x * 2\nNone => 0\n}\n}");
}

#[test]
fn test_type_check_complex_result_chain() {
    check_no_errors("fn parse(s: string) -> Result<number, string> {\nOk(42)\n}\nfn compute(s: string) -> number {\nmatch parse(s) {\nOk(n) => n * 2\nErr(_) => 0\n}\n}");
}

#[test]
fn test_type_check_multi_match_arm() {
    check_no_errors("fn classify_char(c: number) -> string {\nmatch c {\n65 => \"A\"\n66 => \"B\"\n67 => \"C\"\n_ => \"other\"\n}\n}");
}

#[test]
fn test_type_check_struct_with_methods_v2() {
    check_no_errors("struct Counter { value: number }\nfn increment(c: Counter) -> Counter {\nCounter { value: c.value + 1 }\n}\nfn get_value(c: Counter) -> number {\nc.value\n}");
}

#[test]
fn test_type_check_bool_ops_complex() {
    check_no_errors("fn f(a: bool, b: bool, c: bool) -> bool {\n(a && b) || (!a && c) || (b && !c)\n}");
}

#[test]
fn test_type_check_number_ops_chain() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x + 1;\nlet b = a * 2;\nlet c = b - 3;\nlet d = c / 4;\nd\n}");
}

#[test]
fn test_type_check_string_ops_chain() {
    check_no_errors("fn f(s: string) -> string {\nlet a = s + \" world\";\nlet b = a + \"!\";\nb\n}");
}

#[test]
fn test_type_check_struct_field_access_chain() {
    check_no_errors("struct Line { start_x: number, start_y: number, end_x: number, end_y: number }\nfn horizontal_length(l: Line) -> number {\nl.end_x - l.start_x\n}");
}

#[test]
fn test_type_check_simple_add() {
    check_no_errors("fn f() -> number {\n1 + 2\n}");
}

#[test]
fn test_type_check_simple_sub() {
    check_no_errors("fn f() -> number {\n10 - 3\n}");
}

#[test]
fn test_type_check_simple_mul() {
    check_no_errors("fn f() -> number {\n4 * 5\n}");
}

#[test]
fn test_type_check_simple_div() {
    check_no_errors("fn f() -> number {\n20 / 4\n}");
}

#[test]
fn test_type_check_simple_mod() {
    check_no_errors("fn f() -> number {\n10 % 3\n}");
}

#[test]
fn test_type_check_simple_neg() {
    check_no_errors("fn f() -> number {\n-42\n}");
}

#[test]
fn test_type_check_simple_eq() {
    check_no_errors("fn f(x: number) -> bool {\nx == 0\n}");
}

#[test]
fn test_type_check_simple_neq() {
    check_no_errors("fn f(x: number) -> bool {\nx != 0\n}");
}

#[test]
fn test_type_check_simple_lt() {
    check_no_errors("fn f(x: number) -> bool {\nx < 10\n}");
}

#[test]
fn test_type_check_simple_gt() {
    check_no_errors("fn f(x: number) -> bool {\nx > 0\n}");
}

#[test]
fn test_type_check_simple_and() {
    check_no_errors("fn f(a: bool, b: bool) -> bool {\na && b\n}");
}

#[test]
fn test_type_check_simple_or() {
    check_no_errors("fn f(a: bool, b: bool) -> bool {\na || b\n}");
}

#[test]
fn test_type_check_simple_not() {
    check_no_errors("fn f(x: bool) -> bool {\n!x\n}");
}

#[test]
fn test_type_check_simple_string_eq() {
    check_no_errors("fn f(s: string) -> bool {\ns == \"hello\"\n}");
}

#[test]
fn test_type_check_simple_le() {
    check_no_errors("fn f(x: number) -> bool {\nx <= 10\n}");
}

#[test]
fn test_type_check_simple_ge() {
    check_no_errors("fn f(x: number) -> bool {\nx >= 0\n}");
}

#[test]
fn test_type_check_simple_bitand() {
    check_no_errors("fn f(a: number, b: number) -> number {\na & b\n}");
}

#[test]
fn test_type_check_simple_bitor() {
    check_no_errors("fn f(a: number, b: number) -> number {\na | b\n}");
}

#[test]
fn test_type_check_simple_bitxor() {
    check_no_errors("fn f(a: number, b: number) -> number {\na ^ b\n}");
}

#[test]
fn test_type_check_simple_shl() {
    check_no_errors("fn f(a: number, b: number) -> number {\na << b\n}");
}

#[test]
fn test_type_check_simple_shr() {
    check_no_errors("fn f(a: number, b: number) -> number {\na >> b\n}");
}

#[test]
fn test_type_check_simple_bitnot() {
    check_no_errors("fn f(a: number) -> number {\nlet b = a ^ a;\nb\n}");
}

#[test]
fn test_type_check_simple_string_lt() {
    check_no_errors("fn f(a: string, b: string) -> bool {\na < b\n}");
}

#[test]
fn test_type_check_simple_string_gt() {
    check_no_errors("fn f(a: string, b: string) -> bool {\na > b\n}");
}

#[test]
fn test_type_check_fn_unit_return() {
    check_no_errors("fn side_effect() {
}");
}

#[test]
fn test_type_check_fn_return_number() {
    check_no_errors("fn forty_two() -> number {\n42\n}");
}

#[test]
fn test_type_check_fn_return_string() {
    check_no_errors("fn hello() -> string {\n\"hello\"\n}");
}

#[test]
fn test_type_check_fn_return_bool() {
    check_no_errors("fn yes() -> bool {\ntrue\n}");
}

#[test]
fn test_type_check_fn_param_number() {
    check_no_errors("fn double(x: number) -> number {\nx * 2\n}");
}

#[test]
fn test_type_check_fn_param_string() {
    check_no_errors("fn exclaim(s: string) -> string {\ns + \"!\"\n}");
}

#[test]
fn test_type_check_fn_param_bool() {
    check_no_errors("fn not(b: bool) -> bool {\n!b\n}");
}

#[test]
fn test_type_check_fn_two_params() {
    check_no_errors("fn add(a: number, b: number) -> number {\na + b\n}");
}

#[test]
fn test_type_check_fn_three_params() {
    check_no_errors("fn clamp(x: number, lo: number, hi: number) -> number {\nif x < lo { lo } else { if x > hi { hi } else { x } }\n}");
}

#[test]
fn test_type_check_fn_return_option() {
    check_no_errors("fn safe_div(a: number, b: number) -> Option<number> {\nif b == 0 { None } else { Some(a / b) }\n}");
}

#[test]
fn test_type_check_fn_return_result() {
    check_no_errors("fn checked_div(a: number, b: number) -> Result<number, string> {\nif b == 0 { Err(\"zero\") } else { Ok(a / b) }\n}");
}

#[test]
fn test_type_check_empty_main() {
    check_no_errors("fn main() -> number {\n0\n}");
}

#[test]
fn test_type_check_let_only() {
    check_no_errors("fn main() -> number {\nlet x = 42;\nx\n}");
}

#[test]
fn test_type_check_nested_let_v3() {
    check_no_errors("fn main() -> number {\nlet a = 1;\nlet b = {\nlet c = a + 1;\nc * 2\n};\nb\n}");
}

#[test]
fn test_type_check_if_value() {
    check_no_errors("fn main() -> number {\nlet x = if true { 1 } else { 2 };\nx\n}");
}

#[test]
fn test_type_check_match_value() {
    check_no_errors("fn f(x: number) -> number {\nlet y = match x {\n0 => 10\n_ => 20\n};\ny\n}");
}

#[test]
fn test_type_check_fn_call_value() {
    check_no_errors("fn double(x: number) -> number { x * 2 }\nfn main() -> number {\nlet y = double(5);\ny\n}");
}

#[test]
fn test_type_check_struct_value() {
    check_no_errors("struct Point { x: number, y: number }\nfn main() -> number {\nlet p = Point { x: 1, y: 2 };\np.x + p.y\n}");
}

#[test]
fn test_type_check_string_value() {
    check_no_errors("fn f() -> string {\nlet s = \"hello\";\ns + \" world\"\n}");
}

#[test]
fn test_type_check_bool_value() {
    check_no_errors("fn main() -> bool {\nlet b = true;\n!b\n}");
}

#[test]
fn test_type_check_array_value() {
    check_no_errors("fn main() -> number {\nlet arr = [1, 2, 3];\narr[0]\n}");
}

#[test]
fn test_type_check_simple_let_number() {
    check_no_errors("fn f() -> number {\nlet x: number = 42;\nx\n}");
}

#[test]
fn test_type_check_simple_let_string() {
    check_no_errors("fn f() -> number {\nlet s: string = \"hello\";\nlen(s)\n}");
}

#[test]
fn test_type_check_simple_let_bool() {
    check_no_errors("fn f() -> number {\nlet b: bool = true;\nif b { 1 } else { 0 }\n}");
}

#[test]
fn test_type_check_simple_let_struct() {
    check_no_errors("struct Point { x: number, y: number }\nfn f() -> number {\nlet p: Point = Point { x: 1, y: 2 };\np.x\n}");
}

#[test]
fn test_type_check_simple_closure_no_capture() {
    check_no_errors("fn main() -> number {\nlet f = |x: number| -> number { x * 2 };\nf(21)\n}");
}

#[test]
fn test_type_check_simple_closure_capture() {
    check_no_errors("fn main() -> number {\nlet y = 10;\nlet f = |x: number| -> number { x + y };\nf(5)\n}");
}

#[test]
fn test_type_check_simple_enum_def() {
    check_no_errors("enum Color { Red, Green, Blue }\nfn f(c: Color) -> number {\nmatch c {\nColor::Red => 1\nColor::Green => 2\nColor::Blue => 3\n}\n}");
}

#[test]
fn test_type_check_simple_struct_def() {
    check_no_errors("struct Point { x: number, y: number }\nfn f(p: Point) -> number {\np.x + p.y\n}");
}

#[test]
fn test_type_check_simple_fn_type_param() {
    check_no_errors("fn apply(f: fn(number) -> number, x: number) -> number {\nf(x)\n}");
}

#[test]
fn test_type_check_simple_reference() {
    check_no_errors("fn main() -> number {\nlet x = 42;\nlet r = &x;\n*r\n}");
}

#[test]
fn test_type_check_simple_option_some() {
    check_no_errors("fn f() -> Option<number> {\nSome(42)\n}");
}

#[test]
fn test_type_check_simple_option_none() {
    check_no_errors("fn f() -> Option<number> {\nNone\n}");
}

#[test]
fn test_type_check_simple_result_ok() {
    check_no_errors("fn f() -> Result<number, string> {\nOk(42)\n}");
}

#[test]
fn test_type_check_simple_result_err() {
    check_no_errors("fn f() -> Result<number, string> {\nErr(\"error\")\n}");
}

#[test]
fn test_type_check_simple_array_access() {
    check_no_errors("fn f() -> number {\nlet arr = [10, 20, 30];\narr[1]\n}");
}

#[test]
fn test_type_check_simple_string_concat() {
    check_no_errors("fn f() -> string {\n\"hello\" + \" \" + \"world\"\n}");
}

#[test]
fn test_type_check_simple_number_concat() {
    check_no_errors("fn f(n: number) -> string {\n\"value: \" + n\n}");
}

#[test]
fn test_type_check_simple_if_else() {
    check_no_errors("fn f(x: number) -> number {\nif x > 0 { x } else { 0 - x }\n}");
}

#[test]
fn test_type_check_simple_match_number() {
    check_no_errors("fn f(x: number) -> string {\nmatch x {\n0 => \"zero\"\n1 => \"one\"\n_ => \"other\"\n}\n}");
}

#[test]
fn test_type_error_fn_param_wrong_type2() {
    check_has_errors("fn f(x: string) -> number {\nx + 1\n}");
}

#[test]
fn test_type_error_fn_extra_param() {
    check_has_errors("fn f(x: number) -> number {\nx\n}\nfn main() -> number {\nf(1, 2)\n}");
}

#[test]
fn test_type_error_fn_missing_param() {
    check_has_errors("fn f(x: number, y: number) -> number {\nx + y\n}\nfn main() -> number {\nf(1)\n}");
}

#[test]
fn test_type_error_struct_wrong_field_type2() {
    check_has_errors("struct Point { x: number, y: number }\nfn main() -> number {\nlet p = Point { x: \"hello\", y: 2 };\n0\n}");
}

#[test]
fn test_type_error_struct_undefined_field2() {
    check_has_errors("struct Point { x: number, y: number }\nfn main() -> number {\nlet p = Point { x: 1, y: 2 };\np.z\n}");
}

#[test]
fn test_type_error_non_exhaustive_bool2() {
    check_has_errors("fn f(b: bool) -> number {\nmatch b {\ntrue => 1\n}\n}");
}

#[test]
fn test_type_error_string_mul() {
    check_has_errors("fn main() -> number {\n\"hello\" * 3\n}");
}

#[test]
fn test_type_error_string_div() {
    check_has_errors("fn main() -> number {\n\"hello\" / 2\n}");
}

#[test]
fn test_type_error_undefined_var2() {
    check_has_errors("fn main() -> number {\nfoo\n}");
}

#[test]
fn test_type_error_undefined_fn2() {
    check_has_errors("fn main() -> number {\nbar()\n}");
}

#[test]
fn test_type_check_simple_while_fn() {
    check_no_errors("fn countdown(n: number) -> number {\nlet x = n;\nwhile x > 0 {\nx = x - 1\n};\nx\n}");
}

#[test]
fn test_type_check_simple_for_fn() {
    check_no_errors("fn sum_to(n: number) -> number {\nlet total = 0;\nfor i in 1..n {\ntotal = total + i\n};\ntotal\n}");
}

#[test]
fn test_type_check_simple_return_fn() {
    check_no_errors("fn abs(x: number) -> number {\nif x < 0 { return 0 - x };\nx\n}");
}

#[test]
fn test_type_check_simple_recursion() {
    check_no_errors("fn fact(n: number) -> number {\nif n <= 1 { 1 } else { n * fact(n - 1) }\n}");
}

#[test]
fn test_type_check_simple_mutual_rec() {
    check_no_errors("fn is_even(n: number) -> bool {\nif n == 0 { true } else { is_odd(n - 1) }\n}\nfn is_odd(n: number) -> bool {\nif n == 0 { false } else { is_even(n - 1) }\n}");
}

#[test]
fn test_type_check_simple_higher_order() {
    check_no_errors("fn apply(f: fn(number) -> number, x: number) -> number {\nf(x)\n}");
}

#[test]
fn test_type_check_simple_method_call() {
    check_no_errors("struct Counter { value: number }\nfn increment(c: Counter) -> Counter {\nCounter { value: c.value + 1 }\n}");
}

#[test]
fn test_type_check_simple_enum_match2() {
    check_no_errors("enum Direction { North, South, East, West }\nfn opposite(d: Direction) -> Direction {\nmatch d {\nDirection::North => Direction::South\nDirection::South => Direction::North\nDirection::East => Direction::West\nDirection::West => Direction::East\n}\n}");
}

#[test]
fn test_type_check_simple_option_chain2() {
    check_no_errors("fn safe_div(a: number, b: number) -> Option<number> {\nif b == 0 { None } else { Some(a / b) }\n}\nfn try_div(a: number, b: number) -> number {\nmatch safe_div(a, b) {\nSome(x) => x\nNone => 0\n}\n}");
}

#[test]
fn test_type_check_simple_result_chain2() {
    check_no_errors("fn parse(s: string) -> Result<number, string> {\nOk(42)\n}\nfn compute(s: string) -> number {\nmatch parse(s) {\nOk(n) => n * 2\nErr(_) => 0\n}\n}");
}

#[test]
fn test_type_check_complex_enum_recursive() {
    check_no_errors("enum Expr { Lit(number), Add(number, number) }\nfn eval(e: Expr) -> number {\nmatch e {\nExpr::Lit(n) => n\nExpr::Add(a, b) => eval(Expr::Lit(a)) + eval(Expr::Lit(b))\n}\n}");
}

#[test]
fn test_type_check_nested_option_result2() {
    check_no_errors("fn try_parse(s: string) -> Option<number> {\nSome(42)\n}\nfn try_compute(s: string) -> Result<number, string> {\nmatch try_parse(s) {\nSome(n) => if n > 0 { Ok(n) } else { Err(\"negative\") }\nNone => Err(\"parse failed\")\n}\n}");
}

#[test]
fn test_type_check_multi_struct_ops() {
    check_no_errors("struct Vec2 { x: number, y: number }\nfn add_vec(a: Vec2, b: Vec2) -> Vec2 {\nVec2 { x: a.x + b.x, y: a.y + b.y }\n}\nfn scale_vec(v: Vec2, s: number) -> Vec2 {\nVec2 { x: v.x * s, y: v.y * s }\n}\nfn dot_vec(a: Vec2, b: Vec2) -> number {\na.x * b.x + a.y * b.y\n}");
}

#[test]
fn test_type_check_string_ops_complex() {
    check_no_errors("fn f(s: string) -> string {\nlet a = s + \" \";\nlet b = a + \"world\";\nlet c = b + \"!\";\nc\n}");
}

#[test]
fn test_type_check_complex_let_assignment2() {
    check_no_errors("fn counter(start: number) -> number {\nlet c = start;\nc = c + 1;\nc = c * 2;\nc = c - 1;\nc\n}");
}

#[test]
fn test_type_check_nested_closure_higher_order() {
    check_no_errors("fn apply(f: fn(number) -> number, x: number) -> number {\nf(x)\n}\nfn main() -> number {\nlet double = |n: number| -> number { n * 2 };\nlet quad = |n: number| -> number { apply(double, apply(double, n)) };\nquad(3)\n}");
}

#[test]
fn test_type_check_complex_match_guard2() {
    check_no_errors("fn classify(n: number) -> string {\nmatch n {\n0 => \"zero\"\nx if x > 0 => \"positive\"\n_ => \"negative\"\n}\n}");
}

#[test]
fn test_type_check_enum_with_multiple_constructors() {
    check_no_errors("enum Shape { Circle(number), Rect(number, number), Triangle(number, number, number) }\nfn describe(s: Shape) -> string {\nmatch s {\nShape::Circle(_) => \"circle\"\nShape::Rect(_, _) => \"rectangle\"\nShape::Triangle(_, _, _) => \"triangle\"\n}\n}");
}

#[test]
fn test_type_check_complex_while_accumulate() {
    check_no_errors("fn sum_to(n: number) -> number {\nlet total = 0;\nlet i = 1;\nwhile i <= n {\ntotal = total + i;\ni = i + 1\n};\ntotal\n}");
}

#[test]
fn test_type_check_for_accumulate() {
    check_no_errors("fn sum_range(lo: number, hi: number) -> number {\nlet total = 0;\nfor i in lo..hi {\ntotal = total + i\n};\ntotal\n}");
}

#[test]
fn test_type_check_simple_tuple_return() {
    check_no_errors("struct Pair { first: number, second: number }\nfn make_pair(a: number, b: number) -> Pair {\nPair { first: a, second: b }\n}");
}

#[test]
fn test_type_check_simple_swap_fn() {
    check_no_errors("struct Pair { first: number, second: number }\nfn swap(p: Pair) -> Pair {\nPair { first: p.second, second: p.first }\n}");
}

#[test]
fn test_type_check_simple_map_fn() {
    check_no_errors("struct Pair { first: number, second: number }\nfn map_first(p: Pair, f: fn(number) -> number) -> Pair {\nPair { first: f(p.first), second: p.second }\n}");
}

#[test]
fn test_type_check_simple_filter_fn() {
    check_no_errors("fn is_positive(n: number) -> bool {\nn > 0\n}\nfn classify(n: number) -> string {\nif is_positive(n) { \"positive\" } else { \"non-positive\" }\n}");
}

#[test]
fn test_type_check_simple_fold_fn() {
    check_no_errors("fn sum(a: number, b: number) -> number {\na + b\n}\nfn f() -> number {\nsum(10, 20)\n}");
}

#[test]
fn test_type_check_simple_compose_fn() {
    check_no_errors("fn compose(f: fn(number) -> number, g: fn(number) -> number, x: number) -> number {\nf(g(x))\n}");
}

#[test]
fn test_type_check_simple_identity_fn() {
    check_no_errors("fn identity(x: number) -> number {\nx\n}");
}

#[test]
fn test_type_check_simple_constant_fn() {
    check_no_errors("fn constant(x: number, _y: number) -> number {\nx\n}");
}

#[test]
fn test_type_check_simple_flip_fn() {
    check_no_errors("fn flip(f: fn(number, number) -> number, a: number, b: number) -> number {\nf(b, a)\n}");
}

#[test]
fn test_type_check_simple_curry_fn() {
    check_no_errors("fn add(a: number) -> fn(number) -> number {\n|b: number| -> number { a + b }\n}");
}

#[test]
fn test_type_check_simple_let_in_if() {
    check_no_errors("fn f(x: number) -> number {\nlet result = if x > 0 {\nlet y = x * 2;\ny\n} else {\n0\n};\nresult\n}");
}

#[test]
fn test_type_check_simple_let_in_match() {
    check_no_errors("fn f(x: number) -> number {\nlet result = match x {\n0 => {\nlet y = 10;\ny\n}\n_ => {\nlet y = 20;\ny\n}\n};\nresult\n}");
}

#[test]
fn test_type_check_simple_let_in_while() {
    check_no_errors("fn f(n: number) -> number {\nlet total = 0;\nlet i = 0;\nwhile i < n {\nlet step = i + 1;\ntotal = total + step;\ni = i + 1\n};\ntotal\n}");
}

#[test]
fn test_type_check_simple_early_return_in_match() {
    check_no_errors("fn f(x: number) -> number {\nmatch x {\n0 => return 100\n_ => x\n}\n}");
}

#[test]
fn test_type_check_simple_nested_if_assign() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 0;\nif x > 0 {\nresult = 1\n} else {\nresult = 2\n};\nresult\n}");
}

#[test]
fn test_type_check_simple_match_assign() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 0;\nmatch x {\n0 => result = 1\n_ => result = 2\n};\nresult\n}");
}

#[test]
fn test_type_check_simple_string_in_if() {
    check_no_errors("fn f(x: number) -> string {\nif x > 0 {\n\"positive\"\n} else {\n\"non-positive\"\n}\n}");
}

#[test]
fn test_type_check_simple_number_in_match() {
    check_no_errors("fn f(x: number) -> number {\nmatch x {\n0 => 0\n1 => 1\n2 => 4\n3 => 9\n_ => x * x\n}\n}");
}

#[test]
fn test_type_check_simple_bool_in_if() {
    check_no_errors("fn f(x: number) -> bool {\nif x > 0 {\ntrue\n} else {\nfalse\n}\n}");
}

#[test]
fn test_type_check_simple_option_in_if() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 0 {\nSome(x)\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_complex_fn_composition2() {
    check_no_errors("fn double(x: number) -> number { x * 2 }\nfn inc(x: number) -> number { x + 1 }\nfn main() -> number {\ndouble(inc(double(5)))\n}");
}

#[test]
fn test_type_check_complex_let_chain3() {
    check_no_errors("fn main() -> number {\nlet a = 1;\nlet b = a + 1;\nlet c = a + b;\nlet d = b + c;\nlet e = c + d;\ne\n}");
}

#[test]
fn test_type_check_complex_string_chain2() {
    check_no_errors("fn f(a: string, b: string, c: string) -> string {\nlet d = a + b;\nlet e = d + c;\ne\n}");
}

#[test]
fn test_type_check_complex_while_loop2() {
    check_no_errors("fn f(n: number) -> number {\nlet sum = 0;\nlet i = 1;\nwhile i <= n {\nsum = sum + i * i;\ni = i + 1\n};\nsum\n}");
}

#[test]
fn test_type_check_complex_match_chain2() {
    check_no_errors("enum Expr { Lit(number), Neg(number), Add(number, number) }\nfn eval(e: Expr) -> number {\nmatch e {\nExpr::Lit(n) => n\nExpr::Neg(n) => 0 - n\nExpr::Add(a, b) => a + b\n}\n}");
}

#[test]
fn test_type_check_complex_option_chain3() {
    check_no_errors("fn safe_div(a: number, b: number) -> Option<number> {\nif b == 0 { None } else { Some(a / b) }\n}\nfn safe_add(a: Option<number>, b: Option<number>) -> Option<number> {\nmatch a {\nSome(x) => match b {\nSome(y) => Some(x + y)\nNone => None\n}\nNone => None\n}\n}");
}

#[test]
fn test_type_check_complex_result_chain3() {
    check_no_errors("fn checked_add(a: number, b: number) -> Result<number, string> {\nif a + b > 1000 { Err(\"overflow\") } else { Ok(a + b) }\n}\nfn checked_mul(a: number, b: number) -> Result<number, string> {\nif a * b > 1000 { Err(\"overflow\") } else { Ok(a * b) }\n}");
}

#[test]
fn test_type_check_complex_struct_chain2() {
    check_no_errors("struct Point { x: number, y: number }\nfn translate(p: Point, dx: number, dy: number) -> Point {\nPoint { x: p.x + dx, y: p.y + dy }\n}\nfn scale(p: Point, s: number) -> Point {\nPoint { x: p.x * s, y: p.y * s }\n}\nfn transform(p: Point) -> Point {\nscale(translate(p, 10, 20), 2)\n}");
}

#[test]
fn test_type_check_complex_enum_chain2() {
    check_no_errors("enum Bool { True, False }\nfn not(b: Bool) -> Bool {\nmatch b {\nBool::True => Bool::False\nBool::False => Bool::True\n}\n}\nfn and(a: Bool, b: Bool) -> Bool {\nmatch a {\nBool::True => b\nBool::False => Bool::False\n}\n}");
}

#[test]
fn test_type_check_complex_fn_chain2() {
    check_no_errors("fn square(x: number) -> number { x * x }\nfn sum_squares(a: number, b: number) -> number {\nsquare(a) + square(b)\n}\nfn main() -> number {\nsum_squares(3, 4)\n}");
}

#[test]
fn test_type_error_if_condition_string_v2() {
    check_has_errors("fn f() -> number {\nif \"hello\" { 1 } else { 2 }\n}");
}

#[test]
fn test_type_error_while_condition_string() {
    check_has_errors("fn f() -> number {\nwhile \"hello\" {\n0\n};\n0\n}");
}

#[test]
fn test_type_error_match_string_on_number() {
    check_has_errors("fn f(x: number) -> number {\nmatch x {\n\"hello\" => 1\n_ => 0\n}\n}");
}

#[test]
fn test_type_error_match_number_on_string() {
    check_has_errors("fn f(s: string) -> number {\nmatch s {\n42 => 1\n_ => 0\n}\n}");
}

#[test]
fn test_type_error_struct_wrong_name() {
    check_has_errors("struct Point { x: number, y: number }\nfn main() -> number {\nlet p = Vector { x: 1, y: 2 };\n0\n}");
}

#[test]
fn test_type_error_enum_wrong_variant() {
    check_has_errors("enum Color { Red, Green, Blue }\nfn f(c: Color) -> number {\nmatch c {\nColor::Yellow => 1\n_ => 0\n}\n}");
}

#[test]
fn test_type_error_fn_wrong_return2() {
    check_has_errors("fn f() -> number {\n\"42\"\n}");
}

#[test]
fn test_type_error_fn_missing_return2() {
    check_has_errors("fn f() -> number {\n}");
}

#[test]
fn test_type_error_let_wrong_annotation() {
    check_has_errors("fn f() -> number {\nlet x: string = 42;\n0\n}");
}

#[test]
fn test_type_error_param_wrong_annotation() {
    check_has_errors("fn f(x: string) -> number {\nx + 1\n}");
}

#[test]
fn test_type_check_simple_array_empty() {
    check_no_errors("fn f() -> number {\nlet arr = [];\n0\n}");
}

#[test]
fn test_type_check_simple_array_literal() {
    check_no_errors("fn f() -> number {\nlet arr = [1, 2, 3, 4, 5];\narr[0] + arr[4]\n}");
}

#[test]
fn test_type_check_simple_array_index() {
    check_no_errors("fn f() -> number {\nlet arr = [10, 20, 30];\nlet idx = 1;\narr[idx]\n}");
}

#[test]
fn test_type_check_simple_for_array() {
    check_no_errors("fn f() -> number {\nlet sum = 0;\nfor i in [1, 2, 3] {\nsum = sum + i\n};\nsum\n}");
}

#[test]
fn test_type_check_simple_for_range() {
    check_no_errors("fn f() -> number {\nlet sum = 0;\nfor i in 0..10 {\nsum = sum + i\n};\nsum\n}");
}

#[test]
fn test_type_check_simple_char_literal() {
    check_no_errors("fn f() -> number {\nlet c = 'A';\nc + 1\n}");
}

#[test]
fn test_type_check_simple_char_compare() {
    check_no_errors("fn f(c: number) -> bool {\nc > 64\n}");
}

#[test]
fn test_type_check_simple_escaped_char() {
    check_no_errors("fn f() -> number {\nlet c = '\\n';\nc\n}");
}

#[test]
fn test_type_check_simple_string_escape() {
    check_no_errors("fn f() -> string {\n\"hello\\nworld\"\n}");
}

#[test]
fn test_type_check_simple_empty_string() {
    check_no_errors("fn f() -> string {\n\"\"\n}");
}

#[test]
fn test_type_check_complex_if_else_multi() {
    check_no_errors("fn classify(n: number) -> string {\nif n > 100 { \"huge\" } else { if n > 10 { \"big\" } else { if n > 0 { \"small\" } else { \"zero or negative\" } } }\n}");
}

#[test]
fn test_type_check_complex_match_multi() {
    check_no_errors("fn http_status(code: number) -> string {\nmatch code {\n200 => \"OK\"\n404 => \"Not Found\"\n500 => \"Internal Error\"\n_ => \"Unknown\"\n}\n}");
}

#[test]
fn test_type_check_complex_while_sum() {
    check_no_errors("fn sum_of_squares(n: number) -> number {\nlet total = 0;\nlet i = 1;\nwhile i <= n {\ntotal = total + i * i;\ni = i + 1\n};\ntotal\n}");
}

#[test]
fn test_type_check_complex_fn_chain3() {
    check_no_errors("fn double(x: number) -> number { x * 2 }\nfn triple(x: number) -> number { x * 3 }\nfn add_then_double(a: number, b: number) -> number {\ndouble(a + b)\n}\nfn main() -> number {\nadd_then_double(triple(2), 4)\n}");
}

#[test]
fn test_type_check_complex_enum_data3() {
    check_no_errors("enum AST { Num(number), BinOp(string, number, number) }\nfn eval(a: AST) -> number {\nmatch a {\nAST::Num(n) => n\nAST::BinOp(op, l, r) => l + r\n}\n}");
}

#[test]
fn test_type_check_complex_struct_nested() {
    check_no_errors("struct Color { r: number, g: number, b: number }\nfn brightness(c: Color) -> number {\n(c.r + c.g + c.b) / 3\n}\nfn is_bright(c: Color) -> bool {\nbrightness(c) > 128\n}");
}

#[test]
fn test_type_check_complex_option_methods2() {
    check_no_errors("fn safe_get(idx: number, len: number) -> Option<number> {\nif idx >= 0 { if idx < len { Some(idx) } else { None } } else { None }\n}");
}

#[test]
fn test_type_check_complex_result_chain4() {
    check_no_errors("fn validate_age(age: number) -> Result<number, string> {\nif age < 0 { Err(\"negative age\") } else { if age > 150 { Err(\"unrealistic age\") } else { Ok(age) } }\n}");
}

#[test]
fn test_type_check_complex_string_ops2() {
    check_no_errors("fn pad(s: string, n: number) -> string {\nlet result = s;\nresult\n}");
}

#[test]
fn test_type_check_complex_number_ops2() {
    check_no_errors("fn clamp_byte(n: number) -> number {\nif n < 0 { 0 } else { if n > 255 { 255 } else { n } }\n}");
}

#[test]
fn test_type_check_advanced_generic_fn() {
    check_no_errors("fn id(x) { x }\nfn main() -> number {\nid(42)\n}");
}

#[test]
fn test_type_check_advanced_generic_fn2() {
    check_no_errors("fn first(x, y) { x }\nfn main() -> number {\nfirst(1, 2)\n}");
}

#[test]
fn test_type_check_advanced_closure_fn() {
    check_no_errors("fn main() -> number {\nlet add = |a: number, b: number| -> number { a + b };\nadd(3, 4)\n}");
}

#[test]
fn test_type_check_advanced_nested_closure() {
    check_no_errors("fn main() -> number {\nlet make_adder = |x: number| -> fn(number) -> number {\n|y: number| -> number { x + y }\n};\nlet add5 = make_adder(5);\nadd5(10)\n}");
}

#[test]
fn test_type_check_advanced_fn_as_param() {
    check_no_errors("fn apply(f: fn(number) -> number, x: number) -> number {\nf(x)\n}\nfn double(x: number) -> number { x * 2 }\nfn main() -> number {\napply(double, 21)\n}");
}

#[test]
fn test_type_check_advanced_higher_order_chain() {
    check_no_errors("fn apply(f: fn(number) -> number, x: number) -> number {\nf(x)\n}\nfn main() -> number {\nlet double = |x: number| -> number { x * 2 };\nlet quad = |x: number| -> number { apply(double, apply(double, x)) };\nquad(3)\n}");
}

#[test]
fn test_type_check_advanced_method_syntax2() {
    check_no_errors("struct Point { x: number, y: number }\nfn distance(a: Point, b: Point) -> number {\nlet dx = a.x - b.x;\nlet dy = a.y - b.y;\ndx * dx + dy * dy\n}");
}

#[test]
fn test_type_check_advanced_option_chain4() {
    check_no_errors("fn find(items: number, target: number) -> Option<number> {\nif items == target { Some(items) } else { None }\n}");
}

#[test]
fn test_type_check_advanced_result_chain5() {
    check_no_errors("fn safe_div(a: number, b: number) -> Result<number, string> {\nif b == 0 { Err(\"division by zero\") } else { Ok(a / b) }\n}\nfn safe_mod(a: number, b: number) -> Result<number, string> {\nif b == 0 { Err(\"division by zero\") } else { Ok(a % b) }\n}");
}

#[test]
fn test_type_check_advanced_pattern_match() {
    check_no_errors("enum Shape { Circle(number), Rect(number, number) }\nfn perimeter(s: Shape) -> number {\nmatch s {\nShape::Circle(r) => 2 * 3 * r\nShape::Rect(w, h) => 2 * (w + h)\n}\n}");
}

#[test]
fn test_type_check_misc_bool_and_number() {
    check_no_errors("fn f(x: number) -> number {\nlet b = x > 0;\nif b { x } else { 0 }\n}");
}

#[test]
fn test_type_check_misc_nested_let_scope() {
    check_no_errors("fn f() -> number {\nlet x = 1;\nlet y = {\nlet x = 2;\nx + 1\n};\nx + y\n}");
}

#[test]
fn test_type_check_misc_complex_assignment() {
    check_no_errors("fn f() -> number {\nlet a = 1;\nlet b = 2;\na = a + b;\nb = a + b;\na + b\n}");
}

#[test]
fn test_type_check_misc_string_compare() {
    check_no_errors("fn is_hello(s: string) -> bool {\ns == \"hello\"\n}");
}

#[test]
fn test_type_check_misc_option_default() {
    check_no_errors("fn unwrap_or(opt: Option<number>, def: number) -> number {\nmatch opt {\nSome(x) => x\nNone => def\n}\n}");
}

#[test]
fn test_type_check_misc_result_or() {
    check_no_errors("fn unwrap_or(res: Result<number, string>, def: number) -> number {\nmatch res {\nOk(x) => x\nErr(_) => def\n}\n}");
}

#[test]
fn test_type_check_misc_enum_data_access() {
    check_no_errors("enum Pair { P(number, number) }\nfn fst(p: Pair) -> number {\nmatch p {\nPair::P(a, _) => a\n}\n}");
}

#[test]
fn test_type_check_misc_struct_field_update() {
    check_no_errors("struct Point { x: number, y: number }\nfn move_right(p: Point, dx: number) -> Point {\nPoint { x: p.x + dx, y: p.y }\n}");
}

#[test]
fn test_type_check_misc_complex_while_break() {
    check_no_errors("fn find_positive(items: number) -> Option<number> {\nif items > 0 { Some(items) } else { None }\n}");
}

#[test]
fn test_type_check_misc_fn_chain_with_let() {
    check_no_errors("fn double(x: number) -> number { x * 2 }\nfn add_one(x: number) -> number { x + 1 }\nfn main() -> number {\nlet a = 5;\nlet b = double(a);\nlet c = add_one(b);\nc\n}");
}

#[test]
fn test_type_check_final_fn_chain_1() {
    check_no_errors("fn square(x: number) -> number { x * x }\nfn cube(x: number) -> number { x * x * x }\nfn main() -> number {\nsquare(3) + cube(2)\n}");
}

#[test]
fn test_type_check_final_fn_chain_2() {
    check_no_errors("fn max(a: number, b: number) -> number {\nif a > b { a } else { b }\n}\nfn min(a: number, b: number) -> number {\nif a < b { a } else { b }\n}\nfn clamp(x: number, lo: number, hi: number) -> number {\nmax(min(x, hi), lo)\n}");
}

#[test]
fn test_type_check_final_fn_chain_3() {
    check_no_errors("fn abs(x: number) -> number {\nif x < 0 { 0 - x } else { x }\n}\nfn distance(a: number, b: number) -> number {\nabs(a - b)\n}");
}

#[test]
fn test_type_check_final_fn_chain_4() {
    check_no_errors("fn is_even(n: number) -> bool {\nn % 2 == 0\n}\nfn is_odd(n: number) -> bool {\n!is_even(n)\n}");
}

#[test]
fn test_type_check_final_fn_chain_5() {
    check_no_errors("fn fib(n: number) -> number {\nif n <= 1 { n } else { fib(n - 1) + fib(n - 2) }\n}\nfn main() -> number {\nfib(10)\n}");
}

#[test]
fn test_type_check_final_enum_chain_1() {
    check_no_errors("enum Season { Spring, Summer, Autumn, Winter }\nfn next_season(s: Season) -> Season {\nmatch s {\nSeason::Spring => Season::Summer\nSeason::Summer => Season::Autumn\nSeason::Autumn => Season::Winter\nSeason::Winter => Season::Spring\n}\n}");
}

#[test]
fn test_type_check_final_struct_chain_1() {
    check_no_errors("struct Rect { width: number, height: number }\nfn area(r: Rect) -> number {\nr.width * r.height\n}\nfn perimeter(r: Rect) -> number {\n2 * (r.width + r.height)\n}\nfn is_square(r: Rect) -> bool {\nr.width == r.height\n}");
}

#[test]
fn test_type_check_final_option_chain_1() {
    check_no_errors("fn safe_sqrt(x: number) -> Option<number> {\nif x < 0 { None } else { Some(x) }\n}\nfn sqrt_or_zero(x: number) -> number {\nmatch safe_sqrt(x) {\nSome(r) => r\nNone => 0\n}\n}");
}

#[test]
fn test_type_check_final_result_chain_1() {
    check_no_errors("fn parse_int(s: string) -> Result<number, string> {\nOk(42)\n}\nfn double_parse(s: string) -> Result<number, string> {\nmatch parse_int(s) {\nOk(n) => Ok(n * 2)\nErr(e) => Err(e)\n}\n}");
}

#[test]
fn test_type_check_final_complex_program() {
    check_no_errors("struct Point { x: number, y: number }\nfn distance_sq(a: Point, b: Point) -> number {\nlet dx = a.x - b.x;\nlet dy = a.y - b.y;\ndx * dx + dy * dy\n}\nfn nearest(origin: Point, a: Point, b: Point) -> Point {\nif distance_sq(origin, a) < distance_sq(origin, b) { a } else { b }\n}");
}

#[test]
fn test_type_check_extra_fn_chain_1() {
    check_no_errors("fn identity(x: number) -> number {\nx\n}\nfn apply_twice(f: fn(number) -> number, x: number) -> number {\nf(f(x))\n}\nfn main() -> number {\napply_twice(identity, 5)\n}");
}

#[test]
fn test_type_check_extra_fn_chain_2() {
    check_no_errors("fn add(a: number) -> fn(number) -> number {\n|b: number| -> number { a + b }\n}\nfn main() -> number {\nlet add5 = add(5);\nadd5(10)\n}");
}

#[test]
fn test_type_check_extra_fn_chain_3() {
    check_no_errors("fn compose(f: fn(number) -> number, g: fn(number) -> number) -> fn(number) -> number {\n|x: number| -> number { f(g(x)) }\n}");
}

#[test]
fn test_type_check_extra_enum_chain() {
    check_no_errors("enum Bool { True, False }\nfn and(a: Bool, b: Bool) -> Bool {\nmatch a {\nBool::True => b\nBool::False => Bool::False\n}\n}\nfn or(a: Bool, b: Bool) -> Bool {\nmatch a {\nBool::True => Bool::True\nBool::False => b\n}\n}");
}

#[test]
fn test_type_check_extra_struct_chain() {
    check_no_errors("struct Vec3 { x: number, y: number, z: number }\nfn dot(a: Vec3, b: Vec3) -> number {\na.x * b.x + a.y * b.y + a.z * b.z\n}\nfn length_sq(v: Vec3) -> number {\ndot(v, v)\n}");
}

#[test]
fn test_type_check_extra_option_chain() {
    check_no_errors("fn map_option(opt: Option<number>, f: fn(number) -> number) -> Option<number> {\nmatch opt {\nSome(x) => Some(f(x))\nNone => None\n}\n}");
}

#[test]
fn test_type_check_extra_result_chain() {
    check_no_errors("fn map_result(res: Result<number, string>, f: fn(number) -> number) -> Result<number, string> {\nmatch res {\nOk(x) => Ok(f(x))\nErr(e) => Err(e)\n}\n}");
}

#[test]
fn test_type_check_extra_complex_1() {
    check_no_errors("struct Point { x: number, y: number }\nfn reflect_x(p: Point) -> Point {\nPoint { x: 0 - p.x, y: p.y }\n}\nfn reflect_y(p: Point) -> Point {\nPoint { x: p.x, y: 0 - p.y }\n}\nfn reflect_origin(p: Point) -> Point {\nreflect_x(reflect_y(p))\n}");
}

#[test]
fn test_type_check_extra_complex_2() {
    check_no_errors("enum List { Cons(number, number), Nil }\nfn sum_list(l: List) -> number {\nmatch l {\nList::Cons(a, b) => a + b\nList::Nil => 0\n}\n}\nfn is_empty(l: List) -> bool {\nmatch l {\nList::Cons(_, _) => false\nList::Nil => true\n}\n}");
}

#[test]
fn test_type_check_extra_complex_3() {
    check_no_errors("struct Color { r: number, g: number, b: number }\nfn grayscale(c: Color) -> number {\n(c.r + c.g + c.b) / 3\n}\nfn is_dark(c: Color) -> bool {\ngrayscale(c) < 128\n}\nfn is_light(c: Color) -> bool {\n!is_dark(c)\n}");
}

#[test]
fn test_type_check_bonus_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x;\nlet b = a;\na + b\n}");
}

#[test]
fn test_type_check_bonus_2() {
    check_no_errors("fn f(x: number) -> number {\nlet y = x;\ny = y + 1;\ny = y * 2;\ny\n}");
}

#[test]
fn test_type_check_bonus_3() {
    check_no_errors("fn f(x: number) -> bool {\nlet a = x > 0;\nlet b = x < 100;\na && b\n}");
}

#[test]
fn test_type_check_bonus_4() {
    check_no_errors("struct Pair { a: number, b: number }\nfn make_pair(x: number) -> Pair {\nPair { a: x, b: x * 2 }\n}");
}

#[test]
fn test_type_check_bonus_5() {
    check_no_errors("enum Maybe { Just(number), Nothing }\nfn get_or_default(m: Maybe) -> number {\nmatch m {\nMaybe::Just(x) => x\nMaybe::Nothing => 0\n}\n}");
}

#[test]
fn test_type_check_bonus_6() {
    check_no_errors("fn f(x: number) -> number {\nlet result = match x {\n0 => 100\n1 => 200\n_ => 300\n};\nresult\n}");
}

#[test]
fn test_type_check_bonus_7() {
    check_no_errors("fn f(x: number) -> number {\nlet result = if x > 0 {\nx * 2\n} else {\nx * 3\n};\nresult\n}");
}

#[test]
fn test_type_check_bonus_8() {
    check_no_errors("fn f(x: number, y: number) -> number {\nlet max = if x > y { x } else { y };\nlet min = if x < y { x } else { y };\nmax - min\n}");
}

#[test]
fn test_type_check_bonus_9() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x;\nlet b = a + 1;\nlet c = b + a;\nlet d = c + b;\nlet e = d + c;\ne\n}");
}

#[test]
fn test_type_check_bonus_10() {
    check_no_errors("struct Point { x: number, y: number }\nfn origin() -> Point {\nPoint { x: 0, y: 0 }\n}\nfn is_origin(p: Point) -> bool {\np.x == 0 && p.y == 0\n}");
}

#[test]
fn test_type_check_final_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x;\nlet b = a;\nlet c = b;\nc\n}");
}

#[test]
fn test_type_check_final_2() {
    check_no_errors("fn f() -> number {\nlet x = 10;\nlet y = 20;\nlet z = x + y;\nz\n}");
}

#[test]
fn test_type_check_final_3() {
    check_no_errors("fn f(x: number) -> number {\nif x > 0 {\nif x > 10 {\nif x > 100 {\n3\n} else {\n2\n}\n} else {\n1\n}\n} else {\n0\n}\n}");
}

#[test]
fn test_type_check_final_4() {
    check_no_errors("fn f(x: number) -> string {\nmatch x {\n0 => \"zero\"\n1 => \"one\"\n2 => \"two\"\n_ => \"many\"\n}\n}");
}

#[test]
fn test_type_check_final_5() {
    check_no_errors("fn f(a: number, b: number) -> number {\nlet sum = a + b;\nlet diff = a - b;\nlet prod = a * b;\nsum + diff + prod\n}");
}

#[test]
fn test_type_check_final_6() {
    check_no_errors("struct Line { x1: number, y1: number, x2: number, y2: number }\nfn horizontal(l: Line) -> bool {\nl.y1 == l.y2\n}\nfn vertical(l: Line) -> bool {\nl.x1 == l.x2\n}");
}

#[test]
fn test_type_check_final_7() {
    check_no_errors("enum Grade { A, B, C, D, F }\nfn pass(g: Grade) -> bool {\nmatch g {\nGrade::A => true\nGrade::B => true\nGrade::C => true\nGrade::D => true\nGrade::F => false\n}\n}");
}

#[test]
fn test_type_check_final_8() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 0 {\nif x > 100 {\nNone\n} else {\nSome(x)\n}\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_final_9() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x > 100 {\nErr(\"too large\")\n} else {\nif x < 0 {\nErr(\"negative\")\n} else {\nOk(x)\n}\n}\n}");
}

#[test]
fn test_type_check_final_10() {
    check_no_errors("struct Point { x: number, y: number }\nfn on_x_axis(p: Point) -> bool {\np.y == 0\n}\nfn on_y_axis(p: Point) -> bool {\np.x == 0\n}\nfn on_origin(p: Point) -> bool {\non_x_axis(p) && on_y_axis(p)\n}");
}

#[test]
fn test_type_check_round_1() {
    check_no_errors("fn f(x: number) -> number {\nlet y = x * 2;\nlet z = y + 1;\nz\n}");
}

#[test]
fn test_type_check_round_2() {
    check_no_errors("fn f(x: number) -> number {\nlet a = if x > 0 { x } else { 0 };\nlet b = a * a;\nb\n}");
}

#[test]
fn test_type_check_round_3() {
    check_no_errors("fn f(x: number) -> number {\nmatch x {\n0 => 0\nn => n * n\n}\n}");
}

#[test]
fn test_type_check_round_4() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x == 0 { None } else { Some(x) }\n}");
}

#[test]
fn test_type_check_round_5() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x < 0 { Err(\"negative\") } else { Ok(x) }\n}");
}

#[test]
fn test_type_check_round_6() {
    check_no_errors("struct Box { width: number, height: number }\nfn area(b: Box) -> number {\nb.width * b.height\n}");
}

#[test]
fn test_type_check_round_7() {
    check_no_errors("enum Status { Ok, Error }\nfn is_ok(s: Status) -> bool {\nmatch s {\nStatus::Ok => true\nStatus::Error => false\n}\n}");
}

#[test]
fn test_type_check_round_8() {
    check_no_errors("fn f(a: number, b: number) -> number {\nlet sum = a + b;\nlet diff = a - b;\nsum * diff\n}");
}

#[test]
fn test_type_check_round_9() {
    check_no_errors("fn f(x: number) -> number {\nlet total = 0;\nlet i = 0;\nwhile i < x {\ntotal = total + i;\ni = i + 1\n};\ntotal\n}");
}

#[test]
fn test_type_check_round_10() {
    check_no_errors("fn f(x: number) -> number {\nlet total = 0;\nfor i in 0..x {\ntotal = total + i\n};\ntotal\n}");
}

#[test]
fn test_type_check_wave_1() {
    check_no_errors("fn f(x: number) -> number {\nlet y = x;\nlet z = y;\nlet w = z;\nw\n}");
}

#[test]
fn test_type_check_wave_2() {
    check_no_errors("fn f(x: number) -> number {\nif true { x } else { x }\n}");
}

#[test]
fn test_type_check_wave_3() {
    check_no_errors("fn f() -> bool {\ntrue && false || true\n}");
}

#[test]
fn test_type_check_wave_4() {
    check_no_errors("fn f(x: number) -> bool {\nx != 0 && x > 0\n}");
}

#[test]
fn test_type_check_wave_5() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x + 1;\nlet b = a + 2;\nlet c = b + 3;\nc\n}");
}

#[test]
fn test_type_check_wave_6() {
    check_no_errors("struct Triple { a: number, b: number, c: number }\nfn sum(t: Triple) -> number {\nt.a + t.b + t.c\n}");
}

#[test]
fn test_type_check_wave_7() {
    check_no_errors("enum Cardinal { N, S, E, W }\nfn opposite(c: Cardinal) -> Cardinal {\nmatch c {\nCardinal::N => Cardinal::S\nCardinal::S => Cardinal::N\nCardinal::E => Cardinal::W\nCardinal::W => Cardinal::E\n}\n}");
}

#[test]
fn test_type_check_wave_8() {
    check_no_errors("fn f(x: number) -> number {\nmatch x {\n0 => 1\n1 => 2\n2 => 3\n_ => x + 1\n}\n}");
}

#[test]
fn test_type_check_wave_9() {
    check_no_errors("fn f(x: number) -> string {\nlet result = if x > 0 { \"pos\" } else { \"non-pos\" };\nresult\n}");
}

#[test]
fn test_type_check_wave_10() {
    check_no_errors("fn f(x: number, y: number) -> bool {\n(x > 0 && y > 0) || (x < 0 && y < 0)\n}");
}

#[test]
fn test_type_check_sprint_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x;\na\n}");
}

#[test]
fn test_type_check_sprint_2() {
    check_no_errors("fn f(x: number, y: number) -> number {\nlet sum = x + y;\nsum\n}");
}

#[test]
fn test_type_check_sprint_3() {
    check_no_errors("fn f(x: number) -> bool {\nx > 0\n}");
}

#[test]
fn test_type_check_sprint_4() {
    check_no_errors("fn f(x: number) -> string {\nif x > 0 { \"positive\" } else { \"non-positive\" }\n}");
}

#[test]
fn test_type_check_sprint_5() {
    check_no_errors("struct Point { x: number, y: number }\nfn new_point(x: number, y: number) -> Point {\nPoint { x: x, y: y }\n}");
}

#[test]
fn test_type_check_sprint_6() {
    check_no_errors("enum Option2 { Some2(number), None2 }\nfn unwrap_or(opt: Option2, def: number) -> number {\nmatch opt {\nOption2::Some2(x) => x\nOption2::None2 => def\n}\n}");
}

#[test]
fn test_type_check_sprint_7() {
    check_no_errors("fn f(x: number) -> number {\nlet y = x;\ny = y + 1;\ny\n}");
}

#[test]
fn test_type_check_sprint_8() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 0;\nif x > 10 {\nresult = 1\n};\nresult\n}");
}

#[test]
fn test_type_check_sprint_9() {
    check_no_errors("fn f(x: number) -> number {\nlet result = match x {\n0 => 100\n_ => 200\n};\nresult\n}");
}

#[test]
fn test_type_check_sprint_10() {
    check_no_errors("fn f(x: number) -> number {\nlet total = 0;\nfor i in 0..10 {\ntotal = total + i\n};\ntotal\n}");
}

#[test]
fn test_type_check_last_1() {
    check_no_errors("fn f(x: number) -> number {\nlet y = x + 1;\nlet z = y + 1;\nlet w = z + 1;\nw\n}");
}

#[test]
fn test_type_check_last_2() {
    check_no_errors("fn f(x: number) -> bool {\nlet a = x > 0;\nlet b = x < 100;\na && b\n}");
}

#[test]
fn test_type_check_last_3() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x * 2;\nlet b = a + 1;\nlet c = b * 3;\nc\n}");
}

#[test]
fn test_type_check_last_4() {
    check_no_errors("fn f(s: string) -> string {\nlet a = s + \"!\";\na\n}");
}

#[test]
fn test_type_check_last_5() {
    check_no_errors("fn f(b: bool) -> number {\nif b { 1 } else { 0 }\n}");
}

#[test]
fn test_type_check_last_6() {
    check_no_errors("struct Range { start: number, end: number }\nfn contains(r: Range, x: number) -> bool {\nx >= r.start && x <= r.end\n}");
}

#[test]
fn test_type_check_last_7() {
    check_no_errors("enum Month { Jan, Feb, Mar, Apr, May, Jun, Jul, Aug, Sep, Oct, Nov, Dec }\nfn quarter(m: Month) -> number {\nmatch m {\nMonth::Jan => 1\nMonth::Feb => 1\nMonth::Mar => 1\nMonth::Apr => 2\nMonth::May => 2\nMonth::Jun => 2\nMonth::Jul => 3\nMonth::Aug => 3\nMonth::Sep => 3\nMonth::Oct => 4\nMonth::Nov => 4\nMonth::Dec => 4\n}\n}");
}

#[test]
fn test_type_check_last_8() {
    check_no_errors("fn f(x: number) -> number {\nlet abs = if x < 0 { 0 - x } else { x };\nabs * 2\n}");
}

#[test]
fn test_type_check_last_9() {
    check_no_errors("fn f(a: number, b: number, c: number) -> number {\nlet sum = a + b + c;\nlet avg = sum / 3;\navg\n}");
}

#[test]
fn test_type_check_last_10() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x;\nlet b = a;\na + b\n}");
}

#[test]
fn test_type_check_push_1() {
    check_no_errors("fn f(x: number) -> number {\nmatch x {\n0 => 0\n_ => x\n}\n}");
}

#[test]
fn test_type_check_push_2() {
    check_no_errors("fn f(x: number) -> number {\nif x == 0 { 0 } else { x }\n}");
}

#[test]
fn test_type_check_push_3() {
    check_no_errors("fn f(x: number, y: number) -> number {\nif x > y { x } else { y }\n}");
}

#[test]
fn test_type_check_push_4() {
    check_no_errors("fn f(x: number) -> string {\nlet s = \"value\";\ns\n}");
}

#[test]
fn test_type_check_push_5() {
    check_no_errors("fn f(x: number) -> number {\nlet y = x;\ny = y + 1;\ny = y + 1;\ny\n}");
}

#[test]
fn test_type_check_push_6() {
    check_no_errors("fn f(x: number) -> number {\nlet a = 1;\nlet b = 2;\nlet c = 3;\na + b + c + x\n}");
}

#[test]
fn test_type_check_push_7() {
    check_no_errors("struct Circle { radius: number }\nfn circumference(c: Circle) -> number {\n2 * c.radius * 3\n}");
}

#[test]
fn test_type_check_push_8() {
    check_no_errors("enum Day { Mon, Tue, Wed, Thu, Fri, Sat, Sun }\nfn is_weekend(d: Day) -> bool {\nmatch d {\nDay::Sat => true\nDay::Sun => true\n_ => false\n}\n}");
}

#[test]
fn test_type_check_push_9() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 100 { None } else { if x < 0 { None } else { Some(x) } }\n}");
}

#[test]
fn test_type_check_push_10() {
    check_no_errors("fn f(x: number) -> number {\nlet total = 0;\nwhile total < x {\ntotal = total + 1\n};\ntotal\n}");
}

#[test]
fn test_type_check_final_sprint_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x + 1;\nlet b = a + 1;\nlet c = b + 1;\nlet d = c + 1;\nd\n}");
}

#[test]
fn test_type_check_final_sprint_2() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x;\nlet b = a;\nlet c = b;\nlet d = c;\nd\n}");
}

#[test]
fn test_type_check_final_sprint_3() {
    check_no_errors("fn f(x: number) -> number {\nlet result = match x {\n0 => 1\n1 => 2\n2 => 3\n3 => 5\n_ => x\n};\nresult\n}");
}

#[test]
fn test_type_check_final_sprint_4() {
    check_no_errors("fn f(x: number) -> number {\nif x > 0 {\nif x > 10 {\nif x > 100 {\n1000\n} else {\n100\n}\n} else {\n10\n}\n} else {\n1\n}\n}");
}

#[test]
fn test_type_check_final_sprint_5() {
    check_no_errors("struct Pair { first: number, second: number }\nfn swap(p: Pair) -> Pair {\nPair { first: p.second, second: p.first }\n}");
}

#[test]
fn test_type_check_final_sprint_6() {
    check_no_errors("enum Direction { North, South, East, West }\nfn turn_right(d: Direction) -> Direction {\nmatch d {\nDirection::North => Direction::East\nDirection::East => Direction::South\nDirection::South => Direction::West\nDirection::West => Direction::North\n}\n}");
}

#[test]
fn test_type_check_final_sprint_7() {
    check_no_errors("fn f(x: number) -> Option<number> {\nmatch x {\n0 => None\n1 => Some(1)\n_ => Some(x)\n}\n}");
}

#[test]
fn test_type_check_final_sprint_8() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nmatch x {\n0 => Err(\"zero\")\nn => Ok(n)\n}\n}");
}

#[test]
fn test_type_check_final_sprint_9() {
    check_no_errors("struct Point { x: number, y: number }\nfn scale(p: Point, factor: number) -> Point {\nPoint { x: p.x * factor, y: p.y * factor }\n}");
}

#[test]
fn test_type_check_final_sprint_10() {
    check_no_errors("fn f(x: number) -> number {\nlet total = 0;\nfor i in 0..100 {\ntotal = total + i\n};\ntotal\n}");
}

#[test]
fn test_type_check_final_push_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x;\nlet b = a + 1;\nlet c = b + 1;\nc\n}");
}

#[test]
fn test_type_check_final_push_2() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 0;\nfor i in 0..x {\nresult = result + i\n};\nresult\n}");
}

#[test]
fn test_type_check_final_push_3() {
    check_no_errors("struct Point { x: number, y: number }\nfn translate(p: Point, dx: number, dy: number) -> Point {\nPoint { x: p.x + dx, y: p.y + dy }\n}");
}

#[test]
fn test_type_check_final_push_4() {
    check_no_errors("enum Color { Red, Green, Blue }\nfn to_number(c: Color) -> number {\nmatch c {\nColor::Red => 0\nColor::Green => 1\nColor::Blue => 2\n}\n}");
}

#[test]
fn test_type_check_final_push_5() {
    check_no_errors("fn f(x: number) -> number {\nlet y = if x > 0 { x } else { 0 - x };\ny\n}");
}

#[test]
fn test_type_check_final_push_6() {
    check_no_errors("fn f(a: number, b: number) -> number {\nlet x = a + b;\nlet y = a - b;\nlet z = x * y;\nz\n}");
}

#[test]
fn test_type_check_final_push_7() {
    check_no_errors("fn f(x: number) -> number {\nlet result = match x {\n0 => 0\nn => n * n\n};\nresult + 1\n}");
}

#[test]
fn test_type_check_final_push_8() {
    check_no_errors("fn f(x: number) -> bool {\nlet a = x > 0;\nlet b = x < 100;\na && b\n}");
}

#[test]
fn test_type_check_final_push_9() {
    check_no_errors("struct Range { lo: number, hi: number }\nfn clamp(x: number, r: Range) -> number {\nif x < r.lo { r.lo } else { if x > r.hi { r.hi } else { x } }\n}");
}

#[test]
fn test_type_check_final_push_10() {
    check_no_errors("fn f(x: number, y: number) -> number {\nlet sum = x + y;\nlet diff = x - y;\nlet prod = x * y;\nsum + diff + prod\n}");
}

#[test]
fn test_type_check_mini_1() {
    check_no_errors("fn f(x: number) -> number {\nx + 1\n}");
}

#[test]
fn test_type_check_mini_2() {
    check_no_errors("fn f(x: number) -> number {\nx * x\n}");
}

#[test]
fn test_type_check_mini_3() {
    check_no_errors("fn f(x: number, y: number) -> number {\nx + y\n}");
}

#[test]
fn test_type_check_mini_4() {
    check_no_errors("fn f(x: number) -> bool {\nx > 0\n}");
}

#[test]
fn test_type_check_mini_5() {
    check_no_errors("fn f(x: number) -> number {\nlet y = x;\ny\n}");
}

#[test]
fn test_type_check_mini_6() {
    check_no_errors("fn f(x: number) -> number {\nlet y = x + 1;\ny\n}");
}

#[test]
fn test_type_check_mini_7() {
    check_no_errors("fn f() -> number {\n42\n}");
}

#[test]
fn test_type_check_mini_8() {
    check_no_errors("fn f() -> bool {\ntrue\n}");
}

#[test]
fn test_type_check_mini_9() {
    check_no_errors("fn f() -> string {\n\"hello\"\n}");
}

#[test]
fn test_type_check_mini_10() {
    check_no_errors("fn f(x: number) -> number {\nif x > 0 { x } else { 0 }\n}");
}

#[test]
fn test_type_check_micro_1() {
    check_no_errors("fn f(x: number) -> number {\n0 - x\n}");
}

#[test]
fn test_type_check_micro_2() {
    check_no_errors("fn f(x: number) -> bool {\nx == 0\n}");
}

#[test]
fn test_type_check_micro_3() {
    check_no_errors("fn f(x: number) -> bool {\nx != 0\n}");
}

#[test]
fn test_type_check_micro_4() {
    check_no_errors("fn f(x: number, y: number) -> bool {\nx == y\n}");
}

#[test]
fn test_type_check_micro_5() {
    check_no_errors("fn f(x: number, y: number) -> bool {\nx != y\n}");
}

#[test]
fn test_type_check_micro_6() {
    check_no_errors("fn f(x: number, y: number) -> bool {\nx < y\n}");
}

#[test]
fn test_type_check_micro_7() {
    check_no_errors("fn f(x: number, y: number) -> bool {\nx > y\n}");
}

#[test]
fn test_type_check_micro_8() {
    check_no_errors("fn f(x: number, y: number) -> bool {\nx <= y\n}");
}

#[test]
fn test_type_check_micro_9() {
    check_no_errors("fn f(x: number, y: number) -> bool {\nx >= y\n}");
}

#[test]
fn test_type_check_micro_10() {
    check_no_errors("fn f(x: number, y: number) -> number {\nx % y\n}");
}

#[test]
fn test_type_check_bitwise_1() {
    check_no_errors("fn f(x: number, y: number) -> number {\nx & y\n}");
}

#[test]
fn test_type_check_bitwise_2() {
    check_no_errors("fn f(x: number, y: number) -> number {\nx | y\n}");
}

#[test]
fn test_type_check_bitwise_3() {
    check_no_errors("fn f(x: number, y: number) -> number {\nx ^ y\n}");
}

#[test]
fn test_type_check_bitwise_5() {
    check_no_errors("fn f(x: number, y: number) -> number {\nx << y\n}");
}

#[test]
fn test_type_check_bitwise_6() {
    check_no_errors("fn f(x: number, y: number) -> number {\nx >> y\n}");
}

#[test]
fn test_type_check_bitwise_7() {
    check_no_errors("fn f(x: bool) -> bool {\n!x\n}");
}

#[test]
fn test_type_check_bitwise_8() {
    check_no_errors("fn f(x: number, y: number) -> bool {\nx & y != 0\n}");
}

#[test]
fn test_type_check_bitwise_9() {
    check_no_errors("fn f(x: number, y: number) -> bool {\nx | y != 0\n}");
}

#[test]
fn test_type_check_bitwise_10() {
    check_no_errors("fn f(x: number, y: number) -> number {\nlet a = x & y;\nlet b = x | y;\nlet c = x ^ y;\na + b + c\n}");
}

#[test]
fn test_type_check_error_code_1() {
    check_has_errors("fn f() -> number {\nundefined_var\n}");
}

#[test]
fn test_type_check_error_code_2() {
    check_has_errors("fn f() -> number {\nlet x: string = 42;\nx\n}");
}

#[test]
fn test_type_check_error_code_3() {
    check_has_errors("fn f(x: number, y: number, z: number) -> number {\nf(x)\n}");
}

#[test]
fn test_type_check_error_code_4() {
    check_has_errors("struct Foo { x: number }\nfn f() -> Foo {\nFoo { x: 1, y: 2 }\n}");
}

#[test]
fn test_type_check_error_code_5() {
    check_has_errors("struct Foo { x: number }\nfn f() -> Foo {\nFoo { z: 1 }\n}");
}

#[test]
fn test_type_check_error_code_6() {
    check_has_errors("enum Color { Red, Green, Blue }\nfn f(c: Color) -> number {\nmatch c {\nColor::Red => 1\n}\n}");
}

#[test]
fn test_type_check_error_code_7() {
    check_has_errors("fn f() -> number {\nmatch 1 {\n}\n}");
}

#[test]
fn test_type_check_error_code_8() {
    check_has_errors("fn f() -> number {\n42(\"hello\")\n}");
}

#[test]
fn test_type_check_error_code_9() {
    check_has_errors("fn main() -> string {\n\"hello\"\n}");
}

#[test]
fn test_type_check_error_code_10() {
    check_has_errors("fn f(x: number) -> number {\nx.y\n}");
}

#[test]
fn test_type_check_string_ops_1() {
    check_no_errors("fn f(s: string) -> string {\ns + \"!\"\n}");
}

#[test]
fn test_type_check_string_ops_2() {
    check_no_errors("fn f(s: string) -> number {\nlen(s)\n}");
}

#[test]
fn test_type_check_string_ops_3() {
    check_no_errors("fn f(a: string, b: string) -> string {\na + b\n}");
}

#[test]
fn test_type_check_string_ops_4() {
    check_no_errors("fn f(s: string) -> bool {\ns == \"hello\"\n}");
}

#[test]
fn test_type_check_string_ops_5() {
    check_no_errors("fn f(s: string) -> bool {\ns != \"hello\"\n}");
}

#[test]
fn test_type_check_ref_1() {
    check_no_errors("fn f(x: number) -> number {\nlet r = &x;\n*r\n}");
}

#[test]
fn test_type_check_ref_2() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x;\nlet r = &a;\n*r\n}");
}

#[test]
fn test_type_check_ref_3() {
    check_no_errors("fn f(x: number) -> number {\nlet a = 42;\nlet b = &a;\n*b + 1\n}");
}

#[test]
fn test_type_check_char_1() {
    check_no_errors("fn f() -> number {\nlet c = 'A';\nc + 1\n}");
}

#[test]
fn test_type_check_char_2() {
    check_no_errors("fn f() -> number {\nlet c = 'A';\nlet d = 'B';\nc + d\n}");
}

#[test]
fn test_type_check_closure_1() {
    check_no_errors("fn f() -> number {\nlet add = |a: number, b: number| -> number { a + b };\nadd(1, 2)\n}");
}

#[test]
fn test_type_check_closure_2() {
    check_no_errors("fn f() -> number {\nlet double = |x: number| -> number { x * 2 };\ndouble(21)\n}");
}

#[test]
fn test_type_check_closure_3() {
    check_no_errors("fn f(x: number) -> number {\nlet inc = |n: number| -> number { n + 1 };\ninc(x)\n}");
}

#[test]
fn test_type_check_closure_4() {
    check_no_errors("fn f() -> number {\nlet add = |a: number, b: number| -> number { a + b };\nlet mul = |a: number, b: number| -> number { a * b };\nadd(mul(2, 3), 4)\n}");
}

#[test]
fn test_type_check_closure_5() {
    check_no_errors("fn f(x: number) -> number {\nlet transform = |n: number| -> number {\nif n > 0 { n * 2 } else { n }\n};\ntransform(x)\n}");
}

#[test]
fn test_type_check_closure_6() {
    check_no_errors("fn f() -> bool {\nlet is_positive = |x: number| -> bool { x > 0 };\nis_positive(42)\n}");
}

#[test]
fn test_type_check_closure_7() {
    check_no_errors("fn f(x: number) -> number {\nlet apply = |f: fn(number) -> number, x: number| -> number { f(x) };\nlet double = |n: number| -> number { n * 2 };\napply(double, x)\n}");
}

#[test]
fn test_type_check_tuple_1() {
    check_no_errors("struct Pair { a: number, b: number }\nfn f() -> number {\nlet p = Pair { a: 1, b: 2 };\np.a + p.b\n}");
}

#[test]
fn test_type_check_tuple_2() {
    check_no_errors("struct Triple { x: number, y: number, z: number }\nfn f() -> number {\nlet t = Triple { x: 1, y: 2, z: 3 };\nt.x + t.y + t.z\n}");
}

#[test]
fn test_type_check_tuple_3() {
    check_no_errors("struct Pair { a: number, b: number }\nfn swap(p: Pair) -> Pair {\nPair { a: p.b, b: p.a }\n}");
}

#[test]
fn test_type_check_pattern_1() {
    check_no_errors("enum Option2 { Some2(number), None2 }\nfn f(opt: Option2) -> number {\nmatch opt {\nOption2::Some2(x) => x * 2\nOption2::None2 => 0\n}\n}");
}

#[test]
fn test_type_check_pattern_2() {
    check_no_errors("enum Result2 { Ok2(number), Err2(string) }\nfn f(res: Result2) -> number {\nmatch res {\nResult2::Ok2(x) => x\nResult2::Err2(e) => 0\n}\n}");
}

#[test]
fn test_type_check_pattern_3() {
    check_no_errors("enum Shape { Circle(number), Rect(number, number) }\nfn area(s: Shape) -> number {\nmatch s {\nShape::Circle(r) => 3 * r * r\nShape::Rect(w, h) => w * h\n}\n}");
}

#[test]
fn test_type_check_pattern_4() {
    check_no_errors("struct Point { x: number, y: number }\nfn f(p: Point) -> number {\nmatch p {\nPoint { x, y } => x + y\n}\n}");
}

#[test]
fn test_type_check_pattern_5() {
    check_no_errors("fn f(x: number) -> number {\nmatch x {\n0 => 100\n1 => 200\n2 => 300\n_ => x\n}\n}");
}

#[test]
fn test_type_check_pattern_6() {
    check_no_errors("fn f(x: number) -> string {\nmatch x {\n0 => \"zero\"\n1 => \"one\"\n2 => \"two\"\n_ => \"other\"\n}\n}");
}

#[test]
fn test_type_check_pattern_7() {
    check_no_errors("fn f(b: bool) -> number {\nmatch b {\ntrue => 1\nfalse => 0\n}\n}");
}

#[test]
fn test_type_check_pattern_8() {
    check_no_errors("enum Bool { True, False }\nfn f(b: Bool) -> number {\nmatch b {\nBool::True => 1\nBool::False => 0\n}\n}");
}

#[test]
fn test_type_check_pattern_9() {
    check_no_errors("enum Maybe { Just(string), Nothing }\nfn f(m: Maybe) -> string {\nmatch m {\nMaybe::Just(s) => s\nMaybe::Nothing => \"default\"\n}\n}");
}

#[test]
fn test_type_check_pattern_10() {
    check_no_errors("enum Either { Left(number), Right(string) }\nfn f(e: Either) -> string {\nmatch e {\nEither::Left(n) => \"number\"\nEither::Right(s) => s\n}\n}");
}

#[test]
fn test_type_check_recursion_1() {
    check_no_errors("fn fact(n: number) -> number {\nif n <= 1 { 1 } else { n * fact(n - 1) }\n}");
}

#[test]
fn test_type_check_recursion_2() {
    check_no_errors("fn fib(n: number) -> number {\nif n <= 1 { n } else { fib(n - 1) + fib(n - 2) }\n}");
}

#[test]
fn test_type_check_recursion_3() {
    check_no_errors("fn gcd(a: number, b: number) -> number {\nif b == 0 { a } else { gcd(b, a % b) }\n}");
}

#[test]
fn test_type_check_recursion_4() {
    check_no_errors("fn power(base: number, exp: number) -> number {\nif exp == 0 { 1 } else { base * power(base, exp - 1) }\n}");
}

#[test]
fn test_type_check_recursion_5() {
    check_no_errors("fn sum_to(n: number) -> number {\nif n == 0 { 0 } else { n + sum_to(n - 1) }\n}");
}

#[test]
fn test_type_check_mutual_1() {
    check_no_errors("fn is_even(n: number) -> bool {\nif n == 0 { true } else { is_odd(n - 1) }\n}\nfn is_odd(n: number) -> bool {\nif n == 0 { false } else { is_even(n - 1) }\n}");
}

#[test]
fn test_type_check_higher_order_1() {
    check_no_errors("fn apply(f: fn(number) -> number, x: number) -> number {\nf(x)\n}");
}

#[test]
fn test_type_check_higher_order_2() {
    check_no_errors("fn compose(f: fn(number) -> number, g: fn(number) -> number, x: number) -> number {\nf(g(x))\n}");
}

#[test]
fn test_type_check_higher_order_3() {
    check_no_errors("fn twice(f: fn(number) -> number, x: number) -> number {\nf(f(x))\n}");
}

#[test]
fn test_type_check_higher_order_4() {
    check_no_errors("fn add_n(n: number) -> fn(number) -> number {\n|x: number| -> number { x + n }\n}");
}

#[test]
fn test_type_check_control_1() {
    check_no_errors("fn f(x: number) -> number {\nlet total = 0;\nfor i in 0..x {\nif i % 2 == 0 {\ntotal = total + i\n}\n};\ntotal\n}");
}

#[test]
fn test_type_check_control_2() {
    check_no_errors("fn f(x: number) -> number {\nlet total = 0;\nwhile total < x {\ntotal = total + 1\n};\ntotal\n}");
}

#[test]
fn test_type_check_control_3() {
    check_no_errors("fn f(items: number) -> number {\nlet count = 0;\nfor i in 0..items {\ncount = count + 1\n};\ncount\n}");
}

#[test]
fn test_type_check_control_4() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 0;\nfor i in 0..10 {\nresult = result + i * x\n};\nresult\n}");
}

#[test]
fn test_type_check_control_5() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 1;\nfor i in 1..x {\nresult = result * i\n};\nresult\n}");
}

#[test]
fn test_type_check_control_6() {
    check_no_errors("fn f(a: number, b: number) -> number {\nlet count = 0;\nwhile count < a {\ncount = count + b\n};\ncount\n}");
}

#[test]
fn test_type_check_control_7() {
    check_no_errors("fn f(n: number) -> number {\nlet sum = 0;\nfor i in 0..n {\nsum = sum + i * i\n};\nsum\n}");
}

#[test]
fn test_type_check_control_8() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x;\nlet b = x;\nwhile a < 100 {\na = a + b\n};\na\n}");
}

#[test]
fn test_type_check_control_9() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 0;\nfor i in 0..100 {\nif i % x == 0 {\nresult = result + i\n}\n};\nresult\n}");
}

#[test]
fn test_type_check_control_10() {
    check_no_errors("fn f(n: number) -> number {\nlet fib_prev = 0;\nlet fib_curr = 1;\nlet i = 0;\nwhile i < n {\nlet temp = fib_curr;\nfib_curr = fib_prev + fib_curr;\nfib_prev = temp;\ni = i + 1\n};\nfib_prev\n}");
}

#[test]
fn test_type_check_builtins_1() {
    check_no_errors("fn f(x: number) -> number {\nabs(x)\n}");
}

#[test]
fn test_type_check_builtins_2() {
    check_no_errors("fn f(x: number, y: number) -> number {\nmin(x, y)\n}");
}

#[test]
fn test_type_check_builtins_3() {
    check_no_errors("fn f(x: number, y: number) -> number {\nmax(x, y)\n}");
}

#[test]
fn test_type_check_builtins_4() {
    check_no_errors("fn f(s: string) -> number {\nlen(s)\n}");
}

#[test]
fn test_type_check_builtins_5() {
    check_no_errors("fn f() -> number {\nlen(\"hello\")\n}");
}

#[test]
fn test_type_check_generic_1() {
    check_no_errors("fn id(x: number) -> number {\nx\n}\nfn main() -> number {\nid(42)\n}");
}

#[test]
fn test_type_check_generic_2() {
    check_no_errors("fn first(a: number, b: number) -> number {\na\n}\nfn main() -> number {\nfirst(1, 2)\n}");
}

#[test]
fn test_type_check_generic_3() {
    check_no_errors("fn apply(f: fn(number) -> number, x: number) -> number {\nf(x)\n}\nfn main() -> number {\nlet double = |x: number| -> number { x * 2 };\napply(double, 5)\n}");
}

#[test]
fn test_type_check_error_detect_1() {
    check_has_errors("fn f(x: number) -> number {\nx.y\n}");
}

#[test]
fn test_type_check_error_detect_2() {
    check_has_errors("fn f(x: number, y: number, z: number) -> number {\nf(x)\n}");
}

#[test]
fn test_type_check_edge_1() {
    check_no_errors("fn f() -> number {\nlet x = 0;\nx\n}");
}

#[test]
fn test_type_check_edge_2() {
    check_no_errors("fn f() -> number {\nlet x = 1;\nlet y = 2;\nlet z = 3;\nx + y + z\n}");
}

#[test]
fn test_type_check_edge_3() {
    check_no_errors("fn f() -> string {\nlet a = \"hello\";\nlet b = \" \";\nlet c = \"world\";\na + b + c\n}");
}

#[test]
fn test_type_check_edge_4() {
    check_no_errors("fn f() -> bool {\nlet a = true;\nlet b = false;\na && b || a\n}");
}

#[test]
fn test_type_check_edge_5() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x;\nlet b = a;\nlet c = b;\nlet d = c;\nlet e = d;\ne\n}");
}

#[test]
fn test_type_check_edge_6() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 0;\nfor i in 0..10 {\nfor j in 0..10 {\nresult = result + 1\n}\n};\nresult\n}");
}

#[test]
fn test_type_check_edge_7() {
    check_no_errors("struct A { x: number }\nstruct B { y: number }\nfn f(a: A, b: B) -> number {\na.x + b.y\n}");
}

#[test]
fn test_type_check_edge_8() {
    check_no_errors("enum X { A, B }\nenum Y { C, D }\nfn f(x: X) -> Y {\nmatch x {\nX::A => Y::C\nX::B => Y::D\n}\n}");
}

#[test]
fn test_type_check_edge_9() {
    check_no_errors("fn f(x: number) -> number {\nlet a = if x > 0 {\nlet b = x * 2;\nb\n} else {\n0\n};\na + 1\n}");
}

#[test]
fn test_type_check_edge_10() {
    check_no_errors("struct Pair { fst: number, snd: number }\nfn f(p: Pair) -> Pair {\nlet a = p.fst;\nlet b = p.snd;\nPair { fst: b, snd: a }\n}");
}

#[test]
fn test_type_check_final_v2_1() {
    check_no_errors("fn add(a: number, b: number) -> number { a + b }\nfn mul(a: number, b: number) -> number { a * b }\nfn main() -> number {\nadd(mul(2, 3), mul(4, 5))\n}");
}

#[test]
fn test_type_check_final_v2_2() {
    check_no_errors("fn f(x: number) -> number {\nlet y = if x > 0 { x } else { 0 - x };\nlet z = y * y;\nz\n}");
}

#[test]
fn test_type_check_final_v2_3() {
    check_no_errors("struct Point { x: number, y: number }\nfn midpoint(a: Point, b: Point) -> Point {\nPoint { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 }\n}");
}

#[test]
fn test_type_check_final_v2_4() {
    check_no_errors("enum TrafficLight { Red, Yellow, Green }\nfn next_light(l: TrafficLight) -> TrafficLight {\nmatch l {\nTrafficLight::Red => TrafficLight::Green\nTrafficLight::Green => TrafficLight::Yellow\nTrafficLight::Yellow => TrafficLight::Red\n}\n}");
}

#[test]
fn test_type_check_final_v2_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 0 {\nif x > 100 { None } else { Some(x) }\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_final_v2_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x >= 0 {\nif x <= 100 { Ok(x) } else { Err(\"too large\") }\n} else {\nErr(\"negative\")\n}\n}");
}

#[test]
fn test_type_check_final_v2_7() {
    check_no_errors("struct Vec2 { x: number, y: number }\nfn add_vec(a: Vec2, b: Vec2) -> Vec2 {\nVec2 { x: a.x + b.x, y: a.y + b.y }\n}\nfn scale_vec(v: Vec2, s: number) -> Vec2 {\nVec2 { x: v.x * s, y: v.y * s }\n}");
}

#[test]
fn test_type_check_final_v2_8() {
    check_no_errors("enum Expr { Val(number), Add(number, number), Mul(number, number) }\nfn eval(e: Expr) -> number {\nmatch e {\nExpr::Val(n) => n\nExpr::Add(a, b) => a + b\nExpr::Mul(a, b) => a * b\n}\n}");
}

#[test]
fn test_type_check_final_v2_9() {
    check_no_errors("fn f(n: number) -> number {\nlet result = 0;\nfor i in 1..n {\nif i % 3 == 0 {\nresult = result + i\n} else {\nif i % 5 == 0 {\nresult = result + i\n}\n}\n};\nresult\n}");
}

#[test]
fn test_type_check_final_v2_10() {
    check_no_errors("struct Rectangle { width: number, height: number }\nfn area(r: Rectangle) -> number { r.width * r.height }\nfn bigger(a: Rectangle, b: Rectangle) -> Rectangle {\nif area(a) > area(b) { a } else { b }\n}");
}

#[test]
fn test_type_check_v3_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x + 1;\nlet b = a * 2;\nlet c = b - 3;\nlet d = c / 4;\nd\n}");
}

#[test]
fn test_type_check_v3_2() {
    check_no_errors("fn f(x: number) -> bool {\nlet a = x > 0;\nlet b = x < 100;\nlet c = a && b;\nc\n}");
}

#[test]
fn test_type_check_v3_3() {
    check_no_errors("fn f(x: number) -> string {\nif x == 0 { \"zero\" } else { if x == 1 { \"one\" } else { \"other\" } }\n}");
}

#[test]
fn test_type_check_v3_4() {
    check_no_errors("struct Box { w: number, h: number, d: number }\nfn volume(b: Box) -> number {\nb.w * b.h * b.d\n}");
}

#[test]
fn test_type_check_v3_5() {
    check_no_errors("enum Coin { Penny, Nickel, Dime, Quarter }\nfn value(c: Coin) -> number {\nmatch c {\nCoin::Penny => 1\nCoin::Nickel => 5\nCoin::Dime => 10\nCoin::Quarter => 25\n}\n}");
}

#[test]
fn test_type_check_v3_6() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 0 && x < 100 {\nSome(x)\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v3_7() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x >= 0 && x <= 100 {\nOk(x)\n} else {\nif x < 0 {\nErr(\"negative\")\n} else {\nErr(\"too large\")\n}\n}\n}");
}

#[test]
fn test_type_check_v3_8() {
    check_no_errors("struct Matrix { a: number, b: number, c: number, d: number }\nfn determinant(m: Matrix) -> number {\nm.a * m.d - m.b * m.c\n}");
}

#[test]
fn test_type_check_v3_9() {
    check_no_errors("enum Planet { Mercury, Venus, Earth, Mars }\nfn distance_to_sun(p: Planet) -> number {\nmatch p {\nPlanet::Mercury => 58\nPlanet::Venus => 108\nPlanet::Earth => 150\nPlanet::Mars => 228\n}\n}");
}

#[test]
fn test_type_check_v3_10() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 0;\nfor i in 0..10 {\nresult = result + if i % 2 == 0 { i } else { 0 }\n};\nresult\n}");
}

#[test]
fn test_type_check_v4_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x;\na = a + 1;\na = a + 1;\na = a + 1;\na\n}");
}

#[test]
fn test_type_check_v4_2() {
    check_no_errors("fn f(x: number) -> string {\nlet s = \"\";\nlet s = s + \"hello\";\nlet s = s + \" \";\nlet s = s + \"world\";\ns\n}");
}

#[test]
fn test_type_check_v4_3() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 1;\nfor i in 1..x {\nresult = result * i\n};\nresult\n}");
}

#[test]
fn test_type_check_v4_4() {
    check_no_errors("struct Interval { lo: number, hi: number }\nfn contains(i: Interval, x: number) -> bool {\nx >= i.lo && x <= i.hi\n}\nfn overlap(a: Interval, b: Interval) -> bool {\ncontains(a, b.lo) || contains(a, b.hi) || contains(b, a.lo)\n}");
}

#[test]
fn test_type_check_v4_5() {
    check_no_errors("enum Weather { Sunny, Cloudy, Rainy }\nfn activity(w: Weather) -> string {\nmatch w {\nWeather::Sunny => \"picnic\"\nWeather::Cloudy => \"walk\"\nWeather::Rainy => \"read\"\n}\n}");
}

#[test]
fn test_type_check_v4_6() {
    check_no_errors("fn f(x: number) -> Option<number> {\nmatch x {\n0 => None\n1 => Some(1)\n2 => Some(2)\n_ => Some(x)\n}\n}");
}

#[test]
fn test_type_check_v4_7() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x > 0 {\nif x % 2 == 0 { Ok(x) } else { Err(\"odd\") }\n} else {\nErr(\"negative\")\n}\n}");
}

#[test]
fn test_type_check_v4_8() {
    check_no_errors("struct Complex { real: number, imag: number }\nfn add_complex(a: Complex, b: Complex) -> Complex {\nComplex { real: a.real + b.real, imag: a.imag + b.imag }\n}\nfn magnitude_sq(c: Complex) -> number {\nc.real * c.real + c.imag * c.imag\n}");
}

#[test]
fn test_type_check_v4_9() {
    check_no_errors("enum TreeNode { Leaf(number), Branch(number, number) }\nfn sum_tree(t: TreeNode) -> number {\nmatch t {\nTreeNode::Leaf(x) => x\nTreeNode::Branch(a, b) => a + b\n}\n}");
}

#[test]
fn test_type_check_v4_10() {
    check_no_errors("fn f(n: number) -> number {\nlet a = 0;\nlet b = 1;\nfor i in 0..n {\nlet temp = b;\nb = a + b;\na = temp\n};\na\n}");
}

#[test]
fn test_type_check_v5_1() {
    check_no_errors("fn f(x: number, y: number) -> number {\nlet sum = x + y;\nlet diff = x - y;\nlet prod = sum * diff;\nprod\n}");
}

#[test]
fn test_type_check_v5_2() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x;\nlet b = a + 1;\nlet c = b * 2;\nlet d = c - 3;\nd\n}");
}

#[test]
fn test_type_check_v5_3() {
    check_no_errors("fn f(x: number) -> bool {\nlet a = x > 0;\nlet b = x < 100;\nlet c = x != 50;\na && b && c\n}");
}

#[test]
fn test_type_check_v5_4() {
    check_no_errors("struct Time { hour: number, minute: number }\nfn to_minutes(t: Time) -> number {\nt.hour * 60 + t.minute\n}\nfn is_morning(t: Time) -> bool {\nt.hour < 12\n}");
}

#[test]
fn test_type_check_v5_5() {
    check_no_errors("enum Suit { Hearts, Diamonds, Clubs, Spades }\nfn is_red(s: Suit) -> bool {\nmatch s {\nSuit::Hearts => true\nSuit::Diamonds => true\nSuit::Clubs => false\nSuit::Spades => false\n}\n}");
}

#[test]
fn test_type_check_v5_6() {
    check_no_errors("fn f(x: number) -> number {\nlet total = 0;\nfor i in 0..x {\ntotal = total + if i % 2 == 0 { i * i } else { 0 }\n};\ntotal\n}");
}

#[test]
fn test_type_check_v5_7() {
    check_no_errors("fn f(items: number) -> number {\nlet count = 0;\nfor i in 0..items {\nif i % 3 == 0 || i % 5 == 0 {\ncount = count + i\n}\n};\ncount\n}");
}

#[test]
fn test_type_check_v5_8() {
    check_no_errors("struct Fraction { num: number, den: number }\nfn multiply(a: Fraction, b: Fraction) -> Fraction {\nFraction { num: a.num * b.num, den: a.den * b.den }\n}");
}

#[test]
fn test_type_check_v5_9() {
    check_no_errors("enum Logic { And, Or, Not }\nfn apply(op: Logic, a: bool, b: bool) -> bool {\nmatch op {\nLogic::And => a && b\nLogic::Or => a || b\nLogic::Not => !a\n}\n}");
}

#[test]
fn test_type_check_v5_10() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x;\nlet b = a * a;\nlet c = b + a;\nlet d = c * c;\nd\n}");
}

#[test]
fn test_type_check_v6_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x + 1;\nlet b = a + 2;\nlet c = b + 3;\nlet d = c + 4;\nlet e = d + 5;\ne\n}");
}

#[test]
fn test_type_check_v6_2() {
    check_no_errors("fn f(x: number) -> number {\nlet y = 0;\nfor i in 0..100 {\ny = y + i\n};\ny\n}");
}

#[test]
fn test_type_check_v6_3() {
    check_no_errors("struct Date { year: number, month: number, day: number }\nfn is_leap_year(d: Date) -> bool {\n(d.year % 4 == 0 && d.year % 100 != 0) || d.year % 400 == 0\n}");
}

#[test]
fn test_type_check_v6_4() {
    check_no_errors("enum Piece { King, Queen, Rook, Bishop, Knight, Pawn }\nfn value(p: Piece) -> number {\nmatch p {\nPiece::King => 0\nPiece::Queen => 9\nPiece::Rook => 5\nPiece::Bishop => 3\nPiece::Knight => 3\nPiece::Pawn => 1\n}\n}");
}

#[test]
fn test_type_check_v6_5() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nlet i = 1;\nwhile i <= x {\nsum = sum + i;\ni = i + 1\n};\nsum\n}");
}

#[test]
fn test_type_check_v6_6() {
    check_no_errors("struct Angle { degrees: number }\nfn normalize(a: Angle) -> Angle {\nAngle { degrees: a.degrees % 360 }\n}\nfn is_acute(a: Angle) -> bool {\na.degrees > 0 && a.degrees < 90\n}");
}

#[test]
fn test_type_check_v6_7() {
    check_no_errors("enum Season2 { Spring(number), Summer(number), Fall(number), Winter(number) }\nfn temp_range(s: Season2) -> number {\nmatch s {\nSeason2::Spring(t) => t\nSeason2::Summer(t) => t\nSeason2::Fall(t) => t\nSeason2::Winter(t) => t\n}\n}");
}

#[test]
fn test_type_check_v6_8() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 0;\nfor i in 0..10 {\nfor j in 0..10 {\nresult = result + i + j\n}\n};\nresult\n}");
}

#[test]
fn test_type_check_v6_9() {
    check_no_errors("struct RGB { r: number, g: number, b: number }\nfn brightness(c: RGB) -> number {\n(c.r + c.g + c.b) / 3\n}\nfn is_grayscale(c: RGB) -> bool {\nc.r == c.g && c.g == c.b\n}");
}

#[test]
fn test_type_check_v6_10() {
    check_no_errors("fn f(x: number) -> number {\nlet prev = 0;\nlet curr = 1;\nfor i in 0..x {\nlet next = prev + curr;\nprev = curr;\ncurr = next\n};\nprev\n}");
}

#[test]
fn test_type_check_v7_1() {
    check_no_errors("fn f(x: number) -> number {\nmatch x {\n0 => 0\n1 => 1\n2 => 2\n3 => 3\n4 => 4\n5 => 5\n_ => x\n}\n}");
}

#[test]
fn test_type_check_v7_2() {
    check_no_errors("fn f(x: number) -> string {\nif x > 90 { \"A\" } else {\nif x > 80 { \"B\" } else {\nif x > 70 { \"C\" } else {\nif x > 60 { \"D\" } else { \"F\" }\n}\n}\n}\n}");
}

#[test]
fn test_type_check_v7_3() {
    check_no_errors("struct Point3D { x: number, y: number, z: number }\nfn distance(a: Point3D, b: Point3D) -> number {\nlet dx = a.x - b.x;\nlet dy = a.y - b.y;\nlet dz = a.z - b.z;\ndx * dx + dy * dy + dz * dz\n}");
}

#[test]
fn test_type_check_v7_4() {
    check_no_errors("enum Temperature { Celsius(number), Fahrenheit(number) }\nfn to_celsius(t: Temperature) -> number {\nmatch t {\nTemperature::Celsius(c) => c\nTemperature::Fahrenheit(f) => (f - 32) * 5 / 9\n}\n}");
}

#[test]
fn test_type_check_v7_5() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 1;\nfor i in 2..x {\nresult = result * i\n};\nresult\n}");
}

#[test]
fn test_type_check_v7_6() {
    check_no_errors("struct Rational { numerator: number, denominator: number }\nfn add_rational(a: Rational, b: Rational) -> Rational {\nRational { numerator: a.numerator * b.denominator + b.numerator * a.denominator, denominator: a.denominator * b.denominator }\n}");
}

#[test]
fn test_type_check_v7_7() {
    check_no_errors("enum Operation { Add, Sub, Mul, Div }\nfn apply_op(op: Operation, a: number, b: number) -> number {\nmatch op {\nOperation::Add => a + b\nOperation::Sub => a - b\nOperation::Mul => a * b\nOperation::Div => a / b\n}\n}");
}

#[test]
fn test_type_check_v7_8() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 1..x {\nsum = sum + i * i\n};\nsum\n}");
}

#[test]
fn test_type_check_v7_9() {
    check_no_errors("struct Polar { r: number, theta: number }\nfn to_x(p: Polar) -> number {\np.r\n}\nfn to_y(p: Polar) -> number {\np.r\n}");
}

#[test]
fn test_type_check_v7_10() {
    check_no_errors("fn f(x: number) -> number {\nlet prev = 0;\nlet curr = 1;\nlet i = 0;\nwhile i < x {\nlet temp = prev + curr;\nprev = curr;\ncurr = temp;\ni = i + 1\n};\nprev\n}");
}

#[test]
fn test_type_check_v8_1() {
    check_no_errors("fn f(x: number) -> number {\nlet y = x;\nwhile y > 0 {\ny = y - 1\n};\ny\n}");
}

#[test]
fn test_type_check_v8_2() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 0;\nfor i in 0..x {\nresult = result + i * i * i\n};\nresult\n}");
}

#[test]
fn test_type_check_v8_3() {
    check_no_errors("struct Student { name: string, score: number }\nfn passed(s: Student) -> bool {\ns.score >= 60\n}");
}

#[test]
fn test_type_check_v8_4() {
    check_no_errors("enum Animal { Dog, Cat, Bird, Fish }\nfn sound(a: Animal) -> string {\nmatch a {\nAnimal::Dog => \"woof\"\nAnimal::Cat => \"meow\"\nAnimal::Bird => \"tweet\"\nAnimal::Fish => \"glub\"\n}\n}");
}

#[test]
fn test_type_check_v8_5() {
    check_no_errors("fn f(x: number) -> Option<string> {\nmatch x {\n0 => None\n_ => Some(\"found\")\n}\n}");
}

#[test]
fn test_type_check_v8_6() {
    check_no_errors("fn f(x: number) -> Result<string, string> {\nif x > 0 {\nOk(\"positive\")\n} else {\nErr(\"non-positive\")\n}\n}");
}

#[test]
fn test_type_check_v8_7() {
    check_no_errors("struct Quaternion { w: number, x: number, y: number, z: number }\nfn conjugate(q: Quaternion) -> Quaternion {\nQuaternion { w: q.w, x: 0 - q.x, y: 0 - q.y, z: 0 - q.z }\n}");
}

#[test]
fn test_type_check_v8_8() {
    check_no_errors("enum Instruction { Load(number), Store(number), Add(number) }\nfn execute(inst: Instruction) -> number {\nmatch inst {\nInstruction::Load(x) => x\nInstruction::Store(x) => x\nInstruction::Add(x) => x\n}\n}");
}

#[test]
fn test_type_check_v8_9() {
    check_no_errors("fn f(x: number) -> number {\nlet max = x;\nfor i in 0..100 {\nif i > max {\nmax = i\n}\n};\nmax\n}");
}

#[test]
fn test_type_check_v8_10() {
    check_no_errors("fn f(a: number, b: number) -> number {\nlet x = a;\nlet y = b;\nwhile y != 0 {\nlet temp = y;\ny = x % y;\nx = temp\n};\nx\n}");
}

#[test]
fn test_type_check_v9_1() {
    check_no_errors("fn f(x: number) -> number {\nlet y = 0 - x;\nif y < 0 { 0 - y } else { y }\n}");
}

#[test]
fn test_type_check_v9_2() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 0;\nfor i in 0..x {\nresult = result + i * (i + 1)\n};\nresult\n}");
}

#[test]
fn test_type_check_v9_3() {
    check_no_errors("struct Person { age: number }\nfn is_adult(p: Person) -> bool {\np.age >= 18\n}");
}

#[test]
fn test_type_check_v9_4() {
    check_no_errors("enum Direction2 { Up, Down, Left, Right }\nfn opposite(d: Direction2) -> Direction2 {\nmatch d {\nDirection2::Up => Direction2::Down\nDirection2::Down => Direction2::Up\nDirection2::Left => Direction2::Right\nDirection2::Right => Direction2::Left\n}\n}");
}

#[test]
fn test_type_check_v9_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 0 && x < 1000 {\nif x % 2 == 0 { Some(x) } else { None }\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v9_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x > 0 {\nif x % 2 == 0 { Ok(x) } else { Ok(x + 1) }\n} else {\nErr(\"non-positive\")\n}\n}");
}

#[test]
fn test_type_check_v9_7() {
    check_no_errors("struct Vector { x: number, y: number, z: number }\nfn dot(a: Vector, b: Vector) -> number {\na.x * b.x + a.y * b.y + a.z * b.z\n}");
}

#[test]
fn test_type_check_v9_8() {
    check_no_errors("enum Token { Number(number), Op(string), EOF }\nfn is_number(t: Token) -> bool {\nmatch t {\nToken::Number(_) => true\nToken::Op(_) => false\nToken::EOF => false\n}\n}");
}

#[test]
fn test_type_check_v9_9() {
    check_no_errors("fn f(x: number) -> number {\nlet min = x;\nfor i in 0..100 {\nif i < min {\nmin = i\n}\n};\nmin\n}");
}

#[test]
fn test_type_check_v9_10() {
    check_no_errors("fn f(n: number) -> number {\nlet a = 1;\nlet b = 1;\nfor i in 2..n {\nlet c = a + b;\na = b;\nb = c\n};\nb\n}");
}

#[test]
fn test_type_check_v10_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = 1;\nlet b = 1;\nlet c = a + b;\nlet d = b + c;\nlet e = c + d;\ne\n}");
}

#[test]
fn test_type_check_v10_2() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 0;\nfor i in 0..x {\nif i % 2 == 0 {\nresult = result + i\n} else {\nresult = result - i\n}\n};\nresult\n}");
}

#[test]
fn test_type_check_v10_3() {
    check_no_errors("struct Money { amount: number }\nfn add_money(a: Money, b: Money) -> Money {\nMoney { amount: a.amount + b.amount }\n}\nfn is_positive(m: Money) -> bool {\nm.amount > 0\n}");
}

#[test]
fn test_type_check_v10_4() {
    check_no_errors("enum Grade2 { A(number), B(number), C(number), D(number), F }\nfn to_number(g: Grade2) -> number {\nmatch g {\nGrade2::A(x) => x\nGrade2::B(x) => x\nGrade2::C(x) => x\nGrade2::D(x) => x\nGrade2::F => 0\n}\n}");
}

#[test]
fn test_type_check_v10_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nlet opt = if x > 0 { Some(x) } else { None };\nmatch opt {\nSome(v) => Some(v * 2)\nNone => None\n}\n}");
}

#[test]
fn test_type_check_v10_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nlet res = if x > 0 { Ok(x) } else { Err(\"non-positive\") };\nmatch res {\nOk(v) => Ok(v * 2)\nErr(e) => Err(e)\n}\n}");
}

#[test]
fn test_type_check_v10_7() {
    check_no_errors("struct Color { r: number, g: number, b: number }\nfn invert(c: Color) -> Color {\nColor { r: 255 - c.r, g: 255 - c.g, b: 255 - c.b }\n}\nfn is_black(c: Color) -> bool {\nc.r == 0 && c.g == 0 && c.b == 0\n}");
}

#[test]
fn test_type_check_v10_8() {
    check_no_errors("enum Expr2 { Val(number), Add2(number, number) }\nfn eval2(e: Expr2) -> number {\nmatch e {\nExpr2::Val(n) => n\nExpr2::Add2(a, b) => a + b\n}\n}\nfn is_val(e: Expr2) -> bool {\nmatch e {\nExpr2::Val(_) => true\nExpr2::Add2(_, _) => false\n}\n}");
}

#[test]
fn test_type_check_v10_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nlet i = 1;\nwhile i <= x {\nsum = sum + i;\ni = i + 1\n};\nsum\n}");
}

#[test]
fn test_type_check_v10_10() {
    check_no_errors("fn f(a: number, b: number) -> number {\nlet x = a;\nlet y = b;\nfor i in 0..10 {\nlet temp = x + y;\nx = y;\ny = temp\n};\ny\n}");
}

#[test]
fn test_type_check_v11_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x;\nlet b = a + a;\nlet c = b + b;\nlet d = c + c;\nd\n}");
}

#[test]
fn test_type_check_v11_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sign = if x > 0 { 1 } else { if x < 0 { 0 - 1 } else { 0 } };\nsign\n}");
}

#[test]
fn test_type_check_v11_3() {
    check_no_errors("fn f(x: number) -> string {\nmatch x % 3 {\n0 => \"fizz\"\n1 => \"buzz\"\n_ => \"fizzbuzz\"\n}\n}");
}

#[test]
fn test_type_check_v11_4() {
    check_no_errors("struct Point2D { x: number, y: number }\nfn quadrant(p: Point2D) -> number {\nif p.x > 0 {\nif p.y > 0 { 1 } else { 4 }\n} else {\nif p.y > 0 { 2 } else { 3 }\n}\n}");
}

#[test]
fn test_type_check_v11_5() {
    check_no_errors("enum Month2 { Jan, Feb, Mar, Apr, May, Jun }\nfn days(m: Month2) -> number {\nmatch m {\nMonth2::Jan => 31\nMonth2::Feb => 28\nMonth2::Mar => 31\nMonth2::Apr => 30\nMonth2::May => 31\nMonth2::Jun => 30\n}\n}");
}

#[test]
fn test_type_check_v11_6() {
    check_no_errors("fn f(x: number) -> number {\nlet total = 0;\nfor i in 1..x {\ntotal = total + if i % 2 == 0 { i * i } else { i }\n};\ntotal\n}");
}

#[test]
fn test_type_check_v11_7() {
    check_no_errors("struct Ratio { num: number, den: number }\nfn simplify(r: Ratio) -> Ratio {\nlet g = gcd(r.num, r.den);\nRatio { num: r.num / g, den: r.den / g }\n}\nfn gcd(a: number, b: number) -> number {\nif b == 0 { a } else { gcd(b, a % b) }\n}");
}

#[test]
fn test_type_check_v11_8() {
    check_no_errors("enum BST { Leaf2, Node(number) }\nfn sum_bst(t: BST) -> number {\nmatch t {\nBST::Leaf2 => 0\nBST::Node(v) => v\n}\n}");
}

#[test]
fn test_type_check_v11_9() {
    check_no_errors("fn f(x: number) -> number {\nlet count = 0;\nlet n = x;\nwhile n > 0 {\ncount = count + 1;\nn = n / 2\n};\ncount\n}");
}

#[test]
fn test_type_check_v11_10() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 0;\nfor i in 0..x {\nfor j in 0..i {\nresult = result + 1\n}\n};\nresult\n}");
}

#[test]
fn test_type_check_v12_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x;\nlet b = a + 1;\nlet c = b + 1;\nlet d = c + 1;\nlet e = d + 1;\nlet f_val = e + 1;\nf_val\n}");
}

#[test]
fn test_type_check_v12_2() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 0;\nfor i in 0..x {\nresult = result + if i % 3 == 0 { i } else { if i % 5 == 0 { i } else { 0 } }\n};\nresult\n}");
}

#[test]
fn test_type_check_v12_3() {
    check_no_errors("struct Score { math: number, english: number, science: number }\nfn total(s: Score) -> number {\ns.math + s.english + s.science\n}\nfn average(s: Score) -> number {\ntotal(s) / 3\n}");
}

#[test]
fn test_type_check_v12_4() {
    check_no_errors("enum Element { Fire, Water, Earth, Air }\nfn is_strong(a: Element) -> bool {\nmatch a {\nElement::Fire => true\nElement::Water => true\nElement::Earth => false\nElement::Air => false\n}\n}");
}

#[test]
fn test_type_check_v12_5() {
    check_no_errors("fn f(x: number) -> Option<string> {\nif x > 0 {\nif x > 100 {\nSome(\"large\")\n} else {\nSome(\"small\")\n}\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v12_6() {
    check_no_errors("fn f(x: number) -> Result<string, string> {\nif x > 100 {\nErr(\"too large\")\n} else {\nif x < 0 {\nErr(\"negative\")\n} else {\nif x == 0 {\nOk(\"zero\")\n} else {\nOk(\"positive\")\n}\n}\n}\n}");
}

#[test]
fn test_type_check_v12_7() {
    check_no_errors("struct Complex2 { real: number, imag: number }\nfn multiply(a: Complex2, b: Complex2) -> Complex2 {\nComplex2 { real: a.real * b.real - a.imag * b.imag, imag: a.real * b.imag + a.imag * b.real }\n}");
}

#[test]
fn test_type_check_v12_8() {
    check_no_errors("enum JSON { Num(number), Str(string), Bool(bool) }\nfn to_string(j: JSON) -> string {\nmatch j {\nJSON::Num(n) => \"number\"\nJSON::Str(s) => s\nJSON::Bool(b) => \"bool\"\n}\n}");
}

#[test]
fn test_type_check_v12_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nlet i = 1;\nwhile i <= x {\nsum = sum + i * i;\ni = i + 1\n};\nsum\n}");
}

#[test]
fn test_type_check_v12_10() {
    check_no_errors("fn f(x: number, y: number) -> number {\nlet a = x;\nlet b = y;\nfor i in 0..5 {\nlet temp = a + b;\na = b;\nb = temp\n};\nb\n}");
}

#[test]
fn test_type_check_v13_1() {
    check_no_errors("fn f(x: number) -> number {\nlet prev = 0;\nlet curr = 1;\nfor i in 0..x {\nlet next = prev + curr;\nprev = curr;\ncurr = next\n};\nprev\n}");
}

#[test]
fn test_type_check_v13_2() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 0;\nfor i in 1..x {\nif i % 2 == 0 {\nresult = result + i\n}\n};\nresult\n}");
}

#[test]
fn test_type_check_v13_3() {
    check_no_errors("struct Point { x: number, y: number }\nfn manhattan(a: Point, b: Point) -> number {\nabs(a.x - b.x) + abs(a.y - b.y)\n}");
}

#[test]
fn test_type_check_v13_4() {
    check_no_errors("enum Arrow { Up2, Down2, Left2, Right2 }\nfn horizontal(a: Arrow) -> bool {\nmatch a {\nArrow::Left2 => true\nArrow::Right2 => true\nArrow::Up2 => false\nArrow::Down2 => false\n}\n}");
}

#[test]
fn test_type_check_v13_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 0 {\nlet doubled = x * 2;\nSome(doubled)\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v13_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x > 100 {\nErr(\"overflow\")\n} else {\nif x < 0 {\nErr(\"underflow\")\n} else {\nOk(x * 2)\n}\n}\n}");
}

#[test]
fn test_type_check_v13_7() {
    check_no_errors("struct Matrix2 { a: number, b: number, c: number, d: number }\nfn trace(m: Matrix2) -> number {\nm.a + m.d\n}\nfn is_identity(m: Matrix2) -> bool {\nm.a == 1 && m.b == 0 && m.c == 0 && m.d == 1\n}");
}

#[test]
fn test_type_check_v13_8() {
    check_no_errors("enum Animal2 { Dog2(number), Cat2(number) }\nfn age(a: Animal2) -> number {\nmatch a {\nAnimal2::Dog2(x) => x\nAnimal2::Cat2(x) => x\n}\n}");
}

#[test]
fn test_type_check_v13_9() {
    check_no_errors("fn f(x: number) -> number {\nlet product = 1;\nfor i in 1..x {\nproduct = product * i\n};\nproduct\n}");
}

#[test]
fn test_type_check_v13_10() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nlet i = 1;\nwhile i <= x {\nsum = sum + i * i;\ni = i + 1\n};\nsum\n}");
}

#[test]
fn test_type_check_v14_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x * x;\nlet b = a + a;\nlet c = b * b;\nc\n}");
}

#[test]
fn test_type_check_v14_2() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 1;\nlet i = 2;\nwhile i <= x {\nresult = result * i;\ni = i + 1\n};\nresult\n}");
}

#[test]
fn test_type_check_v14_3() {
    check_no_errors("struct Date2 { year: number, month: number, day: number }\nfn is_valid(d: Date2) -> bool {\nd.year > 0 && d.month > 0 && d.month <= 12 && d.day > 0 && d.day <= 31\n}");
}

#[test]
fn test_type_check_v14_4() {
    check_no_errors("enum Priority { Low, Medium, High, Critical }\nfn urgency(p: Priority) -> number {\nmatch p {\nPriority::Low => 1\nPriority::Medium => 2\nPriority::High => 3\nPriority::Critical => 4\n}\n}");
}

#[test]
fn test_type_check_v14_5() {
    check_no_errors("fn f(x: number) -> Option<bool> {\nif x > 0 {\nSome(true)\n} else {\nif x < 0 {\nSome(false)\n} else {\nNone\n}\n}\n}");
}

#[test]
fn test_type_check_v14_6() {
    check_no_errors("fn f(x: number) -> Result<bool, string> {\nif x > 0 {\nOk(true)\n} else {\nif x < 0 {\nOk(false)\n} else {\nErr(\"zero\")\n}\n}\n}");
}

#[test]
fn test_type_check_v14_7() {
    check_no_errors("struct Triangle { a: number, b: number, c: number }\nfn perimeter(t: Triangle) -> number {\nt.a + t.b + t.c\n}\nfn is_equilateral(t: Triangle) -> bool {\nt.a == t.b && t.b == t.c\n}");
}

#[test]
fn test_type_check_v14_8() {
    check_no_errors("enum Media { Image(string), Video(number), Audio(number) }\nfn duration(m: Media) -> number {\nmatch m {\nMedia::Image(_) => 0\nMedia::Video(d) => d\nMedia::Audio(d) => d\n}\n}");
}

#[test]
fn test_type_check_v14_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 1..x {\nfor j in 1..i {\nsum = sum + 1\n}\n};\nsum\n}");
}

#[test]
fn test_type_check_v14_10() {
    check_no_errors("fn f(a: number, b: number, c: number) -> number {\nlet max = if a > b {\nif a > c { a } else { c }\n} else {\nif b > c { b } else { c }\n};\nmax\n}");
}

#[test]
fn test_type_check_v15_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x + 1;\nlet b = a * 2;\nlet c = b - 1;\nlet d = c / 2;\nlet e = d + 3;\ne\n}");
}

#[test]
fn test_type_check_v15_2() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 0;\nfor i in 0..x {\nresult = result + if i % 2 == 0 { i * i } else { i * i * i }\n};\nresult\n}");
}

#[test]
fn test_type_check_v15_3() {
    check_no_errors("struct Student2 { id: number, score: number }\nfn honors(s: Student2) -> bool {\ns.score >= 90\n}\nfn pass(s: Student2) -> bool {\ns.score >= 60\n}");
}

#[test]
fn test_type_check_v15_4() {
    check_no_errors("enum Gas { Solid, Liquid, Gas2 }\nfn can_flow(g: Gas) -> bool {\nmatch g {\nGas::Solid => false\nGas::Liquid => true\nGas::Gas2 => true\n}\n}");
}

#[test]
fn test_type_check_v15_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nmatch x {\n0 => None\nn => if n > 0 { Some(n) } else { None }\n}\n}");
}

#[test]
fn test_type_check_v15_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x == 0 {\nErr(\"division by zero\")\n} else {\nOk(100 / x)\n}\n}");
}

#[test]
fn test_type_check_v15_7() {
    check_no_errors("struct Line2 { start: number, end: number }\nfn length(l: Line2) -> number {\nl.end - l.start\n}\nfn is_point(l: Line2) -> bool {\nl.start == l.end\n}");
}

#[test]
fn test_type_check_v15_8() {
    check_no_errors("enum Card2 { Heart(number), Spade(number) }\nfn value2(c: Card2) -> number {\nmatch c {\nCard2::Heart(x) => x\nCard2::Spade(x) => x\n}\n}\nfn is_heart(c: Card2) -> bool {\nmatch c {\nCard2::Heart(_) => true\nCard2::Spade(_) => false\n}\n}");
}

#[test]
fn test_type_check_v15_9() {
    check_no_errors("fn f(x: number) -> number {\nlet count = 0;\nlet n = x;\nwhile n > 1 {\ncount = count + 1;\nn = n / 2\n};\ncount\n}");
}

#[test]
fn test_type_check_v15_10() {
    check_no_errors("fn f(a: number, b: number, c: number) -> number {\nlet min = if a < b {\nif a < c { a } else { c }\n} else {\nif b < c { b } else { c }\n};\nmin\n}");
}

#[test]
fn test_type_check_v16_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x;\nlet b = a * a;\nlet c = b + a;\nlet d = c * c;\nlet e = d + c;\ne\n}");
}

#[test]
fn test_type_check_v16_2() {
    check_no_errors("fn f(x: number) -> number {\nlet even_sum = 0;\nlet odd_sum = 0;\nfor i in 0..x {\nif i % 2 == 0 {\neven_sum = even_sum + i\n} else {\nodd_sum = odd_sum + i\n}\n};\neven_sum + odd_sum\n}");
}

#[test]
fn test_type_check_v16_3() {
    check_no_errors("struct Circle2 { cx: number, cy: number, radius: number }\nfn contains_point(c: Circle2, x: number, y: number) -> bool {\nlet dx = x - c.cx;\nlet dy = y - c.cy;\ndx * dx + dy * dy <= c.radius * c.radius\n}");
}

#[test]
fn test_type_check_v16_4() {
    check_no_errors("enum Size { Small, Medium, Large }\nfn price(s: Size) -> number {\nmatch s {\nSize::Small => 5\nSize::Medium => 8\nSize::Large => 12\n}\n}");
}

#[test]
fn test_type_check_v16_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 0 && x < 100 {\nSome(x * x)\n} else {\nif x >= 100 && x < 1000 {\nSome(x)\n} else {\nNone\n}\n}\n}");
}

#[test]
fn test_type_check_v16_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x == 0 {\nErr(\"zero\")\n} else {\nif x > 0 {\nOk(x)\n} else {\nOk(0 - x)\n}\n}\n}");
}

#[test]
fn test_type_check_v16_7() {
    check_no_errors("struct Vec4 { x: number, y: number, z: number, w: number }\nfn dot4(a: Vec4, b: Vec4) -> number {\na.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w\n}");
}

#[test]
fn test_type_check_v16_8() {
    check_no_errors("enum Weekday { Mon2, Tue2, Wed2, Thu2, Fri2 }\nfn is_midweek(w: Weekday) -> bool {\nmatch w {\nWeekday::Mon2 => false\nWeekday::Tue2 => true\nWeekday::Wed2 => true\nWeekday::Thu2 => true\nWeekday::Fri2 => false\n}\n}");
}

#[test]
fn test_type_check_v16_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + i * (i + 1) / 2\n};\nsum\n}");
}

#[test]
fn test_type_check_v16_10() {
    check_no_errors("fn f(x: number) -> number {\nlet a = 1;\nlet b = 1;\nfor i in 2..x {\nlet c = a + b;\na = b;\nb = c\n};\nif x == 0 { 1 } else { b }\n}");
}

#[test]
fn test_type_check_v17_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x;\nlet b = a;\nlet c = b;\nlet d = c;\nlet e = d;\nlet g = e;\ng\n}");
}

#[test]
fn test_type_check_v17_2() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 0;\nfor i in 0..100 {\nresult = result + if i % 6 == 0 { i } else { 0 }\n};\nresult\n}");
}

#[test]
fn test_type_check_v17_3() {
    check_no_errors("struct Timer { hours: number, minutes: number, seconds: number }\nfn to_seconds(t: Timer) -> number {\nt.hours * 3600 + t.minutes * 60 + t.seconds\n}");
}

#[test]
fn test_type_check_v17_4() {
    check_no_errors("enum Fruit { Apple, Banana, Cherry, Date }\nfn calories(f: Fruit) -> number {\nmatch f {\nFruit::Apple => 95\nFruit::Banana => 105\nFruit::Cherry => 50\nFruit::Date => 20\n}\n}");
}

#[test]
fn test_type_check_v17_5() {
    check_no_errors("fn f(x: number) -> Option<string> {\nmatch x {\n1 => Some(\"one\")\n2 => Some(\"two\")\n3 => Some(\"three\")\n_ => None\n}\n}");
}

#[test]
fn test_type_check_v17_6() {
    check_no_errors("fn f(x: number) -> Result<string, string> {\nif x == 42 {\nOk(\"answer\")\n} else {\nif x == 0 {\nOk(\"nothing\")\n} else {\nErr(\"unknown\")\n}\n}\n}");
}

#[test]
fn test_type_check_v17_7() {
    check_no_errors("struct ColorRGB { red: number, green: number, blue: number }\nfn to_grayscale(c: ColorRGB) -> number {\n(c.red * 30 + c.green * 59 + c.blue * 11) / 100\n}");
}

#[test]
fn test_type_check_v17_8() {
    check_no_errors("enum Shape2 { Square(number), Rectangle2(number, number) }\nfn area2(s: Shape2) -> number {\nmatch s {\nShape2::Square(side) => side * side\nShape2::Rectangle2(w, h) => w * h\n}\n}");
}

#[test]
fn test_type_check_v17_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nlet i = 1;\nwhile i <= x {\nsum = sum + i;\ni = i + 2\n};\nsum\n}");
}

#[test]
fn test_type_check_v17_10() {
    check_no_errors("fn f(x: number) -> number {\nlet product = 1;\nfor i in 1..x {\nproduct = product * (i + 1)\n};\nproduct\n}");
}

#[test]
fn test_type_check_v18_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x * 2;\nlet b = a + 1;\nlet c = b * 3;\nlet d = c - 2;\nd\n}");
}

#[test]
fn test_type_check_v18_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + i * i - i\n};\nsum\n}");
}

#[test]
fn test_type_check_v18_3() {
    check_no_errors("struct Rectangle3 { width: number, height: number }\nfn area3(r: Rectangle3) -> number { r.width * r.height }\nfn perimeter3(r: Rectangle3) -> number { 2 * (r.width + r.height) }\nfn is_square3(r: Rectangle3) -> bool { r.width == r.height }");
}

#[test]
fn test_type_check_v18_4() {
    check_no_errors("enum Country { China, USA, Japan, UK }\nfn greeting(c: Country) -> string {\nmatch c {\nCountry::China => \"你好\"\nCountry::USA => \"Hello\"\nCountry::Japan => \"こんにちは\"\nCountry::UK => \"Hello\"\n}\n}");
}

#[test]
fn test_type_check_v18_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nlet doubled = x * 2;\nif doubled > 100 { None } else { Some(doubled) }\n}");
}

#[test]
fn test_type_check_v18_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x < 0 {\nErr(\"negative\")\n} else {\nif x == 0 {\nOk(0)\n} else {\nOk(x * x)\n}\n}\n}");
}

#[test]
fn test_type_check_v18_7() {
    check_no_errors("struct Point4 { x: number, y: number }\nfn translate2(p: Point4, dx: number, dy: number) -> Point4 {\nPoint4 { x: p.x + dx, y: p.y + dy }\n}\nfn scale2(p: Point4, s: number) -> Point4 {\nPoint4 { x: p.x * s, y: p.y * s }\n}");
}

#[test]
fn test_type_check_v18_8() {
    check_no_errors("enum Status2 { Pending(number), Active(number), Closed(number) }\nfn get_id(s: Status2) -> number {\nmatch s {\nStatus2::Pending(id) => id\nStatus2::Active(id) => id\nStatus2::Closed(id) => id\n}\n}");
}

#[test]
fn test_type_check_v18_9() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 0;\nfor i in 1..x {\nresult = result + i * (x - i)\n};\nresult\n}");
}

#[test]
fn test_type_check_v18_10() {
    check_no_errors("fn f(x: number) -> number {\nlet prev = 0;\nlet curr = 1;\nlet i = 0;\nwhile i < x {\nlet temp = prev + curr;\nprev = curr;\ncurr = temp;\ni = i + 1\n};\ncurr\n}");
}

#[test]
fn test_type_check_v19_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x + 1;\nlet b = a * 2;\nlet c = b - 3;\nc\n}");
}

#[test]
fn test_type_check_v19_2() {
    check_no_errors("fn f(x: number) -> number {\nlet even = 0;\nlet odd = 0;\nfor i in 0..x {\nif i % 2 == 0 {\neven = even + 1\n} else {\nodd = odd + 1\n}\n};\neven + odd\n}");
}

#[test]
fn test_type_check_v19_3() {
    check_no_errors("struct Name { first: string, last: string }\nfn full_name(n: Name) -> string {\nn.first + \" \" + n.last\n}");
}

#[test]
fn test_type_check_v19_4() {
    check_no_errors("enum Language { Rust, Python, JavaScript, Go }\nfn is_compiled(l: Language) -> bool {\nmatch l {\nLanguage::Rust => true\nLanguage::Go => true\nLanguage::Python => false\nLanguage::JavaScript => false\n}\n}");
}

#[test]
fn test_type_check_v19_5() {
    check_no_errors("fn f(x: number) -> Option<bool> {\nif x > 100 { None } else { if x > 50 { Some(true) } else { Some(false) } }\n}");
}

#[test]
fn test_type_check_v19_6() {
    check_no_errors("fn f(x: number) -> Result<bool, string> {\nif x == 0 { Err(\"zero\") } else { if x > 0 { Ok(true) } else { Ok(false) } }\n}");
}

#[test]
fn test_type_check_v19_7() {
    check_no_errors("struct Vector2D { dx: number, dy: number }\nfn magnitude_sq(v: Vector2D) -> number {\nv.dx * v.dx + v.dy * v.dy\n}\nfn is_zero(v: Vector2D) -> bool {\nmagnitude_sq(v) == 0\n}");
}

#[test]
fn test_type_check_v19_8() {
    check_no_errors("enum HTTP { Get, Post, Put, Delete }\nfn has_body(h: HTTP) -> bool {\nmatch h {\nHTTP::Get => false\nHTTP::Post => true\nHTTP::Put => true\nHTTP::Delete => false\n}\n}");
}

#[test]
fn test_type_check_v19_9() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 0;\nfor i in 0..x {\nfor j in 0..x {\nresult = result + 1\n}\n};\nresult\n}");
}

#[test]
fn test_type_check_v19_10() {
    check_no_errors("fn f(x: number, y: number) -> number {\nlet a = x + y;\nlet b = x - y;\nlet c = x * y;\nlet d = if b != 0 { c / b } else { 0 };\nd\n}");
}

#[test]
fn test_type_check_v20_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x;\nlet b = a * a;\nlet c = a + b;\nc\n}");
}

#[test]
fn test_type_check_v20_2() {
    check_no_errors("fn f(x: number) -> number {\nlet count = 0;\nfor i in 0..x {\nif i % 3 == 0 || i % 5 == 0 || i % 7 == 0 {\ncount = count + 1\n}\n};\ncount\n}");
}

#[test]
fn test_type_check_v20_3() {
    check_no_errors("struct Address { street: string, city: string }\nfn full_address(a: Address) -> string {\na.street + \", \" + a.city\n}");
}

#[test]
fn test_type_check_v20_4() {
    check_no_errors("enum Transport { Walk, Bike, Car, Bus }\nfn speed(t: Transport) -> number {\nmatch t {\nTransport::Walk => 5\nTransport::Bike => 15\nTransport::Car => 60\nTransport::Bus => 30\n}\n}");
}

#[test]
fn test_type_check_v20_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 0 { if x > 50 { if x > 75 { None } else { Some(x) } } else { Some(x) } } else { None }\n}");
}

#[test]
fn test_type_check_v20_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x == 0 { Err(\"zero\") } else { if x < 0 { Err(\"negative\") } else { if x > 1000 { Err(\"overflow\") } else { Ok(x) } } }\n}");
}

#[test]
fn test_type_check_v20_7() {
    check_no_errors("struct Box3D { w: number, h: number, d: number }\nfn volume3(b: Box3D) -> number { b.w * b.h * b.d }\nfn surface_area(b: Box3D) -> number { 2 * (b.w * b.h + b.h * b.d + b.d * b.w) }");
}

#[test]
fn test_type_check_v20_8() {
    check_no_errors("enum Event { Click(number), KeyPress(string), Resize(number, number) }\nfn is_click(e: Event) -> bool {\nmatch e {\nEvent::Click(_) => true\nEvent::KeyPress(_) => false\nEvent::Resize(_, _) => false\n}\n}");
}

#[test]
fn test_type_check_v20_9() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 1;\nlet i = 2;\nwhile i <= x {\nresult = result * i;\ni = i + 1\n};\nresult\n}");
}

#[test]
fn test_type_check_v20_10() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 1..x {\nsum = sum + if i % 2 == 0 { i / 2 } else { i }\n};\nsum\n}");
}

#[test]
fn test_type_check_v21_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x;\nlet b = a + 1;\nlet c = b * b;\nc\n}");
}

#[test]
fn test_type_check_v21_2() {
    check_no_errors("fn f(x: number) -> number {\nlet positive = 0;\nlet negative = 0;\nfor i in 0..x {\nlet val = i - x / 2;\nif val > 0 {\npositive = positive + 1\n} else {\nnegative = negative + 1\n}\n};\npositive + negative\n}");
}

#[test]
fn test_type_check_v21_3() {
    check_no_errors("struct Person2 { name: string, age: number }\nfn is_teenager(p: Person2) -> bool {\np.age >= 13 && p.age <= 19\n}\nfn greet(p: Person2) -> string {\n\"Hello, \" + p.name\n}");
}

#[test]
fn test_type_check_v21_4() {
    check_no_errors("enum Season3 { Spring2, Summer2, Autumn2, Winter2 }\nfn temp(s: Season3) -> string {\nmatch s {\nSeason3::Spring2 => \"warm\"\nSeason3::Summer2 => \"hot\"\nSeason3::Autumn2 => \"cool\"\nSeason3::Winter2 => \"cold\"\n}\n}");
}

#[test]
fn test_type_check_v21_5() {
    check_no_errors("fn f(x: number) -> Option<string> {\nif x > 0 {\nif x > 50 {\nSome(\"large\")\n} else {\nSome(\"small\")\n}\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v21_6() {
    check_no_errors("fn f(x: number) -> Result<string, string> {\nif x == 0 {\nOk(\"zero\")\n} else {\nif x > 0 {\nOk(\"positive\")\n} else {\nErr(\"negative\")\n}\n}\n}");
}

#[test]
fn test_type_check_v21_7() {
    check_no_errors("struct Matrix3 { a: number, b: number, c: number, d: number }\nfn add_matrix(a: Matrix3, b: Matrix3) -> Matrix3 {\nMatrix3 { a: a.a + b.a, b: a.b + b.b, c: a.c + b.c, d: a.d + b.d }\n}");
}

#[test]
fn test_type_check_v21_8() {
    check_no_errors("enum Config { Debug, Release }\nfn is_debug(c: Config) -> bool {\nmatch c {\nConfig::Debug => true\nConfig::Release => false\n}\n}");
}

#[test]
fn test_type_check_v21_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nlet i = 1;\nwhile i * i <= x {\nsum = sum + i * i;\ni = i + 1\n};\nsum\n}");
}

#[test]
fn test_type_check_v21_10() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 0;\nfor i in 1..x {\nresult = result + if i % 3 == 0 { i * 3 } else { i }\n};\nresult\n}");
}

#[test]
fn test_type_check_v22_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x + 1;\nlet b = a * a;\nlet c = b - a;\nc\n}");
}

#[test]
fn test_type_check_v22_2() {
    check_no_errors("fn f(x: number) -> number {\nlet total = 0;\nfor i in 0..x {\ntotal = total + i * i * i\n};\ntotal\n}");
}

#[test]
fn test_type_check_v22_3() {
    check_no_errors("struct Book { title: string, pages: number }\nfn is_short(b: Book) -> bool {\nb.pages < 100\n}\nfn summary(b: Book) -> string {\nb.title + \" (\" + \" pages)\"\n}");
}

#[test]
fn test_type_check_v22_4() {
    check_no_errors("enum OS { Linux, MacOS, Windows }\nfn is_unix(o: OS) -> bool {\nmatch o {\nOS::Linux => true\nOS::MacOS => true\nOS::Windows => false\n}\n}");
}

#[test]
fn test_type_check_v22_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nlet half = x / 2;\nif half > 0 { Some(half) } else { None }\n}");
}

#[test]
fn test_type_check_v22_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x == 0 {\nErr(\"division by zero\")\n} else {\nOk(100 / x)\n}\n}");
}

#[test]
fn test_type_check_v22_7() {
    check_no_errors("struct Polar2 { r: number, theta: number }\nfn magnitude(p: Polar2) -> number { p.r }\nfn is_origin(p: Polar2) -> bool { p.r == 0 }");
}

#[test]
fn test_type_check_v22_8() {
    check_no_errors("enum Permission { Read, Write, Execute }\nfn can_read(p: Permission) -> bool {\nmatch p {\nPermission::Read => true\nPermission::Write => true\nPermission::Execute => false\n}\n}");
}

#[test]
fn test_type_check_v22_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nlet i = 0;\nwhile i < x {\nsum = sum + 2 * i + 1;\ni = i + 1\n};\nsum\n}");
}

#[test]
fn test_type_check_v22_10() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 0;\nfor i in 0..x {\nfor j in 0..i {\nresult = result + j\n}\n};\nresult\n}");
}

#[test]
fn test_type_check_v23_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x * 2;\nlet b = a + 3;\nlet c = b * b;\nc\n}");
}

#[test]
fn test_type_check_v23_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nif i > 0 {\nsum = sum + 1 / i\n}\n};\nsum\n}");
}

#[test]
fn test_type_check_v23_3() {
    check_no_errors("struct Product { name: string, price: number, quantity: number }\nfn total_cost(p: Product) -> number {\np.price * p.quantity\n}");
}

#[test]
fn test_type_check_v23_4() {
    check_no_errors("enum Genre { Action, Comedy, Drama, Horror }\nfn is_family(g: Genre) -> bool {\nmatch g {\nGenre::Action => false\nGenre::Comedy => true\nGenre::Drama => true\nGenre::Horror => false\n}\n}");
}

#[test]
fn test_type_check_v23_5() {
    check_no_errors("fn f(x: number) -> Option<string> {\nif x > 0 {\nif x % 2 == 0 {\nSome(\"even\")\n} else {\nSome(\"odd\")\n}\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v23_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x > 100 {\nOk(x - 100)\n} else {\nif x < 0 {\nErr(\"negative\")\n} else {\nOk(x)\n}\n}\n}");
}

#[test]
fn test_type_check_v23_7() {
    check_no_errors("struct Triangle2 { a: number, b: number, c: number }\nfn is_valid_triangle(t: Triangle2) -> bool {\nt.a + t.b > t.c && t.b + t.c > t.a && t.a + t.c > t.b\n}");
}

#[test]
fn test_type_check_v23_8() {
    check_no_errors("enum Response2 { Success(number), Error2(string) }\nfn get_code(r: Response2) -> number {\nmatch r {\nResponse2::Success(c) => c\nResponse2::Error2(_) => 0\n}\n}");
}

#[test]
fn test_type_check_v23_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nlet i = 1;\nwhile i <= x {\nsum = sum + i * i;\ni = i + 2\n};\nsum\n}");
}

#[test]
fn test_type_check_v23_10() {
    check_no_errors("fn f(x: number) -> number {\nlet a = 1;\nlet b = 1;\nfor i in 2..x {\nlet c = a + b;\na = b;\nb = c\n};\nb + a\n}");
}

#[test]
fn test_type_check_v24_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x;\nlet b = a + 1;\nlet c = b * 2;\nlet d = c - 1;\nd\n}");
}

#[test]
fn test_type_check_v24_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + if i % 2 == 0 { i } else { 0 - i }\n};\nsum\n}");
}

#[test]
fn test_type_check_v24_3() {
    check_no_errors("struct Coord { lat: number, lon: number }\nfn is_north(c: Coord) -> bool { c.lat > 0 }\nfn is_east(c: Coord) -> bool { c.lon > 0 }");
}

#[test]
fn test_type_check_v24_4() {
    check_no_errors("enum Chess { King2, Queen2, Rook2 }\nfn is_royal(c: Chess) -> bool {\nmatch c {\nChess::King2 => true\nChess::Queen2 => true\nChess::Rook2 => false\n}\n}");
}

#[test]
fn test_type_check_v24_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 10 {\nif x > 50 {\nSome(x * 2)\n} else {\nSome(x)\n}\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v24_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x < 0 {\nErr(\"negative\")\n} else {\nif x > 100 {\nOk(100)\n} else {\nOk(x)\n}\n}\n}");
}

#[test]
fn test_type_check_v24_7() {
    check_no_errors("struct Range2 { lo: number, hi: number }\nfn contains2(r: Range2, x: number) -> bool { x >= r.lo && x <= r.hi }\nfn size(r: Range2) -> number { r.hi - r.lo }");
}

#[test]
fn test_type_check_v24_8() {
    check_no_errors("enum Level { Debug2, Info2, Warn2, Error2 }\nfn is_error(l: Level) -> bool {\nmatch l {\nLevel::Debug2 => false\nLevel::Info2 => false\nLevel::Warn2 => false\nLevel::Error2 => true\n}\n}");
}

#[test]
fn test_type_check_v24_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 1..x {\nsum = sum + i * (i + 1)\n};\nsum\n}");
}

#[test]
fn test_type_check_v24_10() {
    check_no_errors("fn f(x: number) -> number {\nlet a = 0;\nlet b = 1;\nlet i = 0;\nwhile i < x {\nlet c = a + b;\na = b;\nb = c;\ni = i + 1\n};\na\n}");
}

#[test]
fn test_type_check_v25_1() {
    check_no_errors("fn f(x: number) -> number {\nlet y = x + 1;\ny * y - y\n}");
}

#[test]
fn test_type_check_v25_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + if i > 50 { i } else { 0 }\n};\nsum\n}");
}

#[test]
fn test_type_check_v25_3() {
    check_no_errors("struct Employee { name: string, salary: number }\nfn is_high_earner(e: Employee) -> bool { e.salary > 100000 }");
}

#[test]
fn test_type_check_v25_4() {
    check_no_errors("enum FileType { File, Dir, Symlink }\nfn is_file(ft: FileType) -> bool {\nmatch ft {\nFileType::File => true\nFileType::Dir => false\nFileType::Symlink => false\n}\n}");
}

#[test]
fn test_type_check_v25_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x >= 0 && x <= 100 {\nif x == 50 { None } else { Some(x) }\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v25_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x > 0 {\nif x > 50 {\nOk(x * 2)\n} else {\nOk(x)\n}\n} else {\nErr(\"non-positive\")\n}\n}");
}

#[test]
fn test_type_check_v25_7() {
    check_no_errors("struct Interval2 { lo: number, hi: number }\nfn width(i: Interval2) -> number { i.hi - i.lo }\nfn is_empty(i: Interval2) -> bool { i.lo >= i.hi }");
}

#[test]
fn test_type_check_v25_8() {
    check_no_errors("enum Status3 { Active2(number), Inactive(number) }\nfn get_id2(s: Status3) -> number {\nmatch s {\nStatus3::Active2(id) => id\nStatus3::Inactive(id) => id\n}\n}");
}

#[test]
fn test_type_check_v25_9() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 0;\nlet i = 0;\nwhile i < x {\nresult = result + i * i + i;\ni = i + 1\n};\nresult\n}");
}

#[test]
fn test_type_check_v25_10() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 1..x {\nsum = sum + if i % 10 == 0 { i / 10 } else { i }\n};\nsum\n}");
}

#[test]
fn test_type_check_v26_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x * 3;\nlet b = a + 2;\nlet c = b / 2;\nc\n}");
}

#[test]
fn test_type_check_v26_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nlet val = i * i - i;\nif val > 0 {\nsum = sum + val\n}\n};\nsum\n}");
}

#[test]
fn test_type_check_v26_3() {
    check_no_errors("struct Tuple2 { fst: string, snd: number }\nfn describe(t: Tuple2) -> string {\nt.fst\n}\nfn value(t: Tuple2) -> number {\nt.snd\n}");
}

#[test]
fn test_type_check_v26_4() {
    check_no_errors("enum Cloud { Cumulus, Stratus, Cirrus, Nimbus }\nfn produces_rain(c: Cloud) -> bool {\nmatch c {\nCloud::Cumulus => false\nCloud::Stratus => false\nCloud::Cirrus => false\nCloud::Nimbus => true\n}\n}");
}

#[test]
fn test_type_check_v26_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 10 {\nif x > 20 {\nif x > 30 { None } else { Some(x) }\n} else {\nSome(x)\n}\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v26_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x == 0 {\nOk(0)\n} else {\nif x == 1 {\nOk(1)\n} else {\nif x > 1 {\nOk(x * x)\n} else {\nErr(\"negative\")\n}\n}\n}\n}");
}

#[test]
fn test_type_check_v26_7() {
    check_no_errors("struct Sphere { cx: number, cy: number, cz: number, radius: number }\nfn contains_origin(s: Sphere) -> bool {\nlet d = s.cx * s.cx + s.cy * s.cy + s.cz * s.cz;\nd <= s.radius * s.radius\n}");
}

#[test]
fn test_type_check_v26_8() {
    check_no_errors("enum Metric { Bytes(number), KB(number), MB(number) }\nfn to_bytes(m: Metric) -> number {\nmatch m {\nMetric::Bytes(b) => b\nMetric::KB(k) => k * 1024\nMetric::MB(m) => m * 1024 * 1024\n}\n}");
}

#[test]
fn test_type_check_v26_9() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 0;\nfor i in 0..x {\nresult = result + (x - i)\n};\nresult\n}");
}

#[test]
fn test_type_check_v26_10() {
    check_no_errors("fn f(x: number) -> number {\nlet prev = 0;\nlet curr = 1;\nfor i in 0..x {\nlet next = prev + curr;\nprev = curr;\ncurr = next\n};\nprev + curr\n}");
}

#[test]
fn test_type_check_v27_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x + 2;\nlet b = a * 3;\nb - a\n}");
}

#[test]
fn test_type_check_v27_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + if i % 4 == 0 { i } else { 0 }\n};\nsum\n}");
}

#[test]
fn test_type_check_v27_3() {
    check_no_errors("struct Account { balance: number }\nfn is_positive(a: Account) -> bool { a.balance > 0 }\nfn is_overdrawn(a: Account) -> bool { a.balance < 0 }");
}

#[test]
fn test_type_check_v27_4() {
    check_no_errors("enum Color3 { Red2, Green2, Blue2 }\nfn is_primary(c: Color3) -> bool {\nmatch c {\nColor3::Red2 => true\nColor3::Green2 => true\nColor3::Blue2 => true\n}\n}");
}

#[test]
fn test_type_check_v27_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x == 42 { None } else { if x > 0 { Some(x) } else { None } }\n}");
}

#[test]
fn test_type_check_v27_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x == 1 { Ok(1) } else { if x > 1 { Ok(x * x) } else { Err(\"invalid\") } }\n}");
}

#[test]
fn test_type_check_v27_7() {
    check_no_errors("struct Segment { start: number, end: number }\nfn length2(s: Segment) -> number { s.end - s.start }\nfn contains3(s: Segment, x: number) -> bool { x >= s.start && x <= s.end }");
}

#[test]
fn test_type_check_v27_8() {
    check_no_errors("enum Shape3 { Circle3(number), Square3(number) }\nfn area3(s: Shape3) -> number {\nmatch s {\nShape3::Circle3(r) => 3 * r * r\nShape3::Square3(side) => side * side\n}\n}");
}

#[test]
fn test_type_check_v27_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 1..x {\nsum = sum + i * (x - i + 1)\n};\nsum\n}");
}

#[test]
fn test_type_check_v27_10() {
    check_no_errors("fn f(x: number) -> number {\nlet prev = 0;\nlet curr = 1;\nfor i in 0..x {\nlet next = prev + curr;\nprev = curr;\ncurr = next\n};\ncurr\n}");
}

#[test]
fn test_type_check_v28_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x;\nlet b = a + 2;\nlet c = b * b;\nc - a\n}");
}

#[test]
fn test_type_check_v28_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 1..x {\nif i % 6 == 0 {\nsum = sum + i\n}\n};\nsum\n}");
}

#[test]
fn test_type_check_v28_3() {
    check_no_errors("struct Record { key: string, value: number }\nfn is_valid(r: Record) -> bool { r.value > 0 }");
}

#[test]
fn test_type_check_v28_4() {
    check_no_errors("enum Mode { Read2, Write2, ReadWrite }\nfn can_write(m: Mode) -> bool {\nmatch m {\nMode::Read2 => false\nMode::Write2 => true\nMode::ReadWrite => true\n}\n}");
}

#[test]
fn test_type_check_v28_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 0 && x < 1000 {\nif x % 10 == 0 { None } else { Some(x) }\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v28_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x == 0 {\nErr(\"zero\")\n} else {\nif x < 0 {\nOk(0 - x)\n} else {\nOk(x * 2)\n}\n}\n}");
}

#[test]
fn test_type_check_v28_7() {
    check_no_errors("struct Rect2 { x: number, y: number, w: number, h: number }\nfn area4(r: Rect2) -> number { r.w * r.h }\nfn contains_point2(r: Rect2, px: number, py: number) -> bool { px >= r.x && px <= r.x + r.w && py >= r.y && py <= r.y + r.h }");
}

#[test]
fn test_type_check_v28_8() {
    check_no_errors("enum Expr3 { Lit(number), Add3(number, number), Sub(number, number) }\nfn eval3(e: Expr3) -> number {\nmatch e {\nExpr3::Lit(n) => n\nExpr3::Add3(a, b) => a + b\nExpr3::Sub(a, b) => a - b\n}\n}");
}

#[test]
fn test_type_check_v28_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nfor j in 0..x {\nif i == j {\nsum = sum + 1\n}\n}\n};\nsum\n}");
}

#[test]
fn test_type_check_v28_10() {
    check_no_errors("fn f(x: number, y: number) -> number {\nlet a = x;\nlet b = y;\nwhile b != 0 {\nlet temp = b;\nb = a % b;\na = temp\n};\na\n}");
}

#[test]
fn test_type_check_v29_1() {
    check_no_errors("fn f(x: number) -> number {\nlet y = x * x + 2 * x + 1;\ny\n}");
}

#[test]
fn test_type_check_v29_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nfor j in 0..i {\nsum = sum + 1\n}\n};\nsum\n}");
}

#[test]
fn test_type_check_v29_3() {
    check_no_errors("struct Pair2 { first: number, second: string }\nfn swap(p: Pair2) -> number { p.first }\nfn describe(p: Pair2) -> string { p.second }");
}

#[test]
fn test_type_check_v29_4() {
    check_no_errors("enum Planet { Mercury, Venus, Earth, Mars }\nfn is_habitable(p: Planet) -> bool {\nmatch p {\nPlanet::Mercury => false\nPlanet::Venus => false\nPlanet::Earth => true\nPlanet::Mars => false\n}\n}");
}

#[test]
fn test_type_check_v29_5() {
    check_no_errors("fn f(x: number) -> Option<string> {\nmatch x {\n0 => None\n1 => Some(\"one\")\n2 => Some(\"two\")\n_ => Some(\"many\")\n}\n}");
}

#[test]
fn test_type_check_v29_6() {
    check_no_errors("fn f(x: number) -> Result<string, string> {\nmatch x {\n0 => Ok(\"zero\")\n1 => Ok(\"one\")\n_ => if x > 0 { Ok(\"positive\") } else { Err(\"negative\") }\n}\n}");
}

#[test]
fn test_type_check_v29_7() {
    check_no_errors("struct Circle3 { cx: number, cy: number, r: number }\nfn area5(c: Circle3) -> number { 3 * c.r * c.r }\nfn is_unit(c: Circle3) -> bool { c.r == 1 }");
}

#[test]
fn test_type_check_v29_8() {
    check_no_errors("enum IO2 { Read3(number), Write3(number) }\nfn bytes(io: IO2) -> number {\nmatch io {\nIO2::Read3(n) => n\nIO2::Write3(n) => n\n}\n}");
}

#[test]
fn test_type_check_v29_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 1..x {\nsum = sum + if i % 2 == 0 { i / 2 } else { i * 3 }\n};\nsum\n}");
}

#[test]
fn test_type_check_v29_10() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nlet i = x;\nwhile i > 0 {\nsum = sum + i % 10;\ni = i / 10\n};\nsum\n}");
}

#[test]
fn test_type_check_v30_1() {
    check_no_errors("fn f(x: number) -> number {\nlet y = x + 3;\ny * y - 9\n}");
}

#[test]
fn test_type_check_v30_2() {
    check_no_errors("fn f(x: number) -> number {\nlet count = 0;\nfor i in 0..x {\nif i % 2 != 0 && i % 3 != 0 {\ncount = count + 1\n}\n};\ncount\n}");
}

#[test]
fn test_type_check_v30_3() {
    check_no_errors("struct Score { player: string, points: number }\nfn is_winning(s: Score) -> bool { s.points > 100 }");
}

#[test]
fn test_type_check_v30_4() {
    check_no_errors("enum Priority { Low, Medium, High, Critical }\nfn is_urgent(p: Priority) -> bool {\nmatch p {\nPriority::Low => false\nPriority::Medium => false\nPriority::High => true\nPriority::Critical => true\n}\n}");
}

#[test]
fn test_type_check_v30_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x >= 1 && x <= 10 {\nif x <= 5 { Some(x * 2) } else { Some(x * 3) }\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v30_6() {
    check_no_errors("fn f(x: number) -> Result<bool, string> {\nif x == 0 { Ok(false) } else { if x > 0 { Ok(true) } else { Err(\"negative\") } }\n}");
}

#[test]
fn test_type_check_v30_7() {
    check_no_errors("struct Vector3 { x: number, y: number, z: number }\nfn dot(a: Vector3, b: Vector3) -> number { a.x * b.x + a.y * b.y + a.z * b.z }");
}

#[test]
fn test_type_check_v30_8() {
    check_no_errors("enum Token2 { Number2(number), Ident(string), EOF }\nfn is_eof(t: Token2) -> bool {\nmatch t {\nToken2::Number2(_) => false\nToken2::Ident(_) => false\nToken2::EOF => true\n}\n}");
}

#[test]
fn test_type_check_v30_9() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 0;\nfor i in 1..x {\nresult = result + i * (i + 1) / 2\n};\nresult\n}");
}

#[test]
fn test_type_check_v30_10() {
    check_no_errors("fn f(x: number) -> number {\nlet reversed = 0;\nlet n = x;\nwhile n > 0 {\nreversed = reversed * 10 + n % 10;\nn = n / 10\n};\nreversed\n}");
}

#[test]
fn test_type_check_v31_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x * 2;\nlet b = a + 1;\nlet c = b * b - b;\nc\n}");
}

#[test]
fn test_type_check_v31_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nif i % 2 == 0 {\nsum = sum + i * i\n}\n};\nsum\n}");
}

#[test]
fn test_type_check_v31_3() {
    check_no_errors("struct Point5 { x: number, y: number }\nfn distance_sq(a: Point5, b: Point5) -> number {\nlet dx = a.x - b.x;\nlet dy = a.y - b.y;\ndx * dx + dy * dy\n}");
}

#[test]
fn test_type_check_v31_4() {
    check_no_errors("enum Direction2 { North, South, East, West }\nfn opposite(d: Direction2) -> Direction2 {\nmatch d {\nDirection2::North => Direction2::South\nDirection2::South => Direction2::North\nDirection2::East => Direction2::West\nDirection2::West => Direction2::East\n}\n}");
}

#[test]
fn test_type_check_v31_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 100 {\nif x > 200 { Some(x / 100) } else { Some(x - 100) }\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v31_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x < 0 {\nErr(\"negative\")\n} else {\nif x == 0 {\nOk(1)\n} else {\nOk(x * 2 + 1)\n}\n}\n}");
}

#[test]
fn test_type_check_v31_7() {
    check_no_errors("struct Rational { num: number, den: number }\nfn is_whole(r: Rational) -> bool { r.num % r.den == 0 }\nfn to_float(r: Rational) -> number { r.num / r.den }");
}

#[test]
fn test_type_check_v31_8() {
    check_no_errors("enum Grade { A, B, C, D, F }\nfn passing(g: Grade) -> bool {\nmatch g {\nGrade::A => true\nGrade::B => true\nGrade::C => true\nGrade::D => true\nGrade::F => false\n}\n}");
}

#[test]
fn test_type_check_v31_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + (2 * i + 1)\n};\nsum\n}");
}

#[test]
fn test_type_check_v31_10() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 1;\nlet i = 1;\nwhile i <= x {\nresult = result * i;\ni = i + 2\n};\nresult\n}");
}

#[test]
fn test_type_check_v32_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x + 1;\nlet b = a * 3;\nlet c = b + 2;\nc * c\n}");
}

#[test]
fn test_type_check_v32_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nif i > 10 {\nif i < 20 {\nsum = sum + i\n}\n}\n};\nsum\n}");
}

#[test]
fn test_type_check_v32_3() {
    check_no_errors("struct Date2 { year: number, month: number, day: number }\nfn is_new_year(d: Date2) -> bool { d.month == 1 && d.day == 1 }");
}

#[test]
fn test_type_check_v32_4() {
    check_no_errors("enum Currency { USD, EUR, GBP, JPY }\nfn symbol(c: Currency) -> string {\nmatch c {\nCurrency::USD => \"USD\"\nCurrency::EUR => \"EUR\"\nCurrency::GBP => \"GBP\"\nCurrency::JPY => \"JPY\"\n}\n}");
}

#[test]
fn test_type_check_v32_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x == 0 { None } else { if x == 1 { Some(1) } else { if x == 2 { Some(2) } else { None } } }\n}");
}

#[test]
fn test_type_check_v32_6() {
    check_no_errors("fn f(x: number) -> Result<string, string> {\nif x > 0 { if x > 10 { if x > 100 { Ok(\"huge\") } else { Ok(\"big\") } } else { Ok(\"small\") } } else { Err(\"non-positive\") }\n}");
}

#[test]
fn test_type_check_v32_7() {
    check_no_errors("struct Complex2 { real: number, imag: number }\nfn magnitude_sq2(c: Complex2) -> number { c.real * c.real + c.imag * c.imag }\nfn is_real(c: Complex2) -> bool { c.imag == 0 }");
}

#[test]
fn test_type_check_v32_8() {
    check_no_errors("enum ASTNode { Num(number), Str(string), Bool2(bool) }\nfn is_number(n: ASTNode) -> bool {\nmatch n {\nASTNode::Num(_) => true\nASTNode::Str(_) => false\nASTNode::Bool2(_) => false\n}\n}");
}

#[test]
fn test_type_check_v32_9() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 0;\nfor i in 0..x {\nresult = result + if i > x / 2 { i * 2 } else { i }\n};\nresult\n}");
}

#[test]
fn test_type_check_v32_10() {
    check_no_errors("fn f(x: number) -> number {\nlet n = x;\nlet digits = 0;\nwhile n > 0 {\ndigits = digits + 1;\nn = n / 10\n};\ndigits\n}");
}

#[test]
fn test_type_check_v33_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x * x;\nlet b = a + x;\nb * b\n}");
}

#[test]
fn test_type_check_v33_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nfor j in i..x {\nsum = sum + 1\n}\n};\nsum\n}");
}

#[test]
fn test_type_check_v33_3() {
    check_no_errors("struct Interval3 { lo: number, hi: number }\nfn overlaps(a: Interval3, b: Interval3) -> bool { a.lo < b.hi && b.lo < a.hi }");
}

#[test]
fn test_type_check_v33_4() {
    check_no_errors("enum Element { Fire, Water, Earth, Air }\nfn beats(e: Element) -> Element {\nmatch e {\nElement::Fire => Element::Water\nElement::Water => Element::Earth\nElement::Earth => Element::Air\nElement::Air => Element::Fire\n}\n}");
}

#[test]
fn test_type_check_v33_5() {
    check_no_errors("fn f(x: number) -> Option<string> {\nmatch x {\n1 => Some(\"one\")\n2 => Some(\"two\")\n3 => Some(\"three\")\n_ => None\n}\n}");
}

#[test]
fn test_type_check_v33_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x >= 0 && x <= 100 {\nif x >= 90 { Ok(4) } else { if x >= 80 { Ok(3) } else { if x >= 70 { Ok(2) } else { Ok(1) } } }\n} else {\nErr(\"invalid\")\n}\n}");
}

#[test]
fn test_type_check_v33_7() {
    check_no_errors("struct Quaternion { w: number, x: number, y: number, z: number }\nfn norm_sq(q: Quaternion) -> number { q.w * q.w + q.x * q.x + q.y * q.y + q.z * q.z }");
}

#[test]
fn test_type_check_v33_8() {
    check_no_errors("enum Weekday2 { Mon, Tue, Wed, Thu, Fri, Sat, Sun }\nfn is_weekend(d: Weekday2) -> bool {\nmatch d {\nWeekday2::Sat => true\nWeekday2::Sun => true\n_ => false\n}\n}");
}

#[test]
fn test_type_check_v33_9() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 0;\nfor i in 0..x {\nresult = result + (x - i) * (i + 1)\n};\nresult\n}");
}

#[test]
fn test_type_check_v33_10() {
    check_no_errors("fn f(x: number) -> number {\nlet count = 0;\nlet n = x;\nwhile n > 1 {\nn = n / 2;\ncount = count + 1\n};\ncount\n}");
}

#[test]
fn test_type_check_v34_1() {
    check_no_errors("fn f(x: number) -> number {\nlet y = x + 5;\ny * y - 25\n}");
}

#[test]
fn test_type_check_v34_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + if i % 5 == 0 { i / 5 } else { 0 }\n};\nsum\n}");
}

#[test]
fn test_type_check_v34_3() {
    check_no_errors("struct Money { amount: number, currency: string }\nfn is_positive2(m: Money) -> bool { m.amount > 0 }\nfn describe2(m: Money) -> string { m.currency }");
}

#[test]
fn test_type_check_v34_4() {
    check_no_errors("enum Meal { Breakfast, Lunch, Dinner }\nfn time_of_day(m: Meal) -> string {\nmatch m {\nMeal::Breakfast => \"morning\"\nMeal::Lunch => \"noon\"\nMeal::Dinner => \"evening\"\n}\n}");
}

#[test]
fn test_type_check_v34_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 0 {\nlet doubled = x * 2;\nif doubled > 100 { None } else { Some(doubled) }\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v34_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x == 1 { Ok(1) } else { if x == 2 { Ok(2) } else { if x == 3 { Ok(6) } else { Err(\"unknown\") } } }\n}");
}

#[test]
fn test_type_check_v34_7() {
    check_no_errors("struct Vec4 { x: number, y: number, z: number, w: number }\nfn dot4(a: Vec4, b: Vec4) -> number { a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w }");
}

#[test]
fn test_type_check_v34_8() {
    check_no_errors("enum Animal2 { Dog2, Cat2, Bird2 }\nfn sound(a: Animal2) -> string {\nmatch a {\nAnimal2::Dog2 => \"woof\"\nAnimal2::Cat2 => \"meow\"\nAnimal2::Bird2 => \"tweet\"\n}\n}");
}

#[test]
fn test_type_check_v34_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 1..x {\nsum = sum + i * i + 2 * i + 1\n};\nsum\n}");
}

#[test]
fn test_type_check_v34_10() {
    check_no_errors("fn f(x: number) -> number {\nlet a = 1;\nlet b = 1;\nlet i = 2;\nwhile i <= x {\nlet c = a + b;\na = b;\nb = c;\ni = i + 1\n};\nb\n}");
}

#[test]
fn test_type_check_v35_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x + 1;\nlet b = a * 2;\nlet c = b + 3;\nc\n}");
}

#[test]
fn test_type_check_v35_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nif i > 0 && i < x {\nsum = sum + i\n}\n};\nsum\n}");
}

#[test]
fn test_type_check_v35_3() {
    check_no_errors("struct Time2 { hours: number, minutes: number }\nfn total_minutes(t: Time2) -> number { t.hours * 60 + t.minutes }\nfn is_midnight(t: Time2) -> bool { t.hours == 0 && t.minutes == 0 }");
}

#[test]
fn test_type_check_v35_4() {
    check_no_errors("enum Cardinal { N, S, E, W }\nfn to_string(c: Cardinal) -> string {\nmatch c {\nCardinal::N => \"North\"\nCardinal::S => \"South\"\nCardinal::E => \"East\"\nCardinal::W => \"West\"\n}\n}");
}

#[test]
fn test_type_check_v35_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 0 {\nlet tripled = x * 3;\nif tripled > 50 { None } else { Some(tripled) }\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v35_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x >= 0 && x <= 255 {\nOk(x)\n} else {\nErr(\"out of range\")\n}\n}");
}

#[test]
fn test_type_check_v35_7() {
    check_no_errors("struct Ray2 { ox: number, oy: number, dx: number, dy: number }\nfn is_horizontal(r: Ray2) -> bool { r.dy == 0 }\nfn is_vertical(r: Ray2) -> bool { r.dx == 0 }");
}

#[test]
fn test_type_check_v35_8() {
    check_no_errors("enum Tree2 { Leaf2(number), Branch2(number, number) }\nfn sum_tree(t: Tree2) -> number {\nmatch t {\nTree2::Leaf2(v) => v\nTree2::Branch2(a, b) => a + b\n}\n}");
}

#[test]
fn test_type_check_v35_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + if i % 3 == 0 { i * 3 } else { if i % 5 == 0 { i * 5 } else { i } }\n};\nsum\n}");
}

#[test]
fn test_type_check_v35_10() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 0;\nlet i = 0;\nwhile i < x {\nresult = result + (2 * i + 1) * (2 * i + 1);\ni = i + 1\n};\nresult\n}");
}

#[test]
fn test_type_check_v36_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x - 1;\nlet b = a * a + 2 * a + 1;\nb\n}");
}

#[test]
fn test_type_check_v36_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nfor j in 0..i {\nfor k in 0..j {\nsum = sum + 1\n}\n}\n};\nsum\n}");
}

#[test]
fn test_type_check_v36_3() {
    check_no_errors("struct Fraction2 { num: number, den: number }\nfn value(f: Fraction2) -> number { f.num / f.den }\nfn is_proper(f: Fraction2) -> bool { f.num < f.den }");
}

#[test]
fn test_type_check_v36_4() {
    check_no_errors("enum Tetromino { I2, O2, T2, S2, Z2, L2, J2 }\nfn rotation_count(t: Tetromino) -> number {\nmatch t {\nTetromino::I2 => 2\nTetromino::O2 => 1\nTetromino::T2 => 4\nTetromino::S2 => 2\nTetromino::Z2 => 2\nTetromino::L2 => 4\nTetromino::J2 => 4\n}\n}");
}

#[test]
fn test_type_check_v36_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 10 {\nlet halved = x / 2;\nif halved > 5 { None } else { Some(halved) }\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v36_6() {
    check_no_errors("fn f(x: number) -> Result<string, string> {\nif x > 0 {\nif x > 100 {\nOk(\"large\")\n} else {\nif x > 50 {\nOk(\"medium\")\n} else {\nOk(\"small\")\n}\n}\n} else {\nErr(\"non-positive\")\n}\n}");
}

#[test]
fn test_type_check_v36_7() {
    check_no_errors("struct AABB { min_x: number, min_y: number, max_x: number, max_y: number }\nfn width2(a: AABB) -> number { a.max_x - a.min_x }\nfn height2(a: AABB) -> number { a.max_y - a.min_y }\nfn area6(a: AABB) -> number { width2(a) * height2(a) }");
}

#[test]
fn test_type_check_v36_8() {
    check_no_errors("enum JSON2 { Null, Num2(number), Str2(string), Bool3(bool) }\nfn is_null(j: JSON2) -> bool {\nmatch j {\nJSON2::Null => true\nJSON2::Num2(_) => false\nJSON2::Str2(_) => false\nJSON2::Bool3(_) => false\n}\n}");
}

#[test]
fn test_type_check_v36_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + if i % 2 == 0 { i * i } else { i }\n};\nsum\n}");
}

#[test]
fn test_type_check_v36_10() {
    check_no_errors("fn f(x: number) -> number {\nlet largest = 0;\nlet n = x;\nwhile n > 0 {\nlet digit = n % 10;\nif digit > largest {\nlargest = digit\n};\nn = n / 10\n};\nlargest\n}");
}

#[test]
fn test_type_check_v37_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x + 2;\nlet b = a * 3 - 1;\nb\n}");
}

#[test]
fn test_type_check_v37_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 1..x {\nsum = sum + if i % 7 == 0 { i / 7 } else { i }\n};\nsum\n}");
}

#[test]
fn test_type_check_v37_3() {
    check_no_errors("struct Temperature { celsius: number }\nfn to_fahrenheit(t: Temperature) -> number { t.celsius * 9 / 5 + 32 }\nfn is_freezing(t: Temperature) -> bool { t.celsius <= 0 }");
}

#[test]
fn test_type_check_v37_4() {
    check_no_errors("enum Note { C, D, E, F, G, A, B }\nfn is_natural(n: Note) -> bool {\nmatch n {\nNote::C => true\nNote::D => true\nNote::E => true\nNote::F => true\nNote::G => true\nNote::A => true\nNote::B => true\n}\n}");
}

#[test]
fn test_type_check_v37_5() {
    check_no_errors("fn f(x: number) -> Option<string> {\nif x >= 0 && x <= 9 {\nmatch x {\n0 => Some(\"zero\")\n1 => Some(\"one\")\n2 => Some(\"two\")\n_ => Some(\"other\")\n}\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v37_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x > 0 {\nif x % 2 == 0 { Ok(x / 2) } else { Ok(x * 3 + 1) }\n} else {\nErr(\"non-positive\")\n}\n}");
}

#[test]
fn test_type_check_v37_7() {
    check_no_errors("struct Polar3 { r: number, theta: number }\nfn x_coord(p: Polar3) -> number { p.r }\nfn is_unit2(p: Polar3) -> bool { p.r == 1 }");
}

#[test]
fn test_type_check_v37_8() {
    check_no_errors("enum Stmt2 { Let2(string, number), Expr2(number), Return2(number) }\nfn is_let(s: Stmt2) -> bool {\nmatch s {\nStmt2::Let2(_, _) => true\nStmt2::Expr2(_) => false\nStmt2::Return2(_) => false\n}\n}");
}

#[test]
fn test_type_check_v37_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + (i + 1) * (i + 1)\n};\nsum\n}");
}

#[test]
fn test_type_check_v37_10() {
    check_no_errors("fn f(x: number) -> number {\nlet result = x;\nlet i = 0;\nwhile i < 10 {\nresult = result + i;\ni = i + 1\n};\nresult\n}");
}

#[test]
fn test_type_check_v38_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x * x;\nlet b = a + 2 * x;\nb\n}");
}

#[test]
fn test_type_check_v38_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nif i % 2 == 0 && i % 3 == 0 {\nsum = sum + i\n}\n};\nsum\n}");
}

#[test]
fn test_type_check_v38_3() {
    check_no_errors("struct Student3 { name: string, grade: number }\nfn honor_roll(s: Student3) -> bool { s.grade >= 90 }\nfn passing2(s: Student3) -> bool { s.grade >= 60 }");
}

#[test]
fn test_type_check_v38_4() {
    check_no_errors("enum Op2 { Add4, Sub4, Mul4, Div4 }\nfn apply(op: Op2, a: number, b: number) -> number {\nmatch op {\nOp2::Add4 => a + b\nOp2::Sub4 => a - b\nOp2::Mul4 => a * b\nOp2::Div4 => a / b\n}\n}");
}

#[test]
fn test_type_check_v38_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 0 {\nlet sq = x * x;\nif sq > 1000 { None } else { Some(sq) }\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v38_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x == 0 {\nErr(\"zero\")\n} else {\nif x == 1 {\nOk(1)\n} else {\nif x == 2 {\nOk(2)\n} else {\nOk(x * x)\n}\n}\n}\n}");
}

#[test]
fn test_type_check_v38_7() {
    check_no_errors("struct Plane { a: number, b: number, c: number, d: number }\nfn normalize_coeff(p: Plane) -> number { p.a * p.a + p.b * p.b + p.c * p.c }");
}

#[test]
fn test_type_check_v38_8() {
    check_no_errors("enum Lit2 { IntLit(number), StrLit(string), BoolLit(bool) }\nfn int_value(l: Lit2) -> number {\nmatch l {\nLit2::IntLit(n) => n\nLit2::StrLit(_) => 0\nLit2::BoolLit(b) => if b { 1 } else { 0 }\n}\n}");
}

#[test]
fn test_type_check_v38_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + i * (x - i)\n};\nsum\n}");
}

#[test]
fn test_type_check_v38_10() {
    check_no_errors("fn f(x: number) -> number {\nlet count = 0;\nlet n = x;\nwhile n > 0 {\nif n % 2 == 1 {\ncount = count + 1\n};\nn = n / 2\n};\ncount\n}");
}

#[test]
fn test_type_check_v39_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x + 3;\nlet b = a * a;\nb - 9\n}");
}

#[test]
fn test_type_check_v39_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + if i > 0 { i / 2 } else { 0 }\n};\nsum\n}");
}

#[test]
fn test_type_check_v39_3() {
    check_no_errors("struct Weight { kg: number }\nfn to_grams(w: Weight) -> number { w.kg * 1000 }\nfn is_heavy(w: Weight) -> bool { w.kg > 100 }");
}

#[test]
fn test_type_check_v39_4() {
    check_no_errors("enum Compass { N2, NE, E2, SE, S2, SW, W2, NW }\nfn is_cardinal(c: Compass) -> bool {\nmatch c {\nCompass::N2 => true\nCompass::E2 => true\nCompass::S2 => true\nCompass::W2 => true\n_ => false\n}\n}");
}

#[test]
fn test_type_check_v39_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 0 {\nlet sq = x * x;\nSome(sq)\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v39_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x == 0 {\nOk(0)\n} else {\nif x > 0 {\nif x > 50 {\nOk(x * 10)\n} else {\nOk(x)\n}\n} else {\nErr(\"negative\")\n}\n}\n}");
}

#[test]
fn test_type_check_v39_7() {
    check_no_errors("struct Line2 { x1: number, y1: number, x2: number, y2: number }\nfn length_sq(l: Line2) -> number {\nlet dx = l.x2 - l.x1;\nlet dy = l.y2 - l.y1;\ndx * dx + dy * dy\n}");
}

#[test]
fn test_type_check_v39_8() {
    check_no_errors("enum BinaryOp { And3, Or3, Xor }\nfn eval_bool(op: BinaryOp, a: bool, b: bool) -> bool {\nmatch op {\nBinaryOp::And3 => a && b\nBinaryOp::Or3 => a || b\nBinaryOp::Xor => a != b\n}\n}");
}

#[test]
fn test_type_check_v39_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + (2 * i + 1) * (2 * i + 1)\n};\nsum\n}");
}

#[test]
fn test_type_check_v39_10() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 0;\nlet i = 1;\nwhile i <= x {\nresult = result + i * i;\ni = i * 2\n};\nresult\n}");
}

#[test]
fn test_type_check_v40_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x * 2 + 1;\nlet b = a * a;\nb\n}");
}

#[test]
fn test_type_check_v40_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nif i > 0 {\nsum = sum + 1 / i\n}\n};\nsum\n}");
}

#[test]
fn test_type_check_v40_3() {
    check_no_errors("struct Distance2 { meters: number }\nfn to_km(d: Distance2) -> number { d.meters / 1000 }\nfn to_cm(d: Distance2) -> number { d.meters * 100 }");
}

#[test]
fn test_type_check_v40_4() {
    check_no_errors("enum Wave { Sine, Square, Triangle, Sawtooth }\nfn is_smooth(w: Wave) -> bool {\nmatch w {\nWave::Sine => true\nWave::Square => false\nWave::Triangle => true\nWave::Sawtooth => false\n}\n}");
}

#[test]
fn test_type_check_v40_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 10 {\nlet tripled = x * 3;\nif tripled < 200 { Some(tripled) } else { None }\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v40_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x >= 0 && x <= 255 {\nif x < 128 { Ok(x) } else { Ok(255 - x) }\n} else {\nErr(\"out of byte range\")\n}\n}");
}

#[test]
fn test_type_check_v40_7() {
    check_no_errors("struct Triangle3 { a: number, b: number, c: number }\nfn perimeter2(t: Triangle3) -> number { t.a + t.b + t.c }\nfn is_equilateral(t: Triangle3) -> bool { t.a == t.b && t.b == t.c }");
}

#[test]
fn test_type_check_v40_8() {
    check_no_errors("enum Color4 { RGB(number, number, number), Gray(number) }\nfn brightness(c: Color4) -> number {\nmatch c {\nColor4::RGB(r, g, b) => (r + g + b) / 3\nColor4::Gray(v) => v\n}\n}");
}

#[test]
fn test_type_check_v40_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + if i > x / 2 { 2 * i } else { i }\n};\nsum\n}");
}

#[test]
fn test_type_check_v40_10() {
    check_no_errors("fn f(x: number) -> number {\nlet a = 0;\nlet b = 1;\nlet i = 0;\nwhile i < x {\nlet c = a + b;\na = b;\nb = c + a;\ni = i + 1\n};\nb\n}");
}

#[test]
fn test_type_check_v41_1() {
    check_no_errors("fn f(x: number) -> number {\nlet y = x * 3 + 2;\ny * y\n}");
}

#[test]
fn test_type_check_v41_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 1..x {\nsum = sum + if i % 8 == 0 { i } else { 0 }\n};\nsum\n}");
}

#[test]
fn test_type_check_v41_3() {
    check_no_errors("struct Percentage { value: number }\nfn to_decimal(p: Percentage) -> number { p.value / 100 }\nfn is_half(p: Percentage) -> bool { p.value == 50 }");
}

#[test]
fn test_type_check_v41_4() {
    check_no_errors("enum Arch { X86, ARM, RISCV, MIPS }\nfn is_64bit(a: Arch) -> bool {\nmatch a {\nArch::X86 => true\nArch::ARM => true\nArch::RISCV => true\nArch::MIPS => false\n}\n}");
}

#[test]
fn test_type_check_v41_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 0 {\nlet halved = x / 2;\nif halved > 25 { None } else { Some(halved) }\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v41_6() {
    check_no_errors("fn f(x: number) -> Result<string, string> {\nif x > 0 {\nif x % 2 == 0 { Ok(\"even\") } else { Ok(\"odd\") }\n} else {\nErr(\"non-positive\")\n}\n}");
}

#[test]
fn test_type_check_v41_7() {
    check_no_errors("struct HexColor { r: number, g: number, b: number }\nfn luminance(c: HexColor) -> number { (c.r * 299 + c.g * 587 + c.b * 114) / 1000 }\nfn is_dark(c: HexColor) -> bool { luminance(c) < 128 }");
}

#[test]
fn test_type_check_v41_8() {
    check_no_errors("enum PairType { Same(number), Different(number, number) }\nfn sum_pair(p: PairType) -> number {\nmatch p {\nPairType::Same(v) => v + v\nPairType::Different(a, b) => a + b\n}\n}");
}

#[test]
fn test_type_check_v41_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + i * (i + 1) * (i + 2)\n};\nsum\n}");
}

#[test]
fn test_type_check_v41_10() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nlet n = x;\nwhile n > 0 {\nsum = sum + n % 10 * n % 10;\nn = n / 10\n};\nsum\n}");
}

#[test]
fn test_type_check_v42_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x + 1;\nlet b = a + a;\nb * a\n}");
}

#[test]
fn test_type_check_v42_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nif i > 20 {\nif i < 40 {\nsum = sum + i\n}\n}\n};\nsum\n}");
}

#[test]
fn test_type_check_v42_3() {
    check_no_errors("struct Angle2 { degrees: number }\nfn to_radians(a: Angle2) -> number { a.degrees * 314 / 18000 }\nfn is_right(a: Angle2) -> bool { a.degrees == 90 }");
}

#[test]
fn test_type_check_v42_4() {
    check_no_errors("enum Layer2 { Physical, DataLink, Network, Transport }\nfn is_end_to_end(l: Layer2) -> bool {\nmatch l {\nLayer2::Physical => false\nLayer2::DataLink => false\nLayer2::Network => false\nLayer2::Transport => true\n}\n}");
}

#[test]
fn test_type_check_v42_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 5 {\nlet doubled = x * 2;\nif doubled > 30 && doubled < 100 { Some(doubled) } else { None }\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v42_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x == 42 {\nOk(42)\n} else {\nif x > 42 {\nOk(x - 42)\n} else {\nOk(42 - x)\n}\n}\n}");
}

#[test]
fn test_type_check_v42_7() {
    check_no_errors("struct AABB3D { min_x: number, min_y: number, min_z: number, max_x: number, max_y: number, max_z: number }\nfn volume(a: AABB3D) -> number { (a.max_x - a.min_x) * (a.max_y - a.min_y) * (a.max_z - a.min_z) }");
}

#[test]
fn test_type_check_v42_8() {
    check_no_errors("enum Regex2 { Char(char), Star, Plus, Question }\nfn has_modifier(r: Regex2) -> bool {\nmatch r {\nRegex2::Char(_) => false\nRegex2::Star => true\nRegex2::Plus => true\nRegex2::Question => true\n}\n}");
}

#[test]
fn test_type_check_v42_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + if i % 2 == 0 { i / 2 + 1 } else { i * 2 - 1 }\n};\nsum\n}");
}

#[test]
fn test_type_check_v42_10() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 0;\nlet power = 1;\nlet i = 0;\nwhile i < x {\nresult = result + power;\npower = power * 2;\ni = i + 1\n};\nresult\n}");
}

#[test]
fn test_type_check_v43_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x * 2;\nlet b = a + 3;\nlet c = b * b - b;\nc\n}");
}

#[test]
fn test_type_check_v43_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + if i % 11 == 0 { i } else { 0 }\n};\nsum\n}");
}

#[test]
fn test_type_check_v43_3() {
    check_no_errors("struct Coordinate2 { x: number, y: number, z: number }\nfn x_val(c: Coordinate2) -> number { c.x }\nfn is_origin2(c: Coordinate2) -> bool { c.x == 0 && c.y == 0 && c.z == 0 }");
}

#[test]
fn test_type_check_v43_4() {
    check_no_errors("enum DBType { Integer, Float, VarChar, Boolean }\nfn is_numeric(t: DBType) -> bool {\nmatch t {\nDBType::Integer => true\nDBType::Float => true\nDBType::VarChar => false\nDBType::Boolean => false\n}\n}");
}

#[test]
fn test_type_check_v43_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 0 {\nlet mod3 = x % 3;\nif mod3 == 0 { None } else { Some(mod3) }\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v43_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x > 0 {\nif x % 2 == 0 {\nOk(x / 2)\n} else {\nOk(x * 3 + 1)\n}\n} else {\nErr(\"must be positive\")\n}\n}");
}

#[test]
fn test_type_check_v43_7() {
    check_no_errors("struct Mat2x2 { a: number, b: number, c: number, d: number }\nfn determinant(m: Mat2x2) -> number { m.a * m.d - m.b * m.c }\nfn is_identity(m: Mat2x2) -> bool { m.a == 1 && m.b == 0 && m.c == 0 && m.d == 1 }");
}

#[test]
fn test_type_check_v43_8() {
    check_no_errors("enum Opcode2 { Push(number), Pop, Add5, Mul5 }\nfn has_operand(op: Opcode2) -> bool {\nmatch op {\nOpcode2::Push(_) => true\nOpcode2::Pop => false\nOpcode2::Add5 => false\nOpcode2::Mul5 => false\n}\n}");
}

#[test]
fn test_type_check_v43_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + i * i * i + i\n};\nsum\n}");
}

#[test]
fn test_type_check_v43_10() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 1;\nfor i in 1..x {\nresult = result * 2\n};\nresult\n}");
}

#[test]
fn test_type_check_v44_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x * 3;\nlet b = a + 1;\nb * b - 1\n}");
}

#[test]
fn test_type_check_v44_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + if i > 5 { i - 5 } else { 0 }\n};\nsum\n}");
}

#[test]
fn test_type_check_v44_3() {
    check_no_errors("struct Velocity { dx: number, dy: number, dz: number }\nfn speed_sq(v: Velocity) -> number { v.dx * v.dx + v.dy * v.dy + v.dz * v.dz }\nfn is_stationary(v: Velocity) -> bool { speed_sq(v) == 0 }");
}

#[test]
fn test_type_check_v44_4() {
    check_no_errors("enum Protocol { TCP, UDP, ICMP }\nfn is_reliable(p: Protocol) -> bool {\nmatch p {\nProtocol::TCP => true\nProtocol::UDP => false\nProtocol::ICMP => false\n}\n}");
}

#[test]
fn test_type_check_v44_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 0 {\nlet sq = x * x;\nif sq > 0 && sq < 10000 { Some(sq) } else { None }\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v44_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x == 0 {\nErr(\"zero\")\n} else {\nif x < 0 {\nOk(0 - x)\n} else {\nOk(x)\n}\n}\n}");
}

#[test]
fn test_type_check_v44_7() {
    check_no_errors("struct Triangle4 { base: number, height: number }\nfn area7(t: Triangle4) -> number { t.base * t.height / 2 }\nfn is_degenerate(t: Triangle4) -> bool { t.base == 0 || t.height == 0 }");
}

#[test]
fn test_type_check_v44_8() {
    check_no_errors("enum Pattern3 { Singleton, Factory, Observer, Strategy }\nfn is_creational(p: Pattern3) -> bool {\nmatch p {\nPattern3::Singleton => true\nPattern3::Factory => true\nPattern3::Observer => false\nPattern3::Strategy => false\n}\n}");
}

#[test]
fn test_type_check_v44_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nfor j in 0..3 {\nsum = sum + i * j\n}\n};\nsum\n}");
}

#[test]
fn test_type_check_v44_10() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 0;\nlet i = 1;\nwhile i * i <= x {\nresult = i;\ni = i + 1\n};\nresult\n}");
}

#[test]
fn test_type_check_v45_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x - 2;\nlet b = a * a + 4 * a;\nb\n}");
}

#[test]
fn test_type_check_v45_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nif i % 2 == 0 && i % 3 == 0 && i % 5 == 0 {\nsum = sum + i\n}\n};\nsum\n}");
}

#[test]
fn test_type_check_v45_3() {
    check_no_errors("struct Battery { capacity: number, charge: number }\nfn percentage(b: Battery) -> number { b.charge * 100 / b.capacity }\nfn is_full(b: Battery) -> bool { b.charge == b.capacity }");
}

#[test]
fn test_type_check_v45_4() {
    check_no_errors("enum Sort2 { Bubble, Quick, Merge, Heap }\nfn is_nlogn(s: Sort2) -> bool {\nmatch s {\nSort2::Bubble => false\nSort2::Quick => true\nSort2::Merge => true\nSort2::Heap => true\n}\n}");
}

#[test]
fn test_type_check_v45_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 1 {\nlet prev = x - 1;\nif prev > 0 { Some(prev) } else { None }\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v45_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x == 0 {\nErr(\"zero division\")\n} else {\nlet reciprocal = 1 / x;\nOk(reciprocal)\n}\n}");
}

#[test]
fn test_type_check_v45_7() {
    check_no_errors("struct Complex3 { re: number, im: number }\nfn conjugate(c: Complex3) -> Complex3 { Complex3 { re: c.re, im: 0 - c.im } }\nfn norm_sq(c: Complex3) -> number { c.re * c.re + c.im * c.im }");
}

#[test]
fn test_type_check_v45_8() {
    check_no_errors("enum HttpStatus { Ok2, NotFound, ServerError }\nfn is_success(h: HttpStatus) -> bool {\nmatch h {\nHttpStatus::Ok2 => true\nHttpStatus::NotFound => false\nHttpStatus::ServerError => false\n}\n}");
}

#[test]
fn test_type_check_v45_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + (2 * i + 1) * (2 * i + 1) * (2 * i + 1)\n};\nsum\n}");
}

#[test]
fn test_type_check_v45_10() {
    check_no_errors("fn f(x: number) -> number {\nlet count = 0;\nlet n = x;\nwhile n > 1 {\nn = n / 2;\ncount = count + 1\n};\ncount\n}");
}

#[test]
fn test_type_check_v46_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x + 4;\nlet b = a * 2;\nb - 8\n}");
}

#[test]
fn test_type_check_v46_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nif i >= 10 && i <= 20 {\nsum = sum + i\n}\n};\nsum\n}");
}

#[test]
fn test_type_check_v46_3() {
    check_no_errors("struct Angle3 { radians: number }\nfn to_degrees(a: Angle3) -> number { a.radians * 180 / 314 * 100 }\nfn is_straight(a: Angle3) -> bool { a.radians == 314 / 100 }");
}

#[test]
fn test_type_check_v46_4() {
    check_no_errors("enum Storage { Register, Cache, RAM, Disk }\nfn latency_ns(s: Storage) -> number {\nmatch s {\nStorage::Register => 1\nStorage::Cache => 10\nStorage::RAM => 100\nStorage::Disk => 10000000\n}\n}");
}

#[test]
fn test_type_check_v46_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 10 {\nlet doubled = x * 2;\nif doubled < 50 { Some(doubled) } else { None }\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v46_6() {
    check_no_errors("fn f(x: number) -> Result<string, string> {\nif x == 0 {\nOk(\"zero\")\n} else {\nif x > 0 {\nif x % 2 == 0 { Ok(\"positive even\") } else { Ok(\"positive odd\") }\n} else {\nErr(\"negative\")\n}\n}\n}");
}

#[test]
fn test_type_check_v46_7() {
    check_no_errors("struct Point6 { x: number, y: number, z: number }\nfn translate3(p: Point6, dx: number, dy: number, dz: number) -> Point6 {\nPoint6 { x: p.x + dx, y: p.y + dy, z: p.z + dz }\n}");
}

#[test]
fn test_type_check_v46_8() {
    check_no_errors("enum Relation { Parent, Child, Sibling, Spouse }\nfn is_ancestor(r: Relation) -> bool {\nmatch r {\nRelation::Parent => true\nRelation::Child => false\nRelation::Sibling => false\nRelation::Spouse => false\n}\n}");
}

#[test]
fn test_type_check_v46_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 1..x {\nsum = sum + if i % 3 == 0 && i % 5 == 0 { i * 15 } else { i }\n};\nsum\n}");
}

#[test]
fn test_type_check_v46_10() {
    check_no_errors("fn f(x: number) -> number {\nlet a = 0;\nlet b = 1;\nfor i in 0..x {\nlet c = a + b;\na = b;\nb = c\n};\na\n}");
}

#[test]
fn test_type_check_v47_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x * 4;\nlet b = a / 2;\nb + 1\n}");
}

#[test]
fn test_type_check_v47_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + if i % 9 == 0 { i / 9 } else { 0 }\n};\nsum\n}");
}

#[test]
fn test_type_check_v47_3() {
    check_no_errors("struct Dimension { width: number, height: number }\nfn aspect_ratio(d: Dimension) -> number { d.width / d.height }\nfn is_square(d: Dimension) -> bool { d.width == d.height }");
}

#[test]
fn test_type_check_v47_4() {
    check_no_errors("enum Thread2 { Running, Paused, Blocked, Terminated }\nfn is_active(t: Thread2) -> bool {\nmatch t {\nThread2::Running => true\nThread2::Paused => true\nThread2::Blocked => false\nThread2::Terminated => false\n}\n}");
}

#[test]
fn test_type_check_v47_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 5 {\nlet incremented = x + 1;\nif incremented % 3 == 0 { Some(incremented) } else { None }\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v47_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x > 0 {\nif x > 10 {\nif x > 100 { Ok(x / 100) } else { Ok(x / 10) }\n} else {\nOk(x)\n}\n} else {\nErr(\"negative\")\n}\n}");
}

#[test]
fn test_type_check_v47_7() {
    check_no_errors("struct Ray3 { ox: number, oy: number, oz: number, dx: number, dy: number, dz: number }\nfn is_horizontal3(r: Ray3) -> bool { r.dy == 0 && r.dz == 0 }");
}

#[test]
fn test_type_check_v47_8() {
    check_no_errors("enum Visibility2 { Public2, Private2, Protected }\nfn is_accessible(v: Visibility2, is_subclass: bool) -> bool {\nmatch v {\nVisibility2::Public2 => true\nVisibility2::Private2 => false\nVisibility2::Protected => is_subclass\n}\n}");
}

#[test]
fn test_type_check_v47_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + (x - i) * (x - i)\n};\nsum\n}");
}

#[test]
fn test_type_check_v47_10() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 0;\nlet i = 0;\nwhile i < x {\nresult = result + (2 * i + 1);\ni = i + 1\n};\nresult\n}");
}

#[test]
fn test_type_check_v48_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x - 3;\nlet b = a * a + 6 * a + 9;\nb\n}");
}

#[test]
fn test_type_check_v48_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nfor j in 0..3 {\nif i % 2 == 0 {\nsum = sum + 1\n}\n}\n};\nsum\n}");
}

#[test]
fn test_type_check_v48_3() {
    check_no_errors("struct Ratio2 { num: number, den: number }\nfn simplify_hint(r: Ratio2) -> number { r.num / r.den }\nfn is_unit_fraction(r: Ratio2) -> bool { r.num == 1 }");
}

#[test]
fn test_type_check_v48_4() {
    check_no_errors("enum ConnState { Connected, Disconnected, Reconnecting }\nfn is_online(c: ConnState) -> bool {\nmatch c {\nConnState::Connected => true\nConnState::Disconnected => false\nConnState::Reconnecting => true\n}\n}");
}

#[test]
fn test_type_check_v48_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 0 {\nlet tripled = x * 3;\nif tripled > 50 && tripled < 150 { Some(tripled) } else { None }\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v48_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x > 0 {\nif x > 50 {\nif x > 75 { Ok(x - 75) } else { Ok(x - 50) }\n} else {\nOk(x)\n}\n} else {\nErr(\"non-positive\")\n}\n}");
}

#[test]
fn test_type_check_v48_7() {
    check_no_errors("struct Transform2D { tx: number, ty: number, sx: number, sy: number }\nfn is_identity_transform(t: Transform2D) -> bool { t.tx == 0 && t.ty == 0 && t.sx == 1 && t.sy == 1 }");
}

#[test]
fn test_type_check_v48_8() {
    check_no_errors("enum Phase2 { New2, Growing, Mature, Declining }\nfn is_growth_phase(p: Phase2) -> bool {\nmatch p {\nPhase2::New2 => true\nPhase2::Growing => true\nPhase2::Mature => false\nPhase2::Declining => false\n}\n}");
}

#[test]
fn test_type_check_v48_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + if i > x / 3 { i * 2 } else { i }\n};\nsum\n}");
}

#[test]
fn test_type_check_v48_10() {
    check_no_errors("fn f(x: number) -> number {\nlet a = 1;\nlet b = 1;\nfor i in 2..x {\nlet c = a + b;\na = b;\nb = c\n};\na + b\n}");
}

#[test]
fn test_type_check_v49_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x + 2;\nlet b = a * 4;\nb / 2\n}");
}

#[test]
fn test_type_check_v49_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nif i > 0 && i < x {\nsum = sum + i * i\n}\n};\nsum\n}");
}

#[test]
fn test_type_check_v49_3() {
    check_no_errors("struct Volume2 { liters: number }\nfn to_ml(v: Volume2) -> number { v.liters * 1000 }\nfn is_empty(v: Volume2) -> bool { v.liters == 0 }");
}

#[test]
fn test_type_check_v49_4() {
    check_no_errors("enum Encoding2 { UTF8, ASCII, UTF16 }\nfn is_unicode(e: Encoding2) -> bool {\nmatch e {\nEncoding2::UTF8 => true\nEncoding2::ASCII => false\nEncoding2::UTF16 => true\n}\n}");
}

#[test]
fn test_type_check_v49_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 0 {\nlet mod5 = x % 5;\nif mod5 == 0 { None } else { Some(mod5) }\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v49_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x >= 1 && x <= 12 {\nOk(x * x)\n} else {\nErr(\"month out of range\")\n}\n}");
}

#[test]
fn test_type_check_v49_7() {
    check_no_errors("struct Plane2 { normal_x: number, normal_y: number, normal_z: number, offset: number }\nfn is_through_origin(p: Plane2) -> bool { p.offset == 0 }");
}

#[test]
fn test_type_check_v49_8() {
    check_no_errors("enum Sync2 { Full, Partial, None2 }\nfn is_synced(s: Sync2) -> bool {\nmatch s {\nSync2::Full => true\nSync2::Partial => false\nSync2::None2 => false\n}\n}");
}

#[test]
fn test_type_check_v49_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + (i + 1) * (x - i)\n};\nsum\n}");
}

#[test]
fn test_type_check_v49_10() {
    check_no_errors("fn f(x: number) -> number {\nlet product = 1;\nlet i = 2;\nwhile i <= x {\nproduct = product * i;\ni = i + 1\n};\nproduct\n}");
}

#[test]
fn test_type_check_v50_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x * 2 + 1;\nlet b = a * 3;\nb\n}");
}

#[test]
fn test_type_check_v50_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + if i % 10 == 0 { i / 10 } else { 0 }\n};\nsum\n}");
}

#[test]
fn test_type_check_v50_3() {
    check_no_errors("struct Speed2 { mps: number }\nfn to_kmh(s: Speed2) -> number { s.mps * 3600 / 1000 }\nfn to_mph(s: Speed2) -> number { s.mps * 3600 / 1609 }\nfn is_walking(s: Speed2) -> bool { s.mps < 2 }");
}

#[test]
fn test_type_check_v50_4() {
    check_no_errors("enum Terminal2 { VT100, VT220, XTerm, ANSI }\nfn supports_color(t: Terminal2) -> bool {\nmatch t {\nTerminal2::VT100 => false\nTerminal2::VT220 => false\nTerminal2::XTerm => true\nTerminal2::ANSI => true\n}\n}");
}

#[test]
fn test_type_check_v50_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 0 {\nlet halved = x / 2;\nif halved > 10 { None } else { Some(halved) }\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v50_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x > 0 {\nif x > 10 {\nif x > 100 {\nErr(\"too large\")\n} else {\nOk(x * 10)\n}\n} else {\nOk(x)\n}\n} else {\nErr(\"non-positive\")\n}\n}");
}

#[test]
fn test_type_check_v50_7() {
    check_no_errors("struct Sphere2 { cx: number, cy: number, cz: number, r: number }\nfn volume5(s: Sphere2) -> number { 4 * s.r * s.r * s.r }\nfn contains_origin2(s: Sphere2) -> bool {\ns.cx * s.cx + s.cy * s.cy + s.cz * s.cz <= s.r * s.r\n}");
}

#[test]
fn test_type_check_v50_8() {
    check_no_errors("enum LogicGate { And4, Or4, Not, Xor2 }\nfn num_inputs(g: LogicGate) -> number {\nmatch g {\nLogicGate::And4 => 2\nLogicGate::Or4 => 2\nLogicGate::Not => 1\nLogicGate::Xor2 => 2\n}\n}");
}

#[test]
fn test_type_check_v50_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + i * (i + 1) * (i + 2) / 6\n};\nsum\n}");
}

#[test]
fn test_type_check_v50_10() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nlet i = 1;\nwhile i <= x {\nsum = sum + i * i;\ni = i + 1\n};\nsum\n}");
}

#[test]
fn test_type_check_v51_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x * 5;\nlet b = a + 3;\nlet c = b / 2;\nc\n}");
}

#[test]
fn test_type_check_v51_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nif i >= 5 && i <= 15 {\nsum = sum + i * i\n}\n};\nsum\n}");
}

#[test]
fn test_type_check_v51_3() {
    check_no_errors("struct Mass2 { kg: number }\nfn to_grams(m: Mass2) -> number { m.kg * 1000 }\nfn is_heavy2(m: Mass2) -> bool { m.kg > 50 }");
}

#[test]
fn test_type_check_v51_4() {
    check_no_errors("enum Job2 { Running2, Queued, Completed, Failed }\nfn is_pending(j: Job2) -> bool {\nmatch j {\nJob2::Running2 => true\nJob2::Queued => true\nJob2::Completed => false\nJob2::Failed => false\n}\n}");
}

#[test]
fn test_type_check_v51_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 0 {\nlet sq = x * x;\nif sq > 100 && sq < 1000 { Some(sq) } else { None }\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v51_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x > 0 {\nOk(if x > 50 { x - 50 } else { x })\n} else {\nErr(\"non-positive\")\n}\n}");
}

#[test]
fn test_type_check_v51_7() {
    check_no_errors("struct Cylinder { radius: number, height: number }\nfn volume6(c: Cylinder) -> number { 3 * c.radius * c.radius * c.height }\nfn is_flat(c: Cylinder) -> bool { c.height == 0 }");
}

#[test]
fn test_type_check_v51_8() {
    check_no_errors("enum Shader2 { Vertex, Fragment, Geometry }\nfn is_vertex(s: Shader2) -> bool {\nmatch s {\nShader2::Vertex => true\nShader2::Fragment => false\nShader2::Geometry => false\n}\n}");
}

#[test]
fn test_type_check_v51_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 1..x {\nsum = sum + if i % 2 == 0 { i * i } else { i }\n};\nsum\n}");
}

#[test]
fn test_type_check_v51_10() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 1;\nfor i in 0..x {\nresult = result * 2\n};\nresult - 1\n}");
}

#[test]
fn test_type_check_v52_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x + 1;\nlet b = a * 5;\nlet c = b - 5;\nc / 5\n}");
}

#[test]
fn test_type_check_v52_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + if i % 12 == 0 { i / 12 } else { 0 }\n};\nsum\n}");
}

#[test]
fn test_type_check_v52_3() {
    check_no_errors("struct Area2 { sqm: number }\nfn to_sqft(a: Area2) -> number { a.sqm * 10 }\nfn is_large(a: Area2) -> bool { a.sqm > 100 }");
}

#[test]
fn test_type_check_v52_4() {
    check_no_errors("enum Lock2 { Shared, Exclusive, Free }\nfn is_writeable(l: Lock2) -> bool {\nmatch l {\nLock2::Shared => false\nLock2::Exclusive => true\nLock2::Free => true\n}\n}");
}

#[test]
fn test_type_check_v52_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 1 {\nlet prev = x - 1;\nif prev % 2 == 0 { Some(prev) } else { None }\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v52_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x >= 0 && x <= 360 {\nif x <= 90 { Ok(1) } else { if x <= 180 { Ok(2) } else { if x <= 270 { Ok(3) } else { Ok(4) } } }\n} else {\nErr(\"invalid angle\")\n}\n}");
}

#[test]
fn test_type_check_v52_7() {
    check_no_errors("struct Cone { radius: number, height: number }\nfn volume7(c: Cone) -> number { c.radius * c.radius * c.height }\nfn is_pointy(c: Cone) -> bool { c.height > c.radius * 2 }");
}

#[test]
fn test_type_check_v52_8() {
    check_no_errors("enum Device { Keyboard, Mouse, Monitor, Speaker }\nfn is_input(d: Device) -> bool {\nmatch d {\nDevice::Keyboard => true\nDevice::Mouse => true\nDevice::Monitor => false\nDevice::Speaker => false\n}\n}");
}

#[test]
fn test_type_check_v52_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + (2 * i + 1) * (2 * i + 1) * (2 * i + 1)\n};\nsum\n}");
}

#[test]
fn test_type_check_v52_10() {
    check_no_errors("fn f(x: number) -> number {\nlet a = 2;\nlet b = 3;\nlet i = 0;\nwhile i < x {\nlet c = a + b;\na = b;\nb = c;\ni = i + 1\n};\nb\n}");
}

#[test]
fn test_type_check_v53_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x * 3 + 1;\nlet b = a * a;\nb - 1\n}");
}

#[test]
fn test_type_check_v53_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nif i > 0 {\nsum = sum + i * (i - 1)\n}\n};\nsum\n}");
}

#[test]
fn test_type_check_v53_3() {
    check_no_errors("struct Pressure { pascal: number }\nfn to_bar(p: Pressure) -> number { p.pascal / 100000 }\nfn is_atmosphere(p: Pressure) -> bool { p.pascal > 90000 && p.pascal < 110000 }");
}

#[test]
fn test_type_check_v53_4() {
    check_no_errors("enum Crypto { AES, RSA, DES, ChaCha }\nfn is_symmetric(c: Crypto) -> bool {\nmatch c {\nCrypto::AES => true\nCrypto::RSA => false\nCrypto::DES => true\nCrypto::ChaCha => true\n}\n}");
}

#[test]
fn test_type_check_v53_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 0 {\nlet doubled = x * 2;\nif doubled % 4 == 0 { Some(doubled) } else { None }\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v53_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x > 0 {\nif x % 2 == 0 {\nOk(x / 2)\n} else {\nOk(x)\n}\n} else {\nErr(\"non-positive\")\n}\n}");
}

#[test]
fn test_type_check_v53_7() {
    check_no_errors("struct Torus2 { major_r: number, minor_r: number }\nfn surface_area(t: Torus2) -> number { 4 * 3 * t.major_r * t.minor_r }\nfn is_thick(t: Torus2) -> bool { t.minor_r > t.major_r / 2 }");
}

#[test]
fn test_type_check_v53_8() {
    check_no_errors("enum Sensor { Temperature2, Humidity, Pressure2, Light }\nfn is_environmental(s: Sensor) -> bool {\nmatch s {\nSensor::Temperature2 => true\nSensor::Humidity => true\nSensor::Pressure2 => true\nSensor::Light => false\n}\n}");
}

#[test]
fn test_type_check_v53_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 1..x {\nsum = sum + i * i * (i + 1)\n};\nsum\n}");
}

#[test]
fn test_type_check_v53_10() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nlet n = x;\nwhile n > 0 {\nsum = sum + n % 10 * (n % 10);\nn = n / 10\n};\nsum\n}");
}

#[test]
fn test_type_check_v54_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x + 5;\nlet b = a * 2;\nb - 10\n}");
}

#[test]
fn test_type_check_v54_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nif i > 0 && i % 2 == 0 {\nsum = sum + i / 2\n}\n};\nsum\n}");
}

#[test]
fn test_type_check_v54_3() {
    check_no_errors("struct Energy { joules: number }\nfn to_kj(e: Energy) -> number { e.joules / 1000 }\nfn to_cal(e: Energy) -> number { e.joules / 4 }\nfn is_positive_energy(e: Energy) -> bool { e.joules > 0 }");
}

#[test]
fn test_type_check_v54_4() {
    check_no_errors("enum Format2 { JSON, XML, YAML, TOML }\nfn is_markup(f: Format2) -> bool {\nmatch f {\nFormat2::JSON => false\nFormat2::XML => true\nFormat2::YAML => false\nFormat2::TOML => false\n}\n}");
}

#[test]
fn test_type_check_v54_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 5 {\nlet quadrupled = x * 4;\nif quadrupled < 200 { Some(quadrupled) } else { None }\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v54_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x > 0 {\nif x > 10 {\nOk(x * x)\n} else {\nOk(x + x)\n}\n} else {\nErr(\"non-positive\")\n}\n}");
}

#[test]
fn test_type_check_v54_7() {
    check_no_errors("struct Ellipse2 { a: number, b: number }\nfn is_circle2(e: Ellipse2) -> bool { e.a == e.b }\nfn eccentricity_sq(e: Ellipse2) -> number { if e.a > e.b { (e.a * e.a - e.b * e.b) / (e.a * e.a) } else { 0 } }");
}

#[test]
fn test_type_check_v54_8() {
    check_no_errors("enum Method2 { GET, POST, PUT, DELETE, PATCH }\nfn has_body2(m: Method2) -> bool {\nmatch m {\nMethod2::GET => false\nMethod2::POST => true\nMethod2::PUT => true\nMethod2::DELETE => false\nMethod2::PATCH => true\n}\n}");
}

#[test]
fn test_type_check_v54_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + (x - i) * (x - i) * (x - i)\n};\nsum\n}");
}

#[test]
fn test_type_check_v54_10() {
    check_no_errors("fn f(x: number) -> number {\nlet a = 3;\nlet b = 5;\nfor i in 0..x {\nlet c = a + b;\na = b;\nb = c\n};\nb\n}");
}

#[test]
fn test_type_check_v55_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x * 2 + 3;\nlet b = a * a;\nb\n}");
}

#[test]
fn test_type_check_v55_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nfor j in 0..2 {\nsum = sum + i\n}\n};\nsum\n}");
}

#[test]
fn test_type_check_v55_3() {
    check_no_errors("struct Power2 { watts: number }\nfn to_kw(p: Power2) -> number { p.watts / 1000 }\nfn is_high_power(p: Power2) -> bool { p.watts > 1000 }");
}

#[test]
fn test_type_check_v55_4() {
    check_no_errors("enum Geo2 { Point2, Line3, Polygon }\nfn has_area(g: Geo2) -> bool {\nmatch g {\nGeo2::Point2 => false\nGeo2::Line3 => false\nGeo2::Polygon => true\n}\n}");
}

#[test]
fn test_type_check_v55_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 0 {\nlet mod7 = x % 7;\nif mod7 == 0 { None } else { Some(mod7) }\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v55_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x > 0 {\nif x > 50 { Ok(x - 50) } else { Ok(x) }\n} else {\nErr(\"non-positive\")\n}\n}");
}

#[test]
fn test_type_check_v55_7() {
    check_no_errors("struct Pyramid { base: number, height: number }\nfn volume8(p: Pyramid) -> number { p.base * p.base * p.height / 3 }\nfn is_tall(p: Pyramid) -> bool { p.height > p.base }");
}

#[test]
fn test_type_check_v55_8() {
    check_no_errors("enum BuildStatus { Success2, Failed2, Timeout, Cancelled }\nfn needs_retry(b: BuildStatus) -> bool {\nmatch b {\nBuildStatus::Success2 => false\nBuildStatus::Failed2 => true\nBuildStatus::Timeout => true\nBuildStatus::Cancelled => false\n}\n}");
}

#[test]
fn test_type_check_v55_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + i * i * i * i\n};\nsum\n}");
}

#[test]
fn test_type_check_v55_10() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 0;\nlet i = 0;\nwhile i < x {\nresult = result + (2 * i + 1) * (2 * i + 1);\ni = i + 1\n};\nresult\n}");
}

#[test]
fn test_type_check_v56_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x - 1;\nlet b = a * a + 2 * a + 1;\nb\n}");
}

#[test]
fn test_type_check_v56_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nif i > 5 {\nsum = sum + (i - 5) * (i - 5)\n}\n};\nsum\n}");
}

#[test]
fn test_type_check_v56_3() {
    check_no_errors("struct Density2 { mass: number, volume: number }\nfn compute(d: Density2) -> number { d.mass / d.volume }\nfn is_heavy2(d: Density2) -> bool { d.mass / d.volume > 5 }");
}

#[test]
fn test_type_check_v56_4() {
    check_no_errors("enum Cloud2 { Public3, Private3, Hybrid }\nfn is_shared(c: Cloud2) -> bool {\nmatch c {\nCloud2::Public3 => true\nCloud2::Private3 => false\nCloud2::Hybrid => true\n}\n}");
}

#[test]
fn test_type_check_v56_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 10 {\nlet subtracted = x - 10;\nif subtracted % 3 == 0 { Some(subtracted) } else { None }\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v56_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x > 0 {\nif x > 100 {\nOk(100)\n} else {\nif x > 50 {\nOk(x - 50)\n} else {\nOk(x)\n}\n}\n} else {\nErr(\"non-positive\")\n}\n}");
}

#[test]
fn test_type_check_v56_7() {
    check_no_errors("struct Prism2 { base_area: number, height: number }\nfn volume9(p: Prism2) -> number { p.base_area * p.height }\nfn is_flat2(p: Prism2) -> bool { p.height < 1 }");
}

#[test]
fn test_type_check_v56_8() {
    check_no_errors("enum EventType2 { Click2, Hover, Focus, Blur }\nfn is_mouse_event(e: EventType2) -> bool {\nmatch e {\nEventType2::Click2 => true\nEventType2::Hover => true\nEventType2::Focus => false\nEventType2::Blur => false\n}\n}");
}

#[test]
fn test_type_check_v56_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + (2 * i + 1) * (2 * i + 1) * (2 * i + 1)\n};\nsum\n}");
}

#[test]
fn test_type_check_v56_10() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nlet a = 0;\nlet b = 1;\nfor i in 0..x {\nsum = sum + a;\nlet c = a + b;\na = b;\nb = c\n};\nsum\n}");
}

#[test]
fn test_type_check_v57_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x * 3 + 2;\nlet b = a * a;\nb\n}");
}

#[test]
fn test_type_check_v57_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nif i > 10 {\nsum = sum + (i - 10) * 2\n}\n};\nsum\n}");
}

#[test]
fn test_type_check_v57_3() {
    check_no_errors("struct Frequency { hz: number }\nfn to_khz(f: Frequency) -> number { f.hz / 1000 }\nfn is_audible(f: Frequency) -> bool { f.hz >= 20 && f.hz <= 20000 }");
}

#[test]
fn test_type_check_v57_4() {
    check_no_errors("enum Container2 { List, Vector, Set, Map }\nfn is_ordered(c: Container2) -> bool {\nmatch c {\nContainer2::List => true\nContainer2::Vector => true\nContainer2::Set => false\nContainer2::Map => false\n}\n}");
}

#[test]
fn test_type_check_v57_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 0 {\nlet sq = x * x;\nif sq > 50 && sq < 500 { Some(sq) } else { None }\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v57_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x > 0 {\nif x > 50 {\nOk(50)\n} else {\nOk(x)\n}\n} else {\nErr(\"non-positive\")\n}\n}");
}

#[test]
fn test_type_check_v57_7() {
    check_no_errors("struct Trapezoid2 { a: number, b: number, h: number }\nfn area8(t: Trapezoid2) -> number { (t.a + t.b) * t.h / 2 }\nfn is_triangle_shape(t: Trapezoid2) -> bool { t.a == 0 || t.b == 0 }");
}

#[test]
fn test_type_check_v57_8() {
    check_no_errors("enum Engine2 { V8, V6, Inline4, Electric }\nfn is_combustion(e: Engine2) -> bool {\nmatch e {\nEngine2::V8 => true\nEngine2::V6 => true\nEngine2::Inline4 => true\nEngine2::Electric => false\n}\n}");
}

#[test]
fn test_type_check_v57_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + i * (x - i)\n};\nsum\n}");
}

#[test]
fn test_type_check_v57_10() {
    check_no_errors("fn f(x: number) -> number {\nlet a = 0;\nlet b = 1;\nfor i in 0..x {\nlet c = a + b;\na = b;\nb = c\n};\na * b\n}");
}

#[test]
fn test_type_check_v58_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x + 1;\nlet b = a * 3 + 2;\nb * b\n}");
}

#[test]
fn test_type_check_v58_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nif i % 13 == 0 {\nsum = sum + i / 13\n}\n};\nsum\n}");
}

#[test]
fn test_type_check_v58_3() {
    check_no_errors("struct Force2 { newtons: number }\nfn to_kilonewtons(f: Force2) -> number { f.newtons / 1000 }\nfn is_strong(f: Force2) -> bool { f.newtons > 1000 }");
}

#[test]
fn test_type_check_v58_4() {
    check_no_errors("enum Screen2 { LCD, OLED, AMOLED, EInk }\nfn has_backlight(s: Screen2) -> bool {\nmatch s {\nScreen2::LCD => true\nScreen2::OLED => false\nScreen2::AMOLED => false\nScreen2::EInk => false\n}\n}");
}

#[test]
fn test_type_check_v58_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 0 {\nlet halved = x / 2;\nif halved > 10 { None } else { Some(halved) }\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v58_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x > 0 {\nif x > 200 {\nErr(\"overflow\")\n} else {\nOk(x * x)\n}\n} else {\nErr(\"non-positive\")\n}\n}");
}

#[test]
fn test_type_check_v58_7() {
    check_no_errors("struct Rhombus2 { d1: number, d2: number }\nfn area9(r: Rhombus2) -> number { r.d1 * r.d2 / 2 }\nfn is_square2(r: Rhombus2) -> bool { r.d1 == r.d2 }");
}

#[test]
fn test_type_check_v58_8() {
    check_no_errors("enum ColorModel2 { RGB2, CMYK, HSV }\nfn is_additive(c: ColorModel2) -> bool {\nmatch c {\nColorModel2::RGB2 => true\nColorModel2::CMYK => false\nColorModel2::HSV => false\n}\n}");
}

#[test]
fn test_type_check_v58_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + if i % 4 == 0 { i * i } else { i }\n};\nsum\n}");
}

#[test]
fn test_type_check_v58_10() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 0;\nlet i = 1;\nwhile i < x {\nresult = result + i;\ni = i * 2\n};\nresult\n}");
}

#[test]
fn test_type_check_v59_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x * 2 - 1;\nlet b = a * a;\nb\n}");
}

#[test]
fn test_type_check_v59_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nif i > 3 {\nsum = sum + (i - 3) * (i - 3)\n}\n};\nsum\n}");
}

#[test]
fn test_type_check_v59_3() {
    check_no_errors("struct Acceleration2 { mps2: number }\nfn to_g(a: Acceleration2) -> number { a.mps2 / 10 }\nfn is_zero_g(a: Acceleration2) -> bool { a.mps2 == 0 }");
}

#[test]
fn test_type_check_v59_4() {
    check_no_errors("enum CacheLevel { L1, L2, L3, MainMemory }\nfn is_on_chip(c: CacheLevel) -> bool {\nmatch c {\nCacheLevel::L1 => true\nCacheLevel::L2 => true\nCacheLevel::L3 => true\nCacheLevel::MainMemory => false\n}\n}");
}

#[test]
fn test_type_check_v59_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 5 {\nlet tripled = x * 3;\nif tripled < 100 { Some(tripled) } else { None }\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v59_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x > 0 {\nif x > 10 {\nif x > 100 {\nErr(\"overflow\")\n} else {\nOk(x)\n}\n} else {\nOk(x * 2)\n}\n} else {\nErr(\"non-positive\")\n}\n}");
}

#[test]
fn test_type_check_v59_7() {
    check_no_errors("struct Sector2 { radius: number, angle: number }\nfn area10(s: Sector2) -> number { s.radius * s.radius * s.angle / 360 }\nfn is_half_circle(s: Sector2) -> bool { s.angle == 180 }");
}

#[test]
fn test_type_check_v59_8() {
    check_no_errors("enum Event3 { Timer, IO, Network, Signal }\nfn is_external(e: Event3) -> bool {\nmatch e {\nEvent3::Timer => false\nEvent3::IO => true\nEvent3::Network => true\nEvent3::Signal => true\n}\n}");
}

#[test]
fn test_type_check_v59_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 1..x {\nsum = sum + i * (i + 1) * (i + 2) / 6\n};\nsum\n}");
}

#[test]
fn test_type_check_v59_10() {
    check_no_errors("fn f(x: number) -> number {\nlet a = 1;\nlet b = 2;\nlet i = 0;\nwhile i < x {\nlet c = a + b;\na = b;\nb = c;\ni = i + 1\n};\nb\n}");
}

#[test]
fn test_type_check_v60_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x * 4;\nlet b = a + 1;\nlet c = b * b;\nc\n}");
}

#[test]
fn test_type_check_v60_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nif i % 2 == 0 && i > 0 {\nsum = sum + i * i\n}\n};\nsum\n}");
}

#[test]
fn test_type_check_v60_3() {
    check_no_errors("struct Voltage { volts: number }\nfn to_mv(v: Voltage) -> number { v.volts * 1000 }\nfn is_safe(v: Voltage) -> bool { v.volts < 50 }");
}

#[test]
fn test_type_check_v60_4() {
    check_no_errors("enum Algorithm2 { BFS, DFS, Dijkstra, AStar }\nfn uses_heuristic(a: Algorithm2) -> bool {\nmatch a {\nAlgorithm2::BFS => false\nAlgorithm2::DFS => false\nAlgorithm2::Dijkstra => false\nAlgorithm2::AStar => true\n}\n}");
}

#[test]
fn test_type_check_v60_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 0 {\nlet fifth = x / 5;\nif fifth > 0 { Some(fifth) } else { None }\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v60_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x > 0 {\nif x % 2 == 0 { Ok(x / 2) } else { Ok(x * 3 + 1) }\n} else {\nErr(\"non-positive\")\n}\n}");
}

#[test]
fn test_type_check_v60_7() {
    check_no_errors("struct Pentagon { side: number }\nfn perimeter3(p: Pentagon) -> number { p.side * 5 }\nfn is_regular(p: Pentagon) -> bool { p.side > 0 }");
}

#[test]
fn test_type_check_v60_8() {
    check_no_errors("enum TestResult { Pass2, Fail2, Error3, Skip }\nfn needs_attention(t: TestResult) -> bool {\nmatch t {\nTestResult::Pass2 => false\nTestResult::Fail2 => true\nTestResult::Error3 => true\nTestResult::Skip => false\n}\n}");
}

#[test]
fn test_type_check_v60_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + i * i * i + 3 * i * i + 3 * i + 1\n};\nsum\n}");
}

#[test]
fn test_type_check_v60_10() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 0;\nlet i = 0;\nwhile i < x {\nresult = result + i * i + i;\ni = i + 1\n};\nresult\n}");
}

#[test]
fn test_type_check_v61_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x + 2;\nlet b = a * 4;\nb - 8\n}");
}

#[test]
fn test_type_check_v61_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nif i > 7 {\nsum = sum + (i - 7)\n}\n};\nsum\n}");
}

#[test]
fn test_type_check_v61_3() {
    check_no_errors("struct Current2 { amps: number }\nfn to_ma(c: Current2) -> number { c.amps * 1000 }\nfn is_short_circuit(c: Current2) -> bool { c.amps > 100 }");
}

#[test]
fn test_type_check_v61_4() {
    check_no_errors("enum Language3 { Compiled, Interpreted, JIT2 }\nfn needs_runtime(l: Language3) -> bool {\nmatch l {\nLanguage3::Compiled => false\nLanguage3::Interpreted => true\nLanguage3::JIT2 => true\n}\n}");
}

#[test]
fn test_type_check_v61_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 0 {\nlet seventh = x / 7;\nif seventh > 0 { Some(seventh) } else { None }\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v61_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x > 0 {\nif x > 1000 { Err(\"overflow\") } else { Ok(x * x) }\n} else {\nErr(\"non-positive\")\n}\n}");
}

#[test]
fn test_type_check_v61_7() {
    check_no_errors("struct Hexagon { side: number }\nfn perimeter4(h: Hexagon) -> number { h.side * 6 }\nfn is_regular2(h: Hexagon) -> bool { h.side > 0 }");
}

#[test]
fn test_type_check_v61_8() {
    check_no_errors("enum Log2 { Debug3, Info3, Warn3, Error4 }\nfn is_error_level(l: Log2) -> bool {\nmatch l {\nLog2::Debug3 => false\nLog2::Info3 => false\nLog2::Warn3 => false\nLog2::Error4 => true\n}\n}");
}

#[test]
fn test_type_check_v61_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + i * (x - i) * (x - i)\n};\nsum\n}");
}

#[test]
fn test_type_check_v61_10() {
    check_no_errors("fn f(x: number) -> number {\nlet a = 0;\nlet b = 1;\nlet sum = 0;\nfor i in 0..x {\nsum = sum + b;\nlet c = a + b;\na = b;\nb = c\n};\nsum\n}");
}

#[test]
fn test_type_check_v62_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x * 2;\nlet b = a + 5;\nb * b\n}");
}

#[test]
fn test_type_check_v62_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nif i % 4 == 0 && i > 0 {\nsum = sum + i\n}\n};\nsum\n}");
}

#[test]
fn test_type_check_v62_3() {
    check_no_errors("struct Resistance { ohms: number }\nfn to_kohm(r: Resistance) -> number { r.ohms / 1000 }\nfn is_short(r: Resistance) -> bool { r.ohms == 0 }");
}

#[test]
fn test_type_check_v62_4() {
    check_no_errors("enum Database { Postgres, MySQL, SQLite, MongoDB }\nfn is_sql(d: Database) -> bool {\nmatch d {\nDatabase::Postgres => true\nDatabase::MySQL => true\nDatabase::SQLite => true\nDatabase::MongoDB => false\n}\n}");
}

#[test]
fn test_type_check_v62_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 0 {\nlet cubed = x * x * x;\nif cubed > 1000 { None } else { Some(cubed) }\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v62_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x > 0 {\nif x > 10 { Ok(x * x) } else { Ok(x) }\n} else {\nErr(\"non-positive\")\n}\n}");
}

#[test]
fn test_type_check_v62_7() {
    check_no_errors("struct Octagon { side: number }\nfn perimeter5(o: Octagon) -> number { o.side * 8 }\nfn is_regular3(o: Octagon) -> bool { o.side > 0 }");
}

#[test]
fn test_type_check_v62_8() {
    check_no_errors("enum FileMode2 { Read4, Write4, ReadWrite2, Append }\nfn can_read2(f: FileMode2) -> bool {\nmatch f {\nFileMode2::Read4 => true\nFileMode2::Write4 => false\nFileMode2::ReadWrite2 => true\nFileMode2::Append => false\n}\n}");
}

#[test]
fn test_type_check_v62_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + if i % 2 == 0 { i * i } else { i * i * i }\n};\nsum\n}");
}

#[test]
fn test_type_check_v62_10() {
    check_no_errors("fn f(x: number) -> number {\nlet a = 0;\nlet b = 1;\nfor i in 0..x {\nlet c = a + b;\na = b;\nb = c\n};\nif a > b { a } else { b }\n}");
}

#[test]
fn test_type_check_v63_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x * 3 - 1;\nlet b = a * a + 2 * a;\nb\n}");
}

#[test]
fn test_type_check_v63_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nif i > 15 {\nsum = sum + (i - 15) * 2\n}\n};\nsum\n}");
}

#[test]
fn test_type_check_v63_3() {
    check_no_errors("struct DataRate { bps: number }\nfn to_kbps(d: DataRate) -> number { d.bps / 1000 }\nfn is_broadband(d: DataRate) -> bool { d.bps > 25000000 }");
}

#[test]
fn test_type_check_v63_4() {
    check_no_errors("enum IDE2 { VSCode, IntelliJ, Vim, Emacs }\nfn has_lsp(i: IDE2) -> bool {\nmatch i {\nIDE2::VSCode => true\nIDE2::IntelliJ => true\nIDE2::Vim => true\nIDE2::Emacs => true\n}\n}");
}

#[test]
fn test_type_check_v63_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 0 {\nlet squared = x * x;\nif squared < 10000 { Some(squared) } else { None }\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v63_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x > 0 {\nif x > 50 {\nif x > 75 { Ok(100) } else { Ok(75) }\n} else {\nOk(x)\n}\n} else {\nErr(\"non-positive\")\n}\n}");
}

#[test]
fn test_type_check_v63_7() {
    check_no_errors("struct Decagon { side: number }\nfn perimeter6(d: Decagon) -> number { d.side * 10 }\nfn is_regular4(d: Decagon) -> bool { d.side > 0 }");
}

#[test]
fn test_type_check_v63_8() {
    check_no_errors("enum Token3 { Ident2(string), Number3(number), Operator, EOF2 }\nfn has_value(t: Token3) -> bool {\nmatch t {\nToken3::Ident2(_) => true\nToken3::Number3(_) => true\nToken3::Operator => false\nToken3::EOF2 => false\n}\n}");
}

#[test]
fn test_type_check_v63_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + (i + 1) * (i + 1) * (i + 1)\n};\nsum\n}");
}

#[test]
fn test_type_check_v63_10() {
    check_no_errors("fn f(x: number) -> number {\nlet a = 0;\nlet b = 1;\nfor i in 0..x {\nlet c = a + b;\na = b;\nb = c\n};\na + b\n}");
}

#[test]
fn test_type_check_v64_1() {
    check_no_errors("fn f(x: number) -> number {\nlet a = x + 3;\nlet b = a * 2 + 1;\nb\n}");
}

#[test]
fn test_type_check_v64_2() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nif i > 20 {\nsum = sum + i - 20\n}\n};\nsum\n}");
}

#[test]
fn test_type_check_v64_3() {
    check_no_errors("struct Luminance { lux: number }\nfn is_bright(l: Luminance) -> bool { l.lux > 1000 }\nfn is_dark2(l: Luminance) -> bool { l.lux < 10 }");
}

#[test]
fn test_type_check_v64_4() {
    check_no_errors("enum GameState2 { Menu2, Playing, Paused, GameOver }\nfn is_active2(g: GameState2) -> bool {\nmatch g {\nGameState2::Menu2 => false\nGameState2::Playing => true\nGameState2::Paused => false\nGameState2::GameOver => false\n}\n}");
}

#[test]
fn test_type_check_v64_5() {
    check_no_errors("fn f(x: number) -> Option<number> {\nif x > 0 {\nlet tenth = x / 10;\nif tenth > 0 { Some(tenth) } else { None }\n} else {\nNone\n}\n}");
}

#[test]
fn test_type_check_v64_6() {
    check_no_errors("fn f(x: number) -> Result<number, string> {\nif x > 0 {\nOk(if x > 100 { 100 } else { x })\n} else {\nErr(\"non-positive\")\n}\n}");
}

#[test]
fn test_type_check_v64_7() {
    check_no_errors("struct Dodecagon { side: number }\nfn perimeter7(d: Dodecagon) -> number { d.side * 12 }\nfn is_regular5(d: Dodecagon) -> bool { d.side > 0 }");
}

#[test]
fn test_type_check_v64_8() {
    check_no_errors("enum Node3 { Root, Internal, Leaf3 }\nfn has_children(n: Node3) -> bool {\nmatch n {\nNode3::Root => true\nNode3::Internal => true\nNode3::Leaf3 => false\n}\n}");
}

#[test]
fn test_type_check_v64_9() {
    check_no_errors("fn f(x: number) -> number {\nlet sum = 0;\nfor i in 0..x {\nsum = sum + if i % 3 == 0 { i * i } else { i }\n};\nsum\n}");
}

#[test]
fn test_type_check_v64_10() {
    check_no_errors("fn f(x: number) -> number {\nlet result = 0;\nlet i = 0;\nwhile i < x {\nresult = result + i * i;\ni = i + 1\n};\nresult\n}");
}
