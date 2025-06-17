//! 线性扫描寄存器分配算法
//! 
//! 实现高效的线性扫描寄存器分配，支持：
//! - 生命周期排序
//! - 活跃区间管理
//! - 寄存器回收
//! - 溢出决策

use super::lifetime_analysis::{RegisterLifetime, CallSiteContext};
use super::super::calling_convention::{CallingConvention, PhysicalRegister};
use karte_lir::RegisterId;
use std::collections::{HashMap, BinaryHeap};
use std::cmp::Reverse;

/// 活跃区间信息
#[derive(Debug, Clone, PartialEq)]
pub struct ActiveInterval {
    /// 生命周期
    pub lifetime: RegisterLifetime,
    /// 分配的物理寄存器
    pub physical_register: PhysicalRegister,
}

impl ActiveInterval {
    /// 创建新的活跃区间
    pub fn new(lifetime: RegisterLifetime, physical_register: PhysicalRegister) -> Self {
        Self {
            lifetime,
            physical_register,
        }
    }
}

/// 溢出候选者
#[derive(Debug, Clone, PartialEq)]
pub struct SpillCandidate {
    /// 虚拟寄存器
    pub virtual_register: RegisterId,
    /// 物理寄存器
    pub physical_register: PhysicalRegister,
    /// 溢出成本
    pub spill_cost: f64,
    /// 生命周期结束位置
    pub end_position: usize,
}

impl Eq for SpillCandidate {}

impl PartialOrd for SpillCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SpillCandidate {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // 按照溢出成本排序，成本低的优先溢出
        self.spill_cost.partial_cmp(&other.spill_cost)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(other.end_position.cmp(&self.end_position))
    }
}

/// 线性扫描寄存器分配器
#[derive(Debug)]
pub struct LinearScanAllocator {
    /// 可用的物理寄存器池
    available_registers: Vec<PhysicalRegister>,
    /// 当前活跃的区间
    active_intervals: Vec<ActiveInterval>,
    /// 分配结果
    allocation: HashMap<RegisterId, PhysicalRegister>,
    /// 需要溢出的寄存器
    spill_candidates: BinaryHeap<SpillCandidate>,
    /// 寄存器压力统计
    max_register_pressure: usize,
    /// 当前寄存器压力
    current_pressure: usize,
}

impl LinearScanAllocator {
    /// 创建新的线性扫描分配器
    pub fn new(available_registers: Vec<PhysicalRegister>) -> Self {
        Self {
            available_registers,
            active_intervals: Vec::new(),
            allocation: HashMap::new(),
            spill_candidates: BinaryHeap::new(),
            max_register_pressure: 0,
            current_pressure: 0,
        }
    }

    /// 执行寄存器分配
    pub fn allocate(&mut self, lifetimes: &[RegisterLifetime]) -> Result<(HashMap<RegisterId, PhysicalRegister>, Vec<RegisterId>), String> {
        // 重置状态
        self.reset();
        
        // 按生命周期开始位置排序
        let mut sorted_lifetimes = lifetimes.to_vec();
        sorted_lifetimes.sort_by_key(|lt| lt.start);
        
        // 逐个分配寄存器
        for lifetime in sorted_lifetimes {
            self.allocate_register(&lifetime)?;
        }
        
        // 提取溢出的寄存器
        let spilled_registers = self.spill_candidates.iter()
            .map(|candidate| candidate.virtual_register)
            .collect();
        
        Ok((self.allocation.clone(), spilled_registers))
    }

    /// 重置分配器状态
    fn reset(&mut self) {
        self.active_intervals.clear();
        self.allocation.clear();
        self.spill_candidates.clear();
        self.max_register_pressure = 0;
        self.current_pressure = 0;
    }

    /// 为单个寄存器分配物理寄存器
    fn allocate_register(&mut self, lifetime: &RegisterLifetime) -> Result<(), String> {
        // 1. 清理过期的活跃区间
        self.expire_old_intervals(lifetime.start);
        
        // 2. 尝试分配可用寄存器
        if let Some(physical_reg) = self.get_available_register() {
            self.assign_register(lifetime.clone(), physical_reg);
            return Ok(());
        }
        
        // 3. 如果没有可用寄存器，选择一个进行溢出
        self.handle_spill(lifetime)?;
        
        Ok(())
    }

    /// 清理过期的活跃区间
    fn expire_old_intervals(&mut self, current_position: usize) {
        // 找出所有已经结束的区间
        let mut expired_indices = Vec::new();
        for (i, interval) in self.active_intervals.iter().enumerate() {
            if interval.lifetime.end < current_position {
                expired_indices.push(i);
            }
        }
        
        // 从后往前移除，避免索引变化
        expired_indices.reverse();
        for index in expired_indices {
            let expired = self.active_intervals.swap_remove(index);
            // 回收物理寄存器
            self.available_registers.push(expired.physical_register);
            self.current_pressure -= 1;
        }
    }

    /// 获取可用的物理寄存器
    fn get_available_register(&mut self) -> Option<PhysicalRegister> {
        self.available_registers.pop()
    }

    /// 分配寄存器
    fn assign_register(&mut self, lifetime: RegisterLifetime, physical_reg: PhysicalRegister) {
        // 记录分配结果
        self.allocation.insert(lifetime.register, physical_reg);
        
        // 添加到活跃区间
        let active_interval = ActiveInterval::new(lifetime, physical_reg);
        self.active_intervals.push(active_interval);
        
        // 更新寄存器压力统计
        self.current_pressure += 1;
        self.max_register_pressure = self.max_register_pressure.max(self.current_pressure);
    }

    /// 处理寄存器溢出
    fn handle_spill(&mut self, lifetime: &RegisterLifetime) -> Result<(), String> {
        // 找到最适合溢出的寄存器
        if let Some(victim) = self.select_spill_candidate(lifetime) {
            // 将被选中的寄存器溢出
            self.spill_register(&victim);
            
            // 为当前寄存器分配被释放的物理寄存器
            self.assign_register(lifetime.clone(), victim.physical_register);
        } else {
            // 如果找不到合适的溢出候选者，直接溢出当前寄存器
            let spill_candidate = SpillCandidate {
                virtual_register: lifetime.register,
                physical_register: 0, // 无物理寄存器
                spill_cost: self.calculate_spill_cost(lifetime),
                end_position: lifetime.end,
            };
            self.spill_candidates.push(spill_candidate);
        }
        
        Ok(())
    }

    /// 选择溢出候选者
    fn select_spill_candidate(&self, current_lifetime: &RegisterLifetime) -> Option<SpillCandidate> {
        let mut best_candidate = None;
        let mut best_cost = f64::INFINITY;
        
        for interval in &self.active_intervals {
            // 只考虑在当前寄存器生命周期结束后才结束的寄存器
            if interval.lifetime.end > current_lifetime.end {
                let cost = self.calculate_spill_cost(&interval.lifetime);
                if cost < best_cost {
                    best_cost = cost;
                    best_candidate = Some(SpillCandidate {
                        virtual_register: interval.lifetime.register,
                        physical_register: interval.physical_register,
                        spill_cost: cost,
                        end_position: interval.lifetime.end,
                    });
                }
            }
        }
        
        // 只有当找到的候选者的溢出成本低于当前寄存器时才溢出
        let current_cost = self.calculate_spill_cost(current_lifetime);
        if best_cost < current_cost {
            best_candidate
        } else {
            None
        }
    }

    /// 计算溢出成本
    fn calculate_spill_cost(&self, lifetime: &RegisterLifetime) -> f64 {
        let mut cost = 1.0;
        
        // 使用频率影响成本
        cost *= lifetime.weight;
        
        // 循环中的寄存器溢出成本更高
        if lifetime.in_loop {
            cost *= 10.0;
        }
        
        // 生命周期长的寄存器溢出成本更高
        cost += lifetime.length() as f64 * 0.1;
        
        // 参数和返回值寄存器溢出成本更高
        if lifetime.is_parameter || lifetime.is_return_value {
            cost *= 2.0;
        }
        
        cost
    }

    /// 执行寄存器溢出
    fn spill_register(&mut self, candidate: &SpillCandidate) {
        // 从活跃区间中移除
        if let Some(pos) = self.active_intervals.iter()
            .position(|interval| interval.lifetime.register == candidate.virtual_register) {
            self.active_intervals.swap_remove(pos);
        }
        
        // 从分配表中移除
        self.allocation.remove(&candidate.virtual_register);
        
        // 添加到溢出候选者列表
        self.spill_candidates.push(candidate.clone());
        
        // 更新寄存器压力
        self.current_pressure -= 1;
    }

    /// 优化函数调用点的寄存器分配
    pub fn optimize_call_site(&mut self, call_context: &CallSiteContext) -> Result<(), String> {
        // 确保参数寄存器在调用点可用
        for (i, &arg_reg) in call_context.arguments.iter().enumerate() {
            if let Some(&physical_reg) = self.allocation.get(&arg_reg) {
                // 检查参数寄存器是否符合调用约定
                // 这里可以添加寄存器移动指令来满足调用约定
                println!("Parameter {} (virtual {:?}) is in physical r{}", 
                        i, arg_reg, physical_reg);
            }
        }
        
        Ok(())
    }

    /// 获取总寄存器数
    pub fn get_total_registers(&self) -> usize {
        self.available_registers.len() + self.active_intervals.len()
    }

    /// 获取已分配寄存器数
    pub fn get_allocated_count(&self) -> usize {
        self.allocation.len()
    }

    /// 获取寄存器压力
    pub fn get_register_pressure(&self) -> f64 {
        if self.get_total_registers() > 0 {
            self.max_register_pressure as f64 / self.get_total_registers() as f64
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::lifetime_analysis::RegisterLifetime;

    #[test]
    fn test_linear_scan_basic() {
        let mut allocator = LinearScanAllocator::new(vec![0, 1, 2, 3]);
        
        let lifetimes = vec![
            RegisterLifetime::new(RegisterId(1), 0, 3),
            RegisterLifetime::new(RegisterId(2), 1, 4),
            RegisterLifetime::new(RegisterId(3), 2, 5),
        ];
        
        let (allocation, spills) = allocator.allocate(&lifetimes).unwrap();
        
        assert_eq!(allocation.len(), 3);
        assert!(spills.is_empty());
    }

    #[test]
    fn test_spill_selection() {
        let mut allocator = LinearScanAllocator::new(vec![0, 1]);
        
        let lifetimes = vec![
            RegisterLifetime::new(RegisterId(1), 0, 10),
            RegisterLifetime::new(RegisterId(2), 1, 11),
            RegisterLifetime::new(RegisterId(3), 2, 5), // 短生命周期，不应该溢出长的
        ];
        
        let (allocation, spills) = allocator.allocate(&lifetimes).unwrap();
        
        assert_eq!(allocation.len(), 2);
        assert_eq!(spills.len(), 1);
    }

    #[test]
    fn test_spill_cost_calculation() {
        let allocator = LinearScanAllocator::new(vec![0, 1, 2, 3]);
        
        let mut lifetime1 = RegisterLifetime::new(RegisterId(1), 0, 10);
        lifetime1.weight = 5.0;
        lifetime1.in_loop = true;
        
        let mut lifetime2 = RegisterLifetime::new(RegisterId(2), 0, 10);
        lifetime2.weight = 1.0;
        lifetime2.in_loop = false;
        
        let cost1 = allocator.calculate_spill_cost(&lifetime1);
        let cost2 = allocator.calculate_spill_cost(&lifetime2);
        
        assert!(cost1 > cost2); // 循环中的寄存器溢出成本更高
    }
} 