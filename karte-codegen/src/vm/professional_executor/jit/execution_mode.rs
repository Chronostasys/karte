//! 执行模式定义
//!
//! 定义了JIT编译器的不同执行策略

/// 执行模式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExecutionMode {
    /// 纯解释器模式 - 所有代码都通过解释器执行
    Interpreter,

    /// 纯JIT模式 - 所有代码都通过JIT编译后执行
    JitOnly,

    /// 混合模式 - 根据启发式算法决定使用解释器还是JIT
    /// 这是默认模式，平衡编译开销和执行性能
    #[default]
    Hybrid,
}

impl ExecutionMode {
    /// 从字符串解析执行模式
    pub fn from_str(s: &str) -> Result<Self, String> {
        match s.to_lowercase().as_str() {
            "interpreter" | "解释器" => Ok(ExecutionMode::Interpreter),
            "jit" | "jit-only" => Ok(ExecutionMode::JitOnly),
            "hybrid" | "混合" => Ok(ExecutionMode::Hybrid),
            _ => Err(format!("未知的执行模式: {}", s)),
        }
    }

    /// 转换为字符串
    pub fn as_str(&self) -> &'static str {
        match self {
            ExecutionMode::Interpreter => "interpreter",
            ExecutionMode::JitOnly => "jit-only",
            ExecutionMode::Hybrid => "hybrid",
        }
    }

    /// 获取中文描述
    pub fn description(&self) -> &'static str {
        match self {
            ExecutionMode::Interpreter => "纯解释器模式",
            ExecutionMode::JitOnly => "纯JIT模式",
            ExecutionMode::Hybrid => "混合模式",
        }
    }
}

/// JIT编译策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompilationStrategy {
    /// 立即编译 - 函数第一次被调用时立即编译
    Eager,

    /// 延迟编译 - 函数被多次调用后才编译
    #[default]
    Lazy,

    /// 基于热点的编译 - 根据函数调用频率决定是否编译
    HotSpot,
}

/// JIT配置
#[derive(Debug, Clone)]
pub struct JitConfig {
    /// 执行模式
    pub execution_mode: ExecutionMode,

    /// 编译策略
    pub compilation_strategy: CompilationStrategy,

    /// 热点阈值 - 函数被调用多少次后认为是热点
    pub hotspot_threshold: u32,

    /// 是否启用优化
    pub enable_optimizations: bool,

    /// 是否生成调试信息
    pub generate_debug_info: bool,

    /// 代码缓存大小限制（字节）
    pub code_cache_size_limit: usize,

    /// 是否启用内联
    pub enable_inlining: bool,

    /// 内联大小阈值
    pub inline_size_threshold: usize,
}

impl Default for JitConfig {
    fn default() -> Self {
        Self {
            execution_mode: ExecutionMode::default(),
            compilation_strategy: CompilationStrategy::default(),
            hotspot_threshold: 10, // 函数被调用10次后编译
            enable_optimizations: true,
            generate_debug_info: false,
            code_cache_size_limit: 64 * 1024 * 1024, // 64MB
            enable_inlining: false,                  // 暂时禁用内联
            inline_size_threshold: 100,              // 100字节以下的函数可以内联
        }
    }
}

impl JitConfig {
    /// 创建调试配置
    pub fn debug() -> Self {
        Self {
            execution_mode: ExecutionMode::Hybrid,
            compilation_strategy: CompilationStrategy::Eager,
            hotspot_threshold: 1,        // 立即编译
            enable_optimizations: false, // 禁用优化以便调试
            generate_debug_info: true,
            code_cache_size_limit: 16 * 1024 * 1024, // 16MB
            enable_inlining: false,
            inline_size_threshold: 50,
        }
    }

    /// 创建性能配置
    pub fn performance() -> Self {
        Self {
            execution_mode: ExecutionMode::JitOnly,
            compilation_strategy: CompilationStrategy::HotSpot,
            hotspot_threshold: 5,
            enable_optimizations: true,
            generate_debug_info: false,
            code_cache_size_limit: 128 * 1024 * 1024, // 128MB
            enable_inlining: true,
            inline_size_threshold: 200,
        }
    }

    /// 创建保守配置（适合内存受限环境）
    pub fn conservative() -> Self {
        Self {
            execution_mode: ExecutionMode::Hybrid,
            compilation_strategy: CompilationStrategy::Lazy,
            hotspot_threshold: 50, // 高阈值
            enable_optimizations: false,
            generate_debug_info: false,
            code_cache_size_limit: 8 * 1024 * 1024, // 8MB
            enable_inlining: false,
            inline_size_threshold: 50,
        }
    }
}
