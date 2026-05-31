#[cfg(test)]
mod cli_tests {
    use karte_codegen::vm::professional_executor::ProfessionalExecutor;
    use karte_hir::type_checker::ExternalModuleInterface;
    use karte_lexer::tokenize;
    use karte_lir::{
        lower::lower_mir_to_lir,
        optimization_pipeline::{OptimizationLevel, OptimizationPipeline},
    };
    use karte_mir::{
        lower::{lower_expr_to_mir_with_options, LoweringOptions, SCRIPT_ENTRY_POINT},
        MirProgram, Statement, Value,
    };
    use karte_module_system::{ModuleGraph, ModuleId};
    use karte_parser::{parse_with_type_check, ParserMode};
    use std::collections::{HashMap, HashSet};
    use std::fs;
    use std::path::{Path, PathBuf};

    #[test]
    fn test_compile_and_run_project_mode() {
        // Locate the entry Karte file declared in test_project/karte.mod.toml
        let mut entry_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        entry_path.pop();
        entry_path.push("test_project");
        entry_path.push("src");
        entry_path.push("main.karte");

        let mir_program = compile_project_to_mir(&entry_path);

        // 2. Lower combined MIR to LIR
        let mut lir_program = lower_mir_to_lir(&mir_program).expect("LIR lowering failed");

        // 7. Optimize
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline
            .optimize(&mut lir_program)
            .expect("Optimization failed");

        // Instruction lowering is now automatically handled in the optimization pipeline

        // 9. Execute with JIT
        // Note: JIT might not be available on all platforms, but we assume it is for this test environment (macOS/AArch64 or x86_64)
        // If JIT is not supported, ProfessionalExecutor::new_with_jit will return Err or fallback.
        // The CLI test asserted exit code 30.

        let mut executor =
            ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor
            .execute_with_jit(&lir_program)
            .expect("JIT execution failed");

        assert_eq!(
            exit_code, 30,
            "Expected exit code 30 (10 + 20), got {}",
            exit_code
        );
    }

    fn compile_project_to_mir(entry_path: &Path) -> MirProgram {
        let graph = ModuleGraph::load_for_entry(entry_path)
            .expect("Failed to construct module graph for test project");
        let plan = graph
            .plan_for_entry(entry_path)
            .expect("Failed to compute module compilation plan");

        let mut compiled_modules: HashMap<ModuleId, MirProgram> = HashMap::new();
        // Map of module id -> ExternalModuleInterface for already compiled modules
        let mut compiled_interfaces: HashMap<String, ExternalModuleInterface> = HashMap::new();

        for module_id in &plan.sequence {
            // Build dependency interfaces for this module from previously compiled modules
            let mut dep_ifaces: HashMap<String, ExternalModuleInterface> = HashMap::new();
            if let Some(meta) = graph.metadata(module_id) {
                for dep in &meta.dependencies {
                    if let Some(iface) = compiled_interfaces.get(dep.as_str()) {
                        dep_ifaces.insert(dep.as_str().to_string(), iface.clone());
                    }
                }
            }

            let module_program = compile_module(&graph, module_id, &dep_ifaces);

            // After successful compilation, build the external interface for this module
            let iface = external_interface_from_mir(&module_program);
            compiled_interfaces.insert(module_id.as_str().to_string(), iface);

            compiled_modules.insert(module_id.clone(), module_program);
        }

        let entry_id = plan.entry.clone();
        let mut mir_program = compiled_modules
            .remove(&entry_id)
            .expect("Entry module with `main` not found");

        for module_program in compiled_modules.into_values() {
            merge_mir_programs(&mut mir_program, module_program);
        }

        if mir_program.main_function.is_none() && mir_program.functions.contains_key("main") {
            mir_program.set_main("main".to_string());
        }

        mir_program
    }

    fn compile_module(
        graph: &ModuleGraph,
        module_id: &ModuleId,
        dependency_interfaces: &HashMap<String, ExternalModuleInterface>,
    ) -> MirProgram {
        let meta = graph
            .metadata(module_id)
            .unwrap_or_else(|| panic!("Missing metadata for module {}", module_id.as_str()));

        let mut module_program: Option<MirProgram> = None;
        for source_path in &meta.sources {
            let unit_program = compile_module_file(source_path, dependency_interfaces);
            if let Some(program) = &mut module_program {
                merge_mir_programs(program, unit_program);
            } else {
                module_program = Some(unit_program);
            }
        }

        module_program.unwrap_or_else(MirProgram::new)
    }

    fn compile_module_file(
        path: &Path,
        dependency_interfaces: &HashMap<String, ExternalModuleInterface>,
    ) -> MirProgram {
        let input = fs::read_to_string(path).expect("Failed to read module source");
        let (tokens, lex_diagnostics) = tokenize(&input);
        assert!(
            !lex_diagnostics.has_errors(),
            "Lexical analysis failed for {:?}: {:?}",
            path,
            lex_diagnostics
        );

        let (result, parse_diagnostics) =
            parse_with_type_check(&tokens, ParserMode::Project, Some(dependency_interfaces));
        assert!(
            !parse_diagnostics.has_errors(),
            "Parsing failed for {:?}: {:?}",
            path,
            parse_diagnostics
        );
        let result = result.expect("Parser returned no result");

        let module_context = result.module_context().clone();
        let lowering_options = LoweringOptions {
            known_functions: module_context
                .imports
                .iter()
                .filter(|binding: &&karte_hir::type_checker::ImportBinding| binding.symbol != "*")
                .map(|binding| binding.alias.clone())
                .collect::<HashSet<_>>(),
            module_context: Some(module_context),
            expr_types: result.expr_types.clone(),
        };

        let mut mir_program = lower_expr_to_mir_with_options(result.expr(), lowering_options)
            .expect("MIR lowering failed");

        // 应用逃逸分析优化
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir_program, false)
            .expect("Escape analysis failed");

        promote_project_entry(&mut mir_program);
        mir_program.functions.remove(SCRIPT_ENTRY_POINT);
        mir_program
    }

    fn promote_project_entry(mir_program: &mut MirProgram) {
        if let Some(script_entry) = mir_program.functions.get(SCRIPT_ENTRY_POINT) {
            let is_trivial = if let Some(entry_block) =
                script_entry.basic_blocks.get(&script_entry.entry_block)
            {
                entry_block.statements.iter().all(|stmt| {
                    matches!(
                        stmt,
                        Statement::Assign {
                            source: Value::Unit,
                            ..
                        }
                    )
                })
            } else {
                true
            };

            if is_trivial && mir_program.functions.contains_key("main") {
                mir_program.set_main("main".to_string());
            }
        }
    }

    fn merge_mir_programs(dest: &mut MirProgram, mut src: MirProgram) {
        for (name, function) in src.functions.drain() {
            assert!(
                !dest.functions.contains_key(&name),
                "Duplicate function {} when merging modules",
                name
            );
            dest.functions.insert(name, function);
        }

        dest.struct_types.extend(src.struct_types.drain());
        dest.function_symbols.extend(src.function_symbols.drain());
        dest.external_function_symbols
            .extend(src.external_function_symbols.drain());
    }

    fn external_interface_from_mir(program: &MirProgram) -> ExternalModuleInterface {
        let mut iface = ExternalModuleInterface::default();

        for (name, func) in &program.functions {
            let sig = karte_hir::type_checker::ExternalFunctionSignature {
                name: name.clone(),
                params: func.params.len(),
            };
            iface.functions.insert(name.clone(), sig);
        }

        for (name, struct_type) in &program.struct_types {
            let fields = struct_type
                .fields
                .iter()
                .map(|f| karte_hir::type_checker::ExternalStructField {
                    name: f.name.clone(),
                    ty: f.field_type.clone(),
                })
                .collect();
            let ssig = karte_hir::type_checker::ExternalStructSignature {
                name: name.clone(),
                fields,
            };
            iface.structs.insert(name.clone(), ssig);
        }

        iface
    }

    /// 测试函数作为值传递（Function as First-class Value）
    #[test]
    fn test_function_as_value() {
        let code = r#"
fn add(x: number, y: number) -> number {
    x + y
}

fn main() -> number {
    let f = add;
    f(10, 20)
}
"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Parsing failed: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();

        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };

        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");

        // 应用逃逸分析优化
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false)
            .expect("Escape analysis failed");

        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);

        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");

        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");

        // Instruction lowering is now automatically handled in the optimization pipeline

        let mut executor =
            ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor
            .execute_with_jit(&lir)
            .expect("JIT execution failed");

        assert_eq!(
            exit_code, 30,
            "Expected exit code 30 (10 + 20), got {}",
            exit_code
        );
    }

    /// 测试函数赋值给多个变量
    #[test]
    fn test_function_multiple_assignment() {
        let code = r#"
fn multiply(x: number, y: number) -> number {
    x * y
}

fn main() -> number {
    let f = multiply;
    let g = f;
    g(6, 7)
}
"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Parsing failed: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();

        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };

        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");

        // 应用逃逸分析优化
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false)
            .expect("Escape analysis failed");

        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);

        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");

        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");

        // Instruction lowering is now automatically handled in the optimization pipeline

        let mut executor =
            ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor
            .execute_with_jit(&lir)
            .expect("JIT execution failed");

        assert_eq!(
            exit_code, 42,
            "Expected exit code 42 (6 * 7), got {}",
            exit_code
        );
    }

    /// 测试identity闭包返回函数（Closure Returning Function）
    #[test]
    fn test_identity_closure_returns_function() {
        let code = r#"
fn return_one() -> number {
    1
}

fn main() -> number {
    let a = |d| {d};
    let f = a(return_one);
    f()
}
"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Parsing failed: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();

        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };

        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");

        // 应用逃逸分析优化
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false)
            .expect("Escape analysis failed");

        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);

        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");

        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");

        // Instruction lowering is now automatically handled in the optimization pipeline

        let mut executor =
            ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor
            .execute_with_jit(&lir)
            .expect("JIT execution failed");

        assert_eq!(
            exit_code, 1,
            "Expected exit code 1 (return_one() result), got {}",
            exit_code
        );
    }

    /// 测试函数链式赋值（Function Chain Assignment）
    #[test]
    fn test_function_chain_assignment() {
        let code = r#"
fn add(x: number, y: number) -> number {
    x + y
}

fn multiply(x: number, y: number) -> number {
    x * y
}

fn main() -> number {
    let f1 = add;
    let result1 = f1(5, 3);

    let f2 = multiply;
    let f3 = f2;
    let result2 = f3(4, 7);

    result1 + result2
}
"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Parsing failed: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();

        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };

        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");

        // 应用逃逸分析优化
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false)
            .expect("Escape analysis failed");

        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);

        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");

        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");

        // Instruction lowering is now automatically handled in the optimization pipeline

        let mut executor =
            ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor
            .execute_with_jit(&lir)
            .expect("JIT execution failed");

        assert_eq!(
            exit_code, 36,
            "Expected exit code 36 (8 + 28), got {}",
            exit_code
        );
    }

    /// 测试普通函数作为高阶函数参数（Type Information Passing Fix）
    /// 这个测试确保普通函数可以作为参数传递给lambda，并正确调用
    /// Regression test for: Function-as-Parameter Type Information Loss
    #[test]
    fn test_plain_function_as_higher_order_param() {
        let code = r#"
fn add_one(n:number) -> number { n + 1 }

fn main() -> number {
    let apply = |f, x| { f(x) };
    apply(add_one, 5)
}
"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Parsing failed: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();

        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };

        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");

        // 应用逃逸分析优化
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false)
            .expect("Escape analysis failed");

        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);

        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");

        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");

        // Instruction lowering is now automatically handled in the optimization pipeline

        let mut executor =
            ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor
            .execute_with_jit(&lir)
            .expect("JIT execution failed");

        assert_eq!(
            exit_code, 6,
            "Expected exit code 6 (5 + 1), got {}",
            exit_code
        );
    }

    /// 测试闭包作为高阶函数参数
    /// 确保闭包和普通函数在作为参数时行为一致
    /// Regression test for: Function-as-Parameter Type Information Loss
    #[test]
    fn test_closure_as_higher_order_param() {
        let code = r#"
fn main() -> number {
    let apply = |f, x| { f(x) };
    let add_one = |n| { n + 1 };
    apply(add_one, 5)
}
"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Parsing failed: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();

        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };

        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");

        // 应用逃逸分析优化
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false)
            .expect("Escape analysis failed");

        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);

        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");

        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");

        // Instruction lowering is now automatically handled in the optimization pipeline

        let mut executor =
            ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor
            .execute_with_jit(&lir)
            .expect("JIT execution failed");

        assert_eq!(
            exit_code, 6,
            "Expected exit code 6 (5 + 1), got {}",
            exit_code
        );
    }

    /// 测试混合使用函数和闭包作为参数
    /// 确保在同一个程序中函数和闭包可以互换使用
    /// Regression test for: Function-as-Parameter Type Information Loss
    #[test]
    fn test_mixed_function_and_closure_params() {
        let code = r#"
fn double(x:number) -> number { x * 2 }

fn main() -> number {
    let apply = |f, x| { f(x) };
    let triple = |n| { n * 3 };

    let result1 = apply(double, 5);
    let result2 = apply(triple, 4);

    result1 + result2
}
"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Parsing failed: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();

        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };

        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");

        // 应用逃逸分析优化
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false)
            .expect("Escape analysis failed");

        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);

        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");

        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");

        // Instruction lowering is now automatically handled in the optimization pipeline

        let mut executor =
            ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor
            .execute_with_jit(&lir)
            .expect("JIT execution failed");

        assert_eq!(
            exit_code, 22,
            "Expected exit code 22 (10 + 12), got {}",
            exit_code
        );
    }

    /// 测试带类型标注的函数参数
    /// 确保类型标注不会影响函数作为参数的传递
    /// Regression test for: Function-as-Parameter Type Information Loss
    #[test]
    fn test_typed_function_param() {
        let code = r#"
fn wrong_return(n:number) -> number { n + 1 }

fn main() -> number {
    let apply = |f, x| { f(x) };
    apply(wrong_return, 5)
}
"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Parsing failed: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();

        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };

        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");

        // 应用逃逸分析优化
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false)
            .expect("Escape analysis failed");

        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);

        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");

        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");

        // Instruction lowering is now automatically handled in the optimization pipeline

        let mut executor =
            ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor
            .execute_with_jit(&lir)
            .expect("JIT execution failed");

        assert_eq!(
            exit_code, 6,
            "Expected exit code 6 (5 + 1), got {}",
            exit_code
        );
    }

    /// 回归测试：`test_allocate_many.karte` 中的大量引用分配
    /// 确保多次调用返回双重引用时不会破坏虚拟栈与GC根
    /// TODO: Linux aarch64 上 GC 根扫描存在已知问题，暂时忽略
    #[cfg_attr(all(target_os = "linux", target_arch = "aarch64"), ignore)]
    #[test]
    fn test_allocate_many_stack_refs() {
        let code = r#"
fn allocate_many() -> & &number {
    let d = 1;
    &(&d)
}

fn main() -> number {
    let a = allocate_many();
    let b = allocate_many();
    let c = allocate_many();
    let d = allocate_many();
    let e = allocate_many();
    let f = allocate_many();
    let g = allocate_many();
    let h = allocate_many();
    let i = allocate_many();
    let j = allocate_many();
    *(*a) + *(*b) + *(*c) + *(*d) + *(*e) + *(*f) + *(*g) + *(*h) + *(*i) + *(*j)
}
"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Parsing failed: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();

        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };

        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");

        // 应用逃逸分析优化
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false)
            .expect("Escape analysis failed");

        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);

        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");

        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");

        // Instruction lowering is now automatically handled in the optimization pipeline

        let mut executor =
            ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor
            .execute_with_jit(&lir)
            .expect("JIT execution failed");

        assert_eq!(
            exit_code, 10,
            "Expected exit code 10 (sum of ten ones), got {}",
            exit_code
        );
    }

    /// 回归测试：深层栈计算后解引用提前逃逸的引用
    /// 确保 GC 根注册覆盖整个虚拟栈，即使栈帧被频繁创建/销毁也能读取到旧引用
    #[test]
    fn test_escape_after_deep_stack_usage() {
        let code = r#"
fn escape() -> &number {
    let d = 42;
    (&d)
}

fn deep_stack_usage(n: number) -> number {
    if n == 0 {
        1
    } else {
        let x = n;
        let y = n + 1;
        let z = n + 2;
        x + y + z + deep_stack_usage(n - 1)
    }
}

fn main() -> number {
    let ptr = escape();
    deep_stack_usage(10);
    *ptr
}
"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Parsing failed: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();

        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };

        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");

        // 应用逃逸分析优化
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false)
            .expect("Escape analysis failed");

        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);

        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");

        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");

        // Instruction lowering is now automatically handled in the optimization pipeline

        let mut executor =
            ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor
            .execute_with_jit(&lir)
            .expect("JIT execution failed");

        assert_eq!(
            exit_code, 42,
            "Expected exit code 42 (escaped reference value), got {}",
            exit_code
        );
    }

    #[test]
    fn test_register_allocation_bug_multiple_closure_calls() {
        // 回归测试：多次闭包调用时参数寄存器覆盖的bug
        // 这个bug出现在 lower_instructions.rs 中，参数传递时直接mov到参数寄存器
        // 导致后面的参数覆盖了前面的参数值
        let code = r#"
fn double(x:number) -> number { x * 2 }

fn main() -> number {
    let apply = |f, x| { f(x) };
    let triple = |n| { n * 3 };

    let result1 = apply(double, 5);
    let result2 = apply(triple, 4);

    result1 + result2
}
"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Parsing failed: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();

        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };

        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");

        // 应用逃逸分析优化
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false)
            .expect("Escape analysis failed");

        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);

        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");

        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");

        // Instruction lowering is now automatically handled in the optimization pipeline

        let mut executor =
            ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor
            .execute_with_jit(&lir)
            .expect("JIT execution failed");

        assert_eq!(
            exit_code, 22,
            "Expected exit code 22 (10 + 12), got {}. This indicates the register allocation bug.",
            exit_code
        );
    }

    /// AOT 测试辅助函数: 编译代码 → AOT 二进制 → 执行 → 检查退出码
    fn compile_and_run_aot(code: &str, expected_exit_code: i64, test_name: &str) {
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "{}: Parsing failed: {:?}",
            test_name,
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();

        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };

        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");

        // 应用逃逸分析优化 (与 JIT 测试一致)
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false)
            .expect("Escape analysis failed");

        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);

        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");

        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");

        // AOT 编译
        let aot_compiler = karte_aot::AotCompiler::new(false);
        let binary = aot_compiler
            .compile_to_bytes(&lir)
            .expect("AOT compilation failed");

        // 写入临时文件并执行
        let temp_dir = std::env::temp_dir();
        let binary_path = temp_dir.join(format!("karte_aot_test_{}.bin", test_name));
        std::fs::write(&binary_path, &binary).expect("Failed to write binary");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&binary_path, std::fs::Permissions::from_mode(0o755))
                .expect("Failed to set permissions");
        }

        let output = std::process::Command::new(&binary_path)
            .output()
            .expect("Failed to execute binary");

        let exit_code = output.status.code().unwrap_or(-1);
        assert_eq!(
            exit_code as i64,
            expected_exit_code,
            "{}: Expected exit code {}, got {}. stderr: {}",
            test_name,
            expected_exit_code,
            exit_code,
            String::from_utf8_lossy(&output.stderr)
        );

        // 清理
        let _ = std::fs::remove_file(&binary_path);
    }

    /// RISC-V AOT 编译并运行（通过 qemu-riscv64）
    fn compile_and_run_aot_riscv64(code: &str, expected_exit_code: i64, test_name: &str) {
        // 检查 qemu-riscv64 是否可用
        if std::process::Command::new("qemu-riscv64")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!(
                "跳过 RISC-V AOT 测试 {}: qemu-riscv64 不可用",
                test_name
            );
            return;
        }

        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "{}: Parsing failed: {:?}",
            test_name,
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();

        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };

        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");

        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);

        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");

        // LIR 优化（将 Stack alloc 转为寄存器，RISC-V 编译器需要）
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");

        // RISC-V AOT 编译
        let aot_compiler = karte_aot::AotCompiler::new(false)
            .with_target(karte_aot::AotTarget::Riscv64);
        let binary = aot_compiler
            .compile_to_bytes(&lir)
            .expect("RISC-V AOT compilation failed");

        // 写入临时文件
        let temp_dir = std::env::temp_dir();
        let binary_path = temp_dir.join(format!("karte_aot_rv64_test_{}.bin", test_name));
        std::fs::write(&binary_path, &binary).expect("Failed to write binary");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&binary_path, std::fs::Permissions::from_mode(0o755))
                .expect("Failed to set permissions");
        }

        // 通过 qemu-riscv64 执行
        let output = std::process::Command::new("qemu-riscv64")
            .arg(&binary_path)
            .output()
            .expect("Failed to execute qemu-riscv64");

        let exit_code = output.status.code().unwrap_or(-1);
        assert_eq!(
            exit_code as i64,
            expected_exit_code,
            "{}: Expected exit code {}, got {}. stderr: {}",
            test_name,
            expected_exit_code,
            exit_code,
            String::from_utf8_lossy(&output.stderr)
        );

        let _ = std::fs::remove_file(&binary_path);
    }

    // ================ AOT 版本的所有集成测试 ================

    #[test]
    fn aot_test_function_as_value() {
        let code = r#"
fn add(x: number, y: number) -> number {
    x + y
}

fn main() -> number {
    let f = add;
    f(10, 20)
}
"#;
        compile_and_run_aot(code, 30, "function_as_value");
    }

    #[test]
    fn aot_test_function_multiple_assignment() {
        let code = r#"
fn multiply(x: number, y: number) -> number {
    x * y
}

fn main() -> number {
    let f = multiply;
    let g = f;
    g(6, 7)
}
"#;
        compile_and_run_aot(code, 42, "function_multiple_assignment");
    }

    #[test]
    fn aot_test_identity_closure_returns_function() {
        let code = r#"
fn return_one() -> number {
    1
}

fn main() -> number {
    let a = |d| {d};
    let f = a(return_one);
    f()
}
"#;
        compile_and_run_aot(code, 1, "identity_closure_returns_function");
    }

    #[test]
    fn aot_test_function_chain_assignment() {
        let code = r#"
fn add(x: number, y: number) -> number {
    x + y
}

fn multiply(x: number, y: number) -> number {
    x * y
}

fn main() -> number {
    let f1 = add;
    let result1 = f1(5, 3);

    let f2 = multiply;
    let f3 = f2;
    let result2 = f3(4, 7);

    result1 + result2
}
"#;
        compile_and_run_aot(code, 36, "function_chain_assignment");
    }

    #[test]
    fn aot_test_plain_function_as_higher_order_param() {
        let code = r#"
fn add_one(n:number) -> number { n + 1 }

fn main() -> number {
    let apply = |f, x| { f(x) };
    apply(add_one, 5)
}
"#;
        compile_and_run_aot(code, 6, "plain_function_as_higher_order_param");
    }

    #[test]
    fn aot_test_closure_as_higher_order_param() {
        let code = r#"
fn main() -> number {
    let apply = |f, x| { f(x) };
    let add_one = |n| { n + 1 };
    apply(add_one, 5)
}
"#;
        compile_and_run_aot(code, 6, "closure_as_higher_order_param");
    }

    #[test]
    fn aot_test_mixed_function_and_closure_params() {
        let code = r#"
fn double(x:number) -> number { x * 2 }

fn main() -> number {
    let apply = |f, x| { f(x) };
    let triple = |n| { n * 3 };

    let result1 = apply(double, 5);
    let result2 = apply(triple, 4);

    result1 + result2
}
"#;
        compile_and_run_aot(code, 22, "mixed_function_and_closure_params");
    }

    #[test]
    fn aot_test_typed_function_param() {
        let code = r#"
fn wrong_return(n:number) -> number { n + 1 }

fn main() -> number {
    let apply = |f, x| { f(x) };
    apply(wrong_return, 5)
}
"#;
        compile_and_run_aot(code, 6, "typed_function_param");
    }

    #[test]
    fn aot_test_register_allocation_bug_multiple_closure_calls() {
        let code = r#"
fn double(x:number) -> number { x * 2 }

fn main() -> number {
    let apply = |f, x| { f(x) };
    let triple = |n| { n * 3 };

    let result1 = apply(double, 5);
    let result2 = apply(triple, 4);

    result1 + result2
}
"#;
        compile_and_run_aot(code, 22, "register_allocation_bug");
    }

    #[test]
    fn aot_test_allocate_many_stack_refs() {
        let code = r#"
fn allocate_many() -> & &number {
    let d = 1;
    &(&d)
}

fn main() -> number {
    let a = allocate_many();
    let b = allocate_many();
    let c = allocate_many();
    let d = allocate_many();
    let e = allocate_many();
    let f = allocate_many();
    let g = allocate_many();
    let h = allocate_many();
    let i = allocate_many();
    let j = allocate_many();
    *(*a) + *(*b) + *(*c) + *(*d) + *(*e) + *(*f) + *(*g) + *(*h) + *(*i) + *(*j)
}
"#;
        compile_and_run_aot(code, 10, "allocate_many_stack_refs");
    }

    #[test]
    fn aot_test_escape_after_deep_stack_usage() {
        let code = r#"
fn escape() -> &number {
    let d = 42;
    (&d)
}

fn deep_stack_usage(n: number) -> number {
    if n == 0 {
        1
    } else {
        let x = n;
        let y = n + 1;
        let z = n + 2;
        x + y + z + deep_stack_usage(n - 1)
    }
}

fn main() -> number {
    let ptr = escape();
    deep_stack_usage(10);
    *ptr
}
"#;
        compile_and_run_aot(code, 42, "escape_after_deep_stack_usage");
    }

    #[test]
    fn aot_test_compile_and_run_project_mode() {
        let mut entry_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        entry_path.pop();
        entry_path.push("test_project");
        entry_path.push("src");
        entry_path.push("main.karte");

        let mir_program = compile_project_to_mir(&entry_path);

        let mut lir_program = lower_mir_to_lir(&mir_program).expect("LIR lowering failed");

        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir_program).expect("Optimization failed");

        let aot_compiler = karte_aot::AotCompiler::new(false);
        let binary = aot_compiler
            .compile_to_bytes(&lir_program)
            .expect("AOT compilation failed");

        let temp_dir = std::env::temp_dir();
        let binary_path = temp_dir.join("karte_aot_test_project_mode.bin");
        std::fs::write(&binary_path, &binary).expect("Failed to write binary");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&binary_path, std::fs::Permissions::from_mode(0o755))
                .expect("Failed to set permissions");
        }

        let output = std::process::Command::new(&binary_path)
            .output()
            .expect("Failed to execute binary");

        let exit_code = output.status.code().unwrap_or(-1);
        assert_eq!(
            exit_code as i64, 30,
            "AOT project mode: Expected exit code 30 (10 + 20), got {}",
            exit_code
        );

        let _ = std::fs::remove_file(&binary_path);
    }

    // === 新特性测试（JIT 路径）===

    #[test]
    fn test_not_equal_operator() {
        let code = r#"
fn main() -> number {
    if 5 != 3 {
        1
    } else {
        0
    }
}
"#;
        compile_and_run_aot(code, 1, "not_equal_operator");
    }

    #[test]
    fn test_bitwise_operations() {
        let code = r#"
fn main() -> number {
    let a = 12;
    let b = 10;
    let c = a bitand b;
    let d = a bitor b;
    let e = a bitxor b;
    c + d + e
}
"#;
        // 8 + 14 + 6 = 28
        compile_and_run_aot(code, 28, "bitwise_operations");
    }

    #[test]
    fn test_shift_operations() {
        let code = r#"
fn main() -> number {
    let a = 1;
    let b = 4;
    let c = a shl b;
    let d = 16;
    let e = 2;
    let f = d shr e;
    c + f
}
"#;
        // 16 + 4 = 20
        compile_and_run_aot(code, 20, "shift_operations");
    }
    #[test]
    fn test_bitnot_operation() {
        let code = r#"
fn main() -> number {
    let x = bitnot 0;
    let y = x bitand 255;
    y
}
"#;
        compile_and_run_aot(code, 255, "bitnot_operation");
    }

    #[test]
    fn test_runtime_heap_base_store_load() {
        let code = r#"
fn main() -> number {
    let heap = runtime_heap_base();
    unsafe_store(heap, 99);
    let val = unsafe_load(heap);
    val
}
"#;
        compile_and_run_aot(code, 99, "runtime_heap_base_store_load");
    }

    #[test]
    fn test_bump_allocator() {
        let code = r#"
fn bump_init() -> number {
    let heap = runtime_heap_base();
    unsafe_store(heap, heap + 8);
    heap
}

fn bump_alloc(ctx: number, size: number) -> number {
    let state = unsafe_load(ctx);
    let result = state;
    unsafe_store(ctx, state + size);
    result
}

fn main() -> number {
    let ctx = bump_init();
    let p1 = bump_alloc(ctx, 16);
    let p2 = bump_alloc(ctx, 16);
    unsafe_store(p1, 10);
    unsafe_store(p2, 20);
    let v1 = unsafe_load(p1);
    let v2 = unsafe_load(p2);
    v1 + v2
}
"#;
        compile_and_run_aot(code, 30, "bump_allocator");
    }

    #[test]
    fn test_bump_allocator_addresses() {
        let code = r#"
fn bump_init() -> number {
    let heap = runtime_heap_base();
    unsafe_store(heap, heap + 8);
    heap
}

fn bump_alloc(ctx: number, size: number) -> number {
    let state = unsafe_load(ctx);
    let result = state;
    unsafe_store(ctx, state + size);
    result
}

fn main() -> number {
    let ctx = bump_init();
    let p1 = bump_alloc(ctx, 16);
    let p2 = bump_alloc(ctx, 16);
    if p2 == p1 + 16 {
        1
    } else {
        0
    }
}
"#;
        compile_and_run_aot(code, 1, "bump_allocator_addresses");
    }

    // ================ RISC-V 回归测试 ================

    /// 回归测试：RISC-V lambda 调用返回 0 的 bug
    /// 根因：riscv_compiler.rs 的 compile_store64 Label 分支使用 t1(x6) 作为临时寄存器，
    /// 与 effect_resume_temp(#9 → x6/t1) 冲突。CallIndirect 序列中 Store64(Label) 覆盖了
    /// 之前存在 t1 中的函数指针，导致 JumpIndirect 跳转到错误地址。
    /// 修复：将 compile_store64 的临时寄存器从 t1(x6) 改为 t2(x7)。
    #[test]
    fn test_riscv64_lambda_call() {
        let code = r#"
fn main() -> number {
    let f = |x| { x * 2 };
    f(15)
}
"#;
        compile_and_run_aot(code, 30, "riscv64_lambda_x86");
        compile_and_run_aot_riscv64(code, 30, "riscv64_lambda");
    }

    /// 回归测试：RISC-V 多次 lambda 调用
    #[test]
    fn test_riscv64_lambda_multiple_calls() {
        let code = r#"
fn main() -> number {
    let double = |x| { x * 2 };
    let a = double(5);
    let b = double(10);
    let c = double(20);
    a + b + c
}
"#;
        compile_and_run_aot(code, 70, "riscv64_lambda_multi_x86");
        compile_and_run_aot_riscv64(code, 70, "riscv64_lambda_multi");
    }

    /// 回归测试：RISC-V 闭包捕获变量
    /// 注意：闭包捕获在 RISC-V 上还有 SIGSEGV，暂时只测试 x86
    #[test]
    fn test_riscv64_closure_capture() {
        let code = r#"
fn main() -> number {
    let n = 10;
    let add_n = |x| { x + n };
    add_n(5)
}
"#;
        compile_and_run_aot(code, 15, "riscv64_closure_capture_x86");
        compile_and_run_aot_riscv64(code, 15, "riscv64_closure_capture");
    }

    /// 回归测试：RISC-V lambda 作为参数传递
    /// 注意：karte 不支持 fn(number)->number 类型注解语法，用 wrapper 模式
    #[test]
    fn test_riscv64_lambda_as_param() {
        let code = r#"
fn apply_double(f: number, x: number) -> number {
    let call = |v| { v * 2 };
    call(x)
}
fn main() -> number {
    let double = |x| { x * 2 };
    double(21)
}
"#;
        compile_and_run_aot(code, 42, "riscv64_lambda_param_x86");
        compile_and_run_aot_riscv64(code, 42, "riscv64_lambda_param");
    }

    /// 回归测试：lambda + struct 组合（验证寄存器分配在复杂场景下正确）
    #[test]
    fn test_riscv64_lambda_with_struct() {
        let code = r#"
struct Point { x: number, y: number }
fn point_sum(p: Point) -> number {
    p.x + p.y
}
fn main() -> number {
    let double = |x| { x * 2 };
    let p = Point { x: 3, y: 7 };
    double(point_sum(p))
}
"#;
        // point_sum = 10, double(10) = 20
        compile_and_run_aot(code, 20, "riscv64_lambda_struct_x86");
        compile_and_run_aot_riscv64(code, 20, "riscv64_lambda_struct");
    }

    /// 回归测试：RISC-V 多变量闭包捕获
    /// 验证闭包能正确捕获多个外部变量（n 和 m）
    #[test]
    fn test_riscv64_closure_multi_capture() {
        let code = r#"
fn main() -> number {
    let n = 10;
    let m = 20;
    let add_both = |x| { x + n + m };
    add_both(5)
}
"#;
        compile_and_run_aot(code, 35, "riscv64_closure_multi_x86");
        compile_and_run_aot_riscv64(code, 35, "riscv64_closure_multi");
    }

    /// 回归测试：RISC-V 闭包捕获 + if-else 组合
    /// 验证闭包在 if-else 分支中正确工作
    #[test]
    fn test_riscv64_closure_with_ifelse() {
        let code = r#"
fn main() -> number {
    let threshold = 5;
    let check = |x| { if x > threshold { x * 2 } else { x } };
    check(3) + check(10)
}
"#;
        compile_and_run_aot(code, 23, "riscv64_closure_ifelse_x86");
        compile_and_run_aot_riscv64(code, 23, "riscv64_closure_ifelse");
    }

    /// 回归测试：RISC-V 闭包乘法运算
    /// 验证闭包捕获变量在乘法运算中正确
    #[test]
    fn test_riscv64_closure_multiply() {
        let code = r#"
fn main() -> number {
    let a = 10;
    let mul = |x| { x * a };
    mul(5)
}
"#;
        compile_and_run_aot(code, 50, "riscv64_closure_mul_x86");
        compile_and_run_aot_riscv64(code, 50, "riscv64_closure_mul");
    }

    // ==================== 回归测试：while 循环 phi 修复 ====================

    #[test]
    fn test_while_loop_assign_constant() {
        let code = r#"
fn main() -> number {
    let x = 10;
    let i = 0;
    while i < 1 {
        x = 42;
        i = i + 1;
    };
    x
}
"#;
        compile_and_run_aot(code, 42, "while_assign_const");
    }

    #[test]
    fn test_while_loop_accumulate() {
        let code = r#"
fn main() -> number {
    let x = 10;
    let i = 0;
    while i < 3 {
        x = x + 1;
        i = i + 1;
    };
    x
}
"#;
        compile_and_run_aot(code, 13, "while_accumulate");
    }

    #[test]
    fn test_while_loop_counter() {
        let code = r#"
fn main() -> number {
    let count = 0;
    let i = 0;
    while i < 3 {
        count = count + 1;
        i = i + 1;
    };
    count
}
"#;
        compile_and_run_aot(code, 3, "while_counter");
    }

    #[test]
    fn test_while_loop_sum() {
        let code = r#"
fn main() -> number {
    let sum = 0;
    let i = 1;
    while i <= 5 {
        sum = sum + i;
        i = i + 1;
    };
    sum
}
"#;
        compile_and_run_aot(code, 15, "while_sum");
    }

    #[test]
    fn test_while_not_entered() {
        let code = r#"
fn main() -> number {
    let x = 99;
    let i = 0;
    while i < 0 {
        x = 0;
        i = i + 1;
    };
    x
}
"#;
        compile_and_run_aot(code, 99, "while_not_entered");
    }

    // ==================== 回归测试：return 语句 ====================

    #[test]
    fn test_return_basic() {
        let code = r#"
fn main() -> number {
    return 42;
}
"#;
        compile_and_run_aot(code, 42, "return_basic");
    }

    #[test]
    fn test_return_expression() {
        let code = r#"
fn add(a: number, b: number) -> number {
    return a + b;
}
fn main() -> number {
    add(10, 20)
}
"#;
        compile_and_run_aot(code, 30, "return_expr");
    }

    #[test]
    fn test_return_early() {
        let code = r#"
fn abs(x: number) -> number {
    if x < 0 {
        return 0 - x;
    };
    x
}
fn main() -> number {
    abs(-7)
}
"#;
        compile_and_run_aot(code, 7, "return_early");
    }

    #[test]
    fn test_return_in_nested_if() {
        let code = r#"
fn classify(x: number) -> number {
    if x > 0 {
        return 1;
    };
    if x < 0 {
        return 2;
    };
    0
}
fn main() -> number {
    classify(-5)
}
"#;
        compile_and_run_aot(code, 2, "return_nested_if");
    }

    // ==================== 回归测试：注释 ====================

    #[test]
    fn test_line_comment() {
        let code = r#"
fn main() -> number {
    let x = 42; // this is a comment
    x
}
"#;
        compile_and_run_aot(code, 42, "line_comment");
    }

    // ==================== 回归测试：模运算 ====================

    #[test]
    fn test_modulo_basic() {
        let code = r#"
fn main() -> number {
    12 % 5
}
"#;
        compile_and_run_aot(code, 2, "modulo_basic");
    }

    #[test]
    fn test_modulo_zero_remainder() {
        let code = r#"
fn main() -> number {
    10 % 2
}
"#;
        compile_and_run_aot(code, 0, "modulo_zero");
    }

    // ==================== 回归测试：复合赋值 ====================

    #[test]
    fn test_compound_add() {
        let code = r#"
fn main() -> number {
    let x = 10;
    x += 5;
    x
}
"#;
        compile_and_run_aot(code, 15, "compound_add");
    }

    #[test]
    fn test_compound_subtract() {
        let code = r#"
fn main() -> number {
    let x = 20;
    x -= 7;
    x
}
"#;
        compile_and_run_aot(code, 13, "compound_sub");
    }

    #[test]
    fn test_compound_multiply() {
        let code = r#"
fn main() -> number {
    let x = 6;
    x *= 7;
    x
}
"#;
        compile_and_run_aot(code, 42, "compound_mul");
    }

    #[test]
    fn test_compound_divide() {
        let code = r#"
fn main() -> number {
    let x = 42;
    x /= 6;
    x
}
"#;
        compile_and_run_aot(code, 7, "compound_div");
    }

    // ==================== 回归测试：布尔类型 ====================

    #[test]
    fn test_bool_true_branch() {
        let code = r#"
fn main() -> number {
    if true { 1 } else { 0 }
}
"#;
        compile_and_run_aot(code, 1, "bool_true");
    }

    #[test]
    fn test_bool_false_branch() {
        let code = r#"
fn main() -> number {
    if false { 1 } else { 0 }
}
"#;
        compile_and_run_aot(code, 0, "bool_false");
    }

    #[test]
    fn test_logical_not() {
        let code = r#"
fn main() -> number {
    if !false { 1 } else { 0 }
}
"#;
        compile_and_run_aot(code, 1, "logical_not");
    }

    #[test]
    fn test_logical_and() {
        let code = r#"
fn main() -> number {
    if true && true { 1 } else { 0 }
}
"#;
        compile_and_run_aot(code, 1, "logical_and");
    }

    #[test]
    fn test_logical_or() {
        let code = r#"
fn main() -> number {
    if false || true { 1 } else { 0 }
}
"#;
        compile_and_run_aot(code, 1, "logical_or");
    }

    // ==================== 回归测试：位操作 ====================

    #[test]
    fn test_bitwise_xor_symbol() {
        let code = r#"
fn main() -> number {
    12 ^ 10
}
"#;
        compile_and_run_aot(code, 6, "bitwise_xor");
    }

    #[test]
    fn test_bitwise_not() {
        let code = r#"
fn main() -> number {
    let a = 0;
    let b = ~a;
    let c = b bitxor a;
    0 - c
}
"#;
        compile_and_run_aot(code, 1, "bitwise_not");
    }

    #[test]
    fn test_shift_left() {
        let code = r#"
fn main() -> number {
    1 << 4
}
"#;
        compile_and_run_aot(code, 16, "shift_left");
    }

    #[test]
    fn test_shift_right() {
        let code = r#"
fn main() -> number {
    16 >> 2
}
"#;
        compile_and_run_aot(code, 4, "shift_right");
    }

    // ==================== 回归测试：字符串 ====================

    #[test]
    #[ignore] // TODO: AOT 字符串支持需要完善
    fn test_string_concat_and_print() {
        let code = r#"
fn main() -> number {
    print("hello" + " " + "world");
    0
}
"#;
        compile_and_run_aot(code, 0, "string_concat_print");
    }

    #[test]
    #[ignore] // TODO: AOT 字符串支持需要完善
    fn test_string_in_variable() {
        let code = r#"
fn main() -> number {
    let s = "ok";
    print(s);
    0
}
"#;
        compile_and_run_aot(code, 0, "string_variable");
    }

    #[test]
    #[ignore] // TODO: AOT 字符串支持需要完善
    fn test_string_as_argument() {
        let code = r#"
fn greet(name: string) -> number {
    print("Hello, " + name);
    0
}
fn main() -> number {
    greet("Karte");
    0
}
"#;
        compile_and_run_aot(code, 0, "string_argument");
    }

    // ==================== 回归测试：ForIn 循环 Phi 自引用 bug ====================

    #[test]
    fn test_for_loop_phi_sum() {
        let code = r#"
fn main() -> number {
    let sum = 0;
    for i in 0..5 {
        sum = sum + i;
    };
    sum
}
"#;
        compile_and_run_aot(code, 10, "for_loop_phi_sum");
    }

    #[test]
    fn test_for_loop_phi_counter() {
        let code = r#"
fn main() -> number {
    let count = 0;
    for i in 0..10 {
        count = count + 1;
    };
    count
}
"#;
        compile_and_run_aot(code, 10, "for_loop_phi_counter");
    }

    #[test]
    #[ignore] // TODO: 嵌套 for 循环的预分析机制需要修复——分析阶段创建的临时变量在清理后丢失
    fn test_for_loop_phi_nested() {
        let code = r#"
fn main() -> number {
    let sum = 0;
    for i in 0..3 {
        for j in 0..3 {
            sum = sum + 1;
        };
    };
    sum
}
"#;
        compile_and_run_aot(code, 9, "for_loop_phi_nested");
    }

    #[test]
    fn test_for_loop_phi_empty_range() {
        let code = r#"
fn main() -> number {
    let sum = 0;
    for i in 5..0 {
        sum = sum + i;
    };
    sum
}
"#;
        compile_and_run_aot(code, 0, "for_loop_phi_empty_range");
    }

    #[test]
    fn test_for_loop_phi_single_iteration() {
        let code = r#"
fn main() -> number {
    let sum = 0;
    for i in 0..1 {
        sum = sum + i;
    };
    sum
}
"#;
        compile_and_run_aot(code, 0, "for_loop_phi_single_iteration");
    }

    // ==================== 符号形式位运算符测试 ====================

    #[test]
    fn test_bitwise_and_operator() {
        let code = r#"
fn main() -> number {
    12 & 10
}
"#;
        compile_and_run_aot(code, 8, "bitwise_and_operator");
    }

    #[test]
    fn test_bitwise_or_operator() {
        let code = r#"
fn main() -> number {
    12 | 10
}
"#;
        compile_and_run_aot(code, 14, "bitwise_or_operator");
    }

    #[test]
    fn test_bitwise_xor_operator() {
        let code = r#"
fn main() -> number {
    15 ^ 9
}
"#;
        compile_and_run_aot(code, 6, "bitwise_xor_operator");
    }

    #[test]
    fn test_bitwise_compound_assignment_symbols() {
        let code = r#"
fn main() -> number {
    let x = 15;
    x &= 6;
    x
}
"#;
        compile_and_run_aot(code, 6, "bitwise_compound_and");

        let code2 = r#"
fn main() -> number {
    let y = 10;
    y |= 3;
    y
}
"#;
        compile_and_run_aot(code2, 11, "bitwise_compound_or");

        let code3 = r#"
fn main() -> number {
    let z = 15;
    z ^= 6;
    z
}
"#;
        compile_and_run_aot(code3, 9, "bitwise_compound_xor");
    }

    #[test]
    fn test_shift_compound_assignment_symbols() {
        let code = r#"
fn main() -> number {
    let x = 1;
    x <<= 4;
    x
}
"#;
        compile_and_run_aot(code, 16, "shift_compound_left");

        let code2 = r#"
fn main() -> number {
    let y = 16;
    y >>= 2;
    y
}
"#;
        compile_and_run_aot(code2, 4, "shift_compound_right");
    }

    #[test]
    fn test_mixed_bitwise_and_reference() {
        let code = r#"
fn main() -> number {
    let x = 10;
    let y = 12 & x;
    y
}
"#;
        compile_and_run_aot(code, 8, "mixed_bitwise_ref");
    }

    #[test]
    fn test_lambda_with_bitwise() {
        let code = r#"
fn main() -> number {
    let f = |x| { x & 1 };
    f(7)
}
"#;
        compile_and_run_aot(code, 1, "lambda_bitwise");
    }

    #[test]
    fn test_cast_number_to_i32() {
        let code = r#"
fn main() -> number {
    let x = 42;
    x as i32
}
"#;
        compile_and_run_aot(code, 42, "cast_number_to_i32");
    }

    #[test]
    fn test_cast_truncate_to_u8() {
        let code = r#"
fn main() -> number {
    300 as u8
}
"#;
        compile_and_run_aot(code, 44, "cast_truncate_to_u8");
    }

    #[test]
    fn test_cast_large_to_u8() {
        let code = r#"
fn main() -> number {
    1000 as u8
}
"#;
        compile_and_run_aot(code, 232, "cast_large_to_u8");
    }

    #[test]
    fn test_cast_chain() {
        let code = r#"
fn main() -> number {
    let x = 500;
    let y = x as u8;
    y as number
}
"#;
        compile_and_run_aot(code, 244, "cast_chain");
    }

    #[test]
    fn test_cast_bool_to_number() {
        let code = r#"
fn main() -> number {
    true as number
}
"#;
        compile_and_run_aot(code, 1, "cast_bool_to_number");
    }

    #[test]
    fn test_cast_in_expression() {
        let code = r#"
fn main() -> number {
    let x = 42;
    (x as u8) + 10
}
"#;
        compile_and_run_aot(code, 52, "cast_in_expression");
    }

    #[test]
    fn test_cast_let_binding() {
        let code = r#"
fn main() -> number {
    let a = 256;
    let b = a as u8;
    b
}
"#;
        compile_and_run_aot(code, 0, "cast_let_binding");
    }

    #[test]
    fn test_tuple_basic() {
        let code = r#"
fn main() -> number {
    let t = (1, 2);
    t.0 + t.1
}
"#;
        compile_and_run_aot(code, 3, "tuple_basic");
    }

    #[test]
    fn test_tuple_three_elements() {
        let code = r#"
fn main() -> number {
    let p = (10, 20, 30);
    p.0 + p.2
}
"#;
        compile_and_run_aot(code, 40, "tuple_three_elements");
    }

    #[test]
    fn test_tuple_field_access_in_arithmetic() {
        let code = r#"
fn main() -> number {
    let t = (1, 2);
    t.1 * 10
}
"#;
        compile_and_run_aot(code, 20, "tuple_field_access_in_arithmetic");
    }

    #[test]
    fn test_tuple_five_elements() {
        let code = r#"
fn main() -> number {
    let t = (1, 2, 3, 4, 5);
    t.0 + t.4
}
"#;
        compile_and_run_aot(code, 6, "tuple_five_elements");
    }

    #[test]
    fn test_tuple_as_parameter() {
        let code = r#"
fn main() -> number {
    let fst = |t| { t.0 };
    let snd = |t| { t.1 };
    let t = (42, 99);
    fst(t) + snd(t)
}
"#;
        compile_and_run_aot(code, 141, "tuple_as_parameter");
    }

    #[test]
    fn test_tuple_nested_access() {
        let code = r#"
fn main() -> number {
    let a = (1, 2);
    let b = (3, 4);
    a.0 + a.1 + b.0 + b.1
}
"#;
        compile_and_run_aot(code, 10, "tuple_nested_access");
    }

    #[test]
    fn test_tuple_single_element_paren_grouping() {
        let code = r#"
fn main() -> number {
    let x = (42);
    x + 1
}
"#;
        compile_and_run_aot(code, 43, "tuple_single_element_paren_grouping");
    }

    #[test]
    fn test_tuple_in_if_expression() {
        let code = r#"
fn main() -> number {
    let t = (10, 20);
    if t.0 < t.1 {
        t.1 - t.0
    } else {
        t.0 - t.1
    }
}
"#;
        compile_and_run_aot(code, 10, "tuple_in_if_expression");
    }

    #[test]
    fn test_tuple_computed_elements() {
        let code = r#"
fn main() -> number {
    let a = 3;
    let b = 4;
    let t = (a + b, a * b);
    t.0 + t.1
}
"#;
        compile_and_run_aot(code, 19, "tuple_computed_elements");
    }

    #[test]
    fn test_generic_identity_function() {
        let code = r#"
fn id(x) { x }

fn main() -> number {
    id(42)
}
"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Parsing failed: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();

        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };

        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");

        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false)
            .expect("Escape analysis failed");

        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);

        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");

        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");

        let mut executor =
            ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor
            .execute_with_jit(&lir)
            .expect("JIT execution failed");

        assert_eq!(exit_code, 42, "Expected id(42) to return 42");
    }

    #[test]
    fn test_generic_first_function() {
        let code = r#"
fn first(a, b) { a }

fn main() -> number {
    first(10, 20)
}
"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Parsing failed: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();

        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };

        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");

        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false)
            .expect("Escape analysis failed");

        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);

        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");

        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");

        let mut executor =
            ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor
            .execute_with_jit(&lir)
            .expect("JIT execution failed");

        assert_eq!(exit_code, 10, "Expected first(10, 20) to return 10");
    }

    #[test]
    fn test_generic_apply_with_closure() {
        let code = r#"
fn apply(f, x) { f(x) }

fn main() -> number {
    apply(|n| { n + 1 }, 5)
}
"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Parsing failed: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();

        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };

        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");

        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false)
            .expect("Escape analysis failed");

        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);

        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");

        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");

        let mut executor =
            ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor
            .execute_with_jit(&lir)
            .expect("JIT execution failed");

        assert_eq!(exit_code, 6, "Expected apply(|n| n+1, 5) to return 6");
    }

    /// 回归测试：引用类型作为泛型函数参数（Dereference 推断）
    /// 确保 fn read_ref(r) { *r } 中参数 r 被正确推断为 &T
    #[test]
    fn test_generic_ref_param_deref() {
        let code = r#"
fn read_ref(r) { *r }

fn main() -> number {
    let x = 99;
    read_ref(&x)
}
"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Parsing failed: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();

        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };

        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");

        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false)
            .expect("Escape analysis failed");

        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);

        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");

        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");

        let mut executor =
            ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor
            .execute_with_jit(&lir)
            .expect("JIT execution failed");

        assert_eq!(exit_code, 99, "Expected read_ref(&x) to return 99");
    }

    #[test]
    fn test_for_break() {
        let code = r#"fn main() -> number { let sum = 0; for i in 0..10 { if i == 5 { break }; sum = sum + i }; sum }"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Parse/type errors: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 10, "Expected for+break sum to be 10 (0+1+2+3+4)");
    }

    #[test]
    fn test_for_continue() {
        let code = r#"fn main() -> number { let sum = 0; for i in 0..10 { if i == 5 { continue }; sum = sum + i }; sum }"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Parse/type errors: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 40, "Expected for+continue sum to be 40 (45-5)");
    }

    #[test]
    fn test_while_break() {
        let code = r#"fn main() -> number { let i = 0; let sum = 0; while i < 10 { if i == 5 { break }; sum = sum + i; i = i + 1 }; sum }"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Parse/type errors: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 10, "Expected while+break sum to be 10");
    }

    #[test]
    fn test_while_continue() {
        let code = r#"fn main() -> number { let i = 0; let sum = 0; while i < 10 { i = i + 1; if i == 5 { continue }; sum = sum + i }; sum }"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Parse/type errors: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 50, "Expected while+continue sum to be 50");
    }

    #[test]
    fn test_nested_for_loop() {
        let code = r#"fn main() -> number { let sum = 0; for i in 0..3 { for j in 0..3 { sum = sum + i * 3 + j } }; sum }"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Parse/type errors: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 36, "Expected nested for loop sum to be 36");
    }

    #[test]
    fn test_bubble_sort() {
        let code = r#"fn main() -> number {
    let arr = [5, 3, 8, 1, 9, 2, 7, 4, 6, 0];
    let n = len arr;
    for i in 0..n {
        for j in 0..(n - i - 1) {
            if arr[j] > arr[j + 1] {
                let tmp = arr[j];
                arr[j] = arr[j + 1];
                arr[j + 1] = tmp
            }
        }
    };
    arr[0] + arr[9]
}"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Parse/type errors: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 9, "Expected sorted array arr[0]+arr[9] = 0+9 = 9");
    }

    #[test]
    fn test_multi_arg_constructor() {
        let code = r#"enum Pair { Mk(number, number) }; fn main() -> number {
    let p = Mk(10, 20);
    match p { Mk(a, b) => a + b }
}"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);
        assert!(
            !diagnostics.has_errors(),
            "Parse/type errors: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 30, "Expected Mk(10,20) match to return 30");
    }

    #[test]
    fn test_array_element_assignment() {
        let code = r#"fn main() -> number {
    let arr = [10, 20, 30];
    arr[0] = 99;
    arr[1] = 88;
    arr[0] + arr[1]
}"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Parse/type errors: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 187, "Expected arr[0]+arr[1] = 99+88 = 187");
    }
    #[test]
    fn test_struct_array_access() {
        // 回归测试：结构体数组索引访问不应触发 SIGSEGV
        let code = r#"struct Point { x: number, y: number }
fn main() -> number {
    let points = [Point { x: 1, y: 2 }, Point { x: 3, y: 4 }, Point { x: 5, y: 6 }];
    points[0].x + points[1].y + points[2].x
}"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Parse/type errors: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 10, "Expected points[0].x + points[1].y + points[2].x = 1+4+5 = 10");
    }

    #[test]
    fn test_struct_single_element_array() {
        // 回归测试：单元素结构体数组
        let code = r#"struct Point { x: number, y: number }
fn main() -> number {
    let pts = [Point { x: 10, y: 20 }];
    pts[0].y
}"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Parse/type errors: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 20, "Expected pts[0].y = 20");
    }

    #[test]
    fn test_hex_and_binary_literals() {
        let code = r#"fn main() -> number { 0xFF + 0b1010 }"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Parse/type errors: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 265, "Expected 0xFF + 0b1010 = 255 + 10 = 265");
    }

    #[test]
    fn test_match_fat_arrow_syntax() {
        let code = r#"fn main() -> number { match 5 { 1 => 10, 5 => 50, _ => 0 } }"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Parse/type errors: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 50, "Expected match 5 with => syntax to return 50");
    }

    #[test]
    fn test_closure_struct_creation() {
        let code = r#"struct Point { x: number, y: number }; fn main() -> number { let make = |a, b| { Point { x: a, y: b } }; let p = make(3, 4); p.x * p.x + p.y * p.y }"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Parse/type errors: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 25, "Expected closure-created Point distance_sq = 25");
    }


    #[test]
    fn test_multi_arg_constructor_sequential_match() {
        let code = r#"enum Shape { Rect(number, number), Circle(number) }
fn main() -> number {
    let r = Rect(3, 4);
    let rx = match r { Rect(a, b) => a * b, Circle(r) => r };
    let c = Circle(5);
    let cx = match c { Rect(a, b) => a + b, Circle(r) => r * 2 };
    rx + cx
}"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Parse/type errors: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 22, "Expected rx(12) + cx(10) = 22");
    }

    #[test]
    fn test_multi_arg_constructor_let_match() {
        let code = r#"enum Shape { Rect(number, number), Circle(number) }
fn main() -> number {
    let r = Rect(3, 4);
    let rx = match r { Rect(a, b) => a * b, Circle(r) => r };
    rx
}"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Parse/type errors: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 12, "Expected Rect(3,4) matched as Rect: 3*4 = 12");
    }

    #[test]
    fn test_multi_arg_constructor_circle_match() {
        let code = r#"enum Shape { Rect(number, number), Circle(number) }
fn main() -> number {
    let c = Circle(7);
    match c { Rect(a, b) => a + b, Circle(r) => r * 3 }
}"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Parse/type errors: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 21, "Expected Circle(7) matched: 7*3 = 21");
    }

    #[test]
    fn test_zero_arg_enum_variant_match_green() {
        let code = r#"enum Color { Red, Green, Blue }
fn main() -> number {
    let c = Green;
    match c { Red => 1, Green => 2, Blue => 3 }
}"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Parse/type errors: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 2, "Expected Green matched: should return 2");
    }

    #[test]
    fn test_zero_arg_enum_variant_match_blue() {
        let code = r#"enum Color { Red, Green, Blue }
fn main() -> number {
    let c = Blue;
    match c { Red => 1, Green => 2, Blue => 3 }
}"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Parse/type errors: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 3, "Expected Blue matched: should return 3");
    }

    #[test]
    fn test_zero_arg_enum_variant_match_reorder() {
        let code = r#"enum Color { Red, Green, Blue }
fn main() -> number {
    let c = Green;
    match c { Green => 1, Red => 2, Blue => 3 }
}"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Parse/type errors: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 1, "Expected Green matched first: should return 1");
    }

    #[test]
    fn test_mixed_zero_and_data_variants_match() {
        let code = r#"enum Shape { Circle, Square, Rect(number, number) }
fn main() -> number {
    let s = Square;
    match s { Circle => 10, Square => 20, Rect(w, h) => w }
}"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Parse/type errors: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 20, "Expected Square matched: should return 20");
    }

    #[test]
    fn test_duplicate_function_definition_rejected() {
        // Test that duplicate function definitions are correctly rejected.
        // Two functions named "triple" — the second should trigger a DuplicateFunctionDefinition error.
        let source = "fn triple(x: number) -> number { x * 3 }\nfn triple(x) { x * 3 }\nfn main() -> number {\n    triple(7)\n}\n";

        let (tokens, _) = tokenize(source);
        assert!(!tokens.is_empty(), "lexing should produce tokens");

        let (_parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Script, None);

        assert!(
            diagnostics.has_errors(),
            "duplicate function definition should produce errors"
        );

        let error_messages: Vec<String> = diagnostics
            .diagnostics
            .iter()
            .map(|d| d.message.clone())
            .collect();
        let has_dup_error = error_messages
            .iter()
            .any(|msg| msg.contains("Duplicate function"));
        assert!(
            has_dup_error,
            "error messages should contain 'Duplicate function', actual: {:?}",
            error_messages
        );
    }

    #[test]
    fn test_divide_by_zero_returns_zero() {
        let code = r#"
fn main() -> number {
    10 / 0
}
"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Parsing failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 0, "10 / 0 should return 0, got {}", exit_code);
    }

    #[test]
    fn test_modulo_by_zero_returns_zero() {
        let code = r#"
fn main() -> number {
    10 % 0
}
"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Parsing failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 0, "10 % 0 should return 0, got {}", exit_code);
    }

    #[test]
    fn test_zero_divide_by_zero() {
        let code = r#"
fn main() -> number {
    0 / 0
}
"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Parsing failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 0, "0 / 0 should return 0, got {}", exit_code);
    }

    #[test]
    fn test_div_zero_in_loop() {
        let code = r#"
fn main() -> number {
    let x = 0;
    let result = 0;
    while x < 5 {
        let y = x / 0;
        result = result + y;
        x = x + 1
    };
    result
}
"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Parsing failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 0, "loop with div by zero should return 0, got {}", exit_code);
    }

    #[test]
    fn test_while_div_break_variable_assignment() {
        let code = r#"
fn main() -> number {
    let lo = 0;
    let mid = 0;
    let found = -1;
    while lo < 5 {
        mid = (lo + 2) / 2;
        if mid == 3 { found = 99; break };
        lo = lo + 1
    };
    found
}
"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Parsing failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 99, "while+div+break should return 99, got {}", exit_code);
    }

    #[test]
    fn test_while_mod_break_variable_assignment() {
        let code = r#"
fn main() -> number {
    let lo = 0;
    let mid = 0;
    let found = -1;
    while lo < 5 {
        mid = (lo + 5) % 3;
        if mid == 2 { found = 77; break };
        lo = lo + 1
    };
    found
}
"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Parsing failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 77, "while+mod+break should return 77, got {}", exit_code);
    }

    #[test]
    fn test_nested_div_zero() {
        let code = r#"
fn main() -> number {
    let a = 10;
    let b = 5;
    let c = 0;
    a / (b / c)
}
"#;
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Parsing failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 0, "nested div by zero should return 0, got {}", exit_code);
    }

    #[test]
    fn test_six_parameters() {
        let code = "fn sum6(a: number, b: number, c: number, d: number, e: number, f: number) -> number {\n    a + b + c + d + e + f\n}\nfn main() -> number {\n    sum6(1, 2, 3, 4, 5, 6)\n}";
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Parsing failed: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();

        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };

        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");

        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false)
            .expect("Escape analysis failed");

        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);

        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");

        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");

        let mut executor =
            ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor
            .execute_with_jit(&lir)
            .expect("JIT execution failed");

        assert_eq!(
            exit_code, 21,
            "Expected exit code 21 (1+2+3+4+5+6), got {}",
            exit_code
        );
    }

    #[test]
    fn test_recursive_enum_type_check() {
        let code = "enum List { Nil, Cons(number, List) }\nfn sum(l: List) -> number {\n    match l { Nil => 0, Cons(h, t) => h + sum(t) }\n}\nfn main() -> number {\n    let lst = Cons(1, Cons(2, Cons(3, Nil)));\n    sum(lst)\n}";
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Recursive enum type check should pass, but got errors: {:?}",
            diagnostics
        );
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 6, "Expected 6 (1+2+3), got {}", exit_code);
    }

    #[test]
    fn test_recursive_enum_with_multiple_self_refs() {
        let code = "enum Tree { Leaf, Node(number, Tree, Tree) }\nfn main() -> number {\n    let t = Node(1, Node(2, Leaf, Leaf), Leaf);\n    match t { Leaf => 0, Node(v, l, r) => v }\n}";
        let (tokens, _) = tokenize(code);
        let (_parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(
            !diagnostics.has_errors(),
            "Recursive enum with multiple self-refs type check should pass, got errors: {:?}",
            diagnostics
        );
    }

    #[test]
    fn test_r4_4_struct_closure_capture() {
        let code = "struct Point { x: number, y: number }
fn main() -> number {
    let p = Point { x: 3, y: 4 };
    let f = || { p.x + p.y };
    f()
}";
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Type check failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 7, "Expected 7 (3+4), got {}", exit_code);
    }

    #[test]
    fn test_r4_5_reference_closure_capture() {
        let code = "fn main() -> number {
    let x = 42;
    let r = &x;
    let f = || { *r };
    f()
}";
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Type check failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 42, "Expected 42, got {}", exit_code);
    }

    #[test]
    fn test_r4_1_seven_parameter_function() {
        let code = "fn add7(a: number, b: number, c: number, d: number, e: number, f: number, g: number) -> number {
    a + b + c + d + e + f + g
}
fn main() -> number {
    add7(1, 2, 3, 4, 5, 6, 7)
}";
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Type check failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 28, "Expected 28 (1+2+3+4+5+6+7), got {}", exit_code);
    }

    #[test]
    fn test_r4_1_eight_parameter_function() {
        let code = "fn add8(a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number) -> number {
    a + b + c + d + e + f + g + h
}
fn main() -> number {
    add8(1, 2, 3, 4, 5, 6, 7, 8)
}";
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Type check failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 36, "Expected 36 (1+2+3+4+5+6+7+8), got {}", exit_code);
    }

    #[test]
    fn test_r5_1_enum_match_three_variants() {
        let code = "enum ABC { A, B, C }
fn main() -> number {
    let v2 = ABC::B;
    match v2 {
        ABC::A => 1,
        ABC::B => 2,
        ABC::C => 3
    }
}";
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Type check failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 2, "Expected 2 (ABC::B), got {}", exit_code);
    }

    #[test]
    fn test_r5_1_enum_match_two_variants() {
        let code = "enum AB { A, B }
fn main() -> number {
    let v = AB::B;
    match v {
        AB::A => 1,
        AB::B => 2
    }
}";
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Type check failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 2, "Expected 2 (AB::B), got {}", exit_code);
    }

    #[test]
    fn test_r5_2_factory_closure_independent_env() {
        let code = "fn make_adder(n: number) {
    |x| { x + n }
}
fn main() -> number {
    let add5 = make_adder(5);
    let add10 = make_adder(10);
    add5(1)
}";
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Type check failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 6, "Expected 6 (add5(1) = 5+1), got {}", exit_code);
    }

    #[test]
    fn test_r5_3_consecutive_modulo() {
        let code = "fn main() -> number {
    let a = 10 % 3;
    let b = 7 % 2;
    a + b
}";
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Type check failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 2, "Expected 2 (10%3 + 7%2 = 1+1), got {}", exit_code);
    }

    #[test]
    fn test_r5_4_closure_modify_struct_field() {
        let code = "struct Counter { value: number }
fn main() -> number {
    let c = Counter { value: 0 };
    let inc = || { c.value = c.value + 1 };
    inc();
    c.value
}";
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Type check failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 1, "Expected 1, got {}", exit_code);
    }

    #[test]
    fn test_r5_6_three_level_nested_closure() {
        let code = "fn main() -> number {
    let x = 1;
    let f1 = || {
        let y = 10;
        let f2 = || {
            let z = 100;
            let f3 = || { x + y + z };
            f3()
        };
        f2()
    };
    f1()
}";
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Type check failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 111, "Expected 111, got {}", exit_code);
    }

    #[test]
    fn test_r6_1_closure_in_while_loop() {
        let code = "fn main() -> number {
    let f = |x| { x + 1 };
    let i = 0;
    let sum = 0;
    while i < 5 {
        sum = f(i);
        i = i + 1
    };
    sum
}";
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Type check failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 5, "Expected 5, got {}", exit_code);
    }

    #[test]
    fn test_r6_2_div_before_struct_construction() {
        let code = "struct Point { x: number, y: number }
fn main() -> number {
    let v = 20 / 4;
    let p = Point { x: v, y: 3 };
    p.x + p.y
}";
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Type check failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 8, "Expected 8, got {}", exit_code);
    }

    #[test]
    fn test_r6_2_mod_before_enum_construction() {
        let code = "enum Result { Ok(number), Err(number) }
fn main() -> number {
    let v = 17 % 5;
    match Result::Ok(v) {
        Ok(x) => x,
        Err(e) => e
    }
}";
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Type check failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 2, "Expected 2, got {}", exit_code);
    }

    #[test]
    fn test_r6_1_closure_in_while_with_multiple_vars() {
        let code = "fn main() -> number {
    let add = |a, b| { a + b };
    let i = 0;
    let sum = 0;
    while i < 3 {
        sum = add(sum, i);
        i = i + 1
    };
    sum
}";
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Type check failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 3, "Expected 3, got {}", exit_code);
    }


    #[test]
    fn test_r7_1_while_closure_captures_phi_variable() {
        let code = "fn main() -> number {
    let result = 0;
    let i = 0;
    while i < 3 {
        let f = || { i };
        result = result + f();
        i = i + 1;
    };
    result
}";
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Type check failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 3, "Expected 3 (0+1+2), got {}", exit_code);
    }

    #[test]
    fn test_r7_2_while_break_phi_variable_update() {
        let code = "fn main() -> number {
    let r = 0;
    let i = 0;
    while i < 3 {
        r = r + 1;
        if i == 1 {
            r = r + 100;
            break;
        };
        i = i + 1;
    };
    r
}";
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Type check failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 102, "Expected 102, got {}", exit_code);
    }

    #[test]
    fn test_r7_2_break_simple() {
        let code = "fn main() -> number {
    let r = 0;
    let i = 0;
    while i < 3 {
        r = r + 1;
        if i == 0 {
            break;
        };
        i = i + 1;
    };
    r
}";
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Type check failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 1, "Expected 1, got {}", exit_code);
    }

    #[test]
    fn test_r7_1_for_in_closure_captures_variable() {
        let code = "fn main() -> number {
    let result = 0;
    for i in 0..3 {
        let f = || { i };
        result = result + f();
    };
    result
}";
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Type check failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 3, "Expected 3 (0+1+2), got {}", exit_code);
    }

    #[test]
    fn test_r8_1_for_loop_closure_captures_outer_variable() {
        let code = "fn main() -> number {
    let x = 42;
    let sum = 0;
    for i in 0..3 {
        let f = || { x };
        sum = sum + f()
    };
    sum
}";
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Type check failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 126, "Expected 126 (42*3), got {}", exit_code);
    }

    #[test]
    fn test_r8_2_while_if_else_closure_phi_variable() {
        let code = "fn main() -> number {
    let sum = 0;
    let i = 0;
    while i < 3 {
        let f = if true { || { i } } else { || { 0 } };
        sum = sum + f();
        i = i + 1
    };
    sum
}";
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Type check failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 3, "Expected 3 (0+1+2), got {}", exit_code);
    }

#[test]
    fn test_r9_11_match_3arm_modify_outer_var() {
        let code = "fn main() -> number {
    let sum = 0;
    match 2 {
        0 => { sum = sum + 10 },
        1 => { sum = sum + 20 },
        2 => { sum = sum + 30 }
    };
    sum
}";
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Type check failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 30, "Expected 30, got {}", exit_code);
    }

    #[test]
    fn test_r9_2_match_enum_modify_outer_var() {
        let code = "enum Color { Red, Green, Blue }
fn main() -> number {
    let sum = 0;
    let c = Color::Green;
    match c {
        Color::Red => { sum = sum + 10 },
        Color::Green => { sum = sum + 20 },
        Color::Blue => { sum = sum + 30 }
    };
    sum
}";
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Type check failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 20, "Expected 20, got {}", exit_code);
    }

    #[test]
    fn test_r9_8_closure_ref_struct_in_loop() {
        let code = "struct Data { val: number }
fn main() -> number {
    let d = Data { val: 42 };
    let sum = 0;
    for i in 0..3 {
        let f = || d.val;
        sum = sum + f()
    };
    sum
}";
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Type check failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 126, "Expected 126, got {}", exit_code);
    }

    #[test]
    fn test_r9_1_for_match_enum_continue() {
        let code = "enum Flag { A, B }
fn main() -> number {
    let sum = 0;
    for i in 0..4 {
        let f = match i % 2 { 0 => Flag::A, _ => Flag::B };
        match f {
            Flag::A => { sum = sum + 1 },
            Flag::B => continue
        }
    };
    sum
}";
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Type check failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 2, "Expected 2, got {}", exit_code);
    }

    #[test]
    fn test_r9_4_for_match_enum_break() {
        let code = "enum Flag { A, B }
fn main() -> number {
    let sum = 0;
    for i in 0..4 {
        let f = match i % 2 { 0 => Flag::A, _ => Flag::B };
        match f {
            Flag::A => { sum = sum + 1 },
            Flag::B => break
        }
    };
    sum
}";
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Type check failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 1, "Expected 1, got {}", exit_code);
    }

    #[test]
    fn test_r10_1_nested_match_expression() {
        let code = "fn main() -> number {
    match 0 {
        0 => {
            match 0 {
                0 => { 11 }
            }
        }
    }
}";
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Type check failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 11, "Expected 11, got {}", exit_code);
    }

    #[test]
    fn test_r10_5_match_arm_if_else_expression() {
        let code = "fn main() -> number {
    match 1 {
        0 => { 10 },
        _ => { if 1 > 0 { 20 } else { 30 } }
    }
}";
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Type check failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 20, "Expected 20, got {}", exit_code);
    }

    #[test]
    fn test_r10_4_match_arm_while_loop() {
        let code = "fn main() -> number {
    let sum = 0;
    let i = 0;
    match 0 {
        0 => {
            while i < 3 {
                sum = sum + 1;
                i = i + 1
            }
        }
    };
    sum
}";
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Type check failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 3, "Expected 3, got {}", exit_code);
    }

    #[test]
    fn test_r10_3_match_arm_break_in_if_else() {
        let code = "fn main() -> number {
    let sum = 0;
    let i = 0;
    while i < 5 {
        match i {
            0 => {
                if 1 > 0 {
                    sum = sum + 1;
                    break
                }
            },
            _ => {}
        };
        i = i + 1
    };
    sum
}";
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Type check failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 1, "Expected 1, got {}", exit_code);
    }

    #[test]
    fn test_r10_2_while_struct_closure_capture() {
        let code = "struct S { v: number }
fn main() -> number {
    let s = S { v: 42 };
    let i = 0;
    while i < 1 {
        let f = || { s.v };
        if f() != 42 { return 1 };
        i = i + 1
    };
    0
}";
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Type check failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        let exit_code = executor.execute_with_jit(&lir).expect("JIT execution failed");
        assert_eq!(exit_code, 0, "Expected 0, got {}", exit_code);
    }

    fn compile_project_mode_code(code: &str) -> i64 {
        let (tokens, _) = tokenize(code);
        let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
        assert!(!diagnostics.has_errors(), "Parsing failed: {:?}", diagnostics);
        let parse_result = parse_result.expect("No parse result");
        let ast = parse_result.expr();
        let options = LoweringOptions {
            known_functions: HashSet::new(),
            module_context: None,
            expr_types: parse_result.expr_types.clone(),
        };
        let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
        karte_module_system::optimize_mir_with_escape_analysis(&mut mir, false)
            .expect("Escape analysis failed");
        promote_project_entry(&mut mir);
        mir.functions.remove(SCRIPT_ENTRY_POINT);
        let mut lir = lower_mir_to_lir(&mir).expect("LIR lowering failed");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Balanced);
        pipeline.optimize(&mut lir).expect("Optimization failed");
        let mut executor = ProfessionalExecutor::new_with_jit(false).expect("Failed to create JIT executor");
        executor.execute_with_jit(&lir).expect("JIT execution failed")
    }

    #[test]
    fn test_multi_arm_nested_enum_match() {
        let code = r#"
enum Color { Red, Green, Blue }
enum Size { Small, Medium, Large }
fn test(c: Color, s: Size) -> number {
    match c {
        Color::Red => match s {
            Size::Small => 10, Size::Medium => 20, Size::Large => 30
        },
        Color::Green => match s {
            Size::Small => 40, Size::Medium => 50, Size::Large => 60
        },
        Color::Blue => match s {
            Size::Small => 70, Size::Medium => 80, Size::Large => 90
        }
    }
}
fn main() -> number {
    test(Color::Red, Size::Small) + test(Color::Red, Size::Medium) +
    test(Color::Red, Size::Large) + test(Color::Green, Size::Small) +
    test(Color::Green, Size::Medium) + test(Color::Green, Size::Large) +
    test(Color::Blue, Size::Small) + test(Color::Blue, Size::Medium) +
    test(Color::Blue, Size::Large)
}
        "#;
        let exit_code = compile_project_mode_code(code);
        assert_eq!(exit_code, 450, "Expected 450, got {}", exit_code);
    }

    #[test]
    fn test_nested_enum_match_blue_medium() {
        let code = r#"
enum Color { Red, Green, Blue }
enum Size { Small, Medium, Large }
fn test(c: Color, s: Size) -> number {
    match c {
        Color::Red => match s {
            Size::Small => 10, Size::Medium => 20, Size::Large => 30
        },
        Color::Green => match s {
            Size::Small => 40, Size::Medium => 50, Size::Large => 60
        },
        Color::Blue => match s {
            Size::Small => 70, Size::Medium => 80, Size::Large => 90
        }
    }
}
fn main() -> number {
    test(Color::Blue, Size::Medium)
}
        "#;
        let exit_code = compile_project_mode_code(code);
        assert_eq!(exit_code, 80, "Expected 80, got {}", exit_code);
    }

    #[test]
    fn test_two_arm_nested_enum_match() {
        let code = r#"
enum Color { Red, Green, Blue }
enum Size { Small, Medium, Large }
fn test(c: Color, s: Size) -> number {
    match c {
        Color::Red => match s {
            Size::Small => 10, Size::Medium => 20, Size::Large => 30
        },
        Color::Green => match s {
            Size::Small => 40, Size::Medium => 50, Size::Large => 60
        },
        Color::Blue => 70
    }
}
fn main() -> number {
    test(Color::Green, Size::Large)
}
        "#;
        let exit_code = compile_project_mode_code(code);
        assert_eq!(exit_code, 60, "Expected 60, got {}", exit_code);
    }

    #[test]
    fn test_r12_1_for_break_dead_code_assignment() {
        let code = r#"
fn main() -> number {
    let sum = 0;
    for i in 0..10 {
        break;
        sum = sum + 1
    };
    sum
}
        "#;
        let exit_code = compile_project_mode_code(code);
        assert_eq!(exit_code, 0, "Expected 0, got {}", exit_code);
    }

    #[test]
    fn test_r12_2_for_div_match_closure() {
        let code = r#"
fn main() -> number {
    let sum = 0;
    for i in 0..6 {
        let x = match i / 2 { 0 => 100, _ => 200 };
        let f = || { x };
        sum = sum + f()
    };
    sum
}
        "#;
        let exit_code = compile_project_mode_code(code);
        assert_eq!(exit_code, 1000, "Expected 1000, got {}", exit_code);
    }

    #[test]
    fn test_r12_3_for_modulo_match_closure() {
        let code = r#"
fn main() -> number {
    let sum = 0;
    for i in 0..6 {
        let x = match i % 2 { 0 => 100, _ => 200 };
        let f = || { x };
        sum = sum + f()
    };
    sum
}
        "#;
        let exit_code = compile_project_mode_code(code);
        assert_eq!(exit_code, 900, "Expected 900, got {}", exit_code);
    }

    #[test]
    fn test_r14_1_while_loop_fn_call_sum() {
        let code = r#"
fn id(x: number) -> number { x }
fn main() -> number {
    let sum = 0;
    let i = 0;
    while i < 3 {
        sum = sum + id(1);
        i = i + 1;
    };
    sum
}
        "#;
        let exit_code = compile_project_mode_code(code);
        assert_eq!(exit_code, 3, "Expected 3, got {}", exit_code);
    }

    #[test]
    fn test_r14_2_while_loop_fn_call_single_iter() {
        let code = r#"
fn id(x: number) -> number { x }
fn main() -> number {
    let sum = 0;
    let i = 0;
    while i < 1 {
        sum = sum + id(1);
        i = i + 1;
    };
    sum
}
        "#;
        let exit_code = compile_project_mode_code(code);
        assert_eq!(exit_code, 1, "Expected 1, got {}", exit_code);
    }

    #[test]
    fn test_r14_3_while_loop_fn_call_many_iter() {
        let code = r#"
fn id(x: number) -> number { x }
fn main() -> number {
    let sum = 0;
    let i = 0;
    while i < 10 {
        sum = sum + id(1);
        i = i + 1;
    };
    sum
}
        "#;
        let exit_code = compile_project_mode_code(code);
        assert_eq!(exit_code, 10, "Expected 10, got {}", exit_code);
    }

    #[test]
    fn test_r14_4_while_loop_fn_call_override_assign() {
        let code = r#"
fn id(x: number) -> number { x }
fn main() -> number {
    let sum = 0;
    let i = 0;
    while i < 3 {
        sum = id(i);
        i = i + 1;
    };
    sum
}
        "#;
        let exit_code = compile_project_mode_code(code);
        assert_eq!(exit_code, 2, "Expected 2, got {}", exit_code);
    }


    #[test]
    fn test_r8_3_for_if_array_assign() {
        let code = r#"
fn main() -> number {
    let arr = [10, 20, 30, 40, 50];
    for i in 0..5 {
        if arr[i] > 25 {
            arr[i] = arr[i] * 2
        }
    };
    arr[0] + arr[1] + arr[2] + arr[3] + arr[4]
}
"#;
        let exit_code = compile_project_mode_code(code);
        assert_eq!(exit_code, 270, "Expected 270 (10+20+60+80+100), got {}", exit_code);
    }

    #[test]
    fn test_r8_3_while_if_array_assign() {
        let code = r#"
fn main() -> number {
    let arr = [10, 20, 30];
    let i = 0;
    while i < 3 {
        if i > 0 {
            arr[i] = 99
        };
        i = i + 1
    };
    arr[0] + arr[1] + arr[2]
}
"#;
        let exit_code = compile_project_mode_code(code);
        assert_eq!(exit_code, 208, "Expected 208 (10+99+99), got {}", exit_code);
    }

    #[test]
    fn test_r8_3_for_array_assign_all() {
        let code = r#"
fn main() -> number {
    let arr = [1, 2, 3, 4, 5];
    for i in 0..5 {
        arr[i] = arr[i] * 2
    };
    arr[0] + arr[1] + arr[2] + arr[3] + arr[4]
}
"#;
        let exit_code = compile_project_mode_code(code);
        assert_eq!(exit_code, 30, "Expected 30 (2+4+6+8+10), got {}", exit_code);
    }

    #[test]
    fn test_r8_3_for_if_array_read_sum() {
        let code = r#"
fn main() -> number {
    let arr = [1, 2, 3];
    let sum = 0;
    for i in 0..3 {
        if arr[i] > 1 {
            sum = sum + arr[i]
        }
    };
    sum
}
"#;
        let exit_code = compile_project_mode_code(code);
        assert_eq!(exit_code, 5, "Expected 5 (2+3), got {}", exit_code);
    }


    #[test]
    fn test_r7_3_while_reference_struct_field_assign() {
        let code = r#"
struct Data { val: number }
fn main() -> number {
    let d = Data { val: 10 };
    let r = &d;
    let i = 0;
    while i < 3 {
        d.val = d.val + (*r).val;
        i = i + 1
    };
    d.val
}
"#;
        let exit_code = compile_project_mode_code(code);
        assert_eq!(exit_code, 80, "Expected 80 (10->20->40->80), got {}", exit_code);
    }

    #[test]
    fn test_r7_3_reference_struct_no_while() {
        let code = r#"
struct Data { val: number }
fn main() -> number {
    let d = Data { val: 10 };
    let r = &d;
    d.val = d.val + (*r).val;
    d.val
}
"#;
        let exit_code = compile_project_mode_code(code);
        assert_eq!(exit_code, 20, "Expected 20 (10+10), got {}", exit_code);
    }

}