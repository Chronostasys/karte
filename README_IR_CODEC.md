# Karte IR Codec 概览

这是 Karte 编译器中 IR 编解码系统的快速索引。详细设计与使用说明请参考以下文档：

- 📖 `docs/IR_CODEC_GUIDE.md`：完整指南，包含格式规范、属性说明与 MIR 输出示例。
- ⚡ `docs/IR_CODEC_QUICK_START.md`：5 分钟上手，涵盖依赖、派生宏与常见场景。
- 📦 `karte-ir-codec/README.md`：运行时 crate API 文档与示例。
- 🧩 `karte-ir-derive/README.md`：派生宏 crate 说明与属性参考。

## 核心特点

- `#[derive(IrCodec)]` 零样板生成 Display/Parse 实现。
- MIR/LIR 输出采用 LLVM 风格，可读、可 round-trip。
- Token 系统支持中缀/前缀表达式、SSA `%`、`num` 等短标记。
- 集合与映射统一缩进，便于调试、快照测试与持久化。

## 简短示例

```rust
use karte_ir_derive::IrCodec;
use karte_ir_codec::{IrDisplay, IrParse};

#[derive(Debug, Clone, PartialEq, IrCodec)]
pub enum Value {
    #[ir_codec(token = "num")]
    Number { #[ir_codec(args)] value: i64 },
    #[ir_codec(token = "var")]
    Variable { #[ir_codec(args)] name: String },
}

let v = Value::Number { value: 42 };
let text = v.to_ir_string();      // "num 42"
let parsed = Value::parse_ir(&text).unwrap();
assert_eq!(parsed, v);
```

若需要了解 MIR 语句、终结语句与集合格式的完整展示，请查阅 `docs/IR_CODEC_GUIDE.md` 的 “MIR 输出参考” 章节。

