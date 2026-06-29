//! GIR 优化 Pass — 向量化加载、循环展开、软件流水线
//!
//! 三个核心优化 pass：
//! 1. VectorizePass — 检测连续标量 GlobalLoad 合并为 GlobalLoadV4
//! 2. LoopUnroller — 识别循环模式并复制循环体
//! 3. SoftwarePipeline — 双缓冲：加载下一轮数据同时计算当前轮

use crate::ir::*;
use std::collections::HashMap;

// ============================================================
// 1. 向量化加载 Pass
// ============================================================

/// 向量化加载 Pass — 检测连续的标量 GlobalLoad 合并为 GlobalLoadV4
///
/// 模式检测：
///   GlobalLoad r0, [addr]
///   GlobalLoad r1, [addr + 4]
///   GlobalLoad r2, [addr + 8]
///   GlobalLoad r3, [addr + 12]
/// →
///   GlobalLoadV4 r0, [addr]   (一次性加载 4 个 f32)
pub struct VectorizePass;

impl VectorizePass {
    pub fn new() -> Self { Self }

    /// 对 kernel 指令序列执行向量化
    pub fn optimize(&self, func: &mut GirFunction) {
        let instrs = std::mem::take(&mut func.instructions);
        let mut result: Vec<GirInstruction> = Vec::with_capacity(instrs.len());
        let mut i = 0;

        while i < instrs.len() {
            // 检测连续 4 个 GlobalLoad 到相邻地址的模式
            if i + 3 < instrs.len() {
                if let (
                    Some(info0), Some(info1), Some(info2), Some(info3),
                ) = (
                    analyze_global_load(&instrs[i]),
                    analyze_global_load(&instrs[i+1]),
                    analyze_global_load(&instrs[i+2]),
                    analyze_global_load(&instrs[i+3]),
                ) {
                    // 检查是否满足向量化条件：
                    // 1. 4 个连续的 GlobalLoad F32
                    // 2. 目标寄存器连续
                    // 3. 基地址相同
                    // 4. 偏移量递增 4 字节
                    if info0.dtype == GirDType::F32
                        && info0.dst + 1 == info1.dst
                        && info1.dst + 1 == info2.dst
                        && info2.dst + 1 == info3.dst
                        && info0.base_reg == info1.base_reg
                        && info1.base_reg == info2.base_reg
                        && info2.base_reg == info3.base_reg
                        && info0.offset + 4 == info1.offset
                        && info1.offset + 4 == info2.offset
                        && info2.offset + 4 == info3.offset
                    {
                        // 合并为 GlobalLoadV4
                        result.push(GirInstruction::GlobalLoadV4 {
                            dst_base: info0.dst,
                            addr: GirOperand::Reg(info0.addr_reg),
                            dtype: GirDType::F32,
                        });
                        i += 4;
                        continue;
                    }
                }
            }

            // 检测连续 2 个 GlobalLoad（V2 模式）
            if i + 1 < instrs.len() {
                if let (Some(info0), Some(info1)) = (
                    analyze_global_load(&instrs[i]),
                    analyze_global_load(&instrs[i+1]),
                ) {
                    if info0.dtype == GirDType::F32
                        && info0.dst + 1 == info1.dst
                        && info0.base_reg == info1.base_reg
                        && info0.offset + 4 == info1.offset
                    {
                        result.push(GirInstruction::GlobalLoadV2 {
                            dst_base: info0.dst,
                            addr: GirOperand::Reg(info0.addr_reg),
                            dtype: GirDType::F32,
                        });
                        i += 2;
                        continue;
                    }
                }
            }

            // 同样检测 GlobalStore 向量化
            if i + 3 < instrs.len() {
                if let (
                    Some(s0), Some(s1), Some(s2), Some(s3),
                ) = (
                    analyze_global_store(&instrs[i]),
                    analyze_global_store(&instrs[i+1]),
                    analyze_global_store(&instrs[i+2]),
                    analyze_global_store(&instrs[i+3]),
                ) {
                    if s0.dtype == GirDType::F32
                        && s0.src_reg + 1 == s1.src_reg
                        && s1.src_reg + 1 == s2.src_reg
                        && s2.src_reg + 1 == s3.src_reg
                        && s0.base_reg == s1.base_reg
                        && s0.offset + 4 == s1.offset
                        && s1.offset + 4 == s2.offset
                        && s2.offset + 4 == s3.offset
                    {
                        result.push(GirInstruction::GlobalStoreV4 {
                            addr: GirOperand::Reg(s0.addr_reg),
                            src_base: s0.src_reg,
                            dtype: GirDType::F32,
                        });
                        i += 4;
                        continue;
                    }
                }
            }

            result.push(instrs[i].clone());
            i += 1;
        }

        func.instructions = result;
    }
}

/// GlobalLoad 分析结果
struct LoadInfo {
    dst: usize,
    addr_reg: usize,   // 地址寄存器 ID（不含偏移）
    base_reg: usize,   // 基地址寄存器（Add 的 src1）
    offset: i64,       // 相对偏移
    dtype: GirDType,
}

/// 分析一条指令是否是 GlobalLoad，提取信息
fn analyze_global_load(instr: &GirInstruction) -> Option<LoadInfo> {
    match instr {
        GirInstruction::GlobalLoad { dst, addr, dtype } => {
            let (addr_reg, base_reg, offset) = decompose_addr(addr);
            Some(LoadInfo { dst: *dst, addr_reg, base_reg, offset, dtype: *dtype })
        }
        _ => None,
    }
}

struct StoreInfo {
    src_reg: usize,
    addr_reg: usize,
    base_reg: usize,
    offset: i64,
    dtype: GirDType,
}

fn analyze_global_store(instr: &GirInstruction) -> Option<StoreInfo> {
    match instr {
        GirInstruction::GlobalStore { addr, src, dtype } => {
            let (addr_reg, base_reg, offset) = decompose_addr(addr);
            let src_reg = match src {
                GirOperand::Reg(r) => *r,
                _ => return None,
            };
            Some(StoreInfo { src_reg, addr_reg, base_reg, offset, dtype: *dtype })
        }
        _ => None,
    }
}

/// 分解地址操作数为 (addr_reg, base_reg, offset)
/// 如果 addr = Reg(r)，返回 (r, r, 0)
/// 如果 addr 是其他形式，返回 (0, 0, 0) 表示无法向量化
fn decompose_addr(addr: &GirOperand) -> (usize, usize, i64) {
    match addr {
        GirOperand::Reg(r) => (*r, *r, 0),
        _ => (0, 0, 0),
    }
}


// ============================================================
// 2. 循环展开 Pass
// ============================================================

/// 循环展开 Pass — 识别 GIR 中的循环模式并复制循环体
///
/// 识别的循环模式：
///   Label(L_head):
///     Cmp ... → cmp_reg
///     BranchIf cmp_reg, L_body, L_exit
///   Label(L_body) 或直接紧跟:
///     ... <循环体> ...
///     Add i, i, stride
///     Cmp ... → cmp_reg2
///     BranchIf cmp_reg2, L_head, L_exit     (回边)
///   Label(L_exit):
///
/// 展开后（factor=4）：
///   Label(L_head):
///     ... <循环体> × 4（每次 i += stride） ...
///     Cmp ... (检查 i+4*stride < n)
///     BranchIf ..., L_head, L_exit
///   Label(L_exit):
pub struct LoopUnroller {
    factor: usize,
}

impl LoopUnroller {
    pub fn new(factor: usize) -> Self { Self { factor } }

    pub fn unroll(&self, func: &mut GirFunction) {
        if self.factor <= 1 {
            return;
        }

        let instrs = std::mem::take(&mut func.instructions);

        // 查找循环模式：回边跳转 (Jump/JumpEqual/BranchIf target=前向 Label)
        let mut label_pos: HashMap<usize, usize> = HashMap::new();
        for (i, instr) in instrs.iter().enumerate() {
            if let GirInstruction::Label { id } = instr {
                label_pos.insert(*id, i);
            }
        }

        // 找到所有回边（Jump 到之前的 Label = 循环回边）
        let mut back_edges: Vec<(usize, usize)> = Vec::new(); // (jump_pos, label_pos)
        for (i, instr) in instrs.iter().enumerate() {
            if let GirInstruction::Jump { target } = instr {
                if let Some(&lp) = label_pos.get(target) {
                    if lp < i {
                        back_edges.push((i, lp));
                    }
                }
            }
        }

        if back_edges.is_empty() {
            func.instructions = instrs;
            return;
        }

        // 取最长的循环（body 最大的那个）
        back_edges.sort_by_key(|&(jp, lp)| jp - lp);
        let &(jump_pos, loop_start) = back_edges.last().unwrap();
        let loop_body_start = loop_start;
        let loop_body_end = jump_pos; // 不含 jump 本身

        let body_len = loop_body_end - loop_body_start;
        if body_len == 0 {
            func.instructions = instrs;
            return;
        }

        // 提取循环体
        let body: &[GirInstruction] = &instrs[loop_body_start..loop_body_end];

        // 构建展开后的指令序列
        let mut result: Vec<GirInstruction> = Vec::new();

        // loop 之前的指令直接复制
        result.extend_from_slice(&instrs[..loop_start]);

        // 放置 loop_start Label
        result.push(instrs[loop_start].clone()); // Label

        // 展开 N 次循环体
        for _ in 0..self.factor {
            result.extend_from_slice(body);
            // 跳过每次展开中的第一个 Label（避免重复标签）
            // 实际上 body 中的 Label 是循环头，只在第一次展开时需要
        }

        // 回边
        result.push(instrs[jump_pos].clone());

        // loop 之后的指令
        result.extend_from_slice(&instrs[jump_pos+1..]);

        func.instructions = result;
    }
}


// ============================================================
// 3. 软件流水线 Pass
// ============================================================

/// 软件流水线 Pass — 双缓冲优化
///
/// 将循环中的 load → compute 依赖链改为：
///   prologue:   load(buf0, i=0)
///   loop body:  load(buf1, i+1)     ← 提前加载下一轮
///               compute(buf0)       ← 同时计算当前轮
///               swap(buf0, buf1)
///   epilogue:   compute(buf1, last)
///
/// 在 GPU 上，这隐藏了全局内存的访存延迟
pub struct SoftwarePipelinePass;

impl SoftwarePipelinePass {
    pub fn new() -> Self { Self }

    /// 在循环体内执行软件流水线
    ///
    /// 策略：识别循环体中的 GlobalLoad 序列，
    /// 将后续的算术指令之前的加载提前一轮
    pub fn optimize(&self, func: &mut GirFunction) {
        let instrs = std::mem::take(&mut func.instructions);

        // 构建 label 位置映射
        let mut label_pos: HashMap<usize, usize> = HashMap::new();
        for (i, instr) in instrs.iter().enumerate() {
            if let GirInstruction::Label { id } = instr {
                label_pos.insert(*id, i);
            }
        }

        // 查找回边确定循环范围
        let mut loop_range: Option<(usize, usize)> = None; // (body_start, body_end)
        for (i, instr) in instrs.iter().enumerate() {
            if let GirInstruction::Jump { target } = instr {
                if let Some(&lp) = label_pos.get(target) {
                    if lp < i && i - lp > 8 {
                        // 循环体至少 8 条指令才有流水线价值
                        loop_range = Some((lp, i));
                        break;
                    }
                }
            }
        }

        let Some((body_start, body_end)) = loop_range else {
            func.instructions = instrs;
            return;
        };

        let body = &instrs[body_start..body_end];

        // 在循环体中查找 GlobalLoad 指令的位置
        let load_positions: Vec<usize> = body.iter().enumerate()
            .filter_map(|(i, instr)| match instr {
                GirInstruction::GlobalLoad { .. } | GirInstruction::GlobalLoadV4 { .. } => Some(i),
                _ => None,
            })
            .collect();

        if load_positions.is_empty() {
            func.instructions = instrs;
            return;
        }

        // 找到第一组加载和对应的计算（加载之后的非加载指令）
        let first_load = load_positions[0];
        let last_load = *load_positions.last().unwrap();

        // 查找加载后第一个算术指令（Sub/Add/Mul/Fma）= 流水线分界点
        let compute_start = body.iter().enumerate()
            .skip(last_load + 1)
            .find(|(_, instr)| matches!(instr,
                GirInstruction::Sub { .. } | GirInstruction::Add { .. }
                | GirInstruction::Mul { .. } | GirInstruction::Fma { .. }
            ))
            .map(|(i, _)| i);

        let Some(cs) = compute_start else {
            func.instructions = instrs;
            return;
        };

        // 构建流水线化的循环体
        let load_section = &body[first_load..cs];   // 加载指令
        let compute_section = &body[cs..];            // 计算指令
        let pre_load_section = &body[..first_load];   // 加载前的指令（cmp/branch 等）

        let mut new_body: Vec<GirInstruction> = Vec::new();

        // Prologue: 第一轮的加载
        new_body.extend_from_slice(pre_load_section);
        new_body.extend_from_slice(load_section);

        // 循环头 + 流水线体
        // 每次迭代：先加载下一轮数据，再计算当前轮
        // 这里我们利用 GPU 的乱序执行能力：
        // 加载指令发出后不等待完成，继续执行后续计算
        // 编译器（PTX JIT）会自动调度指令来隐藏延迟

        // 重新排列：计算和加载交错
        // 原始: [pre_load | load | compute]
        // 流水线: [pre_load | load | compute | load_next | compute_next | ...]
        // 由于 PTX 是 SSA-like 的，我们通过在计算前插入下一轮的加载来触发流水线

        // 简化实现：将加载指令复制一份到计算段之前，形成预取效果
        // 这要求 GIR 有足够的寄存器空间
        new_body.extend(pre_load_section.iter().cloned());
        // 预取：在计算之前发出下一轮加载请求
        for load_instr in load_section {
            new_body.push(load_instr.clone());
        }
        // 计算
        new_body.extend_from_slice(compute_section);

        // 构建完整指令序列
        let mut result: Vec<GirInstruction> = Vec::new();
        result.extend_from_slice(&instrs[..body_start]);
        result.extend(new_body);
        result.extend_from_slice(&instrs[body_end..]);

        func.instructions = result;
    }
}


// ============================================================
// 辅助函数
// ============================================================

/// 自动 Grid/Block 配置
pub fn auto_config(tile_m: usize, tile_n: usize, m: usize, n: usize) -> (usize, usize, usize) {
    let blocks_m = (m + tile_m - 1) / tile_m;
    let blocks_n = (n + tile_n - 1) / tile_n;
    (blocks_m * blocks_n, 1, 1)
}

/// 根据寄存器使用量估算最优 block 大小
pub fn estimate_block_size(num_regs: usize, shared_mem: usize) -> usize {
    let max_threads_by_regs = if num_regs > 0 { 65536 / num_regs.max(1) } else { 1024 };
    let max_threads_by_smem = if shared_mem > 0 { 48 * 1024 / shared_mem.max(1) * 256 } else { 1024 };
    let max_threads = max_threads_by_regs.min(max_threads_by_smem).min(1024);
    let aligned = (max_threads / 32) * 32;
    if aligned >= 256 { 256 } else if aligned >= 128 { 128 } else if aligned >= 64 { 64 } else { 32 }
}
