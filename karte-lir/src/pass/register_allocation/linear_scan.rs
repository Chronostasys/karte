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

use crate::RegisterId;
use super::types::{RegisterLifetime, RegisterAllocationResult, SpillSlot, AllocationStats, SimpleCallingConvention};
use std::collections::{HashMap, HashSet};

/// 线性扫描分配器
/// 
/// 封装了线性扫描算法的核心实现。
pub struct LinearScanAllocator {
    calling_convention: SimpleCallingConvention,
    reserved_registers: HashSet<u8>,
}

impl LinearScanAllocator {
    /// 创建新的线性扫描分配器
    pub fn new(calling_convention: SimpleCallingConvention) -> Self {
        let mut reserved = HashSet::new();
        reserved.insert(calling_convention.stack_pointer);
        reserved.insert(calling_convention.frame_pointer);
        reserved.insert(calling_convention.return_address);
        
        Self {
            calling_convention,
            reserved_registers: reserved,
        }
    }
    
    /// 执行线性扫描寄存器分配
    /// 
    /// 这是分配算法的主入口点。
    /// @param lifetimes - 所有虚拟寄存器的生命周期信息。
    /// @param stack_address_registers - 不可被溢出的栈地址寄存器集合。
    /// @returns 分配结果。
    pub fn allocate(
        &self, 
        mut lifetimes: Vec<RegisterLifetime>, 
        stack_address_registers: &HashSet<RegisterId>
    ) -> RegisterAllocationResult {
        lifetimes.sort_by(|a, b| a.start.cmp(&b.start));
        
        let mut register_mapping = HashMap::new();
        let mut spilled_registers = HashMap::new();
        let mut active_intervals: Vec<RegisterLifetime> = Vec::new();
        let mut available_registers = self.get_available_physical_registers();
        let mut spill_slot_counter = 0;
        let mut max_register_pressure = 0;
        
        // 🔧 修复：预分配函数参数寄存器
        for lifetime in &lifetimes {
            if lifetime.is_function_parameter {
                if let Some(param_index) = lifetime.parameter_index {
                    if let Some(&physical_reg) = self.calling_convention.argument_registers.get(param_index) {
                        register_mapping.insert(lifetime.register, physical_reg);
                        println!("🔧 预分配函数参数: {:?} -> r{} (参数索引: {})", lifetime.register, physical_reg, param_index);
                        available_registers.retain(|&reg| reg != physical_reg);
                        active_intervals.push(lifetime.clone());
                        continue;
                    }
                }
            }
        }
        
        // 🔧 修复：优先分配stack_address_registers，保证它们一定有映射
        for lifetime in &lifetimes {
            if stack_address_registers.contains(&lifetime.register) && !register_mapping.contains_key(&lifetime.register) {
                if let Some(physical_reg) = available_registers.pop() {
                    register_mapping.insert(lifetime.register, physical_reg);
                    println!("🔧 预分配栈地址寄存器: {:?} -> r{}", lifetime.register, physical_reg);
                    active_intervals.push(lifetime.clone());
                } else {
                    // 没有可用物理寄存器，必须溢出
                    spilled_registers.insert(lifetime.register, SpillSlot { slot_id: spill_slot_counter });
                    println!("🔧 溢出栈地址寄存器: {:?} -> slot_{}", lifetime.register, spill_slot_counter);
                    spill_slot_counter += 1;
                }
            }
        }
        
        for current_lifetime in &lifetimes {
            // 跳过已经预分配的函数参数和stack_address_registers
            if current_lifetime.is_function_parameter || stack_address_registers.contains(&current_lifetime.register) {
                continue;
            }
            
            self.expire_old_intervals(&mut active_intervals, &mut available_registers, &register_mapping, current_lifetime.start);
            max_register_pressure = max_register_pressure.max(active_intervals.len());

            if let Some(physical_reg) = available_registers.pop() {
                debug_assert!(!self.reserved_registers.contains(&physical_reg), "分配到保留寄存器: r{}", physical_reg);
                register_mapping.insert(current_lifetime.register, physical_reg);
                active_intervals.push(current_lifetime.clone());
            } else {
                self.spill_at_interval(&mut active_intervals, current_lifetime, &mut register_mapping, &mut spilled_registers, &mut spill_slot_counter, stack_address_registers);
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
        }
    }
    
    /// 释放已结束的活跃区间，并回收它们占用的物理寄存器
    fn expire_old_intervals(
        &self,
        active_intervals: &mut Vec<RegisterLifetime>,
        available_registers: &mut Vec<u8>,
        register_mapping: &HashMap<RegisterId, u8>,
        current_position: usize,
    ) {
        let mut i = 0;
        while i < active_intervals.len() {
            if active_intervals[i].end < current_position {
                let ended = active_intervals.remove(i);
                if let Some(&physical_reg) = register_mapping.get(&ended.register) {
                    if !self.reserved_registers.contains(&physical_reg) {
                        available_registers.push(physical_reg);
                    }
                }
            } else {
                i += 1;
            }
        }
    }

    /// 处理寄存器溢出
    fn spill_at_interval(
        &self,
        active_intervals: &mut Vec<RegisterLifetime>,
        current: &RegisterLifetime,
        register_mapping: &mut HashMap<RegisterId, u8>,
        spilled_registers: &mut HashMap<RegisterId, SpillSlot>,
        spill_slot_counter: &mut usize,
        stack_address_registers: &HashSet<RegisterId>,
    ) {
        if let Some(spill_candidate) = self.find_spill_candidate(active_intervals, current, stack_address_registers) {
            if spill_candidate.end > current.end {
                let freed_reg = register_mapping.remove(&spill_candidate.register).unwrap();
                register_mapping.insert(current.register, freed_reg);
                spilled_registers.insert(spill_candidate.register, SpillSlot { slot_id: *spill_slot_counter });
                *spill_slot_counter += 1;
                let r = spill_candidate.register;
                active_intervals.retain(|i| i.register != r);
                active_intervals.push(current.clone());
            } else {
                spilled_registers.insert(current.register, SpillSlot { slot_id: *spill_slot_counter });
                *spill_slot_counter += 1;
            }
        } else {
            if stack_address_registers.contains(&current.register) {
                panic!("无法溢出栈地址寄存器 {:?}，分配失败", current.register);
            }
            spilled_registers.insert(current.register, SpillSlot { slot_id: *spill_slot_counter });
            *spill_slot_counter += 1;
        }
    }
    
    /// 寻找最佳的溢出候选者
    fn find_spill_candidate<'a>(
        &self,
        active_intervals: &'a [RegisterLifetime],
        current_lifetime: &RegisterLifetime,
        stack_address_registers: &HashSet<RegisterId>,
    ) -> Option<&'a RegisterLifetime> {
        let mut furthest_use = 0;
        let mut spill_candidate = None;

        for interval in active_intervals {
            if stack_address_registers.contains(&interval.register) {
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
    fn find_next_use(&self, lifetime: &RegisterLifetime, from_position: usize) -> usize {
        lifetime.uses.iter().find(|&&use_pos| use_pos >= from_position).copied().unwrap_or(usize::MAX)
    }
    
    /// 获取可用的物理寄存器列表
    fn get_available_physical_registers(&self) -> Vec<u8> {
        self.calling_convention.allocatable_registers.clone()
    }
} 