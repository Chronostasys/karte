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
