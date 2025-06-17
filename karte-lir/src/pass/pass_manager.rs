use super::{FunctionPass, ProgramPass, AnalysisPass, AnalysisManager, PassResult, PassStats, AnalysisResult};
use crate::{LirProgram, LirFunction};
use std::time::Instant;

/// Pass 管理器
/// 
/// 负责组织和执行各种优化 Pass，管理 Pass 之间的依赖关系
pub struct PassManager {
    /// 程序级别的 Pass 列表
    program_passes: Vec<Box<dyn ProgramPass>>,
    /// 函数级别的 Pass 列表
    function_passes: Vec<Box<dyn FunctionPass>>,
    /// 分析 Pass 列表
    analysis_passes: Vec<Box<dyn AnalysisPass>>,
    /// 分析管理器
    analysis_manager: AnalysisManager,
    /// 执行统计
    stats: Vec<PassStats>,
    /// 是否启用调试输出
    debug: bool,
}

impl PassManager {
    /// 创建新的 Pass 管理器
    pub fn new() -> Self {
        Self {
            program_passes: Vec::new(),
            function_passes: Vec::new(),
            analysis_passes: Vec::new(),
            analysis_manager: AnalysisManager::new(),
            stats: Vec::new(),
            debug: false,
        }
    }
    
    /// 启用调试输出
    pub fn with_debug(mut self) -> Self {
        self.debug = true;
        self
    }
    
    /// 添加程序级别的 Pass
    pub fn add_program_pass(&mut self, pass: Box<dyn ProgramPass>) {
        self.program_passes.push(pass);
    }
    
    /// 添加函数级别的 Pass
    pub fn add_function_pass(&mut self, pass: Box<dyn FunctionPass>) {
        self.function_passes.push(pass);
    }
    
    /// 添加分析 Pass
    pub fn add_analysis_pass(&mut self, pass: Box<dyn AnalysisPass>) {
        self.analysis_passes.push(pass);
    }
    
    /// 在程序上运行所有 Pass
    pub fn run_on_program(&mut self, program: &mut LirProgram) -> Result<(), String> {
        self.stats.clear();
        
        if self.debug {
            println!("=== Pass Manager: 开始执行 Pass 序列 ===");
            println!("程序信息: {} 个函数", program.functions.len());
        }
        
        // 1. 运行程序级别的 Pass
        let mut i = 0;
        while i < self.program_passes.len() {
            // 简化的依赖检查
            let required_analyses = self.program_passes[i].required_analyses();
            for analysis_name in required_analyses {
                if !self.analysis_manager.results.contains_key(analysis_name) {
                    return Err(format!("找不到所需的分析: {}", analysis_name));
                }
            }
            
            let result = self.program_passes[i].run_on_program(program, &mut self.analysis_manager);
            let invalidated = self.program_passes[i].invalidated_analyses();
            self.analysis_manager.invalidate_all(&invalidated);
            
            // 记录统计信息
            let mut stats = PassStats::new(self.program_passes[i].name().to_string());
            stats.result = result.clone();
            stats.functions_processed = program.functions.len();
            self.stats.push(stats);
            
            match result {
                PassResult::Failed(msg) => return Err(format!("Pass {} 失败: {}", self.program_passes[i].name(), msg)),
                _ => {}
            }
            
            i += 1;
        }
        
        // 2. 为每个函数运行分析和函数级别的 Pass
        for (func_name, function) in program.functions.iter_mut() {
            if self.debug {
                println!("处理函数: {}", func_name);
            }
            
            // 运行分析 Pass
            self.run_analysis_passes_on_function(function)?;
            
            // 运行函数级别的 Pass
            self.run_function_passes_on_function(function)?;
        }
        
        if self.debug {
            self.print_statistics();
        }
        
        Ok(())
    }
    

    
    /// 为函数运行分析 Pass
    fn run_analysis_passes_on_function(&mut self, function: &LirFunction) -> Result<(), String> {
        let mut i = 0;
        while i < self.analysis_passes.len() {
            let start_time = Instant::now();
            
            if self.debug {
                println!("    执行分析 Pass: {}", self.analysis_passes[i].name());
            }
            
            // 检查依赖（简化版本，避免借用冲突）
            let required = self.analysis_passes[i].required_analyses();
            for analysis_name in required {
                if !self.analysis_manager.results.contains_key(analysis_name) {
                    return Err(format!("找不到所需的分析: {}", analysis_name));
                }
            }
            
            // 运行分析
            match self.analysis_passes[i].analyze_function(function, &self.analysis_manager) {
                Ok(result) => {
                    self.analysis_manager.store_result(self.analysis_passes[i].name().to_string(), result);
                }
                Err(msg) => {
                    return Err(format!("分析 Pass {} 失败: {}", self.analysis_passes[i].name(), msg));
                }
            }
            
            let execution_time = start_time.elapsed().as_millis() as u64;
            
            if self.debug {
                println!("    分析 Pass {} 完成 ({}ms)", self.analysis_passes[i].name(), execution_time);
            }
            
            i += 1;
        }
        
        Ok(())
    }
    
    /// 为函数运行函数级别的 Pass
    fn run_function_passes_on_function(&mut self, function: &mut LirFunction) -> Result<(), String> {
        let mut i = 0;
        while i < self.function_passes.len() {
            let start_time = Instant::now();
            
            if self.debug {
                println!("    执行函数 Pass: {}", self.function_passes[i].name());
            }
            
            // 检查依赖（简化版本）
            let required = self.function_passes[i].required_analyses();
            for analysis_name in required {
                if !self.analysis_manager.results.contains_key(analysis_name) {
                    return Err(format!("找不到所需的分析: {}", analysis_name));
                }
            }
            
            // 运行 Pass
            let result = self.function_passes[i].run_on_function(function, &mut self.analysis_manager);
            
            // 处理失效的分析
            let invalidated = self.function_passes[i].invalidated_analyses();
            self.analysis_manager.invalidate_all(&invalidated);
            
            let execution_time = start_time.elapsed().as_millis() as u64;
            
            // 记录统计信息
            let mut stats = PassStats::new(self.function_passes[i].name().to_string());
            stats.execution_time_ms = execution_time;
            stats.result = result.clone();
            stats.functions_processed = 1;
            self.stats.push(stats);
            
            match result {
                PassResult::Failed(msg) => {
                    return Err(format!("函数 Pass {} 失败: {}", self.function_passes[i].name(), msg));
                }
                _ => {}
            }
            
            i += 1;
        }
        
        Ok(())
    }
    

    
    /// 打印执行统计
    fn print_statistics(&self) {
        println!("=== Pass 执行统计 ===");
        
        let mut total_time = 0u64;
        let mut changed_count = 0;
        let mut unchanged_count = 0;
        let mut failed_count = 0;
        
        for stat in &self.stats {
            println!("  {}: {}ms, {:?}", stat.name, stat.execution_time_ms, stat.result);
            total_time += stat.execution_time_ms;
            
            match stat.result {
                PassResult::Changed => changed_count += 1,
                PassResult::Unchanged => unchanged_count += 1,
                PassResult::Failed(_) => failed_count += 1,
            }
        }
        
        println!("总计: {}ms, {} 个 Pass (改变: {}, 未改变: {}, 失败: {})", 
                total_time, self.stats.len(), changed_count, unchanged_count, failed_count);
    }
    
    /// 获取执行统计
    pub fn get_statistics(&self) -> &[PassStats] {
        &self.stats
    }
    
    /// 清空所有 Pass
    pub fn clear(&mut self) {
        self.program_passes.clear();
        self.function_passes.clear();
        self.analysis_passes.clear();
        self.analysis_manager.clear();
        self.stats.clear();
    }
}

impl Default for PassManager {
    fn default() -> Self {
        Self::new()
    }
} 