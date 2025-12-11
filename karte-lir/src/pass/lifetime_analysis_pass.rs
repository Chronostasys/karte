//! 生命周期分析Pass
//!
//! 负责分析寄存器的生命周期，为后续的指令降级提供信息

use crate::pass::register_allocation::{LifetimeAnalyzer, RegisterLifetime, RegisterType};
use crate::pass::{AnalysisManager, AnalysisPass, AnalysisResult};
use crate::{LirFunction, Register};
use karte_common::calling_convention::CallingConvention;
use std::any::Any;
use std::collections::HashMap;

/// 生命周期分析结果
#[derive(Debug)]
pub struct LifetimeAnalysisResult {
    /// 寄存器生命周期信息
    pub lifetimes: Vec<RegisterLifetime>,
    /// 寄存器类型映射
    pub register_types: HashMap<Register, RegisterType>,
    /// 虚拟寄存器到物理寄存器的映射
    pub register_mapping: HashMap<Register, u8>,
}

impl LifetimeAnalysisResult {
    pub fn new(
        lifetimes: Vec<RegisterLifetime>,
        register_types: HashMap<Register, RegisterType>,
        register_mapping: HashMap<Register, u8>,
    ) -> Self {
        Self {
            lifetimes,
            register_types,
            register_mapping,
        }
    }

    /// 获取指定指令位置活跃的调用者保存寄存器
    pub fn get_live_caller_saved_registers_at(
        &self,
        instruction_index: usize,
        calling_convention: &CallingConvention,
    ) -> std::collections::HashSet<u8> {
        use crate::pass::register_allocation::LifetimeAnalyzer;

        let analyzer = LifetimeAnalyzer::new(calling_convention.clone());
        analyzer.get_live_physical_registers_at(
            instruction_index,
            calling_convention,
            &self.register_mapping,
            &self.lifetimes,
        )
    }
}

impl AnalysisResult for LifetimeAnalysisResult {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// 生命周期分析Pass
#[derive(Debug)]
pub struct LifetimeAnalysisPass {
    analyzer: LifetimeAnalyzer,
}

impl LifetimeAnalysisPass {
    pub fn new() -> Self {
        Self {
            analyzer: LifetimeAnalyzer::new(CallingConvention::standard()),
        }
    }

    /// 创建寄存器映射（虚拟寄存器到物理寄存器）
    fn create_register_mapping(&self, function: &LirFunction) -> HashMap<Register, u8> {
        let mut register_mapping = HashMap::new();

        // 扫描指令，建立虚拟寄存器到物理寄存器的映射
        for instruction in &function.instructions {
            let (defined_regs, used_regs) = instruction.get_defined_and_used_registers();

            // 处理定义的寄存器
            for reg in defined_regs {
                if !register_mapping.contains_key(&reg) {
                    match reg {
                        Register::Physical(phys_reg) => {
                            register_mapping.insert(reg, phys_reg);
                        }
                        Register::Virtual(_) => {
                            let phys_reg = self.infer_physical_register(reg);
                            register_mapping.insert(reg, phys_reg);
                        }
                    }
                }
            }

            // 处理使用的寄存器
            for reg in used_regs {
                if !register_mapping.contains_key(&reg) {
                    match reg {
                        Register::Physical(phys_reg) => {
                            register_mapping.insert(reg, phys_reg);
                        }
                        Register::Virtual(_) => {
                            let phys_reg = self.infer_physical_register(reg);
                            register_mapping.insert(reg, phys_reg);
                        }
                    }
                }
            }
        }

        register_mapping
    }

    /// 推断虚拟寄存器对应的物理寄存器
    fn infer_physical_register(&self, virtual_reg: Register) -> u8 {
        let calling_convention = karte_common::calling_convention::CallingConvention::standard();

        match virtual_reg {
            Register::Physical(phys_reg) => phys_reg,
            Register::Virtual(virt_id) => {
                // 使用启发式方法：将虚拟寄存器ID映射到物理寄存器
                match virt_id {
                    0 => calling_convention.return_register, // 返回值寄存器
                    1 => calling_convention.argument_registers[0], // 参数1
                    2 => calling_convention.argument_registers[1], // 参数2
                    3 => calling_convention.argument_registers[2], // 参数3
                    4 => calling_convention.argument_registers[3], // 参数4
                    _ => {
                        // 对于其他虚拟寄存器，使用callee-saved寄存器
                        (8 + (virt_id % 24)) as u8
                    }
                }
            }
        }
    }
}

impl Default for LifetimeAnalysisPass {
    fn default() -> Self {
        Self::new()
    }
}

impl AnalysisPass for LifetimeAnalysisPass {
    fn name(&self) -> &str {
        "lifetime-analysis"
    }

    fn description(&self) -> &str {
        "生命周期分析 - 分析变量和寄存器的生命周期范围"
    }

    fn analyze_function(
        &mut self,
        function: &LirFunction,
        _analyses: &AnalysisManager,
    ) -> Result<Box<dyn AnalysisResult>, String> {
        // 执行生命周期分析
        let (lifetimes, register_types) = self.analyzer.analyze_simple(function);

        // 创建寄存器映射
        let register_mapping = self.create_register_mapping(function);

        log::debug!(
            "生命周期分析完成，共分析 {} 个寄存器生命周期",
            lifetimes.len()
        );

        Ok(Box::new(LifetimeAnalysisResult::new(
            lifetimes,
            register_types,
            register_mapping,
        )))
    }
}
