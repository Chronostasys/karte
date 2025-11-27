# karte-parser

Karte语言的语法解析器，负责将词法分析器（`karte-lexer`）产生的token流转换为抽象语法树（AST）。

## 模块结构

parser模块采用清晰的模块化设计，将不同类型的解析逻辑分离到独立的模块中：

```
karte-parser/
├── src/
│   ├── lib.rs           (636行) - 主模块和公共API
│   ├── types.rs         (206行) - 类型定义
│   ├── module.rs        (340行) - 模块系统解析
│   ├── pattern.rs       (210行) - 模式匹配解析
│   ├── statement.rs     (919行) - 语句和声明解析
│   └── expression.rs   (1720行) - 表达式解析
```

### lib.rs - 主模块

包含：
- Parser结构体定义
- 公共API函数（`parse()`, `parse_with_type_check()`等）
- 工具方法（`peek()`, `advance()`, `recover_from_error()`等）
- 测试用例

### types.rs - 类型定义

定义了所有parser相关的核心类型：
- `ParserMode` - 解析模式（Script/Project）
- `ParseError` - 解析错误类型
- `ParsedProgram` - 解析结果
- `ModuleDecl`, `ImportDecl` - 模块和导入声明
- `ImportSymbol`, `ImportSpecifier` - 导入符号

### module.rs - 模块系统解析

负责解析Karte的模块系统语法：
- `module` 声明：`module utils.math`
- `import` 声明：`import utils.math::{add, multiply as mul}`
- 模块符号访问：`utils.math::add`

支持两种解析模式：
- **Script模式**：允许顶层表达式和语句，适合REPL和单文件脚本
- **Project模式**：顶层只允许声明，需要显式的模块声明和main函数

### pattern.rs - 模式匹配解析

解析match表达式中的模式：
- 通配符模式：`_`
- 字面量模式：`42`, `true`, `false`
- 变量绑定：`x`
- 构造器模式：`Some(x)`, `None`
- 限定构造器：`Option::Some(x)`

### statement.rs - 语句和声明解析

解析所有类型的语句和声明：
- `let` 语句：`let x = 42;`
- 函数定义：`fn add(x: number, y: number) -> number { x + y }`
- 结构体定义：`struct Point { x: number, y: number }`
- 枚举定义：`enum Option { Some(number), None }`
- 块表达式：`{ stmt1; stmt2; expr }`

### expression.rs - 表达式解析

采用递归下降解析器实现，支持完整的运算符优先级：

#### 运算符优先级（从低到高）

1. **赋值** (`=`) - 右结合
2. **逻辑或** (`||`)
3. **逻辑与** (`&&`)
4. **比较** (`==`, `!=`, `<`, `>`, `<=`, `>=`)
5. **加减** (`+`, `-`)
6. **乘除** (`*`, `/`, `%`)
7. **一元** (`-`, `!`, `*`, `&`)
8. **后缀** (函数调用、字段访问、数组索引)
9. **主表达式** (字面量、标识符、括号表达式等)

#### 支持的表达式类型

- **字面量**：数字、布尔值、字符串
- **运算符**：算术、逻辑、比较、位运算
- **控制流**：`if`-`else`, `while`, `match`
- **函数调用**：`foo(x, y)`
- **Lambda表达式**：`|x, y| x + y`
- **结构体字面量**：`Point { x: 1, y: 2 }`
- **数组字面量**：`[1, 2, 3]`
- **字段访问**：`point.x`
- **数组索引**：`arr[i]`
- **引用操作**：`&x`, `*ptr`
- **内存管理**：`box x`, `arc x`, `free ptr`, `retain ptr`, `release ptr`

## 使用示例

### 基本解析

```rust
use karte_parser::{parse, ParserMode};
use karte_lexer::lex;

let source = "let x = 42; x + 10";
let tokens = lex(source);
let mut parser = karte_parser::Parser::new(&tokens)
    .with_mode(ParserMode::Script);

if let Some(program) = parser.parse() {
    println!("解析成功：{:?}", program.body);
} else {
    eprintln!("解析错误：{:?}", parser.diagnostics());
}
```

### 项目模式解析

```rust
use karte_parser::{parse_program_with_metadata, ParserMode};
use karte_lexer::lex;

let source = r#"
module main

fn main() -> number {
    let x = 42;
    x * 2
}
"#;

let tokens = lex(source);
let (program_opt, diagnostics) = parse_program_with_metadata(&tokens, ParserMode::Project);

if let Some(program) = program_opt {
    println!("模块：{:?}", program.module);
    println!("主体：{:?}", program.body);
}
```

### 错误恢复

Parser实现了错误恢复机制，可以在遇到语法错误后继续解析：

```rust
let mut parser = Parser::new(&tokens);
let exprs = parser.parse_with_recovery();

for expr in exprs {
    println!("解析的表达式：{:?}", expr);
}

// 查看所有诊断信息
for diagnostic in parser.diagnostics().all() {
    eprintln!("{}", diagnostic);
}
```

## 设计特点

### 1. 模块化设计

将3900+行的单一文件拆分为6个职责清晰的模块，每个模块专注于特定的解析任务：
- 类型定义与解析逻辑分离
- 按语法结构组织（模块、模式、语句、表达式）
- 便于维护和扩展

### 2. 完整的错误报告

- 使用`DiagnosticBag`统一管理错误和警告
- 每个错误都包含精确的源码位置（`Span`）
- 提供清晰的错误消息和期望的token信息

### 3. 两种解析模式

**Script模式**：
- 允许顶层语句和表达式
- 自动包装在隐式的main函数中
- 适合REPL和快速脚本

**Project模式**：
- 顶层只允许声明
- 必须有显式的模块声明
- 需要定义main函数
- 适合多模块项目

### 4. 丰富的语法支持

- 完整的运算符优先级
- 模式匹配（`match`表达式）
- Lambda表达式和高阶函数
- 结构体和枚举
- 引用类型和内存管理
- 模块系统和导入

## 测试

Parser模块包含全面的测试用例：

```bash
# 运行parser单元测试
cargo test -p karte-parser

# 运行包含parser的集成测试
cargo test -p karte-tests
```

测试覆盖：
- 赋值表达式和链式赋值
- 字段赋值语法
- 赋值优先级
- 模块符号访问
- 项目模式约束
- 脚本模式语法

## 与其他模块的交互

```
┌─────────────┐
│ karte-lexer │ - 词法分析
└──────┬──────┘
       │ tokens
       ↓
┌──────────────┐
│ karte-parser │ - 语法分析
└──────┬───────┘
       │ AST
       ↓
┌─────────────┐
│  karte-hir  │ - 类型检查和语义分析
└─────────────┘
```

Parser依赖：
- `karte-lexer` - 提供Token和词法分析
- `karte-diagnostics` - 错误报告
- `karte-hir` - AST类型定义和类型检查

被依赖：
- `karte-cli` - 命令行工具
- `karte-tests` - 集成测试
- `karte-module-system` - 模块编译

## 性能特性

- 单次遍历解析（single-pass parsing）
- 递归下降算法，时间复杂度O(n)
- 惰性错误恢复，不影响正常路径性能
- 零拷贝token引用（借用词法分析器的token）

## 未来改进

- [ ] 增量解析支持
- [ ] 更精细的错误恢复策略
- [ ] 语法高亮信息导出
- [ ] LSP协议支持（用于IDE集成）
