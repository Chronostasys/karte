"""Karte 优化前后对比 — RTX 5080"""
import ctypes, ctypes.util, numpy as np, time, torch

cuda = ctypes.CDLL(ctypes.util.find_library('cuda'))
cuda.cuInit(0)
cuda.cuMemAlloc_v2.argtypes = [ctypes.POINTER(ctypes.c_uint64), ctypes.c_size_t]; cuda.cuMemAlloc_v2.restype = ctypes.c_int
cuda.cuMemcpyHtoD_v2.argtypes = [ctypes.c_uint64, ctypes.c_void_p, ctypes.c_size_t]; cuda.cuMemcpyHtoD_v2.restype = ctypes.c_int
cuda.cuMemcpyDtoH_v2.argtypes = [ctypes.c_void_p, ctypes.c_uint64, ctypes.c_size_t]; cuda.cuMemcpyDtoH_v2.restype = ctypes.c_int
cuda.cuLaunchKernel.argtypes = [ctypes.c_void_p]+[ctypes.c_uint]*7+[ctypes.c_void_p]*3; cuda.cuLaunchKernel.restype = ctypes.c_int
_ = torch.cuda.device_count()
ctx = ctypes.c_void_p(); cuda.cuDevicePrimaryCtxRetain(ctypes.byref(ctx), 0); cuda.cuCtxSetCurrent(ctx)

def load(ptx):
    m = ctypes.c_void_p()
    ret = cuda.cuModuleLoadData(ctypes.byref(m), ptx.strip().encode()+b'\x00')
    if ret != 0:
        err = ctypes.c_char_p(); cuda.cuGetErrorString(ret, ctypes.byref(err))
        raise RuntimeError(f"PTX load fail({ret}): {err.value.decode() if err.value else '?'}")
    return m
def gf(m,n):
    f=ctypes.c_void_p(); r=cuda.cuModuleGetFunction(ctypes.byref(f),m,n.encode())
    assert r==0, f"GetFunc {n}: {r}"; return f
def alloc(n):
    p=ctypes.c_uint64(0); cuda.cuMemAlloc_v2(ctypes.byref(p),n); return p.value
def run(f,gr,bl,args):
    n=len(args); buf=(ctypes.c_uint64*n)(*args); ptrs=(ctypes.c_void_p*n)()
    base=ctypes.addressof(buf)
    for i in range(n): ptrs[i]=base+i*8
    r=cuda.cuLaunchKernel(f,gr,1,1,bl,1,1,0,None,ptrs,None)
    assert r==0, f"Launch: {r}"; cuda.cuCtxSynchronize()
def bench(f,w=20,r=300):
    for _ in range(w): f()
    t=[]
    for _ in range(r):
        s=time.perf_counter(); f(); t.append((time.perf_counter()-s)*1e6)
    return np.array(t)

N,DOF,BS,GR = 4096,29,256,16

# === 原始版 ===
PTX_BASE = open('/tmp/ptx_base.txt').read()
# === 优化版 (展开+流水线) ===
PTX_OPT = open('/tmp/ptx_opt.txt').read()
# === 向量化 vec_add ===
PTX_VEC = open('/tmp/ptx_vec.txt').read()

print(f"GPU: {torch.cuda.get_device_name(0)}  SM: {torch.cuda.get_device_capability(0)}")
print(f"配置: {N} envs, {DOF} DOFs\n")

try:
    f_base = gf(load(PTX_BASE), "dof_reward")
    print("✓ 原始版加载成功")
except Exception as e:
    print(f"✗ 原始版: {e}")

try:
    f_opt = gf(load(PTX_OPT), "dof_reward")
    print("✓ 优化版加载成功")
except Exception as e:
    print(f"✗ 优化版: {e}")

try:
    f_vec = gf(load(PTX_VEC), "vec_add_v4")
    print("✓ 向量化版加载成功")
except Exception as e:
    print(f"✗ 向量化版: {e}")

# 数据
rd = np.random.randn(N*DOF).astype(np.float32)
dd = np.random.randn(N*DOF).astype(np.float32)
nb = N*DOF*4
drd,ddd,do = alloc(nb),alloc(nb),alloc(N*4)
cuda.cuMemcpyHtoD_v2(ctypes.c_uint64(drd), rd.ctypes.data, nb)
cuda.cuMemcpyHtoD_v2(ctypes.c_uint64(ddd), dd.ctypes.data, nb)
ref = ((rd.reshape(N,DOF)-dd.reshape(N,DOF))**2).sum(axis=-1)

# 正确性 + 性能
if 'f_base' in dir():
    run(f_base,GR,BS,[drd,ddd,do,DOF]); r0=np.empty(N,dtype=np.float32); cuda.cuMemcpyDtoH_v2(r0.ctypes.data,ctypes.c_uint64(do),N*4)
    print(f"\n原始版 正确性: max_diff={np.abs(ref-r0).max():.2e}")
    t_base=bench(lambda:run(f_base,GR,BS,[drd,ddd,do,DOF]))
    print(f"原始版 (串行循环):      {t_base.mean():.1f} µs")

if 'f_opt' in dir():
    run(f_opt,GR,BS,[drd,ddd,do,DOF]); r1=np.empty(N,dtype=np.float32); cuda.cuMemcpyDtoH_v2(r1.ctypes.data,ctypes.c_uint64(do),N*4)
    print(f"优化版 正确性: max_diff={np.abs(ref-r1).max():.2e}")
    t_opt=bench(lambda:run(f_opt,GR,BS,[drd,ddd,do,DOF]))
    print(f"优化版 (展开×4+流水线): {t_opt.mean():.1f} µs")

# PyTorch
rdt=torch.from_numpy(rd.reshape(N,DOF)).cuda(); ddt=torch.from_numpy(dd.reshape(N,DOF)).cuda()
def pt_fn(): return ((rdt-ddt)**2).sum(dim=-1)
t_pt=bench(lambda:pt_fn()); torch.cuda.synchronize()
t_pt2=[]
for _ in range(300):
    s=time.perf_counter(); pt_fn(); torch.cuda.synchronize(); t_pt2.append((time.perf_counter()-s)*1e6)
t_pt2=np.array(t_pt2)

print(f"\n{'='*60}")
print(f"SUMMARY")
print(f"{'='*60}")
if 't_base' in dir() and 't_opt' in dir():
    print(f"  dof_reward 原始:     {t_base.mean():.1f} µs")
    print(f"  dof_reward 优化:     {t_opt.mean():.1f} µs")
    print(f"  → 优化 pass 收益:    {t_base.mean()/t_opt.mean():.2f}x")
    print(f"  → 优化 vs PyTorch:   {t_pt2.mean()/t_opt.mean():.2f}x")
print(f"  PyTorch 原生:        {t_pt2.mean():.1f} µs")

if 'f_vec' in dir():
    a=np.random.randn(N).astype(np.float32); b=np.random.randn(N).astype(np.float32)
    nb1=N*4; da,db,do2=alloc(nb1),alloc(nb1),alloc(nb1)
    cuda.cuMemcpyHtoD_v2(ctypes.c_uint64(da),a.ctypes.data,nb1); cuda.cuMemcpyHtoD_v2(ctypes.c_uint64(db),b.ctypes.data,nb1)
    GR_V4=(N//4+BS-1)//BS
    run(f_vec,GR_V4,BS,[da,db,do2,N]); rv=np.empty(N,dtype=np.float32); cuda.cuMemcpyDtoH_v2(rv.ctypes.data,ctypes.c_uint64(do2),nb1)
    print(f"\n  vec_add_v4 正确性:   max_diff={np.abs(a+b-rv).max():.2e}")
    t_vec=bench(lambda:run(f_vec,GR_V4,BS,[da,db,do2,N]))
    at=torch.from_numpy(a).cuda(); bt=torch.from_numpy(b).cuda()
    tpv=[]; 
    for _ in range(300):
        s=time.perf_counter(); at+bt; torch.cuda.synchronize(); tpv.append((time.perf_counter()-s)*1e6)
    tpv=np.array(tpv)
    print(f"  vec_add PyTorch:     {tpv.mean():.1f} µs")
    print(f"  vec_add Karte v4:    {t_vec.mean():.1f} µs")
    print(f"  → 比值:              {tpv.mean()/t_vec.mean():.2f}x")
