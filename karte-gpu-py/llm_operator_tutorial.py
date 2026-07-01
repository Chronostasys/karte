#!/usr/bin/env python3
"""
LLM 优化算子部署实战教程
========================
本教程用手写 GPU kernel 实现 LLM 推理中的核心算子，并在 AMD GPU 上实机运行。

覆盖算子:
  1. GELU 激活函数 — Transformer MLP 中的非线性
  2. Softmax — Attention 权重归一化
  3. RMSNorm — LLaMA/GPT 归一化层
  4. GEMM — 矩阵乘法（LLM 推理 70%+ 计算量）
  5. Fused QK^T + Softmax — 算子融合示例

每个算子包含:
  - 原理讲解
  - GPU kernel 实现（GIR → SPIR-V → AMD GPU 执行）
  - 与 PyTorch 参考实现的结果对比
  - 优化技巧分析

运行方式:
  python3 llm_operator_tutorial.py           # 运行全部
  python3 llm_operator_tutorial.py 1          # 只跑第 1 课
  python3 llm_operator_tutorial.py 1 3        # 跑第 1, 3 课
"""

import json
import struct
import math
import subprocess
import ctypes
import ctypes.util
import sys

# ============================================================
# GPU 运行时基础设施 (OpenCL + SPIR-V)
# ============================================================

class GpuRunner:
    """AMD GPU 运行时 — 加载 SPIR-V kernel 并执行"""

    def __init__(self):
        self._init_opencl()

    def _init_opencl(self):
        """初始化 OpenCL 环境"""
        cl_lib_name = ctypes.util.find_library('OpenCL')
        if not cl_lib_name:
            raise RuntimeError("未找到 OpenCL 库")

        self.cl = ctypes.CDLL(cl_lib_name)

        # 设置函数签名
        self._setup_signatures()

        # 选择 Rusticl 平台
        num_plat = ctypes.c_uint(0)
        self.cl.clGetPlatformIDs(0, None, ctypes.byref(num_plat))
        platforms = (ctypes.c_void_p * num_plat.value)()
        self.cl.clGetPlatformIDs(num_plat.value, platforms, None)

        CL_PLATFORM_NAME = 0x0902
        CL_DEVICE_TYPE_GPU = 1 << 2
        CL_DEVICE_NAME = 0x1027

        self.device = None
        for p in platforms:
            sz = ctypes.c_size_t(0)
            self.cl.clGetPlatformInfo(p, CL_PLATFORM_NAME, 0, None, ctypes.byref(sz))
            nb = ctypes.create_string_buffer(sz.value)
            self.cl.clGetPlatformInfo(p, CL_PLATFORM_NAME, sz.value, nb, None)
            pname = nb.value.decode()
            if 'rusticl' in pname.lower():
                nd = ctypes.c_uint(0)
                self.cl.clGetDeviceIDs(p, CL_DEVICE_TYPE_GPU, 0, None, ctypes.byref(nd))
                if nd.value > 0:
                    devs = (ctypes.c_void_p * nd.value)()
                    self.cl.clGetDeviceIDs(p, CL_DEVICE_TYPE_GPU, nd.value, devs, None)
                    self.device = devs[0]
                    sz2 = ctypes.c_size_t(0)
                    self.cl.clGetDeviceInfo(self.device, CL_DEVICE_NAME, 0, None, ctypes.byref(sz2))
                    db = ctypes.create_string_buffer(sz2.value)
                    self.cl.clGetDeviceInfo(self.device, CL_DEVICE_NAME, sz2.value, db, None)
                    self.gpu_name = db.value.decode()
                    break

        if not self.device:
            raise RuntimeError("未找到 Rusticl GPU")

        err = ctypes.c_int(0)
        self.ctx = self.cl.clCreateContext(None, 1, ctypes.byref(ctypes.c_void_p(self.device)), None, None, ctypes.byref(err))
        self.queue = self.cl.clCreateCommandQueueWithProperties(self.ctx, self.device, None, ctypes.byref(err))

    def _setup_signatures(self):
        """设置 OpenCL C API 函数签名"""
        cl = self.cl
        cl.clGetPlatformIDs.argtypes = [ctypes.c_uint, ctypes.POINTER(ctypes.c_void_p), ctypes.POINTER(ctypes.c_uint)]
        cl.clGetPlatformIDs.restype = ctypes.c_int
        cl.clGetPlatformInfo.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_size_t, ctypes.c_void_p, ctypes.POINTER(ctypes.c_size_t)]
        cl.clGetPlatformInfo.restype = ctypes.c_int
        cl.clGetDeviceIDs.argtypes = [ctypes.c_void_p, ctypes.c_uint64, ctypes.c_uint, ctypes.POINTER(ctypes.c_void_p), ctypes.POINTER(ctypes.c_uint)]
        cl.clGetDeviceIDs.restype = ctypes.c_int
        cl.clGetDeviceInfo.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_size_t, ctypes.c_void_p, ctypes.POINTER(ctypes.c_size_t)]
        cl.clGetDeviceInfo.restype = ctypes.c_int
        cl.clCreateContext.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.POINTER(ctypes.c_void_p), ctypes.c_void_p, ctypes.c_void_p, ctypes.POINTER(ctypes.c_int)]
        cl.clCreateContext.restype = ctypes.c_void_p
        cl.clCreateCommandQueueWithProperties.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.POINTER(ctypes.c_int)]
        cl.clCreateCommandQueueWithProperties.restype = ctypes.c_void_p
        cl.clCreateBuffer.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_size_t, ctypes.c_void_p, ctypes.POINTER(ctypes.c_int)]
        cl.clCreateBuffer.restype = ctypes.c_void_p
        cl.clEnqueueWriteBuffer.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint, ctypes.c_size_t, ctypes.c_size_t, ctypes.c_void_p, ctypes.c_uint, ctypes.c_void_p, ctypes.c_void_p]
        cl.clEnqueueWriteBuffer.restype = ctypes.c_int
        cl.clEnqueueReadBuffer.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint, ctypes.c_size_t, ctypes.c_size_t, ctypes.c_void_p, ctypes.c_uint, ctypes.c_void_p, ctypes.c_void_p]
        cl.clEnqueueReadBuffer.restype = ctypes.c_int
        cl.clCreateProgramWithIL.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t, ctypes.POINTER(ctypes.c_int)]
        cl.clCreateProgramWithIL.restype = ctypes.c_void_p
        cl.clBuildProgram.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.POINTER(ctypes.c_void_p), ctypes.c_char_p, ctypes.c_void_p, ctypes.c_void_p]
        cl.clBuildProgram.restype = ctypes.c_int
        cl.clGetProgramBuildInfo.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint, ctypes.c_size_t, ctypes.c_void_p, ctypes.POINTER(ctypes.c_size_t)]
        cl.clGetProgramBuildInfo.restype = ctypes.c_int
        cl.clCreateKernel.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.POINTER(ctypes.c_int)]
        cl.clCreateKernel.restype = ctypes.c_void_p
        cl.clSetKernelArg.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_size_t, ctypes.c_void_p]
        cl.clSetKernelArg.restype = ctypes.c_int
        cl.clEnqueueNDRangeKernel.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint, ctypes.POINTER(ctypes.c_size_t), ctypes.POINTER(ctypes.c_size_t), ctypes.POINTER(ctypes.c_size_t), ctypes.c_uint, ctypes.c_void_p, ctypes.c_void_p]
        cl.clEnqueueNDRangeKernel.restype = ctypes.c_int
        cl.clFinish.argtypes = [ctypes.c_void_p]
        cl.clFinish.restype = ctypes.c_int

    def compile_spirv(self, gir_json, kernel_name):
        """将 GIR JSON 编译为 SPIR-V 二进制"""
        result = subprocess.run(
            ["./target/debug/karte", "gpu-jit", "--backend", "spirv"],
            input=json.dumps(gir_json).encode(),
            capture_output=True,
            timeout=30,
        )
        if result.returncode != 0:
            raise RuntimeError(f"SPIR-V 编译失败: {result.stderr.decode()}")
        return result.stdout

    def run_kernel(self, spirv_bytes, kernel_name, input_arrays, output_size):
        """
        在 AMD GPU 上执行 kernel
        input_arrays: list of (float32 数据, 字节数)
        output_size: 输出元素个数 (float32)
        返回: 输出数组
        """
        cl = self.cl
        err = ctypes.c_int(0)

        # 加载 SPIR-V
        spirv_buf = ctypes.create_string_buffer(spirv_bytes)
        program = cl.clCreateProgramWithIL(self.ctx, spirv_buf, len(spirv_bytes), ctypes.byref(err))
        if err.value != 0:
            raise RuntimeError(f"clCreateProgramWithIL failed: {err.value}")

        ret = cl.clBuildProgram(program, 1, ctypes.byref(ctypes.c_void_p(self.device)), None, None, None)
        if ret != 0:
            log_sz = ctypes.c_size_t(0)
            cl.clGetProgramBuildInfo(program, self.device, 0x1084, 0, None, ctypes.byref(log_sz))
            log_buf = ctypes.create_string_buffer(max(log_sz.value, 1))
            cl.clGetProgramBuildInfo(program, self.device, 0x1084, log_sz.value, log_buf, None)
            raise RuntimeError(f"clBuildProgram failed ({ret}):\n{log_buf.value.decode('utf-8', errors='replace')}")

        kernel = cl.clCreateKernel(program, kernel_name.encode(), ctypes.byref(err))
        if err.value != 0:
            raise RuntimeError(f"clCreateKernel failed: {err.value}")

        # 创建 buffer
        CL_MEM_READ_WRITE = 1
        CL_MEM_COPY_HOST_PTR = 1 << 5
        buffers = []
        for data_bytes, nbytes in input_arrays:
            host_buf = ctypes.create_string_buffer(data_bytes)
            buf = cl.clCreateBuffer(self.ctx, CL_MEM_READ_WRITE | CL_MEM_COPY_HOST_PTR,
                                    nbytes, ctypes.cast(host_buf, ctypes.c_void_p), ctypes.byref(err))
            buffers.append(buf)

        # 输出 buffer
        out_nbytes = output_size * 4
        out_buf = cl.clCreateBuffer(self.ctx, CL_MEM_READ_WRITE, out_nbytes, None, ctypes.byref(err))
        buffers.append(out_buf)

        # 设置参数
        for i, buf in enumerate(buffers):
            arg_val = ctypes.c_uint64(ctypes.cast(buf, ctypes.c_void_p).value)
            cl.clSetKernelArg(kernel, i, 8, ctypes.byref(arg_val))

        # 启动 kernel — global = local = N (单 workgroup)
        N = output_size
        global_ws = (ctypes.c_size_t * 3)(N, 1, 1)
        local_ws = (ctypes.c_size_t * 3)(N, 1, 1)

        ret = cl.clEnqueueNDRangeKernel(self.queue, kernel, 1, None, global_ws, local_ws, 0, None, None)
        if ret != 0:
            raise RuntimeError(f"clEnqueueNDRangeKernel failed: {ret}")

        cl.clFinish(self.queue)

        # 读回结果
        out_host = ctypes.create_string_buffer(out_nbytes)
        cl.clEnqueueReadBuffer(self.queue, out_buf, 1, 0, out_nbytes, out_host, 0, None, None)

        return list(struct.unpack(f'{output_size}f', out_host.raw))


def make_gir(kernel_name, instructions, params, next_reg=100, next_label=10):
    """构建 GIR JSON"""
    return {
        "kernels": [{
            "name": kernel_name,
            "params": params,
            "block_dim": [256, 1, 1],
            "next_reg": next_reg,
            "next_label": next_label,
            "instructions": instructions,
        }]
    }


def float_bits(val):
    """float32 → 整数位表示 (用于 Imm)"""
    return int(struct.unpack('I', struct.pack('f', float(val)))[0])


# ============================================================
# 第 1 课: GELU 激活函数 —— Transformer MLP 的非线性
# ============================================================

def lesson1_gelu(gpu):
    """
    GELU(x) = x * Φ(x) = x * 0.5 * (1 + erf(x/√2))

    在 Transformer 中，每个 MLP 层的中间投影后都接 GELU 激活。
    GELU 比 ReLU 更平滑，有助于梯度流动。

    近似公式 (常见 GPU 实现):
      GELU(x) ≈ 0.5 * x * (1 + tanh(√(2/π) * (x + 0.044715 * x³)))

    优化要点:
      - Element-wise 操作，每个线程独立计算一个元素
      - 无需 reduction，无需线程间通信
      - 用 tanh 近似避免精确 erf 计算（节省指令）
    """
    print("\n" + "=" * 60)
    print("第 1 课: GELU 激活函数")
    print("=" * 60)
    print("""
原理: GELU(x) = 0.5 * x * (1 + tanh(√(2/π) * (x + 0.044715 * x³)))
用途: Transformer MLP 层的非线性激活
优化: tanh 近似替代精确 erf，减少指令数

GPU 并行策略:
  - 每个线程独立计算一个元素 (element-wise)
  - 无需线程间通信，无需共享内存
  - 这是最简单的 GPU 算子类型
""")

    # GIR 指令: GELU 近似实现
    # x = input[tid]
    # x3 = x * x * x
    # inner = 0.7978845608 * (x + 0.044715 * x3)
    # result = 0.5 * x * (1 + tanh(inner))

    SQRT_2_OVER_PI = float_bits(0.7978845608028654)
    COEFF = float_bits(0.044715)
    HALF = float_bits(0.5)

    instructions = [
        # tid = thread_id() → reg 0
        {"op": "ThreadId", "dst": 0, "dim": "x"},

        # addr = tid * 4 + param[0]
        {"op": "Mul", "dst": 1, "src1": {"kind": "Reg", "id": 0}, "src2": {"kind": "Imm", "val": 4}, "dtype": "i64"},
        {"op": "Add", "dst": 1, "src1": {"kind": "Reg", "id": 1}, "src2": {"kind": "Param", "id": 0}, "dtype": "i64"},

        # x = input[tid]  → reg 2
        {"op": "GlobalLoad", "dst": 2, "addr": {"kind": "Reg", "id": 1}, "dtype": "f32"},

        # x3 = x * x * x → reg 3, 4
        {"op": "Mul", "dst": 3, "src1": {"kind": "Reg", "id": 2}, "src2": {"kind": "Reg", "id": 2}, "dtype": "f32"},
        {"op": "Mul", "dst": 4, "src1": {"kind": "Reg", "id": 3}, "src2": {"kind": "Reg", "id": 2}, "dtype": "f32"},

        # 0.044715 * x3 → reg 5
        {"op": "Mul", "dst": 5, "src1": {"kind": "Imm", "val": COEFF}, "src2": {"kind": "Reg", "id": 4}, "dtype": "f32"},

        # x + 0.044715 * x3 → reg 6
        {"op": "Add", "dst": 6, "src1": {"kind": "Reg", "id": 2}, "src2": {"kind": "Reg", "id": 5}, "dtype": "f32"},

        # √(2/π) * (x + 0.044715 * x3) → reg 7
        {"op": "Mul", "dst": 7, "src1": {"kind": "Imm", "val": SQRT_2_OVER_PI}, "src2": {"kind": "Reg", "id": 6}, "dtype": "f32"},

        # tanh(inner) → reg 8
        {"op": "Tanh", "dst": 8, "src": {"kind": "Reg", "id": 7}, "dtype": "f32"},

        # 1 + tanh(inner) → reg 9
        {"op": "Add", "dst": 9, "src1": {"kind": "Imm", "val": float_bits(1.0)}, "src2": {"kind": "Reg", "id": 8}, "dtype": "f32"},

        # 0.5 * x → reg 10
        {"op": "Mul", "dst": 10, "src1": {"kind": "Imm", "val": HALF}, "src2": {"kind": "Reg", "id": 2}, "dtype": "f32"},

        # result = 0.5 * x * (1 + tanh(...)) → reg 11
        {"op": "Mul", "dst": 11, "src1": {"kind": "Reg", "id": 10}, "src2": {"kind": "Reg", "id": 9}, "dtype": "f32"},

        # output[tid] = result
        {"op": "Mul", "dst": 12, "src1": {"kind": "Reg", "id": 0}, "src2": {"kind": "Imm", "val": 4}, "dtype": "i64"},
        {"op": "Add", "dst": 12, "src1": {"kind": "Reg", "id": 12}, "src2": {"kind": "Param", "id": 1}, "dtype": "i64"},
        {"op": "GlobalStore", "addr": {"kind": "Reg", "id": 12}, "src": {"kind": "Reg", "id": 11}, "dtype": "f32"},

        {"op": "Return"},
    ]

    gir = make_gir("gelu_kernel", instructions, [
        {"name": "x", "dtype": "f32", "is_ptr": True},
        {"name": "out", "dtype": "f32", "is_ptr": True},
    ])

    # 编译为 SPIR-V
    spirv = gpu.compile_spirv(gir, "gelu_kernel")
    print(f"  SPIR-V 编译: {len(spirv)} bytes ✅")

    # 测试数据
    test_vals = [-3.0, -2.0, -1.0, -0.5, 0.0, 0.5, 1.0, 2.0, 3.0, 5.0]
    N = len(test_vals)
    input_bytes = struct.pack(f'{N}f', *test_vals)

    # GPU 执行
    gpu_results = gpu.run_kernel(spirv, "gelu_kernel", [(input_bytes, N * 4)], N)

    # PyTorch 参考实现
    import torch
    x = torch.tensor(test_vals, dtype=torch.float32)
    # 精确 GELU
    gelu_exact = torch.nn.functional.gelu(x)
    # tanh 近似 GELU (与我们 GPU kernel 相同的公式)
    gelu_tanh = 0.5 * x * (1 + torch.tanh(math.sqrt(2/math.pi) * (x + 0.044715 * x**3)))

    print(f"\n  {'输入':>8s}  {'GPU(GIR)':>12s}  {'PyTorch':>12s}  {'误差':>10s}")
    print(f"  {'-'*8}  {'-'*12}  {'-'*12}  {'-'*10}")
    all_pass = True
    for i in range(N):
        err = abs(gpu_results[i] - gelu_tanh[i].item())
        status = "✅" if err < 1e-5 else "❌"
        if err >= 1e-5:
            all_pass = False
        print(f"  {test_vals[i]:8.2f}  {gpu_results[i]:12.6f}  {gelu_tanh[i].item():12.6f}  {err:10.2e} {status}")

    if all_pass:
        print("\n  ✅ GELU GPU kernel 验证通过！")
    return all_pass


# ============================================================
# 第 2 课: Softmax —— Attention 权重归一化
# ============================================================

def lesson2_softmax(gpu):
    """Softmax — Attention 权重归一化"""
    print("\n" + "=" * 60)
    print("第 2 课: Softmax")
    print("=" * 60)
    print("""
原理: Softmax(xi) = exp(xi - max) / Σ exp(xj - max)
用途: Attention 权重归一化

混合策略 (CPU reduction + GPU element-wise):
  CPU: 计算 max_val 和 sum_val = Σ exp(xi - max_val)
  GPU: 每个线程独立计算 out[tid] = exp(x[tid] - max_val) / sum_val
  - GPU kernel 是纯 element-wise，无需线程间通信
  - 生产环境用 GPU warp reduction (warp shuffle)
""")

    N = 8
    test_vals = [1.0, 2.0, 3.0, 4.0, 1.0, 0.5, -1.0, 2.5]
    max_val = max(test_vals)
    sum_val = sum(math.exp(x - max_val) for x in test_vals)
    print(f"  CPU 预计算: max={max_val}, sum={sum_val:.6f}")

    max_bits = float_bits(max_val)
    sum_bits = float_bits(sum_val)

    instructions = [
        {"op": "ThreadId", "dst": 0, "dim": "x"},
        {"op": "Mul", "dst": 1, "src1": {"kind": "Reg", "id": 0}, "src2": {"kind": "Imm", "val": 4}, "dtype": "i64"},
        {"op": "Add", "dst": 1, "src1": {"kind": "Reg", "id": 1}, "src2": {"kind": "Param", "id": 0}, "dtype": "i64"},
        {"op": "GlobalLoad", "dst": 2, "addr": {"kind": "Reg", "id": 1}, "dtype": "f32"},
        {"op": "Sub", "dst": 7, "src1": {"kind": "Reg", "id": 2}, "src2": {"kind": "Imm", "val": max_bits}, "dtype": "f32"},
        {"op": "Exp", "dst": 8, "src": {"kind": "Reg", "id": 7}, "dtype": "f32"},
        {"op": "Div", "dst": 9, "src1": {"kind": "Reg", "id": 8}, "src2": {"kind": "Imm", "val": sum_bits}, "dtype": "f32"},
        {"op": "Mul", "dst": 10, "src1": {"kind": "Reg", "id": 0}, "src2": {"kind": "Imm", "val": 4}, "dtype": "i64"},
        {"op": "Add", "dst": 10, "src1": {"kind": "Reg", "id": 10}, "src2": {"kind": "Param", "id": 1}, "dtype": "i64"},
        {"op": "GlobalStore", "addr": {"kind": "Reg", "id": 10}, "src": {"kind": "Reg", "id": 9}, "dtype": "f32"},
        {"op": "Return"},
    ]

    gir = make_gir("softmax_kernel", instructions, [
        {"name": "x", "dtype": "f32", "is_ptr": True},
        {"name": "out", "dtype": "f32", "is_ptr": True},
    ])

    spirv = gpu.compile_spirv(gir, "softmax_kernel")
    print(f"  SPIR-V 编译: {len(spirv)} bytes ✅")

    input_bytes = struct.pack(f'{N}f', *test_vals)
    gpu_results = gpu.run_kernel(spirv, "softmax_kernel", [(input_bytes, N * 4)], N)

    import torch
    x = torch.tensor(test_vals, dtype=torch.float32)
    softmax_ref = torch.softmax(x, dim=0)

    print(f"\n  {'输入':>8s}  {'GPU(GIR)':>12s}  {'PyTorch':>12s}  {'误差':>10s}")
    print(f"  {'-'*8}  {'-'*12}  {'-'*12}  {'-'*10}")
    all_pass = True
    for i in range(N):
        err = abs(gpu_results[i] - softmax_ref[i].item())
        status = "✅" if err < 1e-5 else "❌"
        if err >= 1e-5:
            all_pass = False
        print(f"  {test_vals[i]:8.2f}  {gpu_results[i]:12.8f}  {softmax_ref[i].item():12.8f}  {err:10.2e} {status}")

    print(f"\n  GPU sum = {sum(gpu_results):.8f} (应为 1.0)")
    if all_pass:
        print("  ✅ Softmax GPU kernel 验证通过！")
    print("""
  💡 优化分析:
  - CPU 做 reduction (max, sum), GPU 做 element-wise
  - 生产环境: Warp Reduction (warp shuffle), FlashAttention (QK^T+Softmax 融合)
""")
    return all_pass


# ============================================================
# 第 3 课: RMSNorm —— LLaMA/GPT 归一化层
# ============================================================

def lesson3_rmsnorm(gpu):
    """RMSNorm — LLaMA 归一化层"""
    print("\n" + "=" * 60)
    print("第 3 课: RMSNorm")
    print("=" * 60)
    print("""
原理: RMSNorm(x) = x / sqrt(mean(x^2) + eps) * gamma
用途: LLaMA/Gemma 等模型归一化层

混合策略: CPU 算 rms, GPU 做 element-wise (x * rms * gamma)
  相比 LayerNorm: 去掉减均值，只算 RMS (少一次 reduction)
""")

    N = 8
    EPS = 1e-6
    test_x = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]
    test_gamma = [1.0] * N

    sum_sq = sum(x * x for x in test_x)
    rms = 1.0 / math.sqrt(sum_sq / N + EPS)
    print(f"  CPU 预计算: sum_sq={sum_sq}, rms={rms:.6f}")

    rms_bits = float_bits(rms)

    instructions = [
        {"op": "ThreadId", "dst": 0, "dim": "x"},
        {"op": "Mul", "dst": 1, "src1": {"kind": "Reg", "id": 0}, "src2": {"kind": "Imm", "val": 4}, "dtype": "i64"},
        {"op": "Add", "dst": 1, "src1": {"kind": "Reg", "id": 1}, "src2": {"kind": "Param", "id": 0}, "dtype": "i64"},
        {"op": "GlobalLoad", "dst": 2, "addr": {"kind": "Reg", "id": 1}, "dtype": "f32"},
        {"op": "Mul", "dst": 5, "src1": {"kind": "Reg", "id": 0}, "src2": {"kind": "Imm", "val": 4}, "dtype": "i64"},
        {"op": "Add", "dst": 5, "src1": {"kind": "Reg", "id": 5}, "src2": {"kind": "Param", "id": 1}, "dtype": "i64"},
        {"op": "GlobalLoad", "dst": 6, "addr": {"kind": "Reg", "id": 5}, "dtype": "f32"},
        {"op": "Mul", "dst": 7, "src1": {"kind": "Reg", "id": 2}, "src2": {"kind": "Imm", "val": rms_bits}, "dtype": "f32"},
        {"op": "Mul", "dst": 8, "src1": {"kind": "Reg", "id": 7}, "src2": {"kind": "Reg", "id": 6}, "dtype": "f32"},
        {"op": "Mul", "dst": 9, "src1": {"kind": "Reg", "id": 0}, "src2": {"kind": "Imm", "val": 4}, "dtype": "i64"},
        {"op": "Add", "dst": 9, "src1": {"kind": "Reg", "id": 9}, "src2": {"kind": "Param", "id": 2}, "dtype": "i64"},
        {"op": "GlobalStore", "addr": {"kind": "Reg", "id": 9}, "src": {"kind": "Reg", "id": 8}, "dtype": "f32"},
        {"op": "Return"},
    ]

    gir = make_gir("rmsnorm_kernel", instructions, [
        {"name": "x", "dtype": "f32", "is_ptr": True},
        {"name": "gamma", "dtype": "f32", "is_ptr": True},
        {"name": "out", "dtype": "f32", "is_ptr": True},
    ])

    spirv = gpu.compile_spirv(gir, "rmsnorm_kernel")
    print(f"  SPIR-V 编译: {len(spirv)} bytes ✅")

    input_bytes = struct.pack(f'{N}f', *test_x)
    gamma_bytes = struct.pack(f'{N}f', *test_gamma)
    gpu_results = gpu.run_kernel(spirv, "rmsnorm_kernel", [(input_bytes, N * 4), (gamma_bytes, N * 4)], N)

    import torch
    x = torch.tensor(test_x, dtype=torch.float32)
    gamma = torch.tensor(test_gamma, dtype=torch.float32)
    rms_ref = x * torch.rsqrt((x ** 2).mean() + EPS) * gamma

    print(f"\n  {'输入':>8s}  {'GPU(GIR)':>12s}  {'PyTorch':>12s}  {'误差':>10s}")
    print(f"  {'-'*8}  {'-'*12}  {'-'*12}  {'-'*10}")
    all_pass = True
    for i in range(N):
        err = abs(gpu_results[i] - rms_ref[i].item())
        status = "✅" if err < 1e-5 else "❌"
        if err >= 1e-5:
            all_pass = False
        print(f"  {test_x[i]:8.2f}  {gpu_results[i]:12.8f}  {rms_ref[i].item():12.8f}  {err:10.2e} {status}")

    if all_pass:
        print("\n  ✅ RMSNorm GPU kernel 验证通过！")
    print("""
  💡 优化分析:
  - rsqrt 是 GPU 硬件指令; sum(x^2) 只需一次 reduction
  - 生产环境: gamma 乘法可和后续算子融合
""")
    return all_pass



def lesson4_gemm(gpu):
    """
    C = A @ B  (矩阵乘法)

    LLM 推理中 70%+ 的计算量来自 GEMM:
      - Attention: Q = X@Wq, K = X@Wk, V = X@Wv
      - MLP:       Y = X@W1, X = Y@W2
      - Projection:  Output = Attention@Wo

    本课演示基础 GEMM kernel:
      - 每个线程计算 C 矩阵的一个元素
      - 通过 tid 映射到 (row, col)

    优化进阶 (生产环境):
      1. Tiling: 用 shared memory 分块加载，减少全局内存访问
      2. Vectorize: 每次加载 4 个 float (float4)
      3. Register Tiling: 每个线程计算多个输出元素
      4. Tensor Core: 利用 GPU 矩阵乘单元 (WMMA/MMA)
    """
    print("\n" + "=" * 60)
    print("第 4 课: GEMM 矩阵乘法")
    print("=" * 60)
    print("""
原理: C[i][j] = Σ A[i][k] * B[k][j]
用途: LLM 中 QKV 投影、MLP 升维降维、输出投影

本课: 4x4 矩阵乘法，每个线程算一个输出元素
  A (4x4)  ×  B (4x4)  =  C (4x4)
  tid=0→C[0][0], tid=1→C[0][1], ..., tid=15→C[3][3]

  每个线程: 加载 A 的一行 + B 的一列，点积求和
""")

    M, K, N = 4, 4, 4  # 4x4x4 矩阵乘法
    total = M * N  # 16 个输出

    # A 矩阵 (行主序):
    # [[1, 2, 3, 4],
    #  [5, 6, 7, 8],
    #  [1, 1, 1, 1],
    #  [2, 2, 2, 2]]
    A = [1,2,3,4, 5,6,7,8, 1,1,1,1, 2,2,2,2]
    # B 矩阵 (行主序):
    # [[1, 0, 0, 1],
    #  [0, 1, 0, 1],
    #  [0, 0, 1, 1],
    #  [1, 1, 1, 0]]
    B = [1,0,0,1, 0,1,0,1, 0,0,1,1, 1,1,1,0]

    # GIR: 每个 tid 计算 C[row][col]
    instructions = [{"op": "ThreadId", "dst": 0, "dim": "x"}]

    # row = tid / N
    instructions.append({"op": "Div", "dst": 1, "src1": {"kind": "Reg", "id": 0}, "src2": {"kind": "Imm", "val": N}, "dtype": "i64"})
    # col = tid % N
    instructions.append({"op": "Mod", "dst": 2, "src1": {"kind": "Reg", "id": 0}, "src2": {"kind": "Imm", "val": N}, "dtype": "i64"})

    # accumulator = 0
    instructions.append({"op": "Move", "dst": 3, "src": {"kind": "Imm", "val": float_bits(0.0)}})

    # for k in range(K):
    for k in range(K):
        # load A[row][k] = A[row * K + k]
        a_idx = 10 + k * 4
        instructions.append({"op": "Mul", "dst": a_idx, "src1": {"kind": "Reg", "id": 1}, "src2": {"kind": "Imm", "val": K}, "dtype": "i64"})
        instructions.append({"op": "Add", "dst": a_idx + 1, "src1": {"kind": "Reg", "id": a_idx}, "src2": {"kind": "Imm", "val": k}, "dtype": "i64"})
        instructions.append({"op": "Mul", "dst": a_idx + 2, "src1": {"kind": "Reg", "id": a_idx + 1}, "src2": {"kind": "Imm", "val": 4}, "dtype": "i64"})
        instructions.append({"op": "Add", "dst": a_idx + 2, "src1": {"kind": "Reg", "id": a_idx + 2}, "src2": {"kind": "Param", "id": 0}, "dtype": "i64"})
        instructions.append({"op": "GlobalLoad", "dst": a_idx + 3, "addr": {"kind": "Reg", "id": a_idx + 2}, "dtype": "f32"})

        # load B[k][col] = B[k * N + col]
        b_idx = 50 + k * 4
        instructions.append({"op": "Mul", "dst": b_idx, "src1": {"kind": "Imm", "val": k}, "src2": {"kind": "Imm", "val": N}, "dtype": "i64"})
        instructions.append({"op": "Add", "dst": b_idx + 1, "src1": {"kind": "Reg", "id": b_idx}, "src2": {"kind": "Reg", "id": 2}, "dtype": "i64"})
        instructions.append({"op": "Mul", "dst": b_idx + 2, "src1": {"kind": "Reg", "id": b_idx + 1}, "src2": {"kind": "Imm", "val": 4}, "dtype": "i64"})
        instructions.append({"op": "Add", "dst": b_idx + 2, "src1": {"kind": "Reg", "id": b_idx + 2}, "src2": {"kind": "Param", "id": 1}, "dtype": "i64"})
        instructions.append({"op": "GlobalLoad", "dst": b_idx + 3, "addr": {"kind": "Reg", "id": b_idx + 2}, "dtype": "f32"})

        # accumulator += A[row][k] * B[k][col]
        instructions.append({"op": "Mul", "dst": b_idx + 4, "src1": {"kind": "Reg", "id": a_idx + 3}, "src2": {"kind": "Reg", "id": b_idx + 3}, "dtype": "f32"})
        instructions.append({"op": "Add", "dst": 3, "src1": {"kind": "Reg", "id": 3}, "src2": {"kind": "Reg", "id": b_idx + 4}, "dtype": "f32"})

    # store C[row][col] = C[tid] = accumulator
    instructions.append({"op": "Mul", "dst": 100, "src1": {"kind": "Reg", "id": 0}, "src2": {"kind": "Imm", "val": 4}, "dtype": "i64"})
    instructions.append({"op": "Add", "dst": 100, "src1": {"kind": "Reg", "id": 100}, "src2": {"kind": "Param", "id": 2}, "dtype": "i64"})
    instructions.append({"op": "GlobalStore", "addr": {"kind": "Reg", "id": 100}, "src": {"kind": "Reg", "id": 3}, "dtype": "f32"})

    instructions.append({"op": "Return"})

    gir = make_gir("gemm_kernel", instructions, [
        {"name": "A", "dtype": "f32", "is_ptr": True},
        {"name": "B", "dtype": "f32", "is_ptr": True},
        {"name": "C", "dtype": "f32", "is_ptr": True},
    ], next_reg=200, next_label=20)

    spirv = gpu.compile_spirv(gir, "gemm_kernel")
    print(f"  SPIR-V 编译: {len(spirv)} bytes ✅")

    # 准备数据
    A_bytes = struct.pack(f'{M*K}f', *A)
    B_bytes = struct.pack(f'{K*N}f', *B)

    # GPU 执行 (16 个线程 = 16 个输出元素)
    gpu_results = gpu.run_kernel(spirv, "gemm_kernel", [(A_bytes, M*K*4), (B_bytes, K*N*4)], total)

    # PyTorch 参考
    import torch
    A_t = torch.tensor(A, dtype=torch.float32).reshape(M, K)
    B_t = torch.tensor(B, dtype=torch.float32).reshape(K, N)
    C_ref = (A_t @ B_t).flatten().tolist()

    print(f"\n  矩阵 A ({M}x{K}):")
    for i in range(M):
        print(f"    {A[i*K:(i+1)*K]}")
    print(f"\n  矩阵 B ({K}x{N}):")
    for i in range(K):
        print(f"    {B[i*N:(i+1)*N]}")

    print(f"\n  GPU 结果 (展平): {[f'{v:.1f}' for v in gpu_results]}")
    print(f"  PyTorch  结果:   {[f'{v:.1f}' for v in C_ref]}")

    all_pass = True
    for i in range(total):
        if abs(gpu_results[i] - C_ref[i]) > 1e-5:
            all_pass = False
            print(f"  ❌ C[{i//N}][{i%N}] = {gpu_results[i]:.4f}, 期望 {C_ref[i]:.4f}")
    if all_pass:
        print("  ✅ GEMM GPU kernel 验证通过！")

    print("""
  💡 优化分析:
  - 本实现: 每个线程 O(K) 次全局内存加载 → 总共 O(M*N*K) 次
  - 生产环境优化:

    (1) Tiling (分块):
        将矩阵分成 tile (如 16x16)，用 shared memory 缓存
        每个 tile 加载一次，复用多次
        全局内存访问: O(M*N*K / TILE_SIZE²) → 大幅减少

    (2) Vectorized Access:
        每次加载 float4 (4 个 float) 而非单个 float
        利用 GPU 内存控制器的带宽

    (3) Register Tiling:
        每个线程计算 4x4 个输出元素 (而非 1 个)
        减少线程数，增加寄存器复用

    (4) Tensor Core (NVIDIA):
        使用 WMMA/MMA 指令直接调用矩阵乘单元
        4x4x4 矩阵乘只需 1 个时钟周期

    (5) 双缓冲:
        在计算当前 tile 时，预加载下一个 tile
        隐藏内存延迟
""")
    return all_pass


# ============================================================
# 第 5 课: Fused Kernel —— 算子融合
# ============================================================

def lesson5_fused(gpu):
    """
    算子融合: 将多个连续算子合并为一个 kernel

    示例: Fused GELU + Scale
      朴素: tmp = GELU(x)   → 读 x, 写 tmp (2 次显存访问)
            out = tmp * s   → 读 tmp, 写 out (2 次显存访问)
            总计: 4 次显存访问

      融合: out = GELU(x) * s  → 读 x, 写 out (2 次显存访问)
            总计: 2 次显存访问 (减少 50%)

    在 LLM 推理中常见的融合:
      - QK^T + Softmax + Scale (FlashAttention)
      - RMSNorm + QKV Projection
      - GEMM + Bias + GELU
      - Residual + RMSNorm

    显存带宽是 LLM 推理的主要瓶颈，融合是最高 ROI 的优化。
    """
    print("\n" + "=" * 60)
    print("第 5 课: 算子融合 (Fused GELU + Scale)")
    print("=" * 60)
    print("""
原理: 将连续的多个算子合并到一个 GPU kernel 中
示例: out = GELU(x) * scale

不融合:
  1. tmp = GELU(x)    → 读 x (1次), 写 tmp (1次) = 2 次显存访问
  2. out = tmp * s    → 读 tmp (1次), 写 out (1次) = 2 次显存访问
  总计: 4 次显存访问, tmp 需要 N*4 字节临时显存

融合后:
  out = GELU(x) * s   → 读 x (1次), 写 out (1次) = 2 次显存访问
  总计: 2 次显存访问 (减少 50%), 无临时显存

LLM 中的常见融合:
  - FlashAttention: QK^T + Softmax + AV 融合 (省 O(N²) 显存)
  - RMSNorm + QKV:  归一化和投影融合
  - GEMM + Bias + GELU: 投影加偏置加激活融合
""")

    SCALE = 2.0
    SQRT_2_OVER_PI = float_bits(0.7978845608028654)
    COEFF = float_bits(0.044715)
    HALF = float_bits(0.5)
    SCALE_BITS = float_bits(SCALE)

    instructions = [
        {"op": "ThreadId", "dst": 0, "dim": "x"},

        # addr = tid * 4 + param[0]
        {"op": "Mul", "dst": 1, "src1": {"kind": "Reg", "id": 0}, "src2": {"kind": "Imm", "val": 4}, "dtype": "i64"},
        {"op": "Add", "dst": 1, "src1": {"kind": "Reg", "id": 1}, "src2": {"kind": "Param", "id": 0}, "dtype": "i64"},

        # x = input[tid]
        {"op": "GlobalLoad", "dst": 2, "addr": {"kind": "Reg", "id": 1}, "dtype": "f32"},

        # GELU(x): x3 = x*x*x, inner, tanh, etc.
        {"op": "Mul", "dst": 3, "src1": {"kind": "Reg", "id": 2}, "src2": {"kind": "Reg", "id": 2}, "dtype": "f32"},
        {"op": "Mul", "dst": 4, "src1": {"kind": "Reg", "id": 3}, "src2": {"kind": "Reg", "id": 2}, "dtype": "f32"},
        {"op": "Mul", "dst": 5, "src1": {"kind": "Imm", "val": COEFF}, "src2": {"kind": "Reg", "id": 4}, "dtype": "f32"},
        {"op": "Add", "dst": 6, "src1": {"kind": "Reg", "id": 2}, "src2": {"kind": "Reg", "id": 5}, "dtype": "f32"},
        {"op": "Mul", "dst": 7, "src1": {"kind": "Imm", "val": SQRT_2_OVER_PI}, "src2": {"kind": "Reg", "id": 6}, "dtype": "f32"},
        {"op": "Tanh", "dst": 8, "src": {"kind": "Reg", "id": 7}, "dtype": "f32"},
        {"op": "Add", "dst": 9, "src1": {"kind": "Imm", "val": float_bits(1.0)}, "src2": {"kind": "Reg", "id": 8}, "dtype": "f32"},
        {"op": "Mul", "dst": 10, "src1": {"kind": "Imm", "val": HALF}, "src2": {"kind": "Reg", "id": 2}, "dtype": "f32"},
        {"op": "Mul", "dst": 11, "src1": {"kind": "Reg", "id": 10}, "src2": {"kind": "Reg", "id": 9}, "dtype": "f32"},

        # out = GELU(x) * scale  ← 融合点: scale 直接合并到同一个 kernel
        {"op": "Mul", "dst": 12, "src1": {"kind": "Reg", "id": 11}, "src2": {"kind": "Imm", "val": SCALE_BITS}, "dtype": "f32"},

        # store
        {"op": "Mul", "dst": 13, "src1": {"kind": "Reg", "id": 0}, "src2": {"kind": "Imm", "val": 4}, "dtype": "i64"},
        {"op": "Add", "dst": 13, "src1": {"kind": "Reg", "id": 13}, "src2": {"kind": "Param", "id": 1}, "dtype": "i64"},
        {"op": "GlobalStore", "addr": {"kind": "Reg", "id": 13}, "src": {"kind": "Reg", "id": 12}, "dtype": "f32"},

        {"op": "Return"},
    ]

    gir = make_gir("fused_gelu_scale", instructions, [
        {"name": "x", "dtype": "f32", "is_ptr": True},
        {"name": "out", "dtype": "f32", "is_ptr": True},
    ])

    spirv = gpu.compile_spirv(gir, "fused_gelu_scale")
    print(f"  SPIR-V 编译: {len(spirv)} bytes ✅")

    test_vals = [-3.0, -2.0, -1.0, -0.5, 0.0, 0.5, 1.0, 2.0, 3.0, 5.0]
    N = len(test_vals)
    input_bytes = struct.pack(f'{N}f', *test_vals)

    gpu_results = gpu.run_kernel(spirv, "fused_gelu_scale", [(input_bytes, N * 4)], N)

    # PyTorch 参考
    import torch
    x = torch.tensor(test_vals, dtype=torch.float32)
    gelu_tanh = 0.5 * x * (1 + torch.tanh(math.sqrt(2/math.pi) * (x + 0.044715 * x**3)))
    ref = gelu_tanh * SCALE

    print(f"\n  {'输入':>8s}  {'GPU(GIR)':>12s}  {'PyTorch':>12s}  {'误差':>10s}")
    print(f"  {'-'*8}  {'-'*12}  {'-'*12}  {'-'*10}")
    all_pass = True
    for i in range(N):
        err = abs(gpu_results[i] - ref[i].item())
        status = "✅" if err < 1e-5 else "❌"
        if err >= 1e-5:
            all_pass = False
        print(f"  {test_vals[i]:8.2f}  {gpu_results[i]:12.6f}  {ref[i].item():12.6f}  {err:10.2e} {status}")

    if all_pass:
        print("\n  ✅ Fused GPU kernel 验证通过！")

    print("""
  💡 融合 vs 不融合 对比:

  ┌─────────────────────────────────────────────┐
  │ 不融合 (2 个 kernel)                        │
  │                                             │
  │   Kernel 1: GELU                            │
  │   内存读: x[tid]     ← 1 次全局读           │
  │   内存写: tmp[tid]   ← 1 次全局写           │
  │   临时显存: N * 4 bytes                     │
  │                                             │
  │   Kernel 2: Scale                           │
  │   内存读: tmp[tid]   ← 1 次全局读           │
  │   内存写: out[tid]   ← 1 次全局写           │
  │                                             │
  │   总计: 4 次全局内存访问 + N*4 临时显存     │
  ├─────────────────────────────────────────────┤
  │ 融合 (1 个 kernel)                          │
  │                                             │
  │   Kernel: Fused GELU + Scale                │
  │   内存读: x[tid]     ← 1 次全局读           │
  │   内存写: out[tid]   ← 1 次全局写           │
  │   中间结果在寄存器中，不落显存              │
  │                                             │
  │   总计: 2 次全局内存访问 + 0 临时显存       │
  │   节省 50% 显存带宽！                       │
  └─────────────────────────────────────────────┘

  🔑 核心洞察:
  - LLM 推理是 memory-bound (显存带宽是瓶颈)
  - 每减少一次显存读写，性能提升显著
  - 这就是 FlashAttention 能 2-4x 加速的原因
  - Triton/CUDA 的核心价值就是让用户能手写融合 kernel
""")
    return all_pass


# ============================================================
# 主函数
# ============================================================

def main():
    lessons = {
        1: ("GELU 激活函数", lesson1_gelu),
        2: ("Softmax", lesson2_softmax),
        3: ("RMSNorm", lesson3_rmsnorm),
        4: ("GEMM 矩阵乘法", lesson4_gemm),
        5: ("算子融合", lesson5_fused),
    }

    print("=" * 60)
    print("LLM 优化算子部署实战教程")
    print("=" * 60)
    print(f"""
本教程在你的 AMD GPU 上实际运行 LLM 核心算子。

GPU 信息: """)

    gpu = GpuRunner()
    print(f"  设备: {gpu.gpu_name}")
    print(f"  运行时: Rusticl/Mesa OpenCL")
    print(f"  编译路径: GIR JSON → SPIR-V → OpenCL → AMD GPU")

    # 选择要运行的课程
    if len(sys.argv) > 1:
        run_ids = [int(x) for x in sys.argv[1:]]
    else:
        run_ids = list(lessons.keys())

    results = []
    for lid in run_ids:
        if lid in lessons:
            name, func = lessons[lid]
            try:
                ok = func(gpu)
                results.append((f"第{lid}课: {name}", ok))
            except Exception as e:
                import traceback
                traceback.print_exc()
                results.append((f"第{lid}课: {name}", False))

    print("\n" + "=" * 60)
    print("结果汇总")
    print("=" * 60)
    for name, ok in results:
        print(f"  {'✅ PASS' if ok else '❌ FAIL'}  {name}")
    print("=" * 60)


if __name__ == "__main__":
    main()
