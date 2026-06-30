//! OpenCL 运行时实现
//!
//! 实现 GpuRuntime trait，通过 OpenCL API 操作 GPU。
//! 实际 kernel 编译和启动在 Python 层通过 ctypes 完成（与 CUDA 路线一致）。

use crate::runtime::{GpuRuntime, LaunchConfig, ModuleHandle};
use super::ffi;

/// OpenCL GPU 运行时
pub struct OpenClRuntime {
    /// 是否已初始化
    initialized: std::sync::atomic::AtomicBool,
}

impl OpenClRuntime {
    pub fn new() -> Self {
        Self {
            initialized: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

impl Default for OpenClRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl GpuRuntime for OpenClRuntime {
    fn backend_name(&self) -> &str {
        "opencl"
    }

    fn init(&self) -> Result<(), String> {
        if !ffi::is_opencl_available() {
            return Err("OpenCL 库未找到 — 请安装 ROCm (AMD) 或 Intel OpenCL runtime".to_string());
        }
        self.initialized
            .store(true, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    fn is_available(&self) -> bool {
        ffi::is_opencl_available()
    }

    fn alloc(&self, _nbytes: usize) -> Result<u64, String> {
        // 实际分配在 Python 层通过 ctypes 调用 clCreateBuffer
        // Rust 层提供接口定义和检测
        Err("OpenCL 内存分配需通过 Python ctypes 层调用 clCreateBuffer".to_string())
    }

    fn free(&self, _ptr: u64) -> Result<(), String> {
        Err("OpenCL 内存释放需通过 Python ctypes 层调用 clReleaseMemObject".to_string())
    }

    fn h2d(&self, _dst: u64, _src: *const u8, _nbytes: usize) -> Result<(), String> {
        Err("OpenCL H2D 拷贝需通过 Python ctypes 层调用 clEnqueueWriteBuffer".to_string())
    }

    fn d2h(&self, _dst: *mut u8, _src: u64, _nbytes: usize) -> Result<(), String> {
        Err("OpenCL D2H 拷贝需通过 Python ctypes 层调用 clEnqueueReadBuffer".to_string())
    }

    fn load_module(&self, data: &[u8]) -> Result<ModuleHandle, String> {
        // OpenCL C 源码编译在 Python 层通过 clCreateProgramWithSource 完成
        let _ = data;
        Err("OpenCL 模块编译需通过 Python ctypes 层调用 clCreateProgramWithSource + clBuildProgram".to_string())
    }

    fn get_kernel(&self, _module: &ModuleHandle, _name: &str) -> Result<ModuleHandle, String> {
        Err("OpenCL kernel 获取需通过 Python ctypes 层调用 clCreateKernel".to_string())
    }

    fn launch(
        &self,
        _kernel: &ModuleHandle,
        _config: LaunchConfig,
        _args: &[u64],
    ) -> Result<(), String> {
        Err("OpenCL kernel 启动需通过 Python ctypes 层调用 clEnqueueNDRangeKernel".to_string())
    }

    fn synchronize(&self) -> Result<(), String> {
        Err("OpenCL 同步需通过 Python ctypes 层调用 clFinish".to_string())
    }
}
