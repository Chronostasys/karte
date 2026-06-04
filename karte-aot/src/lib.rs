//! karte-aot — AOT 编译器，生成独立可执行文件
//!
//! 支持 Linux x86_64, AArch64 和 RISC-V 64 平台。
//! 生成的二进制不依赖 glibc，使用原始系统调用。

pub mod elf;
pub mod runtime_x86;
pub mod runtime_aarch64;
pub mod runtime_riscv;
pub mod compiler;

pub use compiler::AotCompiler;
pub use compiler::AotTarget;
