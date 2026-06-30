# karte-gpu

**用纯 Python 编写比 Triton 快 2x 的 GPU 算子**

```
pip install karte-gpu
```

## 为什么选择 karte-gpu

| | PyTorch 原生 | Triton | **karte-gpu** |
|---|---|---|---|
| 代码行数 | 16 行 | 24 行 | **7 行** |
| 性能 (body_rot_reward) | 74.3 µs | 28.8 µs | **12.6 µs** |
| vs PyTorch | 1.0x | 2.58x | **5.90x** |
| 指针运算 | 无 | `tl.load(ptr + offset)` 手动 | **`tensor[tid, j]` 自动** |
| 输出方式 | 返回值 | `tl.store(out_ptr, ...)` | **`return` 自动** |
| Grid 配置 | 自动 | `kernel[(grid,)]` 手动 | **自动** |

## 30 秒上手

```python
import torch
import karte_gpu as karte

# 写一个 kernel——和写普通 Python 函数一样
@karte.jit
def sigmoid_kernel(x: karte.Tensor["N"]) -> karte.Tensor["N"]:
    tid = karte.thread_id()
    val = x[tid]
    return 1.0 / (1.0 + karte.exp(0.0 - val))

# 调用——和调用普通函数一样
x = torch.randn(4096, device='cuda', dtype=torch.float32)
y = sigmoid_kernel(x)

print(y)          # tensor([0.4567, 0.7890, ...], device='cuda:0')
print(torch.allclose(y, torch.sigmoid(x), atol=1e-5))  # True
```

首次调用时自动编译（约 0.5 秒），后续调用命中缓存零开销。

## 前置条件

1. **NVIDIA GPU**（SM 7.5+，即 V100/T4/A100/H100/RTX 30/40/50 系列）
2. **PyTorch 2.0+**（`pip install torch`）
3. **Karte 编译器二进制**：
   ```bash
   # 从源码编译
   git clone <karte-repo>
   cd karte && cargo build --bin karte
   export KARTE_BIN=$(pwd)/target/debug/karte
   ```

## 完整 API

### 类型标注

| 标注 | 用途 |
|------|------|
| `karte.Tensor["N"]` | 1D 张量（向量），N 是动态大小 |
| `karte.Tensor["N", 14, 4]` | 3D 张量，后两维编译期已知 |

### 线程控制

| 函数 | 说明 |
|------|------|
| `karte.thread_id()` | 返回当前线程的全局 ID |
| `karte.unroll(N)` | 编译期展开的 for 循环 |

### 数学函数

| 函数 | 等价 PyTorch | PTX 指令 |
|------|-------------|---------|
| `karte.f32(val)` | `torch.tensor(val)` | 常量 |
| `karte.dot(a, b)` | 向量点积 | mul + add 链 |
| `karte.exp(x)` | `torch.exp(x)` | `ex2.approx.f32` |
| `karte.sqrt(x)` | `torch.sqrt(x)` | `sqrt.approx.f32` |
| `karte.rsqrt(x)` | `torch.rsqrt(x)` | `rsqrt.approx.f32` |
| `karte.log(x)` | `torch.log(x)` | `lg2.approx.f32` |
| `karte.tanh(x)` | `torch.tanh(x)` | sigmoid 近似 |
| `karte.sin(x)` | `torch.sin(x)` | 泰勒展开 |
| `karte.cos(x)` | `torch.cos(x)` | 泰勒展开 |
| `karte.clamp(x, lo, hi)` | `torch.clamp(x, lo, hi)` | max + min |
| `karte.lerp(a, b, t)` | 线性插值 | sub + mul + add |
| `karte.pow(base, exp)` | `torch.pow(base, exp)` | lg2 + mul + ex2 |

### 条件与归约

| 函数 | 等价 PyTorch | PTX 指令 |
|------|-------------|---------|
| `karte.where(cond, a, b)` | `torch.where(cond, a, b)` | `setp` + `selp` |
| `karte.max_val(a, b)` | `torch.max(a, b)` | `max.f32` |
| `karte.min_val(a, b)` | `torch.min(a, b)` | `min.f32` |
| `karte.abs_val(x)` | `torch.abs(x)` | `abs.f32` |
| `karte.reduce_sum(x)` | warp 级求和 | `shfl.sync` 树形归约 |
| `karte.reduce_max(x)` | warp 级最大值 | `shfl.sync` 树形归约 |

### Autograd 集成

```python
@karte.jit
def relu_forward(x: karte.Tensor["N"]) -> karte.Tensor["N"]:
    tid = karte.thread_id()
    return karte.max_val(x[tid], 0.0)

# 包装为支持反向传播
ReLU = karte.autograd(relu_forward)

# 在训练中使用
x = torch.randn(4096, device='cuda', requires_grad=True)
y = ReLU.apply(x)
loss = y.sum()
loss.backward()  # 自动传播梯度
```

### 环境变量

| 变量 | 默认 | 说明 |
|------|------|------|
| `KARTE_BIN` | 自动检测 | karte 编译器二进制路径 |
| `KARTE_AUTOTUNE` | `1` | 自动搜索最优 block_size |
| `KARTE_INTERPRET` | `0` | CPU 解释模式（用于调试） |

## 真实案例：优化 uni-tracker 训练

```python
# 原来（PyTorch 原生，~16 次 kernel launch）:
def _reward_im_body_rot(self):
    diff_quat = quat_inverse_multiply(body_rot, ref_body_rot)
    angle = quat_to_angle_axis(diff_quat)[0]
    error = torch.square(angle).mean(dim=-1)
    return torch.exp(-sigma * error)

# 现在（karte-gpu，1 次 kernel launch，快 5.9x）:
@karte.jit
def body_rot_reward(body_rot: karte.Tensor["N", 14, 4],
                    ref_rot: karte.Tensor["N", 14, 4],
                    sigma: float = 0.25) -> karte.Tensor["N"]:
    tid = karte.thread_id()
    total = karte.f32(0.0)
    for j in karte.unroll(14):
        b = body_rot[tid, j]
        r = ref_rot[tid, j]
        total = total + 8.0 * (1.0 - karte.dot(b, r))
    return karte.exp(0.0 - sigma * total / 14.0)
```

## 工作原理

```
Python @karte.jit 函数
    ↓ AST 解析类型标注 + 符号执行
GIR JSON (GPU 中间表示)
    ↓ Rust 编译器 (karte gpu-jit)
    ├─ VectorizePass — 标量 load → v4 向量化
    ├─ CSE — 公共子表达式消除
    ├─ DCE — 死代码消除
    ├─ SoftwarePipelinePass — 指令重排
    └─ PtxCompiler — PTX 汇编生成
    ↓
CUDA Driver JIT → GPU 执行
```

Python 端零 PTX 拼接，全部由 Rust 编译器生成。

## License

MIT
