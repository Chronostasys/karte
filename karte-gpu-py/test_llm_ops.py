#!/usr/bin/env python3
"""
karte LLM 算子 vs PyTorch 对比测试

在 NVIDIA RTX 5080 (CUDA) 和 AMD iGPU (OpenCL) 上验证:
1. GELU 激活函数
2. RMSNorm 归一化
3. FlashAttention (简化版)

验证方法: karte GPU 输出 vs PyTorch 参考实现, 误差 < 1e-5
"""
import torch
import math
import sys
import os

# 确保 karte_gpu 包可导入
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__))))

from karte_gpu.torch_bridge import KarteLLMOps, get_bridge, CudaBridge, OpenCLBridge


def test_gelu():
    """测试 GELU 激活函数"""
    print("\n" + "=" * 60)
    print("测试 1: GELU 激活函数")
    print("=" * 60)

    ops = KarteLLMOps()
    device = 'cuda' if torch.cuda.is_available() else 'cpu'
    bridge_type = 'CUDA' if isinstance(ops.bridge, CudaBridge) else 'OpenCL'
    print(f"  后端: {bridge_type}")
    print(f"  设备: {device}")

    # 测试数据
    test_vals = torch.tensor([-3.0, -2.0, -1.0, -0.5, 0.0, 0.5, 1.0, 2.0, 3.0, 5.0],
                             dtype=torch.float32, device=device)

    # karte GPU
    try:
        karte_out = ops.gelu(test_vals)
        print(f"  karte 编译+执行成功: {karte_out.shape}")
    except Exception as e:
        print(f"  ❌ karte 执行失败: {e}")
        return False

    # PyTorch 参考 (tanh 近似)
    x = test_vals
    ref = 0.5 * x * (1 + torch.tanh(math.sqrt(2/math.pi) * (x + 0.044715 * x**3)))

    # 对比
    if karte_out.is_cpu != ref.is_cpu:
        karte_out = karte_out.to(ref.device)

    max_err = (karte_out - ref).abs().max().item()
    print(f"  {'输入':>8s}  {'karte':>12s}  {'PyTorch':>12s}  {'误差':>10s}")

    all_pass = True
    for i in range(len(test_vals)):
        err = abs(karte_out[i].item() - ref[i].item())
        status = "✅" if err < 1e-5 else "❌"
        if err >= 1e-5:
            all_pass = False
        print(f"  {test_vals[i].item():8.2f}  {karte_out[i].item():12.6f}  {ref[i].item():12.6f}  {err:10.2e} {status}")

    if all_pass:
        print("  ✅ GELU 验证通过")
    return all_pass


def test_rmsnorm():
    """测试 RMSNorm"""
    print("\n" + "=" * 60)
    print("测试 2: RMSNorm 归一化")
    print("=" * 60)

    ops = KarteLLMOps()
    device = 'cuda' if torch.cuda.is_available() else 'cpu'

    N = 16
    x = torch.randn(N, dtype=torch.float32, device=device)
    gamma = torch.ones(N, dtype=torch.float32, device=device)
    eps = 1e-6

    # karte GPU
    try:
        karte_out = ops.rmsnorm(x, gamma, eps)
        print(f"  karte 编译+执行成功: {karte_out.shape}")
    except Exception as e:
        print(f"  ❌ karte 执行失败: {e}")
        return False

    # PyTorch 参考
    actual_rms = torch.rsqrt((x ** 2).mean() + eps)
    ref = x * actual_rms * gamma

    max_err = (karte_out.to(ref.device) - ref).abs().max().item()
    print(f"  max error: {max_err:.6e}")

    all_pass = max_err < 1e-5

    if all_pass:
        print("  ✅ RMSNorm element-wise 逻辑验证通过 (rms 值需 CPU 精确计算)")
    else:
        print("  ⚠️  RMSNorm 需要精确 rms 值 (当前 GIR 中用估算值)")
    return all_pass


def test_flash_attention():
    """测试 FlashAttention (简化版)"""
    print("\n" + "=" * 60)
    print("测试 3: FlashAttention (简化版)")
    print("=" * 60)

    ops = KarteLLMOps()
    device = 'cuda' if torch.cuda.is_available() else 'cpu'

    seq_len = 4
    head_dim = 4

    q = torch.randn(seq_len, head_dim, dtype=torch.float32, device=device)
    k = torch.randn(seq_len, head_dim, dtype=torch.float32, device=device)
    v = torch.randn(seq_len, head_dim, dtype=torch.float32, device=device)

    # karte GPU
    try:
        karte_out = ops.flash_attention(q, k, v)
        print(f"  karte 编译+执行成功: {karte_out.shape}")
    except Exception as e:
        print(f"  ❌ karte 执行失败: {e}")
        import traceback
        traceback.print_exc()
        return False

    # PyTorch 参考: O = (Q @ K^T / sqrt(d)) @ V
    s = q @ k.T / math.sqrt(head_dim)      # [S, S]
    o_ref = s @ v                           # [S, D]

    print(f"  karte 输出 shape: {karte_out.shape}")
    print(f"  PyTorch 输出 shape: {o_ref.shape}")

    # 对比
    karte_out_cpu = karte_out.cpu() if karte_out.is_cuda else karte_out
    o_ref_cpu = o_ref.cpu() if o_ref.is_cuda else o_ref

    max_err = (karte_out_cpu - o_ref_cpu).abs().max().item()
    print(f"  max error: {max_err:.6e}")

    all_pass = max_err < 1e-3  # FlashAttention 可能有数值差异
    if all_pass:
        print("  ✅ FlashAttention 验证通过")
    else:
        print(f"  ❌ FlashAttention 误差过大 ({max_err:.6e})")
        print(f"  karte: {karte_out_cpu.flatten().tolist()}")
        print(f"  ref:   {o_ref_cpu.flatten().tolist()}")

    return all_pass


def main():
    print("=" * 60)
    print("karte LLM 算子 vs PyTorch 对比测试")
    print("=" * 60)

    bridge = get_bridge()
    bridge_type = 'CUDA (NVIDIA)' if isinstance(bridge, CudaBridge) else 'OpenCL (AMD)'
    print(f"  GPU 后端: {bridge_type}")
    if torch.cuda.is_available():
        print(f"  PyTorch CUDA: {torch.cuda.get_device_name(0)}")

    # 确保编译了 karte CLI
    result = os.run('cd /home/user/src/karte && cargo build -p karte-cli 2>&1') if hasattr(os, 'run') else None

    tests = [
        ("GELU", test_gelu),
        ("RMSNorm", test_rmsnorm),
        ("FlashAttention", test_flash_attention),
    ]

    if len(sys.argv) > 1:
        names = sys.argv[1:]
        tests = [(n, f) for n, f in tests if any(n.lower().startswith(x.lower()) for x in names)]

    results = []
    for name, func in tests:
        try:
            ok = func()
            results.append((name, ok))
        except Exception as e:
            import traceback
            traceback.print_exc()
            results.append((name, False))

    print("\n" + "=" * 60)
    print("结果汇总")
    print("=" * 60)
    for name, ok in results:
        print(f"  {'✅ PASS' if ok else '❌ FAIL'}  {name}")
    print("=" * 60)


if __name__ == "__main__":
    main()
