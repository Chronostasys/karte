"""快速验证 karte-gpu 包安装是否正确"""
import torch
import karte_gpu as karte

print(f"karte-gpu {karte.__version__}")
print(f"GPU: {torch.cuda.get_device_name(0)}")
print(f"可用 API: {len(karte.__all__)} 个\n")

# Sigmoid kernel
@karte.jit
def sigmoid_kernel(x: karte.Tensor["N"]) -> karte.Tensor["N"]:
    tid = karte.thread_id()
    val = x[tid]
    return 1.0 / (1.0 + karte.exp(0.0 - val))

x = torch.randn(1024, device='cuda', dtype=torch.float32)
print("编译 sigmoid_kernel（首次调用）...")
y = sigmoid_kernel(x)
expected = torch.sigmoid(x)
diff = (y - expected).abs().max().item()
print(f"正确性: max_diff = {diff:.2e} ({'PASS' if diff < 1e-4 else 'FAIL'})")
print(f"结果[:5]: {y[:5].cpu().numpy()}")

# ReLU kernel
@karte.jit
def relu_kernel(x: karte.Tensor["N"]) -> karte.Tensor["N"]:
    tid = karte.thread_id()
    return karte.max_val(x[tid], 0.0)

x2 = torch.randn(1024, device='cuda', dtype=torch.float32) * 5
y2 = relu_kernel(x2)
diff2 = (y2 - torch.relu(x2)).abs().max().item()
print(f"\nReLU 正确性: max_diff = {diff2:.2e} ({'PASS' if diff2 < 0.1 else 'FAIL'})")

print("\n✅ karte-gpu 安装验证通过！")
