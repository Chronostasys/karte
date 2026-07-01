#!/usr/bin/env python3
"""
Multi-workgroup GEMM + GELU 测试 + 性能基准

验证:
1. Multi-block GELU (N=1024, block_size=256 → 4 blocks)
2. GEMM (8×8 = A[8,4] @ B[4,8])
3. 性能对比: karte vs PyTorch
"""
import torch
import time
import sys
import os

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__))))
from karte_gpu.torch_bridge import KarteLLMOps, get_bridge, CudaBridge


def test_gelu_multiblock():
    """Multi-block GELU 测试 (N=1024)"""
    print("\n" + "=" * 60)
    print("测试 4: Multi-block GELU (N=1024, 4 blocks × 256 threads)")
    print("=" * 60)

    ops = KarteLLMOps()
    device = 'cuda' if torch.cuda.is_available() else 'cpu'
    N = 1024
    block_size = 256

    x = torch.randn(N, dtype=torch.float32, device=device)

    try:
        karte_out = ops.gelu_multiblock(x, block_size=block_size)
        print(f"  karte multi-block GELU 成功: {karte_out.shape}")
    except Exception as e:
        print(f"  ❌ 执行失败: {e}")
        import traceback
        traceback.print_exc()
        return False

    # PyTorch 参考
    ref = torch.nn.functional.gelu(x, approximate='tanh')

    max_err = (karte_out.to(ref.device) - ref).abs().max().item()
    print(f"  max error: {max_err:.6e}")
    all_pass = max_err < 1e-5

    print(f"  {'✅' if all_pass else '❌'} Multi-block GELU (N=1024)")
    return all_pass


def test_gemm():
    """GEMM 矩阵乘法测试"""
    print("\n" + "=" * 60)
    print("测试 5: GEMM (Multi-workgroup)")
    print("=" * 60)

    ops = KarteLLMOps()
    device = 'cuda' if torch.cuda.is_available() else 'cpu'

    test_cases = [
        (4, 4, 4, "4×4 × 4×4"),
        (8, 4, 8, "8×4 × 4×8"),
        (8, 8, 8, "8×8 × 8×8"),
    ]

    all_pass = True
    for M, K, N, desc in test_cases:
        a = torch.randn(M, K, dtype=torch.float32, device=device)
        b = torch.randn(K, N, dtype=torch.float32, device=device)

        try:
            karte_out = ops.gemm(a, b)
        except Exception as e:
            print(f"  ❌ {desc}: {e}")
            all_pass = False
            continue

        ref = a @ b

        max_err = (karte_out.to(ref.device) - ref).abs().max().item()
        ok = max_err < 1e-4
        if not ok:
            all_pass = False
        print(f"  {'✅' if ok else '❌'} GEMM {desc}: max_err={max_err:.6e}")

    return all_pass


def test_large_gelu():
    """大尺寸 GELU 测试 (N=4096)"""
    print("\n" + "=" * 60)
    print("测试 6: 大尺寸 GELU (N=4096)")
    print("=" * 60)

    ops = KarteLLMOps()
    device = 'cuda' if torch.cuda.is_available() else 'cpu'
    N = 4096
    block_size = 256

    x = torch.randn(N, dtype=torch.float32, device=device)

    try:
        karte_out = ops.gelu_multiblock(x, block_size=block_size)
        ref = torch.nn.functional.gelu(x, approximate='tanh')
        max_err = (karte_out.to(ref.device) - ref).abs().max().item()
        ok = max_err < 1e-5
        print(f"  max error: {max_err:.6e}")
        print(f"  {'✅' if ok else '❌'} N=4096, 16 blocks × 256 threads")
        return ok
    except Exception as e:
        print(f"  ❌ {e}")
        return False


def benchmark():
    """性能对比: karte vs PyTorch"""
    print("\n" + "=" * 60)
    print("性能基准: karte vs PyTorch")
    print("=" * 60)

    ops = KarteLLMOps()
    device = 'cuda' if torch.cuda.is_available() else 'cpu'

    # GELU benchmark
    N = 1024
    x = torch.randn(N, dtype=torch.float32, device=device)
    warmup = 3
    runs = 10

    # Warmup
    for _ in range(warmup):
        ops.gelu_multiblock(x)
        torch.nn.functional.gelu(x, approximate='tanh')

    # karte
    torch.cuda.synchronize() if device == 'cuda' else None
    t0 = time.perf_counter()
    for _ in range(runs):
        ops.gelu_multiblock(x)
    torch.cuda.synchronize() if device == 'cuda' else None
    karte_time = (time.perf_counter() - t0) / runs * 1000

    # PyTorch
    torch.cuda.synchronize() if device == 'cuda' else None
    t0 = time.perf_counter()
    for _ in range(runs):
        torch.nn.functional.gelu(x, approximate='tanh')
    torch.cuda.synchronize() if device == 'cuda' else None
    pytorch_time = (time.perf_counter() - t0) / runs * 1000

    print(f"  GELU N={N}:")
    print(f"    karte:   {karte_time:.3f} ms")
    print(f"    PyTorch: {pytorch_time:.3f} ms")
    print(f"    比值:    {pytorch_time/karte_time:.2f}x")

    # GEMM benchmark
    M, K, N = 8, 8, 8
    a = torch.randn(M, K, dtype=torch.float32, device=device)
    b = torch.randn(K, N, dtype=torch.float32, device=device)

    # Warmup
    for _ in range(warmup):
        ops.gemm(a, b)
        a @ b

    # karte
    torch.cuda.synchronize() if device == 'cuda' else None
    t0 = time.perf_counter()
    for _ in range(runs):
        ops.gemm(a, b)
    torch.cuda.synchronize() if device == 'cuda' else None
    karte_gemm_time = (time.perf_counter() - t0) / runs * 1000

    # PyTorch
    torch.cuda.synchronize() if device == 'cuda' else None
    t0 = time.perf_counter()
    for _ in range(runs):
        a @ b
    torch.cuda.synchronize() if device == 'cuda' else None
    pytorch_gemm_time = (time.perf_counter() - t0) / runs * 1000

    print(f"  GEMM {M}×{K}×{N}:")
    print(f"    karte:   {karte_gemm_time:.3f} ms")
    print(f"    PyTorch: {pytorch_gemm_time:.3f} ms")
    print(f"    比值:    {pytorch_gemm_time/karte_gemm_time:.2f}x")


def main():
    print("=" * 60)
    print("karte Multi-workgroup 测试 + 性能基准")
    print("=" * 60)

    bridge = get_bridge()
    bridge_type = 'CUDA (NVIDIA)' if isinstance(bridge, CudaBridge) else 'OpenCL (AMD)'
    print(f"  GPU 后端: {bridge_type}")

    tests = [
        ("GELU Multi-block", test_gelu_multiblock),
        ("GEMM", test_gemm),
        ("GELU Large", test_large_gelu),
    ]

    results = []
    for name, func in tests:
        try:
            ok = func()
            results.append((name, ok))
        except Exception as e:
            import traceback
            traceback.print_exc()
            results.append((name, False))

    # 性能基准
    try:
        benchmark()
    except Exception as e:
        print(f"  基准测试失败: {e}")

    print("\n" + "=" * 60)
    print("结果汇总")
    print("=" * 60)
    for name, ok in results:
        print(f"  {'✅ PASS' if ok else '❌ FAIL'}  {name}")
    print("=" * 60)


if __name__ == "__main__":
    main()
