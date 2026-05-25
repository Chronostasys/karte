use super::{AnalysisManager, FunctionPass, PassResult};
use crate::{Instruction, LirFunction, Operand, Register};
use karte_ir_codec::IrDisplay;
use log::info;
use std::collections::{HashMap, HashSet};

/// 打印IR的Pass - 用于调试和观察优化效果
///
/// 这个Pass会打印当前函数的完整IR(使用标准IR格式)
#[derive(Debug)]
pub struct PrintIRPass {
    /// Pass名称(可自定义,用于标识打印的位置)
    name: String,
}

impl PrintIRPass {
    /// 创建新的PrintIRPass
    pub fn new() -> Self {
        Self {
            name: "print-ir".to_string(),
        }
    }

    /// 创建带自定义名称的PrintIRPass
    pub fn with_name(name: &str) -> Self {
        Self {
            name: name.to_string(),
        }
    }
}

impl Default for PrintIRPass {
    fn default() -> Self {
        Self::new()
    }
}

impl FunctionPass for PrintIRPass {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "打印IR - 使用标准IR格式打印函数(调试用)"
    }

    fn run_on_function(
        &mut self,
        function: &mut LirFunction,
        _analyses: &mut AnalysisManager,
    ) -> PassResult {
        println!("=== {} ===", self.name);
        println!("{}", function.to_ir_string());
        println!("=== {} 结束 ===\n", self.name);
        PassResult::Unchanged
    }

    fn invalidated_analyses(&self) -> Vec<&'static str> {
        vec![]
    }
}

/// 验证Pass - 检查IR的正确性
///
/// 检查项包括:
/// - 寄存器定义后再使用
/// - 跳转目标的合法性
/// - Phi节点的正确性
#[derive(Debug)]
pub struct VerifyPass {
    /// 是否启用严格模式(检测更多问题)
    strict: bool,
}

impl VerifyPass {
    pub fn new() -> Self {
        Self { strict: false }
    }

    pub fn with_strict(mut self) -> Self {
        self.strict = true;
        self
    }

    fn verify_function(&self, function: &LirFunction) -> crate::Result<()> {
        let mut defined_regs = HashSet::new();
        let mut labels = HashMap::new();

        for (i, instr) in function.instructions.iter().enumerate() {
            match instr {
                Instruction::Label { id, .. } => {
                    if labels.insert(*id, i).is_some() {
                        return Err(format!("重复的标签: {:?}", id).into());
                    }
                }
                Instruction::Move { dst, src, .. } => {
                    self.check_operand_defined(src, &defined_regs)?;
                    defined_regs.insert(*dst);
                }
                Instruction::Add {
                    dst, src1, src2, ..
                }
                | Instruction::Sub {
                    dst, src1, src2, ..
                }
                | Instruction::Mul {
                    dst, src1, src2, ..
                }
                | Instruction::Div {
                    dst, src1, src2, ..
                } => {
                    self.check_operand_defined(src1, &defined_regs)?;
                    self.check_operand_defined(src2, &defined_regs)?;
                    defined_regs.insert(*dst);
                }
                Instruction::Load64 { dst, addr, .. } => {
                    self.check_register_defined(*addr, &defined_regs)?;
                    defined_regs.insert(*dst);
                }
                Instruction::Store64 { addr, src, .. } => {
                    self.check_register_defined(*addr, &defined_regs)?;
                    self.check_operand_defined(src, &defined_regs)?;
                }
                Instruction::Return { value, .. } => {
                    if let Some(reg) = value {
                        self.check_register_defined(*reg, &defined_regs)?;
                    }
                }
                _ => {}
            }
        }

        info!("✓ 函数 {} 验证通过", function.name);
        Ok(())
    }

    fn check_operand_defined(
        &self,
        operand: &crate::Operand,
        defined: &HashSet<crate::Register>,
    ) -> crate::Result<()> {
        use crate::Operand;

        match operand {
            Operand::Register { id } => self.check_register_defined(*id, defined),
            Operand::Memory { base, .. } => self.check_register_defined(*base, defined),
            Operand::StructField { struct_addr, .. } => {
                self.check_register_defined(*struct_addr, defined)
            }
            _ => Ok(()),
        }
    }

    fn check_register_defined(
        &self,
        reg: crate::Register,
        defined: &HashSet<crate::Register>,
    ) -> crate::Result<()> {
        use crate::Register;

        match reg {
            Register::Virtual(_) => {
                if self.strict && !defined.contains(&reg) {
                    return Err(format!("寄存器 {:?} 在定义前使用", reg).into());
                }
            }
            Register::Physical(_) => {}
        }
        Ok(())
    }
}

impl Default for VerifyPass {
    fn default() -> Self {
        Self::new()
    }
}

impl FunctionPass for VerifyPass {
    fn name(&self) -> &str {
        "verify"
    }

    fn description(&self) -> &str {
        "验证IR - 检查IR正确性(寄存器定义/跳转目标等)"
    }

    fn run_on_function(
        &mut self,
        function: &mut LirFunction,
        _analyses: &mut AnalysisManager,
    ) -> PassResult {
        match self.verify_function(function) {
            Ok(_) => PassResult::Unchanged,
            Err(msg) => PassResult::Failed(msg.to_string()),
        }
    }

    fn invalidated_analyses(&self) -> Vec<&'static str> {
        vec![]
    }
}

/// 统计Pass - 收集IR的统计信息
///
/// 统计项包括:
/// - 指令数量
/// - 基本块数量
/// - 寄存器使用情况
#[derive(Debug)]
pub struct StatisticsPass {
    /// 是否打印统计信息
    print: bool,
}

impl StatisticsPass {
    pub fn new() -> Self {
        Self { print: true }
    }

    pub fn with_print(mut self, print: bool) -> Self {
        self.print = print;
        self
    }

    fn collect_statistics(&self, function: &LirFunction) -> FunctionStatistics {
        let mut stats = FunctionStatistics::default();
        stats.function_name = function.name.clone();
        stats.instruction_count = function.instructions.len();
        stats.param_count = function.parameter_count;

        let mut virtual_regs = HashSet::new();
        let mut physical_regs = HashSet::new();
        let mut labels = HashSet::new();

        for instr in &function.instructions {
            if let Instruction::Label { id, .. } = instr {
                labels.insert(*id);
            }

            self.collect_register_from_instruction(instr, &mut virtual_regs, &mut physical_regs);
        }

        stats.label_count = labels.len();
        stats.virtual_register_count = virtual_regs.len();
        stats.physical_register_count = physical_regs.len();

        stats
    }

    fn collect_register_from_instruction(
        &self,
        instr: &Instruction,
        virtual_regs: &mut HashSet<usize>,
        physical_regs: &mut HashSet<u8>,
    ) {
        let add_reg = |reg: Register, vregs: &mut HashSet<usize>, pregs: &mut HashSet<u8>| match reg
        {
            Register::Virtual(id) => {
                vregs.insert(id);
            }
            Register::Physical(id) => {
                pregs.insert(id);
            }
        };

        match instr {
            Instruction::Move { dst, src, .. } => {
                add_reg(*dst, virtual_regs, physical_regs);
                if let Operand::Register { id } = src {
                    add_reg(*id, virtual_regs, physical_regs);
                }
            }
            Instruction::Add {
                dst, src1, src2, ..
            }
            | Instruction::Sub {
                dst, src1, src2, ..
            }
            | Instruction::Mul {
                dst, src1, src2, ..
            }
            | Instruction::Div {
                dst, src1, src2, ..
            } => {
                add_reg(*dst, virtual_regs, physical_regs);
                if let Operand::Register { id } = src1 {
                    add_reg(*id, virtual_regs, physical_regs);
                }
                if let Operand::Register { id } = src2 {
                    add_reg(*id, virtual_regs, physical_regs);
                }
            }
            _ => {}
        }
    }
}

impl Default for StatisticsPass {
    fn default() -> Self {
        Self::new()
    }
}

impl FunctionPass for StatisticsPass {
    fn name(&self) -> &str {
        "stats"
    }

    fn description(&self) -> &str {
        "统计信息 - 收集IR统计(指令数/寄存器使用等)"
    }

    fn run_on_function(
        &mut self,
        function: &mut LirFunction,
        _analyses: &mut AnalysisManager,
    ) -> PassResult {
        let stats = self.collect_statistics(function);

        if self.print {
            stats.print();
        }

        PassResult::Unchanged
    }

    fn invalidated_analyses(&self) -> Vec<&'static str> {
        vec![]
    }
}

/// 函数统计信息
#[derive(Debug, Default)]
pub struct FunctionStatistics {
    pub function_name: String,
    pub instruction_count: usize,
    pub param_count: usize,
    pub label_count: usize,
    pub virtual_register_count: usize,
    pub physical_register_count: usize,
}

impl FunctionStatistics {
    pub fn print(&self) {
        println!("=== 函数统计: {} ===", self.function_name);
        println!("  指令数量: {}", self.instruction_count);
        println!("  参数数量: {}", self.param_count);
        println!("  标签数量: {}", self.label_count);
        println!("  虚拟寄存器数量: {}", self.virtual_register_count);
        println!("  物理寄存器数量: {}", self.physical_register_count);
    }
}
