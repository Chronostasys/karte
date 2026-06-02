#[cfg(test)]
#[cfg(target_arch = "aarch64")]
mod macos_sigsegv_diagnostic {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::panic;

    static HANDLER_INSTALLED: AtomicBool = AtomicBool::new(false);

    /// 安装 macOS SIGSEGV handler，捕获信号并输出诊断信息
    fn install_sigsegv_handler() {
        if HANDLER_INSTALLED.swap(true, Ordering::SeqCst) {
            return;
        }

        unsafe {
            libc::signal(libc::SIGSEGV, handler as libc::sighandler_t);
        }
    }

    extern "C" fn handler(sig: libc::c_int, info: *mut libc::siginfo_t, ctx: *mut std::ffi::c_void) {
        eprintln!("=== SIGSEGV DIAGNOSTIC ===");
        eprintln!("Signal: {}", sig);

        // 获取出错地址
        let addr = unsafe {
            let si = &*info;
            si.si_addr()
        };
        eprintln!("Faulting address: {:p}", addr);

        // 获取寄存器信息 (ucontext_t)
        unsafe {
            let uctx = ctx as *const libc::ucontext_t;
            let mctx = (*uctx).uc_mcontext;
            eprintln!("X0={:?}", mctx.__ss.__x[0]);
            eprintln!("X1={:?}", mctx.__ss.__x[1]);
            eprintln!("SP(X31)={:?}", mctx.__ss.__sp);
            eprintln!("X10={:?}", mctx.__ss.__x[10]);
            eprintln!("X11={:?}", mctx.__ss.__x[11]);
            eprintln!("PC={:?}", mctx.__ss.__pc);
            eprintln!("LR(X30)={:?}", mctx.__ss.__lr);
            eprintln!("FP(X29)={:?}", mctx.__ss.__fp);

            // 计算虚拟栈相关偏移
            let sp = mctx.__ss.__sp;
            let x10 = mctx.__ss.__x[10];
            eprintln!("SP - X10 = {:?}", (sp as isize) - (x10 as isize));
        }
        eprintln!("=== END DIAGNOSTIC ===");

        // 恢复默认 handler 并重新触发信号
        unsafe {
            libc::signal(libc::SIGSEGV, libc::SIG_DFL);
            libc::raise(libc::SIGSEGV);
        }
    }

    #[test]
    fn test_simple_arithmetic_with_diagnostic() {
        install_sigsegv_handler();

        let code = r#"fn main() -> number { 42 }"#;
        let (tokens, _) = karte_lexer::tokenize(code);
        let (parse_result, diagnostics) =
            karte_parser::parse_with_type_check(&tokens, karte_parser::ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Parse/type errors: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();

        let options = karte_mir::lower::LoweringOptions {
            known_functions: std::collections::HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = karte_mir::lower::lower_expr_to_mir_with_options(&ast, options)
            .expect("MIR lowering failed");

        let mut lir = karte_lir::lower::lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = karte_lir::OptimizationPipeline::new(karte_lir::OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");

        let mut executor =
            karte_codegen::vm::professional_executor::ProfessionalExecutor::new_with_jit(false)
                .expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 42, "Expected 42");
    }

    #[test]
    fn test_for_break_with_diagnostic() {
        install_sigsegv_handler();

        let code = r#"fn main() -> number { let sum = 0; for i in 0..10 { if i == 5 { break }; sum = sum + i }; sum }"#;
        let (tokens, _) = karte_lexer::tokenize(code);
        let (parse_result, diagnostics) =
            karte_parser::parse_with_type_check(&tokens, karte_parser::ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Parse/type errors: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();

        let options = karte_mir::lower::LoweringOptions {
            known_functions: std::collections::HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = karte_mir::lower::lower_expr_to_mir_with_options(&ast, options)
            .expect("MIR lowering failed");

        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false)
            .expect("Escape analysis failed");

        let mut lir = karte_lir::lower::lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = karte_lir::OptimizationPipeline::new(karte_lir::OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");

        let mut executor =
            karte_codegen::vm::professional_executor::ProfessionalExecutor::new_with_jit(false)
                .expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 10, "Expected 10");
    }

    #[test]
    fn test_complex_match_with_diagnostic() {
        install_sigsegv_handler();

        let code = r#"fn main() -> number { match 42 { 1 => 100, 42 => 200, _ => 300 } }"#;
        let (tokens, _) = karte_lexer::tokenize(code);
        let (parse_result, diagnostics) =
            karte_parser::parse_with_type_check(&tokens, karte_parser::ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Parse/type errors: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();

        let options = karte_mir::lower::LoweringOptions {
            known_functions: std::collections::HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = karte_mir::lower::lower_expr_to_mir_with_options(&ast, options)
            .expect("MIR lowering failed");

        let mut lir = karte_lir::lower::lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = karte_lir::OptimizationPipeline::new(karte_lir::OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");

        let mut executor =
            karte_codegen::vm::professional_executor::ProfessionalExecutor::new_with_jit(false)
                .expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 200, "Expected 200");
    }
}
