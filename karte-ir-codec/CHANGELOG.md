# 更新日志

## 2025-10-21 - 初始版本

### 新增功能

1. **核心 Trait 系统**
   - `IrDisplay`: 自定义序列化 trait，将 IR 转换为可读文本
   - `IrParse`: 自定义反序列化 trait，从文本解析回 IR
   - 基于 `nom` 的强大组合子解析

2. **自动化 Proc Macro**
   - `#[derive(IrCodec)]`: 自动生成 `IrDisplay` 和 `IrParse` 实现
   - 智能字段跳过：自动跳过 `Span` 类型字段
   - `#[ir_codec(skip)]`: 手动标记要跳过的字段
   - 大型 enum 支持：自动分组处理超过 20 个变体的枚举

3. **集合类型支持**
   - `BTreeMap<K, V>` - 格式: `{key = value, key = value}`
   - `HashMap<K, V>` - 格式: `{key = value, key = value}`
   - 元组 `(T1, T2)`, `(T1, T2, T3)` 等
   - `Vec<T>`, `Option<T>`, `Box<T>`

4. **基础类型实现**
   - 数值类型: `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, `usize`
   - 布尔和字符串: `bool`, `String`, `&str`
   - 智能指针: `Box<T>`, `Option<T>`

### 格式规范

- **Enum 变体格式**:
  - 单位变体: `VariantName`
  - 单字段命名变体: `VariantName(value)`
  - 多字段命名变体: `VariantName { field1 = value1 field2 = value2 }`
  - 元组变体: `VariantName(arg1, arg2)`

- **Struct 格式**:
  - 命名结构体: `{ field1 = value1 field2 = value2 }`
  - 元组结构体: `StructName(value1, value2)`

### 应用范围

- ✅ **MIR (Mid-level IR)**: 所有类型已启用，19 个测试通过
- ✅ **LIR (Low-level IR)**: 所有类型已启用，12 个 codec 测试 + 22 个单元测试通过
- ✅ **诊断系统**: `Span` 类型支持
- ✅ **调用约定**: `Register` 类型支持

### 示例

```rust
use karte_ir_derive::IrCodec;
use karte_ir_codec::{IrDisplay, IrParse};

#[derive(Debug, Clone, PartialEq, IrCodec)]
pub enum Value {
    Number { value: i64 },
    Boolean { value: bool },
    Variable { name: String },
    Struct { 
        name: String, 
        fields: BTreeMap<String, Value> 
    },
}

// 使用
let value = Value::Number { value: 42 };
let text = value.to_ir_string(); // "Number(42)"
let parsed = Value::parse_ir(&text).unwrap(); // 完美还原
assert_eq!(value, parsed);
```

### 性能优化

- 零拷贝解析：使用 `&str` 切片，无需额外分配
- 懒加载：只在需要时才解析字段
- 编译时代码生成：无运行时反射开销

### 未来计划

- [ ] 支持更多标准库类型（HashSet, LinkedList 等）
- [ ] 自定义格式化选项（缩进、换行等）
- [ ] 性能基准测试
- [ ] 更详细的错误消息
- [ ] 支持循环引用检测

