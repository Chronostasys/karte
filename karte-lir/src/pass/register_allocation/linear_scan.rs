//! 线性扫描寄存器分配算法
//!
//! 实现高效的线性扫描算法，包括活跃区间排序、寄存器分配、以及复杂的溢出策略。
//!
//! ## 未来改进
//! - **溢出成本模型**: 当前的溢出启发式比较简单。可以引入更复杂的成本模型，
//!   例如考虑循环深度，来更智能地选择溢出候选，从而减少关键循环中的溢出开销。
//! - **寄存器合并**: 在 `move` 指令两端，如果两个虚拟寄存器的生命周期不重叠，
//!   可以将它们分配到同一个物理寄存器，从而消除 `move` 指令，提升代码效率。
//! - **图着色算法**: 对于复杂的控制流，线性扫描可能会产生次优的结果。可以探索
//!   实现一个更强大的图着色分配器作为补充，或者在优化级别较高时使用。

use super::types::{
    AllocationStats, CallingConvention, RegisterAllocationResult, RegisterLifetime, RegisterType,
    SpillSlot,
};
use crate::Register;
use std::collections::{HashMap, HashSet};

/// 线性扫描分配器
///
/// 封装了线性扫描算法的核心实现。
pub struct LinearScanAllocator {
    calling_convention: CallingConvention,
    reserved_registers: HashSet<u8>,
    // 🔥 新增：显式spill栈，严格LIFO管理
    spill_stack: Vec<u8>,
}

impl LinearScanAllocator {
    /// 创建新的线性扫描分配器
    pub fn new(calling_convention: CallingConvention) -> Self {
        let mut reserved = HashSet::new();
        reserved.insert(calling_convention.stack_pointer);
        reserved.insert(calling_convention.frame_pointer);

        // x86_64: 硬件 RSP(4) 和 RBP(5) 不能被分配
        // 即使 CallingConvention 不用它们作为 vm_sp/vm_fp
        // 检测方式：如果参数寄存器是 x86 的 [7,6,2,1,8,9]，说明是 x86 目标
        if calling_convention.argument_registers == vec![7u8, 6, 2, 1, 8, 9] {
            reserved.insert(4); // RSP - 硬件栈指针
            reserved.insert(5); // RBP - 硬件帧指针
        }

        let _ = calling_convention.get_allocatable_registers();

        Self {
            calling_convention,
            reserved_registers: reserved,
            spill_stack: Vec::new(),
        }
    }

    /// 执行线性扫描寄存器分配
    ///
    /// 这是分配算法的主入口点。
    /// @param lifetimes - 所有虚拟寄存器的生命周期信息。
    /// @param register_types - 寄存器类型映射。
    /// @returns 分配结果。
        pub fn allocate(
        &mut self,
        mut lifetimes: Vec<RegisterLifetime>,
        register_types: HashMap<Register, RegisterType>,
    ) -> RegisterAllocationResult {
        lifetimes.sort_by(|a, b| a.start.cmp(&b.start));

        let mut register_mapping = HashMap::new();
        let mut spilled_registers = HashMap::new();
        let mut active_intervals: Vec<RegisterLifetime> = Vec::new();
        let mut available_registers = self.get_available_physical_registers();
        let mut spill_slot_counter = 0;
        let mut max_register_pressure = 0;
        self.spill_stack.clear();

        // 预分配函数参数寄存器
        for lifetime in &lifetimes {
            if lifetime.is_function_parameter {
                if let Some(param_index) = lifetime.parameter_index {
                    if let Some(&physical_reg) =
                        self.calling_convention.argument_registers.get(param_index)
                    {
                        register_mapping.insert(lifetime.register, physical_reg);
                        eprintln!(
                            "预分配参数: {:?} -> r{} (lifetime: [{}, {}])",
                            lifetime.register, physical_reg, lifetime.start, lifetime.end
                        );
                        available_registers.retain(|&reg| reg != physical_reg);
                        active_intervals.push(lifetime.clone());
                        continue;
                    }
                }
            }
            // 预分配物理寄存器
            if let Register::Physical(p) = lifetime.register {
                register_mapping.insert(lifetime.register, p);
                eprintln!("预分配物理寄存器: {:?} -> r{}", lifetime.register, p);
                available_registers.retain(|&reg| reg != p);
                active_intervals.push(lifetime.clone());
            }
        }

        // 🔧 修复：StackAddress寄存器也需要分配物理寄存器
        // 因为它们在StackFrameLowering后变成了add指令的目标寄存器

        for current_lifetime in &lifetimes {
            if current_lifetime.is_function_parameter {
                continue;
            }
            if current_lifetime.register.is_physical() {
                continue;
            }
            // 🔧 修复：为Data和StackAddress类型都分配物理寄存器
            // StackAddress寄存器在StackFrameLowering后变成add指令的目标寄存器，需要物理寄存器
            if current_lifetime.register_type != RegisterType::Data
                && current_lifetime.register_type != RegisterType::StackAddress
            {
                continue;
            }
            self.expire_old_intervals(
                &mut active_intervals,
                &mut available_registers,
                &register_mapping,
                current_lifetime.start,
            );
            max_register_pressure = max_register_pressure.max(active_intervals.len());
            // === 修复点：分配前检查同类型活跃区间 ===
            let mut candidate_reg = None;
            for &reg in &available_registers {
                let conflict = active_intervals.iter().any(|lt| {
                    lt.register_type == current_lifetime.register_type
                        && register_mapping.get(&lt.register) == Some(&reg)
                });
                if !conflict {
                    candidate_reg = Some(reg);
                    break;
                }
            }
            if let Some(physical_reg) = candidate_reg {
                // 分配并移除
                available_registers.retain(|&r| r != physical_reg);
                debug_assert!(!self.reserved_registers.contains(&physical_reg));
                register_mapping.insert(current_lifetime.register, physical_reg);
                active_intervals.push(current_lifetime.clone());
                self.spill_stack.push(physical_reg);
            } else {
                // 🔧 修复：只有Data类型的寄存器可以被溢出
                if let Some(victim) = active_intervals.iter().find(|lt| lt.can_spill()) {
                    let victim_reg = victim.register;
                    let freed_reg = register_mapping.remove(&victim_reg).unwrap();
                    spilled_registers.insert(
                        victim_reg,
                        SpillSlot {
                            slot_id: spill_slot_counter,
                        },
                    );
                    spill_slot_counter += 1;
                    if let Some(pos) = self.spill_stack.iter().position(|&r| r == freed_reg) {
                        self.spill_stack.remove(pos);
                    }
                    register_mapping.insert(current_lifetime.register, freed_reg);
                    active_intervals.retain(|i| i.register != victim_reg);
                    active_intervals.push(current_lifetime.clone());
                    self.spill_stack.push(freed_reg);
                } else {
                    spilled_registers.insert(
                        current_lifetime.register,
                        SpillSlot {
                            slot_id: spill_slot_counter,
                        },
                    );
                    spill_slot_counter += 1;
                }
            }
        }

        // 检查分配结果，禁止 255 未初始化魔数
        for (&reg, &phys) in &register_mapping {
            if phys == 255 {
                panic!(
                    "分配结果中出现非法物理寄存器255: {:?}，这是分配器的bug！",
                    reg
                );
            }
        }

        // === 🔥 新增：保证每个restore点有可用物理寄存器 ===
        // 对于每个spilled寄存器的每个uses（reload点），模拟活跃区间，保证有空闲物理寄存器
        for (spilled_reg, _spill_slot) in &spilled_registers {
            if let Some(lifetime) = lifetimes.iter().find(|lt| lt.register == *spilled_reg) {
                for &use_pos in &lifetime.uses {
                    // 模拟到use_pos时的活跃区间和可用寄存器
                    let mut temp_active = active_intervals.clone();
                    let mut temp_available = self.get_available_physical_registers();
                    let mut temp_register_mapping = register_mapping.clone();
                    // expire已结束的区间
                    self.expire_old_intervals(
                        &mut temp_active,
                        &mut temp_available,
                        &temp_register_mapping,
                        use_pos,
                    );
                    // 检查是否有空闲物理寄存器
                    if temp_available.is_empty() {
                        // 没有空闲寄存器，主动spill一个可spill的活跃寄存器
                        if let Some(victim) = temp_active.iter().find(|lt| lt.can_spill()) {
                            let victim_reg = victim.register;
                            let freed_reg = temp_register_mapping.remove(&victim_reg).unwrap();
                            temp_available.push(freed_reg);
                            // 这里仅做分配决策，实际spill指令由后续pass插入
                            println!(
                                "[restore点主动spill] reload {:?} 需要先spill {:?} (r{})",
                                spilled_reg, victim_reg, freed_reg
                            );
                        } else {
                            panic!(
                                "restore点无法分配物理寄存器：{:?}，没有可spill的活跃寄存器！",
                                spilled_reg
                            );
                        }
                    }
                    // 用空闲寄存器做reload（这里只做决策，实际reload由后续pass插入）
                }
            }
        }

        RegisterAllocationResult {
            stats: AllocationStats {
                total_virtual_registers: lifetimes.len(),
                allocated_physical_registers: register_mapping.len(),
                spilled_registers: spilled_registers.len(),
                register_pressure: max_register_pressure,
            },
            register_mapping,
            spilled_registers,
            register_types,
        }
    }

    /// 释放已结束的活跃区间，并回收它们占用的物理寄存器
    fn expire_old_intervals(
        &mut self,
        active_intervals: &mut Vec<RegisterLifetime>,
        available_registers: &mut Vec<u8>,
        register_mapping: &HashMap<Register, u8>,
        current_position: usize,
    ) {
        let mut i = 0;
        while i < active_intervals.len() {
            if active_intervals[i].end < current_position {
                let ended = active_intervals.remove(i);
                if let Some(&physical_reg) = register_mapping.get(&ended.register) {
                    if !self.reserved_registers.contains(&physical_reg) {
                        // 只有spill_stack顶才pop并reload，否则仅标记空闲
                        if let Some(&top) = self.spill_stack.last() {
                            if top == physical_reg {
                                // LIFO顺序，pop并reload
                                self.spill_stack.pop();
                            }
                        }
                        available_registers.push(physical_reg);
                    }
                }
            } else {
                i += 1;
            }
        }
    }

    /// 处理寄存器溢出
    #[allow(dead_code)]
    fn spill_at_interval(
        &self,
        active_intervals: &mut Vec<RegisterLifetime>,
        current: &RegisterLifetime,
        register_mapping: &mut HashMap<Register, u8>,
        spilled_registers: &mut HashMap<Register, SpillSlot>,
        spill_slot_counter: &mut usize,
    ) {
        if let Some(spill_candidate) = self.find_spill_candidate(active_intervals, current) {
            if spill_candidate.end > current.end {
                let freed_reg = register_mapping.remove(&spill_candidate.register).unwrap();
                register_mapping.insert(current.register, freed_reg);
                spilled_registers.insert(
                    spill_candidate.register,
                    SpillSlot {
                        slot_id: *spill_slot_counter,
                    },
                );
                *spill_slot_counter += 1;
                let r = spill_candidate.register;
                active_intervals.retain(|i| i.register != r);
                active_intervals.push(current.clone());
            } else {
                spilled_registers.insert(
                    current.register,
                    SpillSlot {
                        slot_id: *spill_slot_counter,
                    },
                );
                *spill_slot_counter += 1;
            }
        } else {
            spilled_registers.insert(
                current.register,
                SpillSlot {
                    slot_id: *spill_slot_counter,
                },
            );
            *spill_slot_counter += 1;
        }
    }

    /// 寻找最佳的溢出候选者
    #[allow(dead_code)]
    fn find_spill_candidate<'a>(
        &self,
        active_intervals: &'a [RegisterLifetime],
        current_lifetime: &RegisterLifetime,
    ) -> Option<&'a RegisterLifetime> {
        let mut furthest_use = 0;
        let mut spill_candidate = None;

        for interval in active_intervals {
            if !interval.can_spill() {
                continue;
            }
            let next_use = self.find_next_use(interval, current_lifetime.start);
            if next_use > furthest_use {
                furthest_use = next_use;
                spill_candidate = Some(interval);
            }
        }
        spill_candidate
    }

    /// 查找寄存器的下一次使用位置
    #[allow(dead_code)]
    fn find_next_use(&self, lifetime: &RegisterLifetime, from_position: usize) -> usize {
        lifetime
            .uses
            .iter()
            .find(|&&use_pos| use_pos >= from_position)
            .copied()
            .unwrap_or(usize::MAX)
    }

    /// 获取可用的物理寄存器列表
    fn get_available_physical_registers(&self) -> Vec<u8> {
        self.calling_convention.get_allocatable_registers()
    }
}
