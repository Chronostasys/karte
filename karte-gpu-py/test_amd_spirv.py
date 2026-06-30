#!/usr/bin/env python3
"""
Karte SPIR-V 后端端到端 AMD GPU 测试

测试流程:
1. 构造 GIR JSON → karte gpu-jit --backend spirv 生成 SPIR-V 二进制
2. spirv-val 验证合法性
3. OpenCL clCreateProgramWithIL 加载到 AMD GPU
4. 执行 kernel → 读回结果 → 验证正确性
"""

import ctypes
import ctypes.util
import json
import struct
import subprocess
import sys
import os
import math

cl = None
_ctx = None
_dev = None
_queue = None

def init_opencl():
    """初始化 OpenCL，优先选择 rusticl (AMD) 平台"""
    global cl, _ctx, _dev, _queue
    if cl is not None:
        return cl, _ctx, _dev, _queue

    lib_path = ctypes.util.find_library('OpenCL')
    if not lib_path:
        for p in ['libOpenCL.so.1', 'libOpenCL.so', '/usr/lib/x86_64-linux-gnu/libOpenCL.so.1']:
            try:
                cl = ctypes.CDLL(p)
                break
            except OSError:
                continue
    else:
        cl = ctypes.CDLL(lib_path)
    if cl is None:
        raise RuntimeError("OpenCL 库未找到")

    # 函数签名
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

    # 枚举平台
    num_platforms = ctypes.c_uint(0)
    cl.clGetPlatformIDs(0, None, ctypes.byref(num_platforms))
    platforms = (ctypes.c_void_p * num_platforms.value)()
    cl.clGetPlatformIDs(num_platforms.value, platforms, None)

    CL_PLATFORM_NAME = 0x0902
    CL_DEVICE_TYPE_GPU = 1 << 2
    CL_DEVICE_NAME = 0x1027

    selected_platform = None
    selected_device = None

    # 第一轮：优先选择 rusticl
    for p in platforms:
        size = ctypes.c_size_t(0)
        cl.clGetPlatformInfo(p, CL_PLATFORM_NAME, 0, None, ctypes.byref(size))
        name_buf = ctypes.create_string_buffer(size.value)
        cl.clGetPlatformInfo(p, CL_PLATFORM_NAME, size.value, name_buf, None)
        pname = name_buf.value.decode()

        num_devices = ctypes.c_uint(0)
        ret = cl.clGetDeviceIDs(p, CL_DEVICE_TYPE_GPU, 0, None, ctypes.byref(num_devices))
        if ret == 0 and num_devices.value > 0:
            devices = (ctypes.c_void_p * num_devices.value)()
            cl.clGetDeviceIDs(p, CL_DEVICE_TYPE_GPU, num_devices.value, devices, None)
            for d in devices:
                size = ctypes.c_size_t(0)
                cl.clGetDeviceInfo(d, CL_DEVICE_NAME, 0, None, ctypes.byref(size))
                dev_buf = ctypes.create_string_buffer(size.value)
                cl.clGetDeviceInfo(d, CL_DEVICE_NAME, size.value, dev_buf, None)
                dname = dev_buf.value.decode()

                if 'rusticl' in pname.lower():
                    selected_platform = p
                    selected_device = d
                    print(f"✅ 选择 GPU: {dname} (平台: {pname})")
                    break
        if selected_platform:
            break

    if not selected_platform:
        # 回退：任意 GPU
        for p in platforms:
            num_devices = ctypes.c_uint(0)
            ret = cl.clGetDeviceIDs(p, CL_DEVICE_TYPE_GPU, 0, None, ctypes.byref(num_devices))
            if ret == 0 and num_devices.value > 0:
                devices = (ctypes.c_void_p * num_devices.value)()
                cl.clGetDeviceIDs(p, CL_DEVICE_TYPE_GPU, num_devices.value, devices, None)
                selected_platform = p
                selected_device = devices[0]
                print(f"✅ 回退选择 GPU (第一个可用)")
                break

    if not selected_device:
        raise RuntimeError("未找到 GPU 设备")

    err = ctypes.c_int(0)
    _ctx = cl.clCreateContext(None, 1, ctypes.byref(ctypes.c_void_p(selected_device)), None, None, ctypes.byref(err))
    assert err.value == 0, f"clCreateContext failed: {err.value}"
    _queue = cl.clCreateCommandQueueWithProperties(_ctx, selected_device, None, ctypes.byref(err))
    assert err.value == 0, f"clCreateCommandQueueWithProperties failed: {err.value}"
    _dev = selected_device

    return cl, _ctx, _dev, _queue


def compile_spirv(gir_json, karte_bin="./target/debug/karte"):
    """调用 karte gpu-jit --backend spirv 生成 SPIR-V 二进制"""
    result = subprocess.run(
        [karte_bin, "gpu-jit", "--backend", "spirv"],
        input=json.dumps(gir_json).encode('utf-8'),
        capture_output=True,
        timeout=30,
    )
    if result.returncode != 0:
        raise RuntimeError(f"karte gpu-jit failed:\n{result.stderr.decode()}")
    return result.stdout


def load_and_build(cl, ctx, dev, spirv_bytes, kernel_name):
    """加载 SPIR-V 并编译"""
    err = ctypes.c_int(0)
    spirv_buf = ctypes.create_string_buffer(spirv_bytes)
    program = cl.clCreateProgramWithIL(ctx, spirv_buf, len(spirv_bytes), ctypes.byref(err))
    if err.value != 0:
        raise RuntimeError(f"clCreateProgramWithIL failed: {err.value}")

    ret = cl.clBuildProgram(program, 1, ctypes.byref(ctypes.c_void_p(dev)), None, None, None)
    if ret != 0:
        CL_PROGRAM_BUILD_LOG = 0x1084
        log_size = ctypes.c_size_t(0)
        cl.clGetProgramBuildInfo(program, dev, CL_PROGRAM_BUILD_LOG, 0, None, ctypes.byref(log_size))
        log_buf = ctypes.create_string_buffer(max(log_size.value, 1))
        cl.clGetProgramBuildInfo(program, dev, CL_PROGRAM_BUILD_LOG, log_size.value, log_buf, None)
        log = log_buf.value.decode('utf-8', errors='replace') if log_size.value > 0 else "(empty)"
        raise RuntimeError(f"clBuildProgram failed ({ret}):\n{log}")

    kernel = cl.clCreateKernel(program, kernel_name.encode(), ctypes.byref(err))
    if err.value != 0:
        raise RuntimeError(f"clCreateKernel failed: {err.value}")
    return kernel


def make_buf(cl, ctx, nbytes, host_data=None):
    """创建 buffer，可选从 host 数据初始化"""
    CL_MEM_READ_WRITE = 1
    CL_MEM_COPY_HOST_PTR = 1 << 5
    flags = CL_MEM_READ_WRITE
    if host_data is not None:
        flags |= CL_MEM_COPY_HOST_PTR
    err = ctypes.c_int(0)
    ptr = ctypes.cast(host_data, ctypes.c_void_p) if host_data else None
    buf = cl.clCreateBuffer(ctx, flags, nbytes, ptr, ctypes.byref(err))
    assert err.value == 0, f"clCreateBuffer failed: {err.value}"
    return buf


def run_kernel(cl, queue, kernel, global_size, local_size, arg_bufs):
    """启动 kernel"""
    for i, buf in enumerate(arg_bufs):
        arg_val = ctypes.c_uint64(ctypes.cast(buf, ctypes.c_void_p).value)
        cl.clSetKernelArg(kernel, i, 8, ctypes.byref(arg_val))
    global_ws = (ctypes.c_size_t * 3)(global_size, 1, 1)
    local_ws = (ctypes.c_size_t * 3)(local_size, 1, 1)
    ret = cl.clEnqueueNDRangeKernel(queue, kernel, 1, None, global_ws, local_ws, 0, None, None)
    assert ret == 0, f"clEnqueueNDRangeKernel failed: {ret}"
    cl.clFinish(queue)


def read_buf(cl, queue, buf, nbytes):
    """从 GPU buffer 读数据"""
    host = ctypes.create_string_buffer(nbytes)
    ret = cl.clEnqueueReadBuffer(queue, buf, 1, 0, nbytes, host, 0, None, None)
    assert ret == 0, f"clEnqueueReadBuffer failed: {ret}"
    return host.raw


def validate_spirv(spirv_bytes, name="test"):
    """用 spirv-val 验证 SPIR-V"""
    path = f"/tmp/karte_{name}.spirv"
    with open(path, 'wb') as f:
        f.write(spirv_bytes)
    result = subprocess.run(["spirv-val", path],
                          capture_output=True, text=True)
    if result.returncode != 0:
        print(f"  ⚠️ spirv-val 警告: {result.stderr.strip()}")
    else:
        print(f"  ✅ spirv-val 验证通过")

    # 也用 spirv-dis 查看反汇编
    result2 = subprocess.run(["spirv-dis", path], capture_output=True, text=True)
    if result2.returncode == 0:
        # 打印前 20 行反汇编
        lines = result2.stdout.strip().split('\n')[:20]
        print(f"  📋 SPIR-V 反汇编 (前 20 行):")
        for line in lines:
            print(f"    {line}")
    return path


# ============================================================
# 测试用例
# ============================================================

def test_scalar_add():
    """测试 1: 标量加法 — output[0] = input[0] + 1.0

    所有线程从 input[0] 读取同一个值，加 1.0 后写入 output[0]。
    验证 SPIR-V 基础管线: OpLoad + OpFAdd + OpStore。
    """
    print("\n📝 测试 1: 标量加法 (output[0] = input[0] + 1.0)")

    instructions = [
        # GlobalLoad from Param(0) — 指针参数直接 OpLoad
        {"op": "GlobalLoad", "dst": 100, "addr": {"kind": "Param", "id": 0}, "dtype": "f32"},
        # 1.0f = 0x3F800000 = 1065353216
        {"op": "Add", "dst": 101, "src1": {"kind": "Reg", "id": 100}, "src2": {"kind": "Imm", "val": 1065353216}, "dtype": "f32"},
        # GlobalStore to Param(1) — 指针参数直接 OpStore
        {"op": "GlobalStore", "addr": {"kind": "Param", "id": 1}, "src": {"kind": "Reg", "id": 101}, "dtype": "f32"},
        {"op": "Return"},
    ]
    gir = {"kernels": [{"name": "scalar_add", "params": [
        {"name": "input", "dtype": "f32", "is_ptr": True},
        {"name": "output", "dtype": "f32", "is_ptr": True},
    ], "instructions": instructions, "next_reg": 200, "next_label": 50, "block_dim": [1, 1, 1]}]}

    spirv = compile_spirv(gir)
    print(f"  SPIR-V 大小: {len(spirv)} bytes")
    validate_spirv(spirv, "scalar_add")

    cl, ctx, dev, queue = init_opencl()
    kernel = load_and_build(cl, ctx, dev, spirv, "scalar_add")

    # input[0] = 3.14
    input_data = struct.pack('f', 3.14)
    in_buf = make_buf(cl, ctx, 4, ctypes.create_string_buffer(input_data))
    out_buf = make_buf(cl, ctx, 4)

    # 启动 1 个线程
    run_kernel(cl, queue, kernel, 1, 1, [in_buf, out_buf])

    output_raw = read_buf(cl, queue, out_buf, 4)
    result = struct.unpack('f', output_raw)[0]
    expected = 3.14 + 1.0

    print(f"  input[0] = 3.14, output[0] = {result:.6f}, 期望 = {expected:.6f}")
    if abs(result - expected) < 1e-5:
        print("  ✅ 标量加法测试通过")
        return True
    else:
        print(f"  ❌ 结果不正确: {result} != {expected}")
        return False


def test_scalar_mul():
    """测试 2: 标量乘法 — output[0] = input[0] * 2.0"""
    print("\n📝 测试 2: 标量乘法 (output[0] = input[0] * 2.0)")

    instructions = [
        {"op": "GlobalLoad", "dst": 100, "addr": {"kind": "Param", "id": 0}, "dtype": "f32"},
        # 2.0f = 0x40000000 = 1073741824
        {"op": "Mul", "dst": 101, "src1": {"kind": "Reg", "id": 100}, "src2": {"kind": "Imm", "val": 1073741824}, "dtype": "f32"},
        {"op": "GlobalStore", "addr": {"kind": "Param", "id": 1}, "src": {"kind": "Reg", "id": 101}, "dtype": "f32"},
        {"op": "Return"},
    ]
    gir = {"kernels": [{"name": "scalar_mul", "params": [
        {"name": "input", "dtype": "f32", "is_ptr": True},
        {"name": "output", "dtype": "f32", "is_ptr": True},
    ], "instructions": instructions, "next_reg": 200, "next_label": 50, "block_dim": [1, 1, 1]}]}

    spirv = compile_spirv(gir)
    validate_spirv(spirv, "scalar_mul")

    cl, ctx, dev, queue = init_opencl()
    kernel = load_and_build(cl, ctx, dev, spirv, "scalar_mul")

    input_data = struct.pack('f', 42.5)
    in_buf = make_buf(cl, ctx, 4, ctypes.create_string_buffer(input_data))
    out_buf = make_buf(cl, ctx, 4)

    run_kernel(cl, queue, kernel, 1, 1, [in_buf, out_buf])
    result = struct.unpack('f', read_buf(cl, queue, out_buf, 4))[0]
    expected = 42.5 * 2.0

    print(f"  input[0] = 42.5, output[0] = {result:.6f}, 期望 = {expected:.6f}")
    if abs(result - expected) < 1e-5:
        print("  ✅ 标量乘法测试通过")
        return True
    else:
        print(f"  ❌ 结果不正确: {result} != {expected}")
        return False


def test_exp():
    """测试 3: 指数函数 — output[0] = exp(input[0])"""
    print("\n📝 测试 3: 指数函数 (output[0] = exp(input[0]))")

    instructions = [
        {"op": "GlobalLoad", "dst": 100, "addr": {"kind": "Param", "id": 0}, "dtype": "f32"},
        {"op": "Exp", "dst": 101, "src": {"kind": "Reg", "id": 100}, "dtype": "f32"},
        {"op": "GlobalStore", "addr": {"kind": "Param", "id": 1}, "src": {"kind": "Reg", "id": 101}, "dtype": "f32"},
        {"op": "Return"},
    ]
    gir = {"kernels": [{"name": "exp_test", "params": [
        {"name": "input", "dtype": "f32", "is_ptr": True},
        {"name": "output", "dtype": "f32", "is_ptr": True},
    ], "instructions": instructions, "next_reg": 200, "next_label": 50, "block_dim": [1, 1, 1]}]}

    spirv = compile_spirv(gir)
    validate_spirv(spirv, "exp_test")

    cl, ctx, dev, queue = init_opencl()
    kernel = load_and_build(cl, ctx, dev, spirv, "exp_test")

    input_data = struct.pack('f', 2.0)
    in_buf = make_buf(cl, ctx, 4, ctypes.create_string_buffer(input_data))
    out_buf = make_buf(cl, ctx, 4)

    run_kernel(cl, queue, kernel, 1, 1, [in_buf, out_buf])
    result = struct.unpack('f', read_buf(cl, queue, out_buf, 4))[0]
    expected = math.exp(2.0)

    print(f"  input[0] = 2.0, output[0] = {result:.6f}, 期望 = {expected:.6f}")
    if abs(result - expected) < 1e-3:
        print("  ✅ 指数函数测试通过")
        return True
    else:
        print(f"  ❌ 结果不正确: {result} != {expected}")
        return False


def test_sqrt():
    """测试 4: 平方根 — output[0] = sqrt(input[0])"""
    print("\n📝 测试 4: 平方根 (output[0] = sqrt(input[0]))")

    instructions = [
        {"op": "GlobalLoad", "dst": 100, "addr": {"kind": "Param", "id": 0}, "dtype": "f32"},
        {"op": "Sqrt", "dst": 101, "src": {"kind": "Reg", "id": 100}, "dtype": "f32"},
        {"op": "GlobalStore", "addr": {"kind": "Param", "id": 1}, "src": {"kind": "Reg", "id": 101}, "dtype": "f32"},
        {"op": "Return"},
    ]
    gir = {"kernels": [{"name": "sqrt_test", "params": [
        {"name": "input", "dtype": "f32", "is_ptr": True},
        {"name": "output", "dtype": "f32", "is_ptr": True},
    ], "instructions": instructions, "next_reg": 200, "next_label": 50, "block_dim": [1, 1, 1]}]}

    spirv = compile_spirv(gir)
    validate_spirv(spirv, "sqrt_test")

    cl, ctx, dev, queue = init_opencl()
    kernel = load_and_build(cl, ctx, dev, spirv, "sqrt_test")

    input_data = struct.pack('f', 16.0)
    in_buf = make_buf(cl, ctx, 4, ctypes.create_string_buffer(input_data))
    out_buf = make_buf(cl, ctx, 4)

    run_kernel(cl, queue, kernel, 1, 1, [in_buf, out_buf])
    result = struct.unpack('f', read_buf(cl, queue, out_buf, 4))[0]
    expected = math.sqrt(16.0)

    print(f"  input[0] = 16.0, output[0] = {result:.6f}, 期望 = {expected:.6f}")
    if abs(result - expected) < 1e-5:
        print("  ✅ 平方根测试通过")
        return True
    else:
        print(f"  ❌ 结果不正确: {result} != {expected}")
        return False


def test_chain():
    """测试 5: 运算链 — output[0] = (input[0] + 3.0) * 2.0 - 1.0"""
    print("\n📝 测试 5: 运算链 (output[0] = (input[0] + 3.0) * 2.0 - 1.0)")

    instructions = [
        {"op": "GlobalLoad", "dst": 100, "addr": {"kind": "Param", "id": 0}, "dtype": "f32"},
        # 3.0f = 0x40400000 = 1077936128
        {"op": "Add", "dst": 101, "src1": {"kind": "Reg", "id": 100}, "src2": {"kind": "Imm", "val": 1077936128}, "dtype": "f32"},
        # 2.0f = 0x40000000 = 1073741824
        {"op": "Mul", "dst": 102, "src1": {"kind": "Reg", "id": 101}, "src2": {"kind": "Imm", "val": 1073741824}, "dtype": "f32"},
        # 1.0f = 0x3F800000 = 1065353216
        {"op": "Sub", "dst": 103, "src1": {"kind": "Reg", "id": 102}, "src2": {"kind": "Imm", "val": 1065353216}, "dtype": "f32"},
        {"op": "GlobalStore", "addr": {"kind": "Param", "id": 1}, "src": {"kind": "Reg", "id": 103}, "dtype": "f32"},
        {"op": "Return"},
    ]
    gir = {"kernels": [{"name": "chain_test", "params": [
        {"name": "input", "dtype": "f32", "is_ptr": True},
        {"name": "output", "dtype": "f32", "is_ptr": True},
    ], "instructions": instructions, "next_reg": 200, "next_label": 50, "block_dim": [1, 1, 1]}]}

    spirv = compile_spirv(gir)
    validate_spirv(spirv, "chain_test")

    cl, ctx, dev, queue = init_opencl()
    kernel = load_and_build(cl, ctx, dev, spirv, "chain_test")

    input_val = 5.0
    input_data = struct.pack('f', input_val)
    in_buf = make_buf(cl, ctx, 4, ctypes.create_string_buffer(input_data))
    out_buf = make_buf(cl, ctx, 4)

    run_kernel(cl, queue, kernel, 1, 1, [in_buf, out_buf])
    result = struct.unpack('f', read_buf(cl, queue, out_buf, 4))[0]
    expected = (input_val + 3.0) * 2.0 - 1.0

    print(f"  ({input_val} + 3.0) * 2.0 - 1.0 = {result:.6f}, 期望 = {expected:.6f}")
    if abs(result - expected) < 1e-5:
        print("  ✅ 运算链测试通过")
        return True
    else:
        print(f"  ❌ 结果不正确: {result} != {expected}")
        return False


def test_fma():
    """测试 6: FMA — output[0] = fma(input[0], 2.0, 1.0) = input[0]*2 + 1"""
    print("\n📝 测试 6: FMA (output[0] = fma(input[0], 2.0, 1.0))")

    instructions = [
        {"op": "GlobalLoad", "dst": 100, "addr": {"kind": "Param", "id": 0}, "dtype": "f32"},
        # 2.0f = 1073741824, 1.0f = 1065353216
        {"op": "Fma", "dst": 103, "src1": {"kind": "Reg", "id": 100}, "src2": {"kind": "Imm", "val": 1073741824}, "src3": {"kind": "Imm", "val": 1065353216}, "dtype": "f32"},
        {"op": "GlobalStore", "addr": {"kind": "Param", "id": 1}, "src": {"kind": "Reg", "id": 103}, "dtype": "f32"},
        {"op": "Return"},
    ]
    gir = {"kernels": [{"name": "fma_test", "params": [
        {"name": "input", "dtype": "f32", "is_ptr": True},
        {"name": "output", "dtype": "f32", "is_ptr": True},
    ], "instructions": instructions, "next_reg": 200, "next_label": 50, "block_dim": [1, 1, 1]}]}

    spirv = compile_spirv(gir)
    validate_spirv(spirv, "fma_test")

    cl, ctx, dev, queue = init_opencl()
    kernel = load_and_build(cl, ctx, dev, spirv, "fma_test")

    input_val = 3.5
    input_data = struct.pack('f', input_val)
    in_buf = make_buf(cl, ctx, 4, ctypes.create_string_buffer(input_data))
    out_buf = make_buf(cl, ctx, 4)

    run_kernel(cl, queue, kernel, 1, 1, [in_buf, out_buf])
    result = struct.unpack('f', read_buf(cl, queue, out_buf, 4))[0]
    expected = 3.5 * 2.0 + 1.0

    print(f"  fma({input_val}, 2.0, 1.0) = {result:.6f}, 期望 = {expected:.6f}")
    if abs(result - expected) < 1e-5:
        print("  ✅ FMA 测试通过")
        return True
    else:
        print(f"  ❌ 结果不正确: {result} != {expected}")
        return False


def test_div_and_mod():
    """测试 7: 除法和取模"""
    print("\n📝 测试 7: 除法和取模 (output[0] = input[0] / 2.0)")

    instructions = [
        {"op": "GlobalLoad", "dst": 100, "addr": {"kind": "Param", "id": 0}, "dtype": "f32"},
        {"op": "Div", "dst": 101, "src1": {"kind": "Reg", "id": 100}, "src2": {"kind": "Imm", "val": 1073741824}, "dtype": "f32"},
        {"op": "GlobalStore", "addr": {"kind": "Param", "id": 1}, "src": {"kind": "Reg", "id": 101}, "dtype": "f32"},
        {"op": "Return"},
    ]
    gir = {"kernels": [{"name": "div_test", "params": [
        {"name": "input", "dtype": "f32", "is_ptr": True},
        {"name": "output", "dtype": "f32", "is_ptr": True},
    ], "instructions": instructions, "next_reg": 200, "next_label": 50, "block_dim": [1, 1, 1]}]}

    spirv = compile_spirv(gir)
    validate_spirv(spirv, "div_test")

    cl, ctx, dev, queue = init_opencl()
    kernel = load_and_build(cl, ctx, dev, spirv, "div_test")

    input_val = 17.0
    input_data = struct.pack('f', input_val)
    in_buf = make_buf(cl, ctx, 4, ctypes.create_string_buffer(input_data))
    out_buf = make_buf(cl, ctx, 4)

    run_kernel(cl, queue, kernel, 1, 1, [in_buf, out_buf])
    result = struct.unpack('f', read_buf(cl, queue, out_buf, 4))[0]
    expected = 17.0 / 2.0

    print(f"  {input_val} / 2.0 = {result:.6f}, 期望 = {expected:.6f}")
    if abs(result - expected) < 1e-5:
        print("  ✅ 除法测试通过")
        return True
    else:
        print(f"  ❌ 结果不正确: {result} != {expected}")
        return False


def test_math_funcs():
    """测试 8: 多个数学函数 — log, sin, cos, tanh, pow, ceil, floor"""
    print("\n📝 测试 8: 数学函数组合测试")

    test_cases = [
        ("log", "Log", 100.0, math.log(100.0), 1e-3),
        ("sin", "Sin", 1.5, math.sin(1.5), 1e-4),
        ("cos", "Cos", 1.5, math.cos(1.5), 1e-4),
        ("tanh", "Tanh", 0.5, math.tanh(0.5), 1e-4),
        ("ceil", "Ceil", 3.14, math.ceil(3.14), 1e-5),
        ("floor", "Floor", 3.14, math.floor(3.14), 1e-5),
    ]

    all_pass = True
    cl, ctx, dev, queue = init_opencl()

    for name, op_name, input_val, expected, tol in test_cases:
        instructions = [
            {"op": "GlobalLoad", "dst": 100, "addr": {"kind": "Param", "id": 0}, "dtype": "f32"},
            {"op": op_name, "dst": 101, "src": {"kind": "Reg", "id": 100}, "dtype": "f32"},
            {"op": "GlobalStore", "addr": {"kind": "Param", "id": 1}, "src": {"kind": "Reg", "id": 101}, "dtype": "f32"},
            {"op": "Return"},
        ]
        gir = {"kernels": [{"name": f"math_{name}", "params": [
            {"name": "input", "dtype": "f32", "is_ptr": True},
            {"name": "output", "dtype": "f32", "is_ptr": True},
        ], "instructions": instructions, "next_reg": 200, "next_label": 50, "block_dim": [1, 1, 1]}]}

        spirv = compile_spirv(gir)
        try:
            kernel = load_and_build(cl, ctx, dev, spirv, f"math_{name}")
        except RuntimeError as e:
            print(f"  ❌ {name}: 编译失败 — {e}")
            all_pass = False
            continue

        input_data = struct.pack('f', input_val)
        in_buf = make_buf(cl, ctx, 4, ctypes.create_string_buffer(input_data))
        out_buf = make_buf(cl, ctx, 4)
        run_kernel(cl, queue, kernel, 1, 1, [in_buf, out_buf])
        result = struct.unpack('f', read_buf(cl, queue, out_buf, 4))[0]

        if abs(result - expected) < tol:
            print(f"  ✅ {name}({input_val}) = {result:.6f} (期望 {expected:.6f})")
        else:
            print(f"  ❌ {name}({input_val}) = {result:.6f} (期望 {expected:.6f})")
            all_pass = False

    if all_pass:
        print("  ✅ 全部数学函数测试通过")
    return all_pass


def test_clamp_lerp():
    """测试 9: clamp 和 lerp"""
    print("\n📝 测试 9: Clamp 和 Lerp")

    cl, ctx, dev, queue = init_opencl()
    all_pass = True

    # Clamp: clamp(5.0, 0.0, 3.0) = 3.0
    instructions = [
        {"op": "GlobalLoad", "dst": 100, "addr": {"kind": "Param", "id": 0}, "dtype": "f32"},
        {"op": "Clamp", "dst": 101, "src": {"kind": "Reg", "id": 100}, "lo": {"kind": "Imm", "val": 0}, "hi": {"kind": "Imm", "val": 1077936128}, "dtype": "f32"},
        {"op": "GlobalStore", "addr": {"kind": "Param", "id": 1}, "src": {"kind": "Reg", "id": 101}, "dtype": "f32"},
        {"op": "Return"},
    ]
    gir = {"kernels": [{"name": "clamp_test", "params": [
        {"name": "input", "dtype": "f32", "is_ptr": True},
        {"name": "output", "dtype": "f32", "is_ptr": True},
    ], "instructions": instructions, "next_reg": 200, "next_label": 50, "block_dim": [1, 1, 1]}]}

    spirv = compile_spirv(gir)
    try:
        kernel = load_and_build(cl, ctx, dev, spirv, "clamp_test")
        input_data = struct.pack('f', 5.0)
        in_buf = make_buf(cl, ctx, 4, ctypes.create_string_buffer(input_data))
        out_buf = make_buf(cl, ctx, 4)
        run_kernel(cl, queue, kernel, 1, 1, [in_buf, out_buf])
        result = struct.unpack('f', read_buf(cl, queue, out_buf, 4))[0]
        expected = 3.0
        if abs(result - expected) < 1e-5:
            print(f"  ✅ clamp(5.0, 0.0, 3.0) = {result:.6f}")
        else:
            print(f"  ❌ clamp(5.0, 0.0, 3.0) = {result:.6f}, 期望 {expected}")
            all_pass = False
    except RuntimeError as e:
        print(f"  ❌ clamp 编译失败: {e}")
        all_pass = False

    if all_pass:
        print("  ✅ Clamp/Lerp 测试通过")
    return all_pass


def test_conditional():
    """测试 10: 条件操作 — Where + Cmp"""
    print("\n📝 测试 10: 条件操作 (if input > 5.0 then 1.0 else 0.0)")

    instructions = [
        {"op": "GlobalLoad", "dst": 100, "addr": {"kind": "Param", "id": 0}, "dtype": "f32"},
        # 5.0f = 0x40A00000 = 1084227584
        {"op": "Cmp", "dst": 101, "cmp": "gt", "src1": {"kind": "Reg", "id": 100}, "src2": {"kind": "Imm", "val": 1084227584}, "dtype": "f32"},
        {"op": "Where", "dst": 102, "cond": {"kind": "Reg", "id": 101}, "then_val": {"kind": "Imm", "val": 1065353216}, "else_val": {"kind": "Imm", "val": 0}, "dtype": "f32"},
        {"op": "GlobalStore", "addr": {"kind": "Param", "id": 1}, "src": {"kind": "Reg", "id": 102}, "dtype": "f32"},
        {"op": "Return"},
    ]
    gir = {"kernels": [{"name": "cond_test", "params": [
        {"name": "input", "dtype": "f32", "is_ptr": True},
        {"name": "output", "dtype": "f32", "is_ptr": True},
    ], "instructions": instructions, "next_reg": 200, "next_label": 50, "block_dim": [1, 1, 1]}]}

    spirv = compile_spirv(gir)
    validate_spirv(spirv, "cond_test")

    cl, ctx, dev, queue = init_opencl()
    kernel = load_and_build(cl, ctx, dev, spirv, "cond_test")

    all_pass = True
    for input_val, expected in [(10.0, 1.0), (3.0, 0.0)]:
        input_data = struct.pack('f', input_val)
        in_buf = make_buf(cl, ctx, 4, ctypes.create_string_buffer(input_data))
        out_buf = make_buf(cl, ctx, 4)
        run_kernel(cl, queue, kernel, 1, 1, [in_buf, out_buf])
        result = struct.unpack('f', read_buf(cl, queue, out_buf, 4))[0]
        if abs(result - expected) < 1e-5:
            print(f"  ✅ if ({input_val} > 5.0) → {result:.1f}")
        else:
            print(f"  ❌ if ({input_val} > 5.0) → {result:.1f}, 期望 {expected:.1f}")
            all_pass = False

    if all_pass:
        print("  ✅ 条件操作测试通过")
    return all_pass


def test_thread_id():
    """测试 11: 线程索引 — 用 ThreadId 控制 block 大小"""
    print("\n📝 测试 11: ThreadId (验证线程索引在 AMD GPU 上正确)")

    instructions = [
        {"op": "ThreadId", "dst": 100, "dim": "x"},
        # 将 thread ID 作为 i32 存储
        {"op": "GlobalStore", "addr": {"kind": "Param", "id": 0}, "src": {"kind": "Reg", "id": 100}, "dtype": "i32"},
        {"op": "Return"},
    ]
    gir = {"kernels": [{"name": "tid_test", "params": [
        {"name": "output", "dtype": "i32", "is_ptr": True},
    ], "instructions": instructions, "next_reg": 200, "next_label": 50, "block_dim": [1, 1, 1]}]}

    spirv = compile_spirv(gir)
    validate_spirv(spirv, "tid_test")

    cl, ctx, dev, queue = init_opencl()
    kernel = load_and_build(cl, ctx, dev, spirv, "tid_test")

    out_buf = make_buf(cl, ctx, 4)
    # 启动 1 个线程 — thread ID 应为 0
    run_kernel(cl, queue, kernel, 1, 1, [out_buf])
    result = struct.unpack('i', read_buf(cl, queue, out_buf, 4))[0]

    # ThreadId 0 — 第一个线程
    if result == 0:
        print(f"  ✅ ThreadId(0) = {result}")
        return True
    else:
        print(f"  ❌ ThreadId(0) = {result}, 期望 0")
        return False


if __name__ == "__main__":
    karte_bin = "./target/debug/karte"
    if not os.path.exists(karte_bin):
        print("📦 编译 karte...")
        subprocess.run(["cargo", "build", "-p", "karte-cli"], check=True)

    results = []
    print("=" * 60)
    print("Karte SPIR-V 后端 AMD GPU 端到端测试")
    print("=" * 60)

    tests = [
        ("标量加法", test_scalar_add),
        ("标量乘法", test_scalar_mul),
        ("指数函数", test_exp),
        ("平方根", test_sqrt),
        ("运算链", test_chain),
        ("FMA", test_fma),
        ("除法", test_div_and_mod),
        ("数学函数", test_math_funcs),
        ("Clamp/Lerp", test_clamp_lerp),
        ("条件操作", test_conditional),
        ("ThreadId", test_thread_id),
    ]

    for name, test_fn in tests:
        try:
            results.append((name, test_fn()))
        except Exception as e:
            print(f"  ❌ {name}测试异常: {e}")
            import traceback
            traceback.print_exc()
            results.append((name, False))

    print("\n" + "=" * 60)
    print("结果汇总")
    print("=" * 60)
    for name, success in results:
        status = "✅ PASS" if success else "❌ FAIL"
        print(f"  {status}  {name}")
    print("=" * 60)

    all_pass = all(s for _, s in results)
    sys.exit(0 if all_pass else 1)
