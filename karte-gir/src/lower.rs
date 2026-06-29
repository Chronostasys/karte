//! LIR → GIR 降级
//!
//! 将 LIR 指令序列降级为 GIR（GPU 中间表示），处理：
//! 1. 标量指令 1:1 映射
//! 2. GPU 特定内建函数调用转换为 GIR 指令
//! 3. 内存操作标注为 Global/Shared

use crate::ir::*;
use karte_lir::ir::{
    Instruction as LirInstr, Operand as LirOperand, Register,
    ComparisonCondition,
};
use std::collections::HashMap;

/// Kernel 元信息
#[derive(Debug, Clone)]
pub struct KernelInfo {
    pub name: String,
    pub params: Vec<GirParam>,
    /// kernel 默认数据类型（GPU 计算通常为 F32）
    pub default_dtype: GirDType,
}

impl Default for KernelInfo {
    fn default() -> Self {
        Self {
            name: String::new(),
            params: Vec::new(),
            default_dtype: GirDType::F32,
        }
    }
}

/// 将一个 LIR 函数降级为 GIR kernel
/// label_to_name: LIR LabelId → 函数名映射（用于检测 GPU 内建函数调用）
pub fn lower_lir_to_gir(
    lir_func: &karte_lir::ir::LirFunction,
    kernel_info: &KernelInfo,
    label_to_name: &HashMap<usize, String>,
) -> GirFunction {
    let mut gir = GirFunction::new(kernel_info.name.clone());
    gir.params = kernel_info.params.clone();

    let mut reg_map: HashMap<usize, usize> = HashMap::new();
    let mut label_map: HashMap<usize, usize> = HashMap::new();

    // 预扫描：创建标签映射
    for instr in &lir_func.instructions {
        if let LirInstr::Label { id, .. } = instr {
            let gir_label = gir.alloc_label();
            label_map.insert(id.0, gir_label);
        }
    }

    let default_dtype = kernel_info.default_dtype;

    for instr in &lir_func.instructions {
        match instr {
            // —— 标量算术 ——
            LirInstr::Add { dst, src1, src2, .. } => {
                let d = map_reg(&mut gir, &mut reg_map, dst);
                let s1 = convert_operand(src1, &reg_map);
                let s2 = convert_operand(src2, &reg_map);
                gir.emit(GirInstruction::Add { dst: d, src1: s1, src2: s2, dtype: default_dtype });
            }
            LirInstr::Sub { dst, src1, src2, .. } => {
                let d = map_reg(&mut gir, &mut reg_map, dst);
                let s1 = convert_operand(src1, &reg_map);
                let s2 = convert_operand(src2, &reg_map);
                gir.emit(GirInstruction::Sub { dst: d, src1: s1, src2: s2, dtype: default_dtype });
            }
            LirInstr::Mul { dst, src1, src2, .. } => {
                let d = map_reg(&mut gir, &mut reg_map, dst);
                let s1 = convert_operand(src1, &reg_map);
                let s2 = convert_operand(src2, &reg_map);
                gir.emit(GirInstruction::Mul { dst: d, src1: s1, src2: s2, dtype: default_dtype });
            }
            LirInstr::Div { dst, src1, src2, .. } => {
                let d = map_reg(&mut gir, &mut reg_map, dst);
                let s1 = convert_operand(src1, &reg_map);
                let s2 = convert_operand(src2, &reg_map);
                gir.emit(GirInstruction::Div { dst: d, src1: s1, src2: s2, dtype: default_dtype });
            }
            LirInstr::Mod { dst, src1, src2, .. } => {
                let d = map_reg(&mut gir, &mut reg_map, dst);
                let s1 = convert_operand(src1, &reg_map);
                let s2 = convert_operand(src2, &reg_map);
                gir.emit(GirInstruction::Mod { dst: d, src1: s1, src2: s2, dtype: default_dtype });
            }

            // —— Move ——
            LirInstr::Move { dst, src, .. } => {
                let d = map_reg(&mut gir, &mut reg_map, dst);
                let s = convert_operand(src, &reg_map);
                gir.emit(GirInstruction::Move { dst: d, src: s });
            }

            // —— 内存操作 ——
            LirInstr::Load64 { dst, addr, offset, .. } => {
                let d = map_reg(&mut gir, &mut reg_map, dst);
                let addr_op = compute_addr(addr, *offset, &reg_map, &mut gir);
                gir.emit(GirInstruction::GlobalLoad { dst: d, addr: addr_op, dtype: default_dtype });
            }
            LirInstr::Store64 { addr, offset, src, .. } => {
                let addr_op = compute_addr(addr, *offset, &reg_map, &mut gir);
                let s = convert_operand(src, &reg_map);
                gir.emit(GirInstruction::GlobalStore { addr: addr_op, src: s, dtype: default_dtype });
            }
            LirInstr::Load32 { dst, addr, offset, .. } => {
                let d = map_reg(&mut gir, &mut reg_map, dst);
                let addr_op = compute_addr(addr, *offset, &reg_map, &mut gir);
                gir.emit(GirInstruction::GlobalLoad { dst: d, addr: addr_op, dtype: GirDType::I32 });
            }
            LirInstr::Store32 { addr, offset, src, .. } => {
                let addr_op = compute_addr(addr, *offset, &reg_map, &mut gir);
                let s = convert_operand(src, &reg_map);
                gir.emit(GirInstruction::GlobalStore { addr: addr_op, src: s, dtype: GirDType::I32 });
            }

            // —— 比较与设置 ——
            LirInstr::CompareSet { dst, condition, src1, src2, .. } => {
                let d = map_reg(&mut gir, &mut reg_map, dst);
                let s1 = convert_operand(src1, &reg_map);
                let s2 = convert_operand(src2, &reg_map);
                let cmp_op = match condition {
                    ComparisonCondition::Equal => CmpOp::Eq,
                    ComparisonCondition::NotEqual => CmpOp::Ne,
                    ComparisonCondition::LessThan => CmpOp::Lt,
                    ComparisonCondition::LessEqual => CmpOp::Le,
                    ComparisonCondition::GreaterThan => CmpOp::Gt,
                    ComparisonCondition::GreaterEqual => CmpOp::Ge,
                };
                gir.emit(GirInstruction::Cmp { dst: d, op: cmp_op, src1: s1, src2: s2, dtype: default_dtype });
            }

            // —— 跳转 ——
            LirInstr::Jump { target, .. } => {
                let t = label_map.get(&target.0).copied().unwrap_or(0);
                gir.emit(GirInstruction::Jump { target: t });
            }
            LirInstr::JumpEqual { target, .. } => {
                // JumpEqual 依赖前面 CompareSet 设置的标志
                // 在 GIR 中展开为条件分支（复用最近的比较结果）
                let t = label_map.get(&target.0).copied().unwrap_or(0);
                let ft = gir.alloc_label();
                gir.emit(GirInstruction::BranchIf { cond: GirOperand::Reg(0), then_label: t, else_label: ft });
                gir.emit(GirInstruction::Label { id: ft });
            }
            LirInstr::JumpNotEqual { target, .. } => {
                let t = label_map.get(&target.0).copied().unwrap_or(0);
                let ft = gir.alloc_label();
                gir.emit(GirInstruction::BranchIf { cond: GirOperand::Reg(1), then_label: t, else_label: ft });
                gir.emit(GirInstruction::Label { id: ft });
            }
            LirInstr::JumpGreater { target, .. } => {
                let t = label_map.get(&target.0).copied().unwrap_or(0);
                gir.emit(GirInstruction::Jump { target: t });
            }
            LirInstr::JumpLess { target, .. } => {
                let t = label_map.get(&target.0).copied().unwrap_or(0);
                gir.emit(GirInstruction::Jump { target: t });
            }
            LirInstr::JumpGreaterEqual { target, .. } => {
                let t = label_map.get(&target.0).copied().unwrap_or(0);
                gir.emit(GirInstruction::Jump { target: t });
            }
            LirInstr::JumpLessEqual { target, .. } => {
                let t = label_map.get(&target.0).copied().unwrap_or(0);
                gir.emit(GirInstruction::Jump { target: t });
            }

            // —— 标签 ——
            LirInstr::Label { id, .. } => {
                let l = label_map.get(&id.0).copied().unwrap_or(0);
                gir.emit(GirInstruction::Label { id: l });
            }

            // —— 函数调用（检测 GPU 内建函数和 tile 操作）——
            LirInstr::Call { target, args, arg_operands, result, .. } => {
                let func_name = label_to_name.get(&target.0);
                let result_reg = result.map(|r| map_reg(&mut gir, &mut reg_map, &r)).unwrap_or_else(|| gir.alloc_reg());

                match func_name.map(|s| s.as_str()) {
                    Some("thread_global_id") => {
                        // global_id = blockIdx.x * blockDim.x + threadIdx.x
                        let bid = gir.alloc_reg();
                        let bdim = gir.alloc_reg();
                        let tid = gir.alloc_reg();
                        gir.emit(GirInstruction::BlockId { dst: bid, dim: ThreadDim::X });
                        gir.emit(GirInstruction::BlockDim { dst: bdim, dim: ThreadDim::X });
                        gir.emit(GirInstruction::ThreadId { dst: tid, dim: ThreadDim::X });
                        gir.emit(GirInstruction::Mul { dst: bid, src1: GirOperand::Reg(bid), src2: GirOperand::Reg(bdim), dtype: GirDType::I32 });
                        gir.emit(GirInstruction::Add { dst: result_reg, src1: GirOperand::Reg(bid), src2: GirOperand::Reg(tid), dtype: GirDType::I32 });
                    }
                    Some("thread_local_id") => {
                        gir.emit(GirInstruction::ThreadId { dst: result_reg, dim: ThreadDim::X });
                    }
                    Some("block_id") => {
                        gir.emit(GirInstruction::BlockId { dst: result_reg, dim: ThreadDim::X });
                    }
                    Some("block_id_2d") => {
                        let y_reg = gir.alloc_reg();
                        gir.emit(GirInstruction::BlockId { dst: result_reg, dim: ThreadDim::X });
                        gir.emit(GirInstruction::BlockId { dst: y_reg, dim: ThreadDim::Y });
                    }
                    Some("block_dim") => {
                        gir.emit(GirInstruction::BlockDim { dst: result_reg, dim: ThreadDim::X });
                    }
                    Some("grid_dim") => {
                        gir.emit(GirInstruction::GridDim { dst: result_reg, dim: ThreadDim::X });
                    }
                    Some("sync_threads") => {
                        gir.emit(GirInstruction::Barrier);
                    }
                    Some("tile_load") => {
                        // tile_load(base, row, col, tile_rows, tile_cols, stride)
                        // arg_operands: base, row, col, (tile_rows, tile_cols, stride 可能是常量)
                        let base = convert_operand(&arg_operands[0], &reg_map);
                        let row = convert_operand(&arg_operands[1], &reg_map);
                        let col = convert_operand(&arg_operands[2], &reg_map);
                        let (tile_rows, tile_cols) = if arg_operands.len() >= 5 {
                            let tr = match &arg_operands[3] { LirOperand::Immediate { value } => *value as usize, _ => 32 };
                            let tc = match &arg_operands[4] { LirOperand::Immediate { value } => *value as usize, _ => 32 };
                            (tr, tc)
                        } else { (32, 32) };
                        let stride = if arg_operands.len() >= 6 {
                            convert_operand(&arg_operands[5], &reg_map)
                        } else { GirOperand::Imm(tile_cols as i64) };
                        gir.emit(GirInstruction::TileLoad {
                            dst: result_reg, base, row, col,
                            tile_rows, tile_cols, stride, dtype: default_dtype,
                        });
                    }
                    Some("tile_store") => {
                        // tile_store(base, row, col, tile_reg, tile_rows, tile_cols, stride)
                        let base = convert_operand(&arg_operands[0], &reg_map);
                        let row = convert_operand(&arg_operands[1], &reg_map);
                        let col = convert_operand(&arg_operands[2], &reg_map);
                        let tile_src = match &arg_operands[3] {
                            LirOperand::Register { id } => map_reg(&mut gir, &mut reg_map, id),
                            _ => gir.alloc_reg(),
                        };
                        let (tile_rows, tile_cols) = if arg_operands.len() >= 6 {
                            let tr = match &arg_operands[4] { LirOperand::Immediate { value } => *value as usize, _ => 32 };
                            let tc = match &arg_operands[5] { LirOperand::Immediate { value } => *value as usize, _ => 32 };
                            (tr, tc)
                        } else { (32, 32) };
                        let stride = if arg_operands.len() >= 7 {
                            convert_operand(&arg_operands[6], &reg_map)
                        } else { GirOperand::Imm(tile_cols as i64) };
                        gir.emit(GirInstruction::TileStore {
                            base, row, col, src: tile_src,
                            tile_rows, tile_cols, stride, dtype: default_dtype,
                        });
                    }
                    Some("tile_zeros") => {
                        let (tile_rows, tile_cols) = if arg_operands.len() >= 2 {
                            let tr = match &arg_operands[0] { LirOperand::Immediate { value } => *value as usize, _ => 32 };
                            let tc = match &arg_operands[1] { LirOperand::Immediate { value } => *value as usize, _ => 32 };
                            (tr, tc)
                        } else { (32, 32) };
                        gir.emit(GirInstruction::TileZeros {
                            dst: result_reg, tile_rows, tile_cols, dtype: default_dtype,
                        });
                    }
                    Some("tile_matmul") => {
                        // tile_matmul(acc, a_tile, b_tile) — acc += a × b
                        let a_reg = match &arg_operands[1] {
                            LirOperand::Register { id } => map_reg(&mut gir, &mut reg_map, id),
                            _ => gir.alloc_reg(),
                        };
                        let b_reg = match &arg_operands[2] {
                            LirOperand::Register { id } => map_reg(&mut gir, &mut reg_map, id),
                            _ => gir.alloc_reg(),
                        };
                        gir.emit(GirInstruction::TileMatmul {
                            dst: result_reg, a: a_reg, b: b_reg,
                            m: 32, k: 32, n: 32,
                            dtype_a: default_dtype, dtype_b: default_dtype, dtype_c: default_dtype,
                        });
                    }
                    Some("shared") => {
                        // shared<T>(n) — 声明共享内存
                        let size = match &arg_operands[0] {
                            LirOperand::Immediate { value } => *value as usize * 8,
                            _ => 256,
                        };
                        gir.emit(GirInstruction::SharedAlloc { dst: result_reg, size, dtype: default_dtype });
                    }
                    Some("warp_shuffle") => {
                        let src = convert_operand(&arg_operands[0], &reg_map);
                        let lane = if arg_operands.len() > 1 { convert_operand(&arg_operands[1], &reg_map) } else { GirOperand::Imm(0) };
                        gir.emit(GirInstruction::WarpShuffle { dst: result_reg, src, src_lane: lane, op: ShuffleOp::Idx, dtype: default_dtype });
                    }
                    Some("warp_reduce") => {
                        let src = convert_operand(&arg_operands[0], &reg_map);
                        // 展开为多个 shuffle 操作
                        let tmp = gir.alloc_reg();
                        gir.emit(GirInstruction::WarpShuffle { dst: tmp, src: src.clone(), src_lane: GirOperand::Imm(16), op: ShuffleOp::Down, dtype: default_dtype });
                        gir.emit(GirInstruction::Add { dst: result_reg, src1: src, src2: GirOperand::Reg(tmp), dtype: default_dtype });
                        let tmp2 = gir.alloc_reg();
                        gir.emit(GirInstruction::WarpShuffle { dst: tmp2, src: GirOperand::Reg(result_reg), src_lane: GirOperand::Imm(8), op: ShuffleOp::Down, dtype: default_dtype });
                        gir.emit(GirInstruction::Add { dst: result_reg, src1: GirOperand::Reg(result_reg), src2: GirOperand::Reg(tmp2), dtype: default_dtype });
                        let tmp3 = gir.alloc_reg();
                        gir.emit(GirInstruction::WarpShuffle { dst: tmp3, src: GirOperand::Reg(result_reg), src_lane: GirOperand::Imm(4), op: ShuffleOp::Down, dtype: default_dtype });
                        gir.emit(GirInstruction::Add { dst: result_reg, src1: GirOperand::Reg(result_reg), src2: GirOperand::Reg(tmp3), dtype: default_dtype });
                        let tmp4 = gir.alloc_reg();
                        gir.emit(GirInstruction::WarpShuffle { dst: tmp4, src: GirOperand::Reg(result_reg), src_lane: GirOperand::Imm(2), op: ShuffleOp::Down, dtype: default_dtype });
                        gir.emit(GirInstruction::Add { dst: result_reg, src1: GirOperand::Reg(result_reg), src2: GirOperand::Reg(tmp4), dtype: default_dtype });
                        let tmp5 = gir.alloc_reg();
                        gir.emit(GirInstruction::WarpShuffle { dst: tmp5, src: GirOperand::Reg(result_reg), src_lane: GirOperand::Imm(1), op: ShuffleOp::Down, dtype: default_dtype });
                        gir.emit(GirInstruction::Add { dst: result_reg, src1: GirOperand::Reg(result_reg), src2: GirOperand::Reg(tmp5), dtype: default_dtype });
                    }
                    _ => {
                        // 非内建函数调用 — 在 kernel 中忽略
                    }
                }
            }

            // —— 返回 ——
            LirInstr::Return { .. } => {
                gir.emit(GirInstruction::Return);
            }

            // —— 位运算（暂不处理，GPU kernel 通常不含这些）——
            // —— 其他指令忽略（Alloc/GC/字符串等在 GPU kernel 中不出现）——
            _ => {}
        }
    }

    gir
}

/// 获取或分配寄存器映射
fn map_reg(gir: &mut GirFunction, map: &mut HashMap<usize, usize>, reg: &Register) -> usize {
    let id = match reg {
        Register::Virtual(id) => *id,
        Register::Physical(id) => *id as usize,
    };
    *map.entry(id).or_insert_with(|| gir.alloc_reg())
}

/// 将 LIR Operand 转换为 GIR Operand
fn convert_operand(operand: &LirOperand, reg_map: &HashMap<usize, usize>) -> GirOperand {
    match operand {
        LirOperand::Register { id } => {
            let id = match id {
                Register::Virtual(id) => *id,
                Register::Physical(id) => *id as usize,
            };
            GirOperand::Reg(*reg_map.get(&id).unwrap_or(&id))
        }
        LirOperand::Immediate { value } => GirOperand::Imm(*value),
        _ => GirOperand::Imm(0),
    }
}

/// 计算带偏移的地址
fn compute_addr(
    addr: &Register,
    offset: i64,
    reg_map: &HashMap<usize, usize>,
    gir: &mut GirFunction,
) -> GirOperand {
    let base_id = match addr {
        Register::Virtual(id) => *id,
        Register::Physical(id) => *id as usize,
    };
    let base = GirOperand::Reg(*reg_map.get(&base_id).unwrap_or(&base_id));
    if offset == 0 {
        base
    } else {
        let addr_reg = gir.alloc_reg();
        gir.emit(GirInstruction::Add {
            dst: addr_reg,
            src1: base,
            src2: GirOperand::Imm(offset),
            dtype: GirDType::I64,
        });
        GirOperand::Reg(addr_reg)
    }
}
