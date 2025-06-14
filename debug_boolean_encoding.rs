use karte_tests::execute_with_pipeline_debug;
use karte_lexer::tokenize;
use karte_parser::parse;
use karte_hir::type_check;

fn main() -> Result<(), String> {
    // 测试单个boolean值的编码
    println!("=== Boolean Encoding Debug ===");
    
    // Test true
    let (tokens, _) = tokenize("true");
    let (expr, _) = parse(&tokens);
    let expr = expr.ok_or("Parse failed")?;
    let (_, _) = type_check(&expr);
    
    println!("Testing 'true':");
    let result = execute_with_pipeline_debug(&expr, true)?;
    println!("Result: {}\n", result);
    
    // Test false
    let (tokens, _) = tokenize("false");
    let (expr, _) = parse(&tokens);
    let expr = expr.ok_or("Parse failed")?;
    let (_, _) = type_check(&expr);
    
    println!("Testing 'false':");
    let result = execute_with_pipeline_debug(&expr, true)?;
    println!("Result: {}\n", result);
    
    // Test logical operation
    let (tokens, _) = tokenize("true && false");
    let (expr, _) = parse(&tokens);
    let expr = expr.ok_or("Parse failed")?;
    let (_, _) = type_check(&expr);
    
    println!("Testing 'true && false':");
    let result = execute_with_pipeline_debug(&expr, true)?;
    println!("Result: {}\n", result);
    
    Ok(())
} 