#[cfg(test)]
mod cli_tests {
    use std::path::PathBuf;
    use std::fs;
    use karte_lexer::tokenize;
    use karte_parser::{parse_with_type_check, ParserMode};
    use karte_mir::{lower::lower_expr_to_mir, Statement, Value, lower::SCRIPT_ENTRY_POINT};
    use karte_lir::{lower::lower_mir_to_lir, optimization_pipeline::{OptimizationLevel, OptimizationPipeline}};
    use karte_codegen::vm::professional_executor::ProfessionalExecutor;

    #[test]
    fn test_compile_and_run_project_mode() {
        // 1. Locate the source file
        let mut project_file = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        project_file.pop();
        project_file.push("test_project");
        project_file.push("src");
        project_file.push("main.karte");

        let input = fs::read_to_string(&project_file).expect("Failed to read main.karte");

        // 2. Tokenize
        let (tokens, lex_diagnostics) = tokenize(&input);
        assert!(!lex_diagnostics.has_errors(), "Lexical analysis failed: {:?}", lex_diagnostics);

        // 3. Parse (Project Mode)
        let (result, parse_diagnostics) = parse_with_type_check(&tokens, ParserMode::Project);
        assert!(!parse_diagnostics.has_errors(), "Parsing failed: {:?}", parse_diagnostics);
        let result = result.expect("Parser returned no result");

        // 4. Lower to MIR
        let mut mir_program = lower_expr_to_mir(&result.expr).expect("MIR lowering failed");

        // 5. Handle Project Mode entry point (similar to CLI logic)
        if let Some(script_entry) = mir_program.functions.get(SCRIPT_ENTRY_POINT) {
            let is_trivial = if let Some(entry_block) =
                script_entry.basic_blocks.get(&script_entry.entry_block)
            {
                entry_block.statements.iter().all(|stmt| match stmt {
                    Statement::Assign {
                        source: Value::Unit,
                        ..
                    } => true,
                    _ => false,
                })
            } else {
                true
            };

            if is_trivial {
                if mir_program.functions.contains_key("main") {
                    mir_program.set_main("main".to_string());
                }
            }
        }

        // 6. Lower to LIR
        let mut lir_program = lower_mir_to_lir(&mir_program).expect("LIR lowering failed");

        // 7. Optimize
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir_program).expect("Optimization failed");

        // 8. Lower Instructions (prepare for execution)
        karte_lir::lower_program_instructions(&mut lir_program).expect("Instruction lowering failed");

        // 9. Execute with JIT
        // Note: JIT might not be available on all platforms, but we assume it is for this test environment (macOS/AArch64 or x86_64)
        // If JIT is not supported, ProfessionalExecutor::new_with_jit will return Err or fallback.
        // The CLI test asserted exit code 30.
        
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir_program).expect("JIT execution failed");

        assert_eq!(exit_code, 30, "Expected exit code 30 (10 + 20), got {}", exit_code);
    }
}
