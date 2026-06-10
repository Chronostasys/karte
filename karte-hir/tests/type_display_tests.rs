use karte_hir::types::{Type, StructField, SumVariant};

#[test]
fn test_type_display_number() {
    assert_eq!(format!("{}", Type::Number), "number");
}

#[test]
fn test_type_display_string() {
    assert_eq!(format!("{}", Type::String), "string");
}

#[test]
fn test_type_display_bool() {
    assert_eq!(format!("{}", Type::Bool), "bool");
}

#[test]
fn test_type_display_unit() {
    assert_eq!(format!("{}", Type::Unit), "()");
}

#[test]
fn test_type_display_function() {
    let fn_type = Type::function(vec![Type::Number, Type::Number], Type::Number);
    let display = format!("{}", fn_type);
    assert!(display.contains("number"), "Function display should contain 'number': {}", display);
}

#[test]
fn test_type_display_struct() {
    let struct_type = Type::struct_type(
        "Point".to_string(),
        vec![
            StructField { name: "x".to_string(), field_type: Type::Number },
            StructField { name: "y".to_string(), field_type: Type::Number },
        ],
    );
    let display = format!("{}", struct_type);
    assert!(display.contains("Point"), "Struct display should contain 'Point': {}", display);
}

#[test]
fn test_type_display_sum() {
    let sum_type = Type::sum(
        "Color".to_string(),
        vec![
            SumVariant { name: "Red".to_string(), data_types: vec![] },
            SumVariant { name: "Green".to_string(), data_types: vec![] },
            SumVariant { name: "Blue".to_string(), data_types: vec![] },
        ],
    );
    let display = format!("{}", sum_type);
    assert!(display.contains("Color"), "Sum type display should contain 'Color': {}", display);
}

#[test]
fn test_type_display_reference() {
    let ref_type = Type::reference(Type::Number);
    let display = format!("{}", ref_type);
    assert!(display.contains("number"), "Reference display should contain 'number': {}", display);
}

#[test]
fn test_type_display_option() {
    let opt_type = Type::option(Type::Number);
    let display = format!("{}", opt_type);
    assert!(display.contains("Option"), "Option display should contain 'Option': {}", display);
}

#[test]
fn test_type_display_result() {
    let res_type = Type::result(Type::Number, Type::String);
    let display = format!("{}", res_type);
    assert!(display.contains("Result"), "Result display should contain 'Result': {}", display);
}
