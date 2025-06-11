use karte_diagnostics::Span;
use karte_hir::{type_check, Expr};

fn main() {
    // 创建一个示例span
    let span = Span::new(0, 10);

    // 示例1: 简单的lambda表达式 - 现在支持所有类型
    // lambda (x) -> x + 1
    let lambda_expr = Expr::Lambda {
        params: vec!["x".to_string()],
        body: Box::new(Expr::BinaryOp {
            left: Box::new(Expr::Identifier {
                name: "x".to_string(),
                span,
            }),
            op: karte_hir::BinaryOperator::Add,
            right: Box::new(Expr::Number { value: 1, span }),
            span,
        }),
        span,
    };

    let (lambda_type, diagnostics) = type_check(&lambda_expr);
    println!("Lambda类型: {}", lambda_type);
    println!("诊断信息数量: {}", diagnostics.len());
    if diagnostics.has_errors() {
        println!("错误详情:\n{}", diagnostics);
    }

    // 示例2: 高阶函数示例
    // lambda (f) -> f(42)
    let higher_order = Expr::Lambda {
        params: vec!["f".to_string()],
        body: Box::new(Expr::FunctionCall {
            function: Box::new(Expr::Identifier {
                name: "f".to_string(),
                span,
            }),
            args: vec![Expr::Number { value: 42, span }],
            span,
        }),
        span,
    };

    let (ho_type, ho_diagnostics) = type_check(&higher_order);
    println!("高阶函数类型: {}", ho_type);
    println!("诊断信息数量: {}", ho_diagnostics.len());
    if ho_diagnostics.has_errors() {
        println!("错误详情:\n{}", ho_diagnostics);
    }

    // 示例3: 复杂的函数组合
    // lambda (x) -> lambda (y) -> x + y
    let curried_add = Expr::Lambda {
        params: vec!["x".to_string()],
        body: Box::new(Expr::Lambda {
            params: vec!["y".to_string()],
            body: Box::new(Expr::BinaryOp {
                left: Box::new(Expr::Identifier {
                    name: "x".to_string(),
                    span,
                }),
                op: karte_hir::BinaryOperator::Add,
                right: Box::new(Expr::Identifier {
                    name: "y".to_string(),
                    span,
                }),
                span,
            }),
            span,
        }),
        span,
    };

    let (curried_type, curried_diagnostics) = type_check(&curried_add);
    println!("柯里化函数类型: {}", curried_type);
    println!("诊断信息数量: {}", curried_diagnostics.len());
    if curried_diagnostics.has_errors() {
        println!("错误详情:\n{}", curried_diagnostics);
    }
}
