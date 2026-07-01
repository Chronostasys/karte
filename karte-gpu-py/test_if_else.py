#!/usr/bin/env python3
"""测试 if/else AST 解释器 — 生成 GIR JSON 并在 AMD GPU 上执行"""

import sys
sys.path.insert(0, 'karte-gpu-py')

import json
import struct
import math
import ctypes
import ctypes.util
import subprocess

# 导入 karte 模块
from karte_gpu import karte_jit
from karte_gpu.karte_jit import (
    jit, Tensor, thread_id, f32, exp, sqrt, log,
    _GirBuilder, _current_builder, _SymTensor, _SymF32, _RegRef,
    _AstInterpreter, _compile, _build_gir_json, _emit_return_store,
    _operand_to_json
)
import numpy as np

def trace_if_else():
    """测试1: if/else 简单选择 — 验证 GIR 生成"""
    print("📝 测试 1: if/else GIR 生成")

    # 定义一个带 if/else 的 kernel
    def kernel(x, output):
        tid = thread_id()
        val = x[tid]
        if val > 0.0:
            y = exp(val)
        else:
            y = f32(-1.0)
        return y

    # 手动运行 _compile 的前半部分来获取 GIR
    import inspect
    import textwrap
    import ast

    builder = _GirBuilder()
    karte_jit._current_builder = builder

    # 设置参数（模拟 _compile 的参数准备）
    builder.params.append(('x', 'u64', True, [1]))
    builder.params.append(('output', 'u64', True, [1]))
    builder.output_param_idx = 1
    builder._tid_reg = 0

    sym_x = _SymTensor(builder, 0, 'x', [1])

    fn_globals = kernel.__globals__.copy()
    fn_globals['thread_id'] = thread_id
    fn_globals['f32'] = f32
    fn_globals['exp'] = exp
    fn_globals['sqrt'] = sqrt
    fn_globals['log'] = log

    interp = _AstInterpreter(builder, fn_globals, ['x', 'output'], [sym_x, None])
    source_tree = ast.parse(textwrap.dedent(inspect.getsource(kernel)))
    func_def = None
    for node in ast.walk(source_tree):
        if isinstance(node, ast.FunctionDef):
            func_def = node
            break

    # 需要先调用 thread_id 设置 _tid_reg
    # 实际上 thread_id() 会在 _expr 中被调用，此时 _current_builder 已经设置
    interp.run(func_def.body)
    builder.return_regs = interp.return_regs if interp.return_regs else []

    # 发射 return store
    _emit_return_store(builder)

    # 生成 GIR JSON
    gir_json = _build_gir_json(builder, "test_if_else")

    print("GIR 指令:")
    for instr in gir_json["kernels"][0]["instructions"]:
        print(f"  {json.dumps(instr)}")

    # 验证关键指令存在
    ops = [i["op"] for i in gir_json["kernels"][0]["instructions"]]
    assert "Cmp" in ops, "缺少 Cmp 指令"
    assert "BranchIf" in ops, "缺少 BranchIf 指令"
    assert "Label" in ops, "缺少 Label 指令"
    assert "Jump" in ops, "缺少 Jump 指令"
    assert "Where" in ops, "缺少 Where 指令 (merge)"
    assert "Exp" in ops, "缺少 Exp 指令 (then branch)"
    print("  ✅ GIR 生成正确 — Cmp/BranchIf/Label/Jump/Where/Exp 全部存在")
    return gir_json


def test_boundary_check_gir():
    """测试2: 边界检查 if tid < N — 验证 i32 比较"""
    print("\n📝 测试 2: 边界检查 if tid < N")

    import inspect
    import textwrap
    import ast

    builder = _GirBuilder()
    karte_jit._current_builder = builder

    builder.params.append(('x', 'u64', True, [1]))
    builder.params.append(('output', 'u64', True, [1]))
    builder.params.append(('N', 's32', False, None))
    builder.output_param_idx = 1
    builder._tid_reg = 0

    sym_x = _SymTensor(builder, 0, 'x', [1])
    sym_output = _SymTensor(builder, 1, 'output', [1])

    fn_globals = {
        'thread_id': thread_id,
        'f32': f32,
    }

    def kernel(x, output, N):
        tid = thread_id()
        if tid < N:
            output[tid] = x[tid] * 2.0
        return None

    interp = _AstInterpreter(builder, fn_globals, ['x', 'output', 'N'], [sym_x, sym_output, 1024])
    source_tree = ast.parse(textwrap.dedent(inspect.getsource(kernel)))
    func_def = None
    for node in ast.walk(source_tree):
        if isinstance(node, ast.FunctionDef):
            func_def = node
            break

    interp.run(func_def.body)
    builder.return_regs = interp.return_regs if interp.return_regs else []

    gir_json = _build_gir_json(builder, "test_boundary")
    print("GIR 指令:")
    for instr in gir_json["kernels"][0]["instructions"]:
        print(f"  {json.dumps(instr)}")

    ops = [i["op"] for i in gir_json["kernels"][0]["instructions"]]
    assert "Cmp" in ops, "缺少 Cmp 指令"
    assert "BranchIf" in ops, "缺少 BranchIf 指令"
    assert "GlobalStore" in ops, "缺少 GlobalStore (in if body)"
    print("  ✅ 边界检查 GIR 正确 — i32 Cmp + BranchIf + GlobalStore")
    return gir_json


def test_nested_if_else_gir():
    """测试3: 嵌套 if/elif/else"""
    print("\n📝 测试 3: 嵌套 if/elif/else")

    import inspect
    import textwrap
    import ast

    builder = _GirBuilder()
    karte_jit._current_builder = builder

    builder.params.append(('x', 'u64', True, [1]))
    builder.params.append(('output', 'u64', True, [1]))
    builder.output_param_idx = 1
    builder._tid_reg = 0

    sym_x = _SymTensor(builder, 0, 'x', [1])

    fn_globals = {'thread_id': thread_id, 'f32': f32, 'exp': exp}

    def kernel(x, output):
        tid = thread_id()
        val = x[tid]
        if val > 1.0:
            y = exp(val)
        elif val > 0.0:
            y = f32(1.0)
        else:
            y = f32(-1.0)
        return y

    interp = _AstInterpreter(builder, fn_globals, ['x', 'output'], [sym_x, None])
    source_tree = ast.parse(textwrap.dedent(inspect.getsource(kernel)))
    func_def = None
    for node in ast.walk(source_tree):
        if isinstance(node, ast.FunctionDef):
            func_def = node
            break

    interp.run(func_def.body)
    builder.return_regs = interp.return_regs if interp.return_regs else []
    _emit_return_store(builder)
    gir_json = _build_gir_json(builder, "test_nested")

    print("GIR 指令:")
    for instr in gir_json["kernels"][0]["instructions"]:
        print(f"  {json.dumps(instr)}")

    ops = [i["op"] for i in gir_json["kernels"][0]["instructions"]]
    cmp_count = ops.count("Cmp")
    branch_count = ops.count("BranchIf")
    assert cmp_count >= 2, f"期望至少 2 个 Cmp，得到 {cmp_count}"
    assert branch_count >= 2, f"期望至少 2 个 BranchIf，得到 {branch_count}"
    print(f"  ✅ 嵌套 if/elif/else 正确 — {cmp_count} Cmp, {branch_count} BranchIf")
    return gir_json


def test_amd_gpu_if_else():
    """测试4: AMD GPU 端到端执行 — if/else kernel"""
    print("\n📝 测试 4: AMD GPU 端到端执行 if/else")

    gir_json = trace_if_else()

    # 编译为 SPIR-V
    result = subprocess.run(
        ["./target/debug/karte", "gpu-jit", "--backend", "spirv"],
        input=json.dumps(gir_json).encode(),
        capture_output=True,
        timeout=30,
    )
    if result.returncode != 0:
        print(f"  ❌ SPIR-V 编译失败: {result.stderr.decode()}")
        return False
    spirv = result.stdout
    print(f"  SPIR-V 大小: {len(spirv)} bytes")

    # 验证
    with open("/tmp/if_else.spirv", "wb") as f:
        f.write(spirv)
    val_result = subprocess.run(["spirv-val", "/tmp/if_else.spirv"], capture_output=True, text=True)
    if val_result.returncode != 0:
        print(f"  ⚠️ spirv-val: {val_result.stderr.strip()}")
    else:
        print(f"  ✅ spirv-val 验证通过")

    # 加载到 AMD GPU
    cl_lib = ctypes.CDLL(ctypes.util.find_library('OpenCL'))
    # ... (OpenCL 设置代码，与 test_amd_spirv.py 相同)

    # 简化: 直接用 ctypes 调用
    # 设置函数签名
    cl_lib.clGetPlatformIDs.argtypes = [ctypes.c_uint, ctypes.POINTER(ctypes.c_void_p), ctypes.POINTER(ctypes.c_uint)]
    cl_lib.clGetPlatformIDs.restype = ctypes.c_int
    cl_lib.clGetPlatformInfo.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_size_t, ctypes.c_void_p, ctypes.POINTER(ctypes.c_size_t)]
    cl_lib.clGetPlatformInfo.restype = ctypes.c_int
    cl_lib.clGetDeviceIDs.argtypes = [ctypes.c_void_p, ctypes.c_uint64, ctypes.c_uint, ctypes.POINTER(ctypes.c_void_p), ctypes.POINTER(ctypes.c_uint)]
    cl_lib.clGetDeviceIDs.restype = ctypes.c_int
    cl_lib.clGetDeviceInfo.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_size_t, ctypes.c_void_p, ctypes.POINTER(ctypes.c_size_t)]
    cl_lib.clGetDeviceInfo.restype = ctypes.c_int
    cl_lib.clCreateContext.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.POINTER(ctypes.c_void_p), ctypes.c_void_p, ctypes.c_void_p, ctypes.POINTER(ctypes.c_int)]
    cl_lib.clCreateContext.restype = ctypes.c_void_p
    cl_lib.clCreateCommandQueueWithProperties.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.POINTER(ctypes.c_int)]
    cl_lib.clCreateCommandQueueWithProperties.restype = ctypes.c_void_p
    cl_lib.clCreateBuffer.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_size_t, ctypes.c_void_p, ctypes.POINTER(ctypes.c_int)]
    cl_lib.clCreateBuffer.restype = ctypes.c_void_p
    cl_lib.clEnqueueWriteBuffer.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint, ctypes.c_size_t, ctypes.c_size_t, ctypes.c_void_p, ctypes.c_uint, ctypes.c_void_p, ctypes.c_void_p]
    cl_lib.clEnqueueWriteBuffer.restype = ctypes.c_int
    cl_lib.clEnqueueReadBuffer.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint, ctypes.c_size_t, ctypes.c_size_t, ctypes.c_void_p, ctypes.c_uint, ctypes.c_void_p, ctypes.c_void_p]
    cl_lib.clEnqueueReadBuffer.restype = ctypes.c_int
    cl_lib.clCreateProgramWithIL.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t, ctypes.POINTER(ctypes.c_int)]
    cl_lib.clCreateProgramWithIL.restype = ctypes.c_void_p
    cl_lib.clBuildProgram.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.POINTER(ctypes.c_void_p), ctypes.c_char_p, ctypes.c_void_p, ctypes.c_void_p]
    cl_lib.clBuildProgram.restype = ctypes.c_int
    cl_lib.clGetProgramBuildInfo.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint, ctypes.c_size_t, ctypes.c_void_p, ctypes.POINTER(ctypes.c_size_t)]
    cl_lib.clGetProgramBuildInfo.restype = ctypes.c_int
    cl_lib.clCreateKernel.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.POINTER(ctypes.c_int)]
    cl_lib.clCreateKernel.restype = ctypes.c_void_p
    cl_lib.clSetKernelArg.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_size_t, ctypes.c_void_p]
    cl_lib.clSetKernelArg.restype = ctypes.c_int
    cl_lib.clEnqueueNDRangeKernel.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint, ctypes.POINTER(ctypes.c_size_t), ctypes.POINTER(ctypes.c_size_t), ctypes.POINTER(ctypes.c_size_t), ctypes.c_uint, ctypes.c_void_p, ctypes.c_void_p]
    cl_lib.clEnqueueNDRangeKernel.restype = ctypes.c_int
    cl_lib.clFinish.argtypes = [ctypes.c_void_p]
    cl_lib.clFinish.restype = ctypes.c_int

    # 选择 Rusticl 平台
    num_plat = ctypes.c_uint(0)
    cl_lib.clGetPlatformIDs(0, None, ctypes.byref(num_plat))
    platforms = (ctypes.c_void_p * num_plat.value)()
    cl_lib.clGetPlatformIDs(num_plat.value, platforms, None)

    CL_PLATFORM_NAME = 0x0902
    CL_DEVICE_TYPE_GPU = 1 << 2
    CL_DEVICE_NAME = 0x1027

    sel_dev = None
    sel_plat = None
    for p in platforms:
        sz = ctypes.c_size_t(0)
        cl_lib.clGetPlatformInfo(p, CL_PLATFORM_NAME, 0, None, ctypes.byref(sz))
        nb = ctypes.create_string_buffer(sz.value)
        cl_lib.clGetPlatformInfo(p, CL_PLATFORM_NAME, sz.value, nb, None)
        pname = nb.value.decode()
        if 'rusticl' in pname.lower():
            nd = ctypes.c_uint(0)
            cl_lib.clGetDeviceIDs(p, CL_DEVICE_TYPE_GPU, 0, None, ctypes.byref(nd))
            if nd.value > 0:
                devs = (ctypes.c_void_p * nd.value)()
                cl_lib.clGetDeviceIDs(p, CL_DEVICE_TYPE_GPU, nd.value, devs, None)
                sel_plat = p
                sel_dev = devs[0]
                sz2 = ctypes.c_size_t(0)
                cl_lib.clGetDeviceInfo(sel_dev, CL_DEVICE_NAME, 0, None, ctypes.byref(sz2))
                db = ctypes.create_string_buffer(sz2.value)
                cl_lib.clGetDeviceInfo(sel_dev, CL_DEVICE_NAME, sz2.value, db, None)
                print(f"  GPU: {db.value.decode()}")
                break

    if not sel_dev:
        print("  ❌ 未找到 Rusticl GPU")
        return False

    err = ctypes.c_int(0)
    ctx = cl_lib.clCreateContext(None, 1, ctypes.byref(ctypes.c_void_p(sel_dev)), None, None, ctypes.byref(err))
    queue = cl_lib.clCreateCommandQueueWithProperties(ctx, sel_dev, None, ctypes.byref(err))

    # 加载 SPIR-V
    spirv_buf = ctypes.create_string_buffer(spirv)
    program = cl_lib.clCreateProgramWithIL(ctx, spirv_buf, len(spirv), ctypes.byref(err))
    if err.value != 0:
        print(f"  ❌ clCreateProgramWithIL failed: {err.value}")
        return False

    ret = cl_lib.clBuildProgram(program, 1, ctypes.byref(ctypes.c_void_p(sel_dev)), None, None, None)
    if ret != 0:
        log_sz = ctypes.c_size_t(0)
        cl_lib.clGetProgramBuildInfo(program, sel_dev, 0x1084, 0, None, ctypes.byref(log_sz))
        log_buf = ctypes.create_string_buffer(max(log_sz.value, 1))
        cl_lib.clGetProgramBuildInfo(program, sel_dev, 0x1084, log_sz.value, log_buf, None)
        print(f"  ❌ clBuildProgram failed ({ret}):\n{log_buf.value.decode('utf-8', errors='replace')}")
        return False

    kernel = cl_lib.clCreateKernel(program, b"test_if_else", ctypes.byref(err))
    if err.value != 0:
        print(f"  ❌ clCreateKernel failed: {err.value}")
        return False

    # 准备测试数据
    N = 8
    input_vals = [0.5, -2.0, 1.0, -0.5, 2.0, -1.0, 0.0, 3.0]
    input_data = struct.pack(f'{N}f', *input_vals)

    CL_MEM_READ_WRITE = 1
    CL_MEM_COPY_HOST_PTR = 1 << 5
    host_buf = ctypes.create_string_buffer(input_data)
    in_buf = cl_lib.clCreateBuffer(ctx, CL_MEM_READ_WRITE | CL_MEM_COPY_HOST_PTR, N * 4,
                                    ctypes.cast(host_buf, ctypes.c_void_p), ctypes.byref(err))
    out_buf = cl_lib.clCreateBuffer(ctx, CL_MEM_READ_WRITE, N * 4, None, ctypes.byref(err))

    # 启动 kernel
    for i, buf in enumerate([in_buf, out_buf]):
        arg_val = ctypes.c_uint64(ctypes.cast(buf, ctypes.c_void_p).value)
        cl_lib.clSetKernelArg(kernel, i, 8, ctypes.byref(arg_val))

    # block_dim 是 [1,1,1]，所以 global = N, local = 1
    global_ws = (ctypes.c_size_t * 3)(N, 1, 1)
    local_ws = (ctypes.c_size_t * 3)(1, 1, 1)
    ret = cl_lib.clEnqueueNDRangeKernel(queue, kernel, 1, None, global_ws, local_ws, 0, None, None)
    if ret != 0:
        print(f"  ❌ clEnqueueNDRangeKernel failed: {ret}")
        return False
    cl_lib.clFinish(queue)

    # 读回结果
    out_host = ctypes.create_string_buffer(N * 4)
    cl_lib.clEnqueueReadBuffer(queue, out_buf, 1, 0, N * 4, out_host, 0, None, None)
    results = struct.unpack(f'{N}f', out_host.raw)

    # 验证
    all_pass = True
    for i in range(N):
        x = input_vals[i]
        if x > 0.0:
            expected = math.exp(x)
        else:
            expected = -1.0
        actual = results[i]
        tol = 1e-3
        if abs(actual - expected) > tol:
            print(f"  ❌ input[{i}]={x} → output={actual:.6f}, 期望={expected:.6f}")
            all_pass = False
        else:
            print(f"  ✅ input[{i}]={x:6.2f} → output={actual:.6f} (期望={expected:.6f})")

    if all_pass:
        print("  ✅ if/else AMD GPU 端到端测试通过！")
    return all_pass


if __name__ == "__main__":
    results = []
    print("=" * 60)
    print("Karte if/else 支持 — 完整测试")
    print("=" * 60)

    try:
        gir = trace_if_else()
        results.append(("if/else GIR 生成", True))
    except Exception as e:
        import traceback
        traceback.print_exc()
        results.append(("if/else GIR 生成", False))

    try:
        gir = test_boundary_check_gir()
        results.append(("边界检查 GIR", True))
    except Exception as e:
        import traceback
        traceback.print_exc()
        results.append(("边界检查 GIR", False))

    try:
        gir = test_nested_if_else_gir()
        results.append(("嵌套 if/elif/else GIR", True))
    except Exception as e:
        import traceback
        traceback.print_exc()
        results.append(("嵌套 if/elif/else GIR", False))

    try:
        passed = test_amd_gpu_if_else()
        results.append(("AMD GPU 端到端", passed))
    except Exception as e:
        import traceback
        traceback.print_exc()
        results.append(("AMD GPU 端到端", False))

    print("\n" + "=" * 60)
    print("结果汇总")
    print("=" * 60)
    for name, ok in results:
        print(f"  {'✅ PASS' if ok else '❌ FAIL'}  {name}")
    print("=" * 60)

    sys.exit(0 if all(ok for _, ok in results) else 1)
