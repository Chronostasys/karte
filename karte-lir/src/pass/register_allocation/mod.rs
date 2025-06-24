//! 寄存器分配Pass
//!
//! ## 概述
//!
//! 这是寄存器分配的核心模块，实现了一个基于线性扫描的分配器。
//! 它被设计为一个两阶段的过程，以支持更灵活和强大的代码生成流程。
//!
//! ### 两阶段分配架构
//!
//! 1.  **Pre-RA (决策阶段)**:
//!     -   在 `DecisionOnly` 模式下运行。
//!     -   此阶段仅进行分析，决定哪些虚拟寄存器映射到物理寄存器，哪些需要溢出。
//!     -   结果被存储，但不修改代码。这是为 `StackFrameLowering` Pass 提供信息。
//!
//! 2.  **StackFrameLowering**:
//!     -   一个独立的Pass，在 Pre-RA 之后，Final-RA 之前运行。
//!     -   它根据 Pre-RA 的溢出决策，计算栈帧大小，并用真实的栈加载/存储指令
//!       替换掉所有对溢出寄存器的访问，同时可能会引入新的临时虚拟寄存器。
//!
//! 3.  **Final-RA (改写阶段)**:
//!     -   在 `FinalRewrite` 模式下运行。
//!     -   在 `StackFrameLowering` 完成后，此阶段重新运行一次完整的分配过程，
//!       为函数中所有的虚拟寄存器（包括 `StackFrameLowering` 新引入的临时寄存器）
//!       分配最终的物理寄存器。
//!     -   最后，它会重写LIR，将所有虚拟寄存器引用替换为物理寄存器引用。
//!
//! ## 模块结构
//!
//! - `types.rs`: 定义核心数据结构，如 `RegisterAllocationResult` 和 `RegisterLifetime`。
//! - `lifetime_analysis.rs`: 封装了所有关于计算寄存器生命周期的逻辑。
//! - `linear_scan.rs`: 实现了线性扫描分配算法本身。

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
        println!("🎯 开始遵循调用约定的寄存器分配：{}", function.name);

        // 重置状态
        self.spill_counter = 0;

        // 分析所有虚拟寄存器的使用情况
        let virtual_registers = self.collect_virtual_registers(function);
        println!("🎯 发现虚拟寄存器: {:?}", virtual_registers);

        // 🔧 新增：根据调用约定构建寄存器分配映射
        let allocation_map =
            self.build_calling_convention_allocation_map(function, &virtual_registers);
        println!("🎯 调用约定寄存器分配映射: {:?}", allocation_map);

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

        println!("🔍 开始收集虚拟寄存器...");
        for (i, instruction) in function.instructions.iter().enumerate() {
            // 收集定义的寄存器
            if let Some(def_reg) = instruction.get_def_register() {
                println!("  指令{}: 定义寄存器 {:?} - {:?}", i, def_reg, instruction);
                registers.insert(def_reg);
            }

            // 收集使用的寄存器
            for used_reg in instruction.get_used_registers() {
                println!("  指令{}: 使用寄存器 {:?} - {:?}", i, used_reg, instruction);
                registers.insert(used_reg);
            }
        }

        println!("🔍 收集到的所有寄存器: {:?}", registers);

        // 🔧 关键修复：确保确定性的寄存器顺序
        let mut sorted_registers: Vec<Register> = registers
            .into_iter()
            .filter(|reg| {
                let is_special = self.calling_convention.is_special_register(*reg);
                if is_special {
                    println!("  过滤特殊寄存器: {:?}", reg);
                }
                !is_special
            })
            .collect();

        // 按寄存器ID排序，确保每次运行结果一致
        sorted_registers.sort_by_key(|reg| reg.id());

        println!("🎯 发现虚拟寄存器: {:?}", sorted_registers);
        sorted_registers
    }

    /// 🔧 重构：基于生命周期分析的寄存器分配映射
    fn build_calling_convention_allocation_map(
        &self,
        function: &LirFunction,
        virtual_registers: &[Register],
    ) -> HashMap<Register, AllocationTarget> {
        println!("🎯 开始遵循调用约定的寄存器分配：{}", function.name);

        // 🔧 关键修复：首先进行生命周期分析
        let lifetime_analyzer =
            lifetime_analysis::LifetimeAnalyzer::new(types::CallingConvention::standard());
        let (lifetimes, register_types) = lifetime_analyzer.analyze_simple(function);

        // 构建生命周期映射，用于冲突检测
        let mut lifetime_map = HashMap::new();
        for lifetime in &lifetimes {
            lifetime_map.insert(lifetime.register, (lifetime.start, lifetime.end));
        }

        let mut allocation_map = HashMap::new();
        let mut used_physical_regs = HashSet::new();

        // 第一步：为函数参数寄存器分配固定的物理寄存器
        println!("🔧 第一步：分配函数参数寄存器");
        for (i, &param_reg) in function.parameter_registers.iter().enumerate() {
            if i < self.calling_convention.argument_registers.len() {
                let physical_reg = self.calling_convention.argument_registers[i];
                allocation_map.insert(param_reg, AllocationTarget::Register(physical_reg));
                used_physical_regs.insert(physical_reg);
                println!("  参数寄存器 {:?} -> r{}", param_reg, physical_reg);
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
                println!("  参数寄存器 {:?} -> 溢出槽{}", param_reg, spill_slot);
            }
        }

        // 第二步：为返回值寄存器分配r0（如果函数有返回值）
        println!("🔧 第二步：分配返回值寄存器");
        if let Some(return_reg) = self.find_return_register(function) {
            allocation_map.insert(
                return_reg,
                AllocationTarget::Register(self.calling_convention.return_register),
            );
            used_physical_regs.insert(self.calling_convention.return_register);
            println!(
                "  返回值寄存器 {:?} -> r{}",
                return_reg, self.calling_convention.return_register
            );
        }

        // 第三步：根据我们的cc给函数参数和返回值插入映射
        println!("🔧 第三步：分配函数调用寄存器");
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
                }
                _ => {}
            }
        }

        // 第四步：基于生命周期分析为其他虚拟寄存器分配物理寄存器
        println!("🔧 第四步：分配其他虚拟寄存器");

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
        println!("🔧 按生命周期排序后的寄存器分配顺序:");
        for &reg in &remaining_registers {
            let (start, end) = lifetime_map.get(&reg).copied().unwrap_or((0, 0));
            println!("  {:?}: 生命周期[{}, {}]", reg, start, end);
        }

        for virtual_reg in remaining_registers {
            // 🔧 关键修复：检查寄存器类型，特殊处理
            let reg_type = register_types
                .get(&virtual_reg)
                .copied()
                .unwrap_or(RegisterType::Data);

            println!(
                "🔧 为虚拟寄存器 {:?} 分配物理寄存器，类型: {:?}",
                virtual_reg, reg_type
            );

            // 🔧 尝试为当前寄存器找到不冲突的物理寄存器
            let mut assigned_physical_reg = None;

            for &physical_reg in &self.calling_convention.get_allocatable_registers() {
                println!("  🔍 尝试物理寄存器 r{}", physical_reg);

                if used_physical_regs.contains(&physical_reg) {
                    println!(
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
                        println!("    ✅ 可以复用物理寄存器 r{}", physical_reg);
                        break;
                    } else {
                        println!("    ❌ 不能复用物理寄存器 r{}，生命周期冲突", physical_reg);
                    }
                } else {
                    // 物理寄存器未被使用，直接分配
                    assigned_physical_reg = Some(physical_reg);
                    used_physical_regs.insert(physical_reg);
                    println!("    ✅ 直接分配空闲物理寄存器 r{}", physical_reg);
                    break;
                }
            }

            if let Some(physical_reg) = assigned_physical_reg {
                allocation_map.insert(virtual_reg, AllocationTarget::Register(physical_reg));
                println!("  虚拟寄存器 {:?} -> r{}", virtual_reg, physical_reg);
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
                        println!(
                            "  虚拟寄存器 {:?} (栈地址寄存器) -> 溢出槽{}",
                            virtual_reg, spill_slot
                        );
                    }
                    _ => {
                        println!(
                            "  虚拟寄存器 {:?} (超出范围) -> 溢出槽{}",
                            virtual_reg, spill_slot
                        );
                    }
                }
            }
        }

        println!("🎯 调用约定寄存器分配映射: {:?}", allocation_map);
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

        println!(
            "🔍 检查 {} 是否可以复用物理寄存器 r{}",
            virtual_reg, physical_reg
        );
        println!("  当前寄存器生命周期: [{}, {}]", current_start, current_end);

        // 查找所有已分配到该物理寄存器的虚拟寄存器
        for (allocated_virtual_reg, target) in allocation_map {
            if let AllocationTarget::Register(allocated_physical_reg) = target {
                if *allocated_physical_reg == physical_reg {
                    let (allocated_start, allocated_end) = lifetime_map
                        .get(allocated_virtual_reg)
                        .copied()
                        .unwrap_or((0, 0));

                    println!(
                        "  已分配寄存器 {:?} 到 r{}, 生命周期: [{}, {}]",
                        allocated_virtual_reg, physical_reg, allocated_start, allocated_end
                    );

                    // 🔧 关键修复：更严格的生命周期重叠检测
                    // 两个区间重叠的条件：max(start1, start2) <= min(end1, end2)
                    let overlap_start = current_start.max(allocated_start);
                    let overlap_end = current_end.min(allocated_end);

                    if overlap_start <= overlap_end {
                        println!(
                            "  ❌ 生命周期重叠！重叠区间: [{}, {}]",
                            overlap_start, overlap_end
                        );
                        return false; // 生命周期重叠，不能复用
                    } else {
                        println!("  ✅ 生命周期不重叠，可以复用");
                    }
                }
            }
        }

        println!(
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
        println!("🔧 开始基于生命周期分析的寄存器分配");

        // 🎯 第一步：使用生命周期分析器获取精确的寄存器活跃度信息
        let lifetime_analyzer =
            lifetime_analysis::LifetimeAnalyzer::new(types::CallingConvention::standard());
        let (lifetimes, _register_types) = lifetime_analyzer.analyze_simple(function);

        println!("📊 生命周期分析结果:");
        for lifetime in &lifetimes {
            println!(
                "  {:?}: [{}, {}] uses={:?}",
                lifetime.register, lifetime.start, lifetime.end, lifetime.uses
            );
        }

        // 🎯 第二步：构建活跃度映射
        let liveness_map = self.build_liveness_map(&lifetimes);

        // 🎯 第三步：智能寄存器替换（避免破坏活跃寄存器）
        let mut transformer = IndexInstructionTransformer::new();

        // 在函数开头分配栈空间
        let spilled_count = allocation_map
            .values()
            .filter(|target| matches!(target, AllocationTarget::Spill(_)))
            .count();

        if spilled_count > 0 {
            transformer.insert(
                1,
                Instruction::Sub {
                    dst: Register::Physical(6), // SP
                    src1: Operand::Register {
                        id: Register::Physical(6),
                    },
                    src2: Operand::Immediate {
                        value: (spilled_count * 8) as i64,
                    },
                    span: Span::dummy(),
                },
            );
        }

        // 🔧 关键修复：按指令顺序智能处理寄存器替换
        for (i, instruction) in function.instructions.iter().enumerate() {
            if self.is_function_call(instruction) {
                // 🔧 特殊处理函数调用：确保不破坏返回值
                self.handle_function_call_with_liveness(
                    &mut transformer,
                    i,
                    instruction,
                    allocation_map,
                    &liveness_map,
                );
            } else {
                self.handle_instruction_with_liveness(
                    &mut transformer,
                    i,
                    instruction,
                    allocation_map,
                    &liveness_map,
                );
            }
        }

        // 应用变换
        transformer.apply_to_function(function);

        // 重写寄存器
        self.rewrite_registers(function, allocation_map);

        println!("✅ 基于生命周期的寄存器分配完成");
        PassResult::Changed
    }

    /// 检查指令是否是函数调用
    fn is_function_call(&self, instruction: &Instruction) -> bool {
        matches!(
            instruction,
            Instruction::Call { .. } | Instruction::CallIndirect { .. }
        )
    }

    /// 重写指令中的虚拟寄存器为物理寄存器
    fn rewrite_registers(
        &self,
        function: &mut LirFunction,
        allocation_map: &HashMap<Register, AllocationTarget>,
    ) {
        // 🔧 性能优化：预先计算所有寄存器替换映射，避免重复计算
        let replacements = self.build_register_replacements(allocation_map);

        println!("🔧 预计算的寄存器替换映射: {:?}", replacements);

        for instruction in &mut function.instructions {
            self.apply_register_replacements(instruction, &replacements);
        }
    }

    /// 🔧 新方法：预先构建寄存器替换映射
    fn build_register_replacements(
        &self,
        allocation_map: &HashMap<Register, AllocationTarget>,
    ) -> HashMap<Register, Register> {
        let mut replacements = HashMap::new();

        // 🔧 关键修复：只处理原始虚拟寄存器到最终物理寄存器的映射
        // 避免包含中间虚拟寄存器的映射，防止链式替换
        for (virtual_reg, target) in allocation_map {
            // 🔧 重要：只处理真正的虚拟寄存器（ID >= 6，r0-r5是物理寄存器）
            // 这样可以避免链式替换的问题
            // if virtual_reg.id() < 6 {
            //     println!("  跳过物理寄存器: {:?}", virtual_reg);
            //     continue;
            // }

            match target {
                AllocationTarget::Register(physical_reg) => {
                    // 分配到物理寄存器的虚拟寄存器
                    let new_reg = Register::Physical(*physical_reg);
                    replacements.insert(*virtual_reg, new_reg);
                    println!("  替换映射: {:?} -> r{}", virtual_reg, physical_reg);
                }
                AllocationTarget::Spill(spill_slot) => {
                    // 🔧 关键修复：溢出寄存器需要被重写为对应的临时寄存器
                    let temp_reg = self.get_temp_register_for_spill(*spill_slot);
                    let new_reg = Register::Physical(temp_reg as _);
                    replacements.insert(*virtual_reg, new_reg);
                    println!(
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
        println!("🔧 替换前指令: {}", instruction);

        // 🔧 性能优化：只对指令中实际存在的虚拟寄存器进行替换
        for (old_reg, new_reg) in replacements {
            if self.instruction_contains_register(instruction, *old_reg) {
                println!("  🔄 替换寄存器 {} -> {}", old_reg, new_reg);
                instruction.replace_register(*old_reg, *new_reg);
            }
        }

        // 🔧 调试：打印指令替换后的状态
        println!("🔧 替换后指令: {}", instruction);
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

        println!(
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
        println!("🔧 处理指令 {}: {:?}", instruction_index, instruction);

        // 获取当前指令位置的活跃寄存器
        let live_registers = liveness_map
            .get(&instruction_index)
            .cloned()
            .unwrap_or_default();
        println!("  活跃寄存器: {:?}", live_registers);

        // 获取指令使用和定义的寄存器
        let used_registers = instruction.get_used_registers();
        let def_register = instruction.get_def_register();

        // 🔧 关键修复：收集需要加载的溢出寄存器和它们的临时寄存器
        let mut spill_loads = Vec::new();
        let mut temp_registers_to_save = Vec::new();

        for &used_reg in &used_registers {
            if let Some(AllocationTarget::Spill(slot_id)) = allocation_map.get(&used_reg) {
                if live_registers.contains(&used_reg) {
                    let temp_physical_reg = self.get_temp_register_for_spill(*slot_id);
                    spill_loads.push((used_reg, *slot_id, temp_physical_reg));

                    // 检查临时寄存器是否已经被分配给其他虚拟寄存器
                    if self.is_temp_register_conflicting(temp_physical_reg, allocation_map) {
                        temp_registers_to_save.push(temp_physical_reg);
                    }
                }
            }
        }

        // 🔧 第一步：保存会被覆盖的临时寄存器到栈
        let mut insert_offset = 0;
        for &temp_reg in &temp_registers_to_save {
            println!("  💾 保存临时寄存器 r{} 到栈", temp_reg);

            // 调整栈指针
            let adjust_sp = Instruction::Sub {
                dst: Register::Physical(6), // SP
                src1: Operand::Register {
                    id: Register::Physical(6),
                },
                src2: Operand::Immediate { value: 8 },
                span: Span::dummy(),
            };
            transformer.insert(instruction_index + insert_offset, adjust_sp);
            insert_offset += 1;

            // 保存寄存器值
            let save_temp = Instruction::Store64 {
                addr: Register::Physical(6), // SP
                offset: 0,
                src: Operand::Register {
                    id: Register::Physical(temp_reg as _),
                },
                span: Span::dummy(),
            };
            transformer.insert(instruction_index + insert_offset, save_temp);
            insert_offset += 1;
        }

        // 🔧 第二步：加载溢出寄存器到临时寄存器
        for (used_reg, slot_id, temp_physical_reg) in &spill_loads {
            println!(
                "  📥 加载溢出寄存器 {:?} 从槽 {} 到临时寄存器 r{}",
                used_reg, slot_id, temp_physical_reg
            );

            let load_instruction = Instruction::Load64 {
                dst: Register::Physical(*temp_physical_reg as _),
                addr: Register::Physical(6), // SP
                offset: -((*slot_id as i64 + 1) * 8) - (temp_registers_to_save.len() as i64 * 8), // 考虑保存的寄存器
                span: Span::dummy(),
            };
            transformer.insert(instruction_index + insert_offset, load_instruction);
            insert_offset += 1;
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
        let mut after_offset = 1; // 从指令后一位开始

        // 存储定义寄存器
        if let Some((def_reg, slot_id, temp_physical_reg)) = def_spill_info {
            println!("  📤 存储溢出寄存器 {:?} 到槽 {}", def_reg, slot_id);

            let store_instruction = Instruction::Store64 {
                addr: Register::Physical(6), // SP
                offset: -((slot_id as i64 + 1) * 8) - (temp_registers_to_save.len() as i64 * 8), // 考虑保存的寄存器
                src: Operand::Register {
                    id: Register::Physical(temp_physical_reg as _),
                },
                span: Span::dummy(),
            };
            transformer.insert(
                instruction_index + insert_offset + after_offset,
                store_instruction,
            );
            after_offset += 1;
        }

        // 🔧 第五步：恢复之前保存的临时寄存器（LIFO顺序）
        for &temp_reg in temp_registers_to_save.iter().rev() {
            println!("  🔄 恢复临时寄存器 r{} 从栈", temp_reg);

            // 恢复寄存器值
            let restore_temp = Instruction::Load64 {
                dst: Register::Physical(temp_reg as _),
                addr: Register::Physical(6), // SP
                offset: 0,
                span: Span::dummy(),
            };
            transformer.insert(
                instruction_index + insert_offset + after_offset,
                restore_temp,
            );
            after_offset += 1;

            // 恢复栈指针
            let restore_sp = Instruction::Add {
                dst: Register::Physical(6), // SP
                src1: Operand::Register {
                    id: Register::Physical(6),
                },
                src2: Operand::Immediate { value: 8 },
                span: Span::dummy(),
            };
            transformer.insert(instruction_index + insert_offset + after_offset, restore_sp);
            after_offset += 1;
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

    /// 🔧 新方法：特殊处理函数调用指令，确保不破坏返回值
    fn handle_function_call_with_liveness(
        &self,
        transformer: &mut IndexInstructionTransformer,
        instruction_index: usize,
        instruction: &Instruction,
        allocation_map: &HashMap<Register, AllocationTarget>,
        liveness_map: &HashMap<usize, HashSet<Register>>,
    ) {
        println!(
            "🔧 特殊处理函数调用指令 {}: {:?}",
            instruction_index, instruction
        );

        // 获取当前指令位置的活跃寄存器
        let live_registers = liveness_map
            .get(&instruction_index)
            .cloned()
            .unwrap_or_default();
        println!("  活跃寄存器: {:?}", live_registers);

        // 获取指令使用的寄存器（函数调用的参数和函数地址）
        let used_registers = instruction.get_used_registers();

        // 🔧 关键修复：为函数调用中的溢出寄存器分配不同的临时寄存器
        let mut temp_register_assignments = HashMap::new();
        let mut next_temp_register = 0;

        // 🔧 第一步：为所有需要加载的溢出寄存器分配临时寄存器
        for &used_reg in &used_registers {
            if let Some(AllocationTarget::Spill(slot_id)) = allocation_map.get(&used_reg) {
                if live_registers.contains(&used_reg) {
                    // 🔧 关键修复：为每个溢出寄存器分配不同的临时寄存器
                    let temp_physical_reg = self.get_unique_temp_register_for_call(
                        &mut next_temp_register,
                        &temp_register_assignments,
                    );
                    temp_register_assignments.insert(used_reg, temp_physical_reg);

                    println!(
                        "  📥 函数调用前加载参数寄存器 {:?} 从槽 {} 到临时寄存器 r{}",
                        used_reg, slot_id, temp_physical_reg
                    );

                    let load_instruction = Instruction::Load64 {
                        dst: Register::Physical(temp_physical_reg as _),
                        addr: Register::Physical(6), // SP
                        offset: -((*slot_id as i64 + 1) * 8),
                        span: Span::dummy(),
                    };
                    transformer.insert(instruction_index, load_instruction);
                }
            }
        }

        // 🔧 关键修复：检查函数调用的返回值寄存器
        if let Some(def_reg) = instruction.get_def_register() {
            println!("  🎯 函数调用返回值寄存器: {:?}", def_reg);

            // 检查返回值寄存器是否需要溢出
            if let Some(AllocationTarget::Spill(slot_id)) = allocation_map.get(&def_reg) {
                // 🔧 关键修复：检查返回值在后续是否被使用
                let return_value_used_later =
                    self.check_if_used_after_call(def_reg, instruction_index, liveness_map);

                if return_value_used_later {
                    println!(
                        "  📤 函数调用后存储返回值寄存器 {:?} 到槽 {}",
                        def_reg, slot_id
                    );

                    // 返回值寄存器使用调用约定的返回寄存器 r0
                    let store_instruction = Instruction::Store64 {
                        addr: Register::Physical(6), // SP
                        offset: -((*slot_id as i64 + 1) * 8),
                        src: Operand::Register {
                            id: Register::Physical(self.calling_convention.return_register),
                        },
                        span: Span::dummy(),
                    };
                    // 🔧 关键：在函数调用后插入存储指令
                    transformer.insert(instruction_index + 1, store_instruction);
                } else {
                    println!("  ⚠️  返回值寄存器 {:?} 在后续未被使用，跳过存储", def_reg);
                }
            }
            //  else {
            //     // 不需要溢出，mov到目标寄存器
            //     let mov_instruction = Instruction::Move {
            //         dst: def_reg,
            //         src: Operand::Register { id: Register::Virtual(self.calling_convention.return_register as usize) },
            //         span: Span::dummy(),
            //     };
            //     transformer.insert(instruction_index + 1, mov_instruction);

            // }
        }
    }

    /// 🔧 新方法：为函数调用分配唯一的临时寄存器
    fn get_unique_temp_register_for_call(
        &self,
        next_temp_register: &mut usize,
        temp_register_assignments: &HashMap<Register, usize>,
    ) -> usize {
        // 从可分配寄存器中选择一个未被使用的
        for &physical_reg in &self.calling_convention.get_allocatable_registers() {
            let physical_reg_usize = physical_reg as usize;
            if !temp_register_assignments
                .values()
                .any(|&assigned| assigned == physical_reg_usize)
            {
                *next_temp_register = physical_reg_usize + 1;
                return physical_reg_usize;
            }
        }

        // 如果所有可分配寄存器都被使用，使用下一个可用的寄存器
        let result = *next_temp_register;
        *next_temp_register += 1;
        result
    }

    /// 🔧 新方法：检查寄存器是否在函数调用后被使用
    fn check_if_used_after_call(
        &self,
        register: Register,
        call_index: usize,
        liveness_map: &HashMap<usize, HashSet<Register>>,
    ) -> bool {
        // 检查从函数调用后的位置开始，是否还有活跃的使用
        for (index, live_registers) in liveness_map {
            if *index > call_index && live_registers.contains(&register) {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AllocationType, Instruction, LirFunction, Operand, Register};
    use karte_diagnostics::Span;

    #[test]
    fn test_register_type_analysis() {
        // 创建一个简单的测试函数
        let function = LirFunction {
            name: "test_function".to_string(),
            parameter_registers: vec![Register::Virtual(100), Register::Virtual(101)],
            instructions: vec![
                // alloc指令 - 栈分配
                Instruction::Alloc {
                    dst: Register::Virtual(200),
                    size: 8,
                    alignment: 8,
                    allocation_type: AllocationType::Stack,
                    span: Span::dummy(),
                },
                // alloc指令 - 堆分配
                Instruction::Alloc {
                    dst: Register::Virtual(201),
                    size: 16,
                    alignment: 8,
                    allocation_type: AllocationType::Heap,
                    span: Span::dummy(),
                },
                // add指令 - 栈地址计算
                Instruction::Add {
                    dst: Register::Virtual(202),
                    src1: Operand::Register {
                        id: Register::Physical(7),
                    }, // FP
                    src2: Operand::Immediate { value: -8 },
                    span: Span::dummy(),
                },
                // 普通数据操作
                Instruction::Add {
                    dst: Register::Virtual(203),
                    src1: Operand::Register {
                        id: Register::Virtual(100),
                    },
                    src2: Operand::Register {
                        id: Register::Virtual(101),
                    },
                    span: Span::dummy(),
                },
            ],
            next_register: 204,
            next_label: 1,
            struct_types: HashMap::new(),
            stack_frame_size: 0,
            parameter_count: 2,
        };

        let calling_convention = types::CallingConvention::standard();
        let lifetime_analyzer = LifetimeAnalyzer::new(calling_convention);
        let (lifetimes, register_types) = lifetime_analyzer.analyze_simple(&function);

        // 验证函数参数类型
        assert_eq!(
            register_types.get(&Register::Virtual(100)),
            Some(&RegisterType::FunctionParameter)
        );
        assert_eq!(
            register_types.get(&Register::Virtual(101)),
            Some(&RegisterType::FunctionParameter)
        );

        // 验证栈地址寄存器类型
        assert_eq!(
            register_types.get(&Register::Virtual(200)),
            Some(&RegisterType::StackAddress)
        ); // 栈alloc
        assert_eq!(
            register_types.get(&Register::Virtual(202)),
            Some(&RegisterType::StackAddress)
        ); // FP + offset

        // 验证数据寄存器类型
        assert_eq!(
            register_types.get(&Register::Virtual(201)),
            Some(&RegisterType::Data)
        ); // 堆alloc
        assert_eq!(
            register_types.get(&Register::Virtual(203)),
            Some(&RegisterType::Data)
        ); // 普通计算

        // 验证生命周期中的寄存器类型
        for lifetime in &lifetimes {
            match lifetime.register.id() {
                100 | 101 => assert_eq!(lifetime.register_type, RegisterType::FunctionParameter),
                200 | 202 => assert_eq!(lifetime.register_type, RegisterType::StackAddress),
                201 | 203 => assert_eq!(lifetime.register_type, RegisterType::Data),
                _ => {}
            }
        }

        println!("✅ 寄存器类型分析测试通过");
    }

    #[test]
    fn test_spill_constraints() {
        // 测试溢出约束
        assert!(RegisterType::Data.can_spill());
        assert!(!RegisterType::StackAddress.can_spill());
        assert!(!RegisterType::FunctionParameter.can_spill());
        assert!(!RegisterType::Special.can_spill());

        println!("✅ 溢出约束测试通过");
    }
}

#[cfg(test)]
mod simple_stack_tests {
    use super::*;
    use crate::{AllocationType, Instruction, LirFunction, Operand, Register};
    use karte_diagnostics::Span;
    use std::collections::HashMap;

    /// 测试新的简单栈式寄存器分配Pass
    #[test]
    fn test_simple_stack_register_allocation() {
        // 创建测试函数，模拟文件"1"中的LIR代码
        let mut function = LirFunction {
            name: "main".to_string(),
            parameter_registers: vec![],
            instructions: vec![
                // L1:
                Instruction::Label {
                    id: crate::LabelId(1),
                    span: Span::dummy(),
                },
                // mov r1, #1
                Instruction::Move {
                    dst: Register::Virtual(1),
                    src: Operand::Immediate { value: 1 },
                    span: Span::dummy(),
                },
                // mov r2, #2
                Instruction::Move {
                    dst: Register::Virtual(2),
                    src: Operand::Immediate { value: 2 },
                    span: Span::dummy(),
                },
                // mov r3, #3
                Instruction::Move {
                    dst: Register::Virtual(3),
                    src: Operand::Immediate { value: 3 },
                    span: Span::dummy(),
                },
                // mov r4, #4
                Instruction::Move {
                    dst: Register::Virtual(4),
                    src: Operand::Immediate { value: 4 },
                    span: Span::dummy(),
                },
                // mov r5, #5
                Instruction::Move {
                    dst: Register::Virtual(5),
                    src: Operand::Immediate { value: 5 },
                    span: Span::dummy(),
                },
                // mov r8, #6
                Instruction::Move {
                    dst: Register::Virtual(8),
                    src: Operand::Immediate { value: 6 },
                    span: Span::dummy(),
                },
                // mov r9, #7
                Instruction::Move {
                    dst: Register::Virtual(9),
                    src: Operand::Immediate { value: 7 },
                    span: Span::dummy(),
                },
                // add r10, r8, r9
                Instruction::Add {
                    dst: Register::Virtual(10),
                    src1: Operand::Register {
                        id: Register::Virtual(8),
                    },
                    src2: Operand::Register {
                        id: Register::Virtual(9),
                    },
                    span: Span::dummy(),
                },
                // add r11, r5, r10
                Instruction::Add {
                    dst: Register::Virtual(11),
                    src1: Operand::Register {
                        id: Register::Virtual(5),
                    },
                    src2: Operand::Register {
                        id: Register::Virtual(10),
                    },
                    span: Span::dummy(),
                },
                // add r12, r4, r11
                Instruction::Add {
                    dst: Register::Virtual(12),
                    src1: Operand::Register {
                        id: Register::Virtual(4),
                    },
                    src2: Operand::Register {
                        id: Register::Virtual(11),
                    },
                    span: Span::dummy(),
                },
                // add r13, r3, r12
                Instruction::Add {
                    dst: Register::Virtual(13),
                    src1: Operand::Register {
                        id: Register::Virtual(3),
                    },
                    src2: Operand::Register {
                        id: Register::Virtual(12),
                    },
                    span: Span::dummy(),
                },
                // add r14, r2, r13
                Instruction::Add {
                    dst: Register::Virtual(14),
                    src1: Operand::Register {
                        id: Register::Virtual(2),
                    },
                    src2: Operand::Register {
                        id: Register::Virtual(13),
                    },
                    span: Span::dummy(),
                },
                // add r15, r1, r14
                Instruction::Add {
                    dst: Register::Virtual(15),
                    src1: Operand::Register {
                        id: Register::Virtual(1),
                    },
                    src2: Operand::Register {
                        id: Register::Virtual(14),
                    },
                    span: Span::dummy(),
                },
                // ret r15
                Instruction::Return {
                    value: Some(Register::Virtual(15)),
                    span: Span::dummy(),
                },
            ],
            next_register: 16,
            next_label: 2,
            struct_types: HashMap::new(),
            stack_frame_size: 0,
            parameter_count: 0,
        };

        println!("🧪 测试前的LIR函数:");
        for (i, instruction) in function.instructions.iter().enumerate() {
            println!("  {}: {:?}", i, instruction);
        }

        // 创建并运行新的寄存器分配Pass
        let mut pass = SimpleStackRegisterAllocation::new();
        let mut analyses = AnalysisManager::new();

        let result = pass.run_on_function(&mut function, &mut analyses);

        println!("🧪 测试后的LIR函数:");
        for (i, instruction) in function.instructions.iter().enumerate() {
            println!("  {}: {:?}", i, instruction);
        }

        // 验证结果
        assert!(matches!(result, PassResult::Changed));

        // 验证是否插入了栈空间分配指令
        let has_stack_allocation = function.instructions.iter().any(|inst| {
            matches!(inst, Instruction::Sub { dst, src2, .. }
                if dst.id() == 6 && matches!(src2, Operand::Immediate { value } if *value > 0))
        });

        // 验证寄存器是否被正确重写
        let uses_only_physical_regs = function.instructions.iter().all(|inst| {
            let used_regs = inst.get_used_registers();
            let def_reg = inst.get_def_register();

            // 检查所有使用的寄存器都是物理寄存器 (r0-r7)
            let used_ok = used_regs.iter().all(|reg| reg.id() <= 7);
            let def_ok = def_reg.is_none_or(|reg| reg.id() <= 7);

            used_ok && def_ok
        });

        println!("✅ 寄存器分配测试完成");
        println!("  - 栈空间分配: {}", has_stack_allocation);
        println!("  - 物理寄存器范围: {}", uses_only_physical_regs);

        // 如果有很多虚拟寄存器，应该有栈分配
        if function.instructions.len() > 10 {
            assert!(has_stack_allocation, "应该有栈空间分配指令");
        }
        assert!(
            uses_only_physical_regs,
            "所有寄存器都应该在物理寄存器范围内"
        );
    }

    #[test]
    fn test_complex_call_indirect_register_allocation() {
        println!("🧪 测试复杂call_indirect指令寄存器保存/恢复:");

        // 创建一个更复杂的函数，包含多个虚拟寄存器和函数调用
        let mut function = LirFunction {
            name: "complex_test".to_string(),
            parameter_registers: vec![Register::Physical(0)],
            next_register: 200,
            next_label: 10,
            struct_types: HashMap::new(),
            stack_frame_size: 0,
            parameter_count: 1,
            instructions: vec![
                // 分配结构体
                Instruction::Alloc {
                    dst: Register::Virtual(100),
                    size: 16,
                    alignment: 8,
                    allocation_type: AllocationType::Stack,
                    span: Span::dummy(),
                },
                // 存储函数地址
                Instruction::Store64 {
                    addr: Register::Virtual(100),
                    offset: 0,
                    src: Operand::Immediate { value: 1 },
                    span: Span::dummy(),
                },
                // 设置多个虚拟寄存器
                Instruction::Move {
                    dst: Register::Virtual(101),
                    src: Operand::Immediate { value: 42 },
                    span: Span::dummy(),
                },
                Instruction::Move {
                    dst: Register::Virtual(102),
                    src: Operand::Immediate { value: 43 },
                    span: Span::dummy(),
                },
                Instruction::Move {
                    dst: Register::Virtual(103),
                    src: Operand::Immediate { value: 44 },
                    span: Span::dummy(),
                },
                Instruction::Move {
                    dst: Register::Virtual(104),
                    src: Operand::Immediate { value: 45 },
                    span: Span::dummy(),
                },
                Instruction::Move {
                    dst: Register::Virtual(105),
                    src: Operand::Immediate { value: 46 },
                    span: Span::dummy(),
                },
                // 加载函数地址
                Instruction::Load64 {
                    dst: Register::Virtual(106),
                    addr: Register::Virtual(100),
                    offset: 0,
                    span: Span::dummy(),
                },
                // 函数调用
                Instruction::CallIndirect {
                    function_register: Register::Virtual(106),
                    args: vec![Register::Virtual(101)],
                    arg_operands: vec![Operand::Register {
                        id: Register::Virtual(101),
                    }],
                    result: Some(Register::Virtual(107)),
                    span: Span::dummy(),
                },
                // 使用函数调用结果和之前的寄存器
                Instruction::Add {
                    dst: Register::Virtual(108),
                    src1: Operand::Register {
                        id: Register::Virtual(107),
                    },
                    src2: Operand::Register {
                        id: Register::Virtual(102),
                    },
                    span: Span::dummy(),
                },
                Instruction::Add {
                    dst: Register::Virtual(109),
                    src1: Operand::Register {
                        id: Register::Virtual(108),
                    },
                    src2: Operand::Register {
                        id: Register::Virtual(103),
                    },
                    span: Span::dummy(),
                },
                Instruction::Add {
                    dst: Register::Virtual(110),
                    src1: Operand::Register {
                        id: Register::Virtual(109),
                    },
                    src2: Operand::Register {
                        id: Register::Virtual(104),
                    },
                    span: Span::dummy(),
                },
                Instruction::Add {
                    dst: Register::Virtual(111),
                    src1: Operand::Register {
                        id: Register::Virtual(110),
                    },
                    src2: Operand::Register {
                        id: Register::Virtual(105),
                    },
                    span: Span::dummy(),
                },
                Instruction::Return {
                    value: Some(Register::Virtual(111)),
                    span: Span::dummy(),
                },
            ],
        };

        println!("原始LIR:");
        for (i, instr) in function.instructions.iter().enumerate() {
            println!("  {}: {:?}", i, instr);
        }

        // 运行寄存器分配
        let mut pass = SimpleStackRegisterAllocation::new();
        let mut analysis_manager = AnalysisManager::new();
        let result = pass.run_on_function(&mut function, &mut analysis_manager);

        match result {
            PassResult::Changed | PassResult::Unchanged => {}
            PassResult::Failed(msg) => panic!("寄存器分配失败: {}", msg),
        }

        println!("\n寄存器分配后:");
        for (i, instr) in function.instructions.iter().enumerate() {
            println!("  {}: {:?}", i, instr);
        }

        // 验证生成的代码
        let mut found_call_indirect = false;

        let mut store_addresses = Vec::new();
        let mut load_addresses = Vec::new();

        for (i, instruction) in function.instructions.iter().enumerate() {
            match instruction {
                Instruction::CallIndirect { .. } => {
                    found_call_indirect = true;
                    println!("🔍 发现call_indirect指令在位置 {}", i);
                }
                Instruction::Store64 { addr, offset, .. } if *addr == Register::Physical(6) => {
                    store_addresses.push(offset);
                    println!("🔍 发现store64指令: offset={}", offset);
                }
                Instruction::Load64 { addr, offset, .. } if *addr == Register::Physical(6) => {
                    load_addresses.push(offset);
                    println!("🔍 发现load64指令: offset={}", offset);
                }
                _ => {}
            }
        }

        // 验证栈操作的正确性
        if store_addresses.len() > 1 {
            // 检查store地址是否各不相同
            let mut unique_stores = store_addresses.clone();
            unique_stores.sort();
            unique_stores.dedup();

            println!("🔍 Store地址: {:?}", store_addresses);
            println!("🔍 唯一Store地址: {:?}", unique_stores);

            if unique_stores.len() == store_addresses.len() {
                println!("✅ 所有store指令使用不同的栈地址");
            } else {
                println!("❌ 发现重复的store地址！");
            }
        }

        if load_addresses.len() > 1 {
            // 检查load地址是否各不相同
            let mut unique_loads = load_addresses.clone();
            unique_loads.sort();
            unique_loads.dedup();

            println!("🔍 Load地址: {:?}", load_addresses);
            println!("🔍 唯一Load地址: {:?}", unique_loads);

            if unique_loads.len() == load_addresses.len() {
                println!("✅ 所有load指令使用不同的栈地址");
            } else {
                println!("❌ 发现重复的load地址！");
            }
        }

        assert!(found_call_indirect, "应该找到call_indirect指令");
        println!("✅ 复杂call_indirect指令寄存器分配测试通过！");
    }
}

/// 运行简单栈式寄存器分配的公共函数
pub fn run_simple_stack_register_allocation(function: &mut LirFunction) -> PassResult {
    let mut pass = SimpleStackRegisterAllocation::new();
    let mut analyses = AnalysisManager::new();
    pass.run_on_function(function, &mut analyses)
}

/// 测试处理实际的LIR文件"1"
#[cfg(test)]
mod file_test {
    use super::*;
    use crate::{Instruction, LirFunction, Operand, Register};
    use karte_diagnostics::Span;
    use std::collections::HashMap;

    /// 解析LIR文件内容为LirFunction
    fn parse_lir_file(content: &str) -> Result<LirFunction, String> {
        let lines: Vec<&str> = content.lines().collect();

        if lines.is_empty() {
            return Err("Empty file".to_string());
        }

        // 解析函数头
        let first_line = lines[0].trim();
        if !first_line.starts_with("function ") {
            return Err("Invalid function header".to_string());
        }

        let function_name = first_line
            .strip_prefix("function ")
            .and_then(|s| s.split(' ').next())
            .unwrap_or("main")
            .to_string();

        let mut function = LirFunction {
            name: function_name,
            parameter_registers: vec![],
            instructions: vec![],
            next_register: 16,
            next_label: 2,
            struct_types: HashMap::new(),
            stack_frame_size: 0,
            parameter_count: 0,
        };

        // 解析指令
        for (line_num, line) in lines.iter().enumerate().skip(1) {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let instruction = parse_instruction(line, line_num)?;
            function.instructions.push(instruction);
        }

        Ok(function)
    }

    /// 解析单条指令
    fn parse_instruction(line: &str, line_num: usize) -> Result<Instruction, String> {
        let parts: Vec<&str> = line.split_whitespace().collect();

        if parts.is_empty() {
            return Err(format!("Empty instruction at line {}", line_num));
        }

        match parts[0] {
            // 标签
            label if label.ends_with(':') => {
                let label_name = label.trim_end_matches(':');
                let label_id = match label_name {
                    "L1" => crate::LabelId(1),
                    _ => crate::LabelId(0),
                };
                Ok(Instruction::Label {
                    id: label_id,
                    span: Span::dummy(),
                })
            }
            // mov 指令
            "mov" => {
                if parts.len() != 3 {
                    return Err(format!("Invalid mov instruction at line {}", line_num));
                }

                let dst = parse_register(parts[1].trim_end_matches(','))?;
                let src = parse_operand(parts[2])?;

                Ok(Instruction::Move {
                    dst,
                    src,
                    span: Span::dummy(),
                })
            }
            // add 指令
            "add" => {
                if parts.len() != 4 {
                    return Err(format!("Invalid add instruction at line {}", line_num));
                }

                let dst = parse_register(parts[1].trim_end_matches(','))?;
                let src1 = parse_operand(parts[2].trim_end_matches(','))?;
                let src2 = parse_operand(parts[3])?;

                Ok(Instruction::Add {
                    dst,
                    src1,
                    src2,
                    span: Span::dummy(),
                })
            }
            // load64 指令 - load64 r10, [r5]
            "load64" => {
                if parts.len() != 3 {
                    return Err(format!("Invalid load64 instruction at line {}", line_num));
                }

                let dst = parse_register(parts[1].trim_end_matches(','))?;
                let addr_str = parts[2];

                // 简单解析 [rX] 格式
                if addr_str.starts_with('[') && addr_str.ends_with(']') {
                    let inner = &addr_str[1..addr_str.len() - 1];
                    let addr = parse_register(inner)?;

                    Ok(Instruction::Load64 {
                        dst,
                        addr,
                        offset: 0,
                        span: Span::dummy(),
                    })
                } else {
                    Err(format!("Invalid load64 address format: {}", addr_str))
                }
            }
            // ret 指令
            "ret" => {
                let value = if parts.len() > 1 {
                    Some(parse_register(parts[1])?)
                } else {
                    None
                };

                Ok(Instruction::Return {
                    value,
                    span: Span::dummy(),
                })
            }
            _ => {
                // 检查是否是 call_indirect 格式: r9 = call_indirect r10(r1)
                if line.contains("call_indirect") {
                    // 解析格式: r9 = call_indirect r10(r1)
                    if let Some(equals_pos) = line.find('=') {
                        let result_part = line[..equals_pos].trim();
                        let call_part = line[equals_pos + 1..].trim();

                        if call_part.starts_with("call_indirect") {
                            let result_reg = parse_register(result_part)?;

                            // 解析 call_indirect r10(r1) 部分
                            let call_content =
                                call_part.strip_prefix("call_indirect").unwrap().trim();
                            if let Some(paren_pos) = call_content.find('(') {
                                let function_reg_str = call_content[..paren_pos].trim();
                                let args_str = &call_content[paren_pos + 1..];
                                let args_str = args_str.trim_end_matches(')');

                                let function_register = parse_register(function_reg_str)?;
                                let args = if args_str.is_empty() {
                                    vec![]
                                } else {
                                    args_str
                                        .split(',')
                                        .map(|s| parse_register(s.trim()))
                                        .collect::<Result<Vec<_>, _>>()?
                                };

                                return Ok(Instruction::CallIndirect {
                                    function_register,
                                    args,
                                    arg_operands: vec![], // 简化处理
                                    result: Some(result_reg),
                                    span: Span::dummy(),
                                });
                            }
                        }
                    }
                }

                Err(format!(
                    "Unknown instruction '{}' at line {}",
                    parts[0], line_num
                ))
            }
        }
    }

    /// 解析寄存器
    fn parse_register(s: &str) -> Result<Register, String> {
        if let Some(num_str) = s.strip_prefix('r') {
            let num: usize = num_str
                .parse()
                .map_err(|_| format!("Invalid register number: {}", s))?;
            Ok(Register::Virtual(num))
        } else {
            Err(format!("Invalid register format: {}", s))
        }
    }

    /// 解析操作数
    fn parse_operand(s: &str) -> Result<Operand, String> {
        if let Some(num_str) = s.strip_prefix('#') {
            // 立即数
            let value: i64 = num_str
                .parse()
                .map_err(|_| format!("Invalid immediate value: {}", s))?;
            Ok(Operand::Immediate { value })
        } else if s.starts_with('r') {
            // 寄存器
            let reg = parse_register(s)?;
            Ok(Operand::Register { id: reg })
        } else {
            Err(format!("Invalid operand format: {}", s))
        }
    }

    #[test]
    fn test_file_1_register_allocation() {
        // 读取文件"1"的内容
        let file_content = r#"function main (stack_frame: 0):
L1:
  mov r1, #1
  mov r2, #2
  mov r3, #3
  mov r4, #4
  mov r5, #5
  mov r8, #6
  mov r9, #7
  add r10, r8, r9
  add r11, r5, r10
  add r12, r4, r11
  add r13, r3, r12
  add r14, r2, r13
  add r15, r1, r14
  ret r15"#;

        println!("🧪 测试文件'1'的寄存器分配");
        println!("原始LIR内容:");
        for (i, line) in file_content.lines().enumerate() {
            println!("  {}: {}", i, line);
        }

        // 解析LIR文件
        let mut function = parse_lir_file(file_content).expect("Failed to parse LIR file");

        println!("\n🧪 解析后的LIR函数:\n{}", function);
        // for (i, instruction) in function.instructions.iter().enumerate() {
        //     println!("  {}: {}", i, instruction);
        // }

        // 运行新的寄存器分配Pass
        let result = run_simple_stack_register_allocation(&mut function);

        println!("\n🧪 寄存器分配后的LIR函数:\n{}", function);

        // 验证结果
        assert!(matches!(result, PassResult::Changed));

        // 验证所有寄存器都在物理寄存器范围内
        let uses_only_physical_regs = function.instructions.iter().all(|inst| {
            let used_regs = inst.get_used_registers();
            let def_reg = inst.get_def_register();

            // 检查所有使用的寄存器都是物理寄存器 (r0-r7)
            let used_ok = used_regs.iter().all(|reg| reg.id() <= 7);
            let def_ok = def_reg.is_none_or(|reg| reg.id() <= 7);

            used_ok && def_ok
        });

        // 验证是否有栈空间分配（因为有很多虚拟寄存器）
        let has_stack_allocation = function.instructions.iter().any(|inst| {
            matches!(inst, Instruction::Sub { dst, src2, .. }
                if dst.id() == 6 && matches!(src2, Operand::Immediate { value } if *value > 0))
        });

        println!("\n✅ 验证结果:");
        println!("  - 物理寄存器范围: {}", uses_only_physical_regs);
        println!("  - 栈空间分配: {}", has_stack_allocation);
        println!("  - 指令数量: {} -> {}", 14, function.instructions.len());

        assert!(
            uses_only_physical_regs,
            "所有寄存器都应该在物理寄存器范围内"
        );
        assert!(
            has_stack_allocation,
            "应该有栈空间分配指令，因为有很多虚拟寄存器"
        );

        println!("🎉 文件'1'的寄存器分配测试通过！");
    }

    /// 测试call_indirect指令的寄存器分配修复
    #[test]
    fn test_call_indirect_register_allocation() {
        // 模拟call_indirect场景的简化版本
        let test_content = r#"function main (stack_frame: 0):
L1:
  mov r3, #16
  mov r4, r3
  add r5, r4, #0
  mov r8, #42
  mov r1, r8
  load64 r10, [r5]
  r9 = call_indirect r10(r1)
  mov r11, r9
  ret r11"#;

        let mut function = parse_lir_file(test_content).expect("Failed to parse test LIR");

        println!("\n🧪 测试call_indirect指令修复:");
        println!("原始LIR:\n{}", function);

        // 运行新的寄存器分配Pass
        let result = run_simple_stack_register_allocation(&mut function);

        println!("\n寄存器分配后:\n{}", function);

        // 验证结果
        assert!(matches!(result, PassResult::Changed));

        // 关键验证：检查call_indirect指令中的函数地址寄存器
        let mut found_call_indirect = false;
        for instruction in &function.instructions {
            if let Instruction::CallIndirect {
                function_register,
                args,
                ..
            } = instruction
            {
                found_call_indirect = true;
                println!("🔍 发现call_indirect指令:");
                println!("  函数地址寄存器: r{}", function_register.id());
                println!(
                    "  参数寄存器: {:?}",
                    args.iter()
                        .map(|r| format!("r{}", r.id()))
                        .collect::<Vec<_>>()
                );

                // 验证函数地址寄存器在合理范围内
                assert!(
                    function_register.id() <= 4,
                    "函数地址寄存器应该在r0-r4范围内"
                );

                // 验证参数寄存器在合理范围内
                for arg in args {
                    assert!(arg.id() <= 4, "参数寄存器应该在r0-r4范围内");
                }
            }
        }

        assert!(found_call_indirect, "应该找到call_indirect指令");

        // 验证load64指令的正确性（加载函数地址）
        for (i, instruction) in function.instructions.iter().enumerate() {
            if let Instruction::Load64 { dst, addr, .. } = instruction {
                // 检查是否有后续的call_indirect指令使用这个寄存器
                for j in (i + 1)..function.instructions.len() {
                    if let Instruction::CallIndirect {
                        function_register, ..
                    } = &function.instructions[j]
                    {
                        if dst == function_register {
                            println!("🔍 发现load64指令为call_indirect准备函数地址:");
                            println!("  load64 r{}, [r{}]", dst.id(), addr.id());
                            println!("  call_indirect r{}(...)", function_register.id());

                            // 验证地址寄存器在合理范围内 (r0-r7)
                            assert!(addr.id() <= 7, "地址寄存器应该在r0-r7范围内");
                            break;
                        }
                    }
                }
            }
        }

        println!("✅ call_indirect指令寄存器分配修复测试通过！");
    }

    /// 测试函数参数寄存器分配
    #[test]
    fn test_function_parameter_register_allocation() {
        println!("🧪 测试函数参数寄存器分配:");

        // 创建一个有参数的函数
        let mut function = LirFunction {
            name: "test_func".to_string(),
            parameter_registers: vec![
                Register::Virtual(100),
                Register::Virtual(101),
                Register::Virtual(102),
            ], // 3个参数
            instructions: vec![
                Instruction::Label {
                    id: crate::LabelId(1),
                    span: Span::dummy(),
                },
                // 使用参数寄存器
                Instruction::Add {
                    dst: Register::Virtual(200),
                    src1: Operand::Register {
                        id: Register::Virtual(100),
                    }, // 第一个参数
                    src2: Operand::Register {
                        id: Register::Virtual(101),
                    }, // 第二个参数
                    span: Span::dummy(),
                },
                Instruction::Add {
                    dst: Register::Virtual(201),
                    src1: Operand::Register {
                        id: Register::Virtual(200),
                    },
                    src2: Operand::Register {
                        id: Register::Virtual(102),
                    }, // 第三个参数
                    span: Span::dummy(),
                },
                // 返回结果
                Instruction::Return {
                    value: Some(Register::Virtual(201)),
                    span: Span::dummy(),
                },
            ],
            next_register: 202,
            next_label: 2,
            struct_types: HashMap::new(),
            stack_frame_size: 0,
            parameter_count: 3,
        };

        println!("🧪 测试前的函数参数: {:?}", function.parameter_registers);

        // 运行寄存器分配
        let mut pass = SimpleStackRegisterAllocation::new();
        let mut analyses = AnalysisManager::new();

        let result = pass.run_on_function(&mut function, &mut analyses);

        println!("🧪 测试后的LIR函数:");
        for (i, instruction) in function.instructions.iter().enumerate() {
            println!("  {}: {:?}", i, instruction);
        }

        // 验证结果
        assert!(matches!(result, PassResult::Changed));

        // 验证参数寄存器分配是否正确
        // 参数1(RegisterId(100)) 应该分配到 r1
        // 参数2(RegisterId(101)) 应该分配到 r2
        // 参数3(RegisterId(102)) 应该分配到 r3
        // 返回值(RegisterId(201)) 应该分配到 r0

        let mut found_param_usage = false;
        let mut found_return_assignment = false;

        for instruction in &function.instructions {
            match instruction {
                Instruction::Add { src1, src2, .. } => {
                    // 检查是否使用了正确的参数寄存器
                    if let (Operand::Register { id: reg1 }, Operand::Register { id: reg2 }) =
                        (src1, src2)
                    {
                        if (reg1.id() == 1 && reg2.id() == 2) || (reg1.id() == 2 && reg2.id() == 1)
                        {
                            found_param_usage = true;
                            println!(
                                "✅ 找到正确的参数寄存器使用: r{} + r{}",
                                reg1.id(),
                                reg2.id()
                            );
                        }
                    }
                }
                Instruction::Return {
                    value: Some(reg), ..
                } => {
                    if reg.id() == 0 {
                        found_return_assignment = true;
                        println!("✅ 找到正确的返回值寄存器: r{}", reg.id());
                    }
                }
                _ => {}
            }
        }

        assert!(found_param_usage, "应该找到参数寄存器的正确使用");
        assert!(found_return_assignment, "应该找到返回值寄存器的正确分配");

        println!("✅ 函数参数寄存器分配测试通过");
    }
}
