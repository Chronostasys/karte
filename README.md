# Karte 编程语言

Karte 是一个使用 Rust 实现的函数式编程语言编译器。这个项目是一个多模块的 Rust workspace，包含了完整的编译器前端和后端实现。

## 📈 最新架构优化：连续内存JIT编译

### 🚀 架构升级亮点

我们重新设计了JIT内存管理架构，实现了重大性能和简化改进：

#### **连续内存分配策略**
- **预分配大块内存**: 启动时预分配128MB连续虚拟地址空间
- **按需提交物理内存**: 只有在实际使用时才提交物理页面，节省内存
- **函数紧密排列**: 所有函数在连续地址空间中分配，消除地址分散问题

#### **相对跳转优化**
- **AArch64 BL指令**: 利用±128MB相对跳转范围，无需复杂的绝对地址计算
- **简化函数调用**: 函数间调用使用简单的相对偏移，提高性能
- **缓存友好**: 连续内存布局提高指令缓存命中率

#### **专业内存管理**
```rust
// 新架构示例
let mut memory_manager = JitMemoryManager::new(true);
memory_manager.initialize()?; // 预分配128MB虚拟空间

// 函数分配到连续空间
let exec_mem = memory_manager.allocate_function_memory("main", &machine_code)?;
let relative_offset = memory_manager.calculate_relative_offset("main", "helper")?;
```

#### **技术特性**
- ✅ **虚拟内存保留策略**: `mmap(PROT_NONE)` 保留地址空间，不占用物理内存
- ✅ **按需页面提交**: `mprotect` 动态提交和设置权限
- ✅ **16字节函数对齐**: 优化AArch64指令访问性能
- ✅ **智能偏移计算**: 自动计算函数间相对跳转距离
- ✅ **统计和调试**: 内存使用统计和详细调试日志

#### **性能提升**
- 🔥 **消除地址修补开销**: 无需复杂的跨函数地址解析
- 🔥 **减少TLB压力**: 连续内存布局减少页表查找
- 🔥 **提高缓存效率**: 相邻函数提高I-Cache命中率
- 🔥 **简化代码生成**: AArch64编译器逻辑大幅简化

### 🧪 验证测试

新架构通过了完整的测试验证：
- `test_continuous_memory_architecture`: 连续内存分配和相对偏移计算
- `test_relative_jump_range`: AArch64相对跳转范围验证
- `test_jit_lambda_multiplication`: 端到端JIT执行测试

---

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
- **加法类型 (Sum Types)**：支持代数数据类型和构造器 (`Some(42)`, `None`, `true`, `false`)
- **模式匹配**：支持 match 表达式和模式绑定 (`match expr { pattern -> result }`)
- **布尔类型**：支持布尔字面量，实现为加法类型 (`true`, `false`)
- **控制流**：支持条件表达式和循环表达式 (`if...then...else`, `while...do`)
- **结构体类型 (Product Types)**：支持自定义结构体和字段访问 (`struct Point { x: number, y: number }`)
- **引用类型**：支持不可变引用和显式解引用 (`&x`, `*ref_x`)

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
- **中间表示**：支持MIR和LIR多层级中间表示，控制流转换为基本块
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
├── karte-mir/         # 中级中间表示 (基本块和控制流图)
├── karte-lir/         # 低级中间表示 (类汇编指令)
├── karte-codegen/     # 代码生成器 (解释器)
├── karte-diagnostics/ # 诊断系统
├── karte-cli/         # 命令行工具
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

# 控制流
cargo run -- "if true then 42 else 0"
cargo run -- "while false do 42"

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

// 布尔类型
true
false

// Option 类型 (可选值)
Some(42)    // 有值的选项
None        // 空选项
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

### 控制流

```rust
// if表达式
if true then 42 else 0
if x > 5 then "big" else "small"

// 没有else的if表达式
if condition then side_effect

// while循环
while false do side_effect

// 嵌套控制流
if true then (if false then 1 else 2) else 3
```

### 模式匹配

```rust
// 布尔匹配
match true {
    true -> 1,
    false -> 0
}

// Option 匹配
match Some(42) {
    Some(x) -> x,
    None -> 0
}

// 通配符模式
match Some(42) {
    _ -> 123
}

// 数字模式匹配
match 42 {
    42 -> "found it",
    _ -> "not found"
}
```

### 构造器

```rust
// 无参数构造器
None
true
false

// 有参数构造器
Some(42)
Some(Some(10))  // 嵌套构造器
```

### 结构体类型

```rust
// 结构体定义
struct Point {
    x: number,
    y: number
}

// 带引用字段的结构体
struct RefStruct {
    data: number,
    ref_data: &number
}

// 结构体字面量
let p = Point { x: 10, y: 20 };

// 字段访问
p.x           // 访问 x 字段
p.y           // 访问 y 字段

// 结构体与引用的组合
let value = 42;
let ref_value = &value;
let s = RefStruct { data: 10, ref_data: ref_value };
s.data + *s.ref_data  // 结果: 52
```

### 引用类型

```rust
// 创建引用
let x = 42;
let ref_x = &x;

// 显式解引用
let value = *ref_x;  // 获取引用指向的值

// 引用运算
let y = 10;
let ref_y = &y;
*ref_x + *ref_y     // 结果: 52

// 嵌套引用
let ref_ref_x = &ref_x;
let inner = *ref_ref_x;  // 得到 &number
let final_value = *inner; // 得到 number

// ❌ 错误示例：不能对引用直接运算
ref_x + 10          // 错误：类型不匹配
*42                 // 错误：不能解引用非引用类型
```

## 类型系统

### 支持的类型

- `number`: 数字类型，用于所有数值计算
- `()`: 单元类型，用于语句的返回值
- `fn(T1, T2, ...) -> R`: 函数类型，表示从参数类型到返回类型的映射
- `Bool = True | False`: 布尔类型，由两个构造器组成
- `Option<T> = Some(T) | None`: 可选类型，表示可能有值或无值
- `&T`: 引用类型，表示对类型T的不可变引用
- **自定义结构体**: 用户定义的产品类型 (`struct Name { field1: Type1, field2: Type2 }`)
- **自定义枚举**: 用户定义的加法类型（枚举）

### 自定义类型

Karte 支持定义自己的加法类型（Sum Types），也称为枚举类型：

```rust
// 简单枚举
enum Color { Red, Green, Blue }

// 带数据的枚举
enum Option { Some(number), None }

// 使用自定义类型
{ 
    enum Color { Red, Green, Blue };
    match Red {
        Red -> 1,
        Green -> 2, 
        Blue -> 3
    }
}
```

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

# 运行加法类型和模式匹配测试
cargo test sum_types
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

### 控制流示例

```rust
Input: if true then 42 else 0
Type: number
Result: 42

Input: if false then 1
Type: ()
Result: ()

Input: while false do 42
Type: ()
Result: ()

Input: if true then (if false then 1 else 2) else 3
Type: number
Result: 2
```

### 布尔类型和模式匹配

```rust
Input: true
Type: Bool = True | False
Result: True

Input: match true { true -> 42, false -> 0 }
Type: number
Result: 42
```

### Option 类型

```rust
Input: Some(42)
Type: Option = Some(number) | None
Result: Some(42)

Input: match Some(42) { Some(x) -> x, None -> 0 }
Type: number
Result: 42
```

### 函数式编程风格

```rust
Input: let map_option = |opt, f| match opt { Some(x) -> Some(f(x)), None -> None }; map_option(Some(21), |x| x * 2)
Type: Option = Some(number) | None
Result: Some(42)
```

### 自定义类型示例

```rust
Input: { enum Color { Red, Green, Blue }; Red }
Type: Color = Red | Green | Blue
Result: Red

Input: { enum Option { Some(number), None }; Some(42) }
Type: Option = Some(number) | None
Result: Some(42)

Input: { enum Color { Red, Green, Blue }; match Red { Red -> 1, Green -> 2, Blue -> 3 } }
Type: number
Result: 1

Input: { enum Option { Some(number), None }; match Some(42) { Some(x) -> x + 1, None -> 0 } }
Type: number
Result: 43
```

### 结构体示例

```rust
Input: struct Point { x: number, y: number }; let p = Point { x: 10, y: 20 }; p.x + p.y
Type: number
Result: 30

Input: struct Person { name: number, age: number }; let person = Person { name: 1, age: 25 }; person.age
Type: number
Result: 25
```

### 引用类型示例

```rust
Input: let x = 42; let ref_x = &x; *ref_x
Type: number
Result: 42

Input: let x = 42; let y = 10; let ref_x = &x; let ref_y = &y; *ref_x + *ref_y
Type: number
Result: 52

Input: struct RefStruct { data: number, ref_data: &number }; let x = 42; let ref_x = &x; let s = RefStruct { data: 10, ref_data: ref_x }; s.data + *s.ref_data
Type: number
Result: 52
```

### 错误检测

```rust
Input: let x = 5; y + 10
Error: Undefined variable: y

Input: 5(10)
Error: Cannot call value of type number

Input: let f = |x, y| x + y; f(1)
Error: Arity mismatch: expected 2 arguments, found 1

Input: let x = 42; *x
Error: Type mismatch: expected &?, found number

Input: let x = 42; let ref_x = &x; ref_x + 10
Error: Binary operations are only supported on numbers
```

## 开发状态

- ✅ 词法分析
- ✅ 语法分析  
- ✅ 类型系统
- ✅ 语法检查
- ✅ 代码生成 (解释器)
- ✅ 中间表示 (MIR/LIR)
- ✅ 控制流 (if/while)
- ✅ 结构体类型 (Product Types)
- ✅ 引用类型 (不可变引用与显式解引用)
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
