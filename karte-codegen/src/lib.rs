//! `karte` 编译器的代码生成与执行后端。
//!
//! `codegen` 是编译流程的最后阶段。它的主要任务是将低级中间表示（LIR）
//! 转换为可执行的代码或在某种虚拟机上运行的字节码。
//!
//! 在一个完整的编译器中，这部分将包含：
//! - **指令选择**: 将 LIR 指令映射到目标架构（如 x86、ARM）的机器指令。
//! - **寄存器分配**: 将 LIR 中的虚拟寄存器分配到物理寄存器。
//! - **代码输出**: 生成汇编代码或二进制目标文件。
//!
//! 目前，为了快速迭代和测试，`codegen` 模块实现了解释器，可以直接执行
//! HIR 和 LIR，这使得我们可以在不进行完整编译到机器码的情况下验证语言的语义和
//! 各个编译阶段的正确性。
//!
//! - **`hir_interpreter`**: 一个可以直接执行 HIR 的树遍历解释器，用于在早期阶段快速验证语义。
//! - **`lir_interpreter`**: 一个可以执行线性 LIR 指令的解释器，用于验证 MIR 和 LIR 转换的正确性。

pub mod hir_interpreter;
pub mod lir_interpreter;
pub mod vm;
pub use hir_interpreter::*;
pub use lir_interpreter::*;
