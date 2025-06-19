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

pub mod types;
pub mod lifetime_analysis;
pub mod linear_scan;

// 重新导出主要类型，方便外部使用
pub use types::*;
pub use lifetime_analysis::LifetimeAnalyzer;
pub use linear_scan::LinearScanAllocator;

use super::{FunctionPass, PassResult, AnalysisManager};
use crate::{LirFunction, RegisterId, Instruction, Operand};
use std::collections::{HashMap, HashSet};

/// 线性扫描寄存器分配Pass
/// 
/// 封装了整个寄存器分配流程，并协调其子模块。
pub struct LinearScanRegisterAllocation {
    mode: RegisterAllocationMode,
    lifetime_analyzer: LifetimeAnalyzer,
    linear_scan_allocator: LinearScanAllocator,
}

impl LinearScanRegisterAllocation {
    /// 创建一个新的寄存器分配器实例
    pub fn new(mode: RegisterAllocationMode) -> Self {
        let calling_convention = SimpleCallingConvention::default();
        let lifetime_analyzer = LifetimeAnalyzer::new(calling_convention.clone());
        let linear_scan_allocator = LinearScanAllocator::new(calling_convention.clone());
        
        Self {
            mode,
            lifetime_analyzer,
            linear_scan_allocator,
        }
    }
}

impl FunctionPass for LinearScanRegisterAllocation {
    fn name(&self) -> &str {
        "linear-scan-register-allocation"
    }

    fn run_on_function(&mut self, function: &mut LirFunction, analyses: &mut AnalysisManager) -> PassResult {
        let (lifetimes, stack_address_registers) = self.analyze_lifetimes(function, analyses);

        let allocation_result = self.linear_scan_allocator.allocate(lifetimes, &stack_address_registers);
        
        if self.mode == RegisterAllocationMode::DecisionOnly {
            // 在决策模式下，只存储结果，不修改代码
            analyses.store_result(format!("pre-ra-decision-{}", function.name), Box::new(allocation_result));
            return PassResult::Changed;
        }

        // 在最终改写模式下，应用分配结果
        self.apply_allocation(function, &allocation_result)
    }
}

impl LinearScanRegisterAllocation {
    /// 分析函数中所有寄存器的生命周期
    fn analyze_lifetimes(&self, function: &LirFunction, analyses: &mut AnalysisManager) -> (Vec<RegisterLifetime>, HashSet<RegisterId>) {
        // 尝试获取CFG和Def-Use分析结果
        let cfg_result = analyses.get_result::<crate::pass::analysis::ControlFlowGraph>("cfg");
        let def_use_result = analyses.get_result::<crate::pass::analysis::DefUseChains>("def-use");
        
        match (cfg_result, def_use_result) {
            (Some(cfg), Some(def_use)) => {
                self.lifetime_analyzer.analyze_with_cfg(function, cfg, def_use)
            },
            _ => {
                // 如果分析不可用，回退到简单模式
                self.lifetime_analyzer.analyze_simple(function)
            }
        }
    }

    /// 将寄存器分配结果应用到LIR函数
    fn apply_allocation(&self, function: &mut LirFunction, allocation: &RegisterAllocationResult) -> PassResult {
        for instruction in &mut function.instructions {
            self.replace_registers_in_instruction(instruction, &allocation.register_mapping)
                .expect("寄存器替换失败，这是一个严重的bug");
        }
        PassResult::Changed
    }

    /// 在单个指令中替换虚拟寄存器为物理寄存器
    fn replace_registers_in_instruction(&self, instruction: &mut Instruction, mapping: &HashMap<RegisterId, u8>) -> Result<(), String> {
        let map_reg = |reg: &mut RegisterId| {
            // 强制SP/FP始终映射到r6/r7
            if reg.0 == 6 {
                *reg = RegisterId(6);
            } else if reg.0 == 7 {
                *reg = RegisterId(7);
            } else if let Some(&physical_reg) = mapping.get(reg) {
                *reg = RegisterId(physical_reg as usize);
            }
        };

        let map_operand = |operand: &mut Operand| {
            if let Operand::Register { id } = operand {
                map_reg(id);
            }
        };

        match instruction {
            Instruction::Move { dst, src, .. } => { map_reg(dst); map_operand(src); }
            Instruction::Add { dst, src1, src2, .. } |
            Instruction::Sub { dst, src1, src2, .. } |
            Instruction::Mul { dst, src1, src2, .. } |
            Instruction::Div { dst, src1, src2, .. } => {
                map_reg(dst);
                map_operand(src1);
                map_operand(src2);
            }
            Instruction::Store64 { addr, src, .. } => { map_reg(addr); map_operand(src); }
            Instruction::Load64 { dst, addr, .. } => { map_reg(dst); map_reg(addr); }
            Instruction::Compare { src1, src2, .. } => { map_operand(src1); map_operand(src2); }
            Instruction::Return { value, .. } => {
                if let Some(reg) = value { map_reg(reg); }
            }
            Instruction::CallIndirect { function_register, args, result, .. } => {
                map_reg(function_register);
                args.iter_mut().for_each(|arg| map_reg(arg));
                if let Some(res) = result { map_reg(res); }
            }
            Instruction::Call { args, result, .. } => {
                args.iter_mut().for_each(|arg| map_reg(arg));
                if let Some(res) = result { map_reg(res); }
            }
            Instruction::Alloc { dst, .. } |
            Instruction::StructAlloc { dst, .. } => { map_reg(dst); }
            Instruction::StructFieldStore { struct_addr, src, .. } => { map_reg(struct_addr); map_operand(src); }
            Instruction::StructFieldLoad { dst, struct_addr, .. } => { map_reg(dst); map_reg(struct_addr); }
            Instruction::Phi { dst, incoming, .. } => {
                map_reg(dst);
                incoming.iter_mut().for_each(|(_, op)| map_operand(op));
            }
            _ => {}
        }
        Ok(())
    }
} 