# Karte 语言模式设计文档

## 1. 概述

为了支持从脚本语言向系统级编译型语言的演进，Karte 将引入 **"区分模式" (Distinguished Mode)** 设计。该设计旨在兼顾脚本开发的灵活性与大型工程的结构化需求。

## 2. 两种模式

### 2.1 标准模式 (Project/Build Mode)

这是构建大型项目时的默认模式，类似于 Rust、Go 或 C。

*   **适用场景**：`karte build`，生产环境项目。
*   **语法约束**：
    *   源文件顶层 **只能** 包含声明 (`fn`, `struct`, `enum`, `const`, `let` 常量)。
    *   **禁止** 顶层游离的执行语句（如 `print("hello")`, `1 + 2`）。
    *   必须显式定义入口函数 `fn main() -> ()` 或 `fn main() -> i32`。
*   **优势**：
    *   **声明提升 (Hoisting)**：编译器可预先扫描所有符号，支持函数相互递归调用，无需关注定义顺序。
    *   **模块安全**：模块加载无副作用，依赖关系清晰。
    *   **优化友好**：明确的入口和调用图便于全程序优化 (LTO)。

### 2.2 脚本模式 (Script/REPL Mode)

这是快速原型开发、REPL 交互或编写胶水脚本时的模式，类似于 Python 或 TypeScript。

*   **适用场景**：`karte run`，REPL，单文件脚本。
*   **语法行为**：
    *   允许顶层出现执行语句。
    *   允许顶层定义函数。
    *   **编译器行为**：编译器会将顶层的所有语句和表达式隐式包装进一个 `main` 函数中。顶层定义的函数会被提升或作为闭包处理（具体实现取决于作用域设计，初期可作为提升处理）。
*   **优势**：
    *   低样板代码 (Low Boilerplate)，"Hello World" 仅需一行。
    *   直观，适合教学和测试。

## 3. 实施路线图

### 阶段一：语法层支持 (Current Focus)

> **Status (2025-11-23)**: 
> * Parser 已支持顶层 `fn` 定义与隐式 `main` 包装。
> * JIT 编译器已修复针对隐式 `main` 的返回值处理问题（解决 `EXC_BAD_ACCESS`）。
> * `karte run` 现可正确执行包含顶层语句和函数定义的脚本。
> * CLI 已集成 `--mode` 参数，支持显式指定 `script` 或 `project` 模式。

目标：让 Parser 能够解析顶层的 `fn` 定义。

1.  **AST 扩展**：在 HIR 中引入 `FunctionDef` 节点。
2.  **Parser 升级**：
    *   在 `parse_program` 中识别 `fn` 关键字。
    *   实现 `parse_function_definition`。
    *   支持 `fn main()` 作为显式入口。

### 阶段二：语义层区分

目标：根据编译上下文决定是否允许顶层语句。

1.  **CLI 开关**：引入 `--mode=script|project` 或根据命令 (`run` vs `build`) 自动切换。
2.  **验证逻辑**：在 Project 模式下，如果发现顶层有非声明语句，报错并提示用户移入 `main` 函数。

### 阶段三：构建系统集成

目标：`karte build` 自动寻找 `main` 符号。

1.  **Entry Point Detection**：链接器/构建器扫描所有模块，寻找 `main` 函数。
2.  **Implicit Main Wrapper**：在 Script 模式下，自动生成 AST 包装层。

## 4. 示例

### 标准模式 (main.karte)

```rust
// 顶层只能是声明
fn add(a: i32, b: i32) -> i32 {
    a + b
}

struct Point {
    x: i32,
    y: i32
}

// 显式入口
fn main() {
    let p = Point { x: 1, y: 2 };
    let sum = add(p.x, p.y);
    // print(sum);
}
```

### 脚本模式 (script.karte)

```rust
// 混合写法
fn square(x: i32) -> i32 {
    x * x
}

// 顶层语句
let result = square(5);
// print(result);
```
