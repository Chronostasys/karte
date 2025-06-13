use karte_hir::{Expr, Parameter, BinaryOperator, Statement};
use karte_diagnostics::Span;

fn dummy_span() -> Span {
    Span { start: 0, end: 0 }
}

fn main() {
    // 测试嵌套调用: (|x| x * 2)((|y| y + 1)(5)) 应该返回 12
    let inner_lambda = Expr::Lambda {
        params: vec![Parameter::simple("y".to_string())],
        body: Box::new(Expr::BinaryOp {
            left: Box::new(Expr::Identifier {
                name: "y".to_string(),
                span: dummy_span(),
            }),
            op: BinaryOperator::Add,
            right: Box::new(Expr::Number {
                value: 1,
                span: dummy_span(),
            }),
            span: dummy_span(),
        }),
        span: dummy_span(),
    };

    let inner_call = Expr::FunctionCall {
        function: Box::new(inner_lambda),
        args: vec![Expr::Number {
            value: 5,
            span: dummy_span(),
        }],
        span: dummy_span(),
    };

    let outer_lambda = Expr::Lambda {
        params: vec![Parameter::simple("x".to_string())],
        body: Box::new(Expr::BinaryOp {
            left: Box::new(Expr::Identifier {
                name: "x".to_string(),
                span: dummy_span(),
            }),
            op: BinaryOperator::Multiply,
            right: Box::new(Expr::Number {
                value: 2,
                span: dummy_span(),
            }),
            span: dummy_span(),
        }),
        span: dummy_span(),
    };

    let outer_call = Expr::FunctionCall {
        function: Box::new(outer_lambda),
        args: vec![inner_call],
        span: dummy_span(),
    };

    // 1. HIR -> MIR
    println!("=== HIR ===");
    println!("{:#?}", outer_call);
    
    let mir_program = karte_mir::lower::lower_expr_to_mir(&outer_call)
        .expect("MIR lowering failed");
    
    println!("\n=== MIR ===");
    println!("{:#?}", mir_program);
    
    // 2. MIR -> LIR
    let lir_program = karte_lir::lower::lower_mir_to_lir(&mir_program)
        .expect("LIR lowering failed");
    
    println!("\n=== LIR ===");
    println!("{:#?}", lir_program);
    
    // 3. 执行LIR，启用调试
    let result = karte_codegen::lir_interpreter::execute_with_debug(&lir_program, true)
        .expect("Execution failed");
    
    println!("\n=== Result ===");
    println!("Expected: 12, Got: {}", result);
} 