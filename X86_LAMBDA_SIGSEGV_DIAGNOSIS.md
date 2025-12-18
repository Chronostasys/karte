# X86 Lambda Multiple Call SIGSEGV 诊断报告

## 问题描述

在x86平台上，涉及多次lambda函数调用的测试会导致SIGSEGV（段错误）。AArch64平台没有此问题。

## 失败测试

共9个测试失败：
- 8个SIGSEGV：所有涉及多次lambda/闭包调用
- 1个FAIL：`test_nested_logical_operations`（逻辑错误，非崩溃）

## 关键观察

1. **单次调用正常**：单次lambda调用或直接函数调用均正常
2. **两次调用崩溃**：两次lambda调用必定崩溃
3. **直接调用正常**：两次直接函数调用正常
4. **问题特定于间接调用**：JumpIndirect相关

## 已尝试的修复（均无效）

### 1. Return Address 寄存器选择
- **原始**：R12（callee-saved）
- **尝试**：R11（caller-saved），R10（caller-saved）
- **结果**：无改善

**理论**：R12是callee-saved，在compile_return中使用会破坏约定
**实际**：改用caller-saved寄存器也无效

### 2. Epilogue 临时寄存器
- **原始**：R10
- **尝试**：R13, RDX, RCX
- **结果**：无改善

**理论**：临时寄存器可能与其他用途冲突
**实际**：更换多个寄存器均无效

### 3. 禁用 Prologue/Epilogue
- **尝试**：完全禁用 `emit_internal_function_prologue/epilogue`
- **结果**：仍然崩溃

**关键发现**：问题不在prologue/epilogue本身

### 4. Callee-Saved 配置
- **原始配置**：RBP/RSP已正确排除在callee_saved之外
- **结果**：配置已正确

## 当前代码状态

### 修改文件
1. `karte-common/src/calling_convention.rs`:
   - `return_address`: REG_R12 → REG_R11
   
2. `karte-codegen/src/vm/professional_executor/jit/x86_compiler.rs`:
   - `emit_internal_function_epilogue`临时寄存器：R10 → RCX

## 深层分析

### Prologue/Epilogue 逻辑验证

**Prologue**（已验证正确）：
```
1. SP -= 16
2. [SP+8] = FP
3. [SP+0] = SP（更新后的值）
4. 保存callee-saved寄存器（如果有）
```

**Epilogue**（已验证正确）：
```
1. 恢复callee-saved寄存器（LIFO）
2. temp = [SP+0]
3. SP = temp
4. FP = [SP+8]
5. SP += 16
```

### 返回流程验证

**Caller（lower_call_indirect）**：
```
1. 保存caller-saved
2. 准备参数
3. SP -= 16; [SP+0] = return_label
4. JumpIndirect
5. [return_label]
6. SP += 16（弹出返回地址）
7. 恢复caller-saved
```

**Callee（compile_return）**：
```
1. 设置返回值到RAX
2. emit_internal_function_epilogue（恢复SP到返回地址位置）
3. return_addr = [SP+0]
4. JumpRegister(return_addr)
```

**理论正确性**：流程逻辑完全对齐AArch64

## 可能的根本原因（待验证）

### 1. 指令编码错误
**假设**：某个x86指令的机器码生成有误
**检查方向**：
- `emit_mov_reg_mem` / `emit_mov_mem_reg`
- `emit_jump_register`
- Memory addressing modes

### 2. 寄存器映射问题
**假设**：虚拟寄存器到物理寄存器映射在某种情况下错误
**检查方向**：
- `get_physical_register()` 实现
- 寄存器分配器在间接调用时的行为

### 3. 栈对齐问题
**假设**：x86-64要求16字节栈对齐，某处违反了这个约定
**检查方向**：
- 虚拟栈操作是否总是16字节对齐
- `emit_sub_reg_imm32` / `emit_add_reg_imm32`

### 4. Wrapper函数特殊性
**假设**：wrapper函数（用于统一闭包调用约定）有特殊问题
**检查方向**：
- Wrapper函数的生成逻辑
- Wrapper函数的prologue/epilogue

### 5. 第二次调用时的状态污染
**假设**：第一次调用成功但遗留错误状态
**检查方向**：
- 全局状态/静态变量
- 寄存器未正确清理
- 虚拟栈指针累积偏移

## 下一步调试建议

### 方法1：机器码级别调试
```bash
# 使用gdb/lldb attach到进程
gdb --args ./target/debug/karte run test_lambda_twice.karte
# 设置断点在第二次lambda调用前
# 单步执行，查看寄存器和内存
```

### 方法2：导出并分析LIR
```bash
# 导出LIR
cargo run -- build --emit-lir test_lambda_twice.karte
# 手动检查生成的LIR，特别是间接调用部分
```

### 方法3：对比AArch64机器码
```bash
# 在AArch64机器上编译相同代码
# 对比x86和AArch64生成的机器码差异
```

### 方法4：插入调试断点
在x86 compiler中插入 `int3` 断点（0xCC），在关键位置暂停执行：
- 第一次lambda调用前/后
- 第二次lambda调用前
- compile_return的返回地址加载处

### 方法5：简化测试用例
创建最小可复现案例：
```karte
fn id(x:number) -> number { x }
fn main() -> number {
    let f = |g| { g(42) };
    let a = f(id);
    let b = f(id);
    a + b
}
```

## 结论

问题的根本原因尚未找到，但可以确定：
1. ✅ 不在prologue/epilogue的高层逻辑
2. ✅ 不在RBP/RSP的callee-saved配置
3. ✅ 不在简单的寄存器选择
4. ❓ 可能在指令编码、寄存器映射或其他深层实现细节

建议使用底层调试工具（gdb/lldb）进行机器码级别的分析。
