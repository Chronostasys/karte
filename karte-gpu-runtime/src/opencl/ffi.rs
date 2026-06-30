//! OpenCL FFI 绑定
//!
//! 动态加载 libOpenCL.so，提供 OpenCL API 类型定义。
//! 遵循 Karte 的 "Runtime 只提供 OS/硬件抽象层" 原则。
//! 实际 kernel 编译和启动在 Python 层通过 ctypes 完成（与 CUDA 路线一致）。

#![allow(non_camel_case_types)]

use libc::{c_int, c_void};

// —— OpenCL 类型别名 ——
pub type cl_int = c_int;
pub type cl_uint = u32;
pub type cl_platform_id = *mut c_void;
pub type cl_device_id = *mut c_void;
pub type cl_context = *mut c_void;
pub type cl_command_queue = *mut c_void;
pub type cl_program = *mut c_void;
pub type cl_kernel = *mut c_void;
pub type cl_mem = *mut c_void;
pub type cl_device_type = u64;
pub type cl_event = *mut c_void;

// —— OpenCL 错误码 ——
pub const CL_SUCCESS: cl_int = 0;
pub const CL_DEVICE_NOT_FOUND: cl_int = -1;
pub const CL_DEVICE_NOT_AVAILABLE: cl_int = -2;
pub const CL_COMPILER_NOT_AVAILABLE: cl_int = -3;
pub const CL_BUILD_PROGRAM_FAILURE: cl_int = -11;
pub const CL_INVALID_PLATFORM: cl_int = -32;
pub const CL_INVALID_DEVICE: cl_int = -33;

// —— 设备类型 ——
pub const CL_DEVICE_TYPE_DEFAULT: cl_device_type = 1 << 0;
pub const CL_DEVICE_TYPE_CPU: cl_device_type = 1 << 1;
pub const CL_DEVICE_TYPE_GPU: cl_device_type = 1 << 2;
pub const CL_DEVICE_TYPE_ALL: cl_device_type = 0xFFFFFFFF;

// —— 内存标志 ——
pub const CL_MEM_READ_WRITE: cl_uint = 1;
pub const CL_MEM_READ_ONLY: cl_uint = 2;
pub const CL_MEM_WRITE_ONLY: cl_uint = 4;
pub const CL_MEM_COPY_HOST_PTR: cl_uint = 1 << 5;

// —— 信息查询 ——
pub const CL_DEVICE_NAME: cl_int = 0x1027;
pub const CL_DEVICE_VENDOR: cl_int = 0x1028;
pub const CL_DEVICE_VERSION: cl_int = 0x102F;
pub const CL_DEVICE_IL_VERSION: cl_int = 0x105B;
pub const CL_DEVICE_MAX_WORK_GROUP_SIZE: cl_int = 0x1004;
pub const CL_DEVICE_MAX_COMPUTE_UNITS: cl_int = 0x1002;
pub const CL_DEVICE_GLOBAL_MEM_SIZE: cl_int = 0x1010;

pub const CL_PROGRAM_BUILD_STATUS: cl_int = 0x1083;
pub const CL_PROGRAM_BUILD_LOG: cl_int = 0x1084;

pub const CL_BUILD_SUCCESS: cl_int = 0;
pub const CL_BUILD_ERROR: cl_int = -2;

// —— 队列属性 ——
pub const CL_QUEUE_PROPERTIES: cl_int = 0x1097;
pub const CL_QUEUE_NONE: u64 = 0;

// —— 动态加载 ——
#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use std::ffi::CString;

    /// 动态加载 OpenCL 库
    pub fn try_init_opencl() -> Option<OpenClLib> {
        let candidates = [
            "libOpenCL.so.1",
            "libOpenCL.so",
            "/opt/rocm/opencl/lib/libOpenCL.so",
            "/usr/lib/x86_64-linux-gnu/libOpenCL.so.1",
        ];

        for path in &candidates {
            unsafe {
                let lib = libc::dlopen(
                    CString::new(*path).unwrap().as_ptr(),
                    libc::RTLD_LAZY | libc::RTLD_GLOBAL,
                );
                if !lib.is_null() {
                    return Some(OpenClLib { handle: lib });
                }
            }
        }
        None
    }

    /// 已加载的 OpenCL 动态库句柄
    pub struct OpenClLib {
        pub handle: *mut c_void,
    }

    impl Drop for OpenClLib {
        fn drop(&mut self) {
            if !self.handle.is_null() {
                unsafe { libc::dlclose(self.handle); }
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod platform {
    use super::*;
    pub fn try_init_opencl() -> Option<OpenClLib> { None }
    pub struct OpenClLib;
}

pub use platform::*;

/// 检测 OpenCL 是否可用
pub fn is_opencl_available() -> bool {
    try_init_opencl().is_some()
}

/// 将 cl_int 错误码转为可读字符串
pub fn cl_error_str(err: cl_int) -> &'static str {
    match err {
        CL_SUCCESS => "CL_SUCCESS",
        CL_DEVICE_NOT_FOUND => "CL_DEVICE_NOT_FOUND",
        CL_DEVICE_NOT_AVAILABLE => "CL_DEVICE_NOT_AVAILABLE",
        CL_COMPILER_NOT_AVAILABLE => "CL_COMPILER_NOT_AVAILABLE",
        CL_BUILD_PROGRAM_FAILURE => "CL_BUILD_PROGRAM_FAILURE",
        CL_INVALID_PLATFORM => "CL_INVALID_PLATFORM",
        CL_INVALID_DEVICE => "CL_INVALID_DEVICE",
        _ => "CL_UNKNOWN_ERROR",
    }
}
