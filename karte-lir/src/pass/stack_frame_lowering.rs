//! StackFrameLowering Pass
//! 
//! 实现基于帧指针（Frame Pointer）的栈帧管理，这是修复栈指针不稳定问题的核心解决方案。
//! 
//! 主要功能：
//! 1. 计算栈帧布局，包括本地变量和溢出槽
//! 2. 生成函数序言和尾声代码  
//! 3. 将 alloc 指令转换为基于 FP 的地址计算
//! 4. 处理寄存器溢出的 load/store 操作

use super::{FunctionPass, PassResult, AnalysisManager};
use crate::{LirFunction, RegisterId, Instruction, Operand, LabelId, AllocationType};
use crate::pass::register_allocation::{RegisterAllocationResult, SpillSlot};
use std::collections::HashMap;
use karte_diagnostics::Span;
use crate::pass::memory2reg::Memory2RegAnalysis;

/// 向下对齐到指定边界
fn align_down(value: i64, alignment: i64) -> i64 {
    value - (value % alignment)
}

/// 栈帧布局信息
#[derive(Debug, Clone)]
pub struct StackFrameLayout {
    /// 总栈帧大小
    pub total_frame_size: usize,
    /// 本地变量（alloc）的偏移映射
    pub local_var_offsets: HashMap<RegisterId, i64>,
    /// 溢出槽的偏移映射
    pub spill_slot_offsets: HashMap<usize, i64>,
    /// 下一个可分配的偏移量
    pub next_offset: i64,
}

impl StackFrameLayout {
    pub fn new() -> Self {
        Self {
            total_frame_size: 0,
            local_var_offsets: HashMap::new(),
            spill_slot_offsets: HashMap::new(),
            next_offset: -8, // 从 FP 向下分配
        }
    }

    /// 分配本地变量槽位
    pub fn allocate_local_var(&mut self, var_reg: RegisterId, size: usize) -> i64 {
        let offset = self.next_offset;
        self.local_var_offsets.insert(var_reg, offset);
        self.next_offset -= size as i64;
        self.total_frame_size = (-self.next_offset) as usize;
        println!("🔧 分配本地变量: {:?} -> [FP{}] (size: {})", var_reg, offset, size);
        offset
    }

    /// 分配溢出槽位
    pub fn allocate_spill_slot(&mut self, slot_id: usize) -> i64 {
        let offset = self.next_offset;
        self.spill_slot_offsets.insert(slot_id, offset);
        self.next_offset -= 8; // 每个溢出槽 8 字节
        self.total_frame_size = (-self.next_offset) as usize;
        println!("🔧 分配溢出槽: slot_{} -> [FP{}]", slot_id, offset);
        offset
    }
}

/// StackFrameLowering Pass
pub struct StackFrameLowering {
    /// 栈指针寄存器
    stack_pointer: RegisterId,
    /// 帧指针寄存器  
    frame_pointer: RegisterId,
    /// 🔧 新增：临时寄存器计数器，用于生成唯一的临时虚拟寄存器
    next_temp_register: usize,
}

impl StackFrameLowering {
    pub fn new() -> Self {
        Self {
            // 根据调用约定：r6 = SP, r7 = FP
            // 🔧 关键修复：使用物理寄存器ID而不是虚拟寄存器ID
            stack_pointer: RegisterId(6),
            frame_pointer: RegisterId(7),
            next_temp_register: 1000, // 从1000开始分配临时寄存器
        }
    }
    
    /// 分配临时虚拟寄存器
    fn allocate_temp_register(&mut self) -> RegisterId {
        let temp_reg = RegisterId(self.next_temp_register);
        self.next_temp_register += 1;
        println!("🔧 分配临时虚拟寄存器: {:?}", temp_reg);
        temp_reg
    }

    /// 🔧 新增：获取栈指针和帧指针的物理寄存器ID
    fn get_stack_pointer_physical(&self) -> u8 {
        6 // r6
    }
    
    fn get_frame_pointer_physical(&self) -> u8 {
        7 // r7
    }

    /// 🔧 新增：确保栈指针和帧指针不被寄存器分配器重新分配
    fn ensure_special_registers_reserved(&self, allocation_result: &mut RegisterAllocationResult) {
        // 确保栈指针和帧指针映射到正确的物理寄存器
        allocation_result.register_mapping.insert(self.stack_pointer, self.get_stack_pointer_physical());
        allocation_result.register_mapping.insert(self.frame_pointer, self.get_frame_pointer_physical());
        
        println!("🔧 保留特殊寄存器: SP={:?}->r{}, FP={:?}->r{}", 
                 self.stack_pointer, self.get_stack_pointer_physical(),
                 self.frame_pointer, self.get_frame_pointer_physical());
    }

    /// 计算栈帧布局
    fn calculate_stack_frame_layout(
        &self,
        function: &LirFunction,
        allocation_result: &RegisterAllocationResult,
    ) -> StackFrameLayout {
        let mut layout = StackFrameLayout::new();
        let mut current_offset = 0i64;

        // 🔧 关键修复：收集所有alloc指令生成的栈地址寄存器
        let mut stack_address_registers = std::collections::HashSet::new();
        for instruction in &function.instructions {
            if let Instruction::Alloc { dst, size, .. } = instruction {
                stack_address_registers.insert(*dst);
            }
        }

        // 1. 分配本地变量（由alloc指令分配的寄存器）
        for instruction in &function.instructions {
            if let Instruction::Alloc { dst, size, alignment, allocation_type, .. } = instruction {
                // 🔧 关键修复：只为栈分配的alloc指令分配栈空间
                // 堆分配的alloc指令应该在虚拟机执行时由堆分配器处理
                match allocation_type {
                    AllocationType::Stack => {
                        // 对齐当前偏移量
                        current_offset = align_down(current_offset - (*size as i64), *alignment as i64);
                        layout.local_var_offsets.insert(*dst, current_offset);
                        println!("🔧 分配本地变量: {:?} -> [FP{}] (size: {}, type: Stack)", dst, current_offset, size);
                    }
                    AllocationType::Heap => {
                        // 堆分配不需要在栈帧中分配空间，将在虚拟机执行时处理
                        println!("🔧 跳过堆分配: {:?} (size: {}, type: Heap) - 将在虚拟机执行时处理", dst, size);
                    }
                    AllocationType::Static => {
                        // 静态分配也不需要在栈帧中分配空间
                        println!("🔧 跳过静态分配: {:?} (size: {}, type: Static) - 将在虚拟机执行时处理", dst, size);
                    }
                }
            }
        }

        // 2. 🔧 关键修复：只为非栈地址寄存器分配溢出槽
        // 栈地址寄存器不应该被溢出，因为它们存储的是栈地址而不是数据
        for (register_id, spill_slot) in &allocation_result.spilled_registers {
            if !stack_address_registers.contains(register_id) {
                // 只有非栈地址寄存器才能溢出
                current_offset = align_down(current_offset - 8, 8); // 每个溢出槽8字节对齐
                layout.spill_slot_offsets.insert(spill_slot.slot_id, current_offset);
                println!("🔧 分配溢出槽: slot_{} -> [FP{}]", spill_slot.slot_id, current_offset);
                println!("🔧 为溢出寄存器 {:?} 分配槽位 {}", register_id, spill_slot.slot_id);
            } else {
                println!("🔧 错误：栈地址寄存器 {:?} 被错误地标记为溢出！这是寄存器分配器的bug", register_id);
                // 这种情况不应该发生，如果发生了，说明寄存器分配器有问题
                // 我们应该忽略这个溢出分配，让栈地址寄存器保持其栈地址功能
            }
        }

        // 设置总栈帧大小
        layout.total_frame_size = (-current_offset) as usize;
        layout.next_offset = current_offset;

        println!("🔧 栈帧布局计算完成: 总大小 {} 字节", layout.total_frame_size);
        layout
    }

    /// 生成函数序言
    fn generate_prologue(&self, layout: &StackFrameLayout) -> Vec<Instruction> {
        let mut prologue = Vec::new();
        let span = Span::dummy();

        // 1. push fp (保存调用者的帧指针)
        prologue.push(Instruction::Store64 {
            addr: self.stack_pointer,
            offset: 0,
            src: Operand::Register { id: self.frame_pointer },
            span,
        });
        prologue.push(Instruction::Sub {
            dst: self.stack_pointer,
            src1: Operand::Register { id: self.stack_pointer },
            src2: Operand::Immediate { value: 8 },
            span,
        });

        // 2. mov fp, sp (设置当前函数的帧指针)
        prologue.push(Instruction::Move {
            dst: self.frame_pointer,
            src: Operand::Register { id: self.stack_pointer },
            span,
        });

        // 3. sub sp, sp, #frame_size (分配栈帧空间)
        if layout.total_frame_size > 0 {
            prologue.push(Instruction::Sub {
                dst: self.stack_pointer,
                src1: Operand::Register { id: self.stack_pointer },
                src2: Operand::Immediate { value: layout.total_frame_size as i64 },
                span,
            });
        }

        println!("🔧 生成函数序言: {} 条指令", prologue.len());
        prologue
    }

    /// 生成函数尾声
    fn generate_epilogue(&self, layout: &StackFrameLayout) -> Vec<Instruction> {
        let mut epilogue = Vec::new();
        let span = Span::dummy();

        // 🔧 关键修复：在恢复栈帧之前，检查返回值寄存器是否与帧指针冲突
        // 如果冲突，需要使用临时寄存器来避免数据损坏
        
        let return_register = RegisterId(0); // r0 是返回值寄存器
        
        if self.frame_pointer.0 == return_register.0 {
            // 🔧 返回值寄存器与帧指针寄存器相同，需要特殊处理
            println!("🔧 检测到返回值寄存器与帧指针冲突，使用临时寄存器");
            
            // 使用临时寄存器 r4 来避免冲突
            let temp_reg = RegisterId(4);
            
            // 1. 将返回值保存到临时寄存器
            epilogue.push(Instruction::Move {
                dst: temp_reg,
                src: Operand::Register { id: self.frame_pointer },
                span,
            });
            
            // 2. 恢复栈指针
            epilogue.push(Instruction::Move {
                dst: self.stack_pointer,
                src: Operand::Register { id: self.frame_pointer },
                span,
            });
            
            // 3. 恢复调用者的帧指针
            epilogue.push(Instruction::Load64 {
                dst: self.frame_pointer,
                addr: self.stack_pointer,
                offset: 8,
                span,
            });
            
            // 4. 调整栈指针
            epilogue.push(Instruction::Add {
                dst: self.stack_pointer,
                src1: Operand::Register { id: self.stack_pointer },
                src2: Operand::Immediate { value: 8 },
                span,
            });
            
            // 5. 将返回值从临时寄存器恢复到返回值寄存器
            epilogue.push(Instruction::Move {
                dst: return_register,
                src: Operand::Register { id: temp_reg },
                span,
            });
        } else {
            // 🔧 正常情况：返回值寄存器与帧指针不冲突
            
            // 1. 恢复栈指针
            epilogue.push(Instruction::Move {
                dst: self.stack_pointer,
                src: Operand::Register { id: self.frame_pointer },
                span,
            });

            // 2. 恢复调用者的帧指针
            epilogue.push(Instruction::Load64 {
                dst: self.frame_pointer,
                addr: self.stack_pointer,
                offset: 8,
                span,
            });
            
            // 3. 调整栈指针
            epilogue.push(Instruction::Add {
                dst: self.stack_pointer,
                src1: Operand::Register { id: self.stack_pointer },
                src2: Operand::Immediate { value: 8 },
                span,
            });
        }

        println!("🔧 生成函数尾声: {} 条指令 (栈帧大小: {})", epilogue.len(), layout.total_frame_size);
        epilogue
    }

    /// 重写指令序列
    fn rewrite_instructions(
        &mut self,
        function: &mut LirFunction,
        layout: &StackFrameLayout,
        allocation_result: &RegisterAllocationResult,
    ) -> Result<(), String> {
        let mut new_instructions = Vec::new();
        let mut prologue_inserted = false;

        // 🔧 关键修复：只记录由alloc指令直接生成的栈地址寄存器
        // 这些寄存器存储的是栈地址，不应该被溢出处理
        let mut stack_address_registers = std::collections::HashSet::new();
        for instruction in &function.instructions {
            if let Instruction::Alloc { dst, .. } = instruction {
                stack_address_registers.insert(*dst);
                println!("🔧 记录栈地址寄存器: {:?} (由alloc指令生成)", dst);
            }
        }

        for instruction in &function.instructions {
            match instruction {
                // 在第一个标签后插入序言
                Instruction::Label { .. } if !prologue_inserted => {
                    new_instructions.push(instruction.clone());
                    if layout.total_frame_size > 0 {
                        new_instructions.extend(self.generate_prologue(layout));
                        println!("🔧 在第一个标签 {:?} 之后插入序言 (栈帧大小: {})", instruction, layout.total_frame_size);
                    }
                    prologue_inserted = true;
                }

                // 转换 alloc 指令为栈地址计算
                Instruction::Alloc { dst, allocation_type, .. } => {
                    match allocation_type {
                        AllocationType::Stack => {
                            // 只转换栈分配的alloc指令
                            if let Some(&offset) = layout.local_var_offsets.get(dst) {
                                // 计算 dst = fp + offset
                                new_instructions.push(Instruction::Add {
                                    dst: *dst,
                                    src1: Operand::Register { id: self.frame_pointer },
                                    src2: Operand::Immediate { value: offset },
                                    span: instruction.get_span(),
                                });
                                println!("🔧 转换栈分配 alloc: {:?} = FP + {}", dst, offset);
                            }
                        }
                        AllocationType::Heap | AllocationType::Static => {
                            // 堆分配和静态分配的alloc指令保持不变，将由虚拟机处理
                            new_instructions.push(instruction.clone());
                            println!("🔧 保持堆/静态分配 alloc 指令不变: {:?} (type: {:?})", dst, allocation_type);
                        }
                    }
                }

                // 🔧 关键修复：对于其他指令，只有在使用溢出寄存器时才进行溢出处理
                // 不要无条件地阻止对所有"栈地址寄存器"的溢出处理
                _ => {
                    // 检查指令中是否使用了溢出寄存器
                    let mut instruction_modified = false;
                    let mut current_instruction = instruction.clone();

                    // 处理指令中定义的溢出寄存器
                    if let Some(def_reg) = current_instruction.get_def_register() {
                        if let Some(spill_slot) = allocation_result.spilled_registers.get(&def_reg) {
                            if !stack_address_registers.contains(&def_reg) {
                                // 为溢出定义寄存器分配临时寄存器
                                let temp_reg = self.allocate_temp_register();
                                println!("🔧 为溢出定义寄存器 {:?} 分配临时寄存器 {:?}", def_reg, temp_reg);
                                
                                // 修改指令的目标寄存器
                                current_instruction.replace_def_register(def_reg, temp_reg);
                                
                                // 生成溢出存储指令
                                if let Some(&offset) = layout.spill_slot_offsets.get(&spill_slot.slot_id) {
                                    new_instructions.push(current_instruction.clone());
                                    new_instructions.push(Instruction::Store64 {
                                        addr: self.frame_pointer,
                                        offset,
                                        src: Operand::Register { id: temp_reg },
                                        span: instruction.get_span(),
                                    });
                                    println!("🔧 生成溢出存储: {:?} <- {:?} to [FP{}]", def_reg, temp_reg, offset);
                                    instruction_modified = true;
                                }
                            } else {
                                // 🔧 栈地址寄存器被错误地标记为溢出，直接保持原指令
                                println!("🔧 栈地址寄存器 {:?} 被错误标记为溢出，保持原指令不变", def_reg);
                                new_instructions.push(current_instruction.clone());
                                instruction_modified = true;
                            }
                        }
                    }

                    // 处理指令中使用的溢出寄存器
                    if !instruction_modified {
                        let used_regs = current_instruction.get_used_registers();
                        
                        for &used_reg in &used_regs {
                            if let Some(spill_slot) = allocation_result.spilled_registers.get(&used_reg) {
                                if !stack_address_registers.contains(&used_reg) {
                                    // 为溢出使用寄存器分配临时寄存器
                                    let temp_reg = self.allocate_temp_register();
                                    
                                    // 生成溢出加载指令
                                    if let Some(&offset) = layout.spill_slot_offsets.get(&spill_slot.slot_id) {
                                        new_instructions.push(Instruction::Load64 {
                                            dst: temp_reg,
                                            addr: self.frame_pointer,
                                            offset,
                                            span: instruction.get_span(),
                                        });
                                        println!("🔧 生成溢出加载: {:?} -> {:?} from [FP{}]", used_reg, temp_reg, offset);
                                        
                                        // 替换指令中的寄存器
                                        current_instruction.replace_register(used_reg, temp_reg);
                                    }
                                } else {
                                    // 🔧 栈地址寄存器被错误地标记为溢出，但不需要特殊处理
                                    println!("🔧 栈地址寄存器 {:?} 被错误标记为溢出，但作为使用不需要特殊处理", used_reg);
                                }
                            }
                        }
                        
                        new_instructions.push(current_instruction);
                    }
                }
            }
        }

        // 在返回指令前插入尾声
        if layout.total_frame_size > 0 {
            let epilogue = self.generate_epilogue(layout);
            let return_pos = new_instructions.len() - 1;
            
            // 在最后一条指令（应该是return）前插入尾声
            new_instructions.splice(return_pos..return_pos, epilogue.iter().cloned());
            println!("🔧 在 return 指令前插入尾声 (栈帧大小: {})", layout.total_frame_size);
        }

        function.instructions = new_instructions;
        Ok(())
    }

    /// 获取指令中使用的溢出寄存器
    fn get_spilled_register_uses(
        &self,
        instruction: &Instruction,
        allocation_result: &RegisterAllocationResult,
    ) -> Vec<RegisterId> {
        let mut used_spilled = Vec::new();
        
        match instruction {
            Instruction::Move { src, .. } => {
                if let Operand::Register { id } = src {
                    if allocation_result.spilled_registers.contains_key(id) {
                        used_spilled.push(*id);
                    }
                }
            }
            Instruction::Add { src1, src2, .. } |
            Instruction::Sub { src1, src2, .. } |
            Instruction::Mul { src1, src2, .. } |
            Instruction::Div { src1, src2, .. } => {
                for src in [src1, src2] {
                    if let Operand::Register { id } = src {
                        if allocation_result.spilled_registers.contains_key(id) {
                            used_spilled.push(*id);
                        }
                    }
                }
            }
            Instruction::Compare { src1, src2, .. } => {
                for src in [src1, src2] {
                    if let Operand::Register { id } = src {
                        if allocation_result.spilled_registers.contains_key(id) {
                            used_spilled.push(*id);
                        }
                    }
                }
            }
            Instruction::Load64 { addr, .. } => {
                if allocation_result.spilled_registers.contains_key(addr) {
                    used_spilled.push(*addr);
                }
            }
            Instruction::Store64 { addr, src, .. } => {
                if allocation_result.spilled_registers.contains_key(addr) {
                    used_spilled.push(*addr);
                }
                if let Operand::Register { id } = src {
                    if allocation_result.spilled_registers.contains_key(id) {
                        used_spilled.push(*id);
                    }
                }
            }
            Instruction::CallIndirect { function_register, args, .. } => {
                if allocation_result.spilled_registers.contains_key(function_register) {
                    used_spilled.push(*function_register);
                }
                for &arg in args {
                    if allocation_result.spilled_registers.contains_key(&arg) {
                        used_spilled.push(arg);
                    }
                }
            }
            Instruction::Return { value, .. } => {
                if let Some(val_reg) = value {
                    if allocation_result.spilled_registers.contains_key(val_reg) {
                        used_spilled.push(*val_reg);
                    }
                }
            }
            _ => {}
        }
        
        used_spilled
    }

    /// 获取指令中定义的溢出寄存器
    fn get_spilled_register_defs(
        &self,
        instruction: &Instruction,
        allocation_result: &RegisterAllocationResult,
    ) -> Vec<RegisterId> {
        let mut defined_spilled = Vec::new();
        
        match instruction {
            Instruction::Move { dst, .. } |
            Instruction::Add { dst, .. } |
            Instruction::Sub { dst, .. } |
            Instruction::Mul { dst, .. } |
            Instruction::Div { dst, .. } |
            Instruction::Load64 { dst, .. } => {
                if allocation_result.spilled_registers.contains_key(dst) {
                    defined_spilled.push(*dst);
                }
            }
            Instruction::CallIndirect { result: Some(dst), .. } => {
                if allocation_result.spilled_registers.contains_key(dst) {
                    defined_spilled.push(*dst);
                }
            }
            _ => {}
        }
        
        defined_spilled
    }

    /// 在指令中替换寄存器
    fn replace_register_in_instruction(
        &self,
        instruction: &mut Instruction,
        old_reg: RegisterId,
        new_reg: RegisterId,
    ) {
        match instruction {
            Instruction::Move { dst, src, .. } => {
                if *dst == old_reg {
                    *dst = new_reg;
                }
                if let Operand::Register { id } = src {
                    if *id == old_reg {
                        *id = new_reg;
                    }
                }
            }
            Instruction::Add { dst, src1, src2, .. } |
            Instruction::Sub { dst, src1, src2, .. } |
            Instruction::Mul { dst, src1, src2, .. } |
            Instruction::Div { dst, src1, src2, .. } => {
                if *dst == old_reg {
                    *dst = new_reg;
                }
                for src in [src1, src2] {
                    if let Operand::Register { id } = src {
                        if *id == old_reg {
                            *id = new_reg;
                        }
                    }
                }
            }
            Instruction::Compare { src1, src2, .. } => {
                for src in [src1, src2] {
                    if let Operand::Register { id } = src {
                        if *id == old_reg {
                            *id = new_reg;
                        }
                    }
                }
            }
            Instruction::Load64 { dst, addr, .. } => {
                if *dst == old_reg {
                    *dst = new_reg;
                }
                if *addr == old_reg {
                    *addr = new_reg;
                }
            }
            Instruction::Store64 { addr, src, .. } => {
                if *addr == old_reg {
                    *addr = new_reg;
                }
                if let Operand::Register { id } = src {
                    if *id == old_reg {
                        *id = new_reg;
                    }
                }
            }
            Instruction::CallIndirect { function_register, args, result, .. } => {
                if *function_register == old_reg {
                    *function_register = new_reg;
                }
                for arg in args {
                    if *arg == old_reg {
                        *arg = new_reg;
                    }
                }
                if let Some(dst) = result {
                    if *dst == old_reg {
                        *dst = new_reg;
                    }
                }
            }
            Instruction::Return { value, .. } => {
                if let Some(val_reg) = value {
                    if *val_reg == old_reg {
                        *val_reg = new_reg;
                    }
                }
            }
            _ => {}
        }
    }

    /// 处理包含溢出寄存器的指令
    fn handle_spilled_instruction(
        &mut self,
        new_instructions: &mut Vec<Instruction>,
        instruction: &Instruction,
        layout: &StackFrameLayout,
        allocation_result: &RegisterAllocationResult,
    ) {
        // 🔧 新策略：为每个溢出寄存器分配独立的临时虚拟寄存器
        let spilled_uses = self.get_spilled_register_uses(instruction, allocation_result);
        let spilled_defs = self.get_spilled_register_defs(instruction, allocation_result);
        
        // 为每个溢出的使用寄存器分配临时寄存器并生成 load
        let mut use_mapping = HashMap::new();
        for &spilled_reg in &spilled_uses {
            if let Some(spill_slot) = allocation_result.spilled_registers.get(&spilled_reg) {
                if let Some(&offset) = layout.spill_slot_offsets.get(&spill_slot.slot_id) {
                    let temp_reg = self.allocate_temp_register();
                    use_mapping.insert(spilled_reg, temp_reg);
                    
                    // 生成 load 指令
                    new_instructions.push(Instruction::Load64 {
                        dst: temp_reg,
                        addr: self.frame_pointer,
                        offset,
                        span: instruction.get_span(),
                    });
                    
                    println!("🔧 生成溢出加载: {:?} -> {:?} from [FP{}]", spilled_reg, temp_reg, offset);
                }
            }
        }
        
        // 为每个溢出的定义寄存器分配临时寄存器
        let mut def_mapping = HashMap::new();
        for &spilled_reg in &spilled_defs {
            let temp_reg = self.allocate_temp_register();
            def_mapping.insert(spilled_reg, temp_reg);
            println!("🔧 为溢出定义寄存器 {:?} 分配临时寄存器 {:?}", spilled_reg, temp_reg);
        }
        
        // 复制并修改指令，替换所有溢出寄存器为临时寄存器
        let mut modified_instruction = instruction.clone();
        for (spilled_reg, temp_reg) in &use_mapping {
            self.replace_register_in_instruction(&mut modified_instruction, *spilled_reg, *temp_reg);
        }
        for (spilled_reg, temp_reg) in &def_mapping {
            self.replace_register_in_instruction(&mut modified_instruction, *spilled_reg, *temp_reg);
        }
        
        // 添加修改后的指令
        new_instructions.push(modified_instruction);
        
        // 为每个溢出的定义寄存器生成 store
        for &spilled_reg in &spilled_defs {
            if let Some(spill_slot) = allocation_result.spilled_registers.get(&spilled_reg) {
                if let Some(&offset) = layout.spill_slot_offsets.get(&spill_slot.slot_id) {
                    if let Some(&temp_reg) = def_mapping.get(&spilled_reg) {
                        // 生成 store 指令
                        new_instructions.push(Instruction::Store64 {
                            addr: self.frame_pointer,
                            offset,
                            src: Operand::Register { id: temp_reg },
                            span: instruction.get_span(),
                        });
                        
                        println!("🔧 生成溢出存储: {:?} <- {:?} to [FP{}]", spilled_reg, temp_reg, offset);
                    }
                }
            }
        }
    }
}

// 为指令添加获取span的辅助方法
trait InstructionExt {
    fn get_span(&self) -> Span;
}

impl InstructionExt for Instruction {
    fn get_span(&self) -> Span {
        match self {
            Instruction::Move { span, .. } |
            Instruction::Add { span, .. } |
            Instruction::Sub { span, .. } |
            Instruction::Mul { span, .. } |
            Instruction::Div { span, .. } |
            Instruction::Compare { span, .. } |
            Instruction::Jump { span, .. } |
            Instruction::JumpEqual { span, .. } |
            Instruction::JumpNotEqual { span, .. } |
            Instruction::JumpLess { span, .. } |
            Instruction::JumpLessEqual { span, .. } |
            Instruction::JumpGreater { span, .. } |
            Instruction::JumpGreaterEqual { span, .. } |
            Instruction::Call { span, .. } |
            Instruction::CallIndirect { span, .. } |
            Instruction::Return { span, .. } |
            Instruction::Label { span, .. } |
            Instruction::Nop { span, .. } |
            Instruction::Load64 { span, .. } |
            Instruction::Store64 { span, .. } |
            Instruction::Alloc { span, .. } |
            Instruction::StructAlloc { span, .. } |
            Instruction::StructFieldLoad { span, .. } |
            Instruction::StructFieldStore { span, .. } |
            Instruction::Phi { span, .. } => *span,
            _ => Span::dummy(),
        }
    }
}

impl FunctionPass for StackFrameLowering {
    fn name(&self) -> &str {
        "stack-frame-lowering"
    }

    fn run_on_function(&mut self, function: &mut LirFunction, analyses: &mut AnalysisManager) -> PassResult {
        println!("🚀 运行 StackFrameLowering Pass for function: {}", function.name);

        // 🔧 修改：获取 Pre-RA 决策结果
        let allocation_result_key = format!("pre-ra-decision-{}", function.name);
        let allocation_result = match analyses.get_result::<RegisterAllocationResult>(&allocation_result_key) {
            Some(result) => result,
            None => {
                // 🔧 回退：如果没有 Pre-RA 决策，尝试获取旧的寄存器分配结果
                let old_key = format!("register-allocation-{}", function.name);
                match analyses.get_result::<RegisterAllocationResult>(&old_key) {
                    Some(result) => {
                        println!("⚠️ 使用旧的寄存器分配结果，建议使用两阶段分配架构");
                        result
                    }
                    None => {
                        println!("⚠️ 未找到寄存器分配结果，跳过栈帧降级");
                        return PassResult::Unchanged;
                    }
                }
            }
        };

        // 🔧 关键修复：创建可变的allocation_result副本并确保栈指针和帧指针不被寄存器分配器重新分配
        let mut allocation_result = allocation_result.clone();
        self.ensure_special_registers_reserved(&mut allocation_result);

        // 计算栈帧布局
        let layout = self.calculate_stack_frame_layout(function, &allocation_result);

        // 重写指令
        match self.rewrite_instructions(function, &layout, &allocation_result) {
            Ok(()) => {
                println!("✅ StackFrameLowering Pass 完成");
                PassResult::Changed
            }
            Err(e) => {
                eprintln!("❌ StackFrameLowering Pass 失败: {}", e);
                PassResult::Unchanged
            }
        }
    }

    fn required_analyses(&self) -> Vec<&'static str> {
        vec![] // 不声明分析依赖，因为我们查找的是函数特定的分析结果键
    }

    fn invalidated_analyses(&self) -> Vec<&'static str> {
        vec![] // 不使其他分析失效
    }
}

impl Default for StackFrameLowering {
    fn default() -> Self {
        Self::new()
    }
} 