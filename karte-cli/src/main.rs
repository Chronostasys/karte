use clap::{Parser, Subcommand, ValueEnum};
use karte_codegen::lir_interpreter::execute;
use karte_diagnostics::DiagnosticBag;
use karte_lexer::tokenize;
use karte_lir::lower::lower_mir_to_lir;
use karte_lir::optimization_pipeline::{OptimizationLevel, OptimizationPipeline};
use karte_lir::LirProgram;
use karte_mir::lower::lower_expr_to_mir;
use karte_parser::parse_with_type_check;
use log::error;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

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
    },

    /// 交互式模式
    Repl,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
enum OptimizationArg {
    Debug,
    Fast,
    Balanced,
    Performance,
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

fn compile_to_lir(
    input: &str,
    filename: &str,
    optimization_level: OptimizationLevel,
    verbose: bool,
) -> Result<LirProgram, Box<dyn std::error::Error>> {
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
        println!("{}", mir_program);
    }

    // Lowering to LIR
    if verbose {
        println!("\n--- Lowering to LIR ---");
        println!("=== 返回高级LIR (包含Alloc指令，待优化) ===");
    }
    let mut lir_program = match lower_mir_to_lir(&mir_program) {
        Ok(prog) => prog,
        Err(errors) => {
            for err in errors {
                error!("LIR Lowering Error: {}", err);
            }
            return Err("LIR lowering failed".into());
        }
    };
    if verbose {
        println!("{}", lir_program);
        println!("================================================");
    }

    // LIR 优化
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
                if stats.instructions_before > 0 {
                    let reduction = (stats.instructions_before - stats.instructions_after) as f64
                        / stats.instructions_before as f64
                        * 100.0;
                    println!("  - 指令减少: {:.1}%", reduction);
                }
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
        println!("{}", lir_program);
    }

    // 指令降级
    if verbose {
        println!("\n--- 指令降级 ---");
    }
    if let Err(lowering_error) = karte_lir::lower_program_instructions(&mut lir_program) {
        error!("指令降级错误: {}", lowering_error);
        return Err("Instruction lowering failed".into());
    }

    if verbose {
        println!("\n--- 降级后LIR (可执行) ---");
        println!("{}", lir_program);
    }

    Ok(lir_program)
}

fn execute_lir(lir_program: &LirProgram, verbose: bool) -> Result<(), Box<dyn std::error::Error>> {
    if verbose {
        println!("\n--- Executing LIR ---");
    }
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
        let lir_code = format!("{}", lir_program);
        fs::write(output_path, lir_code)?;
        println!("LIR代码已输出到: {}", output_path);
    }

    if emit_lir {
        println!("{}", lir_program);
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
        let lir_code = format!("{}", lir_program);
        fs::write(output_path, lir_code)?;
        println!("LIR代码已输出到: {}", output_path);
    }

    if emit_lir {
        println!("{}", lir_program);
    } else {
        execute_lir(&lir_program, verbose)?;
    }

    Ok(())
}

fn load_and_execute_lir(filename: &str, verbose: bool) -> Result<(), Box<dyn std::error::Error>> {
    if verbose {
        println!("Loading LIR from: {}", filename);
    }

    let _content = fs::read_to_string(filename)?;
    // 这里需要实现LIR的解析功能
    // 暂时返回错误，因为LIR解析器还没有实现
    Err("LIR file execution not yet implemented. Please compile from source code instead.".into())
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
    let optimization_level: OptimizationLevel = cli.optimization.into();

    if cli.verbose {
        env::set_var("RUST_LOG", "info");
    }

    match cli.command {
        Some(Commands::Run {
            input,
            emit_lir,
            output,
        }) => {
            let input = input.or(cli.input);
            if let Some(input) = input {
                // 检查是否为文件路径
                if Path::new(&input).exists() && Path::new(&input).is_file() {
                    if let Err(err) = process_file(
                        &input,
                        optimization_level,
                        cli.verbose,
                        emit_lir,
                        output.as_deref(),
                    ) {
                        error!("Error processing file '{}': {}", input, err);
                        std::process::exit(1);
                    }
                } else if let Err(err) = process_expression(
                    &input,
                    optimization_level,
                    cli.verbose,
                    emit_lir,
                    output.as_deref(),
                ) {
                    error!("Error processing expression: {}", err);
                    std::process::exit(1);
                }
            } else {
                error!("No input provided. Use 'karte run <input>' or 'karte repl' for interactive mode.");
                std::process::exit(1);
            }
        }

        Some(Commands::Compile { input, output }) => {
            let output = output.unwrap_or_else(|| {
                let mut path = Path::new(&input).to_path_buf();
                path.set_extension("lir");
                path.to_string_lossy().to_string()
            });

            if let Err(err) =
                process_file(&input, optimization_level, cli.verbose, true, Some(&output))
            {
                error!("Error compiling file '{}': {}", input, err);
                std::process::exit(1);
            }
        }

        Some(Commands::Execute { input }) => {
            if let Err(err) = load_and_execute_lir(&input, cli.verbose) {
                error!("Error executing LIR file '{}': {}", input, err);
                std::process::exit(1);
            }
        }

        Some(Commands::Repl) => {
            run_repl(optimization_level, cli.verbose);
        }

        None => {
            // 兼容旧版本的用法
            if let Some(input) = cli.input {
                // 检查是否为文件路径
                if Path::new(&input).exists() && Path::new(&input).is_file() {
                    if let Err(err) = process_file(
                        &input,
                        optimization_level,
                        cli.verbose,
                        cli.emit_lir,
                        cli.output.as_deref(),
                    ) {
                        error!("Error processing file '{}': {}", input, err);
                        std::process::exit(1);
                    }
                } else if let Err(err) = process_expression(
                    &input,
                    optimization_level,
                    cli.verbose,
                    cli.emit_lir,
                    cli.output.as_deref(),
                ) {
                    error!("Error processing expression: {}", err);
                    std::process::exit(1);
                }
            } else {
                run_repl(optimization_level, cli.verbose);
            }
        }
    }
}
