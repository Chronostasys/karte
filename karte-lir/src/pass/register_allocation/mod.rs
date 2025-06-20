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
        let (lifetimes, register_types) = self.analyze_lifetimes(function, analyses);

        let allocation_result = self.linear_scan_allocator.allocate(lifetimes, register_types);
        
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
    fn analyze_lifetimes(&self, function: &LirFunction, analyses: &mut AnalysisManager) -> (Vec<RegisterLifetime>, HashMap<RegisterId, RegisterType>) {
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
            self.replace_registers_in_instruction(instruction, allocation)
                .expect("寄存器替换失败，这是一个严重的bug");
        }
        PassResult::Changed
    }

    /// 在单个指令中替换虚拟寄存器为物理寄存器
    fn replace_registers_in_instruction(&self, instruction: &mut Instruction, allocation: &RegisterAllocationResult) -> Result<(), String> {
        let map_reg = |reg: &mut RegisterId| -> Result<(), String> {
            // 强制SP/FP始终映射到r6/r7
            if reg.0 == 6 {
                *reg = RegisterId(6);
            } else if reg.0 == 7 {
                *reg = RegisterId(7);
            } else if let Some(&physical_reg) = allocation.register_mapping.get(reg) {
                *reg = RegisterId(physical_reg as usize);
            } else if allocation.spilled_registers.contains_key(reg) {
                *reg = RegisterId(0); // 使用r0作为溢出寄存器的临时物理寄存器
            } else {
                // 如果是StackAddress类型，直接跳过（由StackFrameLowering处理）
                if let Some(&register_type) = allocation.register_types.get(reg) {
                    if register_type == RegisterType::StackAddress {
                        return Ok(());
                    }
                }
                return Err(format!("寄存器 {:?} 未分配物理寄存器，这是寄存器分配器的bug", reg));
            }
            Ok(())
        };

        let map_operand = |operand: &mut Operand| -> Result<(), String> {
            if let Operand::Register { id } = operand {
                map_reg(id)?;
            }
            Ok(())
        };

        match instruction {
            Instruction::Move { dst, src, .. } => { 
                map_reg(dst)?; 
                map_operand(src)?; 
            }
            Instruction::Add { dst, src1, src2, .. } |
            Instruction::Sub { dst, src1, src2, .. } |
            Instruction::Mul { dst, src1, src2, .. } |
            Instruction::Div { dst, src1, src2, .. } => {
                map_reg(dst)?;
                map_operand(src1)?;
                map_operand(src2)?;
            }
            Instruction::Store64 { addr, src, .. } => { 
                map_reg(addr)?; 
                map_operand(src)?; 
            }
            Instruction::Load64 { dst, addr, .. } => { 
                map_reg(dst)?; 
                map_reg(addr)?; 
            }
            Instruction::Compare { src1, src2, .. } => { 
                map_operand(src1)?; 
                map_operand(src2)?; 
            }
            Instruction::Return { value, .. } => {
                if let Some(reg) = value { 
                    map_reg(reg)?; 
                }
            }
            Instruction::CallIndirect { function_register, args, result, .. } => {
                map_reg(function_register)?;
                for arg in args {
                    map_reg(arg)?;
                }
                if let Some(res) = result { 
                    map_reg(res)?; 
                }
            }
            Instruction::Call { args, result, .. } => {
                for arg in args {
                    map_reg(arg)?;
                }
                if let Some(res) = result { 
                    map_reg(res)?; 
                }
            }
            Instruction::Alloc { dst, .. } |
            Instruction::StructAlloc { dst, .. } => { 
                map_reg(dst)?; 
            }
            Instruction::StructFieldStore { struct_addr, src, .. } => { 
                map_reg(struct_addr)?; 
                map_operand(src)?; 
            }
            Instruction::StructFieldLoad { dst, struct_addr, .. } => { 
                map_reg(dst)?; 
                map_reg(struct_addr)?; 
            }
            Instruction::Phi { dst, incoming, .. } => {
                map_reg(dst)?;
                for (_, op) in incoming {
                    map_operand(op)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LirFunction, RegisterId, Instruction, Operand, AllocationType};
    use karte_diagnostics::Span;

    #[test]
    fn test_register_type_analysis() {
        // 创建一个简单的测试函数
        let mut function = LirFunction {
            name: "test_function".to_string(),
            parameter_registers: vec![RegisterId(100), RegisterId(101)],
            instructions: vec![
                // alloc指令 - 栈分配
                Instruction::Alloc {
                    dst: RegisterId(200),
                    size: 8,
                    alignment: 8,
                    allocation_type: AllocationType::Stack,
                    span: Span::dummy(),
                },
                // alloc指令 - 堆分配
                Instruction::Alloc {
                    dst: RegisterId(201),
                    size: 16,
                    alignment: 8,
                    allocation_type: AllocationType::Heap,
                    span: Span::dummy(),
                },
                // add指令 - 栈地址计算
                Instruction::Add {
                    dst: RegisterId(202),
                    src1: Operand::Register { id: RegisterId(7) }, // FP
                    src2: Operand::Immediate { value: -8 },
                    span: Span::dummy(),
                },
                // 普通数据操作
                Instruction::Add {
                    dst: RegisterId(203),
                    src1: Operand::Register { id: RegisterId(100) },
                    src2: Operand::Register { id: RegisterId(101) },
                    span: Span::dummy(),
                },
            ],
            next_register: 204,
            next_label: 1,
            struct_types: HashMap::new(),
            stack_frame_size: 0,
            parameter_count: 2,
        };

        let calling_convention = SimpleCallingConvention::default();
        let lifetime_analyzer = LifetimeAnalyzer::new(calling_convention);
        let (lifetimes, register_types) = lifetime_analyzer.analyze_simple(&function);

        // 验证函数参数类型
        assert_eq!(register_types.get(&RegisterId(100)), Some(&RegisterType::FunctionParameter));
        assert_eq!(register_types.get(&RegisterId(101)), Some(&RegisterType::FunctionParameter));

        // 验证栈地址寄存器类型
        assert_eq!(register_types.get(&RegisterId(200)), Some(&RegisterType::StackAddress)); // 栈alloc
        assert_eq!(register_types.get(&RegisterId(202)), Some(&RegisterType::StackAddress)); // FP + offset

        // 验证数据寄存器类型
        assert_eq!(register_types.get(&RegisterId(201)), Some(&RegisterType::Data)); // 堆alloc
        assert_eq!(register_types.get(&RegisterId(203)), Some(&RegisterType::Data)); // 普通计算

        // 验证生命周期中的寄存器类型
        for lifetime in &lifetimes {
            match lifetime.register.0 {
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