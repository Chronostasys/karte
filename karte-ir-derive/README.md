# Karte IR Derive

**Karte IR Derive** 是为 Karte 编译器提供的 proc macro crate，用于自动生成 IR 类型的序列化和反序列化代码。

## 概述

这个 crate 提供了三个 derive macro：

- `#[derive(IrCodec)]`：同时生成 `IrDisplay` 和 `IrParse` 实现
- `#[derive(IrDisplay)]`：只生成 `IrDisplay` 实现（序列化）
- `#[derive(IrParse)]`：只生成 `IrParse` 实现（反序列化）

## 使用方法

```rust
use karte_ir_derive::IrCodec;

#[derive(IrCodec)]
pub enum Value {
    Number { value: i64 },
    Variable { name: String },
}
```

详细文档请参考 `karte-ir-codec` crate。

## 实现原理

### Display 生成

对于 enum：
- Unit 变体: `VariantName`
- Tuple 变体: `VariantName(arg1, arg2, ...)`
- Named 变体（单字段）: `VariantName(value)`
- Named 变体（多字段）: `VariantName { field1 = value1, field2 = value2 }`

对于 struct：
- Named struct: `{ field1 = value1, field2 = value2 }`
- Tuple struct: `(value1, value2, ...)`
- Unit struct: `()`

### Parse 生成

使用 nom parser combinator 库生成对应的解析器，能够解析 Display 生成的格式。

## 属性支持

### `#[ir_codec(skip)]`

跳过字段的序列化（计划中）：

```rust
#[derive(IrCodec)]
pub struct Statement {
    pub value: Value,
    #[ir_codec(skip)]
    pub span: Span,
}
```

### `#[ir_codec(format = "...")]`

自定义格式（计划中）：

```rust
#[derive(IrCodec)]
#[ir_codec(format = "compact")]
pub enum Value { /* ... */ }
```

## 代码结构

- `lib.rs`: 主入口，定义 derive macro
- `display.rs`: 生成 Display 实现的逻辑
- `parse.rs`: 生成 Parse 实现的逻辑
- `utils.rs`: 共享的工具函数

## 许可证

MIT

