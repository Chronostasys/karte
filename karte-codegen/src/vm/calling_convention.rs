//! Karte 虚拟机调用约定
//! 
//! 定义了函数调用时的寄存器使用规范，遵循现代编译器的最佳实践。
//! 采用类似 System V ABI 的约定，适合 RISC 架构。

pub use karte_common::calling_convention::*;