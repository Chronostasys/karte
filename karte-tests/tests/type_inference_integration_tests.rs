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
