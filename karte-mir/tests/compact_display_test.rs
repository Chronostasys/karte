use karte_ir_codec::IrDisplay;
use karte_mir::*;

#[test]
fn test_compact_value_display() {
    // 测试简洁的值显示

    // Number(1) -> num 1
    let val = Value::Number { value: 42 };
    let output = val.to_ir_string();
    println!("\n数字: {}", output);
    assert!(
        output.starts_with("num"),
        "数字输出应当以 num 开头: {}",
        output
    );
    assert!(output.contains("42"), "数字输出应包含具体数值: {}", output);

    // Variable(x) -> var x
    let val = Value::Variable {
        name: "x".to_string(),
    };
    let output = val.to_ir_string();
    println!("变量: {}", output);
    assert!(
        output.starts_with("var"),
        "变量输出应当以 var 开头: {}",
        output
    );
    assert!(output.contains("x"), "变量输出应包含变量名: {}", output);

    // Boolean(true) -> bool true
    let val = Value::Boolean { value: true };
    let output = val.to_ir_string();
    println!("布尔: {}", output);
    assert!(
        output.starts_with("bool"),
        "布尔输出应当以 bool 开头: {}",
        output
    );
    assert!(output.contains("true"), "布尔输出应包含 true: {}", output);

    // Unit -> () (但当前实现显示为 Unit，因为它没有字段)
    let val = Value::Unit;
    let output = val.to_ir_string();
    println!("单元: {}", output);
    assert_eq!(output.trim(), "()");

    // Temp(TempId(5)) -> t TempId(5)
    let val = Value::Temp { id: TempId(5) };
    let output = val.to_ir_string();
    println!("临时变量: {}", output);
    assert!(
        output.contains("%5"),
        "Temp 输出应包含实际 temp id: {}",
        output
    );

    // Function(add) -> fn add
    let val = Value::Function {
        name: "add".to_string(),
    };
    let output = val.to_ir_string();
    println!("函数: {}", output);
    assert!(
        output.starts_with("fn"),
        "函数输出应当以 fn 开头: {}",
        output
    );
    assert!(output.contains("add"), "函数输出应包含名称: {}", output);

    // Reference(&x) -> & var x
    let val = Value::Reference {
        value: Box::new(Value::Variable {
            name: "x".to_string(),
        }),
    };
    let output = val.to_ir_string();
    println!("引用: {}", output);
    assert!(output.starts_with("&"), "引用输出应以 & 开头: {}", output);
    assert!(output.contains("x"), "引用输出应包含被引用值: {}", output);
}

#[test]
fn test_compact_terminator_display() {
    // 测试终结语句的简洁显示

    // Goto(bb1) -> goto BasicBlockId(1)
    let term = Terminator::Goto {
        target: BasicBlockId(1),
        span: Default::default(),
    };
    let output = term.to_ir_string();
    println!("\n跳转: {}", output);
    assert!(output.starts_with("goto "));

    // Return(n 42) -> ret n 42
    let term = Terminator::Return {
        value: Some(Value::Number { value: 42 }),
        span: Default::default(),
    };
    let output = term.to_ir_string();
    println!("返回: {}", output);
    assert!(output.starts_with("ret "));
}

#[test]
fn test_operator_display() {
    // 测试运算符的符号显示
    let op = BinaryOperator::Add;
    assert_eq!(op.to_ir_string(), "+");

    let op = BinaryOperator::Multiply;
    assert_eq!(op.to_ir_string(), "*");

    let op = BinaryOperator::Equal;
    assert_eq!(op.to_ir_string(), "==");

    let op = UnaryOperator::Not;
    assert_eq!(op.to_ir_string(), "!");

    let op = UnaryOperator::Minus;
    assert_eq!(op.to_ir_string(), "-");
}

#[test]
fn test_format_comparison() {
    println!("\n=== 格式对比 ===");

    println!("\n优化前 vs 优化后:");

    let val = Value::Number { value: 42 };
    println!("  Number(42) -> {}", val.to_ir_string());

    let val = Value::Variable {
        name: "x".to_string(),
    };
    println!("  Variable(x) -> {}", val.to_ir_string());

    let val = Value::Function {
        name: "factorial".to_string(),
    };
    println!("  Function(factorial) -> {}", val.to_ir_string());

    let op = BinaryOperator::Add;
    println!("  BinaryOperator::Add -> {}", op.to_ir_string());

    let term = Terminator::Return {
        value: Some(Value::Number { value: 0 }),
        span: Default::default(),
    };
    println!("  Return(Number(0)) -> {}", term.to_ir_string());
}
