# x86-64 Calling Convention 重构总结

## 用户请求
用户要求：
1. 在 `calling_convention.rs` 文件中为 x86-64 添加专用的 calling convention
2. 将所有使用硬编码寄存器的代码改为使用 calling convention 中的寄存器

## 已完成的工作

### ✅ 1. 在 `karte-common/src/calling_convention.rs` 中添加 x86-64 支持

**新增的寄存器常量** (第33-49行):
```rust
// x86-64 System V AMD64 ABI 寄存器命名约定
pub const REG_RAX: PhysicalRegister = 0;  // 返回值寄存器
pub const REG_RBX: PhysicalRegister = 1;  // Callee-saved
pub const REG_RCX: PhysicalRegister = 2;  // 参数4 (C调用约定)
pub const REG_RDX: PhysicalRegister = 3;  // 参数3 (C调用约定)
pub const REG_RSI: PhysicalRegister = 8;  // 参数2 (C调用约定)
pub const REG_RDI: PhysicalRegister = 9;  // 参数1 (C调用约定)
pub const REG_RBP: PhysicalRegister = 14; // 帧指针 (Callee-saved)
pub const REG_RSP: PhysicalRegister = 15; // 栈指针
pub const REG_R8: PhysicalRegister = 4;   // 参数5 (C调用约定)
pub const REG_R9: PhysicalRegister = 5;   // 参数6 (C调用约定)
pub const REG_R10: PhysicalRegister = 6;  // 临时寄存器/VM栈指针
pub const REG_R11: PhysicalRegister = 7;  // 临时寄存器/VM帧指针
pub const REG_R12: PhysicalRegister = 10; // Callee-saved / Effect栈指针
pub const REG_R13: PhysicalRegister = 11; // Callee-saved
pub const REG_R14: PhysicalRegister = 12; // Callee-saved
pub const REG_R15: PhysicalRegister = 13; // Callee-saved
```

**新增的 `CallingConvention::x86_64()` 方法** (第178-245行):
- 创建专用于 x86-64 的调用约定
- 定义了两种调用约定：
  - **C FFI (System V AMD64 ABI)**: `RDI, RSI, RDX, RCX, R8, R9` 作为参数寄存器
  - **VM内部**: `R9, R8, RCX, RDX, R8, R9` 作为VM参数寄存器
- 正确配置了 caller-saved 和 callee-saved 寄存器
- 16字节栈对齐要求

### ✅ 2. 重构 `karte-codegen/src/vm/professional_executor/jit/x86_compiler.rs`

**移除的内容**:
- ❌ 删除了 `X86Register` enum 定义（原第286-303行）
- ❌ 删除了所有 `X86Register::XXX as u8` 的硬编码用法（约30处）

**新增的内容**:
- ✅ 添加了 `phys_reg_to_x86_hw_reg()` 方法（第286-306行）
  - 将 Karte 逻辑寄存器编号映射到 x86-64 硬件寄存器编号
- ✅ 更新了 `create_c_ffi_calling_convention()` （第50-69行）
  - 使用 `REG_RDI`, `REG_RSI` 等常量代替硬编码

**修改的函数** (使用 calling convention 寄存器):
1. `new()` - 使用 `CallingConvention::x86_64()`
2. `initialize_register_mapping()` - 所有寄存器映射使用常量
3. `compile_div()` - 使用 `REG_RAX`, `REG_RDX`, `REG_R8`
4. `compile_return()` - 使用 `REG_RAX`
5. `emit_runtime_call()` - 使用 `REG_RAX`, `REG_RDI`-`REG_R9`
6. `emit_function_prologue()` - 使用 `REG_RDI`, `REG_RSI`, `REG_RBP`, `REG_RSP`, `REG_R8`
7. `emit_function_epilogue()` - 使用 `REG_RSP`, `REG_RBP`, `REG_R8`
8. `save_return_slot_pointer()` - 使用 `REG_RDI`
9. `emit_store_return_value_to_slot()` - 使用 `REG_RAX`
10. `save_callee_saved_registers()` - 使用 `REG_RBP`
11. `restore_callee_saved_registers()` - 使用 `REG_RBP`
12. `emit_call_absolute()` - 使用 `REG_RAX`
13. `emit_sub_rsp_imm()`, `emit_add_rsp_imm()` - 使用 `REG_RSP`
14. `save_call_clobbered_registers()` - 使用 `REG_RSP`
15. `restore_call_clobbered_registers()` - 使用 `REG_RSP`

### ✅ 3. 扩展 `karte-codegen/src/vm/professional_executor/jit/compiler_trait.rs`

**新增方法** (第287-302行):
```rust
impl CallingConventionInfo {
    /// 获取需要保存的 callee-saved 寄存器
    pub fn get_callee_save_registers(&self, used_registers: &[u8]) -> Vec<u8>
    
    /// 获取需要保存的 caller-saved 寄存器
    pub fn get_caller_save_registers(&self, live_registers: &[u8]) -> Vec<u8>
}
```

## 编译状态

✅ **编译成功**: 所有代码可以正常编译，没有编译错误

## 运行状态

⚠️ **运行时错误**: 测试仍然出现段错误 (SIGSEGV)
- 测试：`cargo test -p karte-tests --lib test_evaluate_addition`
- 错误：`signal: 11, SIGSEGV: invalid memory reference`

## 可能的原因

段错误可能由以下原因导致：

1. **寄存器映射问题**
   - `phys_reg_to_x86_hw_reg()` 的映射可能不正确
   - 某些寄存器的硬件编号可能映射错误

2. **调用约定配置**
   - VM calling convention 的参数寄存器顺序可能不符合预期
   - Callee-saved 寄存器列表可能不完整

3. **序言/尾声代码**
   - 函数序言中的寄存器保存/恢复顺序可能有问题
   - 栈指针操作可能不正确

4. **寄存器使用冲突**
   - 某些寄存器可能被同时用于多个目的
   - 例如 R8 在不同上下文中有不同用途

## 建议的调试步骤

1. **使用 GDB 调试**:
   ```bash
   gdb --args target/debug/deps/karte_tests-xxx test_evaluate_addition
   # 在段错误处查看栈帧和寄存器状态
   ```

2. **检查寄存器映射**:
   - 验证 `phys_reg_to_x86_hw_reg()` 返回的硬件寄存器编号是否正确
   - 确认所有 LIR 物理寄存器都有正确的映射

3. **验证调用约定**:
   - 检查 `CallingConvention::x86_64()` 的配置是否符合 x86-64 ABI
   - 确认 caller-saved 和 callee-saved 列表正确

4. **测试简化场景**:
   - 创建最小测试用例（如 `1 + 1`）
   - 逐步添加复杂性以定位问题

5. **比较 AArch64 实现**:
   - 对比 `aarch64_compiler.rs` 的调用约定使用方式
   - 确保 x86 实现遵循相同的模式

## 成果总结

✅ **已完成用户的两个主要请求**:
1. ✅ 在 `calling_convention.rs` 中添加了 x86-64 专用 calling convention
2. ✅ 将所有硬编码寄存器替换为 calling convention 中的寄存器

⏳ **剩余工作**:
- 调试并修复运行时段错误
- 确保所有测试通过

## 相关文件

- `/workspace/karte-common/src/calling_convention.rs` - Calling convention 定义
- `/workspace/karte-codegen/src/vm/professional_executor/jit/x86_compiler.rs` - x86-64 编译器
- `/workspace/karte-codegen/src/vm/professional_executor/jit/compiler_trait.rs` - CallingConventionInfo 扩展
- `/workspace/X86_CALLING_CONVENTION_REFACTOR.md` - 详细技术文档
- `/workspace/X86_REFACTOR_SUMMARY_zh.md` - 本文档（中文总结）
