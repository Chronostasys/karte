use std::collections::HashMap;
use karte_ir_codec::{IrParse, IrDisplay};
use karte_ir_derive::IrCodec;

#[derive(Debug, Clone, PartialEq, IrCodec)]
pub struct MyStruct {
    pub name: String,
    pub value: i64,
}

#[test]
fn test_hashmap_with_struct() {
    let input = r#"{
        key1: 
            MyStruct
                name: 
                    hello
                value: 
                    42
        }"#;
    
    println!("Input: {}", input);
    
    let result = HashMap::<String, MyStruct>::parse_ir(input);
    println!("Result: {:?}", result);
    
    assert!(result.is_ok());
    let map = result.unwrap();
    assert_eq!(map.len(), 1);
    assert_eq!(map.get("key1").unwrap().name, "hello");
    assert_eq!(map.get("key1").unwrap().value, 42);
}
