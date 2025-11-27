use indicatif::{ProgressBar, ProgressStyle, ProgressDrawTarget};
use karte_codegen::vm::professional_executor::ProfessionalExecutor;
use karte_ir_codec::IrDisplay;
use crate::IrStage;
use karte_lir::optimization_pipeline::OptimizationLevel;
use karte_lir::LirProgram;
use karte_module_system::{
    compile_entry_file, compile_module_in_layer, compile_source_to_artifacts, compile_to_lir,
    merge_lir_program, merge_module_artifacts, CompilationArtifacts,
    CompilationCache, LayerCompilationResult, ModuleGraph, ModuleId, ModuleInterfaceArtifact,
};
use karte_parser::ParserMode;
use karte_rt::{ffi, HeapStats};
use log::{error, warn};
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

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



pub fn execute_lir(lir_program: &LirProgram, verbose: bool) -> Result<(), Box<dyn std::error::Error>> {
    if verbose {
        println!("\n--- Executing LIR ---");
    }

    // 始终尝试使用 JIT 执行；解释器已被移除，因此失败会返回错误
    if verbose {
        println!("entry: {}", lir_program.main_function.as_ref().map_or("<none>".to_string(), |f| f.clone()));
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
) -> Result<(), Box<dyn std::error::Error>> {
    let artifacts = compile_entry_file(filename, optimization_level, verbose, mode)?;
    let lir_program = artifacts.lir_program;
    let before_stats = heap_stats.then_some(capture_heap_stats());

    if let Some(output_path) = output_file {
        let lir_code = lir_program.to_ir_string();
        write_content_creating_parent(output_path, &lir_code)?;
        println!("LIR代码已输出到: {}", output_path);
    }

    if emit_lir {
        println!("{}", lir_program.to_ir_string());
    } else {
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
    let lir_program = compile_to_lir(input, "input", optimization_level, verbose, mode)?;
    let before_stats = heap_stats.then_some(capture_heap_stats());

    if let Some(output_path) = output_file {
        let lir_code = lir_program.to_ir_string();
        write_content_creating_parent(output_path, &lir_code)?;
        println!("LIR代码已输出到: {}", output_path);
    }

    if emit_lir {
        println!("{}", lir_program.to_ir_string());
    } else {
        execute_lir(&lir_program, verbose)?;
    }

    if let Some(before) = before_stats {
        let after = capture_heap_stats();
        print_heap_stats("expression", "input", before, after);
    }

    Ok(())
}

pub fn export_ir(
    input: &str,
    stage: IrStage,
    output: Option<&str>,
    optimization_level: OptimizationLevel,
    verbose: bool,
    mode: ParserMode,
) -> Result<(), Box<dyn std::error::Error>> {
    let (source_content, filename_owned, from_file) = if Path::new(input).exists() {
        (fs::read_to_string(input)?, Some(input.to_string()), true)
    } else {
        (input.to_string(), None, false)
    };

    let filename = filename_owned.as_deref().unwrap_or("input");

    let artifacts = if from_file {
        compile_entry_file(filename, optimization_level, verbose, mode)?
    } else {
        compile_source_to_artifacts(
            &source_content,
            filename,
            optimization_level,
            verbose,
            None,
            mode,
            None,
        )?
    };

    let content = match stage {
        IrStage::Mir => artifacts.mir_program.to_ir_string(),
        IrStage::Lir => artifacts.lir_program.to_ir_string(),
    };

    if let Some(custom_path) = output {
        write_content_creating_parent(custom_path, &content)?;
        println!("{} IR已输出到: {}", stage.label(), custom_path);
    } else if from_file {
        let mut default_path = PathBuf::from(filename);
        default_path.set_extension(stage.default_extension());
        write_content_creating_parent(&default_path, &content)?;
        println!("{} IR已输出到: {}", stage.label(), default_path.display());
    } else {
        println!("{}", content);
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
                    let filename = input.strip_prefix("load ")
                        .expect("已检查前缀存在")
                        .trim();
                    if let Err(err) = process_file(
                        filename,
                        optimization_level,
                        verbose,
                        false,
                        None,
                        false,
                        ParserMode::Script,
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
    let output_dir = Path::new(output_dir);
    fs::create_dir_all(output_dir).map_err(|e| format!("Failed to create output dir: {}", e))?;

    let graph = ModuleGraph::load_for_entry(entry_path).map_err(|e| e.to_string())?;
    let plan = graph
        .plan_for_entry(entry_path)
        .map_err(|e| e.to_string())?;

    println!("构建计划: {} 个模块", plan.sequence.len());
    if verbose {
        for module in &plan.sequence {
            println!("  - {}", module);
        }
    }

    let layers = graph.schedule_layers(&plan);
    println!("分层调度: {} 层", layers.len());

    let total_modules: usize = plan.sequence.len();
    let progress_bar = if progress {
        let pb = ProgressBar::new(total_modules as u64);

        // Compute a stable bar width based on terminal size, but clamp it so
        // the bar doesn't become huge. Reserve space for spinner, percent,
        // pos/len and timestamps (approx 40 cols).
        let term_width = get_terminal_width();
        let reserved = 40usize;
        let max_bar = 60usize;
        let min_bar = 20usize;
        let avail = term_width.saturating_sub(reserved);
        let bar_len = std::cmp::min(max_bar, std::cmp::max(min_bar, avail));

        let template = format!(
            "{{spinner:.green}} {{bar:{w}.cyan/blue}} {{percent:>3}}% {{pos}}/{{len}} [{{elapsed}}<{{eta}}] {{msg}}",
            w = bar_len
        );

        let style = ProgressStyle::with_template(&template)
            .expect("进度条模板格式应该有效")
            .progress_chars("#>-")
            .tick_strings(&[
                "⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏",
            ]);

        pb.set_style(style);
        pb.set_draw_target(ProgressDrawTarget::stderr_with_hz(30));
        pb.enable_steady_tick(std::time::Duration::from_millis(120));
        // Keep the message short so it doesn't affect layout stability.
        pb.set_message("编译中");
        Some(Arc::new(pb))
    } else {
        None
    };

    let cache = CompilationCache::new_with_root(output_dir.join("cache"));
    let _cache = Arc::new(Mutex::new(cache));

    let compiled_modules = Arc::new(Mutex::new(HashMap::new()));
    let canonical_entry = fs::canonicalize(entry_path).map_err(|e| format!("canonicalize entry failed: {}", e))?;
    let mut interface_hashes: HashMap<ModuleId, u64> = HashMap::new();
    let mut interface_artifacts: HashMap<ModuleId, ModuleInterfaceArtifact> = HashMap::new();
    let mut compiled_artifacts_by_module: HashMap<ModuleId, Vec<CompilationArtifacts>> = HashMap::new();
    let mut entry_result: Option<CompilationArtifacts> = None;
    let entry_module = plan.entry.clone();

    for (i, layer) in layers.iter().enumerate() {
        if let Some(pb) = &progress_bar {
            pb.println(format!(
                "正在编译第 {}/{} 层 ({} 个模块)...",
                i + 1,
                layers.len(),
                layer.len()
            ));
        } else {
            println!(
                "正在编译第 {}/{} 层 ({} 个模块)...",
                i + 1,
                layers.len(),
                layer.len()
            );
        }

        let dependency_snapshot = interface_hashes.clone();
        let artifact_snapshot = interface_artifacts.clone();

        let results: Vec<Result<LayerCompilationResult, String>> = layer
            .par_iter()
            .map(|module_id| {
                compile_module_in_layer(
                    &graph,
                    module_id,
                    &dependency_snapshot,
                    &artifact_snapshot,
                    optimization_level,
                    verbose,
                    progress_bar.clone(),
                    ParserMode::Project,
                    &entry_module,
                    &canonical_entry,
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

    println!("构建完成！");

    let mut final_program = LirProgram::new();
    let modules = compiled_modules.lock()
        .expect("锁不应该被污染");

    for module_id in &plan.sequence {
        if let Some(prog) = modules.get(module_id) {
            merge_lir_program(&mut final_program, prog);
            if module_id == &plan.entry {
                final_program.main_function = prog.main_function.clone();
            }
        }
    }

    if final_program.main_function.is_none() {
        if let Some(entry_artifacts) = &entry_result {
            final_program.main_function = entry_artifacts.lir_program.main_function.clone();
        }
    }

    let output_file = output_dir.join("main.lir");
    let lir_code = final_program.to_ir_string();
    fs::write(&output_file, lir_code).map_err(|e| format!("Failed to write output: {}", e))?;

    println!("输出文件: {}", output_file.display());

    if emit_mir {
        let mut entry_artifacts = entry_result.ok_or_else(|| "入口模块未被编译，无法生成合并的 MIR".to_string())?;
        merge_module_artifacts(&plan, &compiled_artifacts_by_module, &mut entry_artifacts);
        let mir_out_dir = Path::new(&output_dir).join("mir");
        if let Err(e) = fs::create_dir_all(&mir_out_dir) {
            warn!(
                "Failed to create mir output dir {}: {}",
                mir_out_dir.display(),
                e
            );
        } else {
            let out = mir_out_dir.join("main.mir");
            if let Err(e) = fs::write(&out, entry_artifacts.mir_program.to_ir_string()) {
                warn!("Failed to write merged MIR file {}: {}", out.display(), e);
            } else {
                println!("Wrote MIR to {}", out.display());
            }
        }
    }

    Ok(())
}
