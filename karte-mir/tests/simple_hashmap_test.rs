use karte_ir_codec::IrParse;
use std::collections::HashMap;

#[test]
fn test_simple_hashmap_with_newline() {
    // 最简单的测试: HashMap<String, String> with newline after colon
    let input = "{
    key1: 
        value1
    }";
    
    println!("Input: {}", input);
    let result = HashMap::<String, String>::parse_ir(input);
    println!("Result: {:?}", result);
    assert!(result.is_ok(), "Failed: {:?}", result);
}

#[test]
fn test_simple_hashmap_no_newline() {
    // 对比测试: 没有换行的情况
    let input = "{
    key1: value1
    }";
    
    println!("Input: {}", input);
    let result = HashMap::<String, String>::parse_ir(input);
    println!("Result: {:?}", result);
    assert!(result.is_ok(), "Failed: {:?}", result);
}
