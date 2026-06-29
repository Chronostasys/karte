//! # karte-gir — GPU 中间表示 (GPU Intermediate Representation)
//!
//! GIR 是 LIR 到 GPU 目标代码之间的中间层，负责：
//! 1. 将标量 LIR 指令提升为 SIMT 语义
//! 2. 插入内存层次标注（global / shared / local）
//! 3. 展开 tile 操作为线程级计算
//! 4. 插入同步指令

pub mod ir;
pub mod json;
pub mod lower;
pub mod tile_expansion;
pub mod optimization;

pub use ir::*;
pub use json::*;
pub use lower::lower_lir_to_gir;
pub use tile_expansion::expand_tiles;
pub use optimization::{auto_config, estimate_block_size, VectorizePass, LoopUnroller, SoftwarePipelinePass, CsePass, DcePass};
