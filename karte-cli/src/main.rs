use clap::{Parser, Subcommand, ValueEnum};
// runtime/execution and progress utilities moved to `runner` module
use karte_ir_codec::IrParse;
use karte_lir::optimization_pipeline::OptimizationLevel;
#[cfg(test)]
use karte_lir::LirFunction;
use karte_lir::LirProgram;
use karte_mir::MirProgram;
use karte_module_system::lower_mir_to_unoptimized_lir;
use karte_parser::ParserMode;
use log::error;
use std::fs;
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

    /// 显示详细的编译过程 (-v 基本信息/-vv 详细/-vvv 全部)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    /// 解析模式 (script/project)
    #[arg(long, value_enum)]
    mode: Option<ModeArg>,

    /// 只输出LIR代码，不执行
    #[arg(long)]
    emit_lir: bool,

    /// 输出LIR到指定文件
    #[arg(long)]
    output: Option<String>,

    /// 在执行后输出 heap/RC 统计信息
    #[arg(long)]
    heap_stats: bool,

    /// 输出 JIT 生成的机器码反汇编
    #[arg(long)]
    emit_asm: bool,

    /// 输入文件或表达式
    input: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// 编译并运行Karte代码
    Run {
        /// 输入文件或表达式
        input: Option<String>,

        /// 解析模式 (script/project)
        #[arg(long, value_enum)]
        mode: Option<ModeArg>,

        /// 只输出LIR代码，不执行
        #[arg(long)]
        emit_lir: bool,

        /// 输出LIR到指定文件
        #[arg(long)]
        output: Option<String>,

        /// 在执行后输出 heap/RC 统计信息
        #[arg(long)]
        heap_stats: bool,

        /// 输出 JIT 生成的机器码反汇编
        #[arg(long)]
        emit_asm: bool,
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
        /// 输出中间 MIR 文件到 output_dir/mir
        #[arg(long)]
        emit_mir: bool,
        /// 安静模式，不显示进度条
        #[arg(long)]
        quiet: bool,
    },

    /// 列出所有可用的优化 Pass
    ListPasses,

    /// 使用自定义 Pass 管线优化代码
    Optimize {
        /// 输入文件
        input: String,

        /// Pass 管线 (逗号分隔)，例如: "cfg,def-use,dce,print-ir"
        #[arg(short, long)]
        pipeline: String,

        /// 输出文件
        #[arg(short, long)]
        output: Option<String>,

        /// 启用调试模式（打印管线信息）
        #[arg(long)]
        debug: bool,
    },

    /// AOT 编译：生成独立可执行文件（不依赖 glibc）
    Aot {
        /// 输入文件
        input: String,

        /// 输出文件名
        #[arg(short, long)]
        output: Option<String>,

        /// 解析模式 (script/project)
        #[arg(long, value_enum)]
        mode: Option<ModeArg>,

        /// 目标架构 (x86_64/riscv64)
        #[arg(long, default_value = "x86_64")]
        target: String,
    },
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
enum ModeArg {
    Script,
    Project,
}

impl From<ModeArg> for ParserMode {
    fn from(arg: ModeArg) -> Self {
        match arg {
            ModeArg::Script => ParserMode::Script,
            ModeArg::Project => ParserMode::Project,
        }
    }
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
    verbose: u8,
) -> Result<LirProgram, Box<dyn std::error::Error>> {
    if verbose > 0 {
        println!("Loading {} from: {}", stage.label(), filename);
    }

    let content = fs::read_to_string(filename)?;

    let mut lir_program = match stage {
        IrStage::Lir => {
            if verbose > 0 {
                println!("解析 LIR...");
            }
            parse_ir_content::<LirProgram>(&content, "LIR")?
        }
        IrStage::Mir => {
            if verbose > 0 {
                println!("解析 MIR...");
            }
            let mir_program = parse_ir_content::<MirProgram>(&content, "MIR")?;
            if verbose > 0 {
                println!("MIR 解析完成，降级到未优化LIR");
            }
            lower_mir_to_unoptimized_lir(&mir_program, verbose > 0)?
        }
    };

    // 智能优化：检测是否需要优化
    if lir_program.contains_virtual_registers() {
        if verbose > 0 {
            println!("检测到虚拟寄存器，应用优化管道...");
        }
        let mut pipeline = karte_lir::OptimizationPipeline::new(optimization_level);
        pipeline
            .optimize(&mut lir_program)
            .map_err(|errors| -> Box<dyn std::error::Error> {
                format!("LIR优化失败: {}", errors.join(", ")).into()
            })?;
        if verbose > 0 {
            println!("优化完成");
        }
    } else {
        if verbose > 0 {
            println!("LIR已优化（仅包含物理寄存器），直接执行");
        }
    }

    Ok(lir_program)
}

fn load_and_execute_ir(
    filename: &str,
    stage: IrStage,
    optimization_level: OptimizationLevel,
    verbose: u8,
) -> Result<(), Box<dyn std::error::Error>> {
    let lir_program = load_ir_for_execution(filename, stage, optimization_level, verbose)?;
    runner::execute_lir(&lir_program, verbose, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use karte_module_system::{compile_entry_file, merge_lir_program};
    use std::path::PathBuf;

    #[test]
    fn compile_entry_merges_dependency_functions() {
        let mut entry_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        entry_path.pop();
        entry_path.push("test_project");
        entry_path.push("src");
        entry_path.push("main.karte");

        let artifacts = compile_entry_file(
            entry_path
                .to_str()
                .expect("non-UTF8 path to test project entry"),
            OptimizationLevel::Balanced,
            false,
            ParserMode::Project,
        )
        .expect("project compilation should succeed");

        assert!(
            artifacts
                .mir_program
                .function_symbols
                .values()
                .any(|symbol| symbol == "utils::add"),
            "merged MIR should record canonical dependency functions"
        );
        assert!(
            artifacts.lir_program.functions.contains_key("utils::add"),
            "merged LIR should contain dependency functions"
        );
        assert_eq!(
            artifacts.lir_program.main_function.as_deref(),
            Some("main::main"),
            "entry LIR should retain canonical main function"
        );
    }

    #[test]
    fn merge_lir_program_preserves_main_function() {
        let mut target = LirProgram::new();
        let mut source = LirProgram::new();
        let main_name = "foo.bar::main".to_string();
        let mut function = LirFunction::new(main_name.clone());
        function.parameter_count = 0;
        source.add_function(function);
        source.set_main(main_name.clone());

        merge_lir_program(&mut target, &source);

        assert_eq!(target.main_function.as_deref(), Some(main_name.as_str()));
    }
}

/// 🔧 新增：获取环境变量中的JIT设置
// The runner module holds the non-CLI runtime/processing logic.
mod runner;

fn main() {
    env_logger::init();
    let cli = Cli::parse();

    let optimization_level = cli.optimization.into();

    let default_mode = cli.mode.map(|m| m.into()).unwrap_or(ParserMode::Script);
    let default_mode_is_explicit = cli.mode.is_some();

    match cli.command {
        Some(Commands::Run {
            input,
            emit_lir,
            output,
            heap_stats,
            mode,
            emit_asm,
        }) => match input {
            Some(ref input_str) => {
                let (mode, mode_is_explicit) = if let Some(m) = mode {
                    (m.into(), true)
                } else if default_mode_is_explicit {
                    (default_mode, true)
                } else {
                    (default_mode, false)
                };
                if Path::new(input_str).exists() {
                    if let Err(err) = runner::process_file(
                        input_str,
                        optimization_level,
                        cli.verbose,
                        emit_lir,
                        output.as_deref(),
                        heap_stats,
                        mode,
                        mode_is_explicit,
                        emit_asm,
                    ) {
                        error!("Error: {}", err);
                        std::process::exit(1);
                    }
                } else if let Err(err) = runner::process_expression(
                    input_str,
                    optimization_level,
                    cli.verbose,
                    emit_lir,
                    output.as_deref(),
                    heap_stats,
                    mode,
                    emit_asm,
                ) {
                    error!("Error: {}", err);
                    std::process::exit(1);
                }
            }
            None => {
                runner::run_repl(optimization_level, cli.verbose);
            }
        },
        Some(Commands::Execute { input, stage }) => {
            if let Err(err) = load_and_execute_ir(&input, stage, optimization_level, cli.verbose) {
                error!("Error: {}", err);
                std::process::exit(1);
            }
        }
        Some(Commands::Repl) => {
            runner::run_repl(optimization_level, cli.verbose);
        }
        Some(Commands::Build {
            input,
            output_dir,
            jobs,
            emit_mir,
            quiet,
        }) => {
            if let Some(j) = jobs {
                rayon::ThreadPoolBuilder::new()
                    .num_threads(j)
                    .build_global()
                    .unwrap();
            }

            // Use the consolidated build_project which provides per-layer progress
            // and will also write merged MIR when requested.
            if let Err(e) = runner::build_project(
                &input,
                &output_dir,
                optimization_level,
                cli.verbose,
                emit_mir,
                !quiet,
            ) {
                error!("Build failed: {}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::ListPasses) => {
            use karte_lir::OptimizationPipeline;
            OptimizationPipeline::list_available_passes();
        }
        Some(Commands::Optimize {
            input,
            pipeline,
            output,
            debug,
        }) => {
            if let Err(e) = runner::optimize_with_pipeline(
                &input,
                &pipeline,
                output.as_deref(),
                debug,
                cli.verbose,
            ) {
                error!("Optimization failed: {}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::Aot { input, output, mode, target }) => {
            let mode = mode.map(|m| m.into()).unwrap_or_else(|| {
                if default_mode_is_explicit { default_mode } else { ParserMode::Script }
            });
            let output_path = output.unwrap_or_else(|| "a.out".to_string());
            let aot_target = match target.as_str() {
                "x86_64" | "x86" => karte_aot::AotTarget::X86_64,
                "riscv64" | "rv64" => karte_aot::AotTarget::Riscv64,
                "aarch64" | "arm64" => karte_aot::AotTarget::AArch64,
                _ => {
                    error!("不支持的目标架构: {} (支持: x86_64, riscv64, aarch64)", target);
                    std::process::exit(1);
                }
            };
            if let Err(e) = runner::aot_compile(&input, &output_path, optimization_level, mode, cli.verbose, aot_target) {
                error!("AOT 编译失败: {}", e);
                std::process::exit(1);
            }
        }
        None => {
            if let Some(ref input) = cli.input {
                if Path::new(input).exists() {
                    if let Err(err) = runner::process_file(
                        input,
                        optimization_level,
                        cli.verbose,
                        cli.emit_lir,
                        cli.output.as_deref(),
                        cli.heap_stats,
                        default_mode,
                        default_mode_is_explicit,
                        cli.emit_asm,
                    ) {
                        error!("Error: {}", err);
                        std::process::exit(1);
                    }
                } else if let Err(err) = runner::process_expression(
                    input,
                    optimization_level,
                    cli.verbose,
                    cli.emit_lir,
                    cli.output.as_deref(),
                    cli.heap_stats,
                    default_mode,
                    cli.emit_asm,
                ) {
                    error!("Error: {}", err);
                    std::process::exit(1);
                }
            } else {
                runner::run_repl(optimization_level, cli.verbose);
            }
        }
    }
}
