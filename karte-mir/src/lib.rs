//! `karte` 编译器的中级中间表示 (MIR)。
//!
//! MIR 是一种基于控制流图（CFG）的表示形式，位于高级中间表示（HIR）和低级中间表示（LIR）之间。
//! 它的设计旨在简化数据流分析和各种优化。
//!
//! MIR 的核心概念包括：
//!
//! - **`Value`**: 操作数，可以是变量、常量、临时值等。
//! - **`Statement`**: 不改变控制流的简单操作，如赋值、二元运算等。
//! - **`Terminator`**: 位于基本块末尾，用于改变控制流的指令，如分支和函数返回。
//! - **`BasicBlock`**: 一系列顺序执行的 `Statement`，并以一个 `Terminator` 结尾。
//! - **`MirFunction`**: 一个函数在 MIR 中的表示，由多个基本块构成。
//! - **`MirProgram`**: 整个程序在 MIR 中的表示，包含所有函数。
//!
//! 从 HIR 到 MIR 的降低（lowering）过程（见 `lower` 模块）会将抽象语法树（AST）的结构
//! 转换成 MIR 的显式 CFG 形式，从而为后续的编译阶段（如 LIR 生成）提供一个更清晰、
//! 更易于分析的输入。

pub fn add(left: u64, right: u64) -> u64 {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}

pub mod codec;
pub mod ir;
pub mod lower;

pub use ir::*;
