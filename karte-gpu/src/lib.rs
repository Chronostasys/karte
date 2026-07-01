//! # karte-gpu — GPU 后端代码生成
//!
//! 将 GIR 编译为 GPU 目标代码（PTX / SPIR-V）。
//! - PTX 后端: NVIDIA GPU
//! - SPIR-V 后端: AMD / Intel / NVIDIA GPU（跨厂商，低级虚拟 ISA）

pub mod ptx;
pub mod spirv;
pub mod tile_expansion;
pub mod operator_fusion;
pub mod auto_tuning;

pub use ptx::PtxCompiler;
pub use spirv::SpirvCompiler;
pub use tile_expansion::{TileExpander, TileConfig};
pub use operator_fusion::{OperatorFusion, FusionPattern};
pub use auto_tuning::{AutoTuner, TuningResult, TuningConfig, CompilePipeline};

/// GPU 后端 trait
pub trait GpuBackend {
    type Output;

    fn compile(&mut self, gir: &karte_gir::GirProgram) -> Self::Output;
    fn target_name(&self) -> &str;
}
