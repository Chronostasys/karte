# IR Codec 快速开始

## 5分钟上手指南

### 第一步：添加依赖

在你的 IR crate 的 `Cargo.toml` 中：

```toml
[dependencies]
karte-ir-codec = { path = "../karte-ir-codec" }
karte-ir-derive = { path = "../karte-ir-derive" }
nom.workspace = true  # 如果需要自定义解析器
```

### 第二步：添加 derive

```rust
use karte_ir_derive::IrCodec;

// 简单 enum
#[derive(Debug, Clone, PartialEq, IrCodec)]
pub enum BinaryOp {
    #[ir_codec(token = "+")] Add,
    #[ir_codec(token = "-")] Subtract,
}

// 带字段的 enum
#[derive(Debug, Clone, PartialEq, IrCodec)]
pub enum Value {
    #[ir_codec(token = "num")]
    Number { #[ir_codec(args)] value: i64 },
    #[ir_codec(token = "var")]
    Variable { #[ir_codec(args)] name: String },
}

// Tuple struct
#[derive(Debug, Clone, PartialEq, IrCodec)]
pub struct TempId(pub usize);
```

### 第三步：使用

```rust
use karte_ir_codec::{IrDisplay, IrParse};

// 序列化
let value = Value::Number { value: 42 };
let text = value.to_ir_string();
println!("{}", text);  // "num 42"

// 反序列化
let parsed = Value::parse_ir(&text).unwrap();
assert_eq!(parsed, value);
```

## 常见模式

### 往返测试

```rust
#[test]
fn test_roundtrip() {
    let original = Value::Number { value: 42 };
    let serialized = original.to_ir_string();
    let deserialized = Value::parse_ir(&serialized).unwrap();
    assert_eq!(original, deserialized);
}
```

### 调试打印

```rust
fn debug_ir(program: &MirProgram) {
    for (name, func) in &program.functions {
        println!("Function {}:", name);
        println!("{}", func.to_ir_string());
    }
}
```

### 保存/加载

```rust
// 保存
std::fs::write("output.mir", program.to_ir_string()).unwrap();

// 加载
let text = std::fs::read_to_string("output.mir").unwrap();
let program = MirProgram::parse_ir(&text).unwrap();
```

## 支持的类型

✅ enum (Unit, Tuple, Named)  
✅ struct (Named, Tuple, Unit)  
✅ `String`, `i64`, `usize`, `bool`  
✅ `Option<T>`, `Box<T>`, `Vec<T>`  

## 更多资源

- 📖 [完整指南](./IR_CODEC_GUIDE.md)
- 📦 [karte-ir-codec README](../karte-ir-codec/README.md)
- 💡 [示例代码](../karte-ir-codec/examples/)
- 🔧 [实际应用](../karte-mir/src/codec.rs)

