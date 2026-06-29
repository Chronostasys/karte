"""
PyTorch vs Triton vs Karte — body_rot_reward 同一负载
RTX 5080 (SM 12.0)

Karte 的赢面：
1. ld.global.v4.f32 向量化加载四元数（4×f32 一次读完）
2. 只计算 dw 分量（4元素点积），跳过 dx/dy/dz
3. 14 body 全展开，零分支
4. 零 Python 开销，ctypes 直通 CUDA Driver
"""
import torch, triton, triton.language as tl
import ctypes, ctypes.util, numpy as np, time

# ============================================================
# CUDA Driver 初始化
# ============================================================
cuda = ctypes.CDLL(ctypes.util.find_library('cuda')); cuda.cuInit(0)
cuda.cuMemAlloc_v2.argtypes = [ctypes.POINTER(ctypes.c_uint64), ctypes.c_size_t]; cuda.cuMemAlloc_v2.restype = ctypes.c_int
cuda.cuMemcpyHtoD_v2.argtypes = [ctypes.c_uint64, ctypes.c_void_p, ctypes.c_size_t]; cuda.cuMemcpyHtoD_v2.restype = ctypes.c_int
cuda.cuMemcpyDtoH_v2.argtypes = [ctypes.c_void_p, ctypes.c_uint64, ctypes.c_size_t]; cuda.cuMemcpyDtoH_v2.restype = ctypes.c_int
cuda.cuLaunchKernel.argtypes = [ctypes.c_void_p]+[ctypes.c_uint]*7+[ctypes.c_void_p]*3; cuda.cuLaunchKernel.restype = ctypes.c_int
_ = torch.cuda.device_count()
ctx = ctypes.c_void_p(); cuda.cuDevicePrimaryCtxRetain(ctypes.byref(ctx), 0); cuda.cuCtxSetCurrent(ctx)

def cu_load(ptx):
    m=ctypes.c_void_p()
    r=cuda.cuModuleLoadData(ctypes.byref(m), ptx.strip().encode()+b'\x00')
    assert r==0, f"PTX load: {r}"
    return m
def cu_gf(m,n):
    f=ctypes.c_void_p(); r=cuda.cuModuleGetFunction(ctypes.byref(f),m,n.encode()); assert r==0; return f
def cu_alloc(n):
    p=ctypes.c_uint64(0); cuda.cuMemAlloc_v2(ctypes.byref(p),n); return p.value
def cu_run(f,gr,bl,args):
    n=len(args); buf=(ctypes.c_uint64*n)(*args); ptrs=(ctypes.c_void_p*n)()
    base=ctypes.addressof(buf)
    for i in range(n): ptrs[i]=base+i*8
    r=cuda.cuLaunchKernel(f,gr,1,1,bl,1,1,0,None,ptrs,None)
    assert r==0; cuda.cuCtxSynchronize()

# ============================================================
# 1. PyTorch 原生实现
# ============================================================
def quat_inverse(q):
    x,y,z,w = q[...,0],q[...,1],q[...,2],q[...,3]
    return torch.stack((-x,-y,-z,w),dim=-1)

def quat_multiply(q1,q2):
    x1,y1,z1,w1 = q1[...,0],q1[...,1],q1[...,2],q1[...,3]
    x2,y2,z2,w2 = q2[...,0],q2[...,1],q2[...,2],q2[...,3]
    x = w1*x2+x1*w2+y1*z2-z1*y2; y = w1*y2-x1*z2+y1*w2+z1*x2
    z = w1*z2+x1*y2-y1*x2+z1*w2; w = w1*w2-x1*x2-y1*y2-z1*z2
    return torch.stack((x,y,z,w),dim=-1)

def pytorch_body_rot_reward(body_rot, ref_body_rot, sigma=0.25):
    diff_quat = quat_multiply(quat_inverse(body_rot), ref_body_rot)
    dw = diff_quat[...,3]
    error = (8.0*(1.0 - dw.clamp(-1,1))).mean(dim=-1)
    return torch.exp(-sigma * error)

# ============================================================
# 2. Triton 实现
# ============================================================
@triton.jit
def triton_body_rot_reward(
    body_ptr, ref_ptr, out_ptr,
    sigma: tl.constexpr,
    N_BODIES: tl.constexpr,
):
    pid = tl.program_id(0)
    total_error = 0.0
    for j in tl.static_range(N_BODIES):
        off = pid * N_BODIES * 4 + j * 4
        bx = tl.load(body_ptr + off + 0)
        by = tl.load(body_ptr + off + 1)
        bz = tl.load(body_ptr + off + 2)
        bw = tl.load(body_ptr + off + 3)
        rx = tl.load(ref_ptr + off + 0)
        ry = tl.load(ref_ptr + off + 1)
        rz = tl.load(ref_ptr + off + 2)
        rw = tl.load(ref_ptr + off + 3)
        dw = bw*rw + bx*rx + by*ry + bz*rz
        total_error += 8.0 * (1.0 - dw)
    mean_error = total_error / N_BODIES
    # exp(-x) = 2^(-x * log2(e)) ≈ 2^(-x * 1.4427)
    reward = tl.math.exp(-sigma * mean_error)
    tl.store(out_ptr + pid, reward)

# ============================================================
# 3. Karte PTX — 手写最优 kernel
# ============================================================
# 核心: ld.global.v4.f32 一次性加载四元数 + 14 body 全展开 + 只算 dw 点积
KARTE_BODY_ROT_PTX = """
.version 8.0
.target sm_12_0
.address_size 64
.entry body_rot_reward(.param .u64 body_ptr, .param .u64 ref_ptr, .param .u64 out_ptr, .param .f32 sigma_val) {
.reg .u32 %r<6>; .reg .u64 %rd<16>; .reg .f32 %f<32>; .reg .pred %p<2>;
ld.param.u64 %rd0, [body_ptr]; ld.param.u64 %rd1, [ref_ptr]; ld.param.u64 %rd2, [out_ptr]; ld.param.f32 %f0, [sigma_val];
mov.u32 %r0, %ctaid.x; mov.u32 %r1, %ntid.x; mov.u32 %r2, %tid.x;
mul.lo.u32 %r0, %r0, %r1; add.u32 %r3, %r0, %r2;
cvt.u64.u32 %rd3, %r3; mul.lo.u64 %rd3, %rd3, 224;
mov.f32 %f1, 0f00000000;
"""
# 动态生成 14 body 全展开的 PTX
for j in range(14):
    off = j * 16  # j * 4 * sizeof(f32)
    KARTE_BODY_ROT_PTX += f"""
add.u64 %rd4, %rd0, %rd3; add.u64 %rd4, %rd4, {off};
ld.global.v4.f32 {{%f2,%f3,%f4,%f5}}, [%rd4];
add.u64 %rd5, %rd1, %rd3; add.u64 %rd5, %rd5, {off};
ld.global.v4.f32 {{%f6,%f7,%f8,%f9}}, [%rd5];
mul.f32 %f10, %f5, %f9; mad.f32 %f10, %f2, %f6, %f10; mad.f32 %f10, %f3, %f7, %f10; mad.f32 %f10, %f4, %f8, %f10;
sub.f32 %f11, 0f41000000, %f10;
add.f32 %f1, %f1, %f11;
"""
# 0f41000000 = 8.0f
KARTE_BODY_ROT_PTX += """
mad.f32 %f1, %f1, 0f3D893748, 0f00000000;
mad.f32 %f1, %f0, %f1, 0f00000000;
neg.f32 %f1, %f1;
mul.f32 %f1, %f1, 0f3FB8AA3B;
ex2.approx.f32 %f1, %f1;
add.u64 %rd6, %rd2, %r3; add.u64 %rd6, %rd6, %r3; add.u64 %rd6, %rd6, %r3;
st.global.f32 [%rd6], %f1;
ret;
}
"""

# ============================================================
# Benchmark
# ============================================================
NUM_ENVS = 4096
N_BODIES = 14

device = torch.device('cuda')
print(f"GPU: {torch.cuda.get_device_name(0)}  SM: {torch.cuda.get_device_capability(0)}")
print(f"配置: {NUM_ENVS} envs, {N_BODIES} bodies\n")

# 生成数据
torch.manual_seed(42)
body_rot = torch.randn(NUM_ENVS, N_BODIES, 4, device=device)
body_rot = body_rot / body_rot.norm(dim=-1, keepdim=True)
ref_body_rot = torch.randn(NUM_ENVS, N_BODIES, 4, device=device)
ref_body_rot = ref_body_rot / ref_body_rot.norm(dim=-1, keepdim=True)

# === 正确性验证 ===
ref_out = pytorch_body_rot_reward(body_rot, ref_body_rot)

# Triton
out_tri = torch.empty(NUM_ENVS, device=device, dtype=torch.float32)
triton_body_rot_reward[(NUM_ENVS,)](
    body_rot, ref_body_rot, out_tri,
    sigma=0.25, N_BODIES=N_BODIES,
)
print(f"Triton  正确性: max_diff={(ref_out - out_tri).abs().max():.2e}")

# Karte
m_karte = cu_load(KARTE_BODY_ROT_PTX)
f_karte = cu_gf(m_karte, "body_rot_reward")
nb = NUM_ENVS * N_BODIES * 4 * 4
d_body = cu_alloc(nb); cu_run_or_noop = cuda.cuMemcpyHtoD_v2(ctypes.c_uint64(d_body), body_rot.contiguous().cpu().numpy().ctypes.data, nb)
d_ref = cu_alloc(nb); cuda.cuMemcpyHtoD_v2(ctypes.c_uint64(d_ref), ref_body_rot.contiguous().cpu().numpy().ctypes.data, nb)
d_out = cu_alloc(NUM_ENVS * 4)
sigma_bits = ctypes.c_uint64(0x3E800000)  # 0.25f in hex
# Actually need to pass sigma as f32 param. CUDA driver params are passed as u64.
sigma_f32 = np.float32(0.25)
sigma_u32 = sigma_f32.view(np.uint32)
cu_run(f_karte, (NUM_ENVS+255)//256, 256, [d_body, d_ref, d_out, int(sigma_u32)])
karte_out_np = np.empty(NUM_ENVS, dtype=np.float32)
cuda.cuMemcpyDtoH_v2(karte_out_np.ctypes.data, ctypes.c_uint64(d_out), NUM_ENVS*4)
print(f"Karte   正确性: max_diff={np.abs(ref_out.cpu().numpy() - karte_out_np).max():.2e}")
print()

# === Benchmark 函数 ===
def bench(fn, warmup=50, repeats=500):
    for _ in range(warmup): fn()
    torch.cuda.synchronize()
    t = []
    for _ in range(repeats):
        s = time.perf_counter(); fn(); torch.cuda.synchronize(); t.append((time.perf_counter()-s)*1e6)
    return np.array(t)

def bench_cuda(f, gr, bl, args, warmup=50, repeats=500):
    for _ in range(warmup): cu_run(f, gr, bl, args)
    t = []
    for _ in range(repeats):
        s = time.perf_counter(); cu_run(f, gr, bl, args); t.append((time.perf_counter()-s)*1e6)
    return np.array(t)

# PyTorch
t_pt = bench(lambda: pytorch_body_rot_reward(body_rot, ref_body_rot))

# Triton
def run_triton():
    triton_body_rot_reward[(NUM_ENVS,)](body_rot, ref_body_rot, out_tri, sigma=0.25, N_BODIES=N_BODIES)
t_tri = bench(run_triton)

# Karte
GR = (NUM_ENVS + 255) // 256
t_karte = bench_cuda(f_karte, GR, 256, [d_body, d_ref, d_out, int(sigma_u32)])

# === 结果 ===
print("="*70)
print(f"body_rot_reward [{NUM_ENVS} envs × {N_BODIES} bodies]")
print("="*70)
print(f"  PyTorch 原生:   {t_pt.mean():.1f} ± {t_pt.std():.1f} µs")
print(f"  Triton:         {t_tri.mean():.1f} ± {t_tri.std():.1f} µs   ({t_pt.mean()/t_tri.mean():.2f}x vs PyTorch)")
print(f"  Karte GPU:      {t_karte.mean():.1f} ± {t_karte.std():.1f} µs   ({t_pt.mean()/t_karte.mean():.2f}x vs PyTorch)")
print()
print(f"  Karte vs Triton: {t_tri.mean()/t_karte.mean():.2f}x")
print()

# 30× 批量
def pt30():
    for _ in range(30): pytorch_body_rot_reward(body_rot, ref_body_rot)
def tri30():
    for _ in range(30): triton_body_rot_reward[(NUM_ENVS,)](body_rot, ref_body_rot, out_tri, sigma=0.25, N_BODIES=N_BODIES)
def karte30():
    for _ in range(30): cu_run(f_karte, GR, 256, [d_body, d_ref, d_out, int(sigma_u32)])

print("="*70)
print(f"30× body_rot_reward (模拟训练中的批量奖励)")
print("="*70)
t_pt30 = bench(pt30, warmup=10, repeats=100)
t_tri30 = bench(tri30, warmup=10, repeats=100)
t_k30 = bench(karte30, warmup=10, repeats=100)
print(f"  PyTorch 30×:   {t_pt30.mean():.1f} µs")
print(f"  Triton  30×:   {t_tri30.mean():.1f} µs   ({t_pt30.mean()/t_tri30.mean():.2f}x vs PyTorch)")
print(f"  Karte   30×:   {t_k30.mean():.1f} µs   ({t_pt30.mean()/t_k30.mean():.2f}x vs PyTorch)")
print(f"  Karte vs Triton: {t_tri30.mean()/t_k30.mean():.2f}x")
