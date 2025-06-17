pub mod assignment_integration_tests;
pub mod codegen_tests;
pub mod control_flow_tests;
pub mod custom_types_tests;
pub mod integration_tests;
pub mod lexer_tests;
pub mod logical_operators_tests;
pub mod parser_tests;
pub mod reference_tests;
pub mod struct_tests;
pub mod sum_types_tests;
pub mod type_checker_tests;

use karte_diagnostics::Span;
use karte_hir::Expr;

/// 创建一个虚拟的span用于测试
pub fn dummy_span() -> Span {
    Span { start: 0, end: 0 }
}

/// 简化的执行函数，直接返回i64结果，带调试选项
pub fn execute_with_pipeline_debug(expr: &Expr, debug: bool) -> Result<i64, String> {
    // 1. HIR -> MIR
    let mir_program = karte_mir::lower::lower_expr_to_mir(expr)
        .map_err(|e| format!("MIR lowering error: {:?}", e))?;
    
    if debug {
        println!("=== MIR Program ===");
        println!("{}", mir_program);
    }
    
    // 2. MIR -> LIR (生成高级LIR，包含Alloc指令)
    let mut lir_program = karte_lir::lower::lower_mir_to_lir(&mir_program)
        .map_err(|e| format!("LIR lowering error: {:?}", e))?;
    
    if debug {
        println!("=== 返回高级LIR (包含Alloc指令，待优化) ===");
        println!("{}", lir_program);
        println!("================================================");
    }
    
    // 3. 优化管道（Memory2Reg等优化在这里处理Alloc指令）
    let mut pipeline = karte_lir::OptimizationPipeline::new(karte_lir::OptimizationLevel::Balanced);
    let opt_stats = pipeline.optimize(&mut lir_program)
        .map_err(|e| format!("Optimization error: {:?}", e))?;
    
    if debug {
        println!("=== 优化统计 ===");
        opt_stats.print();
        println!("=== 优化后的LIR ===");
        println!("{}", lir_program);
        println!("================================================");
    }
    
    // 4. 指令降级：将高级LIR指令降级为基础指令（在优化之后）
    karte_lir::lower_program_instructions(&mut lir_program)
        .map_err(|e| format!("Instruction lowering error: {:?}", e))?;
    
    if debug {
        println!("=== 指令降级后的LIR (基础指令) ===");
        println!("{}", lir_program);
        println!("================================================");
    }
    
    // 5. 执行（使用简化的寄存器映射，因为寄存器分配已经在编译时完成）
    karte_codegen::lir_interpreter::execute_professional(&lir_program, debug)
}

/// 简化的执行函数，直接返回i64结果
pub fn execute_with_pipeline(expr: &Expr) -> Result<i64, String> {
    execute_with_pipeline_debug(expr, true)
}

/// 从字符串输入执行完整流程的便利函数，返回i64
pub fn execute_from_string(input: &str) -> Result<i64, String> {
    let (tokens, mut diagnostics) = karte_lexer::tokenize(input);
    if diagnostics.has_errors() {
        return Err(format!("Lexer errors: {:?}", diagnostics));
    }

    let (expr, parse_diagnostics) = karte_parser::parse(&tokens);
    diagnostics.extend(parse_diagnostics);
    
    if diagnostics.has_errors() {
        return Err(format!("Parser errors: {:?}", diagnostics));
    }

    let expr = expr.ok_or("Parse failed")?;
    
    // 类型检查
    let (_, type_diagnostics) = karte_hir::type_check(&expr);
    if type_diagnostics.has_errors() {
        return Err(format!("Type check errors: {:?}", type_diagnostics));
    }
    
    execute_with_pipeline(&expr)
}
