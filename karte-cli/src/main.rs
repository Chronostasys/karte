use karte_codegen::lir_interpreter::execute;
use karte_diagnostics::DiagnosticBag;
use karte_lexer::tokenize;
use karte_lir::lower::lower_mir_to_lir;
use karte_lir::optimization_pipeline::{OptimizationPipeline, OptimizationLevel};
use karte_mir::lower::lower_expr_to_mir;
use karte_parser::parse_with_type_check;
use log::{error, info, warn};
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

fn print_diagnostics(diagnostics: &DiagnosticBag, source_code: &str, filename: &str) {
    for diagnostic in &diagnostics.diagnostics {
        let level_str = match diagnostic.level {
            karte_diagnostics::DiagnosticLevel::Error => "ERROR",
            karte_diagnostics::DiagnosticLevel::Warning => "WARNING",
            karte_diagnostics::DiagnosticLevel::Info => "INFO",
            karte_diagnostics::DiagnosticLevel::Hint => "HINT",
        };

        println!(
            "{}:{}-{}: {}: {}",
            filename,
            diagnostic.span.start,
            diagnostic.span.end,
            level_str,
            diagnostic.message
        );
    }
}

fn process_expression(input: &str, filename: &str, optimization_level: OptimizationLevel) {
    if filename != "input" {
        println!("Processing file: {}", filename);
    } else {
        println!("Input: {}", input);
    }

    // 词法分析
    let (tokens, lex_diagnostics) = tokenize(input);

    if !lex_diagnostics.is_empty() {
        print_diagnostics(&lex_diagnostics, input, filename);
        if lex_diagnostics.has_errors() {
            return;
        }
    }

    if filename == "input" {
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
            return;
        }
    }

    if let Some(result) = result {
        if filename == "input" {
            println!("AST: {}", result.expr);
        }
        println!("Type: {}", result.result_type);

        // --- New Lowering and Execution Pipeline ---
        println!("\n--- Lowering to MIR ---");
        let mir_program = match lower_expr_to_mir(&result.expr) {
            Ok(prog) => prog,
            Err(errors) => {
                for err in errors {
                    error!("MIR Lowering Error: {}", err);
                }
                return;
            }
        };
        println!("{}", mir_program);

        println!("\n--- Lowering to LIR ---");
        println!("=== 返回高级LIR (包含Alloc指令，待优化) ===");
        let mut lir_program = match lower_mir_to_lir(&mir_program) {
            Ok(prog) => prog,
            Err(errors) => {
                for err in errors {
                    error!("LIR Lowering Error: {}", err);
                }
                return;
            }
        };
        println!("{}", lir_program);
        println!("================================================");

        println!("\n--- LIR 优化 ---");
        let mut pipeline = OptimizationPipeline::new(optimization_level);
        match pipeline.optimize(&mut lir_program) {
            Ok(stats) => {
                println!("优化完成:");
                println!("  - 总耗时: {}ms", stats.total_time_ms);
                println!("  - 执行pass数: {}", stats.passes_executed);
                println!("  - 指令数变化: {} -> {}", stats.instructions_before, stats.instructions_after);
                if stats.instructions_before > 0 {
                    let reduction = (stats.instructions_before - stats.instructions_after) as f64 / stats.instructions_before as f64 * 100.0;
                    println!("  - 指令减少: {:.1}%", reduction);
                }
            }
            Err(errors) => {
                for err in errors {
                    error!("LIR 优化错误: {}", err);
                }
                return;
            }
        }
        
        println!("\n--- 优化后LIR ---");
        println!("{}", lir_program);

        println!("\n--- 寄存器分配 ---");
        // 注意：寄存器分配需要在指令降级之前进行，这样Memory2Reg优化生成的新寄存器才能被正确分配
        // 这里我们暂时跳过寄存器分配，因为当前的实现在执行器中进行
        println!("寄存器分配将在执行器中进行");
        
        println!("\n--- 指令降级 ---");
        if let Err(lowering_error) = karte_lir::lower_program_instructions(&mut lir_program) {
            error!("指令降级错误: {}", lowering_error);
            return;
        }
        
        println!("\n--- 降级后LIR (可执行) ---");
        println!("{}", lir_program);

        println!("\n--- Executing LIR ---");
        match execute(&lir_program) {
            Ok(value) => {
                println!("Result: {}", value);
            }
            Err(err) => {
                error!("Runtime error: {}", err);
            }
        }
    } else {
        error!("Failed to parse expression or type check failed");
    }

    println!();
}

fn process_file(filename: &str, optimization_level: OptimizationLevel) -> Result<(), Box<dyn std::error::Error>> {
    // 读取文件内容
    let content = fs::read_to_string(filename)?;
    
    // 处理文件内容
    process_expression(&content, filename, optimization_level);
    
    Ok(())
}

fn parse_optimization_level(arg: &str) -> Option<OptimizationLevel> {
    match arg {
        "--debug" | "-O0" => Some(OptimizationLevel::Debug),
        "--fast" | "-O1" => Some(OptimizationLevel::Fast),
        "--balanced" | "-O2" => Some(OptimizationLevel::Balanced),
        "--performance" | "-O3" => Some(OptimizationLevel::Performance),
        _ => None,
    }
}

fn print_help() {
    println!("Karte 编程语言解释器");
    println!();
    println!("用法:");
    println!("  karte [选项] [表达式或文件]");
    println!();
    println!("选项:");
    println!("  --debug, -O0        无优化，调试模式 (保留所有栈操作)");
    println!("  --fast, -O1         快速优化 (Memory2Reg + 常量折叠)");
    println!("  --balanced, -O2     平衡优化 (默认，所有基本优化)");
    println!("  --performance, -O3  高性能优化 (激进优化)");
    println!("  --help, -h          显示此帮助信息");
    println!();
    println!("示例:");
    println!("  karte \"1 + 2 * 3\"              # 使用默认优化计算表达式");
    println!("  karte --debug \"1 + 2 * 3\"      # 无优化，显示完整栈操作");
    println!("  karte -O3 \"1 + 2 * 3\"          # 激进优化");
    println!("  karte program.karte             # 执行文件");
    println!("  karte --balanced program.karte  # 使用平衡优化执行文件");
}

fn main() {
    env_logger::init();

    let args: Vec<String> = env::args().collect();
    let mut optimization_level = OptimizationLevel::Balanced; // 默认使用平衡优化
    let mut non_option_args = Vec::new();

    // 解析命令行参数
    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        
        if arg == "--help" || arg == "-h" {
            print_help();
            return;
        }
        
        if let Some(level) = parse_optimization_level(arg) {
            optimization_level = level;
        } else {
            non_option_args.push(arg.clone());
        }
        
        i += 1;
    }

    if !non_option_args.is_empty() {
        // 处理命令行参数
        for arg in &non_option_args {
            // 检查是否为文件路径
            if Path::new(arg).exists() && Path::new(arg).is_file() {
                // 如果是文件，读取并执行文件内容
                match process_file(arg, optimization_level) {
                    Ok(()) => {},
                    Err(err) => {
                        error!("Error reading file '{}': {}", arg, err);
                    }
                }
            } else {
                // 如果不是文件，当作表达式处理
                process_expression(arg, "input", optimization_level);
            }
        }
    } else {
        // 交互式模式
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
        println!("你也可以传入文件名作为参数来执行文件: cargo run filename.karte");
        println!("使用 --help 查看优化选项");
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
                        match process_file(filename, optimization_level) {
                            Ok(()) => {},
                            Err(err) => {
                                error!("Error reading file '{}': {}", filename, err);
                            }
                        }
                        continue;
                    }

                    process_expression(input, "input", optimization_level);
                }
                Err(error) => {
                    error!("Error reading input: {}", error);
                    break;
                }
            }
        }
    }
}
