//! # karte-gpu — GPU 后端代码生成
//!
//! 将 GIR 编译为 GPU 目标代码（PTX / SPIR-V）。
//! 目前实现 PTX（NVIDIA）后端。

pub mod ptx;

pub use ptx::PtxCompiler;

/// GPU 后端 trait
pub trait GpuBackend {
    type Output;

    fn compile(&mut self, gir: &karte_gir::GirProgram) -> Self::Output;
    fn target_name(&self) -> &str;
}
