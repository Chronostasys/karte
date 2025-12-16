use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use karte_codegen::vm::professional_executor::ProfessionalExecutor;
use karte_ir_codec::IrDisplay;
use karte_lir::optimization_pipeline::OptimizationLevel;
use karte_lir::LirProgram;
use karte_mir::{MirProgram, Statement, Terminator, Value};
use karte_module_system::{
    compile_entry_file, compile_project_with_context, compile_to_lir, merge_module_artifacts,
    CompilationArtifacts, ProjectBuildContext, ProjectCompilationOutput,
};
use karte_parser::ParserMode;
use karte_rt::{ffi, HeapStats};
use log::{error, warn};
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn get_terminal_width() -> usize {
    // Prefer explicit COLUMNS env (common in CI/containers). Fallback to 80.
    if let Ok(cols_str) = std::env::var("COLUMNS") {
        if let Ok(cols) = cols_str.parse::<usize>() {
            if cols > 0 {
                return cols;
            }
        }
    }

    // Fallback: try to read from tty via `stty size` as a best-effort without
    // introducing an extra crate. If that fails, return 80.
    if let Ok(output) = std::process::Command::new("sh")
        .arg("-c")
        .arg("stty size 2>/dev/null || true")
        .output()
    {
        if output.status.success() {
            if let Ok(s) = String::from_utf8(output.stdout) {
                let parts: Vec<&str> = s.split_whitespace().collect();
                if parts.len() >= 2 {
                    if let Ok(cols) = parts[1].parse::<usize>() {
                        if cols > 0 {
                            return cols;
                        }
                    }
                }
            }
        }
    }

    80
}

enum BuildInput {
    AutoDetect,
    Project(ProjectBuildContext),
    Script,
}

enum BuildProduct {
    Project(ProjectCompilationOutput),
    Script(CompilationArtifacts),
}

struct BuildOutputPaths {
    lir_path: PathBuf,
    mir_path: Option<PathBuf>,
}

fn canonical_mir_program(program: &MirProgram) -> MirProgram {
    let mut canonical = program.clone();
    let mut symbol_map = program.function_symbols.clone();
    symbol_map.extend(program.external_function_symbols.clone());

    if program.functions.is_empty() {
        canonical.function_symbols.clear();
        return canonical;
    }

    let mut renamed = HashMap::with_capacity(program.functions.len());
    for (alias, function) in &program.functions {
        let canonical_name = symbol_map
            .get(alias)
            .cloned()
            .unwrap_or_else(|| alias.clone());
        let mut function = function.clone();
        function.name = canonical_name.clone();
        canonicalize_function_values(&mut function, &symbol_map);
        renamed.insert(canonical_name, function);
    }
    canonical.functions = renamed;

    if let Some(main) = &program.main_function {
        let canonical_main = symbol_map
            .get(main)
            .cloned()
            .unwrap_or_else(|| main.clone());
        canonical.main_function = Some(canonical_main);
    }

    canonical.function_symbols.clear();
    canonical
}

fn canonicalize_function_values(
    function: &mut karte_mir::MirFunction,
    symbols: &HashMap<String, String>,
) {
    for block in function.basic_blocks.values_mut() {
        for statement in &mut block.statements {
            canonicalize_statement(statement, symbols);
        }
        if let Some(terminator) = &mut block.terminator {
            canonicalize_terminator(terminator, symbols);
        }
    }
}

fn canonicalize_statement(statement: &mut Statement, symbols: &HashMap<String, String>) {
    match statement {
        Statement::Assign { target, source, .. } => {
            canonicalize_value(target, symbols);
            canonicalize_value(source, symbols);
        }
        Statement::BinaryOp {
            target,
            left,
            right,
            ..
        } => {
            canonicalize_value(target, symbols);
            canonicalize_value(left, symbols);
            canonicalize_value(right, symbols);
        }
        Statement::UnaryOp {
            target, operand, ..
        } => {
            canonicalize_value(target, symbols);
            canonicalize_value(operand, symbols);
        }
        Statement::Call {
            target,
            function,
            args,
            ..
        } => {
            if let Some(target) = target {
                canonicalize_value(target, symbols);
            }
            canonicalize_value(function, symbols);
            for arg in args {
                canonicalize_value(arg, symbols);
            }
        }
        Statement::Store { target, value, .. } => {
            canonicalize_value(target, symbols);
            canonicalize_value(value, symbols);
        }
        Statement::FieldAccess { target, object, .. } => {
            canonicalize_value(target, symbols);
            canonicalize_value(object, symbols);
        }
        Statement::Dereference {
            target, reference, ..
        } => {
            canonicalize_value(target, symbols);
            canonicalize_value(reference, symbols);
        }
        Statement::ConstructorArgExtract {
            target,
            constructor,
            ..
        } => {
            canonicalize_value(target, symbols);
            canonicalize_value(constructor, symbols);
        }
        Statement::FieldAssign { object, value, .. } => {
            canonicalize_value(object, symbols);
            canonicalize_value(value, symbols);
        }
        Statement::Allocate { target, .. } => {
            canonicalize_value(target, symbols);
        }
        Statement::Deallocate { pointer, .. } => {
            canonicalize_value(pointer, symbols);
        }
        Statement::Retain { value, .. } | Statement::Release { value, .. } => {
            canonicalize_value(value, symbols);
        }
        Statement::MarkGcRoot { value, .. } => {
            canonicalize_value(value, symbols);
        }
        Statement::WriteBarrier { object, value, .. } => {
            canonicalize_value(object, symbols);
            canonicalize_value(value, symbols);
        }
        Statement::ReadBarrier { target, object, .. } => {
            canonicalize_value(target, symbols);
            canonicalize_value(object, symbols);
        }
        Statement::HeapAlloc { target, .. } => {
            canonicalize_value(target, symbols);
        }
        Statement::Phi {
            target, incoming, ..
        } => {
            canonicalize_value(target, symbols);
            for (_, value) in incoming {
                canonicalize_value(value, symbols);
            }
        }
        Statement::EffectPerform {
            tag,
            payload,
            target,
            ..
        } => {
            canonicalize_value(tag, symbols);
            canonicalize_value(payload, symbols);
            if let Some(target) = target {
                canonicalize_value(target, symbols);
            }
        }
        Statement::EffectResume { value, .. } => {
            canonicalize_value(value, symbols);
        }
        Statement::EffectHandlerPush { tag, .. } => {
            canonicalize_value(tag, symbols);
        }
        Statement::EffectHandlerPop { .. } => {}
        Statement::StackAllocate { target, .. } => {
            canonicalize_value(target, symbols);
        }
    }
}

fn canonicalize_terminator(terminator: &mut Terminator, symbols: &HashMap<String, String>) {
    match terminator {
        Terminator::Goto { .. } => {}
        Terminator::Branch { condition, .. } => {
            canonicalize_value(condition, symbols);
        }
        Terminator::Return { value, .. } => {
            if let Some(value) = value {
                canonicalize_value(value, symbols);
            }
        }
        Terminator::Match { value, .. } => {
            canonicalize_value(value, symbols);
        }
    }
}

fn canonicalize_value(value: &mut Value, symbols: &HashMap<String, String>) {
    match value {
        Value::Function { name, .. } => {
            if let Some(canonical) = symbols.get(name) {
                *name = canonical.clone();
            }
        }
        Value::Closure {
            function_name,
            captured_values,
            ..
        } => {
            if let Some(canonical) = symbols.get(function_name) {
                *function_name = canonical.clone();
            }
            for captured in captured_values {
                canonicalize_value(captured, symbols);
            }
        }
        Value::Constructor { arg, .. } | Value::QualifiedConstructor { arg, .. } => {
            if let Some(arg) = arg.as_deref_mut() {
                canonicalize_value(arg, symbols);
            }
        }
        Value::Struct { fields, .. } => {
            for field_value in fields.values_mut() {
                canonicalize_value(field_value, symbols);
            }
        }
        Value::Reference { value, .. } => {
            canonicalize_value(value, symbols);
        }
        Value::Variable { .. }
        | Value::Number { .. }
        | Value::Boolean { .. }
        | Value::Unit
        | Value::Temp { .. } => {}
    }
}

impl BuildProduct {
    fn lir_program(&self) -> &LirProgram {
        match self {
            BuildProduct::Project(p) => &p.final_program,
            BuildProduct::Script(a) => &a.lir_program,
        }
    }

    fn into_lir_program(self) -> LirProgram {
        match self {
            BuildProduct::Project(p) => p.final_program,
            BuildProduct::Script(a) => a.lir_program,
        }
    }

    fn write_outputs(
        &mut self,
        output_dir: &Path,
        emit_mir: bool,
    ) -> Result<BuildOutputPaths, String> {
        fs::create_dir_all(output_dir).map_err(|e| {
            format!(
                "Failed to create output dir {}: {}",
                output_dir.display(),
                e
            )
        })?;

        let lir_path = output_dir.join("main.lir");
        fs::write(&lir_path, self.lir_program().to_ir_string())
            .map_err(|e| format!("Failed to write output {}: {}", lir_path.display(), e))?;

        let mir_path = if emit_mir {
            let mir_dir = output_dir.join("mir");
            if let Err(e) = fs::create_dir_all(&mir_dir) {
                warn!(
                    "Failed to create mir output dir {}: {}",
                    mir_dir.display(),
                    e
                );
                None
            } else {
                let out = mir_dir.join("main.mir");
                match self {
                    BuildProduct::Project(project) => {
                        let entry_artifacts = project
                            .entry_artifacts
                            .as_mut()
                            .ok_or_else(|| "入口模块未被编译，无法生成合并的 MIR".to_string())?;
                        merge_module_artifacts(
                            &project.plan,
                            &project.compiled_artifacts_by_module,
                            entry_artifacts,
                        );
                        let canonical_mir = canonical_mir_program(&entry_artifacts.mir_program);
                        if let Err(e) = fs::write(&out, canonical_mir.to_ir_string()) {
                            warn!("Failed to write merged MIR file {}: {}", out.display(), e);
                            None
                        } else {
                            Some(out)
                        }
                    }
                    BuildProduct::Script(artifacts) => {
                        let canonical_mir = canonical_mir_program(&artifacts.mir_program);
                        if let Err(e) = fs::write(&out, canonical_mir.to_ir_string()) {
                            warn!("Failed to write MIR file {}: {}", out.display(), e);
                            None
                        } else {
                            Some(out)
                        }
                    }
                }
            }
        } else {
            None
        };

        Ok(BuildOutputPaths { lir_path, mir_path })
    }
}

fn create_progress_bar(
    total_modules: usize,
    layer_count: usize,
    progress: bool,
) -> Option<Arc<ProgressBar>> {
    if !progress {
        return None;
    }

    let pb = ProgressBar::new(total_modules as u64);

    // Compute adaptive sections so the template scales with any terminal width.
    let term_width = get_terminal_width();
    let reserved = 48usize;
    let max_bar = 80usize;
    let min_bar = 20usize;
    let avail = term_width.saturating_sub(reserved);
    let bar_len = std::cmp::min(max_bar, std::cmp::max(min_bar, avail));

    // Allow the message line to grow on larger screens but stay bounded.
    let msg_reserved = 12usize;
    let min_msg = 24usize;
    let max_msg = 120usize;
    let msg_avail = term_width.saturating_sub(msg_reserved);
    let msg_width = std::cmp::min(max_msg, std::cmp::max(min_msg, msg_avail));

    let template = format!(
        "{{prefix:.bold.cyan}}\n  {{spinner:.green}} [{{elapsed_precise}}<{{eta_precise}}] \
             {{bar:{bar_len}.bright_cyan/bright_blue}} {{percent:>3}}% \
             ({{human_pos}}/{{human_len}}) {{per_sec:>7}}\n  -> {{msg:<{msg_width}!}}",
        bar_len = bar_len,
        msg_width = msg_width,
    );

    let style = ProgressStyle::with_template(&template)
        .expect("进度条模板格式应该有效")
        .progress_chars("█▓░")
        .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]);

    pb.set_style(style);
    pb.set_draw_target(ProgressDrawTarget::stderr_with_hz(30));
    pb.enable_steady_tick(std::time::Duration::from_millis(120));
    pb.set_prefix(format!("Layer 1/{}", std::cmp::max(1, layer_count)));
    pb.set_message("初始化构建计划");
    Some(Arc::new(pb))
}

fn compile_script_entry(
    entry_path: &Path,
    optimization_level: OptimizationLevel,
    verbose: bool,
) -> Result<CompilationArtifacts, String> {
    let path_str = entry_path
        .to_str()
        .ok_or_else(|| "入口文件路径不是有效的 UTF-8".to_string())?;
    compile_entry_file(path_str, optimization_level, verbose, ParserMode::Script)
        .map_err(|e| e.to_string())
}

fn build_project_product(
    context: ProjectBuildContext,
    optimization_level: OptimizationLevel,
    verbose: bool,
    progress: bool,
    announce: bool,
) -> Result<BuildProduct, String> {
    if announce {
        println!("构建计划: {} 个模块", context.total_modules());
        if verbose {
            for module in &context.plan.sequence {
                println!("  - {}", module);
            }
        }
        println!("分层调度: {} 层", context.layer_count());
    }

    let progress_bar =
        create_progress_bar(context.total_modules(), context.layer_count(), progress);
    let project =
        compile_project_with_context(&context, optimization_level, verbose, progress_bar)?;

    if announce {
        println!("构建完成！");
    }

    Ok(BuildProduct::Project(project))
}

fn build_script_product(
    entry_path: &Path,
    optimization_level: OptimizationLevel,
    verbose: bool,
    announce: bool,
) -> Result<BuildProduct, String> {
    if announce {
        println!("脚本构建: {}", entry_path.display());
    }
    let artifacts = compile_script_entry(entry_path, optimization_level, verbose)?;
    if announce {
        println!("构建完成！");
    }
    Ok(BuildProduct::Script(artifacts))
}

fn build_product_for_entry(
    entry_path: &Path,
    input: BuildInput,
    optimization_level: OptimizationLevel,
    verbose: bool,
    progress: bool,
    announce: bool,
) -> Result<BuildProduct, String> {
    match input {
        BuildInput::Project(context) => {
            build_project_product(context, optimization_level, verbose, progress, announce)
        }
        BuildInput::Script => {
            build_script_product(entry_path, optimization_level, verbose, announce)
        }
        BuildInput::AutoDetect => {
            if let Some(context) = ProjectBuildContext::try_new(entry_path)? {
                build_project_product(context, optimization_level, verbose, progress, announce)
            } else {
                build_script_product(entry_path, optimization_level, verbose, announce)
            }
        }
    }
}

pub fn execute_lir(
    lir_program: &LirProgram,
    verbose: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if verbose {
        println!("\n--- Executing LIR ---");
    }

    // 始终尝试使用 JIT 执行；解释器已被移除，因此失败会返回错误
    if verbose {
        println!(
            "entry: {}",
            lir_program
                .main_function
                .as_ref()
                .map_or("<none>".to_string(), |f| f.clone())
        );
        println!("尝试使用 JIT 执行...");
    }

    match ProfessionalExecutor::new_with_jit(verbose) {
        Ok(mut executor) => {
            if verbose {
                println!("使用JIT执行器");
            }
            match executor.execute_with_jit(lir_program) {
                Ok(exit_code) => {
                    println!("JIT执行完成，退出码: {}", exit_code);
                    Ok(())
                }
                Err(err) => Err(format!("JIT执行失败: {}", err).into()),
            }
        }
        Err(err) => Err(format!("无法创建JIT执行器: {}", err).into()),
    }
}

pub fn capture_heap_stats() -> HeapStats {
    let mut stats = HeapStats::default();
    ffi::karte_jit_runtime_heap_stats(&mut stats as *mut _);
    stats
}

pub fn print_heap_stats(context: &str, target: &str, before: HeapStats, after: HeapStats) {
    println!("[heap][{}] {}", context, target);
    println!(
        "  active_allocations: {} -> {} (Δ{})",
        before.active_allocations,
        after.active_allocations,
        delta_u64(after.active_allocations, before.active_allocations)
    );
    println!(
        "  rc_tracked_objects: {} -> {} (Δ{})",
        before.rc_tracked_objects,
        after.rc_tracked_objects,
        delta_u64(after.rc_tracked_objects, before.rc_tracked_objects)
    );
    println!(
        "  retain/release ops: +{}/+{} (rc_zero Δ {})",
        delta_u64(after.total_retain_ops, before.total_retain_ops),
        delta_u64(after.total_release_ops, before.total_release_ops),
        delta_u64(after.rc_zero_releases, before.rc_zero_releases)
    );
    println!(
        "  bytes_in_use: {} -> {} (Δ{})",
        before.bytes_in_use,
        after.bytes_in_use,
        delta_usize(after.bytes_in_use, before.bytes_in_use)
    );
}

fn delta_u64(after: u64, before: u64) -> i128 {
    after as i128 - before as i128
}

fn delta_usize(after: usize, before: usize) -> i128 {
    after as i128 - before as i128
}

pub fn write_content_creating_parent<P: AsRef<Path>>(path: P, content: &str) -> io::Result<()> {
    let path_ref = path.as_ref();
    if let Some(parent) = path_ref.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(path_ref, content)
}

pub fn process_file(
    filename: &str,
    optimization_level: OptimizationLevel,
    verbose: bool,
    emit_lir: bool,
    output_file: Option<&str>,
    heap_stats: bool,
    mode: ParserMode,
    mode_is_explicit: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let entry_path = Path::new(filename);
    let project_context = ProjectBuildContext::try_new(entry_path)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    let build_input = if let Some(context) = project_context {
        if mode != ParserMode::Project {
            if mode_is_explicit {
                return Err(format!(
                    "检测到 `{}` 位于 Karte 项目中，但指定了 --mode script；请改用 --mode project",
                    filename
                )
                .into());
            } else {
                println!(
                    "ℹ️  检测到项目结构，自动切换到 project 模式编译 `{}`",
                    filename
                );
            }
        }
        BuildInput::Project(context)
    } else {
        if mode == ParserMode::Project {
            if mode_is_explicit {
                return Err(format!(
                    "`{}` 看起来是独立脚本，无法使用 --mode project；请省略该参数或使用 --mode script",
                    filename
                )
                .into());
            } else {
                println!("ℹ️  `{}` 不在项目中，自动切换到 script 模式", filename);
            }
        }
        BuildInput::Script
    };

    let progress = matches!(build_input, BuildInput::Project(_));
    let build_product = build_product_for_entry(
        entry_path,
        build_input,
        optimization_level,
        verbose,
        progress,
        false,
    )
    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    let mut lir_program = build_product.into_lir_program();
    let before_stats = heap_stats.then_some(capture_heap_stats());

    if let Some(output_path) = output_file {
        let lir_code = lir_program.to_ir_string();
        write_content_creating_parent(output_path, &lir_code)?;
        println!("LIR代码已输出到: {}", output_path);
    }

    if emit_lir {
        println!("{}", lir_program.to_ir_string());
    } else {
        let mut pipeline = karte_lir::OptimizationPipeline::new(optimization_level);
        pipeline
            .optimize(&mut lir_program)
            .map_err(|errors| -> Box<dyn std::error::Error> {
                format!("LIR优化失败: {}", errors.join(", ")).into()
            })?;
        execute_lir(&lir_program, verbose)?;
    }

    if let Some(before) = before_stats {
        let after = capture_heap_stats();
        print_heap_stats("file", filename, before, after);
    }

    Ok(())
}

pub fn process_expression(
    input: &str,
    optimization_level: OptimizationLevel,
    verbose: bool,
    emit_lir: bool,
    output_file: Option<&str>,
    heap_stats: bool,
    mode: ParserMode,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut lir_program = compile_to_lir(input, "input", optimization_level, verbose, mode)?;
    let before_stats = heap_stats.then_some(capture_heap_stats());

    if let Some(output_path) = output_file {
        let lir_code = lir_program.to_ir_string();
        write_content_creating_parent(output_path, &lir_code)?;
        println!("LIR代码已输出到: {}", output_path);
    }

    if emit_lir {
        println!("{}", lir_program.to_ir_string());
    } else {
        let mut pipeline = karte_lir::OptimizationPipeline::new(optimization_level);
        pipeline
            .optimize(&mut lir_program)
            .map_err(|errors| -> Box<dyn std::error::Error> {
                format!("LIR优化失败: {}", errors.join(", ")).into()
            })?;
        execute_lir(&lir_program, verbose)?;
    }

    if let Some(before) = before_stats {
        let after = capture_heap_stats();
        print_heap_stats("expression", "input", before, after);
    }

    Ok(())
}

pub fn run_repl(optimization_level: OptimizationLevel, verbose: bool) {
    println!("Karte REPL (JIT 执行)");
    println!("当前优化级别: {:?}", optimization_level);
    println!();
    println!("支持的功能:");
    println!("  - 基础运算: 1 + 2 * 3");
    println!("  - 变量绑定: let x = 5; x + 10");
    println!("  - 函数定义: let f = |x| x * 2; f(5)");
    println!("  - 布尔类型: true, false");
    println!("  - 条件表达式: if true then 42 else 0");
    println!("  - 循环表达式: while false do 42");
    println!("  - 加法类型: Some(42), None");
    println!("  - 模式匹配: match Some(42) {{ Some(x) -> x, None -> 0 }}");
    println!("  - 结构体: struct Point {{ x: number, y: number }}");
    println!("输入表达式进行计算，输入 'quit' 或 'exit' 退出");
    println!("你也可以传入文件名作为参数来执行文件: karte run filename.karte");
    println!("使用 --help 查看所有选项");
    println!();

    loop {
        print!("> ");
        // 刷新stdout，忽略错误（在REPL中不是致命错误）
        let _ = io::stdout().flush();

        let mut input = String::new();
        match io::stdin().read_line(&mut input) {
            Ok(_) => {
                let input = input.trim();

                if input.is_empty() {
                    continue;
                }

                if input == "quit" || input == "exit" {
                    println!("再见！");
                    break;
                }

                if input.starts_with("load ") {
                    let filename = input.strip_prefix("load ").expect("已检查前缀存在").trim();
                    if let Err(err) = process_file(
                        filename,
                        optimization_level,
                        verbose,
                        false,
                        None,
                        false,
                        ParserMode::Script,
                        true,
                    ) {
                        error!("Error reading file '{}': {}", filename, err);
                    }
                    continue;
                }

                if let Err(err) = process_expression(
                    input,
                    optimization_level,
                    verbose,
                    false,
                    None,
                    false,
                    ParserMode::Script,
                ) {
                    error!("Error: {}", err);
                }
            }
            Err(error) => {
                error!("Error reading input: {}", error);
                break;
            }
        }
    }
}

pub fn build_project(
    entry_path: &str,
    output_dir: &str,
    optimization_level: OptimizationLevel,
    verbose: bool,
    emit_mir: bool,
    progress: bool,
) -> Result<(), String> {
    let entry_path = Path::new(entry_path);
    let output_dir_path = Path::new(output_dir);
    let mut build_product = build_product_for_entry(
        entry_path,
        BuildInput::AutoDetect,
        optimization_level,
        verbose,
        progress,
        true,
    )?;

    let BuildOutputPaths { lir_path, mir_path } =
        build_product.write_outputs(output_dir_path, emit_mir)?;

    println!("输出文件: {}", lir_path.display());
    if let Some(mir) = mir_path {
        println!("Wrote MIR to {}", mir.display());
    }

    Ok(())
}

/// 使用自定义 Pass 管线优化代码
pub fn optimize_with_pipeline(
    input: &str,
    pipeline: &str,
    output: Option<&str>,
    debug: bool,
    verbose: bool,
) -> Result<(), String> {
    use karte_ir_codec::{IrDisplay, IrParse};
    use karte_lir::{LirProgram, OptimizationPipeline};
    use std::fs;

    // 读取输入文件
    let content = fs::read_to_string(input).map_err(|e| format!("读取文件失败: {}", e))?;

    // 解析LIR
    let mut program =
        LirProgram::parse_ir(&content).map_err(|e| format!("解析LIR失败: {:?}", e))?;

    if verbose {
        println!("输入文件: {}", input);
        println!("Pass管线: {}", pipeline);
    }

    // 使用自定义管线优化
    let stats = OptimizationPipeline::optimize_with_custom_pipeline(&mut program, pipeline, debug)
        .map_err(|errors| format!("优化失败: {}", errors.join(", ")))?;

    // 打印统计信息
    if verbose || debug {
        println!("\n=== 优化统计 ===");
        println!("优化前指令数: {}", stats.instructions_before);
        println!("优化后指令数: {}", stats.instructions_after);
        println!(
            "指令减少: {}",
            stats.instructions_before as i64 - stats.instructions_after as i64
        );
        println!("执行的Pass数: {}", stats.passes_executed);
        println!("成功优化Pass数: {}", stats.changed_passes);
        println!("总耗时: {}ms", stats.total_time_ms);
    }

    // 输出结果
    let output_content = program.to_ir_string();

    if let Some(output_path) = output {
        write_content_creating_parent(output_path, &output_content)
            .map_err(|e| format!("写入输出文件失败: {}", e))?;
        println!("\n优化后的LIR已写入: {}", output_path);
    } else {
        println!("\n=== 优化后的LIR ===");
        println!("{}", output_content);
    }

    Ok(())
}
