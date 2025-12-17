# x86-64 Calling Convention 重构总结

## 概述
本次重构的目标是将 x86_compiler.rs 中的硬编码寄存器替换为使用 `karte-common/calling_convention.rs` 中定义的调用约定，使代码更加模块化和可维护。

## 完成的工作

### 1. 在 `calling_convention.rs` 中添加 x86-64 支持

**新增内容**:
- 添加了 x86-64 寄存器常量定义 (`REG_RAX`, `REG_RBX`, `REG_RCX`, 等)
- 添加了 `CallingConvention::x86_64()` 方法，创建 x86-64 专用的调用约定
- 支持两种调用约定：
  - **C FFI**: System V AMD64 ABI (RDI, RSI, RDX, RCX, R8, R9 用于参数)
  - **VM内部**: Karte VM 调用约定 (R9, R8, RCX, RDX, R4, R5 用于参数)

**寄存器映射策略**:
```
x86-64 物理寄存器 → Karte 逻辑寄存器编号
RAX  → 0  (返回值)
RBX  → 1  (Callee-saved)
RCX  → 2  (参数3)
RDX  → 3  (参数4)
R8   → 4  (参数2 VM, 参数5 C)
R9   → 5  (参数1 VM, 参数6 C)
R10  → 6  (VM栈指针)
R11  → 7  (VM帧指针)
RSI  → 8  (参数2 C)
RDI  → 9  (参数1 C)
R12  → 10 (Effect栈指针)
R13  → 11 (Callee-saved)
R14  → 12 (Callee-saved)
R15  → 13 (Callee-saved)
RBP  → 14 (帧指针)
RSP  → 15 (栈指针)
```

### 2. 重构 `x86_compiler.rs`

**主要更改**:

1. **移除硬编码**:
   - 删除了 `X86Register` enum
   - 移除了所有 `X86Register::XXX as u8` 的使用

2. **使用 Calling Convention**:
   - `new()` 方法现在使用 `CallingConvention::x86_64()`
   - 添加了 `create_c_ffi_calling_convention()` 创建 C FFI 调用约定
   - 所有寄存器访问都通过 calling convention 获取

3. **新增辅助方法**:
   ```rust
   fn phys_reg_to_x86_hw_reg(&self, reg: PhysicalRegister) -> u8
   ```
   将 Karte 逻辑寄存器编号映射到 x86-64 硬件寄存器编号

4. **更新的函数**:
   - `initialize_register_mapping()`: 使用 calling convention 中的寄存器常量
   - `compile_div()`: 使用 `REG_RAX`, `REG_RDX`, `REG_R8`
   - `compile_return()`: 使用 `REG_RAX`
   - `emit_runtime_call()`: 使用 `REG_RDI`, `REG_RSI`, `REG_RDX`, 等
   - `emit_function_prologue()`: 使用 `REG_RDI`, `REG_RSI`, `REG_RBP`, `REG_RSP`, `REG_R8`
   - `emit_function_epilogue()`: 使用 `REG_RSP`, `REG_RBP`, `REG_R8`
   - `save_return_slot_pointer()`: 使用 `REG_RDI`
   - `emit_store_return_value_to_slot()`: 使用 `REG_RAX`
   - `save_callee_saved_registers()`: 使用 `REG_RBP`
   - `restore_callee_saved_registers()`: 使用 `REG_RBP`
   - `emit_call_absolute()`: 使用 `REG_RAX`
   - `emit_sub_rsp_imm()`, `emit_add_rsp_imm()`: 使用 `REG_RSP`
   - `save_call_clobbered_registers()`: 使用 `REG_RSP`
   - `restore_call_clobbered_registers()`: 使用 `REG_RSP`

### 3. 扩展 `CallingConventionInfo`

**新增方法** (在 `compiler_trait.rs` 中):
```rust
impl CallingConventionInfo {
    pub fn get_callee_save_registers(&self, used_registers: &[u8]) -> Vec<u8>
    pub fn get_caller_save_registers(&self, live_registers: &[u8]) -> Vec<u8>
}
```

这些方法过滤出需要保存的寄存器列表。

## 当前状态

✅ **编译成功**: 代码可以成功编译，没有编译错误

⏳ **测试状态**: 正在测试 JIT 执行是否正常工作

## 优势

1. **可维护性**: 寄存器分配集中管理，易于修改和扩展
2. **可移植性**: 不同架构的调用约定清晰分离
3. **类型安全**: 使用 calling convention 常量而非魔法数字
4. **一致性**: x86 和 AArch64 使用相同的 calling convention 抽象

## 后续工作

1. 运行完整测试套件验证正确性
2. 根据测试结果调试和修复问题
3. 优化寄存器分配策略
4. 添加更多文档说明调用约定的使用

## 相关文件

- `/workspace/karte-common/src/calling_convention.rs` - Calling convention 定义
- `/workspace/karte-codegen/src/vm/professional_executor/jit/x86_compiler.rs` - x86-64 编译器
- `/workspace/karte-codegen/src/vm/professional_executor/jit/compiler_trait.rs` - 编译器 trait 和 CallingConventionInfo
