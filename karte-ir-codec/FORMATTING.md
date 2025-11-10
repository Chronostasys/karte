# IR 格式化指南

## 概述

IrCodec 提供了强大的格式化选项，让 IR 输出更美观、易读。本文档介绍所有可用的格式化属性和最佳实践。

## 格式化属性

### `#[ir_codec(token = "...")]`

将 enum 变体用符号表示，支持数学表达式风格。

```rust
#[derive(IrCodec)]
enum Expr {
    #[ir_codec(token = "+")]
    Add {
        #[ir_codec(args)]
        left: Box<Expr>,
        #[ir_codec(args)]
        right: Box<Expr>,
    },
    
    #[ir_codec(token = "-")]
    Neg {
        #[ir_codec(args)]
        operand: Box<Expr>,
    },
    
    Number { value: i64 },
}
```

**输出示例：**
```rust
// 1 + 2
let expr = Expr::Add {
    left: Box::new(Expr::Number { value: 1 }),
    right: Box::new(Expr::Number { value: 2 }),
};
println!("{}", expr.to_ir_string());
// 输出: Number(1) + Number(2)

// -5
let expr = Expr::Neg {
    operand: Box::new(Expr::Number { value: 5 }),
};
println!("{}", expr.to_ir_string());
// 输出: - Number(5)
```

**规则：**
- **2 个 #[ir_codec(args)]**: 中缀表达式 `left token right`
- **1 个 #[ir_codec(args)]**: 前缀表达式 `token arg`
- **0 个 args**: 直接显示 token

### `#[ir_codec(args)]`

标记字段为参数，配合 `token` 使用生成数学表达式。

```rust
#[derive(IrCodec)]
enum BinaryOp {
    #[ir_codec(token = "*")]
    Mul {
        #[ir_codec(args)]
        left: Value,
        #[ir_codec(args)]
        right: Value,
    },
}
```

**输出：** `left * right` 而不是 `Mul { left = ..., right = ... }`

### `#[ir_codec(body)]`

将字段换行显示，适用于大型集合（如指令列表、基本块映射）。

```rust
#[derive(IrCodec)]
pub struct LirFunction {
    pub name: String,
    #[ir_codec(body, label = "body")]  // 换行显示
    pub instructions: Vec<Instruction>,
    pub parameter_count: usize,
}
```

**输出示例：**
```
LirFunction
  name: lambda$0
  body: [
    Label(LabelId(1)),
    Move( dst: Virtual(5), src: Register(Virtual(2))),
    Mul( dst: Virtual(6), src1: Register(Virtual(5)), src2: Immediate(2)),
    Return(Virtual(6))
  ]
  params: 2
```

### `#[ir_codec(label = "xxx")]`

自定义字段标签，使输出更易读。

```rust
#[derive(IrCodec)]
pub struct MirFunction {
    pub name: String,
    #[ir_codec(label = "params")]
    pub params: Vec<String>,
    #[ir_codec(body, label = "blocks")]
    pub basic_blocks: HashMap<BasicBlockId, BasicBlock>,
}
```

**输出示例：**
```
MirFunction
  name: factorial
  params: [n]
  blocks: {
    BasicBlockId(0) = BasicBlock { ... }
  }
```

### `#[ir_codec(skip)]`

跳过字段序列化，用于内部状态或缓存。

```rust
#[derive(IrCodec)]
pub struct LirFunction {
    pub name: String,
    pub instructions: Vec<Instruction>,
    #[ir_codec(skip)]
    pub next_register: usize,  // 内部计数器，不序列化
}
```

### `#[ir_codec(compact)]`

使用紧凑格式（未来功能）。

### `#[ir_codec(newline_items)]`

集合元素换行显示（未来功能）。

## 自动格式化规则

### Vec 自动格式化

- **≤3 元素**：紧凑格式 `[a, b, c]`
- **>3 元素**：换行格式
  ```
  [
    item1,
    item2,
    item3,
    item4
  ]
  ```

### Enum 变体格式化

- **单位变体**：`VariantName`
- **单字段变体**：`VariantName(value)`
- **多字段变体（紧凑）**：`VariantName( field1: v1, field2: v2)`
- **多字段变体（换行）**：
  ```
  VariantName
    field1: value1
    field2: value2
  ```

### Struct 格式化

- **无 body 字段**：紧凑格式 `StructName(field1: v1, field2: v2)`
- **有 body 字段**：换行格式
  ```
  StructName
    field1: value1
    field2: value2
  ```

## 最佳实践

### 1. 为大型集合使用 `body` 标签

```rust
#[derive(IrCodec)]
pub struct Program {
    pub name: String,
    #[ir_codec(body, label = "functions")]
    pub functions: HashMap<String, Function>,
}
```

### 2. 跳过内部状态字段

```rust
#[derive(IrCodec)]
pub struct Function {
    pub name: String,
    pub instructions: Vec<Instruction>,
    #[ir_codec(skip)]
    pub next_temp_id: usize,  // 生成器状态
    #[ir_codec(skip)]
    pub span: Span,           // 源码位置（自动跳过）
}
```

### 3. 使用自定义标签增强可读性

```rust
#[derive(IrCodec)]
pub struct FunctionDef {
    pub name: String,
    #[ir_codec(label = "args")]
    pub parameters: Vec<String>,
    #[ir_codec(body, label = "body")]
    pub instructions: Vec<Instruction>,
}
```

**输出：**
```
FunctionDef
  name: add
  args: [a, b]
  body: [
    Add(dst: r0, src1: r1, src2: r2),
    Return(r0)
  ]
```

## 完整示例

### 输入代码：

```rust
#[derive(Debug, Clone, PartialEq, IrCodec)]
pub struct LirFunction {
    pub name: String,
    #[ir_codec(body, label = "body")]
    pub instructions: Vec<Instruction>,
    #[ir_codec(label = "params")]
    pub parameter_count: usize,
    #[ir_codec(skip)]
    pub next_register: usize,
    #[ir_codec(skip)]
    pub struct_types: HashMap<StructTypeId, StructLayout>,
}
```

### 输出效果：

```
LirFunction
  name: fibonacci
  body: [
    Label(LabelId(0)),
    Move( dst: Virtual(1), src: Immediate(0)),
    Move( dst: Virtual(2), src: Immediate(1)),
    Loop(LabelId(1)),
    Add( dst: Virtual(3), src1: Register(Virtual(1)), src2: Register(Virtual(2))),
    Move( dst: Virtual(1), src: Register(Virtual(2))),
    Move( dst: Virtual(2), src: Register(Virtual(3))),
    JumpIfLess( cond: Virtual(4), target: LabelId(1)),
    Return(Virtual(2))
  ]
  params: 1
```

## 对比：优化前后

### 优化前（丑陋）：
```
{ name = lambda$0 instructions = [Label(LabelId(1)), Move { dst = Virtual(5) src = Register(Virtual(2)) }, Mul { dst = Virtual(6) src1 = Register(Virtual(5)) src2 = Immediate(2) }, Return(Virtual(6))] next_register = 11 struct_types = {} stack_frame_size = 0 parameter_count = 2 parameter_registers = [Virtual(1), Virtual(2)] }
```

### 优化后（美观）：
```
LirFunction
  name: lambda$0
  body: [
    Label(LabelId(1)),
    Move( dst: Virtual(5), src: Register(Virtual(2))),
    Mul( dst: Virtual(6), src1: Register(Virtual(5)), src2: Immediate(2)),
    Return(Virtual(6))
  ]
  params: 2
```

## 未来扩展

计划添加的格式化选项：

- `#[ir_codec(inline_threshold = 5)]` - 自定义内联阈值
- `#[ir_codec(indent = 4)]` - 自定义缩进空格数
- `#[ir_codec(separator = " | ")]` - 自定义分隔符
- `#[ir_codec(multiline)]` - 强制多行显示
- `#[ir_codec(custom_format = "{}:")]` - 自定义格式字符串

