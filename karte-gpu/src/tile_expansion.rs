//! Tile Expansion Pass — 将高层 Tile 指令展开为线程级指令
//!
//! 展开策略:
//! - TileZeros → 寄存器零初始化 (acc = 0)
//! - TileLoad → GlobalLoad + SharedStore + Barrier (协作加载到 shared memory)
//! - TileMatmul → SharedLoad + FMA 嵌套循环 (从 shared memory 读, 累加到寄存器)
//! - TileStore → GlobalStore (从寄存器直接写回全局内存)

use karte_gir::*;
use std::collections::HashMap;

/// Tile 展开配置
#[derive(Debug, Clone)]
pub struct TileConfig {
    pub tile_rows: usize,
    pub tile_cols: usize,
    pub tile_k: usize,
}

impl Default for TileConfig {
    fn default() -> Self {
        Self { tile_rows: 16, tile_cols: 16, tile_k: 16 }
    }
}

/// Tile 数据位置
#[derive(Debug, Clone, Copy, PartialEq)]
enum TileLocation {
    /// 结果在寄存器中 (TileZeros / TileMatmul 的输出)
    Register(usize),
    /// 数据在 shared memory 中 (TileLoad 的输出)
    SharedMemory(usize),
}

/// Tile 信息
#[derive(Debug, Clone)]
struct TileInfo {
    location: TileLocation,
    rows: usize,
    cols: usize,
    dtype: GirDType,
}

/// Tile 展开器
pub struct TileExpander {
    config: TileConfig,
}

impl TileExpander {
    pub fn new(config: TileConfig) -> Self { Self { config } }
    pub fn with_default() -> Self { Self::new(TileConfig::default()) }

    pub fn expand_kernel(&self, func: &GirFunction) -> GirFunction {
        let mut new_func = GirFunction::new(func.name.clone());
        new_func.params = func.params.clone();
        new_func.shared_mem_size = func.shared_mem_size;
        new_func.next_reg = func.next_reg;
        new_func.next_label = func.next_label;
        new_func.grid_dim = func.grid_dim;
        new_func.block_dim = func.block_dim;

        let mut tile_map: HashMap<usize, TileInfo> = HashMap::new();
        let mut shared_mem_offset: usize = 0;

        for instr in &func.instructions {
            match instr {
                GirInstruction::TileZeros { dst, tile_rows, tile_cols, dtype } => {
                    let info = TileInfo {
                        location: TileLocation::Register(*dst),
                        rows: *tile_rows, cols: *tile_cols, dtype: *dtype,
                    };
                    tile_map.insert(*dst, info);
                    // acc = 0
                    new_func.instructions.push(GirInstruction::Move {
                        dst: *dst, src: GirOperand::Imm(0),
                    });
                }

                GirInstruction::TileLoad { dst, base, row, col, tile_rows, tile_cols, stride, dtype } => {
                    let size = tile_rows * tile_cols * dtype.size_in_bytes();
                    let sm_offset = shared_mem_offset;
                    shared_mem_offset += size;
                    let info = TileInfo {
                        location: TileLocation::SharedMemory(sm_offset),
                        rows: *tile_rows, cols: *tile_cols, dtype: *dtype,
                    };
                    tile_map.insert(*dst, info);
                    let instrs = self.expand_tile_load(
                        base, row, col, *tile_rows, *tile_cols, stride, *dtype,
                        sm_offset, &mut new_func,
                    );
                    new_func.instructions.extend(instrs);
                }

                GirInstruction::TileStore { base, row, col, src, tile_rows, tile_cols, stride, dtype } => {
                    let info = tile_map.get(src).cloned().unwrap_or_else(|| TileInfo {
                        location: TileLocation::Register(*src),
                        rows: *tile_rows, cols: *tile_cols, dtype: *dtype,
                    });
                    let instrs = self.expand_tile_store(
                        base, row, col, *tile_rows, *tile_cols, stride, *dtype,
                        &info, &mut new_func,
                    );
                    new_func.instructions.extend(instrs);
                }

                GirInstruction::TileMatmul { dst, a, b, m, k, n, dtype_a, dtype_b, dtype_c } => {
                    let info_a = tile_map.get(a).cloned();
                    let info_b = tile_map.get(b).cloned();
                    let info_dst = TileInfo {
                        location: TileLocation::Register(*dst),
                        rows: *m, cols: *n, dtype: *dtype_c,
                    };
                    tile_map.insert(*dst, info_dst);
                    let instrs = self.expand_tile_matmul(
                        *dst, *m, *k, *n,
                        *dtype_a, *dtype_b, *dtype_c,
                        &info_a, &info_b, &mut new_func,
                    );
                    new_func.instructions.extend(instrs);
                }

                _ => {
                    new_func.instructions.push(instr.clone());
                }
            }
        }

        new_func.shared_mem_size = new_func.shared_mem_size.max(shared_mem_offset);
        new_func
    }

    pub fn expand_program(&self, prog: &GirProgram) -> GirProgram {
        let mut new_prog = GirProgram::new();
        for kernel in &prog.kernels {
            let expanded = self.expand_kernel(kernel);
            new_prog.add_kernel(expanded);
        }
        new_prog
    }

    fn expand_tile_load(
        &self, base: &GirOperand, row: &GirOperand, col: &GirOperand,
        tile_rows: usize, tile_cols: usize, stride: &GirOperand, dtype: GirDType,
        sm_offset: usize, func: &mut GirFunction,
    ) -> Vec<GirInstruction> {
        let mut instrs = Vec::new();
        let elem_size = dtype.size_in_bytes() as i64;

        let tid = func.alloc_reg();
        let local_row = func.alloc_reg();
        let local_col = func.alloc_reg();
        let global_row = func.alloc_reg();
        let global_col = func.alloc_reg();
        let g_idx = func.alloc_reg();
        let g_addr = func.alloc_reg();
        let s_idx = func.alloc_reg();
        let s_addr = func.alloc_reg();
        let val = func.alloc_reg();

        instrs.push(GirInstruction::ThreadId { dst: tid, dim: ThreadDim::X });
        instrs.push(GirInstruction::Div { dst: local_row, src1: GirOperand::Reg(tid), src2: GirOperand::Imm(tile_cols as i64), dtype: GirDType::I64 });
        instrs.push(GirInstruction::Mod { dst: local_col, src1: GirOperand::Reg(tid), src2: GirOperand::Imm(tile_cols as i64), dtype: GirDType::I64 });
        instrs.push(GirInstruction::Add { dst: global_row, src1: row.clone(), src2: GirOperand::Reg(local_row), dtype: GirDType::I64 });
        instrs.push(GirInstruction::Add { dst: global_col, src1: col.clone(), src2: GirOperand::Reg(local_col), dtype: GirDType::I64 });
        instrs.push(GirInstruction::Mul { dst: g_idx, src1: GirOperand::Reg(global_row), src2: stride.clone(), dtype: GirDType::I64 });
        instrs.push(GirInstruction::Add { dst: g_idx, src1: GirOperand::Reg(g_idx), src2: GirOperand::Reg(global_col), dtype: GirDType::I64 });
        instrs.push(GirInstruction::Mul { dst: g_addr, src1: GirOperand::Reg(g_idx), src2: GirOperand::Imm(elem_size), dtype: GirDType::I64 });
        instrs.push(GirInstruction::Add { dst: g_addr, src1: GirOperand::Reg(g_addr), src2: base.clone(), dtype: GirDType::I64 });
        instrs.push(GirInstruction::GlobalLoad { dst: val, addr: GirOperand::Reg(g_addr), dtype });
        instrs.push(GirInstruction::Mul { dst: s_idx, src1: GirOperand::Reg(local_row), src2: GirOperand::Imm(tile_cols as i64), dtype: GirDType::I64 });
        instrs.push(GirInstruction::Add { dst: s_idx, src1: GirOperand::Reg(s_idx), src2: GirOperand::Reg(local_col), dtype: GirDType::I64 });
        instrs.push(GirInstruction::Mul { dst: s_addr, src1: GirOperand::Reg(s_idx), src2: GirOperand::Imm(elem_size), dtype: GirDType::I64 });
        instrs.push(GirInstruction::Add { dst: s_addr, src1: GirOperand::Reg(s_addr), src2: GirOperand::Imm(sm_offset as i64), dtype: GirDType::I64 });
        instrs.push(GirInstruction::SharedStore { addr: GirOperand::Reg(s_addr), src: GirOperand::Reg(val), dtype });
        instrs.push(GirInstruction::Barrier);

        instrs
    }

    fn expand_tile_store(
        &self, base: &GirOperand, row: &GirOperand, col: &GirOperand,
        tile_rows: usize, tile_cols: usize, stride: &GirOperand, dtype: GirDType,
        info: &TileInfo, func: &mut GirFunction,
    ) -> Vec<GirInstruction> {
        let mut instrs = Vec::new();
        let elem_size = dtype.size_in_bytes() as i64;

        let tid = func.alloc_reg();
        let local_row = func.alloc_reg();
        let local_col = func.alloc_reg();
        let global_row = func.alloc_reg();
        let global_col = func.alloc_reg();
        let g_idx = func.alloc_reg();
        let g_addr = func.alloc_reg();

        instrs.push(GirInstruction::ThreadId { dst: tid, dim: ThreadDim::X });
        instrs.push(GirInstruction::Div { dst: local_row, src1: GirOperand::Reg(tid), src2: GirOperand::Imm(tile_cols as i64), dtype: GirDType::I64 });
        instrs.push(GirInstruction::Mod { dst: local_col, src1: GirOperand::Reg(tid), src2: GirOperand::Imm(tile_cols as i64), dtype: GirDType::I64 });
        instrs.push(GirInstruction::Add { dst: global_row, src1: row.clone(), src2: GirOperand::Reg(local_row), dtype: GirDType::I64 });
        instrs.push(GirInstruction::Add { dst: global_col, src1: col.clone(), src2: GirOperand::Reg(local_col), dtype: GirDType::I64 });
        instrs.push(GirInstruction::Mul { dst: g_idx, src1: GirOperand::Reg(global_row), src2: stride.clone(), dtype: GirDType::I64 });
        instrs.push(GirInstruction::Add { dst: g_idx, src1: GirOperand::Reg(g_idx), src2: GirOperand::Reg(global_col), dtype: GirDType::I64 });
        instrs.push(GirInstruction::Mul { dst: g_addr, src1: GirOperand::Reg(g_idx), src2: GirOperand::Imm(elem_size), dtype: GirDType::I64 });
        instrs.push(GirInstruction::Add { dst: g_addr, src1: GirOperand::Reg(g_addr), src2: base.clone(), dtype: GirDType::I64 });

        match info.location {
            TileLocation::Register(reg_id) => {
                // 结果在寄存器中: 直接 store 寄存器值到 global memory
                instrs.push(GirInstruction::GlobalStore {
                    addr: GirOperand::Reg(g_addr),
                    src: GirOperand::Reg(reg_id),
                    dtype,
                });
            }
            TileLocation::SharedMemory(sm_offset) => {
                // 结果在 shared memory: 先 load 再 store
                let s_idx = func.alloc_reg();
                let s_addr = func.alloc_reg();
                let val = func.alloc_reg();
                instrs.push(GirInstruction::Mul { dst: s_idx, src1: GirOperand::Reg(local_row), src2: GirOperand::Imm(tile_cols as i64), dtype: GirDType::I64 });
                instrs.push(GirInstruction::Add { dst: s_idx, src1: GirOperand::Reg(s_idx), src2: GirOperand::Reg(local_col), dtype: GirDType::I64 });
                instrs.push(GirInstruction::Mul { dst: s_addr, src1: GirOperand::Reg(s_idx), src2: GirOperand::Imm(elem_size), dtype: GirDType::I64 });
                instrs.push(GirInstruction::Add { dst: s_addr, src1: GirOperand::Reg(s_addr), src2: GirOperand::Imm(sm_offset as i64), dtype: GirDType::I64 });
                instrs.push(GirInstruction::SharedLoad { dst: val, addr: GirOperand::Reg(s_addr), dtype });
                instrs.push(GirInstruction::GlobalStore { addr: GirOperand::Reg(g_addr), src: GirOperand::Reg(val), dtype });
            }
        }

        instrs
    }

    fn expand_tile_matmul(
        &self, dst: usize, m: usize, k: usize, n: usize,
        dtype_a: GirDType, dtype_b: GirDType, dtype_c: GirDType,
        info_a: &Option<TileInfo>, info_b: &Option<TileInfo>,
        func: &mut GirFunction,
    ) -> Vec<GirInstruction> {
        let mut instrs = Vec::new();
        let dtype = dtype_c;
        let elem_size = dtype.size_in_bytes() as i64;

        let (a_offset, a_cols) = match info_a {
            Some(info) => match info.location {
                TileLocation::SharedMemory(off) => (off as i64, info.cols as i64),
                TileLocation::Register(_) => (0, m as i64),
            },
            None => (0, m as i64),
        };
        let (b_offset, b_cols) = match info_b {
            Some(info) => match info.location {
                TileLocation::SharedMemory(off) => (off as i64, info.cols as i64),
                TileLocation::Register(_) => (0, k as i64),
            },
            None => (0, k as i64),
        };

        let tid = func.alloc_reg();
        let row_reg = func.alloc_reg();
        let col_reg = func.alloc_reg();

        instrs.push(GirInstruction::ThreadId { dst: tid, dim: ThreadDim::X });
        instrs.push(GirInstruction::Div { dst: row_reg, src1: GirOperand::Reg(tid), src2: GirOperand::Imm(n as i64), dtype: GirDType::I64 });
        instrs.push(GirInstruction::Mod { dst: col_reg, src1: GirOperand::Reg(tid), src2: GirOperand::Imm(n as i64), dtype: GirDType::I64 });

        // acc = 0
        instrs.push(GirInstruction::Move { dst, src: GirOperand::Imm(0) });

        // for kk in range(k): acc += A[row][kk] * B[kk][col]
        for kk in 0..k {
            let a_idx = func.alloc_reg();
            let a_addr = func.alloc_reg();
            let a_val = func.alloc_reg();

            instrs.push(GirInstruction::Mul { dst: a_idx, src1: GirOperand::Reg(row_reg), src2: GirOperand::Imm(a_cols), dtype: GirDType::I64 });
            instrs.push(GirInstruction::Add { dst: a_idx, src1: GirOperand::Reg(a_idx), src2: GirOperand::Imm(kk as i64), dtype: GirDType::I64 });
            instrs.push(GirInstruction::Mul { dst: a_addr, src1: GirOperand::Reg(a_idx), src2: GirOperand::Imm(elem_size), dtype: GirDType::I64 });
            instrs.push(GirInstruction::Add { dst: a_addr, src1: GirOperand::Reg(a_addr), src2: GirOperand::Imm(a_offset), dtype: GirDType::I64 });
            instrs.push(GirInstruction::SharedLoad { dst: a_val, addr: GirOperand::Reg(a_addr), dtype: dtype_a });

            let b_idx = func.alloc_reg();
            let b_addr = func.alloc_reg();
            let b_val = func.alloc_reg();

            instrs.push(GirInstruction::Mul { dst: b_idx, src1: GirOperand::Imm(kk as i64), src2: GirOperand::Imm(b_cols), dtype: GirDType::I64 });
            instrs.push(GirInstruction::Add { dst: b_idx, src1: GirOperand::Reg(b_idx), src2: GirOperand::Reg(col_reg), dtype: GirDType::I64 });
            instrs.push(GirInstruction::Mul { dst: b_addr, src1: GirOperand::Reg(b_idx), src2: GirOperand::Imm(elem_size), dtype: GirDType::I64 });
            instrs.push(GirInstruction::Add { dst: b_addr, src1: GirOperand::Reg(b_addr), src2: GirOperand::Imm(b_offset), dtype: GirDType::I64 });
            instrs.push(GirInstruction::SharedLoad { dst: b_val, addr: GirOperand::Reg(b_addr), dtype: dtype_b });

            // acc += a_val * b_val
            instrs.push(GirInstruction::Fma {
                dst,
                src1: GirOperand::Reg(a_val),
                src2: GirOperand::Reg(b_val),
                src3: GirOperand::Reg(dst),
                dtype,
            });
        }

        instrs
    }
}
