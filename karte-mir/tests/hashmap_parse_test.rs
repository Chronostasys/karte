use karte_mir::*;
use karte_ir_codec::{IrDisplay, IrParse};

#[test]
fn test_parse_mir_function_standalone() {
    let input = "MirFunction
    name: test
    params: []
    blocks: {}";
    
    println!("Parsing MirFunction from: {}", input);
    let result = MirFunction::parse_ir(input);
    println!("Result: {:?}", result);
    assert!(result.is_ok(), "Failed to parse: {:?}", result);
}

#[test]
fn test_parse_mir_function_with_leading_space() {
    let input = " MirFunction
    name: test
    params: []
    blocks: {}";
    
    println!("Parsing MirFunction with leading space from: {}", input);
    let result = MirFunction::parse_ir(input);
    println!("Result: {:?}", result);
    assert!(result.is_ok(), "Failed to parse: {:?}", result);
}

#[test]
fn test_parse_hashmap_single_function() {
    let input = "{
    main: MirFunction
        name: main
        params: []
        blocks: {}
    }";
    
    println!("Parsing HashMap<String, MirFunction> from: {}", input);
    let result = std::collections::HashMap::<String, MirFunction>::parse_ir(input);
    println!("Result: {:?}", result);
    assert!(result.is_ok(), "Failed to parse: {:?}", result);
}

#[test]
fn test_parse_hashmap_with_real_blocks() {
    // New format: all values start on a new line after the colon
    let input = "{
    main: 
        MirFunction
            name: 
                main
            params: 
                []
            blocks: 
                {
                    bb0: 
                        BasicBlock
                            id: 
                                bb0
                            statements: 
                                [
                                    %1 = num 2,
                                    %2 = %1,
                                    %3 = num 3,
                                    %0 = %2 * %3
                                    ]
                            terminator: 
                                ret %0
                    }
    }";
    
    println!("Parsing HashMap<String, MirFunction> with real blocks from: {}", input);
    let result = std::collections::HashMap::<String, MirFunction>::parse_ir(input);
    println!("Result: {:?}", result);
    assert!(result.is_ok(), "Failed to parse: {:?}", result);
}
