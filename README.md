# Karte 编程语言

Karte 是一个使用 Rust 实现的函数式编程语言编译器，完整实现了从词法分析到 JIT 代码生成的编译器管线。

## 平台支持

⚠️ **目前仅支持 AArch64 架构**（如 Apple Silicon M1/M2/M3 和 ARM Linux）

## 快速开始

### 编译项目

```bash
cargo build
```

### 基础使用

```bash
# 脚本模式 - 快速执行表达式
cargo run -p karte-cli -- run "let x = 5; x + 10"

# 项目模式 - 多模块项目（需要 karte.mod.toml）
cargo run -p karte-cli -- run --mode project test_project/src/main.karte

# 更多选项
cargo run -p karte-cli -- run --verbose "let x = 42; x * 2"
cargo run -p karte-cli -- run --optimization aggressive examples/demo.karte
cargo run -p karte-cli -- run --emit-lir examples/demo.karte
```

## 语言特性

- **基础类型**：`number`、`()` (单元类型)
- **代数数据类型**：Sum types (枚举) 和 Product types (结构体)
- **模式匹配**：完整的 match 表达式支持
- **Lambda 表达式**：一等函数和闭包
- **控制流**：`if-then-else` 和 `while-do` 表达式
- **引用类型**：不可变引用 `&T` 和显式解引用
- **多模块系统**：基于 `karte.mod.toml` 的项目组织和增量编译缓存

## 高级特性

- **逃逸分析**：编译时分析变量生命周期，自动决定栈或堆分配
- **Immix GC**：高性能垃圾回收，基于 32KB 块结构的标记-清扫算法
- **Effect 系统**：支持副作用追踪和控制
- **JIT 编译**：AArch64 连续内存 JIT，相对跳转优化，±128MB 寻址范围

## 示例

```karte
let add = |x, y| x + y;
add(10, 20)
```

## 测试

```bash
# 运行所有测试
cargo test

# 运行特定测试
cargo test -p karte-lexer
cargo test -p karte-parser
cargo test -p karte-lir
```

## 项目结构

- `karte-lexer` - 词法分析
- `karte-parser` - 语法分析与类型检查
- `karte-hir` - 高级中间表示
- `karte-mir` - 中级中间表示
- `karte-lir` - 低级中间表示
- `karte-codegen` - JIT 代码生成 (AArch64)
- `karte-cli` - 命令行工具
- `karte-module-system` - 多模块编译和缓存系统

## 开发

更多详细信息，请参考 [CLAUDE.md](CLAUDE.md)。

## 许可证

MIT License
