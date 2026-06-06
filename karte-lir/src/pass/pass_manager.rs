use super::{
    analysis::{ControlFlowAnalysis, DefUseAnalysis, LivenessAnalysisPass},
    lifetime_analysis_pass::LifetimeAnalysisPass,
    AnalysisManager, AnalysisPass, FunctionPass, PassResult, PassStats, ProgramPass,
};
use crate::{Instruction, KarteError, LirFunction, LirProgram};
use log::{debug, info, trace};
use once_cell::sync::Lazy;
use std::time::Instant;

/// 全局注册的标准 Analysis Passes
///
/// 这些 Analysis Pass 在编译时全局注册一次，所有 PassManager 实例都会自动包含。
/// Analysis Pass 会根据 transformation passes 的 `required_analyses()` 自动按需执行。
///
/// 当前注册的 Analysis Passes：
/// - Control Flow Analysis (CFG)
/// - Definition-Use Analysis
/// - Liveness Analysis (活跃度分析)
/// - Lifetime Analysis (生命周期分析)
pub static STANDARD_ANALYSIS_PASSES: Lazy<
    Vec<Box<dyn Fn() -> Box<dyn AnalysisPass> + Send + Sync>>,
> = Lazy::new(|| {
    vec![
        Box::new(|| Box::new(ControlFlowAnalysis::new()) as Box<dyn AnalysisPass>),
        Box::new(|| Box::new(DefUseAnalysis::new()) as Box<dyn AnalysisPass>),
        Box::new(|| Box::new(LivenessAnalysisPass::new()) as Box<dyn AnalysisPass>),
        Box::new(|| Box::new(LifetimeAnalysisPass::new()) as Box<dyn AnalysisPass>),
    ]
});

/// Pass 管理器（按migration_to_ssa.md第5-6周计划增强）
///
/// 负责组织和执行各种优化 Pass，管理 Pass 之间的依赖关系
/// 新增功能：
/// - 改进的分析失效处理
/// - Pass依赖验证
/// - 更好的错误处理和统计
pub struct PassManager {
    /// 程序级别的 Pass 列表
    program_passes: Vec<Box<dyn ProgramPass>>,
    /// 函数级别的 Pass 列表
    function_passes: Vec<Box<dyn FunctionPass>>,
    /// 分析管理器
    analysis_manager: AnalysisManager,
    /// 执行统计
    stats: Vec<PassStats>,
    /// 是否启用调试输出
    debug: bool,
    /// 是否启用严格的依赖检查（新增）
    strict_dependency_check: bool,
    /// 是否验证分析失效处理（新增）
    validate_invalidation: bool,
}

impl PassManager {
    /// 创建新的 Pass 管理器
    pub fn new() -> Self {
        Self {
            program_passes: Vec::new(),
            function_passes: Vec::new(),
            analysis_manager: AnalysisManager::new(),
            stats: Vec::new(),
            debug: false,
            strict_dependency_check: true, // 默认启用严格依赖检查
            validate_invalidation: true,   // 默认启用失效验证
        }
    }

    /// 启用调试输出
    pub fn with_debug(mut self) -> Self {
        self.debug = true;
        self
    }

    /// 启用严格的依赖检查（按migration计划新增）
    pub fn with_strict_dependency_check(mut self, enable: bool) -> Self {
        self.strict_dependency_check = enable;
        self
    }

    /// 启用分析失效验证（按migration计划新增）
    pub fn with_invalidation_validation(mut self, enable: bool) -> Self {
        self.validate_invalidation = enable;
        self
    }

    /// 创建专业的Pass管理器（按migration计划）
    pub fn create_professional() -> Self {
        Self::new()
            .with_strict_dependency_check(true)
            .with_invalidation_validation(true)
    }

    /// 添加程序级别的 Pass
    pub fn add_program_pass(&mut self, pass: Box<dyn ProgramPass>) {
        self.program_passes.push(pass);
    }

    /// 添加函数级别的 Pass
    pub fn add_function_pass(&mut self, pass: Box<dyn FunctionPass>) {
        self.function_passes.push(pass);
    }

    /// 批量添加函数级别的 Pass
    ///
    /// # 示例
    /// ```
    /// use karte_lir::pass::*;
    ///
    /// let mut manager = PassManager::new();
    /// manager.add_function_passes(vec![
    ///     Box::new(ConstantFolding::new()),
    ///     Box::new(DeadCodeElimination::new()),
    ///     Box::new(PeepholeOptimizer::new()),
    /// ]);
    /// ```
    pub fn add_function_passes(&mut self, passes: Vec<Box<dyn FunctionPass>>) {
        self.function_passes.extend(passes);
    }

    /// 在程序上运行所有 Pass（增强版本）
    pub fn run_on_program(&mut self, program: &mut LirProgram) -> crate::Result<()> {
        self.stats.clear();

        // 注入目标架构的调用约定到 AnalysisManager
        // 各 pass 可以通过 get_calling_convention() 获取正确的 CC
        let cc = program.calling_convention();
        self.analysis_manager.store_calling_convention(cc);

        // 统计总函数数和总指令数
        let total_funcs = program.functions.len();
        let total_instrs: usize = program.functions.values().map(|f| f.instructions.len()).sum();
        eprintln!("[LIR PIPELINE] {} 个函数, 共 {} 条指令", total_funcs, total_instrs);

        if self.debug {
            info!("=== 专业Pass管理器: 开始执行Pass序列 ===");
            info!("程序信息: {} 个函数", program.functions.len());
            info!("目标架构: {}", program.target());
            info!("严格依赖检查: {}", self.strict_dependency_check);
            info!("失效验证: {}", self.validate_invalidation);
        }

        // 1. 运行程序级别的 Pass
        self.run_program_passes(program)?;

        // 2. 为每个函数运行分析和函数级别的 Pass
        let target_arch = program.target().to_string();
        // 确定性排序：按函数名排序，确保编译结果可复现
        let mut func_names: Vec<String> = program.functions.keys().cloned().collect();
        func_names.sort();
        for func_name in &func_names {
            let function = program.functions.get_mut(func_name).unwrap();
            let instr_count = function.instructions.len();
            let func_start = std::time::Instant::now();
            eprintln!("[LIR FUNC] {}: {} 条指令", func_name, instr_count);
            if self.debug {
                info!("处理函数: {}", func_name);
            }

            // 将程序级目标架构传播到函数级（供 get_calling_convention() 使用）
            function.target_arch = if target_arch.is_empty() {
                None
            } else {
                Some(target_arch.clone())
            };

            // 运行分析 Pass
            self.run_analysis_passes_on_function(function)?;

            // 运行函数级别的 Pass
            self.run_function_passes_on_function(function)?;

            // 清理函数级分析结果（每个函数处理完后清理）
            if self.validate_invalidation {
                self.cleanup_function_analyses();
            }
            let elapsed = func_start.elapsed();
            if instr_count > 100 || elapsed.as_millis() > 100 {
                eprintln!("[LIR FUNC] {} DONE: {} 条指令, {:.2}s", func_name, instr_count, elapsed.as_secs_f64());
            }
        }

        if self.debug {
            self.print_statistics();
            info!("=== Pass序列执行完成 ===");
        }

        Ok(())
    }

    /// 运行程序级别的Pass（新增方法）
    fn run_program_passes(&mut self, program: &mut LirProgram) -> crate::Result<()> {
        let mut i = 0;
        while i < self.program_passes.len() {
            let start_time = Instant::now();

            // 依赖检查
            let required_analyses = self.program_passes[i].required_analyses();
            for analysis_name in required_analyses {
                if !self.analysis_manager.results.contains_key(analysis_name) {
                    return Err(format!("找不到所需的分析: {}", analysis_name).into());
                }
            }

            if self.debug {
                info!("  执行程序Pass: {}", self.program_passes[i].name());
            }

            let result = self.program_passes[i].run_on_program(program, &mut self.analysis_manager);
            let invalidated = self.program_passes[i].invalidated_analyses();

            // 分析失效处理
            if self.validate_invalidation && self.debug && !invalidated.is_empty() {
                debug!("    失效分析: {:?}", invalidated);
            }
            self.analysis_manager.invalidate_all(&invalidated);

            let execution_time = start_time.elapsed().as_millis() as u64;

            // 记录统计信息
            let mut stats = PassStats::new(self.program_passes[i].name().to_string());
            stats.execution_time_ms = execution_time;
            stats.result = result.clone();
            stats.functions_processed = program.functions.len();
            self.stats.push(stats);

            if let PassResult::Failed(msg) = result {
                return Err(format!(
                    "程序Pass {} 失败: {}",
                    self.program_passes[i].name(),
                    msg
                ).into());
            }

            if self.debug {
                info!(
                    "    程序Pass {} 完成 ({}ms)",
                    self.program_passes[i].name(),
                    execution_time
                );
            }

            i += 1;
        }
        Ok(())
    }

    /// 递归运行分析 Pass 及其所有依赖
    ///
    /// 该方法确保在运行目标分析之前，先运行其所有依赖的分析
    fn run_analysis_with_dependencies(
        &mut self,
        function: &LirFunction,
        analysis_name: &str,
    ) -> crate::Result<()> {
        // 如果已经运行过，直接返回
        if self.analysis_manager.results.contains_key(analysis_name) {
            return Ok(());
        }

        // 从全局注册表查找对应的 analysis pass
        let analysis_pass_opt = STANDARD_ANALYSIS_PASSES.iter().find_map(|constructor| {
            let pass = constructor();
            if pass.name() == analysis_name {
                Some(pass)
            } else {
                None
            }
        });

        let mut analysis_pass =
            analysis_pass_opt.ok_or_else(|| KarteError::from(format!("找不到所需的分析: {}", analysis_name)))?;

        if self.debug {
            info!("    (按需运行分析: {})", analysis_name);
        }

        // 🔧 递归运行所有依赖的分析
        let required = analysis_pass.required_analyses();
        for dep_analysis_name in required {
            if !self
                .analysis_manager
                .results
                .contains_key(dep_analysis_name)
            {
                self.run_analysis_with_dependencies(function, dep_analysis_name)?;
            }
        }

        // 运行当前分析
        let result = analysis_pass
            .analyze_function(function, &self.analysis_manager)
            .map_err(|msg| KarteError::from(format!("分析 Pass {} 失败: {}", analysis_name, msg)))?;

        self.analysis_manager
            .store_result(analysis_name.to_string(), result);

        Ok(())
    }

    /// 为函数运行分析 Pass（从全局注册表读取）
    fn run_analysis_passes_on_function(&mut self, function: &LirFunction) -> crate::Result<()> {
        // 🔧 优化：递归运行所有必需的分析Pass
        // 按照全局注册表的顺序运行所有分析，自动处理依赖关系
        let analysis_pass_count = STANDARD_ANALYSIS_PASSES.len();

        for i in 0..analysis_pass_count {
            let mut analysis_pass = STANDARD_ANALYSIS_PASSES[i]();
            let pass_name = analysis_pass.name().to_string();

            // 如果已经运行过，跳过
            if self.analysis_manager.results.contains_key(&pass_name) {
                continue;
            }

            let start_time = Instant::now();

            if self.debug {
                info!("    执行分析 Pass: {}", pass_name);
            }

            // 🔧 递归运行所需的依赖分析
            let required = analysis_pass.required_analyses();
            for analysis_name in required {
                if !self.analysis_manager.results.contains_key(analysis_name) {
                    // 从全局注册表中查找并运行依赖的分析
                    let dep_pass_opt = STANDARD_ANALYSIS_PASSES.iter().find_map(|constructor| {
                        let pass = constructor();
                        if pass.name() == analysis_name {
                            Some(pass)
                        } else {
                            None
                        }
                    });

                    if let Some(mut dep_pass) = dep_pass_opt {
                        // 递归运行依赖的分析（注意：这里简化处理，假设不会有循环依赖）
                        let dep_result = dep_pass
                            .analyze_function(function, &self.analysis_manager)
                            .map_err(|msg| {
                                KarteError::from(format!("依赖分析 Pass {} 失败: {}", analysis_name, msg))
                            })?;
                        self.analysis_manager
                            .store_result(analysis_name.to_string(), dep_result);

                        if self.debug {
                            info!("    (自动运行依赖分析: {})", analysis_name);
                        }
                    } else {
                        return Err(format!("找不到所需的分析: {}", analysis_name).into());
                    }
                }
            }

            // 运行分析
            match analysis_pass.analyze_function(function, &self.analysis_manager) {
                Ok(result) => {
                    self.analysis_manager
                        .store_result(pass_name.clone(), result);
                }
                Err(msg) => {
                    return Err(format!("分析 Pass {} 失败: {}", pass_name, msg).into());
                }
            }

            let execution_time = start_time.elapsed().as_millis() as u64;

            if self.debug {
                info!("    分析 Pass {} 完成 ({}ms)", pass_name, execution_time);
            }
        }

        Ok(())
    }

    /// 为函数运行函数级别的 Pass
    fn run_function_passes_on_function(
        &mut self,
        function: &mut LirFunction,
    ) -> crate::Result<()> {
        let func_name = function.name.clone();
        let mut i = 0;
        while i < self.function_passes.len() {
            info!(
                "run_function_passes_on_function, passname: {}",
                self.function_passes[i].name()
            );
            let start_time = Instant::now();

            if self.debug {
                info!("    执行函数 Pass: {}", self.function_passes[i].name());
            }

            // 自动补全所需分析（从全局注册表读取）
            let required = self.function_passes[i].required_analyses();
            for analysis_name in required {
                if !self.analysis_manager.results.contains_key(analysis_name) {
                    // 🔧 使用递归方法运行分析及其依赖
                    self.run_analysis_with_dependencies(function, analysis_name)?;
                }
            }

            // 运行 Pass
            let result =
                self.function_passes[i].run_on_function(function, &mut self.analysis_manager);

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
                    return Err(format!(
                        "函数 Pass {} 失败: {}",
                        self.function_passes[i].name(),
                        msg
                    ).into());
                }
                _ => {
                    info!(
                        "    函数 Pass {} 完成 ({}ms)",
                        self.function_passes[i].name(),
                        execution_time
                    );
                    debug!("    优化后lir: {:?}", function);
                }
            }

            trace!("lir after pass:\n{}", function);

            i += 1;
        }

        Ok(())
    }

    /// 清理函数级分析结果（新增方法）
    fn cleanup_function_analyses(&mut self) {
        // 清理函数特定的分析结果，保留程序级分析
        let function_analyses = ["def-use", "cfg", "liveness"];
        self.analysis_manager.invalidate_all(&function_analyses);

        if self.debug {
            info!("    清理函数级分析结果");
        }
    }

    /// 打印执行统计
    fn print_statistics(&self) {
        info!("=== Pass 执行统计 ===");

        let mut total_time = 0u64;
        let mut changed_count = 0;
        let mut unchanged_count = 0;
        let mut failed_count = 0;

        for stat in &self.stats {
            info!(
                "  {}: {}ms, {:?}",
                stat.name, stat.execution_time_ms, stat.result
            );
            total_time += stat.execution_time_ms;

            match stat.result {
                PassResult::Changed => changed_count += 1,
                PassResult::Unchanged => unchanged_count += 1,
                PassResult::Failed(_) => failed_count += 1,
            }
        }

        info!(
            "总计: {}ms, {} 个 Pass (改变: {}, 未改变: {}, 失败: {})",
            total_time,
            self.stats.len(),
            changed_count,
            unchanged_count,
            failed_count
        );
    }

    /// 保存生命周期分析结果到函数中
    ///
    /// 这个方法在所有优化Pass完成后调用，将生命周期分析和寄存器分配结果
    /// 保存到LirFunction中，供JIT编译器使用
    fn save_lifetime_analysis_to_function(&self, function: &mut LirFunction) {
        use crate::pass::lifetime_analysis_pass::LifetimeAnalysisResult;
        use crate::pass::register_allocation::RegisterAllocationResult;

        // 尝试获取生命周期分析结果
        if let Some(lifetime_result) = self
            .analysis_manager
            .get_result::<LifetimeAnalysisResult>("lifetime-analysis")
        {
            function.lowered_lifetimes = Some(lifetime_result.lifetimes.clone());

            info!(
                "💾 已保存生命周期分析结果到函数 {}: {} 个寄存器",
                function.name,
                lifetime_result.lifetimes.len()
            );
        } else {
            info!("⚠️  函数 {} 没有生命周期分析结果", function.name);
        }

        // 尝试获取寄存器分配结果
        if let Some(alloc_result) = self
            .analysis_manager
            .get_result::<RegisterAllocationResult>("register-allocation")
        {
            function.lowered_register_mapping = Some(alloc_result.register_mapping.clone());

            info!(
                "💾 已保存寄存器映射到函数 {}: {} 个映射",
                function.name,
                alloc_result.register_mapping.len()
            );
        } else {
            info!("⚠️  函数 {} 没有寄存器分配结果", function.name);
        }
    }

    /// 获取执行统计
    pub fn get_statistics(&self) -> &[PassStats] {
        &self.stats
    }

    /// 清空所有 Pass
    pub fn clear(&mut self) {
        self.program_passes.clear();
        self.function_passes.clear();
        self.analysis_manager.clear();
        self.stats.clear();
    }

    /// 打印当前管道中的所有 Pass
    pub fn print_pipeline(&self) {
        println!("=== Pass 管道 ===");

        if !self.program_passes.is_empty() {
            println!("\n程序级别 Pass:");
            for (i, pass) in self.program_passes.iter().enumerate() {
                println!("  {}. {}", i + 1, pass.name());
            }
        }

        // 打印全局注册的分析 Passes
        println!("\n分析 Pass (全局注册):");
        for (i, constructor) in STANDARD_ANALYSIS_PASSES.iter().enumerate() {
            let pass = constructor();
            println!("  {}. {}", i + 1, pass.name());
        }

        if !self.function_passes.is_empty() {
            println!("\n函数级别 Pass:");
            for (i, pass) in self.function_passes.iter().enumerate() {
                println!("  {}. {}", i + 1, pass.name());
            }
        }

        let total_passes =
            self.program_passes.len() + STANDARD_ANALYSIS_PASSES.len() + self.function_passes.len();
        println!("\n总计: {} 个 Pass", total_passes);
        println!("===================\n");
    }

    /// 获取管道中的 Pass 数量
    pub fn pass_count(&self) -> usize {
        self.program_passes.len() + STANDARD_ANALYSIS_PASSES.len() + self.function_passes.len()
    }
}

impl Default for PassManager {
    fn default() -> Self {
        Self::new()
    }
}
