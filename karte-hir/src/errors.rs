use crate::types::{Type, TypeVar};
use karte_diagnostics::Span;
use std::fmt;

/// 类型检查错误
#[derive(Debug, Clone)]
pub enum TypeCheckError {
    UndefinedVariable {
        name: String,
        span: Span,
    },
    TypeMismatch {
        expected: Type,
        found: Type,
        span: Span,
    },
    ArityMismatch {
        expected: usize,
        found: usize,
        span: Span,
    },
    NotCallable {
        found_type: Type,
        span: Span,
    },
    CannotInferType {
        span: Span,
    },
    InfiniteType {
        var: TypeVar,
        ty: Type,
        span: Span,
    },
}

impl fmt::Display for TypeCheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeCheckError::UndefinedVariable { name, .. } => {
                write!(f, "Undefined variable: {}", name)
            }
            TypeCheckError::TypeMismatch {
                expected, found, ..
            } => {
                write!(f, "Type mismatch: expected {}, found {}", expected, found)
            }
            TypeCheckError::ArityMismatch {
                expected, found, ..
            } => {
                write!(
                    f,
                    "Arity mismatch: expected {} arguments, found {}",
                    expected, found
                )
            }
            TypeCheckError::NotCallable { found_type, .. } => {
                write!(f, "Cannot call value of type {}", found_type)
            }
            TypeCheckError::CannotInferType { .. } => {
                write!(f, "Cannot infer type")
            }
            TypeCheckError::InfiniteType { .. } => {
                write!(f, "Infinite type")
            }
        }
    }
}

impl TypeCheckError {
    pub fn span(&self) -> Span {
        match self {
            TypeCheckError::UndefinedVariable { span, .. }
            | TypeCheckError::TypeMismatch { span, .. }
            | TypeCheckError::ArityMismatch { span, .. }
            | TypeCheckError::NotCallable { span, .. }
            | TypeCheckError::CannotInferType { span }
            | TypeCheckError::InfiniteType { span, .. } => *span,
        }
    }
}
