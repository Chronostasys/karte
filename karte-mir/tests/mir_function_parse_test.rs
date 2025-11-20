use karte_ir_codec::IrParse;
use karte_mir::ir::MirFunction;

#[test]
fn test_parse_mir_function() {
    let input = r#"MirFunction
                name: 
                    test_func
                params: 
                    []
                blocks: 
                    {}"#;

    println!("Parsing MirFunction from:\n{}", input);

    let result = MirFunction::parse_ir(input);
    println!("Result: {:?}", result);

    assert!(result.is_ok(), "Failed to parse MirFunction: {:?}", result);
}
