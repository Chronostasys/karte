"""
karte_ops.py — Karte GPU 算子 Python 绑定

用户使用流程:
  1. 用 Karte 语言编写 kernel (.karte 文件)
  2. karte gpu-compile 编译为 PTX
  3. 通过 karte_ops.load() 加载 PTX
  4. 在 PyTorch 训练脚本中像调用函数一样使用

示例:
    import karte_ops
    import torch

    # 加载 Karte 编译的 kernel
    karte_ops.load("body_rot_reward.ptx")

    # 在训练循环中使用
    reward = karte_ops.body_rot_reward(body_rot_tensor, ref_rot_tensor, sigma=0.25)
"""
import ctypes
import ctypes.util
import numpy as np
import torch
import os
import subprocess
import tempfile

# ============================================================
# CUDA Driver API 绑定
# ============================================================

_libcuda = None
_ctx = None
_initialized = False

def _init_cuda():
    global _libcuda, _ctx, _initialized
    if _initialized:
        return
    _libcuda = ctypes.CDLL(ctypes.util.find_library('cuda'))
    _libcuda.cuInit(0)

    # 函数原型
    _libcuda.cuMemAlloc_v2.argtypes = [ctypes.POINTER(ctypes.c_uint64), ctypes.c_size_t]
    _libcuda.cuMemAlloc_v2.restype = ctypes.c_int
    _libcuda.cuMemcpyHtoD_v2.argtypes = [ctypes.c_uint64, ctypes.c_void_p, ctypes.c_size_t]
    _libcuda.cuMemcpyHtoD_v2.restype = ctypes.c_int
    _libcuda.cuMemcpyDtoH_v2.argtypes = [ctypes.c_void_p, ctypes.c_uint64, ctypes.c_size_t]
    _libcuda.cuMemcpyDtoH_v2.restype = ctypes.c_int
    _libcuda.cuLaunchKernel.argtypes = (
        [ctypes.c_void_p] + [ctypes.c_uint] * 7 + [ctypes.c_void_p] * 3
    )
    _libcuda.cuLaunchKernel.restype = ctypes.c_int

    # 绑定到 PyTorch 的 CUDA context
    _ = torch.cuda.device_count()
    _ctx = ctypes.c_void_p()
    _libcuda.cuDevicePrimaryCtxRetain(ctypes.byref(_ctx), 0)
    _libcuda.cuCtxSetCurrent(_ctx)
    _initialized = True


# ============================================================
# 核心 API
# ============================================================

class KarteKernel:
    """一个已加载的 Karte GPU kernel"""

    def __init__(self, module, func, name, param_specs):
        """
        param_specs: [(name, dtype, is_ptr), ...]
        dtype: 'f32', 'i32', 'u64'
        """
        self._module = module
        self._func = func
        self._name = name
        self._param_specs = param_specs
        self._block_size = 256

    def __call__(self, *args, block_size=256):
        """
        调用 kernel。

        参数对应 kernel 声明的参数列表。
        张量参数自动获取 GPU 设备指针。
        标量参数自动转换。

        返回值: 如果 kernel 有输出张量参数，返回该张量。
        """
        _init_cuda()
        cuda = _libcuda

        assert len(args) == len(self._param_specs), \
            f"kernel '{self._name}' 期望 {len(self._param_specs)} 个参数，得到 {len(args)}"

        # 构建参数数组
        raw_args = []
        output_tensor = None

        for i, (spec_name, spec_dtype, is_ptr) in enumerate(self._param_specs):
            arg = args[i]

            if is_ptr:
                # 张量参数 — 获取 GPU 设备指针
                if isinstance(arg, torch.Tensor):
                    # 确保在 GPU 上 + 连续 + 正确 dtype
                    if arg.dtype == torch.float32 and spec_dtype == 'f32':
                        pass
                    elif arg.dtype == torch.int32 and spec_dtype == 'i32':
                        pass
                    elif spec_dtype in ('f32', 'i32'):
                        arg = arg.to(getattr(torch, spec_dtype))
                    # u64 等指针类型不需要转换

                    if not arg.is_contiguous():
                        arg = arg.contiguous()

                    if not arg.is_cuda:
                        arg = arg.cuda()

                    raw_args.append(arg.data_ptr())
                elif isinstance(arg, int):
                    raw_args.append(arg)
                else:
                    raise TypeError(f"参数 {spec_name}: 期望 Tensor 或 int，得到 {type(arg)}")

                # 如果参数名以 'out' 开头，记录为输出
                if spec_name.startswith('out') and output_tensor is None:
                    output_tensor = arg
            else:
                # 标量参数
                if spec_dtype == 'f32':
                    fval = np.float32(arg)
                    raw_args.append(int(fval.view(np.uint32)))
                else:
                    raw_args.append(int(arg))

        # 计算 grid/block
        # 根据第一个张量参数推断 batch size
        batch_size = 1
        for i, (_, _, is_ptr) in enumerate(self._param_specs):
            if is_ptr and isinstance(args[i], torch.Tensor):
                # 简单策略: 每个 thread 处理 batch 中的一个元素
                if len(args[i].shape) > 1:
                    batch_size = args[i].shape[0]
                    break
                elif 'out' in self._param_specs[i][0]:
                    batch_size = args[i].shape[0]
                    break

        grid = (batch_size + block_size - 1) // block_size

        # 准备参数指针数组
        n = len(raw_args)
        buf = (ctypes.c_uint64 * n)(*raw_args)
        ptrs = (ctypes.c_void_p * n)()
        base = ctypes.addressof(buf)
        for j in range(n):
            ptrs[j] = base + j * 8

        # 启动 kernel
        ret = cuda.cuLaunchKernel(
            self._func, grid, 1, 1, block_size, 1, 1,
            0, None, ptrs, None
        )
        assert ret == 0, f"cuLaunchKernel failed: {ret}"
        cuda.cuCtxSynchronize()

        return output_tensor if output_tensor is not None else None


_loaded_kernels = {}

def load(ptx_path: str):
    """
    加载 Karte 编译生成的 PTX 文件。

    用法:
        karte_ops.load("body_rot_reward.ptx")
        reward = karte_ops.body_rot_reward(body_rot, ref_rot, sigma=0.25)
    """
    _init_cuda()
    cuda = _libcuda

    with open(ptx_path, 'r') as f:
        ptx_text = f.read()

    module = ctypes.c_void_p()
    ret = cuda.cuModuleLoadData(ctypes.byref(module), ptx_text.strip().encode() + b'\x00')
    if ret != 0:
        raise RuntimeError(f"加载 PTX 失败 (code={ret}): {ptx_path}")

    # 解析 PTX 提取 kernel 名称和参数信息
    kernels = _parse_ptx_kernels(ptx_text)

    for name, params in kernels.items():
        func = ctypes.c_void_p()
        ret = cuda.cuModuleGetFunction(ctypes.byref(func), module, name.encode())
        if ret != 0:
            continue
        kernel = KarteKernel(module, func, name, params)
        _loaded_kernels[name] = kernel
        globals()[name] = kernel


def compile_and_load(karte_source: str, karte_binary: str = None):
    """
    从 Karte 源码编译并加载。

    用法:
        karte_ops.compile_and_load("my_kernel.karte")
        result = karte_ops.my_kernel(input_tensor)
    """
    karte_bin = karte_binary or _find_karte_binary()
    if karte_bin is None:
        raise RuntimeError("找不到 karte 可执行文件，请设置 KARTE_BIN 环境变量或指定路径")

    # 编译为 PTX
    with tempfile.NamedTemporaryFile(suffix='.ptx', delete=False) as f:
        ptx_path = f.name

    result = subprocess.run(
        [karte_bin, 'gpu-compile', karte_source, '-o', ptx_path],
        capture_output=True, text=True
    )
    if result.returncode != 0:
        raise RuntimeError(f"Karte 编译失败:\n{result.stderr}")

    load(ptx_path)
    os.unlink(ptx_path)


def _find_karte_binary() -> str:
    """查找 karte 可执行文件"""
    # 1. 环境变量
    env = os.environ.get('KARTE_BIN')
    if env and os.path.exists(env):
        return env

    # 2. 项目 target 目录
    candidates = [
        os.path.expanduser('~/src/karte/target/debug/karte'),
        os.path.expanduser('~/src/karte/target/release/karte'),
        'karte',
    ]
    for c in candidates:
        try:
            subprocess.run([c, '--help'], capture_output=True, timeout=5)
            return c
        except Exception:
            continue

    return None


def _parse_ptx_kernels(ptx_text: str):
    """
    从 PTX 文本解析 kernel 名称和参数信息。

    返回: {kernel_name: [(param_name, dtype, is_ptr), ...]}
    """
    import re
    kernels = {}

    # 匹配 .entry kernel_name(.param ...)
    entry_pattern = r'\.entry\s+(\w+)\s*\(([^)]*)\)'
    for match in re.finditer(entry_pattern, ptx_text):
        name = match.group(1)
        params_str = match.group(2).strip()
        params = []

        if params_str:
            for i, param_decl in enumerate(params_str.split(',')):
                param_decl = param_decl.strip()
                # 解析 .param .u64 name 或 .param .f32 name 或 .param .s32 name
                # 参数名可能是 %param_N 格式
                is_ptr = '.u64' in param_decl or '.b64' in param_decl
                if '.f32' in param_decl:
                    dtype = 'f32'
                elif '.s32' in param_decl or '.u32' in param_decl:
                    dtype = 'i32'
                elif '.u64' in param_decl or '.b64' in param_decl:
                    dtype = 'u64'
                    is_ptr = True
                else:
                    dtype = 'f32'

                param_name = f'param_{i}'
                if 'out' in name.lower() and is_ptr and i >= 2:
                    param_name = f'out_{i}'

                params.append((param_name, dtype, is_ptr))

        kernels[name] = params

    return kernels


def available_kernels():
    """列出已加载的 kernel"""
    return list(_loaded_kernels.keys())
