//! `karte` 编译器的低级中间表示 (LIR)。
//!
//! LIR 是一种线性的、更接近于机器码的表示形式，位于中级中间表示（MIR）之后，
//! 最终代码生成（codegen）之前。它的设计旨在简化到目标机器指令的转换过程。
//!
//! LIR 的核心概念是线性的指令序列，而不是 MIR 中的控制流图。
//!
//! - **`Operand`**: 操作数，可以是寄存器、立即数或内存地址。
//! - **`Instruction`**: 类似于汇编的指令，如 `mov`, `add`, `jmp` 等。
//! - **`LirFunction`**: 一个函数在 LIR 中的表示，由一系列指令构成。
//! - **`LirProgram`**: 整个程序在 LIR 中的表示。
//!
//! 从 MIR 到 LIR 的降低（lowering）过程（见 `lower` 模块）会将 MIR 的基本块结构
//! "线性化"为一个指令流，将 `Value` 映射到寄存器或栈上的位置，并将控制流
//! 转换为显式的跳转指令。这个阶段是为最终的代码生成做准备的关键一步。
//!
//! ## 结构体支持
//!
//! 新的LIR设计包含了专业的结构体支持：
//! - **`StructLayoutManager`**: 管理结构体的内存布局
//! - **结构体专用指令**: 如 `StructAlloc`, `StructFieldLoad`, `StructFieldStore`
//! - **内存管理**: 支持栈分配和堆分配
//! - **对齐优化**: 自动计算字段对齐和填充

pub mod ir;
pub mod lower;
pub mod optimization_pipeline;
pub mod pass;
pub mod struct_layout;
pub mod tagged_union;

pub use ir::*;
pub use lower::*;
pub use optimization_pipeline::*;
pub use pass::*;
pub use struct_layout::*;
pub use tagged_union::*;
