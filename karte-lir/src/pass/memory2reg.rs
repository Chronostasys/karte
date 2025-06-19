use super::{FunctionPass, AnalysisManager, PassResult, AnalysisResult};
use super::analysis::{ControlFlowGraph, DefUseChains};
use crate::{LirFunction, Instruction, RegisterId, Operand, AllocationType, LabelId};
use karte_diagnostics::Span;
use std::collections::{HashMap, HashSet, VecDeque};
use std::any::Any;

/// 栈槽信息
#[derive(Debug, Clone)]
pub struct StackSlot {
    /// 分配指令的位置
    pub alloc_instruction: usize,
    /// 分配的寄存器（保存栈地址）
    pub address_register: RegisterId,
    /// 栈槽大小
    pub size: usize,
    /// 是否可以提升为寄存器
    pub promotable: bool,
    /// 加载指令位置列表
    pub loads: Vec<usize>,
    /// 存储指令位置列表  
    pub stores: Vec<usize>,
    /// 存储指令到基本块的映射
    pub store_to_block: HashMap<usize, LabelId>,
    /// 加载指令到基本块的映射
    pub load_to_block: HashMap<usize, LabelId>,
}

/// 基本块信息
#[derive(Debug, Clone)]
pub struct BasicBlock {
    pub label: LabelId,
    pub start: usize,
    pub end: usize,
    pub predecessors: Vec<LabelId>,
    pub successors: Vec<LabelId>,
}

/// Memory2Reg 分析结果
#[derive(Debug, Clone)]
pub struct Memory2RegAnalysis {
    /// 栈槽信息
    pub stack_slots: HashMap<RegisterId, StackSlot>,
    /// 可提升的栈槽
    pub promotable_slots: Vec<RegisterId>,
    /// 基本块信息
    pub basic_blocks: HashMap<LabelId, BasicBlock>,
    /// 需要插入φ节点的位置
    pub phi_insertions: Vec<PhiInsertion>,
}

/// φ节点插入信息
#[derive(Debug, Clone)]
pub struct PhiInsertion {
    pub block: LabelId,
    pub variable: RegisterId,
    pub dst_register: RegisterId,
    pub incoming: Vec<(LabelId, Operand)>,
    /// 实际分配的φ节点结果寄存器（在插入时设置）
    pub actual_dst_register: Option<RegisterId>,
}

impl AnalysisResult for Memory2RegAnalysis {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Memory2Reg Pass
/// 
/// 将合适的栈分配提升为寄存器分配，减少内存访问
#[derive(Debug)]
pub struct Memory2RegPass;

impl Memory2RegPass {
    pub fn new() -> Self {
        Self
    }
    
    /// 分析栈槽使用模式
    fn analyze_stack_slots(&self, function: &LirFunction) -> Memory2RegAnalysis {
        let mut stack_slots = HashMap::new();
        let mut promotable_slots = Vec::new();
        
        // 第一阶段：分析基本块结构
        let basic_blocks = self.analyze_basic_blocks(function);
        println!("🔍 分析到 {} 个基本块", basic_blocks.len());
        
        // 第二阶段：识别栈分配
        for (i, instruction) in function.instructions.iter().enumerate() {
            if let Instruction::Alloc { dst, size, allocation_type: AllocationType::Stack, .. } = instruction {
                let slot = StackSlot {
                    alloc_instruction: i,
                    address_register: *dst,
                    size: *size,
                    promotable: true, // 初始假设可提升
                    loads: Vec::new(),
                    stores: Vec::new(),
                    store_to_block: HashMap::new(),
                    load_to_block: HashMap::new(),
                };
                println!("🔍 发现栈分配: 寄存器 {:?}, 大小 {}", dst, size);
                stack_slots.insert(*dst, slot);
            }
        }
        
        // 第三阶段：分析每个栈槽的使用并记录所在基本块
        for (i, instruction) in function.instructions.iter().enumerate() {
            let current_block = self.find_basic_block_for_instruction(i, &basic_blocks);
            
            match instruction {
                Instruction::Load64 { dst: _, addr, offset, .. } => {
                    if *offset == 0 {
                        if let Some(slot) = stack_slots.get_mut(addr) {
                            slot.loads.push(i);
                            if let Some(block_label) = current_block {
                                slot.load_to_block.insert(i, block_label);
                                println!("🔍 记录load: 寄存器 {:?} 在块 {:?}", addr, block_label);
                            }
                        }
                    } else {
                        // 有偏移的访问，标记为不可提升
                        if let Some(slot) = stack_slots.get_mut(addr) {
                            slot.promotable = false;
                        }
                    }
                }
                Instruction::StructFieldLoad { dst: _, struct_addr, .. } => {
                    // StructFieldLoad 从结构体地址加载字段，不应该直接提升栈槽
                    // 但如果结构体地址本身是栈槽，我们需要记录这个使用
                    if let Some(slot) = stack_slots.get_mut(struct_addr) {
                        slot.loads.push(i);
                        if let Some(block_label) = current_block {
                            slot.load_to_block.insert(i, block_label);
                            println!("🔍 记录StructFieldLoad: 结构体地址寄存器 {:?} 在块 {:?}", struct_addr, block_label);
                        }
                    }
                }
                Instruction::Store64 { addr, offset, src: _, .. } => {
                    if *offset == 0 {
                        if let Some(slot) = stack_slots.get_mut(addr) {
                            slot.stores.push(i);
                            if let Some(block_label) = current_block {
                                slot.store_to_block.insert(i, block_label);
                                println!("🔍 记录store: 寄存器 {:?} 在块 {:?}", addr, block_label);
                            }
                        }
                    } else {
                        // 有偏移的访问，标记为不可提升
                        if let Some(slot) = stack_slots.get_mut(addr) {
                            slot.promotable = false;
                        }
                    }
                }
                Instruction::Move { dst: _, src, .. } => {
                    // 检查是否将栈地址传递给其他地方
                    if let Operand::Register { id } = src {
                            if let Some(slot) = stack_slots.get_mut(id) {
                            println!("🚨 栈地址被传递: {:?} 标记为不可提升", id);
                                slot.promotable = false; // 地址被传递，不安全提升
                        }
                    }
                }
                Instruction::Call { args, .. } => {
                    // 检查栈地址是否作为参数传递
                    for arg in args {
                        if let Some(slot) = stack_slots.get_mut(arg) {
                            slot.promotable = false; // 可能被函数修改
                        }
                    }
                }
                _ => {}
            }
        }
        
        // 第三阶段补充：检查引用操作（需要单独遍历以避免借用冲突）
        for (i, instruction) in function.instructions.iter().enumerate() {
            if let Instruction::Store64 { addr, offset, src, .. } = instruction {
                if *offset == 0 {
                    // 🚨 关键修复：检查是否存储的是另一个栈地址（引用操作）
                    if let Operand::Register { id: src_reg } = src {
                        if stack_slots.contains_key(src_reg) && stack_slots.contains_key(addr) {
                            println!("🚨 检测到引用操作: 栈槽 {:?} 存储了另一个栈地址 {:?}", addr, src_reg);
                            // 存储栈地址的槽不可提升（它需要真实的内存地址）
                            if let Some(slot) = stack_slots.get_mut(addr) {
                                slot.promotable = false;
                            }
                            // 被引用的栈槽也不可提升（它的地址被取用）
                            if let Some(referenced_slot) = stack_slots.get_mut(src_reg) {
                                referenced_slot.promotable = false;
                                println!("🚨 被引用的栈槽 {:?} 也标记为不可提升", src_reg);
                            }
                        }
                    }
                }
            }
        }
        
        // 🚨 新增：第四阶段 - 引用链传播检测
        // 检测间接引用：如果一个栈槽存储了从另一个（不可提升的）栈槽加载的值
        let mut changed = true;
        while changed {
            changed = false;
            for (i, instruction) in function.instructions.iter().enumerate() {
                if let Instruction::Store64 { addr, offset, src, .. } = instruction {
                    if *offset == 0 {
                        if let Operand::Register { id: src_reg } = src {
                            // 检查源寄存器是否是从不可提升的栈槽加载的
                            if let Some(load_from_stack) = self.trace_register_to_stack_load(*src_reg, i, function) {
                                if stack_slots.contains_key(&load_from_stack) && stack_slots.contains_key(addr) {
                                    // 检查被加载的栈槽是否不可提升
                                    if let Some(source_slot) = stack_slots.get(&load_from_stack) {
                                        if !source_slot.promotable {
                                            // 如果源栈槽不可提升，那么存储其值的栈槽也不可提升
                                            if let Some(target_slot) = stack_slots.get_mut(addr) {
                                                if target_slot.promotable {
                                                    println!("🚨 检测到间接引用: 栈槽 {:?} 存储了从不可提升栈槽 {:?} 加载的值", addr, load_from_stack);
                                                    target_slot.promotable = false;
                                                    changed = true;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        
        // 第四阶段：分析φ节点需求
        let mut phi_insertions = Vec::new();
        
        // 收集可提升的栈槽并分析φ节点需求
        for (addr_reg, slot) in &stack_slots {
            if slot.promotable && !slot.stores.is_empty() {
                    promotable_slots.push(*addr_reg);
                
                println!("🔍 分析栈槽 {:?}: stores={}, loads={}", 
                    addr_reg, slot.stores.len(), slot.loads.len());
                
                // 如果有多个存储或跨基本块访问，可能需要φ节点
                if self.needs_phi_nodes(slot, &basic_blocks) {
                    println!("🎯 栈槽 {:?} 需要φ节点!", addr_reg);
                    let phi_nodes = self.compute_phi_placements(slot, function);
                    println!("🎯 计算出 {} 个φ节点", phi_nodes.len());
                    phi_insertions.extend(phi_nodes);
                } else {
                    println!("❌ 栈槽 {:?} 不需要φ节点", addr_reg);
                }
            } else if !slot.promotable {
                println!("🚨 栈槽 {:?} 不可提升 (涉及引用操作或地址传递)", addr_reg);
            }
        }
        
        println!("🎯 总共需要插入 {} 个φ节点", phi_insertions.len());
        
        Memory2RegAnalysis {
            stack_slots,
            promotable_slots,
            basic_blocks,
            phi_insertions,
        }
    }
    
    /// 分析基本块结构
    fn analyze_basic_blocks(&self, function: &LirFunction) -> HashMap<LabelId, BasicBlock> {
        let mut basic_blocks = HashMap::new();
        let mut current_block_start = 0;
        let mut current_label = LabelId(0); // 默认第一个块
        
        // 第一遍：识别所有基本块的边界
        for (i, instruction) in function.instructions.iter().enumerate() {
            match instruction {
                Instruction::Label { id, .. } => {
                    // 结束前一个基本块（如果存在）
                    if i > current_block_start {
                        let block = BasicBlock {
                            label: current_label,
                            start: current_block_start,
                            end: i - 1,
                            predecessors: Vec::new(),
                            successors: Vec::new(),
                        };
                        basic_blocks.insert(current_label, block);
                    }
                    
                    // 开始新的基本块
                    current_label = *id;
                    current_block_start = i;
                }
                Instruction::Jump { .. } | 
                Instruction::JumpEqual { .. } | 
                Instruction::JumpNotEqual { .. } |
                Instruction::JumpLess { .. } |
                Instruction::JumpLessEqual { .. } |
                Instruction::JumpGreater { .. } |
                Instruction::JumpGreaterEqual { .. } |
                Instruction::Return { .. } => {
                    // 跳转指令结束当前基本块
                    let block = BasicBlock {
                        label: current_label,
                        start: current_block_start,
                        end: i,
                        predecessors: Vec::new(),
                        successors: Vec::new(),
                    };
                    basic_blocks.insert(current_label, block);
                    current_block_start = i + 1;
                }
                _ => {}
            }
        }
        
        // 处理最后一个基本块
        if current_block_start < function.instructions.len() {
            let block = BasicBlock {
                label: current_label,
                start: current_block_start,
                end: function.instructions.len() - 1,
                predecessors: Vec::new(),
                successors: Vec::new(),
            };
            basic_blocks.insert(current_label, block);
        }
        
        basic_blocks
    }
    
    /// 查找指令所在的基本块
    fn find_basic_block_for_instruction(&self, instruction_index: usize, basic_blocks: &HashMap<LabelId, BasicBlock>) -> Option<LabelId> {
        for (label, block) in basic_blocks {
            if instruction_index >= block.start && instruction_index <= block.end {
                return Some(*label);
            }
        }
        None
    }
    
    /// 判断是否需要φ节点
    fn needs_phi_nodes(&self, slot: &StackSlot, basic_blocks: &HashMap<LabelId, BasicBlock>) -> bool {
        // 如果有多个存储在不同的基本块中，或者有跨基本块的访问，就需要φ节点
        let store_blocks: HashSet<LabelId> = slot.store_to_block.values().cloned().collect();
        let load_blocks: HashSet<LabelId> = slot.load_to_block.values().cloned().collect();
        
        // 超过一个存储块，或者load和store在不同块中
        store_blocks.len() > 1 || (!store_blocks.is_empty() && !load_blocks.is_empty() && !store_blocks.is_subset(&load_blocks))
    }
    
    /// 计算φ节点的放置位置
    fn compute_phi_placements(&self, slot: &StackSlot, function: &LirFunction) -> Vec<PhiInsertion> {
        let mut phi_insertions = Vec::new();
        
        // 找到所有有load的块，在其中插入φ节点
        for (&load_pos, &load_block) in &slot.load_to_block {
            // 收集所有前驱块的存储值
            let mut incoming = Vec::new();
            
            for (&store_pos, &store_block) in &slot.store_to_block {
                if store_pos < load_pos { // 确保存储在加载之前
                    // 从实际的store指令中提取值
                    if store_pos < function.instructions.len() {
                        if let Instruction::Store64 { src, .. } = &function.instructions[store_pos] {
                            println!("🔧 从store指令 {} 提取值: {:?}", store_pos, src);
                            incoming.push((store_block, src.clone()));
                        }
                    }
                }
            }
            
            if incoming.len() > 1 {
                println!("🔧 为块 {:?} 创建φ节点，incoming值: {:?}", load_block, incoming);
                phi_insertions.push(PhiInsertion {
                    block: load_block,
                    variable: slot.address_register,
                    dst_register: RegisterId(0), // 将在实际插入时分配新寄存器
                    incoming,
                    actual_dst_register: None,
                });
            }
        }
        
        phi_insertions
    }
    
    /// 执行 Memory2Reg 变换
    fn transform_function(&self, function: &mut LirFunction, analysis: &Memory2RegAnalysis) -> bool {
        let mut changed = false;
        
        println!("🚀 开始Memory2Reg变换");
        
        // 创建可变的phi_insertions副本
        let mut phi_insertions = analysis.phi_insertions.clone();
        
        // 第一步：插入φ节点
        if !phi_insertions.is_empty() {
            println!("🚀 步骤1: 插入φ节点");
            changed |= self.insert_phi_nodes(function, &mut phi_insertions);
            
            // 显示插入φ节点后的LIR状态
            println!("📋 插入φ节点后的LIR:");
            for (i, instruction) in function.instructions.iter().enumerate() {
                println!("  {}: {}", i, instruction);
            }
        }
        
        // 第二步：收集所有栈槽的变换操作
        println!("🚀 步骤2: 收集变换操作");
        let mut all_instructions_to_remove = Vec::new();
        let mut all_instructions_to_modify = Vec::new();
        
        for &addr_reg in &analysis.promotable_slots {
            if let Some(slot) = analysis.stack_slots.get(&addr_reg) {
                println!("🚀 处理栈槽 {:?}", addr_reg);
                let (mut remove_ops, mut modify_ops) = self.collect_transform_operations_with_phi(function, slot, &phi_insertions);
                all_instructions_to_remove.append(&mut remove_ops);
                all_instructions_to_modify.append(&mut modify_ops);
            }
        }
        
        println!("🚀 总共移除 {} 条指令, 修改 {} 条指令", 
            all_instructions_to_remove.len(), all_instructions_to_modify.len());
        
        // 第三步：一次性应用所有变换，避免索引冲突
        println!("🚀 步骤3: 应用变换");
        changed |= self.apply_all_transforms(function, all_instructions_to_remove, all_instructions_to_modify);
        
        if changed {
            println!("✅ Memory2Reg变换完成，有修改");
            // 显示最终的LIR状态
            println!("📋 Memory2Reg后的LIR:");
            for (i, instruction) in function.instructions.iter().enumerate() {
                println!("  {}: {}", i, instruction);
            }
        } else {
            println!("⚠️ Memory2Reg变换完成，无修改");
        }
        
        changed
    }
    
    /// 插入φ节点
    fn insert_phi_nodes(&self, function: &mut LirFunction, phi_insertions: &mut [PhiInsertion]) -> bool {
        if phi_insertions.is_empty() {
            println!("⚠️ 没有φ节点需要插入");
            return false;
        }
        
        println!("🔧 开始插入 {} 个φ节点", phi_insertions.len());
        
        // 为每个φ节点分配新的寄存器
        let mut phi_instructions = Vec::new();
        
        for (idx, phi_insertion) in phi_insertions.iter_mut().enumerate() {
            // 分配新寄存器作为φ节点的目标
            let phi_dst = function.new_register();
            
            // 记录实际分配的寄存器
            phi_insertion.actual_dst_register = Some(phi_dst);
            
            println!("🔧 φ节点 {}: 目标寄存器 {:?}, 原变量 {:?}, 目标块 {:?}", 
                idx, phi_dst, phi_insertion.variable, phi_insertion.block);
            
            // 重新构建incoming列表，提取实际值
            let mut incoming = Vec::new();
            for &(block_label, ref operand) in &phi_insertion.incoming {
                // 使用实际的store指令中的值
                incoming.push((block_label, operand.clone()));
                println!("🔧   - 来自块 {:?} 的值: {:?}", block_label, operand);
            }
            
            let phi_instruction = Instruction::Phi {
                dst: phi_dst,
                incoming,
                span: Span::dummy(),
            };
            
            // 找到目标基本块的开始位置并插入φ节点
            if let Some(insert_pos) = self.find_block_start_position(function, phi_insertion.block) {
                println!("🔧   - 插入位置: {}", insert_pos);
                phi_instructions.push((insert_pos, phi_instruction));
            } else {
                println!("❌   - 找不到块 {:?} 的插入位置", phi_insertion.block);
            }
        }
        
        // 按位置逆序插入，避免索引偏移
        phi_instructions.sort_by(|a, b| b.0.cmp(&a.0));
        for (pos, instruction) in phi_instructions {
            if pos < function.instructions.len() {
                println!("🔧 在位置 {} 插入φ节点: {:?}", pos, instruction);
                function.instructions.insert(pos, instruction);
            }
        }
        
        println!("✅ φ节点插入完成");
        !phi_insertions.is_empty()
    }
    
    /// 查找基本块的开始位置
    fn find_block_start_position(&self, function: &LirFunction, block_label: LabelId) -> Option<usize> {
        for (i, instruction) in function.instructions.iter().enumerate() {
            if let Instruction::Label { id, .. } = instruction {
                if *id == block_label {
                    return Some(i + 1); // φ节点放在标签之后
                }
            }
        }
        None
    }
    
    /// 收集支持φ节点的变换操作
    fn collect_transform_operations_with_phi(&self, function: &LirFunction, slot: &StackSlot, phi_insertions: &[PhiInsertion]) -> (Vec<usize>, Vec<(usize, Instruction)>) {
        let mut instructions_to_remove = Vec::new();
        let mut instructions_to_modify = Vec::new();
        
        // 如果有φ节点支持，使用更智能的变换
        if self.has_phi_support_for_slot(slot, phi_insertions) {
            // 对于有φ节点支持的栈槽，使用φ节点来处理多个定义
            self.transform_with_phi_support(function, slot, &mut instructions_to_remove, &mut instructions_to_modify, phi_insertions);
        } else {
            // 回退到原来的简单变换逻辑
            self.collect_simple_transform_operations(function, slot, &mut instructions_to_remove, &mut instructions_to_modify);
        }
        
        (instructions_to_remove, instructions_to_modify)
    }
    
    /// 检查是否有φ节点支持此栈槽
    fn has_phi_support_for_slot(&self, slot: &StackSlot, phi_insertions: &[PhiInsertion]) -> bool {
        phi_insertions.iter().any(|phi| phi.variable == slot.address_register)
    }
    
    /// 使用φ节点支持进行变换
    fn transform_with_phi_support(&self, function: &LirFunction, slot: &StackSlot, 
                                  instructions_to_remove: &mut Vec<usize>, 
                                  instructions_to_modify: &mut Vec<(usize, Instruction)>,
                                  phi_insertions: &[PhiInsertion]) {
        println!("🎯 使用φ节点支持变换栈槽 {:?}", slot.address_register);
        
        // 对于有φ节点的情况，我们可以更激进地优化
        // 移除所有相关的alloc、store和load指令
        // φ节点会处理值的正确传播
        
        println!("🎯 移除alloc指令: {}", slot.alloc_instruction);
        instructions_to_remove.push(slot.alloc_instruction);
        
        // 移除所有store指令
        println!("🎯 移除 {} 个store指令", slot.stores.len());
        for &store_pos in &slot.stores {
            println!("🎯   - 移除store指令: {}", store_pos);
            instructions_to_remove.push(store_pos);
        }
        
        // 🔧 重要修复：φ节点插入后需要重新查找load指令
        // 因为φ节点插入可能改变了指令索引
        println!("🎯 在φ节点插入后重新查找load指令");
        let mut actual_load_positions = Vec::new();
        
        // 重新扫描function找到实际的load指令位置
        for (i, instruction) in function.instructions.iter().enumerate() {
            if let Instruction::Load64 { addr, .. } = instruction {
                // 检查这个load是否访问我们的栈槽
                if *addr == slot.address_register {
                    println!("🔧 找到栈槽 {:?} 的load指令在位置 {}", slot.address_register, i);
                    actual_load_positions.push(i);
                }
            }
        }
        
        // 将load指令替换为使用φ节点的结果
        println!("🎯 替换 {} 个实际找到的load指令", actual_load_positions.len());
        for &load_pos in &actual_load_positions {
            if load_pos < function.instructions.len() {
                if let Instruction::Load64 { dst, .. } = &function.instructions[load_pos] {
                    println!("🎯   - 分析load指令 {} (dst: {:?})", load_pos, dst);
                    
                    // 找到对应的φ节点结果寄存器
                    let phi_result_reg = self.find_phi_result_for_instruction_position(load_pos, slot, phi_insertions, function);
                    
                    // 如果找到了有效的φ节点结果寄存器，进行替换
                    if phi_result_reg.0 != 998 { // 不是默认占位符
                        println!("🎯   - 替换load指令 {} -> mov {:?}, {:?}", load_pos, dst, phi_result_reg);
                        instructions_to_modify.push((load_pos, Instruction::Move {
                            dst: *dst,
                            src: Operand::Register { id: phi_result_reg },
                            span: function.instructions[load_pos].get_span(),
                        }));
                    } else {
                        println!("❌   - 无法找到φ节点结果寄存器，跳过load指令 {}", load_pos);
                    }
                }
            }
        }
    }
    
    /// 为指定位置的load指令找到对应的φ节点结果寄存器
    fn find_phi_result_for_instruction_position(&self, load_pos: usize, slot: &StackSlot, phi_insertions: &[PhiInsertion], function: &LirFunction) -> RegisterId {
        println!("🔧 查找指令位置 {} 对应的φ节点 (变量: {:?})", load_pos, slot.address_register);
        
        // 找到load指令所在的基本块
        let mut load_block = None;
        let mut current_block = None;
        
        for (i, instruction) in function.instructions.iter().enumerate() {
            if let Instruction::Label { id, .. } = instruction {
                current_block = Some(*id);
            }
            
            if i == load_pos {
                load_block = current_block;
                break;
            }
        }
        
        if let Some(block) = load_block {
            println!("🔧 load指令在块 {:?}", block);
            
            // 查找在该基本块中插入的φ节点
            for phi in phi_insertions {
                if phi.variable == slot.address_register && phi.block == block {
                    // 使用实际分配的φ节点寄存器
                    if let Some(actual_dst) = phi.actual_dst_register {
                        println!("🔧 找到φ节点结果寄存器: {:?} for load at {}", actual_dst, load_pos);
                        return actual_dst;
                    } else {
                        println!("🔧 找到φ节点但没有actual_dst_register: {:?}", phi.dst_register);
                        return phi.dst_register;
                    }
                }
            }
            
            println!("🔧 在块 {:?} 中未找到变量 {:?} 的φ节点", block, slot.address_register);
        } else {
            println!("🔧 未找到load指令 {} 对应的基本块", load_pos);
        }
        
        println!("⚠️ 未找到对应的φ节点，使用默认寄存器");
        RegisterId(998) // 默认占位符
    }
    
    /// 简单变换操作（原有逻辑）
    fn collect_simple_transform_operations(&self, function: &LirFunction, slot: &StackSlot,
                                          instructions_to_remove: &mut Vec<usize>, 
                                          instructions_to_modify: &mut Vec<(usize, Instruction)>) {
        // 🔧 重大改进：处理更多情况，包括寄存器存储和跨栈槽值传播
        
        println!("🚀 分析栈槽 {:?}: stores={}, loads={}", slot.address_register, slot.stores.len(), slot.loads.len());
        
        // 🔧 新增：引用-解引用模式检测
        if self.is_reference_dereference_pattern(function, slot) {
            println!("🎯 检测到引用-解引用模式，应用特殊优化");
            self.optimize_reference_dereference_pattern(function, slot, instructions_to_remove, instructions_to_modify);
            return;
        }
        
        // 情况1: 单一存储（立即数或寄存器）
        if slot.stores.len() == 1 {
            let store_pos = slot.stores[0];
            if store_pos >= function.instructions.len() {
                return;
            }
            
            if let Instruction::Store64 { src, .. } = &function.instructions[store_pos] {
                println!("🚀 单一存储分析: src={:?}", src);
                
                // 🔧 新增：跨栈槽值传播 - 如果src是寄存器，尝试追踪其来源
                let effective_src = match src {
                    Operand::Register { id } => {
                        // 尝试找到这个寄存器的定义
                        self.trace_register_value(*id, store_pos, function).unwrap_or_else(|| src.clone())
                    }
                    _ => src.clone()
                };
                
                println!("🚀 有效源操作数: {:?}", effective_src);
                
                // 对于单一存储，我们可以直接传播值
                match &effective_src {
                    Operand::Immediate { .. } | Operand::Register { .. } => {
                        // 立即数或寄存器：都可以优化
                        
                        // 将所有load指令替换为直接使用源操作数
                        for &load_pos in &slot.loads {
                            if load_pos < function.instructions.len() {
                                if let Instruction::Load64 { dst, .. } = &function.instructions[load_pos] {
                                    println!("🚀 替换load {} -> mov {:?}, {:?}", load_pos, dst, effective_src);
                                    // 替换load为mov
                                    instructions_to_modify.push((load_pos, Instruction::Move {
                                        dst: *dst,
                                        src: effective_src.clone(),
                                        span: function.instructions[load_pos].get_span(),
                                    }));
                                }
                            }
                        }
                        
                        // 移除store指令和alloc指令
                        instructions_to_remove.push(store_pos);
                        instructions_to_remove.push(slot.alloc_instruction);
                    }
                    _ => {
                        // 其他复杂操作数，暂不优化
                        println!("🚀 复杂操作数，暂不优化: {:?}", effective_src);
                    }
                }
            }
        } 
        // 情况2: 多个存储 - 需要φ函数处理，但我们先实现简单版本
        else if slot.stores.len() > 1 && slot.loads.len() > 0 {
            // 🔧 新增：处理多重赋值的简单情况
            // 如果所有stores都是在不同的基本块中，我们可以考虑优化
            
            // 简化实现：如果最后一个store支配所有的load，我们可以优化
            if let Some(&last_store_pos) = slot.stores.last() {
                if last_store_pos < function.instructions.len() {
                    if let Instruction::Store64 { src, .. } = &function.instructions[last_store_pos] {
                        // 检查这个store是否在所有load之前
                        let all_loads_after_store = slot.loads.iter().all(|&load_pos| load_pos > last_store_pos);
                        
                        if all_loads_after_store {
                            // 可以优化：用最后的store值替换所有后续的load
                            match src {
                                Operand::Immediate { .. } | Operand::Register { .. } => {
                                    // 替换所有load
                                    for &load_pos in &slot.loads {
                                        if load_pos < function.instructions.len() {
                                            if let Instruction::Load64 { dst, .. } = &function.instructions[load_pos] {
                                                instructions_to_modify.push((load_pos, Instruction::Move {
                                                    dst: *dst,
                                                    src: src.clone(),
                                                    span: function.instructions[load_pos].get_span(),
                                                }));
                                            }
                                        }
                                    }
                                    
                                    // 移除最后的store（但保留其他store和alloc，因为可能有其他用途）
                                    instructions_to_remove.push(last_store_pos);
                                    
                                    // 如果所有store都被处理了，可以移除alloc
                                    if slot.stores.len() == 1 {
                                        instructions_to_remove.push(slot.alloc_instruction);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
        // 情况3: 没有存储但有加载 - 这可能是未初始化访问，不优化
        else if slot.stores.is_empty() && !slot.loads.is_empty() {
            // 不优化未初始化的栈槽
        }
        // 情况4: 只有存储没有加载 - 死存储，可以移除
        else if !slot.stores.is_empty() && slot.loads.is_empty() {
            // 移除所有死存储
            for &store_pos in &slot.stores {
                instructions_to_remove.push(store_pos);
            }
            instructions_to_remove.push(slot.alloc_instruction);
        }
    }
    
    /// 一次性应用所有变换操作
    fn apply_all_transforms(&self, function: &mut LirFunction, 
                          instructions_to_remove: Vec<usize>, 
                          instructions_to_modify: Vec<(usize, Instruction)>) -> bool {
        if instructions_to_remove.is_empty() && instructions_to_modify.is_empty() {
            return false;
        }
        
        // 去重并排序
        let mut unique_removes: Vec<usize> = instructions_to_remove.into_iter().collect();
        unique_removes.sort();
        unique_removes.dedup();
        
        let mut unique_modifies = instructions_to_modify;
        unique_modifies.sort_by(|a, b| a.0.cmp(&b.0));
        unique_modifies.dedup_by(|a, b| a.0 == b.0);
        
        // 先处理修改操作（索引不变）
        for (pos, new_instruction) in unique_modifies.iter().rev() {
            if *pos < function.instructions.len() {
                function.instructions[*pos] = new_instruction.clone();
            }
        }
        
        // 然后处理移除操作（从后往前，避免索引变化）
        for &pos in unique_removes.iter().rev() {
            if pos < function.instructions.len() {
                function.instructions.remove(pos);
            }
        }
        
        true
    }

    /// 追踪寄存器值的来源，用于跨栈槽值传播
    fn trace_register_value(&self, register: RegisterId, before_pos: usize, function: &LirFunction) -> Option<Operand> {
        println!("🔍 追踪寄存器 {:?} 在位置 {} 之前的值", register, before_pos);
        
        // 向前扫描，找到最近的对该寄存器的定义
        for i in (0..before_pos).rev() {
            if i >= function.instructions.len() {
                continue;
            }
            
            let instruction = &function.instructions[i];
            match instruction {
                Instruction::Move { dst, src, .. } if *dst == register => {
                    println!("🔍 找到mov定义: {:?} = {:?}", dst, src);
                    return Some(src.clone());
                }
                Instruction::Load64 { dst, addr, .. } if *dst == register => {
                    println!("🔍 找到load定义: {:?} = [{}]", dst, addr);
                    // 🔧 改进：对于load指令，尝试追踪栈槽的存储值
                    if self.is_stack_address_register(*addr, function) {
                        // 从栈地址加载，追踪栈槽的存储值
                        return self.trace_stack_slot_value(*addr, i, function);
                    } else {
                        // 从非栈地址加载，无法追踪
                        return None;
                    }
                }
                Instruction::StructFieldLoad { dst, struct_addr, .. } if *dst == register => {
                    println!("🔍 找到StructFieldLoad定义: {:?} = field from {:?}", dst, struct_addr);
                    // StructFieldLoad 从结构体加载字段，无法简单追踪
                    return None;
                }
                Instruction::Add { dst, src1, src2, .. } |
                Instruction::Sub { dst, src1, src2, .. } |
                Instruction::Mul { dst, src1, src2, .. } |
                Instruction::Div { dst, src1, src2, .. } if *dst == register => {
                    println!("🔍 找到算术定义: {:?} = {:?} op {:?}", dst, src1, src2);
                    // 算术运算的结果无法简单追踪
                    return None;
                }
                _ => {
                    // 其他指令，继续向前查找
                }
            }
        }
        
        println!("🔍 未找到寄存器 {:?} 的定义", register);
        None
    }
    
    /// 追踪栈槽的存储值
    fn trace_stack_slot_value(&self, stack_addr: RegisterId, before_pos: usize, function: &LirFunction) -> Option<Operand> {
        println!("🔍 追踪栈槽 {:?} 在位置 {} 之前的存储值", stack_addr, before_pos);
        
        // 向前扫描，找到最近的对该栈槽的存储
        for i in (0..before_pos).rev() {
            if i >= function.instructions.len() {
                continue;
            }
            
            let instruction = &function.instructions[i];
            match instruction {
                Instruction::Store64 { addr, src, .. } if *addr == stack_addr => {
                    println!("🔍 找到栈槽存储: [{}] = {:?}", addr, src);
                    // 如果存储的是寄存器，可以进一步追踪
                    match src {
                        Operand::Register { id } => {
                            // 递归追踪寄存器值（但限制递归深度）
                            if let Some(traced_value) = self.trace_register_value(*id, i, function) {
                                println!("🔍 递归追踪得到: {:?}", traced_value);
                                return Some(traced_value);
                            } else {
                                return Some(src.clone());
                            }
                        }
                        _ => {
                            return Some(src.clone());
                        }
                    }
                }
                _ => {
                    // 其他指令，继续向前查找
                }
            }
        }
        
        println!("🔍 未找到栈槽 {:?} 的存储值", stack_addr);
        None
    }

    /// 检测是否是引用-解引用模式
    fn is_reference_dereference_pattern(&self, function: &LirFunction, slot: &StackSlot) -> bool {
        // 引用-解引用模式的特征：
        // 1. 栈槽只有一个store（存储引用地址）
        // 2. 栈槽有一个或多个load（用于解引用）
        // 3. store的源是另一个栈槽的地址（引用操作的结果）
        
        if slot.stores.len() != 1 || slot.loads.is_empty() {
            return false;
        }
        
        let store_pos = slot.stores[0];
        if store_pos >= function.instructions.len() {
            return false;
        }
        
        if let Instruction::Store64 { src, .. } = &function.instructions[store_pos] {
            match src {
                Operand::Register { id } => {
                    // 检查这个寄存器是否是另一个栈槽的地址
                    // 简化判断：如果寄存器ID对应一个栈分配，则认为是引用
                    self.is_stack_address_register(*id, function)
                }
                _ => false
            }
        } else {
            false
        }
    }
    
    /// 检查寄存器是否是栈地址寄存器
    fn is_stack_address_register(&self, register: RegisterId, function: &LirFunction) -> bool {
        // 查找是否有alloc指令分配了这个寄存器作为栈地址
        for instruction in &function.instructions {
            if let Instruction::Alloc { dst, .. } = instruction {
                if *dst == register {
                    return true;
                }
            }
        }
        false
    }
    
    /// 优化引用-解引用模式
    fn optimize_reference_dereference_pattern(&self, function: &LirFunction, slot: &StackSlot,
                                            instructions_to_remove: &mut Vec<usize>, 
                                            instructions_to_modify: &mut Vec<(usize, Instruction)>) {
        println!("🎯 优化引用-解引用模式 for 栈槽 {:?}", slot.address_register);
        
        let store_pos = slot.stores[0];
        if let Instruction::Store64 { src, .. } = &function.instructions[store_pos] {
            if let Operand::Register { id: ref_addr_reg } = src {
                // 找到被引用的栈槽的实际值
                if let Some(referenced_value) = self.find_referenced_stack_value(*ref_addr_reg, function) {
                    println!("🎯 找到被引用的值: {:?}", referenced_value);
                    
                    // 将所有load指令（解引用）替换为直接使用被引用的值
                    for &load_pos in &slot.loads {
                        if load_pos < function.instructions.len() {
                            if let Instruction::Load64 { dst, .. } = &function.instructions[load_pos] {
                                println!("🎯 替换解引用load {} -> mov {:?}, {:?}", load_pos, dst, referenced_value);
                                instructions_to_modify.push((load_pos, Instruction::Move {
                                    dst: *dst,
                                    src: referenced_value.clone(),
                                    span: function.instructions[load_pos].get_span(),
                                }));
                            }
                        }
                    }
                    
                    // 移除引用相关的指令
                    instructions_to_remove.push(store_pos);
                    instructions_to_remove.push(slot.alloc_instruction);
                } else {
                    println!("🎯 无法找到被引用的值，跳过优化");
                }
            }
        }
    }
    
    /// 找到被引用栈槽的实际值
    fn find_referenced_stack_value(&self, ref_addr_reg: RegisterId, function: &LirFunction) -> Option<Operand> {
        // 查找对被引用栈槽的存储，获取其实际值
        for instruction in &function.instructions {
            if let Instruction::Store64 { addr, src, .. } = instruction {
                if *addr == ref_addr_reg {
                    println!("🎯 找到被引用栈槽 {:?} 的存储值: {:?}", ref_addr_reg, src);
                    return Some(src.clone());
                }
            }
        }
        
        println!("🎯 未找到被引用栈槽 {:?} 的存储值", ref_addr_reg);
        None
    }

    /// 追踪寄存器是否是从栈槽加载的，返回栈槽的地址寄存器
    fn trace_register_to_stack_load(&self, register: RegisterId, before_pos: usize, function: &LirFunction) -> Option<RegisterId> {
        // 向前搜索寄存器的定义
        for i in (0..before_pos).rev() {
            match &function.instructions[i] {
                Instruction::Load64 { dst, addr, offset, .. } if *dst == register && *offset == 0 => {
                    // 找到了从栈地址加载的定义
                    return Some(*addr);
                }
                Instruction::Move { dst, .. } if *dst == register => {
                    // 如果是move指令，停止搜索（被重新定义）
                    return None;
                }
                Instruction::Call { .. } => {
                    // 函数调用可能修改寄存器，停止搜索
                    return None;
                }
                _ => {}
            }
        }
        None
    }
}

impl FunctionPass for Memory2RegPass {
    fn name(&self) -> &str {
        "mem2reg"
    }
    
    fn run_on_function(&mut self, function: &mut LirFunction, analyses: &mut AnalysisManager) -> PassResult {
        // 运行分析
        let analysis = self.analyze_stack_slots(function);
        
        if analysis.promotable_slots.is_empty() {
            return PassResult::Unchanged;
        }
        
        // 存储分析结果
        analyses.store_result(self.name().to_string(), Box::new(analysis.clone()));
        
        // 执行变换
        if self.transform_function(function, &analysis) {
            PassResult::Changed
        } else {
            PassResult::Unchanged
        }
    }
    
    fn invalidated_analyses(&self) -> Vec<&'static str> {
        vec!["def-use", "cfg"] // Memory2Reg 会改变控制流和定义使用关系
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
            Instruction::Return { span, .. } |
            Instruction::Label { span, .. } |
            Instruction::Nop { span, .. } |
            Instruction::Load64 { span, .. } |
            Instruction::Store64 { span, .. } |
            Instruction::StructFieldLoad { span, .. } |
            Instruction::Alloc { span, .. } |
            Instruction::Phi { span, .. } => *span,
            _ => Span::dummy(),
        }
    }
} 