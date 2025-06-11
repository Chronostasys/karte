use karte_codegen::evaluate;
use karte_diagnostics::DiagnosticBag;
use karte_lexer::tokenize;
use karte_parser::parse_with_type_check;
use log::{error, info, warn};
use std::env;
use std::io::{self, Write};

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

fn process_expression(input: &str) {
    println!("Input: {}", input);

    // 词法分析
    let (tokens, lex_diagnostics) = tokenize(input);

    if !lex_diagnostics.is_empty() {
        print_diagnostics(&lex_diagnostics, input, "input");
        if lex_diagnostics.has_errors() {
            return;
        }
    }

    println!(
        "Tokens: {:?}",
        tokens.iter().map(|t| &t.token).collect::<Vec<_>>()
    );

    // 语法分析和类型检查
    let (result, parse_diagnostics) = parse_with_type_check(&tokens);

    if !parse_diagnostics.is_empty() {
        print_diagnostics(&parse_diagnostics, input, "input");
        if parse_diagnostics.has_errors() {
            return;
        }
    }

    if let Some(result) = result {
        println!("AST: {}", result.expr);
        println!("Type: {}", result.result_type);

        // 求值
        match evaluate(&result.expr) {
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

fn main() {
    env_logger::init();

    let args: Vec<String> = env::args().collect();

    if args.len() > 1 {
        // 处理命令行参数中的表达式
        for expr in &args[1..] {
            process_expression(expr);
        }
    } else {
        // 交互式模式
        println!("Karte Calculator - 四则运算解析器 Demo");
        println!("输入表达式进行计算，输入 'quit' 或 'exit' 退出");
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

                    process_expression(input);
                }
                Err(error) => {
                    error!("Error reading input: {}", error);
                    break;
                }
            }
        }
    }
}
