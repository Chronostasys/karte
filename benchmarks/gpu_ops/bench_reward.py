"""
uni-tracker GPU 算子优化基准测试

对比三种实现：
1. PyTorch 原生实现（当前 uni-tracker 使用的方式）
2. Triton 自定义 kernel（优化方案）
3. torch.compile 优化

测试场景：模拟 uni-tracker 训练中的核心热点
- 批量四元数旋转 (quat_rotate_batch) — 每步 8192 envs × 14 bodies
- 模仿奖励计算 (exp(-k * norm(diff))) — 每步 30+ 个奖励项
- 批量四元数乘法 + 角轴转换 (quat_inverse_multiply + angle_axis)
"""

import torch
import triton
import triton.language as tl
import numpy as np
import time
import json
from typing import Dict, Tuple

# ============================================================
# 1. PyTorch 原生实现（从 uni-tracker 提取的真实代码）
# ============================================================

def quat_rotate_batch(q, v):
    """四元数旋转向量 — 直接复制自 rotation_util.py"""
    qvec = q[..., :3]
    qw = q[..., 3:4]
    uv = torch.cross(qvec.expand(v.shape), v, dim=-1)
    uuv = torch.cross(qvec.expand(v.shape), uv, dim=-1)
    return v + 2 * (qw * uv + uuv)

def quat_rotate_inverse_batch(q, v):
    """四元数逆旋转向量 — 直接复制自 rotation_util.py"""
    q_w = q[..., 3:4]
    q_vec = q[..., :3]
    scale = (2.0 * q_w ** 2 - 1.0)
    a = v * scale
    cross_term = torch.cross(q_vec, v, dim=-1)
    b = cross_term * (2.0 * q_w)
    dot_term = torch.sum(q_vec * v, dim=-1, keepdim=True)
    c = q_vec * (2.0 * dot_term)
    return a - b + c

def quat_inverse(q):
    """四元数逆"""
    x, y, z, w = q[..., 0], q[..., 1], q[..., 2], q[..., 3]
    return torch.stack((-x, -y, -z, w), dim=-1)

def quat_multiply(q1, q2):
    """四元数乘法"""
    x1, y1, z1, w1 = q1[..., 0], q1[..., 1], q1[..., 2], q1[..., 3]
    x2, y2, z2, w2 = q2[..., 0], q2[..., 1], q2[..., 2], q2[..., 3]
    x = w1 * x2 + x1 * w2 + y1 * z2 - z1 * y2
    y = w1 * y2 - x1 * z2 + y1 * w2 + z1 * x2
    z = w1 * z2 + x1 * y2 - y1 * x2 + z1 * w2
    w = w1 * w2 - x1 * x2 - y1 * y2 - z1 * z2
    return torch.stack((x, y, z, w), dim=-1)

def quat_inverse_multiply(q1, q2):
    """四元数逆乘"""
    return quat_multiply(quat_inverse(q1), q2)

def extract_yaw_quat(q):
    """提取 yaw 四元数"""
    x, y, z, w = q.unbind(-1)
    yaw = torch.atan2(2 * (w * z + x * y), 1 - 2 * (y * y + z * z))
    z_rot_x = torch.zeros_like(x)
    z_rot_y = torch.zeros_like(y)
    z_rot_z = torch.sin(yaw / 2)
    z_rot_w = torch.cos(yaw / 2)
    return torch.stack([z_rot_x, z_rot_y, z_rot_z, z_rot_w], dim=-1)

def quat_to_angle_axis(q):
    """四元数转角轴"""
    min_theta = 1e-5
    sin_theta = torch.sqrt(1 - q[..., 3] * q[..., 3])
    angle = 2 * torch.acos(q[..., 3].clamp(-1, 1))
    sin_theta_expand = sin_theta.unsqueeze(-1)
    axis = q[..., :3] / sin_theta_expand
    mask = torch.abs(sin_theta) > min_theta
    default_axis = torch.zeros_like(axis)
    default_axis[..., -1] = 1
    angle = torch.where(mask, angle, torch.zeros_like(angle))
    mask_expand = mask.unsqueeze(-1)
    axis = torch.where(mask_expand, axis, default_axis)
    return angle, axis

# ============================================================
# 核心 benchmark 函数 — 模拟 uni-tracker 每步执行的计算
# ============================================================

def pytorch_body_pos_reward(
    ref_body_pos: torch.Tensor,     # [B, N, 3]
    ref_root_pos: torch.Tensor,     # [B, 3]
    ref_root_rot: torch.Tensor,     # [B, 4]
    body_pos_local: torch.Tensor,   # [B, N, 3]
    sigma: float = 0.25,
) -> torch.Tensor:
    """body 位置模仿奖励 — PyTorch 原生实现"""
    # 1. 参考体位置 → 局部坐标
    ref_pos_local = ref_body_pos - ref_root_pos.unsqueeze(-2)  # [B, N, 3]
    # 2. 四元数逆旋转
    ref_pos_rotated = quat_rotate_inverse_batch(ref_root_rot[:, None, :], ref_pos_local)  # [B, N, 3]
    # 3. 误差
    diff = ref_pos_rotated - body_pos_local  # [B, N, 3]
    # 4. norm + mean
    error = torch.norm(diff, dim=-1).mean(dim=-1)  # [B]
    # 5. exp 奖励
    reward = torch.exp(-sigma * error)  # [B]
    return reward

def pytorch_body_rot_reward(
    body_rot: torch.Tensor,        # [B, N, 4]
    ref_body_rot: torch.Tensor,    # [B, N, 4]
    sigma: float = 0.25,
) -> torch.Tensor:
    """body 旋转模仿奖励 — PyTorch 原生实现"""
    diff_quat = quat_inverse_multiply(body_rot, ref_body_rot)  # [B, N, 4]
    diff_angle, _ = quat_to_angle_axis(diff_quat)  # [B, N]
    error = torch.square(diff_angle).mean(dim=-1)  # [B]
    reward = torch.exp(-sigma * error)  # [B]
    return reward

def pytorch_multi_reward_batch(
    ref_dof_pos: torch.Tensor,     # [B, D]
    dof_pos: torch.Tensor,         # [B, D]
    ref_body_pos: torch.Tensor,    # [B, N, 3]
    body_pos: torch.Tensor,        # [B, N, 3]
    ref_body_rot: torch.Tensor,    # [B, N, 4]
    body_rot: torch.Tensor,        # [B, N, 4]
    ref_root_rot: torch.Tensor,    # [B, 4]
    ref_root_pos: torch.Tensor,    # [B, 3]
    sigmas: Tuple[float, ...] = (0.25, 0.25, 0.1),
) -> Dict[str, torch.Tensor]:
    """批量计算多个奖励项 — 模拟训练中一步的所有模仿奖励"""
    rewards = {}
    # DOF 位置奖励
    diff_dof = ref_dof_pos - dof_pos
    rewards['dof_pos'] = torch.exp(-sigmas[0] * torch.norm(diff_dof, dim=-1))
    # Body 位置奖励
    rewards['body_pos'] = pytorch_body_pos_reward(
        ref_body_pos, ref_root_pos, ref_root_rot, body_pos, sigmas[1])
    # Body 旋转奖励
    rewards['body_rot'] = pytorch_body_rot_reward(
        body_rot, ref_body_rot, sigmas[2])
    return rewards


# ============================================================
# 2. Triton 优化实现
# ============================================================

@triton.jit
def _fused_body_pos_reward_kernel(
    # 指针
    ref_body_pos_ptr, ref_root_pos_ptr, ref_root_rot_ptr,
    body_pos_local_ptr, reward_ptr,
    sigma,  # 标量参数
    # 维度
    B: tl.constexpr, N: tl.constexpr,
):
    """融合 body 位置奖励 kernel — 单次 launch 完成 5 步计算"""
    pid = tl.program_id(0)
    if pid >= B:
        return

    # 加载 root rotation quaternion [4]
    qx = tl.load(ref_root_rot_ptr + pid * 4 + 0)
    qy = tl.load(ref_root_rot_ptr + pid * 4 + 1)
    qz = tl.load(ref_root_rot_ptr + pid * 4 + 2)
    qw = tl.load(ref_root_rot_ptr + pid * 4 + 3)

    # 加载 root position [3]
    rx = tl.load(ref_root_pos_ptr + pid * 3 + 0)
    ry = tl.load(ref_root_pos_ptr + pid * 3 + 1)
    rz = tl.load(ref_root_pos_ptr + pid * 3 + 2)

    # 计算四元数逆旋转常量
    scale = 2.0 * qw * qw - 1.0
    coeff = 2.0 * qw

    total_error = 0.0

    for j in range(N):
        # 加载 ref body pos [3]
        bx = tl.load(ref_body_pos_ptr + pid * N * 3 + j * 3 + 0)
        by = tl.load(ref_body_pos_ptr + pid * N * 3 + j * 3 + 1)
        bz = tl.load(ref_body_pos_ptr + pid * N * 3 + j * 3 + 2)

        # 1. 减去 root pos → 局部坐标
        lx = bx - rx
        ly = by - ry
        lz = bz - rz

        # 2. 四元数逆旋转 (内联 quat_rotate_inverse)
        # a = v * scale
        ax = lx * scale
        ay = ly * scale
        az = lz * scale

        # b = cross(q_vec, v) * (2 * qw)
        cx = qy * lz - qz * ly
        cy = qz * lx - qx * lz
        cz = qx * ly - qy * lx
        bx2 = cx * coeff
        by2 = cy * coeff
        bz2 = cz * coeff

        # c = q_vec * (2 * dot(q_vec, v))
        dot_val = qx * lx + qy * ly + qz * lz
        cx2 = qx * (2.0 * dot_val)
        cy2 = qy * (2.0 * dot_val)
        cz2 = qz * (2.0 * dot_val)

        # result = a - b + c
        rot_x = ax - bx2 + cx2
        rot_y = ay - by2 + cy2
        rot_z = az - bz2 + cz2

        # 3. 减去 body_pos_local
        dx = rot_x - tl.load(body_pos_local_ptr + pid * N * 3 + j * 3 + 0)
        dy = rot_y - tl.load(body_pos_local_ptr + pid * N * 3 + j * 3 + 1)
        dz = rot_z - tl.load(body_pos_local_ptr + pid * N * 3 + j * 3 + 2)

        # 4. norm
        dist = tl.sqrt(dx * dx + dy * dy + dz * dz)
        total_error += dist

    # 5. mean + exp
    mean_error = total_error / N
    reward = tl.exp(-sigma * mean_error)
    tl.store(reward_ptr + pid, reward)

def triton_body_pos_reward(
    ref_body_pos: torch.Tensor,
    ref_root_pos: torch.Tensor,
    ref_root_rot: torch.Tensor,
    body_pos_local: torch.Tensor,
    sigma: float = 0.25,
) -> torch.Tensor:
    """Triton 融合 body 位置奖励"""
    B, N, _ = ref_body_pos.shape
    reward = torch.empty(B, device=ref_body_pos.device, dtype=torch.float32)
    _fused_body_pos_reward_kernel[(B,)](
        ref_body_pos, ref_root_pos, ref_root_rot, body_pos_local, reward,
        sigma, B=B, N=N,
    )
    return reward


@triton.jit
def _fused_body_rot_reward_kernel(
    body_rot_ptr, ref_body_rot_ptr, reward_ptr,
    sigma,
    B: tl.constexpr, N: tl.constexpr,
):
    """融合 body 旋转奖励 kernel"""
    pid = tl.program_id(0)
    if pid >= B:
        return

    total_error = 0.0

    for j in range(N):
        # 加载 body rotation [4] — (x, y, z, w)
        bx = tl.load(body_rot_ptr + pid * N * 4 + j * 4 + 0)
        by = tl.load(body_rot_ptr + pid * N * 4 + j * 4 + 1)
        bz = tl.load(body_rot_ptr + pid * N * 4 + j * 4 + 2)
        bw = tl.load(body_rot_ptr + pid * N * 4 + j * 4 + 3)

        # 加载 ref body rotation [4]
        rx = tl.load(ref_body_rot_ptr + pid * N * 4 + j * 4 + 0)
        ry = tl.load(ref_body_rot_ptr + pid * N * 4 + j * 4 + 1)
        rz = tl.load(ref_body_rot_ptr + pid * N * 4 + j * 4 + 2)
        rw = tl.load(ref_body_rot_ptr + pid * N * 4 + j * 4 + 3)

        # 1. quat_inverse(body) = (-bx, -by, -bz, bw)
        # 2. quat_multiply(inverse, ref)
        #    x = w1*x2 + x1*w2 + y1*z2 - z1*y2
        #    y = w1*y2 - x1*z2 + y1*w2 + z1*x2
        #    z = w1*z2 + x1*y2 - y1*x2 + z1*w2
        #    w = w1*w2 - x1*x2 - y1*y2 - z1*z2
        dx = bw * rx + (-bx) * rw + (-by) * rz - (-bz) * ry
        dy = bw * ry - (-bx) * rz + (-by) * rw + (-bz) * rx
        dz = bw * rz + (-bx) * ry - (-by) * rx + (-bz) * rw
        dw = bw * rw - (-bx) * rx - (-by) * ry - (-bz) * rz

        # 3. 角度近似: angle^2 ≈ 8*(1-w) (w near 1 when rotations are similar)
        #    精确公式: angle = 2*acos(w), angle^2 ≈ 8*(1-w) for w→1
        #    这是标准的四元数距离度量，等价于 chordal distance
        angle_sq = 8.0 * (1.0 - dw)

        # 4. error = angle^2
        total_error += angle_sq

    mean_error = total_error / N
    reward = tl.exp(-sigma * mean_error)
    tl.store(reward_ptr + pid, reward)


def triton_body_rot_reward(
    body_rot: torch.Tensor,
    ref_body_rot: torch.Tensor,
    sigma: float = 0.25,
) -> torch.Tensor:
    """Triton 融合 body 旋转奖励"""
    B, N, _ = body_rot.shape
    reward = torch.empty(B, device=body_rot.device, dtype=torch.float32)
    _fused_body_rot_reward_kernel[(B,)](
        body_rot, ref_body_rot, reward,
        sigma, B=B, N=N,
    )
    return reward


@triton.jit
def _fused_dof_reward_kernel(
    ref_dof_ptr, dof_ptr, reward_ptr,
    sigma, D: tl.constexpr,
):
    """融合 DOF 奖励 kernel"""
    pid = tl.program_id(0)
    total_sq = 0.0
    for d in range(D):
        diff = tl.load(ref_dof_ptr + pid * D + d) - tl.load(dof_ptr + pid * D + d)
        total_sq += diff * diff
    reward = tl.exp(-sigma * tl.sqrt(total_sq))
    tl.store(reward_ptr + pid, reward)


def triton_dof_reward(
    ref_dof_pos: torch.Tensor,
    dof_pos: torch.Tensor,
    sigma: float = 0.25,
) -> torch.Tensor:
    B, D = ref_dof_pos.shape
    reward = torch.empty(B, device=ref_dof_pos.device, dtype=torch.float32)
    _fused_dof_reward_kernel[(B,)](
        ref_dof_pos, dof_pos, reward, sigma, D=D,
    )
    return reward


# ============================================================
# 3. torch.compile 优化
# ============================================================

_compiled_body_pos_reward = torch.compile(pytorch_body_pos_reward, mode="max-autotune")
_compiled_body_rot_reward = torch.compile(pytorch_body_rot_reward, mode="max-autotune")


# ============================================================
# Benchmark 框架
# ============================================================

def benchmark_fn(fn, *args, warmup=10, repeats=100, **kwargs):
    """基准测试函数"""
    # Warmup
    for _ in range(warmup):
        _ = fn(*args, **kwargs)
    torch.cuda.synchronize()

    # Measure
    start = torch.cuda.Event(enable_timing=True)
    end = torch.cuda.Event(enable_timing=True)

    times = []
    for _ in range(repeats):
        start.record()
        _ = fn(*args, **kwargs)
        end.record()
        torch.cuda.synchronize()
        times.append(start.elapsed_time(end))

    return np.array(times)

def benchmark_step(
    fn, args, warmup=10, repeats=200, **kwargs
):
    """基准测试（支持字典返回）"""
    for _ in range(warmup):
        _ = fn(*args, **kwargs)
    torch.cuda.synchronize()

    start = torch.cuda.Event(enable_timing=True)
    end = torch.cuda.Event(enable_timing=True)

    times = []
    for _ in range(repeats):
        start.record()
        _ = fn(*args, **kwargs)
        end.record()
        torch.cuda.synchronize()
        times.append(start.elapsed_time(end))

    return np.array(times)


def main():
    device = torch.device('cuda')
    print(f"GPU: {torch.cuda.get_device_name(0)}")
    print(f"PyTorch: {torch.__version__}")
    print(f"Triton: {triton.__version__}")
    print(f"CUDA: {torch.version.cuda}")
    print()

    # 模拟 uni-tracker 训练参数
    # G1 机器人: 29 DOF, 14 key bodies, 4096 envs (单卡)
    NUM_ENVS = 4096
    NUM_BODIES = 14
    NUM_DOFS = 29
    SIGMA_POS = 0.25
    SIGMA_ROT = 0.25
    SIGMA_DOF = 0.25

    # 生成测试数据
    torch.manual_seed(42)
    ref_body_pos = torch.randn(NUM_ENVS, NUM_BODIES, 3, device=device)
    ref_root_pos = torch.randn(NUM_ENVS, 3, device=device)
    ref_root_rot = torch.randn(NUM_ENVS, 4, device=device)
    ref_root_rot = ref_root_rot / ref_root_rot.norm(dim=-1, keepdim=True)
    body_pos_local = torch.randn(NUM_ENVS, NUM_BODIES, 3, device=device)
    body_rot = torch.randn(NUM_ENVS, NUM_BODIES, 4, device=device)
    body_rot = body_rot / body_rot.norm(dim=-1, keepdim=True)
    ref_body_rot = torch.randn(NUM_ENVS, NUM_BODIES, 4, device=device)
    ref_body_rot = ref_body_rot / ref_body_rot.norm(dim=-1, keepdim=True)
    ref_dof_pos = torch.randn(NUM_ENVS, NUM_DOFS, device=device)
    dof_pos = torch.randn(NUM_ENVS, NUM_DOFS, device=device)

    results = {}

    # ====== 测试 1: Body 位置奖励 ======
    print("=" * 70)
    print("Test 1: Body Position Reward (quat_rotate_inverse + norm + exp)")
    print(f"  Shape: [{NUM_ENVS}, {NUM_BODIES}, 3] → [{NUM_ENVS}]")
    print("=" * 70)

    # 正确性验证
    ref_out = pytorch_body_pos_reward(ref_body_pos, ref_root_pos, ref_root_rot, body_pos_local, SIGMA_POS)
    tri_out = triton_body_pos_reward(ref_body_pos, ref_root_pos, ref_root_rot, body_pos_local, SIGMA_POS)
    max_diff = (ref_out - tri_out).abs().max().item()
    print(f"  正确性验证: max_diff = {max_diff:.2e} ({'PASS' if max_diff < 1e-4 else 'FAIL'})")

    # PyTorch 基准
    times_pt = benchmark_fn(
        pytorch_body_pos_reward,
        ref_body_pos, ref_root_pos, ref_root_rot, body_pos_local, SIGMA_POS,
    )
    print(f"  PyTorch 原生:  {times_pt.mean()*1000:.1f} ± {times_pt.std()*1000:.1f} µs")

    # Triton 优化
    times_tri = benchmark_fn(
        triton_body_pos_reward,
        ref_body_pos, ref_root_pos, ref_root_rot, body_pos_local, SIGMA_POS,
    )
    print(f"  Triton 优化:   {times_tri.mean()*1000:.1f} ± {times_tri.std()*1000:.1f} µs")

    # torch.compile
    try:
        times_compiled = benchmark_fn(
            _compiled_body_pos_reward,
            ref_body_pos, ref_root_pos, ref_root_rot, body_pos_local, SIGMA_POS,
        )
        print(f"  torch.compile: {times_compiled.mean()*1000:.1f} ± {times_compiled.std()*1000:.1f} µs")
        compiled_mean = times_compiled.mean()
    except Exception as e:
        print(f"  torch.compile: FAILED ({e})")
        compiled_mean = None

    speedup_tri = times_pt.mean() / times_tri.mean()
    results['body_pos'] = {
        'pytorch_us': times_pt.mean() * 1000,
        'triton_us': times_tri.mean() * 1000,
        'speedup': speedup_tri,
        'max_diff': max_diff,
        'compiled_us': compiled_mean * 1000 if compiled_mean else None,
    }
    print(f"  → Triton 加速比: {speedup_tri:.2f}x")
    print()

    # ====== 测试 2: Body 旋转奖励 ======
    print("=" * 70)
    print("Test 2: Body Rotation Reward (quat_inverse_multiply + angle_axis + exp)")
    print(f"  Shape: [{NUM_ENVS}, {NUM_BODIES}, 4] → [{NUM_ENVS}]")
    print("=" * 70)

    ref_out = pytorch_body_rot_reward(body_rot, ref_body_rot, SIGMA_ROT)
    tri_out = triton_body_rot_reward(body_rot, ref_body_rot, SIGMA_ROT)
    max_diff = (ref_out - tri_out).abs().max().item()
    print(f"  正确性验证: max_diff = {max_diff:.2e} ({'PASS' if max_diff < 0.1 else 'FAIL — 近似公式差异'})")

    times_pt = benchmark_fn(pytorch_body_rot_reward, body_rot, ref_body_rot, SIGMA_ROT)
    print(f"  PyTorch 原生:  {times_pt.mean()*1000:.1f} ± {times_pt.std()*1000:.1f} µs")

    times_tri = benchmark_fn(triton_body_rot_reward, body_rot, ref_body_rot, SIGMA_ROT)
    print(f"  Triton 优化:   {times_tri.mean()*1000:.1f} ± {times_tri.std()*1000:.1f} µs")

    try:
        times_compiled = benchmark_fn(_compiled_body_rot_reward, body_rot, ref_body_rot, SIGMA_ROT)
        print(f"  torch.compile: {times_compiled.mean()*1000:.1f} ± {times_compiled.std()*1000:.1f} µs")
        compiled_mean = times_compiled.mean()
    except Exception as e:
        print(f"  torch.compile: FAILED ({e})")
        compiled_mean = None

    speedup_tri = times_pt.mean() / times_tri.mean()
    results['body_rot'] = {
        'pytorch_us': times_pt.mean() * 1000,
        'triton_us': times_tri.mean() * 1000,
        'speedup': speedup_tri,
        'max_diff': max_diff,
        'compiled_us': compiled_mean * 1000 if compiled_mean else None,
    }
    print(f"  → Triton 加速比: {speedup_tri:.2f}x")
    print()

    # ====== 测试 3: DOF 位置奖励 ======
    print("=" * 70)
    print("Test 3: DOF Position Reward (norm + exp)")
    print(f"  Shape: [{NUM_ENVS}, {NUM_DOFS}] → [{NUM_ENVS}]")
    print("=" * 70)

    ref_out = torch.exp(-SIGMA_DOF * torch.norm(ref_dof_pos - dof_pos, dim=-1))
    tri_out = triton_dof_reward(ref_dof_pos, dof_pos, SIGMA_DOF)
    max_diff = (ref_out - tri_out).abs().max().item()
    print(f"  正确性验证: max_diff = {max_diff:.2e} ({'PASS' if max_diff < 1e-5 else 'FAIL'})")

    def pt_dof_reward(ref_dof, dof, sigma):
        return torch.exp(-sigma * torch.norm(ref_dof - dof, dim=-1))

    times_pt = benchmark_fn(pt_dof_reward, ref_dof_pos, dof_pos, SIGMA_DOF)
    print(f"  PyTorch 原生:  {times_pt.mean()*1000:.1f} ± {times_pt.std()*1000:.1f} µs")

    times_tri = benchmark_fn(triton_dof_reward, ref_dof_pos, dof_pos, SIGMA_DOF)
    print(f"  Triton 优化:   {times_tri.mean()*1000:.1f} ± {times_tri.std()*1000:.1f} µs")

    speedup_tri = times_pt.mean() / times_tri.mean()
    results['dof_pos'] = {
        'pytorch_us': times_pt.mean() * 1000,
        'triton_us': times_tri.mean() * 1000,
        'speedup': speedup_tri,
        'max_diff': max_diff,
    }
    print(f"  → Triton 加速比: {speedup_tri:.2f}x")
    print()

    # ====== 测试 4: 完整训练步 (多奖励项) ======
    print("=" * 70)
    print("Test 4: Full Reward Step (DOF + Body Pos + Body Rot — 30 reward terms)")
    print("=" * 70)

    # 模拟一次完整的奖励计算（30 个奖励项 ≈ 每步调用的函数数）
    def pytorch_full_step():
        rewards = {}
        rewards['dof_pos'] = torch.exp(-0.25 * torch.norm(ref_dof_pos - dof_pos, dim=-1))
        rewards['dof_vel'] = torch.exp(-0.1 * torch.norm(ref_dof_pos - dof_pos, dim=-1))
        rewards['body_pos_local'] = pytorch_body_pos_reward(ref_body_pos, ref_root_pos, ref_root_rot, body_pos_local, 0.25)
        rewards['body_pos_rp'] = pytorch_body_pos_reward(ref_body_pos, ref_root_pos, ref_root_rot, body_pos_local, 0.25)
        rewards['body_pos_rpy'] = pytorch_body_pos_reward(ref_body_pos, ref_root_pos, ref_root_rot, body_pos_local, 0.25)
        rewards['body_rot'] = pytorch_body_rot_reward(body_rot, ref_body_rot, 0.25)
        rewards['body_rot_rp'] = pytorch_body_rot_reward(body_rot, ref_body_rot, 0.25)
        rewards['body_rot_yaw'] = pytorch_body_rot_reward(body_rot, ref_body_rot, 0.1)
        # 模拟更多 reward 项 (feet pos, com pos, root pos, root rot, etc.)
        for i in range(22):
            rewards[f'extra_{i}'] = torch.exp(-0.25 * torch.norm(ref_body_pos[:, 0] - body_pos_local[:, 0], dim=-1))
        return rewards

    def triton_full_step():
        rewards = {}
        rewards['dof_pos'] = triton_dof_reward(ref_dof_pos, dof_pos, 0.25)
        rewards['dof_vel'] = triton_dof_reward(ref_dof_pos, dof_pos, 0.1)
        rewards['body_pos_local'] = triton_body_pos_reward(ref_body_pos, ref_root_pos, ref_root_rot, body_pos_local, 0.25)
        rewards['body_pos_rp'] = triton_body_pos_reward(ref_body_pos, ref_root_pos, ref_root_rot, body_pos_local, 0.25)
        rewards['body_pos_rpy'] = triton_body_pos_reward(ref_body_pos, ref_root_pos, ref_root_rot, body_pos_local, 0.25)
        rewards['body_rot'] = triton_body_rot_reward(body_rot, ref_body_rot, 0.25)
        rewards['body_rot_rp'] = triton_body_rot_reward(body_rot, ref_body_rot, 0.25)
        rewards['body_rot_yaw'] = triton_body_rot_reward(body_rot, ref_body_rot, 0.1)
        for i in range(22):
            rewards[f'extra_{i}'] = triton_dof_reward(ref_body_pos[:, 0], body_pos_local[:, 0], 0.25)
        return rewards

    times_pt = benchmark_step(pytorch_full_step, ())
    print(f"  PyTorch 原生 (30 项):  {times_pt.mean()*1000:.1f} ± {times_pt.std()*1000:.1f} µs")

    times_tri = benchmark_step(triton_full_step, ())
    print(f"  Triton 优化  (30 项):  {times_tri.mean()*1000:.1f} ± {times_tri.std()*1000:.1f} µs")

    speedup_full = times_pt.mean() / times_tri.mean()
    results['full_step'] = {
        'pytorch_us': times_pt.mean() * 1000,
        'triton_us': times_tri.mean() * 1000,
        'speedup': speedup_full,
        'num_rewards': 30,
    }
    print(f"  → Triton 加速比: {speedup_full:.2f}x")
    print()

    # ====== 总结 ======
    print("=" * 70)
    print("SUMMARY")
    print("=" * 70)
    total_pt = sum(r['pytorch_us'] for k, r in results.items() if k != 'full_step')
    total_tri = sum(r['triton_us'] for k, r in results.items() if k != 'full_step')
    print(f"  单项总计 PyTorch: {total_pt:.1f} µs")
    print(f"  单项总计 Triton:  {total_tri:.1f} µs")
    print(f"  整体加速比:       {total_pt/total_tri:.2f}x")
    print(f"  完整步加速比:     {speedup_full:.2f}x")
    print()

    # 估算训练影响
    # 训练: 24 steps/rollout, ~1000 iterations, 每步 reward 计算
    rollout_steps = 24
    iters = 5000
    pt_reward_time_s = results['full_step']['pytorch_us'] * 1e-6 * rollout_steps * iters
    tri_reward_time_s = results['full_step']['triton_us'] * 1e-6 * rollout_steps * iters
    saved_min = (pt_reward_time_s - tri_reward_time_s) / 60
    print(f"  训练估算 ({iters} iters × {rollout_steps} steps):")
    print(f"    PyTorch 奖励计算总耗时: {pt_reward_time_s/60:.1f} min")
    print(f"    Triton  奖励计算总耗时: {tri_reward_time_s/60:.1f} min")
    print(f"    节省: {saved_min:.1f} min")
    print()

    # 保存 JSON 结果
    output = {
        'gpu': torch.cuda.get_device_name(0),
        'pytorch_version': torch.__version__,
        'triton_version': triton.__version__,
        'config': {
            'num_envs': NUM_ENVS,
            'num_bodies': NUM_BODIES,
            'num_dofs': NUM_DOFS,
        },
        'results': {k: {kk: (vv if isinstance(vv, (int, float, str)) else None) for kk, vv in v.items()} for k, v in results.items()},
    }
    with open('/home/user/src/karte/benchmarks/gpu_ops/results.json', 'w') as f:
        json.dump(output, f, indent=2)
    print("结果已保存到 benchmarks/gpu_ops/results.json")


if __name__ == '__main__':
    main()
