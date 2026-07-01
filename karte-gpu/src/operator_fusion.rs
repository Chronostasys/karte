//! 算子融合 — 自动将连续的 element-wise kernel 合并为单个 kernel
//!
//! 核心思路:
//! 1. 分析 GIR 程序中多个 kernel 的数据流
//! 2. 如果 kernel A 的输出是 kernel B 的输入, 且两者都是 element-wise 操作
//!    则可以合并为一个 kernel: 中间结果不落全局显存
//! 3. 融合条件: 两个 kernel 的 grid/block 配置相同, 且没有 reduction 操作
//!
//! 融合示例:
//!   kernel1: out1[tid] = gelu(x[tid])       → 读 x, 写 out1
//!   kernel2: out2[tid] = out1[tid] * scale   → 读 out1, 写 out2
//!   融合后: out2[tid] = gelu(x[tid]) * scale → 读 x, 写 out2 (省 2 次显存访问)

use karte_gir::*;
use std::collections::HashMap;

/// 融合模式
#[derive(Debug, Clone, PartialEq)]
pub enum FusionPattern {
    /// 两个 element-wise kernel 串联
    ElementWiseChain,
    /// GEMM + 后续 element-wise (如 GEMM + Bias + GELU)
    GemmPostFusion,
    /// 不可融合
    NotFusable,
}

/// 算子融合器
pub struct OperatorFusion {
    /// 融合是否启用
    enabled: bool,
}

impl OperatorFusion {
    pub fn new() -> Self {
        Self { enabled: true }
    }

    pub fn disabled() -> Self {
        Self { enabled: false }
    }

    /// 分析 kernel 是否可融合
    pub fn analyze_kernel(kernel: &GirFunction) -> FusionPattern {
        // 检查是否包含 reduction 操作
        let has_reduce = kernel.instructions.iter().any(|i| matches!(i,
            GirInstruction::Reduce { .. }
        ));

        // 检查是否包含 GEMM (TileMatmul/Mma)
        let has_gemm = kernel.instructions.iter().any(|i| matches!(i,
            GirInstruction::TileMatmul { .. } | GirInstruction::Mma { .. }
        ));

        // 检查是否纯 element-wise (无 Barrier, 无 WarpShuffle, 无 Reduce)
        let has_sync = kernel.instructions.iter().any(|i| matches!(i,
            GirInstruction::Barrier | GirInstruction::WarpShuffle { .. }
        ));

        if has_reduce || has_sync {
            if has_gemm {
                return FusionPattern::GemmPostFusion;
            }
            return FusionPattern::NotFusable;
        }

        if has_gemm {
            return FusionPattern::GemmPostFusion;
        }

        FusionPattern::ElementWiseChain
    }

    /// 尝试融合程序中的连续 kernel
    /// 返回融合后的新程序
    pub fn fuse(&self, prog: &GirProgram) -> GirProgram {
        if !self.enabled || prog.kernels.len() <= 1 {
            return prog.clone();
        }

        let mut fused = GirProgram::new();
        let mut i = 0;

        while i < prog.kernels.len() {
            let current = &prog.kernels[i];

            // 尝试与下一个 kernel 融合
            if i + 1 < prog.kernels.len() {
                let next = &prog.kernels[i + 1];
                if let Some(fused_kernel) = self.try_fuse_pair(current, next) {
                    fused.add_kernel(fused_kernel);
                    i += 2; // 跳过已融合的两个 kernel
                    continue;
                }
            }

            // 无法融合, 直接保留
            fused.add_kernel(current.clone());
            i += 1;
        }

        fused
    }

    /// 尝试融合两个 kernel
    fn try_fuse_pair(&self, first: &GirFunction, second: &GirFunction) -> Option<GirFunction> {
        // 条件 1: 两个都是 element-wise
        let pat1 = Self::analyze_kernel(first);
        let pat2 = Self::analyze_kernel(second);

        if pat1 != FusionPattern::ElementWiseChain || pat2 != FusionPattern::ElementWiseChain {
            return None;
        }

        // 条件 2: block_dim 相同
        if first.block_dim != second.block_dim {
            return None;
        }

        // 条件 3: first 的输出参数是 second 的输入参数
        // 找到 first 写入的指针参数 (GlobalStore 的目标)
        let first_outputs = Self::find_output_params(first);
        let second_inputs = Self::find_input_params(second);

        // 检查是否有 first.output == second.input 的匹配
        let mut shared_params: Vec<(usize, usize)> = Vec::new();
        for &out_idx in &first_outputs {
            for &in_idx in &second_inputs {
                if first.params[out_idx].name == second.params[in_idx].name {
                    shared_params.push((out_idx, in_idx));
                }
            }
        }

        if shared_params.is_empty() {
            return None;
        }

        // 执行融合
        let mut fused = GirFunction::new(format!("{}_{}_fused", first.name, second.name));

        // 合并参数: first 的所有参数 + second 中不在 shared 中的参数
        let mut param_map_second: HashMap<usize, usize> = HashMap::new();
        for (i, p) in first.params.iter().enumerate() {
            fused.params.push(p.clone());
        }

        for (i, p) in second.params.iter().enumerate() {
            // 如果是共享参数, 映射到 first 中的对应参数
            if let Some((first_idx, _)) = shared_params.iter().find(|(_, si)| *si == i) {
                param_map_second.insert(i, *first_idx);
            } else {
                let new_idx = fused.params.len();
                fused.params.push(p.clone());
                param_map_second.insert(i, new_idx);
            }
        }

        // 合并指令
        // first 的指令直接复制 (GlobalStore 到中间参数改为写入寄存器)
        // second 的指令中, 对中间参数的 GlobalLoad 改为从寄存器读取

        // 为共享参数创建临时寄存器
        let mut intermediate_regs: HashMap<usize, usize> = HashMap::new();
        let mut next_reg = first.next_reg.max(second.next_reg);
        for &(out_idx, _) in &shared_params {
            let reg = next_reg;
            next_reg += 1;
            intermediate_regs.insert(out_idx, reg);
        }

        // 复制 first 的指令, 将 GlobalStore 到中间参数的改为 Move 到寄存器
        for instr in &first.instructions {
            match instr {
                GirInstruction::GlobalStore { addr, src, dtype } => {
                    // 检查 addr 是否是中间参数
                    if let Some(reg) = Self::check_store_to_param(addr, &shared_params.iter().map(|(o, _)| *o).collect::<Vec<_>>()) {
                        if let Some(&tmp_reg) = intermediate_regs.get(&reg) {
                            // 替换为 Move 到临时寄存器 (不写显存!)
                            fused.instructions.push(GirInstruction::Move {
                                dst: tmp_reg,
                                src: src.clone(),
                            });
                            continue;
                        }
                    }
                    fused.instructions.push(instr.clone());
                }
                _ => fused.instructions.push(instr.clone()),
            }
        }

        // 复制 second 的指令, 将 GlobalLoad 从中间参数的改为从寄存器读取
        for instr in &second.instructions {
            match instr {
                GirInstruction::GlobalLoad { dst, addr, dtype } => {
                    // 检查 addr 是否引用中间参数
                    if let Some(reg) = Self::check_load_from_param(addr, &shared_params.iter().map(|(_, s)| *s).collect::<Vec<_>>()) {
                        // second 中的参数索引映射到 first
                        if let Some(&first_param_idx) = param_map_second.get(&reg) {
                            // 如果 first_param_idx 对应中间寄存器
                            if let Some(&tmp_reg) = intermediate_regs.get(&first_param_idx) {
                                // 替换为 Move 从临时寄存器 (不读显存!)
                                fused.instructions.push(GirInstruction::Move {
                                    dst: *dst,
                                    src: GirOperand::Reg(tmp_reg),
                                });
                                continue;
                            }
                        }
                    }
                    fused.instructions.push(instr.clone());
                }
                // 替换参数引用
                GirInstruction::Add { dst, src1, src2, dtype } => {
                    fused.instructions.push(GirInstruction::Add {
                        dst: *dst,
                        src1: Self::remap_operand(src1, &param_map_second),
                        src2: Self::remap_operand(src2, &param_map_second),
                        dtype: *dtype,
                    });
                }
                GirInstruction::Mul { dst, src1, src2, dtype } => {
                    fused.instructions.push(GirInstruction::Mul {
                        dst: *dst,
                        src1: Self::remap_operand(src1, &param_map_second),
                        src2: Self::remap_operand(src2, &param_map_second),
                        dtype: *dtype,
                    });
                }
                _ => {
                    // 对其他指令, 尝试重映射参数引用
                    let mut cloned = instr.clone();
                    Self::remap_instruction_params(&mut cloned, &param_map_second);
                    fused.instructions.push(cloned);
                }
            }
        }

        fused.next_reg = next_reg;
        fused.next_label = first.next_label.max(second.next_label);
        fused.block_dim = first.block_dim;
        fused.grid_dim = first.grid_dim;
        fused.shared_mem_size = first.shared_mem_size.max(second.shared_mem_size);

        Some(fused)
    }

    /// 找到 kernel 写入的指针参数索引
    fn find_output_params(kernel: &GirFunction) -> Vec<usize> {
        let mut result = Vec::new();
        for instr in &kernel.instructions {
            if let GirInstruction::GlobalStore { addr, .. } = instr {
                if let GirOperand::Param(idx) = addr {
                    if !result.contains(idx) {
                        result.push(*idx);
                    }
                }
                // 也检查 Add(Mul(tid, sz), Param(idx)) 模式
                if let GirOperand::Reg(_) = addr {
                    // 需要追踪 addr reg 的来源 — 简化: 检查指令序列
                }
            }
        }
        // 也检查 Add(X, Param(idx)) 模式中的 Param
        for instr in &kernel.instructions {
            if let GirInstruction::GlobalStore { addr, .. } = instr {
                if let GirOperand::Reg(rid) = addr {
                    // 在指令中反向查找 rid 是否由 Add(_, Param(idx)) 产生
                    for prev in &kernel.instructions {
                        match prev {
                            GirInstruction::Add { dst, src1, src2, .. } if *dst == *rid => {
                                if let GirOperand::Param(idx) = src2 {
                                    if !result.contains(idx) {
                                        result.push(*idx);
                                    }
                                }
                                if let GirOperand::Param(idx) = src1 {
                                    if !result.contains(idx) {
                                        result.push(*idx);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        result
    }

    /// 找到 kernel 读取的指针参数索引
    fn find_input_params(kernel: &GirFunction) -> Vec<usize> {
        let mut result = Vec::new();
        for instr in &kernel.instructions {
            if let GirInstruction::GlobalLoad { addr, .. } = instr {
                if let GirOperand::Param(idx) = addr {
                    if !result.contains(idx) {
                        result.push(*idx);
                    }
                }
            }
            let _ = instr;
        }
        // 检查 Add(X, Param(idx)) 中的 Param
        for instr in &kernel.instructions {
            if let GirInstruction::GlobalLoad { addr, .. } = instr {
                if let GirOperand::Reg(rid) = addr {
                    for prev in &kernel.instructions {
                        match prev {
                            GirInstruction::Add { dst, src1, src2, .. } if *dst == *rid => {
                                if let GirOperand::Param(idx) = src2 {
                                    if !result.contains(idx) { result.push(*idx); }
                                }
                                if let GirOperand::Param(idx) = src1 {
                                    if !result.contains(idx) { result.push(*idx); }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        result
    }

    /// 检查 store 地址是否引用特定参数
    fn check_store_to_param(addr: &GirOperand, param_indices: &[usize]) -> Option<usize> {
        match addr {
            GirOperand::Param(idx) => {
                if param_indices.contains(idx) { Some(*idx) } else { None }
            }
            _ => None,
        }
    }

    /// 检查 load 地址是否引用特定参数
    fn check_load_from_param(addr: &GirOperand, param_indices: &[usize]) -> Option<usize> {
        match addr {
            GirOperand::Param(idx) => {
                if param_indices.contains(idx) { Some(*idx) } else { None }
            }
            _ => None,
        }
    }

    /// 重映射操作数中的参数索引
    fn remap_operand(operand: &GirOperand, map: &HashMap<usize, usize>) -> GirOperand {
        match operand {
            GirOperand::Param(idx) => {
                GirOperand::Param(*map.get(idx).unwrap_or(idx))
            }
            _ => operand.clone(),
        }
    }

    /// 重映射指令中的参数引用
    fn remap_instruction_params(instr: &mut GirInstruction, map: &HashMap<usize, usize>) {
        match instr {
            GirInstruction::GlobalStore { addr, .. } => {
                *addr = Self::remap_operand(addr, map);
            }
            GirInstruction::GlobalLoad { addr, .. } => {
                *addr = Self::remap_operand(addr, map);
            }
            GirInstruction::Add { src1, src2, .. } => {
                *src1 = Self::remap_operand(src1, map);
                *src2 = Self::remap_operand(src2, map);
            }
            GirInstruction::Mul { src1, src2, .. } => {
                *src1 = Self::remap_operand(src1, map);
                *src2 = Self::remap_operand(src2, map);
            }
            GirInstruction::Sub { src1, src2, .. } => {
                *src1 = Self::remap_operand(src1, map);
                *src2 = Self::remap_operand(src2, map);
            }
            GirInstruction::Div { src1, src2, .. } => {
                *src1 = Self::remap_operand(src1, map);
                *src2 = Self::remap_operand(src2, map);
            }
            GirInstruction::Fma { src1, src2, src3, .. } => {
                *src1 = Self::remap_operand(src1, map);
                *src2 = Self::remap_operand(src2, map);
                *src3 = Self::remap_operand(src3, map);
            }
            _ => {}
        }
    }
}

impl Default for OperatorFusion {
    fn default() -> Self {
        Self::new()
    }
}
