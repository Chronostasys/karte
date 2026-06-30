//! OpenCL 运行时模块
//!
//! 提供 OpenCL FFI 绑定和 GPU 运行时实现，
//! 支持 AMD / Intel / NVIDIA GPU。

pub mod ffi;
pub mod runtime;

pub use runtime::OpenClRuntime;
