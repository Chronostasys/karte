"""
karte_jit.py — Karte JIT 编译器 Python 绑定

比 Triton 更简洁的 GPU 算子开发体验：

    import karte

    @karte.jit
    def body_rot_reward(
        body_rot: karte.Tensor["N", 14, 4],
        ref_rot:  karte.Tensor["N", 14, 4],
        sigma: float = 0.25,
    ) -> karte.Tensor["N"]:
        tid = karte.thread_id()
        total = karte.f32(0.0)
        for j in karte.unroll(14):
            b = body_rot[tid, j]      # 自动 v4 向量化加载
            r = ref_rot[tid, j]
            total += 8.0 * (1.0 - karte.dot(b, r))
        return karte.exp(-sigma * total / 14.0)

    # 调用——和普通函数一模一样
    reward = body_rot_reward(body_tensor, ref_tensor, 0.25)

与 Triton 对比的优势:
1. 返回值替代 out_ptr  —— return 就行，不用 tl.store
2. 张量索引替代指针运算 —— body[tid, j] 替代 tl.load(ptr+offset)
3. 自动 grid/block     —— 根据返回张量形状自动计算
4. 类型标注             —— karte.Tensor["N",14,4] 编译期可知形状
5. 高级操作             —— karte.dot() / karte.exp() / karte.sqrt()
"""

import ctypes
import ctypes.util
import numpy as np
import torch
import hashlib
import os
import sys
import functools
import inspect
import textwrap
import re
import json
import subprocess
import time

# ============================================================
# Autotuning 和动态形状缓存配置
# ============================================================

_autotune_flag = os.environ.get('KARTE_AUTOTUNE', '1') == '1'

_BLOCK_SIZE_CANDIDATES = [64, 128, 256, 512, 1024]

_interpret_mode = os.environ.get('KARTE_INTERPRET', '0') == '1'


def is_interpret_mode():
    """是否在 CPU 解释模式"""
    return _interpret_mode

# ============================================================
# GPU 后端检测与初始化
# ============================================================

_backend = None

def _detect_backend():
    """自动检测可用 GPU 后端: cuda / opencl / cpu"""
    global _backend
    if _backend is not None:
        return _backend
    # 1. 尝试 CUDA (NVIDIA)
    try:
        if ctypes.util.find_library('cuda'):
            torch.cuda.device_count()  # 确认可用
            _backend = 'cuda'
            return _backend
    except Exception:
        pass
    # 2. 尝试 OpenCL (AMD / Intel)
    if ctypes.util.find_library('OpenCL') or os.path.exists('/opt/rocm/opencl/lib/libOpenCL.so'):
        _backend = 'opencl'
        return _backend
    # 3. 回退 CPU
    _backend = 'cpu'
    return _backend

def get_backend():
    """获取当前 GPU 后端"""
    if _backend is None:
        return _detect_backend()
    return _backend

# ============================================================
# CUDA Driver 初始化
# ============================================================

_cuda = None

def _ensure_cuda():
    global _cuda
    if _cuda is not None:
        return _cuda
    _cuda = ctypes.CDLL(ctypes.util.find_library('cuda'))
    _cuda.cuInit(0)
    _cuda.cuMemAlloc_v2.argtypes = [ctypes.POINTER(ctypes.c_uint64), ctypes.c_size_t]
    _cuda.cuMemAlloc_v2.restype = ctypes.c_int
    _cuda.cuMemcpyHtoD_v2.argtypes = [ctypes.c_uint64, ctypes.c_void_p, ctypes.c_size_t]
    _cuda.cuMemcpyHtoD_v2.restype = ctypes.c_int
    _cuda.cuMemcpyDtoH_v2.argtypes = [ctypes.c_void_p, ctypes.c_uint64, ctypes.c_size_t]
    _cuda.cuMemcpyDtoH_v2.restype = ctypes.c_int
    _cuda.cuLaunchKernel.argtypes = (
        [ctypes.c_void_p] + [ctypes.c_uint] * 7 + [ctypes.c_void_p] * 3
    )
    _cuda.cuLaunchKernel.restype = ctypes.c_int
    # 不抢 context — 使用 PyTorch/Isaac Gym 已创建的 current context
    # cuDevicePrimaryCtxRetain 会抢占 Isaac Gym 的 context 导致 SIGSEGV
    _ = torch.cuda.device_count()  # 确保 PyTorch CUDA 已初始化
    return _cuda


# ============================================================
# OpenCL 初始化 (AMD / Intel GPU)
# ============================================================

_opencl = None

def _ensure_opencl():
    """初始化 OpenCL 运行时，返回 (cl_lib, platform, device, context, queue)"""
    global _opencl
    if _opencl is not None:
        return _opencl

    # 尝试加载 OpenCL 库
    cl_lib = None
    for path in ['OpenCL', 'libOpenCL.so.1', 'libOpenCL.so',
                 '/opt/rocm/opencl/lib/libOpenCL.so']:
        try:
            cl_lib = ctypes.CDLL(path)
            break
        except OSError:
            continue
    if cl_lib is None:
        raise RuntimeError("OpenCL 库未找到 — 请安装 ROCm (AMD) 或 Intel OpenCL runtime")

    # OpenCL API 签名设置
    cl_lib.clGetPlatformIDs.argtypes = [
        ctypes.c_uint, ctypes.POINTER(ctypes.c_void_p), ctypes.POINTER(ctypes.c_uint)
    ]
    cl_lib.clGetPlatformIDs.restype = ctypes.c_int

    cl_lib.clGetDeviceIDs.argtypes = [
        ctypes.c_void_p, ctypes.c_uint64, ctypes.c_uint,
        ctypes.POINTER(ctypes.c_void_p), ctypes.POINTER(ctypes.c_uint)
    ]
    cl_lib.clGetDeviceIDs.restype = ctypes.c_int

    cl_lib.clCreateContext.argtypes = [
        ctypes.POINTER(ctypes.c_void_p), ctypes.c_uint,
        ctypes.POINTER(ctypes.c_void_p), ctypes.c_void_p, ctypes.c_void_p,
        ctypes.POINTER(ctypes.c_int)
    ]
    cl_lib.clCreateContext.restype = ctypes.c_void_p

    cl_lib.clCreateCommandQueueWithProperties.argtypes = [
        ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.POINTER(ctypes.c_int)
    ]
    cl_lib.clCreateCommandQueueWithProperties.restype = ctypes.c_void_p

    cl_lib.clCreateBuffer.argtypes = [
        ctypes.c_void_p, ctypes.c_uint, ctypes.c_size_t, ctypes.c_void_p,
        ctypes.POINTER(ctypes.c_int)
    ]
    cl_lib.clCreateBuffer.restype = ctypes.c_void_p

    cl_lib.clEnqueueWriteBuffer.argtypes = [
        ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint, ctypes.c_size_t,
        ctypes.c_size_t, ctypes.c_void_p, ctypes.c_uint, ctypes.c_void_p, ctypes.c_void_p
    ]
    cl_lib.clEnqueueWriteBuffer.restype = ctypes.c_int

    cl_lib.clEnqueueReadBuffer.argtypes = [
        ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint, ctypes.c_size_t,
        ctypes.c_size_t, ctypes.c_void_p, ctypes.c_uint, ctypes.c_void_p, ctypes.c_void_p
    ]
    cl_lib.clEnqueueReadBuffer.restype = ctypes.c_int

    cl_lib.clCreateProgramWithSource.argtypes = [
        ctypes.c_void_p, ctypes.c_uint,
        ctypes.POINTER(ctypes.c_char_p), ctypes.POINTER(ctypes.c_size_t),
        ctypes.POINTER(ctypes.c_int)
    ]
    cl_lib.clCreateProgramWithSource.restype = ctypes.c_void_p

    cl_lib.clBuildProgram.argtypes = [
        ctypes.c_void_p, ctypes.c_uint, ctypes.POINTER(ctypes.c_void_p),
        ctypes.c_char_p, ctypes.c_void_p, ctypes.c_void_p
    ]
    cl_lib.clBuildProgram.restype = ctypes.c_int

    cl_lib.clCreateKernel.argtypes = [
        ctypes.c_void_p, ctypes.c_char_p, ctypes.POINTER(ctypes.c_int)
    ]
    cl_lib.clCreateKernel.restype = ctypes.c_void_p

    cl_lib.clSetKernelArg.argtypes = [
        ctypes.c_void_p, ctypes.c_uint, ctypes.c_size_t, ctypes.c_void_p
    ]
    cl_lib.clSetKernelArg.restype = ctypes.c_int

    cl_lib.clEnqueueNDRangeKernel.argtypes = [
        ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint,
        ctypes.POINTER(ctypes.c_size_t), ctypes.POINTER(ctypes.c_size_t),
        ctypes.POINTER(ctypes.c_size_t), ctypes.c_uint, ctypes.c_void_p, ctypes.c_void_p
    ]
    cl_lib.clEnqueueNDRangeKernel.restype = ctypes.c_int

    cl_lib.clFinish.argtypes = [ctypes.c_void_p]
    cl_lib.clFinish.restype = ctypes.c_int

    cl_lib.clReleaseMemObject.argtypes = [ctypes.c_void_p]
    cl_lib.clReleaseMemObject.restype = ctypes.c_int

    cl_lib.clGetProgramBuildInfo.argtypes = [
        ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint, ctypes.c_size_t,
        ctypes.c_void_p, ctypes.POINTER(ctypes.c_size_t)
    ]
    cl_lib.clGetProgramBuildInfo.restype = ctypes.c_int

    # 获取平台和设备
    num_platforms = ctypes.c_uint(0)
    ret = cl_lib.clGetPlatformIDs(0, None, ctypes.byref(num_platforms))
    if ret != 0 or num_platforms.value == 0:
        raise RuntimeError(f"未找到 OpenCL 平台 (err={ret})")

    platforms = (ctypes.c_void_p * num_platforms.value)()
    cl_lib.clGetPlatformIDs(num_platforms.value, platforms, None)

    # 优先选择 GPU 设备
    CL_DEVICE_TYPE_GPU = 1 << 2
    platform = None
    device = None

    for p in platforms:
        num_devices = ctypes.c_uint(0)
        ret = cl_lib.clGetDeviceIDs(
            p, CL_DEVICE_TYPE_GPU, 0, None, ctypes.byref(num_devices))
        if ret == 0 and num_devices.value > 0:
            devices = (ctypes.c_void_p * num_devices.value)()
            cl_lib.clGetDeviceIDs(
                p, CL_DEVICE_TYPE_GPU, num_devices.value, devices, None)
            platform = p
            device = devices[0]
            break

    if platform is None or device is None:
        raise RuntimeError("未找到 OpenCL GPU 设备")

    # 创建上下文
    err = ctypes.c_int(0)
    context = cl_lib.clCreateContext(None, 1, ctypes.byref(ctypes.c_void_p(device)),
                                     None, None, ctypes.byref(err))
    if err.value != 0:
        raise RuntimeError(f"clCreateContext 失败 (err={err.value})")

    # 创建命令队列
    queue = cl_lib.clCreateCommandQueueWithProperties(
        context, device, None, ctypes.byref(err))
    if err.value != 0:
        raise RuntimeError(f"clCreateCommandQueueWithProperties 失败 (err={err.value})")

    _opencl = {
        'lib': cl_lib,
        'platform': platform,
        'device': device,
        'context': context,
        'queue': queue,
    }
    return _opencl

def _operand_to_json(val):
    """将 Python 值转换为 GIR operand JSON"""
    if isinstance(val, _SymF32):
        return {"kind": "Reg", "id": val.reg}
    elif isinstance(val, float):
        return {"kind": "Imm", "val": int(np.float32(val).view(np.uint32))}
    elif isinstance(val, int):
        return {"kind": "Imm", "val": val}
    elif isinstance(val, dict):
        return val  # 已经是 JSON 格式
    else:
        return {"kind": "Imm", "val": 0}


def _get_sm_version_str():
    """获取当前 GPU 的 SM 版本（如 '80', '120'）"""
    try:
        cap = torch.cuda.get_device_capability(0)
        return f"{cap[0]}{cap[1]}"
    except Exception:
        return "80"


def _find_karte_binary():
    """查找 karte 二进制文件路径"""
    # 1. 环境变量优先
    karte_bin = os.environ.get('KARTE_BIN')
    if karte_bin:
        return karte_bin

    # 2. pip 包内嵌二进制（karte_gpu/bin/karte）
    this_dir = os.path.dirname(os.path.abspath(__file__))
    bundled = os.path.join(this_dir, 'bin', 'karte')
    if os.path.exists(bundled):
        return bundled

    # 3. 源码仓库的 target 目录
    repo_root = os.path.normpath(os.path.join(this_dir, '..', '..'))
    for c in [
        os.path.join(repo_root, 'target', 'release', 'karte'),
        os.path.join(repo_root, 'target', 'debug', 'karte'),
    ]:
        if os.path.exists(c):
            return c

    # 4. 回退：尝试 PATH 中的 karte
    return 'karte'


# ============================================================
# 符号类型 — tracer 用于记录 GIR 指令
# ============================================================

class _GirBuilder:
    """GIR 指令构建器 — 记录 kernel 的所有操作为 JSON dict"""

    def __init__(self):
        self.gir_instrs = []    # GIR 指令 dict 列表
        self.next_reg = 0       # 寄存器分配计数器
        self.next_label = 0
        self.params = []        # 参数信息: [(name, ptype, is_ptr, shape)]
        self.return_regs = None # 返回值寄存器列表
        self._tid_reg = 0       # 全局线程 ID 寄存器
        self.output_param_idx = None  # 输出参数索引

    def alloc_reg(self):
        r = self.next_reg
        self.next_reg += 1
        return r

    def alloc_regs(self, n):
        """分配 n 个连续寄存器，返回起始 ID。
        用于向量化操作（如 GlobalLoadV4 需要 4 个连续寄存器）。"""
        r = self.next_reg
        self.next_reg += n
        return r

    def emit_gir(self, instr_dict):
        """记录一条 GIR 指令"""
        self.gir_instrs.append(instr_dict)

    def alloc_label(self):
        l = self.next_label
        self.next_label += 1
        return l


class _SymF32:
    """符号 f32 值 — 对应一个虚拟寄存器"""

    __slots__ = ['reg', '_builder']

    def __init__(self, reg, builder):
        self.reg = reg
        self._builder = builder

    def __add__(self, other):
        r = self._builder.alloc_reg()
        self._builder.emit_gir({
            "op": "Add",
            "dst": r,
            "src1": _operand_to_json(self),
            "src2": _operand_to_json(other),
            "dtype": "f32"
        })
        return _SymF32(r, self._builder)

    def __radd__(self, other):
        return self.__add__(other)

    def __sub__(self, other):
        r = self._builder.alloc_reg()
        self._builder.emit_gir({
            "op": "Sub",
            "dst": r,
            "src1": _operand_to_json(self),
            "src2": _operand_to_json(other),
            "dtype": "f32"
        })
        return _SymF32(r, self._builder)

    def __rsub__(self, other):
        """other - self"""
        r = self._builder.alloc_reg()
        self._builder.emit_gir({
            "op": "Sub",
            "dst": r,
            "src1": _operand_to_json(other),
            "src2": _operand_to_json(self),
            "dtype": "f32"
        })
        return _SymF32(r, self._builder)

    def __mul__(self, other):
        r = self._builder.alloc_reg()
        self._builder.emit_gir({
            "op": "Mul",
            "dst": r,
            "src1": _operand_to_json(self),
            "src2": _operand_to_json(other),
            "dtype": "f32"
        })
        return _SymF32(r, self._builder)

    def __rmul__(self, other):
        return self.__mul__(other)

    def __rtruediv__(self, other):
        """other / self"""
        r = self._builder.alloc_reg()
        self._builder.emit_gir({
            "op": "Div",
            "dst": r,
            "src1": _operand_to_json(other),
            "src2": _operand_to_json(self),
            "dtype": "f32"
        })
        return _SymF32(r, self._builder)

    def __radd__(self, other):
        return self.__add__(other)

    def __neg__(self):
        r = self._builder.alloc_reg()
        self._builder.emit_gir({
            "op": "Sub",
            "dst": r,
            "src1": {"kind": "Imm", "val": 0},
            "src2": _operand_to_json(self),
            "dtype": "f32"
        })
        return _SymF32(r, self._builder)

    def __truediv__(self, other):
        r = self._builder.alloc_reg()
        self._builder.emit_gir({
            "op": "Div",
            "dst": r,
            "src1": _operand_to_json(self),
            "src2": _operand_to_json(other),
            "dtype": "f32"
        })
        return _SymF32(r, self._builder)

    def __lt__(self, other):
        """比较运算: 返回 _SymF32 (0.0 或 1.0) 供 karte.where() 使用"""
        r = self._builder.alloc_reg()
        self._builder.emit_gir({
            "op": "Cmp",
            "dst": r,
            "cmp": "lt",
            "src1": _operand_to_json(self),
            "src2": _operand_to_json(other),
            "dtype": "f32"
        })
        return _SymF32(r, self._builder)

    def __gt__(self, other):
        r = self._builder.alloc_reg()
        self._builder.emit_gir({"op": "Cmp", "dst": r, "cmp": "gt",
            "src1": _operand_to_json(self), "src2": _operand_to_json(other), "dtype": "f32"})
        return _SymF32(r, self._builder)

    def __le__(self, other):
        r = self._builder.alloc_reg()
        self._builder.emit_gir({"op": "Cmp", "dst": r, "cmp": "le",
            "src1": _operand_to_json(self), "src2": _operand_to_json(other), "dtype": "f32"})
        return _SymF32(r, self._builder)

    def __ge__(self, other):
        r = self._builder.alloc_reg()
        self._builder.emit_gir({"op": "Cmp", "dst": r, "cmp": "ge",
            "src1": _operand_to_json(self), "src2": _operand_to_json(other), "dtype": "f32"})
        return _SymF32(r, self._builder)


class _SymTensor:
    """符号张量 — 索引操作自动生成 GPU 加载指令"""

    __slots__ = ['_builder', '_param_idx', '_name', '_shape']

    def __init__(self, builder, param_idx, name, shape):
        self._builder = builder
        self._param_idx = param_idx
        self._name = name
        self._shape = shape

    def __getitem__(self, indices):
        """
        张量索引 — 支持:
          tensor[tid, j]      → 加载 shape[-1] 个连续 f32 (自动 v4)
          tensor[tid, j, 0]   → 加载单个 f32
        """
        if not isinstance(indices, tuple):
            indices = (indices,)

        builder = self._builder

        # 判断加载宽度
        # 如果只索引到倒数第二维，最后一维全部加载（v4 或标量）
        last_dim = self._shape[-1]
        full_load = len(indices) == len(self._shape) - 1

        if full_load and last_dim == 4:
            # 向量化加载 4×f32
            addr_reg = self._compute_addr(indices)
            base = builder.alloc_regs(4)
            regs = [base, base + 1, base + 2, base + 3]
            builder.emit_gir({
                "op": "GlobalLoadV4",
                "dst_base": base,
                "addr": {"kind": "Reg", "id": addr_reg},
                "dtype": "f32"
            })
            return tuple(_SymF32(r, builder) for r in regs)

        elif full_load and last_dim == 1:
            addr_reg = self._compute_addr(indices)
            r = builder.alloc_reg()
            builder.emit_gir({
                "op": "GlobalLoad",
                "dst": r,
                "addr": {"kind": "Reg", "id": addr_reg},
                "dtype": "f32"
            })
            return _SymF32(r, builder)

        else:
            # 标量加载
            addr_reg = self._compute_addr(indices)
            r = builder.alloc_reg()
            builder.emit_gir({
                "op": "GlobalLoad",
                "dst": r,
                "addr": {"kind": "Reg", "id": addr_reg},
                "dtype": "f32"
            })
            return _SymF32(r, builder)

    def _compute_addr(self, indices):
        """计算元素的字节偏移地址，产出 GIR 指令"""
        builder = self._builder
        shape = self._shape

        # 获取 tid 的寄存器
        tid_reg = builder._tid_reg

        if len(indices) == 1:
            # tensor[tid] 或 tensor[k] — 单维索引
            idx = indices[0]
            stride = 1
            for d in shape[1:]:
                if isinstance(d, (int, float)):
                    stride *= int(d)
            byte_stride = stride * 4
            # addr = param_ptr + idx * byte_stride
            addr = builder.alloc_reg()
            if isinstance(idx, _RegRef):
                # 索引是 tid 寄存器引用
                builder.emit_gir({
                    "op": "Mul", "dst": addr,
                    "src1": {"kind": "Reg", "id": idx.reg},
                    "src2": {"kind": "Imm", "val": byte_stride},
                    "dtype": "i64"
                })
            elif isinstance(idx, int):
                # 索引是字面常量（如循环变量 k=0, k=1 等）
                byte_offset = idx * stride * 4
                builder.emit_gir({
                    "op": "Mul", "dst": addr,
                    "src1": {"kind": "Imm", "val": byte_offset},
                    "src2": {"kind": "Imm", "val": 1},
                    "dtype": "i64"
                })
            else:
                builder.emit_gir({
                    "op": "Mul", "dst": addr,
                    "src1": {"kind": "Reg", "id": tid_reg},
                    "src2": {"kind": "Imm", "val": byte_stride},
                    "dtype": "i64"
                })
            builder.emit_gir({
                "op": "Add", "dst": addr,
                "src1": {"kind": "Reg", "id": addr},
                "src2": {"kind": "Param", "id": self._param_idx},
                "dtype": "i64"
            })
            return addr

        elif len(indices) == 2:
            # tensor[tid, j] — batch * n_bodies * last_dim
            n_bodies = shape[1]
            last_dim = shape[-1] if len(shape) > 2 else 1
            elem_per_row = n_bodies * last_dim

            # j 可能是 int（编译期已知）或 _SymF32（动态）
            if isinstance(indices[1], int):
                j_val = indices[1]
                # base_idx = tid * elem_per_row + j * last_dim
                base = builder.alloc_reg()
                builder.emit_gir({
                    "op": "Mul", "dst": base,
                    "src1": {"kind": "Reg", "id": tid_reg},
                    "src2": {"kind": "Imm", "val": elem_per_row},
                    "dtype": "i32"
                })
                builder.emit_gir({
                    "op": "Add", "dst": base,
                    "src1": {"kind": "Reg", "id": base},
                    "src2": {"kind": "Imm", "val": j_val * last_dim},
                    "dtype": "i32"
                })
            elif isinstance(indices[1], _SymF32):
                j_reg = indices[1].reg
                base = builder.alloc_reg()
                builder.emit_gir({
                    "op": "Mul", "dst": base,
                    "src1": {"kind": "Reg", "id": tid_reg},
                    "src2": {"kind": "Imm", "val": elem_per_row},
                    "dtype": "i32"
                })
                row_off = builder.alloc_reg()
                builder.emit_gir({
                    "op": "Mul", "dst": row_off,
                    "src1": {"kind": "Reg", "id": j_reg},
                    "src2": {"kind": "Imm", "val": last_dim},
                    "dtype": "i32"
                })
                builder.emit_gir({
                    "op": "Add", "dst": base,
                    "src1": {"kind": "Reg", "id": base},
                    "src2": {"kind": "Reg", "id": row_off},
                    "dtype": "i32"
                })
            else:
                raise TypeError(f"索引类型 {type(indices[1])} 不支持")

            # byte_offset = base * 4 + param_ptr
            addr = builder.alloc_reg()
            builder.emit_gir({
                "op": "Mul", "dst": addr,
                "src1": {"kind": "Reg", "id": base},
                "src2": {"kind": "Imm", "val": 4},
                "dtype": "i64"
            })
            builder.emit_gir({
                "op": "Add", "dst": addr,
                "src1": {"kind": "Reg", "id": addr},
                "src2": {"kind": "Param", "id": self._param_idx},
                "dtype": "i64"
            })
            return addr

        elif len(indices) == 3:
            # tensor[tid, j, k]
            n_bodies = shape[1]
            last_dim = shape[2]
            elem_per_row = n_bodies * last_dim

            j_val = indices[1] if isinstance(indices[1], int) else None
            k_val = indices[2] if isinstance(indices[2], int) else None

            base = builder.alloc_reg()
            builder.emit_gir({
                "op": "Mul", "dst": base,
                "src1": {"kind": "Reg", "id": tid_reg},
                "src2": {"kind": "Imm", "val": elem_per_row},
                "dtype": "i32"
            })

            if j_val is not None:
                j_off = j_val * last_dim
                builder.emit_gir({
                    "op": "Add", "dst": base,
                    "src1": {"kind": "Reg", "id": base},
                    "src2": {"kind": "Imm", "val": j_off},
                    "dtype": "i32"
                })
            if k_val is not None:
                builder.emit_gir({
                    "op": "Add", "dst": base,
                    "src1": {"kind": "Reg", "id": base},
                    "src2": {"kind": "Imm", "val": k_val},
                    "dtype": "i32"
                })

            addr = builder.alloc_reg()
            builder.emit_gir({
                "op": "Mul", "dst": addr,
                "src1": {"kind": "Reg", "id": base},
                "src2": {"kind": "Imm", "val": 4},
                "dtype": "i64"
            })
            builder.emit_gir({
                "op": "Add", "dst": addr,
                "src1": {"kind": "Reg", "id": addr},
                "src2": {"kind": "Param", "id": self._param_idx},
                "dtype": "i64"
            })
            return addr

        raise NotImplementedError(f"索引维度 {len(indices)} 暂不支持")


# ============================================================
# 内建函数
# ============================================================

def thread_id():
    """获取当前线程的全局 ID"""
    builder = _current_builder
    bid = builder.alloc_reg()
    bdim = builder.alloc_reg()
    tlid = builder.alloc_reg()
    builder.emit_gir({"op": "BlockId", "dst": bid, "dim": "x"})
    builder.emit_gir({"op": "BlockDim", "dst": bdim, "dim": "x"})
    builder.emit_gir({"op": "ThreadId", "dst": tlid, "dim": "x"})
    tid = builder.alloc_reg()
    builder.emit_gir({
        "op": "Mul", "dst": tid,
        "src1": {"kind": "Reg", "id": bid},
        "src2": {"kind": "Reg", "id": bdim},
        "dtype": "i32"
    })
    builder.emit_gir({
        "op": "Add", "dst": tid,
        "src1": {"kind": "Reg", "id": tid},
        "src2": {"kind": "Reg", "id": tlid},
        "dtype": "i32"
    })
    builder._tid_reg = tid
    return _RegRef(tid)


class _RegRef:
    """寄存器引用 — 包装一个寄存器 ID，与字面 int 区分"""
    __slots__ = ['reg']
    def __init__(self, reg):
        self.reg = reg


def f32(val):
    """创建一个 f32 符号常量 — 使用 Mul f32 确保寄存器被 PtxCompiler 标记为 F32 类型"""
    builder = _current_builder
    r = builder.alloc_reg()
    bits = int(np.float32(val).view(np.uint32))
    # 用 f32 Mul 1.0 来初始化，确保 PtxCompiler track_reg(dst, F32)
    # Mul(val, 1.0f) — 1.0f = 0x3F800000
    builder.emit_gir({
        "op": "Mul",
        "dst": r,
        "src1": {"kind": "Imm", "val": bits},
        "src2": {"kind": "Imm", "val": 0x3F800000},  # 1.0f
        "dtype": "f32"
    })
    return _SymF32(r, builder)


def dot(a, b):
    """4 元素点积 — 接受 (_SymF32 × 4) 元组"""
    builder = _current_builder
    if isinstance(a, tuple) and isinstance(b, tuple):
        acc = None
        for i in range(len(a)):
            prod = a[i] * b[i]
            if acc is None:
                acc = prod
            else:
                acc = acc + prod
        return acc
    raise TypeError("karte.dot 需要 2 个元组参数")


def exp(x):
    """近似指数函数: exp(x) = 2^(x * log2(e))"""
    builder = _current_builder
    if isinstance(x, _SymF32):
        r = builder.alloc_reg()
        builder.emit_gir({
            "op": "Exp",
            "dst": r,
            "src": {"kind": "Reg", "id": x.reg},
            "dtype": "f32"
        })
        return _SymF32(r, builder)
    raise TypeError("karte.exp 需要 _SymF32 参数")


def sqrt(x):
    """平方根"""
    builder = _current_builder
    if isinstance(x, _SymF32):
        r = builder.alloc_reg()
        builder.emit_gir({
            "op": "Sqrt",
            "dst": r,
            "src": {"kind": "Reg", "id": x.reg},
            "dtype": "f32"
        })
        return _SymF32(r, builder)
    raise TypeError("karte.sqrt 需要 _SymF32 参数")


def rsqrt(x):
    """倒数平方根: 1 / sqrt(x)"""
    builder = _current_builder
    if isinstance(x, _SymF32):
        r = builder.alloc_reg()
        builder.emit_gir({
            "op": "Rsqrt",
            "dst": r,
            "src": {"kind": "Reg", "id": x.reg},
            "dtype": "f32"
        })
        return _SymF32(r, builder)
    raise TypeError("karte.rsqrt 需要 _SymF32 参数")


def log(x):
    """自然对数"""
    builder = _current_builder
    if isinstance(x, _SymF32):
        r = builder.alloc_reg()
        builder.emit_gir({
            "op": "Log",
            "dst": r,
            "src": {"kind": "Reg", "id": x.reg},
            "dtype": "f32"
        })
        return _SymF32(r, builder)
    raise TypeError("karte.log 需要 _SymF32 参数")


def where(cond, then_val, else_val):
    """条件选择: cond != 0 ? then_val : else_val"""
    builder = _current_builder
    if isinstance(cond, _SymF32):
        r = builder.alloc_reg()
        builder.emit_gir({
            "op": "Where",
            "dst": r,
            "cond": _operand_to_json(cond),
            "then_val": _operand_to_json(then_val),
            "else_val": _operand_to_json(else_val),
            "dtype": "f32"
        })
        return _SymF32(r, builder)
    raise TypeError("karte.where 需要 _SymF32 条件")


def reduce_sum(x):
    """Warp 级归约求和"""
    builder = _current_builder
    if isinstance(x, _SymF32):
        r = builder.alloc_reg()
        builder.emit_gir({
            "op": "Reduce",
            "dst": r,
            "src": _operand_to_json(x),
            "reduce": "sum",
            "dtype": "f32"
        })
        return _SymF32(r, builder)
    raise TypeError("karte.reduce_sum 需要 _SymF32 参数")


def max_val(a, b):
    """取最大值"""
    builder = _current_builder
    r = builder.alloc_reg()
    builder.emit_gir({
        "op": "Max",
        "dst": r,
        "src1": _operand_to_json(a),
        "src2": _operand_to_json(b),
        "dtype": "f32"
    })
    return _SymF32(r, builder)


def min_val(a, b):
    """取最小值"""
    builder = _current_builder
    r = builder.alloc_reg()
    builder.emit_gir({
        "op": "Min",
        "dst": r,
        "src1": _operand_to_json(a),
        "src2": _operand_to_json(b),
        "dtype": "f32"
    })
    return _SymF32(r, builder)


def tanh(x):
    """双曲正切"""
    builder = _current_builder
    r = builder.alloc_reg()
    builder.emit_gir({"op": "Tanh", "dst": r, "src": _operand_to_json(x), "dtype": "f32"})
    return _SymF32(r, builder)


def cos(x):
    """余弦"""
    builder = _current_builder
    r = builder.alloc_reg()
    builder.emit_gir({"op": "Cos", "dst": r, "src": _operand_to_json(x), "dtype": "f32"})
    return _SymF32(r, builder)


def sin(x):
    """正弦"""
    builder = _current_builder
    r = builder.alloc_reg()
    builder.emit_gir({"op": "Sin", "dst": r, "src": _operand_to_json(x), "dtype": "f32"})
    return _SymF32(r, builder)


def clamp(x, lo, hi):
    """将值限制在 [lo, hi] 范围内"""
    builder = _current_builder
    r = builder.alloc_reg()
    builder.emit_gir({
        "op": "Clamp", "dst": r,
        "src": _operand_to_json(x),
        "lo": _operand_to_json(lo),
        "hi": _operand_to_json(hi),
        "dtype": "f32"
    })
    return _SymF32(r, builder)


def lerp(a, b, t):
    """线性插值: a + t*(b-a)"""
    builder = _current_builder
    r = builder.alloc_reg()
    builder.emit_gir({
        "op": "Lerp", "dst": r,
        "a": _operand_to_json(a),
        "b": _operand_to_json(b),
        "t": _operand_to_json(t),
        "dtype": "f32"
    })
    return _SymF32(r, builder)


def ceil(x):
    """向上取整"""
    builder = _current_builder
    r = builder.alloc_reg()
    builder.emit_gir({"op": "Ceil", "dst": r, "src": _operand_to_json(x), "dtype": "f32"})
    return _SymF32(r, builder)


def floor(x):
    """向下取整"""
    builder = _current_builder
    r = builder.alloc_reg()
    builder.emit_gir({"op": "Floor", "dst": r, "src": _operand_to_json(x), "dtype": "f32"})
    return _SymF32(r, builder)


def pow(base, exp):
    """幂运算"""
    builder = _current_builder
    r = builder.alloc_reg()
    builder.emit_gir({
        "op": "Pow", "dst": r,
        "base": _operand_to_json(base),
        "exp": _operand_to_json(exp),
        "dtype": "f32"
    })
    return _SymF32(r, builder)


def abs_val(x):
    """绝对值"""
    builder = _current_builder
    r = builder.alloc_reg()
    builder.emit_gir({"op": "Abs", "dst": r, "src": _operand_to_json(x), "dtype": "f32"})
    return _SymF32(r, builder)


def reduce_max(x):
    """Warp 级归约最大值"""
    builder = _current_builder
    r = builder.alloc_reg()
    builder.emit_gir({
        "op": "Reduce", "dst": r,
        "src": _operand_to_json(x),
        "reduce": "max",
        "dtype": "f32"
    })
    return _SymF32(r, builder)


class _UnrollRange:
    """编译期展开循环"""
    def __init__(self, n):
        self.n = n

    def __iter__(self):
        return iter(range(self.n))


def unroll(n):
    """编译期展开的 for 循环"""
    return _UnrollRange(n)


# ============================================================
# Tensor 类型标注
# ============================================================

class Tensor:
    """
    类型标注: karte.Tensor["N", 14, 4]

    用在 @karte.jit 函数的类型标注中，声明张量的形状。
    字符串维度（如 "N"）表示运行时动态大小。
    整数维度表示编译期已知大小。
    """
    def __init__(self, *shape):
        self.shape = shape

    def __class_getitem__(cls, *args):
        # 支持 karte.Tensor["N", 14, 4] 语法
        if len(args) == 1 and isinstance(args[0], tuple):
            return cls(*args[0])
        return cls(*args)


# ============================================================
# JIT 编译器核心
# ============================================================

_current_builder = None
_kernel_cache = {}  # cache_key (source_hash + shapes_hash) → compiled kernel


def jit(fn):
    """
    @karte.jit 装饰器 — 将 Python 函数编译为 GPU kernel

    首次调用时:
    1. 解析类型标注获取张量形状
    2. 符号执行函数体，记录 GIR 指令
    3. 生成 GIR JSON，调用 Rust 编译器获取 PTX
    4. Autotuning: 尝试不同 block_size，选择最快的配置
    5. 通过 CUDA Driver API 加载 PTX
    6. 缓存编译结果（按源码+参数形状组合）

    后续调用: 直接执行缓存的 kernel
    """
    source = inspect.getsource(fn)
    source_hash = hashlib.sha256(source.encode()).hexdigest()[:16]

    @functools.wraps(fn)
    def wrapper(*args, **kwargs):
        if _interpret_mode:
            return _interpret_kernel(fn, args, kwargs)

        # 构建包含参数形状的缓存 key
        shapes = []
        for arg in args:
            if isinstance(arg, torch.Tensor):
                shapes.append(tuple(arg.shape))
            elif isinstance(arg, (int, float)):
                shapes.append(('scalar', type(arg).__name__))
            else:
                shapes.append(('other',))
        shapes_hash = hashlib.sha256(str(shapes).encode()).hexdigest()[:8]
        cache_key = f"{source_hash}_{shapes_hash}"

        if cache_key in _kernel_cache:
            return _kernel_cache[cache_key](*args, **kwargs)

        # 首次调用: 编译 + (可选) autotuning
        compiled = _compile(fn, args)

        # Autotuning: 尝试不同 block_size，选最快的
        if _autotune_enabled():
            compiled = _autotune(fn, args, compiled)

        _kernel_cache[cache_key] = compiled
        return compiled(*args, **kwargs)

    wrapper._karte_source = source
    wrapper._karte_hash = source_hash
    return wrapper


def _compile(fn, call_args, block_size=256):
    """
    编译一个 @karte.jit 函数

    1. 从类型标注提取参数信息
    2. 符号执行函数体
    3. 生成 GIR JSON，调用 Rust 编译器获取 PTX
    4. 加载到 GPU
    """
    global _current_builder

    sig = inspect.signature(fn)
    params = list(sig.parameters.values())

    builder = _GirBuilder()
    _current_builder = builder

    # 构建参数信息
    sym_args = []

    for i, param in enumerate(params):
        ann = param.annotation

        # 判断是否是张量参数
        is_tensor = False
        shape = None
        if isinstance(ann, Tensor):
            is_tensor = True
            shape = [int(x) if isinstance(x, (int, float)) else x for x in ann.shape]
        elif isinstance(ann, str) and 'Tensor' in ann:
            is_tensor = True
            nums = re.findall(r'\d+', ann)
            shape = [int(x) for x in nums]
        elif ann is Tensor or (isinstance(ann, type) and ann is Tensor):
            is_tensor = True
            shape = [1]

        if is_tensor:
            # 用实际张量形状解析动态维度（字符串 → int）
            resolved_shape = list(shape)
            if i < len(call_args) and isinstance(call_args[i], torch.Tensor):
                actual_shape = call_args[i].shape
                for d_idx in range(min(len(resolved_shape), len(actual_shape))):
                    if not isinstance(resolved_shape[d_idx], (int, float)):
                        resolved_shape[d_idx] = actual_shape[d_idx]
            builder.params.append((param.name, 'u64', True, resolved_shape))
            sym_args.append(_SymTensor(builder, i, param.name, resolved_shape))
        elif ann is float or (isinstance(ann, str) and 'float' in ann.lower()):
            builder.params.append((param.name, 'f32', False, None))
            sym_args.append(_SymF32(i, builder))
        else:
            builder.params.append((param.name, 's32', False, None))
            sym_args.append(i)

    # 初始化 tid 寄存器
    builder._tid_reg = 0

    # 检查函数是否有 Tensor 返回类型标注，如果有则添加隐藏的输出参数
    return_ann = sig.return_annotation
    has_tensor_return = False
    if isinstance(return_ann, Tensor):
        has_tensor_return = True
    if has_tensor_return and builder.return_regs is None:
        # 添加隐藏的输出指针参数
        out_idx = len(builder.params)
        builder.params.append(('__return', 'u64', True, [1]))
        builder.output_param_idx = out_idx
    else:
        # 使用最后一个指针参数作为输出（兼容旧行为）
        out_idx = None
        for i, (_, _, is_ptr, _) in enumerate(builder.params):
            if is_ptr:
                out_idx = i
        builder.output_param_idx = out_idx

    # 符号执行函数体
    # 注入内建函数到函数的全局命名空间
    fn_globals = fn.__globals__.copy()
    fn_globals['thread_id'] = thread_id
    fn_globals['f32'] = f32
    fn_globals['dot'] = dot
    fn_globals['exp'] = exp
    fn_globals['sqrt'] = sqrt
    fn_globals['rsqrt'] = rsqrt
    fn_globals['log'] = log
    fn_globals['where'] = where
    fn_globals['reduce_sum'] = reduce_sum
    fn_globals['max_val'] = max_val
    fn_globals['min_val'] = min_val
    fn_globals['unroll'] = unroll

    # 创建临时函数执行 — 用 AST 精确移除类型标注和装饰器
    import ast
    source_tree = ast.parse(textwrap.dedent(inspect.getsource(fn)))
    for node in ast.walk(source_tree):
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            node.decorator_list = []  # 移除装饰器
            for arg in node.args.args + node.args.kwonlyargs:
                arg.annotation = None
            if node.args.vararg:
                node.args.vararg.annotation = None
            if node.args.kwarg:
                node.args.kwarg.annotation = None
            node.returns = None
    exec_code = ast.unparse(source_tree)

    # 编译并执行
    local_ns = {}
    temp_fn_code = exec_code.replace(f'def {fn.__name__}', 'def __karte_kernel__')
    exec(temp_fn_code, fn_globals, local_ns)
    kernel_fn = local_ns['__karte_kernel__']

    # 使用符号参数执行
    result = kernel_fn(*sym_args)

    # 处理返回值
    if isinstance(result, _SymF32):
        builder.return_regs = [result.reg]
    elif result is None:
        builder.return_regs = []
    elif isinstance(result, (int, float)):
        r = builder.alloc_reg()
        bits = np.float32(result).view(np.uint32)
        builder.emit_gir({
            "op": "Move",
            "dst": r,
            "src": {"kind": "Imm", "val": int(bits)}
        })
        builder.return_regs = [r]

    # 为返回值生成 store 指令：写入输出张量的 tid 位置
    _emit_return_store(builder)

    # 生成 GIR JSON
    gir_json = _build_gir_json(builder, fn.__name__, block_size)

    # 检测后端
    backend = _detect_backend()

    karte_bin = _find_karte_binary()

    if backend == 'cuda':
        # NVIDIA PTX 路线
        sm_target = f'sm_{_get_sm_version_str()}'
        result = subprocess.run(
            [karte_bin, 'gpu-jit', '--backend', 'nvidia',
             '--target', sm_target,
             '--block-size', str(block_size)],
            input=json.dumps(gir_json),
            capture_output=True, text=True,
            timeout=60
        )
        if result.returncode != 0:
            raise RuntimeError(f"Karte Rust 编译失败:\nSTDERR:\n{result.stderr}\nSTDOUT:\n{result.stdout}")

        # 加载 PTX
        cuda = _ensure_cuda()
        module = ctypes.c_void_p()
        ret = cuda.cuModuleLoadData(ctypes.byref(module), result.stdout.strip().encode() + b'\x00')
        if ret != 0:
            raise RuntimeError(f"PTX 加载失败 (code={ret})\nPTX:\n{result.stdout}")

        func_handle = ctypes.c_void_p()
        cuda.cuModuleGetFunction(ctypes.byref(func_handle), module, fn.__name__.encode())

        return _create_kernel_wrapper(func_handle, builder, fn.__name__, block_size, fn)

    elif backend == 'opencl':
        # OpenCL C 路线 (AMD / Intel)
        result = subprocess.run(
            [karte_bin, 'gpu-jit', '--backend', 'opencl',
             '--block-size', str(block_size)],
            input=json.dumps(gir_json),
            capture_output=True, text=True,
            timeout=60
        )
        if result.returncode != 0:
            raise RuntimeError(f"Karte Rust 编译失败:\nSTDERR:\n{result.stderr}\nSTDOUT:\n{result.stdout}")

        ocl_source = result.stdout

        # 编译 OpenCL C 源码
        cl = _ensure_opencl()
        cl_lib = cl['lib']
        context = cl['context']
        device = cl['device']

        # clCreateProgramWithSource
        source_bytes = ocl_source.encode('utf-8')
        source_ptr = ctypes.c_char_p(source_bytes)
        source_len = ctypes.c_size_t(len(source_bytes))
        err = ctypes.c_int(0)
        program = cl_lib.clCreateProgramWithSource(
            context, 1, ctypes.byref(source_ptr),
            ctypes.byref(source_len), ctypes.byref(err))
        if err.value != 0:
            raise RuntimeError(f"clCreateProgramWithSource 失败 (err={err.value})")

        # clBuildProgram — 启用子组扩展
        build_options = b"-cl-std=CL2.0 -cl-fast-relaxed-math"
        ret = cl_lib.clBuildProgram(
            program, 1, ctypes.byref(ctypes.c_void_p(device)),
            build_options, None, None)
        if ret != 0:
            # 获取编译错误日志
            log_size = ctypes.c_size_t(0)
            cl_lib.clGetProgramBuildInfo(
                program, device, 0x1084, 0, None, ctypes.byref(log_size))
            log_buf = ctypes.create_string_buffer(log_size.value)
            cl_lib.clGetProgramBuildInfo(
                program, device, 0x1084, log_size.value, log_buf, None)
            raise RuntimeError(
                f"OpenCL 编译失败 (err={ret}):\n{log_buf.value.decode('utf-8', errors='replace')}\n"
                f"源码:\n{ocl_source[:2000]}")

        # clCreateKernel
        kernel = cl_lib.clCreateKernel(
            program, fn.__name__.encode(), ctypes.byref(err))
        if err.value != 0:
            raise RuntimeError(f"clCreateKernel 失败 (err={err.value})")

        return _create_opencl_kernel_wrapper(
            kernel, cl, builder, fn.__name__, block_size, fn)

    else:
        raise RuntimeError(f"无可用 GPU 后端 (backend={backend})")


def _emit_return_store(builder):
    """为返回值生成 global_store 指令"""
    if not builder.return_regs or len(builder.return_regs) == 0:
        return

    out_param = builder.output_param_idx
    if out_param is None:
        return

    tid = builder._tid_reg
    # addr = tid * 4 + param_out
    addr = builder.alloc_reg()
    builder.emit_gir({
        "op": "Mul", "dst": addr,
        "src1": {"kind": "Reg", "id": tid},
        "src2": {"kind": "Imm", "val": 4},
        "dtype": "i64"
    })
    builder.emit_gir({
        "op": "Add", "dst": addr,
        "src1": {"kind": "Reg", "id": addr},
        "src2": {"kind": "Param", "id": out_param},
        "dtype": "i64"
    })
    builder.emit_gir({
        "op": "GlobalStore",
        "addr": {"kind": "Reg", "id": addr},
        "src": {"kind": "Reg", "id": builder.return_regs[0]},
        "dtype": "f32"
    })


def _build_gir_json(builder, name, block_size=256):
    """构建 GIR JSON 程序"""
    # 构建参数声明
    params_json = []
    for pname, ptype, is_ptr, shape in builder.params:
        if ptype == 'f32':
            dtype = "f32"
        elif ptype == 'u64':
            dtype = "f32"  # 指针参数的元素类型
        else:
            dtype = "i32"
        params_json.append({"name": pname, "dtype": dtype, "is_ptr": is_ptr})

    return {
        "kernels": [{
            "name": name,
            "params": params_json,
            "instructions": builder.gir_instrs,
            "next_reg": builder.next_reg,
            "next_label": builder.next_label,
            "block_dim": [block_size, 1, 1],
        }]
    }


def _create_kernel_wrapper(func_handle, builder, name, block_size=256, fn=None):
    """创建 Python 可调用的 kernel 包装器"""

    out_param_idx = builder.output_param_idx  # 由 _compile 设置

    def wrapper(*args, **kwargs):
        if _interpret_mode and fn is not None:
            return _interpret_kernel(fn, args, kwargs)

        cuda = _ensure_cuda()

        # 允许 args 数量少于 builder.params（当有隐藏的输出参数时）
        num_user_params = len(args)
        total_params = len(builder.params)
        has_hidden_output = (out_param_idx is not None and out_param_idx >= num_user_params)

        raw_args = []
        batch_size = 1
        out_tensor = None

        for i, arg in enumerate(args):
            pname, ptype, is_ptr, shape = builder.params[i]

            if is_ptr:
                if isinstance(arg, torch.Tensor):
                    if not arg.is_cuda:
                        arg = arg.cuda()
                    if not arg.is_contiguous():
                        arg = arg.contiguous()
                    raw_args.append(arg.data_ptr())

                    # 推断 batch_size：取最大的第一维（跳过输出参数）
                    if shape and len(arg.shape) >= 1 and i != out_param_idx:
                        candidate = arg.shape[0]
                        if candidate > batch_size:
                            batch_size = candidate
                else:
                    raw_args.append(arg)
            else:
                if ptype == 'f32':
                    fval = np.float32(arg)
                    raw_args.append(int(fval.view(np.uint32)))
                else:
                    raw_args.append(int(arg))

        # 为隐藏的输出参数分配张量
        if has_hidden_output and out_param_idx is not None:
            out_tensor = torch.empty(batch_size, device='cuda', dtype=torch.float32)
            # 确保 raw_args 中有足够的元素
            while len(raw_args) < out_param_idx:
                raw_args.append(0)
            raw_args.append(out_tensor.data_ptr())
        elif out_param_idx is not None and out_param_idx < len(args):
            # 用户提供的输出张量
            out_tensor = args[out_param_idx]
            if isinstance(out_tensor, torch.Tensor):
                if not out_tensor.is_cuda:
                    out_tensor = out_tensor.cuda()
                if not out_tensor.is_contiguous():
                    out_tensor = out_tensor.contiguous()

        assert len(raw_args) == total_params, \
            f"参数数量不匹配: 期望 {total_params}, 实际 {len(raw_args)}"

        grid = (batch_size + block_size - 1) // block_size

        n = len(raw_args)
        buf = (ctypes.c_uint64 * n)(*raw_args)
        ptrs = (ctypes.c_void_p * n)()
        base = ctypes.addressof(buf)
        for j in range(n):
            ptrs[j] = base + j * 8

        ret = cuda.cuLaunchKernel(
            func_handle, grid, 1, 1, block_size, 1, 1,
            0, None, ptrs, None
        )
        assert ret == 0, f"cuLaunchKernel failed: {ret}"
        cuda.cuCtxSynchronize()

        return out_tensor

    return wrapper


def _create_opencl_kernel_wrapper(kernel, cl_state, builder, name, block_size=256, fn=None):
    """创建 OpenCL kernel 的 Python 可调用包装器"""

    cl_lib = cl_state['lib']
    context = cl_state['context']
    queue = cl_state['queue']
    out_param_idx = builder.output_param_idx
    cl_buffers = []  # 持有 cl_mem 引用防止 GC

    def wrapper(*args, **kwargs):
        if _interpret_mode and fn is not None:
            return _interpret_kernel(fn, args, kwargs)

        num_user_params = len(args)
        total_params = len(builder.params)
        has_hidden_output = (out_param_idx is not None and out_param_idx >= num_user_params)

        # 准备参数
        cl_buffers.clear()
        arg_values = []
        batch_size = 1
        out_tensor = None

        for i, arg in enumerate(args):
            pname, ptype, is_ptr, shape = builder.params[i]

            if is_ptr:
                if isinstance(arg, torch.Tensor):
                    # 对于 OpenCL，需要创建 cl_mem 或使用 PyTorch CUDA tensor 的指针
                    # AMD GPU 上 PyTorch 可能使用 ROCm/HIP，数据已在 GPU 上
                    if not arg.is_cuda:
                        arg = arg.cuda()
                    if not arg.is_contiguous():
                        arg = arg.contiguous()

                    # 在 OpenCL 中创建 buffer 并写入数据
                    CL_MEM_READ_WRITE = 1
                    CL_MEM_COPY_HOST_PTR = 1 << 5
                    nbytes = arg.numel() * arg.element_size()

                    # 尝试使用 PyTorch tensor 的_data_ptr 作为 OpenCL buffer
                    # ROCm 的 OpenCL 实现可以直接访问 HIP 分配的内存
                    err = ctypes.c_int(0)
                    cl_mem = cl_lib.clCreateBuffer(
                        context, CL_MEM_READ_WRITE | CL_MEM_COPY_HOST_PTR,
                        nbytes, arg.data_ptr(), ctypes.byref(err))
                    if err.value != 0:
                        # 回退: 创建空 buffer 并手动拷贝
                        cl_mem = cl_lib.clCreateBuffer(
                            context, CL_MEM_READ_WRITE, nbytes, None, ctypes.byref(err))
                        if err.value != 0:
                            raise RuntimeError(f"clCreateBuffer 失败 (err={err.value})")
                        cl_lib.clEnqueueWriteBuffer(
                            queue, cl_mem, 1, 0, nbytes,
                            arg.data_ptr(), 0, None, None)

                    cl_buffers.append(cl_mem)
                    arg_values.append(cl_mem)

                    if shape and len(arg.shape) >= 1 and i != out_param_idx:
                        candidate = arg.shape[0]
                        if candidate > batch_size:
                            batch_size = candidate
                else:
                    arg_values.append(arg)
            else:
                if ptype == 'f32':
                    fval = np.float32(arg)
                    arg_values.append(int(fval.view(np.uint32)))
                else:
                    arg_values.append(int(arg))

        # 为隐藏的输出参数分配张量和 buffer
        if has_hidden_output and out_param_idx is not None:
            out_tensor = torch.empty(batch_size, device='cuda', dtype=torch.float32)
            while len(arg_values) < out_param_idx:
                arg_values.append(0)
            CL_MEM_READ_WRITE = 1
            nbytes = out_tensor.numel() * out_tensor.element_size()
            err = ctypes.c_int(0)
            out_cl_mem = cl_lib.clCreateBuffer(
                context, CL_MEM_READ_WRITE, nbytes, None, ctypes.byref(err))
            cl_buffers.append(out_cl_mem)
            arg_values.append(out_cl_mem)
        elif out_param_idx is not None and out_param_idx < len(args):
            out_tensor = args[out_param_idx]
            if isinstance(out_tensor, torch.Tensor):
                if not out_tensor.is_cuda:
                    out_tensor = out_tensor.cuda()
                if not out_tensor.is_contiguous():
                    out_tensor = out_tensor.contiguous()

        # 设置 kernel 参数
        for i, val in enumerate(arg_values):
            if isinstance(val, ctypes.c_void_p) or isinstance(val, int):
                # cl_mem 参数 — 传递指针值
                buf_val = ctypes.c_uint64(ctypes.cast(val, ctypes.c_void_p).value if isinstance(val, ctypes.c_void_p) else val)
                cl_lib.clSetKernelArg(kernel, i, 8, ctypes.byref(buf_val))
            else:
                buf_val = ctypes.c_int(int(val))
                cl_lib.clSetKernelArg(kernel, i, 4, ctypes.byref(buf_val))

        # 启动 kernel
        grid = (batch_size + block_size - 1) // block_size
        global_work_size = (ctypes.c_size_t * 3)(grid * block_size, 1, 1)
        local_work_size = (ctypes.c_size_t * 3)(block_size, 1, 1)

        ret = cl_lib.clEnqueueNDRangeKernel(
            queue, kernel, 1, None,
            global_work_size, local_work_size,
            0, None, None)
        if ret != 0:
            raise RuntimeError(f"clEnqueueNDRangeKernel 失败 (err={ret})")

        cl_lib.clFinish(queue)

        # 读取输出数据回 PyTorch tensor
        if out_tensor is not None and len(cl_buffers) > 0:
            out_cl_mem = cl_buffers[-1]  # 最后一个 buffer 是输出
            nbytes = out_tensor.numel() * out_tensor.element_size()
            cl_lib.clEnqueueReadBuffer(
                queue, out_cl_mem, 1, 0, nbytes,
                out_tensor.data_ptr(), 0, None, None)

        return out_tensor

    return wrapper


# ============================================================
# Autotuning
# ============================================================

def _autotune_enabled():
    """是否启用 autotuning"""
    return _autotune_flag


def _autotune(fn, args, default_compiled):
    """
    自动调优: 尝试不同 block_size，选择最快的配置。

    策略:
    1. 用默认 block_size (256) benchmark 10 次作为基线
    2. 尝试候选 block_size
    3. 对每个配置 benchmark 10 次调用
    4. 选择最快的
    """
    backend = _detect_backend()
    if backend == 'cuda':
        _ensure_cuda()
    elif backend == 'opencl':
        _ensure_opencl()

    results = {}

    # Benchmark 默认配置 (256)
    torch.cuda.synchronize()
    times = []
    for _ in range(10):
        t0 = time.perf_counter()
        default_compiled(*args)
        torch.cuda.synchronize()
        times.append(time.perf_counter() - t0)
    results[256] = sum(times) / len(times)

    best_compiled = default_compiled
    best_time = results[256]

    # 尝试其他候选 block_size
    for bs in _BLOCK_SIZE_CANDIDATES:
        if bs == 256:
            continue

        try:
            # 用新 block_size 重新编译
            alt_compiled = _compile(fn, args, block_size=bs)

            # Benchmark
            torch.cuda.synchronize()
            times = []
            for _ in range(10):
                t0 = time.perf_counter()
                alt_compiled(*args)
                torch.cuda.synchronize()
                times.append(time.perf_counter() - t0)
            avg_time = sum(times) / len(times)
            results[bs] = avg_time

            if avg_time < best_time:
                best_time = avg_time
                best_compiled = alt_compiled
        except Exception:
            # 某些 block_size 可能编译失败或执行失败，跳过
            continue

    return best_compiled


# ============================================================
# CPU 解释模式
# ============================================================

def _interpret_kernel(fn, args, kwargs):
    """CPU 解释执行 — 直接运行原始 Python 函数（CPU 上）"""
    cpu_args = []
    for arg in args:
        if isinstance(arg, torch.Tensor):
            cpu_args.append(arg.cpu())
        else:
            cpu_args.append(arg)
    result = fn(*cpu_args, **kwargs)
    if isinstance(result, torch.Tensor):
        return result.cuda()
    return result


# ============================================================
# 公共 API
# ============================================================

__all__ = ['jit', 'Tensor', 'thread_id', 'f32', 'dot', 'exp', 'sqrt', 'rsqrt', 'log',
           'tanh', 'cos', 'sin', 'clamp', 'lerp', 'ceil', 'floor', 'pow',
           'max_val', 'min_val', 'abs_val', 'where', 'reduce_sum', 'reduce_max',
           'unroll', 'is_interpret_mode']


# ============================================================
# torch.autograd.Function 集成
# ============================================================

class KarteAutogradFunction(torch.autograd.Function):
    """
    将 @karte.jit kernel 包装为 torch.autograd.Function。

    用法:
        @karte.jit
        def my_forward(x: karte.Tensor["N"], ...) -> karte.Tensor["N"]:
            ...

        @karte.jit
        def my_backward(grad_out: karte.Tensor["N"], ...) -> karte.Tensor["N"]:
            ...

        # 包装为 autograd Function
        MyOp = karte.autograd(my_forward, my_backward)

        # 在训练中使用 — 支持 .backward()
        result = MyOp.apply(x, y)
        loss = result.sum()
        loss.backward()  # 自动调用 my_backward
    """

    @staticmethod
    def forward(ctx, forward_kernel, backward_kernel, *args):
        ctx.forward_kernel = forward_kernel
        ctx.backward_kernel = backward_kernel
        ctx.save_for_backward(*[a for a in args if isinstance(a, torch.Tensor)])
        result = forward_kernel(*args)
        return result

    @staticmethod
    def backward(ctx, grad_output):
        saved = ctx.saved_tensors
        if not grad_output.is_cuda:
            grad_output = grad_output.cuda()

        if ctx.backward_kernel is not None:
            grads = ctx.backward_kernel(grad_output, *saved)
            return (None, None, grad_output) + (None,) * len(saved)
        else:
            return (None, None) + tuple(torch.zeros_like(s) for s in saved)


def autograd(forward_kernel, backward_kernel=None):
    """
    将 @karte.jit forward/backward kernel 包装为支持 autograd 的可调用对象。

    参数:
        forward_kernel: @karte.jit 装饰的前向 kernel
        backward_kernel: @karte.jit 装饰的反向 kernel（可选）

    返回:
        一个类似 torch.autograd.Function 的对象，使用 .apply() 调用

    示例:
        @karte.jit
        def relu_forward(x: karte.Tensor["N"]) -> karte.Tensor["N"]:
            tid = karte.thread_id()
            val = x[tid]
            return karte.where(val < 0.0, 0.0, val)

        @karte.jit
        def relu_backward(grad: karte.Tensor["N"], x: karte.Tensor["N"]) -> karte.Tensor["N"]:
            tid = karte.thread_id()
            val = x[tid]
            grad_val = grad[tid]
            return karte.where(val < 0.0, 0.0, grad_val)

        ReLU = karte.autograd(relu_forward, relu_backward)
        x = torch.randn(4096, device='cuda', requires_grad=True)
        y = ReLU.apply(x)
        y.sum().backward()
        print(x.grad)  # 非零梯度
    """
    class _WrappedFunction(torch.autograd.Function):
        @staticmethod
        def forward(ctx, *args):
            ctx.save_for_backward(*[a for a in args if isinstance(a, torch.Tensor)])
            result = forward_kernel(*args)
            return result

        @staticmethod
        def backward(ctx, grad_output):
            if not grad_output.is_cuda:
                grad_output = grad_output.cuda()

            if backward_kernel is not None:
                saved_tensors = ctx.saved_tensors
                grad_result = backward_kernel(grad_output, *saved_tensors)
                n_saved = len(saved_tensors)
                n_non_tensor = len(ctx.saved_tensors) - n_saved
                return tuple([grad_result if i == 0 else None for i in range(len(ctx.saved_tensors))])
            else:
                return tuple(torch.zeros_like(s) for s in ctx.saved_tensors)

    return _WrappedFunction
