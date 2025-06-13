# LIR 解释器原理详解

## 概述

LIR (Low-Level Intermediate Representation) 解释器是 `karte` 编译器中用于直接执行低级中间表示代码的组件。它模拟了一个简单的虚拟机，具有寄存器、内存和程序计数器等基本组件，能够执行类似汇编语言的线性指令序列。

## 架构设计

### 虚拟机状态 (`InterpreterState`)

解释器的核心是 `InterpreterState` 结构体，它维护了虚拟机的完整状态：

```rust
struct InterpreterState {
    pc: usize,                                    // 程序计数器
    registers: HashMap<RegisterId, i64>,          // 寄存器文件
    memory: Vec<i64>,                            // 内存模型
    comparison_result: std::cmp::Ordering,       // 比较标志位
    call_stack: Vec<usize>,                      // 函数调用栈
}
```

#### 各组件详解

1. **程序计数器 (PC)**
   - 指向当前要执行的指令在指令序列中的索引
   - 每执行完一条指令后自动递增，除非遇到跳转指令

2. **寄存器文件**
   - 使用 `HashMap` 实现，支持动态分配寄存器
   - 寄存器通过 `RegisterId` 标识，值为 64 位整数
   - 未初始化的寄存器默认值为 0

3. **内存模型**
   - 简化的线性内存模型，预分配 1024 个 64 位整数槽位
   - 当前实现中主要用于扩展性，实际指令集中暂未大量使用

4. **比较标志位**
   - 存储最近一次比较操作的结果
   - 用于条件跳转指令的判断依据

5. **调用栈**
   - 维护函数调用的返回地址
   - 支持嵌套函数调用和递归

## 执行流程

### 1. 程序初始化

```rust
pub fn execute(program: &LirProgram) -> Result<i64, String>
```

执行流程从 `execute` 函数开始：

1. **查找主函数**: 从 `LirProgram` 中获取主函数名称和定义
2. **构建函数入口映射**: 为每个函数创建名称到入口标签的映射
3. **指令扁平化**: 将所有函数的指令合并为一个线性指令序列
4. **标签地址映射**: 预处理所有标签，建立标签到 PC 地址的映射表

### 2. 程序计数器初始化

```rust
let main_entry_label = function_entry_labels.get(main_fn_name)?;
state.pc = *label_map.get(main_entry_label)?;
```

将程序计数器设置为主函数的入口地址，开始执行。

### 3. 指令执行循环

主执行循环采用经典的取指-译码-执行模式：

```rust
while let Some(instruction) = instructions.get(state.pc) {
    // 取指：获取当前 PC 指向的指令
    // 译码：模式匹配确定指令类型
    // 执行：根据指令类型执行相应操作
    // 更新 PC：移动到下一条指令（除非是跳转指令）
}
```

## 指令集实现

### 数据移动指令

- **`Move`**: 将源操作数的值复制到目标寄存器
  ```rust
  Instruction::Move { dst, src, .. } => {
      let val = state.get_operand_value(src);
      state.set_register(dst, val);
  }
  ```

### 算术运算指令

支持基本的四则运算：
- **`Add`**: 加法运算
- **`Sub`**: 减法运算  
- **`Mul`**: 乘法运算
- **`Div`**: 除法运算（包含除零检查）

```rust
Instruction::Add { dst, src1, src2, .. } => {
    let v1 = state.get_operand_value(src1);
    let v2 = state.get_operand_value(src2);
    state.set_register(dst, v1 + v2);
}
```

### 比较与跳转指令

#### 比较指令
```rust
Instruction::Compare { src1, src2, .. } => {
    let v1 = state.get_operand_value(src1);
    let v2 = state.get_operand_value(src2);
    state.comparison_result = v1.cmp(&v2);
}
```

比较指令设置标志位，为后续的条件跳转做准备。

#### 跳转指令族
- **`Jump`**: 无条件跳转
- **`JumpEqual`**: 相等时跳转
- **`JumpNotEqual`**: 不相等时跳转
- **`JumpGreater`**: 大于时跳转
- **`JumpGreaterEqual`**: 大于等于时跳转
- **`JumpLess`**: 小于时跳转
- **`JumpLessEqual`**: 小于等于时跳转

所有跳转指令都会：
1. 检查跳转条件（条件跳转）
2. 更新程序计数器到目标标签地址
3. 设置 `pc_changed` 标志防止 PC 自动递增

### 函数调用机制

#### 函数调用 (`Call`)
```rust
Instruction::Call { target, args, result: _, .. } => {
    // 1. 保存返回地址
    state.call_stack.push(state.pc + 1);
    
    // 2. 跳转到目标函数
    let target_pc = label_map.get(target)?;
    state.pc = *target_pc;
    pc_changed = true;
}
```

#### 函数返回 (`Return`)
```rust
Instruction::Return { value, .. } => {
    let result = value.as_ref().map_or(0, |reg| state.get_register(reg));
    if let Some(return_addr) = state.call_stack.pop() {
        // 普通函数返回
        state.pc = return_addr;
        state.set_register(&RegisterId(0), result); // 返回值约定
    } else {
        // 主函数返回，程序结束
        return Ok(result);
    }
}
```

## 操作数处理

解释器支持多种操作数类型：

```rust
fn get_operand_value(&self, operand: &Operand) -> i64 {
    match operand {
        Operand::Register { id } => self.get_register(id),  // 寄存器值
        Operand::Immediate { value } => *value,             // 立即数
        _ => panic!("Invalid operand for value"),           // 其他类型
    }
}
```

- **寄存器操作数**: 从寄存器文件中读取值
- **立即数操作数**: 直接使用常量值
- **标签操作数**: 主要用于跳转指令的目标地址
- **内存操作数**: 用于内存访问（当前实现中较少使用）

## 调试支持

解释器内置了调试输出功能：

```rust
println!("PC: {}, Instruction: {:?}", state.pc, instruction);
println!("Registers: {:?}", state.registers);
```

每执行一条指令都会输出：
- 当前程序计数器值
- 正在执行的指令详情
- 当前所有寄存器的状态

这对于调试 LIR 代码生成和验证执行流程非常有用。

## 错误处理

解释器实现了完善的错误处理机制：

1. **除零检查**: 除法指令会检查除数是否为零
2. **函数查找**: 验证主函数和调用目标是否存在
3. **标签解析**: 确保所有跳转目标都有对应的标签
4. **栈溢出保护**: 通过调用栈管理防止无限递归

## 设计特点与限制

### 优点
1. **简单直观**: 直接模拟硬件执行模型，易于理解和调试
2. **完整性**: 支持完整的控制流和函数调用机制
3. **可扩展**: 架构设计支持添加新的指令类型
4. **调试友好**: 内置详细的执行状态输出

### 当前限制
1. **性能**: 解释执行比编译执行慢很多
2. **内存模型**: 简化的内存模型，不支持复杂的内存操作
3. **类型系统**: 只支持 64 位整数，缺乏丰富的数据类型
4. **并发**: 不支持多线程或并发执行

## 在编译器流程中的作用

LIR 解释器在 `karte` 编译器中扮演重要角色：

1. **验证工具**: 验证 MIR 到 LIR 转换的正确性
2. **测试平台**: 为编译器各阶段提供快速的执行验证
3. **原型开发**: 在完整的代码生成器开发完成前提供可执行的后端
4. **教学工具**: 帮助理解低级代码的执行语义

通过这个解释器，开发者可以在不需要完整汇编器和链接器的情况下，快速验证编译器前端生成的 LIR 代码是否正确，大大加速了编译器的开发和调试过程。 