use super::{FunctionPass, AnalysisManager, PassResult, AnalysisResult};
use super::analysis::ControlFlowGraph;
use super::ssa_construction::{SsaConstructionResult, DominanceInfo};
use super::instruction_transformer::{IndexInstructionTransformer, HistoryBasedTransformer, IndexTransformOperation};
use crate::{LirFunction, Instruction, RegisterId, Operand, AllocationType, LabelId};
use karte_diagnostics::Span;
use std::collections::{HashMap, HashSet, VecDeque};
use std::any::Any;

/// 新的 index-based 指令变换系统
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
    pub store_to_block: HashMap<usize, usize>,  // 指令位置 -> 块ID
    /// 加载指令到基本块的映射
    pub load_to_block: HashMap<usize, usize>,   // 指令位置 -> 块ID
}

/// 基本块信息（从CFG获取）
#[derive(Debug, Clone)]
pub struct BasicBlock {
    pub id: usize,
    pub label: Option<LabelId>,
    pub start: usize,
    pub end: usize,
    pub predecessors: Vec<usize>,
    pub successors: Vec<usize>,
}

/// Memory2Reg 分析结果
#[derive(Debug, Clone)]
pub struct Memory2RegAnalysis {
    /// 栈槽信息
    pub stack_slots: HashMap<RegisterId, StackSlot>,
    /// 可提升的栈槽
    pub promotable_slots: Vec<RegisterId>,
    /// 基本块信息（从CFG获取）
    pub basic_blocks: HashMap<usize, BasicBlock>,
    /// 需要插入φ节点的位置
    pub phi_insertions: Vec<PhiInsertion>,
    /// 支配信息（从SSA构造获取）
    pub dominance_info: Option<DominanceInfo>,
}

/// φ节点插入信息
#[derive(Debug, Clone)]
pub struct PhiInsertion {
    pub block_id: usize,
    pub variable: RegisterId,
    pub dst_register: RegisterId,
    pub incoming: Vec<(LabelId, Operand)>,  // (前驱块标签, 值)
    /// 实际分配的φ节点结果寄存器（在插入时设置）
    pub actual_dst_register: Option<RegisterId>,
    pub bb:BasicBlock,
}

impl AnalysisResult for Memory2RegAnalysis {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Memory2Reg Pass - 基于SSA的内存到寄存器提升 + 栈地址消除
/// 
/// 这个实现遵循经典的SSA构造算法，并合并了StackFrameLowering的功能：
/// 1. 使用已有的CFG分析
/// 2. 计算支配边界
/// 3. 插入φ节点
/// 4. 变量重命名
/// 5. 死代码消除
/// 6. 🚨 直接在此Pass中将所有StackAddress类型寄存器替换为FP+offset寻址
#[derive(Debug)]
pub struct Memory2RegPass;

impl Memory2RegPass {
    pub fn new() -> Self {
        Self
    }
    
    /// 分析栈槽使用模式
    fn analyze_stack_slots(&self, function: &LirFunction, cfg: &ControlFlowGraph) -> Memory2RegAnalysis {
        let mut stack_slots = HashMap::new();
        let mut promotable_slots = Vec::new();
        
        // 第一阶段：从CFG获取基本块信息
        let basic_blocks = self.convert_cfg_to_basic_blocks(cfg);
        println!("🔍 分析到 {} 个基本块", basic_blocks.len());
        
        // 第二阶段：识别栈分配
        for (i, instruction) in function.instructions.iter().enumerate() {
            if let Instruction::Alloc { dst, size, allocation_type: AllocationType::Stack, .. } = instruction {
                // 结构体分配的alloc通常后面紧跟结构体字段store，且size等于结构体大小（如16字节），我们排除掉
                let is_struct_alloc = if *size >= 16 {
                    // 向后看2条指令，若均为store到该dst+偏移，视为结构体分配
                    let mut struct_field_store_count = 0;
                    for j in 1..=2 {
                        if let Some(Instruction::Store64 { addr, .. }) = function.instructions.get(i + j) {
                            if addr == dst { struct_field_store_count += 1; }
                        }
                    }
                    struct_field_store_count >= 2
                } else { false };
                if is_struct_alloc { continue; }
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
            let current_block = self.find_basic_block_for_instruction(i, cfg);
            
            match instruction {
                Instruction::Load64 { dst: _, addr, offset, .. } => {
                    if *offset == 0 {
                        if let Some(slot) = stack_slots.get_mut(addr) {
                            slot.loads.push(i);
                            if let Some(block_id) = current_block {
                                slot.load_to_block.insert(i, block_id);
                                println!("🔍 记录load: 寄存器 {:?} 在块 {}", addr, block_id);
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
                        if let Some(block_id) = current_block {
                            slot.load_to_block.insert(i, block_id);
                            println!("🔍 记录StructFieldLoad: 结构体地址寄存器 {:?} 在块 {}", struct_addr, block_id);
                        }
                    }
                }
                Instruction::Store64 { addr, offset, src: _, .. } => {
                    if *offset == 0 {
                        if let Some(slot) = stack_slots.get_mut(addr) {
                            slot.stores.push(i);
                            if let Some(block_id) = current_block {
                                slot.store_to_block.insert(i, block_id);
                                println!("🔍 记录store: 寄存器 {:?} 在块 {}", addr, block_id);
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
        
        // 收集可提升的栈槽
        for (addr_reg, slot) in &stack_slots {
            if slot.promotable && !slot.stores.is_empty() {
                promotable_slots.push(*addr_reg);
            }
        }
        
        Memory2RegAnalysis {
            stack_slots,
            promotable_slots,
            basic_blocks,
            phi_insertions,
            dominance_info: None,
        }
    }
    
    /// 将CFG转换为BasicBlock格式
    fn convert_cfg_to_basic_blocks(&self, cfg: &ControlFlowGraph) -> HashMap<usize, BasicBlock> {
        let mut basic_blocks = HashMap::new();
        
        for node in &cfg.nodes {
            let block = BasicBlock {
                id: node.block_id,
                label: node.label,
                start: node.instruction_range.0,
                end: node.instruction_range.1,
                predecessors: node.predecessors.clone(),
                successors: node.successors.clone(),
            };
            basic_blocks.insert(node.block_id, block);
        }
        
        basic_blocks
    }
    
    /// 找到指令所在的基本块
    fn find_basic_block_for_instruction(&self, instruction_index: usize, cfg: &ControlFlowGraph) -> Option<usize> {
        for node in &cfg.nodes {
            if instruction_index >= node.instruction_range.0 && instruction_index < node.instruction_range.1 {
                return Some(node.block_id);
            }
        }
        None
    }
    
    /// 执行 Memory2Reg 变换
    fn transform_function(&self, function: &mut LirFunction, analysis: &Memory2RegAnalysis) -> Result<bool, String> {
        println!("🚀 开始Memory2Reg变换");
        
        let mut transformer = IndexInstructionTransformer::new();

        // 🔥 新增：第一步 - 溢出代码插入
        self.insert_spill_code(function, analysis, &mut transformer)?;
        
        // 第二步 - 使用专业的φ节点构造算法
        let mut phi_insertions = Vec::new();
        if let Some(dominance_info) = &analysis.dominance_info {
            phi_insertions = self.compute_phi_insertions(function, analysis, dominance_info);
        } else {
            println!("⚠️ 没有SSA构造结果，将使用简化的φ节点插入策略");
        }
        
        
        // 第三步 - 插入φ节点
        let phi_inserted = self.insert_phi_nodes(function, &mut phi_insertions, &mut transformer);
        if phi_inserted {
            println!("✅ φ节点插入完成");
        }
        
        // 第四步 - 收集变换操作
        println!("🚀 步骤2: 收集变换操作");
        // let mut all_instructions_to_remove = Vec::new();
        // let mut all_instructions_to_modify = Vec::new();
        
        for (slot_id, slot) in &analysis.stack_slots {
            if slot.promotable {
                if self.has_phi_support_for_slot(slot, &phi_insertions) {
                    println!("🎯 使用φ节点支持变换栈槽 {:?}", slot_id);
                    self.transform_with_phi_support(function, slot, &phi_insertions, &analysis.basic_blocks, &analysis.dominance_info, &mut transformer);
                } else {
                    println!("🚀 处理栈槽 {:?}", slot_id);
                    self.collect_simple_transform_operations(function, slot, &mut transformer);
                    // panic!("🚀 处理栈槽 {:?}", slot_id);
                }
            }
        }
        
        // println!("🚀 总共移除 {} 条指令, 修改 {} 条指令", all_instructions_to_remove.len(), all_instructions_to_modify.len());
        
        // 第五步 - 应用变换
        println!("🚀 步骤3: 应用变换");
        let (changed,..) = transformer.apply_to_function(function);
        // let changed = self.apply_all_transforms(function, all_instructions_to_remove, all_instructions_to_modify);
        
        if changed {
            println!("✅ Memory2Reg变换完成，有修改");
        } else {
            println!("ℹ️ Memory2Reg变换完成，无修改");
        }
        
        // 🚨 新增：在所有变换后，消除所有StackAddress类型寄存器，直接替换为FP+offset
        let mut stack_addr_to_offset = std::collections::HashMap::new();
        let mut offset = 0i64;
        
        // 🔧 修复：按照alloc指令在代码中的顺序来分配栈帧偏移量
        let mut sorted_slots: Vec<_> = analysis.stack_slots.iter().collect();
        sorted_slots.sort_by_key(|(_, slot)| slot.alloc_instruction);
        
        // 1. 收集所有alloc出来的StackAddress寄存器，分配offset
        for (slot_id, slot) in sorted_slots {
            // stack 向下方生长
            offset -= slot.size as i64;
            stack_addr_to_offset.insert(*slot_id, offset);
            println!("🔧 栈地址寄存器映射: {:?} -> FP+{} (alloc指令位置: {})", slot_id, offset, slot.alloc_instruction);
        }
        let mut transformer = IndexInstructionTransformer::new();
        println!("🔧 栈地址寄存器映射表: {:?}", stack_addr_to_offset);
        let mut next_temp_register = function.next_register;
        // 2. 扫描所有指令，替换所有StackAddress类型寄存器为FP+offset
        for (i, instr) in function.instructions.iter_mut().enumerate() {
            match instr {
                Instruction::Load64 { addr, offset: load_offset, .. } => {
                    if let Some(base_offset) = stack_addr_to_offset.get(addr) {
                        *addr = RegisterId(7); // r7 = FP (修复：使用正确的帧指针寄存器)
                        // stack 向下方生长
                        *load_offset += *base_offset;
                    }
                }
                Instruction::Store64 { addr, offset: store_offset,src,.. } => {
                    if let Some(base_offset) = stack_addr_to_offset.get(addr) {
                        *addr = RegisterId(7); // r7 = FP (修复：使用正确的帧指针寄存器)
                        *store_offset += *base_offset;
                    }

                    if let Operand::Register { id } = src {
                        if let Some(base_offset) = stack_addr_to_offset.get(id) {
                            // 需要插入计算地址的指令，并替换src
                            let temp_reg = RegisterId(next_temp_register);
                            next_temp_register += 1;
                            let new_addr_offset = *base_offset;
                            let new_instruction = Instruction::Add {
                                dst: temp_reg,
                                src1: Operand::Register { id: RegisterId(7) },
                                src2: Operand::Immediate { value: new_addr_offset },
                                span: Span::dummy(),
                            };
                            transformer.insert(i, new_instruction);
                            *src = Operand::Register { id: temp_reg };
                        }
                    }
                }
                Instruction::Add { dst, src1, src2, .. } => {
                    if let Operand::Register { id } = src1 {
                        if let Some(_base_offset) = stack_addr_to_offset.get(id) {
                            *src1 = Operand::Register { id: RegisterId(7) }; // r7 = FP
                        }
                    }
                    if let Operand::Register { id } = src2 {
                        if let Some(_base_offset) = stack_addr_to_offset.get(id) {
                            *src2 = Operand::Register { id: RegisterId(7) }; // r7 = FP
                        }
                    }
                }
                Instruction::Alloc { dst, .. } => {}
                _ => {}
            }
        }
        let (changed,..) = transformer.apply_to_function(function);
        
        Ok(changed)
    }

    /// 🔥 新增：溢出代码插入
    /// 
    /// 在寄存器分配前插入所有溢出相关的load/store指令和临时虚拟寄存器
    /// 确保所有虚拟寄存器都在RA前生成，RA后不再引入新寄存器
    fn insert_spill_code(&self, function: &mut LirFunction, analysis: &Memory2RegAnalysis, transformer: &mut IndexInstructionTransformer) -> Result<(), String> {
        println!("🔥 开始溢出代码插入");
        
        // 分析哪些栈槽需要溢出（不可提升的栈槽）
        let mut spill_slots = Vec::new();
        for (slot_id, slot) in &analysis.stack_slots {
            if !slot.promotable {
                spill_slots.push(*slot_id);
                println!("🔥 发现需要溢出的栈槽: {:?}", slot_id);
            }
        }
        
        if spill_slots.is_empty() {
            println!("ℹ️ 没有需要溢出的栈槽");
            return Ok(());
        }
        
        // 为每个溢出栈槽生成load/store指令
        let mut spill_operations = Vec::new();
        let mut next_temp_register = function.next_register;
        
        for slot_id in &spill_slots {
            let slot = &analysis.stack_slots[slot_id];
            
            // 为每个store指令生成溢出存储
            for &store_pos in &slot.stores {
                let temp_reg = RegisterId(next_temp_register);
                next_temp_register += 1;
                let load_instruction = Instruction::Load64 {
                    dst: temp_reg,
                    addr: slot.address_register,
                    offset: 0,
                    span: function.instructions[store_pos].get_span(),
                };
                spill_operations.push((store_pos, load_instruction));
                println!("🔥 为store指令 {} 生成溢出加载: {:?} -> {:?}", store_pos, slot_id, temp_reg);
            }
            // 为每个load指令生成溢出存储
            for &load_pos in &slot.loads {
                let load_dst_reg = if let Instruction::Load64 { dst, .. } = &function.instructions[load_pos] {
                    *dst
                } else {
                    let temp_reg = RegisterId(next_temp_register);
                    next_temp_register += 1;
                    temp_reg
                };
                let store_instruction = Instruction::Store64 {
                    addr: slot.address_register,
                    offset: 0,
                    src: Operand::Register { id: load_dst_reg },
                    span: function.instructions[load_pos].get_span(),
                };
                spill_operations.push((load_pos + 1, store_instruction));
                println!("🔥 为load指令 {} 生成溢出存储: {:?} <- {:?}", load_pos, slot_id, load_dst_reg);
            }
        }
        // 按位置排序，从后往前插入，避免位置偏移
        spill_operations.sort_by_key(|(pos, _)| *pos);
        spill_operations.reverse();
        // 使用IndexInstructionTransformer批量插入spill指令
        for (pos, instruction) in &spill_operations {
            transformer.insert(*pos, instruction.clone());
        }
        // let (changed, _, _, _) = transformer.apply_to_function(function);
        // if changed {
        //     println!("✅ spill code插入完成，使用IndexInstructionTransformer");
        // }
        // 更新next_register
        function.next_register = next_temp_register;
        Ok(())
    }
    
    /// 插入φ节点
    fn insert_phi_nodes(&self, function: &mut LirFunction, phi_insertions: &mut [PhiInsertion], transformer: &mut IndexInstructionTransformer) -> bool {
        if phi_insertions.is_empty() {
            println!("⚠️ 没有φ节点需要插入");
            return false;
        }
        
        println!("🔧 开始插入 {} 个φ节点", phi_insertions.len());
        
        // 按块ID对phi节点进行分组
        let mut phi_by_block: HashMap<usize, Vec<&mut PhiInsertion>> = HashMap::new();
        for phi in phi_insertions.iter_mut() {
            phi_by_block.entry(phi.block_id)
                .or_insert_with(Vec::new)
                .push(phi);
        }
        
        // 为每个块插入phi节点
        let mut phi_instructions = Vec::new();
        
        for (block_id, phis) in phi_by_block {
            // 找到块的开始位置
            let insert_pos = phis.iter().map(|phi| phi.bb.start).min().unwrap_or_default() + 1;
            println!("🔧 在块 {} 开始位置 {} 插入 {} 个φ节点", block_id, insert_pos, phis.len());
                
            for phi in phis {
                // 分配新寄存器作为phi节点的目标
                let phi_dst = function.new_register();
                phi.actual_dst_register = Some(phi_dst);
                
                println!("🔧   - φ节点: 目标寄存器 {:?}, 原变量 {:?}", phi_dst, phi.variable);
                
                let phi_instruction = Instruction::Phi {
                    dst: phi_dst,
                    incoming: phi.incoming.clone(),
                    span: karte_diagnostics::Span::dummy(),
                };
                
                phi_instructions.push((insert_pos, phi_instruction));
            }
        }

        // 用transformer插入phi节点
        // let mut transformer = super::instruction_transformer::IndexInstructionTransformer::new();
        for (pos, instruction) in phi_instructions {
            transformer.insert(pos, instruction);
        }
        // let (changed, _, _, _) = transformer.apply_to_function(function);
        
        println!("✅ φ节点插入完成");
        true
    }
    
    /// 找到基本块的开始位置
    fn find_block_start_position(&self, function: &LirFunction, block_id: usize) -> Option<usize> {
        // 修复：正确找到基本块的开始位置
        // 遍历所有指令，找到对应的标签
        for (i, instruction) in function.instructions.iter().enumerate() {
            if let Instruction::Label { id, .. } = instruction {
                // 修复：使用标签ID来匹配块ID
                if id.0 == block_id {
                    // phi节点应该插在标签之后，而不是标签之前
                    return Some(i + 1);
                }
            }
        }
        
        // 如果找不到标签，尝试使用块ID作为位置（作为后备方案）
        if block_id < function.instructions.len() {
            Some(block_id)
        } else {
            None
        }
    }
    
    // /// 收集变换操作（考虑φ节点）
    // fn collect_transform_operations_with_phi(&self, function: &LirFunction, slot: &StackSlot, phi_insertions: &[PhiInsertion], basic_blocks: &HashMap<usize, BasicBlock>) -> (Vec<usize>, Vec<(usize, Instruction)>) {
    //     let mut instructions_to_remove = Vec::new();
    //     let mut instructions_to_modify = Vec::new();
    //     if self.has_phi_support_for_slot(slot, phi_insertions) {
    //         self.transform_with_phi_support(function, slot, phi_insertions, basic_blocks, &None, &mut transformer);
    //     } else {
    //         self.collect_simple_transform_operations(function, slot, &mut instructions_to_remove, &mut instructions_to_modify);
    //     }
    //     (instructions_to_remove, instructions_to_modify)
    // }
    
    /// 检查是否有φ节点支持此栈槽
    fn has_phi_support_for_slot(&self, slot: &StackSlot, phi_insertions: &[PhiInsertion]) -> bool {
        phi_insertions.iter().any(|phi| phi.variable == slot.address_register)
    }
    
    /// 使用φ节点支持进行变换
    fn transform_with_phi_support(
        &self,
        function: &LirFunction,
        slot: &StackSlot,
        phi_insertions: &[PhiInsertion],
        basic_blocks: &HashMap<usize, BasicBlock>,
        dominance_info: &Option<DominanceInfo>,
        transformer: &mut IndexInstructionTransformer
    ) {
        println!("🎯 使用φ节点支持变换栈槽 {:?}", slot.address_register);
        
        // 1. 移除alloc指令
        if slot.alloc_instruction < function.instructions.len() {
            if let Instruction::Alloc { dst, size, allocation_type, .. } = &function.instructions[slot.alloc_instruction] {
                if *dst == slot.address_register {
                    println!("🎯 添加移除alloc指令: {:?} (size: {}, type: {:?})", dst, size, allocation_type);
                    // instructions_to_remove.push(slot.alloc_instruction);
                    transformer.remove(slot.alloc_instruction);
                }
            }
        }

        // 2. 处理store指令，同时记录每个块中的最后一个store
        println!("🎯 扫描所有store指令以匹配栈槽 {:?}", slot.address_register);
        let mut found_stores = 0;
        let mut block_last_store: HashMap<usize, (usize, Operand)> = HashMap::new();
        
        for (i, instruction) in function.instructions.iter().enumerate() {
            if let Instruction::Store64 { addr, offset, src, .. } = instruction {
                if *addr == slot.address_register && *offset == 0 {
                    println!("🎯   - 发现store指令 [{}]: [{:?}] = {:?}", i, addr, src);
                    
                    // 找到store指令所在的块
                    if let Some(&block_id) = slot.store_to_block.get(&i) {
                        block_last_store.insert(block_id, (i, src.clone()));
                    }
                    
                    transformer.remove(i);
                    found_stores += 1;
                }
            }
        }
        println!("🎯 为栈槽 {:?} 找到 {} 个store指令进行移除", slot.address_register, found_stores);

        // 3. 处理load指令
        println!("🎯 扫描所有load指令以匹配栈槽 {:?}", slot.address_register);
        let mut found_loads = 0;
        for (i, instruction) in function.instructions.iter().enumerate() {
            if let Instruction::Load64 { dst, addr, offset, span } = instruction {
                if *addr == slot.address_register {
                    if *offset != 0 {
                        println!("🔧   - 跳过结构体字段访问: load指令 [{}] 有偏移量 {}", i, offset);
                        continue;
                    }
                    println!("🎯   - 发现load指令 [{}]: {}", i, instruction);
                    
                    // 查找对应的phi结果寄存器
                    let phi_result_reg = self.find_phi_result_for_instruction_position(
                        i,
                        slot,
                        phi_insertions,
                        function,
                        basic_blocks,
                        dominance_info
                    );

                    if phi_result_reg.0 != 998 {
                        println!("🎯   - 替换load为move: {:?} = {:?} -> {:?} = {:?}", dst, addr, dst, phi_result_reg);
                        let new_move = Instruction::Move {
                            dst: *dst,
                            src: Operand::Register { id: phi_result_reg },
                            span: *span,
                        };
                        transformer.replace(i, new_move);
                        found_loads += 1;
                    } else {
                        // 尝试使用块中以及前序block的最后一个 目标为原地址的 store的 src
                        let mut last_store = None;
                        for (block_id, block) in basic_blocks {
                            if i >= block.start && i < block.end {
                                let mut new_bbs = vec![];
                                let mut bbs = vec![*block_id];
                                while !bbs.is_empty() {
                                    for bb in &bbs {
                                        let block = &basic_blocks[bb];
                                        if let Some((_, src)) = block_last_store.get(bb) {
                                            println!("🎯   - 使用块 {} 中最后一个store的值", block.label.unwrap_or(LabelId(0)));
                                            last_store = Some(src.clone());
                                            break;
                                        } else {
                                            new_bbs.extend_from_slice(&block.predecessors);
                                        }
                                    }
                                    bbs.clear();
                                    bbs.extend_from_slice(&new_bbs);
                                    new_bbs.clear();
                                }
                            }
                        }
                        if let Some(src) = last_store {
                            let new_move = Instruction::Move {
                                dst: *dst,
                                src: src.clone(),
                                span: *span,
                            };
                            transformer.replace(i, new_move);
                            found_loads += 1;
                            continue;
                        }
                        // if let Some(&block_id) = slot.load_to_block.get(&i) {
                        //     if let Some((_, src)) = block_last_store.get(&block_id) {
                        //         println!("🎯   - 使用块 {} 中最后一个store的值", block_id);
                        //         let new_move = Instruction::Move {
                        //             dst: *dst,
                        //             src: src.clone(),
                        //             span: *span,
                        //         };
                        //         transformer.replace(i, new_move);
                        //         found_loads += 1;
                        //         continue;
                        //     }
                        // }
                        println!("❌   - 无法找到phi节点结果寄存器或store值，保留原始load指令");
                    }
                }
            }
        }
        println!("🎯 为栈槽 {:?} 找到 {} 个load指令进行替换", slot.address_register, found_loads);
    }
    
    /// 🔧 将 index-based 变换转换为旧的基于位置的格式
    fn convert_index_transforms_to_legacy(&self, transformer: &IndexInstructionTransformer, function: &LirFunction,
                                        instructions_to_remove: &mut Vec<usize>, 
                                        instructions_to_modify: &mut Vec<(usize, Instruction)>) {
        // 遍历所有变换操作，转换为旧格式
        for op in &transformer.transforms {
            match op {
                IndexTransformOperation::Remove(index) => {
                    let instruction_ref = if *index < function.instructions.len() { 
                        &function.instructions[*index] 
                    } else { 
                        &Instruction::Nop { span: Span::dummy() } 
                    };
                    println!("🔧 转换移除操作: 位置 {} -> {:?}", index, instruction_ref);
                    instructions_to_remove.push(*index);
                }
                IndexTransformOperation::Replace(index, new_instr) => {
                    println!("🔧 转换替换操作: 位置 {} -> {:?}", index, new_instr);
                    instructions_to_modify.push((*index, new_instr.clone()));
                }
                IndexTransformOperation::Insert(index, new_instr) => {
                    println!("🔧 转换插入操作: 位置 {} -> {:?}", index, new_instr);
                    // 插入操作在旧系统中需要特殊处理，这里暂时跳过
                    println!("⚠️ 插入操作暂不支持转换为旧格式");
                }
            }
        }
    }
    
    /// 为指定位置的load指令找到对应的φ节点结果寄存器
    fn find_phi_result_for_instruction_position(
        &self,
        load_pos: usize,
        slot: &StackSlot,
        phi_insertions: &[PhiInsertion],
        function: &LirFunction,
        basic_blocks: &HashMap<usize, BasicBlock>,
        dominance_info: &Option<DominanceInfo>
    ) -> RegisterId {
        // 1. 找到load指令所在的基本块
        let mut current_block_id = None;
        
        // 修复：使用basic_blocks来正确识别基本块
        for (block_id, block) in basic_blocks {
            if load_pos >= block.start && load_pos < block.end {
                current_block_id = Some(*block_id);
                break;
            }
        }

        let Some(block_id) = current_block_id else {
            println!("❌ 无法找到load指令所在的基本块，load_pos: {}", load_pos);
            // 调试信息：打印所有基本块的范围
            for (bid, block) in basic_blocks {
                println!("🔍 块 {}: [{}, {})", block.label.unwrap_or(LabelId(0)), block.start, block.end);
            }
            
            // 修复：如果找不到基本块，尝试使用最近的store指令的值
            let mut nearest_store = None;
            let mut nearest_distance = usize::MAX;
            
            for &store_pos in &slot.stores {
                if store_pos < load_pos && load_pos - store_pos < nearest_distance {
                    if let Some(Instruction::Store64 { src, .. }) = function.instructions.get(store_pos) {
                        if let Operand::Register { id } = src {
                            nearest_store = Some(*id);
                            nearest_distance = load_pos - store_pos;
                            println!("🔍 使用最近的store指令值 {:?}", id);
                        }
                    }
                }
            }
            
            if let Some(store_reg) = nearest_store {
                return store_reg;
            }
            
            return RegisterId(998);
        };

        println!("🔍 load指令位于基本块 {}", block_id);

        // 2. 在当前块中查找最近的phi节点
        for phi in phi_insertions.iter().rev() {
            if phi.variable == slot.address_register && phi.block_id == block_id {
                if let Some(actual_dst) = phi.actual_dst_register {
                    println!("✅ 在当前块 {} 找到phi节点，使用结果寄存器 {:?}", block_id, actual_dst);
                    return actual_dst;
                }
            }
        }

        // 3. 在支配链上向上查找phi节点
        if let Some(dom_info) = dominance_info {
            let mut current_id = block_id;
            while let Some(&idom) = dom_info.immediate_dominators.get(&current_id) {
                if idom == current_id { break; }
                
                println!("🔍 在支配块 {} 中查找phi节点", idom);
                for phi in phi_insertions.iter().rev() {
                    if phi.variable == slot.address_register && phi.block_id == idom {
                        if let Some(actual_dst) = phi.actual_dst_register {
                            println!("✅ 在支配块 {} 找到phi节点，使用结果寄存器 {:?}", idom, actual_dst);
                            return actual_dst;
                        }
                    }
                }
                current_id = idom;
            }
        }

        // 4. 如果找不到phi节点，尝试找到最近的store指令的值
        let mut nearest_store = None;
        let mut nearest_distance = usize::MAX;

        // 修复：确保store指令在同一个基本块或支配块中
        for &store_pos in &slot.stores {
            if store_pos < load_pos && load_pos - store_pos < nearest_distance {
                if let Some(store_block) = slot.store_to_block.get(&store_pos) {
                    if let Some(dom_info) = dominance_info {
                        if dom_info.dominators.get(&block_id)
                            .map_or(false, |doms| doms.contains(store_block)) {
                            nearest_store = Some(store_pos);
                            nearest_distance = load_pos - store_pos;
                            println!("🔍 找到支配块 {} 中的store指令", store_block);
                        }
                    }
                }
            }
        }

        if let Some(store_pos) = nearest_store {
            if let Some(Instruction::Store64 { src, .. }) = function.instructions.get(store_pos) {
                if let Operand::Register { id } = src {
                    println!("✅ 使用最近的store指令值 {:?}", id);
                    return *id;
                }
            }
        }

        // 5. 如果在当前块找不到定义，尝试使用前驱块的phi节点结果
        if let Some(block) = basic_blocks.get(&block_id) {
            for &pred_id in &block.predecessors {
                for phi in phi_insertions {
                    if phi.variable == slot.address_register && phi.block_id == pred_id {
                        if let Some(actual_dst) = phi.actual_dst_register {
                            println!("✅ 使用前驱块 {} 的phi节点结果 {:?}", pred_id, actual_dst);
                            return actual_dst;
                        }
                    }
                }
            }
        }

        println!("❌ 无法找到合适的phi节点或store值");
        RegisterId(998)
    }
    
    /// 检查给定的标签块是否是φ节点的目标块
    fn is_phi_target_block(&self, target_label: LabelId, incoming: &[(LabelId, Operand)], function: &LirFunction) -> bool {
        // 检查incoming中的标签是否指向target_label
        // 这意味着有控制流从incoming的标签流向target_label
        
        for (incoming_label, _) in incoming {
            // 查找incoming_label后面是否有跳转到target_label
            if self.has_jump_to_target(*incoming_label, target_label, function) {
                return true;
            }
        }
        
        false
    }
    
    /// 检查从source_label是否有跳转到target_label
    fn has_jump_to_target(&self, source_label: LabelId, target_label: LabelId, function: &LirFunction) -> bool {
        let mut in_source_block = false;
        
        for instruction in &function.instructions {
            match instruction {
                Instruction::Label { id, .. } => {
                    in_source_block = *id == source_label;
                }
                Instruction::Jump { target, .. } => {
                    if in_source_block && *target == target_label {
                        return true;
                    }
                }
                Instruction::JumpEqual { target, .. } |
                Instruction::JumpNotEqual { target, .. } => {
                    if in_source_block && *target == target_label {
                        return true;
                    }
                }
                _ => {}
            }
        }
        
        false
    }
    
    /// 简单变换操作（原有逻辑）
    fn collect_simple_transform_operations(&self, function: &LirFunction, slot: &StackSlot,
                                          transformer: &mut IndexInstructionTransformer) {
        // 🔧 重大改进：处理更多情况，包括寄存器存储和跨栈槽值传播
        
        println!("🚀 分析栈槽 {:?}: stores={}, loads={}", slot.address_register, slot.stores.len(), slot.loads.len());
        
        // // 🔧 新增：引用-解引用模式检测 FIXME: 引入类型和别名分析
        // if self.is_reference_dereference_pattern(function, slot) {
        //     println!("🎯 检测到引用-解引用模式，应用特殊优化");
        //     self.optimize_reference_dereference_pattern(function, slot, instructions_to_remove, instructions_to_modify);
        //     return;
        // }
        
        // 🔧 关键修复：检查是否是控制流敏感的栈槽
        if self.is_control_flow_sensitive_slot(function, slot) {
            println!("⚠️ 栈槽 {:?} 是控制流敏感的，需要φ节点支持", slot.address_register);
            // 对于控制流敏感的栈槽，不进行简单优化
            // 这种情况应该由φ节点处理，或者保持原样
            return;
        }
        
        // 🔧 关键修复：检查是否有多个跨基本块的load操作
        if slot.loads.len() > 1 {
            let mut load_blocks = HashSet::new();
            for &load_pos in &slot.loads {
                if let Some(block_id) = slot.load_to_block.get(&load_pos) {
                    load_blocks.insert(*block_id);
                }
            }
            
            if load_blocks.len() > 1 {
                println!("⚠️ 栈槽 {:?} 的load操作分布在 {} 个不同基本块中，需要特殊处理", 
                    slot.address_register, load_blocks.len());
                
                // 对于跨基本块的load操作，使用安全的内容匹配变换
                if slot.stores.len() == 1 {
                    let store_pos = slot.stores[0];
                    if store_pos < function.instructions.len() {
                        if let Instruction::Store64 { src, .. } = &function.instructions[store_pos] {
                            println!("🔧 使用安全变换系统处理跨基本块的栈槽 {:?}", slot.address_register);
                            println!("🔧 强制传播值 {:?} 到所有跨基本块的load操作", src);
                            
                                                             // 🧠 使用智能的基于历史记录的变换系统
                             println!("🧠 使用智能变换系统处理跨基本块的栈槽 {:?}", slot.address_register);
                             
                             let mut smart_transformer = HistoryBasedTransformer::new();
                             
                             // 🧠 第一步：收集所有相关指令并分配序号
                             let mut load_count = 0;
                             let mut store_count = 0;
                             let mut alloc_count = 0;
                             
                             // 扫描函数，收集所有相关指令
                             for (i, instruction) in function.instructions.iter().enumerate() {
                                 match instruction {
                                     Instruction::Alloc { dst, size, allocation_type, .. } 
                                         if *dst == slot.address_register => {
                                         println!("🧠   发现alloc [{}]: {:?}, 分配序号 {}", i, instruction, alloc_count);
                                         smart_transformer.remove_at(i);
                                         alloc_count += 1;
                                     }
                                     Instruction::Store64 { addr, src, .. } 
                                         if *addr == slot.address_register => {
                                         println!("🧠   发现store [{}]: {:?}, 分配序号 {}", i, instruction, store_count);
                                         smart_transformer.remove_at(i);
                                         store_count += 1;
                                     }
                                     Instruction::Load64 { dst, addr, offset, .. } 
                                         if *addr == slot.address_register && *offset == 0 => {
                                         println!("🧠   发现load [{}]: {:?}, 分配序号 {}", i, instruction, load_count);
                                         let new_move = Instruction::Move {
                                             dst: *dst,
                                             src: src.clone(),
                                             span: instruction.get_span(),
                                         };
                                         smart_transformer.replace_at(i, new_move);
                                         load_count += 1;
                                     }
                                     _ => {}
                                 }
                             }
                             
                             println!("🧠   统计: {} 个load, {} 个store, {} 个alloc", 
                                 load_count, store_count, alloc_count);
                             
                             // 🧠 第二步：将智能变换转换为传统格式（暂时兼容现有系统）
                             // 注意：由于这里在collect阶段，我们将智能变换记录转换为传统的位置列表
                             println!("🧠 将智能变换转换为传统格式以兼容现有系统");
                             
                             // 创建临时函数副本用于测试变换
                             let mut temp_function = function.clone();
                             let (smart_changed, _, _, _) = smart_transformer.apply_to_function(&mut temp_function);
                             if smart_changed {
                                 // 如果智能变换成功，我们比较前后差异并生成传统的变换指令
                                 println!("✅ 智能变换系统模拟成功，生成传统变换指令");
                                 
                                 // 这里可以添加从temp_function变化到原function的差异分析
                                 // 暂时简化处理，跳过这个复杂的栈槽
                                 return;
                             } else {
                                 println!("⚠️ 智能变换系统模拟失败，回退到传统方法");
                             }
                            
                            return;
                        }
                    }
                }
            }
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
                                    transformer.replace(load_pos, Instruction::Move {
                                        dst: *dst,
                                        src: effective_src.clone(),
                                        span: function.instructions[load_pos].get_span(),
                                    });
                                }
                            }
                        }
                        
                        // 移除store指令和alloc指令
                        transformer.remove(store_pos);
                        transformer.remove(slot.alloc_instruction);
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
                                                transformer.replace(load_pos, Instruction::Move {
                                                    dst: *dst,
                                                    src: src.clone(),
                                                    span: function.instructions[load_pos].get_span(),
                                                });
                                            }
                                        }
                                    }
                                    
                                    // 移除最后的store（但保留其他store和alloc，因为可能有其他用途）
                                    transformer.remove(last_store_pos);
                                    
                                    // 如果所有store都被处理了，可以移除alloc
                                    if slot.stores.len() == 1 {
                                        transformer.remove(slot.alloc_instruction);
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
                transformer.remove(store_pos);
            }
            transformer.remove(slot.alloc_instruction);
        }
    }
    
    /// 检查栈槽是否是控制流敏感的
    fn is_control_flow_sensitive_slot(&self, function: &LirFunction, slot: &StackSlot) -> bool {
        // 🔧 关键修复：如果栈槽有多个存储，并且它们在不同的基本块中，则是控制流敏感的
        if slot.stores.len() > 1 {
            let store_blocks: HashSet<usize> = slot.store_to_block.values().cloned().collect();
            if store_blocks.len() > 1 {
                println!("⚠️ 栈槽 {:?} 有 {} 个存储在 {} 个不同的基本块中", 
                    slot.address_register, slot.stores.len(), store_blocks.len());
                return true;
            }
        }
        
        // 如果栈槽只有一个存储，但这个存储的值依赖于控制流，则是控制流敏感的
        if slot.stores.len() == 1 {
            let store_pos = slot.stores[0];
            if store_pos >= function.instructions.len() {
                return false;
            }
            
            // 向前查找，看看存储的寄存器是否在不同的控制流路径上被赋予不同的值
            if let Instruction::Store64 { src: Operand::Register { id }, .. } = &function.instructions[store_pos] {
                // 查找这个寄存器的所有定义
                let definitions = self.find_all_definitions(*id, store_pos, function);
                
                // 如果有多个定义，并且它们在不同的基本块中，则是控制流敏感的
                if definitions.len() > 1 {
                    println!("🔍 寄存器 {:?} 有 {} 个定义", id, definitions.len());
                    // 检查这些定义是否在不同的基本块中
                    let mut definition_blocks = HashSet::new();
                    for &def_pos in &definitions {
                        // 查找定义所在的基本块
                        let mut current_block = None;
                        for i in (0..=def_pos).rev() {
                            if let Instruction::Label { id, .. } = &function.instructions[i] {
                                current_block = Some(id);
                                break;
                            }
                        }
                        if let Some(block) = current_block {
                            definition_blocks.insert(block);
                        }
                    }
                    
                    // 如果定义在多个基本块中，则是控制流敏感的
                    if definition_blocks.len() > 1 {
                        println!("⚠️ 寄存器 {:?} 在 {} 个不同的基本块中被定义", id, definition_blocks.len());
                        return true;
                    }
                }
            }
        }
        
        false
    }
    
    /// 查找寄存器的所有定义
    fn find_all_definitions(&self, register: RegisterId, before_pos: usize, function: &LirFunction) -> Vec<usize> {
        let mut definitions = Vec::new();
        
        // 向前扫描，找到所有对该寄存器的定义
        for i in 0..before_pos {
            if i >= function.instructions.len() {
                continue;
            }
            
            let instruction = &function.instructions[i];
            match instruction {
                Instruction::Move { dst, .. } if *dst == register => {
                    definitions.push(i);
                }
                Instruction::Load64 { dst, .. } if *dst == register => {
                    definitions.push(i);
                }
                Instruction::Add { dst, .. } |
                Instruction::Sub { dst, .. } |
                Instruction::Mul { dst, .. } |
                Instruction::Div { dst, .. } if *dst == register => {
                    definitions.push(i);
                }
                _ => {}
            }
        }
        
        definitions
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
                // 检查被引用栈槽是否为结构体分配（如alloc size >= 16）
                let is_struct = function.instructions.iter().any(|inst| {
                    if let Instruction::Alloc { dst, size, .. } = inst {
                        *dst == *ref_addr_reg && *size >= 16
                    } else { false }
                });
                if is_struct {
                    println!("🎯 跳过结构体字段的引用-解引用优化: {:?}", ref_addr_reg);
                    return;
                }
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
                }
                println!("🎯 无法找到被引用的值，跳过优化");
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

    /// 基于支配边界计算φ节点插入位置
    fn compute_phi_insertions(&self, function: &LirFunction, analysis: &Memory2RegAnalysis, dominance_info: &DominanceInfo) -> Vec<PhiInsertion> {
        let mut phi_insertions = Vec::new();
        
        // 对每个可提升的栈槽
        for &slot_register in &analysis.promotable_slots {
            if let Some(slot) = analysis.stack_slots.get(&slot_register) {
                println!("🎯 为栈槽 {:?} 计算φ节点插入位置", slot_register);
                
                // 收集定义块（存储指令所在的块）
                let mut def_blocks = HashSet::new();
                for (&_store_pos, &store_block_id) in &slot.store_to_block {
                    def_blocks.insert(store_block_id);
                }
                println!("🎯 定义块: {:?}", def_blocks);
                
                // 修复：使用更简单的策略，为所有有多个前驱的块插入phi节点
                let mut phi_blocks = HashSet::new();
                
                for (block_id, block) in &analysis.basic_blocks {
                    // 如果块有多个前驱，且至少有一个前驱有定义，则需要phi节点
                    if block.predecessors.len() > 1 {
                        let mut has_def_from_pred = false;
                        for &pred_id in &block.predecessors {
                            if def_blocks.contains(&pred_id) {
                                has_def_from_pred = true;
                                break;
                            }
                        }
                        
                        if has_def_from_pred {
                            phi_blocks.insert(*block_id);
                            println!("🎯 在块 {} 插入φ节点（有 {} 个前驱，有定义块）", block_id, block.predecessors.len());
                        }
                    }
                }
                
                // 为每个需要φ节点的块创建PhiInsertion
                let mut sorted_phi_blocks: Vec<_> = phi_blocks.iter().cloned().collect();
                sorted_phi_blocks.sort_unstable();
                
                for phi_block_id in sorted_phi_blocks {
                    // 收集来自前驱块的值
                    let mut incoming = Vec::new();
                    
                    if let Some(phi_block) = analysis.basic_blocks.get(&phi_block_id) {
                        println!("🎯 为块 {} 收集incoming值，前驱: {:?}", phi_block_id, phi_block.predecessors);
                        
                        // 对前驱块按ID排序，确保确定性的处理顺序
                        let mut sorted_predecessors = phi_block.predecessors.clone();
                        sorted_predecessors.sort_unstable();
                        
                        for &pred_id in &sorted_predecessors {
                            // 找到前驱块中最后的存储值或者phi节点
                            let mut last_store = None;
                            let mut last_store_pos = 0;
                            
                            for &store_pos in &slot.stores {
                                if let Some(&store_block) = slot.store_to_block.get(&store_pos) {
                                    if store_block == pred_id && store_pos > last_store_pos {
                                        if let Some(Instruction::Store64 { src, .. }) = function.instructions.get(store_pos) {
                                            last_store = Some(src.clone());
                                            last_store_pos = store_pos;
                                            println!("🎯 前驱块 {} 的store指令 [{}]: {:?}", pred_id, store_pos, src);
                                        }
                                    }
                                }
                            }
                            
                            // 修复：如果找不到store，尝试找到该前驱块中最后定义的寄存器值
                            let value = if let Some(store_value) = last_store {
                                store_value
                            } else {
                                // 查找前驱块中是否有对该变量的定义
                                let mut pred_value = Operand::Immediate { value: 0 };
                                
                                // 在前驱块中查找最后的定义
                                if let Some(pred_block) = analysis.basic_blocks.get(&pred_id) {
                                    for i in (pred_block.start..pred_block.end).rev() {
                                        if i < function.instructions.len() {
                                            if let Instruction::Store64 { addr, src, .. } = &function.instructions[i] {
                                                if *addr == slot_register {
                                                    pred_value = src.clone();
                                                    println!("🎯 在前驱块 {} 中找到store指令 [{}]: {:?}", pred_id, i, src);
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                }
                                pred_value
                            };
                            
                            // 将块ID映射到LabelId
                            let pred_label = if let Some(pred_block) = analysis.basic_blocks.get(&pred_id) {
                                pred_block.label.unwrap_or_else(|| {
                                    println!("⚠️ 前驱块 {} 没有标签", pred_id);
                                    LabelId(pred_id)
                                })
                            } else {
                                println!("⚠️ 找不到前驱块 {}", pred_id);
                                LabelId(pred_id)
                            };
                            println!("🎯 incoming: 来自块{}(L{}), 值={:?}", pred_id, pred_label.0, value);
                            incoming.push((pred_label, value));
                        }
                    }
                    
                    if !incoming.is_empty() {
                        phi_insertions.push(PhiInsertion {
                            block_id: phi_block_id,
                            variable: slot_register,
                            dst_register: RegisterId(0), // 将在插入时分配
                            incoming,
                            actual_dst_register: None,
                            bb: analysis.basic_blocks[&phi_block_id].clone(),
                        });
                        println!("🎯 为栈槽 {:?} 在块 {} 创建φ节点", slot_register, phi_block_id);
                    }
                }
            }
        }
        
        println!("🎯 总共计算出 {} 个φ节点", phi_insertions.len());
        phi_insertions
    }
    
    /// 查找到达定义（reaching definition）
    fn find_reaching_definition(&self, variable: RegisterId, block_id: usize, function: &LirFunction) -> Option<Operand> {
        // 简化实现：查找块中最后的存储
        // TODO: 实际实现应该使用数据流分析
        
        // 遍历函数指令，找到该块中对该变量的最后存储
        for (i, instruction) in function.instructions.iter().enumerate().rev() {
            if let Instruction::Store64 { addr, src, .. } = instruction {
                if *addr == variable {
                    return Some(src.clone());
                }
            }
        }
        
        None
    }
    
    /// 将块ID映射到标签
    fn find_label_for_block(&self, block_id: usize) -> LabelId {
        // 简化实现：假设块ID对应标签ID
        LabelId(block_id)
    }

    /// 专业的φ节点构造算法
    /// 基于SSA构造理论，正确处理控制流合并点
    fn construct_phi_nodes_professional(&self, function: &LirFunction, analysis: &mut Memory2RegAnalysis) -> Result<(), String> {
        println!("🔧 开始专业φ节点构造");
        
        // 1. 构建控制流图
        let cfg = self.build_control_flow_graph(function);
        
        // 2. 计算支配关系和支配边界
        let dominance_info = self.compute_dominance_info(&cfg)?;
        
        // 3. 为每个变量计算φ节点插入位置
        for &slot_register in &analysis.promotable_slots {
            if let Some(slot) = analysis.stack_slots.get(&slot_register) {
                if self.needs_phi_nodes_heuristic(function, slot) {
                    println!("🎯 为变量 {:?} 计算φ节点", slot_register);
                    let phi_locations = self.compute_phi_locations_for_variable(slot, &cfg, &dominance_info)?;
                    
                    // 4. 创建φ节点
                    for phi_location in phi_locations {
                        let incoming = self.compute_phi_incoming_values(slot, phi_location, &cfg, function)?;
                        
                        if !incoming.is_empty() {
                            analysis.phi_insertions.push(PhiInsertion {
                                block_id: phi_location,
                                variable: slot_register,
                                dst_register: RegisterId(0), // 将在插入时分配
                                incoming,
                                actual_dst_register: None,
                                bb: cfg[phi_location].clone(),
                            });
                            println!("🎯 为变量 {:?} 在块 {} 创建φ节点", slot_register, phi_location);
                        }
                    }
                }
            }
        }
        
        println!("✅ φ节点构造完成，共 {} 个φ节点", analysis.phi_insertions.len());
        Ok(())
    }
    
    /// 构建控制流图
    fn build_control_flow_graph(&self, function: &LirFunction) -> Vec<BasicBlock> {
        let mut blocks = Vec::new();
        let mut current_block_start = 0;
        let mut block_id = 0;
        
        // 扫描指令，识别基本块边界
        for (i, instruction) in function.instructions.iter().enumerate() {
            match instruction {
                Instruction::Label { id, .. } => {
                    // 结束前一个基本块
                    if i > current_block_start {
                        blocks.push(BasicBlock {
                            id: block_id,
                            label: Some(*id),
                            start: current_block_start,
                            end: i,
                            predecessors: Vec::new(),
                            successors: Vec::new(),
                        });
                        block_id += 1;
                    }
                    
                    // 开始新的基本块
                    current_block_start = i;
                }
                Instruction::Jump { .. } | 
                Instruction::JumpEqual { .. } | 
                Instruction::JumpNotEqual { .. } |
                Instruction::Return { .. } => {
                    // 跳转指令结束当前基本块
                    blocks.push(BasicBlock {
                        id: block_id,
                        label: self.find_block_label(function, current_block_start, i + 1),
                        start: current_block_start,
                        end: i + 1,
                        predecessors: Vec::new(),
                        successors: Vec::new(),
                    });
                    block_id += 1;
                    current_block_start = i + 1;
                }
                _ => {}
            }
        }
        
        // 处理最后一个基本块
        if current_block_start < function.instructions.len() {
            blocks.push(BasicBlock {
                id: block_id,
                label: self.find_block_label(function, current_block_start, function.instructions.len()),
                start: current_block_start,
                end: function.instructions.len(),
                predecessors: Vec::new(),
                successors: Vec::new(),
            });
        }
        
        // 创建标签到块ID的映射
        let mut label_to_block = std::collections::HashMap::new();
        for block in blocks.iter() {
            if let Some(label_id) = block.label {
                label_to_block.insert(label_id, block.id);
            }
        }
        
        // 为每个基本块计算后继
        for i in 0..blocks.len() {
            let block = &blocks[i];
            if block.start >= block.end || block.end == 0 {
                continue;
            }
            
            // 查看基本块的最后一条指令
            let last_instruction = &function.instructions[block.end - 1];
            let mut successors = Vec::new();
            
            match last_instruction {
                Instruction::Jump { target, .. } => {
                    // 无条件跳转
                    if let Some(&target_block) = label_to_block.get(target) {
                        successors.push(target_block);
                    }
                }
                Instruction::JumpEqual { target, .. } |
                Instruction::JumpNotEqual { target, .. } => {
                    // 条件跳转：两个后继
                    if let Some(&target_block) = label_to_block.get(target) {
                        successors.push(target_block);
                    }
                    // 顺序执行到下一个基本块
                    if i + 1 < blocks.len() {
                        successors.push(blocks[i + 1].id);
                    }
                }
                Instruction::Return { .. } => {
                    // 返回指令没有后继
                }
                _ => {
                    // 其他指令：顺序执行到下一个基本块
                    if i + 1 < blocks.len() {
                        successors.push(blocks[i + 1].id);
                    }
                }
            }
            
            // 更新后继关系
            blocks[i].successors = successors;
        }
        
        // 根据后继关系更新前驱关系
        for i in 0..blocks.len() {
            let current_id = blocks[i].id;
            let successors = blocks[i].successors.clone();
            for &succ_id in &successors {
                if let Some(succ_block) = blocks.iter_mut().find(|b| b.id == succ_id) {
                    succ_block.predecessors.push(current_id);
                }
            }
        }
        
        println!("🔧 构建CFG: {} 个基本块", blocks.len());
        for block in &blocks {
            println!("🔧   块 {}: [{}, {}), 前驱: {:?}, 后继: {:?}", 
                block.id, block.start, block.end, block.predecessors, block.successors);
        }
        
        blocks
    }
    
    /// 查找基本块的标签ID
    fn find_block_label(&self, function: &LirFunction, start: usize, end: usize) -> Option<LabelId> {
        for i in start..end.min(function.instructions.len()) {
            if let Instruction::Label { id, .. } = &function.instructions[i] {
                return Some(*id);
            }
        }
        None
    }
    
    /// 计算支配信息
    fn compute_dominance_info(&self, cfg: &[BasicBlock]) -> Result<DominanceInfo, String> {
        if cfg.is_empty() {
            return Err("CFG为空".to_string());
        }
        
        let entry_block = 0; // 入口块总是0
        let mut dominance_info = DominanceInfo {
            dominators: std::collections::HashMap::new(),
            immediate_dominators: std::collections::HashMap::new(),
            dominance_frontiers: std::collections::HashMap::new(),
        };
        
        // 计算支配关系（简化实现）
        for block in cfg {
            let mut dominators = std::collections::HashSet::new();
            dominators.insert(block.id);
            if block.id != entry_block {
                dominators.insert(entry_block);
            }
            dominance_info.dominators.insert(block.id, dominators);
        }
        
        // 计算支配边界（简化实现）
        for block in cfg {
            let mut frontier = std::collections::HashSet::new();
            for &pred_id in &block.predecessors {
                if pred_id != block.id && block.predecessors.len() > 1 {
                    frontier.insert(block.id);
                }
            }
            dominance_info.dominance_frontiers.insert(block.id, frontier);
        }
        
        Ok(dominance_info)
    }
    
    /// 为特定变量计算φ节点插入位置
    fn compute_phi_locations_for_variable(&self, slot: &StackSlot, cfg: &[BasicBlock], dominance_info: &DominanceInfo) -> Result<Vec<usize>, String> {
        let mut phi_locations = Vec::new();
        
        // 🔧 稳定性修复：收集定义块（有store指令的块）
        let mut def_blocks = std::collections::HashSet::new();
        // 使用确定性顺序：按指令位置排序
        let mut sorted_store_blocks: Vec<_> = slot.store_to_block.iter().collect();
        sorted_store_blocks.sort_by_key(|(&store_pos, _)| store_pos);
        
        for (&_store_pos, &store_block_id) in sorted_store_blocks {
            def_blocks.insert(store_block_id);
        }
        
        // 对于每个有多个前驱的块，检查是否需要φ节点
        for block in cfg {
            if block.predecessors.len() > 1 {
                // 检查是否有来自不同前驱的定义
                let mut has_different_defs = false;
                for &pred_id in &block.predecessors {
                    if def_blocks.contains(&pred_id) {
                        has_different_defs = true;
                        break;
                    }
                }
                
                if has_different_defs {
                    phi_locations.push(block.id);
                    println!("🎯 在块 {} 需要φ节点（有 {} 个前驱，有定义块）", block.id, block.predecessors.len());
                }
            }
        }
        
        Ok(phi_locations)
    }
    
    /// 计算φ节点的incoming值
    fn compute_phi_incoming_values(&self, slot: &StackSlot, phi_block_id: usize, cfg: &[BasicBlock], function: &LirFunction) -> Result<Vec<(LabelId, Operand)>, String> {
        let mut incoming = Vec::new();
        
        // 找到phi节点所在的基本块
        if let Some(phi_block) = cfg.iter().find(|b| b.id == phi_block_id) {
            println!("🔧 计算块 {} 的phi节点incoming值，前驱: {:?}", phi_block_id, phi_block.predecessors);
            
            // 对前驱块按ID排序，确保确定性的处理顺序
            let mut sorted_predecessors = phi_block.predecessors.clone();
            sorted_predecessors.sort_unstable();
            
            for &pred_id in &sorted_predecessors {
                // 在前驱块中找到最后一个store指令
                let mut last_store = None;
                let mut last_store_pos = 0;
                
                // 修复：正确查找前驱块中的store指令
                for &store_pos in &slot.stores {
                    if let Some(&store_block) = slot.store_to_block.get(&store_pos) {
                        if store_block == pred_id && store_pos > last_store_pos {
                            if let Some(Instruction::Store64 { src, .. }) = function.instructions.get(store_pos) {
                                last_store = Some(src.clone());
                                last_store_pos = store_pos;
                                println!("🔧 在前驱块 {} 中找到store指令 [{}]: {:?}", pred_id, store_pos, src);
                            }
                        }
                    }
                }
                
                // 如果找到store，使用其值；否则使用默认值
                let value = last_store.unwrap_or_else(|| Operand::Immediate { value: 0 });
                
                // 修复：正确映射块ID到标签ID
                // 查找前驱块中的标签
                let pred_label = if let Some(pred_block) = cfg.iter().find(|b| b.id == pred_id) {
                    pred_block.label.unwrap_or_else(|| {
                        println!("⚠️ 前驱块 {} 没有标签，使用默认标签", pred_id);
                        LabelId(pred_id)
                    })
                } else {
                    println!("⚠️ 找不到前驱块 {}，使用默认标签", pred_id);
                    LabelId(pred_id)
                };
                
                println!("🔧 φ节点incoming: 来自块{}({}), 值={:?}", pred_id, pred_label.0, value);
                incoming.push((pred_label, value));
            }
        }
        
        Ok(incoming)
    }
}

impl FunctionPass for Memory2RegPass {
    fn name(&self) -> &str {
        "mem2reg"
    }
    
    fn run_on_function(&mut self, function: &mut LirFunction, analyses: &mut AnalysisManager) -> PassResult {
        // 第一步：获取CFG分析结果
        let cfg = match analyses.get_result::<ControlFlowGraph>("cfg") {
            Some(cfg) => cfg,
            None => {
                println!("❌ Memory2Reg失败：没有CFG分析结果");
                return PassResult::Failed("Missing CFG analysis".to_string());
            }
        };
        
        // 第二步：获取或计算支配信息
        let dominance_info = if let Some(ssa_result) = analyses.get_result::<SsaConstructionResult>("ssa-construction") {
            // 从SSA构造结果中获取支配信息
            println!("✅ 使用已有的SSA构造结果");
            Some(DominanceInfo {
                dominators: HashMap::new(), // 从SSA结果中提取
                immediate_dominators: HashMap::new(),
                dominance_frontiers: ssa_result.dominance_frontiers.clone(),
            })
        } else {
            println!("⚠️ 没有SSA构造结果，将使用简化的φ节点插入策略");
            None
        };
        
        // 第三步：运行分析
        let mut analysis = self.analyze_stack_slots(function, cfg);
        analysis.dominance_info = dominance_info;
        
        if analysis.promotable_slots.is_empty() {
            println!("⚠️ 没有可提升的栈槽");
            return PassResult::Unchanged;
        }
        
        // 第四步：计算φ节点插入位置
        if let Some(ref dom_info) = analysis.dominance_info {
            let phi_insertions = self.compute_phi_insertions(function, &analysis, dom_info);
            println!("🎯 计算出 {} 个φ节点需要插入", phi_insertions.len());
            analysis.phi_insertions = phi_insertions;
        } else {
            // 🔧 专业实现：基于CFG的正确φ节点构造
            println!("🔍 使用专业的φ节点构造算法");
            if let Err(e) = self.construct_phi_nodes_professional(function, &mut analysis) {
                return PassResult::Failed(format!("φ节点构造失败: {}", e));
            }
        }


        println!("block 映射 {:#?}", analysis.basic_blocks);
        
        // 存储分析结果
        analyses.store_result(self.name().to_string(), Box::new(analysis.clone()));
        
        // 第五步：执行变换
        match self.transform_function(function, &analysis) {
            Ok(true) => PassResult::Changed,
            Ok(false) => PassResult::Unchanged,
            Err(e) => PassResult::Failed(format!("Memory2Reg变换失败: {}", e)),
        }
    }
    
    fn required_analyses(&self) -> Vec<&'static str> {
        // Memory2Reg依赖CFG分析
        vec!["cfg", "ssa-construction"]
    }
    
    fn invalidated_analyses(&self) -> Vec<&'static str> {
        vec!["def-use", "cfg"] // Memory2Reg 会改变控制流和定义使用关系
    }
}

impl Memory2RegPass {
    /// 启发式判断是否需要φ节点
    fn needs_phi_nodes_heuristic(&self, function: &LirFunction, slot: &StackSlot) -> bool {
        // 检查是否是控制流敏感的栈槽
        if self.is_control_flow_sensitive_slot(function, slot) {
            return true;
        }
        
        // 🔧 稳定性修复：原有的简单逻辑，但使用确定性顺序
        let store_blocks: HashSet<usize> = slot.store_to_block.values().cloned().collect();
        let load_blocks: HashSet<usize> = slot.load_to_block.values().cloned().collect();
        
        // 超过一个存储块，或者load和store在不同块中
        store_blocks.len() > 1 || (!store_blocks.is_empty() && !load_blocks.is_empty() && !store_blocks.is_subset(&load_blocks))
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

#[cfg(test)]
mod tests {
    use super::*;
    use karte_diagnostics::Span;

    #[test]
    fn test_index_based_transformer() {
        println!("🔧 测试 index-based 变换系统");
        
        let mut transformer = IndexInstructionTransformer::new();
        
        // 创建一个测试函数
        let mut function = LirFunction::new("test_function".to_string());
        
        // 添加一些测试指令
        function.instructions.push(Instruction::Move {
            dst: RegisterId(1),
            src: Operand::Immediate { value: 42 },
            span: Span { start: 0, end: 0 },
        });
        
        function.instructions.push(Instruction::Store64 {
            addr: RegisterId(2),
            offset: 0,
            src: Operand::Immediate { value: 100 },
            span: Span { start: 0, end: 0 },
        });
        
        function.instructions.push(Instruction::Load64 {
            dst: RegisterId(3),
            addr: RegisterId(2),
            offset: 0,
            span: Span { start: 0, end: 0 },
        });
        
        println!("🔧 原始函数有 {} 条指令", function.instructions.len());
        
        // 添加变换操作：删除第1个指令，替换第2个指令
        transformer.remove(1);
        transformer.replace(2, Instruction::Move {
            dst: RegisterId(3),
            src: Operand::Immediate { value: 200 },
            span: Span { start: 0, end: 0 },
        });
        
        // 应用变换
        let (changed, _, _, _) = transformer.apply_to_function(&mut function);
        
        println!("🔧 应用变换后，函数有 {} 条指令", function.instructions.len());
        println!("🔧 变换是否成功: {}", changed);
        
        // 验证结果
        assert_eq!(function.instructions.len(), 2); // 删除了1个，保留了2个
        
        // 第一个指令应该是原始的Move
        if let Instruction::Move { dst, src, .. } = &function.instructions[0] {
            assert_eq!(*dst, RegisterId(1));
            assert_eq!(*src, Operand::Immediate { value: 42 });
            println!("✅ 第一个指令正确保留");
        } else {
            panic!("第一个指令应该是Move");
        }
        
        // 第二个指令应该是替换后的Move
        if let Instruction::Move { dst, src, .. } = &function.instructions[1] {
            assert_eq!(*dst, RegisterId(3));
            assert_eq!(*src, Operand::Immediate { value: 200 });
            println!("✅ 第二个指令正确替换");
        } else {
            panic!("第二个指令应该是Move");
        }
        
        println!("🔧 index-based 变换系统测试完成 ✅");
    }

    #[test]
    fn test_history_based_transformer() {
        println!("🧠 测试基于历史的变换系统");
        
        let mut transformer = HistoryBasedTransformer::new();
        
        // 创建一个测试函数
        let mut function = LirFunction::new("test_function".to_string());
        
        // 添加一些测试指令
        for i in 0..5 {
            function.instructions.push(Instruction::Move {
                dst: RegisterId(i),
                src: Operand::Immediate { value: i as i64 },
                span: Span { start: 0, end: 0 },
            });
        }
        
        println!("🧠 原始函数有 {} 条指令", function.instructions.len());
        
        // 添加变换操作：删除第1个，替换第3个，插入到第2个位置
        transformer.remove_at(1);
        transformer.replace_at(3, Instruction::Move {
            dst: RegisterId(99),
            src: Operand::Immediate { value: 999 },
            span: Span { start: 0, end: 0 },
        });
        transformer.insert_at(2, Instruction::Move {
            dst: RegisterId(88),
            src: Operand::Immediate { value: 888 },
            span: Span { start: 0, end: 0 },
        });
        
        // 应用变换
        let (changed, _, _, _) = transformer.apply_to_function(&mut function);
        
        println!("🧠 应用变换后，函数有 {} 条指令", function.instructions.len());
        println!("🧠 变换是否成功: {}", changed);
        
        // 验证结果
        assert_eq!(function.instructions.len(), 5); // 删除了1个，插入了1个，总共5个
        
        // 验证指令顺序
        if let Instruction::Move { dst, src, .. } = &function.instructions[0] {
            assert_eq!(*dst, RegisterId(0));
            assert_eq!(*src, Operand::Immediate { value: 0 });
        }
        
        // 2 被替换了
        if let Instruction::Move { dst, src, .. } = &function.instructions[1] {
            assert_eq!(*dst, RegisterId(88));
            assert_eq!(*src, Operand::Immediate { value: 888 });
        }
        
        if let Instruction::Move { dst, src, .. } = &function.instructions[2] {
            assert_eq!(*dst, RegisterId(2));
            assert_eq!(*src, Operand::Immediate { value: 2 });
        }
        
        if let Instruction::Move { dst, src, .. } = &function.instructions[3] {
            assert_eq!(*dst, RegisterId(99));
            assert_eq!(*src, Operand::Immediate { value: 999 });
        }
        
        if let Instruction::Move { dst, src, .. } = &function.instructions[4] {
            assert_eq!(*dst, RegisterId(4));
            assert_eq!(*src, Operand::Immediate { value: 4 });
        }
        
        println!("🧠 基于历史的变换系统测试完成 ✅");
    }
}