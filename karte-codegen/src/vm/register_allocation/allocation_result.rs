//! 寄存器分配结果
//! 
//! 包含寄存器分配的完整结果信息，用于虚拟机执行。

use super::lifetime_analysis::RegisterLifetime;
use super::super::calling_convention::{CallingConvention, PhysicalRegister};
use karte_lir::RegisterId;
use std::collections::HashMap;

/// 寄存器分配结果
#[derive(Debug, Clone)]
pub struct AllocationResult {
    /// 虚拟寄存器到物理寄存器的映射
    pub register_mapping: HashMap<RegisterId, PhysicalRegister>,
    /// 被溢出的寄存器列表
    pub spilled_registers: Vec<RegisterId>,
    /// 寄存器生命周期信息
    pub lifetimes: Vec<RegisterLifetime>,
    /// 使用的调用约定
    pub calling_convention: CallingConvention,
}

impl AllocationResult {
    /// 创建新的分配结果
    pub fn new(calling_convention: CallingConvention) -> Self {
        Self {
            register_mapping: HashMap::new(),
            spilled_registers: Vec::new(),
            lifetimes: Vec::new(),
            calling_convention,
        }
    }

    /// 获取虚拟寄存器对应的物理寄存器
    pub fn get_physical_register(&self, virtual_reg: &RegisterId) -> Option<PhysicalRegister> {
        self.register_mapping.get(virtual_reg).copied()
    }

    /// 检查寄存器是否被溢出
    pub fn is_spilled(&self, virtual_reg: &RegisterId) -> bool {
        self.spilled_registers.contains(virtual_reg)
    }

    /// 获取分配的寄存器数量
    pub fn allocated_count(&self) -> usize {
        self.register_mapping.len()
    }

    /// 获取溢出的寄存器数量
    pub fn spilled_count(&self) -> usize {
        self.spilled_registers.len()
    }

    /// 获取分配效率（分配的寄存器占总寄存器的比例）
    pub fn allocation_efficiency(&self) -> f64 {
        let total = self.allocated_count() + self.spilled_count();
        if total > 0 {
            self.allocated_count() as f64 / total as f64
        } else {
            1.0
        }
    }

    /// 获取寄存器使用统计
    pub fn get_register_usage_stats(&self) -> RegisterUsageStats {
        let mut physical_reg_usage = HashMap::new();
        
        // 统计每个物理寄存器的使用次数
        for &physical_reg in self.register_mapping.values() {
            *physical_reg_usage.entry(physical_reg).or_insert(0) += 1;
        }

        // 计算寄存器压力
        let max_simultaneous_usage = self.calculate_max_simultaneous_usage();
        let available_registers = self.calling_convention.get_allocatable_registers().len();
        let register_pressure = if available_registers > 0 {
            max_simultaneous_usage as f64 / available_registers as f64
        } else {
            0.0
        };

        RegisterUsageStats {
            allocated_registers: self.allocated_count(),
            spilled_registers: self.spilled_count(),
            available_registers,
            register_pressure,
            physical_register_usage: physical_reg_usage,
        }
    }

    /// 计算最大同时使用的寄存器数量
    fn calculate_max_simultaneous_usage(&self) -> usize {
        if self.lifetimes.is_empty() {
            return 0;
        }

        // 收集所有时间点
        let mut events = Vec::new();
        for lifetime in &self.lifetimes {
            if self.register_mapping.contains_key(&lifetime.register) {
                events.push((lifetime.start, 1)); // 开始使用
                events.push((lifetime.end + 1, -1i32)); // 结束使用
            }
        }

        // 按时间排序
        events.sort_by_key(|&(time, _)| time);

        // 扫描事件，计算最大同时使用量
        let mut current_usage = 0;
        let mut max_usage = 0;

        for (_, delta) in events {
            current_usage += delta;
            max_usage = max_usage.max(current_usage);
        }

        max_usage as usize
    }

    /// 验证分配结果的正确性
    pub fn validate(&self) -> Result<(), String> {
        // 检查映射的一致性
        for (&virtual_reg, &physical_reg) in &self.register_mapping {
            // 确保物理寄存器在可分配范围内
            let allocatable_regs = self.calling_convention.get_allocatable_registers();
            if !allocatable_regs.contains(&physical_reg) {
                return Err(format!(
                    "Virtual register {:?} mapped to non-allocatable physical register r{}", 
                    virtual_reg, physical_reg
                ));
            }

            // 确保溢出的寄存器没有物理寄存器映射
            if self.spilled_registers.contains(&virtual_reg) {
                return Err(format!(
                    "Virtual register {:?} is both allocated and spilled", 
                    virtual_reg
                ));
            }
        }

        // 检查生命周期重叠的寄存器是否分配到不同的物理寄存器
        for i in 0..self.lifetimes.len() {
            for j in i + 1..self.lifetimes.len() {
                let lt1 = &self.lifetimes[i];
                let lt2 = &self.lifetimes[j];

                if lt1.overlaps_with(lt2) {
                    if let (Some(&phys1), Some(&phys2)) = (
                        self.register_mapping.get(&lt1.register),
                        self.register_mapping.get(&lt2.register)
                    ) {
                        if phys1 == phys2 {
                            return Err(format!(
                                "Overlapping virtual registers {:?} and {:?} both mapped to physical register r{}",
                                lt1.register, lt2.register, phys1
                            ));
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// 打印分配结果摘要
    pub fn print_summary(&self) {
        println!("=== Register Allocation Summary ===");
        println!("Allocated registers: {}", self.allocated_count());
        println!("Spilled registers: {}", self.spilled_count());
        println!("Allocation efficiency: {:.2}%", self.allocation_efficiency() * 100.0);

        if !self.register_mapping.is_empty() {
            println!("\nRegister Mapping:");
            let mut sorted_mapping: Vec<_> = self.register_mapping.iter().collect();
            sorted_mapping.sort_by_key(|(virtual_reg, _)| virtual_reg.0);
            
            for (&virtual_reg, &physical_reg) in sorted_mapping {
                println!("  {:?} -> r{}", virtual_reg, physical_reg);
            }
        }

        if !self.spilled_registers.is_empty() {
            println!("\nSpilled Registers:");
            for &reg in &self.spilled_registers {
                println!("  {:?}", reg);
            }
        }

        let stats = self.get_register_usage_stats();
        println!("\nRegister Pressure: {:.2}", stats.register_pressure);
        println!("Available Physical Registers: {}", stats.available_registers);
    }
}

/// 寄存器使用统计
#[derive(Debug, Clone)]
pub struct RegisterUsageStats {
    /// 分配的寄存器数量
    pub allocated_registers: usize,
    /// 溢出的寄存器数量
    pub spilled_registers: usize,
    /// 可用的物理寄存器数量
    pub available_registers: usize,
    /// 寄存器压力 (0.0-1.0)
    pub register_pressure: f64,
    /// 每个物理寄存器的使用次数
    pub physical_register_usage: HashMap<PhysicalRegister, usize>,
}

impl RegisterUsageStats {
    /// 获取最常用的物理寄存器
    pub fn most_used_register(&self) -> Option<(PhysicalRegister, usize)> {
        self.physical_register_usage
            .iter()
            .max_by_key(|(_, &count)| count)
            .map(|(&reg, &count)| (reg, count))
    }

    /// 获取平均每个物理寄存器的使用次数
    pub fn average_usage_per_register(&self) -> f64 {
        if self.physical_register_usage.is_empty() {
            return 0.0;
        }

        let total_usage: usize = self.physical_register_usage.values().sum();
        total_usage as f64 / self.physical_register_usage.len() as f64
    }

    /// 检查是否存在寄存器压力过高的情况
    pub fn has_high_pressure(&self) -> bool {
        self.register_pressure > 0.8
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::lifetime_analysis::RegisterLifetime;

    #[test]
    fn test_allocation_result_basic() {
        let cc = CallingConvention::standard();
        let mut result = AllocationResult::new(cc);

        // 添加一些映射
        result.register_mapping.insert(RegisterId(1), 0);
        result.register_mapping.insert(RegisterId(2), 1);
        result.spilled_registers.push(RegisterId(3));

        assert_eq!(result.allocated_count(), 2);
        assert_eq!(result.spilled_count(), 1);
        assert_eq!(result.allocation_efficiency(), 2.0 / 3.0);

        assert_eq!(result.get_physical_register(&RegisterId(1)), Some(0));
        assert!(result.is_spilled(&RegisterId(3)));
        assert!(!result.is_spilled(&RegisterId(1)));
    }

    #[test]
    fn test_validation() {
        let cc = CallingConvention::standard();
        let mut result = AllocationResult::new(cc);

        // 正确的分配
        result.register_mapping.insert(RegisterId(1), 0);
        result.register_mapping.insert(RegisterId(2), 1);
        assert!(result.validate().is_ok());

        // 错误的分配：同一个寄存器既分配又溢出
        result.spilled_registers.push(RegisterId(1));
        assert!(result.validate().is_err());
    }

    #[test]
    fn test_max_simultaneous_usage() {
        let cc = CallingConvention::standard();
        let mut result = AllocationResult::new(cc);

        // 添加生命周期
        result.lifetimes = vec![
            RegisterLifetime::new(RegisterId(1), 0, 2), // [0, 2]
            RegisterLifetime::new(RegisterId(2), 1, 4), // [1, 4]
            RegisterLifetime::new(RegisterId(3), 3, 6), // [3, 6]
        ];

        // 分配物理寄存器
        result.register_mapping.insert(RegisterId(1), 0);
        result.register_mapping.insert(RegisterId(2), 1);
        result.register_mapping.insert(RegisterId(3), 2);

        let max_usage = result.calculate_max_simultaneous_usage();
        assert_eq!(max_usage, 2); // 在时间点1-2，有2个寄存器同时活跃（reg1和reg2）
    }
} 