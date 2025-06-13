use karte_codegen::lir_interpreter::execute;
use karte_diagnostics::DiagnosticBag;
use karte_lexer::tokenize;
use karte_lir::lower::lower_mir_to_lir;
use karte_mir::lower::lower_expr_to_mir;
use karte_parser::parse_with_type_check;
use log::{error, info, warn};
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

fn print_diagnostics(diagnostics: &DiagnosticBag, source_code: &str, filename: &str) {
    if !diagnostics.is_empty() {
        // 使用 miette 美观显示
        if diagnostics.print_fancy(source_code, filename).is_err() {
            // 如果美观显示失败，回退到简单显示
            eprintln!("Diagnostics:");
            for diagnostic in &diagnostics.diagnostics {
                match diagnostic.level {
                    karte_diagnostics::DiagnosticLevel::Error => {
                        error!("{}", diagnostic);
                    }
                    karte_diagnostics::DiagnosticLevel::Warning => {
                        warn!("{}", diagnostic);
                    }
                    _ => {
                        info!("{}", diagnostic);
                    }
                }
            }
        }
    }
}

fn process_expression(input: &str, filename: &str) {
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
        let lir_program = match lower_mir_to_lir(&mir_program) {
            Ok(prog) => prog,
            Err(errors) => {
                for err in errors {
                    error!("LIR Lowering Error: {}", err);
                }
                return;
            }
        };
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

fn process_file(filename: &str) -> Result<(), Box<dyn std::error::Error>> {
    // 读取文件内容
    let content = fs::read_to_string(filename)?;
    
    // 处理文件内容
    process_expression(&content, filename);
    
    Ok(())
}

fn main() {
    env_logger::init();

    let args: Vec<String> = env::args().collect();

    if args.len() > 1 {
        // 处理命令行参数
        for arg in &args[1..] {
            // 检查是否为文件路径
            if Path::new(arg).exists() && Path::new(arg).is_file() {
                // 如果是文件，读取并执行文件内容
                match process_file(arg) {
                    Ok(()) => {},
                    Err(err) => {
                        error!("Error reading file '{}': {}", arg, err);
                    }
                }
            } else {
                // 如果不是文件，当作表达式处理
                process_expression(arg, "input");
            }
        }
    } else {
        // 交互式模式
        println!("Karte 编程语言解释器");
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
                        match process_file(filename) {
                            Ok(()) => {},
                            Err(err) => {
                                error!("Error reading file '{}': {}", filename, err);
                            }
                        }
                        continue;
                    }

                    process_expression(input, "input");
                }
                Err(error) => {
                    error!("Error reading input: {}", error);
                    break;
                }
            }
        }
    }
}
