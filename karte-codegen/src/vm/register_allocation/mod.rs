//! Karte 专业寄存器分配系统
//! 
//! 重新设计的寄存器分配系统，包含：
//! - 线性扫描寄存器分配算法
//! - 生命周期分析
//! - 溢出处理
//! - 调用约定集成

pub mod lifetime_analysis;
pub mod linear_scan;
pub mod spill_manager;
pub mod allocation_result;

pub use lifetime_analysis::*;
pub use linear_scan::*;
pub use spill_manager::*;
pub use allocation_result::*;

use super::calling_convention::{CallingConvention, PhysicalRegister};
use karte_lir::{LirFunction, RegisterId};
use std::collections::HashMap;

/// 寄存器分配器主控制器
#[derive(Debug)]
pub struct ProfessionalRegisterAllocator {
    /// 调用约定
    calling_convention: CallingConvention,
    /// 生命周期分析器
    lifetime_analyzer: LifetimeAnalyzer,
    /// 线性扫描分配器
    linear_scan: LinearScanAllocator,
    /// 溢出管理器
    spill_manager: SpillManager,
}

impl ProfessionalRegisterAllocator {
    /// 创建新的寄存器分配器
    pub fn new(calling_convention: CallingConvention) -> Self {
        let allocatable_regs = calling_convention.get_allocatable_registers();
        
        Self {
            calling_convention: calling_convention.clone(),
            lifetime_analyzer: LifetimeAnalyzer::new(),
            linear_scan: LinearScanAllocator::new(allocatable_regs),
            spill_manager: SpillManager::new(calling_convention),
        }
    }

    /// 为函数分配寄存器
    pub fn allocate_function(&mut self, function: &mut LirFunction) -> Result<AllocationResult, String> {
        // 1. 分析寄存器生命周期
        let lifetimes = self.lifetime_analyzer.analyze_function(function)?;
        
        // 2. 执行线性扫描分配
        let (allocation, spills) = self.linear_scan.allocate(&lifetimes)?;
        
        // 3. 处理溢出
        if !spills.is_empty() {
            self.spill_manager.handle_spills(function, &spills)?;
        }
        
        // 4. 构建分配结果
        let result = AllocationResult {
            register_mapping: allocation,
            spilled_registers: spills,
            lifetimes,
            calling_convention: self.calling_convention.clone(),
        };
        
        Ok(result)
    }

    /// 为单个函数调用优化寄存器分配
    pub fn optimize_for_call(&mut self, 
                           function: &mut LirFunction,
                           call_site: usize,
                           args: &[RegisterId],
                           return_reg: Option<RegisterId>) -> Result<(), String> {
        // 分析调用点周围的寄存器使用
        let call_context = self.lifetime_analyzer.analyze_call_site(function, call_site, args, return_reg)?;
        
        // 优化参数传递的寄存器分配
        self.linear_scan.optimize_call_site(&call_context)?;
        
        Ok(())
    }

    /// 获取分配统计信息
    pub fn get_statistics(&self) -> AllocationStatistics {
        AllocationStatistics {
            total_registers: self.linear_scan.get_total_registers(),
            allocated_registers: self.linear_scan.get_allocated_count(),
            spilled_registers: self.spill_manager.get_spill_count(),
            allocation_pressure: self.linear_scan.get_register_pressure(),
        }
    }
}

/// 分配统计信息
#[derive(Debug, Clone)]
pub struct AllocationStatistics {
    pub total_registers: usize,
    pub allocated_registers: usize,
    pub spilled_registers: usize,
    pub allocation_pressure: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use karte_lir::{Instruction, Operand};
    use karte_diagnostics::Span;

    #[test]
    fn test_register_allocator_creation() {
        let cc = CallingConvention::standard();
        let mut allocator = ProfessionalRegisterAllocator::new(cc);
        
        // 创建一个简单的测试函数
        let mut function = LirFunction::new("test".to_string());
        function.add_instruction(Instruction::Move {
            dst: RegisterId(1),
            src: Operand::Immediate { value: 42 },
            span: Span::dummy(),
        });
        
        let result = allocator.allocate_function(&mut function);
        assert!(result.is_ok());
    }
} 