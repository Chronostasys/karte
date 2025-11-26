# Karte IR 编解码系统指南

## 概述

Karte IR 编解码系统是一套面向编译器中间表示（IR）的文本序列化与反序列化工具链。它由运行时库和派生宏组成，可以在保持类型安全的前提下，把复杂的 IR 结构转换成紧凑、可读的文本，再从文本恢复为原始数据结构。

系统设计目标：

- **零样板代码**：只需 `#[derive(IrCodec)]` 即可生成 Display/Parse 实现。
- **格式可读**：输出贴近 LLVM 风格，便于调试和快照比对。
- **完全可逆**：`to_ir_string` 与 `parse_ir` 始终互为逆运算。
- **可扩展**：通过属性调整 token、缩进、字段标签等展示细节。

## 组件一览

| Crate | 角色 | 关键类型/功能 |
|-------|------|---------------|
| `karte-ir-codec` | 运行时库 | `IrDisplay`, `IrParse`, `ParseError`，以及集合/基础类型实现 |
| `karte-ir-derive` | Proc Macro | `#[derive(IrCodec)]`, `#[derive(IrDisplay)]`, `#[derive(IrParse)]` 与属性解析 |

两者协同工作：`karte-ir-derive` 在编译期分析用户定义的 struct/enum，生成调用运行时 trait 的代码；`karte-ir-codec` 负责具体的格式化与解析逻辑。

## 快速开始

### 1. 添加依赖

```toml
[dependencies]
karte-ir-codec = { path = "../karte-ir-codec" }
karte-ir-derive = { path = "../karte-ir-derive" }
```

### 2. 为 IR 类型派生

```rust
use karte_ir_derive::IrCodec;

#[derive(Debug, Clone, PartialEq, IrCodec)]
pub enum BinaryOperator {
    #[ir_codec(token = "+")] Add,
    #[ir_codec(token = "*")] Multiply,
}

#[derive(Debug, Clone, PartialEq, IrCodec)]
pub enum Value {
    #[ir_codec(token = "var")]
    Variable { #[ir_codec(args)] name: String },
    #[ir_codec(token = "num")]
    Number { #[ir_codec(args)] value: i64 },
}
```

### 3. 序列化与反序列化

```rust
use karte_ir_codec::{IrDisplay, IrParse};

let value = Value::Number { value: 42 };
let text = value.to_ir_string();      // "num 42"
let parsed = Value::parse_ir(&text)?; // Value::Number { value: 42 }
```

## MIR 输出参考

以下示例基于 `karte-mir/src/ir.rs` 中的实际定义。

### 值 (Value) token

| 变体 | Token | 示例输出 |
|------|-------|-----------|
| `Variable { name }` | `var` | `var x` |
| `Number { value }` | `num` | `num 5` |
| `Boolean { value }` | `bool` | `bool true` |
| `Unit` | `()` | `()` |
| `Temp { id }` | `%` | `%3` |
| `Function { name }` | `fn` | `fn lambda$0` |
| `Reference { value }` | `&` | `& var x` |
| 复合结构 (`Struct`, `Closure`, `Constructor`, `QualifiedConstructor`) | — | `Struct { name = Closure, fields = { ... } }` |

### 运算符 token

| 类型 | Token | 示例 |
|------|-------|-------|
| `BinaryOperator::Add` | `+` | `%0 = %1 + %2` |
| `BinaryOperator::Equal` | `==` | `%0 = %1 == %2` |
| `UnaryOperator::Minus` | `-` | `%0 = -num 1` |
| `UnaryOperator::Not` | `!` | `%0 = !bool false` |

### 语句与终结符

- **赋值 (`Assign`)** — `#[ir_codec(token = "=")]`。

  ```text
  %1 = var x
  ```

- **二元运算 (`BinaryOp`)** — 特殊处理 `binop` token，将运算符置于中间。

  ```text
  %0 = %1 * %2
  ```

- **一元运算 (`UnaryOp`)** — `unop` token 输出 `target = op operand`。

  ```text
  %3 = -num 5
  ```

- **字段访问 (`FieldAccess`)** — `target = object.field`。

  ```text
  %5 = %2.function_ptr
  ```

- **函数调用 (`Call`)** — 当前实现保留结构化写法，方便观察可选 target。

  ```text
  Call { target = %0, function = %4, args = [
      %5,
      %3
  ] }
  ```

- **终结语句 (`Terminator`)** — 核心 token。

  ```text
  goto bb1
  if %1 then bb2 else bb3
  ret %0
  match var tag { ... }
  ```

### 基本块与程序结构

- `Vec<Statement>` 总是使用多行格式，语句之间逗号换行。

  ```text
  statements: [
      %1 = var x,
      %2 = num 2,
      %0 = %1 * %2
  ]
  ```

- `HashMap`/`BTreeMap` 采用 4 空格缩进：

  ```text
  fields = {
      env_ptr: num 0,
      function_ptr: fn lambda$0
  }
  ```

- 顶层 `MirProgram` 使用 `#[ir_codec(program)]`：不再重复结构名，而是直接列出 `functions`, `main_function`, `temp_values` 等字段。

下列片段来自实际 lowering 日志，展示当前完整格式：

```text
functions: {
        main: MirFunction
                name: main
                params: []
                blocks: {
                        bb0: BasicBlock
                                id: bb0
                                statements: [
                                        %1 = Struct { name = Closure, fields = {
                                                    env_ptr: num 0,
                                                    function_ptr: fn lambda$0
                                                    } },
                                        %2 = %1,
                                        %3 = num 5,
                                        %4 = %2.function_ptr,
                                        %5 = %2.env_ptr,
                                        Call { target = %0, function = %4, args = [
                                                    %5,
                                                    %3
                                                    ] }
                                        ]
                                terminator: ret %0
                        }
        }
```

### 跨模块命名示例

项目模式（`--mode project`）会先根据 `karte.mod.toml` 编译依赖模块，再合并为统一的 MIR/LIR。合并后，`functions` 字典中的 key 即为 canonical `module::symbol` 名称，`main_function` 同样持久化该命名方式：

```text
functions: {
        main::main: MirFunction
                name: main::main
                params: []
                blocks: { ... }
        utils::add: MirFunction
                name: utils::add
                params: [Param]
                blocks: { ... }
        }
main_function: main::main
```

- `MirProgram::function_symbols` / `external_function_symbols` 仍会在内存中记录原始别名（例如 `main` → `main::main`，`utils.add` → `utils::add`），但为保持 IR 文本可读性，这两个映射在编码时被 `#[ir_codec(skip)]` 忽略。
- 降到 LIR 后会继承相同的命名策略，JIT 执行器据此定位入口函数与跨模块调用，调试 `karte-tests::cli_integration_tests::test_compile_and_run_project_mode` 可看到完整链路。

## 集合与缩进规则

运行时库在 `karte-ir-codec/src/display.rs` 与 `collections.rs` 中实现了统一的缩进策略。

- `Vec<T>` 总是输出为：

  ```text
  [
      item1,
      item2,
      item3
  ]
  ```

  每个元素缩进 4 空格，元素本身若含多行，会通过 `write_multiline_suffix` 继续缩进。

- `BTreeMap<K, V>` 与 `Vec` 相同的缩进，元素之间使用单换行。

- `HashMap<K, V>` 采用同样缩进，但元素之间留空行，便于区分不同条目。

- `Option<T>`：`Some` 直接展示内部值，`None` 输出 `none`。

## 派生属性速查

| 属性 | 作用 |
|------|------|
| `#[ir_codec(token = "...")]` | 指定枚举变体或新类型的展示 token（可用于中缀/前缀运算、短名称）。 |
| `#[ir_codec(args)]` | 标记字段为“位置参数”，配合 token 生成中缀或指令式输出。 |
| `#[ir_codec(body)]` | 强制字段使用多行块格式（常用于集合字段）。 |
| `#[ir_codec(label = "...")]` | 覆盖字段在输出中的标签，例如将 `basic_blocks` 显示为 `blocks`。 |
| `#[ir_codec(skip)]` | 从输出与解析中移除字段（如 `Span`、缓存、计数器）。 |
| `#[ir_codec(program)]` | 用于顶层 Program 结构，省略外层类型名，直接展开核心字段。 |
| `#[ir_codec(extra)]` | 把字段排除在主序列化结构之外，可用于调试附加信息。 |

更多实现细节可参见 `karte-ir-derive/src/display.rs` 与 `karte-ir-derive/src/parse.rs`。

## 调试与测试建议

- 为 IR 数据结构编写往返测试。

  ```rust
  #[test]
  fn test_value_roundtrip() {
      let originals = [
          Value::Number { value: 42 },
          Value::Variable { name: "x".into() },
          Value::Boolean { value: false },
      ];

      for value in originals {
          let text = value.to_ir_string();
          let parsed = Value::parse_ir(&text).unwrap();
          assert_eq!(parsed, value);
      }
  }
  ```

- 使用 `cargo test -p karte-mir` 运行 MIR 显示/解析测试，可覆盖多种值与终结语句格式。

- 需要检查具体字符串时，可在测试中启用 `-- --nocapture` 或临时打印输出。

## 相关资源

- [快速开始](./IR_CODEC_QUICK_START.md)
- [crate README](../karte-ir-codec/README.md)
- [示例代码](../karte-ir-codec/examples/)
- MIR 定义与格式实现：`karte-mir/src/ir.rs`
- 集合格式化实现：`karte-ir-codec/src/display.rs`, `karte-ir-codec/src/collections.rs`

以上内容即当前项目使用的权威格式说明，后续若有格式调整，请同步更新本文档并在 Quick Start/README 中添加引用链接。

