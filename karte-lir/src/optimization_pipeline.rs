use crate::pass::*;
use crate::pass::analysis::*;
use crate::pass::transformation::*;
use crate::pass::memory2reg::*;
use crate::pass::register_allocation::*;
use crate::{LirProgram, LirFunction};

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
#[derive(Debug, Clone)]
pub struct OptimizationConfig {
    /// 是否启用 Memory2Reg 优化
    pub enable_mem2reg: bool,
    /// 是否启用死代码消除
    pub enable_dce: bool,
    /// 是否启用常量折叠
    pub enable_const_fold: bool,
    /// 优化级别 (0-3)
    pub optimization_level: u8,
    /// 是否启用调试输出
    pub debug: bool,
}

impl Default for OptimizationConfig {
    fn default() -> Self {
        Self {
            enable_mem2reg: true,
            enable_dce: true,
            enable_const_fold: true,
            optimization_level: 2,
            debug: false,
        }
    }
}

/// 优化流水线
pub struct OptimizationPipeline {
    config: OptimizationConfig,
}

impl OptimizationPipeline {
    /// 创建新的优化流水线
    pub fn new(level: OptimizationLevel) -> Self {
        Self { config: level.to_config() }
    }
    
    /// 从配置创建优化流水线
    pub fn from_config(config: OptimizationConfig) -> Self {
        Self { config }
    }
    
    /// 创建默认的优化流水线
    pub fn default() -> Self {
        Self::new(OptimizationLevel::Balanced)
    }
    
    /// 运行优化
    pub fn optimize(&mut self, program: &mut LirProgram) -> Result<OptimizationStats, Vec<String>> {
        let mut pass_manager = PassManager::new();
        
        if self.config.debug {
            pass_manager = pass_manager.with_debug();
        }
        
        // 统计优化前的指令数
        let instructions_before = self.count_instructions(program);
        
        // 根据配置添加 Pass
        self.configure_passes(&mut pass_manager);
        
        // 运行优化
        let start_time = std::time::Instant::now();
        pass_manager.run_on_program(program).map_err(|e| vec![e])?;
        let total_time = start_time.elapsed();
        
        // 统计优化后的指令数
        let instructions_after = self.count_instructions(program);
        
        // 收集统计信息
        let stats = self.collect_stats(&pass_manager, total_time, instructions_before, instructions_after);
        
        Ok(stats)
    }
    
    /// 统计程序中的指令数量
    fn count_instructions(&self, program: &LirProgram) -> usize {
        program.functions.values()
            .map(|func| func.instructions.len())
            .sum()
    }
    
    /// 配置 Pass 序列
    fn configure_passes(&self, pass_manager: &mut PassManager) {
        // 添加分析 Pass
        pass_manager.add_analysis_pass(Box::new(ControlFlowAnalysis::new()));
        pass_manager.add_analysis_pass(Box::new(DefUseAnalysis::new()));
        
        // 根据优化级别和配置添加变换 Pass
        match self.config.optimization_level {
            0 => {
                // 无优化，只做基本分析和寄存器分配
                pass_manager.add_function_pass(Box::new(LinearScanRegisterAllocation::new()));
            }
            1 => {
                // 基本优化
                if self.config.enable_const_fold {
                    pass_manager.add_function_pass(Box::new(ConstantFolding::new()));
                }
                // 寄存器分配总是最后执行
                pass_manager.add_function_pass(Box::new(LinearScanRegisterAllocation::new()));
            }
            2 => {
                // 标准优化
                if self.config.enable_const_fold {
                    pass_manager.add_function_pass(Box::new(ConstantFolding::new()));
                }
                if self.config.enable_mem2reg {
                    pass_manager.add_function_pass(Box::new(Memory2RegPass::new()));
                }
                if self.config.enable_dce {
                    pass_manager.add_function_pass(Box::new(DeadCodeElimination::new()));
                }
                // 寄存器分配总是最后执行
                pass_manager.add_function_pass(Box::new(LinearScanRegisterAllocation::new()));
            }
            3 => {
                // 激进优化
                if self.config.enable_const_fold {
                    pass_manager.add_function_pass(Box::new(ConstantFolding::new()));
                }
                if self.config.enable_mem2reg {
                    pass_manager.add_function_pass(Box::new(Memory2RegPass::new()));
                }
                // 多轮优化
                if self.config.enable_const_fold {
                    pass_manager.add_function_pass(Box::new(ConstantFolding::new()));
                }
                if self.config.enable_dce {
                    pass_manager.add_function_pass(Box::new(DeadCodeElimination::new()));
                }
                // 再次运行 mem2reg，以处理新的机会
                if self.config.enable_mem2reg {
                    pass_manager.add_function_pass(Box::new(Memory2RegPass::new()));
                }
                // 寄存器分配总是最后执行
                pass_manager.add_function_pass(Box::new(LinearScanRegisterAllocation::new()));
            }
            _ => {
                // 使用级别 3 的配置
                self.configure_passes_for_level(pass_manager, 3);
            }
        }
    }
    
    /// 为指定级别配置 Pass
    fn configure_passes_for_level(&self, pass_manager: &mut PassManager, level: u8) {
        match level {
            3 => {
                if self.config.enable_const_fold {
                    pass_manager.add_function_pass(Box::new(ConstantFolding::new()));
                }
                if self.config.enable_mem2reg {
                    pass_manager.add_function_pass(Box::new(Memory2RegPass::new()));
                }
                if self.config.enable_const_fold {
                    pass_manager.add_function_pass(Box::new(ConstantFolding::new()));
                }
                if self.config.enable_dce {
                    pass_manager.add_function_pass(Box::new(DeadCodeElimination::new()));
                }
                if self.config.enable_mem2reg {
                    pass_manager.add_function_pass(Box::new(Memory2RegPass::new()));
                }
            }
            _ => {
                // 回退到级别 2
                self.configure_passes_for_level(pass_manager, 2);
            }
        }
    }
    
    /// 收集优化统计信息
    fn collect_stats(&self, pass_manager: &PassManager, total_time: std::time::Duration, instructions_before: usize, instructions_after: usize) -> OptimizationStats {
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
            enable_mem2reg: false,
            enable_dce: false,
            enable_const_fold: false,
            optimization_level: 0,
            debug: true,
        }
    }
    
    /// 快速配置：基本优化，编译速度优先
    pub fn fast() -> OptimizationConfig {
        OptimizationConfig {
            enable_mem2reg: true,
            enable_dce: false,
            enable_const_fold: true,
            optimization_level: 1,
            debug: false,
        }
    }
    
    /// 平衡配置：标准优化，平衡编译速度和执行效率
    pub fn balanced() -> OptimizationConfig {
        OptimizationConfig {
            enable_mem2reg: true,
            enable_dce: true,
            enable_const_fold: true,
            optimization_level: 2,
            debug: false,
        }
    }
    
    /// 高性能配置：激进优化，执行效率优先
    pub fn performance() -> OptimizationConfig {
        OptimizationConfig {
            enable_mem2reg: true,
            enable_dce: true,
            enable_const_fold: true,
            optimization_level: 3,
            debug: false,
        }
    }
} 