//! GPU 运行时抽象层
//!
//! 统一 CUDA 和 OpenCL 后端的运行时接口，
//! 使上层代码不感知具体 GPU 厂商。

use std::ffi::c_void;

/// Kernel 启动配置
#[derive(Debug, Clone, Copy)]
pub struct LaunchConfig {
    /// Grid 维度
    pub grid: (usize, usize, usize),
    /// Block 维度
    pub block: (usize, usize, usize),
    /// 共享内存大小（字节）
    pub shared_mem: usize,
}

impl LaunchConfig {
    pub fn new(grid: usize, block: usize) -> Self {
        Self {
            grid: (grid, 1, 1),
            block: (block, 1, 1),
            shared_mem: 0,
        }
    }

    pub fn new_2d(grid: (usize, usize), block: (usize, usize)) -> Self {
        Self {
            grid: (grid.0, grid.1, 1),
            block: (block.0, block.1, 1),
            shared_mem: 0,
        }
    }

    pub fn with_shared_mem(mut self, bytes: usize) -> Self {
        self.shared_mem = bytes;
        self
    }

    /// 全局线程总数
    pub fn total_threads(&self) -> usize {
        self.grid.0 * self.grid.1 * self.grid.2
            * self.block.0 * self.block.1 * self.block.2
    }
}

/// 已加载的 GPU 模块句柄
pub type ModuleHandle = *mut c_void;

/// GPU 运行时抽象 — 后端无关的统一接口
///
/// 无论底层是 CUDA (NVIDIA) 还是 OpenCL (AMD/Intel)，
/// 上层代码通过此 trait 操作 GPU。
pub trait GpuRuntime: Send + Sync {
    /// 后端名称: "cuda" / "opencl"
    fn backend_name(&self) -> &str;

    /// 初始化运行时
    fn init(&self) -> Result<(), String>;

    /// 检测后端是否可用（不初始化）
    fn is_available(&self) -> bool;

    /// 分配 GPU 内存，返回设备地址
    fn alloc(&self, nbytes: usize) -> Result<u64, String>;

    /// 释放 GPU 内存
    fn free(&self, ptr: u64) -> Result<(), String>;

    /// Host → Device 拷贝
    fn h2d(&self, dst: u64, src: *const u8, nbytes: usize) -> Result<(), String>;

    /// Device → Host 拷贝
    fn d2h(&self, dst: *mut u8, src: u64, nbytes: usize) -> Result<(), String>;

    /// 加载内核模块
    /// - CUDA: data 是 PTX 文本 (UTF-8 bytes)
    /// - OpenCL: data 是 OpenCL C 源码 (UTF-8 bytes) 或 SPIR-V 二进制
    fn load_module(&self, data: &[u8]) -> Result<ModuleHandle, String>;

    /// 从已加载模块获取 kernel 函数句柄
    fn get_kernel(&self, module: &ModuleHandle, name: &str) -> Result<ModuleHandle, String>;

    /// 启动 kernel
    fn launch(
        &self,
        kernel: &ModuleHandle,
        config: LaunchConfig,
        args: &[u64],
    ) -> Result<(), String>;

    /// 同步等待上一批操作完成
    fn synchronize(&self) -> Result<(), String>;
}

/// 全局 GPU 后端检测 — 返回可用后端列表
pub fn detect_available_backends() -> Vec<&'static str> {
    let mut backends = Vec::new();

    // 检测 CUDA
    if crate::ffi::is_cuda_available() {
        backends.push("cuda");
    }

    // 检测 OpenCL
    if crate::opencl::ffi::is_opencl_available() {
        backends.push("opencl");
    }

    backends
}

/// 自动选择最佳后端
pub fn auto_select_backend() -> Option<&'static str> {
    detect_available_backends().first().copied()
}
