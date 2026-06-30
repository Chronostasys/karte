import sys; sys.path.insert(0, '/home/user/src/karte/benchmarks/gpu_ops')
import karte_jit as karte
import karte_jit as kj
import torch, ctypes, ctypes.util, numpy as np

# 保存原始 _create_kernel_wrapper
_orig_create = kj._create_kernel_wrapper

def debug_create(func, builder, name, block_size=256, fn=None):
    """添加调试输出的 wrapper"""
    wrapper = _orig_create(func, builder, name, block_size, fn)
    
    def debug_wrapper(*args, **kwargs):
        print(f"  [DEBUG] kernel '{name}' called with {len(args)} args")
        for i, a in enumerate(args):
            if isinstance(a, torch.Tensor):
                print(f"  [DEBUG] arg[{i}]: Tensor shape={a.shape} ptr={a.data_ptr():#x} cuda={a.is_cuda}")
            else:
                print(f"  [DEBUG] arg[{i}]: {type(a).__name__} val={a}")
        result = wrapper(*args, **kwargs)
        if isinstance(result, torch.Tensor):
            print(f"  [DEBUG] result: shape={result.shape} val={result.cpu().numpy()[:8]}")
        return result
    
    return debug_wrapper

kj._create_kernel_wrapper = debug_create

@karte.jit
def row_first(a: karte.Tensor["M", "K"]) -> karte.Tensor["M"]:
    tid = karte.thread_id()
    return a[tid, 0]

a = torch.tensor([[10,1,1,1],[20,2,2,2],[30,3,3,3],[40,4,4,4]],
                 device='cuda', dtype=torch.float32)
print("Running row_first...")
r = row_first(a)
print(f"\nFinal result: {r.cpu().numpy()}")
print(f"Expected:     [10. 20. 30. 40.]")
