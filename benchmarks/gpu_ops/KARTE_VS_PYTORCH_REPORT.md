# Karte GPU vs PyTorch 基准测试报告

> 日期: 2026-06-29 | GPU: NVIDIA GeForce RTX 5080 (SM 12.0, Blackwell)
> 测试: Karte PtxCompiler 生成的 PTX kernel 通过 CUDA Driver API 在 5080 上实际执行

---

## 测试方法

1. 用 Karte 的 GIR（GPU IR）构造 kernel 定义
2. 通过 Karte 的 **PtxCompiler** 编译为 NVIDIA PTX 汇编（`.target sm_12_0`）
3. 通过 Python ctypes 调用 CUDA Driver API（`cuModuleLoadData` + `cuLaunchKernel`）在 5080 上执行
4. 与 PyTorch 原生实现对比正确性和性能

### PTX 指令选择来自 Karte

Karte 生成的 PTX 核心指令：
- `mov.u32 %r0, %ctaid.x` / `%tid.x` — 线程索引（来自 GIR `BlockId`/`ThreadId`）
- `cvt.u64.u32 %rd5, %r4` — 寄存器宽度转换（来自 GIR `Mul I64`）
- `ld.global.f32 %f0, [%rd6]` — 全局内存加载（来自 GIR `GlobalLoad F32`）
- `add.f32 %f2, %f0, %f1` — 浮点加法（来自 GIR `Add F32`）
- `sub.f32 %f3, %f1, %f2` — 浮点减法（来自 GIR `Sub F32`）
- `mul.f32 %f4, %f3, %f3` — 浮点乘法（来自 GIR `Mul F32`）
- `st.global.f32 [%rd8], %f2` — 全局内存存储（来自 GIR `GlobalStore F32`）

---

## 测试结果

### Test 1: vec_add（element-wise 向量加法）

| 实现 | 耗时 | 加速比 |
|------|------|--------|
| PyTorch 原生 | 6.9 µs | 1.0x |
| **Karte GPU** | **7.1 µs** | **0.98x** |

正确性: max_diff = 0.00e+00 (**PASS**)

**分析**: vec_add 是最简单的 element-wise 操作，PyTorch 内部已经高度优化为单次 CUDA kernel launch。两者性能基本持平。

### Test 2: dof_reward（融合 diff + square + accumulate）

每个 thread 处理一个 env 的 29 个 DOF：
```
for i in 0..29:
    diff = ref[i] - dof[i]
    sum_sq += diff * diff
out[tid] = sum_sq
```

| 实现 | 耗时 | 加速比 |
|------|------|--------|
| PyTorch 原生 (`((rd-dd)**2).sum(dim=-1)`) | 14.2 µs | 1.0x |
| **Karte GPU** (单 kernel 融合) | **8.9 µs** | **1.59x** |

正确性: max_diff = 3.05e-05 (**PASS**)

**分析**: PyTorch 的 `((rd-dd)**2).sum(dim=-1)` 分解为 3 次独立的 CUDA kernel launch（sub → square → sum_reduce），中间产生 2 个临时张量。Karte 将整个计算融合为单次 kernel launch，零临时张量，全部在寄存器中完成。**1.59x 加速来自算子融合**。

### Test 3: 30× dof_reward（模拟训练中的批量奖励计算）

| 实现 | 耗时 | 加速比 |
|------|------|--------|
| PyTorch 30× | 302.3 µs | 1.0x |
| Karte 30× | 314.5 µs | 0.96x |

**分析**: 30 次连续调用中，Python 循环开销（每次约 10µs 的 Python 解释器时间）主导了总耗时，掩盖了 kernel 本身的性能差异。如果将 30 个奖励项进一步融合为单个 kernel（类似 Triton 的做法），可以消除这部分开销。

---

## 核心发现

| 场景 | Karte 表现 | 原因 |
|------|-----------|------|
| 简单 element-wise | ≈ PyTorch | PyTorch 已是优化过的单 kernel |
| **融合多步操作** | **1.59x 更快** | Karte 单 kernel vs PyTorch 多 kernel |
| 批量小 kernel | ≈ PyTorch | Python 循环开销主导 |

### Karte GPU 编译流水线验证

```
Karte GIR 定义
    ↓ PtxCompiler (karte-gpu/src/ptx.rs)
PTX 汇编 (.target sm_12_0)
    ↓ cuModuleLoadData (CUDA Driver API)
JIT 编译为 SASS (5080 原生机器码)
    ↓ cuLaunchKernel
GPU 执行
```

**全链路验证通过**：
- GIR → PTX 指令选择正确（`add.f32`, `ld.global.f32`, `mul.f32`）
- PTX 通过 CUDA Driver 在 SM 12.0 (Blackwell) 上 JIT 编译成功
- 计算结果与 PyTorch 数值一致
- 性能在融合计算场景下优于 PyTorch

---

## 文件清单

| 文件 | 说明 |
|------|------|
| `benchmarks/gpu_ops/bench_karte_vs_pytorch.py` | Karte vs PyTorch 基准测试（CUDA Driver API 桥接） |
| `karte-gpu/examples/gen_benchmark_ptx.rs` | Karte GIR → PTX 编译器示例 |
| `karte-gpu/src/ptx.rs` | PtxCompiler — GIR → PTX 代码生成器（f32 支持） |
| `karte-gir/src/ir.rs` | GIR 指令集定义（含 Tile/SharedMem/MMA 指令） |
