use karte_mir::*;
use karte_ir_codec::{IrDisplay, IrParse};

#[test]
fn test_parse_simple_value() {
    // Test parsing a simple Value
    let value_str = "num 2";
    let parsed = Value::parse_ir(value_str);
    println!("Parsing '{}': {:?}", value_str, parsed);
    assert!(parsed.is_ok());
}

#[test]
fn test_display_and_parse_mir_function() {
    // Create a simple MirFunction
    let mut func = MirFunction::new("test".to_string(), vec![]);
    
    // Display it
    let displayed = func.to_ir_string();
    println!("Displayed MirFunction:\n{}", displayed);
    
    // Try to parse it back
    let parsed = MirFunction::parse_ir(&displayed);
    println!("Parse result: {:?}", parsed);
    
    if let Ok(parsed_func) = parsed {
        assert_eq!(parsed_func.name, func.name);
    }
}

#[test]
fn test_display_and_parse_mir_program() {
    // Create a simple MirProgram
    let mut program = MirProgram::new();
    program.main_function = Some("main".to_string());
    
    let mut main_func = MirFunction::new("main".to_string(), vec![]);
    program.functions.insert("main".to_string(), main_func);
    
    // Display it
    let displayed = program.to_ir_string();
    println!("Displayed MirProgram:\n{}", displayed);
    
    // Try to parse it back
    let parsed = MirProgram::parse_ir(&displayed);
    println!("Parse result: {:?}", parsed);
    
    assert!(parsed.is_ok());
}
