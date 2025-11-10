# 数学表达式风格格式化

## 概述

IrCodec 支持使用 `#[ir_codec(token = "...")]` 和 `#[ir_codec(args)]` 属性将 enum 变体格式化为数学表达式风格，大幅提升可读性。

## 基础用法

### 中缀表达式（2 个参数）

当变体有 2 个标记为 `args` 的字段时，自动生成中缀表达式。

```rust
use karte_ir_derive::IrCodec;

#[derive(Debug, Clone, IrCodec)]
enum Expr {
    #[ir_codec(token = "+")]
    Add {
        #[ir_codec(args)]
        left: Box<Expr>,
        #[ir_codec(args)]
        right: Box<Expr>,
    },
    
    #[ir_codec(token = "*")]
    Mul {
        #[ir_codec(args)]
        left: Box<Expr>,
        #[ir_codec(args)]
        right: Box<Expr>,
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
assert_eq!(expr.to_ir_string(), "Number(1) + Number(2)");

// (x + 1) * 2
let expr = Expr::Mul {
    left: Box::new(Expr::Add {
        left: Box::new(Expr::Variable { name: "x".to_string() }),
        right: Box::new(Expr::Number { value: 1 }),
    }),
    right: Box::new(Expr::Number { value: 2 }),
};
// 输出: Variable(x) + Number(1) * Number(2)
```

### 前缀表达式（1 个参数）

当变体有 1 个标记为 `args` 的字段时，生成前缀表达式。

```rust
#[derive(Debug, Clone, IrCodec)]
enum Expr {
    #[ir_codec(token = "-")]
    Neg {
        #[ir_codec(args)]
        operand: Box<Expr>,
    },
    
    #[ir_codec(token = "!")]
    Not {
        #[ir_codec(args)]
        operand: Box<Expr>,
    },
    
    Number { value: i64 },
}
```

**输出示例：**
```rust
// -5
let expr = Expr::Neg {
    operand: Box::new(Expr::Number { value: 5 }),
};
assert_eq!(expr.to_ir_string(), "- Number(5)");

// !true
let expr = Expr::Not {
    operand: Box::new(Expr::Bool { value: true }),
};
assert_eq!(expr.to_ir_string(), "! Bool(true)");
```

### 常量符号（0 个参数）

当变体没有参数但有 token 时，直接显示符号。

```rust
#[derive(Debug, Clone, IrCodec)]
enum Token {
    #[ir_codec(token = "+")]
    Plus,
    
    #[ir_codec(token = "*")]
    Star,
    
    #[ir_codec(token = "==")]
    Equals,
}
```

**输出：** `+`, `*`, `==`

## 完整示例

### 带有额外字段的表达式

```rust
#[derive(Debug, Clone, IrCodec)]
enum Statement {
    #[ir_codec(token = "=")]
    Assign {
        target: String,
        #[ir_codec(args)]
        value: Expr,
        #[ir_codec(skip)]
        span: Span,  // 自动跳过
    },
}
```

**输出：** `target = value`（span 被跳过）

### 运算符定义

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, IrCodec)]
pub enum BinaryOperator {
    #[ir_codec(token = "+")]
    Add,
    #[ir_codec(token = "-")]
    Sub,
    #[ir_codec(token = "*")]
    Mul,
    #[ir_codec(token = "/")]
    Div,
    #[ir_codec(token = "%")]
    Mod,
    #[ir_codec(token = "==")]
    Eq,
    #[ir_codec(token = "!=")]
    Ne,
    #[ir_codec(token = "<")]
    Lt,
    #[ir_codec(token = "<=")]
    Le,
    #[ir_codec(token = ">")]
    Gt,
    #[ir_codec(token = ">=")]
    Ge,
    #[ir_codec(token = "&&")]
    And,
    #[ir_codec(token = "||")]
    Or,
}
```

**输出：** `+`, `-`, `*`, `/`, `%`, `==`, `!=`, `<`, `<=`, `>`, `>=`, `&&`, `||`

## 对比

### 优化前（冗长）
```
Add { left = Number { value = 1 }, right = Number { value = 2 } }
Neg { operand = Number { value = 5 } }
BinaryOp { op = Add, left = x, right = y }
```

### 优化后（简洁）
```
Number(1) + Number(2)
- Number(5)
x + y
```

## 注意事项

1. **参数数量**：
   - 0 个 `args`: 显示 token
   - 1 个 `args`: 前缀表达式
   - 2 个 `args`: 中缀表达式
   - 3+ 个 `args`: 回退到默认格式

2. **与其他属性组合**：
   - `#[ir_codec(skip)]` 与 `token/args` 兼容
   - `#[ir_codec(body)]` 不应与 `args` 同时使用
   - `#[ir_codec(label)]` 可以与 `args` 组合

3. **括号**：
   - 框架不会自动添加括号
   - 需要在解析或显示时手动处理优先级

## 最佳实践

### 1. 为所有运算符定义 token

```rust
#[derive(IrCodec)]
pub enum UnaryOp {
    #[ir_codec(token = "!")]
    Not,
    #[ir_codec(token = "-")]
    Neg,
    #[ir_codec(token = "+")]
    Plus,
}
```

### 2. 一致性

保持同类运算符的格式一致：
- 算术运算：`+`, `-`, `*`, `/`
- 比较运算：`==`, `!=`, `<`, `>`
- 逻辑运算：`&&`, `||`, `!`

### 3. 可读性优先

选择最接近数学或编程语言习惯的符号：
- ✅ `+`, `-`, `*`, `/`
- ✅ `==`, `!=`, `<`, `>`
- ❌ `ADD`, `SUB` (太冗长)

## 实际应用

### MIR 中的应用

```rust
// karte-mir/src/ir.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, IrCodec)]
pub enum BinaryOperator {
    #[ir_codec(token = "+")]
    Add,
    #[ir_codec(token = "-")]
    Sub,
    // ... 其他运算符
}

// 使用
let stmt = Statement::BinaryOp {
    dst: temp1,
    op: BinaryOperator::Add,
    left: Value::Number { value: 1 },
    right: Value::Number { value: 2 },
    span: Default::default(),
};

// 输出可以是: temp1 = 1 + 2
```

### LIR 中的应用

```rust
// 指令可以显示为: r1 = r2 + r3
// 而不是: Add { dst: r1, src1: r2, src2: r3 }
```

## 未来增强

计划支持：
- 后缀表达式
- 自定义优先级和括号
- 多行格式化
- 自定义分隔符

