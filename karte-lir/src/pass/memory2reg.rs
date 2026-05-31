use crate::pass::analysis::ControlFlowGraph;
use crate::pass::instruction_transformer::{HistoryBasedTransformer, IndexInstructionTransformer};
use crate::pass::ssa_construction::{DominanceInfo, SsaConstructionResult};
use crate::pass::{AnalysisManager, AnalysisResult, FunctionPass, PassResult};
use crate::{AllocationType, Instruction, LabelId, LirFunction, Operand, Register};
// use karte_diagnostics::Span; // 下沉到 StackFrameLayoutPass 后，此处不直接构造基于 FP 的临时指令
use log::{debug, error, info, warn};
use std::any::Any;
use std::collections::{HashMap, HashSet};

/// 新的 index-based 指令变换系统
#[derive(Debug, Clone)]
pub struct StackSlot {
    /// 分配指令的位置
    pub alloc_instruction: usize,
    /// 分配的寄存器（保存栈地址）
    pub address_register: Register,
    /// 栈槽大小
    pub size: usize,
    /// 是否可以提升为寄存器
    pub promotable: bool,
    /// 加载指令位置列表
    pub loads: Vec<usize>,
    /// 存储指令位置列表  
    pub stores: Vec<usize>,
    /// 存储指令到基本块的映射
    pub store_to_block: HashMap<usize, usize>, // 指令位置 -> 块ID
    /// 加载指令到基本块的映射
    pub load_to_block: HashMap<usize, usize>, // 指令位置 -> 块ID
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
    pub stack_slots: HashMap<Register, StackSlot>,
    /// 可提升的栈槽
    pub promotable_slots: Vec<Register>,
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
    pub variable: Register,
    pub dst_register: Register,
    pub incoming: Vec<(LabelId, Operand)>, // (前驱块标签, 值)
    /// 实际分配的φ节点结果寄存器（在插入时设置）
    pub actual_dst_register: Option<Register>,
    pub bb: BasicBlock,
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

impl Default for Memory2RegPass {
    fn default() -> Self {
        Self::new()
    }
}

impl Memory2RegPass {
    pub fn new() -> Self {
        Self
    }

    /// 分析栈槽使用模式
    fn analyze_stack_slots(
        &self,
        function: &LirFunction,
        cfg: &ControlFlowGraph,
    ) -> Memory2RegAnalysis {
        let mut stack_slots = HashMap::new();
        let mut promotable_slots = Vec::new();

        // 第一阶段：从CFG获取基本块信息
        let basic_blocks = self.convert_cfg_to_basic_blocks(cfg);
        info!("🔍 分析到 {} 个基本块", basic_blocks.len());

        // 第二阶段：识别栈分配
        for (i, instruction) in function.instructions.iter().enumerate() {
            if let Instruction::Alloc {
                dst,
                size,
                allocation_type: AllocationType::Stack,
                ..
            } = instruction
            {
                // 结构体分配的alloc通常后面紧跟结构体字段store，且size等于结构体大小（如16字节），我们排除掉
                let is_struct_alloc = if *size >= 16 {
                    // 向后看2条指令，若均为store到该dst+偏移，视为结构体分配
                    let mut struct_field_store_count = 0;
                    for j in 1..=2 {
                        if let Some(Instruction::Store64 { addr, .. }) =
                            function.instructions.get(i + j)
                        {
                            if addr == dst {
                                struct_field_store_count += 1;
                            }
                        }
                    }
                    struct_field_store_count >= 2
                } else {
                    false
                };
                if is_struct_alloc {
                    continue;
                }
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
                info!("🔍 发现栈分配: 寄存器 {:?}, 大小 {}", dst, size);
                stack_slots.insert(*dst, slot);
            }
        }

        // 第三阶段：分析每个栈槽的使用并记录所在基本块
        for (i, instruction) in function.instructions.iter().enumerate() {
            let current_block = self.find_basic_block_for_instruction(i, cfg);

            match instruction {
                Instruction::Load64 {
                    dst: _,
                    addr,
                    offset,
                    ..
                } => {
                    if *offset == 0 {
                        if let Some(slot) = stack_slots.get_mut(addr) {
                            slot.loads.push(i);
                            if let Some(block_id) = current_block {
                                slot.load_to_block.insert(i, block_id);
                                debug!("🔍 记录load: 寄存器 {:?} 在块 {}", addr, block_id);
                            }
                        }
                    } else {
                        // 有偏移的访问，标记为不可提升
                        if let Some(slot) = stack_slots.get_mut(addr) {
                            slot.promotable = false;
                        }
                    }
                }
                Instruction::StructFieldLoad {
                    dst: _,
                    struct_addr,
                    ..
                } => {
                    // StructFieldLoad 从结构体地址加载字段，不应该直接提升栈槽
                    // 但如果结构体地址本身是栈槽，我们需要记录这个使用
                    if let Some(slot) = stack_slots.get_mut(struct_addr) {
                        slot.loads.push(i);
                        if let Some(block_id) = current_block {
                            slot.load_to_block.insert(i, block_id);
                            info!(
                                "🔍 记录StructFieldLoad: 结构体地址寄存器 {:?} 在块 {}",
                                struct_addr, block_id
                            );
                        }
                    }
                }
                Instruction::Store64 {
                    addr,
                    offset,
                    src: _,
                    span,
                } => {
                    if *offset == 0 {
                        if let Some(slot) = stack_slots.get_mut(addr) {
                            // Phi Store64 使用 span={MAX,MAX} 标记
                            // 这些 Store64 由 phi_store_map 生成，不能被 Memory2Reg 提升
                            if span.start == usize::MAX && span.end == usize::MAX {
                                slot.promotable = false;
                            }
                            slot.stores.push(i);
                            if let Some(block_id) = current_block {
                                slot.store_to_block.insert(i, block_id);
                                debug!("🔍 记录store: 寄存器 {:?} 在块 {}", addr, block_id);
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
                            warn!("🚨 栈地址被传递: {:?} 标记为不可提升", id);
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
        for instruction in function.instructions.iter() {
            if let Instruction::Store64 {
                addr, offset, src, ..
            } = instruction
            {
                if *offset == 0 {
                    // 🚨 关键修复：检查是否存储的是另一个栈地址（引用操作）
                    if let Operand::Register { id: src_reg } = src {
                        if stack_slots.contains_key(src_reg) && stack_slots.contains_key(addr) {
                            info!(
                                "🚨 检测到引用操作: 栈槽 {:?} 存储了另一个栈地址 {:?}",
                                addr, src_reg
                            );
                            // 存储栈地址的槽不可提升（它需要真实的内存地址）
                            if let Some(slot) = stack_slots.get_mut(addr) {
                                slot.promotable = false;
                            }
                            // 被引用的栈槽也不可提升（它的地址被取用）
                            if let Some(referenced_slot) = stack_slots.get_mut(src_reg) {
                                referenced_slot.promotable = false;
                                warn!("🚨 被引用的栈槽 {:?} 也标记为不可提升", src_reg);
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
                if let Instruction::Store64 {
                    addr, offset, src, ..
                } = instruction
                {
                    if *offset == 0 {
                        if let Operand::Register { id: src_reg } = src {
                            // 检查源寄存器是否是从不可提升的栈槽加载的
                            if let Some(load_from_stack) =
                                self.trace_register_to_stack_load(*src_reg, i, function)
                            {
                                if stack_slots.contains_key(&load_from_stack)
                                    && stack_slots.contains_key(addr)
                                {
                                    // 检查被加载的栈槽是否不可提升
                                    if let Some(source_slot) = stack_slots.get(&load_from_stack) {
                                        if !source_slot.promotable {
                                            // 如果源栈槽不可提升，那么存储其值的栈槽也不可提升
                                            if let Some(target_slot) = stack_slots.get_mut(addr) {
                                                if target_slot.promotable {
                                                    warn!("🚨 检测到间接引用: 栈槽 {:?} 存储了从不可提升栈槽 {:?} 加载的值", addr, load_from_stack);
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
        let phi_insertions = Vec::new();

        // 收集可提升的栈槽
        // 🔧 修复：按确定性顺序遍历 stack_slots，避免 HashMap 遍历顺序不确定
        // 导致的虚拟寄存器 ID 分配不确定性
        let mut addr_regs: Vec<Register> = stack_slots.keys().cloned().collect();
        addr_regs.sort_by_key(|r| r.id());
        for addr_reg in &addr_regs {
            if let Some(slot) = stack_slots.get(addr_reg) {
                if slot.promotable && !slot.stores.is_empty() {
                    promotable_slots.push(*addr_reg);
                }
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
    fn find_basic_block_for_instruction(
        &self,
        instruction_index: usize,
        cfg: &ControlFlowGraph,
    ) -> Option<usize> {
        for node in &cfg.nodes {
            if instruction_index >= node.instruction_range.0
                && instruction_index < node.instruction_range.1
            {
                return Some(node.block_id);
            }
        }
        None
    }

    /// 执行 Memory2Reg 变换
    fn transform_function(
        &self,
        function: &mut LirFunction,
        analysis: &Memory2RegAnalysis,
    ) -> crate::Result<bool> {
        info!("🚀 开始Memory2Reg变换");

        let mut transformer = IndexInstructionTransformer::new();

        // 🔥 新增：第一步 - 溢出代码插入
        self.insert_spill_code(function, analysis, &mut transformer)?;

        // 第二步 - 使用专业的φ节点构造算法
        let mut phi_insertions = Vec::new();
        if let Some(dominance_info) = &analysis.dominance_info {
            phi_insertions = self.compute_phi_insertions(function, analysis, dominance_info);
        } else {
            warn!("⚠️ 没有SSA构造结果，将使用简化的φ节点插入策略");
        }

        // 第三步 - 插入φ节点
        let phi_inserted = self.insert_phi_nodes(function, &mut phi_insertions, &mut transformer);
        if phi_inserted {
            info!("✅ φ节点插入完成");
        }

        // 第四步 - 收集变换操作
        info!("🚀 步骤2: 收集变换操作");
        // let mut all_instructions_to_remove = Vec::new();
        // let mut all_instructions_to_modify = Vec::new();

        // 🔧 修复：按确定性顺序遍历 stack_slots
        let mut sorted_slots: Vec<_> = analysis.stack_slots.iter().collect();
        sorted_slots.sort_by_key(|(slot_id, _)| slot_id.id());

        for (slot_id, slot) in &sorted_slots {
            if slot.promotable {
                if self.has_phi_support_for_slot(slot, &phi_insertions) {
                    debug!("🎯 使用φ节点支持变换栈槽 {:?}", slot_id);
                    self.transform_with_phi_support(
                        function,
                        slot,
                        &phi_insertions,
                        &analysis.basic_blocks,
                        &analysis.dominance_info,
                        &mut transformer,
                    );
                } else {
                    info!("🚀 处理栈槽 {:?}", slot_id);
                    self.collect_simple_transform_operations(
                        function,
                        slot,
                        &analysis.basic_blocks,
                        &mut transformer,
                    );
                    // panic!("🚀 处理栈槽 {:?}", slot_id);
                }
            }
        }

        // info!("🚀 总共移除 {} 条指令, 修改 {} 条指令", all_instructions_to_remove.len(), all_instructions_to_modify.len());

        // 第五步 - 应用变换
        info!("🚀 步骤3: 应用变换");
        let (changed, ..) = transformer.apply_to_function(function);
        // let changed = self.apply_all_transforms(function, all_instructions_to_remove, all_instructions_to_modify);

        if changed {
            info!("✅ Memory2Reg变换完成，有修改");
        } else {
            info!("ℹ️ Memory2Reg变换完成，无修改");
        }

        // 栈地址到具体帧偏移的下沉改由统一的 StackFrameLayoutPass 完成
        Ok(changed)
    }

    /// 🔥 新增：溢出代码插入
    ///
    /// 在寄存器分配前插入所有溢出相关的load/store指令和临时虚拟寄存器
    /// 确保所有虚拟寄存器都在RA前生成，RA后不再引入新寄存器
    fn insert_spill_code(
        &self,
        function: &mut LirFunction,
        analysis: &Memory2RegAnalysis,
        transformer: &mut IndexInstructionTransformer,
    ) -> crate::Result<()> {
        // 关闭预插入溢出代码：统一由寄存器分配与后置栈帧布局处理
        // 之前的实现会在store之前插入无意义的load，破坏初始化顺序，导致错误。
        let _ = (function, analysis, transformer); // silence unused warnings
        Ok(())
    }

    /// 插入φ节点
    fn insert_phi_nodes(
        &self,
        function: &mut LirFunction,
        phi_insertions: &mut [PhiInsertion],
        transformer: &mut IndexInstructionTransformer,
    ) -> bool {
        if phi_insertions.is_empty() {
            warn!("⚠️ 没有φ节点需要插入");
            return false;
        }

        info!("🔧 开始插入 {} 个φ节点", phi_insertions.len());

        // 按块ID对phi节点进行分组
        let mut phi_by_block: HashMap<usize, Vec<&mut PhiInsertion>> = HashMap::new();
        for phi in phi_insertions.iter_mut() {
            phi_by_block.entry(phi.block_id).or_default().push(phi);
        }

        // 为每个块插入phi节点
        let mut phi_instructions = Vec::new();

        // 🔧 修复：按块ID排序遍历 phi_by_block，确保 new_register() 调用顺序确定
        let mut sorted_block_ids: Vec<usize> = phi_by_block.keys().copied().collect();
        sorted_block_ids.sort();

        for block_id in sorted_block_ids {
            let phis = phi_by_block.remove(&block_id).unwrap();
            // 找到块的开始位置
            let insert_pos = phis
                .iter()
                .map(|phi| phi.bb.start)
                .min()
                .unwrap_or_default()
                + 1;
            info!(
                "🔧 在块 {} 开始位置 {} 插入 {} 个φ节点",
                block_id,
                insert_pos,
                phis.len()
            );

            for phi in phis {
                // 分配新寄存器作为phi节点的目标
                if phi.actual_dst_register.is_none() {
                    let phi_dst = function.new_register();
                    phi.actual_dst_register = Some(phi_dst);
                }

                let phi_dst = phi.actual_dst_register.unwrap();

                info!(
                    "🔧   - φ节点: 目标寄存器 {:?}, 原变量 {:?}",
                    phi_dst, phi.variable
                );

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

        info!("✅ φ节点插入完成");
        true
    }

    /// 检查是否有φ节点支持此栈槽
    fn has_phi_support_for_slot(&self, slot: &StackSlot, phi_insertions: &[PhiInsertion]) -> bool {
        phi_insertions
            .iter()
            .any(|phi| phi.variable == slot.address_register)
    }

    /// 使用φ节点支持进行变换
    fn transform_with_phi_support(
        &self,
        function: &LirFunction,
        slot: &StackSlot,
        phi_insertions: &[PhiInsertion],
        basic_blocks: &HashMap<usize, BasicBlock>,
        dominance_info: &Option<DominanceInfo>,
        transformer: &mut IndexInstructionTransformer,
    ) {
        debug!("🎯 使用φ节点支持变换栈槽 {:?}", slot.address_register);

        // 1. 记录alloc指令索引（延迟移除，等load全部替换成功后再移除）
        let mut alloc_to_remove: Option<usize> = None;
        if slot.alloc_instruction < function.instructions.len() {
            if let Instruction::Alloc {
                dst,
                size,
                allocation_type,
                ..
            } = &function.instructions[slot.alloc_instruction]
            {
                if *dst == slot.address_register {
                    info!(
                        "🎯 记录alloc指令待移除: {:?} (size: {}, type: {:?})",
                        dst, size, allocation_type
                    );
                    alloc_to_remove = Some(slot.alloc_instruction);
                }
            }
        }

        // 2. 处理store指令，同时记录每个块中的最后一个store
        debug!("🎯 扫描所有store指令以匹配栈槽 {:?}", slot.address_register);
        let mut found_stores = 0;
        let mut stores_to_remove: Vec<usize> = Vec::new();
        let mut block_last_store: HashMap<usize, (usize, Operand)> = HashMap::new();

        for (i, instruction) in function.instructions.iter().enumerate() {
            if let Instruction::Store64 {
                addr, offset, src, ..
            } = instruction
            {
                if *addr == slot.address_register && *offset == 0 {
                    debug!("🎯   - 发现store指令 [{}]: [{:?}] = {:?}", i, addr, src);

                    // 找到store指令所在的块
                    if let Some(&block_id) = slot.store_to_block.get(&i) {
                        block_last_store.insert(block_id, (i, src.clone()));
                    }

                    stores_to_remove.push(i);
                    found_stores += 1;
                }
            }
        }
        info!(
            "🎯 为栈槽 {:?} 找到 {} 个store指令进行移除",
            slot.address_register, found_stores
        );

        // 3. 处理load指令
        debug!("🎯 扫描所有load指令以匹配栈槽 {:?}", slot.address_register);
        let mut found_loads = 0;
        let mut total_loads = 0;
        for (i, instruction) in function.instructions.iter().enumerate() {
            if let Instruction::Load64 {
                dst,
                addr,
                offset,
                span,
            } = instruction
            {
                if *addr == slot.address_register {
                    if *offset != 0 {
                        info!(
                            "🔧   - 跳过结构体字段访问: load指令 [{}] 有偏移量 {}",
                            i, offset
                        );
                        continue;
                    }
                    total_loads += 1;
                    debug!("🎯   - 发现load指令 [{}]: {}", i, instruction);

                    // 优先检查同块内最近的 store——如果 load 和 store 在同一基本块
                    // 且 store 在 load 之前，直接使用 store 的 src（无论 Register 还是 Immediate）。
                    // 这避免了通过 Phi 传递值时首次迭代取到错误初始值的问题。
                    // 根因：find_phi_result_for_instruction_position 只处理 Register 源操作数，
                    // 对 Immediate 源操作数会跳过同块 store，转而使用循环头 Phi 的结果，
                    // 但 Phi 首次迭代的 incoming 来自循环前（未定义/零值），导致结果错误。
                    let mut same_block_store_src: Option<Operand> = None;
                    let mut same_block_nearest_dist = usize::MAX;
                    if let Some(&current_block_id) = slot.load_to_block.get(&i) {
                        for &store_pos in &slot.stores {
                            if store_pos < i && i - store_pos < same_block_nearest_dist {
                                if let Some(store_block) = slot.store_to_block.get(&store_pos) {
                                    if *store_block == current_block_id {
                                        if let Instruction::Store64 { src, .. } =
                                            &function.instructions[store_pos]
                                        {
                                            same_block_store_src = Some(src.clone());
                                            same_block_nearest_dist = i - store_pos;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if let Some(src) = same_block_store_src {
                        info!(
                            "M2R 替换load为move(direct): load64 dst: {:?}, addr: {:?} -> mov dst: {:?}, src: {:?} (同块store直接传播) at instr {}",
                            dst, addr, dst, src, i
                        );
                        let new_move = Instruction::Move {
                            dst: *dst,
                            src,
                            span: *span,
                        };
                        transformer.replace(i, new_move);
                        found_loads += 1;
                        continue;
                    }

                    // 查找对应的phi结果寄存器
                    let phi_result_reg = self.find_phi_result_for_instruction_position(
                        i,
                        slot,
                        phi_insertions,
                        function,
                        basic_blocks,
                        dominance_info,
                    );

                    if phi_result_reg.id() != 998 {
                        info!(
                            "M2R 替换load为move: load64 dst: {:?}, addr: {:?} -> mov dst: {:?}, src: {:?} (phi_result) at instr {}",
                            dst, addr, dst, phi_result_reg, i
                        );
                        let new_move = Instruction::Move {
                            dst: *dst,
                            src: Operand::Register { id: phi_result_reg },
                            span: *span,
                        };
                        transformer.replace(i, new_move);
                        found_loads += 1;
                    } else {
                        // 尝试使用块中以及前序block的最后一个 store 的 src
                        // 重要：只在线性路径上搜索（每个块只有一个前驱时才继续）
                        // 不跨越分支汇合点，否则会从错误的分支获取 Store 值
                        // 🔧 同时检查回溯路径上的phi节点：对于跨多块使用的变量，
                        // phi节点可能在回溯路径的某个块中（而非直接前驱）
                        let mut last_store = None;
                        if let Some(&current_block_id) = slot.load_to_block.get(&i) {
                            let mut current_bb = current_block_id;
                            loop {
                                // 🔧 先检查当前块是否有phi节点（在回溯路径上）
                                let mut found_phi = false;
                                for phi in phi_insertions {
                                    if phi.variable == slot.address_register
                                        && phi.block_id == current_bb
                                    {
                                        if let Some(actual_dst) = phi.actual_dst_register {
                                            info!(
                                                "M2R 替换load为move(fallback+phi): load64 dst: {:?}, addr: {:?} -> mov dst: {:?}, src: {:?} (phi in block {}) at instr {}",
                                                dst, addr, dst, actual_dst, current_bb, i
                                            );
                                            let new_move = Instruction::Move {
                                                dst: *dst,
                                                src: Operand::Register { id: actual_dst },
                                                span: *span,
                                            };
                                            transformer.replace(i, new_move);
                                            found_loads += 1;
                                            found_phi = true;
                                        }
                                        break;
                                    }
                                }
                                if found_phi {
                                    break;
                                }
                                // 检查当前块是否有 store
                                if let Some((_, src)) = block_last_store.get(&current_bb) {
                                    last_store = Some(src.clone());
                                    break;
                                }
                                // 只在当前块只有一个前驱时继续向上搜索
                                if let Some(block) = basic_blocks.get(&current_bb) {
                                    if block.predecessors.len() == 1 {
                                        current_bb = block.predecessors[0];
                                    } else {
                                        // 多个前驱（分支汇合点），停止搜索
                                        break;
                                    }
                                } else {
                                    break;
                                }
                            }
                        }
                        if let Some(src) = last_store {
                            info!(
                                "M2R 替换load为move(fallback): load64 dst: {:?}, addr: {:?} -> mov dst: {:?}, src: {:?} at instr {}",
                                dst, addr, dst, src, i
                            );
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
                        //         info!("🎯   - 使用块 {} 中最后一个store的值", block_id);
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
                        debug!("栈槽 {:?} 的phi节点无法解析，保留原始load指令（phi可能在远处的merge块中）", addr);
                    }
                }
            }
        }
        info!(
            "🎯 为栈槽 {:?} 替换 {}/{} 个load指令",
            slot.address_register, found_loads, total_loads
        );

        // 4. 只有当所有load都成功替换时，才实际移除alloc和store
        if found_loads == total_loads {
            if let Some(alloc_idx) = alloc_to_remove {
                transformer.remove(alloc_idx);
            }
            for store_idx in stores_to_remove {
                transformer.remove(store_idx);
            }
            info!(
                "✅ 栈槽 {:?} 全部load替换成功，移除alloc和{}个store",
                slot.address_register, found_stores
            );
        } else if total_loads > 0 {
            warn!(
                "⚠️ 栈槽 {:?} 只有 {}/{} 个load被替换，保留alloc和store（安全降级）",
                slot.address_register, found_loads, total_loads
            );
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
        dominance_info: &Option<DominanceInfo>,
    ) -> Register {
        // 1. 找到load指令所在的基本块
        // 🔧 修复：使用预计算的 load_to_block 映射，而不是遍历所有块检查范围
        let current_block_id = slot.load_to_block.get(&load_pos).copied();

        let Some(block_id) = current_block_id else {
            error!("❌ 无法找到load指令所在的基本块，load_pos: {}", load_pos);
            // 调试信息：打印所有基本块的范围
            for block in basic_blocks.values() {
                info!(
                    "🔍 块 {}: [{}, {})",
                    block.label.unwrap_or(LabelId(0)),
                    block.start,
                    block.end
                );
            }

            // 修复：如果找不到基本块，尝试使用最近的store指令的值
            let mut nearest_store = None;
            let mut nearest_distance = usize::MAX;

            for &store_pos in &slot.stores {
                if store_pos < load_pos && load_pos - store_pos < nearest_distance {
                    if let Some(Instruction::Store64 { src, .. }) =
                        function.instructions.get(store_pos)
                    {
                        if let Operand::Register { id } = src {
                            nearest_store = Some(*id);
                            nearest_distance = load_pos - store_pos;
                            debug!("🔍 使用最近的store指令值 {:?}", id);
                        }
                    }
                }
            }

            if let Some(store_reg) = nearest_store {
                return store_reg;
            }

            return Register::Virtual(998);
        };

        debug!("🔍 load指令位于基本块 {}", block_id);

        // 2. 在当前块中查找最近的phi节点
        for phi in phi_insertions.iter().rev() {
            if phi.variable == slot.address_register && phi.block_id == block_id {
                if let Some(actual_dst) = phi.actual_dst_register {
                info!(
                        "✅ 在当前块 {} 找到phi节点，使用结果寄存器 {:?}",
                        block_id, actual_dst
                    );
                    return actual_dst;
                }
            }
        }

        // 3. 不在支配链上向上查找 phi 节点
        // 
        // 注意：之前的实现在支配链上查找 Phi 并使用其结果寄存器，
        // 但这会导致错误：Phi 的结果是"在 Phi 所在块开头选择的值"，
        // 而 load 需要的是"在 load 所在位置最近的定义"。
        // 如果 Phi 和 load 之间有其他定义（Store/Move），Phi 的结果可能已过时。
        // 
        // 只有在 load 和 Phi 在同一个块中时，才能安全地使用 Phi 结果（这在步骤1已处理）。
        // 对于跨块的 Phi 查找，由于存在跨分支错误关联的风险，这里不执行。

        // 4. 如果找不到phi节点，尝试在当前块中找到最近的store指令的值
        //    注意：只使用当前块中的 Store，不跨块查找。
        //    跨块使用 Store 值可能导致错误——不同分支中的 Store 值可能不同，
        //    load 应该通过 Phi 选择正确的值，而不是直接使用某个分支的 Store。
        let mut nearest_store = None;
        let mut nearest_distance = usize::MAX;

        for &store_pos in &slot.stores {
            if store_pos < load_pos && load_pos - store_pos < nearest_distance {
                if let Some(store_block) = slot.store_to_block.get(&store_pos) {
                    // 只使用当前块中的 Store
                    if *store_block == block_id {
                        nearest_store = Some(store_pos);
                        nearest_distance = load_pos - store_pos;
                        debug!("🔍 找到当前块 {} 中的store指令", store_block);
                    }
                }
            }
        }

        if let Some(store_pos) = nearest_store {
            if let Some(Instruction::Store64 { src, .. }) = function.instructions.get(store_pos) {
                if let Operand::Register { id } = src {
                    info!("✅ 使用最近的store指令值 {:?}", id);
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
                            info!("✅ 使用前驱块 {} 的phi节点结果 {:?}", pred_id, actual_dst);
                            return actual_dst;
                        }
                    }
                }
            }
        }

        // 6. 如果前驱也没有phi，沿着支配树向上查找
        // 这对于跨越多个块的load尤为重要：例如变量在entry定义，
        // 但load在while循环内部的if-then块中，phi节点在循环头
        if let Some(dom_info) = dominance_info {
            let mut current = block_id;
            loop {
                if let Some(&idom) = dom_info.immediate_dominators.get(&current) {
                    if idom == current {
                        break; // reached root
                    }
                    for phi in phi_insertions {
                        if phi.variable == slot.address_register && phi.block_id == idom {
                            if let Some(actual_dst) = phi.actual_dst_register {
                                info!(
                                    "✅ 沿支配树在块 {} 找到phi节点结果 {:?}",
                                    idom, actual_dst
                                );
                                return actual_dst;
                            }
                        }
                    }
                    current = idom;
                } else {
                    break;
                }
            }
        }

        warn!("❌ 无法找到合适的phi节点或store值");
        Register::Virtual(998)
    }

    /// 简单变换操作（原有逻辑）
    fn collect_simple_transform_operations(
        &self,
        function: &LirFunction,
        slot: &StackSlot,
        basic_blocks: &HashMap<usize, BasicBlock>,
        transformer: &mut IndexInstructionTransformer,
    ) {
        // 🔧 重大改进：处理更多情况，包括寄存器存储和跨栈槽值传播

        info!(
            "🚀 分析栈槽 {:?}: stores={}, loads={}",
            slot.address_register,
            slot.stores.len(),
            slot.loads.len()
        );

        // // 🔧 新增：引用-解引用模式检测 FIXME: 引入类型和别名分析
        // if self.is_reference_dereference_pattern(function, slot) {
        //     info!("🎯 检测到引用-解引用模式，应用特殊优化");
        //     self.optimize_reference_dereference_pattern(function, slot, instructions_to_remove, instructions_to_modify);
        //     return;
        // }

        // 🔧 关键修复：检查是否是控制流敏感的栈槽
        if self.is_control_flow_sensitive_slot(function, slot, basic_blocks) {
            info!(
                "⚠️ 栈槽 {:?} 是控制流敏感的，需要φ节点支持",
                slot.address_register
            );
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
                info!(
                    "⚠️ 栈槽 {:?} 的load操作分布在 {} 个不同基本块中，需要特殊处理",
                    slot.address_register,
                    load_blocks.len()
                );

                // 对于跨基本块的load操作，使用安全的内容匹配变换
                if slot.stores.len() == 1 {
                    let store_pos = slot.stores[0];
                    if store_pos < function.instructions.len() {
                        if let Instruction::Store64 { src, .. } = &function.instructions[store_pos]
                        {
                            info!(
                                "🔧 使用安全变换系统处理跨基本块的栈槽 {:?}",
                                slot.address_register
                            );
                            info!("🔧 强制传播值 {:?} 到所有跨基本块的load操作", src);

                            // 第二步：直接应用变换到真实函数
                            // 移除 Alloc 和 Store64
                            transformer.remove(slot.alloc_instruction);
                            transformer.remove(store_pos);
                            // 将所有 Load64 替换为 Move
                            for &load_pos in &slot.loads {
                                if load_pos < function.instructions.len() {
                                    if let Instruction::Load64 { dst, .. } = &function.instructions[load_pos] {
                                        transformer.replace(
                                            load_pos,
                                            Instruction::Move {
                                                dst: *dst,
                                                src: src.clone(),
                                                span: function.instructions[load_pos].get_span(),
                                            },
                                        );
                                    }
                                }
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
                debug!("🚀 单一存储分析: src={:?}", src);

                // 🔧 新增：跨栈槽值传播 - 如果src是寄存器，尝试追踪其来源
                let effective_src = match src {
                    Operand::Register { id } => {
                        // 尝试找到这个寄存器的定义
                        self.trace_register_value(*id, store_pos, function)
                            .unwrap_or_else(|| src.clone())
                    }
                    _ => src.clone(),
                };

                debug!("🚀 有效源操作数: {:?}", effective_src);

                // 对于单一存储，我们可以直接传播值
                match &effective_src {
                    Operand::Immediate { .. } | Operand::Register { .. } => {
                        // 立即数或寄存器：都可以优化

                        // 将所有load指令替换为直接使用源操作数
                        for &load_pos in &slot.loads {
                            if load_pos < function.instructions.len() {
                                if let Instruction::Load64 { dst, .. } =
                                    &function.instructions[load_pos]
                                {
                                    info!(
                                        "🚀 替换load {} -> mov {:?}, {:?}",
                                        load_pos, dst, effective_src
                                    );
                                    // 替换load为mov
                                    transformer.replace(
                                        load_pos,
                                        Instruction::Move {
                                            dst: *dst,
                                            src: effective_src.clone(),
                                            span: function.instructions[load_pos].get_span(),
                                        },
                                    );
                                }
                            }
                        }

                        // 移除store指令和alloc指令
                        transformer.remove(store_pos);
                        transformer.remove(slot.alloc_instruction);
                    }
                    _ => {
                        // 其他复杂操作数，暂不优化
                        debug!("🚀 复杂操作数，暂不优化: {:?}", effective_src);
                    }
                }
            }
        }
        // 情况2: 多个存储 - 需要φ函数处理，但我们先实现简单版本
        else if slot.stores.len() > 1 && !slot.loads.is_empty() {
            // 🔧 新增：处理多重赋值的简单情况
            // 如果所有stores都是在不同的基本块中，我们可以考虑优化

            // 简化实现：如果最后一个store支配所有的load，我们可以优化
            if let Some(&last_store_pos) = slot.stores.last() {
                if last_store_pos < function.instructions.len() {
                    if let Instruction::Store64 { src, .. } = &function.instructions[last_store_pos]
                    {
                        // 检查这个store是否在所有load之前
                        let all_loads_after_store =
                            slot.loads.iter().all(|&load_pos| load_pos > last_store_pos);

                        if all_loads_after_store {
                            // 可以优化：用最后的store值替换所有后续的load
                            match src {
                                Operand::Immediate { .. } | Operand::Register { .. } => {
                                    // 替换所有load
                                    for &load_pos in &slot.loads {
                                        if load_pos < function.instructions.len() {
                                            if let Instruction::Load64 { dst, .. } =
                                                &function.instructions[load_pos]
                                            {
                                                transformer.replace(
                                                    load_pos,
                                                    Instruction::Move {
                                                        dst: *dst,
                                                        src: src.clone(),
                                                        span: function.instructions[load_pos]
                                                            .get_span(),
                                                    },
                                                );
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
    fn is_control_flow_sensitive_slot(
        &self,
        function: &LirFunction,
        slot: &StackSlot,
        basic_blocks: &HashMap<usize, BasicBlock>,
    ) -> bool {
        // 🔧 关键修复：如果栈槽有多个存储，并且它们在不同的基本块中，则是控制流敏感的
        if slot.stores.len() > 1 {
            let store_blocks: HashSet<usize> = slot.store_to_block.values().cloned().collect();
            if store_blocks.len() > 1 {
                info!(
                    "⚠️ 栈槽 {:?} 有 {} 个存储在 {} 个不同的基本块中",
                    slot.address_register,
                    slot.stores.len(),
                    store_blocks.len()
                );
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
            if let Instruction::Store64 {
                src: Operand::Register { id },
                ..
            } = &function.instructions[store_pos]
            {
                // 查找这个寄存器的所有定义
                let definitions = self.find_all_definitions(*id, store_pos, function);

                // 如果有多个定义，并且它们在不同的基本块中，则是控制流敏感的
                if definitions.len() > 1 {
                    debug!("🔍 寄存器 {:?} 有 {} 个定义", id, definitions.len());
                    // 🔧 修复：使用 basic_blocks 查找定义所在的块，而不是扫描 Label 指令
                    let mut definition_blocks = HashSet::new();
                    for &def_pos in &definitions {
                        // 使用 basic_blocks 的 instruction_range 查找定义所在的块
                        for (block_id, block) in basic_blocks {
                            if def_pos >= block.start && def_pos < block.end {
                                definition_blocks.insert(*block_id);
                                break;
                            }
                        }
                    }

                    // 如果定义在多个基本块中，则是控制流敏感的
                    if definition_blocks.len() > 1 {
                        info!(
                            "⚠️ 寄存器 {:?} 在 {} 个不同的基本块中被定义",
                            id,
                            definition_blocks.len()
                        );
                        return true;
                    }
                }
            }
        }

        false
    }

    /// 查找寄存器的所有定义（扫描整个函数，不仅是 before_pos 之前）
    /// 🔧 修复：BlockLayoutPass 重排后，定义可能在指令序列中位于使用之后
    /// 因此需要扫描整个函数来找到所有定义
    fn find_all_definitions(
        &self,
        register: Register,
        _before_pos: usize,
        function: &LirFunction,
    ) -> Vec<usize> {
        let mut definitions = Vec::new();

        // 扫描整个函数，找到所有对该寄存器的定义
        for (i, instruction) in function.instructions.iter().enumerate() {
            match instruction {
                Instruction::Move { dst, .. } if *dst == register => {
                    definitions.push(i);
                }
                Instruction::Load64 { dst, .. } if *dst == register => {
                    definitions.push(i);
                }
                Instruction::LoadGlobal { dst, .. } if *dst == register => {
                    definitions.push(i);
                }
                Instruction::Add { dst, .. }
                | Instruction::Sub { dst, .. }
                | Instruction::Mul { dst, .. }
                | Instruction::Div { dst, .. }
                | Instruction::Mod { dst, .. }
                | Instruction::BitAnd { dst, .. }
                | Instruction::BitOr { dst, .. }
                | Instruction::BitXor { dst, .. }
                | Instruction::ShiftLeft { dst, .. }
                | Instruction::ShiftRight { dst, .. }
                | Instruction::BitNot { dst, .. }
                | Instruction::IntCast { dst, .. }
                    if *dst == register =>
                {
                    definitions.push(i);
                }
                _ => {}
            }
        }

        definitions
    }

    /// 追踪寄存器值的来源，用于跨栈槽值传播
    fn trace_register_value(
        &self,
        register: Register,
        before_pos: usize,
        function: &LirFunction,
    ) -> Option<Operand> {
        info!(
            "🔍 追踪寄存器 {:?} 在位置 {} 之前的值",
            register, before_pos
        );

        // 向前扫描，找到最近的对该寄存器的定义
        // 🔧 修复：如果同一寄存器在多个分支中有不同的定义（如 div 零除保护），
        // 则不能安全传播单个分支的值，必须返回 None
        let mut first_def_pos: Option<usize> = None;
        let mut first_def_value: Option<Operand> = None;
        for i in (0..before_pos).rev() {
            if i >= function.instructions.len() {
                continue;
            }

            let instruction = &function.instructions[i];
            match instruction {
                Instruction::Move { dst, src, .. } if *dst == register => {
                    if first_def_pos.is_some() {
                        // 找到第二个定义，说明值依赖控制流分支，不可安全传播
                        debug!("🔍 寄存器 {:?} 有多个定义点（位置 {:?} 和 {}），跳过传播（控制流依赖）", register, first_def_pos, i);
                        return None;
                    }
                    debug!("🔍 找到mov定义: {:?} = {:?}", dst, src);
                    first_def_pos = Some(i);
                    first_def_value = Some(src.clone());
                }
                Instruction::Load64 { dst, addr, .. } if *dst == register => {
                    if first_def_pos.is_some() {
                        // 找到第二个定义（load），说明值依赖控制流分支
                        debug!("🔍 寄存器 {:?} 有多个定义点（位置 {:?} 和 {}），跳过传播（控制流依赖）", register, first_def_pos, i);
                        return None;
                    }
                    debug!("🔍 找到load定义: {:?} = [{}]", dst, addr);
                    // 🔧 修复：仅当栈槽有多个store时阻止穿透追踪
                    // 多个store意味着值依赖控制流（如match不同分支），线性追踪会选错分支的值
                    if self.is_stack_address_register(*addr, function) {
                        if self.has_multiple_stores_to_slot(*addr, function) {
                            debug!("🔍 栈槽 {:?} 有多个store，跳过穿透追踪（控制流依赖）", addr);
                            return None;
                        } else {
                            return self.trace_stack_slot_value(*addr, i, function);
                        }
                    } else {
                        return None;
                    }
                }
                Instruction::LoadGlobal { dst, .. } if *dst == register => {
                    debug!("🔍 找到LoadGlobal定义: {:?}", dst);
                    // LoadGlobal 加载全局变量，无法简单追踪
                    return None;
                }
                Instruction::StructFieldLoad {
                    dst, struct_addr, ..
                } if *dst == register => {
                    info!(
                        "🔍 找到StructFieldLoad定义: {:?} = field from {:?}",
                        dst, struct_addr
                    );
                    // StructFieldLoad 从结构体加载字段，无法简单追踪
                    return None;
                }
                Instruction::Add {
                    dst, src1, src2, ..
                }
                | Instruction::Sub {
                    dst, src1, src2, ..
                }
                | Instruction::Mul {
                    dst, src1, src2, ..
                }
                | Instruction::Div {
                    dst, src1, src2, ..
                }
                | Instruction::Mod {
                    dst, src1, src2, ..
                }
                | Instruction::BitAnd {
                    dst, src1, src2, ..
                }
                | Instruction::BitOr {
                    dst, src1, src2, ..
                }
                | Instruction::BitXor {
                    dst, src1, src2, ..
                }
                | Instruction::ShiftLeft {
                    dst, src1, src2, ..
                }
                | Instruction::ShiftRight {
                    dst, src1, src2, ..
                } if *dst == register => {
                    if first_def_pos.is_some() {
                        // 找到第二个定义（算术），说明值依赖控制流分支
                        debug!("🔍 寄存器 {:?} 有多个定义点（位置 {:?} 和 {}），跳过传播（控制流依赖）", register, first_def_pos, i);
                        return None;
                    }
                    debug!("🔍 找到算术定义: {:?} = {:?} op {:?}", dst, src1, src2);
                    // 算术运算的结果无法简单追踪
                    first_def_pos = Some(i);
                    // 算术结果不是常量，记录位置但不传播值
                    return None;
                }
                Instruction::BitNot { dst, src, .. } if *dst == register => {
                    debug!("🔍 找到位非定义: {:?} = bitnot {:?}", dst, src);
                    return None;
                }
                Instruction::IntCast { dst, src, .. } if *dst == register => {
                    debug!("🔍 找到类型转换定义: {:?} = intcast {:?}", dst, src);
                    return None;
                }
                _ => {
                    // 其他指令，继续向前查找
                }
            }
        }

        // 返回第一个定义的值（如果有）
        if let Some(value) = first_def_value {
            debug!("🔍 寄存器 {:?} 唯一定义值: {:?}", register, value);
            Some(value)
        } else {
            debug!("🔍 未找到寄存器 {:?} 的定义", register);
            None
        }
    }

    /// 追踪栈槽的存储值
    fn trace_stack_slot_value(
        &self,
        stack_addr: Register,
        before_pos: usize,
        function: &LirFunction,
    ) -> Option<Operand> {
        info!(
            "🔍 追踪栈槽 {:?} 在位置 {} 之前的存储值",
            stack_addr, before_pos
        );

        // 向前扫描，找到最近的对该栈槽的存储
        for i in (0..before_pos).rev() {
            if i >= function.instructions.len() {
                continue;
            }

            let instruction = &function.instructions[i];
            match instruction {
                Instruction::Store64 { addr, src, .. } if *addr == stack_addr => {
                    debug!("🔍 找到栈槽存储: [{}] = {:?}", addr, src);
                    // 如果存储的是寄存器，可以进一步追踪
                    match src {
                        Operand::Register { id } => {
                            // 递归追踪寄存器值（但限制递归深度）
                            if let Some(traced_value) = self.trace_register_value(*id, i, function)
                            {
                                debug!("🔍 递归追踪得到: {:?}", traced_value);
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

        debug!("🔍 未找到栈槽 {:?} 的存储值", stack_addr);
        None
    }

    /// 检查寄存器是否是栈地址寄存器
    fn is_stack_address_register(&self, register: Register, function: &LirFunction) -> bool {
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

    /// 检查栈槽是否有多个store（控制流依赖）
    /// 多个store意味着值可能来自不同分支，不能安全地线性追踪
    fn has_multiple_stores_to_slot(&self, slot_addr: Register, function: &LirFunction) -> bool {
        let mut store_count = 0;
        for instruction in &function.instructions {
            if let Instruction::Store64 { addr, offset, .. } = instruction {
                if *addr == slot_addr && *offset == 0 {
                    store_count += 1;
                    if store_count > 1 {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// 追踪寄存器是否是从栈槽加载的，返回栈槽的地址寄存器
    fn trace_register_to_stack_load(
        &self,
        register: Register,
        before_pos: usize,
        function: &LirFunction,
    ) -> Option<Register> {
        // 向前搜索寄存器的定义
        for i in (0..before_pos).rev() {
            match &function.instructions[i] {
                Instruction::Load64 {
                    dst, addr, offset, ..
                } if *dst == register && *offset == 0 => {
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
    fn compute_phi_insertions(
        &self,
        function: &mut LirFunction,
        analysis: &Memory2RegAnalysis,
        _dominance_info: &DominanceInfo,
    ) -> Vec<PhiInsertion> {
        let mut phi_insertions = Vec::new();
        info!("🔧 开始重构的phi节点插入算法");

        for &slot_register in &analysis.promotable_slots {
            if let Some(slot) = analysis.stack_slots.get(&slot_register) {
                debug!("🎯 为栈槽 {:?} 计算phi节点插入位置", slot_register);
                // 新增：详细打印store/load分布
                debug!("  - stores: {:?}", slot.stores);
                debug!("  - loads:  {:?}", slot.loads);
                for (i, &store_pos) in slot.stores.iter().enumerate() {
                    if let Some(&block_id) = slot.store_to_block.get(&store_pos) {
                        debug!("    store[{}] @{} in block {}", i, store_pos, block_id);
                    }
                }
                for (i, &load_pos) in slot.loads.iter().enumerate() {
                    if let Some(&block_id) = slot.load_to_block.get(&load_pos) {
                        debug!("    load[{}] @{} in block {}", i, load_pos, block_id);
                    }
                }
                // 新增：打印所有基本块信息
                for (block_id, block) in &analysis.basic_blocks {
                    info!(
                        "    block {}: label={:?}, range=[{}, {}), preds={:?}, succs={:?}",
                        block_id,
                        block.label,
                        block.start,
                        block.end,
                        block.predecessors,
                        block.successors
                    );
                }
                let mut phi_blocks = HashSet::new();

                // 🔧 修复：使用正确的phi节点插入策略
                // 1. 首先找到所有有多个前驱的基本块（合流点）
                let mut confluence_blocks = HashSet::new();
                for (block_id, block) in &analysis.basic_blocks {
                    if block.predecessors.len() > 1 {
                        confluence_blocks.insert(*block_id);
                        info!(
                            "🎯 发现合流点: 块{} 有{}个前驱",
                            block_id,
                            block.predecessors.len()
                        );
                    }
                }

                // 递归判断该块或其后继（不含自身）是否有load指令
                // 🔧 修复：使用预计算的 load_to_block 映射，而不是检查指令范围
                fn block_or_successors_have_load(
                    block_id: usize,
                    basic_blocks: &HashMap<usize, BasicBlock>,
                    slot: &StackSlot,
                    visited: &mut HashSet<usize>,
                ) -> bool {
                    if !visited.insert(block_id) {
                        return false;
                    }
                    // 使用 load_to_block 映射检查该块是否有 load
                    let has_load = slot
                        .load_to_block
                        .values()
                        .any(|&load_block| load_block == block_id);
                    if has_load {
                        return true;
                    }
                    if let Some(block) = basic_blocks.get(&block_id) {
                        for &succ in &block.successors {
                            if block_or_successors_have_load(succ, basic_blocks, slot, visited) {
                                return true;
                            }
                        }
                    }
                    false
                }

                // 2. 迭代式计算需要插入 phi 节点的合流点
                // 修复：前驱块可能有直接 Store64，也可能有 Phi 节点（间接 store）
                // 必须迭代直到收敛，因为前驱的 Phi 可能由更早的迭代产生
                let mut changed = true;
                while changed {
                    changed = false;
                    for &block_id in &confluence_blocks {
                        if phi_blocks.contains(&block_id) {
                            continue;
                        }

                        let mut visited = HashSet::new();
                        let has_relevant_use = block_or_successors_have_load(
                            block_id,
                            &analysis.basic_blocks,
                            slot,
                            &mut visited,
                        );

                        // 检查前驱块是否有直接 Store64 或已有 Phi 节点
                        let mut predecessors_with_stores = HashSet::new();
                        if let Some(block) = analysis.basic_blocks.get(&block_id) {
                            for &pred_id in &block.predecessors {
                                let has_direct_store = slot
                                    .store_to_block
                                    .values()
                                    .any(|&store_block| store_block == pred_id);
                                let has_phi = phi_blocks.contains(&pred_id);
                                if has_direct_store || has_phi {
                                    predecessors_with_stores.insert(pred_id);
                                }
                            }
                        }

                        let needs_phi = has_relevant_use
                            && analysis.basic_blocks.get(&block_id).map_or(false, |b| b.predecessors.len() > 1)
                            && predecessors_with_stores.len() >= 1;

                        info!(
                            "🎯 分析块{}: 有load={}, 前驱有store数={}, 需要phi={}",
                            block_id,
                            has_relevant_use,
                            predecessors_with_stores.len(),
                            needs_phi
                        );

                        if needs_phi {
                            phi_blocks.insert(block_id);
                            changed = true;
                            debug!("🎯 块{} 需要phi节点: 有相关使用且前驱有store或phi", block_id);
                        }
                    }
                }

                // 🔧 修复：先创建所有phi节点，分配结果寄存器
                let mut phi_nodes_with_registers = Vec::new();
                let mut sorted_phi_blocks: Vec<_> = phi_blocks.iter().cloned().collect();
                sorted_phi_blocks.sort_unstable();

                for phi_block_id in sorted_phi_blocks {
                    let phi_result_register = function.new_register();

                    phi_nodes_with_registers.push((phi_block_id, phi_result_register));
                    info!(
                        "🎯 为块 {} 的phi节点分配结果寄存器 {:?}",
                        phi_block_id, phi_result_register
                    );
                }

                // 🔧 修复：创建phi节点到结果寄存器的映射
                let phi_block_to_register: HashMap<usize, Register> = phi_nodes_with_registers
                    .iter()
                    .map(|(block_id, reg)| (*block_id, *reg))
                    .collect();

                // 现在计算每个phi节点的incoming值
                for (phi_block_id, phi_result_register) in phi_nodes_with_registers {
                    let mut incoming = Vec::new();
                    if let Some(phi_block) = analysis.basic_blocks.get(&phi_block_id) {
                        let mut sorted_predecessors = phi_block.predecessors.clone();
                        sorted_predecessors.sort_unstable();
                        for &pred_id in &sorted_predecessors {
                            // 🔧 修复：正确计算incoming值，使用真实的结果寄存器
                            let value = self.compute_incoming_value_for_phi_with_registers(
                                pred_id,
                                phi_block_id,
                                slot,
                                &phi_blocks,
                                &phi_block_to_register,
                                function,
                                &analysis.basic_blocks,
                            );
                            let pred_label = self.find_label_for_predecessor_block(
                                pred_id,
                                &analysis.basic_blocks,
                                function,
                            );
                            info!(
                                "🎯 incoming: 来自块{}(标签{:?}), 值={:?}",
                                pred_id, pred_label, value
                            );
                            incoming.push((pred_label, value));
                        }
                    }
                    if !incoming.is_empty() {
                        phi_insertions.push(PhiInsertion {
                            block_id: phi_block_id,
                            variable: slot_register,
                            dst_register: phi_result_register, // 🔧 使用分配的真实寄存器
                            incoming,
                            actual_dst_register: Some(phi_result_register), // 🔧 设置实际结果寄存器
                            bb: analysis.basic_blocks[&phi_block_id].clone(),
                        });
                        info!(
                            "🎯 为栈槽 {:?} 在块 {} 创建phi节点，结果寄存器: {:?}",
                            slot_register, phi_block_id, phi_result_register
                        );
                    }
                }
            }
        }
        info!("🎯 总共计算出 {} 个phi节点", phi_insertions.len());
        phi_insertions
    }

    /// 🔧 新增：正确计算phi节点的incoming值，使用真实的结果寄存器
    fn compute_incoming_value_for_phi_with_registers(
        &self,
        pred_block_id: usize,
        phi_block_id: usize,
        slot: &StackSlot,
        phi_blocks: &HashSet<usize>,
        phi_block_to_register: &HashMap<usize, Register>,
        function: &LirFunction,
        basic_blocks: &HashMap<usize, BasicBlock>,
    ) -> Operand {
        // 🔧 修复：优先检查前驱块是否有store指令
        if let Some(last_store) =
            self.find_last_store_in_block(pred_block_id, slot, function, basic_blocks)
        {
            info!(
                "🎯 前驱块 {} 有store指令，直接使用store值: {:?}",
                pred_block_id, last_store
            );
            return last_store;
        }

        // 如果前驱块本身有phi节点，使用phi节点的真实结果寄存器
        if phi_blocks.contains(&pred_block_id) {
            if let Some(&phi_result_register) = phi_block_to_register.get(&pred_block_id) {
                info!(
                    "🎯 前驱块 {} 有phi节点，使用phi结果寄存器 {:?}",
                    pred_block_id, phi_result_register
                );
                return Operand::Register {
                    id: phi_result_register,
                };
            }
        }

        // 🔧 修复：使用递归的"到达定义"查找算法
        // 从前驱块开始，递归查找该分支上最近一次store指令
        if let Some(value) = self.find_reaching_definition_on_path(
            pred_block_id,
            phi_block_id,
            slot,
            function,
            basic_blocks,
            phi_blocks,
            phi_block_to_register,
        ) {
            info!("🎯 前驱块 {} 的到达定义: {:?}", pred_block_id, value);
            return value;
        }

        // 如果找不到任何store指令，使用默认值0
        info!("🎯 前驱块 {} 没有找到store指令，使用默认值0", pred_block_id);
        Operand::Immediate { value: 0 }
    }

    /// 🔧 新增：递归查找到达定义
    /// 从指定块开始，沿着控制流逆向查找最近一次store指令
    /// 
    /// 重要：只在"线性"路径上搜索——每个块只有一个前驱时才继续向上查找。
    /// 不跨越分支汇合点（有多个前驱的块），否则会从错误的分支获取 Store 值。
    fn find_reaching_definition_on_path(
        &self,
        start_block_id: usize,
        target_block_id: usize,
        slot: &StackSlot,
        function: &LirFunction,
        basic_blocks: &HashMap<usize, BasicBlock>,
        phi_blocks: &HashSet<usize>,
        phi_block_to_register: &HashMap<usize, Register>,
    ) -> Option<Operand> {
        info!(
            "🔍 在路径上查找到达定义: 从块{}到块{}",
            start_block_id, target_block_id
        );

        // 1. 首先检查当前块是否有store指令
        if let Some(last_store) =
            self.find_last_store_in_block(start_block_id, slot, function, basic_blocks)
        {
            info!("🔍 在块{}找到store指令: {:?}", start_block_id, last_store);
            return Some(last_store);
        }

        // 2. 如果当前块有phi节点，使用phi节点的结果
        if phi_blocks.contains(&start_block_id) {
            if let Some(&phi_result_register) = phi_block_to_register.get(&start_block_id) {
                info!(
                    "🔍 块{}有phi节点，使用结果寄存器{:?}",
                    start_block_id, phi_result_register
                );
                return Some(Operand::Register {
                    id: phi_result_register,
                });
            }
        }

        // 3. 如果当前块只有一个前驱（线性路径），沿前驱继续向上查找
        //    如果有多个前驱（分支汇合点），停止搜索——避免从错误分支获取值
        if let Some(block) = basic_blocks.get(&start_block_id) {
            if block.predecessors.len() == 1 {
                let pred_id = block.predecessors[0];
                if pred_id == target_block_id {
                    // 🔧 回边到 phi 块：使用 phi 结果寄存器创建自引用 phi
                    // 例如 while 循环中 block4(loop body) -> block2(loop header)
                    // 闭包地址在循环中不变，phi 的 incoming 应为 phi 自身的结果
                    if phi_blocks.contains(&target_block_id) {
                        if let Some(&phi_reg) = phi_block_to_register.get(&target_block_id) {
                            info!(
                                "🔍 回边到 phi 块{}，使用 phi 结果寄存器{:?}",
                                target_block_id, phi_reg
                            );
                            return Some(Operand::Register { id: phi_reg });
                        }
                    }
                    // 不是 phi 块的回边，停止搜索
                } else if pred_id != start_block_id {
                    return self.find_reaching_definition_on_path(
                        pred_id,
                        target_block_id,
                        slot,
                        function,
                        basic_blocks,
                        phi_blocks,
                        phi_block_to_register,
                    );
                }
            }
        }

        info!("🔍 在块{}没有找到到达定义（停止搜索）", start_block_id);
        None
    }

    /// 🔧 修复：在指定块中查找最后一次store指令
    /// 使用预计算的 store_to_block 映射，而不是重新扫描指令范围
    /// 这样可以确保在 block 重排后仍然正确工作
    fn find_last_store_in_block(
        &self,
        block_id: usize,
        slot: &StackSlot,
        function: &LirFunction,
        _basic_blocks: &HashMap<usize, BasicBlock>,
    ) -> Option<Operand> {
        info!(
            "🔍 在块{}中查找store指令，栈槽{:?}，使用store_to_block映射",
            block_id, slot.address_register
        );

        // 使用预计算的 store_to_block 映射查找该块中的 stores
        // 这个映射在 analyze_stack_slots() 中构建，与 CFG 保持一致
        let mut last_store_idx: Option<usize> = None;

        for (&store_idx, &store_block) in &slot.store_to_block {
            if store_block == block_id {
                info!("🔍   找到store指令在块{}: 指令索引{}", block_id, store_idx);
                match last_store_idx {
                    None => last_store_idx = Some(store_idx),
                    Some(last) if store_idx > last => last_store_idx = Some(store_idx),
                    _ => {}
                }
            }
        }

        if let Some(idx) = last_store_idx {
            if let Some(Instruction::Store64 { src, .. }) = function.instructions.get(idx) {
                info!(
                    "🔍   命中Store64: 块{} 指令[{}] src={:?}",
                    block_id, idx, src
                );
                return Some(src.clone());
            }
        }

        info!("🔍 在块{}中没有找到任何store指令", block_id);
        None
    }

    /// 🔧 查找前驱块对应的标签
    /// 使用 BasicBlock.label 字段，该字段来自 CFG 分析，不依赖指令范围
    fn find_label_for_predecessor_block(
        &self,
        pred_block_id: usize,
        basic_blocks: &HashMap<usize, BasicBlock>,
        _function: &LirFunction,
    ) -> LabelId {
        // 使用预计算的 label 字段
        if let Some(pred_block) = basic_blocks.get(&pred_block_id) {
            if let Some(label) = pred_block.label {
                info!("🔧 前驱块 {} 有标签: {:?}", pred_block_id, label);
                return label;
            }
        }

        // 如果找不到，使用 block_id 作为虚拟标签（这种情况不应该发生）
        warn!(
            "⚠️ 警告：找不到前驱块 {} 的标签，使用虚拟标签",
            pred_block_id
        );
        LabelId(pred_block_id)
    }
}

impl FunctionPass for Memory2RegPass {
    fn name(&self) -> &str {
        "mem2reg"
    }

    fn description(&self) -> &str {
        "Memory2Reg优化 - 将内存操作提升为寄存器操作"
    }

    fn run_on_function(
        &mut self,
        function: &mut LirFunction,
        analyses: &mut AnalysisManager,
    ) -> PassResult {
        // 第一步：获取CFG分析结果
        let cfg = match analyses.get_result::<ControlFlowGraph>("cfg") {
            Some(cfg) => cfg,
            None => {
                info!("❌ Memory2Reg失败：没有CFG分析结果");
                return PassResult::Failed("Missing CFG analysis".to_string());
            }
        };

        // 第二步：获取或计算支配信息
        let dominance_info = if let Some(ssa_result) =
            analyses.get_result::<SsaConstructionResult>("ssa-construction")
        {
            // 从SSA构造结果中获取支配信息
            info!("✅ 使用已有的SSA构造结果");
            Some(DominanceInfo {
                dominators: HashMap::new(), // 从SSA结果中提取
                immediate_dominators: HashMap::new(),
                dominance_frontiers: ssa_result.dominance_frontiers.clone(),
            })
        } else {
            warn!("⚠️ 没有SSA构造结果，将使用简化的φ节点插入策略");
            None
        };

        // 第三步：运行分析
        let mut analysis = self.analyze_stack_slots(function, cfg);
        analysis.dominance_info = dominance_info;

        if analysis.promotable_slots.is_empty() {
            info!("⚠️ 没有可提升的栈槽");
            return PassResult::Unchanged;
        }

        // 第四步：计算φ节点插入位置
        if let Some(ref dom_info) = analysis.dominance_info {
            let phi_insertions = self.compute_phi_insertions(function, &analysis, dom_info);
            info!("🎯 计算出 {} 个φ节点需要插入", phi_insertions.len());
            analysis.phi_insertions = phi_insertions;
        } else {
            // 🔧 专业实现：基于CFG的正确φ节点构造
            info!("🔍 使用专业的φ节点构造算法");
            // 使用现有的phi节点计算逻辑
            let phi_insertions =
                self.compute_phi_insertions(function, &analysis, &DominanceInfo::default());
            analysis.phi_insertions = phi_insertions;
        }

        // info!("block 映射 {:#?}", analysis.basic_blocks);

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

#[cfg(test)]
mod tests;
