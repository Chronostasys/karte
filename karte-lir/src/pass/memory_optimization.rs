//! 综合内存优化 Pass
//!
//! 包含三个子优化，用于减少虚拟栈上的冗余 Load64/Store64：
//!
//! 1. **Store-Load Forwarding (SLF)**: Store64 [addr+off] = val 后续
//!    Load64 r, [addr+off] → Move r, val。
//!
//! 2. **Redundant Load Elimination (RLE)**: 如果同一地址最近被 Load64 过
//!    且中间没有 Store64 修改，直接复用之前 Load 的结果寄存器。
//!
//! 3. **Dead Store Elimination (DSE)**: 如果 Store64 的值在该地址被再次
//!    Store64 之前没有被 Load64 读取，删除该 Store64。
//!
//! 4. **Value Propagation**: 转发产生的 Move 建立值映射，后续使用处传播。
//!
//! 安全性：
//! - 控制流标签/跳转时清空所有映射（保守安全）
//! - Call/CallIndirect 清空所有映射
//! - 只处理虚拟寄存器间的传播（Physical 寄存器不传播）
//! - 地址寄存器被重定义时清除相关映射

use super::instruction_transformer::IndexInstructionTransformer;
use super::{AnalysisManager, FunctionPass, PassResult};
use crate::{Instruction, LirFunction, Operand, Register};

/// 判断寄存器是否为物理寄存器
fn is_physical(reg: Register) -> bool {
    matches!(reg, Register::Physical(_))
}

/// 获取指令中所有被定义（写入）的寄存器
fn defs(instr: &Instruction) -> Vec<Register> {
    match instr {
        Instruction::Move { dst, .. }
        | Instruction::Load64 { dst, .. }
        | Instruction::Add { dst, .. }
        | Instruction::Sub { dst, .. }
        | Instruction::Mul { dst, .. }
        | Instruction::Div { dst, .. }
        | Instruction::Mod { dst, .. }
        | Instruction::BitAnd { dst, .. }
        | Instruction::BitOr { dst, .. }
        | Instruction::BitXor { dst, .. }
        | Instruction::BitNot { dst, .. }
        | Instruction::IntCast { dst, .. }
        | Instruction::ShiftLeft { dst, .. }
        | Instruction::ShiftRight { dst, .. }
        | Instruction::CompareSet { dst, .. }
        | Instruction::Alloc { dst, .. }
        | Instruction::LoadGlobal { dst, .. }
        | Instruction::Phi { dst, .. }
        | Instruction::StructAlloc { dst, .. }
        | Instruction::StructFieldLoad { dst, .. }
        | Instruction::StructFieldAddr { dst, .. }
        | Instruction::StringConcat { dst, .. }
        | Instruction::StringEqual { dst, .. }
        | Instruction::StringCharAt { dst, .. }
        | Instruction::StringSubstring { dst, .. }
        | Instruction::StringCompare { dst, .. }
        | Instruction::CharToString { dst, .. }
        | Instruction::GcAlloc { dst, .. }
        | Instruction::MemLoad64 { dst, .. }
        | Instruction::ToString { dst, .. } => vec![*dst],
        Instruction::LoadPair { dst1, dst2, .. } => vec![*dst1, *dst2],
        Instruction::Call { result, .. } | Instruction::CallIndirect { result, .. } => {
            result.map(|r| vec![r]).unwrap_or_default()
        }
        _ => vec![],
    }
}

/// 获取操作数中的寄存器
fn op_reg(op: &Operand) -> Option<Register> {
    match op {
        Operand::Register { id } => Some(*id),
        _ => None,
    }
}

/// 内存位置标识
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct MemLoc {
    addr: Register,
    offset: i64,
}

/// Store 信息
struct StoreInfo {
    value: Operand,
    idx: usize,
    read: bool,
}

pub struct MemoryOptimizationPass;

impl MemoryOptimizationPass {
    pub fn new() -> Self { Self }

    fn optimize_function(&self, function: &mut LirFunction) -> bool {
        use std::collections::HashMap;

        let mut any_changed = false;

        // 多轮迭代直到不动点（最多 5 轮）
        for _iteration in 0..5 {
            let mut transformer = IndexInstructionTransformer::new();

            // (addr, offset) -> Store 信息
            let mut store_map: HashMap<MemLoc, StoreInfo> = HashMap::new();
            // (addr, offset) -> Load 结果寄存器
            let mut load_cache: HashMap<MemLoc, Register> = HashMap::new();
            // 值传播：Register -> Operand
            let mut value_map: HashMap<Register, Operand> = HashMap::new();
            // 已删除的 Store 索引集合
            let mut dead_stores: Vec<usize> = Vec::new();

            let mut changed_this_round = false;

            for (i, instr) in function.instructions.iter().enumerate() {
                // 控制流边界：清空 store_map 和 load_cache（安全性要求）
                // 但保留 value_map（寄存器间的 copy propagation 跨 Label 安全）
                if matches!(instr,
                    Instruction::Label { .. }
                    | Instruction::Jump { .. }
                    | Instruction::JumpEqual { .. }
                    | Instruction::JumpNotEqual { .. }
                    | Instruction::JumpGreater { .. }
                    | Instruction::JumpGreaterEqual { .. }
                    | Instruction::JumpLess { .. }
                    | Instruction::JumpLessEqual { .. }
                ) {
                    store_map.clear();
                    load_cache.clear();
                    // 注意：不清空 value_map！寄存器值不因控制流变化。
                    // 但 value_map 中的条目如果有 Store64 依赖，在 Call 时已被清空。
                    continue;
                }

                match instr {
                    // ======== Store64 处理 ========
                    Instruction::Store64 { addr, offset, src, span: _ } => {
                        let loc = MemLoc { addr: *addr, offset: *offset };

                        // DSE: 同地址前一个 Store 没被 Read -> 删除前一个
                        if let Some(old_store) = store_map.get_mut(&loc) {
                            if !old_store.read {
                                dead_stores.push(old_store.idx);
                                changed_this_round = true;
                            }
                        }

                        // 用 value_map 传播 src
                        let effective_src = if let Some(reg) = op_reg(src) {
                            if let Some(replacement) = value_map.get(&reg) {
                                changed_this_round = true;
                                replacement.clone()
                            } else {
                                src.clone()
                            }
                        } else {
                            src.clone()
                        };

                        store_map.insert(loc, StoreInfo {
                            value: effective_src,
                            idx: i,
                            read: false,
                        });

                        // 清除同地址的 load 缓存（值已变）
                        load_cache.remove(&loc);
                    }

                    // ======== Load64 处理 ========
                    Instruction::Load64 { dst, addr, offset, span } => {
                        let loc = MemLoc { addr: *addr, offset: *offset };

                        // SLF: Store->Load 转发
                        if let Some(store_info) = store_map.get_mut(&loc) {
                            store_info.read = true;
                            if *dst != *addr {
                                transformer.replace(i, Instruction::Move {
                                    dst: *dst,
                                    src: store_info.value.clone(),
                                    span: *span,
                                });
                                // 建立值传播（物理寄存器也可以，因为有重定义追踪）
                                value_map.insert(*dst, store_info.value.clone());
                                changed_this_round = true;
                            }
                        }
                        // RLE: 同地址重复 Load
                        else if let Some(&cached_dst) = load_cache.get(&loc) {
                            if *dst != *addr && *dst != cached_dst {
                                transformer.replace(i, Instruction::Move {
                                    dst: *dst,
                                    src: Operand::Register { id: cached_dst },
                                    span: *span,
                                });
                                value_map.insert(*dst, Operand::Register { id: cached_dst });
                                changed_this_round = true;
                            }
                        } else {
                            // 第一次 Load 这个地址，缓存
                            load_cache.insert(loc, *dst);
                        }
                    }

                    // ======== Store8 可能影响内存 ========
                    Instruction::Store8 { addr, .. } => {
                        store_map.retain(|k, _| k.addr != *addr);
                        load_cache.retain(|k, _| k.addr != *addr);
                    }

                    // ======== StorePair/LoadPair ========
                    Instruction::StorePair { addr, .. } => {
                        store_map.retain(|k, _| k.addr != *addr);
                        load_cache.retain(|k, _| k.addr != *addr);
                    }
                    Instruction::LoadPair { dst1, dst2, addr, offset, span } => {
                        let loc = MemLoc { addr: *addr, offset: *offset };

                        if let Some(store_info) = store_map.get_mut(&loc) {
                            store_info.read = true;
                            transformer.replace(i, Instruction::Move {
                                dst: *dst1,
                                src: store_info.value.clone(),
                                span: *span,
                            });
                            changed_this_round = true;
                        } else {
                            load_cache.insert(loc, *dst1);
                        }
                    }

                    // ======== Move: 值传播 ========
                    Instruction::Move { dst, src, span } => {
                        let effective_src = if let Some(reg) = op_reg(src) {
                            value_map.get(&reg).cloned().unwrap_or_else(|| src.clone())
                        } else {
                            src.clone()
                        };
                        if effective_src != *src {
                            transformer.replace(i, Instruction::Move {
                                dst: *dst,
                                src: effective_src.clone(),
                                span: *span,
                            });
                            value_map.insert(*dst, effective_src);
                            changed_this_round = true;
                        } else {
                            // 普通 Move 也建立值传播（含传递闭包）
                            if let Operand::Register { id } = src {
                                if *id != *dst {
                                    let root = value_map.get(id).cloned().unwrap_or_else(|| src.clone());
                                    value_map.insert(*dst, root);
                                }
                            }
                        }
                    }

                    // ======== Call/CallIndirect 清空 ========
                    Instruction::Call { .. } | Instruction::CallIndirect { .. } => {
                        store_map.clear();
                        load_cache.clear();
                        value_map.retain(|r, _| !is_physical(*r));
                    }

                    // ======== 算术指令：清除 value_map 中被定义的寄存器 ========
                    _ => {
                        // 其他指令的 dst 会使 value_map 中的对应条目失效
                        // 在下面的寄存器重定义处理中统一清除
                    }
                }

                // 清除因寄存器重定义而失效的映射
                for def_reg in defs(instr) {
                    store_map.retain(|k, _| k.addr != def_reg);
                    load_cache.retain(|k, _| k.addr != def_reg);
                    store_map.retain(|_, v| {
                        op_reg(&v.value).map_or(true, |id| id != def_reg)
                    });
                    value_map.remove(&def_reg);
                    value_map.retain(|_, v| {
                        op_reg(v).map_or(true, |id| id != def_reg)
                    });
                    load_cache.retain(|_, &mut dst| dst != def_reg);
                }
            }

            // 删除 dead stores
            for idx in dead_stores {
                transformer.remove(idx);
            }

            let (applied, ..) = transformer.apply_to_function(function);
            if changed_this_round || applied {
                any_changed = true;
            } else {
                break; // 不动点
            }
        }

        any_changed
    }
}

impl Default for MemoryOptimizationPass {
    fn default() -> Self { Self::new() }
}

impl FunctionPass for MemoryOptimizationPass {
    fn name(&self) -> &str { "memory_optimization" }
    fn description(&self) -> &str { "综合内存优化: SLF + RLE + DSE + Value Propagation" }
    fn run_on_function(&mut self, f: &mut LirFunction, _: &mut AnalysisManager) -> PassResult {
        if self.optimize_function(f) { PassResult::Changed } else { PassResult::Unchanged }
    }
    fn invalidated_analyses(&self) -> Vec<&'static str> { vec!["def-use", "memory"] }
}
