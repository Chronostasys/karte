//! # karte-gpu-runtime — GPU 运行时 (多后端 FFI 绑定)
//!
//! 提供 GPU 内存管理、kernel 加载和启动的底层 API。
//! 支持 CUDA (NVIDIA) 和 OpenCL (AMD/Intel/NVIDIA) 双后端。
//! 遵循 Karte 的 "Runtime 只提供 OS/硬件抽象层" 原则。

pub mod ffi;
pub mod tensor;
pub mod launcher;
pub mod cpu_executor;
pub mod runtime;
pub mod opencl;
pub mod cuda_runtime;

pub use tensor::GpuTensor;
pub use launcher::KernelLauncher;
pub use cpu_executor::{CpuExecutor, execute_kernel_on_cpu};
pub use runtime::{GpuRuntime, LaunchConfig, ModuleHandle, detect_available_backends, auto_select_backend};
pub use opencl::OpenClRuntime;
pub use cuda_runtime::CudaRuntime;
