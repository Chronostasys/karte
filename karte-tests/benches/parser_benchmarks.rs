use criterion::{black_box, criterion_group, criterion_main, Criterion};
use karte_lexer::tokenize;
use karte_parser::{parse, parse_with_type_check};

fn bench_lexer(c: &mut Criterion) {
    let inputs = vec![
        "1 + 2 * 3",
        "let x = 5; x + 10",
        "let f = |x| x * 2; f(7)",
        "let x = 3; let y = 4; let multiply = |a, b| a * b; multiply(x, y)",
        "(1 + 2) * (3 - 4) / 5",
    ];

    c.bench_function("lexer_simple", |b| {
        b.iter(|| {
            for input in &inputs {
                black_box(tokenize(black_box(input)));
            }
        })
    });
}

fn bench_parser(c: &mut Criterion) {
    let inputs = [
        "1 + 2 * 3",
        "let x = 5; x + 10",
        "let f = |x| x * 2; f(7)",
        "let x = 3; let y = 4; let multiply = |a, b| a * b; multiply(x, y)",
    ];

    // 预先分词
    let tokenized: Vec<_> = inputs.iter().map(|input| tokenize(input).0).collect();

    c.bench_function("parser_without_type_check", |b| {
        b.iter(|| {
            for tokens in &tokenized {
                black_box(parse(black_box(tokens)));
            }
        })
    });

    c.bench_function("parser_with_type_check", |b| {
        b.iter(|| {
            for tokens in &tokenized {
                black_box(parse_with_type_check(black_box(tokens)));
            }
        })
    });
}

fn bench_complex_expressions(c: &mut Criterion) {
    // 生成更复杂的表达式
    let complex_expr = "let a = 1; let b = 2; let c = 3; \
                       let f1 = |x| x + 1; let f2 = |y| y * 2; \
                       let compose = |g, h| |x| g(h(x)); \
                       compose(f1, f2)(a + b + c)";

    let (tokens, _) = tokenize(complex_expr);

    c.bench_function("complex_expression_parse", |b| {
        b.iter(|| {
            black_box(parse(black_box(&tokens)));
        })
    });

    c.bench_function("complex_expression_type_check", |b| {
        b.iter(|| {
            black_box(parse_with_type_check(black_box(&tokens)));
        })
    });
}

criterion_group!(
    benches,
    bench_lexer,
    bench_parser,
    bench_complex_expressions
);
criterion_main!(benches);
