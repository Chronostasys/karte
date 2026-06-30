# Karte AMD GPU 支持技术方案

> 日期: 2026-06-30
> 状态: **已实现** (Phase 1: OpenCL C 源码生成)

## 一、现状分析（实现前）

### 1.1 当前 GPU 架构总览

Karte 已有一套完整的 GPU 算子开发流水线，**但仅支持 NVIDIA GPU (PTX + CUDA)**：

```
Python @karte.jit 函数
  → 符号执行 (_GirBuilder 构建 GIR JSON)
  → Rust karte binary (gpu-jit --target sm_80)
    → karte-gir::json::deserialize_program (JSON → GIR)
    → karte-gir 优化 (Vectorize / LoopUnroll / SoftwarePipeline / CSE / DCE)
    → karte-gpu::PtxCompiler (GIR → PTX 文本)
  → PTX 文本输出到 stdout
  → Python: cuModuleLoadData(PTX) + cuLaunchKernel
```

### 1.2 GPU 相关 Crate 一览

| Crate | 职责 | 行数 | AMD 兼容性 |
|-------|------|------|-----------|
| `karte-gir` | GPU 中间表示 (GIR) — 后端无关的指令集 | ~2600 | ✅ **已后端无关**，无需改动指令集 |
| `karte-gpu` | GPU 后端代码生成 | ~730 | ❌ 仅有 `PtxCompiler` (NVIDIA PTX) |
| `karte-gpu-runtime` | GPU 运行时 (FFI + 内存管理 + 启动器) | ~380 | ❌ 硬编码 CUDA Driver API |
| `karte-gpu-py` | Python JIT 绑定 (`@karte.jit`) | ~1425 | ❌ 硬编码 CUDA ctypes 调用 |
| `karte-cli` | CLI 入口 (`gpu-jit` 子命令) | ~60 (GPU 部分) | ❌ `--target sm_XX` 硬编码 NVIDIA |

### 1.3 后端无关性评估（关键发现）

**GIR 指令集已经完成后端抽象**，这是支持 AMD 的最大优势：

- 60+ 指令覆盖：标量算术、GPU 内存层次 (global/shared/v4 向量化)、同步通信 (barrier/warp shuffle)、Tensor Core (MMA)、Tile 操作、线程索引、数学函数
- 指令语义基于 SIMT 模型（线程、线程块、网格），不绑定任何厂商 ISA
- 唯一的厂商耦合：`GirDType::ptx_suffix()` 方法（返回 `"s32"`/`"f32"` 等 PTX 类型名）

**`GpuBackend` trait 已定义但仅 PTX 实现**：

```rust
pub trait GpuBackend {
    type Output;
    fn compile(&mut self, gir: &GirProgram) -> Self::Output;
    fn target_name(&self) -> &str;
}
```

**优化 Pass 全部后端无关**——在 GIR 层操作，代码生成之前执行，AMD 后端可直接复用。

### 1.4 需要改造的环节

| 环节 | 问题 | 改造难度 |
|------|------|---------|
| `GirDType` | `ptx_suffix()` 耦合 PTX 命名 | 低 — 抽象为 trait |
| `karte-gpu` | 仅 PTX 后端 | **高 — 需新建 SPIR-V/GCN 后端** |
| `karte-gpu-runtime` | 硬编码 CUDA FFI | 中 — 需抽象 Runtime trait + OpenCL/HIP 实现 |
| `karte-gpu-py` | ctypes 直接调 CUDA | 中 — 需后端检测 + 路由 |
| `karte-cli` | `--target sm_XX` | 低 — 增加 AMD target 选项 |

---

## 二、技术路线选型

### 2.1 方案对比

| | 方案 A: SPIR-V + OpenCL | 方案 B: GCN + HIP | 方案 C: HIP 源码 + RTC |
|---|---|---|---|
| **生成产物** | SPIR-V 二进制 | GCN 汇编文本 | HIP C++ 源码 |
| **运行时** | OpenCL (libOpenCL.so) | HIP (libamdhip64.so) | HIP RTC (libamdhip64.so) |
| **AMD 支持** | ✅ ROCm OpenCL 优秀 | ✅ 原生 ISA | ✅ 原生 |
| **NVIDIA 支持** | ✅ 通过 OpenCL | ❌ 仅 AMD | ⚠️ HIP-CUDA 互操作 |
| **Intel 支持** | ✅ 原生 | ❌ | ❌ |
| **与现有架构对齐** | 类似 PTX 文本→加载 | 最类似 PTX（直接 ISA） | 需引入 C++ 源码生成 |
| **ISA 稳定性** | ✅ SPIR-V 规范稳定 | ⚠️ GCN 版本碎片化 (GCN/RDNA/CDNA) | ✅ 编译器处理 |
| **Tensor Core 等价** | SPIR-V Cooperative Matrix | MFMA (CDNA) / WMMA (RDNA3) | HIP 内建函数 |
| **实现复杂度** | 中 — SPIR-V 二进制生成 | 高 — GCN 指令编码 | 中 — 但偏离 GIR 直接生成路线 |

### 2.2 推荐路线：双阶段策略

**Phase 1 — SPIR-V + OpenCL（跨厂商，快速落地）**

理由：
1. **最小改动 GIR**：SPIR-V 的 workgroup 模型与 GIR 的 SIMT 语义天然对应
2. **跨厂商**：一次实现同时支持 AMD / Intel / NVIDIA，最大化投资回报
3. **运行时成熟**：OpenCL 2.1+ 原生支持 SPIR-V 输入 (`clCreateProgramWithIL`)
4. **ROCm OpenCL 性能**：AMD ROCm 的 OpenCL 实现性能优异，接近原生
5. **稳定规范**：SPIR-V 是 Khronos 标准，不像 GCN 汇编随架构变化

**Phase 2 — GCN + HIP（AMD 原生，极致性能，可选）**

在 Phase 1 验证可行后，为追求极致 AMD 性能时添加：
- GIR → GCN 汇编（直接 ISA，对齐 PTX 路线）
- HIP 运行时（更低的启动开销，MFMA 原生支持）
- 两套后端共存，用户可选 `backend="opencl"` 或 `backend="hip"`

---

## 三、Phase 1 详细设计：SPIR-V + OpenCL

### 3.1 架构设计

```
                                    ┌─────────────────────┐
Python @karte.jit ──→ GIR JSON ──→ │  karte gpu-jit CLI   │
                                    │  --backend spirv     │
                                    └──────┬──────────────┘
                                           │
                              ┌────────────┴────────────┐
                              ▼                         ▼
                     ┌──────────────┐          ┌──────────────┐
                     │ karte-gir    │          │ karte-gir    │
                     │ (优化 pass)   │          │ (优化 pass)   │
                     └──────┬───────┘          └──────┬───────┘
                            │                         │
                     ┌──────▼───────┐          ┌──────▼───────┐
                     │ PtxCompiler  │          │ SpirvCompiler │ ← 新增
                     │ (NVIDIA)     │          │ (AMD/Intel)  │
                     └──────┬───────┘          └──────┬───────┘
                            │                         │
                        PTX 文本               SPIR-V 二进制
                            │                         │
                     ┌──────▼───────┐          ┌──────▼───────┐
                     │ CUDA Runtime │          │ OpenCL Runtime│ ← 新增
                     │ (libcuda.so) │          │(libOpenCL.so) │
                     └──────────────┘          └──────────────┘
```

### 3.2 `GirDType` 去耦合

**当前问题**：`GirDType` 硬编码 `ptx_suffix()` 方法。

**改造方案**：引入后端类型映射 trait，`GirDType` 只保留语义类型：

```rust
// karte-gir/src/ir.rs — 改造

/// 后端类型映射 trait — 每个后端实现自己的类型名
pub trait TypeMapper {
    fn map(&self, dtype: GirDType) -> &'static str;
}

// GirDType 移除 ptx_suffix()，保留语义方法
impl GirDType {
    pub fn size_in_bytes(&self) -> usize { /* 保持不变 */ }
    // 删除 ptx_suffix() — 移到各后端的 TypeMapper 实现
}

// karte-gpu/src/ptx.rs — PTX 后端的类型映射
pub struct PtxTypeMapper;
impl TypeMapper for PtxTypeMapper {
    fn map(&self, dtype: GirDType) -> &'static str {
        match dtype {
            GirDType::I32 => "s32",
            GirDType::I64 => "s64",
            GirDType::F16 => "f16",
            GirDType::F32 => "f32",
            GirDType::F64 => "f64",
        }
    }
}

// karte-gpu/src/spirv.rs — SPIR-V 后端的类型映射
pub struct SpirvTypeMapper;
impl TypeMapper for SpirvTypeMapper {
    fn map(&self, dtype: GirDType) -> &'static str {
        match dtype {
            GirDType::I32 => "i32",
            GirDType::I64 => "i64",
            GirDType::F16 => "f16",
            GirDType::F32 => "f32",
            GirDType::F64 => "f64",
        }
    }
}
```

### 3.3 SPIR-V 代码生成后端 (`karte-gpu/src/spirv.rs`)

#### 3.3.1 设计

SPIR-V 是二进制格式（32-bit word 序列），不像 PTX 是文本。`SpirvCompiler` 需要构建完整的 SPIR-V 模块：

```
SPIR-V 模块结构:
  Header (magic + version + generator + bound + schema)
  Capabilities (Kernel, Addresses, Float16/64, etc.)
  Extensions (SPV_KHR_workgroup_memory_scope)
  Memory Model (OpenCL)
  Entry Points (kernel functions)
  Types (void, bool, int, float, vector, pointer, array, struct)
  Constants
  Global Variables (kernel params, workgroup vars)
  Functions (CFG: blocks + instructions)
```

#### 3.3.2 核心映射关系

| GIR 概念 | SPIR-V 等价 |
|---------|-------------|
| Kernel (GirFunction) | `OpFunction` + `OpEntryPoint Kernel` |
| ThreadId X/Y/Z | `OpBuilt-in WorkgroupId` / `LocalInvocationId` / `GlobalInvocationId` |
| BlockDim | `OpBuilt-in WorkgroupSize` |
| GlobalLoad/Store | `OpLoad` / `OpStore` (CrossWorkgroup storage) |
| SharedLoad/Store | `OpLoad` / `OpStore` (Workgroup storage) |
| GlobalLoadV4 | `OpLoad` TypeVector 4×float |
| Barrier | `OpControlBarrier` (Workgroup scope) |
| WarpShuffle | SPIR-V subgroups (`OpGroupNonUniformShuffle`) |
| Cmp + BranchIf | `OpBranchConditional` |
| Fma | `OpExtInst` (GLSLstd450 Fma) 或 `OpFma` |
| Exp | `OpExtInst GLSLstd450 Exp` |
| Sqrt / Log / Sin / Cos / Tanh | `OpExtInst GLSLstd450` 系列 |
| Mma (Tensor Core) | SPV_KHR_cooperative_matrix (`OpCooperativeMatrixMulAdd`) |

#### 3.3.3 实现结构

```rust
// karte-gpu/src/spirv.rs

use karte_gir::*;

/// SPIR-V 编译器 — 将 GIR 编译为 SPIR-V 二进制
pub struct SpirvCompiler {
    /// SPIR-V word 缓冲
    words: Vec<u32>,
    /// 类型 ID 分配
    next_id: u32,
    /// 类型缓存: dtype → type_id
    type_cache: HashMap<GirDType, u32>,
    /// 寄存器 → SPIR-V ID 映射
    reg_map: HashMap<usize, u32>,
    /// GLSLstd450 扩展指令集 ID
    ext_inst_glsl_id: u32,
    /// 目标 SPIR-V 版本 (1.0 ~ 1.6)
    spirv_version: (u32, u32),
}

impl SpirvCompiler {
    pub fn new() -> Self { /* ... */ }

    /// 编译 GIR 程序为 SPIR-V 二进制
    pub fn compile(&mut self, gir: &GirProgram) -> Vec<u32> {
        self.emit_header();
        self.emit_capabilities();
        self.emit_memory_model();
        self.emit_extensions();
        for kernel in &gir.kernels {
            self.compile_kernel(kernel);
        }
        self.fixup_header_bound();
        self.words.clone()
    }

    fn emit_capabilities(&mut self) {
        // OpCapability Kernel
        // OpCapability Addresses (64-bit pointers)
        // OpCapability Float16 / Float64 (按需)
        // OpCapability CooperativeMatrixNV (Tensor Core, 需扩展)
    }

    fn emit_memory_model(&mut self) {
        // OpMemoryModel Physical64 OpenCL
    }

    fn compile_kernel(&mut self, func: &GirFunction) {
        // 1. 声明函数类型和参数类型
        // 2. OpEntryPoint Kernel
        // 3. OpFunction
        // 4. 参数变量 (OpFunctionParameter → OpVariable)
        // 5. 编译指令序列 (GirInstruction → SPIR-V Op)
        // 6. OpFunctionEnd
    }

    fn compile_instruction(&mut self, instr: &GirInstruction) {
        match instr {
            GirInstruction::Add { dst, src1, src2, dtype: GirDType::F32 } => {
                // OpFAdd %dst %src1 %src2
            }
            GirInstruction::GlobalLoad { dst, addr, dtype } => {
                // OpLoad %type %dst %addr (Aligned optional)
            }
            GirInstruction::Barrier => {
                // OpControlBarrier Workgroup Workgroup Workgroup
            }
            GirInstruction::ThreadId { dst, dim } => {
                // 通过 OpBuilt-in + OpLoad 获取
            }
            GirInstruction::Mma { .. } => {
                // OpCooperativeMatrixMulAdd (SPV_KHR_cooperative_matrix)
            }
            // ... 其余指令映射
        }
    }
}

impl GpuBackend for SpirvCompiler {
    type Output = Vec<u32>;
    fn compile(&mut self, gir: &GirProgram) -> Self::Output { self.compile(gir) }
    fn target_name(&self) -> &str { "spirv" }
}
```

#### 3.3.4 SPIR-V 输出格式

SPIR-V 编译器输出 `Vec<u32>`（二进制字序列）。CLI 输出时有两种选择：
- **二进制模式**：直接写 raw bytes 到 stdout（`--output-format binary`）
- **文本模式**：输出 SPIR-V 文本汇编（`--output-format text`，便于调试）

### 3.4 OpenCL 运行时 (`karte-gpu-runtime/src/opencl/`)

#### 3.4.1 FFI 绑定

```rust
// karte-gpu-runtime/src/opencl/ffi.rs

/// 动态加载 libOpenCL.so
pub fn try_init_opencl() -> Option<OpenClLib> {
    // dlopen("libOpenCL.so.1") 或 dlopen("libOpenCL.so")
    // ROCm 环境通常在 /opt/rom/opencl/lib/
}

// 核心 OpenCL API 子集:
// clGetPlatformIDs / clGetDeviceIDs     — 设备枚举
// clCreateContext / clCreateCommandQueue — 上下文/队列
// clCreateProgramWithIL                  — 加载 SPIR-V
// clBuildProgram / clCreateKernel        — 编译/创建 kernel
// clCreateBuffer                         — GPU 内存分配
// clEnqueueWriteBuffer / clEnqueueReadBuffer — H2D / D2H
// clEnqueueNDRangeKernel                 — 启动 kernel
// clFinish                              — 同步
```

#### 3.4.2 运行时抽象

引入 `GpuRuntime` trait 统一 CUDA 和 OpenCL：

```rust
// karte-gpu-runtime/src/lib.rs — 新增

/// GPU 运行时抽象 — 不管后端的统一接口
pub trait GpuRuntime: Send + Sync {
    /// 初始化运行时
    fn init(&self) -> Result<(), String>;
    /// 检测是否可用
    fn is_available(&self) -> bool;
    /// 后端名称 ("cuda" / "opencl")
    fn backend_name(&self) -> &str;
    /// 分配 GPU 内存
    fn alloc(&self, nbytes: usize) -> Result<u64, String>;
    /// 释放 GPU 内存
    fn free(&self, ptr: u64) -> Result<(), String>;
    /// Host → Device 拷贝
    fn h2d(&self, dst: u64, src: *const u8, nbytes: usize) -> Result<(), String>;
    /// Device → Host 拷贝
    fn d2h(&self, dst: *mut u8, src: u64, nbytes: usize) -> Result<(), String>;
    /// 加载内核模块（PTX 文本 或 SPIR-V 二进制）
    fn load_module(&self, data: &[u8]) -> Result<ModuleHandle, String>;
    /// 启动 kernel
    fn launch(&self, kernel: &ModuleHandle, name: &str,
              config: LaunchConfig, args: &[u64]) -> Result<(), String>;
    /// 同步
    fn synchronize(&self) -> Result<(), String>;
}

pub type ModuleHandle = *mut std::ffi::c_void;
```

#### 3.4.3 OpenCL 实现

```rust
// karte-gpu-runtime/src/opencl/runtime.rs

pub struct OpenClRuntime {
    platform: cl_platform_id,
    device: cl_device_id,
    context: cl_context,
    queue: cl_command_queue,
}

impl GpuRuntime for OpenClRuntime {
    fn backend_name(&self) -> &str { "opencl" }

    fn alloc(&self, nbytes: usize) -> Result<u64, String> {
        // clCreateBuffer(CL_MEM_READ_WRITE, nbytes)
        // 返回 cl_mem 作为 u64
    }

    fn load_module(&self, data: &[u8]) -> Result<ModuleHandle, String> {
        // clCreateProgramWithIL(context, data, len) — SPIR-V 输入
        // clBuildProgram(program, device, options)
        // 返回 cl_program
    }

    fn launch(&self, program: &ModuleHandle, name: &str,
              config: LaunchConfig, args: &[u64]) -> Result<(), String> {
        // clCreateKernel(program, name)
        // clSetKernelArg for each arg
        // clEnqueueNDRangeKernel(queue, kernel, work_dim,
        //     global_work_offset, global_work_size, local_work_size)
        //   - global_work_size = grid × block
        //   - local_work_size = block
    }
    // ...
}
```

#### 3.4.4 CUDA 运行时迁移

将现有 CUDA FFI 重构为 `GpuRuntime` 实现：

```rust
// karte-gpu-runtime/src/cuda/runtime.rs (重构现有 ffi.rs + launcher.rs)

pub struct CudaRuntime { /* cuContext etc. */ }

impl GpuRuntime for CudaRuntime {
    fn backend_name(&self) -> &str { "cuda" }
    fn alloc(&self, nbytes: usize) -> Result<u64, String> {
        // cuMemAlloc_v2
    }
    fn load_module(&self, data: &[u8]) -> Result<ModuleHandle, String> {
        // cuModuleLoadData — data 是 PTX 文本
    }
    fn launch(&self, module: &ModuleHandle, name: &str,
              config: LaunchConfig, args: &[u64]) -> Result<(), String> {
        // cuModuleGetFunction + cuLaunchKernel
    }
}
```

### 3.5 CLI 改造

```rust
// karte-cli/src/main.rs — GpuJit 命令改造

GpuJit {
    /// 后端: nvidia (PTX) / amd (SPIR-V) / auto
    #[arg(long, default_value = "auto")]
    backend: String,

    /// 目标 (NVIDIA: sm_80; AMD: gfx906/gfx1030/gfx1100; auto: 自动检测)
    #[arg(long)]
    target: Option<String>,

    /// 输出格式: text (PTX/汇编) / binary (SPIR-V raw)
    #[arg(long, default_value = "text")]
    output_format: String,
}

// 分发逻辑
match backend.as_str() {
    "nvidia" | "cuda" => {
        let ptx = PtxCompiler::new().target(..).compile(&gir);
        print!("{}", ptx); // 文本输出
    }
    "amd" | "spirv" => {
        let spirv = SpirvCompiler::new().compile(&gir);
        if output_format == "binary" {
            std::io::stdout().write_all(bytemuck::cast_slice(&spirv));
        } else {
            // SPIR-V 文本汇编（调试用）
            print_spirv_text(&spirv);
        }
    }
    "auto" => {
        // 检测可用 GPU，路由到对应后端
    }
}
```

### 3.6 Python JIT 改造

```python
# karte-gpu-py/karte_gpu/karte_jit.py — 后端检测与路由

# 新增: 后端检测
def _detect_backend():
    """自动检测可用 GPU 后端"""
    # 1. 尝试 CUDA (NVIDIA)
    if ctypes.util.find_library('cuda'):
        torch.cuda.device_count()  # 确认可用
        return 'cuda'
    # 2. 尝试 OpenCL (AMD / Intel)
    if ctypes.util.find_library('OpenCL'):
        return 'opencl'
    # 3. 回退 CPU
    return 'cpu'

_backend = _detect_backend()

# _compile 改造
def _compile(fn, call_args, block_size=256):
    # ... 生成 GIR JSON (不变) ...

    if _backend == 'cuda':
        sm_target = f'sm_{_get_sm_version_str()}'
        result = subprocess.run(
            [karte_bin, 'gpu-jit', '--backend', 'nvidia',
             '--target', sm_target, '--block-size', str(block_size)],
            input=json.dumps(gir_json), capture_output=True, text=True
        )
        ptx_text = result.stdout
        _load_and_launch_cuda(ptx_text, ...)

    elif _backend == 'opencl':
        result = subprocess.run(
            [karte_bin, 'gpu-jit', '--backend', 'amd',
             '--output-format', 'binary', '--block-size', str(block_size)],
            input=json.dumps(gir_json), capture_output=True  # binary output
        )
        spirv_binary = result.stdout  # raw SPIR-V bytes
        _load_and_launch_opencl(spirv_binary, ...)

# 新增: OpenCL ctypes 绑定
def _ensure_opencl():
    """初始化 OpenCL 运行时"""
    cl = ctypes.CDLL(ctypes.util.find_library('OpenCL'))
    # clGetPlatformIDs / clGetDeviceIDs (CL_DEVICE_TYPE_GPU)
    # clCreateContext / clCreateCommandQueue
    # clCreateProgramWithIL (SPIR-V)
    # clBuildProgram / clCreateKernel
    # clSetKernelArg / clEnqueueNDRangeKernel
    return cl
```

### 3.7 用户侧 API（保持不变）

```python
# 用户代码完全不变 — 自动检测后端
import karte

@karte.jit
def body_rot_reward(body_rot: karte.Tensor["N", 14, 4],
                    ref_rot:  karte.Tensor["N", 14, 4],
                    sigma: float = 0.25) -> karte.Tensor["N"]:
    tid = karte.thread_id()
    total = karte.f32(0.0)
    for j in karte.unroll(14):
        b = body_rot[tid, j]
        r = ref_rot[tid, j]
        total += 8.0 * (1.0 - karte.dot(b, r))
    return karte.exp(-sigma * total / 14.0)

# 在 NVIDIA GPU 上 → 自动用 PTX/CUDA
# 在 AMD GPU 上 → 自动用 SPIR-V/OpenCL
reward = body_rot_reward(body_tensor, ref_tensor, 0.25)
```

---

## 四、GIR → SPIR-V 指令映射详表

### 4.1 标量算术

| GIR 指令 | SPIR-V Op | 备注 |
|---------|-----------|------|
| `Move` | `OpCopyObject` | |
| `Add` (F32) | `OpFAdd` | |
| `Add` (I32/I64) | `OpIAdd` | |
| `Sub` | `OpFSub` / `OpISub` | |
| `Mul` | `OpFMul` / `OpIMul` | |
| `Div` (float) | `OpFDiv` | |
| `Div` (int) | `OpSDiv` / `OpUDiv` | 有符号/无符号 |
| `Mod` | `OpSRem` / `OpUMod` | |
| `Fma` | `OpFma` | SPIR-V 原生支持 |
| `Exp` | `OpExtInst GLSLstd450 Exp` | |
| `Recip` | `OpFDiv` with 1.0 或 `OpExtInst GLSLstd450 Recip` | |

### 4.2 数学函数

| GIR 指令 | SPIR-V Op |
|---------|-----------|
| `Sqrt` | `OpExtInst GLSLstd450 Sqrt` |
| `Log` | `OpExtInst GLSLstd450 Log` |
| `Rsqrt` | `OpExtInst GLSLstd450 InverseSqrt` |
| `Abs` | `OpExtInst GLSLstd450 FAbs` |
| `Max` / `Min` | `OpExtInst GLSLstd450 FMax/FMin` 或 `OpSMax/SMin` |
| `Tanh` | `OpExtInst GLSLstd450 Tanh` |
| `Sin` / `Cos` | `OpExtInst GLSLstd450 Sin/Cos` |
| `Clamp` | `OpExtInst GLSLstd450 FClamp/SClamp` |
| `Lerp` | `OpExtInst GLSLstd450 FMix` (float) |
| `Ceil` / `Floor` | `OpExtInst GLSLstd450 Ceil/Floor` |
| `Pow` | `OpExtInst GLSLstd450 Pow` |

### 4.3 GPU 内存

| GIR 指令 | SPIR-V Op | Storage Class |
|---------|-----------|---------------|
| `GlobalLoad` | `OpLoad` | CrossWorkgroup |
| `GlobalStore` | `OpStore` | CrossWorkgroup |
| `GlobalLoadV4` | `OpLoad` (TypeVector 4×float) | CrossWorkgroup |
| `GlobalStoreV4` | `OpStore` (TypeVector 4×float) | CrossWorkgroup |
| `GlobalLoadV2` | `OpLoad` (TypeVector 2×float) | CrossWorkgroup |
| `GlobalStoreV2` | `OpStore` (TypeVector 2×float) | CrossWorkgroup |
| `SharedLoad` | `OpLoad` | Workgroup |
| `SharedStore` | `OpStore` | Workgroup |
| `MaskedGlobalLoad` | `OpSelect + OpLoad` | 条件加载 |

### 4.4 同步与通信

| GIR 指令 | SPIR-V Op | 备注 |
|---------|-----------|------|
| `Barrier` | `OpControlBarrier` | Workgroup scope, Mem+Exec semantic |
| `WarpShuffle` | `OpGroupNonUniformShuffle` (Subgroup) | SPV_KHR_subgroup_vote/shuffle |
| `Reduce` | `OpGroupNonUniformIAdd/FMax/FMin` | Subgroup reduce |

### 4.5 线程索引

| GIR 指令 | SPIR-V Built-in | 获取方式 |
|---------|-----------------|---------|
| `ThreadId X/Y/Z` | `LocalInvocationId` | `OpLoad` from built-in variable |
| `BlockId X/Y/Z` | `WorkgroupId` | `OpLoad` from built-in variable |
| `BlockDim X/Y/Z` | `WorkgroupSize` | `OpLoad` from built-in constant |
| `GridDim X/Y/Z` | `NumWorkgroups` | `OpLoad` from built-in variable |

### 4.6 Tensor Core

| GIR 指令 | SPIR-V Op | 备注 |
|---------|-----------|------|
| `Mma` | `OpCooperativeMatrixMulAdd` | SPV_KHR_cooperative_matrix |
| `TileMatmul` | tile_expansion 展开 → `OpCooperativeMatrixMulAdd` | 需先做 tile expansion |

### 4.7 控制流

| GIR 指令 | SPIR-V Op |
|---------|-----------|
| `Cmp` | `OpFOrdLessThan` / `OpIEqual` 等 |
| `BranchIf` | `OpBranchConditional` |
| `Jump` | `OpBranch` |
| `Label` | `OpLabel` (basic block start) |
| `Return` | `OpReturn` + `OpFunctionEnd` |
| `Where` | `OpSelect` |

---

## 五、实现计划与里程碑

### Phase 1: SPIR-V + OpenCL（跨厂商 AMD 支持）

| 里程碑 | 内容 | 估计工作量 |
|--------|------|-----------|
| **1.1 类型去耦合** | `GirDType` 移除 `ptx_suffix()`，引入 `TypeMapper` trait | 0.5 天 |
| **1.2 SPIR-V 后端骨架** | `SpirvCompiler` 基础结构：header、capabilities、memory model、类型声明、entry point | 2 天 |
| **1.3 SPIR-V 指令映射** | 全部 GIR 指令 → SPIR-V Op 映射（标量算术 + 内存 + 线程索引 + 控制流） | 3 天 |
| **1.4 SPIR-V 数学函数** | GLSLstd450 扩展指令集接入 | 1 天 |
| **1.5 SPIR-V Warp/同步** | Subgroup 扩展（shuffle, reduce, barrier） | 1.5 天 |
| **1.6 SPIR-V Tensor Core** | SPV_KHR_cooperative_matrix 扩展（MMA） | 1.5 天 |
| **1.7 OpenCL 运行时** | FFI 绑定 + `GpuRuntime` trait + `OpenClRuntime` 实现 | 2 天 |
| **1.8 CUDA 运行时重构** | 现有 CUDA FFI 迁移到 `GpuRuntime` trait | 1 天 |
| **1.9 CLI 改造** | `--backend` / `--target` / `--output-format` 参数 | 0.5 天 |
| **1.10 Python JIT 改造** | 后端检测 + OpenCL ctypes 绑定 + 路由逻辑 | 1.5 天 |
| **1.11 集成测试** | AMD GPU 上验证 body_rot / sigmoid / GEMV / MMA kernel | 2 天 |
| **总计** | | **~17 天** |

### Phase 2: GCN + HIP（可选，AMD 原生极致性能）

| 里程碑 | 内容 | 估计工作量 |
|--------|------|-----------|
| 2.1 GCN 指令编码研究 | GCN/RDNA/CDNA ISA 手册，确定 target 架构 | 2 天 |
| 2.2 GCN 后端 | `GcnCompiler` — GIR → GCN 汇编文本 | 5 天 |
| 2.3 MFMA 支持 | CDNA MFMA / RDNA3 WMMA 指令映射 | 2 天 |
| 2.4 HIP 运行时 | HIP FFI + `HipRuntime` implementing `GpuRuntime` | 3 天 |
| 2.5 Code Object 加载 | HSA code object V5 格式加载 | 2 天 |
| 2.6 集成测试 + 性能对比 | 对比 OpenCL vs HIP 性能 | 2 天 |

---

## 六、技术风险与对策

### 6.1 SPIR-V 二进制生成的复杂度

**风险**: SPIR-V 是二进制格式，不像 PTX 是可读文本，调试困难。

**对策**:
- 实现 SPIR-V 文本汇编输出模式（`--output-format text`），类似 `spirv-dis` 工具
- 用 `spirv-val`（SPIR-V Tools）做合法性验证
- 用 `spirv-cross` 反编译验证语义正确性
- 关键: SPIR-V 的 ID 分配和前向引用需要 careful 管理

### 6.2 OpenCL SPIR-V 兼容性

**风险**: 不同 OpenCL 版本和厂商对 SPIR-V 的支持程度不同。

**对策**:
- 检测 `CL_DEVICE_IL_VERSION` 确认 SPIR-V 支持
- 降级策略：无 SPIR-V 时回退到 OpenCL C 源码生成（Phase 1.5 可选增强）
- ROCm OpenCL 2.0+ 全面支持 SPIR-V，AMD 环境 reliable

### 6.3 Tensor Core 等价 (Cooperative Matrix)

**风险**: `SPV_KHR_cooperative_matrix` 扩展尚在实验阶段，不同硬件支持情况不同。

**对策**:
- Phase 1 先不实现 Tensor Core，用标量循环展开 + 向量化 (`GlobalLoadV4`) 替代
- NVIDIA MMA 有 PTX 专用路径；AMD 用 Wave32/Wave64 + MFMA
- Phase 2 的 GCN 后端可直接生成 MFMA 指令，绕过 SPIR-V 扩展不确定性

### 6.4 Warp 大小差异

**风险**: NVIDIA warp = 32 线程， AMD wavefront = 32 或 64 线程（取决于架构）。

**对策**:
- GIR 的 `WarpShuffle` / `Reduce` 在 SPIR-V 层用 Subgroup 抽象，自动适配
- `auto_config()` 中的 block_size 候选值需考虑 wavefront 大小
- GIR 新增 `WaveSize` 查询指令，让 kernel 代码自适应

---

## 七、文件变更清单

### 新增文件

| 文件 | 说明 |
|------|------|
| `karte-gpu/src/spirv.rs` | SPIR-V 代码生成后端 (~1500 行估计) |
| `karte-gpu-runtime/src/runtime.rs` | `GpuRuntime` trait 定义 |
| `karte-gpu-runtime/src/opencl/mod.rs` | OpenCL 运行时模块入口 |
| `karte-gpu-runtime/src/opencl/ffi.rs` | OpenCL FFI 绑定 |
| `karte-gpu-runtime/src/opencl/runtime.rs` | `OpenClRuntime` 实现 |
| `karte-gpu-runtime/src/cuda/mod.rs` | CUDA 运行时模块（重构） |
| `karte-gpu-runtime/src/cuda/runtime.rs` | `CudaRuntime` 实现 |
| `docs/agent/amd-gpu-support-design.md` | 本文档 |
| `karte-tests/src/gpu_spirv_tests.rs` | SPIR-V 后端测试 |

### 修改文件

| 文件 | 改动 |
|------|------|
| `karte-gir/src/ir.rs` | 移除 `ptx_suffix()`，新增 `TypeMapper` trait |
| `karte-gpu/src/lib.rs` | 导出 `SpirvCompiler`，新增 `spirv` 模块 |
| `karte-gpu/src/ptx.rs` | 使用 `PtxTypeMapper` 替代直接 `ptx_suffix()` |
| `karte-gpu-runtime/src/lib.rs` | 导出 `GpuRuntime` trait + 各后端模块 |
| `karte-gpu-runtime/src/ffi.rs` | 移至 `cuda/ffi.rs`（重构） |
| `karte-gpu-runtime/src/tensor.rs` | 基于 `GpuRuntime` trait 重构 |
| `karte-gpu-runtime/src/launcher.rs` | 基于 `GpuRuntime` trait 重构 |
| `karte-cli/src/main.rs` | `GpuJit` 增加 `--backend` 参数 |
| `karte-gpu-py/karte_gpu/karte_jit.py` | 后端检测 + OpenCL 绑定 + 路由 |
| `Cargo.toml` | `karte-gpu-runtime` 依赖 `libloading` (替代手动 dlopen) |

---

## 八、总结

Karte 的 GPU 架构有**极好的后端无关基础**——GIR 指令集完全抽象，`GpuBackend` trait 预留了多后端扩展点，优化 pass 在 GIR 层操作。支持 AMD GPU 的核心工作是：

1. **新增 SPIR-V 代码生成后端**（`SpirvCompiler`）—— GIR 到 SPIR-V 的完整指令映射
2. **新增 OpenCL 运行时**—— SPIR-V 二进制加载 + kernel 启动
3. **抽象运行时接口**—— `GpuRuntime` trait 统一 CUDA/OpenCL
4. **后端自动检测与路由**—— Python JIT 和 CLI 层透明切换

Phase 1 (SPIR-V + OpenCL) 预计 ~17 天，实现后 Karte 将同时支持 NVIDIA (PTX/CUDA) 和 AMD (SPIR-V/OpenCL) GPU，用户代码零改动。
