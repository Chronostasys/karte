//! Tile 展开 Pass — 将高级 tile 指令展开为线程级操作
//!
//! 核心思想：每个线程块协作处理一个 tile，tile 中的元素
//! 由块内线程均分。

use crate::ir::*;
use std::collections::HashMap;

/// Tile 展开器
pub struct TileExpander {
    /// 块内线程数（默认 256）
    block_size: usize,
    /// 寄存器分配计数器
    next_reg: usize,
    /// 标签分配计数器
    next_label: usize,
    /// 共享内存总大小（字节）
    shared_mem_total: usize,
}

impl TileExpander {
    pub fn new(block_size: usize) -> Self {
        Self {
            block_size,
            next_reg: 1000, // 从 1000 开始避免冲突
            next_label: 500,
            shared_mem_total: 0,
        }
    }

    /// 分配寄存器
    fn alloc_reg(&mut self) -> usize {
        let id = self.next_reg;
        self.next_reg += 1;
        id
    }

    /// 分配标签
    fn alloc_label(&mut self) -> usize {
        let id = self.next_label;
        self.next_label += 1;
        id
    }

    /// 展开整个 GIR kernel
    pub fn expand(&mut self, func: &mut GirFunction) {
        let block_size = self.block_size;
        let mut expanded: Vec<GirInstruction> = Vec::new();

        // 先计算共享内存需求
        for instr in &func.instructions {
            if let GirInstruction::SharedAlloc { size, .. } = instr {
                self.shared_mem_total += size;
            }
            if let GirInstruction::TileLoad { tile_rows, tile_cols, dtype, .. } = instr {
                self.shared_mem_total += tile_rows * tile_cols * dtype.size_in_bytes();
            }
            if let GirInstruction::TileZeros { tile_rows, tile_cols, dtype, .. } = instr {
                self.shared_mem_total += tile_rows * tile_cols * dtype.size_in_bytes();
            }
        }
        func.shared_mem_size = self.shared_mem_total;

        let mut smem_offset: usize = 0;
        let mut tile_smem_map: HashMap<usize, (usize, usize, usize)> = HashMap::new(); // reg → (offset, rows, cols)

        for instr in &func.instructions {
            match instr {
                GirInstruction::SharedAlloc { dst, size, .. } => {
                    tile_smem_map.insert(*dst, (smem_offset, *size, 1));
                    smem_offset += size;
                    // 不生成指令，共享内存声明在 PTX 层处理
                }

                GirInstruction::TileZeros { dst, tile_rows, tile_cols, dtype } => {
                    let smem_off = smem_offset;
                    let total = tile_rows * tile_cols;
                    tile_smem_map.insert(*dst, (smem_off, *tile_rows, *tile_cols));
                    smem_offset += total * dtype.size_in_bytes();

                    // 每个线程清零自己负责的元素
                    // tid = thread_local_id()
                    let tid_reg = self.alloc_reg();
                    expanded.push(GirInstruction::ThreadId { dst: tid_reg, dim: ThreadDim::X });

                    // 循环: for i in (tid..total).step_by(block_size) { smem[i] = 0 }
                    let i_reg = self.alloc_reg();
                    expanded.push(GirInstruction::Move { dst: i_reg, src: GirOperand::Reg(tid_reg) });

                    let loop_label = self.alloc_label();
                    let end_label = self.alloc_label();
                    expanded.push(GirInstruction::Label { id: loop_label });

                    // 检查 i < total
                    let cmp_reg = self.alloc_reg();
                    expanded.push(GirInstruction::Cmp {
                        dst: cmp_reg, op: CmpOp::Lt,
                        src1: GirOperand::Reg(i_reg),
                        src2: GirOperand::Imm(total as i64),
                        dtype: GirDType::I32,
                    });
                    expanded.push(GirInstruction::BranchIf {
                        cond: GirOperand::Reg(cmp_reg),
                        then_label: end_label, else_label: loop_label,
                    });

                    // smem[i] = 0
                    let addr_reg = self.alloc_reg();
                    let smem_base = self.alloc_reg();
                    expanded.push(GirInstruction::Move { dst: smem_base, src: GirOperand::Imm(smem_off as i64) });
                    expanded.push(GirInstruction::Add {
                        dst: addr_reg, src1: GirOperand::Reg(smem_base),
                        src2: GirOperand::Reg(i_reg),
                        dtype: GirDType::I64,
                    });
                    expanded.push(GirInstruction::SharedStore {
                        addr: GirOperand::Reg(addr_reg),
                        src: GirOperand::Imm(0),
                        dtype: *dtype,
                    });

                    // i += block_size
                    expanded.push(GirInstruction::Add {
                        dst: i_reg, src1: GirOperand::Reg(i_reg),
                        src2: GirOperand::Imm(block_size as i64),
                        dtype: GirDType::I32,
                    });
                    expanded.push(GirInstruction::Jump { target: loop_label });
                    expanded.push(GirInstruction::Label { id: end_label });

                    // 屏障
                    expanded.push(GirInstruction::Barrier);
                }

                GirInstruction::TileLoad { dst, base, row, col, tile_rows, tile_cols, stride, dtype } => {
                    let smem_off = smem_offset;
                    let total = tile_rows * tile_cols;
                    tile_smem_map.insert(*dst, (smem_off, *tile_rows, *tile_cols));
                    smem_offset += total * dtype.size_in_bytes();

                    let tid_reg = self.alloc_reg();
                    expanded.push(GirInstruction::ThreadId { dst: tid_reg, dim: ThreadDim::X });

                    // 每个线程加载 total/block_size 个元素
                    let i_reg = self.alloc_reg();
                    expanded.push(GirInstruction::Move { dst: i_reg, src: GirOperand::Reg(tid_reg) });

                    let loop_label = self.alloc_label();
                    let end_label = self.alloc_label();
                    expanded.push(GirInstruction::Label { id: loop_label });

                    let cmp_reg = self.alloc_reg();
                    expanded.push(GirInstruction::Cmp {
                        dst: cmp_reg, op: CmpOp::Lt,
                        src1: GirOperand::Reg(i_reg),
                        src2: GirOperand::Imm(total as i64),
                        dtype: GirDType::I32,
                    });
                    expanded.push(GirInstruction::BranchIf {
                        cond: GirOperand::Reg(cmp_reg),
                        then_label: end_label, else_label: loop_label,
                    });

                    // 计算行列: local_row = i / tile_cols, local_col = i % tile_cols
                    let local_row = self.alloc_reg();
                    let local_col = self.alloc_reg();
                    expanded.push(GirInstruction::Div {
                        dst: local_row, src1: GirOperand::Reg(i_reg),
                        src2: GirOperand::Imm(*tile_cols as i64), dtype: GirDType::I32,
                    });
                    expanded.push(GirInstruction::Mod {
                        dst: local_col, src1: GirOperand::Reg(i_reg),
                        src2: GirOperand::Imm(*tile_cols as i64), dtype: GirDType::I32,
                    });

                    // 全局地址 = base + (row + local_row) * stride + (col + local_col)
                    let global_row = self.alloc_reg();
                    let global_col = self.alloc_reg();
                    expanded.push(GirInstruction::Add {
                        dst: global_row, src1: row.clone(),
                        src2: GirOperand::Reg(local_row), dtype: GirDType::I64,
                    });
                    expanded.push(GirInstruction::Add {
                        dst: global_col, src1: col.clone(),
                        src2: GirOperand::Reg(local_col), dtype: GirDType::I64,
                    });
                    let addr_reg = self.alloc_reg();
                    expanded.push(GirInstruction::Mul {
                        dst: addr_reg, src1: GirOperand::Reg(global_row),
                        src2: stride.clone(), dtype: GirDType::I64,
                    });
                    let elem_size = dtype.size_in_bytes();
                    expanded.push(GirInstruction::Add {
                        dst: addr_reg, src1: GirOperand::Reg(addr_reg),
                        src2: GirOperand::Reg(global_col), dtype: GirDType::I64,
                    });
                    // addr *= elem_size (字节偏移)
                    let byte_addr = self.alloc_reg();
                    expanded.push(GirInstruction::Mul {
                        dst: byte_addr, src1: GirOperand::Reg(addr_reg),
                        src2: GirOperand::Imm(elem_size as i64), dtype: GirDType::I64,
                    });
                    let full_addr = self.alloc_reg();
                    expanded.push(GirInstruction::Add {
                        dst: full_addr, src1: base.clone(),
                        src2: GirOperand::Reg(byte_addr), dtype: GirDType::I64,
                    });

                    // 从全局内存加载
                    let val_reg = self.alloc_reg();
                    expanded.push(GirInstruction::GlobalLoad {
                        dst: val_reg, addr: GirOperand::Reg(full_addr), dtype: *dtype,
                    });

                    // 存储到共享内存
                    let smem_addr = self.alloc_reg();
                    let smem_base = self.alloc_reg();
                    expanded.push(GirInstruction::Move { dst: smem_base, src: GirOperand::Imm(smem_off as i64) });
                    let smem_byte_off = self.alloc_reg();
                    expanded.push(GirInstruction::Mul {
                        dst: smem_byte_off, src1: GirOperand::Reg(i_reg),
                        src2: GirOperand::Imm(elem_size as i64), dtype: GirDType::I64,
                    });
                    expanded.push(GirInstruction::Add {
                        dst: smem_addr, src1: GirOperand::Reg(smem_base),
                        src2: GirOperand::Reg(smem_byte_off), dtype: GirDType::I64,
                    });
                    expanded.push(GirInstruction::SharedStore {
                        addr: GirOperand::Reg(smem_addr),
                        src: GirOperand::Reg(val_reg),
                        dtype: *dtype,
                    });

                    // i += block_size
                    expanded.push(GirInstruction::Add {
                        dst: i_reg, src1: GirOperand::Reg(i_reg),
                        src2: GirOperand::Imm(block_size as i64),
                        dtype: GirDType::I32,
                    });
                    expanded.push(GirInstruction::Jump { target: loop_label });
                    expanded.push(GirInstruction::Label { id: end_label });

                    // 屏障同步
                    expanded.push(GirInstruction::Barrier);
                }

                GirInstruction::TileStore { base, row, col, src, tile_rows, tile_cols, stride, dtype } => {
                    let smem_off = tile_smem_map.get(src)
                        .map(|(off, _, _)| *off)
                        .unwrap_or(0);

                    let tid_reg = self.alloc_reg();
                    expanded.push(GirInstruction::ThreadId { dst: tid_reg, dim: ThreadDim::X });

                    let i_reg = self.alloc_reg();
                    expanded.push(GirInstruction::Move { dst: i_reg, src: GirOperand::Reg(tid_reg) });

                    let total = tile_rows * tile_cols;
                    let loop_label = self.alloc_label();
                    let end_label = self.alloc_label();
                    expanded.push(GirInstruction::Label { id: loop_label });

                    let cmp_reg = self.alloc_reg();
                    expanded.push(GirInstruction::Cmp {
                        dst: cmp_reg, op: CmpOp::Lt,
                        src1: GirOperand::Reg(i_reg),
                        src2: GirOperand::Imm(total as i64),
                        dtype: GirDType::I32,
                    });
                    expanded.push(GirInstruction::BranchIf {
                        cond: GirOperand::Reg(cmp_reg),
                        then_label: end_label, else_label: loop_label,
                    });

                    // 从共享内存加载值
                    let smem_addr = self.alloc_reg();
                    let smem_base = self.alloc_reg();
                    expanded.push(GirInstruction::Move { dst: smem_base, src: GirOperand::Imm(smem_off as i64) });
                    let elem_size = dtype.size_in_bytes();
                    let smem_byte_off = self.alloc_reg();
                    expanded.push(GirInstruction::Mul {
                        dst: smem_byte_off, src1: GirOperand::Reg(i_reg),
                        src2: GirOperand::Imm(elem_size as i64), dtype: GirDType::I64,
                    });
                    expanded.push(GirInstruction::Add {
                        dst: smem_addr, src1: GirOperand::Reg(smem_base),
                        src2: GirOperand::Reg(smem_byte_off), dtype: GirDType::I64,
                    });
                    let val_reg = self.alloc_reg();
                    expanded.push(GirInstruction::SharedLoad {
                        dst: val_reg, addr: GirOperand::Reg(smem_addr), dtype: *dtype,
                    });

                    // 计算全局地址
                    let local_row = self.alloc_reg();
                    let local_col = self.alloc_reg();
                    expanded.push(GirInstruction::Div {
                        dst: local_row, src1: GirOperand::Reg(i_reg),
                        src2: GirOperand::Imm(*tile_cols as i64), dtype: GirDType::I32,
                    });
                    expanded.push(GirInstruction::Mod {
                        dst: local_col, src1: GirOperand::Reg(i_reg),
                        src2: GirOperand::Imm(*tile_cols as i64), dtype: GirDType::I32,
                    });
                    let global_row = self.alloc_reg();
                    let global_col = self.alloc_reg();
                    expanded.push(GirInstruction::Add {
                        dst: global_row, src1: row.clone(),
                        src2: GirOperand::Reg(local_row), dtype: GirDType::I64,
                    });
                    expanded.push(GirInstruction::Add {
                        dst: global_col, src1: col.clone(),
                        src2: GirOperand::Reg(local_col), dtype: GirDType::I64,
                    });
                    let addr_reg = self.alloc_reg();
                    expanded.push(GirInstruction::Mul {
                        dst: addr_reg, src1: GirOperand::Reg(global_row),
                        src2: stride.clone(), dtype: GirDType::I64,
                    });
                    expanded.push(GirInstruction::Add {
                        dst: addr_reg, src1: GirOperand::Reg(addr_reg),
                        src2: GirOperand::Reg(global_col), dtype: GirDType::I64,
                    });
                    let byte_addr = self.alloc_reg();
                    expanded.push(GirInstruction::Mul {
                        dst: byte_addr, src1: GirOperand::Reg(addr_reg),
                        src2: GirOperand::Imm(elem_size as i64), dtype: GirDType::I64,
                    });
                    let full_addr = self.alloc_reg();
                    expanded.push(GirInstruction::Add {
                        dst: full_addr, src1: base.clone(),
                        src2: GirOperand::Reg(byte_addr), dtype: GirDType::I64,
                    });

                    // 存储到全局内存
                    expanded.push(GirInstruction::GlobalStore {
                        addr: GirOperand::Reg(full_addr),
                        src: GirOperand::Reg(val_reg),
                        dtype: *dtype,
                    });

                    expanded.push(GirInstruction::Add {
                        dst: i_reg, src1: GirOperand::Reg(i_reg),
                        src2: GirOperand::Imm(block_size as i64),
                        dtype: GirDType::I32,
                    });
                    expanded.push(GirInstruction::Jump { target: loop_label });
                    expanded.push(GirInstruction::Label { id: end_label });

                    expanded.push(GirInstruction::Barrier);
                }

                GirInstruction::TileMatmul { dst, a, b, m, k, n, dtype_a, dtype_b, dtype_c } => {
                    // Tile 矩阵乘法展开为标量 FMA 操作
                    // 每个线程处理 dst tile 中的一部分元素
                    let a_off = tile_smem_map.get(a).map(|v| v.0).unwrap_or(0);
                    let b_off = tile_smem_map.get(b).map(|v| v.0).unwrap_or(0);
                    let c_off = tile_smem_map.get(dst).map(|v| v.0).unwrap_or(0);

                    let tid_reg = self.alloc_reg();
                    expanded.push(GirInstruction::ThreadId { dst: tid_reg, dim: ThreadDim::X });

                    // 每个 tid 处理第 tid 个元素 C[tid/m][tid%m]
                    let elem_size_c = dtype_c.size_in_bytes();
                    let elem_size_a = dtype_a.size_in_bytes();
                    let elem_size_b = dtype_b.size_in_bytes();

                    let ci_reg = self.alloc_reg();
                    let cm_reg = self.alloc_reg();
                    let cn_reg = self.alloc_reg();
                    expanded.push(GirInstruction::Move { dst: ci_reg, src: GirOperand::Reg(tid_reg) });

                    let loop_label = self.alloc_label();
                    let end_label = self.alloc_label();
                    expanded.push(GirInstruction::Label { id: loop_label });

                    let total = m * n;
                    let cmp_reg = self.alloc_reg();
                    expanded.push(GirInstruction::Cmp {
                        dst: cmp_reg, op: CmpOp::Lt,
                        src1: GirOperand::Reg(ci_reg),
                        src2: GirOperand::Imm(total as i64),
                        dtype: GirDType::I32,
                    });
                    expanded.push(GirInstruction::BranchIf {
                        cond: GirOperand::Reg(cmp_reg),
                        then_label: end_label, else_label: loop_label,
                    });

                    // cm = ci / n, cn = ci % n
                    expanded.push(GirInstruction::Div {
                        dst: cm_reg, src1: GirOperand::Reg(ci_reg),
                        src2: GirOperand::Imm(*n as i64), dtype: GirDType::I32,
                    });
                    expanded.push(GirInstruction::Mod {
                        dst: cn_reg, src1: GirOperand::Reg(ci_reg),
                        src2: GirOperand::Imm(*n as i64), dtype: GirDType::I32,
                    });

                    // acc = C[cm][cn]
                    let c_addr = self.alloc_reg();
                    let c_base = self.alloc_reg();
                    expanded.push(GirInstruction::Move { dst: c_base, src: GirOperand::Imm(c_off as i64) });
                    let c_byte = self.alloc_reg();
                    expanded.push(GirInstruction::Mul {
                        dst: c_byte, src1: GirOperand::Reg(ci_reg),
                        src2: GirOperand::Imm(elem_size_c as i64), dtype: GirDType::I64,
                    });
                    expanded.push(GirInstruction::Add {
                        dst: c_addr, src1: GirOperand::Reg(c_base),
                        src2: GirOperand::Reg(c_byte), dtype: GirDType::I64,
                    });
                    let acc_reg = self.alloc_reg();
                    expanded.push(GirInstruction::SharedLoad {
                        dst: acc_reg, addr: GirOperand::Reg(c_addr), dtype: *dtype_c,
                    });

                    // for kk in 0..k: acc += A[cm][kk] * B[kk][cn]
                    let kk_reg = self.alloc_reg();
                    expanded.push(GirInstruction::Move { dst: kk_reg, src: GirOperand::Imm(0) });
                    let inner_loop = self.alloc_label();
                    let inner_end = self.alloc_label();
                    expanded.push(GirInstruction::Label { id: inner_loop });

                    let kk_cmp = self.alloc_reg();
                    expanded.push(GirInstruction::Cmp {
                        dst: kk_cmp, op: CmpOp::Lt,
                        src1: GirOperand::Reg(kk_reg),
                        src2: GirOperand::Imm(*k as i64),
                        dtype: GirDType::I32,
                    });
                    expanded.push(GirInstruction::BranchIf {
                        cond: GirOperand::Reg(kk_cmp),
                        then_label: inner_end, else_label: inner_loop,
                    });

                    // 加载 A[cm][kk]
                    let a_idx = self.alloc_reg();
                    expanded.push(GirInstruction::Mul {
                        dst: a_idx, src1: GirOperand::Reg(cm_reg),
                        src2: GirOperand::Imm(*k as i64), dtype: GirDType::I32,
                    });
                    expanded.push(GirInstruction::Add {
                        dst: a_idx, src1: GirOperand::Reg(a_idx),
                        src2: GirOperand::Reg(kk_reg), dtype: GirDType::I32,
                    });
                    let a_addr = self.alloc_reg();
                    let a_base = self.alloc_reg();
                    expanded.push(GirInstruction::Move { dst: a_base, src: GirOperand::Imm(a_off as i64) });
                    let a_byte = self.alloc_reg();
                    expanded.push(GirInstruction::Mul {
                        dst: a_byte, src1: GirOperand::Reg(a_idx),
                        src2: GirOperand::Imm(elem_size_a as i64), dtype: GirDType::I64,
                    });
                    expanded.push(GirInstruction::Add {
                        dst: a_addr, src1: GirOperand::Reg(a_base),
                        src2: GirOperand::Reg(a_byte), dtype: GirDType::I64,
                    });
                    let a_val = self.alloc_reg();
                    expanded.push(GirInstruction::SharedLoad {
                        dst: a_val, addr: GirOperand::Reg(a_addr), dtype: *dtype_a,
                    });

                    // 加载 B[kk][cn]
                    let b_idx = self.alloc_reg();
                    expanded.push(GirInstruction::Mul {
                        dst: b_idx, src1: GirOperand::Reg(kk_reg),
                        src2: GirOperand::Imm(*n as i64), dtype: GirDType::I32,
                    });
                    expanded.push(GirInstruction::Add {
                        dst: b_idx, src1: GirOperand::Reg(b_idx),
                        src2: GirOperand::Reg(cn_reg), dtype: GirDType::I32,
                    });
                    let b_addr = self.alloc_reg();
                    let b_base = self.alloc_reg();
                    expanded.push(GirInstruction::Move { dst: b_base, src: GirOperand::Imm(b_off as i64) });
                    let b_byte = self.alloc_reg();
                    expanded.push(GirInstruction::Mul {
                        dst: b_byte, src1: GirOperand::Reg(b_idx),
                        src2: GirOperand::Imm(elem_size_b as i64), dtype: GirDType::I64,
                    });
                    expanded.push(GirInstruction::Add {
                        dst: b_addr, src1: GirOperand::Reg(b_base),
                        src2: GirOperand::Reg(b_byte), dtype: GirDType::I64,
                    });
                    let b_val = self.alloc_reg();
                    expanded.push(GirInstruction::SharedLoad {
                        dst: b_val, addr: GirOperand::Reg(b_addr), dtype: *dtype_b,
                    });

                    // acc += a_val * b_val
                    expanded.push(GirInstruction::Fma {
                        dst: acc_reg,
                        src1: GirOperand::Reg(a_val),
                        src2: GirOperand::Reg(b_val),
                        src3: GirOperand::Reg(acc_reg),
                        dtype: *dtype_c,
                    });

                    expanded.push(GirInstruction::Add {
                        dst: kk_reg, src1: GirOperand::Reg(kk_reg),
                        src2: GirOperand::Imm(1), dtype: GirDType::I32,
                    });
                    expanded.push(GirInstruction::Jump { target: inner_loop });
                    expanded.push(GirInstruction::Label { id: inner_end });

                    // 写回 C[cm][cn]
                    expanded.push(GirInstruction::SharedStore {
                        addr: GirOperand::Reg(c_addr),
                        src: GirOperand::Reg(acc_reg),
                        dtype: *dtype_c,
                    });

                    expanded.push(GirInstruction::Add {
                        dst: ci_reg, src1: GirOperand::Reg(ci_reg),
                        src2: GirOperand::Imm(self.block_size as i64),
                        dtype: GirDType::I32,
                    });
                    expanded.push(GirInstruction::Jump { target: loop_label });
                    expanded.push(GirInstruction::Label { id: end_label });

                    expanded.push(GirInstruction::Barrier);
                }

                // 其他指令直接保留
                _ => {
                    expanded.push(instr.clone());
                }
            }
        }

        // 用展开后的指令替换
        func.instructions = expanded;
        func.next_reg = func.next_reg.max(self.next_reg);
        func.next_label = func.next_label.max(self.next_label);
    }
}

/// 对 GIR 程序中的所有 kernel 执行 tile 展开
pub fn expand_tiles(gir: &mut GirProgram, block_size: usize) {
    for kernel in &mut gir.kernels {
        TileExpander::new(block_size).expand(kernel);
    }
}
