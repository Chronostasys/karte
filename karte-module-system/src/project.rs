use crate::cache::CompilationCache;
use crate::{
    compute_module_cache_version, validate_module_imports, write_module_interface_artifact,
    ModuleGraph, ModuleId, ModuleInterfaceAccumulator, ModuleInterfaceArtifact,
    ModuleInterfaceSummary, ModuleMetadata, ModulePlan,
};
use indicatif::ProgressBar;
use karte_diagnostics::{DiagnosticBag, Span};
use karte_escape_analysis::{EscapeAnalyzer, EscapePointDetector, EscapePointTransformer};
use karte_hir::type_checker::{
    ExternalFunctionSignature, ExternalModuleInterface, ExternalStructField,
    ExternalStructSignature,
};
use karte_hir::ModuleContext;
use karte_ir_codec::IrDisplay;
use karte_lexer::tokenize;
use karte_lir::lower::lower_mir_to_lir;
use karte_lir::optimization_pipeline::{OptimizationLevel, OptimizationPipeline};
use karte_lir::LirProgram;
use karte_mir::{
    lower::{lower_expr_to_mir_with_options, LoweringOptions, SCRIPT_ENTRY_POINT},
    MirProgram, Statement, Value,
};
use karte_parser::{parse_with_type_check, ImportDecl, ParsedProgram, ParserMode};
use log::{error, info, warn};
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct CompilationArtifacts {
    parsed_program: ParsedProgram,
    pub mir_program: MirProgram,
    pub lir_program: LirProgram,
}

impl CompilationArtifacts {
    pub fn module_name(&self) -> Option<&str> {
        self.parsed_program
            .module
            .as_ref()
            .map(|decl| decl.name.as_str())
    }

    pub fn imports(&self) -> &[ImportDecl] {
        &self.parsed_program.imports
    }
}

pub struct CacheContext<'a> {
    pub module_id: &'a str,
    pub interface_hash: u64,
}

pub struct ProjectBuildContext {
    pub graph: ModuleGraph,
    pub plan: ModulePlan,
    pub layers: Vec<Vec<ModuleId>>,
    pub canonical_entry: std::path::PathBuf,
}

impl ProjectBuildContext {
    pub fn new(entry_path: &Path) -> Result<Self, String> {
        Self::try_new(entry_path)?
            .ok_or_else(|| "入口文件不在 Karte 项目中，缺少 karte.mod.toml".to_string())
    }

    pub fn try_new(entry_path: &Path) -> Result<Option<Self>, String> {
        let graph = ModuleGraph::load_for_entry(entry_path).map_err(|e| e.to_string())?;

        if graph.manifest_path().is_none() {
            return Ok(None);
        }

        let plan = graph
            .plan_for_entry(entry_path)
            .map_err(|e| e.to_string())?;
        let layers = graph.schedule_layers(&plan);
        let canonical_entry = fs::canonicalize(entry_path)
            .map_err(|e| format!("canonicalize entry failed: {}", e))?;

        Ok(Some(Self {
            graph,
            plan,
            layers,
            canonical_entry,
        }))
    }

    pub fn total_modules(&self) -> usize {
        self.plan.sequence.len()
    }

    pub fn layer_count(&self) -> usize {
        self.layers.len()
    }
}

pub struct ProjectCompilationOutput {
    pub plan: ModulePlan,
    pub final_program: LirProgram,
    pub entry_artifacts: Option<CompilationArtifacts>,
    pub compiled_artifacts_by_module: HashMap<ModuleId, Vec<CompilationArtifacts>>,
}

pub fn compile_project_with_context(
    context: &ProjectBuildContext,
    optimization_level: OptimizationLevel,
    verbose: bool,
    progress_bar: Option<Arc<ProgressBar>>,
) -> Result<ProjectCompilationOutput, String> {
    let compiled_modules = Arc::new(Mutex::new(HashMap::new()));
    let mut interface_hashes: HashMap<ModuleId, u64> = HashMap::new();
    let mut interface_artifacts: HashMap<ModuleId, ModuleInterfaceArtifact> = HashMap::new();
    let mut compiled_artifacts_by_module: HashMap<ModuleId, Vec<CompilationArtifacts>> =
        HashMap::new();
    let mut entry_result: Option<CompilationArtifacts> = None;
    let entry_module = context.plan.entry.clone();

    for (i, layer) in context.layers.iter().enumerate() {
        if let Some(pb) = &progress_bar {
            pb.set_prefix(format!("Layer {}/{}", i + 1, context.layer_count()));
            pb.set_message(format!("准备 {} 个模块", layer.len()));
            pb.println(format!(
                "正在编译第 {}/{} 层 ({} 个模块)...",
                i + 1,
                context.layer_count(),
                layer.len()
            ));
        } else if verbose {
            println!(
                "正在编译第 {}/{} 层 ({} 个模块)...",
                i + 1,
                context.layer_count(),
                layer.len()
            );
        }

        let dependency_snapshot = interface_hashes.clone();
        let artifact_snapshot = interface_artifacts.clone();

        let results: Vec<Result<LayerCompilationResult, String>> = layer
            .par_iter()
            .map(|module_id| {
                compile_module_in_layer(
                    &context.graph,
                    module_id,
                    &dependency_snapshot,
                    &artifact_snapshot,
                    optimization_level,
                    verbose,
                    progress_bar.clone(),
                    ParserMode::Project,
                    &entry_module,
                    &context.canonical_entry,
                )
            })
            .collect();

        for result in results {
            match result {
                Ok(layer_result) => {
                    let module_id = layer_result.module_id.clone();
                    if module_id == entry_module {
                        if let Some(art) = layer_result.entry_artifacts.clone() {
                            entry_result = Some(art);
                        }
                    }
                    for unit in &layer_result.compiled_artifacts {
                        compiled_modules
                            .lock()
                            .expect("锁不应该被污染")
                            .insert(module_id.clone(), unit.lir_program.clone());
                    }
                    compiled_artifacts_by_module
                        .insert(module_id.clone(), layer_result.compiled_artifacts.clone());
                    interface_hashes.insert(module_id.clone(), layer_result.interface_hash);
                    interface_artifacts.insert(module_id.clone(), layer_result.interface_artifact);

                    if let Some(pb) = &progress_bar {
                        pb.set_message(format!("完成 {}", module_id));
                        pb.inc(1);
                    }
                }
                Err(e) => return Err(e),
            }
        }
    }

    if let Some(pb) = &progress_bar {
        pb.finish_with_message("✅ 编译完成");
    }

    let mut final_program = LirProgram::new();
    let modules = compiled_modules.lock().expect("锁不应该被污染");

    for module_id in &context.plan.sequence {
        if let Some(prog) = modules.get(module_id) {
            merge_lir_program(&mut final_program, prog);
            if module_id == &context.plan.entry {
                final_program.main_function = prog.main_function.clone();
            }
        }
    }

    if final_program.main_function.is_none() {
        if let Some(entry_artifacts) = &entry_result {
            final_program.main_function = entry_artifacts.lir_program.main_function.clone();
        }
    }

    Ok(ProjectCompilationOutput {
        plan: context.plan.clone(),
        final_program,
        entry_artifacts: entry_result,
        compiled_artifacts_by_module,
    })
}

fn print_diagnostics(diagnostics: &DiagnosticBag, source_code: &str, filename: &str) {
    diagnostics
        .print_fancy(source_code, filename)
        .expect("failed to print diagnostics");
}

pub fn compile_source_to_artifacts(
    input: &str,
    filename: &str,
    optimization_level: OptimizationLevel,
    verbose: bool,
    cache_key: Option<CacheContext>,
    mode: ParserMode,
    dependency_interfaces: Option<&HashMap<String, ExternalModuleInterface>>,
) -> Result<CompilationArtifacts, Box<dyn std::error::Error>> {
    if verbose {
        if filename != "input" {
            println!("Processing file: {}", filename);
        } else {
            println!("Input: {}", input);
        }
    }

    let (tokens, lex_diagnostics) = tokenize(input);

    if !lex_diagnostics.is_empty() {
        print_diagnostics(&lex_diagnostics, input, filename);
        if lex_diagnostics.has_errors() {
            return Err("Lexical analysis failed".into());
        }
    }

    if verbose && filename == "input" {
        println!(
            "Tokens: {:?}",
            tokens.iter().map(|t| &t.token).collect::<Vec<_>>()
        );
    }

    let (result, parse_diagnostics) = parse_with_type_check(&tokens, mode, dependency_interfaces);

    if !parse_diagnostics.is_empty() {
        print_diagnostics(&parse_diagnostics, input, filename);
        if parse_diagnostics.has_errors() {
            return Err("Parsing or type checking failed".into());
        }
    }

    let mut result = result.ok_or("Failed to parse expression or type check failed")?;

    // 先使用 result 的引用构建 LoweringOptions 和完成 MIR lowering，
    // 再 move result.program。虽然 ParsedProgram.body 已经是 Box<Expr>（堆分配），
    // 但仍需在 move 前完成 MIR lowering 以确保 expr_types 中的指针键有效。
    let result_type = result.result_type.clone();
    let module_context = result.module_context().clone();
    let lowering_options = LoweringOptions {
        known_functions: {
            let mut known = std::collections::HashSet::new();
            for binding in &module_context.imports {
                if binding.symbol != "*" {
                    known.insert(binding.alias.clone());
                }
            }
            let dependency_interfaces = module_context.dependency_interfaces();
            if dependency_interfaces.contains_key("std.prelude") {
                let prelude_modules = ["std.core", "std.math", "std.io", "std.string"];
                for mod_key in &prelude_modules {
                    if let Some(iface) = dependency_interfaces.get(*mod_key) {
                        for func_name in iface.functions.keys() {
                            known.insert(func_name.clone());
                        }
                    }
                }
            }
            known
        },
        module_context: Some(module_context.clone()),
        expr_types: result.expr_types.clone(),
    };

    if verbose && filename == "input" {
        println!("AST: {}", result.expr());
    }
    if verbose {
        println!("Type: {}", result_type);
    }

    // 🔧 关键修复：在 MIR lowering 前 clone module_context
    // 以便在 lowering 后注册 prelude 函数的 external_function_symbols
    let prelude_context = lowering_options.module_context.clone();

    if verbose {
        println!("\n--- Lowering to MIR ---");
    }
    let mut mir_program =
        match lower_expr_to_mir_with_options(result.expr(), lowering_options) {
            Ok(prog) => prog,
            Err(errors) => {
                for err in errors {
                    error!("MIR Lowering Error: {}", err);
                }
                return Err("MIR lowering failed".into());
            }
        };

    // 🔧 关键修复：将 prelude 函数注册到 external_function_symbols
    // 这样 LIR lowering 的标签预分配阶段会为短名称（如 "gcd"）生成与规范名称
    // （如 "std.core::gcd"）相同的标签，从而 Cross-Module JIT 调用能正确解析
    if let Some(module_context) = prelude_context {
        let dependency_interfaces = module_context.dependency_interfaces();
        if dependency_interfaces.contains_key("std.prelude") {
            let prelude_modules = ["std.core", "std.math", "std.io", "std.string"];
            for mod_key in &prelude_modules {
                if let Some(iface) = dependency_interfaces.get(*mod_key) {
                    for (func_name, _func_info) in &iface.functions {
                        let canonical = format!("{}::{}", mod_key, func_name);
                        mir_program.set_external_function_symbol(func_name, &canonical);
                    }
                }
            }
        }
    }

    if let Some(script_entry) = mir_program.functions.get(SCRIPT_ENTRY_POINT) {
        let is_trivial =
            if let Some(entry_block) = script_entry.basic_blocks.get(&script_entry.entry_block) {
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

        if is_trivial {
            // skip
        }
    }

    if let Some(script_entry) = mir_program.functions.get(SCRIPT_ENTRY_POINT) {
        if mir_program.functions.contains_key("main") {
            if verbose {
                println!("Project Mode detected: switching entry point to 'main'");
            }
            mir_program.set_main("main".to_string());
        }
    }

    if verbose {
        println!("{}", mir_program.to_ir_string());
    }

    // 【Phase 6】运行逃逸分析优化MIR（永远启用）
    if verbose {
        println!("\n--- 运行逃逸分析 ---");
    }
    optimize_mir_with_escape_analysis(&mut mir_program, verbose)
        .map_err(|e| format!("逃逸分析失败: {}", e))?;

    if verbose {
        println!("\n--- 优化后的 MIR ---");
        println!("{}", mir_program.to_ir_string());
    }

    let lir_program = lower_mir_to_unoptimized_lir(&mir_program, verbose)?;

    if let Some(ctx) = cache_key {
        let cache = CompilationCache::new();
        let safe_module_id = crate::sanitize_module_id_for_filename(ctx.module_id);
        let key_str = format!("{}-{:x}", safe_module_id, ctx.interface_hash);
        cache.store(&key_str, &mir_program, &lir_program);
    }

    // 在所有使用 result 引用的操作完成之后，再 move program
    let parsed_program = result.program;

    Ok(CompilationArtifacts {
        parsed_program,
        mir_program,
        lir_program,
    })
}

pub fn compile_to_lir(
    input: &str,
    filename: &str,
    optimization_level: OptimizationLevel,
    verbose: bool,
    mode: ParserMode,
) -> Result<LirProgram, Box<dyn std::error::Error>> {
    let artifacts = compile_source_to_artifacts(
        input,
        filename,
        optimization_level,
        verbose,
        None,
        mode,
        None,
    )?;
    Ok(artifacts.lir_program)
}

pub fn compile_entry_file(
    filename: &str,
    optimization_level: OptimizationLevel,
    verbose: bool,
    mode: ParserMode,
) -> Result<CompilationArtifacts, Box<dyn std::error::Error>> {
    let entry_path = Path::new(filename);
    let module_graph = ModuleGraph::load_for_entry(entry_path).map_err(|err| format!("{}", err))?;
    let plan = module_graph
        .plan_for_entry(entry_path)
        .map_err(|err| format!("{}", err))?;
    let canonical_entry = fs::canonicalize(entry_path)?;
    let mut interface_hashes: HashMap<ModuleId, u64> = HashMap::new();
    let mut interface_artifacts: HashMap<ModuleId, ModuleInterfaceArtifact> = HashMap::new();
    let mut compiled_artifacts_by_module: HashMap<ModuleId, Vec<CompilationArtifacts>> =
        HashMap::new();
    let layers = module_graph.schedule_layers(&plan);

    if verbose {
        if let Some(manifest) = module_graph.manifest_path() {
            println!("[module] manifest: {}", manifest.display());
        }
        for description in module_graph.describe() {
            println!("[module] {}", description);
        }
        println!("[module] topo count = {}", plan.sequence.len());
        println!("[module] layer count = {}", layers.len());
    }

    let mut entry_result = None;
    for (layer_idx, layer) in layers.iter().enumerate() {
        if verbose {
            let summary = layer
                .iter()
                .map(|id| id.as_str().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            println!("[module] layer {} => [{}]", layer_idx + 1, summary);
        }

        let dependency_snapshot = interface_hashes.clone();
        let artifact_snapshot = interface_artifacts.clone();
        let results: Vec<Result<LayerCompilationResult, String>> = layer
            .par_iter()
            .map(|module_id| {
                compile_module_in_layer(
                    &module_graph,
                    module_id,
                    &dependency_snapshot,
                    &artifact_snapshot,
                    optimization_level,
                    verbose,
                    None,
                    mode,
                    &plan.entry,
                    &canonical_entry,
                )
            })
            .collect();

        for result in results {
            match result {
                Ok(layer_result) => {
                    let module_id = layer_result.module_id.clone();
                    if module_id == plan.entry {
                        if let Some(artifacts) = layer_result.entry_artifacts {
                            entry_result = Some(artifacts);
                        }
                    }
                    compiled_artifacts_by_module
                        .insert(module_id.clone(), layer_result.compiled_artifacts);
                    interface_hashes.insert(module_id.clone(), layer_result.interface_hash);
                    interface_artifacts.insert(module_id, layer_result.interface_artifact);
                }
                Err(err) => {
                    return Err(Box::new(io::Error::new(io::ErrorKind::Other, err)));
                }
            }
        }
    }

    let mut entry_artifacts =
        entry_result.ok_or_else(|| io::Error::new(io::ErrorKind::Other, "入口模块未被编译"))?;
    merge_module_artifacts(&plan, &compiled_artifacts_by_module, &mut entry_artifacts);
    Ok(entry_artifacts)
}

/// 将MIR降级到未优化的LIR（包含虚拟寄存器）
pub fn lower_mir_to_unoptimized_lir(
    mir_program: &MirProgram,
    verbose: bool,
) -> Result<LirProgram, Box<dyn std::error::Error>> {
    if verbose {
        println!("\n--- Lowering to Unoptimized LIR ---");
    }

    let lir_program = match lower_mir_to_lir(mir_program) {
        Ok(prog) => prog,
        Err(errors) => {
            for err in errors {
                error!("LIR Lowering Error: {}", err);
            }
            return Err("LIR lowering failed".into());
        }
    };

    if verbose {
        println!("未优化LIR生成完成（包含虚拟寄存器）");
    }

    Ok(lir_program)
}

pub fn lower_mir_to_final_lir(
    mir_program: &MirProgram,
    optimization_level: OptimizationLevel,
    verbose: bool,
) -> Result<LirProgram, Box<dyn std::error::Error>> {
    if verbose {
        println!("\n--- Lowering to LIR ---");
        println!("=== 返回高级LIR (包含Alloc指令，待优化) ===");
    }

    let mut lir_program = match lower_mir_to_lir(mir_program) {
        Ok(prog) => prog,
        Err(errors) => {
            for err in errors {
                error!("LIR Lowering Error: {}", err);
            }
            return Err("LIR lowering failed".into());
        }
    };

    if verbose {
        println!("{}", lir_program.to_ir_string());
        println!("================================================");
    }

    if verbose {
        println!("\n--- LIR 优化 ---");
    }
    let mut pipeline = OptimizationPipeline::new(optimization_level);
    match pipeline.optimize(&mut lir_program) {
        Ok(stats) => {
            if verbose {
                println!("优化完成:");
                println!("  - 总耗时: {}ms", stats.total_time_ms);
                println!("  - 执行pass数: {}", stats.passes_executed);
                println!(
                    "  - 指令数变化: {} -> {}",
                    stats.instructions_before, stats.instructions_after
                );
            }
        }
        Err(errors) => {
            for err in errors {
                error!("LIR 优化错误: {}", err);
            }
            return Err("LIR optimization failed".into());
        }
    }

    if verbose {
        println!("\n--- 优化后LIR ---");
        println!("{}", lir_program.to_ir_string());
    }

    // 注意：指令降级现在已在优化管线中自动执行
    if verbose {
        println!("\n--- 指令降级（在优化管线中自动执行） ---");
    }

    if verbose {
        println!("\n--- 降级后LIR (可执行) ---");
        println!("{}", lir_program.to_ir_string());
    }

    Ok(lir_program)
}

pub fn merge_module_artifacts(
    plan: &ModulePlan,
    compiled: &HashMap<ModuleId, Vec<CompilationArtifacts>>,
    entry_artifacts: &mut CompilationArtifacts,
) {
    let mut merged_lir = LirProgram::new();
    let mut merged_mir = MirProgram::new();
    for module_id in &plan.sequence {
        if let Some(units) = compiled.get(module_id) {
            for unit in units {
                merge_mir_program(&mut merged_mir, &unit.mir_program);
                merge_lir_program(&mut merged_lir, &unit.lir_program);
            }
        }
    }
    merged_mir.main_function = entry_artifacts.mir_program.main_function.clone();
    merged_lir.main_function = entry_artifacts.lir_program.main_function.clone();
    entry_artifacts.mir_program = merged_mir;
    entry_artifacts.lir_program = merged_lir;
}

pub fn merge_mir_program(target: &mut MirProgram, source: &MirProgram) {
    for (name, function) in &source.functions {
        target.functions.insert(name.clone(), function.clone());
    }
    for (name, ty) in &source.struct_types {
        target.struct_types.insert(name.clone(), ty.clone());
    }
    for (name, symbol) in &source.function_symbols {
        target.function_symbols.insert(name.clone(), symbol.clone());
    }
    for (name, symbol) in &source.external_function_symbols {
        target
            .external_function_symbols
            .insert(name.clone(), symbol.clone());
    }
}

pub fn merge_lir_program(target: &mut LirProgram, source: &LirProgram) {
    for (name, function) in &source.functions {
        target
            .functions
            .entry(name.clone())
            .or_insert_with(|| function.clone());
    }
    for (name, layout) in &source.global_struct_types {
        target
            .global_struct_types
            .insert(name.clone(), layout.clone());
    }
    for (name, memory) in &source.global_variables {
        target.global_variables.insert(name.clone(), *memory);
    }
    if target.main_function.is_none() {
        target.main_function = source.main_function.clone();
    }
}

#[derive(Clone)]
pub struct LayerCompilationResult {
    pub module_id: ModuleId,
    pub interface_hash: u64,
    pub entry_artifacts: Option<CompilationArtifacts>,
    pub compiled_artifacts: Vec<CompilationArtifacts>,
    pub interface_artifact: ModuleInterfaceArtifact,
}

pub fn compile_module_in_layer(
    graph: &ModuleGraph,
    module_id: &ModuleId,
    dependency_interfaces: &HashMap<ModuleId, u64>,
    dependency_interface_artifacts: &HashMap<ModuleId, ModuleInterfaceArtifact>,
    optimization_level: OptimizationLevel,
    verbose: bool,
    progress_bar: Option<Arc<ProgressBar>>,
    mode: ParserMode,
    entry_module: &ModuleId,
    canonical_entry: &Path,
) -> Result<LayerCompilationResult, String> {
    let meta = graph
        .metadata(module_id)
        .ok_or_else(|| format!("缺少模块元数据: {}", module_id))?
        .clone();

    if let Some(pb) = &progress_bar {
        pb.set_message(format!("正在编译 {}", module_id.as_str()));
    }
    let cache_version = compute_module_cache_version(module_id, &meta, dependency_interfaces);
    let mut interface_acc = ModuleInterfaceAccumulator::default();
    let mut entry_result = None;
    let enforce_module_ids = graph.manifest_path().is_some();
    let mut compiled_artifacts = Vec::new();
    let dependency_interface_map =
        build_type_checker_dependency_map(&meta, dependency_interface_artifacts);

    for source_path in &meta.sources {
        let source = fs::read_to_string(source_path)
            .map_err(|e| format!("读取 {} 失败: {}", source_path.display(), e))?;
        let path_str = source_path.to_string_lossy().to_string();
        let cache_ctx = CacheContext {
            module_id: module_id.as_str(),
            interface_hash: cache_version,
        };
        let artifacts = compile_source_to_artifacts(
            &source,
            &path_str,
            optimization_level,
            verbose,
            Some(cache_ctx),
            mode,
            Some(&dependency_interface_map),
        )
        .map_err(|e| format!("编译 {} 失败: {}", source_path.display(), e))?;
        if enforce_module_ids {
            ensure_module_decl(module_id, &artifacts, source_path)?;
            validate_module_imports(
                module_id,
                artifacts.imports(),
                &meta,
                dependency_interface_artifacts,
            )?;
        }
        interface_acc.observe(&artifacts.mir_program);
        if module_id == entry_module && source_path == canonical_entry {
            entry_result = Some(artifacts.clone());
        }
        compiled_artifacts.push(artifacts);
    }

    let interface_summary = interface_acc.finalize(module_id, &meta, dependency_interfaces);
    if let Err(err) = write_module_interface_artifact(module_id, &interface_summary.artifact) {
        warn!("[module] 无法写入接口文件 {}: {}", module_id.as_str(), err);
    } else if verbose && progress_bar.is_none() {
        println!(
            "[module] interface persisted for {} -> {}",
            module_id.as_str(),
            interface_summary.artifact.interface_hash
        );
    }

    let ModuleInterfaceSummary {
        interface_hash: public_interface_hash,
        artifact,
    } = interface_summary;

    Ok(LayerCompilationResult {
        module_id: module_id.clone(),
        interface_hash: public_interface_hash,
        entry_artifacts: entry_result,
        compiled_artifacts,
        interface_artifact: artifact,
    })
}

fn build_type_checker_dependency_map(
    meta: &ModuleMetadata,
    artifacts: &HashMap<ModuleId, ModuleInterfaceArtifact>,
) -> HashMap<String, ExternalModuleInterface> {
    let mut map = HashMap::new();
    for dep in &meta.dependencies {
        if let Some(artifact) = artifacts.get(dep) {
            map.insert(
                dep.as_str().to_string(),
                external_interface_from_artifact(artifact),
            );
        }
    }
    map
}

fn external_interface_from_artifact(artifact: &ModuleInterfaceArtifact) -> ExternalModuleInterface {
    let functions = artifact
        .exports
        .functions
        .iter()
        .map(|func| {
            (
                func.name.clone(),
                ExternalFunctionSignature {
                    name: func.name.clone(),
                    params: func.params,
                },
            )
        })
        .collect();

    let structs = artifact
        .exports
        .structs
        .iter()
        .map(|structure| {
            (
                structure.name.clone(),
                ExternalStructSignature {
                    name: structure.name.clone(),
                    fields: structure
                        .fields
                        .iter()
                        .map(|field| ExternalStructField {
                            name: field.name.clone(),
                            ty: field.ty.clone(),
                        })
                        .collect(),
                },
            )
        })
        .collect();

    ExternalModuleInterface { functions, structs }
}

fn ensure_module_decl(
    module_id: &ModuleId,
    artifacts: &CompilationArtifacts,
    source_path: &Path,
) -> Result<(), String> {
    match artifacts.module_name() {
        Some(name) if name == module_id.as_str() => Ok(()),
        Some(name) => Err(format!(
            "{} 声明 `module {}`，但 manifest 将其注册为 `{}`",
            source_path.display(),
            name,
            module_id.as_str()
        )),
        None => Err(format!(
            "{} 缺少 `module {}` 声明 (manifest id `{}`)",
            source_path.display(),
            module_id.as_str(),
            module_id.as_str()
        )),
    }
}

/// 对MIR程序运行逃逸分析并插入优化的分配指令
///
/// 该函数会：
/// 1. 对每个MIR函数运行逃逸分析
/// 2. 基于逃逸分析结果生成分配策略（栈、堆、内联、寄存器）
/// 3. 在函数入口基本块插入分配指令（StackAllocate等）
///
/// # 参数
/// - `mir_program`: 要优化的MIR程序
/// - `verbose`: 是否输出详细日志
///
/// # 返回
/// 优化后的MIR程序（会修改原始程序）
pub fn optimize_mir_with_escape_analysis(
    mir_program: &mut MirProgram,
    verbose: bool,
) -> Result<(), String> {
    if verbose {
        println!("\n=== 开始逃逸点插入优化 ===");
        println!("程序包含 {} 个函数", mir_program.functions.len());
        info!("开始逃逸点插入优化...");
    }

    // 步骤1: 运行逃逸分析
    let mut analyzer = EscapeAnalyzer::new();
    analyzer
        .analyze_program(mir_program)
        .map_err(|e| format!("逃逸分析失败: {}", e))?;

    if verbose {
        println!("逃逸分析完成");
        analyzer.print_results();
    }

    // 步骤2: 获取逃逸分析结果
    let escape_info = analyzer.get_all_escape_info().clone();
    let variable_names = analyzer.get_variable_name_mapping().clone();

    if verbose {
        println!("\n=== 构建逃逸点检测器 ===");
        println!("找到 {} 个变量", escape_info.len());
    }

    // 步骤3: 构建逃逸点检测器
    let detector = EscapePointDetector::new(escape_info.clone(), variable_names.clone());

    if verbose {
        println!("\n=== 开始逃逸点转换 ===");
    }

    // 步骤4: 应用逃逸点转换
    let mut transformer = EscapePointTransformer::new(detector);
    transformer.transform_program(mir_program);

    if verbose {
        println!("逃逸点转换完成");
        info!("逃逸点转换完成");
    }

    Ok(())
}
