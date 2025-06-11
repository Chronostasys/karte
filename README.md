# Karte 编程语言

Karte 是一个使用 Rust 实现的函数式编程语言编译器。这个项目是一个多模块的 Rust workspace，包含了完整的编译器前端和后端实现。

## 功能特性

### 语言特性

- **基础数据类型**：支持数字类型 (`number`) 和单元类型 (`()`)
- **四则运算**：支持加法、减法、乘法、除法运算 (`+`, `-`, `*`, `/`)
- **一元运算**：支持正号和负号 (`+x`, `-x`)
- **变量系统**：支持 Rust 风格的 let 语句 (`let x = 5;`)
- **Lambda 表达式**：支持匿名函数 (`|x| x + 1`, `|x, y| x * y`)
- **函数调用**：支持函数调用和闭包 (`f(1, 2)`, `(|x| x + 1)(5)`)
- **语句序列**：支持多语句程序 (`;` 分隔)
- **块表达式**：支持嵌套作用域和变量绑定

### 编译器特性

- **词法分析**：完整的分词器，支持所有语言特性
- **语法分析**：递归下降解析器，支持运算符优先级
- **类型系统**：静态类型检查，包括：
  - 基础类型检查 (`number`, `()`, `fn(...) -> ...`)
  - 未定义变量检测
  - 函数参数数量检查 (arity checking)
  - 类型兼容性检查
- **语法检查**：编译时错误检测和诊断
- **代码生成**：AST 解释器，支持运行时求值
- **诊断系统**：结构化的错误报告和警告

### 工具链

- **CLI 工具**：命令行解释器，支持表达式计算和交互模式
- **诊断输出**：详细的错误信息和类型信息显示

## 项目结构

```
karte/
├── karte-lexer/       # 词法分析器
├── karte-parser/      # 语法分析器 + 类型检查器
├── karte-hir/         # 高级中间表示 + 类型系统
├── karte-codegen/     # 代码生成器 (解释器)
├── karte-diagnostics/ # 诊断系统
├── karte-cli/         # 命令行工具
├── karte-mir/         # 中级中间表示 (未实现)
├── karte-lir/         # 低级中间表示 (未实现)
└── karte-lsp/         # 语言服务器协议 (未实现)
```

## 快速开始

### 编译项目

```bash
cargo build --release
```

### 运行示例

```bash
# 基础四则运算
cargo run -- "1 + 2 * 3"

# 变量和函数
cargo run -- "let x = 5; let f = |y| x + y; f(10)"

# 复杂表达式
cargo run -- "let x = 3; let y = 4; let multiply = |a, b| a * b; multiply(x, y)"

# 交互模式
cargo run
```

## 语言语法

### 数据类型

```rust
// 数字
42
-17

// 单元类型 (语句返回值)
let x = 5;  // 返回 ()
```

### 变量和语句

```rust
// let 语句
let x = 42;
let y = x + 8;

// 语句序列
let a = 1; let b = 2; a + b
```

### 函数和Lambda

```rust
// 单参数 lambda
let double = |x| x * 2;

// 多参数 lambda
let add = |x, y| x + y;

// 函数调用
double(5)     // 结果: 10
add(3, 4)     // 结果: 7

// 立即调用的函数表达式 (IIFE)
(|x| x + 1)(5)  // 结果: 6
```

### 表达式和运算

```rust
// 四则运算
1 + 2 - 3 * 4 / 2

// 一元运算
-x + (+y)

// 带括号的表达式
(1 + 2) * (3 - 4)
```

## 类型系统

### 支持的类型

- `number`: 数字类型，用于所有数值计算
- `()`: 单元类型，用于语句的返回值
- `fn(T1, T2, ...) -> R`: 函数类型，表示从参数类型到返回类型的映射

### 类型检查功能

```rust
// ✅ 正确的类型使用
let x = 42;          // x: number
let f = |y| y + 1;   // f: fn(number) -> number
f(x)                 // 调用正确: number

// ❌ 类型错误
let x = 42;
x(5)                 // 错误: Cannot call value of type number

// ❌ 未定义变量
y + 10               // 错误: Undefined variable: y

// ❌ 参数数量不匹配
let f = |x, y| x + y;  // f: fn(number, number) -> number
f(1)                   // 错误: Arity mismatch: expected 2 arguments, found 1
```

## 测试

```bash
# 运行所有测试
cargo test

# 运行特定模块测试
cargo test -p karte-lexer
cargo test -p karte-parser
cargo test -p karte-hir
cargo test -p karte-codegen

# 运行类型检查测试
cargo test test_type_check
```

## 示例程序

### 基础计算

```rust
Input: let x = 5; x + 10
Type: number
Result: 15
```

### 函数定义和调用

```rust
Input: let f = |x| x * 2; f(7)
Type: number  
Result: 14
```

### 复杂程序

```rust
Input: let x = 3; let y = 4; let multiply = |a, b| a * b; multiply(x, y)
Type: number
Result: 12
```

### 错误检测

```rust
Input: let x = 5; y + 10
Error: Undefined variable: y

Input: 5(10)
Error: Cannot call value of type number

Input: let f = |x, y| x + y; f(1)
Error: Arity mismatch: expected 2 arguments, found 1
```

## 开发状态

- ✅ 词法分析
- ✅ 语法分析  
- ✅ 类型系统
- ✅ 语法检查
- ✅ 代码生成 (解释器)
- ✅ 诊断系统
- ✅ CLI 工具
- ⏳ 语言服务器 (LSP)
- ⏳ 编译到机器码

## 技术栈

- **语言**: Rust 2021
- **依赖**:
  - `serde` - 序列化支持
  - `thiserror` - 错误处理
  - `env_logger` - 日志系统
  - `log` - 日志接口

## 贡献

欢迎提交 Issue 和 Pull Request！

## 许可证

MIT License
