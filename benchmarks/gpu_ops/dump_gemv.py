import sys; sys.path.insert(0, '/home/user/src/karte/benchmarks/gpu_ops')
import karte_jit as karte
import subprocess, json, torch

# 捕获 PTX
original_run = subprocess.run
captured = {}

class MockResult:
    def __init__(self, ret, out, err):
        self.returncode = ret; self.stdout = out; self.stderr = err

def capturing_run(cmd, **kw):
    if 'gpu-jit' in str(cmd) and 'input' in kw:
        result = original_run(cmd, **kw)
        captured['ptx'] = result.stdout
        return result
    return original_run(cmd, **kw)

subprocess.run = capturing_run

@karte.jit
def gemv(a: karte.Tensor["M", "K"], b: karte.Tensor["K"]) -> karte.Tensor["M"]:
    tid = karte.thread_id()
    acc = karte.f32(0.0)
    for k in karte.unroll(4):
        acc = acc + a[tid, k] * b[k]
    return acc

subprocess.run = original_run

a = torch.randn(8, 4, device='cuda', dtype=torch.float32)
b = torch.randn(4, device='cuda', dtype=torch.float32)
r = gemv(a, b)
e = a @ b
d = (r - e).abs().max().item()
print(f'GEMV: max_diff={d:.4e}')

if 'ptx' in captured:
    ptx = captured['ptx']
    with open('/tmp/gemv_debug.ptx', 'w') as f: f.write(ptx)
    # 找到地址计算相关的指令
    for line in ptx.split('\n'):
        l = line.strip()
        if l and not l.startswith('.') and not l.startswith('{') and not l.startswith('}') and any(k in l for k in ['mul', 'add', 'ld.', 'st.', 'cvt', 'mov.u']):
            print(l)
