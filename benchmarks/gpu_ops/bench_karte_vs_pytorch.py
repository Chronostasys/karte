"""Karte GPU vs PyTorch — RTX 5080"""
import ctypes, ctypes.util, numpy as np, time, sys, torch

cuda = ctypes.CDLL(ctypes.util.find_library('cuda'))
cuda.cuInit(0)  # 必须先初始化 Driver API

def cu_init():
    """在 PyTorch CUDA 初始化后调用 — 绑定到 PyTorch 的 context"""
    cuda.cuMemAlloc_v2.argtypes = [ctypes.POINTER(ctypes.c_uint64), ctypes.c_size_t]
    cuda.cuMemAlloc_v2.restype = ctypes.c_int
    cuda.cuMemcpyHtoD_v2.argtypes = [ctypes.c_uint64, ctypes.c_void_p, ctypes.c_size_t]
    cuda.cuMemcpyHtoD_v2.restype = ctypes.c_int
    cuda.cuMemcpyDtoH_v2.argtypes = [ctypes.c_void_p, ctypes.c_uint64, ctypes.c_size_t]
    cuda.cuMemcpyDtoH_v2.restype = ctypes.c_int
    cuda.cuLaunchKernel.argtypes = [ctypes.c_void_p, ctypes.c_uint,ctypes.c_uint,ctypes.c_uint, ctypes.c_uint,ctypes.c_uint,ctypes.c_uint, ctypes.c_uint, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p]
    cuda.cuLaunchKernel.restype = ctypes.c_int
    # 获取 PyTorch 创建的 primary context
    ctx = ctypes.c_void_p()
    cuda.cuDevicePrimaryCtxRetain(ctypes.byref(ctx), 0)
    cuda.cuCtxSetCurrent(ctx)
    cuda.cuMemAlloc_v2.restype = ctypes.c_int
    cuda.cuMemcpyHtoD_v2.argtypes = [ctypes.c_uint64, ctypes.c_void_p, ctypes.c_size_t]
    cuda.cuMemcpyHtoD_v2.restype = ctypes.c_int
    cuda.cuMemcpyDtoH_v2.argtypes = [ctypes.c_void_p, ctypes.c_uint64, ctypes.c_size_t]
    cuda.cuMemcpyDtoH_v2.restype = ctypes.c_int
    cuda.cuLaunchKernel.argtypes = [ctypes.c_void_p, ctypes.c_uint,ctypes.c_uint,ctypes.c_uint, ctypes.c_uint,ctypes.c_uint,ctypes.c_uint, ctypes.c_uint, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p]
    cuda.cuLaunchKernel.restype = ctypes.c_int

def cu_alloc(n):
    p = ctypes.c_uint64(0); cuda.cuMemAlloc_v2(ctypes.byref(p), n); return p.value
def cu_h2d(p, arr, n):
    cuda.cuMemcpyHtoD_v2(ctypes.c_uint64(p), arr.ctypes.data_as(ctypes.c_void_p), n)
def cu_d2h(p, n):
    a = np.empty(n//4, dtype=np.float32)
    cuda.cuMemcpyDtoH_v2(a.ctypes.data_as(ctypes.c_void_p), ctypes.c_uint64(p), n)
    return a
def cu_load(ptx):
    m = ctypes.c_void_p()
    ret = cuda.cuModuleLoadData(ctypes.byref(m), ptx.strip().encode()+b'\x00')
    assert ret==0, f"PTX load: {ret}"; return m
def cu_func(m, name):
    f = ctypes.c_void_p()
    ret = cuda.cuModuleGetFunction(ctypes.byref(f), m, name.encode())
    assert ret==0, f"GetFunc: {ret}"; return f
def cu_run(func, grid, block, args):
    n=len(args); buf=(ctypes.c_uint64*n)(*args); ptrs=(ctypes.c_void_p*n)()
    base=ctypes.addressof(buf)
    for i in range(n): ptrs[i]=base+i*8
    ret=cuda.cuLaunchKernel(func,grid,1,1,block,1,1,0,None,ptrs,None)
    assert ret==0, f"Launch: {ret}"; cuda.cuCtxSynchronize()

VEC_ADD = """
.version 8.0
.target sm_12_0
.address_size 64
.entry vec_add(.param .u64 a, .param .u64 b, .param .u64 out, .param .u32 n) {
.reg .u32 %r<16>; .reg .u64 %rd<16>; .reg .f32 %f<8>; .reg .pred %p<4>;
ld.param.u64 %rd0, [a]; ld.param.u64 %rd1, [b]; ld.param.u64 %rd2, [out]; ld.param.u32 %r3, [n];
mov.u32 %r0, %ctaid.x; mov.u32 %r1, %ntid.x; mov.u32 %r2, %tid.x;
mul.lo.u32 %r0, %r0, %r1; add.u32 %r4, %r0, %r2;
setp.ge.u32 %p0, %r4, %r3; @%p0 bra END;
cvt.u64.u32 %rd5, %r4; mul.lo.u64 %rd5, %rd5, 4;
add.u64 %rd6, %rd0, %rd5; ld.global.f32 %f0, [%rd6];
add.u64 %rd7, %rd1, %rd5; ld.global.f32 %f1, [%rd7];
add.f32 %f2, %f0, %f1;
add.u64 %rd8, %rd2, %rd5; st.global.f32 [%rd8], %f2;
END: ret;
}
"""

DOF_REW = """
.version 8.0
.target sm_12_0
.address_size 64
.entry dof_reward(.param .u64 ref_ptr, .param .u64 dof_ptr, .param .u64 out_ptr, .param .u32 ndof_val) {
.reg .u32 %r<24>; .reg .u64 %rd<24>; .reg .f32 %f<24>; .reg .pred %p<4>;
ld.param.u64 %rd0, [ref_ptr]; ld.param.u64 %rd1, [dof_ptr]; ld.param.u64 %rd2, [out_ptr]; ld.param.u32 %r3, [ndof_val];
mov.u32 %r0, %ctaid.x; mov.u32 %r1, %ntid.x; mov.u32 %r2, %tid.x;
mul.lo.u32 %r0, %r0, %r1; add.u32 %r4, %r0, %r2;
mul.lo.u32 %r5, %r4, %r3;
mov.f32 %f0, 0f00000000; mov.u32 %r6, 0;
LOOP:
setp.ge.u32 %p1, %r6, %r3; @%p1 bra LOOP_END;
add.u32 %r7, %r5, %r6;
cvt.u64.u32 %rd10, %r7; mul.lo.u64 %rd10, %rd10, 4;
add.u64 %rd11, %rd0, %rd10; ld.global.f32 %f1, [%rd11];
add.u64 %rd12, %rd1, %rd10; ld.global.f32 %f2, [%rd12];
sub.f32 %f3, %f1, %f2;
mul.f32 %f4, %f3, %f3; add.f32 %f0, %f4, %f0;
add.u32 %r6, %r6, 1; bra LOOP;
LOOP_END:
cvt.u64.u32 %rd20, %r4; mul.lo.u64 %rd20, %rd20, 4;
add.u64 %rd21, %rd2, %rd20; st.global.f32 [%rd21], %f0;
ret;
}
"""

def bench(fn, w=10, r=200):
    for _ in range(w): fn()
    torch.cuda.synchronize()
    t=[]
    for _ in range(r):
        s=time.perf_counter(); fn(); torch.cuda.synchronize(); t.append((time.perf_counter()-s)*1e6)
    return np.array(t)

def bench_cu(f,gr,bl,args,w=10,r=200):
    for _ in range(w): cu_run(f,gr,bl,args)
    t=[]
    for _ in range(r):
        s=time.perf_counter(); cu_run(f,gr,bl,args); t.append((time.perf_counter()-s)*1e6)
    return np.array(t)

N,DOF,BS,GR=4096,29,256,16
_ = torch.cuda.device_count()  # 确保 PyTorch CUDA 初始化
cu_init()  # 设置 CUDA 函数原型
print(f"GPU: {torch.cuda.get_device_name(0)}  SM: {torch.cuda.get_device_capability(0)}")
print(f"配置: {N} envs, {DOF} DOFs\n", flush=True)

f1=cu_func(cu_load(VEC_ADD),"vec_add")
f2=cu_func(cu_load(DOF_REW),"dof_reward")
print("Karte PTX 加载成功\n", flush=True)

# Test 1: vec_add
print("="*60); print(f"Test 1: vec_add [{N} f32]"); print("="*60)
a=np.random.randn(N).astype(np.float32); b=np.random.randn(N).astype(np.float32)
da,db,do=cu_alloc(N*4),cu_alloc(N*4),cu_alloc(N*4)
cu_h2d(da,a,N*4); cu_h2d(db,b,N*4)
cu_run(f1,GR,BS,[da,db,do,N])
out=cu_d2h(do,N*4); ref=a+b; d=np.abs(ref-out).max()
print(f"  正确性: max_diff={d:.2e} ({'PASS' if d<1e-5 else 'FAIL'})")
at=torch.from_numpy(a).cuda(); bt=torch.from_numpy(b).cuda()
t_pt=bench(lambda:at+bt); t_k=bench_cu(f1,GR,BS,[da,db,do,N])
print(f"  PyTorch: {t_pt.mean():.1f}µs   Karte: {t_k.mean():.1f}µs   →  {t_pt.mean()/t_k.mean():.2f}x\n")

# Test 2: dof_reward
print("="*60); print(f"Test 2: dof_reward [{N}×{DOF} fused]"); print("="*60)
rd=np.random.randn(N,DOF).astype(np.float32); dd=np.random.randn(N,DOF).astype(np.float32)
drd,ddd,do2=cu_alloc(N*DOF*4),cu_alloc(N*DOF*4),cu_alloc(N*4)
cu_h2d(drd,rd.reshape(-1),N*DOF*4); cu_h2d(ddd,dd.reshape(-1),N*DOF*4)
cu_run(f2,GR,BS,[drd,ddd,do2,DOF])
out2=cu_d2h(do2,N*4)
rdt=torch.from_numpy(rd).cuda(); ddt=torch.from_numpy(dd).cuda()
ref2=((rdt-ddt)**2).sum(dim=-1).cpu().numpy(); d2=np.abs(ref2-out2).max()
print(f"  正确性: max_diff={d2:.2e} ({'PASS' if d2<1e-2 else 'FAIL'})")
t_pt2=bench(lambda:((rdt-ddt)**2).sum(dim=-1)); t_k2=bench_cu(f2,GR,BS,[drd,ddd,do2,DOF])
print(f"  PyTorch: {t_pt2.mean():.1f}µs   Karte: {t_k2.mean():.1f}µs   →  {t_pt2.mean()/t_k2.mean():.2f}x\n")

# Test 3: 30×
print("="*60); print(f"Test 3: 30× dof_reward"); print("="*60)
def pt30():
    for _ in range(30): ((rdt-ddt)**2).sum(dim=-1)
def k30():
    for _ in range(30): cu_run(f2,GR,BS,[drd,ddd,do2,DOF])
t_pt3=bench(pt30,w=5,r=100)
t_k3=bench(lambda:[cu_run(f2,GR,BS,[drd,ddd,do2,DOF]) for _ in range(30)],w=5,r=100)
print(f"  PyTorch 30×: {t_pt3.mean():.1f}µs   Karte 30×: {t_k3.mean():.1f}µs   →  {t_pt3.mean()/t_k3.mean():.2f}x\n")

print("="*60); print("SUMMARY: Karte GPU vs PyTorch"); print("="*60)
print(f"  vec_add:     Karte {t_k.mean():.1f}µs vs PyTorch {t_pt.mean():.1f}µs → {t_pt.mean()/t_k.mean():.2f}x")
print(f"  dof_reward:  Karte {t_k2.mean():.1f}µs vs PyTorch {t_pt2.mean():.1f}µs → {t_pt2.mean()/t_k2.mean():.2f}x")
print(f"  30×dof:      Karte {t_k3.mean():.1f}µs vs PyTorch {t_pt3.mean():.1f}µs → {t_pt3.mean()/t_k3.mean():.2f}x")
