use karte_mir::ir::{Value, TempId};
use karte_ir_codec::{IrDisplay, IrParse};

#[test]
fn test_temp_id_display() {
    let temp = TempId(0);
    let s = format!("{}", temp);
    println!("TempId(0) displays as: '{}'", s);
}

#[test]
fn test_value_temp_display() {
    let val = Value::Temp { id: TempId(0) };
    let s = format!("{}", val);
    println!("Value::Temp displays as: '{}'", s);
}

#[test]
fn test_parse_value_temp() {
    let inputs = vec![
        "%0",
        "Temp { id: %0 }",
        "temp %0",
    ];
    
    for input in inputs {
        println!("Trying to parse '{}' as Value", input);
        let result = Value::parse_ir(input);
        println!("Result: {:?}\n", result);
    }
}
