use clap::{Parser, Subcommand, ValueEnum};
use karte_codegen::lir_interpreter::execute;
use karte_codegen::vm::professional_executor::ProfessionalExecutor;
use karte_diagnostics::DiagnosticBag;
use karte_ir_codec::{IrDisplay, IrParse};
use karte_lexer::tokenize;
use karte_lir::lower::lower_mir_to_lir;
use karte_lir::optimization_pipeline::{OptimizationLevel, OptimizationPipeline};
use karte_lir::LirProgram;
use karte_mir::{lower::lower_expr_to_mir, MirProgram};
use karte_parser::parse_with_type_check;
use log::error;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

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

fn compile_source_to_artifacts(
    input: &str,
    filename: &str,
    optimization_level: OptimizationLevel,
    verbose: bool,
) -> Result<CompilationArtifacts, Box<dyn std::error::Error>> {
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
    let mir_program = match lower_expr_to_mir(&result.expr) {
        Ok(prog) => prog,
        Err(errors) => {
            for err in errors {
                error!("MIR Lowering Error: {}", err);
            }
            return Err("MIR lowering failed".into());
        }
    };
    if verbose {
        println!("{}", mir_program.to_ir_string());
    }

    let lir_program = lower_mir_to_final_lir(&mir_program, optimization_level, verbose)?;

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
    let artifacts = compile_source_to_artifacts(input, filename, optimization_level, verbose)?;
    Ok(artifacts.lir_program)
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

    // 🔧 新增：优先尝试JIT执行，失败则回退到解释器
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
                        if verbose {
                            println!("JIT执行失败，回退到解释器: {}", err);
                        }
                        // 继续到解释器执行
                    }
                }
            }
            Err(err) => {
                if verbose {
                    println!("无法创建JIT执行器，回退到解释器: {}", err);
                }
                // 继续到解释器执行
            }
        }
    }

    // 🔧 保持原有的解释器执行逻辑
    match execute(lir_program) {
        Ok(value) => {
            println!("Result: {}", value);
            Ok(())
        }
        Err(err) => {
            error!("Runtime error: {}", err);
            Err("Execution failed".into())
        }
    }
}

fn process_file(
    filename: &str,
    optimization_level: OptimizationLevel,
    verbose: bool,
    emit_lir: bool,
    output_file: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let content = fs::read_to_string(filename)?;
    let lir_program = compile_to_lir(&content, filename, optimization_level, verbose)?;

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

    Ok(())
}

fn process_expression(
    input: &str,
    optimization_level: OptimizationLevel,
    verbose: bool,
    emit_lir: bool,
    output_file: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let lir_program = compile_to_lir(input, "input", optimization_level, verbose)?;

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

    let artifacts =
        compile_source_to_artifacts(&source_content, filename, optimization_level, verbose)?;

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
                        process_file(filename, optimization_level, verbose, false, None)
                    {
                        error!("Error reading file '{}': {}", filename, err);
                    }
                    continue;
                }

                if let Err(err) =
                    process_expression(input, optimization_level, verbose, false, None)
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

fn main() {
    env_logger::init();
    let cli = Cli::parse();

    let optimization_level = cli.optimization.into();

    match cli.command {
        Some(Commands::Run {
            input,
            emit_lir,
            output,
        }) => match input {
            Some(ref input_str) => {
                if Path::new(input_str).exists() {
                    if let Err(err) = process_file(
                        input_str,
                        optimization_level,
                        cli.verbose,
                        emit_lir,
                        output.as_deref(),
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
            let source = match fs::read_to_string(&input) {
                Ok(content) => content,
                Err(err) => {
                    error!("Failed to read input file '{}': {}", input, err);
                    std::process::exit(1);
                }
            };

            let lir_program = match compile_to_lir(&source, &input, optimization_level, cli.verbose)
            {
                Ok(program) => program,
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
        None => {
            if let Some(ref input) = cli.input {
                if Path::new(input).exists() {
                    if let Err(err) = process_file(
                        input,
                        optimization_level,
                        cli.verbose,
                        cli.emit_lir,
                        cli.output.as_deref(),
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
