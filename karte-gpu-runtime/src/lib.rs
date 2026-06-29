//! # karte-gpu-runtime — GPU 运行时 (CUDA Driver API FFI 绑定)
//!
//! 提供 GPU 内存管理、kernel 加载和启动的底层 API。
//! 遵循 Karte 的 "Runtime 只提供 OS/硬件抽象层" 原则。

pub mod ffi;
pub mod tensor;
pub mod launcher;
pub mod cpu_executor;

pub use tensor::GpuTensor;
pub use launcher::KernelLauncher;
pub use cpu_executor::{CpuExecutor, execute_kernel_on_cpu};
