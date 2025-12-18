# x86-64 JIT 编译器修复最终报告

## 执行摘要

通过系统性地修复 x86-64 calling convention 配置错误和重构硬编码寄存器，成功将测试通过率从 **44%** 提升到 **64%**。

## 测试结果对比

| 指标 | 修复前 | 修复后 | 改善 |
|------|--------|--------|------|
| **通过测试** | 105 | 150 | +45 (+43%) |
| **失败测试** | 129 | 84 | -45 (-35%) |
| **跳过测试** | 5 | 5 | 0 |
| **通过率** | 43.9% | 64.1% | +20.2% |

## 关键修复

### 1. ✅ VM 栈指针配置错误（根本原因）

**问题**：
在 `/workspace/karte-common/src/calling_convention.rs` 的 `CallingConvention::x86_64()` 方法中，VM 的栈指针和帧指针被错误地配置为系统栈寄存器：

```rust
// ❌ 错误配置
stack_pointer: REG_RSP,    // 15 - 系统栈指针
frame_pointer: REG_RBP,    // 14 - 系统帧指针
```

**影响**：
- 函数序言中试图将虚拟栈地址保存到系统的 RBP/RSP 中
- 导致系统栈被破坏
- 几乎所有 JIT 执行测试都段错误

**修复**：
```rust
// ✅ 正确配置
stack_pointer: REG_R10,    // 6 - VM虚拟栈指针
frame_pointer: REG_R11,    // 7 - VM虚拟帧指针
use_system_stack_pointer: false,  // 使用VM虚拟栈
```

**效果**：
- **45个测试**从失败变为通过
- 所有基础算术、逻辑运算、控制流测试现在通过

**相关文件**：
- `/workspace/karte-common/src/calling_convention.rs:251-252, 268`

---

### 2. ✅ Effect 寄存器冲突

**问题**：
Effect 相关寄存器与 VM 栈/帧指针使用了相同的寄存器：

```rust
// ❌ 冲突配置
stack_pointer: REG_R10,          // R10
effect_tag_register: REG_R10,    // R10 冲突！
frame_pointer: REG_R11,          // R11
effect_resume_temp: REG_R11,     // R11 冲突！
```

**修复**：
```rust
// ✅ 避免冲突
stack_pointer: REG_R10,          // R10
effect_tag_register: REG_R13,    // R13 (callee-saved)
frame_pointer: REG_R11,          // R11
effect_resume_temp: REG_R14,     // R14 (callee-saved)
```

**相关文件**：
- `/workspace/karte-common/src/calling_convention.rs:254-257`

---

### 3. ✅ 硬编码寄存器重构

**工作内容**：
- 删除了 `X86Register` enum
- 移除了约 30 处 `X86Register::XXX as u8` 硬编码
- 所有寄存器访问改为使用 calling convention 常量
- 新增 `phys_reg_to_x86_hw_reg()` 映射函数

**影响的函数**（15个）：
- `compile_div`, `compile_return`, `emit_runtime_call`
- `emit_function_prologue`, `emit_function_epilogue`
- `save_callee_saved_registers`, `restore_callee_saved_registers`
- 等等

**相关文件**：
- `/workspace/karte-codegen/src/vm/professional_executor/jit/x86_compiler.rs` (全文)
- `/workspace/karte-codegen/src/vm/professional_executor/jit/compiler_trait.rs` (CallingConventionInfo)

---

### 4. ⏳ 除法指令优化（待完善）

**改进**：
- 简化了实现，移除了手动的 push/pop 栈操作
- 依赖寄存器分配器处理 RAX/RDX 的冲突

**当前状态**：
- ⚠️ `test_evaluate_division` 仍然 SIGFPE
- 需要进一步调试

**相关文件**：
- `/workspace/karte-codegen/src/vm/professional_executor/jit/x86_compiler.rs:484-555`

---

## 当前测试状态

### ✅ 通过的测试类别（150个）

1. **基础运算** (100% 通过)
   - 算术：加减乘、一元运算
   - 比较：等于、不等于、小于、大于、小于等于、大于等于
   - 逻辑：与、或、非、短路求值

2. **控制流** (100% 通过)
   - if/else 表达式
   - while 循环
   - 嵌套控制流

3. **变量和赋值** (100% 通过)
   - let 绑定
   - 赋值语句
   - 变量作用域

4. **模式匹配** (部分通过)
   - 简单匹配
   - 布尔字面量

5. **Parser/Type Checker** (100% 通过)
   - 所有词法、语法、类型检查测试

### ⚠️ 失败的测试类别（84个）

1. **函数调用** (大部分失败)
   - Lambda 创建和调用
   - 闭包作为参数
   - 高阶函数
   - 多参数函数

2. **结构体** (全部失败)
   - 结构体定义和构造
   - 字段访问
   - 嵌套结构体

3. **自定义类型** (全部失败)
   - Enum 定义
   - 参数化类型
   - 模式匹配with数据

4. **引用** (全部失败)
   - 简单引用
   - 解引用
   - 引用算术

5. **数组** (全部失败)
   - 数组字面量
   - 索引访问

6. **代数效应** (全部失败)
   - Effect pipeline
   - Effect 传播

7. **除法** (1个失败)
   - `test_evaluate_division` - SIGFPE

---

## 剩余问题分析

### 问题 1: 函数调用失败

**可能原因**：
- 参数传递可能使用了错误的寄存器顺序
- VM calling convention 的参数寄存器配置可能不正确
- 内部函数序言/尾声可能有问题

**调试建议**：
1. 检查 `argument_registers` 配置是否与 LIR 生成的代码一致
2. 对比 AArch64 的参数传递方式
3. 添加调试日志查看参数寄存器的值

### 问题 2: 结构体/数组失败

**可能原因**：
- 内存布局计算可能有问题
- Load64/Store64 指令可能使用了错误的寄存器
- 指针算术可能不正确

**调试建议**：
1. 检查 `StructFieldLoad`/`StructFieldStore` 的编译
2. 验证内存访问的基地址寄存器

### 问题 3: 除法 SIGFPE

**可能原因**：
- 寄存器保存/恢复逻辑仍有问题
- CQO 指令可能在错误的时机执行
- 临时寄存器 R8 可能与其他寄存器冲突

**调试建议**：
1. 使用 GDB 查看除法指令执行时的寄存器状态
2. 检查生成的机器码是否正确
3. 对比 AArch64 的除法实现

---

## 代码质量改进

1. **✅ 模块化**：Calling convention 集中管理，易于维护
2. **✅ 可移植性**：x86 和 AArch64 使用相同的抽象
3. **✅ 类型安全**：使用常量而非魔法数字
4. **✅ 文档完善**：添加了详细的注释和说明

---

## 下一步工作

### 优先级 P0（Critical）

1. **修复除法指令** - 唯一的 SIGFPE 错误
2. **修复函数调用** - 24个失败测试
3. **修复简单函数调用** - `test_simple_function_call`

### 优先级 P1（High）

4. **修复结构体支持** - 15个失败测试
5. **修复引用支持** - 6个失败测试

### 优先级 P2（Medium）

6. **修复自定义类型** - 17个失败测试
7. **修复数组支持** - 2个失败测试

### 优先级 P3（Low）

8. **修复代数效应** - 3个失败测试
9. **性能优化**
10. **添加更多测试**

---

## 总结

通过本次系统性的重构和修复：

1. **✅ 完成了用户的两个主要请求**：
   - 为 x86-64 添加专用 calling convention
   - 将硬编码寄存器改为使用 calling convention

2. **✅ 显著提升了测试通过率**：
   - 从 44% 提升到 64%
   - 45个测试从失败变为通过

3. **✅ 修复了根本性的架构问题**：
   - VM 栈指针配置错误
   - 寄存器冲突

4. **⏳ 识别了剩余问题**：
   - 函数调用、结构体、引用、除法等
   - 提供了详细的调试建议

项目现在具有良好的基础，剩余的问题可以逐个解决。建议按优先级顺序处理，优先修复函数调用和除法问题，这两个是最基础的功能。

---

## 相关文档

- [详细技术文档](/workspace/X86_CALLING_CONVENTION_REFACTOR.md)
- [中文总结](/workspace/X86_REFACTOR_SUMMARY_zh.md)
- [实现总结](/workspace/X86_IMPLEMENTATION_SUMMARY.md)
