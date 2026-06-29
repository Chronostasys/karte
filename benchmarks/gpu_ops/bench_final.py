"""
PyTorch vs Triton vs Karte — body_rot_reward 同一负载
RTX 5080 (SM 12.0)

Karte 的 PTX 由 Karte 自己的编译流水线产出（GIR构造 → VectorizePass → SoftwarePipelinePass → PtxCompiler），
零手写 PTX。
"""
import torch, triton, triton.language as tl
import ctypes, ctypes.util, numpy as np, time, subprocess, sys

# ============================================================
# CUDA Driver
# ============================================================
cuda = ctypes.CDLL(ctypes.util.find_library('cuda')); cuda.cuInit(0)
cuda.cuMemAlloc_v2.argtypes = [ctypes.POINTER(ctypes.c_uint64), ctypes.c_size_t]; cuda.cuMemAlloc_v2.restype = ctypes.c_int
cuda.cuMemcpyHtoD_v2.argtypes = [ctypes.c_uint64, ctypes.c_void_p, ctypes.c_size_t]; cuda.cuMemcpyHtoD_v2.restype = ctypes.c_int
cuda.cuMemcpyDtoH_v2.argtypes = [ctypes.c_void_p, ctypes.c_uint64, ctypes.c_size_t]; cuda.cuMemcpyDtoH_v2.restype = ctypes.c_int
cuda.cuLaunchKernel.argtypes = [ctypes.c_void_p]+[ctypes.c_uint]*7+[ctypes.c_void_p]*3; cuda.cuLaunchKernel.restype = ctypes.c_int
_ = torch.cuda.device_count()
ctx = ctypes.c_void_p(); cuda.cuDevicePrimaryCtxRetain(ctypes.byref(ctx), 0); cuda.cuCtxSetCurrent(ctx)

def cu_load(ptx_text):
    m = ctypes.c_void_p()
    r = cuda.cuModuleLoadData(ctypes.byref(m), ptx_text.strip().encode()+b'\x00')
    assert r == 0, f"PTX load: {r}"
    return m
def cu_func(m, name):
    f = ctypes.c_void_p(); r = cuda.cuModuleGetFunction(ctypes.byref(f), m, name.encode())
    assert r == 0, f"GetFunc: {r}"; return f
def cu_alloc(n):
    p = ctypes.c_uint64(0); cuda.cuMemAlloc_v2(ctypes.byref(p), n); return p.value
def cu_run(f, gr, bl, args):
    n = len(args); buf = (ctypes.c_uint64*n)(*args); ptrs = (ctypes.c_void_p*n)()
    base = ctypes.addressof(buf)
    for i in range(n): ptrs[i] = base + i*8
    r = cuda.cuLaunchKernel(f, gr,1,1, bl,1,1, 0, None, ptrs, None)
    assert r == 0, f"Launch: {r}"; cuda.cuCtxSynchronize()

# ============================================================
# Karte PTX — 由 Karte 编译流水线自动生成
# ============================================================
print("生成 Karte PTX (GIR → pass → PtxCompiler)...")
result = subprocess.run(
    ['cargo', 'run', '--example', 'gen_body_rot_ptx', '-p', 'karte-gpu'],
    capture_output=True, text=True, cwd='/home/user/src/karte'
)
if result.returncode != 0:
    print(f"编译失败:\n{result.stderr[:500]}"); sys.exit(1)
karte_ptx = result.stdout
print(f"Karte PTX: {len(karte_ptx)} bytes, 包含 {karte_ptx.count('ld.global')} 次全局内存加载")

# ============================================================
# 1. PyTorch 原生
# ============================================================
def pytorch_body_rot_reward(body_rot, ref_body_rot, sigma=0.25):
    # quat_inverse(body) = (-x, -y, -z, w)
    inv = torch.stack((-body_rot[...,0],-body_rot[...,1],-body_rot[...,2],body_rot[...,3]), dim=-1)
    # quat_multiply(inv, ref): only need w component
    # w = w1*w2 - x1*x2 - y1*y2 - z1*z2
    # with inv=(-bx,-by,-bz,bw), ref=(rx,ry,rz,rw):
    # dw = bw*rw + bx*rx + by*ry + bz*rz
    dw = (inv[...,3]*ref_body_rot[...,3] - inv[...,0]*ref_body_rot[...,0]
          - inv[...,1]*ref_body_rot[...,1] - inv[...,2]*ref_body_rot[...,2])
    # = bw*rw + bx*rx + by*ry + bz*rz
    error = (8.0*(1.0 - dw.clamp(-1,1))).mean(dim=-1)
    return torch.exp(-sigma * error)

# ============================================================
# 2. Triton
# ============================================================
@triton.jit
def triton_body_rot(body_ptr, ref_ptr, out_ptr, sigma: tl.constexpr, N: tl.constexpr):
    pid = tl.program_id(0)
    total = 0.0
    for j in tl.static_range(N):
        off = pid * N * 4 + j * 4
        bx = tl.load(body_ptr + off + 0)
        by = tl.load(body_ptr + off + 1)
        bz = tl.load(body_ptr + off + 2)
        bw = tl.load(body_ptr + off + 3)
        rx = tl.load(ref_ptr + off + 0)
        ry = tl.load(ref_ptr + off + 1)
        rz = tl.load(ref_ptr + off + 2)
        rw = tl.load(ref_ptr + off + 3)
        dw = bw*rw + bx*rx + by*ry + bz*rz
        total += 8.0 * (1.0 - dw)
    mean_err = total / N
    reward = tl.math.exp(-sigma * mean_err)
    tl.store(out_ptr + pid, reward)

# ============================================================
# 3. 加载 Karte PTX
# ============================================================
m_k = cu_load(karte_ptx)
f_k = cu_func(m_k, "body_rot_reward")
print("Karte PTX 加载成功\n")

# ============================================================
# 测试配置
# ============================================================
NUM_ENVS = 4096; N_BODIES = 14; BS = 256
GR = (NUM_ENVS + BS - 1) // BS
device = torch.device('cuda')

print(f"GPU: {torch.cuda.get_device_name(0)}  SM: {torch.cuda.get_device_capability(0)}")
print(f"配置: {NUM_ENVS} envs × {N_BODIES} bodies\n")

# 数据
torch.manual_seed(42)
body_rot = torch.randn(NUM_ENVS, N_BODIES, 4, device=device)
body_rot = body_rot / body_rot.norm(dim=-1, keepdim=True)
ref_rot = torch.randn(NUM_ENVS, N_BODIES, 4, device=device)
ref_rot = ref_rot / ref_rot.norm(dim=-1, keepdim=True)

# === 正确性 ===
ref_out = pytorch_body_rot_reward(body_rot, ref_rot)

out_tri = torch.empty(NUM_ENVS, device=device, dtype=torch.float32)
triton_body_rot[(NUM_ENVS,)](body_rot, ref_rot, out_tri, sigma=0.25, N=N_BODIES)

# Karte
nb = NUM_ENVS * N_BODIES * 4 * 4
d_body = cu_alloc(nb); cuda.cuMemcpyHtoD_v2(ctypes.c_uint64(d_body), body_rot.contiguous().cpu().numpy().ctypes.data, nb)
d_ref = cu_alloc(nb); cuda.cuMemcpyHtoD_v2(ctypes.c_uint64(d_ref), ref_rot.contiguous().cpu().numpy().ctypes.data, nb)
d_out = cu_alloc(NUM_ENVS * 4)
sigma_u32 = np.float32(0.25).view(np.uint32)
cu_run(f_k, GR, BS, [d_body, d_ref, d_out, int(sigma_u32)])
karte_np = np.empty(NUM_ENVS, dtype=np.float32)
cuda.cuMemcpyDtoH_v2(karte_np.ctypes.data, ctypes.c_uint64(d_out), NUM_ENVS * 4)

print(f"正确性验证:")
print(f"  Triton vs PyTorch: max_diff = {(ref_out - out_tri).abs().max():.2e}")
print(f"  Karte  vs PyTorch: max_diff = {np.abs(ref_out.cpu().numpy() - karte_np).max():.2e}")
print()

# === Benchmark ===
def bench(fn, warmup=50, rep=500):
    for _ in range(warmup): fn()
    torch.cuda.synchronize()
    t = []
    for _ in range(rep):
        s = time.perf_counter(); fn(); torch.cuda.synchronize(); t.append((time.perf_counter()-s)*1e6)
    return np.array(t)

def bench_cuda(f, gr, bl, args, warmup=50, rep=500):
    for _ in range(warmup): cu_run(f, gr, bl, args)
    t = []
    for _ in range(rep):
        s = time.perf_counter(); cu_run(f, gr, bl, args); t.append((time.perf_counter()-s)*1e6)
    return np.array(t)

t_pt = bench(lambda: pytorch_body_rot_reward(body_rot, ref_rot))
t_tri = bench(lambda: triton_body_rot[(NUM_ENVS,)](body_rot, ref_rot, out_tri, sigma=0.25, N=N_BODIES))
t_k = bench_cuda(f_k, GR, BS, [d_body, d_ref, d_out, int(sigma_u32)])

print("="*65)
print(f"body_rot_reward [{NUM_ENVS} envs × {N_BODIES} bodies]")
print("="*65)
print(f"  PyTorch 原生:   {t_pt.mean():.1f} ± {t_pt.std():.1f} µs")
print(f"  Triton:         {t_tri.mean():.1f} ± {t_tri.std():.1f} µs   ({t_pt.mean()/t_tri.mean():.2f}x vs PyTorch)")
print(f"  Karte GPU:      {t_k.mean():.1f} ± {t_k.std():.1f} µs   ({t_pt.mean()/t_k.mean():.2f}x vs PyTorch)")
print()
print(f"  Karte vs Triton: {t_tri.mean()/t_k.mean():.2f}x")
print()

# 30× 批量
def pt30():
    for _ in range(30): pytorch_body_rot_reward(body_rot, ref_rot)
def tri30():
    for _ in range(30): triton_body_rot[(NUM_ENVS,)](body_rot, ref_rot, out_tri, sigma=0.25, N=N_BODIES)
def k30():
    for _ in range(30): cu_run(f_k, GR, BS, [d_body, d_ref, d_out, int(sigma_u32)])

t_pt30 = bench(pt30, warmup=10, rep=100)
t_tri30 = bench(tri30, warmup=10, rep=100)
t_k30 = bench(k30, warmup=10, rep=100)

print("="*65)
print(f"30× body_rot_reward (批量奖励模拟)")
print("="*65)
print(f"  PyTorch 30×:   {t_pt30.mean():.1f} µs")
print(f"  Triton  30×:   {t_tri30.mean():.1f} µs   ({t_pt30.mean()/t_tri30.mean():.2f}x vs PyTorch)")
print(f"  Karte   30×:   {t_k30.mean():.1f} µs   ({t_pt30.mean()/t_k30.mean():.2f}x vs PyTorch)")
print(f"  Karte vs Triton: {t_tri30.mean()/t_k30.mean():.2f}x")
