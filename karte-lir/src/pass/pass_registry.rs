use super::*;
use std::collections::HashMap;

/// Pass 构造器类型
pub type FunctionPassConstructor = Box<dyn Fn() -> Box<dyn FunctionPass> + Send + Sync>;
pub type AnalysisPassConstructor = Box<dyn Fn() -> Box<dyn AnalysisPass> + Send + Sync>;

/// Pipeline 预设类型
///
/// 提供四个标准的优化 pipeline 配置：
/// - Debug: 最小化优化，适合调试
/// - Fast: 快速编译，基本优化
/// - Balanced: 平衡编译速度和执行性能
/// - Performance: 激进优化，最佳执行性能
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelinePreset {
    /// Debug 模式：最小化优化，保持调试友好
    Debug,
    /// Fast 模式：快速编译，基本优化
    Fast,
    /// Balanced 模式：平衡的优化
    Balanced,
    /// Performance 模式：激进优化
    Performance,
}

/// Pass 注册表 - 管理所有可用的 Pass
///
/// 提供以下功能:
/// - 注册各种 Pass 的构造函数
/// - 通过字符串名称构建 Pass 实例
/// - 列出所有可用的 Pass
///
/// 注意: Pass 的名称和描述来自 Pass 实例的 name() 和 description() 方法，
/// 而不是硬编码在注册表中，确保一致性。
pub struct PassRegistry {
    /// 函数级别 Pass 的构造器
    function_passes: HashMap<String, FunctionPassConstructor>,
}

impl PassRegistry {
    /// 获取全局共享的 PassRegistry 实例（懒加载，只初始化一次）
    pub fn global() -> &'static Self {
        use std::sync::OnceLock;
        static INSTANCE: OnceLock<PassRegistry> = OnceLock::new();
        INSTANCE.get_or_init(|| Self::with_standard_passes())
    }

    /// 创建新的 Pass 注册表（用于测试或自定义场景）
    pub fn new() -> Self {
        Self {
            function_passes: HashMap::new(),
        }
    }

    /// 创建默认的 Pass 注册表，预注册 Balanced 模式的所有 transformation passes
    ///
    /// 此方法用于支持字符串 pipeline 构建（例如 `build_pipeline_from_string("dce,const-fold")`）。
    /// 注册的 Pass 与 `build_balanced_pipeline()` 中使用的 transformation passes 一致。
    ///
    /// Analysis Passes 从全局注册表自动加载，不需要在此注册。
    ///
    /// 如果使用强类型 API（`build_*_pipeline()`），则不需要调用此方法。
    pub fn with_standard_passes() -> Self {
        let mut registry = Self::new();

        // === 注册 Balanced Pipeline 使用的所有 transformation passes ===

        // 降级和优化
        registry.register_function_pass(Box::new(|| Box::new(EffectLoweringPass::new())));
        registry.register_function_pass(Box::new(|| Box::new(ConstantFolding::new())));

        // SSA 和内存优化
        registry.register_function_pass(Box::new(|| Box::new(SsaConstructionPass::new())));
        registry.register_function_pass(Box::new(|| Box::new(Memory2RegPass::new())));
        registry.register_function_pass(Box::new(|| Box::new(PhiEliminationPass::new())));

        // Codegen前准备 - 显式化跳转和基本块布局优化（必须在寄存器分配前）
        registry.register_function_pass(Box::new(|| Box::new(ExplicitJumpPass::new())));
        registry.register_function_pass(Box::new(|| Box::new(BlockLayoutPass::new())));

        // Codegen
        registry
            .register_function_pass(Box::new(|| Box::new(LinearScanRegisterAllocation::new())));
        registry.register_function_pass(Box::new(|| {
            Box::new(crate::pass::stack_frame_layout::StackFrameLayoutPass::new())
        }));

        // 后期优化
        registry.register_function_pass(Box::new(|| Box::new(PeepholeOptimizer::new())));
        registry.register_function_pass(Box::new(|| Box::new(DeadCodeElimination::new())));

        // 最终降级
        registry.register_function_pass(Box::new(|| Box::new(InstructionLoweringPass::new())));

        // StorePair/LoadPair �窺孔优化（在指令降级之后）
        // 🔧 暂时禁用，因为16字节对齐问题
        // registry.register_function_pass(Box::new(|| Box::new(PeepholeOptimizationPass::new(16))));

        // === 注册额外的工具 Pass（用于字符串 pipeline 的灵活性）===
        registry.register_function_pass(Box::new(|| Box::new(PrintIRPass::new())));
        registry.register_function_pass(Box::new(|| Box::new(VerifyPass::new())));
        registry.register_function_pass(Box::new(|| Box::new(StatisticsPass::new())));

        registry
    }

    /// 注册函数级别 Pass
    ///
    /// 注意: Pass 的名称自动从构造器创建的实例获取，无需手动指定
    pub fn register_function_pass(&mut self, constructor: FunctionPassConstructor) {
        // 创建临时实例获取名称
        let pass = constructor();
        let name = pass.name().to_string();
        self.function_passes.insert(name, constructor);
    }

    /// 通过名称创建函数 Pass
    pub fn create_function_pass(&self, name: &str) -> Option<Box<dyn FunctionPass>> {
        self.function_passes.get(name).map(|ctor| ctor())
    }

    /// 通过名称创建分析 Pass（从全局注册表查找）
    pub fn create_analysis_pass(&self, name: &str) -> Option<Box<dyn AnalysisPass>> {
        use crate::pass::pass_manager::STANDARD_ANALYSIS_PASSES;
        STANDARD_ANALYSIS_PASSES.iter().find_map(|constructor| {
            let pass = constructor();
            if pass.name() == name {
                Some(pass)
            } else {
                None
            }
        })
    }

    /// 列出所有可用的函数 Pass
    pub fn list_function_passes(&self) -> Vec<(String, String)> {
        let mut passes: Vec<_> = self
            .function_passes
            .iter()
            .map(|(name, ctor)| {
                let pass = ctor();
                (name.clone(), pass.description().to_string())
            })
            .collect();
        passes.sort_by(|(name1, _), (name2, _)| name1.cmp(name2));
        passes
    }

    /// 列出所有可用的分析 Pass（从全局注册表）
    pub fn list_analysis_passes(&self) -> Vec<(String, String)> {
        use crate::pass::pass_manager::STANDARD_ANALYSIS_PASSES;
        let mut passes: Vec<_> = STANDARD_ANALYSIS_PASSES
            .iter()
            .map(|constructor| {
                let pass = constructor();
                (pass.name().to_string(), pass.description().to_string())
            })
            .collect();
        passes.sort_by(|(name1, _), (name2, _)| name1.cmp(name2));
        passes
    }

    /// 打印所有可用的 Pass
    pub fn print_available_passes(&self) {
        println!("=== 可用的函数级别 Pass ===");
        for (name, desc) in self.list_function_passes() {
            println!("  {}: {}", name, desc);
        }

        println!("\n=== 可用的分析 Pass (全局注册) ===");
        for (name, desc) in self.list_analysis_passes() {
            println!("  {}: {}", name, desc);
        }
    }

    /// 从字符串构建 Pass 管道
    ///
    /// 格式: "pass1,pass2,pass3"
    /// 例如: "dce,const-fold,peephole"
    ///
    /// 注意: Analysis passes 从全局注册表自动加载并根据依赖按需执行，
    /// 不需要在字符串中显式指定。如果字符串中包含 analysis pass 名称，会被忽略。
    pub fn build_pipeline_from_string(&self, pipeline: &str) -> crate::Result<PassManager> {
        // 特殊逻辑，如果 pipeline 为 O0 O1 O2 O3，构建预设的 pipeline
        if pipeline == "O0" {
            return Ok(self.build_debug_pipeline());
        } else if pipeline == "O1" {
            return Ok(self.build_fast_pipeline());
        } else if pipeline == "O2" {
            return Ok(self.build_balanced_pipeline());
        } else if pipeline == "O3" {
            return Ok(self.build_performance_pipeline());
        }
        let mut manager = PassManager::new();
        let passes: Vec<&str> = pipeline.split(',').map(|s| s.trim()).collect();

        for pass_name in passes {
            if pass_name.is_empty() {
                continue;
            }

            // 检查是否为 analysis pass（从全局注册表）
            if self.create_analysis_pass(pass_name).is_some() {
                // Analysis passes 会自动按需执行，跳过
                continue;
            }

            // 尝试作为函数 Pass
            if let Some(function_pass) = self.create_function_pass(pass_name) {
                manager.add_function_pass(function_pass);
                continue;
            }

            return Err(format!("未知的 Pass: {}", pass_name).into());
        }

        Ok(manager)
    }

    /// 构建 Debug 模式 Pipeline（强类型 API）
    ///
    /// 最小化优化，但包含必要的 codegen passes 以生成可执行代码：
    /// - 寄存器分配
    /// - 栈帧布局
    /// - 指令降级
    /// - 调用位置活跃寄存器标注（必需）
    /// - IR 打印（用于调试）
    ///
    /// Analysis passes 从全局注册表自动加载。
    pub fn build_debug_pipeline(&self) -> PassManager {
        let mut manager = PassManager::new();

        // 必要的 codegen passes（无优化，但需要生成机器码）
        manager.add_function_passes(vec![
            // 显式化跳转 - 必须在基本块布局优化前，避免重排破坏隐式fall-through
            Box::new(ExplicitJumpPass::new()),
            // 基本块布局优化 - 即使是Debug模式也需要，确保生命周期分析正确
            Box::new(BlockLayoutPass::new()),
            Box::new(LinearScanRegisterAllocation::new()),
            Box::new(crate::pass::stack_frame_layout::StackFrameLayoutPass::new()),
            Box::new(InstructionLoweringPass::new()),
            // 🔧 调用位置活跃寄存器标注 - 必须在 InstructionLowering 之后运行
            Box::new(CallsiteLiveRegisterPass::new()),
            Box::new(PrintIRPass::new()), // 打印最终的 IR
        ]);

        manager
    }

    /// 构建 Fast 模式 Pipeline（强类型 API）
    ///
    /// 快速编译，包含基本优化：
    /// - 常量折叠
    /// - 寄存器分配
    /// - 栈帧布局
    /// - 指令降级
    /// - 调用位置活跃寄存器标注（必需）
    ///
    /// Analysis passes 从全局注册表自动加载。
    pub fn build_fast_pipeline(&self) -> PassManager {
        let mut manager = PassManager::new();

        // 基本优化（批量添加）
        manager.add_function_passes(vec![
            Box::new(ConstantFolding::new()),
            // 显式化跳转 - 必须在基本块布局优化前
            Box::new(ExplicitJumpPass::new()),
            // 基本块布局优化 - 必须在寄存器分配前
            Box::new(BlockLayoutPass::new()),
            Box::new(LinearScanRegisterAllocation::new()),
            Box::new(crate::pass::stack_frame_layout::StackFrameLayoutPass::new()),
            Box::new(InstructionLoweringPass::new()),
            // 🔧 调用位置活跃寄存器标注 - 必须在 InstructionLowering 之后运行
            Box::new(CallsiteLiveRegisterPass::new()),
        ]);

        manager
    }

    /// 构建 Balanced 模式 Pipeline（强类型 API）
    ///
    /// 平衡的优化，包含完整的优化流程：
    /// - Effect 降级
    /// - 常量折叠
    /// - SSA 构造和 Memory2Reg
    /// - Phi 节点消除
    /// - 基本块布局优化
    /// - 寄存器分配和栈帧布局
    /// - 窥孔优化和死代码消除
    /// - 指令降级
    ///
    /// 注意：Analysis passes（CFG、Def-Use、Lifetime）从全局注册表自动加载，
    /// 并根据各 Pass 的 `required_analyses()` 自动按需执行。
    pub fn build_balanced_pipeline(&self) -> PassManager {
        let mut manager = PassManager::new();

        // 只添加 transformation passes，analysis 会自动执行（批量添加）
        manager.add_function_passes(vec![
            // 🔧 2025-12: Effect指令降级必须在CFG相关pass之前运行
            // 因为CFG分析不理解effect指令，会错误地认为handler块不可达
            Box::new(EffectLoweringPass::new()),
            Box::new(ConstantFolding::new()),
            Box::new(SsaConstructionPass::new()),
        ]);

        // 在 debug 模式下添加 SSA 后 invariant check
        if cfg!(debug_assertions) {
            manager.add_function_pass(Box::new(PipelineVerifyPass::after_ssa()));
        }

        manager.add_function_passes(vec![
            Box::new(Memory2RegPass::new()),
        ]);

        // 在 debug 模式下添加 Memory2Reg 后 invariant check
        if cfg!(debug_assertions) {
            manager.add_function_pass(Box::new(PipelineVerifyPass::after_memory2reg()));
        }

        manager.add_function_passes(vec![
            Box::new(PhiEliminationPass::new()),
            // 🔧 显式化跳转 - 必须在基本块布局优化前，避免重排破坏隐式fall-through
            Box::new(ExplicitJumpPass::new()),
            // 🔧 基本块布局优化 - 必须在寄存器分配前运行
            Box::new(BlockLayoutPass::new()),
            Box::new(LinearScanRegisterAllocation::new()),
        ]);

        // 在 debug 模式下添加寄存器分配后 invariant check
        if cfg!(debug_assertions) {
            manager.add_function_pass(Box::new(PipelineVerifyPass::after_register_allocation()));
        }

        manager.add_function_passes(vec![
            Box::new(crate::pass::stack_frame_layout::StackFrameLayoutPass::new()),
            Box::new(PeepholeOptimizer::new()),
            Box::new(DeadCodeElimination::new()),
            Box::new(PeepholeOptimizer::new()),
            Box::new(InstructionLoweringPass::new()),
            // 🔧 调用位置活跃寄存器标注 - 必须在 InstructionLowering 之后运行
            Box::new(CallsiteLiveRegisterPass::new()),
        ]);

        manager
    }

    /// 构建 Performance 模式 Pipeline（强类型 API）
    ///
    /// 激进优化，在 Balanced 基础上增加：
    /// - 额外的常量折叠和死代码消除轮次
    /// - 调用位置活跃寄存器标注（必需）
    /// - 最终的验证 Pass
    ///
    /// Analysis passes 从全局注册表自动加载。
    pub fn build_performance_pipeline(&self) -> PassManager {
        let mut manager = PassManager::new();

        // 激进优化
        manager.add_function_passes(vec![
            // 🔧 2025-12: Effect指令降级必须在CFG相关pass之前运行
            Box::new(EffectLoweringPass::new()),
            Box::new(ConstantFolding::new()),
            Box::new(SsaConstructionPass::new()),
        ]);

        // 在 debug 模式下添加 SSA 后 invariant check
        if cfg!(debug_assertions) {
            manager.add_function_pass(Box::new(PipelineVerifyPass::after_ssa()));
        }

        manager.add_function_passes(vec![
            Box::new(Memory2RegPass::new()),
        ]);

        // 在 debug 模式下添加 Memory2Reg 后 invariant check
        if cfg!(debug_assertions) {
            manager.add_function_pass(Box::new(PipelineVerifyPass::after_memory2reg()));
        }

        manager.add_function_passes(vec![
            Box::new(PhiEliminationPass::new()),
            // 🔧 显式化跳转 - 必须在基本块布局优化前
            Box::new(ExplicitJumpPass::new()),
            // 🔧 基本块布局优化 - 必须在寄存器分配前
            Box::new(BlockLayoutPass::new()),
            Box::new(LinearScanRegisterAllocation::new()),
        ]);

        // 在 debug 模式下添加寄存器分配后 invariant check
        if cfg!(debug_assertions) {
            manager.add_function_pass(Box::new(PipelineVerifyPass::after_register_allocation()));
        }

        manager.add_function_passes(vec![
            Box::new(crate::pass::stack_frame_layout::StackFrameLayoutPass::new()),
            Box::new(PeepholeOptimizer::new()),
            Box::new(DeadCodeElimination::new()),
            Box::new(PeepholeOptimizer::new()),
            // 额外的优化轮次
            Box::new(ConstantFolding::new()),
            Box::new(DeadCodeElimination::new()),
            Box::new(InstructionLoweringPass::new()),
            // 🔧 调用位置活跃寄存器标注 - 必须在 InstructionLowering 之后运行
            Box::new(CallsiteLiveRegisterPass::new()),
            Box::new(VerifyPass::new()),
        ]);

        manager
    }

    /// 从预设类型构建 Pipeline
    ///
    /// 使用强类型 API 构建预定义的标准 pipeline
    ///
    /// # 示例
    /// ```
    /// use karte_lir::{PassRegistry, PipelinePreset};
    ///
    /// let registry = PassRegistry::default();
    /// let manager = registry.build_preset_pipeline(PipelinePreset::Balanced);
    /// ```
    pub fn build_preset_pipeline(&self, preset: PipelinePreset) -> PassManager {
        match preset {
            PipelinePreset::Debug => self.build_debug_pipeline(),
            PipelinePreset::Fast => self.build_fast_pipeline(),
            PipelinePreset::Balanced => self.build_balanced_pipeline(),
            PipelinePreset::Performance => self.build_performance_pipeline(),
        }
    }
}

impl Default for PassRegistry {
    fn default() -> Self {
        Self::with_standard_passes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_standard_passes() {
        let registry = PassRegistry::with_standard_passes();

        assert!(registry.create_function_pass("dce").is_some());
        assert!(registry.create_function_pass("const-fold").is_some());
        assert!(registry.create_analysis_pass("cfg").is_some());
        assert!(registry.create_function_pass("unknown").is_none());
    }

    #[test]
    fn test_build_pipeline_from_string() {
        let registry = PassRegistry::with_standard_passes();

        let result = registry.build_pipeline_from_string("cfg,def-use,dce,const-fold");
        assert!(result.is_ok());

        let result = registry.build_pipeline_from_string("unknown-pass");
        assert!(result.is_err());
    }

    #[test]
    fn test_list_passes() {
        let registry = PassRegistry::with_standard_passes();

        let function_passes = registry.list_function_passes();
        assert!(!function_passes.is_empty());

        let analysis_passes = registry.list_analysis_passes();
        assert!(!analysis_passes.is_empty());
    }
}
