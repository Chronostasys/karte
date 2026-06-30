//! CUDA 运行时实现
//!
//! 实现 GpuBackend trait，包装现有 CUDA FFI 绑定。
//! 与 OpenClRuntime 对称，由 GpuRuntime trait 统一调度。

use crate::runtime::{GpuRuntime, LaunchConfig, ModuleHandle};

/// CUDA GPU 运行时
pub struct CudaRuntime {
    /// 是否已初始化
    initialized: std::sync::atomic::AtomicBool,
}

impl CudaRuntime {
    pub fn new() -> Self {
        Self {
            initialized: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

impl Default for CudaRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl GpuRuntime for CudaRuntime {
    fn backend_name(&self) -> &str {
        "cuda"
    }

    fn init(&self) -> Result<(), String> {
        if !crate::ffi::is_cuda_available() {
            return Err("CUDA 库未找到 — 请安装 NVIDIA CUDA driver".to_string());
        }
        self.initialized
            .store(true, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    fn is_available(&self) -> bool {
        crate::ffi::is_cuda_available()
    }

    fn alloc(&self, _nbytes: usize) -> Result<u64, String> {
        // 实际分配在 Python 层通过 ctypes 调用 cuMemAlloc_v2
        Err("CUDA 内存分配需通过 Python ctypes 层调用 cuMemAlloc_v2".to_string())
    }

    fn free(&self, _ptr: u64) -> Result<(), String> {
        Err("CUDA 内存释放需通过 Python ctypes 层调用 cuMemFree".to_string())
    }

    fn h2d(&self, _dst: u64, _src: *const u8, _nbytes: usize) -> Result<(), String> {
        Err("CUDA H2D 拷贝需通过 Python ctypes 层调用 cuMemcpyHtoD_v2".to_string())
    }

    fn d2h(&self, _dst: *mut u8, _src: u64, _nbytes: usize) -> Result<(), String> {
        Err("CUDA D2H 拷贝需通过 Python ctypes 层调用 cuMemcpyDtoH_v2".to_string())
    }

    fn load_module(&self, data: &[u8]) -> Result<ModuleHandle, String> {
        // PTX 文本加载在 Python 层通过 cuModuleLoadData 完成
        let _ = data;
        Err("CUDA 模块加载需通过 Python ctypes 层调用 cuModuleLoadData".to_string())
    }

    fn get_kernel(&self, _module: &ModuleHandle, _name: &str) -> Result<ModuleHandle, String> {
        Err("CUDA kernel 获取需通过 Python ctypes 层调用 cuModuleGetFunction".to_string())
    }

    fn launch(
        &self,
        _kernel: &ModuleHandle,
        _config: LaunchConfig,
        _args: &[u64],
    ) -> Result<(), String> {
        Err("CUDA kernel 启动需通过 Python ctypes 层调用 cuLaunchKernel".to_string())
    }

    fn synchronize(&self) -> Result<(), String> {
        Err("CUDA 同步需通过 Python ctypes 层调用 cuCtxSynchronize".to_string())
    }
}
