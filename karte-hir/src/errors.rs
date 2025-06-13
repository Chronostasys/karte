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
    InvalidConstructor {
        name: String,
        span: Span,
    },
    InvalidPattern {
        message: String,
        span: Span,
    },
    EmptyMatch {
        span: Span,
    },
    MissingFields {
        struct_name: String,
        expected: usize,
        found: usize,
        span: Span,
    },
    UnknownField {
        struct_name: String,
        field_name: String,
        span: Span,
    },
    NotAStruct {
        name: String,
        span: Span,
    },
    UndefinedType {
        name: String,
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
            TypeCheckError::InvalidConstructor { name, .. } => {
                write!(f, "Invalid constructor: {}", name)
            }
            TypeCheckError::InvalidPattern { message, .. } => {
                write!(f, "Invalid pattern: {}", message)
            }
            TypeCheckError::EmptyMatch { .. } => {
                write!(f, "Empty match expression")
            }
            TypeCheckError::MissingFields {
                struct_name, expected, found, ..
            } => {
                write!(
                    f,
                    "Struct {} missing fields: expected {} fields, found {}",
                    struct_name, expected, found
                )
            }
            TypeCheckError::UnknownField {
                struct_name, field_name, ..
            } => {
                write!(f, "Unknown field {} in struct {}", field_name, struct_name)
            }
            TypeCheckError::NotAStruct { name, .. } => {
                write!(f, "{} is not a struct", name)
            }
            TypeCheckError::UndefinedType { name, .. } => {
                write!(f, "Undefined type: {}", name)
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
            | TypeCheckError::InfiniteType { span, .. }
            | TypeCheckError::InvalidConstructor { span, .. }
            | TypeCheckError::InvalidPattern { span, .. }
            | TypeCheckError::EmptyMatch { span } => *span,
            | TypeCheckError::MissingFields { span, .. }
            | TypeCheckError::UnknownField { span, .. }
            | TypeCheckError::NotAStruct { span, .. }
            | TypeCheckError::UndefinedType { span, .. } => *span,
        }
    }
}
