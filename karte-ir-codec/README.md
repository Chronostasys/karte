# Karte IR Codec

**Karte IR Codec** 是一个类似 `serde` 的自动化 IR 序列化/反序列化系统，专门为编译器的中间表示（IR）设计。

## 特性

- 🚀 **自动化**：使用 proc macro 自动生成 Display 和 Parse 实现
- 📝 **可读格式**：生成的文本格式清晰易读，适合调试和测试
- 🔄 **双向转换**：可以从 IR 转换为文本，也可以从文本解析回 IR
- ⚙️ **可配置**：支持通过 attribute 控制序列化格式
- 🎯 **类型安全**：完全类型安全，编译时检查

## 快速开始

### 1. 添加依赖

在你的 `Cargo.toml` 中添加：

```toml
[dependencies]
karte-ir-codec = { path = "../karte-ir-codec" }
karte-ir-derive = { path = "../karte-ir-derive" }
```

### 2. 使用 derive macro

```rust
use karte_ir_derive::IrCodec;

// 为 enum 类型自动生成 Display 和 Parse
#[derive(Debug, Clone, PartialEq, IrCodec)]
pub enum BinaryOperator {
    #[ir_codec(token = "+")] Add,
    #[ir_codec(token = "-")] Subtract,
    #[ir_codec(token = "*")] Multiply,
    #[ir_codec(token = "/")] Divide,
}

// 为 struct 类型自动生成 Display 和 Parse
#[derive(Debug, Clone, PartialEq, IrCodec)]
pub struct TempId(pub usize);

// 复杂的 enum，支持命名字段和元组字段
#[derive(Debug, Clone, PartialEq, IrCodec)]
pub enum Value {
    #[ir_codec(token = "var")]
    Variable { #[ir_codec(args)] name: String },
    #[ir_codec(token = "num")]
    Number { #[ir_codec(args)] value: i64 },
    #[ir_codec(token = "bool")]
    Boolean { #[ir_codec(args)] value: bool },
    #[ir_codec(token = "()")]
    Unit,
    Temp { id: TempId },
}
```

### 3. 使用序列化和反序列化

```rust
use karte_ir_codec::{IrDisplay, IrParse};

// 序列化为字符串
let value = Value::Number { value: 42 };
let text = value.to_ir_string();
// 输出: "num 42"

// 从字符串反序列化
let parsed = Value::parse_ir(&text).unwrap();
assert_eq!(parsed, value);
```

## 支持的类型

### 基础类型

以下基础类型已经实现了 `IrDisplay` 和 `IrParse`：

- `String`
- `i64`, `usize`
- `bool`
- `Option<T>` (其中 `T: IrDisplay + IrParse`)
- `Box<T>` (其中 `T: IrDisplay + IrParse`)
- `Vec<T>` (其中 `T: IrDisplay + IrParse`)

### Enum 类型

支持三种 enum 变体：

1. **Unit 变体**：

    ```rust
   enum Op { Add, Subtract }
   // 格式: "Add", "Subtract"
   ```

2. **Tuple 变体**：

    ```rust
   enum Value { Number(i64), Temp(TempId) }
   // 格式: "Number(42)", "Temp(TempId(0))"
   ```

3. **Named 变体**：

    ```rust
   enum Value { Variable { name: String } }
   // 单字段格式: "Variable(x)"
   // 多字段格式: "Variable { name = x, type = Int }"
   ```

### Struct 类型

支持三种 struct 类型：

1. **Named struct**：

    ```rust
   struct Point { x: i64, y: i64 }
   // 格式: "{ x = 10, y = 20 }"
   ```

2. **Tuple struct**：

    ```rust
   struct TempId(usize);
   // 格式: "TempId(42)"
   ```

3. **Unit struct**：

    ```rust
   struct Unit;
   // 格式: "()"
   ```

## 高级用法

### 跳过字段

使用 `#[ir_codec(skip)]` 跳过某些字段（计划中）：

```rust
#[derive(IrCodec)]
pub struct Statement {
    pub value: Value,
    #[ir_codec(skip)]
    pub span: Span,  // 不序列化 span 信息
}
```

### 自定义格式

使用 `#[ir_codec(format = "...")]` 自定义格式（计划中）：

```rust
#[derive(IrCodec)]
#[ir_codec(format = "compact")]  // 紧凑格式
pub enum Value {
    // ...
}
```

### 只生成 Display 或 Parse

如果只需要单向转换：

```rust
#[derive(IrDisplay)]  // 只生成 Display
pub enum ReadOnlyValue { /* ... */ }

#[derive(IrParse)]    // 只生成 Parse
pub enum WriteOnlyValue { /* ... */ }
```

## 示例

完整示例请参考 `karte-mir/src/codec.rs`。

### 完整的 IR 类型示例

```rust
use karte_ir_derive::IrCodec;

#[derive(Debug, Clone, PartialEq, IrCodec)]
pub struct BasicBlockId(pub usize);

#[derive(Debug, Clone, PartialEq, IrCodec)]
pub struct TempId(pub usize);

#[derive(Debug, Clone, PartialEq, IrCodec)]
pub enum BinaryOperator {
    Add, Subtract, Multiply, Divide,
    Equal, NotEqual,
    LessThan, LessEqual,
    GreaterThan, GreaterEqual,
}

#[derive(Debug, Clone, PartialEq, IrCodec)]
pub enum Value {
    Variable { name: String },
    Number { value: i64 },
    Boolean { value: bool },
    Unit,
    Temp { id: TempId },
    Constructor {
        name: String,
        arg: Option<Box<Value>>,
    },
}

#[derive(Debug, Clone, PartialEq, IrCodec)]
pub enum Statement {
    Assign {
        target: Value,
        source: Value,
    },
    BinaryOp {
        target: Value,
        left: Value,
        op: BinaryOperator,
        right: Value,
    },
}
```

### 使用示例

```rust
use karte_ir_codec::{IrDisplay, IrParse};

// 创建一个 Statement
let stmt = Statement::BinaryOp {
    target: Value::Temp { id: TempId(0) },
    left: Value::Variable { name: "x".to_string() },
    op: BinaryOperator::Add,
    right: Value::Number { value: 1 },
};

// 序列化为可读文本
let text = stmt.to_ir_string();
// 输出: "BinaryOp { target = Temp(TempId(0)), left = Variable(x), op = Add, right = Number(1) }"

// 从文本反序列化
let parsed_stmt = Statement::parse_ir(&text).unwrap();
assert_eq!(parsed_stmt, stmt);

// 可以保存到文件或用于测试
std::fs::write("ir_output.txt", text).unwrap();
```

## 架构

本系统由两个 crate 组成：

### `karte-ir-codec` (runtime)

提供核心 trait 定义和运行时支持：

- `IrDisplay` trait：用于序列化
- `IrParse` trait：用于反序列化
- 基础类型的实现
- Parser 辅助函数（基于 nom）

### `karte-ir-derive` (proc macro)

提供 derive macro：

- `#[derive(IrCodec)]`：同时生成 Display 和 Parse
- `#[derive(IrDisplay)]`：只生成 Display
- `#[derive(IrParse)]`：只生成 Parse

## 与 serde 的对比

| 特性 | karte-ir-codec | serde |
|------|----------------|-------|
| 目标 | IR 序列化，强调可读性 | 通用数据序列化 |
| 格式 | 自定义文本格式（类似 Debug） | JSON/TOML/Binary 等 |
| 可读性 | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ |
| 性能 | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ |
| 双向转换 | ✅ | ✅ |
| 用途 | IR 调试、测试、持久化 | 通用数据交换 |

## 最佳实践

1. **为所有 IR 类型添加 `IrCodec`**：确保整个 IR 都可以序列化
2. **使用单元测试验证**：为每个类型编写往返测试（serialize → deserialize）
3. **跳过不必要的字段**：如 `Span` 等调试信息可以跳过
4. **保持格式一致性**：使用统一的命名和格式约定

## 未来计划

- [ ] 支持 `HashMap` 和 `BTreeMap`
- [ ] 支持更多自定义格式选项
- [ ] 生成更紧凑的格式选项
- [ ] 支持增量解析
- [ ] 提供格式化工具（类似 rustfmt）
- [ ] 支持跨版本兼容性

## 许可证

MIT

