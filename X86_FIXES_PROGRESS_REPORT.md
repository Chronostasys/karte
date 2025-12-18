# X86-64 JIT 编译器修复进度报告

## 问题概述
在 x86-64 平台上，karte-tests 有 23 个测试失败，全部是 SIGSEGV 段错误。所有失败的测试都涉及 JIT 执行，特别是函数调用相关的功能。

## 测试通过率
- **通过**: 211/239 (88.3%)
- **失败**: 23/239 (9.6%) - 全部 SIGSEGV
- **跳过**: 5/239 (2.1%)

## 已发现并修复的问题

### 1. 返回地址寄存器冲突
**问题**: x86-64 calling convention 使用 RAX 作为返回地址寄存器，但 RAX 也是返回值寄存器，导致返回值被覆盖。

**修复**:
- 文件: `karte-common/src/calling_convention.rs:276`
- 将 `return_address` 从 `REG_RAX` 改为 `REG_R12`
- 文件: `karte-codegen/src/vm/professional_executor/jit/x86_compiler.rs:753-808`
- 修改 `compile_return` 逻辑：
  - 在 epilogue 后使用 R11 临时保存返回地址
  - 在跳转前才设置返回值到 RAX

### 2. 内部函数序言/尾声的栈指针保存/恢复
**问题**: 原始实现直接保存更新后的 SP，但在恢复时需要正确处理。

**当前状态**: 已调整为与 AArch64 一致的语义：
- 序言保存更新后的 SP
- 尾声使用临时寄存器（R11）避免基址破坏
- 增加 `SP += 16` 恢复到进入函数前的位置

文件: `karte-codegen/src/vm/professional_executor/jit/x86_compiler.rs:1743-1827`

### 3. Store64 Label 实现
**问题**: 原始实现使用 `emit_lea_reg_rip_rel`，但该函数没有正确记录待修补的标签引用。

**修复**:
- 文件: `karte-codegen/src/vm/professional_executor/jit/x86_compiler.rs:863-879`
- 改用 `MOV R11, imm64` 指令
- 使用 `emit_label_address` 正确记录待修补位置

### 4. 标签地址修补逻辑
**问题**: 对于本地标签（相对偏移），修补时没有加上 `exec_base` 转换为绝对地址。

**修复**:
- 文件: `karte-codegen/src/vm/professional_executor/jit/code_buffer.rs:492-515`
- 判断是否为本地标签（偏移 < 0x10000）
- 本地标签加上 `exec_base` 转换为绝对地址

## 当前状态

### 工作的测试
- 不含函数调用的简单表达式（如 `fn main() -> number { 42 }`）
- 局部变量和算术运算（如 `let a = 1; let b = 2; a + b`）

### 失败的测试
所有包含函数调用的测试都会 SIGSEGV：
- `test_function_call` (简单的 `add(1, 2)`)
- `test_allocate_many_stack_refs`
- `test_closure_as_higher_order_param`
- 等等...

## 可能还存在的问题

### 1. 指令编码问题
`MOV R11, imm64` 的 REX 前缀计算可能有误：
```rust
self.emit_rex_prefix(code_builder, true, temp_reg, 0, 0);
code_builder.emit_byte(0xB8 + (temp_reg & 0x07));
```

R11 的编号是 11，`temp_reg & 0x07` = 3，所以opcode是 `0xB8 + 3 = 0xBB`。
但 R11 需要 REX.B=1，可能需要检查 `emit_rex_prefix` 的实现。

### 2. 参数传递
LIR 的 Call 指令降级后，参数通过栈中转然后加载到参数寄存器。
这个过程可能有问题，需要检查：
- `karte-lir/src/pass/instruction_lowering_pass.rs:421-457`

### 3. 跳转指令修补
虽然修改了标签地址修补，但 Jump 指令本身的修补逻辑（第 472-488 行）可能也需要类似的处理。

## 下一步调试建议

1. **启用详细日志**
   ```bash
   RUST_LOG=debug ./target/debug/karte run test.karte
   ```

2. **检查生成的机器码**
   添加日志输出实际生成的字节码，对比预期的 x86-64 指令编码

3. **单步调试**
   使用 LLDB/GDB 在 JIT 执行点设置断点，查看崩溃时的寄存器状态和栈布局

4. **简化测试**
   创建最小化的测试用例，逐步增加复杂度：
   - 空函数调用
   - 单参数函数
   - 双参数函数
   - 有返回值的函数

5. **对比 AArch64**
   在 AArch64 平台上运行相同测试，对比生成的 LIR，确认 x86-64 的降级逻辑是否正确

## 已修改的文件

1. `karte-common/src/calling_convention.rs` - 修改返回地址寄存器
2. `karte-codegen/src/vm/professional_executor/jit/x86_compiler.rs` - 多处修复
3. `karte-codegen/src/vm/professional_executor/jit/code_buffer.rs` - 标签地址修补

## 测试命令

```bash
# 运行所有测试
cargo nextest run -p karte-tests --no-fail-fast

# 运行单个测试
cargo test -p karte-tests --lib cli_integration_tests::cli_tests::test_function_as_value -- --nocapture

# 测试简单函数
./target/debug/karte run /tmp/test_function_call.karte
```
