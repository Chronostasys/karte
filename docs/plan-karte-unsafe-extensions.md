# 计划：Karte 语言扩展 — unsafe 内存操作 + 位运算 + != 运算符

> 生成时间：2026-05-26
> 状态：待确认

## 背景与目标

AOT runtime 中的 GC 是手写 x86 机器码（runtime_x86.rs），极难维护。目标是给 Karte 语言加足够的底层能力，使 GC 核心逻辑可以用 Karte 源代码编写。

### 设计原则
- **专业性**：新特性不是为 GC 量身定做的 hack，而是专业系统编程语言的标配能力
- **最小侵入**：复用现有 LIR 的 Load64/Store64/Add/Sub 等基础指令，不需要新的 LIR 指令
- **安全分级**：unsafe 操作需要显式标注，与安全子集明确区分

## 现状分析

### 需要新增的语言特性

| 特性 | 分类 | 优先级 | 说明 |
|------|------|--------|------|
| `!=` 运算符 | 比较运算 | P0 | 基础运算符，MIR 已有 NotEqual 但 HIR 没有 |
| `&` (bitwise AND) | 位运算 | P0 | 用 `bitand` 关键字避免与引用 `&` 冲突 |
| `\|` (bitwise OR) | 位运算 | P0 | 用 `bitor` 关键字避免与 lambda `\|` 冲突 |
| `^` (bitwise XOR) | 位运算 | P0 | `bitxor` |
| `<<` / `>>` (shift) | 位运算 | P0 | `shl` / `shr` |
| `~` (bitwise NOT) | 位运算 | P0 | `bitnot` |
| `unsafe_load(addr)` | 内存操作 | P0 | 从任意地址读 8 字节，返回 number |
| `unsafe_store(addr, val)` | 内存操作 | P0 | 向任意地址写 8 字节 |
| `unsafe_load32(addr)` | 内存操作 | P1 | 读 4 字节 |
| `unsafe_store32(addr, val)` | 内存操作 | P1 | 写 4 字节 |
| `unsafe_load8(addr)` | 内存操作 | P1 | 读 1 字节 |
| `unsafe_store8(addr, val)` | 内存操作 | P1 | 写 1 字节 |

### Token 设计决策

**为什么用关键字而非符号？**
- `&` 已被引用语法占用
- `|` 已被 lambda 参数语法占用
- 用 `bitand`/`bitor`/`bitxor`/`bitnot`/`shl`/`shr` 避免歧义，Rust 社区对此有先例（宏名）
- 作为 infix 关键字运算符，它们在解析时处于乘法和加法之间

**`!=` 是例外** — 这个符号没有任何冲突，直接加为 Token。

### unsafe 内建函数设计

```karte
fn unsafe_load(addr: number) -> number        // 读 *[addr]
fn unsafe_store(addr: number, val: number)    // 写 *[addr] = val
fn unsafe_load8(addr: number) -> number       // 读 *u8[addr]
fn unsafe_store8(addr: number, val: number)   // 写 *u8[addr] = val
fn unsafe_load32(addr: number) -> number      // 读 *u32[addr]
fn unsafe_store32(addr: number, val: number)  // 写 *u32[addr] = val
```

这些不是普通函数，是编译器内建（intrinsic），在 MIR 层直接 lower 到 Load64/Store64/Load32/Store32 等 LIR 指令。

## 关键文件

| 文件 | 职责 | 修改类型 |
|------|------|----------|
| `karte-lexer/src/lib.rs` | Token 定义 | 修改：加 NotEqual token |
| `karte-hir/src/ast.rs` | AST 定义 | 修改：BinaryOperator 加位运算和 NotEqual，Expr 加 UnsafeLoad/UnsafeStore |
| `karte-hir/src/types.rs` | 类型系统 | 不改 |
| `karte-hir/src/type_checker.rs` | 类型检查 | 修改：新运算符的类型推断规则 |
| `karte-parser/src/expression.rs` | 表达式解析 | 修改：加位运算优先级层、!= 解析、unsafe_* 解析 |
| `karte-mir/src/ir.rs` | MIR 定义 | 修改：BinaryOperator 加位运算，Statement 加 UnsafeLoad/UnsafeStore |
| `karte-mir/src/lower/helpers.rs` | HIR→MIR 映射 | 修改：convert_binary_op |
| `karte-mir/src/lower/expr.rs` | 表达式 lowering | 修改：lower UnsafeLoad/UnsafeStore |
| `karte-mir/src/codec.rs` | MIR 序列化 | 修改：BinaryOperator 加位运算 |
| `karte-lir/src/ir.rs` | LIR 指令 | 修改：加 Load32/Store32/Load8/Store8（如果需要） |
| `karte-lir/src/lower/stmt.rs` | MIR→LIR lowering | 修改：lower UnsafeLoad/UnsafeStore 到 Load64/Store64 |
| `karte-codegen/.../x86_compiler.rs` | x86 后端 | 修改：编译新位运算指令、新的 load/store 大小 |
| `karte-codegen/.../aarch64_compiler.rs` | AArch64 后端 | 修改：同上 |
| `karte-codegen/.../instruction_processor.rs` | 解释器 | 修改：执行新指令 |
| `karte-aot/src/runtime_x86.rs` | AOT runtime | 最终：用 Karte GC 替换 |

## 详细计划

### 阶段一：`!=` 运算符（最简单，验证全链路）

所有层都有 NotEqual 的支持基础（MIR 已有），只需打通前端。

- [ ] 1.1 Lexer: 加 `NotEqual` token — `karte-lexer/src/lib.rs`
- [ ] 1.2 HIR AST: `BinaryOperator` 加 `NotEqual` — `karte-hir/src/ast.rs`
- [ ] 1.3 Parser: `parse_comparison` 中解析 `!=` — `karte-parser/src/expression.rs`
- [ ] 1.4 Type checker: NotEqual 类型推断 — `karte-hir/src/type_checker.rs`
- [ ] 1.5 MIR mapping: HIR NotEqual → MIR NotEqual（已有）— `karte-mir/src/lower/helpers.rs`
- [ ] 1.6 测试验证

### 阶段二：位运算（bitand/bitor/bitxor/bitnot/shl/shr）

需要全链路新增。

- [ ] 2.1 HIR AST: `BinaryOperator` 加 BitAnd/BitOr/BitXor/ShiftLeft/ShiftRight，`UnaryOperator` 加 BitNot — `karte-hir/src/ast.rs`
- [ ] 2.2 MIR: `BinaryOperator` 加 BitAnd/BitOr/BitXor/ShiftLeft/ShiftRight，`UnaryOperator` 加 BitNot — `karte-mir/src/ir.rs`，`karte-mir/src/codec.rs`
- [ ] 2.3 Parser: 在 `parse_comparison` 和 `parse_additive` 之间加 `parse_bitwise` 层，解析 `bitand`/`bitor`/`bitxor` 关键字；在 `parse_term` 和 `parse_factor` 之间加 `parse_shift` 层解析 `shl`/`shr`；`parse_factor` 中解析 `bitnot` — `karte-parser/src/expression.rs`
- [ ] 2.4 Type checker: 位运算类型推断（都要求 Number → Number）— `karte-hir/src/type_checker.rs`
- [ ] 2.5 MIR mapping: convert_binary_op/convert_unary_op — `karte-mir/src/lower/helpers.rs`
- [ ] 2.6 LIR → x86: x86 后端编译位运算（AND/OR/XOR/SHL/SHR/NOT 指令）— `karte-codegen/.../x86_compiler.rs`
- [ ] 2.7 LIR → AArch64: AArch64 后端编译位运算 — `karte-codegen/.../aarch64_compiler.rs`
- [ ] 2.8 解释器: InstructionProcessor 执行位运算 — `karte-codegen/.../instruction_processor.rs`
- [ ] 2.9 测试验证

### 阶段三：unsafe 内存操作（unsafe_load/unsafe_store）

核心能力，GC 的基石。

- [ ] 3.1 HIR AST: Expr 加 `UnsafeLoad { addr, size }` 和 `UnsafeStore { addr, value, size }` — `karte-hir/src/ast.rs`
- [ ] 3.2 Parser: `parse_primary` 中解析 `unsafe_load(...)` / `unsafe_store(...)` / `unsafe_load8(...)` 等 — `karte-parser/src/expression.rs`
- [ ] 3.3 Type checker: 所有参数和返回值都是 Number — `karte-hir/src/type_checker.rs`
- [ ] 3.4 MIR: Statement 加 `UnsafeLoad { target, addr, size }` 和 `UnsafeStore { addr, value, size }` — `karte-mir/src/ir.rs`
- [ ] 3.5 MIR lowering: lower_expression 中处理 UnsafeLoad/UnsafeStore — `karte-mir/src/lower/expr.rs`
- [ ] 3.6 LIR: 加 `Load32`/`Store32`/`Load8`/`Store8` 指令（Load64/Store64 已有）— `karte-lir/src/ir.rs`
- [ ] 3.7 LIR lowering: MIR UnsafeLoad/UnsafeStore → LIR Load64/Store64/Load32/Store32/Load8/Store8 — `karte-lir/src/lower/stmt.rs`
- [ ] 3.8 x86 后端: 编译 Load32/Store32/Load8/Store8 — `karte-codegen/.../x86_compiler.rs`
- [ ] 3.9 AArch64 后端: 同上 — `karte-codegen/.../aarch64_compiler.rs`
- [ ] 3.10 解释器: 执行新 load/store — `karte-codegen/.../instruction_processor.rs`
- [ ] 3.11 测试验证

### 阶段四：用 Karte 重写 GC

- [ ] 4.1 用 Karte 实现 bump allocator
- [ ] 4.2 用 Karte 实现 mark-sweep GC
- [ ] 4.3 用 Karte 实现 compaction
- [ ] 4.4 在 AOT 编译中嵌入 Karte 编译的 GC 代码
- [ ] 4.5 清理 runtime_x86.rs 中的手写 GC 代码
- [ ] 4.6 全量测试

## 验证方案

- 阶段一/二/三：每个阶段完成后 `cargo nextest run --workspace` 全量通过
- 阶段三：新增 unsafe_load/unsafe_store 集成测试
- 阶段四：GC 用 Karte 写完后，AOT 逃逸分析测试（allocate_many, escape_after_deep_stack）必须通过

## 回滚策略

每个阶段独立 commit。如果某个阶段引入问题，`git revert` 单个 commit 即可。

## 注意事项

- `&` 和 `|` 符号已被占用，位运算用关键字名（bitand/bitor/bitxor/bitnot/shl/shr）
- `!=` 没有冲突，直接用符号
- unsafe_* 内建函数在 MIR 层直接 lower 到 LIR 指令，不需要运行时调用
- LIR 的 Load64/Store64 已存在，位运算需要 x86 AND/OR/XOR/SHL/SHR/NOT 编码
- AArch64 后端需要同步修改
- 位运算优先级：bitnot(一元) > shift > bitand > bitor(最低二元位运算)
