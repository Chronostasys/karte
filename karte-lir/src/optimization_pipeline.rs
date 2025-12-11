use crate::pass::pass_registry::PassRegistry;
use crate::pass::PipelinePreset;
use crate::pass::*;
use crate::LirProgram;

/// 优化级别
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptimizationLevel {
    /// 无优化，调试友好
    Debug,
    /// 快速优化，编译速度优先
    Fast,
    /// 平衡优化，速度和性能平衡
    Balanced,
    /// 高性能优化，执行效率优先
    Performance,
}

impl OptimizationLevel {
    pub fn to_config(self) -> OptimizationConfig {
        match self {
            OptimizationLevel::Debug => OptimizationPresets::debug(),
            OptimizationLevel::Fast => OptimizationPresets::fast(),
            OptimizationLevel::Balanced => OptimizationPresets::balanced(),
            OptimizationLevel::Performance => OptimizationPresets::performance(),
        }
    }
}

/// 优化流水线配置
///
/// 简化的配置接口，通过 optimization_level 控制优化程度
/// 具体的 Pass 序列由 PipelinePreset 定义
#[derive(Debug, Clone)]
pub struct OptimizationConfig {
    /// 优化级别 (0-3)
    /// - 0: Debug - 最小化优化
    /// - 1: Fast - 快速编译
    /// - 2: Balanced - 平衡优化
    /// - 3: Performance - 激进优化
    pub optimization_level: u8,
    /// 是否启用调试输出
    pub debug: bool,
}

impl Default for OptimizationConfig {
    fn default() -> Self {
        Self {
            optimization_level: 2, // Balanced 模式
            debug: false,
        }
    }
}

/// 优化流水线（按migration_to_ssa.md第5-6周计划重新设计）
pub struct OptimizationPipeline {
    config: OptimizationConfig,
}

impl OptimizationPipeline {
    /// 创建新的优化流水线
    pub fn new(level: OptimizationLevel) -> Self {
        Self {
            config: level.to_config(),
        }
    }

    /// 从配置创建优化流水线
    pub fn from_config(config: OptimizationConfig) -> Self {
        Self { config }
    }

    /// 创建默认的优化流水线
    pub fn default() -> Self {
        Self::new(OptimizationLevel::Balanced)
    }

    /// 列出所有可用的Pass
    pub fn list_available_passes() {
        let registry = PassRegistry::default();
        registry.print_available_passes();
    }

    /// 使用自定义Pass管线优化程序
    ///
    /// 格式: "pass1,pass2,pass3"
    /// 例如: "cfg,def-use,dce,const-fold,print-ir"
    ///
    /// 可用的Pass名称可以通过 list_available_passes() 查看
    pub fn optimize_with_custom_pipeline(
        program: &mut LirProgram,
        pipeline_str: &str,
        debug: bool,
    ) -> Result<OptimizationStats, Vec<String>> {
        let registry = PassRegistry::default();
        let mut pass_manager = registry
            .build_pipeline_from_string(pipeline_str)
            .map_err(|e| vec![e])?;

        if debug {
            println!("=== 使用自定义Pass管线 ===");
            pass_manager.print_pipeline();
        }

        let instructions_before = program
            .functions
            .values()
            .map(|func| func.instructions.len())
            .sum();

        let start_time = std::time::Instant::now();
        pass_manager.run_on_program(program).map_err(|e| vec![e])?;
        let total_time = start_time.elapsed();

        let instructions_after = program
            .functions
            .values()
            .map(|func| func.instructions.len())
            .sum();

        let pass_stats = pass_manager.get_statistics();
        let mut changed_passes = 0;
        let mut unchanged_passes = 0;
        let mut failed_passes = 0;
        let mut total_pass_time = 0u64;

        for stat in pass_stats {
            total_pass_time += stat.execution_time_ms;
            match stat.result {
                PassResult::Changed => changed_passes += 1,
                PassResult::Unchanged => unchanged_passes += 1,
                PassResult::Failed(_) => failed_passes += 1,
            }
        }

        Ok(OptimizationStats {
            total_time_ms: total_time.as_millis() as u64,
            pass_time_ms: total_pass_time,
            total_passes: pass_stats.len(),
            passes_executed: pass_stats.len(),
            changed_passes,
            unchanged_passes,
            failed_passes,
            optimization_level: 0, // 自定义管线没有固定的优化级别
            instructions_before,
            instructions_after,
        })
    }

    /// 运行优化
    pub fn optimize(&mut self, program: &mut LirProgram) -> Result<OptimizationStats, Vec<String>> {
        // 统计优化前的指令数
        let instructions_before = self.count_instructions(program);

        // 使用 PassRegistry 的强类型 API 构建预设 pipeline
        let registry = PassRegistry::default();
        let preset = self.optimization_level_to_preset();
        let mut pass_manager = registry.build_preset_pipeline(preset);

        if self.config.debug {
            pass_manager = pass_manager.with_debug();
            pass_manager.print_pipeline();
        }

        // 运行优化
        let start_time = std::time::Instant::now();
        pass_manager.run_on_program(program).map_err(|e| vec![e])?;
        let total_time = start_time.elapsed();

        // 统计优化后的指令数
        let instructions_after = self.count_instructions(program);

        // 收集统计信息
        let stats = self.collect_stats(
            &pass_manager,
            total_time,
            instructions_before,
            instructions_after,
        );

        Ok(stats)
    }

    /// 统计程序中的指令数量
    fn count_instructions(&self, program: &LirProgram) -> usize {
        program
            .functions
            .values()
            .map(|func| func.instructions.len())
            .sum()
    }

    /// 根据配置的优化级别选择 Pipeline 预设
    fn optimization_level_to_preset(&self) -> PipelinePreset {
        match self.config.optimization_level {
            0 => PipelinePreset::Debug,
            1 => PipelinePreset::Fast,
            2 => PipelinePreset::Balanced,
            3 => PipelinePreset::Performance,
            _ => PipelinePreset::Balanced, // 默认使用 Balanced
        }
    }

    /// 收集优化统计信息
    fn collect_stats(
        &self,
        pass_manager: &PassManager,
        total_time: std::time::Duration,
        instructions_before: usize,
        instructions_after: usize,
    ) -> OptimizationStats {
        let pass_stats = pass_manager.get_statistics();

        let mut changed_passes = 0;
        let mut unchanged_passes = 0;
        let mut failed_passes = 0;
        let mut total_pass_time = 0u64;

        for stat in pass_stats {
            total_pass_time += stat.execution_time_ms;
            match stat.result {
                PassResult::Changed => changed_passes += 1,
                PassResult::Unchanged => unchanged_passes += 1,
                PassResult::Failed(_) => failed_passes += 1,
            }
        }

        OptimizationStats {
            total_time_ms: total_time.as_millis() as u64,
            pass_time_ms: total_pass_time,
            total_passes: pass_stats.len(),
            passes_executed: pass_stats.len(),
            changed_passes,
            unchanged_passes,
            failed_passes,
            optimization_level: self.config.optimization_level,
            instructions_before,
            instructions_after,
        }
    }
}

/// 优化统计信息
#[derive(Debug, Clone)]
pub struct OptimizationStats {
    /// 总执行时间（毫秒）
    pub total_time_ms: u64,
    /// Pass 执行时间（毫秒）
    pub pass_time_ms: u64,
    /// 总 Pass 数量
    pub total_passes: usize,
    /// 执行的 Pass 数量
    pub passes_executed: usize,
    /// 产生变化的 Pass 数量
    pub changed_passes: usize,
    /// 未产生变化的 Pass 数量
    pub unchanged_passes: usize,
    /// 失败的 Pass 数量
    pub failed_passes: usize,
    /// 优化级别
    pub optimization_level: u8,
    /// 优化前指令数量
    pub instructions_before: usize,
    /// 优化后指令数量
    pub instructions_after: usize,
}

impl OptimizationStats {
    /// 打印统计信息
    pub fn print(&self) {
        println!("=== 优化统计 ===");
        println!("优化级别: {}", self.optimization_level);
        println!("总时间: {}ms", self.total_time_ms);
        println!("Pass 时间: {}ms", self.pass_time_ms);
        println!("总 Pass 数: {}", self.total_passes);
        println!("成功优化: {}", self.changed_passes);
        println!("无变化: {}", self.unchanged_passes);
        println!("失败: {}", self.failed_passes);

        if self.total_passes > 0 {
            let success_rate = (self.changed_passes as f32 / self.total_passes as f32) * 100.0;
            println!("优化成功率: {:.1}%", success_rate);
        }
    }

    /// 检查是否有失败的 Pass
    pub fn has_failures(&self) -> bool {
        self.failed_passes > 0
    }

    /// 检查是否有任何优化发生
    pub fn has_changes(&self) -> bool {
        self.changed_passes > 0
    }
}

/// 为特定用例预定义的优化配置
pub struct OptimizationPresets;

impl OptimizationPresets {
    /// 调试配置：无优化，保持代码原样
    pub fn debug() -> OptimizationConfig {
        OptimizationConfig {
            optimization_level: 0,
            debug: true,
        }
    }

    /// 快速配置：基本优化，编译速度优先
    pub fn fast() -> OptimizationConfig {
        OptimizationConfig {
            optimization_level: 1,
            debug: false,
        }
    }

    /// 平衡配置：标准优化，平衡编译速度和执行效率
    pub fn balanced() -> OptimizationConfig {
        OptimizationConfig {
            optimization_level: 2,
            debug: false,
        }
    }

    /// 高性能配置：激进优化，执行效率优先
    pub fn performance() -> OptimizationConfig {
        OptimizationConfig {
            optimization_level: 3,
            debug: false,
        }
    }
}
