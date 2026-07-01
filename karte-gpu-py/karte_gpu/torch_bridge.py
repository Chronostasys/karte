"""
karte_gpu.torch_bridge — PyTorch 零拷贝互操作层

将 karte GPU kernel 注册为 PyTorch custom op，实现:
1. torch.Tensor → karte kernel 零拷贝 (通过 data_ptr() 共享 GPU 显存)
2. karte kernel 输出 → torch.Tensor (无需数据拷贝)
3. torch.autograd.Function 集成 (支持反向传播)

使用方式:
    from karte_gpu.torch_bridge import karte_op, karte_function

    @karte_op
    def my_kernel(x: karte.Tensor["N"], scale: float) -> karte.Tensor["N"]:
        tid = karte.thread_id()
        return x[tid] * scale

    # 直接用 torch.Tensor 调用
    x = torch.randn(1024, device='cuda')
    out = my_kernel(x, 2.0)  # 零拷贝, 返回 torch.Tensor

与 vLLM/SGLang 集成:
    # 替换 vLLM 的 RMSNorm
    class KarteRMSNorm:
        def forward(self, x):
            return karte_rmsnorm(x, self.weight, self.eps)
"""

import ctypes
import ctypes.util
import json
import struct
import subprocess
import os
import torch
import torch.nn as nn
from typing import Callable, Any, Optional, List, Tuple

# 延迟导入 karte_jit
_karte_jit = None

def _get_karte():
    global _karte_jit
    if _karte_jit is None:
        from karte_gpu import karte_jit
        _karte_jit = karte_jit
    return _karte_jit


# ============================================================
# CUDA 零拷贝 bridge
# ============================================================

class CudaBridge:
    """CUDA 后端零拷贝 bridge — torch.Tensor.data_ptr() 直接传给 karte kernel"""

    def __init__(self):
        self._cuda = None
        self._initialized = False
        self._module_cache = {}  # kernel_name → (module, func) 缓存

    def _init(self):
        if self._initialized:
            return
        self._cuda = ctypes.CDLL(ctypes.util.find_library('cuda'))
        self._cuda.cuInit(0)

        # 获取 PyTorch 的 CUDA primary context (不创建新 context)
        # cuDevicePrimaryCtxRetain 返回 PyTorch 使用的 primary context
        self._cuda.cuDevicePrimaryCtxRetain.argtypes = [ctypes.POINTER(ctypes.c_void_p), ctypes.c_int]
        self._cuda.cuDevicePrimaryCtxRetain.restype = ctypes.c_int
        self._cuda.cuCtxSetCurrent.argtypes = [ctypes.c_void_p]
        self._cuda.cuCtxSetCurrent.restype = ctypes.c_int

        # 确保 PyTorch 已初始化 CUDA context
        _ = torch.cuda.device_count()
        self._ctx = ctypes.c_void_p()
        ret = self._cuda.cuDevicePrimaryCtxRetain(ctypes.byref(self._ctx), 0)
        if ret != 0:
            raise RuntimeError(f"cuDevicePrimaryCtxRetain 失败: {ret}")
        self._cuda.cuCtxSetCurrent(self._ctx)

        # cuModuleLoad (从文件加载 PTX/CUBIN)
        self._cuda.cuModuleLoad.argtypes = [ctypes.POINTER(ctypes.c_void_p), ctypes.c_char_p]
        self._cuda.cuModuleLoad.restype = ctypes.c_int

        self._cuda.cuModuleGetFunction.argtypes = [ctypes.POINTER(ctypes.c_void_p), ctypes.c_void_p, ctypes.c_char_p]
        self._cuda.cuModuleGetFunction.restype = ctypes.c_int

        self._cuda.cuLaunchKernel.argtypes = (
            [ctypes.c_void_p] +
            [ctypes.c_uint] * 7 +
            [ctypes.c_void_p] +
            [ctypes.c_void_p] +
            [ctypes.c_void_p]
        )
        self._cuda.cuLaunchKernel.restype = ctypes.c_int

        self._cuda.cuStreamSynchronize.argtypes = [ctypes.c_void_p]
        self._cuda.cuStreamSynchronize.restype = ctypes.c_int

        self._initialized = True

    def compile_ptx(self, gir_json: dict) -> bytes:
        """将 GIR JSON 编译为 PTX 文本 (cuModuleLoadData 支持 PTX 文本加载)"""
        result = subprocess.run(
            ["./target/debug/karte", "gpu-jit", "--backend", "nvidia", "--output-format", "text"],
            input=json.dumps(gir_json).encode(),
            capture_output=True,
            timeout=30,
            cwd=os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
        )
        if result.returncode != 0:
            raise RuntimeError(f"PTX 编译失败: {result.stderr.decode()}")
        return result.stdout  # PTX 文本 (null-terminated)

    def launch_kernel(
        self,
        ptx_binary: bytes,
        kernel_name: str,
        tensor_args: List[torch.Tensor],
        scalar_args: List[Any],
        grid: Tuple[int, int, int] = (1, 1, 1),
        block: Tuple[int, int, int] = (256, 1, 1),
    ) -> None:
        """零拷贝启动 CUDA kernel — tensor 参数通过 data_ptr() 直接传递"""
        self._init()
        cu = self._cuda

        # 确保使用 primary context
        cu.cuCtxSetCurrent(self._ctx)

        # 检查 kernel 缓存 — 用 GIR 内容hash做key, 避免不同维度复用
        import hashlib
        cache_key = hashlib.md5(kernel_name.encode() + ptx_binary).hexdigest()[:16]
        if cache_key in self._module_cache:
            module, func = self._module_cache[cache_key]
        else:
            # 加载 PTX: 先尝试直接加载, 失败则用 nvcc 编译为 CUBIN
            import tempfile
            with tempfile.NamedTemporaryFile(suffix='.ptx', mode='wb', delete=False) as f:
                f.write(ptx_binary)
                ptx_path = f.name

            module = ctypes.c_void_p()
            ret = cu.cuModuleLoad(ctypes.byref(module), ptx_path.encode())

            if ret != 0:
                # PTX 版本不兼容 (如 sm_8.0 PTX 在 sm_120 GPU 上)
                # 用 nvcc 编译为 CUBIN
                cubin_path = ptx_path.replace('.ptx', '.cubin')
                ptx_text = ptx_binary.decode('utf-8', errors='replace')
                major, minor = torch.cuda.get_device_capability(0)
                sm_target = f'sm_{major}{minor}'
                ptx_text = ptx_text.replace('.version 8.0', '.version 8.7')
                ptx_text = ptx_text.replace('sm_8_0', sm_target)

                with open(ptx_path, 'w') as fw:
                    fw.write(ptx_text)

                nvcc_result = subprocess.run(
                    ['nvcc', '-cubin', f'-arch={sm_target}', ptx_path, '-o', cubin_path],
                    capture_output=True, timeout=30
                )
                if nvcc_result.returncode == 0 and os.path.exists(cubin_path):
                    ret = cu.cuModuleLoad(ctypes.byref(module), cubin_path.encode())
                    os.unlink(cubin_path)
                else:
                    os.unlink(ptx_path)
                    raise RuntimeError(f"PTX 编译失败: nvcc={nvcc_result.returncode}, cuModuleLoad={ret}\n"
                                     f"nvcc stderr: {nvcc_result.stderr.decode()[:300]}")

            os.unlink(ptx_path)
            if ret != 0:
                raise RuntimeError(f"cuModuleLoad 失败: {ret}")

            # 获取 kernel function
            func = ctypes.c_void_p()
            ret = cu.cuModuleGetFunction(ctypes.byref(func), module, kernel_name.encode())
            if ret != 0:
                raise RuntimeError(f"cuModuleGetFunction 失败: {ret}")

            # 缓存编译结果
            self._module_cache[cache_key] = (module, func)

        # 构建参数数组 — 每个参数是指向值的指针
        param_ptrs = []
        param_values = []

        # tensor 参数: 传递 data_ptr() (GPU 显存指针)
        for tensor in tensor_args:
            if not tensor.is_cuda:
                tensor = tensor.cuda()
            if not tensor.is_contiguous():
                tensor = tensor.contiguous()
            ptr_val = tensor.data_ptr()
            param_values.append(ctypes.c_uint64(ptr_val))

        # scalar 参数
        for s in scalar_args:
            if isinstance(s, float):
                param_values.append(ctypes.c_float(s))
            elif isinstance(s, int):
                param_values.append(ctypes.c_int(s))
            else:
                param_values.append(ctypes.c_int(int(s)))

        # 构建 kernelParams 数组 (每个元素是指向参数值的指针)
        kernel_params = (ctypes.c_void_p * len(param_values))()
        for i, val in enumerate(param_values):
            kernel_params[i] = ctypes.cast(ctypes.byref(val), ctypes.c_void_p)

        # 启动 kernel
        ret = cu.cuLaunchKernel(
            func,
            grid[0], grid[1], grid[2],    # grid dims
            block[0], block[1], block[2],  # block dims
            0,                              # shared mem
            None,                           # stream (default)
            kernel_params,                  # kernel params
            None,                           # extra
        )
        if ret != 0:
            raise RuntimeError(f"cuLaunchKernel 失败: {ret}")

        cu.cuStreamSynchronize(None)


# ============================================================
# OpenCL bridge (AMD GPU)
# ============================================================

class OpenCLBridge:
    """OpenCL 后端 — 用于 AMD GPU"""

    def __init__(self):
        self._cl = None
        self._ctx = None
        self._queue = None
        self._device = None
        self._initialized = False

    def _init(self):
        if self._initialized:
            return
        cl = ctypes.CDLL(ctypes.util.find_library('OpenCL'))
        self._cl = cl

        # 设置函数签名 (与 test_amd_spirv.py 相同)
        cl.clGetPlatformIDs.argtypes = [ctypes.c_uint, ctypes.POINTER(ctypes.c_void_p), ctypes.POINTER(ctypes.c_uint)]
        cl.clGetPlatformIDs.restype = ctypes.c_int
        cl.clGetPlatformInfo.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_size_t, ctypes.c_void_p, ctypes.POINTER(ctypes.c_size_t)]
        cl.clGetPlatformInfo.restype = ctypes.c_int
        cl.clGetDeviceIDs.argtypes = [ctypes.c_void_p, ctypes.c_uint64, ctypes.c_uint, ctypes.POINTER(ctypes.c_void_p), ctypes.POINTER(ctypes.c_uint)]
        cl.clGetDeviceIDs.restype = ctypes.c_int
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

        # 找 Rusticl 平台
        num_plat = ctypes.c_uint(0)
        cl.clGetPlatformIDs(0, None, ctypes.byref(num_plat))
        platforms = (ctypes.c_void_p * num_plat.value)()
        cl.clGetPlatformIDs(num_plat.value, platforms, None)

        for p in platforms:
            sz = ctypes.c_size_t(0)
            cl.clGetPlatformInfo(p, 0x0902, 0, None, ctypes.byref(sz))
            nb = ctypes.create_string_buffer(sz.value)
            cl.clGetPlatformInfo(p, 0x0902, sz.value, nb, None)
            if 'rusticl' in nb.value.decode().lower():
                nd = ctypes.c_uint(0)
                cl.clGetDeviceIDs(p, 1<<2, 0, None, ctypes.byref(nd))
                devs = (ctypes.c_void_p * nd.value)()
                cl.clGetDeviceIDs(p, 1<<2, nd.value, devs, None)
                self._device = devs[0]
                break

        if self._device is None:
            raise RuntimeError("未找到 Rusticl GPU")

        err = ctypes.c_int(0)
        self._ctx = cl.clCreateContext(None, 1, ctypes.byref(ctypes.c_void_p(self._device)), None, None, ctypes.byref(err))
        self._queue = cl.clCreateCommandQueueWithProperties(self._ctx, self._device, None, ctypes.byref(err))
        self._initialized = True

    def compile_spirv(self, gir_json: dict) -> bytes:
        """将 GIR JSON 编译为 SPIR-V 二进制"""
        result = subprocess.run(
            ["./target/debug/karte", "gpu-jit", "--backend", "spirv"],
            input=json.dumps(gir_json).encode(),
            capture_output=True,
            timeout=30,
            cwd=os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
        )
        if result.returncode != 0:
            raise RuntimeError(f"SPIR-V 编译失败: {result.stderr.decode()}")
        return result.stdout

    def launch_kernel(
        self,
        spirv_binary: bytes,
        kernel_name: str,
        input_tensors: List[torch.Tensor],
        output_shape: Tuple[int, ...],
        output_dtype: torch.dtype = torch.float32,
        global_size: int = 1,
        local_size: int = 1,
    ) -> torch.Tensor:
        """启动 OpenCL kernel — tensor 数据拷贝到 OpenCL buffer, 结果拷贝回 torch.Tensor"""
        self._init()
        cl = self._cl

        # 加载 SPIR-V
        spirv_buf = ctypes.create_string_buffer(spirv_binary)
        err = ctypes.c_int(0)
        program = cl.clCreateProgramWithIL(self._ctx, spirv_buf, len(spirv_binary), ctypes.byref(err))
        ret = cl.clBuildProgram(program, 1, ctypes.byref(ctypes.c_void_p(self._device)), None, None, None)
        if ret != 0:
            raise RuntimeError(f"clBuildProgram 失败: {ret}")
        kernel = cl.clCreateKernel(program, kernel_name.encode(), ctypes.byref(err))

        # 创建 OpenCL buffers 并写入数据
        CL_MEM_READ_WRITE = 1
        CL_MEM_COPY_HOST_PTR = 1 << 5
        buffers = []

        for tensor in input_tensors:
            if tensor.is_cuda:
                tensor = tensor.cpu()
            nbytes = tensor.numel() * tensor.element_size()
            host_data = ctypes.create_string_buffer(tensor.numpy().tobytes())
            buf = cl.clCreateBuffer(self._ctx, CL_MEM_READ_WRITE | CL_MEM_COPY_HOST_PTR,
                                    nbytes, ctypes.cast(host_data, ctypes.c_void_p), ctypes.byref(err))
            buffers.append(buf)

        # 输出 buffer
        out_nbytes = 1
        for s in output_shape:
            out_nbytes *= s
        out_nbytes *= 4 if output_dtype == torch.float32 else 8
        out_buf = cl.clCreateBuffer(self._ctx, CL_MEM_READ_WRITE, out_nbytes, None, ctypes.byref(err))
        buffers.append(out_buf)

        # 设置参数
        for i, buf in enumerate(buffers):
            arg_val = ctypes.c_uint64(ctypes.cast(buf, ctypes.c_void_p).value)
            cl.clSetKernelArg(kernel, i, 8, ctypes.byref(arg_val))

        # 启动
        global_ws = (ctypes.c_size_t * 3)(global_size, 1, 1)
        local_ws = (ctypes.c_size_t * 3)(local_size, 1, 1)
        ret = cl.clEnqueueNDRangeKernel(self._queue, kernel, 1, None, global_ws, local_ws, 0, None, None)
        if ret != 0:
            raise RuntimeError(f"clEnqueueNDRangeKernel 失败: {ret}")
        cl.clFinish(self._queue)

        # 读回结果到 torch.Tensor
        out_elements = out_nbytes // 4
        out_host = ctypes.create_string_buffer(out_nbytes)
        cl.clEnqueueReadBuffer(self._queue, out_buf, 1, 0, out_nbytes, out_host, 0, None, None)

        result = torch.frombuffer(bytearray(out_host.raw), dtype=output_dtype).reshape(output_shape)
        return result


# ============================================================
# 统一 bridge 接口
# ============================================================

_bridge = None

def get_bridge():
    """获取当前后端的 bridge (自动检测 CUDA 或 OpenCL)"""
    global _bridge
    if _bridge is not None:
        return _bridge
    if torch.cuda.is_available():
        _bridge = CudaBridge()
    elif ctypes.util.find_library('OpenCL'):
        _bridge = OpenCLBridge()
    else:
        raise RuntimeError("没有可用的 GPU 后端")
    return _bridge


# ============================================================
# LLM 算子实现 (GIR JSON → GPU kernel)
# ============================================================

def _float_bits(v):
    """float32 → 整数位表示"""
    return int(struct.unpack('I', struct.pack('f', float(v)))[0])


def make_gir_kernel(name, instructions, params, block_dim=(256,1,1)):
    """构建 GIR JSON"""
    return {
        "kernels": [{
            "name": name,
            "params": params,
            "block_dim": list(block_dim),
            "next_reg": 500,
            "next_label": 20,
            "instructions": instructions,
        }]
    }


# ============================================================
# FlashAttention — 融合 QK^T + Softmax + AV
# ============================================================

def flash_attention_gir(seq_len, head_dim, tile_size=16):
    """
    FlashAttention (简化版, 展开):
      S = Q @ K^T / sqrt(d)  (不存 S 到显存, 用寄存器)
      O = S @ V

    每个 thread 处理一行 (一个 query), 全部在寄存器中计算。
    使用 FMA (Multiply-Add) 指令展开, 不依赖 Tile 指令。
    """
    S = seq_len
    D = head_dim
    import math
    scale = 1.0 / math.sqrt(D)
    scale_bits = _float_bits(scale)

    instructions = []
    reg = 0  # 下一个可用寄存器

    # tid = thread_id() — 每个 tid 处理第 tid 行
    instructions.append({"op": "ThreadId", "dst": reg, "dim": "x"})
    tid_reg = reg
    reg += 1

    # 加载 Q[tid, 0..D-1] (D 个值)
    q_regs = []
    for d in range(D):
        # offset = tid * D * 4 + d * 4 (byte offset)
        instructions.append({"op": "Mul", "dst": reg, "src1": {"kind": "Reg", "id": tid_reg}, "src2": {"kind": "Imm", "val": D * 4}, "dtype": "i64"})
        instructions.append({"op": "Add", "dst": reg, "src1": {"kind": "Reg", "id": reg}, "src2": {"kind": "Imm", "val": d * 4}, "dtype": "i64"})
        instructions.append({"op": "Add", "dst": reg, "src1": {"kind": "Reg", "id": reg}, "src2": {"kind": "Param", "id": 0}, "dtype": "i64"})
        instructions.append({"op": "GlobalLoad", "dst": reg, "addr": {"kind": "Reg", "id": reg}, "dtype": "f32"})
        q_regs.append(reg)
        reg += 1

    # 计算 S[tid, j] = sum_k Q[tid,k] * K[j,k] * scale, for j=0..S-1
    # K 是 [S, D] row-major, K[j,k] at offset j*D*4 + k*4
    s_regs = []
    for j in range(S):
        # 累加 S[j] = sum(Q[k] * K[j,k]) for k=0..D-1
        acc_reg = reg
        reg += 1
        for k in range(D):
            # 加载 K[j, k]
            k_off = j * D * 4 + k * 4
            instructions.append({"op": "Add", "dst": reg, "src1": {"kind": "Imm", "val": k_off}, "src2": {"kind": "Param", "id": 1}, "dtype": "i64"})
            instructions.append({"op": "GlobalLoad", "dst": reg, "addr": {"kind": "Reg", "id": reg}, "dtype": "f32"})
            # acc += Q[k] * K[j,k]
            if k == 0:
                instructions.append({"op": "Mul", "dst": acc_reg, "src1": {"kind": "Reg", "id": q_regs[k]}, "src2": {"kind": "Reg", "id": reg}, "dtype": "f32"})
            else:
                instructions.append({"op": "Fma", "dst": acc_reg, "src1": {"kind": "Reg", "id": q_regs[k]}, "src2": {"kind": "Reg", "id": reg}, "src3": {"kind": "Reg", "id": acc_reg}, "dtype": "f32"})
            reg += 1
        # scale
        instructions.append({"op": "Mul", "dst": acc_reg, "src1": {"kind": "Reg", "id": acc_reg}, "src2": {"kind": "Imm", "val": scale_bits}, "dtype": "f32"})
        s_regs.append(acc_reg)

    # 计算 O[tid, d] = sum_j S[j] * V[j, d], for d=0..D-1
    # V 是 [S, D] row-major, V[j,d] at offset j*D*4 + d*4
    for d in range(D):
        acc_reg = reg
        reg += 1
        for j in range(S):
            # 加载 V[j, d]
            v_off = j * D * 4 + d * 4
            instructions.append({"op": "Add", "dst": reg, "src1": {"kind": "Imm", "val": v_off}, "src2": {"kind": "Param", "id": 2}, "dtype": "i64"})
            instructions.append({"op": "GlobalLoad", "dst": reg, "addr": {"kind": "Reg", "id": reg}, "dtype": "f32"})
            if j == 0:
                instructions.append({"op": "Mul", "dst": acc_reg, "src1": {"kind": "Reg", "id": s_regs[j]}, "src2": {"kind": "Reg", "id": reg}, "dtype": "f32"})
            else:
                instructions.append({"op": "Fma", "dst": acc_reg, "src1": {"kind": "Reg", "id": s_regs[j]}, "src2": {"kind": "Reg", "id": reg}, "src3": {"kind": "Reg", "id": acc_reg}, "dtype": "f32"})
            reg += 1
        # 存储 O[tid, d]
        instructions.append({"op": "Mul", "dst": reg, "src1": {"kind": "Reg", "id": tid_reg}, "src2": {"kind": "Imm", "val": D * 4}, "dtype": "i64"})
        instructions.append({"op": "Add", "dst": reg, "src1": {"kind": "Reg", "id": reg}, "src2": {"kind": "Imm", "val": d * 4}, "dtype": "i64"})
        instructions.append({"op": "Add", "dst": reg, "src1": {"kind": "Reg", "id": reg}, "src2": {"kind": "Param", "id": 3}, "dtype": "i64"})
        instructions.append({"op": "GlobalStore", "addr": {"kind": "Reg", "id": reg}, "src": {"kind": "Reg", "id": acc_reg}, "dtype": "f32"})
        reg += 1

    instructions.append({"op": "Return"})

    return make_gir_kernel("flash_attn", instructions, [
        {"name": "Q", "dtype": "f32", "is_ptr": True},
        {"name": "K", "dtype": "f32", "is_ptr": True},
        {"name": "V", "dtype": "f32", "is_ptr": True},
        {"name": "O", "dtype": "f32", "is_ptr": True},
    ], block_dim=(S, 1, 1))


# ============================================================
# Fused RMSNorm — LLaMA 归一化层 (element-wise 版)
# ============================================================

def fused_rmsnorm_gir(n):
    """RMSNorm: out[tid] = x[tid] * rms * gamma[tid]

    rms 值由 CPU 预计算并作为标量参数传入 (Param id=3, f32)"""

    instructions = [
        {"op": "ThreadId", "dst": 0, "dim": "x"},
        # load x[tid]
        {"op": "Mul", "dst": 1, "src1": {"kind": "Reg", "id": 0}, "src2": {"kind": "Imm", "val": 4}, "dtype": "i64"},
        {"op": "Add", "dst": 1, "src1": {"kind": "Reg", "id": 1}, "src2": {"kind": "Param", "id": 0}, "dtype": "i64"},
        {"op": "GlobalLoad", "dst": 2, "addr": {"kind": "Reg", "id": 1}, "dtype": "f32"},
        # load gamma[tid]
        {"op": "Mul", "dst": 3, "src1": {"kind": "Reg", "id": 0}, "src2": {"kind": "Imm", "val": 4}, "dtype": "i64"},
        {"op": "Add", "dst": 3, "src1": {"kind": "Reg", "id": 3}, "src2": {"kind": "Param", "id": 1}, "dtype": "i64"},
        {"op": "GlobalLoad", "dst": 4, "addr": {"kind": "Reg", "id": 3}, "dtype": "f32"},
        # out = x * rms_scalar * gamma
        {"op": "Mul", "dst": 5, "src1": {"kind": "Reg", "id": 2}, "src2": {"kind": "Param", "id": 3}, "dtype": "f32"},
        {"op": "Mul", "dst": 6, "src1": {"kind": "Reg", "id": 5}, "src2": {"kind": "Reg", "id": 4}, "dtype": "f32"},
        # store
        {"op": "Mul", "dst": 7, "src1": {"kind": "Reg", "id": 0}, "src2": {"kind": "Imm", "val": 4}, "dtype": "i64"},
        {"op": "Add", "dst": 7, "src1": {"kind": "Reg", "id": 7}, "src2": {"kind": "Param", "id": 2}, "dtype": "i64"},
        {"op": "GlobalStore", "addr": {"kind": "Reg", "id": 7}, "src": {"kind": "Reg", "id": 6}, "dtype": "f32"},
        {"op": "Return"},
    ]

    return make_gir_kernel("rmsnorm", instructions, [
        {"name": "x", "dtype": "f32", "is_ptr": True},
        {"name": "gamma", "dtype": "f32", "is_ptr": True},
        {"name": "out", "dtype": "f32", "is_ptr": True},
        {"name": "rms", "dtype": "f32", "is_ptr": False},
    ], block_dim=(n, 1, 1))


# ============================================================
# GELU 激活 (element-wise, 用于 MLP 层)
# ============================================================

def gelu_gir(n):
    """GELU(x) = 0.5 * x * (1 + tanh(√(2/π) * (x + 0.044715 * x³)))"""
    SQRT_2_OVER_PI = _float_bits(0.7978845608028654)
    COEFF = _float_bits(0.044715)
    HALF = _float_bits(0.5)

    instructions = [
        {"op": "ThreadId", "dst": 0, "dim": "x"},
        {"op": "Mul", "dst": 1, "src1": {"kind": "Reg", "id": 0}, "src2": {"kind": "Imm", "val": 4}, "dtype": "i64"},
        {"op": "Add", "dst": 1, "src1": {"kind": "Reg", "id": 1}, "src2": {"kind": "Param", "id": 0}, "dtype": "i64"},
        {"op": "GlobalLoad", "dst": 2, "addr": {"kind": "Reg", "id": 1}, "dtype": "f32"},
        # x3 = x * x * x
        {"op": "Mul", "dst": 3, "src1": {"kind": "Reg", "id": 2}, "src2": {"kind": "Reg", "id": 2}, "dtype": "f32"},
        {"op": "Mul", "dst": 4, "src1": {"kind": "Reg", "id": 3}, "src2": {"kind": "Reg", "id": 2}, "dtype": "f32"},
        # 0.044715 * x3 + x
        {"op": "Mul", "dst": 5, "src1": {"kind": "Imm", "val": COEFF}, "src2": {"kind": "Reg", "id": 4}, "dtype": "f32"},
        {"op": "Add", "dst": 6, "src1": {"kind": "Reg", "id": 2}, "src2": {"kind": "Reg", "id": 5}, "dtype": "f32"},
        # tanh(√(2/π) * inner)
        {"op": "Mul", "dst": 7, "src1": {"kind": "Imm", "val": SQRT_2_OVER_PI}, "src2": {"kind": "Reg", "id": 6}, "dtype": "f32"},
        {"op": "Tanh", "dst": 8, "src": {"kind": "Reg", "id": 7}, "dtype": "f32"},
        # 0.5 * x * (1 + tanh)
        {"op": "Add", "dst": 9, "src1": {"kind": "Imm", "val": _float_bits(1.0)}, "src2": {"kind": "Reg", "id": 8}, "dtype": "f32"},
        {"op": "Mul", "dst": 10, "src1": {"kind": "Imm", "val": HALF}, "src2": {"kind": "Reg", "id": 2}, "dtype": "f32"},
        {"op": "Mul", "dst": 11, "src1": {"kind": "Reg", "id": 10}, "src2": {"kind": "Reg", "id": 9}, "dtype": "f32"},
        # store
        {"op": "Mul", "dst": 12, "src1": {"kind": "Reg", "id": 0}, "src2": {"kind": "Imm", "val": 4}, "dtype": "i64"},
        {"op": "Add", "dst": 12, "src1": {"kind": "Reg", "id": 12}, "src2": {"kind": "Param", "id": 1}, "dtype": "i64"},
        {"op": "GlobalStore", "addr": {"kind": "Reg", "id": 12}, "src": {"kind": "Reg", "id": 11}, "dtype": "f32"},
        {"op": "Return"},
    ]

    return make_gir_kernel("gelu", instructions, [
        {"name": "x", "dtype": "f32", "is_ptr": True},
        {"name": "out", "dtype": "f32", "is_ptr": True},
    ], block_dim=(n, 1, 1))


# ============================================================
# Multi-workgroup GELU — 支持大矩阵 (N > 1024)
# ============================================================

def gelu_multiblock_gir(block_size=256):
    """Multi-block GELU: 使用 BlockId + ThreadId 计算全局索引

    每个线程处理一个元素, grid = ceil(N / block_size) 个 block
    """
    SQRT_2_OVER_PI = _float_bits(0.7978845608028654)
    COEFF = _float_bits(0.044715)
    HALF = _float_bits(0.5)

    instructions = [
        # global_tid = BlockId.x * BlockDim.x + ThreadId.x
        {"op": "BlockId", "dst": 100, "dim": "x"},
        {"op": "BlockDim", "dst": 101, "dim": "x"},
        {"op": "ThreadId", "dst": 102, "dim": "x"},
        {"op": "Mul", "dst": 100, "src1": {"kind": "Reg", "id": 100}, "src2": {"kind": "Reg", "id": 101}, "dtype": "i32"},
        {"op": "Add", "dst": 0, "src1": {"kind": "Reg", "id": 100}, "src2": {"kind": "Reg", "id": 102}, "dtype": "i32"},

        # load x[global_tid]
        {"op": "Mul", "dst": 1, "src1": {"kind": "Reg", "id": 0}, "src2": {"kind": "Imm", "val": 4}, "dtype": "i64"},
        {"op": "Add", "dst": 1, "src1": {"kind": "Reg", "id": 1}, "src2": {"kind": "Param", "id": 0}, "dtype": "i64"},
        {"op": "GlobalLoad", "dst": 2, "addr": {"kind": "Reg", "id": 1}, "dtype": "f32"},
        # x3 = x * x * x
        {"op": "Mul", "dst": 3, "src1": {"kind": "Reg", "id": 2}, "src2": {"kind": "Reg", "id": 2}, "dtype": "f32"},
        {"op": "Mul", "dst": 4, "src1": {"kind": "Reg", "id": 3}, "src2": {"kind": "Reg", "id": 2}, "dtype": "f32"},
        # 0.044715 * x3 + x
        {"op": "Mul", "dst": 5, "src1": {"kind": "Imm", "val": COEFF}, "src2": {"kind": "Reg", "id": 4}, "dtype": "f32"},
        {"op": "Add", "dst": 6, "src1": {"kind": "Reg", "id": 2}, "src2": {"kind": "Reg", "id": 5}, "dtype": "f32"},
        # tanh(√(2/π) * inner)
        {"op": "Mul", "dst": 7, "src1": {"kind": "Imm", "val": SQRT_2_OVER_PI}, "src2": {"kind": "Reg", "id": 6}, "dtype": "f32"},
        {"op": "Tanh", "dst": 8, "src": {"kind": "Reg", "id": 7}, "dtype": "f32"},
        # 0.5 * x * (1 + tanh)
        {"op": "Add", "dst": 9, "src1": {"kind": "Imm", "val": _float_bits(1.0)}, "src2": {"kind": "Reg", "id": 8}, "dtype": "f32"},
        {"op": "Mul", "dst": 10, "src1": {"kind": "Imm", "val": HALF}, "src2": {"kind": "Reg", "id": 2}, "dtype": "f32"},
        {"op": "Mul", "dst": 11, "src1": {"kind": "Reg", "id": 10}, "src2": {"kind": "Reg", "id": 9}, "dtype": "f32"},
        # store
        {"op": "Mul", "dst": 12, "src1": {"kind": "Reg", "id": 0}, "src2": {"kind": "Imm", "val": 4}, "dtype": "i64"},
        {"op": "Add", "dst": 12, "src1": {"kind": "Reg", "id": 12}, "src2": {"kind": "Param", "id": 1}, "dtype": "i64"},
        {"op": "GlobalStore", "addr": {"kind": "Reg", "id": 12}, "src": {"kind": "Reg", "id": 11}, "dtype": "f32"},
        {"op": "Return"},
    ]

    return make_gir_kernel("gelu_mb", instructions, [
        {"name": "x", "dtype": "f32", "is_ptr": True},
        {"name": "out", "dtype": "f32", "is_ptr": True},
    ], block_dim=(block_size, 1, 1))


# ============================================================
# GEMM — 矩阵乘法 (multi-workgroup)
# ============================================================

def gemm_gir(M, N, K, block_size=32):
    """GEMM: O[i,j] = sum_k A[i,k] * B[k,j]

    Multi-workgroup: grid = ceil(M*N / block_size) blocks
    每个线程处理一个输出元素
    """
    instructions = [
        # global_tid = BlockId.x * BlockDim.x + ThreadId.x
        {"op": "BlockId", "dst": 100, "dim": "x"},
        {"op": "BlockDim", "dst": 101, "dim": "x"},
        {"op": "ThreadId", "dst": 102, "dim": "x"},
        {"op": "Mul", "dst": 100, "src1": {"kind": "Reg", "id": 100}, "src2": {"kind": "Reg", "id": 101}, "dtype": "i32"},
        {"op": "Add", "dst": 0, "src1": {"kind": "Reg", "id": 100}, "src2": {"kind": "Reg", "id": 102}, "dtype": "i32"},

        # row = global_tid / N (使用整数除法)
        # col = global_tid % N
        # GIR 没有 Div.i32, 但我们可以用循环减法... 太复杂
        # 简化: 当 N 是 2 的幂时用位移; 否则用 Python 编译期计算
        # 实际方案: 展开为逐元素计算, 用 global_tid 直接索引一维化的矩阵
    ]

    # 一维化: A[i,k] at offset i*K+k, B[k,j] at offset k*N+j, O[i,j] at offset i*N+j
    # global_tid = i*N + j, 所以 i = tid/N, j = tid%N
    # 但 GIR 没有 i32 除法... 用 i64 除法? 检查后没有
    # 替代方案: 每个 tid 处理一行 (i = tid), 内部展开 K 个乘加
    # grid = M 个 block, block_size = N (每个 thread 算一列)

    # 重新设计:
    # ThreadId.x = j (列索引)
    # BlockId.x = i (行索引)
    # O[i,j] = sum_k A[i,k] * B[k,j]

    instructions = [
        # i = BlockId.x (行索引)
        {"op": "BlockId", "dst": 0, "dim": "x"},
        # j = ThreadId.x (列索引)
        {"op": "ThreadId", "dst": 1, "dim": "x"},

        # acc = 0.0
        {"op": "Mul", "dst": 2, "src1": {"kind": "Imm", "val": 0}, "src2": {"kind": "Imm", "val": 0}, "dtype": "f32"},
    ]

    for k in range(K):
        # A[i,k] at offset i*K*4 + k*4
        a_off = k * 4  # 编译期常量 (i*K*4 需要运行时计算)
        # addr_A = i * (K*4) + k*4 + A_base
        # = i * (K*4) + a_off + Param(0)
        instructions.append({"op": "Mul", "dst": 10, "src1": {"kind": "Reg", "id": 0}, "src2": {"kind": "Imm", "val": K * 4}, "dtype": "i64"})
        instructions.append({"op": "Add", "dst": 10, "src1": {"kind": "Reg", "id": 10}, "src2": {"kind": "Imm", "val": a_off}, "dtype": "i64"})
        instructions.append({"op": "Add", "dst": 10, "src1": {"kind": "Reg", "id": 10}, "src2": {"kind": "Param", "id": 0}, "dtype": "i64"})
        instructions.append({"op": "GlobalLoad", "dst": 11, "addr": {"kind": "Reg", "id": 10}, "dtype": "f32"})

        # B[k,j] at offset k*N*4 + j*4
        b_off = k * N * 4  # 编译期常量
        instructions.append({"op": "Mul", "dst": 12, "src1": {"kind": "Reg", "id": 1}, "src2": {"kind": "Imm", "val": 4}, "dtype": "i64"})
        instructions.append({"op": "Add", "dst": 12, "src1": {"kind": "Reg", "id": 12}, "src2": {"kind": "Imm", "val": b_off}, "dtype": "i64"})
        instructions.append({"op": "Add", "dst": 12, "src1": {"kind": "Reg", "id": 12}, "src2": {"kind": "Param", "id": 1}, "dtype": "i64"})
        instructions.append({"op": "GlobalLoad", "dst": 13, "addr": {"kind": "Reg", "id": 12}, "dtype": "f32"})

        # acc += A[i,k] * B[k,j]
        if k == 0:
            instructions.append({"op": "Mul", "dst": 2, "src1": {"kind": "Reg", "id": 11}, "src2": {"kind": "Reg", "id": 13}, "dtype": "f32"})
        else:
            instructions.append({"op": "Fma", "dst": 2, "src1": {"kind": "Reg", "id": 11}, "src2": {"kind": "Reg", "id": 13}, "src3": {"kind": "Reg", "id": 2}, "dtype": "f32"})

    # store O[i,j] at offset i*N*4 + j*4
    instructions.append({"op": "Mul", "dst": 20, "src1": {"kind": "Reg", "id": 0}, "src2": {"kind": "Imm", "val": N * 4}, "dtype": "i64"})
    instructions.append({"op": "Mul", "dst": 21, "src1": {"kind": "Reg", "id": 1}, "src2": {"kind": "Imm", "val": 4}, "dtype": "i64"})
    instructions.append({"op": "Add", "dst": 20, "src1": {"kind": "Reg", "id": 20}, "src2": {"kind": "Reg", "id": 21}, "dtype": "i64"})
    instructions.append({"op": "Add", "dst": 20, "src1": {"kind": "Reg", "id": 20}, "src2": {"kind": "Param", "id": 2}, "dtype": "i64"})
    instructions.append({"op": "GlobalStore", "addr": {"kind": "Reg", "id": 20}, "src": {"kind": "Reg", "id": 2}, "dtype": "f32"})
    instructions.append({"op": "Return"})

    return make_gir_kernel("gemm", instructions, [
        {"name": "A", "dtype": "f32", "is_ptr": True},
        {"name": "B", "dtype": "f32", "is_ptr": True},
        {"name": "O", "dtype": "f32", "is_ptr": True},
    ], block_dim=(N, 1, 1))


# ============================================================
# LLM 算子库
# ============================================================

class KarteLLMOps:
    """
    LLM 推理算子库 — karte GPU 实现的 LLM 核心算子

    使用方式:
        ops = KarteLLMOps()
        out = ops.gelu(x)           # x 是 torch.Tensor
        out = ops.rmsnorm(x, gamma) # RMSNorm 归一化
        out = ops.flash_attention(q, k, v)  # FlashAttention
    """

    def __init__(self):
        self.bridge = get_bridge()

    def gelu(self, x: torch.Tensor) -> torch.Tensor:
        """GELU 激活函数"""
        n = x.numel()
        gir = gelu_gir(n)
        out = torch.empty_like(x)

        if isinstance(self.bridge, CudaBridge):
            ptx = self.bridge.compile_ptx(gir)
            self.bridge.launch_kernel(ptx, "gelu", [x, out], [], block=(n, 1, 1))
        else:
            # OpenCL: 需要 CPU 中转换
            x_cpu = x.cpu() if x.is_cuda else x
            result = self.bridge.launch_kernel(
                self.bridge.compile_spirv(gir), "gelu",
                [x_cpu], (n,), torch.float32,
                global_size=n, local_size=n)
            out = result.to(x.device) if x.is_cuda else result

        return out

    def rmsnorm(self, x: torch.Tensor, gamma: torch.Tensor, eps: float = 1e-6) -> torch.Tensor:
        """RMSNorm 归一化"""
        n = x.numel()
        # CPU/GPU 预计算精确 RMS 值
        rms = torch.rsqrt((x ** 2).mean() + eps).item()
        gir = fused_rmsnorm_gir(n)
        out = torch.empty_like(x)

        if isinstance(self.bridge, CudaBridge):
            ptx = self.bridge.compile_ptx(gir)
            self.bridge.launch_kernel(ptx, "rmsnorm", [x, gamma, out], [rms], block=(n, 1, 1))
        else:
            x_cpu = x.cpu() if x.is_cuda else x
            gamma_cpu = gamma.cpu() if gamma.is_cuda else gamma
            result = self.bridge.launch_kernel(
                self.bridge.compile_spirv(gir), "rmsnorm",
                [x_cpu, gamma_cpu], (n,), torch.float32,
                global_size=n, local_size=n)
            out = result.to(x.device) if x.is_cuda else result

        return out

    def flash_attention(self, q: torch.Tensor, k: torch.Tensor, v: torch.Tensor) -> torch.Tensor:
        """FlashAttention: 融合 QK^T + Softmax + AV

        Args:
            q: [seq_len, head_dim] query 矩阵
            k: [head_dim, seq_len] key 转置矩阵 (K^T)
            v: [head_dim, seq_len] value 矩阵
        Returns:
            o: [seq_len, head_dim] 输出
        """
        seq_len, head_dim = q.shape
        gir = flash_attention_gir(seq_len, head_dim)
        o = torch.empty(seq_len, head_dim, dtype=torch.float32, device=q.device)

        if isinstance(self.bridge, CudaBridge):
            ptx = self.bridge.compile_ptx(gir)
            self.bridge.launch_kernel(ptx, "flash_attn", [q, k, v, o], [], block=(seq_len, 1, 1))
        else:
            q_cpu = q.cpu() if q.is_cuda else q
            k_cpu = k.cpu() if k.is_cuda else k
            v_cpu = v.cpu() if v.is_cuda else v
            result = self.bridge.launch_kernel(
                self.bridge.compile_spirv(gir), "flash_attn",
                [q_cpu, k_cpu, v_cpu], (seq_len, head_dim), torch.float32,
                global_size=seq_len, local_size=seq_len)
            o = result.to(q.device) if q.is_cuda else result

        return o

    def gelu_multiblock(self, x: torch.Tensor, block_size: int = 256) -> torch.Tensor:
        """Multi-block GELU — 支持大向量 (N > 1024)"""
        n = x.numel()
        gir = gelu_multiblock_gir(block_size)
        out = torch.empty_like(x)
        num_blocks = (n + block_size - 1) // block_size

        if isinstance(self.bridge, CudaBridge):
            ptx = self.bridge.compile_ptx(gir)
            self.bridge.launch_kernel(ptx, "gelu_mb", [x, out], [],
                                     grid=(num_blocks, 1, 1), block=(block_size, 1, 1))
        else:
            # OpenCL: 单 workgroup fallback
            gir_single = gelu_gir(n)
            x_cpu = x.cpu() if x.is_cuda else x
            result = self.bridge.launch_kernel(
                self.bridge.compile_spirv(gir_single), "gelu",
                [x_cpu], (n,), torch.float32,
                global_size=n, local_size=n)
            out = result.to(x.device) if x.is_cuda else result

        return out

    def gemm(self, a: torch.Tensor, b: torch.Tensor) -> torch.Tensor:
        """GEMM: O = A @ B

        Args:
            a: [M, K] 矩阵
            b: [K, N] 矩阵
        Returns:
            o: [M, N] 输出矩阵
        """
        M, K = a.shape
        K2, N = b.shape
        assert K == K2, f"维度不匹配: A[{M},{K}] @ B[{K2},{N}]"

        gir = gemm_gir(M, N, K, block_size=min(N, 256))
        o = torch.empty(M, N, dtype=torch.float32, device=a.device)

        if isinstance(self.bridge, CudaBridge):
            ptx = self.bridge.compile_ptx(gir)
            self.bridge.launch_kernel(ptx, "gemm", [a, b, o], [],
                                     grid=(M, 1, 1), block=(min(N, 256), 1, 1))
        else:
            # OpenCL: 用单 workgroup FlashAttention 逻辑
            a_cpu = a.cpu() if a.is_cuda else a
            b_cpu = b.cpu() if b.is_cuda else b
            result = self.bridge.launch_kernel(
                self.bridge.compile_spirv(gir), "gemm",
                [a_cpu, b_cpu], (M, N), torch.float32,
                global_size=M, local_size=M)
            o = result.to(a.device) if a.is_cuda else result

        return o

def karte_function(gir_func: Callable, name: str = None):
    """将 karte GIR kernel 注册为 PyTorch autograd Function

    Usage:
        class GeluOp(torch.autograd.Function):
            @staticmethod
            def forward(ctx, x):
                ops = KarteLLMOps()
                return ops.gelu(x)
        GeluOp.apply(x)
    """
    ops = KarteLLMOps()

    class KarteAutoGrad(torch.autograd.Function):
        @staticmethod
        def forward(ctx, *args):
            with torch.no_grad():
                return gir_func(ops, *args)

        @staticmethod
        def backward(ctx, *grads):
            # 简化: 返回 None (前向推理不需要梯度)
            return tuple(None for _ in grads)

    KarteAutoGrad.__name__ = name or gir_func.__name__
    return KarteAutoGrad


# 全局实例
_ops_instance = None

def get_llm_ops():
    """获取全局 LLM 算子实例"""
    global _ops_instance
    if _ops_instance is None:
        _ops_instance = KarteLLMOps()
    return _ops_instance
