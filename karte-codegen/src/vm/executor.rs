//! 指令执行引擎
//! 
//! 实现LIR指令的执行逻辑，包括算术运算、控制流、内存访问等

use super::{VirtualMachine, JumpCondition, MemoryManager, RegisterAllocator, NUM_REGISTERS};
use karte_lir::{Instruction, LabelId, LirProgram, Operand, RegisterId};
use std::collections::HashMap;

/// 指令执行器
#[derive(Debug)]
pub struct InstructionExecutor {
    /// 虚拟机状态
    vm: VirtualMachine,
    /// 内存管理器
    memory: MemoryManager,
    /// 标签到程序计数器的映射
    label_map: HashMap<LabelId, usize>,
    /// 当前执行的程序
    program: Option<LirProgram>,
    /// 所有指令的统一序列
    all_instructions: Vec<Instruction>,
    /// 调试模式
    debug_mode: bool,
}

impl InstructionExecutor {
    /// 创建新的指令执行器
    pub fn new(debug_mode: bool) -> Self {
        Self {
            vm: VirtualMachine::new(),
            memory: MemoryManager::new(),
            label_map: HashMap::new(),
            program: None,
            all_instructions: Vec::new(),
            debug_mode,
        }
    }

    /// 执行LIR程序
    pub fn execute_program(&mut self, program: &LirProgram) -> Result<i64, String> {
        // 准备执行环境
        self.prepare_execution(program)?;
        
        // 执行主函数
        self.execute_main_function()
    }

    /// 准备执行环境
    fn prepare_execution(&mut self, program: &LirProgram) -> Result<(), String> {
        // 重置虚拟机状态
        self.vm.reset();
        self.memory.reset();
        self.label_map.clear();
        self.all_instructions.clear();

        // 构建所有函数的统一指令序列和标签映射
        let mut instruction_offset = 0;
        for (func_name, function) in &program.functions {
            if self.debug_mode {
                println!("Processing function: {}", func_name);
            }
            
            // 添加这个函数的所有指令到统一序列
            for instruction in &function.instructions {
                self.all_instructions.push(instruction.clone());
            }
            
            // 为这个函数的所有指令建立标签映射
            for (index, instruction) in function.instructions.iter().enumerate() {
                if let Instruction::Label { id, .. } = instruction {
                    self.label_map.insert(*id, instruction_offset + index);
                    if self.debug_mode {
                        println!("  Label {:?} -> PC {}", id, instruction_offset + index);
                    }
                }
            }
            
            instruction_offset += function.instructions.len();
        }

        // 保存程序引用
        self.program = Some(program.clone());

        // 处理主函数的寄存器分配
        if let Some(main_fn_name) = &program.main_function {
            if let Some(function) = program.functions.get(main_fn_name) {
                // 创建寄存器分配器并分析生命周期
                let mut allocator = RegisterAllocator::new();
                allocator.analyze_lifetimes(function);
                
                // 总是使用溢出分配来避免寄存器冲突问题
                let stats = allocator.get_allocation_stats();
                if self.debug_mode {
                    println!("Using spill allocation to avoid register conflicts. Register pressure: {}", stats.register_pressure);
                }
                
                // 创建一个可变的函数副本用于插入溢出指令
                let mut function_copy = function.clone();
                
                // 设置栈指针寄存器（使用特殊的栈指针寄存器）
                let stack_reg = RegisterId(999997); // 特殊的栈指针寄存器
                allocator.set_stack_register(stack_reg);
                
                // 使用支持溢出的分配方法
                let _allocation_result = allocator.allocate_registers_with_spill(&mut function_copy)?;
                allocator.apply_allocation(&mut self.vm)?;
                
                if self.debug_mode {
                    allocator.print_allocation();
                }
                
                Ok(())
            } else {
                Err(format!("Main function '{}' not found", main_fn_name))
            }
        } else {
            Err("No main function specified".to_string())
        }
    }

    /// 执行主函数
    fn execute_main_function(&mut self) -> Result<i64, String> {
        let program = self.program.as_ref()
            .ok_or("No program prepared for execution")?;

        // 找到主函数的入口标签
        let main_fn_name = program.main_function.as_ref()
            .ok_or("No main function specified")?;
        let main_fn = program.functions.get(main_fn_name)
            .ok_or("Main function not found")?;
            
        let main_entry_label = main_fn.instructions.iter()
            .find_map(|instr| {
                if let Instruction::Label { id, .. } = instr {
                    Some(*id)
                } else {
                    None
                }
            })
            .ok_or("Cannot find entry label for main function")?;

        // 设置程序计数器
        self.vm.pc = *self.label_map.get(&main_entry_label)
            .ok_or("Cannot find PC address for main function entry label")?;

        // 执行指令循环
        self.execute_instruction_loop()
    }

    /// 执行指令循环
    fn execute_instruction_loop(&mut self) -> Result<i64, String> {
        let mut skip_next_move = false; // 标记是否跳过下一条Move指令
        
        loop {
            if self.vm.pc >= self.all_instructions.len() {
                break;
            }

            let instruction = self.all_instructions[self.vm.pc].clone();
            
            // 检查是否需要跳过这条Move指令
            if skip_next_move {
                skip_next_move = false; // 首先重置标志
                if let Instruction::Move { dst, src, .. } = &instruction {
                    if let Operand::Register { id } = src {
                        // 检查是否是从r0读取的Move指令（检查虚拟寄存器或物理寄存器映射）
                        let is_from_r0 = self.vm.register_mapping.get(id).copied() == Some(0) || 
                                        id == &karte_lir::RegisterId(0);
                        if is_from_r0 {
                            if self.debug_mode {
                                println!("PC: {}, Skipping redundant Move after Call: {:?}", self.vm.pc, instruction);
                            }
                            self.vm.pc += 1;
                            continue;
                        }
                    }
                }
                // 如果不是需要跳过的Move指令，继续正常执行
            }
            
            if self.debug_mode {
                println!("PC: {}, Executing: {:?}", self.vm.pc, instruction);
                self.vm.print_state();
            }

            let exec_result = self.execute_instruction(&instruction)?;
            
            // 检查是否应该退出程序
            if let Some(exit_code) = exec_result {
                return Ok(exit_code);
            }
            
            // 处理Call指令的特殊逻辑：执行整个调用过程并获取结果
            let mut call_completed = false;
            if let Instruction::Call { target, args, result, .. } = &instruction {
                // Call指令已经执行完毕，现在需要等待函数返回并获取结果
                let original_pc = self.vm.pc;
                
                // 执行被调用的函数直到返回
                while !self.vm.call_stack.is_empty() {
                    if self.vm.pc >= self.all_instructions.len() {
                        return Err("Program counter out of bounds during function call".to_string());
                    }
                    
                    let next_instruction = self.all_instructions[self.vm.pc].clone();
                    if self.debug_mode {
                        println!("PC: {}, Executing: {:?}", self.vm.pc, next_instruction);
                        self.vm.print_state();
                    }
                    
                    let exec_result = self.execute_instruction(&next_instruction)?;
                    if let Some(exit_code) = exec_result {
                        return Ok(exit_code);
                    }
                    
                    // 处理PC递增
                    if let Instruction::Call { .. } | Instruction::CallIndirect { .. } | 
                       Instruction::Jump { .. } | Instruction::JumpEqual { .. } | 
                       Instruction::JumpNotEqual { .. } | Instruction::JumpGreater { .. } | 
                       Instruction::JumpGreaterEqual { .. } | Instruction::JumpLess { .. } | 
                       Instruction::JumpLessEqual { .. } | Instruction::Return { .. } = &next_instruction {
                        // 这些指令可能改变PC，不需要递增
                    } else {
                        self.vm.pc += 1;
                    }
                }
                
                // 函数已返回，处理结果
                if let Some(result_reg) = result {
                    if self.debug_mode {
                        println!("  -> Call completed, result: r0 = {} -> {:?}", self.vm.registers[0], result_reg);
                        println!("  -> Before setting result: {:?} = {}", result_reg, 
                            self.vm.get_virtual_register(result_reg).unwrap_or(0));
                    }
                    self.vm.set_virtual_register(result_reg, self.vm.registers[0])?;
                    if self.debug_mode {
                        println!("  -> After setting result: {:?} = {}", result_reg, 
                            self.vm.get_virtual_register(result_reg).unwrap_or(0));
                    }
                    
                    // 标记跳过下一条可能的冗余Move指令
                    skip_next_move = true;
                }
                
                // 标记Call已完成，主循环应该跳过PC递增
                call_completed = true;
            }
            
            // 如果PC没有改变，则递增
            if !call_completed && match &instruction {
                Instruction::Call { .. } | Instruction::CallIndirect { .. } | 
                Instruction::Jump { .. } | Instruction::JumpEqual { .. } | 
                Instruction::JumpNotEqual { .. } | Instruction::JumpGreater { .. } | 
                Instruction::JumpGreaterEqual { .. } | Instruction::JumpLess { .. } | 
                Instruction::JumpLessEqual { .. } | Instruction::Return { .. } => false,
                _ => true
            } {
                self.vm.pc += 1;
            }
        }

        Ok(0) // 默认返回值
    }

    /// 执行单条指令
    /// 返回 Some(exit_code) 如果程序应该退出，None 如果继续执行
    fn execute_instruction(&mut self, instruction: &Instruction) -> Result<Option<i64>, String> {
        match instruction {
            Instruction::Move { dst, src, .. } => {
                // 检查是否是溢出相关的特殊指令
                if dst.0 == 999998 {
                    // 这是一个存储到内存的溢出指令
                    // 在真实实现中，这里应该将值存储到栈中
                    if self.debug_mode {
                        println!("  -> Spill store instruction (simplified: no-op)");
                    }
                    return Ok(None);
                }
                
                let value = self.get_operand_value(src)?;
                if self.debug_mode {
                    println!("  -> Move: {:?} = {} (was {}), source: {:?}", dst, value, 
                        self.vm.get_virtual_register(dst).unwrap_or(0), src);
                    
                    // 额外的调试信息
                    if let Operand::Register { id } = src {
                        println!("  -> Source register {:?} value: {}", id, 
                            self.vm.get_virtual_register(id).unwrap_or(0));
                        if let Some(&physical_reg) = self.vm.register_mapping.get(id) {
                            println!("  -> Source physical register r{}: {}", physical_reg, 
                                self.vm.registers[physical_reg as usize]);
                        }
                    }
                }
                self.vm.set_virtual_register(dst, value)?;
                Ok(None)
            }
            
            Instruction::Add { dst, src1, src2, .. } => {
                let v1 = self.get_operand_value(src1)?;
                let v2 = self.get_operand_value(src2)?;
                self.vm.set_virtual_register(dst, v1 + v2)?;
                Ok(None)
            }
            
            Instruction::Sub { dst, src1, src2, .. } => {
                let v1 = self.get_operand_value(src1)?;
                let v2 = self.get_operand_value(src2)?;
                self.vm.set_virtual_register(dst, v1 - v2)?;
                Ok(None)
            }
            
            Instruction::Mul { dst, src1, src2, .. } => {
                let v1 = self.get_operand_value(src1)?;
                let v2 = self.get_operand_value(src2)?;
                let result = v1 * v2;
                if self.debug_mode {
                    println!("  -> Mul: {} * {} = {} -> {:?}", v1, v2, result, dst);
                }
                self.vm.set_virtual_register(dst, result)?;
                if self.debug_mode {
                    println!("  -> After Mul: {:?} = {}", dst, self.vm.get_virtual_register(dst).unwrap_or(0));
                }
                Ok(None)
            }
            
            Instruction::Div { dst, src1, src2, .. } => {
                let v1 = self.get_operand_value(src1)?;
                let v2 = self.get_operand_value(src2)?;
                if v2 == 0 {
                    return Err("Division by zero".to_string());
                }
                self.vm.set_virtual_register(dst, v1 / v2)?;
                Ok(None)
            }
            
            Instruction::Compare { src1, src2, .. } => {
                let v1 = self.get_operand_value(src1)?;
                let v2 = self.get_operand_value(src2)?;
                self.vm.compare(v1, v2);
                Ok(None)
            }
            
            Instruction::Jump { target, .. } => {
                self.vm.pc = *self.label_map.get(target)
                    .ok_or(format!("Label {:?} not found", target))?;
                Ok(None)
            }
            
            Instruction::JumpEqual { target, .. } => {
                if self.vm.check_condition(JumpCondition::Equal) {
                    self.vm.pc = *self.label_map.get(target)
                        .ok_or(format!("Label {:?} not found", target))?;
                } else {
                    self.vm.pc += 1; // 条件不满足时递增PC
                }
                Ok(None)
            }
            
            Instruction::JumpNotEqual { target, .. } => {
                if self.vm.check_condition(JumpCondition::NotEqual) {
                    self.vm.pc = *self.label_map.get(target)
                        .ok_or(format!("Label {:?} not found", target))?;
                } else {
                    self.vm.pc += 1; // 条件不满足时递增PC
                }
                Ok(None)
            }
            
            Instruction::JumpGreater { target, .. } => {
                if self.vm.check_condition(JumpCondition::Greater) {
                    self.vm.pc = *self.label_map.get(target)
                        .ok_or(format!("Label {:?} not found", target))?;
                } else {
                    self.vm.pc += 1; // 条件不满足时递增PC
                }
                Ok(None)
            }
            
            Instruction::JumpGreaterEqual { target, .. } => {
                if self.vm.check_condition(JumpCondition::GreaterEqual) {
                    self.vm.pc = *self.label_map.get(target)
                        .ok_or(format!("Label {:?} not found", target))?;
                } else {
                    self.vm.pc += 1; // 条件不满足时递增PC
                }
                Ok(None)
            }
            
            Instruction::JumpLess { target, .. } => {
                if self.vm.check_condition(JumpCondition::Less) {
                    self.vm.pc = *self.label_map.get(target)
                        .ok_or(format!("Label {:?} not found", target))?;
                } else {
                    self.vm.pc += 1; // 条件不满足时递增PC
                }
                Ok(None)
            }
            
            Instruction::JumpLessEqual { target, .. } => {
                if self.vm.check_condition(JumpCondition::LessEqual) {
                    self.vm.pc = *self.label_map.get(target)
                        .ok_or(format!("Label {:?} not found", target))?;
                } else {
                    self.vm.pc += 1; // 条件不满足时递增PC
                }
                Ok(None)
            }
            
            Instruction::Call { target, args, result, .. } => {
                // 函数调用：设置参数并跳转到目标函数
                if self.debug_mode {
                    println!("  -> Executing Call to {:?} with args {:?}", target, args);
                }
                
                // 设置参数到物理寄存器r0, r1, r2, ...
                // 在设置之前先读取所有参数值，避免寄存器冲突
                let mut arg_values = Vec::new();
                for arg_reg in args.iter() {
                    let arg_value = self.vm.get_virtual_register(arg_reg)?;
                    arg_values.push(arg_value);
                    if self.debug_mode {
                        println!("  -> Argument {:?} = {}", arg_reg, arg_value);
                    }
                }
                
                // 现在设置物理寄存器
                for (i, arg_value) in arg_values.iter().enumerate() {
                    self.vm.set_physical_register(i as u8, *arg_value)?;
                    if self.debug_mode {
                        println!("  -> Setting r{} = {}", i, arg_value);
                    }
                }
                
                // 保存返回地址并跳转
                self.vm.call_stack.push(self.vm.pc + 1);
                self.vm.pc = *self.label_map.get(target)
                    .ok_or(format!("Function {:?} not found", target))?;
                    
                if self.debug_mode {
                    println!("  -> Jumping to PC: {}", self.vm.pc);
                }
                
                Ok(None) // 继续执行
            }
            
            Instruction::CallIndirect { function_register, args, result, .. } => {
                // 间接函数调用：通过寄存器中的函数地址调用
                if self.debug_mode {
                    println!("  -> Executing CallIndirect");
                }
                
                // 获取函数地址（标签ID）
                let function_address = self.vm.get_virtual_register(function_register)?;
                let target_label = karte_lir::LabelId(function_address as usize);
                
                if self.debug_mode {
                    println!("  -> Function address: {}, target label: {:?}", function_address, target_label);
                }
                
                // 设置参数到寄存器r0, r1, r2, ...
                for (i, arg_reg) in args.iter().enumerate() {
                    let arg_value = self.vm.get_virtual_register(arg_reg)?;
                    self.vm.set_physical_register(i as u8, arg_value)?;
                    if self.debug_mode {
                        println!("  -> Setting r{} = {}", i, arg_value);
                    }
                }
                
                // 保存返回地址
                self.vm.call_stack.push(self.vm.pc + 1);
                
                // 跳转到目标函数
                if let Some(&target_pc) = self.label_map.get(&target_label) {
                    self.vm.pc = target_pc;
                    if self.debug_mode {
                        println!("  -> Jumping to PC: {}", target_pc);
                    }
                    Ok(None) // 继续执行
                } else {
                    return Err(format!("Function address {:?} not found in label map", target_label));
                }
            }
            
            Instruction::Return { value, .. } => {
                // 处理返回值
                if let Some(return_reg) = value {
                    let return_value = self.vm.get_virtual_register(return_reg)?;
                    self.vm.registers[0] = return_value; // 将返回值放在r0中
                }
                
                // 恢复调用栈
                if let Some(return_pc) = self.vm.call_stack.pop() {
                    self.vm.pc = return_pc;
                    Ok(None) // 继续执行
                } else {
                    // 从主函数返回，程序结束
                    let result = if let Some(reg) = value {
                        self.vm.get_virtual_register(reg)?
                    } else {
                        0
                    };
                    Ok(Some(result)) // 程序退出
                }
            }
            
            Instruction::Label { .. } => {
                // 标签不是真正的指令，只是标记位置
                Ok(None)
            }
            
            Instruction::Nop { .. } => {
                // 空操作
                Ok(None)
            }
        }
    }

    /// 获取操作数的值
    fn get_operand_value(&self, operand: &Operand) -> Result<i64, String> {
        match operand {
            Operand::Register { id } => self.vm.get_virtual_register(id),
            Operand::Immediate { value } => Ok(*value),
            Operand::Label { id } => {
                Ok(*self.label_map.get(id).unwrap_or(&0) as i64)
            }
            Operand::Memory { base, offset } => {
                // 内存访问的简化实现
                let base_value = self.vm.get_virtual_register(base)?;
                let address = (base_value + offset) as usize;
                self.memory.read_memory(address)
            }
        }
    }

    /// 获取虚拟机引用
    pub fn get_vm(&self) -> &VirtualMachine {
        &self.vm
    }

    /// 获取内存管理器引用
    pub fn get_memory(&self) -> &MemoryManager {
        &self.memory
    }

    /// 设置调试模式
    pub fn set_debug_mode(&mut self, debug: bool) {
        self.debug_mode = debug;
    }
}

impl Default for InstructionExecutor {
    fn default() -> Self {
        Self::new(false)
    }
} 