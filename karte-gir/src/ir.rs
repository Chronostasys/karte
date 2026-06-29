//! GIR 指令集与数据结构定义

use std::fmt;
use karte_hir::FloatKind;

/// GIR 操作数 — GPU 感知的值表示
#[derive(Debug, Clone, PartialEq)]
pub enum GirOperand {
    /// 虚拟寄存器
    Reg(usize),
    /// 立即数（存储为 i64，浮点数存储为 bit pattern）
    Imm(i64),
    /// 标签引用
    Label(usize),
    /// 参数引用（kernel 参数索引）
    Param(usize),
}

/// 数据类型 — 决定 GPU 指令的类型后缀
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GirDType {
    I32,
    I64,
    F16,
    F32,
    F64,
}

impl GirDType {
    /// 字节大小
    pub fn size_in_bytes(&self) -> usize {
        match self {
            GirDType::I32 => 4,
            GirDType::I64 | GirDType::F64 => 8,
            GirDType::F16 => 2,
            GirDType::F32 => 4,
        }
    }

    /// PTX 类型后缀
    pub fn ptx_suffix(&self) -> &'static str {
        match self {
            GirDType::I32 => "s32",
            GirDType::I64 => "s64",
            GirDType::F16 => "f16",
            GirDType::F32 => "f32",
            GirDType::F64 => "f64",
        }
    }
}

impl fmt::Display for GirDType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.ptx_suffix())
    }
}

/// 线程维度
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadDim {
    X,
    Y,
    Z,
}

/// 比较操作符
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// Shuffle 操作类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShuffleOp {
    /// 从指定 lane 获取值
    Idx,
    /// 蝶形交换
    Bfly,
    /// 向上聚类
    Up,
    /// 向下聚类
    Down,
    /// 异或
    Xor,
}

/// GIR 指令 — GPU 感知的中间表示
#[derive(Debug, Clone, PartialEq)]
pub enum GirInstruction {
    // —— 标量算术指令（每个线程执行）——
    Move { dst: usize, src: GirOperand },
    Add { dst: usize, src1: GirOperand, src2: GirOperand, dtype: GirDType },
    Sub { dst: usize, src1: GirOperand, src2: GirOperand, dtype: GirDType },
    Mul { dst: usize, src1: GirOperand, src2: GirOperand, dtype: GirDType },
    Div { dst: usize, src1: GirOperand, src2: GirOperand, dtype: GirDType },
    /// 取余: dst = src1 % src2
    Mod { dst: usize, src1: GirOperand, src2: GirOperand, dtype: GirDType },
    /// 融合乘加: dst = src1 * src2 + src3
    Fma { dst: usize, src1: GirOperand, src2: GirOperand, src3: GirOperand, dtype: GirDType },
    /// 近似指数函数: dst ≈ exp(src)
    /// PtxCompiler 展开为: mul.f32 (×log2(e)) + ex2.approx.f32
    Exp { dst: usize, src: GirOperand, dtype: GirDType },
    /// 倒数: dst = 1.0 / src（PTX: recip.approx）
    Recip { dst: usize, src: GirOperand, dtype: GirDType },

    // —— 比较与分支 ——
    Cmp { dst: usize, op: CmpOp, src1: GirOperand, src2: GirOperand, dtype: GirDType },
    /// 条件为真跳转
    BranchIf { cond: GirOperand, then_label: usize, else_label: usize },
    /// 无条件跳转
    Jump { target: usize },

    // —— GPU 内存层次 ——
    /// 从全局内存标量加载
    GlobalLoad { dst: usize, addr: GirOperand, dtype: GirDType },
    /// 存储到全局内存（标量）
    GlobalStore { addr: GirOperand, src: GirOperand, dtype: GirDType },
    /// 向量化加载 — 一次加载 4 个连续 f32（128-bit），结果放入 dst..dst+3 寄存器
    /// addr 必须是 16 字节对齐的
    GlobalLoadV4 { dst_base: usize, addr: GirOperand, dtype: GirDType },
    /// 向量化存储 — 将 src_base..src_base+3 的 4 个 f32 一次写入
    GlobalStoreV4 { addr: GirOperand, src_base: usize, dtype: GirDType },
    /// 向量化加载 2 个 f32（64-bit）
    GlobalLoadV2 { dst_base: usize, addr: GirOperand, dtype: GirDType },
    /// 向量化存储 2 个 f32
    GlobalStoreV2 { addr: GirOperand, src_base: usize, dtype: GirDType },
    /// 从共享内存加载
    SharedLoad { dst: usize, addr: GirOperand, dtype: GirDType },
    /// 存储到共享内存
    SharedStore { addr: GirOperand, src: GirOperand, dtype: GirDType },

    // —— GPU 同步与通信 ——
    /// 线程块屏障同步
    Barrier,
    /// Warp shuffle
    WarpShuffle { dst: usize, src: GirOperand, src_lane: GirOperand, op: ShuffleOp, dtype: GirDType },

    // —— Tensor Core ——
    /// 矩阵乘加: dst = a × b + dst
    Mma {
        dst: usize, a: GirOperand, b: GirOperand,
        m: usize, k: usize, n: usize,
        dtype_a: GirDType, dtype_b: GirDType, dtype_c: GirDType,
    },

    // —— 高级 Tile 操作（在 tile_expansion pass 中展开为线程级指令）——
    /// 声明共享内存
    SharedAlloc { dst: usize, size: usize, dtype: GirDType },
    /// 从全局内存加载 tile 到共享内存/寄存器
    /// base=张量基地址, row/col=起始坐标, tile_rows/tile_cols=tile大小, stride=行步幅
    TileLoad {
        dst: usize, base: GirOperand, row: GirOperand, col: GirOperand,
        tile_rows: usize, tile_cols: usize, stride: GirOperand, dtype: GirDType,
    },
    /// 将 tile 从寄存器/共享内存写回全局内存
    TileStore {
        base: GirOperand, row: GirOperand, col: GirOperand,
        src: usize, tile_rows: usize, tile_cols: usize, stride: GirOperand, dtype: GirDType,
    },
    /// 零初始化 tile（累加器）
    TileZeros { dst: usize, tile_rows: usize, tile_cols: usize, dtype: GirDType },
    /// Tile 矩阵乘法: dst += a × b
    TileMatmul {
        dst: usize, a: usize, b: usize,
        m: usize, k: usize, n: usize,
        dtype_a: GirDType, dtype_b: GirDType, dtype_c: GirDType,
    },

    // —— 线程索引 ——
    /// 获取线程索引: dst = threadIdx.{dim}
    ThreadId { dst: usize, dim: ThreadDim },
    /// 获取块索引: dst = blockIdx.{dim}
    BlockId { dst: usize, dim: ThreadDim },
    /// 获取块维度: dst = blockDim.{dim}
    BlockDim { dst: usize, dim: ThreadDim },
    /// 获取网格维度: dst = gridDim.{dim}
    GridDim { dst: usize, dim: ThreadDim },

    // —— 标签与控制流 ——
    Label { id: usize },
    Return,
}

/// GIR kernel 参数
#[derive(Debug, Clone)]
pub struct GirParam {
    pub name: String,
    pub dtype: GirDType,
    /// 是否为指针（Tensor 参数）
    pub is_ptr: bool,
}

/// GIR 函数 = 一个 GPU kernel
#[derive(Debug, Clone)]
pub struct GirFunction {
    pub name: String,
    pub instructions: Vec<GirInstruction>,
    pub params: Vec<GirParam>,
    /// 声明的共享内存总量（字节）
    pub shared_mem_size: usize,
    /// 下一个虚拟寄存器 ID
    pub next_reg: usize,
    /// 下一个标签 ID
    pub next_label: usize,
    /// 建议 grid 配置
    pub grid_dim: (usize, usize, usize),
    /// 建议 block 配置
    pub block_dim: (usize, usize, usize),
}

impl GirFunction {
    pub fn new(name: String) -> Self {
        Self {
            name,
            instructions: Vec::new(),
            params: Vec::new(),
            shared_mem_size: 0,
            next_reg: 0,
            next_label: 0,
            grid_dim: (1, 1, 1),
            block_dim: (256, 1, 1),
        }
    }

    /// 分配新寄存器
    pub fn alloc_reg(&mut self) -> usize {
        let id = self.next_reg;
        self.next_reg += 1;
        id
    }

    /// 分配 N 个连续寄存器，返回起始 ID
    /// 用于向量化操作（如 GlobalLoadV4 需要 4 个连续寄存器）
    pub fn alloc_regs(&mut self, n: usize) -> usize {
        let id = self.next_reg;
        self.next_reg += n;
        id
    }

    /// 分配新标签
    pub fn alloc_label(&mut self) -> usize {
        let id = self.next_label;
        self.next_label += 1;
        id
    }

    /// 添加指令
    pub fn emit(&mut self, instr: GirInstruction) {
        self.instructions.push(instr);
    }
}

/// GIR 程序 = 一组 GPU kernel
#[derive(Debug, Clone, Default)]
pub struct GirProgram {
    pub kernels: Vec<GirFunction>,
}

impl GirProgram {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_kernel(&mut self, kernel: GirFunction) {
        self.kernels.push(kernel);
    }
}

/// 将 FloatKind 转换为 GirDType
impl From<FloatKind> for GirDType {
    fn from(fk: FloatKind) -> Self {
        match fk {
            FloatKind::F16 | FloatKind::BF16 => GirDType::F16,
            FloatKind::F32 => GirDType::F32,
            FloatKind::F64 => GirDType::F64,
        }
    }
}
