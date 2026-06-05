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
use crate::pass::analysis::{ControlFlowGraph, DefUseChains, LivenessAnalysis};
use crate::{Instruction, LirFunction, Operand, Register};
use karte_common::calling_convention::CC;
use std::collections::{HashMap, HashSet};

/// 寄存器生命周期分析器
///
/// 封装了所有与计算虚拟寄存器生命周期相关的逻辑。
#[derive(Clone, Debug)]
pub struct LifetimeAnalyzer {
    calling_convention: CallingConvention,
}

impl LifetimeAnalyzer {
    /// 创建新的生命周期分析器
    pub fn new(calling_convention: CallingConvention) -> Self {
        Self { calling_convention }
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
                        // 检查是否是帧指针寄存器 (ARM64: x29)
                        if fp_reg.id() == self.calling_convention.frame_pointer as usize {
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
                live_ranges: vec![], // 简单模式不计算精确范围
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
                    live_ranges: vec![],
                });

                // 🔧 修复：函数参数的 end 保留为函数末尾，不缩短
                // 函数参数可能在函数任意位置被引用（通过不同的 Virtual 寄存器），
                // 缩短 end 会导致寄存器被错误复用
                if !lifetime.is_function_parameter {
                    lifetime.end = i;
                }
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
                            live_ranges: vec![],
                        },
                    );
                }
            }
        }

        // 🔧 关键修复：处理循环回边（back-edge）对生命周期的影响
        //
        // 线性扫描不考虑控制流回边，例如 while 循环末尾的 Jump 指令跳回循环头。
        // 这导致循环体内的寄存器生命周期被低估：
        //   - 定义在循环体后半段（如指令 73）的寄存器，
        //     通过回边跳回循环头后，在前半段（如指令 46）被使用
        //   - 线性扫描算出的生命周期 [46, 73] 看似不与外部寄存器 [4, 12] 重叠
        //   - 但实际上该寄存器需要跨越整个循环存活
        //
        // 修复策略：识别所有回边（Jump/Branch 到更早的 Label），将循环体内
        // 所有出现过的寄存器的生命周期扩展到覆盖整个循环范围 [loop_start, loop_end]。
        {
            // 第一步：建立 LabelId → 指令位置 的映射
            let mut label_positions: HashMap<usize, usize> = HashMap::new();
            for (i, instruction) in function.instructions.iter().enumerate() {
                if let Instruction::Label { id, .. } = instruction {
                    label_positions.insert(id.0, i);
                }
            }

            // 第二步：收集所有回边（跳转目标位置 < 当前位置）
            // back_edges: Vec<(jump_position, target_position)>
            let mut back_edges: Vec<(usize, usize)> = Vec::new();
            for (i, instruction) in function.instructions.iter().enumerate() {
                let target_id = match instruction {
                    Instruction::Jump { target, .. } => target.0,
                    Instruction::JumpNotEqual { target, .. } => target.0,
                    Instruction::JumpEqual { target, .. } => target.0,
                    Instruction::JumpGreater { target, .. } => target.0,
                    Instruction::JumpGreaterEqual { target, .. } => target.0,
                    Instruction::JumpLess { target, .. } => target.0,
                    Instruction::JumpLessEqual { target, .. } => target.0,
                    _ => continue,
                };
                if let Some(&target_pos) = label_positions.get(&target_id) {
                    if target_pos < i {
                        back_edges.push((i, target_pos));
                        log::info!(
                            "🔄 发现循环回边: 指令 {} → 指令 {} (Label {})",
                            i, target_pos, target_id
                        );
                    }
                }
            }

            // 第三步：对每个回边，扩展所有与循环范围有交集的寄存器生命周期
            //
            // 关键修复：之前的实现只扩展循环指令中"出现"的寄存器，
            // 但一个在循环前部定义和使用的寄存器（如 tagged union 缓冲地址）
            // 可能不出现于循环后部（如 bb9），导致其生命周期未被扩展到
            // 覆盖后部代码，寄存器分配器将其与后部的中间值分配到同一物理寄存器，
            // 循环第二次迭代时物理寄存器已被覆写，导致 SIGSEGV。
            //
            // 正确做法：遍历所有寄存器，只要生命周期与循环范围有交集就扩展。
            for &(jump_pos, target_pos) in &back_edges {
                let loop_start = target_pos; // 循环头（Label 位置）
                let loop_end = jump_pos;     // 回边跳转指令位置

                // 扩展所有与循环范围有交集的寄存器（不仅仅是循环体中出现的）
                for (reg, lifetime) in lifetimes.iter_mut() {
                    if lifetime.is_function_parameter {
                        continue; // 函数参数已经覆盖全范围
                    }
                    // 如果寄存器的生命周期与循环范围有交集，扩展到覆盖整个循环
                    if lifetime.start <= loop_end && lifetime.end >= loop_start {
                        let old_start = lifetime.start;
                        let old_end = lifetime.end;
                        lifetime.start = lifetime.start.min(loop_start);
                        lifetime.end = lifetime.end.max(loop_end);
                        if lifetime.start != old_start || lifetime.end != old_end {
                            log::debug!(
                                "🔄 扩展循环寄存器 {:?} 生命周期: [{}, {}] → [{}, {}]",
                                reg, old_start, old_end, lifetime.start, lifetime.end
                            );
                        }
                    }
                }
            }

            // 第四步：检测循环携带寄存器（use 在 def 之前），
            // 将其生命周期 start 扩展到函数开头（position 0）。
            //
            // 背景：循环携带寄存器的值在首次迭代时来源于循环前的物理寄存器残留值。
            // 如果寄存器分配器将该物理寄存器同时分配给了循环前的其他虚拟寄存器，
            // 首次迭代时物理寄存器中的值是错误的。
            //
            // 示例 bug：v_count_init 在 [16] 写 Physical(1)=0，
            //           v_increment 在 [73] 写 Physical(1)=32，在 [46] 读。
            //           线性生命周期 [46,73] 不与 [16,16] 重叠，分配器复用 Physical(1)。
            //           首次迭代时 [46] 读到 0 而非 32。
            //
            // 修复：对 use-before-def 的寄存器，将 start 扩展到 0，
            //       确保它与所有其他寄存器的生命周期冲突，从而获得独立的物理寄存器。
            if !back_edges.is_empty() {
                // 收集每个寄存器的首次定义位置
                let mut first_def_positions: HashMap<Register, usize> = HashMap::new();
                for (i, instruction) in function.instructions.iter().enumerate() {
                    if let Some(def_reg) = instruction.get_def_register() {
                        first_def_positions.entry(def_reg).or_insert(i);
                    }
                }

                // 对每个非函数参数寄存器，检查是否 use-before-def
                for (reg, lifetime) in lifetimes.iter_mut() {
                    if lifetime.is_function_parameter {
                        continue;
                    }

                    let first_def = first_def_positions.get(&lifetime.register).copied();
                    // lifetime.uses 包含所有出现位置（def + use）
                    // 如果第一个出现位置的指令不是 def，说明 use 在 def 之前
                    if let (Some(def_pos), Some(&first_occurrence)) =
                        (first_def, lifetime.uses.first())
                    {
                        if first_occurrence < def_pos {
                            // 循环携带寄存器：start 扩展到 0
                            let old_start = lifetime.start;
                            lifetime.start = 0;
                            if old_start != 0 {
                                log::debug!(
                                    "🔄 循环携带寄存器 {:?} (首次出现={}, 首次定义={}), 生命周期 start 扩展: {} → 0",
                                    reg, first_occurrence, def_pos, old_start
                                );
                            }
                        }
                    }
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
    /// 这是一个降级的分析方法，当 LivenessAnalysis 不可用时使用。
    /// 它使用 Def-Use 链来计算生命周期，但不使用活跃度信息精确化结束点。
    ///
    /// @param function - 需要分析的LIR函数。
    /// @param _cfg - 函数的控制流图（保留用于向后兼容，但不使用）。
    /// @param def_use - 函数的Def-Use链。
    /// @returns 一个元组，包含 (生命周期列表, 寄存器类型映射)。
    pub fn analyze_with_cfg(
        &self,
        function: &LirFunction,
        _cfg: &ControlFlowGraph,
        def_use: &DefUseChains,
    ) -> (Vec<RegisterLifetime>, HashMap<Register, RegisterType>) {
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
                live_ranges: vec![], // CFG模式不计算精确范围
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

            // 注意：这个版本不使用活跃度分析精确化结束点
            // 如果需要更精确的分析，应使用 analyze_with_liveness 方法

            // 🔧 修复：确保所有寄存器都有类型信息
            register_types.entry(register).or_insert(RegisterType::Data);
            let register_type = register_types.get(&register).unwrap();

            lifetimes.insert(
                register,
                RegisterLifetime {
                    register,
                    start,
                    end, // 直接使用 def-use 计算的结束点
                    uses,
                    is_function_parameter: false,
                    parameter_index: None,
                    register_type: *register_type,
                    live_ranges: vec![],
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
                            live_ranges: vec![],
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

    /// 使用活跃度分析进行精确的生命周期计算
    ///
    /// 这是最精确的分析方法，它直接使用 LivenessAnalysisPass 的结果来计算生命周期。
    /// 它会计算每个寄存器的精确活跃区间 (live_ranges)，正确处理钻石形 CFG 等
    /// 复杂控制流场景。
    ///
    /// @param function - 需要分析的LIR函数。
    /// @param cfg - 函数的控制流图。
    /// @param def_use - 函数的Def-Use链。
    /// @param liveness - 活跃度分析结果（由 LivenessAnalysisPass 提供）。
    /// @returns 一个元组，包含 (生命周期列表, 寄存器类型映射)。
    pub fn analyze_with_liveness(
        &self,
        function: &LirFunction,
        _cfg: &ControlFlowGraph,
        def_use: &DefUseChains,
        liveness: &LivenessAnalysis,
    ) -> (Vec<RegisterLifetime>, HashMap<Register, RegisterType>) {
        let mut lifetimes = HashMap::new();
        let mut register_types = self.analyze_register_types(function);

        // 使用 LivenessAnalysis 中预构建的反向索引
        // register → 该寄存器活跃的指令位置列表（已在 liveness 分析中排序去重）
        let register_live_instructions = &liveness.register_live_instructions;

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
                live_ranges: vec![(0, function.instructions.len().saturating_sub(1))],
            };
            lifetimes.insert(param_reg, lifetime);
        }

        // 对每个寄存器计算精确的生命周期
        for (&register, def_positions) in &def_use.definitions {
            if lifetimes.contains_key(&register) {
                continue; // 跳过函数参数
            }

            let mut start = usize::MAX;
            let mut end = 0;
            let mut uses = Vec::new();

            // 计算第一次定义的位置
            for &def_pos in def_positions {
                start = start.min(def_pos);
                end = end.max(def_pos);
            }

            // 收集所有使用位置
            if let Some(use_positions) = def_use.uses.get(&register) {
                for &use_pos in use_positions {
                    uses.push(use_pos);
                    end = end.max(use_pos);
                }
            }

            // 🔧 新增：计算精确的活跃区间
            // 使用预计算的 register → 活跃指令列表，避免 O(registers × instructions) 遍历
            let live_ranges = self.compute_live_ranges_fast(
                register,
                def_positions,
                def_use.uses.get(&register),
                register_live_instructions.get(&register),
            );

            // 如果有精确的 live_ranges，更新 start 和 end
            let (refined_start, refined_end) = if !live_ranges.is_empty() {
                let min_start = live_ranges.iter().map(|r| r.0).min().unwrap_or(start);
                let max_end = live_ranges.iter().map(|r| r.1).max().unwrap_or(end);
                (min_start, max_end)
            } else {
                (start, end)
            };

            // 🔧 修复：确保所有寄存器都有类型信息
            register_types.entry(register).or_insert(RegisterType::Data);
            let register_type = register_types.get(&register).unwrap();

            lifetimes.insert(
                register,
                RegisterLifetime {
                    register,
                    start: refined_start,
                    end: refined_end,
                    uses,
                    is_function_parameter: false,
                    parameter_index: None,
                    register_type: *register_type,
                    live_ranges,
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
                            live_ranges: vec![(i, i)],
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

    /// 使用预计算的反向索引快速计算活跃区间
    ///
    /// 与 `compute_live_ranges` 不同，此方法使用 `register → 活跃指令列表` 的预计算索引，
    /// 将复杂度从 O(registers × instructions) 降到 O(k)（k=该寄存器活跃的指令数）。
    fn compute_live_ranges_fast(
        &self,
        register: Register,
        def_positions: &[usize],
        use_positions: Option<&Vec<usize>>,
        live_instructions: Option<&Vec<usize>>,
    ) -> Vec<(usize, usize)> {
        let mut live_indices: Vec<usize> = Vec::new();

        for &def_pos in def_positions {
            live_indices.push(def_pos);
        }

        if let Some(uses) = use_positions {
            for &use_pos in uses {
                live_indices.push(use_pos);
            }
        }

        if let Some(instrs) = live_instructions {
            live_indices.extend_from_slice(instrs);
        }

        live_indices.sort_unstable();
        live_indices.dedup();

        if live_indices.is_empty() {
            return vec![];
        }

        let mut ranges = Vec::new();
        let mut range_start = live_indices[0];
        let mut range_end = live_indices[0];

        for &idx in live_indices.iter().skip(1) {
            if idx == range_end + 1 {
                range_end = idx;
            } else {
                ranges.push((range_start, range_end));
                range_start = idx;
                range_end = idx;
            }
        }
        ranges.push((range_start, range_end));

        let _ = register;
        ranges
    }

    /// 计算寄存器的精确活跃区间
    ///
    /// 通过分析活跃度信息，将连续活跃的指令合并成区间。
    /// 这样可以正确处理钻石形 CFG 等复杂控制流场景。
    ///
    /// 注意：`live_at_instruction` 记录的是"指令后"的活跃状态，
    /// 所以我们需要同时考虑定义位置和使用位置。
    fn compute_live_ranges(
        &self,
        register: Register,
        num_instructions: usize,
        live_at_instruction: &HashMap<usize, HashSet<Register>>,
        def_positions: &[usize],
        use_positions: Option<&Vec<usize>>,
    ) -> Vec<(usize, usize)> {
        // 收集所有该寄存器活跃的指令索引
        let mut live_indices: Vec<usize> = Vec::new();

        // 定义点算活跃
        for &def_pos in def_positions {
            live_indices.push(def_pos);
        }

        // 使用点也算活跃（因为 live_at_instruction 是"指令后"状态，
        // 可能不包含最后一次使用的指令）
        if let Some(uses) = use_positions {
            for &use_pos in uses {
                live_indices.push(use_pos);
            }
        }

        // 从活跃度分析中收集（"指令后"状态）
        for instr_idx in 0..num_instructions {
            if let Some(live_set) = live_at_instruction.get(&instr_idx) {
                if live_set.contains(&register) {
                    // 如果寄存器在指令后活跃，那么它在该指令和下一条指令都需要值
                    live_indices.push(instr_idx);
                }
            }
        }

        // 去重并排序
        live_indices.sort();
        live_indices.dedup();

        if live_indices.is_empty() {
            return vec![];
        }

        // 将连续的索引合并成区间
        let mut ranges = Vec::new();
        let mut range_start = live_indices[0];
        let mut range_end = live_indices[0];

        for &idx in live_indices.iter().skip(1) {
            if idx == range_end + 1 {
                // 连续，扩展当前区间
                range_end = idx;
            } else {
                // 不连续，保存当前区间并开始新区间
                ranges.push((range_start, range_end));
                range_start = idx;
                range_end = idx;
            }
        }

        // 保存最后一个区间
        ranges.push((range_start, range_end));

        ranges
    }

    /// 使用活跃度信息精确化生命周期结束点
    ///
    /// 找到寄存器最后一次出现在活跃集合中的位置。
    fn refine_end_with_liveness(
        &self,
        register: Register,
        initial_end: usize,
        live_at_instruction: &HashMap<usize, HashSet<Register>>,
    ) -> usize {
        // 🔧 修复：找到寄存器最后一次活跃的位置（最大的指令索引）
        // 遍历所有指令，找到包含该寄存器的活跃集合的最大索引
        let mut max_live_idx = None;

        for (&instr_idx, live_set) in live_at_instruction {
            if live_set.contains(&register) {
                max_live_idx = match max_live_idx {
                    None => Some(instr_idx),
                    Some(current_max) => Some(current_max.max(instr_idx)),
                };
            }
        }

        // 如果找到了活跃位置，使用最大索引；否则使用初始结束位置
        max_live_idx.unwrap_or(initial_end)
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
