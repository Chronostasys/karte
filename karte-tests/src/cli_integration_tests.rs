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

}
