# 寄存器分配修复计划

## 问题分析

当前的 `spilled_register_storage` 设计存在以下问题：

1. **违背寄存器分配原理**：溢出寄存器应该存储在栈上，而不是独立的 HashMap 中
2. **内存管理不一致**：溢出值没有真正存储在虚拟机内存中
3. **lowering 不完整**：仍可能生成访问非法寄存器的代码

## 修复方案

### 1. 修改虚拟机架构

#### 移除 `spilled_register_storage`
```rust
// 从 VirtualMachine 中移除
pub struct VirtualMachine {
    pub registers: [i64; NUM_REGISTERS],
    pub pc: usize,
    pub sp: usize,
    pub flags: ComparisonFlags,
    pub memory: Vec<i64>,
    pub call_stack: Vec<usize>,
    pub register_mapping: HashMap<RegisterId, u8>,
    // 移除这一行：
    // pub spilled_register_storage: HashMap<RegisterId, i64>,
}
```

#### 修改寄存器访问逻辑
```rust
impl VirtualMachine {
    /// 获取虚拟寄存器的值（仅通过物理寄存器映射）
    pub fn get_virtual_register(&self, reg_id: &RegisterId) -> Result<i64, String> {
        if let Some(&physical_reg) = self.register_mapping.get(reg_id) {
            self.get_physical_register(physical_reg)
        } else {
            Err(format!("Unmapped virtual register: {:?}", reg_id))
        }
    }

    /// 设置虚拟寄存器的值（仅通过物理寄存器映射）
    pub fn set_virtual_register(&mut self, reg_id: &RegisterId, value: i64) -> Result<(), String> {
        if let Some(&physical_reg) = self.register_mapping.get(reg_id) {
            self.set_physical_register(physical_reg, value)
        } else {
            Err(format!("Unmapped virtual register: {:?}", reg_id))
        }
    }
}
```

### 2. 完善寄存器分配器

#### 溢出寄存器映射到栈位置
```rust
/// 溢出寄存器信息
#[derive(Debug, Clone)]
pub struct SpillInfo {
    /// 虚拟寄存器ID
    pub virtual_register: RegisterId,
    /// 栈偏移量（相对于栈指针）
    pub stack_offset: i64,
    /// 溢出槽大小
    pub slot_size: usize,
}

/// 改进的寄存器分配结果
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
```

#### 寄存器分配器生成栈访问指令
```rust
impl SpillManager {
    /// 为溢出寄存器生成栈访问指令
    pub fn generate_spill_code(&self, 
                              virtual_reg: &RegisterId, 
                              spill_info: &SpillInfo,
                              is_load: bool) -> Vec<Instruction> {
        let mut instructions = Vec::new();
        let sp_reg = self.calling_convention.stack_pointer;
        
        if is_load {
            // 从栈加载：temp_reg = [sp + offset]
            instructions.push(Instruction::Load64 {
                dst: *virtual_reg,
                addr: RegisterId(sp_reg as usize),
                offset: spill_info.stack_offset,
                span: Span::dummy(),
            });
        } else {
            // 存储到栈：[sp + offset] = temp_reg
            instructions.push(Instruction::Store64 {
                addr: RegisterId(sp_reg as usize),
                offset: spill_info.stack_offset,
                src: Operand::Register { id: *virtual_reg },
                span: Span::dummy(),
            });
        }
        
        instructions
    }
}
```

### 3. 修改 lowering 阶段

#### 确保只生成合法寄存器访问
```rust
/// 寄存器合法性检查器
pub struct RegisterValidator {
    max_physical_registers: usize,
    calling_convention: CallingConvention,
}

impl RegisterValidator {
    /// 验证指令中的寄存器访问是否合法
    pub fn validate_instruction(&self, instruction: &Instruction) -> Result<(), String> {
        let registers = self.extract_registers(instruction);
        
        for reg in registers {
            // 检查是否为合法的物理寄存器
            if reg.0 >= self.max_physical_registers {
                return Err(format!("Illegal register access: {:?}", reg));
            }
        }
        
        Ok(())
    }
    
    /// 验证函数中的所有寄存器访问
    pub fn validate_function(&self, function: &LirFunction) -> Result<(), String> {
        for instruction in &function.instructions {
            self.validate_instruction(instruction)?;
        }
        Ok(())
    }
}
```

#### 强制寄存器分配在 lowering 之前完成
```rust
/// 修改后的 lowering 管道
pub struct LoweringPipeline {
    register_allocator: ProfessionalRegisterAllocator,
    instruction_lowerer: InstructionLowerer,
    validator: RegisterValidator,
}

impl LoweringPipeline {
    pub fn lower_program(&mut self, program: &mut LirProgram) -> Result<(), String> {
        // 1. 先进行寄存器分配
        for (_, function) in program.functions.iter_mut() {
            let allocation_result = self.register_allocator.allocate_function(function)?;
            
            // 2. 应用寄存器分配结果
            self.apply_register_allocation(function, &allocation_result)?;
            
            // 3. 处理溢出寄存器
            self.handle_spilled_registers(function, &allocation_result)?;
        }
        
        // 4. 降级高级指令
        self.instruction_lowerer.lower_program(program)?;
        
        // 5. 验证最终结果
        for (_, function) in &program.functions {
            self.validator.validate_function(function)?;
        }
        
        Ok(())
    }
    
    /// 处理溢出寄存器
    fn handle_spilled_registers(&mut self, 
                               function: &mut LirFunction, 
                               allocation: &AllocationResult) -> Result<(), String> {
        // 为每个使用溢出寄存器的指令生成加载/存储指令
        let mut new_instructions = Vec::new();
        
        for instruction in &function.instructions {
            let (modified_instruction, pre_loads, post_stores) = 
                self.rewrite_instruction_with_spills(instruction, allocation)?;
            
            new_instructions.extend(pre_loads);
            new_instructions.push(modified_instruction);
            new_instructions.extend(post_stores);
        }
        
        function.instructions = new_instructions;
        Ok(())
    }
}
```

### 4. 执行时验证

#### 专业执行器验证
```rust
impl ProfessionalExecutor {
    /// 验证程序中的寄存器访问
    pub fn validate_program(&self, program: &LirProgram) -> Result<(), String> {
        for (_, function) in &program.functions {
            for instruction in &function.instructions {
                self.validate_register_access(instruction)?;
            }
        }
        Ok(())
    }
    
    /// 验证单条指令的寄存器访问
    fn validate_register_access(&self, instruction: &Instruction) -> Result<(), String> {
        // 确保所有寄存器访问都通过正确的物理寄存器映射
        // 不允许访问未映射的虚拟寄存器
        match instruction {
            Instruction::Move { dst, src, .. } => {
                self.validate_register_operand(dst)?;
                self.validate_operand(src)?;
            }
            // ... 其他指令类型
            _ => {}
        }
        Ok(())
    }
}
```

## 实施步骤

### 阶段1：移除 spilled_register_storage
1. 修改 `VirtualMachine` 结构体
2. 更新相关的寄存器访问方法
3. 修复所有编译错误

### 阶段2：完善寄存器分配器
1. 修改 `AllocationResult` 结构体
2. 实现栈溢出槽管理
3. 生成正确的栈访问指令

### 阶段3：修改 lowering 管道
1. 创建寄存器验证器
2. 修改 lowering 流程
3. 确保寄存器分配在 lowering 之前完成

### 阶段4：测试和验证
1. 运行所有现有测试
2. 添加新的寄存器分配测试
3. 验证栈溢出功能正确性

## 预期效果

1. **合规性**：符合标准的寄存器分配理论
2. **一致性**：溢出寄存器与栈管理一致
3. **可靠性**：编译期保证寄存器访问合法
4. **性能**：真正的内存访问，支持缓存等优化

这个修复将使 Karte 的寄存器分配系统更加专业和可靠，为后续的性能优化奠定基础。 