use clap::{Parser, Subcommand, ValueEnum};
use karte_codegen::lir_interpreter::execute_professional;
use karte_codegen::vm::professional_executor::ProfessionalExecutor;
use karte_diagnostics::DiagnosticBag;
use karte_ir_codec::{IrDisplay, IrParse};
use karte_lexer::tokenize;
use karte_lir::lower::lower_mir_to_lir;
use karte_lir::optimization_pipeline::{OptimizationLevel, OptimizationPipeline};
use karte_lir::LirProgram;
use karte_mir::{
    lower::{lower_expr_to_mir, SCRIPT_ENTRY_POINT},
    MirProgram, Statement, Value,
};
use karte_parser::parse_with_type_check;
use karte_rt::{ffi, HeapStats};
use log::error;
use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeSet, HashMap};
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

mod cache;
use cache::CompilationCache;
mod modules;
use modules::{ModuleGraph, ModuleId, ModuleMetadata};

#[derive(Parser)]
#[command(name = "karte")]
#[command(about = "Karte 编程语言编译器")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// 优化级别
    #[arg(short, long, value_enum, default_value_t = OptimizationArg::Balanced)]
    optimization: OptimizationArg,

    /// 显示详细的编译过程
    #[arg(short, long)]
    verbose: bool,

    /// 只输出LIR代码，不执行
    #[arg(long)]
    emit_lir: bool,

    /// 输出LIR到指定文件
    #[arg(long)]
    output: Option<String>,

    /// 在执行后输出 heap/RC 统计信息
    #[arg(long)]
    heap_stats: bool,

    /// 输入文件或表达式
    input: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// 编译并运行Karte代码
    Run {
        /// 输入文件或表达式
        input: Option<String>,

        /// 只输出LIR代码，不执行
        #[arg(long)]
        emit_lir: bool,

        /// 输出LIR到指定文件
        #[arg(long)]
        output: Option<String>,

        /// 在执行后输出 heap/RC 统计信息
        #[arg(long)]
        heap_stats: bool,
    },

    /// 编译Karte代码到LIR
    Compile {
        /// 输入文件
        input: String,

        /// 输出文件
        #[arg(short, long)]
        output: Option<String>,
    },

    /// 运行LIR文件
    Execute {
        /// LIR文件路径
        input: String,
        /// IR阶段 (默认 LIR)
        #[arg(long, value_enum, default_value_t = IrStage::Lir)]
        stage: IrStage,
    },

    /// 交互式模式
    Repl,

    /// 导出IR到文件或标准输出
    Export {
        /// 输入文件或表达式
        input: String,

        /// 导出的IR阶段
        #[arg(long, value_enum, default_value_t = IrStage::Lir)]
        stage: IrStage,

        /// 输出文件
        #[arg(short, long)]
        output: Option<String>,
    },

    /// 构建项目（支持增量编译与并发调度）
    Build {
        /// 入口文件
        input: String,

        /// 输出目录
        #[arg(short, long, default_value = "target")]
        output_dir: String,

        /// 并发编译任务数 (默认: CPU核心数)
        #[arg(short = 'j', long)]
        jobs: Option<usize>,
    },
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
enum OptimizationArg {
    Debug,
    Fast,
    Balanced,
    Performance,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum IrStage {
    Mir,
    Lir,
}

impl IrStage {
    fn label(self) -> &'static str {
        match self {
            IrStage::Mir => "MIR",
            IrStage::Lir => "LIR",
        }
    }

    fn default_extension(self) -> &'static str {
        match self {
            IrStage::Mir => "mir",
            IrStage::Lir => "lir",
        }
    }
}

struct CompilationArtifacts {
    mir_program: MirProgram,
    lir_program: LirProgram,
}

struct CacheContext<'a> {
    module_id: &'a str,
    interface_hash: u64,
}

impl From<OptimizationArg> for OptimizationLevel {
    fn from(opt: OptimizationArg) -> Self {
        match opt {
            OptimizationArg::Debug => OptimizationLevel::Debug,
            OptimizationArg::Fast => OptimizationLevel::Fast,
            OptimizationArg::Balanced => OptimizationLevel::Balanced,
            OptimizationArg::Performance => OptimizationLevel::Performance,
        }
    }
}

fn print_diagnostics(diagnostics: &DiagnosticBag, source_code: &str, filename: &str) {
    diagnostics.print_fancy(source_code, filename).unwrap();
}

fn compile_source_to_artifacts<'a>(
    input: &str,
    filename: &str,
    optimization_level: OptimizationLevel,
    verbose: bool,
    cache_ctx: Option<CacheContext<'a>>,
) -> Result<CompilationArtifacts, Box<dyn std::error::Error>> {
    let cache = CompilationCache::new();
    let mut cache_key = None;
    if cache.is_enabled() && cache.eligible_filename(filename) {
        let content_fingerprint = fingerprint_content(input);
        let module_id = cache_ctx.as_ref().map(|ctx| ctx.module_id);
        let interface_hash = cache_ctx
            .as_ref()
            .map(|ctx| ctx.interface_hash)
            .unwrap_or(content_fingerprint);
        let key = cache.make_key(
            filename,
            content_fingerprint,
            interface_hash,
            module_id,
            optimization_level,
        );
        if let Some((mir_program, lir_program)) = cache.load(&key) {
            if verbose {
                println!("使用增量缓存: {}", filename);
            }
            return Ok(CompilationArtifacts {
                mir_program,
                lir_program,
            });
        }
        cache_key = Some(key);
    }

    if verbose {
        if filename != "input" {
            println!("Processing file: {}", filename);
        } else {
            println!("Input: {}", input);
        }
    }

    // 词法分析
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

    // 语法分析和类型检查
    let (result, parse_diagnostics) = parse_with_type_check(&tokens);

    if !parse_diagnostics.is_empty() {
        print_diagnostics(&parse_diagnostics, input, filename);
        if parse_diagnostics.has_errors() {
            return Err("Parsing or type checking failed".into());
        }
    }

    let result = result.ok_or("Failed to parse expression or type check failed")?;

    if verbose && filename == "input" {
        println!("AST: {}", result.expr);
    }
    if verbose {
        println!("Type: {}", result.result_type);
    }

    // Lowering to MIR
    if verbose {
        println!("\n--- Lowering to MIR ---");
    }
    let mut mir_program = match lower_expr_to_mir(&result.expr) {
        Ok(prog) => prog,
        Err(errors) => {
            for err in errors {
                error!("MIR Lowering Error: {}", err);
            }
            return Err("MIR lowering failed".into());
        }
    };

    // 检查是否为 Project Mode (脚本入口为空，且存在 main 函数)
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
                if verbose {
                    println!("Project Mode detected: switching entry point to 'main'");
                }
                mir_program.set_main("main".to_string());
            }
        }
    }

    if verbose {
        println!("{}", mir_program.to_ir_string());
    }

    let lir_program = lower_mir_to_final_lir(&mir_program, optimization_level, verbose)?;

    if let Some(key) = cache_key {
        cache.store(&key, &mir_program, &lir_program);
    }

    Ok(CompilationArtifacts {
        mir_program,
        lir_program,
    })
}

fn compile_to_lir(
    input: &str,
    filename: &str,
    optimization_level: OptimizationLevel,
    verbose: bool,
) -> Result<LirProgram, Box<dyn std::error::Error>> {
    let artifacts =
        compile_source_to_artifacts(input, filename, optimization_level, verbose, None)?;
    Ok(artifacts.lir_program)
}

fn compile_entry_file(
    filename: &str,
    optimization_level: OptimizationLevel,
    verbose: bool,
) -> Result<CompilationArtifacts, Box<dyn std::error::Error>> {
    let entry_path = Path::new(filename);
    let module_graph = ModuleGraph::load_for_entry(entry_path).map_err(|err| format!("{}", err))?;
    let plan = module_graph
        .plan_for_entry(entry_path)
        .map_err(|err| format!("{}", err))?;
    let canonical_entry = fs::canonicalize(entry_path)?;
    let mut interface_hashes: HashMap<ModuleId, u64> = HashMap::new();

    if verbose {
        if let Some(manifest) = module_graph.manifest_path() {
            println!("[module] manifest: {}", manifest.display());
        }
        for description in module_graph.describe() {
            println!("[module] {}", description);
        }
        println!("[module] topo count = {}", plan.sequence.len());
    }

    let mut entry_result = None;
    for module_id in plan.sequence {
        let meta = module_graph
            .metadata(&module_id)
            .ok_or_else(|| format!("缺少模块元数据: {}", module_id))?;

        let cache_version = compute_module_cache_version(&module_id, meta, &interface_hashes);
        let mut interface_acc = ModuleInterfaceAccumulator::default();

        for source_path in &meta.sources {
            let source = fs::read_to_string(source_path)?;
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
            )?;
            interface_acc.observe(&artifacts.mir_program);
            if module_id == plan.entry && *source_path == canonical_entry {
                entry_result = Some(artifacts);
            }
        }

        let public_interface_hash =
            interface_acc.finish(&module_id, &meta.dependencies, &interface_hashes);
        interface_hashes.insert(module_id.clone(), public_interface_hash);
    }

    entry_result.ok_or_else(|| "入口模块未被编译".into())
}

fn lower_mir_to_final_lir(
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

    if verbose {
        println!("\n--- 指令降级 ---");
    }
    if let Err(lowering_error) = karte_lir::lower_program_instructions(&mut lir_program) {
        error!("指令降级错误: {}", lowering_error);
        return Err("Instruction lowering failed".into());
    }

    if verbose {
        println!("\n--- 降级后LIR (可执行) ---");
        println!("{}", lir_program.to_ir_string());
    }

    Ok(lir_program)
}

fn parse_ir_content<T: IrParse>(
    content: &str,
    stage_label: &str,
) -> Result<T, Box<dyn std::error::Error>> {
    T::parse_ir(content).map_err(|err| format!("解析{} IR失败: {}", stage_label, err).into())
}

fn load_ir_for_execution(
    filename: &str,
    stage: IrStage,
    optimization_level: OptimizationLevel,
    verbose: bool,
) -> Result<LirProgram, Box<dyn std::error::Error>> {
    if verbose {
        println!("Loading {} from: {}", stage.label(), filename);
    }

    let content = fs::read_to_string(filename)?;

    match stage {
        IrStage::Lir => {
            if verbose {
                println!("解析 LIR...");
            }
            parse_ir_content::<LirProgram>(&content, "LIR")
        }
        IrStage::Mir => {
            if verbose {
                println!("解析 MIR...");
            }
            let mir_program = parse_ir_content::<MirProgram>(&content, "MIR")?;
            if verbose {
                println!("MIR 解析完成");
                println!("{}", mir_program.to_ir_string());
            }
            lower_mir_to_final_lir(&mir_program, optimization_level, verbose)
        }
    }
}

fn load_and_execute_ir(
    filename: &str,
    stage: IrStage,
    optimization_level: OptimizationLevel,
    verbose: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let lir_program = load_ir_for_execution(filename, stage, optimization_level, verbose)?;
    execute_lir(&lir_program, verbose)
}

fn fingerprint_content(content: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

fn compute_module_cache_version(
    module_id: &ModuleId,
    meta: &ModuleMetadata,
    dependency_interfaces: &HashMap<ModuleId, u64>,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    module_id.as_str().hash(&mut hasher);
    meta.source_fingerprint.hash(&mut hasher);
    let mut deps = meta.dependencies.clone();
    deps.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    for dep in deps {
        if let Some(dep_hash) = dependency_interfaces.get(&dep) {
            dep_hash.hash(&mut hasher);
        }
    }
    hasher.finish()
}

#[derive(Default)]
struct ModuleInterfaceAccumulator {
    functions: BTreeSet<String>,
    structs: BTreeSet<String>,
    mains: BTreeSet<String>,
}

impl ModuleInterfaceAccumulator {
    fn observe(&mut self, program: &MirProgram) {
        for (name, function) in &program.functions {
            let sig = format!("fn {}({})", name, function.params.len());
            self.functions.insert(sig);
        }
        for (name, ty) in &program.struct_types {
            let mut fields = ty
                .fields
                .iter()
                .map(|field| format!("{}:{}", field.name, field.field_type))
                .collect::<Vec<_>>();
            fields.sort();
            let sig = format!("struct {}{{{}}}", name, fields.join(";"));
            self.structs.insert(sig);
        }
        if let Some(main) = &program.main_function {
            self.mains.insert(main.clone());
        }
    }

    fn finish(
        self,
        module_id: &ModuleId,
        dependencies: &[ModuleId],
        dependency_interfaces: &HashMap<ModuleId, u64>,
    ) -> u64 {
        let mut hasher = DefaultHasher::new();
        module_id.as_str().hash(&mut hasher);
        for sig in self.functions {
            sig.hash(&mut hasher);
        }
        for sig in self.structs {
            sig.hash(&mut hasher);
        }
        for main in self.mains {
            main.hash(&mut hasher);
        }
        let mut deps = dependencies.to_vec();
        deps.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        for dep in deps {
            if let Some(dep_hash) = dependency_interfaces.get(&dep) {
                dep_hash.hash(&mut hasher);
            }
        }
        hasher.finish()
    }
}

/// 🔧 新增：获取环境变量中的JIT设置
fn should_use_jit() -> bool {
    env::var("KARTE_JIT")
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(true)
}

/// 🔧 修改：改进的execute_lir函数，支持JIT
fn execute_lir(lir_program: &LirProgram, verbose: bool) -> Result<(), Box<dyn std::error::Error>> {
    if verbose {
        println!("\n--- Executing LIR ---");
    }

    // 🔧 新增：检查是否应该使用JIT
    let use_jit = should_use_jit();

    if use_jit && verbose {
        println!("尝试使用JIT执行器...");
    }

    if use_jit {
        match ProfessionalExecutor::new_with_jit(verbose) {
            Ok(mut executor) => {
                if verbose {
                    println!("使用JIT执行器");
                }
                match executor.execute_with_jit(lir_program) {
                    Ok(exit_code) => {
                        println!("JIT执行完成，退出码: {}", exit_code);
                        return Ok(());
                    }
                    Err(err) => {
                        // 🔧 修复：始终打印错误信息以帮助调试
                        eprintln!("JIT执行失败: {}", err);
                        eprintln!("回退到解释器执行");
                        // 继续到解释器执行
                    }
                }
            }
            Err(err) => {
                // 🔧 修复：始终打印错误信息以帮助调试
                eprintln!("无法创建JIT执行器: {}", err);
                eprintln!("回退到解释器执行");
                // 继续到解释器执行
            }
        }
    }

    // 🔧 修复：实现解释器回退
    if verbose {
        println!("使用解释器执行LIR程序");
    }
    
    match execute_professional(lir_program, verbose) {
        Ok(exit_code) => {
            if verbose {
                println!("解释器执行完成，返回值: {}", exit_code);
            }
            Ok(())
        }
        Err(err) => {
            eprintln!("解释器执行失败: {}", err);
            Err(err.into())
        }
    }
}

fn capture_heap_stats() -> HeapStats {
    let mut stats = HeapStats::default();
    ffi::karte_jit_runtime_heap_stats(&mut stats as *mut _);
    stats
}

fn print_heap_stats(context: &str, target: &str, before: HeapStats, after: HeapStats) {
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

fn process_file(
    filename: &str,
    optimization_level: OptimizationLevel,
    verbose: bool,
    emit_lir: bool,
    output_file: Option<&str>,
    heap_stats: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let artifacts = compile_entry_file(filename, optimization_level, verbose)?;
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

fn process_expression(
    input: &str,
    optimization_level: OptimizationLevel,
    verbose: bool,
    emit_lir: bool,
    output_file: Option<&str>,
    heap_stats: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let lir_program = compile_to_lir(input, "input", optimization_level, verbose)?;
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

fn export_ir(
    input: &str,
    stage: IrStage,
    output: Option<&str>,
    optimization_level: OptimizationLevel,
    verbose: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let (source_content, filename_owned, from_file) = if Path::new(input).exists() {
        (fs::read_to_string(input)?, Some(input.to_string()), true)
    } else {
        (input.to_string(), None, false)
    };

    let filename = filename_owned.as_deref().unwrap_or("input");

    let artifacts = if from_file {
        compile_entry_file(filename, optimization_level, verbose)?
    } else {
        compile_source_to_artifacts(&source_content, filename, optimization_level, verbose, None)?
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

fn write_content_creating_parent<P: AsRef<Path>>(path: P, content: &str) -> io::Result<()> {
    let path_ref = path.as_ref();
    if let Some(parent) = path_ref.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(path_ref, content)
}

fn run_repl(optimization_level: OptimizationLevel, verbose: bool) {
    println!("Karte 编程语言解释器");
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
        io::stdout().flush().unwrap();

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

                // 在交互模式中也支持文件加载
                if input.starts_with("load ") {
                    let filename = input.strip_prefix("load ").unwrap().trim();
                    if let Err(err) =
                        process_file(filename, optimization_level, verbose, false, None, false)
                    {
                        error!("Error reading file '{}': {}", filename, err);
                    }
                    continue;
                }

                if let Err(err) =
                    process_expression(input, optimization_level, verbose, false, None, false)
                {
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

use rayon::prelude::*;
use std::sync::{Arc, Mutex};

fn build_project(
    entry_path: &str,
    output_dir: &str,
    optimization_level: OptimizationLevel,
    verbose: bool,
) -> Result<(), String> {
    let entry_path = Path::new(entry_path);
    let output_dir = Path::new(output_dir);
    fs::create_dir_all(output_dir).map_err(|e| format!("Failed to create output dir: {}", e))?;

    // 1. 加载模块图
    let graph = ModuleGraph::load_for_entry(entry_path).map_err(|e| e.to_string())?;
    let plan = graph.plan_for_entry(entry_path).map_err(|e| e.to_string())?;

    println!("构建计划: {} 个模块", plan.sequence.len());
    if verbose {
        for module in &plan.sequence {
            println!("  - {}", module);
        }
    }

    // 2. 计算分层调度
    let layers = graph.schedule_layers(&plan);
    println!("分层调度: {} 层", layers.len());

    // 3. 缓存管理
    let cache = CompilationCache::new_with_root(output_dir.join("cache"));
    let cache = Arc::new(Mutex::new(cache));

    // 4. 并发编译
    // 共享的编译结果（LIR程序片段），用于链接
    // 注意：这里简化处理，实际上可能需要更复杂的链接逻辑
    // 目前假设每个模块编译为独立的LIR片段，最后合并
    let compiled_modules = Arc::new(Mutex::new(HashMap::new()));

    for (i, layer) in layers.iter().enumerate() {
        println!("正在编译第 {}/{} 层 ({} 个模块)...", i + 1, layers.len(), layer.len());
        
        // 并行处理当前层的所有模块
        let results: Vec<Result<(ModuleId, LirProgram), String>> = layer
            .par_iter()
            .map(|module_id| {
                let metadata = graph.metadata(module_id).unwrap();
                let cache_key = cache::CacheKey::new(module_id.as_str(), metadata.source_fingerprint);
                
                // 检查缓存
                let cache_lock = cache.lock().unwrap();
                if let Some(_cached_lir) = cache_lock.get(&cache_key) {
                    if verbose {
                        println!("  [Cache] {}", module_id);
                    }
                    // 反序列化缓存的LIR (这里简化为重新解析文本格式，实际应使用二进制格式)
                    // 暂时不支持从缓存恢复完整LIR对象，所以这里只是模拟命中
                    // 实际实现需要 LirProgram 支持 serde 或者自定义二进制编解码
                    // 这里为了演示，如果命中缓存，我们仍然重新编译（因为还没实现LIR反序列化）
                    // TODO: 实现 LirProgram 的序列化/反序列化
                }
                drop(cache_lock);

                if verbose {
                    println!("  [Compiling] {}", module_id);
                }

                // 读取源码
                let mut combined_source = String::new();
                for src_path in &metadata.sources {
                    let content = fs::read_to_string(src_path)
                        .map_err(|e| format!("Failed to read {}: {}", src_path.display(), e))?;
                    combined_source.push_str(&content);
                    combined_source.push('\n');
                }

                // 编译单个模块
                // 注意：这里简化了依赖处理。实际上编译一个模块可能需要其依赖的符号表（HIR/MIR阶段）
                // 目前 Karte 还是单文件编译模型，这里假设模块间通过 extern 引用，或者源码合并
                // 为了支持真正的模块化，需要在 HIR/MIR 阶段引入符号表导入机制
                // 这里暂时只做源码层面的编译，不解决跨模块符号解析（假设是独立的或者通过运行时链接）
                
                let (tokens, _) = tokenize(&combined_source);
                let (ast_result, diagnostics) = parse_with_type_check(&tokens);
                
                if diagnostics.has_errors() {
                    return Err(format!("Parse failed for {}: {:?}", module_id, diagnostics));
                }
                
                let ast = ast_result.ok_or_else(|| format!("Parse failed for {}", module_id))?;
                
                let mir_program = lower_expr_to_mir(&ast.expr)
                    .map_err(|errs| format!("MIR lowering failed for {}: {:?}", module_id, errs))?;
                
                let mut lir_program = lower_mir_to_lir(&mir_program)
                    .map_err(|errs| format!("LIR lowering failed for {}: {:?}", module_id, errs))?;

                // 优化
                let mut pipeline = OptimizationPipeline::new(optimization_level);
                pipeline
                    .optimize(&mut lir_program)
                    .expect("Optimization failed");

                // 更新缓存
                let cache_lock = cache.lock().unwrap();
                cache_lock.put(cache_key, lir_program.to_ir_string()); // 存入文本IR作为缓存
                drop(cache_lock);

                Ok((module_id.clone(), lir_program))
            })
            .collect();

        // 收集结果并处理错误
        for result in results {
            match result {
                Ok((id, program)) => {
                    compiled_modules.lock().unwrap().insert(id, program);
                }
                Err(e) => return Err(e),
            }
        }
    }

    println!("构建完成！");
    
    // 5. 链接（合并所有LIR程序）
    // 简单地将所有函数合并到一个 LirProgram 中
    let mut final_program = LirProgram::new();
    let modules = compiled_modules.lock().unwrap();
    
    // 按照 plan 顺序合并，保证确定性
    for module_id in &plan.sequence {
        if let Some(prog) = modules.get(module_id) {
            for (name, func) in &prog.functions {
                // 简单的重名处理：如果不是入口模块，可能需要加前缀
                // 这里假设函数名全局唯一
                final_program.functions.insert(name.clone(), func.clone());
            }
            // 合并全局变量和结构体定义
            for (name, layout) in &prog.global_struct_types {
                final_program.add_global_struct_type(name.clone(), layout.clone());
            }
        }
    }

    // 输出最终产物
    let output_file = output_dir.join("main.lir");
    let lir_code = final_program.to_ir_string();
    fs::write(&output_file, lir_code).map_err(|e| format!("Failed to write output: {}", e))?;
    
    println!("输出文件: {}", output_file.display());

    Ok(())
}

fn main() {
    env_logger::init();
    let cli = Cli::parse();

    let optimization_level = cli.optimization.into();

    match cli.command {
        Some(Commands::Run {
            input,
            emit_lir,
            output,
            heap_stats,
        }) => match input {
            Some(ref input_str) => {
                if Path::new(input_str).exists() {
                    if let Err(err) = process_file(
                        input_str,
                        optimization_level,
                        cli.verbose,
                        emit_lir,
                        output.as_deref(),
                        heap_stats,
                    ) {
                        error!("Error: {}", err);
                        std::process::exit(1);
                    }
                } else if let Err(err) = process_expression(
                    input_str,
                    optimization_level,
                    cli.verbose,
                    emit_lir,
                    output.as_deref(),
                    heap_stats,
                ) {
                    error!("Error: {}", err);
                    std::process::exit(1);
                }
            }
            None => {
                run_repl(optimization_level, cli.verbose);
            }
        },
        Some(Commands::Compile { input, output }) => {
            let lir_program = match compile_entry_file(&input, optimization_level, cli.verbose) {
                Ok(artifacts) => artifacts.lir_program,
                Err(err) => {
                    error!("Compilation failed: {}", err);
                    std::process::exit(1);
                }
            };

            let output_file = output.unwrap_or_else(|| {
                Path::new(&input)
                    .with_extension("lir")
                    .to_string_lossy()
                    .to_string()
            });

            let lir_code = lir_program.to_ir_string();
            if let Err(err) = fs::write(&output_file, lir_code) {
                error!("Failed to write output: {}", err);
                std::process::exit(1);
            }

            println!("Compiled to: {}", output_file);
        }
        Some(Commands::Execute { input, stage }) => {
            if let Err(err) = load_and_execute_ir(&input, stage, optimization_level, cli.verbose) {
                error!("Error: {}", err);
                std::process::exit(1);
            }
        }
        Some(Commands::Export {
            input,
            stage,
            output,
        }) => {
            if let Err(err) = export_ir(
                &input,
                stage,
                output.as_deref(),
                optimization_level,
                cli.verbose,
            ) {
                error!("Error: {}", err);
                std::process::exit(1);
            }
        }
        Some(Commands::Repl) => {
            run_repl(optimization_level, cli.verbose);
        }
        Some(Commands::Build {
            input,
            output_dir,
            jobs,
        }) => {
            if let Some(j) = jobs {
                rayon::ThreadPoolBuilder::new()
                    .num_threads(j)
                    .build_global()
                    .unwrap();
            }

            if let Err(err) = build_project(&input, &output_dir, optimization_level, cli.verbose) {
                error!("Build failed: {}", err);
                std::process::exit(1);
            }
        }
        None => {
            if let Some(ref input) = cli.input {
                if Path::new(input).exists() {
                    if let Err(err) = process_file(
                        input,
                        optimization_level,
                        cli.verbose,
                        cli.emit_lir,
                        cli.output.as_deref(),
                        cli.heap_stats,
                    ) {
                        error!("Error: {}", err);
                        std::process::exit(1);
                    }
                } else if let Err(err) = process_expression(
                    input,
                    optimization_level,
                    cli.verbose,
                    cli.emit_lir,
                    cli.output.as_deref(),
                    cli.heap_stats,
                ) {
                    error!("Error: {}", err);
                    std::process::exit(1);
                }
            } else {
                run_repl(optimization_level, cli.verbose);
            }
        }
    }
}
