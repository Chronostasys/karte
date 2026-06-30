//! GIR JSON 序列化/反序列化模块
//!
//! 提供 GirInstruction / GirOperand / GirFunction / GirProgram 与 JSON 之间的双向转换，
//! 使外部程序（Python 等）能通过 JSON 与 karte 编译器交互。

use crate::ir::*;
use serde::{Deserialize, Serialize};

// ============================================================================
// 辅助：枚举 ↔ 字符串转换
// ============================================================================

fn dtype_to_str(d: GirDType) -> &'static str {
    match d {
        GirDType::F32 => "f32",
        GirDType::I32 => "i32",
        GirDType::I64 => "i64",
        GirDType::F16 => "f16",
        GirDType::F64 => "f64",
    }
}

fn str_to_dtype(s: &str) -> GirDType {
    match s {
        "f32" => GirDType::F32,
        "i32" => GirDType::I32,
        "i64" => GirDType::I64,
        "f16" => GirDType::F16,
        "f64" => GirDType::F64,
        _ => GirDType::F32, // 默认回退
    }
}

fn cmp_op_to_str(op: CmpOp) -> &'static str {
    match op {
        CmpOp::Eq => "eq",
        CmpOp::Ne => "ne",
        CmpOp::Lt => "lt",
        CmpOp::Le => "le",
        CmpOp::Gt => "gt",
        CmpOp::Ge => "ge",
    }
}

fn str_to_cmp_op(s: &str) -> CmpOp {
    match s {
        "eq" => CmpOp::Eq,
        "ne" => CmpOp::Ne,
        "lt" => CmpOp::Lt,
        "le" => CmpOp::Le,
        "gt" => CmpOp::Gt,
        "ge" => CmpOp::Ge,
        _ => CmpOp::Eq,
    }
}

fn thread_dim_to_str(dim: ThreadDim) -> &'static str {
    match dim {
        ThreadDim::X => "x",
        ThreadDim::Y => "y",
        ThreadDim::Z => "z",
    }
}

fn str_to_thread_dim(s: &str) -> ThreadDim {
    match s {
        "x" => ThreadDim::X,
        "y" => ThreadDim::Y,
        "z" => ThreadDim::Z,
        _ => ThreadDim::X,
    }
}

fn shuffle_op_to_str(op: ShuffleOp) -> &'static str {
    match op {
        ShuffleOp::Idx => "idx",
        ShuffleOp::Bfly => "bfly",
        ShuffleOp::Up => "up",
        ShuffleOp::Down => "down",
        ShuffleOp::Xor => "xor",
    }
}

fn str_to_shuffle_op(s: &str) -> ShuffleOp {
    match s {
        "idx" => ShuffleOp::Idx,
        "bfly" => ShuffleOp::Bfly,
        "up" => ShuffleOp::Up,
        "down" => ShuffleOp::Down,
        "xor" => ShuffleOp::Xor,
        _ => ShuffleOp::Idx,
    }
}

fn reduce_op_to_str(op: ReduceOp) -> &'static str {
    match op {
        ReduceOp::Sum => "sum",
        ReduceOp::Max => "max",
        ReduceOp::Min => "min",
    }
}

fn str_to_reduce_op(s: &str) -> ReduceOp {
    match s {
        "sum" => ReduceOp::Sum,
        "max" => ReduceOp::Max,
        "min" => ReduceOp::Min,
        _ => ReduceOp::Sum,
    }
}

// ============================================================================
// GirOperand ↔ GirOperandJson
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind")]
pub enum GirOperandJson {
    Reg { id: usize },
    Imm { val: i64 },
    Param { id: usize },
    Label { id: usize },
}

impl From<&GirOperand> for GirOperandJson {
    fn from(op: &GirOperand) -> Self {
        match op {
            GirOperand::Reg(id) => GirOperandJson::Reg { id: *id },
            GirOperand::Imm(val) => GirOperandJson::Imm { val: *val },
            GirOperand::Param(id) => GirOperandJson::Param { id: *id },
            GirOperand::Label(id) => GirOperandJson::Label { id: *id },
        }
    }
}

impl From<GirOperandJson> for GirOperand {
    fn from(json: GirOperandJson) -> Self {
        match json {
            GirOperandJson::Reg { id } => GirOperand::Reg(id),
            GirOperandJson::Imm { val } => GirOperand::Imm(val),
            GirOperandJson::Param { id } => GirOperand::Param(id),
            GirOperandJson::Label { id } => GirOperand::Label(id),
        }
    }
}

// ============================================================================
// GirInstruction ↔ GirInstructionJson
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op")]
pub enum GirInstructionJson {
    Move {
        dst: usize,
        src: GirOperandJson,
    },
    Add {
        dst: usize,
        src1: GirOperandJson,
        src2: GirOperandJson,
        dtype: String,
    },
    Sub {
        dst: usize,
        src1: GirOperandJson,
        src2: GirOperandJson,
        dtype: String,
    },
    Mul {
        dst: usize,
        src1: GirOperandJson,
        src2: GirOperandJson,
        dtype: String,
    },
    Div {
        dst: usize,
        src1: GirOperandJson,
        src2: GirOperandJson,
        dtype: String,
    },
    Mod {
        dst: usize,
        src1: GirOperandJson,
        src2: GirOperandJson,
        dtype: String,
    },
    Fma {
        dst: usize,
        src1: GirOperandJson,
        src2: GirOperandJson,
        src3: GirOperandJson,
        dtype: String,
    },
    Exp {
        dst: usize,
        src: GirOperandJson,
        dtype: String,
    },
    Recip {
        dst: usize,
        src: GirOperandJson,
        dtype: String,
    },
    Cmp {
        dst: usize,
        #[serde(rename = "cmp")]
        op: String,
        src1: GirOperandJson,
        src2: GirOperandJson,
        dtype: String,
    },
    BranchIf {
        cond: GirOperandJson,
        then_label: usize,
        else_label: usize,
    },
    Jump {
        target: usize,
    },
    GlobalLoad {
        dst: usize,
        addr: GirOperandJson,
        dtype: String,
    },
    GlobalStore {
        addr: GirOperandJson,
        src: GirOperandJson,
        dtype: String,
    },
    GlobalLoadV4 {
        dst_base: usize,
        addr: GirOperandJson,
        dtype: String,
    },
    GlobalStoreV4 {
        addr: GirOperandJson,
        src_base: usize,
        dtype: String,
    },
    GlobalLoadV2 {
        dst_base: usize,
        addr: GirOperandJson,
        dtype: String,
    },
    GlobalStoreV2 {
        addr: GirOperandJson,
        src_base: usize,
        dtype: String,
    },
    SharedLoad {
        dst: usize,
        addr: GirOperandJson,
        dtype: String,
    },
    SharedStore {
        addr: GirOperandJson,
        src: GirOperandJson,
        dtype: String,
    },
    Barrier,
    WarpShuffle {
        dst: usize,
        src: GirOperandJson,
        src_lane: GirOperandJson,
        #[serde(rename = "shuffle")]
        op: String,
        dtype: String,
    },
    Mma {
        dst: usize,
        a: GirOperandJson,
        b: GirOperandJson,
        m: usize,
        k: usize,
        n: usize,
        dtype_a: String,
        dtype_b: String,
        dtype_c: String,
    },
    SharedAlloc {
        dst: usize,
        size: usize,
        dtype: String,
    },
    TileLoad {
        dst: usize,
        base: GirOperandJson,
        row: GirOperandJson,
        col: GirOperandJson,
        tile_rows: usize,
        tile_cols: usize,
        stride: GirOperandJson,
        dtype: String,
    },
    TileStore {
        base: GirOperandJson,
        row: GirOperandJson,
        col: GirOperandJson,
        src: usize,
        tile_rows: usize,
        tile_cols: usize,
        stride: GirOperandJson,
        dtype: String,
    },
    TileZeros {
        dst: usize,
        tile_rows: usize,
        tile_cols: usize,
        dtype: String,
    },
    TileMatmul {
        dst: usize,
        a: usize,
        b: usize,
        m: usize,
        k: usize,
        n: usize,
        dtype_a: String,
        dtype_b: String,
        dtype_c: String,
    },
    ThreadId {
        dst: usize,
        dim: String,
    },
    BlockId {
        dst: usize,
        dim: String,
    },
    BlockDim {
        dst: usize,
        dim: String,
    },
    GridDim {
        dst: usize,
        dim: String,
    },
    Label {
        id: usize,
    },
    Return,
    MaskedGlobalLoad {
        dst: usize,
        addr: GirOperandJson,
        mask: GirOperandJson,
        default_val: GirOperandJson,
        dtype: String,
    },
    MaskedGlobalStore {
        addr: GirOperandJson,
        src: GirOperandJson,
        mask: GirOperandJson,
        dtype: String,
    },
    Reduce {
        dst: usize,
        src: GirOperandJson,
        #[serde(rename = "reduce")]
        op: String,
        dtype: String,
    },
    Where {
        dst: usize,
        cond: GirOperandJson,
        then_val: GirOperandJson,
        else_val: GirOperandJson,
        dtype: String,
    },
    Sqrt {
        dst: usize,
        src: GirOperandJson,
        dtype: String,
    },
    Log {
        dst: usize,
        src: GirOperandJson,
        dtype: String,
    },
    Rsqrt {
        dst: usize,
        src: GirOperandJson,
        dtype: String,
    },
    Abs {
        dst: usize,
        src: GirOperandJson,
        dtype: String,
    },
    Max {
        dst: usize,
        src1: GirOperandJson,
        src2: GirOperandJson,
        dtype: String,
    },
    Min {
        dst: usize,
        src1: GirOperandJson,
        src2: GirOperandJson,
        dtype: String,
    },
    Tanh {
        dst: usize,
        src: GirOperandJson,
        dtype: String,
    },
    Cos {
        dst: usize,
        src: GirOperandJson,
        dtype: String,
    },
    Sin {
        dst: usize,
        src: GirOperandJson,
        dtype: String,
    },
    Clamp {
        dst: usize,
        src: GirOperandJson,
        lo: GirOperandJson,
        hi: GirOperandJson,
        dtype: String,
    },
    Lerp {
        dst: usize,
        a: GirOperandJson,
        b: GirOperandJson,
        t: GirOperandJson,
        dtype: String,
    },
    Ceil {
        dst: usize,
        src: GirOperandJson,
        dtype: String,
    },
    Floor {
        dst: usize,
        src: GirOperandJson,
        dtype: String,
    },
    Pow {
        dst: usize,
        base: GirOperandJson,
        exp: GirOperandJson,
        dtype: String,
    },
}

impl From<&GirInstruction> for GirInstructionJson {
    fn from(instr: &GirInstruction) -> Self {
        match instr {
            // —— 标量算术 ——
            GirInstruction::Move { dst, src } => GirInstructionJson::Move {
                dst: *dst,
                src: GirOperandJson::from(src),
            },
            GirInstruction::Add { dst, src1, src2, dtype } => GirInstructionJson::Add {
                dst: *dst,
                src1: GirOperandJson::from(src1),
                src2: GirOperandJson::from(src2),
                dtype: dtype_to_str(*dtype).to_string(),
            },
            GirInstruction::Sub { dst, src1, src2, dtype } => GirInstructionJson::Sub {
                dst: *dst,
                src1: GirOperandJson::from(src1),
                src2: GirOperandJson::from(src2),
                dtype: dtype_to_str(*dtype).to_string(),
            },
            GirInstruction::Mul { dst, src1, src2, dtype } => GirInstructionJson::Mul {
                dst: *dst,
                src1: GirOperandJson::from(src1),
                src2: GirOperandJson::from(src2),
                dtype: dtype_to_str(*dtype).to_string(),
            },
            GirInstruction::Div { dst, src1, src2, dtype } => GirInstructionJson::Div {
                dst: *dst,
                src1: GirOperandJson::from(src1),
                src2: GirOperandJson::from(src2),
                dtype: dtype_to_str(*dtype).to_string(),
            },
            GirInstruction::Mod { dst, src1, src2, dtype } => GirInstructionJson::Mod {
                dst: *dst,
                src1: GirOperandJson::from(src1),
                src2: GirOperandJson::from(src2),
                dtype: dtype_to_str(*dtype).to_string(),
            },
            GirInstruction::Fma { dst, src1, src2, src3, dtype } => GirInstructionJson::Fma {
                dst: *dst,
                src1: GirOperandJson::from(src1),
                src2: GirOperandJson::from(src2),
                src3: GirOperandJson::from(src3),
                dtype: dtype_to_str(*dtype).to_string(),
            },
            GirInstruction::Exp { dst, src, dtype } => GirInstructionJson::Exp {
                dst: *dst,
                src: GirOperandJson::from(src),
                dtype: dtype_to_str(*dtype).to_string(),
            },
            GirInstruction::Recip { dst, src, dtype } => GirInstructionJson::Recip {
                dst: *dst,
                src: GirOperandJson::from(src),
                dtype: dtype_to_str(*dtype).to_string(),
            },

            // —— 比较与分支 ——
            GirInstruction::Cmp { dst, op, src1, src2, dtype } => GirInstructionJson::Cmp {
                dst: *dst,
                op: cmp_op_to_str(*op).to_string(),
                src1: GirOperandJson::from(src1),
                src2: GirOperandJson::from(src2),
                dtype: dtype_to_str(*dtype).to_string(),
            },
            GirInstruction::BranchIf { cond, then_label, else_label } => GirInstructionJson::BranchIf {
                cond: GirOperandJson::from(cond),
                then_label: *then_label,
                else_label: *else_label,
            },
            GirInstruction::Jump { target } => GirInstructionJson::Jump {
                target: *target,
            },

            // —— GPU 内存层次 ——
            GirInstruction::GlobalLoad { dst, addr, dtype } => GirInstructionJson::GlobalLoad {
                dst: *dst,
                addr: GirOperandJson::from(addr),
                dtype: dtype_to_str(*dtype).to_string(),
            },
            GirInstruction::GlobalStore { addr, src, dtype } => GirInstructionJson::GlobalStore {
                addr: GirOperandJson::from(addr),
                src: GirOperandJson::from(src),
                dtype: dtype_to_str(*dtype).to_string(),
            },
            GirInstruction::GlobalLoadV4 { dst_base, addr, dtype } => GirInstructionJson::GlobalLoadV4 {
                dst_base: *dst_base,
                addr: GirOperandJson::from(addr),
                dtype: dtype_to_str(*dtype).to_string(),
            },
            GirInstruction::GlobalStoreV4 { addr, src_base, dtype } => GirInstructionJson::GlobalStoreV4 {
                addr: GirOperandJson::from(addr),
                src_base: *src_base,
                dtype: dtype_to_str(*dtype).to_string(),
            },
            GirInstruction::GlobalLoadV2 { dst_base, addr, dtype } => GirInstructionJson::GlobalLoadV2 {
                dst_base: *dst_base,
                addr: GirOperandJson::from(addr),
                dtype: dtype_to_str(*dtype).to_string(),
            },
            GirInstruction::GlobalStoreV2 { addr, src_base, dtype } => GirInstructionJson::GlobalStoreV2 {
                addr: GirOperandJson::from(addr),
                src_base: *src_base,
                dtype: dtype_to_str(*dtype).to_string(),
            },
            GirInstruction::SharedLoad { dst, addr, dtype } => GirInstructionJson::SharedLoad {
                dst: *dst,
                addr: GirOperandJson::from(addr),
                dtype: dtype_to_str(*dtype).to_string(),
            },
            GirInstruction::SharedStore { addr, src, dtype } => GirInstructionJson::SharedStore {
                addr: GirOperandJson::from(addr),
                src: GirOperandJson::from(src),
                dtype: dtype_to_str(*dtype).to_string(),
            },

            // —— GPU 同步与通信 ——
            GirInstruction::Barrier => GirInstructionJson::Barrier,
            GirInstruction::WarpShuffle { dst, src, src_lane, op, dtype } => GirInstructionJson::WarpShuffle {
                dst: *dst,
                src: GirOperandJson::from(src),
                src_lane: GirOperandJson::from(src_lane),
                op: shuffle_op_to_str(*op).to_string(),
                dtype: dtype_to_str(*dtype).to_string(),
            },

            // —— Tensor Core ——
            GirInstruction::Mma { dst, a, b, m, k, n, dtype_a, dtype_b, dtype_c } => GirInstructionJson::Mma {
                dst: *dst,
                a: GirOperandJson::from(a),
                b: GirOperandJson::from(b),
                m: *m,
                k: *k,
                n: *n,
                dtype_a: dtype_to_str(*dtype_a).to_string(),
                dtype_b: dtype_to_str(*dtype_b).to_string(),
                dtype_c: dtype_to_str(*dtype_c).to_string(),
            },

            // —— 高级 Tile 操作 ——
            GirInstruction::SharedAlloc { dst, size, dtype } => GirInstructionJson::SharedAlloc {
                dst: *dst,
                size: *size,
                dtype: dtype_to_str(*dtype).to_string(),
            },
            GirInstruction::TileLoad { dst, base, row, col, tile_rows, tile_cols, stride, dtype } => GirInstructionJson::TileLoad {
                dst: *dst,
                base: GirOperandJson::from(base),
                row: GirOperandJson::from(row),
                col: GirOperandJson::from(col),
                tile_rows: *tile_rows,
                tile_cols: *tile_cols,
                stride: GirOperandJson::from(stride),
                dtype: dtype_to_str(*dtype).to_string(),
            },
            GirInstruction::TileStore { base, row, col, src, tile_rows, tile_cols, stride, dtype } => GirInstructionJson::TileStore {
                base: GirOperandJson::from(base),
                row: GirOperandJson::from(row),
                col: GirOperandJson::from(col),
                src: *src,
                tile_rows: *tile_rows,
                tile_cols: *tile_cols,
                stride: GirOperandJson::from(stride),
                dtype: dtype_to_str(*dtype).to_string(),
            },
            GirInstruction::TileZeros { dst, tile_rows, tile_cols, dtype } => GirInstructionJson::TileZeros {
                dst: *dst,
                tile_rows: *tile_rows,
                tile_cols: *tile_cols,
                dtype: dtype_to_str(*dtype).to_string(),
            },
            GirInstruction::TileMatmul { dst, a, b, m, k, n, dtype_a, dtype_b, dtype_c } => GirInstructionJson::TileMatmul {
                dst: *dst,
                a: *a,
                b: *b,
                m: *m,
                k: *k,
                n: *n,
                dtype_a: dtype_to_str(*dtype_a).to_string(),
                dtype_b: dtype_to_str(*dtype_b).to_string(),
                dtype_c: dtype_to_str(*dtype_c).to_string(),
            },

            // —— 线程索引 ——
            GirInstruction::ThreadId { dst, dim } => GirInstructionJson::ThreadId {
                dst: *dst,
                dim: thread_dim_to_str(*dim).to_string(),
            },
            GirInstruction::BlockId { dst, dim } => GirInstructionJson::BlockId {
                dst: *dst,
                dim: thread_dim_to_str(*dim).to_string(),
            },
            GirInstruction::BlockDim { dst, dim } => GirInstructionJson::BlockDim {
                dst: *dst,
                dim: thread_dim_to_str(*dim).to_string(),
            },
            GirInstruction::GridDim { dst, dim } => GirInstructionJson::GridDim {
                dst: *dst,
                dim: thread_dim_to_str(*dim).to_string(),
            },

            // —— 高级数学与条件操作 ——
            GirInstruction::MaskedGlobalLoad { dst, addr, mask, default_val, dtype } => GirInstructionJson::MaskedGlobalLoad {
                dst: *dst,
                addr: GirOperandJson::from(addr),
                mask: GirOperandJson::from(mask),
                default_val: GirOperandJson::from(default_val),
                dtype: dtype_to_str(*dtype).to_string(),
            },
            GirInstruction::MaskedGlobalStore { addr, src, mask, dtype } => GirInstructionJson::MaskedGlobalStore {
                addr: GirOperandJson::from(addr),
                src: GirOperandJson::from(src),
                mask: GirOperandJson::from(mask),
                dtype: dtype_to_str(*dtype).to_string(),
            },
            GirInstruction::Reduce { dst, src, op, dtype } => GirInstructionJson::Reduce {
                dst: *dst,
                src: GirOperandJson::from(src),
                op: reduce_op_to_str(*op).to_string(),
                dtype: dtype_to_str(*dtype).to_string(),
            },
            GirInstruction::Where { dst, cond, then_val, else_val, dtype } => GirInstructionJson::Where {
                dst: *dst,
                cond: GirOperandJson::from(cond),
                then_val: GirOperandJson::from(then_val),
                else_val: GirOperandJson::from(else_val),
                dtype: dtype_to_str(*dtype).to_string(),
            },
            GirInstruction::Sqrt { dst, src, dtype } => GirInstructionJson::Sqrt {
                dst: *dst,
                src: GirOperandJson::from(src),
                dtype: dtype_to_str(*dtype).to_string(),
            },
            GirInstruction::Log { dst, src, dtype } => GirInstructionJson::Log {
                dst: *dst,
                src: GirOperandJson::from(src),
                dtype: dtype_to_str(*dtype).to_string(),
            },
            GirInstruction::Rsqrt { dst, src, dtype } => GirInstructionJson::Rsqrt {
                dst: *dst,
                src: GirOperandJson::from(src),
                dtype: dtype_to_str(*dtype).to_string(),
            },
            GirInstruction::Abs { dst, src, dtype } => GirInstructionJson::Abs {
                dst: *dst,
                src: GirOperandJson::from(src),
                dtype: dtype_to_str(*dtype).to_string(),
            },
            GirInstruction::Max { dst, src1, src2, dtype } => GirInstructionJson::Max {
                dst: *dst,
                src1: GirOperandJson::from(src1),
                src2: GirOperandJson::from(src2),
                dtype: dtype_to_str(*dtype).to_string(),
            },
            GirInstruction::Min { dst, src1, src2, dtype } => GirInstructionJson::Min {
                dst: *dst,
                src1: GirOperandJson::from(src1),
                src2: GirOperandJson::from(src2),
                dtype: dtype_to_str(*dtype).to_string(),
            },
            GirInstruction::Tanh { dst, src, dtype } => GirInstructionJson::Tanh {
                dst: *dst,
                src: GirOperandJson::from(src),
                dtype: dtype_to_str(*dtype).to_string(),
            },
            GirInstruction::Cos { dst, src, dtype } => GirInstructionJson::Cos {
                dst: *dst,
                src: GirOperandJson::from(src),
                dtype: dtype_to_str(*dtype).to_string(),
            },
            GirInstruction::Sin { dst, src, dtype } => GirInstructionJson::Sin {
                dst: *dst,
                src: GirOperandJson::from(src),
                dtype: dtype_to_str(*dtype).to_string(),
            },
            GirInstruction::Clamp { dst, src, lo, hi, dtype } => GirInstructionJson::Clamp {
                dst: *dst,
                src: GirOperandJson::from(src),
                lo: GirOperandJson::from(lo),
                hi: GirOperandJson::from(hi),
                dtype: dtype_to_str(*dtype).to_string(),
            },
            GirInstruction::Lerp { dst, a, b, t, dtype } => GirInstructionJson::Lerp {
                dst: *dst,
                a: GirOperandJson::from(a),
                b: GirOperandJson::from(b),
                t: GirOperandJson::from(t),
                dtype: dtype_to_str(*dtype).to_string(),
            },
            GirInstruction::Ceil { dst, src, dtype } => GirInstructionJson::Ceil {
                dst: *dst,
                src: GirOperandJson::from(src),
                dtype: dtype_to_str(*dtype).to_string(),
            },
            GirInstruction::Floor { dst, src, dtype } => GirInstructionJson::Floor {
                dst: *dst,
                src: GirOperandJson::from(src),
                dtype: dtype_to_str(*dtype).to_string(),
            },
            GirInstruction::Pow { dst, base, exp, dtype } => GirInstructionJson::Pow {
                dst: *dst,
                base: GirOperandJson::from(base),
                exp: GirOperandJson::from(exp),
                dtype: dtype_to_str(*dtype).to_string(),
            },

            // —— 标签与控制流 ——
            GirInstruction::Label { id } => GirInstructionJson::Label { id: *id },
            GirInstruction::Return => GirInstructionJson::Return,
        }
    }
}

impl From<&GirInstructionJson> for GirInstruction {
    fn from(json: &GirInstructionJson) -> Self {
        match json {
            GirInstructionJson::Move { dst, src } => GirInstruction::Move {
                dst: *dst,
                src: GirOperand::from(src.clone()),
            },
            GirInstructionJson::Add { dst, src1, src2, dtype } => GirInstruction::Add {
                dst: *dst,
                src1: GirOperand::from(src1.clone()),
                src2: GirOperand::from(src2.clone()),
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::Sub { dst, src1, src2, dtype } => GirInstruction::Sub {
                dst: *dst,
                src1: GirOperand::from(src1.clone()),
                src2: GirOperand::from(src2.clone()),
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::Mul { dst, src1, src2, dtype } => GirInstruction::Mul {
                dst: *dst,
                src1: GirOperand::from(src1.clone()),
                src2: GirOperand::from(src2.clone()),
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::Div { dst, src1, src2, dtype } => GirInstruction::Div {
                dst: *dst,
                src1: GirOperand::from(src1.clone()),
                src2: GirOperand::from(src2.clone()),
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::Mod { dst, src1, src2, dtype } => GirInstruction::Mod {
                dst: *dst,
                src1: GirOperand::from(src1.clone()),
                src2: GirOperand::from(src2.clone()),
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::Fma { dst, src1, src2, src3, dtype } => GirInstruction::Fma {
                dst: *dst,
                src1: GirOperand::from(src1.clone()),
                src2: GirOperand::from(src2.clone()),
                src3: GirOperand::from(src3.clone()),
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::Exp { dst, src, dtype } => GirInstruction::Exp {
                dst: *dst,
                src: GirOperand::from(src.clone()),
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::Recip { dst, src, dtype } => GirInstruction::Recip {
                dst: *dst,
                src: GirOperand::from(src.clone()),
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::Cmp { dst, op, src1, src2, dtype } => GirInstruction::Cmp {
                dst: *dst,
                op: str_to_cmp_op(op),
                src1: GirOperand::from(src1.clone()),
                src2: GirOperand::from(src2.clone()),
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::BranchIf { cond, then_label, else_label } => GirInstruction::BranchIf {
                cond: GirOperand::from(cond.clone()),
                then_label: *then_label,
                else_label: *else_label,
            },
            GirInstructionJson::Jump { target } => GirInstruction::Jump { target: *target },

            GirInstructionJson::GlobalLoad { dst, addr, dtype } => GirInstruction::GlobalLoad {
                dst: *dst,
                addr: GirOperand::from(addr.clone()),
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::GlobalStore { addr, src, dtype } => GirInstruction::GlobalStore {
                addr: GirOperand::from(addr.clone()),
                src: GirOperand::from(src.clone()),
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::GlobalLoadV4 { dst_base, addr, dtype } => GirInstruction::GlobalLoadV4 {
                dst_base: *dst_base,
                addr: GirOperand::from(addr.clone()),
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::GlobalStoreV4 { addr, src_base, dtype } => GirInstruction::GlobalStoreV4 {
                addr: GirOperand::from(addr.clone()),
                src_base: *src_base,
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::GlobalLoadV2 { dst_base, addr, dtype } => GirInstruction::GlobalLoadV2 {
                dst_base: *dst_base,
                addr: GirOperand::from(addr.clone()),
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::GlobalStoreV2 { addr, src_base, dtype } => GirInstruction::GlobalStoreV2 {
                addr: GirOperand::from(addr.clone()),
                src_base: *src_base,
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::SharedLoad { dst, addr, dtype } => GirInstruction::SharedLoad {
                dst: *dst,
                addr: GirOperand::from(addr.clone()),
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::SharedStore { addr, src, dtype } => GirInstruction::SharedStore {
                addr: GirOperand::from(addr.clone()),
                src: GirOperand::from(src.clone()),
                dtype: str_to_dtype(dtype),
            },

            GirInstructionJson::Barrier => GirInstruction::Barrier,

            GirInstructionJson::WarpShuffle { dst, src, src_lane, op, dtype } => GirInstruction::WarpShuffle {
                dst: *dst,
                src: GirOperand::from(src.clone()),
                src_lane: GirOperand::from(src_lane.clone()),
                op: str_to_shuffle_op(op),
                dtype: str_to_dtype(dtype),
            },

            GirInstructionJson::Mma { dst, a, b, m, k, n, dtype_a, dtype_b, dtype_c } => GirInstruction::Mma {
                dst: *dst,
                a: GirOperand::from(a.clone()),
                b: GirOperand::from(b.clone()),
                m: *m,
                k: *k,
                n: *n,
                dtype_a: str_to_dtype(dtype_a),
                dtype_b: str_to_dtype(dtype_b),
                dtype_c: str_to_dtype(dtype_c),
            },

            GirInstructionJson::SharedAlloc { dst, size, dtype } => GirInstruction::SharedAlloc {
                dst: *dst,
                size: *size,
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::TileLoad { dst, base, row, col, tile_rows, tile_cols, stride, dtype } => GirInstruction::TileLoad {
                dst: *dst,
                base: GirOperand::from(base.clone()),
                row: GirOperand::from(row.clone()),
                col: GirOperand::from(col.clone()),
                tile_rows: *tile_rows,
                tile_cols: *tile_cols,
                stride: GirOperand::from(stride.clone()),
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::TileStore { base, row, col, src, tile_rows, tile_cols, stride, dtype } => GirInstruction::TileStore {
                base: GirOperand::from(base.clone()),
                row: GirOperand::from(row.clone()),
                col: GirOperand::from(col.clone()),
                src: *src,
                tile_rows: *tile_rows,
                tile_cols: *tile_cols,
                stride: GirOperand::from(stride.clone()),
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::TileZeros { dst, tile_rows, tile_cols, dtype } => GirInstruction::TileZeros {
                dst: *dst,
                tile_rows: *tile_rows,
                tile_cols: *tile_cols,
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::TileMatmul { dst, a, b, m, k, n, dtype_a, dtype_b, dtype_c } => GirInstruction::TileMatmul {
                dst: *dst,
                a: *a,
                b: *b,
                m: *m,
                k: *k,
                n: *n,
                dtype_a: str_to_dtype(dtype_a),
                dtype_b: str_to_dtype(dtype_b),
                dtype_c: str_to_dtype(dtype_c),
            },

            GirInstructionJson::ThreadId { dst, dim } => GirInstruction::ThreadId {
                dst: *dst,
                dim: str_to_thread_dim(dim),
            },
            GirInstructionJson::BlockId { dst, dim } => GirInstruction::BlockId {
                dst: *dst,
                dim: str_to_thread_dim(dim),
            },
            GirInstructionJson::BlockDim { dst, dim } => GirInstruction::BlockDim {
                dst: *dst,
                dim: str_to_thread_dim(dim),
            },
            GirInstructionJson::GridDim { dst, dim } => GirInstruction::GridDim {
                dst: *dst,
                dim: str_to_thread_dim(dim),
            },

            GirInstructionJson::MaskedGlobalLoad { dst, addr, mask, default_val, dtype } => GirInstruction::MaskedGlobalLoad {
                dst: *dst,
                addr: GirOperand::from(addr.clone()),
                mask: GirOperand::from(mask.clone()),
                default_val: GirOperand::from(default_val.clone()),
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::MaskedGlobalStore { addr, src, mask, dtype } => GirInstruction::MaskedGlobalStore {
                addr: GirOperand::from(addr.clone()),
                src: GirOperand::from(src.clone()),
                mask: GirOperand::from(mask.clone()),
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::Reduce { dst, src, op, dtype } => GirInstruction::Reduce {
                dst: *dst,
                src: GirOperand::from(src.clone()),
                op: str_to_reduce_op(op),
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::Where { dst, cond, then_val, else_val, dtype } => GirInstruction::Where {
                dst: *dst,
                cond: GirOperand::from(cond.clone()),
                then_val: GirOperand::from(then_val.clone()),
                else_val: GirOperand::from(else_val.clone()),
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::Sqrt { dst, src, dtype } => GirInstruction::Sqrt {
                dst: *dst,
                src: GirOperand::from(src.clone()),
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::Log { dst, src, dtype } => GirInstruction::Log {
                dst: *dst,
                src: GirOperand::from(src.clone()),
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::Rsqrt { dst, src, dtype } => GirInstruction::Rsqrt {
                dst: *dst,
                src: GirOperand::from(src.clone()),
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::Abs { dst, src, dtype } => GirInstruction::Abs {
                dst: *dst,
                src: GirOperand::from(src.clone()),
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::Max { dst, src1, src2, dtype } => GirInstruction::Max {
                dst: *dst,
                src1: GirOperand::from(src1.clone()),
                src2: GirOperand::from(src2.clone()),
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::Min { dst, src1, src2, dtype } => GirInstruction::Min {
                dst: *dst,
                src1: GirOperand::from(src1.clone()),
                src2: GirOperand::from(src2.clone()),
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::Tanh { dst, src, dtype } => GirInstruction::Tanh {
                dst: *dst,
                src: GirOperand::from(src.clone()),
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::Cos { dst, src, dtype } => GirInstruction::Cos {
                dst: *dst,
                src: GirOperand::from(src.clone()),
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::Sin { dst, src, dtype } => GirInstruction::Sin {
                dst: *dst,
                src: GirOperand::from(src.clone()),
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::Clamp { dst, src, lo, hi, dtype } => GirInstruction::Clamp {
                dst: *dst,
                src: GirOperand::from(src.clone()),
                lo: GirOperand::from(lo.clone()),
                hi: GirOperand::from(hi.clone()),
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::Lerp { dst, a, b, t, dtype } => GirInstruction::Lerp {
                dst: *dst,
                a: GirOperand::from(a.clone()),
                b: GirOperand::from(b.clone()),
                t: GirOperand::from(t.clone()),
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::Ceil { dst, src, dtype } => GirInstruction::Ceil {
                dst: *dst,
                src: GirOperand::from(src.clone()),
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::Floor { dst, src, dtype } => GirInstruction::Floor {
                dst: *dst,
                src: GirOperand::from(src.clone()),
                dtype: str_to_dtype(dtype),
            },
            GirInstructionJson::Pow { dst, base, exp, dtype } => GirInstruction::Pow {
                dst: *dst,
                base: GirOperand::from(base.clone()),
                exp: GirOperand::from(exp.clone()),
                dtype: str_to_dtype(dtype),
            },

            GirInstructionJson::Label { id } => GirInstruction::Label { id: *id },
            GirInstructionJson::Return => GirInstruction::Return,
        }
    }
}

// ============================================================================
// GirParam / GirFunction / GirProgram ↔ JSON
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GirParamJson {
    pub name: String,
    pub dtype: String,
    pub is_ptr: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GirFunctionJson {
    pub name: String,
    pub params: Vec<GirParamJson>,
    pub instructions: Vec<GirInstructionJson>,
    pub next_reg: usize,
    pub next_label: usize,
    pub block_dim: [usize; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GirProgramJson {
    pub kernels: Vec<GirFunctionJson>,
}

// --- From/Into 转换 ---

impl From<&GirFunction> for GirFunctionJson {
    fn from(f: &GirFunction) -> Self {
        GirFunctionJson {
            name: f.name.clone(),
            params: f
                .params
                .iter()
                .map(|p| GirParamJson {
                    name: p.name.clone(),
                    dtype: dtype_to_str(p.dtype).to_string(),
                    is_ptr: p.is_ptr,
                })
                .collect(),
            instructions: f.instructions.iter().map(GirInstructionJson::from).collect(),
            next_reg: f.next_reg,
            next_label: f.next_label,
            block_dim: [f.block_dim.0, f.block_dim.1, f.block_dim.2],
        }
    }
}

impl From<&GirFunctionJson> for GirFunction {
    fn from(f: &GirFunctionJson) -> Self {
        GirFunction {
            name: f.name.clone(),
            instructions: f.instructions.iter().map(GirInstruction::from).collect(),
            params: f
                .params
                .iter()
                .map(|p| GirParam {
                    name: p.name.clone(),
                    dtype: str_to_dtype(&p.dtype),
                    is_ptr: p.is_ptr,
                })
                .collect(),
            shared_mem_size: 0,
            next_reg: f.next_reg,
            next_label: f.next_label,
            grid_dim: (1, 1, 1),
            block_dim: (f.block_dim[0], f.block_dim[1], f.block_dim[2]),
        }
    }
}

impl From<&GirProgram> for GirProgramJson {
    fn from(p: &GirProgram) -> Self {
        GirProgramJson {
            kernels: p.kernels.iter().map(GirFunctionJson::from).collect(),
        }
    }
}

impl From<&GirProgramJson> for GirProgram {
    fn from(p: &GirProgramJson) -> Self {
        GirProgram {
            kernels: p.kernels.iter().map(GirFunction::from).collect(),
        }
    }
}

// ============================================================================
// 公共 API
// ============================================================================

/// 将 GirProgram 序列化为 JSON 字符串
pub fn serialize_program(program: &GirProgram) -> String {
    let json = GirProgramJson::from(program);
    serde_json::to_string_pretty(&json).unwrap_or_else(|e| {
        format!(r#"{{"error": "{}"}}"#, e)
    })
}

/// 从 JSON 字符串反序列化 GirProgram
pub fn deserialize_program(json: &str) -> Result<GirProgram, String> {
    let program_json: GirProgramJson =
        serde_json::from_str(json).map_err(|e| format!("JSON 解析失败: {}", e))?;
    Ok(GirProgram::from(&program_json))
}

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_operand_roundtrip() {
        let cases = vec![
            GirOperand::Reg(42),
            GirOperand::Imm(-999),
            GirOperand::Param(1),
            GirOperand::Label(7),
        ];
        for op in cases {
            let json = GirOperandJson::from(&op);
            let json_str = serde_json::to_string(&json).unwrap();
            let parsed: GirOperandJson = serde_json::from_str(&json_str).unwrap();
            let roundtrip = GirOperand::from(parsed);
            assert_eq!(op, roundtrip, "operand roundtrip failed");
        }
    }

    #[test]
    fn test_instruction_roundtrip() {
        let cases = vec![
            GirInstruction::Move { dst: 0, src: GirOperand::Imm(42) },
            GirInstruction::Add { dst: 1, src1: GirOperand::Reg(0), src2: GirOperand::Imm(1), dtype: GirDType::I32 },
            GirInstruction::Cmp { dst: 2, op: CmpOp::Lt, src1: GirOperand::Reg(1), src2: GirOperand::Imm(10), dtype: GirDType::I32 },
            GirInstruction::BranchIf { cond: GirOperand::Reg(2), then_label: 0, else_label: 1 },
            GirInstruction::GlobalLoad { dst: 3, addr: GirOperand::Param(0), dtype: GirDType::F32 },
            GirInstruction::Barrier,
            GirInstruction::ThreadId { dst: 4, dim: ThreadDim::X },
            GirInstruction::Label { id: 0 },
            GirInstruction::Return,
            // 新指令
            GirInstruction::Sqrt { dst: 5, src: GirOperand::Reg(3), dtype: GirDType::F32 },
            GirInstruction::Max { dst: 6, src1: GirOperand::Reg(5), src2: GirOperand::Imm(0), dtype: GirDType::F32 },
            GirInstruction::Reduce { dst: 7, src: GirOperand::Reg(6), op: ReduceOp::Sum, dtype: GirDType::F32 },
            GirInstruction::Where { dst: 8, cond: GirOperand::Reg(2), then_val: GirOperand::Reg(1), else_val: GirOperand::Imm(0), dtype: GirDType::I32 },
            GirInstruction::MaskedGlobalLoad { dst: 9, addr: GirOperand::Param(0), mask: GirOperand::Reg(2), default_val: GirOperand::Imm(0), dtype: GirDType::F32 },
        ];
        for instr in cases {
            let json = GirInstructionJson::from(&instr);
            let json_str = serde_json::to_string(&json).unwrap();
            let parsed: GirInstructionJson = serde_json::from_str(&json_str).unwrap();
            let roundtrip = GirInstruction::from(&parsed);
            assert_eq!(instr, roundtrip, "instruction roundtrip failed");
        }
    }

    #[test]
    fn test_function_roundtrip() {
        let mut func = GirFunction::new("test_kernel".to_string());
        func.params.push(GirParam { name: "A".to_string(), dtype: GirDType::F32, is_ptr: true });
        func.params.push(GirParam { name: "n".to_string(), dtype: GirDType::I32, is_ptr: false });
        let tid = func.alloc_reg();
        func.emit(GirInstruction::ThreadId { dst: tid, dim: ThreadDim::X });
        let ld = func.alloc_reg();
        func.emit(GirInstruction::GlobalLoad { dst: ld, addr: GirOperand::Param(0), dtype: GirDType::F32 });
        func.emit(GirInstruction::Return);

        let func_json = GirFunctionJson::from(&func);
        let json_str = serde_json::to_string(&func_json).unwrap();
        let parsed: GirFunctionJson = serde_json::from_str(&json_str).unwrap();
        let roundtrip = GirFunction::from(&parsed);

        assert_eq!(func.name, roundtrip.name);
        assert_eq!(func.params.len(), roundtrip.params.len());
        assert_eq!(func.instructions, roundtrip.instructions);
        assert_eq!(func.next_reg, roundtrip.next_reg);
        assert_eq!(func.next_label, roundtrip.next_label);
        assert_eq!(func.block_dim, roundtrip.block_dim);
    }

    #[test]
    fn test_program_roundtrip() {
        let mut program = GirProgram::new();
        let mut func = GirFunction::new("kernel1".to_string());
        func.emit(GirInstruction::Move { dst: 0, src: GirOperand::Imm(42) });
        func.emit(GirInstruction::Return);
        program.add_kernel(func);

        let json_str = serialize_program(&program);
        let roundtrip = deserialize_program(&json_str).unwrap();

        assert_eq!(program.kernels.len(), roundtrip.kernels.len());
        assert_eq!(program.kernels[0].instructions, roundtrip.kernels[0].instructions);
    }
}
