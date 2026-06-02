#[cfg(test)]
#[cfg(target_arch = "aarch64")]
mod macos_sigsegv_diagnostic {
    /// macOS 上的 SIGSEGV 诊断测试
    /// 通过捕获 SIGSEGV 信号来输出出错地址和寄存器状态

    #[test]
    fn test_simple_arithmetic_diagnostic() {
        let code = r#"fn main() -> number { 42 }"#;
        run_with_diagnostic(code, 42, "test_simple_arithmetic_diagnostic");
    }

    #[test]
    fn test_for_break_diagnostic() {
        let code = r#"fn main() -> number { let sum = 0; for i in 0..10 { if i == 5 { break }; sum = sum + i }; sum }"#;
        run_with_diagnostic(code, 10, "test_for_break_diagnostic");
    }

    #[test]
    fn test_complex_match_diagnostic() {
        let code = r#"fn main() -> number { match 42 { 1 => 100, 42 => 200, _ => 300 } }"#;
        run_with_diagnostic(code, 200, "test_complex_match_diagnostic");
    }

    #[test]
    fn test_struct_field_access_diagnostic() {
        let code = r#"fn main() -> number { struct Point { x: number, y: number }; let p = Point { x: 10, y: 20 }; p.x + p.y }"#;
        run_with_diagnostic(code, 30, "test_struct_field_access_diagnostic");
    }

    #[test]
    fn test_enum_match_diagnostic() {
        let code = r#"fn main() -> number { enum Color { Red, Green, Blue }; let c = Green; match c { Red => 1, Green => 2, Blue => 3 } }"#;
        run_with_diagnostic(code, 2, "test_enum_match_diagnostic");
    }

    #[test]
    fn test_closure_basic_diagnostic() {
        let code = r#"fn main() -> number { let f = |x| x * 2; f(21) }"#;
        run_with_diagnostic(code, 42, "test_closure_basic_diagnostic");
    }

    fn run_with_diagnostic(code: &str, expected: i64, test_name: &str) {
        use karte_codegen::vm::professional_executor::ProfessionalExecutor;
        use karte_lexer::tokenize;
        use karte_lir::{
            lower::lower_mir_to_lir,
            optimization_pipeline::{OptimizationLevel, OptimizationPipeline},
        };
        use karte_mir::lower::{lower_expr_to_mir_with_options, LoweringOptions};
        use karte_parser::{parse_with_type_check, ParserMode};
        use std::collections::HashSet;

        eprintln!("[DIAG] {} starting", test_name);

        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) =
            parse_with_type_check(&tokens, ParserMode::Project, None);
        if diagnostics.has_errors() {
            eprintln!("[DIAG] {} parse/type errors: {:?}", test_name, diagnostics);
            panic!("{} parse/type errors: {:?}", test_name, diagnostics);
        }
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();

        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options)
            .unwrap_or_else(|e| panic!("{} MIR lowering error: {:?}", test_name, e));

        let mut lir = lower_mir_to_lir(&mir)
            .unwrap_or_else(|e| panic!("{} LIR lowering error: {:?}", test_name, e));

        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline
            .optimize(&mut lir)
            .unwrap_or_else(|e| panic!("{} Optimization error: {:?}", test_name, e));

        // 打印 main 函数的 LIR 指令
        for (name, func) in &lir.functions {
            if name.contains("main") {
                eprintln!("[DIAG] {} === {} ({} instrs, frame={}) ===", test_name, name, func.instructions.len(), func.stack_frame_size);
                for (i, instr) in func.instructions.iter().enumerate() {
                    eprintln!("[DIAG] {}   [{}] {:?}", test_name, i, instr);
                }
            }
        }

        eprintln!("[DIAG] {} executing...", test_name);

        let mut executor = ProfessionalExecutor::new_with_jit(false)
            .unwrap_or_else(|e| panic!("{} JIT executor error: {:?}", test_name, e));
        
        // 启用 asm dump
        executor.enable_asm_dump();

        let result = executor.execute_with_jit(&lir);
        match result {
            Ok(exit_code) => {
                eprintln!(
                    "[DIAG] {} exit_code={}, expected={}",
                    test_name, exit_code, expected
                );
                assert_eq!(
                    exit_code, expected,
                    "{}: Expected {}, got {}",
                    test_name, expected, exit_code
                );
            }
            Err(e) => {
                eprintln!("[DIAG] {} JIT execution error: {:?}", test_name, e);
                panic!("{} JIT execution error: {:?}", test_name, e);
            }
        }

        eprintln!("[DIAG] {} PASSED", test_name);
    }
}
