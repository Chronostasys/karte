//! 寄存器生命周期分析
//!
//! 负责分析虚拟寄存器在函数中的活跃范围，为寄存器分配提供基础信息。
//!
//! ## 工作流程
//! 1. **CFG/Def-Use 分析 (可选)**: 如果提供了控制流图和Def-Use链，
//!    分析器可以执行更精确的、基于数据流的活跃度分析。
//! 2. **生命周期计算**:
//!    - **精确模式**: 使用活跃度信息来确定每个虚拟寄存器从定义到最后一次
//!      使用的精确范围。
//!    - **简单模式**: 如果没有CFG，则回退到简单的指令扫描来估算生命周期。
//! 3. **寄存器类型分析**: 使用新的寄存器类型系统来正确识别和分类寄存器，
//!    包括栈地址寄存器、函数参数等。

use super::types::{CallingConvention, RegisterLifetime, RegisterType};
use crate::pass::analysis::{ControlFlowGraph, DefUseChains};
use crate::{Instruction, LirFunction, Operand, Register};
use karte_common::calling_convention::CC;
use std::collections::{HashMap, HashSet};

/// 寄存器生命周期分析器
///
/// 封装了所有与计算虚拟寄存器生命周期相关的逻辑。
#[derive(Clone, Debug)]
pub struct LifetimeAnalyzer {
    _calling_convention: CallingConvention,
}

impl LifetimeAnalyzer {
    /// 创建新的生命周期分析器
    pub fn new(calling_convention: CallingConvention) -> Self {
        Self {
            _calling_convention: calling_convention,
        }
    }

    /// 🔧 新增：分析寄存器类型
    ///
    /// 根据指令模式识别寄存器的语义类型
    fn analyze_register_types(&self, function: &LirFunction) -> HashMap<Register, RegisterType> {
        let mut register_types = HashMap::new();

        // 1. 识别函数参数寄存器
        for &param_reg in function.parameter_registers.iter() {
            register_types.insert(param_reg, RegisterType::FunctionParameter);
        }

        // 2. 识别栈地址寄存器
        for instruction in &function.instructions {
            match instruction {
                // alloc指令的目标寄存器是栈地址寄存器
                Instruction::Alloc {
                    dst,
                    allocation_type,
                    ..
                } => {
                    match allocation_type {
                        crate::AllocationType::Stack => {
                            register_types.insert(*dst, RegisterType::StackAddress);
                            log::info!("🔍 识别栈地址寄存器: {:?} (来自alloc指令)", dst);
                        }
                        crate::AllocationType::Heap | crate::AllocationType::Static => {
                            // 堆分配和静态分配的目标寄存器是数据寄存器
                            register_types.insert(*dst, RegisterType::Data);
                        }
                    }
                }

                // 由StackFrameLowering转换的add指令：dst = fp + offset
                Instruction::Add {
                    dst, src1, src2, ..
                } => {
                    if let Operand::Register { id: fp_reg } = src1 {
                        // 检查是否是帧指针寄存器（r7）
                        if fp_reg.id() == 7 {
                            // 检查第二个操作数是否是立即数（偏移量）
                            if let Operand::Immediate { .. } = src2 {
                                register_types.insert(*dst, RegisterType::StackAddress);
                                log::info!("🔍 识别栈地址寄存器: {:?} (来自add指令)", dst);
                            }
                        }
                    }
                }

                // 其他指令的寄存器默认为数据寄存器
                _ => {}
            }
        }

        log::info!(
            "🔍 寄存器类型分析完成，共识别 {} 个寄存器类型",
            register_types.len()
        );
        for (reg, reg_type) in &register_types {
            log::info!("🔍 寄存器 {:?} -> {:?}", reg, reg_type);
        }

        register_types
    }

    /// 使用简单的指令扫描分析生命周期 (回退方案)
    ///
    /// 这是一个不依赖于复杂分析（如CFG或Def-Use）的简单版本。
    /// 它通过单次遍历指令来估算生命周期。
    ///
    /// @param function - 需要分析的LIR函数。
    /// @returns 一个元组，包含 (生命周期列表, 寄存器类型映射)。
    pub fn analyze_simple(
        &self,
        function: &LirFunction,
    ) -> (Vec<RegisterLifetime>, HashMap<Register, RegisterType>) {
        let mut lifetimes = HashMap::new();
        let mut register_types = self.analyze_register_types(function);

        // 首先处理函数参数
        for (param_idx, &param_reg) in function.parameter_registers.iter().enumerate() {
            let lifetime = RegisterLifetime {
                register: param_reg,
                start: 0,
                end: function.instructions.len().saturating_sub(1),
                uses: Vec::new(),
                is_function_parameter: true,
                parameter_index: Some(param_idx),
                register_type: RegisterType::FunctionParameter,
            };
            lifetimes.insert(param_reg, lifetime);
        }

        // 扫描所有指令，记录寄存器的使用
        for (i, instruction) in function.instructions.iter().enumerate() {
            let (defined_regs, used_regs) = instruction.get_defined_and_used_registers();

            for reg in defined_regs.iter().chain(used_regs.iter()) {
                // 🔧 修复：确保所有寄存器都有类型信息
                if !register_types.contains_key(reg) {
                    register_types.insert(*reg, RegisterType::Data);
                }

                let register_type = register_types.get(reg).unwrap();

                let lifetime = lifetimes.entry(*reg).or_insert_with(|| RegisterLifetime {
                    register: *reg,
                    start: i,
                    end: i,
                    uses: Vec::new(),
                    is_function_parameter: false,
                    parameter_index: None,
                    register_type: *register_type,
                });

                lifetime.end = i;
                lifetime.uses.push(i);
            }
        }

        // 🔥 新增：补全所有指令中出现但未被分析的虚拟寄存器
        for (i, instruction) in function.instructions.iter().enumerate() {
            let (defined_regs, used_regs) = instruction.get_defined_and_used_registers();
            for reg in defined_regs.iter().chain(used_regs.iter()) {
                if !lifetimes.contains_key(reg) {
                    let register_type = *register_types.get(reg).unwrap_or(&RegisterType::Data);
                    lifetimes.insert(
                        *reg,
                        RegisterLifetime {
                            register: *reg,
                            start: i,
                            end: i,
                            uses: vec![i],
                            is_function_parameter: false,
                            parameter_index: None,
                            register_type,
                        },
                    );
                }
            }
        }

        // 🔧 修复：确保生命周期列表的确定性顺序
        let mut lifetime_list: Vec<_> = lifetimes.into_values().collect();
        lifetime_list.sort_by_key(|lt| (lt.register.id(), lt.start, lt.end));
        (lifetime_list, register_types)
    }

    /// 使用CFG和Def-Use信息进行更精确的生命周期分析
    ///
    /// 这是首选的分析方法，它利用数据流分析来获得最准确的生命周期。
    ///
    /// @param function - 需要分析的LIR函数。
    /// @param cfg - 函数的控制流图。
    /// @param def_use - 函数的Def-Use链。
    /// @returns 一个元组，包含 (生命周期列表, 寄存器类型映射)。
    pub fn analyze_with_cfg(
        &self,
        function: &LirFunction,
        cfg: &ControlFlowGraph,
        def_use: &DefUseChains,
    ) -> (Vec<RegisterLifetime>, HashMap<Register, RegisterType>) {
        let liveness = self.compute_liveness(cfg, def_use);
        let mut lifetimes = HashMap::new();
        let mut register_types = self.analyze_register_types(function);

        for (param_idx, &param_reg) in function.parameter_registers.iter().enumerate() {
            let lifetime = RegisterLifetime {
                register: param_reg,
                start: 0,
                end: function.instructions.len().saturating_sub(1),
                uses: Vec::new(),
                is_function_parameter: true,
                parameter_index: Some(param_idx),
                register_type: RegisterType::FunctionParameter,
            };
            lifetimes.insert(param_reg, lifetime);
        }

        for (&register, def_positions) in &def_use.definitions {
            if lifetimes.contains_key(&register) {
                continue;
            }

            let mut start = usize::MAX;
            let mut end = 0;
            let mut uses = Vec::new();

            for &def_pos in def_positions {
                start = start.min(def_pos);
                end = end.max(def_pos);
            }

            if let Some(use_positions) = def_use.uses.get(&register) {
                for &use_pos in use_positions {
                    uses.push(use_pos);
                    end = end.max(use_pos);
                }
            }

            let refined_end = self.refine_lifetime_end(register, end, &liveness);

            // 🔧 修复：确保所有寄存器都有类型信息
            register_types.entry(register).or_insert(RegisterType::Data);
            let register_type = register_types.get(&register).unwrap();

            lifetimes.insert(
                register,
                RegisterLifetime {
                    register,
                    start,
                    end: refined_end,
                    uses,
                    is_function_parameter: false,
                    parameter_index: None,
                    register_type: *register_type,
                },
            );
        }
        // 🔥 新增：补全所有指令中出现但未被分析的虚拟寄存器
        for (i, instruction) in function.instructions.iter().enumerate() {
            let (defined_regs, used_regs) = instruction.get_defined_and_used_registers();
            for reg in defined_regs.iter().chain(used_regs.iter()) {
                if !lifetimes.contains_key(reg) {
                    let register_type = *register_types.get(reg).unwrap_or(&RegisterType::Data);
                    lifetimes.insert(
                        *reg,
                        RegisterLifetime {
                            register: *reg,
                            start: i,
                            end: i,
                            uses: vec![i],
                            is_function_parameter: false,
                            parameter_index: None,
                            register_type,
                        },
                    );
                }
            }
        }

        // 🔧 修复：确保生命周期列表的确定性顺序
        let mut lifetime_list: Vec<_> = lifetimes.into_values().collect();
        lifetime_list.sort_by_key(|lt| (lt.register.id(), lt.start, lt.end));
        (lifetime_list, register_types)
    }

    /// 计算活跃度分析 (Liveness Analysis)
    ///
    /// 实现标准的向后数据流分析算法来计算每个程序点的活跃变量集合。
    /// 这是精确生命周期分析的基础。
    fn compute_liveness(
        &self,
        cfg: &ControlFlowGraph,
        def_use: &DefUseChains,
    ) -> HashMap<usize, HashSet<Register>> {
        let mut block_use = HashMap::new();
        let mut block_def = HashMap::new();

        for node in &cfg.nodes {
            let mut use_set = HashSet::new();
            let mut def_set = HashSet::new();

            let (start, end) = node.instruction_range;
            for instr_idx in start..end {
                if let Some(uses) = def_use.instruction_uses.get(&instr_idx) {
                    for &reg in uses {
                        if !def_set.contains(&reg) {
                            use_set.insert(reg);
                        }
                    }
                }
                if let Some(defs) = def_use.instruction_defs.get(&instr_idx) {
                    for &reg in defs {
                        def_set.insert(reg);
                    }
                }
            }
            block_use.insert(node.block_id, use_set);
            block_def.insert(node.block_id, def_set);
        }

        let mut live_in: HashMap<usize, HashSet<Register>> = HashMap::new();
        let mut live_out: HashMap<usize, HashSet<Register>> = HashMap::new();

        for node in &cfg.nodes {
            live_in.insert(node.block_id, HashSet::new());
            live_out.insert(node.block_id, HashSet::new());
        }

        let mut changed = true;
        while changed {
            changed = false;
            for node in cfg.nodes.iter().rev() {
                let block_id = node.block_id;

                let mut new_live_out = HashSet::new();
                for &successor in &node.successors {
                    if let Some(succ_live_in) = live_in.get(&successor) {
                        new_live_out.extend(succ_live_in.iter().copied());
                    }
                }

                let mut new_live_in = block_use.get(&block_id).cloned().unwrap_or_default();
                let def_set = block_def.get(&block_id).cloned().unwrap_or_default();
                let live_out_minus_def: HashSet<_> =
                    new_live_out.difference(&def_set).copied().collect();
                new_live_in.extend(live_out_minus_def);

                if live_out.get(&block_id) != Some(&new_live_out) {
                    live_out.insert(block_id, new_live_out);
                    changed = true;
                }
                if live_in.get(&block_id) != Some(&new_live_in) {
                    live_in.insert(block_id, new_live_in);
                    changed = true;
                }
            }
        }

        let mut live_at_instruction = HashMap::new();
        for node in &cfg.nodes {
            let (start, end) = node.instruction_range;
            let mut current_live = live_out.get(&node.block_id).cloned().unwrap_or_default();

            for instr_idx in (start..end).rev() {
                live_at_instruction.insert(instr_idx, current_live.clone());
                if let Some(defs) = def_use.instruction_defs.get(&instr_idx) {
                    for &reg in defs {
                        current_live.remove(&reg);
                    }
                }
                if let Some(uses) = def_use.instruction_uses.get(&instr_idx) {
                    for &reg in uses {
                        current_live.insert(reg);
                    }
                }
            }
        }
        live_at_instruction
    }

    /// 使用活跃度信息精确化生命周期结束点
    fn refine_lifetime_end(
        &self,
        register: Register,
        initial_end: usize,
        liveness: &HashMap<usize, HashSet<Register>>,
    ) -> usize {
        for instr_idx in (0..=initial_end).rev() {
            if let Some(live_set) = liveness.get(&instr_idx) {
                if !live_set.contains(&register) {
                    return instr_idx;
                }
            }
        }
        initial_end
    }

    /// 获取指定指令位置的活跃物理寄存器
    ///
    /// 这个方法用于优化函数调用时的寄存器保存，只保存真正包含活跃值的调用者保存寄存器。
    ///
    /// # 参数
    /// * `instruction_index` - 指令索引位置
    /// * `calling_convention` - 调用约定
    /// * `register_mapping` - 虚拟寄存器到物理寄存器的映射（如果已分配）
    /// * `lifetimes` - 寄存器生命周期列表（由analyze_simple或analyze_with_cfg生成）
    ///
    /// # 返回值
    /// 需要保存的活跃调用者保存物理寄存器集合
    pub fn get_live_physical_registers_at(
        &self,
        instruction_index: usize,
        calling_convention: &CallingConvention,
        register_mapping: &HashMap<Register, u8>,
        lifetimes: &[RegisterLifetime],
    ) -> HashSet<u8> {
        let mut live_physical_regs = HashSet::new();

        // 1. 找出在该指令位置活跃的所有虚拟寄存器
        for lifetime in lifetimes {
            // 检查虚拟寄存器是否在该指令位置活跃
            if instruction_index >= lifetime.start && instruction_index <= lifetime.end {
                // 2. 将活跃的虚拟寄存器映射到物理寄存器
                if let Some(&physical_reg) = register_mapping.get(&lifetime.register) {
                    // 3. 检查该物理寄存器是否是调用者保存寄存器
                    if calling_convention.is_caller_saved(physical_reg) {
                        // 4. 排除返回值寄存器，因为它会被调用覆盖
                        if physical_reg != calling_convention.return_register {
                            live_physical_regs.insert(physical_reg);
                        }
                    }
                }
            }
        }

        log::debug!(
            "指令 {} 位置需要保存的调用者保存寄存器: {:?}",
            instruction_index,
            live_physical_regs
        );

        live_physical_regs
    }
}
