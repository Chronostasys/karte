import sys; sys.path.insert(0, '/home/user/src/karte/benchmarks/gpu_ops')
import karte_jit as karte
import torch

@karte.jit
def row_first(a: karte.Tensor["M", "K"]) -> karte.Tensor["M"]:
    tid = karte.thread_id()
    return a[tid, 0]

# Test with M=256 (fills the block)
M, K = 256, 4
a = torch.arange(M*K, device='cuda', dtype=torch.float32).reshape(M, K)
r = row_first(a)
e = a[:, 0]
d = (r - e).abs().max().item()
status = 'PASS' if d < 0.01 else 'FAIL'
print(f'RowFirst M=256: max_diff={d:.4e} ({status})')
if d > 0.01:
    print(f'  result[:8]: {r[:8].cpu().numpy()}')
    print(f'  expect[:8]: {e[:8].cpu().numpy()}')

# Test with M=4 (underfills block — OOB threads)
a4 = torch.tensor([[10,1,1,1],[20,2,2,2],[30,3,3,3],[40,4,4,4]],
                  device='cuda', dtype=torch.float32)
r4 = row_first(a4)
e4 = a4[:, 0]
d4 = (r4 - e4).abs().max().item()
status4 = 'PASS' if d4 < 0.01 else 'FAIL'
print(f'\nRowFirst M=4:   max_diff={d4:.4e} ({status4})')
print(f'  result: {r4.cpu().numpy()}')
print(f'  expect: {e4.cpu().numpy()}')
