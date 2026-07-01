//! Auto-Tuning — 自动搜索最优 tile size 配置
//!
//! 核心思路:
//! 1. 为同一个 kernel 生成多个 tile size 变体 (如 8×8, 16×16, 32×32, 64×64)
//! 2. 编译每个变体为 SPIR-V/PTX
//! 3. 在 GPU 上实际运行每个变体, 测量执行时间
//! 4. 选择最快的变体作为最终配置
//!
//! 使用方式:
//!   let tuner = AutoTuner::new(TuningConfig::default());
//!   let result = tuner.tune(&gir_program, &spv_compiler, &gpu_runner);
//!   println!("最优配置: {:?}", result.best_config);

use karte_gir::*;

/// 调优配置
#[derive(Debug, Clone)]
pub struct TuningConfig {
    /// 候选 tile 大小列表
    pub tile_sizes: Vec<(usize, usize, usize)>, // (tile_m, tile_n, tile_k)
    /// 每个 tile 的运行重复次数 (取平均)
    pub num_runs: usize,
    /// 是否预热 (先跑一次再计时)
    pub warmup: bool,
}

impl Default for TuningConfig {
    fn default() -> Self {
        Self {
            tile_sizes: vec![
                (8, 8, 8),
                (16, 16, 16),
                (32, 32, 16),
                (16, 32, 16),
                (32, 16, 16),
            ],
            num_runs: 3,
            warmup: true,
        }
    }
}

/// 调优结果
#[derive(Debug, Clone)]
pub struct TuningResult {
    /// 最优 tile 配置
    pub best_config: (usize, usize, usize),
    /// 最优执行时间 (微秒)
    pub best_time_us: f64,
    /// 所有变体的结果
    pub all_results: Vec<((usize, usize, usize), f64)>,
}

impl TuningResult {
    pub fn speedup_vs_baseline(&self, baseline_us: f64) -> f64 {
        if self.best_time_us > 0.0 {
            baseline_us / self.best_time_us
        } else {
            1.0
        }
    }
}

/// Auto-Tuner
pub struct AutoTuner {
    config: TuningConfig,
}

impl AutoTuner {
    pub fn new(config: TuningConfig) -> Self {
        Self { config }
    }

    pub fn with_default() -> Self {
        Self::new(TuningConfig::default())
    }

    /// 获取候选配置列表
    pub fn candidate_configs(&self) -> &[(usize, usize, usize)] {
        &self.config.tile_sizes
    }

    /// 为同一个 kernel 生成不同 tile size 的变体
    /// 返回: [(tile_config, modified_gir_program)]
    pub fn generate_variants(&self, prog: &GirProgram) -> Vec<((usize, usize, usize), GirProgram)> {
        let mut variants = Vec::new();

        for &tile in &self.config.tile_sizes {
            let mut new_prog = GirProgram::new();
            for kernel in &prog.kernels {
                let mut new_kernel = kernel.clone();
                // 更新 block_dim 以匹配 tile大小
                let threads = tile.0 * tile.1; // tile_m * tile_n 个线程
                new_kernel.block_dim = (threads, 1, 1);
                // 更新 TileLoad/TileMatmul 中的 tile 大小
                for instr in &mut new_kernel.instructions {
                    match instr {
                        GirInstruction::TileLoad { tile_rows, tile_cols, .. } => {
                            *tile_rows = tile.0;
                            *tile_cols = tile.1;
                        }
                        GirInstruction::TileStore { tile_rows, tile_cols, .. } => {
                            *tile_rows = tile.0;
                            *tile_cols = tile.1;
                        }
                        GirInstruction::TileZeros { tile_rows, tile_cols, .. } => {
                            *tile_rows = tile.0;
                            *tile_cols = tile.1;
                        }
                        GirInstruction::TileMatmul { m, n, k, .. } => {
                            *m = tile.0;
                            *n = tile.1;
                            *k = tile.2;
                        }
                        _ => {}
                    }
                }
                new_prog.add_kernel(new_kernel);
            }
            variants.push((tile, new_prog));
        }

        variants
    }

    /// 分析 kernel 特性, 推荐 tile 大小 (无需实际运行)
    pub fn recommend_tile_size(kernel: &GirFunction) -> (usize, usize, usize) {
        // 检查 kernel 类型
        let has_gemm = kernel.instructions.iter().any(|i| matches!(i,
            GirInstruction::TileMatmul { .. } | GirInstruction::Mma { .. }
        ));
        let has_reduction = kernel.instructions.iter().any(|i| matches!(i,
            GirInstruction::Reduce { .. }
        ));

        if has_gemm {
            // GEMM: 32×32×16 是常见最优配置
            (32, 32, 16)
        } else if has_reduction {
            // Reduction: 16×16 适合 (需 shared memory)
            (16, 16, 16)
        } else {
            // Element-wise: tile 大小不影响 (每线程独立)
            (16, 16, 16)
        }
    }
}

/// 编译管线 — 组合 tile_expansion + operator_fusion + auto_tuning
pub struct CompilePipeline {
    pub tile_expander: crate::tile_expansion::TileExpander,
    pub fusion: crate::operator_fusion::OperatorFusion,
    pub tuner: AutoTuner,
}

impl Default for CompilePipeline {
    fn default() -> Self {
        Self {
            tile_expander: crate::tile_expansion::TileExpander::with_default(),
            fusion: crate::operator_fusion::OperatorFusion::new(),
            tuner: AutoTuner::with_default(),
        }
    }
}

impl CompilePipeline {
    /// 完整优化管线:
    /// 1. 算子融合 (合并连续 element-wise kernel)
    /// 2. Tile 展开 (展开 TileLoad/TileMatmul 为底层指令)
    /// 3. (可选) Auto-tuning (搜索引擎)
    pub fn optimize(&self, prog: &GirProgram) -> GirProgram {
        // Step 1: 算子融合
        let fused = self.fusion.fuse(prog);

        // Step 2: Tile 展开
        let expanded = self.tile_expander.expand_program(&fused);

        expanded
    }

    /// 带 auto-tuning 的优化 (需要 GPU 运行时回调)
    pub fn optimize_with_tuning<F>(
        &self,
        prog: &GirProgram,
        benchmark: &F,
    ) -> (GirProgram, TuningResult)
    where
        F: Fn(&GirProgram) -> f64, // 返回执行时间 (微秒)
    {
        // Step 1: 算子融合
        let fused = self.fusion.fuse(prog);

        // Step 2: 生成 tile 变体并基准测试
        let variants = self.tuner.generate_variants(&fused);

        let mut results = Vec::new();
        for (tile_config, variant_prog) in &variants {
            // 展开每个变体
            let expanded = self.tile_expander.expand_program(variant_prog);

            // 基准测试
            let time = benchmark(&expanded);
            results.push((*tile_config, time));
        }

        // 选择最快的
        results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let best = results[0];

        // 用最优配置生成最终程序
        let best_config = crate::tile_expansion::TileConfig {
            tile_rows: best.0.0,
            tile_cols: best.0.1,
            tile_k: best.0.2,
        };
        let best_expander = crate::tile_expansion::TileExpander::new(best_config);
        let best_fused = self.fusion.fuse(prog);
        let final_prog = best_expander.expand_program(&best_fused);

        let result = TuningResult {
            best_config: best.0,
            best_time_us: best.1,
            all_results: results,
        };

        (final_prog, result)
    }
}
