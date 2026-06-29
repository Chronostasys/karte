//! CUDA Driver API FFI 绑定
//!
//! 最小化的 CUDA Driver API 子集，仅包含 Karte GPU 需要的核心函数。

#![allow(non_camel_case_types)]

use libc::{c_int, c_void};

/// CUDA 类型别名
pub type CuResult = c_int;
pub type CuDevice = c_int;
pub type CuContext = *mut c_void;
pub type CuModule = *mut c_void;
pub type CuFunction = *mut c_void;
pub type CuDeviceptr = u64;

/// CUDA 成功状态码
pub const CUDA_SUCCESS: CuResult = 0;

// —— 库加载策略 ——
// 动态加载 libcuda.so.1，如果找不到则在运行时优雅降级

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use std::ffi::CString;

    /// 动态加载 CUDA 库并获取函数指针
    pub fn try_init_cuda() -> Option<CudaLib> {
        unsafe {
            let lib = libc::dlopen(
                CString::new("libcuda.so.1").unwrap().as_ptr(),
                libc::RTLD_LAZY | libc::RTLD_GLOBAL,
            );
            if lib.is_null() {
                let lib2 = libc::dlopen(
                    CString::new("libcuda.so").unwrap().as_ptr(),
                    libc::RTLD_LAZY | libc::RTLD_GLOBAL,
                );
                if lib2.is_null() {
                    return None;
                }
                Some(CudaLib { handle: lib2 })
            } else {
                Some(CudaLib { handle: lib })
            }
        }
    }

    /// 已加载的 CUDA 动态库句柄
    pub struct CudaLib {
        pub handle: *mut c_void,
    }

    impl Drop for CudaLib {
        fn drop(&mut self) {
            if !self.handle.is_null() {
                unsafe { libc::dlclose(self.handle); }
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod platform {
    pub fn try_init_cuda() -> Option<CudaLib> { None }
    pub struct CudaLib;
}

pub use platform::*;

/// 初始化 CUDA — 返回 true 表示成功
pub fn gpu_init() -> bool {
    // 在实际实现中，这里会调用 cuInit(0)
    // 目前返回 false 表示无 GPU 支持（CPU 回退模式）
    false
}

/// 检查 CUDA 是否可用
pub fn is_cuda_available() -> bool {
    platform::try_init_cuda().is_some()
}
