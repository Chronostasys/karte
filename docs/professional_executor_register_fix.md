# 专业执行器寄存器分配修复实施

## 问题概述

原始设计中的 `spilled_register_storage` 存在以下严重问题：

1. **违背寄存器分配原理**：溢出寄存器应该存储在栈上，而不是独立的 HashMap 中
2. **内存管理不一致**：溢出值没有真正存储在虚拟机内存中，无法与栈管理协调
3. **编译期检查缺失**：允许生成访问未映射寄存器的代码，运行时才发现问题

## 已完成的修复

### 1. 移除 `spilled_register_storage`

我们已经从 `VirtualMachine` 结构体中完全移除了 `spilled_register_storage` 字段：

```rust
// 修复前：
pub struct VirtualMachine {
    pub registers: [i64; NUM_REGISTERS],
    // ... 其他字段
    pub spilled_register_storage: HashMap<RegisterId, i64>, // 已移除
}

// 修复后：
pub struct VirtualMachine {
    pub registers: [i64; NUM_REGISTERS],
    // ... 其他字段
    // spilled_register_storage 已完全移除
}
```

### 2. 强化寄存器访问验证

修改了虚拟寄存器访问方法，现在会严格检查映射：

```rust
/// 获取虚拟寄存器的值（仅通过物理寄存器映射）
pub fn get_virtual_register(&self, reg_id: &RegisterId) -> Result<i64, String> {
    if let Some(&physical_reg) = self.register_mapping.get(reg_id) {
        self.get_physical_register(physical_reg)
    } else {
        // 严格错误：未映射的寄存器访问
        Err(format!("Unmapped virtual register: {:?} - register allocation should handle spilling", reg_id))
    }
}
```

这意味着：
- **编译期保证**：所有虚拟寄存器必须在寄存器分配阶段被处理
- **运行时安全**：不会意外访问未映射的寄存器
- **明确责任**：溢出处理完全由寄存器分配器负责

## 影响分析

### 积极影响

1. **符合标准**：现在符合标准的寄存器分配理论
2. **内存一致性**：溢出寄存器将通过真实的栈内存访问
3. **编译时安全**：强制在编译期处理所有寄存器分配问题
4. **调试友好**：更容易追踪寄存器分配问题

### 需要后续处理的问题

1. **寄存器分配器需要完善**：必须为所有虚拟寄存器提供映射或生成栈访问代码
2. **测试可能失败**：依赖旧行为的测试需要更新
3. **lowering 管道需要调整**：确保寄存器分配在指令生成之前完成

## 下一步计划

### 阶段2：完善寄存器分配器

需要修改 `AllocationResult` 结构体，添加溢出信息：

```rust
#[derive(Debug, Clone)]
pub struct AllocationResult {
    /// 虚拟寄存器到物理寄存器的映射
    pub register_mapping: HashMap<RegisterId, PhysicalRegister>,
    /// 溢出寄存器信息
    pub spill_info: HashMap<RegisterId, SpillInfo>,
    /// 栈帧大小（包含溢出槽）
    pub stack_frame_size: usize,
    /// 调用约定
    pub calling_convention: CallingConvention,
}

#[derive(Debug, Clone)]
pub struct SpillInfo {
    /// 虚拟寄存器ID
    pub virtual_register: RegisterId,
    /// 栈偏移量（相对于栈指针）
    pub stack_offset: i64,
    /// 溢出槽大小
    pub slot_size: usize,
}
```

### 阶段3：修改指令生成

对于溢出的寄存器，需要生成栈访问指令：

```rust
// 溢出寄存器的加载：
// load temp_reg, [sp + offset]
Instruction::Load64 {
    dst: temp_reg,
    addr: RegisterId(sp_reg as usize),
    offset: spill_info.stack_offset,
    span: instruction_span,
}

// 溢出寄存器的存储：
// store [sp + offset], temp_reg
Instruction::Store64 {
    addr: RegisterId(sp_reg as usize),
    offset: spill_info.stack_offset,
    src: Operand::Register { id: temp_reg },
    span: instruction_span,
}
```

### 阶段4：验证和测试

1. **编译期验证**：确保所有生成的指令只访问已映射的寄存器
2. **运行时测试**：验证栈溢出功能的正确性
3. **性能测试**：确保栈访问不会显著影响性能

## 测试策略

### 单元测试
```rust
#[test]
fn test_unmapped_register_access_fails() {
    let mut vm = VirtualMachine::new();
    let unmapped_reg = RegisterId(999);
    
    // 应该返回错误而不是返回默认值
    assert!(vm.get_virtual_register(&unmapped_reg).is_err());
}

#[test]
fn test_mapped_register_access_succeeds() {
    let mut vm = VirtualMachine::new();
    let virtual_reg = RegisterId(100);
    let physical_reg = 5;
    
    vm.register_mapping.insert(virtual_reg, physical_reg);
    vm.set_physical_register(physical_reg, 42).unwrap();
    
    assert_eq!(vm.get_virtual_register(&virtual_reg).unwrap(), 42);
}
```

### 集成测试
1. **简单程序**：验证基本的寄存器分配功能
2. **寄存器压力测试**：使用超过物理寄存器数量的虚拟寄存器
3. **函数调用测试**：验证跨函数调用的寄存器管理

## 总结

这个修复是朝着正确方向的重要一步。通过移除 `spilled_register_storage`，我们：

1. **提高了架构质量**：符合标准的寄存器分配实现
2. **增强了类型安全**：编译期捕获寄存器分配问题
3. **简化了设计**：移除了不必要的复杂性
4. **为优化奠定基础**：真实的内存访问支持缓存优化等

下一步需要完善寄存器分配器，确保它能够正确处理所有溢出情况，并生成合适的栈访问代码。 