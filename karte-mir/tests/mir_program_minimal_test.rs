use karte_mir::ir::MirProgram;
use karte_ir_codec::IrParse;

#[test]
fn test_parse_minimal_mir_program() {
    let input = r#"functions: 
    {}
main_function: 
    main
main_return_value: 
    %0
temp_values: 
    {}
struct_types: 
    {}"#;
    
    println!("Parsing minimal MirProgram from:\n{}", input);
    
    let result = MirProgram::parse_ir(input);
    println!("Result: {:?}", result);
    
    assert!(result.is_ok(), "Failed to parse minimal MirProgram: {:?}", result);
}
