//! Karte 逃逸分析模块
//!
//! 该模块实现了编译时逃逸分析，用于确定哪些变量可以安全地分配在栈上，
//! 哪些必须分配在堆上。通过逃逸分析，可以大幅减少堆分配和 GC 压力。

pub mod allocation_strategy;
pub mod analyzer;
pub mod context;
pub mod error;
pub mod graph;
pub mod instruction_generator;
pub mod types;
pub mod escape_point_detector;
pub mod escape_point_transformer;

pub use allocation_strategy::{
    AllocationStatistics, AllocationStrategy, AllocationStrategySelector, StackFrameLayout,
    StackSlot,
};
pub use analyzer::EscapeAnalyzer;
pub use context::AnalysisContext;
pub use error::{EscapeAnalysisError, Result};
pub use graph::VariableGraph;
pub use instruction_generator::{AllocationInstruction, InstructionGenerator};
pub use types::{
    AllocationSuggestion, EscapePoint, EscapeState, FunctionId, LifetimeConstraint,
    VariableEscapeInfo, VariableId,
};
pub use escape_point_detector::EscapePointDetector;
pub use escape_point_transformer::EscapePointTransformer;
