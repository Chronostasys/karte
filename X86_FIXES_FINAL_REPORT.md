# X86-64 JIT 编译器修复最终报告

## 测试结果改进

### 修复前
- **通过**: 211/239 (88.3%)
- **失败**: 23/239 (9.6%) - 全部 SIGSEGV

### 修复后
- **通过**: 214/239 (89.5%) ✅ **+3 个**
- **失败**: 20/239 (8.3%) ✅ **-3 个**
- **跳过**: 5/239 (2.1%)

## 关键修复

### 1. MOV R64, imm64 指令的 REX 前缀错误 ✅ **已修复**

**问题**: Store64 Label 实现中，MOV R11, imm64 指令的 REX 前缀参数顺序错误。

**位置**: `karte-codegen/src/vm/professional_executor/jit/x86_compiler.rs:873`

**错误代码**:
```rust
self.emit_rex_prefix(code_builder, true, temp_reg, 0, 0);  // ❌ 错误
```

**正确代码**:
```rust
self.emit_rex_prefix(code_builder, true, 0, 0, temp_reg);  // ✅ 正确
```

**原因分析**:
- `MOV r64, imm64` 使用 opcode `B8+r` 编码
- 寄存器编码在 opcode 中，对应 REX.B 位（第4个参数），不是 REX.R 位（第2个参数）
- R11 的编号是 11 (1011b)，高位为 1，需要 REX.B = 1
- 错误的参数顺序导致 REX.B = 0，使得 MOV 变成了 MOV RBX (寄存器3) 而不是 R11 (寄存器11)
- 结果是标签地址被写入了错误的寄存器

**调试过程**:
1. GDB 显示 RIP = 0x246，R11 = 0x246 - 说明跳转到了相对偏移而不是绝对地址
2. 日志显示标签地址修补完成，但实际上没生效
3. 检查发现 MOV 指令的 REX 前缀参数顺序错误

### 2. 返回地址寄存器冲突 ✅ **已修复**

**问题**: x86-64 calling convention 使用 RAX 作为返回地址寄存器，但 RAX 也是返回值寄存器。

**修复**: 
- 文件: `karte-common/src/calling_convention.rs:276`
- 将 `return_address` 从 `REG_RAX` 改为 `REG_R12`

### 3. 内部函数序言/尾声的栈指针处理 ✅ **已修复**

**问题**: x86-64 不能直接执行 `mov SP, [SP+0]`，因为会破坏基址寄存器。

**修复**:
- 文件: `karte-codegen/src/vm/professional_executor/jit/x86_compiler.rs:1743-1827`
- 使用临时寄存器（R11）避免基址破坏
- 对齐 AArch64 的序言/尾声语义

### 4. 标签地址修补逻辑 ✅ **已修复**

**问题**: 对于本地标签（相对偏移），修补时没有加上 exec_base 转换为绝对地址。

**修复**:
- 文件: `karte-codegen/src/vm/professional_executor/jit/code_buffer.rs:492-515`
- 判断是否为本地标签（偏移 < 0x10000）
- 本地标签加上 exec_base 转换为绝对地址

## 当前状态

### 工作的测试
- ✅ 简单函数调用（如 `add(1, 2)`）
- ✅ 多参数函数
- ✅ 嵌套函数调用
- ✅ 所有不涉及闭包的测试

### 仍然失败的测试 (20个)
所有涉及闭包和 lambda 的测试：
- `test_closure_as_higher_order_param`
- `test_function_as_value`
- `test_identity_closure_returns_function`
- `test_lambda_creation`
- `test_let_with_lambda`
- 等...

**失败模式**: 全部 SIGSEGV

## 已修改的文件

1. **karte-common/src/calling_convention.rs**
   - 修改返回地址寄存器从 RAX 到 R12

2. **karte-codegen/src/vm/professional_executor/jit/x86_compiler.rs**
   - 修复 MOV R64, imm64 的 REX 前缀参数顺序 (line 873)
   - 修复内部函数序言/尾声的栈指针处理 (lines 1743-1827)
   - 修复 compile_return 中返回值和返回地址的处理顺序 (lines 753-808)
   - 修复 Store64 Label 的实现 (lines 863-880)

3. **karte-codegen/src/vm/professional_executor/jit/code_buffer.rs**
   - 修复标签地址修补逻辑，正确处理本地标签 (lines 492-515)

## 下一步调试建议

### 1. 闭包相关问题
剩余的失败都涉及闭包，可能的问题：
- 闭包结构的内存布局
- 闭包捕获变量的访问
- CallIndirect 指令的实现
- 环境指针的传递

### 2. 调试方法
```bash
# 创建最小闭包测试
cat > /tmp/test_closure.karte << 'EOF'
fn main() -> number {
    let f = |x: number| { x + 1 };
    f(5)
}
EOF

# 运行并查看崩溃
gdb --args ./target/debug/karte run /tmp/test_closure.karte

# 在 GDB 中查看崩溃时的状态
run
info registers
bt
```

### 3. 检查项
- [ ] CallIndirect 的 JMP 指令编码是否正确
- [ ] 闭包结构的字段偏移是否正确（function_ptr, env_ptr）
- [ ] 环境指针的传递是否符合调用约定
- [ ] Load64 从闭包结构加载函数指针时的偏移

## 成就总结

✅ **修复了函数调用基础设施**
- 简单函数调用现在可以正常工作
- 参数传递正确
- 返回值处理正确

✅ **修复了关键的指令编码错误**
- MOV R64, imm64 的 REX 前缀
- 标签地址的修补逻辑

✅ **提升了测试通过率**
- 从 88.3% 提升到 89.5%
- 修复了 3 个测试

## 技术要点

### x86-64 REX 前缀编码
```
REX 前缀格式: 0100WRXB
- W: 0=32位操作数, 1=64位操作数
- R: ModR/M.reg 字段的扩展位 (bit 3)
- X: SIB.index 字段的扩展位 (bit 3)
- B: ModR/M.r/m 或 SIB.base 或 opcode.reg 字段的扩展位 (bit 3)
```

对于 `MOV r64, imm64` (opcode B8+r)：
- 寄存器编码在 opcode 中 (+r)
- 需要设置 REX.B 位来访问 R8-R15
- **不需要** REX.R 位

### 标签地址修补
```rust
// 本地标签（函数内的跳转目标）存储为相对偏移
if offset < 0x10000 {
    absolute_addr = exec_base + offset;
}
// 全局标签（跨函数调用）已经是绝对地址
else {
    absolute_addr = offset;
}
```

## 参考资料

- [Intel® 64 and IA-32 Architectures Software Developer's Manual](https://www.intel.com/content/www/us/en/developer/articles/technical/intel-sdm.html)
- [x86-64 Instruction Encoding](https://wiki.osdev.org/X86-64_Instruction_Encoding)
- [REX prefix](https://en.wikipedia.org/wiki/X86-64#REX_prefix)
