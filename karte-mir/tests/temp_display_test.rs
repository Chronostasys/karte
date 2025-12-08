use karte_ir_codec::IrDisplay;
use karte_mir::{TempId, Value};

#[test]
fn test_temp_id_display() {
    let temp_id = TempId(42);
    assert_eq!(temp_id.to_ir_string(), "%42");
}

#[test]
fn test_value_temp_display() {
    let temp_value = Value::Temp {
        id: TempId(42),
        ty: None,
    };
    let output = temp_value.to_ir_string();
    println!("Value::Temp output: {}", output);
    // 期望: %42 或者 Temp { id: %42 } 或其他格式
    assert!(
        output.contains("42"),
        "Output should contain the temp ID number: {}",
        output
    );
}
