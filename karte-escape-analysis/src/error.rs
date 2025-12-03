//! 逃逸分析错误类型

use thiserror::Error;

/// 逃逸分析错误
#[derive(Error, Debug, Clone)]
pub enum EscapeAnalysisError {
    #[error("变量未找到: {0}")]
    VariableNotFound(String),

    #[error("函数未找到: {0}")]
    FunctionNotFound(String),

    #[error("类型信息缺失: {0}")]
    MissingTypeInfo(String),

    #[error("循环依赖检测到: {0}")]
    CircularDependency(String),

    #[error("无效的逃逸点: {0}")]
    InvalidEscapePoint(String),

    #[error("分析失败: {0}")]
    AnalysisFailed(String),
}

/// Result 类型别名
pub type Result<T> = std::result::Result<T, EscapeAnalysisError>;
