"""
karte_gpu — 用纯 Python 编写高性能 GPU 算子

@karte.jit 装饰器将 Python 函数自动编译为 NVIDIA PTX，
通过 Rust 编译器管线优化后直接在 GPU 上执行。

快速上手:
    import karte_gpu as karte

    @karte.jit
    def my_kernel(x: karte.Tensor["N"],
                  y: karte.Tensor["N"]) -> karte.Tensor["N"]:
        tid = karte.thread_id()
        a = x[tid]
        b = y[tid]
        return a + b

    result = my_kernel(x_tensor, y_tensor)
"""

from .karte_jit import (
    jit, Tensor, thread_id, f32, dot, exp, sqrt, rsqrt, log,
    tanh, cos, sin, clamp, lerp, ceil, floor, pow,
    max_val, min_val, abs_val, where, reduce_sum, reduce_max,
    unroll, is_interpret_mode, autograd,
)

__version__ = "0.1.0"

__all__ = [
    # 核心 API
    'jit', 'Tensor', 'autograd',
    # 线程控制
    'thread_id', 'unroll',
    # 数学函数
    'f32', 'dot', 'exp', 'sqrt', 'rsqrt', 'log',
    'tanh', 'cos', 'sin', 'clamp', 'lerp', 'ceil', 'floor', 'pow',
    'max_val', 'min_val', 'abs_val',
    # 条件与归约
    'where', 'reduce_sum', 'reduce_max',
    # 工具
    'is_interpret_mode',
]
