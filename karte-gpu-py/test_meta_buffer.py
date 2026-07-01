#!/usr/bin/env python3
"""复现 SPIR-V 多指针参数 meta buffer 加载问题"""
import json, subprocess, struct, ctypes, ctypes.util

def float_bits(v):
    return int(struct.unpack('I', struct.pack('f', float(v)))[0])

gir = {
    'kernels': [{
        'name': 'meta_load_test',
        'params': [
            {'name': 'x', 'dtype': 'f32', 'is_ptr': True},
            {'name': 'meta', 'dtype': 'f32', 'is_ptr': True},
            {'name': 'out', 'dtype': 'f32', 'is_ptr': True},
        ],
        'block_dim': [256, 1, 1],
        'next_reg': 100,
        'next_label': 10,
        'instructions': [
            {'op': 'ThreadId', 'dst': 0, 'dim': 'x'},
            # load x[tid]
            {'op': 'Mul', 'dst': 1, 'src1': {'kind': 'Reg', 'id': 0}, 'src2': {'kind': 'Imm', 'val': 4}, 'dtype': 'i64'},
            {'op': 'Add', 'dst': 1, 'src1': {'kind': 'Reg', 'id': 1}, 'src2': {'kind': 'Param', 'id': 0}, 'dtype': 'i64'},
            {'op': 'GlobalLoad', 'dst': 2, 'addr': {'kind': 'Reg', 'id': 1}, 'dtype': 'f32'},
            # load meta[0] = max (offset 0)
            {'op': 'Add', 'dst': 3, 'src1': {'kind': 'Imm', 'val': 0}, 'src2': {'kind': 'Param', 'id': 1}, 'dtype': 'i64'},
            {'op': 'GlobalLoad', 'dst': 4, 'addr': {'kind': 'Reg', 'id': 3}, 'dtype': 'f32'},
            # load meta[4] = sum (offset 4)
            {'op': 'Add', 'dst': 5, 'src1': {'kind': 'Imm', 'val': 4}, 'src2': {'kind': 'Param', 'id': 1}, 'dtype': 'i64'},
            {'op': 'GlobalLoad', 'dst': 6, 'addr': {'kind': 'Reg', 'id': 5}, 'dtype': 'f32'},
            # out = x + max + sum
            {'op': 'Add', 'dst': 7, 'src1': {'kind': 'Reg', 'id': 2}, 'src2': {'kind': 'Reg', 'id': 4}, 'dtype': 'f32'},
            {'op': 'Add', 'dst': 8, 'src1': {'kind': 'Reg', 'id': 7}, 'src2': {'kind': 'Reg', 'id': 6}, 'dtype': 'f32'},
            # store
            {'op': 'Mul', 'dst': 9, 'src1': {'kind': 'Reg', 'id': 0}, 'src2': {'kind': 'Imm', 'val': 4}, 'dtype': 'i64'},
            {'op': 'Add', 'dst': 9, 'src1': {'kind': 'Reg', 'id': 9}, 'src2': {'kind': 'Param', 'id': 2}, 'dtype': 'i64'},
            {'op': 'GlobalStore', 'addr': {'kind': 'Reg', 'id': 9}, 'src': {'kind': 'Reg', 'id': 8}, 'dtype': 'f32'},
            {'op': 'Return'},
        ],
    }]
}

result = subprocess.run(['./target/debug/karte', 'gpu-jit', '--backend', 'spirv'],
    input=json.dumps(gir).encode(), capture_output=True, timeout=30)
if result.returncode != 0:
    print('编译失败:', result.stderr.decode())
    exit(1)
spirv = result.stdout
print(f'SPIR-V: {len(spirv)} bytes')

# OpenCL 执行
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
cl.clEnqueueWriteBuffer.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint, ctypes.c_size_t, ctypes.c_size_t, ctypes.c_void_p, ctypes.c_uint, ctypes.c_void_p, ctypes.c_void_p]
cl.clEnqueueWriteBuffer.restype = ctypes.c_int
cl.clEnqueueReadBuffer.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint, ctypes.c_size_t, ctypes.c_size_t, ctypes.c_void_p, ctypes.c_uint, ctypes.c_void_p, ctypes.c_void_p]
cl.clEnqueueReadBuffer.restype = ctypes.c_int
cl.clCreateProgramWithIL.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t, ctypes.POINTER(ctypes.c_int)]
cl.clCreateProgramWithIL.restype = ctypes.c_void_p
cl.clBuildProgram.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.POINTER(ctypes.c_void_p), ctypes.c_char_p, ctypes.c_void_p, ctypes.c_void_p]
cl.clBuildProgram.restype = ctypes.c_int
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

spirv_buf = ctypes.create_string_buffer(spirv)
program = cl.clCreateProgramWithIL(ctx, spirv_buf, len(spirv), ctypes.byref(err))
ret = cl.clBuildProgram(program, 1, ctypes.byref(ctypes.c_void_p(sel_dev)), None, None, None)
if ret != 0:
    log_sz = ctypes.c_size_t(0)
    cl.clGetProgramBuildInfo(program, sel_dev, 0x1084, 0, None, ctypes.byref(log_sz))
    log_buf = ctypes.create_string_buffer(max(log_sz.value, 1))
    cl.clGetProgramBuildInfo(program, sel_dev, 0x1084, log_sz.value, log_buf, None)
    print('Build failed:', log_buf.value.decode())
    exit(1)

kernel = cl.clCreateKernel(program, b'meta_load_test', ctypes.byref(err))

N = 4
x_data = struct.pack(f'{N}f', 1, 2, 3, 4)
meta_data = struct.pack('2f', 10, 100)

CL_MEM_READ_WRITE = 1
CL_MEM_COPY_HOST_PTR = 1 << 5
x_buf = cl.clCreateBuffer(ctx, CL_MEM_READ_WRITE | CL_MEM_COPY_HOST_PTR, N*4, ctypes.cast(ctypes.create_string_buffer(x_data), ctypes.c_void_p), ctypes.byref(err))
meta_buf = cl.clCreateBuffer(ctx, CL_MEM_READ_WRITE | CL_MEM_COPY_HOST_PTR, 8, ctypes.cast(ctypes.create_string_buffer(meta_data), ctypes.c_void_p), ctypes.byref(err))
out_buf = cl.clCreateBuffer(ctx, CL_MEM_READ_WRITE, N*4, None, ctypes.byref(err))

for i, buf in enumerate([x_buf, meta_buf, out_buf]):
    arg_val = ctypes.c_uint64(ctypes.cast(buf, ctypes.c_void_p).value)
    cl.clSetKernelArg(kernel, i, 8, ctypes.byref(arg_val))

global_ws = (ctypes.c_size_t * 3)(N, 1, 1)
local_ws = (ctypes.c_size_t * 3)(N, 1, 1)
ret = cl.clEnqueueNDRangeKernel(queue, kernel, 1, None, global_ws, local_ws, 0, None, None)
print(f'kernel launch: {ret}')
cl.clFinish(queue)

out_host = ctypes.create_string_buffer(N * 4)
cl.clEnqueueReadBuffer(queue, out_buf, 1, 0, N * 4, out_host, 0, None, None)
results = struct.unpack(f'{N}f', out_host.raw)

print(f'x: {struct.unpack(f"{N}f", x_data)}')
print(f'meta: {struct.unpack("2f", meta_data)} (max=10, sum=100)')
print(f'GPU out: {results}')
expected_vals = [1+10+100, 2+10+100, 3+10+100, 4+10+100]
print(f'期望 out: {expected_vals}')
for i in range(N):
    if abs(results[i] - expected_vals[i]) > 1e-5:
        print(f'  ❌ out[{i}]={results[i]}, 期望 {expected_vals[i]}')
    else:
        print(f'  ✅ out[{i}]={results[i]}')
