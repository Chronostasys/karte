#!/usr/bin/env python3
"""
Tiled GEMM 端到端真机验证 — 使用 tile_expansion pass
在 AMD GPU 上执行 4×4 矩阵乘法, 验证 tile 指令展开后正确性
"""
import json
import subprocess
import struct
import ctypes
import ctypes.util
import math

def float_bits(v):
    return int(struct.unpack('I', struct.pack('f', float(v)))[0])

def make_tiled_gemm_gir():
    """构建使用 Tile 指令的 GEMM kernel GIR JSON"""
    m = 4; k = 4; n = 4

    return {
        "kernels": [{
            "name": "tiled_gemm",
            "params": [
                {"name": "A", "dtype": "f32", "is_ptr": True},
                {"name": "B", "dtype": "f32", "is_ptr": True},
                {"name": "C", "dtype": "f32", "is_ptr": True},
            ],
            "block_dim": [16, 1, 1],  # 4x4=16 个线程
            "next_reg": 100,
            "next_label": 10,
            "instructions": [
                # TileZeros: 初始化累加器
                {"op": "TileZeros", "dst": 0, "tile_rows": m, "tile_cols": n, "dtype": "f32"},

                # TileLoad A: 从全局内存加载 A[0:4, 0:4] 到 shared memory
                {"op": "TileLoad", "dst": 1, "base": {"kind": "Param", "id": 0},
                 "row": {"kind": "Imm", "val": 0}, "col": {"kind": "Imm", "val": 0},
                 "tile_rows": m, "tile_cols": k, "stride": {"kind": "Imm", "val": k}, "dtype": "f32"},

                # TileLoad B: 从全局内存加载 B[0:4, 0:4] 到 shared memory
                {"op": "TileLoad", "dst": 2, "base": {"kind": "Param", "id": 1},
                 "row": {"kind": "Imm", "val": 0}, "col": {"kind": "Imm", "val": 0},
                 "tile_rows": k, "tile_cols": n, "stride": {"kind": "Imm", "val": n}, "dtype": "f32"},

                # TileMatmul: C += A @ B
                {"op": "TileMatmul", "dst": 0, "a": 1, "b": 2,
                 "m": m, "k": k, "n": n,
                 "dtype_a": "f32", "dtype_b": "f32", "dtype_c": "f32"},

                # TileStore: 将结果写回全局内存
                {"op": "TileStore", "base": {"kind": "Param", "id": 2},
                 "row": {"kind": "Imm", "val": 0}, "col": {"kind": "Imm", "val": 0},
                 "src": 0, "tile_rows": m, "tile_cols": n,
                 "stride": {"kind": "Imm", "val": n}, "dtype": "f32"},

                {"op": "Return"},
            ],
        }]
    }

def compile_with_tile_expansion(gir_json):
    """使用 karte CLI 编译 GIR (tile_expansion pass 自动展开)"""
    result = subprocess.run(
        ["./target/debug/karte", "gpu-jit", "--backend", "spirv"],
        input=json.dumps(gir_json).encode(),
        capture_output=True,
        timeout=30,
    )
    if result.returncode != 0:
        print(f"编译失败: {result.stderr.decode()}")
        return None
    return result.stdout

def run_on_amd_gpu(spirv_bytes, kernel_name, input_arrays, output_size):
    """在 AMD GPU 上执行 SPIR-V kernel"""
    cl = ctypes.CDLL(ctypes.util.find_library('OpenCL'))

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

    num_plat = ctypes.c_uint(0)
    cl.clGetPlatformIDs(0, None, ctypes.byref(num_plat))
    platforms = (ctypes.c_void_p * num_plat.value)()
    cl.clGetPlatformIDs(num_plat.value, platforms, None)
    sel_dev = None
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
            sel_dev = devs[0]
            break

    err = ctypes.c_int(0)
    ctx = cl.clCreateContext(None, 1, ctypes.byref(ctypes.c_void_p(sel_dev)), None, None, ctypes.byref(err))
    queue = cl.clCreateCommandQueueWithProperties(ctx, sel_dev, None, ctypes.byref(err))

    spirv_buf = ctypes.create_string_buffer(spirv_bytes)
    program = cl.clCreateProgramWithIL(ctx, spirv_buf, len(spirv_bytes), ctypes.byref(err))
    ret = cl.clBuildProgram(program, 1, ctypes.byref(ctypes.c_void_p(sel_dev)), None, None, None)
    if ret != 0:
        log_sz = ctypes.c_size_t(0)
        cl.clGetProgramBuildInfo(program, sel_dev, 0x1084, 0, None, ctypes.byref(log_sz))
        log_buf = ctypes.create_string_buffer(max(log_sz.value, 1))
        cl.clGetProgramBuildInfo(program, sel_dev, 0x1084, log_sz.value, log_buf, None)
        log_text = log_buf.value.decode('utf-8', errors='replace')
        raise RuntimeError(f"clBuildProgram failed ({ret}):\n{log_text if log_text.strip() else '(empty build log)'}\n\nSPIR-V size: {len(spirv_bytes)} bytes")

    kernel = cl.clCreateKernel(program, kernel_name.encode(), ctypes.byref(err))

    CL_MEM_READ_WRITE = 1
    CL_MEM_COPY_HOST_PTR = 1 << 5
    buffers = []
    for data_bytes, nbytes in input_arrays:
        host_buf = ctypes.create_string_buffer(data_bytes)
        buf = cl.clCreateBuffer(ctx, CL_MEM_READ_WRITE | CL_MEM_COPY_HOST_PTR, nbytes, ctypes.cast(host_buf, ctypes.c_void_p), ctypes.byref(err))
        buffers.append(buf)

    out_nbytes = output_size * 4
    out_buf = cl.clCreateBuffer(ctx, CL_MEM_READ_WRITE, out_nbytes, None, ctypes.byref(err))
    buffers.append(out_buf)

    for i, buf in enumerate(buffers):
        arg_val = ctypes.c_uint64(ctypes.cast(buf, ctypes.c_void_p).value)
        cl.clSetKernelArg(kernel, i, 8, ctypes.byref(arg_val))

    N = output_size
    global_ws = (ctypes.c_size_t * 3)(N, 1, 1)
    local_ws = (ctypes.c_size_t * 3)(N, 1, 1)
    ret = cl.clEnqueueNDRangeKernel(queue, kernel, 1, None, global_ws, local_ws, 0, None, None)
    if ret != 0:
        raise RuntimeError(f"clEnqueueNDRangeKernel failed: {ret}")
    cl.clFinish(queue)

    out_host = ctypes.create_string_buffer(out_nbytes)
    cl.clEnqueueReadBuffer(queue, out_buf, 1, 0, out_nbytes, out_host, 0, None, None)
    return list(struct.unpack(f'{output_size}f', out_host.raw))


def main():
    print("=" * 60)
    print("Tiled GEMM 端到端真机验证 — tile_expansion pass")
    print("=" * 60)

    # 构建 tiled GEMM GIR
    gir = make_tiled_gemm_gir()
    print(f"\n  GIR 构建: 4x4 tiled GEMM (TileZeros + TileLoad + TileMatmul + TileStore)")

    # 编译为 SPIR-V (karte CLI 会自动调用 tile_expansion pass)
    spirv = compile_with_tile_expansion(gir)
    if spirv is None:
        print("  ❌ SPIR-V 编译失败")
        return False
    print(f"  SPIR-V 编译: {len(spirv)} bytes ✅")

    # 测试数据
    A = [1, 2, 3, 4,  5, 6, 7, 8,  1, 1, 1, 1,  2, 2, 2, 2]  # 4x4
    B = [1, 0, 0, 1,  0, 1, 0, 1,  0, 0, 1, 1,  1, 1, 1, 0]  # 4x4
    N = 16  # 4x4=16 个输出

    A_bytes = struct.pack(f'{N}f', *A)
    B_bytes = struct.pack(f'{N}f', *B)

    # 在 AMD GPU 上执行
    try:
        gpu_results = run_on_amd_gpu(spirv, "tiled_gemm", [(A_bytes, N*4), (B_bytes, N*4)], N)
    except Exception as e:
        print(f"  ❌ GPU 执行失败: {e}")
        return False

    # PyTorch 参考实现
    try:
        import torch
        A_t = torch.tensor(A, dtype=torch.float32).reshape(4, 4)
        B_t = torch.tensor(B, dtype=torch.float32).reshape(4, 4)
        C_ref = (A_t @ B_t).flatten().tolist()
    except ImportError:
        # 手动计算
        C_ref = []
        for i in range(4):
            for j in range(4):
                s = sum(A[i*4+kk] * B[kk*4+j] for kk in range(4))
                C_ref.append(s)

    print(f"\n  矩阵 A (4x4):")
    for i in range(4):
        print(f"    {A[i*4:(i+1)*4]}")
    print(f"\n  矩阵 B (4x4):")
    for i in range(4):
        print(f"    {B[i*4:(i+1)*4]}")

    print(f"\n  GPU 结果:  {[round(v, 2) for v in gpu_results]}")
    print(f"  期望结果:  {[round(v, 2) for v in C_ref]}")

    all_pass = True
    for i in range(N):
        if abs(gpu_results[i] - C_ref[i]) > 1e-5:
            print(f"  ❌ C[{i//4}][{i%4}] = {gpu_results[i]:.4f}, 期望 {C_ref[i]:.4f}")
            all_pass = False

    if all_pass:
        print("\n  ✅ Tiled GEMM AMD GPU 端到端验证通过！")
        print("  ✅ tile_expansion pass 正确展开 TileLoad/TileMatmul/TileStore")
        print("  ✅ Shared memory + Barrier + FMA 循环在 AMD GPU 上正确执行")

    print("\n" + "=" * 60)
    return all_pass


if __name__ == "__main__":
    success = main()
    exit(0 if success else 1)
