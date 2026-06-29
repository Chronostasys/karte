# Karte GPU 算子开发语言设计方案

> 版本: v1.0 | 日期: 2026-06-29
> 状态: 设计阶段

---

## 目录

- [1. 设计愿景与目标](#1-设计愿景与目标)
- [2. 与主流框架的差异化定位](#2-与主流框架的差异化定位)
- [3. 语言设计](#3-语言设计)
- [4. 类型系统扩展](#4-类型系统扩展)
- [5. 编译流水线设计](#5-编译流水线设计)
- [6. GIR — GPU 中间表示](#6-gir--gpu-中间表示)
- [7. GPU 后端代码生成](#7-gpu-后端代码生成)
- [8. GPU 运行时设计](#8-gpu-运行时设计)
- [9. 标准库设计](#9-标准库设计)
- [10. 分阶段实施路线图](#10-分阶段实施路线图)
- [11. 逐文件改动清单](#11-逐文件改动清单)
- [12. 风险分析与缓解](#12-风险分析与缓解)

---

## 1. 设计愿景与目标

### 1.1 一句话定位

> **Karte-GPU：一个使用 Karte 语法编写高性能 GPU 算子的编译器扩展，以 tile 抽象为核心，将算法与硬件调度分离。**

### 1.2 设计目标

| 优先级 | 目标 | 衡量标准 |
|--------|------|---------|
| P0 | **能用 Karte 语法编写并在 GPU 上运行 kernel** | vec_add / matmul 通过编译并返回正确结果 |
| P0 | **tile 抽象屏蔽 SIMT 复杂度** | 用户不需要手写 thread_id / shared_memory |
| P1 | **达到 Triton 同级性能** | GEMM 在 A100/H100 上达到 cuBLAS 80%+ |
| P1 | **多后端支持** | 至少支持 NVIDIA (PTX) + AMD (GCN) |
| P2 | **自动调优** | 自动搜索最优 tile size / 流水线深度 |
| P2 | **CPU 回退执行** | 同一 kernel 可在 CPU 上调试运行 |

### 1.3 不做什么

- ❌ 不做 PyTorch/TensorFlow 级别的自动微分（这是框架的事）
- ❌ 不做动态图执行（Karte 是编译型语言）
- ❌ 第一阶段不做 CPU SIMD 向量化（保持 CPU 路径简单）

---

## 2. 与主流框架的差异化定位

```
性能 ↑
  │  CUDA C++ (手写)
  │       ╱
  │     ╱  Karte-GPU (本方案)
  │    ╱      ╱
  │  TileLang ╱
  │        ╱  Triton
  │      ╱
  │    ╱
  │  ╱  cuBLAS (预编译库)
  │╱ PyTorch (调用 cuBLAS)
  └──────────────────────→ 开发效率
```

### 2.1 对比分析

| 维度 | CUDA C++ | Triton | TileLang | **Karte-GPU** |
|------|----------|--------|----------|---------------|
| 语言层级 | 汇编级 | Python DSL | Python DSL | **系统语言（原生编译）** |
| tile 抽象 | ❌ 手写 | ✅ 隐式 | ✅ 显式 | ✅ **类型系统内建** |
| 共享内存 | 手动 | 自动 | 半自动 | **类型驱动** |
| Tensor Core | 手写 PTX | 自动 | 自动 | **声明式** |
| 编译方式 | NVCC→PTX→SASS | Python→MLIR→LLVM | Python→TVM/TIR | **Karte→GIR→PTX** |
| 宿主代码 | C++ | Python | Python | **同一语言** |
| 类型安全 | 弱 | Python 动态 | Python 动态 | **静态强类型** |
| 调试体验 | 差 (cuda-gdb) | 中 | 中 | **CPU 回退调试** |

### 2.2 核心差异化

Karte-GPU 的独特优势在于：

1. **宿主与 kernel 同一语言**：不需要 Python host + C++ kernel 的割裂。Karte 代码中 CPU 函数和 GPU kernel 无缝共存。
2. **静态类型 + shape 推断**：编译期捕获维度不匹配、越界访问等错误，而非运行时崩溃。
3. **编译型语言性能**：没有 Python 解释器开销，host 端 dispatch 直接编译为机器码。
4. **CPU 回退**：同一份 tile 代码可以在 CPU 上运行（单线程模拟），便于调试和 CI。

---

## 3. 语言设计

### 3.1 `kernel` 函数

GPU kernel 使用 `kernel fn` 关键字声明，区别于普通 `fn`：

```karte
// 向量加法 kernel
kernel fn vec_add(
    x: &Tensor<f32, 1>,        // 1D f32 张量
    y: &Tensor<f32, 1>,
    out: &mut Tensor<f32, 1>,
    n: number,                  // 元素总数
) {
    let i = thread_global_id(); // 当前线程的全局一维索引
    if i < n {
        out[i] = x[i] + y[i];
    }
}
```

**语义**：
- `kernel fn` 标记的函数在 **GPU 上以 SIMT 模式执行**
- 所有线程执行相同的代码，通过 `thread_global_id()` 区分各自的工作
- kernel 不能有返回值（结果写入 `out` 参数）
- kernel 不能直接调用普通 `fn`（但普通 `fn` 可以调用 kernel）

### 3.2 kernel 启动语法

普通函数中使用 `launch!` 宏启动 kernel：

```karte
fn main() -> number {
    let n = 1024;
    let x = tensor_zeros::<f32>(n);       // 在 GPU 上分配
    let y = tensor_zeros::<f32>(n);
    let mut out = tensor_zeros::<f32>(n);

    // 启动 kernel: (blocks, threads_per_block)
    launch!(vec_add, (32, 32))(&x, &y, &mut out, n);

    // 或者自动计算 grid 配置
    launch_auto!(vec_add, n, 256)(&x, &y, &mut out, n);

    out[0]  // 返回第一个元素验证
}
```

### 3.3 Tile 抽象（核心特性）

Tile 是 Karte-GPU 的核心抽象，表示一个固定大小的数据块，由一个线程块（block）协作处理：

```karte
// 矩阵乘法 kernel — 使用 tile 抽象
kernel fn matmul(
    a: &Tensor<f16, 2>,        // M×K 矩阵
    b: &Tensor<f16, 2>,        // K×N 矩阵
    c: &mut Tensor<f32, 2>,    // M×N 矩阵
    m: number, k: number, n: number,
) {
    // 声明 tile — 编译期确定大小
    // 每个线程块处理一个 BM×BN 的输出 tile
    let bm: const = 128;
    let bn: const = 64;
    let bk: const = 32;

    // 获取当前线程块负责的 tile 坐标
    let (tile_m, tile_n) = block_id_2d();  // (blockIdx.x, blockIdx.y)

    // 分配累加器 tile（存储在寄存器/共享内存中）
    let acc = tile_zeros::<f32>(bm, bn);

    // 沿 K 维度迭代
    for tk in 0..(k / bk) {
        // 从全局内存加载 tile 到共享内存
        // 编译器自动处理: 合并访问 + shared memory 分配 + bank conflict 消除
        let a_tile = tile_load(a, tile_m * bm, tk * bk, bm, bk);
        let b_tile = tile_load(b, tk * bk, tile_n * bn, bk, bn);

        // tile 矩阵乘法 — 映射到 Tensor Core MMA 指令
        acc = tile_matmul(acc, a_tile, b_tile);
    }

    // 将结果 tile 写回全局内存
    tile_store(c, tile_m * bm, tile_n * bn, acc);
}
```

### 3.4 共享内存与同步

当用户需要比 tile 更精细的控制时：

```karte
kernel fn reduce(x: &Tensor<f32, 1>, out: &mut Tensor<f32, 1>, n: number) {
    let tid = thread_local_id();   // threadIdx.x
    let bid = block_id();           // blockIdx.x

    // 共享内存声明 — block 内所有线程可见
    let smem = shared::<f32>(256);

    // 从全局内存加载到共享内存
    let gid = bid * 256 + tid;
    if gid < n {
        smem[tid] = x[gid];
    }

    sync_threads();  // 屏障同步

    // 树形归约
    let mut s = 256 / 2;
    while s > 0 {
        if tid < s && gid + s < n {
            smem[tid] = smem[tid] + smem[tid + s];
        }
        sync_threads();
        s = s / 2;
    }

    if tid == 0 {
        out[bid] = smem[0];
    }
}
```

### 3.5 GPU 内建函数一览

| 函数 | 返回类型 | 说明 |
|------|---------|------|
| `thread_global_id()` | `number` | 全局线程索引（一维） |
| `thread_local_id()` | `number` | 块内线程索引 (`threadIdx.x`) |
| `block_id()` | `number` | 块索引（一维）(`blockIdx.x`) |
| `block_id_2d()` | `(number, number)` | 块索引（二维）(`blockIdx.x/y`) |
| `block_dim()` | `number` | 块内线程数 (`blockDim.x`) |
| `grid_dim()` | `number` | 网格块数 (`gridDim.x`) |
| `sync_threads()` | `()` | 块内屏障同步 (`__syncthreads`) |
| `shared::<T>(n)` | `SharedMem<T>` | 分配共享内存 |
| `tile_load(...)` | `Tile<T>` | 从全局内存加载 tile |
| `tile_store(...)` | `()` | 将 tile 写回全局内存 |
| `tile_zeros::<T>(m, n)` | `Tile<T>` | 分配零初始化 tile |
| `tile_matmul(...)` | `Tile<T>` | tile 矩阵乘法 |
| `warp_shuffle(val, src_lane)` | `T` | warp 内寄存器交换 |
| `warp_reduce(val, op)` | `T` | warp 级归约 |

### 3.6 语法关键字总结

新增关键字（在 Parser 中识别）：

| 关键字 | 用途 | AST 表示 |
|--------|------|---------|
| `kernel` | 声明 GPU kernel 函数 | `Statement::KernelDef` |
| `launch!` | 启动 kernel | `Expr::KernelLaunch` |
| `shared` | 共享内存声明 | `Expr::SharedAlloc` |
| `tile` | tile 类型标注 | `Type::Tile` |

> 注：`const` 量（编译期常量）可以复用现有 `let` + `const` 修饰符，无需新增关键字。

---

## 4. 类型系统扩展

### 4.1 新增 Type 变体

在 `karte-hir/src/types.rs` 的 `Type` enum 中新增：

```rust
pub enum Type {
    // ... 现有变体 ...

    /// GPU 张量：多维数组，存储在 GPU 全局内存中
    Tensor {
        dtype: Box<Type>,    // 元素类型: f32, f16, i32, etc.
        ndim: usize,          // 维度数: 1=向量, 2=矩阵, ...
    },

    /// GPU Tile：固定大小的数据块，存储在寄存器/共享内存中
    /// 由一个线程块协作持有
    Tile {
        dtype: Box<Type>,    // 元素类型
        rows: usize,          // 行数（编译期常量）
        cols: usize,          // 列数
    },

    /// 共享内存引用
    SharedMem {
        dtype: Box<Type>,    // 元素类型
        len: usize,           // 元素数量
    },
}
```

### 4.2 浮点类型支持

当前 Karte 只有 `Number`（i64）。GPU 需要 f32/f16/f64。扩展 `IntKind` 为 `ScalarKind`：

```rust
pub enum ScalarKind {
    // 现有整数
    I8, I16, I32, I64,
    U8, U16, U32, U64,
    USize,
    // 新增浮点
    F16,    // 半精度 (GPU 常用)
    BF16,   // 脑浮点 (AI 训练常用)
    F32,    // 单精度
    F64,    // 双精度
}
```

```rust
// 语法糖
type f16 = number::<F16>;   // 底层仍然是 i64 表示，但 codegen 时输出为 .f16
type f32 = number::<F32>;
type f64 = number::<F64>;
```

> **关键设计**：Karte 的运行时值始终是 `i64`，但类型标注携带 `ScalarKind` 信息，GPU 后端在 codegen 时将其映射到 PTX 的 `.f16` / `.f32` / `.f64` 类型。这样不需要改动整个 LIR 指令集。

### 4.3 Shape 推断规则

```
Tensor<f32, 2>          → 2D f32 张量（运行时确定形状）
Tile<f32, 128, 64>      → 128×64 的 f32 tile（编译期确定）
SharedMem<f32, 256>     → 256 个 f32 的共享内存

tile_load(t, row, col, m, n) → Tile<T, m, n>   // 从 Tensor 加载 Tile
tile_store(t, row, col, tile: Tile<T, m, n>) → ()
tile_matmul(a: Tile<T, M, K>, b: Tile<T, K, N>) → Tile<T, M, N>
tile_zeros::<T>(m, n) → Tile<T, m, n>
```

### 4.4 类型检查规则

1. **kernel 参数限制**：kernel 参数只能是 `&Tensor<T,N>`、`&mut Tensor<T,N>`、`number`
2. **tile 生命周期**：tile 只能在 kernel 内部创建和使用，不能跨 kernel 传递
3. **shared memory 限制**：共享内存总量不能超过硬件限制（A100: 164KB, H100: 228KB）
4. **同步规则**：`shared` 内存在 `sync_threads()` 后才能保证可见性

---

## 5. 编译流水线设计

### 5.1 总体架构

```
Karte Source (.karte)
    │
    ├── 普通代码 (fn, let, ...)
    │       │
    │       ├──[1] Lexer → Parser → HIR → MIR → LIR → CPU Codegen (x86/AArch64)
    │       │                                          ↑ 完全复用现有流水线
    │       │
    │       └──[1a] 遇到 launch!(kernel, grid)(args...)
    │                │
    │                ├── 编译期记录: kernel 名称、grid 配置、参数类型
    │                └── LIR 中生成: GpuLaunch 指令（含参数 marshalling）
    │
    └── kernel 代码 (kernel fn ...)
            │
            ├──[2] Lexer → Parser → HIR → MIR (kernel 函数标记)
            │
            ├──[3] MIR → LIR (kernel LIR: 仅含 kernel 内逻辑)
            │
            ├──[4] LIR → GIR (GPU IR: 加入 SIMT 语义 + 内存层次)
            │           ↑ 新增 karte-gir crate
            │
            ├──[5] GIR → PTX (NVIDIA) / GCN (AMD) / SPIR-V (跨平台)
            │           ↑ 新增 karte-gpu crate
            │
            └──[6] PTX → 编译为 GPU 二进制 (cuBIN)
                      ↑ 通过 NVIDIA Driver API JIT
```

### 5.2 双路编译模式

```
┌─────────────────────────────────────────────────────────────┐
│                    Karte Source File                        │
│  fn main() { ... launch!(kernel, grid)(args) ... }         │
│  kernel fn kernel(...) { ... }                              │
└──────────────────────┬──────────────────────────────────────┘
                       │
              ┌────────┴────────┐
              │  Parser + HIR   │  (分离 kernel 和 host 函数)
              └────────┬────────┘
                       │
           ┌───────────┴───────────┐
           ▼                       ▼
    ┌──────────────┐        ┌──────────────┐
    │  Host 路径   │        │ Kernel 路径  │
    │              │        │              │
    │  MIR → LIR   │        │  MIR → LIR   │
    │     ↓        │        │     ↓        │
    │  CPU Codegen │        │  LIR → GIR   │
    │  (x86/ARM)   │        │     ↓        │
    │     ↓        │        │  PTX/SPIR-V  │
    │  Host 机器码  │        │     ↓        │
    └──────┬───────┘        │  GPU Binary  │
           │                └──────┬───────┘
           │                       │
           └───────────┬───────────┘
                       ▼
              ┌────────────────┐
              │  Linker        │  (将 GPU binary 嵌入 host 可执行文件)
              │  Host ELF +    │
              │  GPU kernels   │
              └────────────────┘
```

### 5.3 CLI 扩展

```bash
# 编译并运行（自动检测 GPU kernel，双路编译）
karte run project.karte --gpu

# 编译 GPU kernel 为 PTX 文件（调试用）
karte gpu-compile kernel.karte --emit-ptx -o kernel.ptx

# AOT 编译为包含 GPU kernel 的独立可执行文件
karte aot project.karte -o output --target gpu

# 查看生成的 GIR
karte run project.karte --emit-gir -o project.gir
```

---

## 6. GIR — GPU 中间表示

### 6.1 设计理念

GIR（GPU IR）是 LIR 到 GPU 目标代码之间的中间层，负责：
1. 将标量 LIR 指令提升为 SIMT 语义
2. 插入内存层次标注（global / shared / local）
3. 展开 tile 操作为线程级计算
4. 插入同步指令

### 6.2 数据结构

```rust
// karte-gir/src/lib.rs

/// GIR 指令 — GPU 感知的中间表示
pub enum GirInstruction {
    // —— 标量指令（直接从 LIR 映射，每个线程执行）——
    Move { dst: GirOperand, src: GirOperand },
    Add { dst: GirOperand, src1: GirOperand, src2: GirOperand },
    Sub { dst: GirOperand, src1: GirOperand, src2: GirOperand },
    Mul { dst: GirOperand, src1: GirOperand, src2: GirOperand },
    Fma { dst: GirOperand, src1: GirOperand, src2: GirOperand, src3: GirOperand }, // dst = src1*src2 + src3
    Cmp { dst: GirOperand, op: CmpOp, src1: GirOperand, src2: GirOperand },
    // ... 其他标量指令 ...

    // —— GPU 特有指令 ——
    /// 从全局内存加载
    GlobalLoad {
        dst: GirOperand,
        addr: GirOperand,        // 全局内存地址
        dtype: ScalarKind,       // 数据类型决定加载宽度
        is_coalesced: bool,      // 编译器标记是否合并访问
    },
    /// 存储到全局内存
    GlobalStore {
        addr: GirOperand,
        src: GirOperand,
        dtype: ScalarKind,
    },
    /// 从共享内存加载
    SharedLoad {
        dst: GirOperand,
        addr: GirOperand,        // 共享内存偏移
        dtype: ScalarKind,
    },
    /// 存储到共享内存
    SharedStore {
        addr: GirOperand,
        src: GirOperand,
        dtype: ScalarKind,
    },
    /// 线程块屏障
    Barrier,
    /// Warp shuffle
    WarpShuffle {
        dst: GirOperand,
        src: GirOperand,
        src_lane: GirOperand,
        op: ShuffleOp,           // sync_up / sync_down / xor / idx
    },
    /// Tensor Core 矩阵乘加
    Mma {
        dst: GirOperand,         // 累加器 C (Tile)
        a: GirOperand,           // 矩阵 A
        b: GirOperand,           // 矩阵 B
        m: usize, k: usize, n: usize,  // MMA 指令形状 (如 16×8×16)
        dtype_a: ScalarKind,
        dtype_b: ScalarKind,
        dtype_c: ScalarKind,
    },
    /// 获取线程索引
    ThreadId { dim: ThreadDim },  // .x / .y / .z
    /// 获取块索引
    BlockId { dim: ThreadDim },
    /// 获取块维度
    BlockDim { dim: ThreadDim },

    // —— 控制流（与 LIR 类似）——
    Label { id: LabelId },
    Jump { target: LabelId },
    Branch { cond: GirOperand, then_label: LabelId, else_label: LabelId },
    Return,
}

pub enum ThreadDim { X, Y, Z }

/// GIR 函数 = 一个 GPU kernel
pub struct GirFunction {
    pub name: String,
    pub instructions: Vec<GirInstruction>,
    pub params: Vec<GirParam>,
    pub shared_mem_size: usize,     // 声明的共享内存总量
    pub num_registers: usize,       // 估算的寄存器使用量
    pub grid_dim: (usize, usize, usize),  // 建议 grid 配置
    pub block_dim: (usize, usize, usize), // 建议 block 配置
}

/// GIR 程序 = 一组 GPU kernel
pub struct GirProgram {
    pub kernels: Vec<GirFunction>,
}
```

### 6.3 LIR → GIR 降级 Pass

```rust
// karte-gir/src/lower.rs

pub fn lower_lir_to_gir(
    lir_func: &LirFunction,
    kernel_info: &KernelInfo,    // 来自 HIR 的 kernel 元信息
) -> GirFunction {
    let mut lowerer = GirLowerer::new(kernel_info);

    for instr in &lir_func.instructions {
        lowerer.lower_instruction(instr);
    }

    lowerer.finish()
}
```

降级规则：

| LIR 指令 | GIR 指令 | 说明 |
|----------|---------|------|
| `Load64(addr)` | `GlobalLoad(addr, dtype)` | 根据类型信息标注 dtype |
| `Store64(addr, val)` | `GlobalStore(addr, val, dtype)` | 同上 |
| `Add/Sub/Mul` | `Add/Sub/Mul` | 1:1 映射 |
| `Call("tile_load", ...)` | `GlobalLoad` × N | 展开为多个线程的加载 |
| `Call("tile_store", ...)` | `GlobalStore` × N | 展开为多个线程的存储 |
| `Call("tile_matmul", ...)` | `Mma` × N | 映射到 Tensor Core 指令序列 |
| `Call("sync_threads", ...)` | `Barrier` | 直接映射 |
| `Call("thread_global_id", ...)` | `ThreadId(X) + BlockId(X) * BlockDim(X)` | 组合计算 |

### 6.4 tile 展开策略

tile 操作在 LIR→GIR 降级时展开为线程级操作：

```
// 源码
let a_tile = tile_load(a, row, col, 128, 64);

// GIR (伪代码) — 假设 block 配置为 (256, 1, 1)
// 128×64 = 8192 个元素，256 个线程，每个线程加载 32 个元素
for i in 0..32 {
    let local_row = (tid + i) / 64;   // 0..127
    let local_col = (tid + i) % 64;   // 0..63
    let global_offset = (row + local_row) * stride + (col + local_col);
    smem[tid * 32 + i] = GlobalLoad(a + global_offset);
}
```

**关键优化**：编译器自动选择 tile 到线程的映射方式，保证合并访问（coalesced access）。

---

## 7. GPU 后端代码生成

### 7.1 PTX 后端

```rust
// karte-gpu/src/ptx.rs

pub struct PtxCompiler {
    output: String,           // PTX 文本
    temp_count: usize,        // 临时寄存器计数
    label_count: usize,       // 标签计数
}

impl PtxCompiler {
    pub fn compile(&mut self, gir: &GirProgram) -> String {
        let mut ptx = String::new();
        ptx.push_str(".version 8.0\n");
        ptx.push_str(".target sm_90\n");  // H100, 可配置
        ptx.push_str(".address_size 64\n\n");

        for kernel in &gir.kernels {
            ptx.push_str(&self.compile_kernel(kernel));
        }
        ptx
    }

    fn compile_kernel(&mut self, func: &GirFunction) -> String {
        // .entry kernel_name(.param ...) {
        //     // 寄存器声明
        //     // 指令序列
        // }
    }
}
```

### 7.2 GIR → PTX 映射

| GIR 指令 | PTX 指令 |
|----------|---------|
| `Add(dst, a, b)` (f32) | `add.f32 %rd, %ra, %rb;` |
| `Mul(dst, a, b)` (f32) | `mul.f32 %rd, %ra, %rb;` |
| `Fma(dst, a, b, c)` | `fma.rn.f32 %rd, %ra, %rb, %rc;` |
| `GlobalLoad(dst, addr)` | `ld.global.f32 %rd, [%ra];` |
| `GlobalStore(addr, src)` | `st.global.f32 [%ra], %rs;` |
| `SharedLoad(dst, addr)` | `ld.shared.f32 %rd, [%ra];` |
| `SharedStore(addr, src)` | `st.shared.f32 [%ra], %rs;` |
| `Barrier` | `bar.sync 0;` |
| `ThreadId(X)` | `mov.u32 %rd, %tid.x;` |
| `BlockId(X)` | `mov.u32 %rd, %ctaid.x;` |
| `WarpShuffle(dst, src, lane, op)` | `shfl.sync.bfly.b32 %rd, %rs, 0x1, 0x1f, 0x1f;` |
| `Mma(dst, a, b, m, k, n)` | `mma.m16n8k16.row.col.f32.f16.f16.f32 {...};` |

### 7.3 PTX 代码示例

输入 Karte 代码：
```karte
kernel fn vec_add(x: &Tensor<f32, 1>, y: &Tensor<f32, 1>, out: &mut Tensor<f32, 1>, n: number) {
    let i = thread_global_id();
    if i < n {
        out[i] = x[i] + y[i];
    }
}
```

生成 PTX：
```ptx
.version 8.0
.target sm_80
.address_size 64

.entry vec_add(
    .param .u64 x_ptr,
    .param .u64 y_ptr,
    .param .u64 out_ptr,
    .param .u64 n_val
) {
    .reg .u64 %rd<10>;
    .reg .u32 %r<5>;
    .reg .pred %p<3>;
    .reg .f32 %f<5>;

    // 加载参数
    ld.param.u64 %rd1, [x_ptr];     // x 基地址
    ld.param.u64 %rd2, [y_ptr];     // y 基地址
    ld.param.u64 %rd3, [out_ptr];   // out 基地址
    ld.param.u64 %rd4, [n_val];     // n

    // 计算全局线程索引: i = blockIdx.x * blockDim.x + threadIdx.x
    mov.u32 %r1, %tid.x;
    mov.u32 %r2, %ntid.x;
    mov.u32 %r3, %ctaid.x;
    mad.lo.u32 %r4, %r3, %r2, %r1;  // %r4 = i

    // 比较 i < n
    setp.lo.u64 %p1, %r4, %rd4;
    @%p1 bra IF_TRUE;
    bra IF_END;

IF_TRUE:
    // out[i] = x[i] + y[i]
    cvta.to.global.u64 %rd5, %rd1;
    mad.wide.u32 %rd6, %r4, 4, %rd5;   // x + i * sizeof(f32)
    ld.global.f32 %f1, [%rd6];

    cvta.to.global.u64 %rd7, %rd2;
    mad.wide.u32 %rd8, %r4, 4, %rd7;
    ld.global.f32 %f2, [%rd8];

    add.f32 %f3, %f1, %f2;

    cvta.to.global.u64 %rd9, %rd3;
    mad.wide.u32 %rd10, %r4, 4, %rd9;
    st.global.f32 [%rd10], %f3;

IF_END:
    ret;
}
```

### 7.4 后端抽象 Trait

```rust
// karte-gpu/src/lib.rs

pub trait GpuBackend {
    type Output;

    fn compile(&mut self, gir: &GirProgram) -> Self::Output;
    fn target_name(&self) -> &str;
}

pub struct PtxBackend {
    sm_version: (u32, u32),  // SM 8.0, 9.0, etc.
}

pub struct SpirvBackend {
    // SPIR-V for AMD/Intel/Vulkan
}

pub struct CudaContext {
    device_id: i32,
    // CUDA Driver API context
}
```

---

## 8. GPU 运行时设计

### 8.1 运行时分层

```
┌──────────────────────────────────────────────────┐
│  Karte 用户程序                                   │
│  launch!(kernel, grid)(tensor_args...)           │
└──────────────────┬───────────────────────────────┘
                   │
┌──────────────────┴───────────────────────────────┐
│  karte-gpu-runtime (Rust)                         │
│  ┌──────────────────────────────────────────┐    │
│  │ Tensor Manager                           │    │
│  │ - tensor_alloc(dtype, shape) → Tensor    │    │
│  │ - tensor_free(tensor)                    │    │
│  │ - tensor_htod(host_data) → Tensor        │    │
│  │ - tensor_dtoh(tensor) → host_data        │    │
│  └──────────────────────────────────────────┘    │
│  ┌──────────────────────────────────────────┐    │
│  │ Kernel Launcher                          │    │
│  │ - load_ptx(ptx_text) → KernelModule      │    │
│  │ - launch_kernel(kernel, grid, args)      │    │
│  │ - synchronize()                          │    │
│  └──────────────────────────────────────────┘    │
└──────────────────┬───────────────────────────────┘
                   │
┌──────────────────┴───────────────────────────────┐
│  CUDA Driver API (libcuda.so) / ROCm (libamdcom) │
│  cuInit / cuMemAlloc / cuLaunchKernel / ...      │
└──────────────────────────────────────────────────┘
```

### 8.2 GPU 内存管理

```rust
// karte-gpu-runtime/src/tensor.rs

pub struct GpuTensor {
    device_ptr: u64,        // GPU 设备地址
    dtype: ScalarKind,      // 元素类型
    shape: Vec<usize>,      // 形状
    nbytes: usize,          // 总字节数
}

impl GpuTensor {
    pub fn zeros(dtype: ScalarKind, shape: &[usize]) -> Self {
        let nbytes = shape.iter().product::<usize>() * dtype.size();
        let device_ptr = unsafe { cu_mem_alloc(nbytes as u64) };
        // 初始化为零
        unsafe { cu_memset_d8(device_ptr, 0, nbytes as u64) };
        Self { device_ptr, dtype, shape: shape.to_vec(), nbytes }
    }

    pub fn from_host(data: &[u8], dtype: ScalarKind, shape: &[usize]) -> Self {
        let tensor = Self::zeros(dtype, shape);
        unsafe {
            cu_memcpy_h2d(tensor.device_ptr, data.as_ptr(), data.len());
        }
        tensor
    }

    pub fn to_host(&self) -> Vec<u8> {
        let mut buf = vec![0u8; self.nbytes];
        unsafe {
            cu_memcpy_d2h(buf.as_mut_ptr(), self.device_ptr, self.nbytes);
        }
        buf
    }
}

impl Drop for GpuTensor {
    fn drop(&mut self) {
        unsafe { cu_mem_free(self.device_ptr); }
    }
}
```

### 8.3 Kernel 启动

```rust
// karte-gpu-runtime/src/launcher.rs

pub struct KernelLauncher {
    module: CuModule,
    function: CuFunction,
}

impl KernelLauncher {
    pub fn from_ptx(ptx_source: &str, kernel_name: &str) -> Self {
        let module = unsafe {
            let mut module: CuModule = std::mem::zeroed();
            cu_module_load_data(&mut module, ptx_source.as_ptr() as *const _);
            module
        };
        let function = unsafe {
            let mut func: CuFunction = std::mem::zeroed();
            cu_module_get_function(&mut func, module, kernel_name.as_ptr());
            func
        };
        Self { module, function }
    }

    pub fn launch(
        &self,
        grid: (usize, usize, usize),
        block: (usize, usize, usize),
        args: &mut [u64],        // 参数指针数组
        shared_mem: usize,
    ) {
        unsafe {
            cu_launch_kernel(
                self.function,
                grid.0, grid.1, grid.2,
                block.0, block.1, block.2,
                shared_mem,
                null(),
                args.as_mut_ptr(),
                null(),
            );
        }
    }
}
```

### 8.4 与现有 ExecutionEngine 的集成

```rust
// karte-codegen 中扩展

pub enum ExecutionTarget {
    Cpu,                        // 现有 CPU JIT
    Gpu {                       // GPU 执行
        device_id: i32,
    },
}

impl ExecutionEngine {
    pub fn execute_with_gpu(&mut self, program: &LirProgram) -> Result<i64> {
        // 1. 分离 host 函数和 kernel 函数
        let (host_funcs, kernel_funcs) = split_functions(program);

        // 2. 编译 host 函数 (CPU JIT — 现有路径)
        let host_code = self.compile_host_functions(&host_funcs)?;

        // 3. 编译 kernel 函数 (LIR → GIR → PTX → GPU Binary)
        let gir = lower_lir_to_gir_program(&kernel_funcs);
        let ptx = PtxBackend::new().compile(&gir);
        let gpu_module = GpuModule::from_ptx(&ptx)?;

        // 4. Patch host 代码中的 launch! 指令
        //    将 GpuLaunch 指令替换为调用 GPU runtime 的代码
        self.patch_gpu_launches(&mut host_code, &gpu_module)?;

        // 5. 执行 host 代码
        self.execute(host_code)
    }
}
```

---

## 9. 标准库设计

### 9.1 新增标准库模块

```
std/
├── gpu/
│   ├── tensor.karte      // Tensor 类型定义和基本操作
│   ├── launch.karte      // launch! 宏的实现
│   ├── tile.karte        // tile 操作的高层封装
│   └── memory.karte      // GPU 内存管理辅助函数
```

### 9.2 tensor.karte

```karte
// GPU 张量的 host 端操作

fn tensor_zeros_f32(n: number) -> Tensor<f32, 1> {
    gpu_alloc_f32(n)
}

fn tensor_zeros_2d_f32(m: number, n: number) -> Tensor<f32, 2> {
    gpu_alloc_f32(m * n)
}

fn tensor_to_host_f32(t: &Tensor<f32, 1>, n: number) -> [f32] {
    gpu_memcpy_d2h(t, n)
}

fn tensor_from_host_f32(data: [f32], n: number) -> Tensor<f32, 1> {
    let t = gpu_alloc_f32(n);
    gpu_memcpy_h2d(t, data, n);
    t
}
```

### 9.3 Runtime Primitive（最小集）

遵循 Karte 的"Runtime 只提供 OS 抽象"原则，GPU runtime 只提供最底层的 CUDA API 绑定：

| Runtime Primitive | 功能 | 对应 CUDA API |
|-------------------|------|--------------|
| `gpu_init` | 初始化 GPU 设备 | `cuInit(0)` |
| `gpu_alloc(nbytes)` | 分配显存 | `cuMemAlloc` |
| `gpu_free(ptr)` | 释放显存 | `cuMemFree` |
| `gpu_memcpy_h2d(dst, src, n)` | Host→Device 拷贝 | `cuMemcpyHtoD` |
| `gpu_memcpy_d2h(dst, src, n)` | Device→Host 拷贝 | `cuMemcpyDtoH` |
| `gpu_load_ptx(ptx_text)` | 加载 PTX 模块 | `cuModuleLoadData` |
| `gpu_launch_kernel(...)` | 启动 kernel | `cuLaunchKernel` |
| `gpu_sync()` | 等待 GPU 完成 | `cuCtxSynchronize` |

所有高级操作（如 tensor 初始化、数据格式转换）都在标准库中用 Karte 代码实现。

---

## 10. 分阶段实施路线图

### 阶段 0：基础类型支持（预计 2 周）

**目标**：让 Karte 支持 f32/f16 浮点类型标注

| 任务 | 涉及文件 | 工作量 |
|------|---------|--------|
| 新增 `ScalarKind` 枚举（含 F16/BF16/F32/F64） | `karte-hir/src/types.rs` | 中 |
| Lexer 识别 `f32`/`f16`/`f64` 类型标注 | `karte-lexer/src/lib.rs` | 小 |
| Parser 解析浮点类型标注 | `karte-parser/src/types.rs` | 小 |
| TypeChecker 处理浮点类型 | `karte-hir/src/type_checker.rs` | 中 |
| LIR Instruction 携带 dtype 信息 | `karte-lir/src/ir.rs` | 中 |
| CPU Codegen 输出 SSE/AVX 浮点指令 | `karte-codegen/.../x86_compiler.rs` | 中 |

**验收标准**：`fn main() -> f32 { let x: f32 = 3.14; x }` 能在 CPU 上正确编译和执行。

### 阶段 1：最小 GPU 原型（预计 4 周）

**目标**：vec_add 能在 NVIDIA GPU 上运行

```
输入: kernel fn vec_add(x: &Tensor<f32,1>, y: &Tensor<f32,1>, out: &mut Tensor<f32,1>, n: number)
输出: PTX 文本 + 正确执行结果
```

| 任务 | 新建/修改文件 | 工作量 |
|------|-------------|--------|
| 新增 `kernel fn` 语法（Lexer/Parser/AST） | `karte-parser/src/statement.rs`, `karte-hir/src/ast.rs` | 中 |
| TypeChecker 处理 KernelDef | `karte-hir/src/type_checker.rs` | 中 |
| MIR lowering: KernelDef → MirFunction | `karte-mir/src/lower/stmt.rs` | 小 |
| 新建 `karte-gir` crate | `karte-gir/src/lib.rs`, `lower.rs`, `ir.rs` | 大 |
| 新建 `karte-gpu` crate (PTX 后端) | `karte-gpu/src/ptx.rs`, `lib.rs` | 大 |
| GPU 运行时绑定 (CUDA Driver FFI) | `karte-gpu-runtime/src/ffi.rs` | 中 |
| CLI `gpu` 子命令 | `karte-cli/src/main.rs`, `runner.rs` | 小 |
| `launch!` 语法和代码生成 | `karte-parser/src/expression.rs` | 中 |
| host/kernel 函数分离逻辑 | `karte-codegen/src/vm/professional_executor/` | 中 |

**验收标准**：
- `karte run vec_add.karte --gpu` 正确执行并返回预期结果
- `karte gpu-compile vec_add.karte --emit-ptx` 生成可读的 PTX 文本

### 阶段 2：Tile 抽象（预计 6 周）

**目标**：用 tile 语法实现 GEMM，达到 cuBLAS 60%+ 性能

| 任务 | 说明 | 工作量 |
|------|------|--------|
| Tile 类型系统 (`Type::Tile`) | `karte-hir/src/types.rs` + 8 个 match 臂 | 中 |
| tile 内建函数语义 | `tile_load/store/zeros/matmul` 的类型推断 | 大 |
| GIR tile 展开 Pass | tile 操作 → 线程级加载/计算/存储 | 大 |
| 共享内存自动分配 | 分析 tile 使用，计算共享内存需求 | 大 |
| Bank conflict 消除 | shared memory padding 策略 | 中 |
| Tensor Core MMA 指令生成 | PTX `mma` 指令选择 | 大 |
| `shared<T>(n)` 语法 | Parser + TypeChecker + MIR lowering | 中 |
| `sync_threads()` 语义 | GIR Barrier → PTX bar.sync | 小 |

**验收标准**：
- 512×512 矩阵乘法在 A100 上正确执行
- 性能不低于 naive 实现的 10 倍

### 阶段 3：完善与优化（预计 8 周）

| 任务 | 说明 |
|------|------|
| **自动 grid/block 配置** | `launch_auto!` 根据 tensor 大小自动计算 |
| **双缓冲流水线** | tile 加载与计算重叠 |
| **循环展开** | K 维度迭代自动展开 |
| **寄存器分配优化** | GIR 级寄存器分配器 |
| **CPU 回退执行** | kernel 在 CPU 上单线程模拟运行 |
| **Warp 级原语** | `warp_reduce`, `warp_shuffle` |
| **多后端** | SPIR-V (AMD/Intel) 后端 |
| **自动调优** | 搜索 tile_size × 流水线深度的最优组合 |
| **LSP 支持** | kernel 函数的补全、类型检查 |

### 阶段 4：生态建设

- GPU 标准库（卷积、attention、layernorm 等常用算子）
- 与 PyTorch/ONNX 的互操作（通过 CUDA UVA 或共享内存）
- 性能 profiling 工具
- GPU 算子测试框架

### 里程碑总览

```
阶段 0 (2周)     阶段 1 (4周)      阶段 2 (6周)       阶段 3 (8周)
├── f32 类型    ├── kernel fn     ├── Tile 类型      ├── 自动调优
├── 浮点 codegen ├── PTX 后端     ├── tile_matmul    ├── 双缓冲
└── CPU 验证    ├── GPU runtime   ├── Tensor Core    ├── 多后端
               └── vec_add 运行   ├── 共享内存       └── CPU 回退
                                 └── GEMM 运行
```

---

## 11. 逐文件改动清单

### 11.1 新建 Crate

| Crate | 路径 | 职责 |
|-------|------|------|
| `karte-gir` | `karte-gir/` | GPU IR 定义、LIR→GIR 降级、GIR 优化 |
| `karte-gpu` | `karte-gpu/` | GPU 后端代码生成（PTX/SPIR-V） |
| `karte-gpu-runtime` | `karte-gpu-runtime/` | CUDA/ROCm Driver API FFI 绑定 |

### 11.2 修改文件清单

#### Lexer

| 文件 | 改动 |
|------|------|
| `karte-lexer/src/lib.rs` | 新增 `Kernel`、`Shared`、`Tile` 关键字 token；识别 `f32`/`f16`/`f64`/`bf16` 类型标注 |

#### Parser

| 文件 | 改动 |
|------|------|
| `karte-parser/src/statement.rs:152` | `parse_statement()` 新增 `"kernel"` 分支 → `parse_kernel_def()` |
| `karte-parser/src/expression.rs` | 新增 `launch!` 宏解析；`shared<T>(n)` 表达式解析 |
| `karte-parser/src/types.rs` | KEYWORDS 列表新增关键字；浮点类型解析 |
| `karte-parser/src/lib.rs:241` | `ParseResult` 新增 `kernel_functions: HashSet<String>` |

#### HIR

| 文件 | 改动 |
|------|------|
| `karte-hir/src/types.rs:113` | `Type` enum 新增 `Tensor`/`Tile`/`SharedMem` 变体 |
| `karte-hir/src/types.rs` | 新增 `ScalarKind` 枚举（替代/扩展 `IntKind`） |
| `karte-hir/src/types.rs` | 8 个方法补充 match 臂: `structural_eq`, `Display`, `byte_size`, `substitute`, `free_vars`, `contains_var`, `is_bool`, `is_numeric` |
| `karte-hir/src/ast.rs:417` | `Statement` 新增 `KernelDef` 变体 |
| `karte-hir/src/ast.rs:60` | `Expr` 新增 `KernelLaunch`、`SharedAlloc`、`TileLoad`、`TileStore` 变体 |
| `karte-hir/src/type_checker.rs` | `infer_stmt` 处理 `KernelDef`；新增 tile/tensor 类型推断规则 |

#### MIR

| 文件 | 改动 |
|------|------|
| `karte-mir/src/ir.rs` | `MirFunction` 新增 `is_kernel: bool` 标记 |
| `karte-mir/src/lower/stmt.rs` | `Statement::KernelDef` → `MirFunction` lowering |
| `karte-mir/src/lower/expr.rs` | `Expr::KernelLaunch`/`SharedAlloc`/`TileLoad` → MIR 操作 |

#### LIR

| 文件 | 改动 |
|------|------|
| `karte-lir/src/ir.rs:149` | `Instruction` 新增 `GpuLaunch`、`GpuSync`、`GpuAlloc`、`GpuFree`、`GpuMemcpy` |
| `karte-lir/src/lower/` | MIR GPU 操作 → LIR GPU 指令降级 |
| `karte-lir/src/pass/` | GPU launch 指令的寄存器分配处理 |

#### Codegen (新建 GPU 路径)

| 文件 | 改动 |
|------|------|
| `karte-codegen/src/vm/professional_executor/execution_engine.rs` | 新增 `execute_with_gpu()` 方法 |
| `karte-codegen/src/vm/professional_executor/execution_engine.rs` | `compile_and_execute_with_jit` 增加 GPU 分支 |

#### CLI

| 文件 | 改动 |
|------|------|
| `karte-cli/src/main.rs` | 新增 `Gpu`/`GpuCompile` 子命令；`--gpu` flag |
| `karte-cli/src/runner.rs` | 新增 `gpu_run()` / `gpu_compile()` 函数 |

#### Module System

| 文件 | 改动 |
|------|------|
| `karte-module-system/src/interface.rs` | `ModuleExports` 新增 kernel 导出 |
| `karte-module-system/src/project.rs` | 编译流程支持 kernel 函数的 GIR 编译 |

#### Cargo Workspace

| 文件 | 改动 |
|------|------|
| `Cargo.toml` (根) | 新增 `karte-gir`、`karte-gpu`、`karte-gpu-runtime` 成员 |
| `karte-cli/Cargo.toml` | 添加 GPU crate 依赖 |
| `karte-codegen/Cargo.toml` | 添加 `karte-gir`、`karte-gpu` 依赖 |

---

## 12. 风险分析与缓解

### 12.1 技术风险

| 风险 | 严重度 | 缓解策略 |
|------|--------|---------|
| **GIR 设计过于复杂** | 高 | 分层设计：先支持标量 kernel (vec_add)，再逐步加入 tile |
| **PTX 指令集庞大** | 高 | 第一阶段只支持 .f32 标量指令子集；MMA/Tensor Core 推迟到阶段 2 |
| **CUDA Driver API 绑定复杂** | 中 | 使用 `ruscuda` 或手动 FFI；初期只绑定 8 个核心 API |
| **调试困难** | 高 | 实现 CPU 回退模式；GIR 文本输出便于调试 |
| **性能不达标** | 中 | 分阶段目标：先正确，再优化；阶段 2 以 Triton 为 benchmark |
| **浮点类型改动波及全栈** | 中 | ScalarKind 底层仍为 i64 存储，仅在 codegen 层区分；LIR 指令不变 |

### 12.2 架构兼容性

| 关注点 | 分析 |
|--------|------|
| **现有测试是否受影响** | 不受影响。GPU 功能通过 `kernel fn` 和 `--gpu` flag 激活，不使用 GPU 的代码路径完全不变 |
| **AOT 模式兼容性** | GPU AOT 需要将 PTX 编译结果嵌入 ELF 数据段，host 端通过 `cuModuleLoadData` 加载 |
| **逃逸分析兼容性** | GPU kernel 的内存模型独立于 CPU GC，逃逸分析不需要修改 |
| **模块系统兼容性** | kernel 函数作为特殊的导出符号，接口格式需扩展但不影响现有模块 |

### 12.3 硬件依赖

- **开发环境**：需要有 NVIDIA GPU 的机器
- **CI 环境**：CPU 回退模式可在无 GPU 的 CI 上测试 kernel 逻辑正确性
- **目标硬件**：第一阶段 SM 7.5+ (V100/T4/A100/H100)，后续扩展到 AMD CDNA

---

## 附录 A：完整 GEMM Kernel 示例

```karte
// 标准 GEMM kernel — 展示 tile 抽象的完整用法

kernel fn matmul_kernel(
    a: &Tensor<f16, 2>,
    b: &Tensor<f16, 2>,
    c: &mut Tensor<f32, 2>,
    m: number, k: number, n: number,
) {
    // Tile 配置
    let bm: const = 128;
    let bn: const = 128;
    let bk: const = 32;

    // 获取当前 tile 坐标
    let (pid_m, pid_n) = block_id_2d();
    let row_offset = pid_m * bm;
    let col_offset = pid_n * bn;

    // 累加器
    let acc = tile_zeros::<f32>(bm, bn);

    // K 维度循环
    for kk in 0..(k / bk) {
        // 从全局内存加载 tile
        let a_tile = tile_load(a, row_offset, kk * bk, bm, bk);
        let b_tile = tile_load(b, kk * bk, col_offset, bk, bn);

        // tile 矩阵乘加 (自动映射到 Tensor Core)
        acc = tile_matmul(acc, a_tile, b_tile);
    }

    // 写回结果
    tile_store(c, row_offset, col_offset, acc);
}

fn main() -> number {
    let m = 512;
    let k = 512;
    let n = 512;

    // 分配 GPU 张量并初始化
    let a = tensor_full::<f16>(m, k, 1.0);
    let b = tensor_full::<f16>(k, n, 1.0);
    let mut c = tensor_zeros::<f32>(m, n);

    // 启动 kernel
    let grid = (m / 128, n / 128);
    let block = (256, 1, 1);
    launch!(matmul_kernel, grid, block)(&a, &b, &mut c, m, k, n);

    // 验证结果
    gpu_sync();
    let result = tensor_read(&c, 0, 0);  // c[0][0] 应该 = 512.0
    result as number
}
```

## 附录 B：与现有 JitCompiler trait 的关系

```
现有 CPU 路径:
  LirProgram → JitCompiler trait → X86Compiler / AArch64Compiler → 机器码

新增 GPU 路径:
  LirProgram (kernel 函数)
    → lower_lir_to_gir() → GirProgram
    → GpuBackend trait → PtxBackend → PTX 文本
    → CUDA Driver → GPU 机器码

两条路径在 LirProgram 层分叉:
  - host 函数 → CPU JitCompiler
  - kernel 函数 → GPU GIR → PTX
```

GIR 后端 **不需要** 实现 `JitCompiler` trait，因为 GPU 的执行模型（SIMT、显存管理、kernel launch）与 CPU JIT 有本质区别。GIR 有自己独立的编译器 trait（`GpuBackend`）。

## 附录 C：Karte 标准库 GPU 模块规划

```
std/
└── gpu/
    ├── mod.karte          // 模块声明
    ├── tensor.karte       // Tensor host 端操作 (alloc/free/copy)
    ├── kernel.karte       // kernel 启动辅助函数
    ├── tile.karte         // tile 操作的高层封装
    ├── reduce.karte       // 归约 kernel 模板
    ├── elementwise.karte  // element-wise 操作模板
    └── blas.karte         // GEMM/GEMV/批量 GEMM
```
