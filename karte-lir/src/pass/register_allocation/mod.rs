pub mod lifetime_analysis;
pub mod linear_scan;
pub mod types;

// 重新导出主要类型，方便外部使用
pub use lifetime_analysis::LifetimeAnalyzer;
pub use linear_scan::LinearScanAllocator;
pub use types::*;

use super::{AnalysisManager, FunctionPass, PassResult};
use crate::{IndexInstructionTransformer, Instruction, LirFunction, Operand, Register};
use karte_diagnostics::Span;
use log::{debug, info, trace};
use std::collections::{HashMap, HashSet};

/// 🔧 遵循调用约定的简单栈式寄存器分配Pass
///
/// 调用约定：
/// - r0: 返回值
/// - r1-r4: 参数传递（最多4个参数）
/// - r5: 返回地址
/// - r6: 栈指针（SP）
/// - r7: 帧指针（FP）
pub struct SimpleStackRegisterAllocation {
    /// 调用约定
    calling_convention: CallingConvention,
    /// 当前溢出的寄存器计数
    spill_counter: usize,
    /// 溢出槽id -> 槽地址寄存器（通过 Alloc(Stack) 产生）
    spill_slot_addr_map: HashMap<usize, Register>,
    /// 临时物理寄存器 -> scratch 槽地址寄存器（用于保存/恢复临时寄存器）
    scratch_slot_addr_map: HashMap<usize, Register>,
    /// 本次分配中识别出的栈地址寄存器集合（用于重写阶段区分地址与数据）
    stack_address_registers: std::collections::HashSet<Register>,
}

/// 分配目标
#[derive(Debug, Clone, Copy)]
enum AllocationTarget {
    /// 分配到物理寄存器
    Register(u8),
    /// 溢出到栈槽
    Spill(usize),
}

impl Default for SimpleStackRegisterAllocation {
    fn default() -> Self {
        Self::new()
    }
}

impl SimpleStackRegisterAllocation {
    pub fn new() -> Self {
        Self {
            calling_convention: CallingConvention::standard(),
            spill_counter: 0,
            spill_slot_addr_map: HashMap::new(),
            scratch_slot_addr_map: HashMap::new(),
            stack_address_registers: std::collections::HashSet::new(),
        }
    }
}

impl FunctionPass for SimpleStackRegisterAllocation {
    fn name(&self) -> &str {
        "simple-stack-register-allocation"
    }

    fn run_on_function(
        &mut self,
        function: &mut LirFunction,
        _analyses: &mut AnalysisManager,
    ) -> PassResult {
        info!("🎯 开始遵循调用约定的寄存器分配：{}", function.name);

        // 重置状态
        self.spill_counter = 0;
        self.spill_slot_addr_map.clear();
        self.scratch_slot_addr_map.clear();
        self.stack_address_registers.clear();

        // 分析所有虚拟寄存器的使用情况
        let virtual_registers = self.collect_virtual_registers(function);
        info!("🎯 发现虚拟寄存器: {:?}", virtual_registers);

        // 🔧 新增：根据调用约定构建寄存器分配映射
        let allocation_map =
            self.build_calling_convention_allocation_map(function, &virtual_registers);
        info!("🎯 调用约定寄存器分配映射: {:?}", allocation_map);

        // 应用分配并处理溢出
        self.apply_allocation_with_spilling(function, &allocation_map)
    }

    fn required_analyses(&self) -> Vec<&'static str> {
        vec![]
    }

    fn invalidated_analyses(&self) -> Vec<&'static str> {
        vec!["cfg", "def-use"]
    }
}

impl SimpleStackRegisterAllocation {
    /// 收集函数中所有使用的虚拟寄存器
    fn collect_virtual_registers(&self, function: &LirFunction) -> Vec<Register> {
        let mut registers = HashSet::new();

        trace!("🔍 开始收集虚拟寄存器...");
        for (i, instruction) in function.instructions.iter().enumerate() {
            // 收集定义的寄存器
            if let Some(def_reg) = instruction.get_def_register() {
                trace!("  指令{}: 定义寄存器 {:?} - {:?}", i, def_reg, instruction);
                registers.insert(def_reg);
            }

            // 收集使用的寄存器
            for used_reg in instruction.get_used_registers() {
                trace!("  指令{}: 使用寄存器 {:?} - {:?}", i, used_reg, instruction);
                registers.insert(used_reg);
            }
        }

        trace!("🔍 收集到的所有寄存器: {:?}", registers);

        // 🔧 关键修复：确保确定性的寄存器顺序
        let mut sorted_registers: Vec<Register> = registers
            .into_iter()
            .filter(|reg| {
                let is_special = self.calling_convention.is_special_register(*reg);
                if is_special {
                    trace!("  过滤特殊寄存器: {:?}", reg);
                }
                !is_special
            })
            .collect();

        // 按寄存器ID排序，确保每次运行结果一致
        sorted_registers.sort_by_key(|reg| reg.id());

        trace!("🎯 发现虚拟寄存器: {:?}", sorted_registers);
        sorted_registers
    }

    /// 🔧 重构：基于生命周期分析的寄存器分配映射
    fn build_calling_convention_allocation_map(
        &mut self,
        function: &LirFunction,
        virtual_registers: &[Register],
    ) -> HashMap<Register, AllocationTarget> {
        info!("🎯 开始遵循调用约定的寄存器分配：{}", function.name);

        // 🔧 关键修复：首先进行生命周期分析
        let lifetime_analyzer =
            lifetime_analysis::LifetimeAnalyzer::new(types::CallingConvention::standard());
        let (lifetimes, register_types) = lifetime_analyzer.analyze_simple(function);

        // 记录所有栈地址寄存器，供后续重写阶段使用
        self.stack_address_registers = register_types
            .iter()
            .filter_map(|(reg, ty)| {
                if *ty == RegisterType::StackAddress {
                    Some(*reg)
                } else {
                    None
                }
            })
            .collect();

        // 构建生命周期映射，用于冲突检测
        let mut lifetime_map = HashMap::new();
        for lifetime in &lifetimes {
            lifetime_map.insert(lifetime.register, (lifetime.start, lifetime.end));
        }

        let mut allocation_map = HashMap::new();
        let mut used_physical_regs = HashSet::new();

        // 第一步：为函数参数寄存器分配固定的物理寄存器
        info!("🔧 第一步：分配函数参数寄存器");
        for (i, &param_reg) in function.parameter_registers.iter().enumerate() {
            if i < self.calling_convention.argument_registers.len() {
                let physical_reg = self.calling_convention.argument_registers[i];
                allocation_map.insert(param_reg, AllocationTarget::Register(physical_reg));
                used_physical_regs.insert(physical_reg);
                info!("  参数寄存器 {:?} -> r{}", param_reg, physical_reg);
            } else {
                // 参数过多，需要溢出到栈
                let spill_slot = allocation_map
                    .values()
                    .filter_map(|target| {
                        if let AllocationTarget::Spill(slot) = target {
                            Some(*slot)
                        } else {
                            None
                        }
                    })
                    .max()
                    .unwrap_or(0)
                    + 1;
                allocation_map.insert(param_reg, AllocationTarget::Spill(spill_slot));
                info!("  参数寄存器 {:?} -> 溢出槽{}", param_reg, spill_slot);
            }
        }

        // 第二步：为返回值寄存器分配r0（如果函数有返回值）
        info!("🔧 第二步：分配返回值寄存器");
        if let Some(return_reg) = self.find_return_register(function) {
            allocation_map.insert(
                return_reg,
                AllocationTarget::Register(self.calling_convention.return_register),
            );
            used_physical_regs.insert(self.calling_convention.return_register);
            info!(
                "  返回值寄存器 {:?} -> r{}",
                return_reg, self.calling_convention.return_register
            );
        }

        // 第三步：根据我们的cc给函数参数和返回值插入映射
        info!("🔧 第三步：分配函数调用寄存器");
        for inst in &function.instructions {
            match inst {
                Instruction::Call { args, result, .. }
                | Instruction::CallIndirect { args, result, .. } => {
                    for (i, arg) in args.iter().enumerate() {
                        allocation_map.insert(
                            *arg,
                            AllocationTarget::Register(
                                self.calling_convention.argument_registers[i],
                            ),
                        );
                        used_physical_regs.insert(self.calling_convention.argument_registers[i]);
                    }
                    // 返回值映射 cc的返回值
                    if let Some(reg) = result {
                        allocation_map.insert(
                            *reg,
                            AllocationTarget::Register(self.calling_convention.return_register),
                        );
                        used_physical_regs.insert(self.calling_convention.return_register);
                    }
                    if let Instruction::CallIndirect {
                        function_register, ..
                    } = inst
                    {
                        allocation_map.insert(*function_register, AllocationTarget::Register(5));
                        used_physical_regs.insert(5);
                    }
                }
                _ => {}
            }
        }

        // 第四步：基于生命周期分析为其他虚拟寄存器分配物理寄存器
        info!("🔧 第四步：分配其他虚拟寄存器");

        // 🔧 关键修复：按生命周期开始时间排序，确保确定性分配
        let mut remaining_registers: Vec<Register> = virtual_registers
            .iter()
            .filter(|&reg| !allocation_map.contains_key(reg))
            .copied()
            .collect();

        remaining_registers.sort_by(|&a, &b| {
            let start_a = lifetime_map.get(&a).map(|(start, _)| *start).unwrap_or(0);
            let start_b = lifetime_map.get(&b).map(|(start, _)| *start).unwrap_or(0);
            start_a.cmp(&start_b).then_with(|| a.id().cmp(&b.id())) // 生命周期相同时按ID排序
        });

        // 🔧 调试：显示排序后的寄存器顺序
        info!("🔧 按生命周期排序后的寄存器分配顺序:");
        for &reg in &remaining_registers {
            let (start, end) = lifetime_map.get(&reg).copied().unwrap_or((0, 0));
            info!("  {:?}: 生命周期[{}, {}]", reg, start, end);
        }

        for virtual_reg in remaining_registers {
            // 🔧 关键修复：检查寄存器类型，特殊处理
            let reg_type = register_types
                .get(&virtual_reg)
                .copied()
                .unwrap_or(RegisterType::Data);

            info!(
                "🔧 为虚拟寄存器 {:?} 分配物理寄存器，类型: {:?}",
                virtual_reg, reg_type
            );

            // 🔧 新增：检查是否为纯地址用途的栈地址寄存器，跳过分配
            if reg_type == RegisterType::StackAddress {
                let has_non_address_usage = self.has_non_address_usage(virtual_reg, function);
                if !has_non_address_usage {
                    info!(
                        "  跳过纯地址寄存器 {:?} 的分配，将由StackFrameLayout处理",
                        virtual_reg
                    );
                    continue;
                } else {
                    info!(
                        "  栈地址寄存器 {:?} 有非地址用途，进行正常分配",
                        virtual_reg
                    );
                }
            }

            // 🔧 尝试为当前寄存器找到不冲突的物理寄存器
            let mut assigned_physical_reg = None;

            for &physical_reg in &self.calling_convention.get_allocatable_registers() {
                info!("  🔍 尝试物理寄存器 r{}", physical_reg);

                if used_physical_regs.contains(&physical_reg) {
                    info!(
                        "    物理寄存器 r{} 已被使用，检查是否可以复用",
                        physical_reg
                    );
                    // 检查是否可以复用：当前寄存器的生命周期是否与已分配的寄存器冲突
                    let can_reuse = self.can_reuse_physical_register(
                        virtual_reg,
                        physical_reg,
                        &allocation_map,
                        &lifetime_map,
                    );

                    if can_reuse {
                        assigned_physical_reg = Some(physical_reg);
                        info!("    ✅ 可以复用物理寄存器 r{}", physical_reg);
                        break;
                    } else {
                        info!("    ❌ 不能复用物理寄存器 r{}，生命周期冲突", physical_reg);
                    }
                } else {
                    // 物理寄存器未被使用，直接分配
                    assigned_physical_reg = Some(physical_reg);
                    used_physical_regs.insert(physical_reg);
                    info!("    ✅ 直接分配空闲物理寄存器 r{}", physical_reg);
                    break;
                }
            }

            if let Some(physical_reg) = assigned_physical_reg {
                allocation_map.insert(virtual_reg, AllocationTarget::Register(physical_reg));
                info!("  虚拟寄存器 {:?} -> r{}", virtual_reg, physical_reg);
            } else {
                // 没有可用的物理寄存器，需要溢出
                let spill_slot = allocation_map
                    .values()
                    .filter_map(|target| {
                        if let AllocationTarget::Spill(slot) = target {
                            Some(*slot)
                        } else {
                            None
                        }
                    })
                    .max()
                    .unwrap_or(0)
                    + 1;
                allocation_map.insert(virtual_reg, AllocationTarget::Spill(spill_slot));

                // 🔧 特殊处理：检查是否超出范围或为特殊寄存器类型
                match reg_type {
                    RegisterType::StackAddress => {
                        info!(
                            "  虚拟寄存器 {:?} (栈地址寄存器) -> 溢出槽{}",
                            virtual_reg, spill_slot
                        );
                    }
                    _ => {
                        info!(
                            "  虚拟寄存器 {:?} (超出范围) -> 溢出槽{}",
                            virtual_reg, spill_slot
                        );
                    }
                }
            }
        }

        info!("🎯 调用约定寄存器分配映射: {:?}", allocation_map);
        allocation_map
    }

    /// 🔧 新方法：检查是否可以复用物理寄存器（基于生命周期分析）
    fn can_reuse_physical_register(
        &self,
        virtual_reg: Register,
        physical_reg: u8,
        allocation_map: &HashMap<Register, AllocationTarget>,
        lifetime_map: &HashMap<Register, (usize, usize)>,
    ) -> bool {
        let (current_start, current_end) =
            lifetime_map.get(&virtual_reg).copied().unwrap_or((0, 0));

        debug!(
            "🔍 检查 {} 是否可以复用物理寄存器 r{}",
            virtual_reg, physical_reg
        );
        debug!("  当前寄存器生命周期: [{}, {}]", current_start, current_end);

        // 查找所有已分配到该物理寄存器的虚拟寄存器
        for (allocated_virtual_reg, target) in allocation_map {
            if let AllocationTarget::Register(allocated_physical_reg) = target {
                if *allocated_physical_reg == physical_reg {
                    let (allocated_start, allocated_end) = lifetime_map
                        .get(allocated_virtual_reg)
                        .copied()
                        .unwrap_or((0, 0));

                    debug!(
                        "  已分配寄存器 {:?} 到 r{}, 生命周期: [{}, {}]",
                        allocated_virtual_reg, physical_reg, allocated_start, allocated_end
                    );

                    // 🔧 关键修复：更严格的生命周期重叠检测
                    // 两个区间重叠的条件：max(start1, start2) <= min(end1, end2)
                    let overlap_start = current_start.max(allocated_start);
                    let overlap_end = current_end.min(allocated_end);

                    if overlap_start <= overlap_end {
                        debug!(
                            "  ❌ 生命周期重叠！重叠区间: [{}, {}]",
                            overlap_start, overlap_end
                        );
                        return false; // 生命周期重叠，不能复用
                    } else {
                        debug!("  ✅ 生命周期不重叠，可以复用");
                    }
                }
            }
        }

        debug!(
            "  ✅ 物理寄存器 r{} 可以被 {:?} 复用",
            physical_reg, virtual_reg
        );
        true // 没有冲突，可以复用
    }

    /// 🔧 新方法：查找函数的返回值寄存器
    fn find_return_register(&self, function: &LirFunction) -> Option<Register> {
        // 查找return指令中使用的寄存器
        for instruction in &function.instructions {
            if let Instruction::Return {
                value: Some(reg), ..
            } = instruction
            {
                return Some(*reg);
            }
        }
        None
    }

    /// 🔧 重构：基于生命周期分析的寄存器分配
    fn apply_allocation_with_spilling(
        &mut self,
        function: &mut LirFunction,
        allocation_map: &HashMap<Register, AllocationTarget>,
    ) -> PassResult {
        info!("🔧 开始基于生命周期分析的寄存器分配");

        // 🎯 第一步：使用生命周期分析器获取精确的寄存器活跃度信息
        let lifetime_analyzer =
            lifetime_analysis::LifetimeAnalyzer::new(types::CallingConvention::standard());
        let (lifetimes, _register_types) = lifetime_analyzer.analyze_simple(function);

        info!("📊 生命周期分析结果:");
        for lifetime in &lifetimes {
            info!(
                "  {:?}: [{}, {}] uses={:?}",
                lifetime.register, lifetime.start, lifetime.end, lifetime.uses
            );
        }

        // 🎯 第二步：构建活跃度映射
        let liveness_map = self.build_liveness_map(&lifetimes);

        // 🎯 第三步：智能寄存器替换（避免破坏活跃寄存器）
        let mut transformer = IndexInstructionTransformer::new();

        // 保持原策略：不为spill槽显式插入Alloc，统一由SP做临时保存区域
        {
            self.spill_slot_addr_map.clear();
            self.scratch_slot_addr_map.clear();
        }

        // 🔧 关键修复：按指令顺序智能处理寄存器替换
        for (i, instruction) in function.instructions.iter().enumerate() {
            // if self.is_function_call(instruction) {
            //     // 🔧 特殊处理函数调用：确保不破坏返回值
            //     self.handle_function_call_with_liveness(
            //         &mut transformer,
            //         i,
            //         instruction,
            //         allocation_map,
            //         &liveness_map,
            //         function.stack_frame_size,
            //     );
            // } else {
            //     self.handle_instruction_with_liveness(
            //         &mut transformer,
            //         i,
            //         instruction,
            //         allocation_map,
            //         &liveness_map,
            //         function.stack_frame_size,
            //     );
            // }
            self.handle_instruction_with_liveness(
                &mut transformer,
                i,
                instruction,
                allocation_map,
                &liveness_map,
            );
        }

        // 应用变换
        transformer.apply_to_function(function);

        // 重写寄存器
        self.rewrite_registers(function, allocation_map);
        info!("✅ 基于生命周期的寄存器分配完成");
        PassResult::Changed
    }

    /// 重写指令中的虚拟寄存器为物理寄存器
    fn rewrite_registers(
        &self,
        function: &mut LirFunction,
        allocation_map: &HashMap<Register, AllocationTarget>,
    ) {
        // 🔧 性能优化：预先计算所有寄存器替换映射，避免重复计算
        let replacements = self.build_register_replacements(allocation_map, function);

        trace!("🔧 预计算的寄存器替换映射: {:?}", replacements);

        for instruction in &mut function.instructions {
            self.apply_register_replacements(instruction, &replacements);
        }
    }

    /// 🔧 新方法：检查栈地址寄存器是否有非地址用途
    fn has_non_address_usage(&self, register: Register, function: &LirFunction) -> bool {
        for instruction in &function.instructions {
            match instruction {
                Instruction::Load64 { addr, dst, .. } => {
                    // 作为地址使用，这是地址用途
                    if *addr == register {
                        continue;
                    }
                    // 作为目标寄存器使用，这是非地址用途
                    if *dst == register {
                        return true;
                    }
                    // 检查是否作为其他操作数使用
                    let used_regs = instruction.get_used_registers();
                    if used_regs.contains(&register) {
                        return true; // 作为非地址操作数使用
                    }
                }
                Instruction::Store64 { addr, src, .. } => {
                    // 作为地址使用，这是地址用途
                    if *addr == register {
                        continue;
                    }
                    // 作为存储的数据使用，这是非地址用途
                    if let Operand::Register { id } = src {
                        if *id == register {
                            return true;
                        }
                    }
                }
                Instruction::Alloc { dst, .. } => {
                    // 作为alloc的目标，这是地址定义，不是非地址用途
                    if *dst == register {
                        continue;
                    }
                }
                _ => {
                    // 其他指令中使用或定义该寄存器，视为非地址用途
                    let used = instruction.get_used_registers().contains(&register);
                    let defined = instruction.get_def_register() == Some(register);
                    if used || defined {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// 🔧 新方法：预先构建寄存器替换映射
    fn build_register_replacements(
        &self,
        allocation_map: &HashMap<Register, AllocationTarget>,
        function: &LirFunction,
    ) -> HashMap<Register, Register> {
        let mut replacements = HashMap::new();

        // 🔧 关键修复：分析寄存器用途，只跳过纯地址用途的寄存器
        for (virtual_reg, target) in allocation_map {
            // 检查栈地址寄存器是否有非地址用途
            if self.stack_address_registers.contains(virtual_reg) {
                let has_non_address_usage = self.has_non_address_usage(*virtual_reg, function);
                if !has_non_address_usage {
                    info!(
                        "  跳过纯地址寄存器 {:?} 的替换，将由StackFrameLayout处理",
                        virtual_reg
                    );
                    continue;
                } else {
                    info!(
                        "  栈地址寄存器 {:?} 有非地址用途，进行正常分配",
                        virtual_reg
                    );
                }
            }

            match target {
                AllocationTarget::Register(physical_reg) => {
                    // 分配到物理寄存器的虚拟寄存器
                    let new_reg = Register::Physical(*physical_reg);
                    replacements.insert(*virtual_reg, new_reg);
                    info!("  替换映射: {:?} -> r{}", virtual_reg, physical_reg);
                }
                AllocationTarget::Spill(spill_slot) => {
                    // 🔧 修复：所有spill寄存器直接映射到临时物理寄存器
                    // 不再区分地址寄存器和数据寄存器，统一处理
                    let temp_reg = self.get_temp_register_for_spill(*spill_slot);
                    let new_reg = Register::Physical(temp_reg as _);
                    replacements.insert(*virtual_reg, new_reg);
                    info!(
                        "  替换映射: {:?} -> r{} (临时寄存器，槽{})",
                        virtual_reg, temp_reg, spill_slot
                    );
                }
            }
        }

        replacements
    }

    /// 🔧 新方法：应用预计算的寄存器替换映射到单条指令
    fn apply_register_replacements(
        &self,
        instruction: &mut Instruction,
        replacements: &HashMap<Register, Register>,
    ) {
        // 🔧 调试：打印指令替换前的状态
        trace!("🔧 替换前指令: {}", instruction);

        // 🔧 性能优化：只对指令中实际存在的虚拟寄存器进行替换
        for (old_reg, new_reg) in replacements {
            if self.instruction_contains_register(instruction, *old_reg) {
                info!("  🔄 替换寄存器 {} -> {}", old_reg, new_reg);
                instruction.replace_register(*old_reg, *new_reg);
            }
        }

        // 🔧 调试：打印指令替换后的状态
        trace!("🔧 替换后指令: {}", instruction);
    }

    /// 🔧 新方法：检查指令是否包含指定的寄存器
    fn instruction_contains_register(&self, instruction: &Instruction, reg: Register) -> bool {
        // 检查目标寄存器
        if let Some(def_reg) = instruction.get_def_register() {
            if def_reg == reg {
                return true;
            }
        }

        // 检查使用的寄存器
        let used_registers = instruction.get_used_registers();
        used_registers.contains(&reg)
    }

    /// 🔧 新方法：构建活跃度映射
    fn build_liveness_map(
        &self,
        lifetimes: &[types::RegisterLifetime],
    ) -> HashMap<usize, HashSet<Register>> {
        let mut liveness_map = HashMap::new();

        // 为每个指令位置构建活跃寄存器集合
        let max_instruction = lifetimes.iter().map(|lt| lt.end).max().unwrap_or(0);

        for i in 0..=max_instruction {
            let mut live_registers = HashSet::new();

            for lifetime in lifetimes {
                if i >= lifetime.start && i <= lifetime.end {
                    live_registers.insert(lifetime.register);
                }
            }

            liveness_map.insert(i, live_registers);
        }

        info!(
            "🔍 活跃度映射构建完成，共 {} 个指令位置",
            liveness_map.len()
        );
        liveness_map
    }

    /// 🔧 新方法：基于活跃度信息智能处理指令
    fn handle_instruction_with_liveness(
        &self,
        transformer: &mut IndexInstructionTransformer,
        instruction_index: usize,
        instruction: &Instruction,
        allocation_map: &HashMap<Register, AllocationTarget>,
        liveness_map: &HashMap<usize, HashSet<Register>>,
    ) {
        info!("🔧 处理指令 {}: {:?}", instruction_index, instruction);

        // 获取当前指令位置的活跃寄存器
        let live_registers = liveness_map
            .get(&instruction_index)
            .cloned()
            .unwrap_or_default();
        info!("  活跃寄存器: {:?}", live_registers);

        // 获取指令使用和定义的寄存器
        let used_registers = instruction.get_used_registers();
        let def_register = instruction.get_def_register();

        let def_physical_reg = def_register.and_then(|reg| {
            if let Some(AllocationTarget::Register(reg)) = allocation_map.get(&reg) {
                Some(*reg)
            } else {
                None
            }
        });

        // 🔧 关键修复：收集需要加载的溢出寄存器和它们的临时寄存器
        let mut spill_loads = Vec::new();
        let mut temp_registers_to_save = Vec::new();

        // iter used_registers + def_register
        for &used_reg in used_registers.iter() {
            if let Some(AllocationTarget::Spill(slot_id)) = allocation_map.get(&used_reg) {
                if live_registers.contains(&used_reg) {
                    let temp_physical_reg = self.get_temp_register_for_spill(*slot_id);
                    spill_loads.push((used_reg, *slot_id, temp_physical_reg));

                    // 检查临时寄存器是否已经被分配给其他虚拟寄存器
                    if self.is_temp_register_conflicting(temp_physical_reg, allocation_map)
                        && def_physical_reg != Some(temp_physical_reg as u8)
                    {
                        // 输出寄存器不能被覆盖
                        temp_registers_to_save.push(temp_physical_reg);
                    }
                }
            }
        }
        for &used_reg in def_register.iter() {
            if let Some(AllocationTarget::Spill(slot_id)) = allocation_map.get(&used_reg) {
                if live_registers.contains(&used_reg) {
                    let temp_physical_reg = self.get_temp_register_for_spill(*slot_id);

                    // 检查临时寄存器是否已经被分配给其他虚拟寄存器
                    if self.is_temp_register_conflicting(temp_physical_reg, allocation_map) {
                        temp_registers_to_save.push(temp_physical_reg);
                    }
                }
            }
        }

        // 🔧 第一步：保存会被覆盖的临时寄存器到栈（直接使用SP）
        for &temp_reg in &temp_registers_to_save {
            info!("  💾 保存临时寄存器 r{} 到栈", temp_reg);
            // 使用栈指针直接压栈，避免虚拟地址寄存器
            let push_temp = Instruction::Store64 {
                addr: Register::Physical(6), // SP (r6)
                offset: 0,                   // 压栈后写入 [SP]
                src: Operand::Register {
                    id: Register::Physical(temp_reg as _),
                },
                span: Span::dummy(),
            };
            // 同时调整栈指针
            let adjust_sp = Instruction::Sub {
                dst: Register::Physical(6), // SP
                src1: Operand::Register {
                    id: Register::Physical(6),
                },
                src2: Operand::Immediate { value: 8 },
                span: Span::dummy(),
            };
            transformer.insert(instruction_index, adjust_sp);
            transformer.insert(instruction_index, push_temp);
        }

        // 🔧 第二步：加载溢出寄存器到临时寄存器（直接使用SP偏移）
        for (used_reg, slot_id, temp_physical_reg) in &spill_loads {
            info!(
                "  📥 加载溢出寄存器 {:?} 从槽 {} 到临时寄存器 r{}",
                used_reg, slot_id, temp_physical_reg
            );
            // 使用SP相对偏移，每个槽8字节，向下分配
            let stack_offset = -((*slot_id as i64) * 8 + 8);
            let load_instruction = Instruction::Load64 {
                dst: Register::Physical(*temp_physical_reg as _),
                addr: Register::Physical(6), // SP (r6)
                offset: stack_offset,
                span: Span::dummy(),
            };
            transformer.insert(instruction_index, load_instruction);
        }

        // 🔧 第三步：处理定义寄存器的存储
        let mut def_spill_info = None;
        if let Some(def_reg) = def_register {
            if let Some(AllocationTarget::Spill(slot_id)) = allocation_map.get(&def_reg) {
                let will_be_used_later =
                    self.check_if_used_later(def_reg, instruction_index, liveness_map);
                if will_be_used_later {
                    let temp_physical_reg = self.get_temp_register_for_spill(*slot_id);
                    def_spill_info = Some((def_reg, *slot_id, temp_physical_reg));
                }
            }
        }

        // 🔧 第四步：在指令后插入存储和恢复指令
        let after_offset = 1; // 从指令后一位开始

        // 存储定义寄存器（直接使用SP偏移）
        if let Some((def_reg, slot_id, temp_physical_reg)) = def_spill_info {
            info!("  📤 存储溢出寄存器 {:?} 到槽 {}", def_reg, slot_id);
            // 使用SP相对偏移，每个槽8字节，向下分配
            let stack_offset = -((slot_id as i64) * 8 + 8);
            let store_instruction = Instruction::Store64 {
                addr: Register::Physical(6), // SP (r6)
                offset: stack_offset,
                src: Operand::Register {
                    id: Register::Physical(temp_physical_reg as _),
                },
                span: Span::dummy(),
            };
            transformer.insert(instruction_index + after_offset, store_instruction);
        }

        // 🔧 第五步：恢复之前保存的临时寄存器（LIFO顺序，直接从栈弹出）
        for &temp_reg in temp_registers_to_save.iter().rev() {
            info!("  🔄 恢复临时寄存器 r{} 从栈", temp_reg);
            // 直接从栈顶弹出，避免虚拟地址寄存器
            let pop_temp = Instruction::Load64 {
                dst: Register::Physical(temp_reg as _),
                addr: Register::Physical(6), // SP (r6)
                offset: 0,                   // 从栈顶加载
                span: Span::dummy(),
            };
            // 恢复栈指针
            let restore_sp = Instruction::Add {
                dst: Register::Physical(6), // SP
                src1: Operand::Register {
                    id: Register::Physical(6),
                },
                src2: Operand::Immediate { value: 8 },
                span: Span::dummy(),
            };
            transformer.insert(instruction_index + after_offset, pop_temp);
            transformer.insert(instruction_index + after_offset, restore_sp);
        }
    }

    /// 🔧 新方法：检查临时寄存器是否与已分配的寄存器冲突
    fn is_temp_register_conflicting(
        &self,
        temp_reg: usize,
        allocation_map: &HashMap<Register, AllocationTarget>,
    ) -> bool {
        for target in allocation_map.values() {
            if let AllocationTarget::Register(physical_reg) = target {
                if *physical_reg as usize == temp_reg {
                    return true;
                }
            }
        }
        false
    }

    /// 🔧 新方法：为溢出槽分配确定性的临时寄存器
    fn get_temp_register_for_spill(&self, slot_id: usize) -> usize {
        // 🔧 关键修复：使用更智能的临时寄存器分配策略
        // 为不同的溢出槽分配不同的临时寄存器，避免冲突
        match slot_id {
            1 => 0,                                                                   // 溢出槽1使用r0
            2 => 1, // 溢出槽2使用r1
            3 => 2, // 溢出槽3使用r2
            4 => 3, // 溢出槽4使用r3
            _ => slot_id % self.calling_convention.get_allocatable_registers().len(), // 其他槽轮换使用
        }
    }

    /// 🔧 新方法：检查寄存器是否在后续指令中被使用
    fn check_if_used_later(
        &self,
        register: Register,
        current_index: usize,
        liveness_map: &HashMap<usize, HashSet<Register>>,
    ) -> bool {
        // 检查从当前指令之后的位置是否还有活跃的使用
        for (index, live_registers) in liveness_map {
            if *index > current_index && live_registers.contains(&register) {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod simple_stack_tests;

/// 运行简单栈式寄存器分配的公共函数
pub fn run_simple_stack_register_allocation(function: &mut LirFunction) -> PassResult {
    let mut pass = SimpleStackRegisterAllocation::new();
    let mut analyses = AnalysisManager::new();
    pass.run_on_function(function, &mut analyses)
}

/// 测试处理实际的LIR文件"1"
#[cfg(test)]
mod file_test;
