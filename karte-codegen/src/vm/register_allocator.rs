//! 寄存器分配算法
//! 
//! 实现了线性扫描寄存器分配算法，支持寄存器溢出到内存

use super::{NUM_REGISTERS, VirtualMachine};
use karte_lir::{Instruction, LirFunction, RegisterId, Operand, LabelId};
use karte_diagnostics::Span;
use std::collections::HashMap;

/// 寄存器生命周期信息
#[derive(Debug, Clone)]
pub struct RegisterLifetime {
    /// 寄存器ID
    pub register: RegisterId,
    /// 首次使用位置
    pub start: usize,
    /// 最后使用位置
    pub end: usize,
}

/// 溢出槽信息
#[derive(Debug, Clone)]
pub struct SpillSlot {
    /// 槽ID
    pub slot_id: usize,
    /// 内存偏移量（相对于栈指针）
    pub offset: i64,
    /// 栈偏移量（用于实际内存访问）
    pub stack_offset: i64,
    /// 被溢出的寄存器
    pub spilled_register: RegisterId,
}

/// 寄存器分配器
#[derive(Debug)]
#[deprecated(note = "use karte-lir/pass/register_allocation instead")]
pub struct RegisterAllocator {
    /// 可用的物理寄存器
    available_registers: Vec<u8>,
    /// 寄存器生命周期信息
    lifetimes: Vec<RegisterLifetime>,
    /// 虚拟寄存器到物理寄存器的分配结果
    allocation: HashMap<RegisterId, u8>,
    /// 溢出槽管理
    spill_slots: Vec<SpillSlot>,
    /// 虚拟寄存器到溢出槽的映射
    spilled_registers: HashMap<RegisterId, usize>,
    /// 下一个可用的溢出槽偏移量
    next_spill_offset: i64,
    /// 栈指针寄存器（用于访问溢出槽）
    stack_register: Option<RegisterId>,
}

/// 寄存器分配结果
#[derive(Debug, Clone)]
pub struct RegisterAllocationResult {
    /// 成功分配的寄存器映射
    pub register_assignments: HashMap<RegisterId, u8>,
    /// 溢出的寄存器映射
    pub spill_assignments: HashMap<RegisterId, SpillSlot>,
}

/// 分配统计信息
#[derive(Debug, Clone)]
pub struct AllocationStats {
    pub total_virtual_registers: usize,
    pub allocated_physical_registers: usize,
    pub available_physical_registers: usize,
    pub register_pressure: usize,
    pub spilled_registers: usize,
    pub spill_slots_used: usize,
}

impl RegisterAllocator {
    /// 创建新的寄存器分配器
    pub fn new() -> Self {
        // 🔧 修复：只使用可分配的寄存器，排除特殊寄存器
        // 根据调用约定，r5(返回地址)、r6(栈指针)、r7(帧指针)是特殊寄存器
        // 只有 r0-r4 可以用于寄存器分配
        let available_registers = vec![0, 1, 2, 3, 4];
        
        Self {
            available_registers,
            lifetimes: Vec::new(),
            allocation: HashMap::new(),
            spill_slots: Vec::new(),
            spilled_registers: HashMap::new(),
            next_spill_offset: -8, // 从栈顶开始分配，每个slot 8字节
            stack_register: None,
        }
    }

    /// 设置栈指针寄存器
    pub fn set_stack_register(&mut self, stack_reg: RegisterId) {
        self.stack_register = Some(stack_reg);
    }

    /// 分析函数中虚拟寄存器的生命周期
    pub fn analyze_lifetimes(&mut self, function: &LirFunction) {
        let mut register_uses: HashMap<RegisterId, (Option<usize>, Option<usize>)> = HashMap::new();

        // 扫描所有指令，记录每个寄存器的使用位置
        for (pos, instruction) in function.instructions.iter().enumerate() {
            let registers = self.extract_registers_from_instruction(instruction);
            
            for reg in registers {
                let entry = register_uses.entry(reg).or_insert((None, None));
                
                // 更新首次使用位置
                if entry.0.is_none() {
                    entry.0 = Some(pos);
                }
                
                // 更新最后使用位置
                entry.1 = Some(pos);
            }
        }

        // 识别循环结构并延长循环中使用的寄存器生命周期
        let loop_registers = self.identify_loop_registers(function);

        // 构建生命周期信息
        self.lifetimes.clear();
        for (register, (start, end)) in register_uses {
            if let (Some(start), Some(mut end)) = (start, end) {
                // 如果寄存器在循环中使用，延长其生命周期到程序结束
                if loop_registers.contains(&register) {
                    end = function.instructions.len().saturating_sub(1);
                }
                
                self.lifetimes.push(RegisterLifetime {
                    register,
                    start,
                    end,
                });
            }
        }
        
        // 修正生命周期：确保在同一条指令中使用的寄存器有重叠的生命周期
        for (index, instruction) in function.instructions.iter().enumerate() {
            let used_registers = self.extract_registers_from_instruction(instruction);
            if used_registers.len() > 1 {
                // 如果一条指令使用多个寄存器，确保它们的生命周期都包含这条指令
                let mut max_end = index;
                for &reg in &used_registers {
                    if let Some(lifetime) = self.lifetimes.iter().find(|lt| lt.register == reg) {
                        max_end = max_end.max(lifetime.end);
                    }
                }
                
                // 扩展所有寄存器的生命周期到最大值，确保重叠
                for &reg in &used_registers {
                    if let Some(lifetime) = self.lifetimes.iter_mut().find(|lt| lt.register == reg) {
                        lifetime.end = lifetime.end.max(max_end);
                    }
                }
            }
        }

        // 按照起始位置排序
        self.lifetimes.sort_by_key(|lt| lt.start);
    }

    /// 识别在循环中使用的寄存器
    fn identify_loop_registers(&self, function: &LirFunction) -> std::collections::HashSet<RegisterId> {
        let mut loop_registers = std::collections::HashSet::new();
        
        // 查找所有跳转指令，识别可能的循环结构
        let mut jump_targets = std::collections::HashMap::new();
        let mut backward_jumps = Vec::new();
        
        for (pos, instruction) in function.instructions.iter().enumerate() {
            match instruction {
                Instruction::Label { id, .. } => {
                    jump_targets.insert(id, pos);
                }
                Instruction::Jump { target, .. } |
                Instruction::JumpEqual { target, .. } |
                Instruction::JumpNotEqual { target, .. } |
                Instruction::JumpLess { target, .. } |
                Instruction::JumpLessEqual { target, .. } |
                Instruction::JumpGreater { target, .. } |
                Instruction::JumpGreaterEqual { target, .. } => {
                    if let Some(&target_pos) = jump_targets.get(target) {
                        if target_pos <= pos {
                            // 这是一个向后跳转，可能是循环
                            backward_jumps.push((target_pos, pos));
                        }
                    }
                }
                _ => {}
            }
        }
        
        // 对于每个识别的循环，收集其中使用的寄存器
        for (loop_start, loop_end) in backward_jumps {
            for pos in loop_start..=loop_end {
                if pos < function.instructions.len() {
                    let registers = self.extract_registers_from_instruction(&function.instructions[pos]);
                    for reg in registers {
                        loop_registers.insert(reg);
                    }
                }
            }
        }
        
        loop_registers
    }

    /// 从指令中提取所有涉及的寄存器
    fn extract_registers_from_instruction(&self, instruction: &Instruction) -> Vec<RegisterId> {
        let mut registers = Vec::new();

        match instruction {
            Instruction::Move { dst, src, .. } => {
                registers.push(*dst);
                if let Operand::Register { id } = src {
                    registers.push(*id);
                } else if let Operand::Memory { base, .. } = src {
                    registers.push(*base);
                }
            }
            Instruction::Add { dst, src1, src2, .. } |
            Instruction::Sub { dst, src1, src2, .. } |
            Instruction::Mul { dst, src1, src2, .. } |
            Instruction::Div { dst, src1, src2, .. } => {
                registers.push(*dst);
                if let Operand::Register { id } = src1 {
                    registers.push(*id);
                } else if let Operand::Memory { base, .. } = src1 {
                    registers.push(*base);
                }
                if let Operand::Register { id } = src2 {
                    registers.push(*id);
                } else if let Operand::Memory { base, .. } = src2 {
                    registers.push(*base);
                }
            }
            Instruction::Compare { src1, src2, .. } => {
                if let Operand::Register { id } = src1 {
                    registers.push(*id);
                } else if let Operand::Memory { base, .. } = src1 {
                    registers.push(*base);
                }
                if let Operand::Register { id } = src2 {
                    registers.push(*id);
                } else if let Operand::Memory { base, .. } = src2 {
                    registers.push(*base);
                }
            }
            Instruction::Call { args, result, .. } => {
                registers.extend(args.iter().cloned());
                if let Some(result_reg) = result {
                    registers.push(*result_reg);
                }
            }
            Instruction::Return { value, .. } => {
                if let Some(reg) = value {
                    registers.push(*reg);
                }
            }
            _ => {} // 其他指令不涉及寄存器或只涉及标签
        }

        registers
    }

    /// 查找与给定寄存器在同一条指令中使用的其他寄存器
    fn find_instruction_conflicts(&self, function: &LirFunction, target_lifetime: &RegisterLifetime) -> Vec<RegisterId> {
        let mut conflicts = Vec::new();
        
        // 检查目标寄存器生命周期内的每条指令
        for pos in target_lifetime.start..=target_lifetime.end.min(function.instructions.len().saturating_sub(1)) {
            if let Some(instruction) = function.instructions.get(pos) {
                let registers_in_instruction = self.extract_registers_from_instruction(instruction);
                
                // 如果这条指令包含目标寄存器，则收集所有其他寄存器
                if registers_in_instruction.contains(&target_lifetime.register) {
                    for reg in registers_in_instruction {
                        if reg != target_lifetime.register && !conflicts.contains(&reg) {
                            conflicts.push(reg);
                        }
                    }
                }
            }
        }
        
        conflicts
    }

    /// 使用线性扫描算法进行寄存器分配（支持溢出）
    pub fn allocate_registers_with_spill(&mut self, function: &mut LirFunction) -> Result<HashMap<RegisterId, u8>, String> {
        self.allocation.clear();
        self.spilled_registers.clear();
        self.spill_slots.clear();
        self.next_spill_offset = -8;
        
        let mut active_intervals: Vec<RegisterLifetime> = Vec::new();
        let mut spill_instructions: Vec<(usize, Instruction)> = Vec::new();

        // 克隆lifetimes以避免借用冲突
        let lifetimes = self.lifetimes.clone();
        for lifetime in &lifetimes {
            // 释放已经结束生命周期的寄存器
            let mut i = 0;
            while i < active_intervals.len() {
                // 只有当寄存器完全结束后（end < start），才能被复用
                if active_intervals[i].end < lifetime.start {
                    let ended_interval = active_intervals.remove(i);
                    if let Some(&physical_reg) = self.allocation.get(&ended_interval.register) {
                        self.available_registers.push(physical_reg);
                    }
                } else {
                    i += 1;
                }
            }

            // 检查当前寄存器是否与已分配寄存器在同一条指令中使用
            let conflicting_registers = self.find_instruction_conflicts(function, lifetime);
            let mut forbidden_registers = std::collections::HashSet::new();
            
            // 调试信息
            if !conflicting_registers.is_empty() {
                println!("Register {:?} conflicts with: {:?}", lifetime.register, conflicting_registers);
            }
            
            // 禁止冲突寄存器使用的物理寄存器
            for conflicting_reg in conflicting_registers {
                if let Some(&physical_reg) = self.allocation.get(&conflicting_reg) {
                    forbidden_registers.insert(physical_reg);
                    println!("  -> Forbidding physical register r{} due to conflict with {:?}", 
                            physical_reg, conflicting_reg);
                }
            }

            // 也要考虑生命周期重叠的寄存器冲突
            for other_lifetime in &lifetimes {
                if other_lifetime.register != lifetime.register &&
                   !(other_lifetime.end < lifetime.start || lifetime.end < other_lifetime.start) {
                    // 生命周期重叠
                    if let Some(&physical_reg) = self.allocation.get(&other_lifetime.register) {
                        // 检查是否在同一条指令中使用
                        if self.registers_used_together(function, &lifetime.register, &other_lifetime.register) {
                            forbidden_registers.insert(physical_reg);
                            println!("  -> Forbidding physical register r{} due to lifetime overlap with {:?}", 
                                    physical_reg, other_lifetime.register);
                        }
                    }
                }
            }

            // 为当前寄存器分配物理寄存器，避免冲突
            let mut assigned = false;
            let mut best_register = None;
            
            // 优先选择没有被禁止的寄存器
            for &physical_reg in &self.available_registers {
                if !forbidden_registers.contains(&physical_reg) {
                    best_register = Some(physical_reg);
                    break;
                }
            }
            
            if let Some(physical_reg) = best_register {
                // 找到一个不冲突的物理寄存器
                self.allocation.insert(lifetime.register, physical_reg);
                active_intervals.push(lifetime.clone());
                // 从可用寄存器中移除
                if let Some(pos) = self.available_registers.iter().position(|&r| r == physical_reg) {
                    self.available_registers.remove(pos);
                }
                assigned = true;
                println!("  -> Assigned {:?} to physical register r{}", lifetime.register, physical_reg);
            }
            
            if !assigned {
                // 需要溢出 - 选择结束最晚的寄存器进行溢出
                println!("  -> No available registers, performing spill for {:?}", lifetime.register);
                self.perform_register_spill(&mut active_intervals, lifetime, &mut spill_instructions)?;
            }
        }

        // 插入溢出指令到函数中
        self.insert_spill_instructions(function, spill_instructions)?;

        // 打印最终分配结果
        println!("Final register allocation:");
        for (virtual_reg, &physical_reg) in &self.allocation {
            println!("  {:?} -> r{}", virtual_reg, physical_reg);
        }

        Ok(self.allocation.clone())
    }

    /// 检查两个寄存器是否在同一条指令中使用
    fn registers_used_together(&self, function: &LirFunction, reg1: &RegisterId, reg2: &RegisterId) -> bool {
        for instruction in &function.instructions {
            let registers = self.extract_registers_from_instruction(instruction);
            if registers.contains(reg1) && registers.contains(reg2) {
                return true;
            }
        }
        false
    }

    /// 执行寄存器溢出
    fn perform_register_spill(
        &mut self,
        active_intervals: &mut Vec<RegisterLifetime>,
        current: &RegisterLifetime,
    spill_instructions: &mut Vec<(usize, Instruction)>,
    ) -> Result<(), String> {
        // 找到结束最晚的活跃区间
        if let Some(max_end_idx) = active_intervals
            .iter()
            .enumerate()
            .max_by_key(|(_, interval)| interval.end)
            .map(|(idx, _)| idx)
        {
            let spill_interval = &active_intervals[max_end_idx];
            
            // 如果当前区间结束得更早，溢出当前区间
            if current.end < spill_interval.end {
                self.spill_register_to_memory(current, spill_instructions)?;
            } else {
                // 溢出结束最晚的区间，并为当前区间分配其物理寄存器
                let spilled_register = spill_interval.register;
                if let Some(&physical_reg) = self.allocation.get(&spilled_register) {
                    // 生成溢出指令
                    self.spill_register_to_memory(spill_interval, spill_instructions)?;
                    
                    // 移除溢出寄存器的分配，为当前寄存器分配该物理寄存器
                    self.allocation.remove(&spilled_register);
                    self.allocation.insert(current.register, physical_reg);
                    
                    // 更新活跃区间
                    active_intervals.remove(max_end_idx);
                    active_intervals.push(current.clone());
                }
            }
        }

        Ok(())
    }

    /// 将寄存器溢出到内存
    fn spill_register_to_memory(
        &mut self,
        lifetime: &RegisterLifetime,
        spill_instructions: &mut Vec<(usize, Instruction)>,
    ) -> Result<(), String> {
        // 分配溢出槽
        let slot_id = self.spill_slots.len();
        let spill_slot = SpillSlot {
            slot_id,
            offset: self.next_spill_offset,
            stack_offset: self.next_spill_offset, // 直接使用偏移量作为栈偏移
            spilled_register: lifetime.register,
        };
        
        self.spill_slots.push(spill_slot.clone());
        self.spilled_registers.insert(lifetime.register, slot_id);
        self.next_spill_offset -= 8; // 下一个槽位

        // 获取栈指针寄存器
        let stack_reg = self.stack_register.ok_or("Stack register not set for spilling")?;

        // 生成存储指令：mov [stack + offset], register
        let store_instruction = Instruction::Move {
            dst: RegisterId(999999), // 临时标记，后续会被替换为内存操作
            src: Operand::Register { id: lifetime.register },
            span: Span::dummy(),
        };

        // 在寄存器首次定义后插入存储指令
        spill_instructions.push((lifetime.start + 1, store_instruction));

        println!("Spilling register {:?} to slot {} (offset {})", 
                lifetime.register, slot_id, spill_slot.offset);

        Ok(())
    }

    /// 插入溢出指令到函数中
    fn insert_spill_instructions(
        &mut self,
        function: &mut LirFunction,
        mut spill_instructions: Vec<(usize, Instruction)>,
    ) -> Result<(), String> {
        // 按位置倒序排序，这样插入时不会影响前面的位置
        spill_instructions.sort_by(|a, b| b.0.cmp(&a.0));

        let stack_reg = self.stack_register.ok_or("Stack register not set")?;

        for (pos, instruction) in spill_instructions {
            match instruction {
                Instruction::Move { src, .. } => {
                    if let Operand::Register { id: spilled_reg } = src {
                        // 找到对应的溢出槽
                        if let Some(&slot_id) = self.spilled_registers.get(&spilled_reg) {
                            let slot = &self.spill_slots[slot_id];
                            
                            // 创建存储指令：mov [stack + offset], register
                            let store_instruction = Instruction::Move {
                                dst: RegisterId(999998), // 特殊标记表示这是内存目标
                                src: Operand::Register { id: spilled_reg },
                                span: Span::dummy(),
                            };

                            // 插入到指定位置
                            if pos < function.instructions.len() {
                                function.instructions.insert(pos, store_instruction);
                            } else {
                                function.instructions.push(store_instruction);
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// 使用线性扫描算法进行寄存器分配（旧版本，保持向后兼容）
    pub fn allocate_registers(&mut self) -> Result<HashMap<RegisterId, u8>, String> {
        self.allocation.clear();
        let mut active_intervals: Vec<RegisterLifetime> = Vec::new();

        // 克隆lifetimes以避免借用冲突
        let lifetimes = self.lifetimes.clone();
        
        // 🔧 调试：打印初始状态
        println!("=== 寄存器分配开始 ===");
        println!("可用寄存器: {:?}", self.available_registers);
        println!("需要分配的寄存器数量: {}", lifetimes.len());
        println!("寄存器生命周期:");
        for lifetime in &lifetimes {
            println!("  {:?}: {} -> {}", lifetime.register, lifetime.start, lifetime.end);
        }
        
        for lifetime in &lifetimes {
            // 释放已经结束生命周期的寄存器
            let mut i = 0;
            while i < active_intervals.len() {
                if active_intervals[i].end < lifetime.start {
                    let ended_interval = active_intervals.remove(i);
                    if let Some(&physical_reg) = self.allocation.get(&ended_interval.register) {
                        self.available_registers.push(physical_reg);
                    }
                } else {
                    i += 1;
                }
            }

            // 🔧 修复：更智能的冲突检测，只检查真正同时活跃的寄存器
            let mut forbidden_registers = std::collections::HashSet::new();
            
            // 检查与当前活跃区间的冲突
            for active_interval in &active_intervals {
                // 只有当两个区间真正重叠时才认为冲突
                if !(active_interval.end < lifetime.start || lifetime.end < active_interval.start) {
                    if let Some(&physical_reg) = self.allocation.get(&active_interval.register) {
                        forbidden_registers.insert(physical_reg);
                    }
                }
            }

            // 为当前寄存器分配物理寄存器，避免冲突
            let mut assigned = false;
            
            println!("  分配寄存器 {:?}:", lifetime.register);
            println!("    可用寄存器: {:?}", self.available_registers);
            println!("    禁用寄存器: {:?}", forbidden_registers);
            
            // 🔧 修复：更高效的寄存器分配策略
            // 首先尝试分配未被禁止的寄存器
            for &physical_reg in &self.available_registers.clone() {
                if !forbidden_registers.contains(&physical_reg) {
                    // 找到一个不冲突的物理寄存器
                    self.allocation.insert(lifetime.register, physical_reg);
                    active_intervals.push(lifetime.clone());
                    // 从可用寄存器中移除
                    if let Some(pos) = self.available_registers.iter().position(|&r| r == physical_reg) {
                        self.available_registers.remove(pos);
                    }
                    assigned = true;
                    println!("    -> 成功分配到 r{}", physical_reg);
                    break;
                }
            }
            
            if !assigned {
                println!("    -> 分配失败，需要溢出");
                // 如果没有可用寄存器，需要溢出（spill）
                return self.handle_register_spill(&mut active_intervals, lifetime);
            }
        }

        Ok(self.allocation.clone())
    }

    /// 处理寄存器溢出（旧版本，返回错误）
    fn handle_register_spill(
        &mut self,
        active_intervals: &mut Vec<RegisterLifetime>,
        current: &RegisterLifetime,
    ) -> Result<HashMap<RegisterId, u8>, String> {
        // 找到结束最晚的活跃区间
        if let Some(max_end_idx) = active_intervals
            .iter()
            .enumerate()
            .max_by_key(|(_, interval)| interval.end)
            .map(|(idx, _)| idx)
        {
            let spill_interval = &active_intervals[max_end_idx];
            
            // 如果当前区间结束得更早，溢出当前区间
            if current.end < spill_interval.end {
                return Err(format!(
                    "Register spill required for {:?} (use allocate_registers_with_spill for spill support)",
                    current.register
                ));
            } else {
                // 溢出结束最晚的区间
                let spilled_register = spill_interval.register;
                
                return Err(format!(
                    "Register spill required for {:?} (use allocate_registers_with_spill for spill support)",
                    spilled_register
                ));
            }
        }

        Err("No registers available for allocation".to_string())
    }

    /// 应用寄存器分配结果到虚拟机
    pub fn apply_allocation(&self, vm: &mut VirtualMachine) -> Result<(), String> {
        vm.register_mapping = self.allocation.clone();
        Ok(())
    }

    /// 获取寄存器分配的统计信息
    pub fn get_allocation_stats(&self) -> AllocationStats {
        AllocationStats {
            total_virtual_registers: self.lifetimes.len(),
            allocated_physical_registers: self.allocation.len(),
            available_physical_registers: self.available_registers.len(),
            register_pressure: self.calculate_max_register_pressure(),
            spilled_registers: self.spilled_registers.len(),
            spill_slots_used: self.spill_slots.len(),
        }
    }

    /// 计算最大寄存器压力
    fn calculate_max_register_pressure(&self) -> usize {
        if self.lifetimes.is_empty() {
            return 0;
        }

        let max_position = self.lifetimes.iter().map(|lt| lt.end).max().unwrap_or(0);
        let mut max_pressure = 0;

        for pos in 0..=max_position {
            let pressure = self.lifetimes.iter()
                .filter(|lt| lt.start <= pos && pos <= lt.end)
                .count();
            max_pressure = max_pressure.max(pressure);
        }

        max_pressure
    }

    /// 打印分配结果（用于调试）
    pub fn print_allocation(&self) {
        println!("=== Register Allocation Results ===");
        println!("Virtual -> Physical mapping:");
        for (virtual_reg, &physical_reg) in &self.allocation {
            println!("  {:?} -> r{}", virtual_reg, physical_reg);
        }
        
        if !self.spilled_registers.is_empty() {
            println!("Spilled registers:");
            for (virtual_reg, &slot_id) in &self.spilled_registers {
                let slot = &self.spill_slots[slot_id];
                println!("  {:?} -> spill slot {} (offset {})", 
                        virtual_reg, slot_id, slot.offset);
            }
        }
        
        println!("Register lifetimes:");
        for lifetime in &self.lifetimes {
            println!("  {:?}: {} -> {}", lifetime.register, lifetime.start, lifetime.end);
        }
        
        let stats = self.get_allocation_stats();
        println!("Statistics: {:?}", stats);
    }

    /// 获取寄存器分配结果
    pub fn get_register_allocation(&self) -> RegisterAllocationResult {
        let mut spill_assignments = HashMap::new();
        
        // 构建溢出分配映射
        for (register, slot_id) in &self.spilled_registers {
            if let Some(spill_slot) = self.spill_slots.get(*slot_id) {
                spill_assignments.insert(*register, spill_slot.clone());
            }
        }
        
        RegisterAllocationResult {
            register_assignments: self.allocation.clone(),
            spill_assignments,
        }
    }
}

impl Default for RegisterAllocator {
    fn default() -> Self {
        Self::new()
    }
} 