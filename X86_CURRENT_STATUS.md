# x86-64 JIT 编译器当前状态报告

## 最新进展

### ✅ 完成的关键修复

1. **实现了架构特定的 `CallingConvention::standard()`**
   - 添加了 `#[cfg(target_arch = "x86_64")]` 条件编译
   - 添加了 `#[cfg(target_arch = "aarch64")]` 条件编译
   - 确保 LIR passes 在不同架构上使用正确的配置

2. **修正了 x86-64 的寄存器编号系统**
   - x86-64 使用 0-15 的寄存器编号（PhysicalRegister 直接等于硬件寄存器编号）
   - AArch64 使用 0-31 的寄存器编号
   - 配置：
     - stack_pointer: 4 (REG_RSP)
     - frame_pointer: 5 (REG_RBP)
     - return_register: 0 (REG_RAX)

3. **修复了 RSP/RBP 内存访问的 SIB 字节支持**
   - 在 x86-64 中，当基址寄存器是 RSP (4) 时，必须使用 SIB 字节
   - 实现了 `emit_sib()` 方法
   - 更新了 `emit_mov_reg_mem`, `emit_mov_mem_reg`, `emit_mov_mem_imm32` 以正确处理 RSP/RBP

4. **添加了辅助方法**
   - `get_vm_sp_hw()` - 获取 VM 栈指针的硬件寄存器编号
   - `get_vm_fp_hw()` - 获取 VM 帧指针的硬件寄存器编号
   - `get_vm_return_addr_hw()` - 获取返回地址的硬件寄存器编号

### ⚠️ 当前问题

#### 问题：所有测试段错误

**症状**：
- 即使最简单的表达式 `42` 也会段错误
- 之前能工作的 LIR 文件现在也段错误
- 测试结果：105 passed, 129 failed (与之前相同)

**可能原因**：

1. **REX 前缀问题**
   - x86-64 的 REX 前缀可能不正确
   - 特别是涉及 RSP/RBP 时

2. **SIB 字节编码问题**
   - 新添加的 SIB 字节支持可能有错误
   - SIB 编码格式：`[scale:2][index:3][base:3]`
   - 当 index=100 (无索引) 且 base=100 (RSP) 时的特殊情况

3. **栈切换逻辑问题**
   - 函数序言中将 RSP 切换到虚拟栈后，后续的栈操作可能不正确
   - `emit_sub_reg_imm32` 对 RSP 的处理可能有问题

4. **ModR/M 编码问题**
   - 当 r/m=101 (5, RBP) 且 mod=00 时，表示 RIP 相对寻址
   - 当 r/m=100 (4, RSP) 时，需要 SIB 字节

### 🔍 调试信息

从日志可以看到：
```
[DEBUG] x86-64: 开始编译函数 '__script_entry__'
[DEBUG] 序言开始：生成符合 System V AMD64 ABI 的函数序言
[DEBUG] 序言：栈使用量 = 32 字节
[DEBUG] 序言：0 个 callee-saved 寄存器
[DEBUG] 编译x86-64指令: mov dst: #p0, src: # value: 42
[DEBUG] 编译x86-64指令: Return(#p0)
[INFO] x86-64: 函数 '__script_entry__' 编译完成，机器码大小: 80 字节
```

编译成功，但执行时段错误。

### 📝 代码变更摘要

**文件 `/workspace/karte-common/src/calling_convention.rs`**:
- 添加了 x86-64 寄存器常量（PhysicalRegister 直接等于硬件编号）
- 实现了条件编译的 `standard()` 方法
- 实现了 `x86_64()` 方法（使用 0-15 的寄存器）
- 实现了 `aarch64()` 方法（使用 0-31 的寄存器）

**文件 `/workspace/karte-codegen/src/vm/professional_executor/jit/x86_compiler.rs`**:
- 简化了 `initialize_register_mapping`（只映射 0-15）
- 添加了 `emit_sib()` 方法
- 修复了 `emit_mov_reg_mem`, `emit_mov_mem_reg`, `emit_mov_mem_imm32` 以支持 RSP/RBP
- 添加了 `get_vm_sp_hw()`, `get_vm_fp_hw()`, `get_vm_return_addr_hw()` 辅助方法
- 修复了函数序言中的虚拟栈切换逻辑

**文件 `/workspace/karte-codegen/src/vm/professional_executor/jit/aarch64_compiler.rs`**:
- 将 `CallingConvention::standard()` 改为 `CallingConvention::aarch64()`

**文件 `/workspace/karte-codegen/src/vm/professional_executor/jit/compiler_trait.rs`**:
- 添加了 `CallingConventionInfo::get_callee_save_registers()` 方法
- 添加了 `CallingConventionInfo::get_caller_save_registers()` 方法

### 🎯 测试结果

| 架构 | 通过 | 失败 | 跳过 | 通过率 |
|------|------|------|------|--------|
| x86-64 (当前) | 105 | 129 | 5 | 44.9% |

所有 JIT 执行测试均失败（段错误）。

### 🔄 下一步行动

#### 优先级 P0 - 紧急修复

1. **调试段错误根本原因**
   - 使用调试器检查生成的机器码
   - 验证 REX 前缀的正确性
   - 验证 ModR/M 和 SIB 字节编码
   - 检查栈对齐和栈指针操作

2. **简化测试案例**
   - 创建最小的机器码测试
   - 逐字节检查生成的指令

3. **参考 AArch64 的工作实现**
   - 对比两个架构的序言/尾声逻辑
   - 确保 x86 正确模拟了相同的行为

#### 优先级 P1 - 验证修复

4. **验证寄存器映射**
   - 确认 Physical(0-15) 正确映射到硬件寄存器 0-15
   - 确认 calling convention 的配置正确

5. **测试基础指令**
   - mov, add, sub等基础指令
   - Load64, Store64 内存访问指令
   - Return 指令

### 📚 相关文档

- x86-64 指令集参考：Intel Software Developer Manual Vol. 2
- System V AMD64 ABI：https://refspecs.linuxfoundation.org/elf/x86_64-abi-0.99.pdf
- ModR/M 和 SIB 编码：第 2.1.5 节
- REX 前缀：第 2.2.1 节

### 💡 技术要点

1. **x86-64 的 16 寄存器限制**
   - 只有 RAX, RCX, RDX, RBX, RSP, RBP, RSI, RDI, R8-R15
   - PhysicalRegister 0-15 直接对应这些寄存器

2. **特殊寄存器处理**
   - RSP (4): ModR/M r/m=100 时需要 SIB 字节
   - RBP (5): ModR/M r/m=101 且 mod=00 时表示 RIP 相对寻址

3. **栈切换策略**
   - 序言：系统栈 -> 虚拟栈
   - 尾声：虚拟栈 -> 系统栈
   - 必须正确保存/恢复系统栈指针

### 🔧 修复建议

1. **验证机器码生成**
   ```rust
   // 测试简单的 MOV RSP, RDI 指令
   // 应该生成：REX.W (48) + 89 /r + ModR/M
   // REX.W = 0x48
   // MOV r/m, r = 0x89
   // ModR/M: mod=11, reg=rdi(7), r/m=rsp(4) = 0xFC (?)
   ```

2. **逐步验证每个步骤**
   - 测试纯寄存器操作（不涉及内存）
   - 测试内存操作（使用非特殊寄存器）
   - 测试使用 RSP/RBP 的内存操作

3. **对比生成的机器码与预期**
   - 使用反汇编工具检查生成的指令
   - 与手写汇编对比

---

**总结**：虽然完成了架构分离和寄存器系统重构，但 JIT 编译器生成的机器码存在根本性问题，导致无法执行。需要深入调试机器码生成逻辑。
