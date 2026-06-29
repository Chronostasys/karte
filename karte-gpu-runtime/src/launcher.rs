//! Kernel 启动器
//!
//! 负责 PTX 模块加载和 kernel 启动。

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
}

/// Kernel 启动器
pub struct KernelLauncher {
    /// PTX 源代码
    pub ptx_source: String,
    /// Kernel 名称
    pub kernel_name: String,
}

impl KernelLauncher {
    /// 从 PTX 文本创建启动器
    pub fn from_ptx(ptx: &str, kernel_name: &str) -> Self {
        Self {
            ptx_source: ptx.to_string(),
            kernel_name: kernel_name.to_string(),
        }
    }

    /// 启动 kernel（需要 GPU 环境）
    pub fn launch(&self, _config: LaunchConfig, _args: &[u64]) -> Result<(), String> {
        // 在实际实现中:
        // 1. cuModuleLoadData(&module, ptx_source.as_ptr())
        // 2. cuModuleGetFunction(&func, module, kernel_name)
        // 3. cuLaunchKernel(func, grid, block, shared_mem, stream, args, extra)
        // 4. cuCtxSynchronize()

        Err("GPU 不可用 — 请使用 CPU 回退模式".to_string())
    }

    /// CPU 回退执行（模拟 kernel 执行）
    pub fn launch_cpu_fallback<F>(&self, config: LaunchConfig, mut executor: F)
    where
        F: FnMut(usize, usize), // (global_thread_id, block_id)
    {
        let total_threads = config.grid.0 * config.block.0;
        for tid in 0..total_threads {
            let block_id = tid / config.block.0;
            executor(tid, block_id);
        }
    }
}
