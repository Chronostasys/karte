use crate::types::{Type, TypeVar};
use karte_diagnostics::Span;
use std::fmt;

/// 类型检查错误
#[derive(Debug, Clone)]
pub enum TypeCheckError {
    UndefinedVariable {
        name: String,
        span: Span,
        /// 拼写建议（最相似的已定义变量名）
        suggestion: Option<String>,
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
    InvalidAssignmentTarget {
        span: Span,
    },
    ModuleInterfaceUnavailable {
        module: String,
        span: Span,
    },
    UndefinedModuleSymbol {
        module: String,
        symbol: String,
        span: Span,
    },
    IndexOutOfBounds {
        index: i64,
        length: i64,
        span: Span,
    },
    DuplicateFunctionDefinition {
        name: String,
        span: Span,
    },
    /// 内建函数使用错误
    BuiltinFunctionError {
        function: String,
        message: String,
        span: Span,
    },
    InvalidMainReturnType {
        found: Type,
        span: Span,
    },
    /// match 表达式非穷尽
    NonExhaustiveMatch {
        missing_patterns: Vec<String>,
        span: Span,
    },
}

impl fmt::Display for TypeCheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeCheckError::UndefinedVariable { name, suggestion, .. } => {
                if let Some(s) = suggestion {
                    write!(f, "Undefined variable: {} (did you mean '{}'?)", name, s)
                } else {
                    write!(f, "Undefined variable: {}", name)
                }
            }
            TypeCheckError::TypeMismatch {
                expected, found, ..
            } => {
                write!(f, "Type mismatch: expected `{}`, found `{}`", expected, found)
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
                struct_name,
                expected,
                found,
                ..
            } => {
                write!(
                    f,
                    "Struct {} missing fields: expected {} fields, found {}",
                    struct_name, expected, found
                )
            }
            TypeCheckError::UnknownField {
                struct_name,
                field_name,
                ..
            } => {
                write!(f, "Unknown field {} in struct {}", field_name, struct_name)
            }
            TypeCheckError::NotAStruct { name, .. } => {
                write!(f, "{} is not a struct", name)
            }
            TypeCheckError::UndefinedType { name, .. } => {
                write!(f, "Undefined type: {}", name)
            }
            TypeCheckError::InvalidAssignmentTarget { .. } => {
                write!(f, "Invalid assignment target")
            }
            TypeCheckError::ModuleInterfaceUnavailable { module, .. } => {
                write!(
                    f,
                    "Module `{}` is not available in this compilation unit",
                    module
                )
            }
            TypeCheckError::UndefinedModuleSymbol { module, symbol, .. } => {
                write!(f, "Module `{}` does not export `{}`", module, symbol)
            }
            TypeCheckError::DuplicateFunctionDefinition { name, .. } => {
                write!(f, "Duplicate function definition: {}", name)
            }
            TypeCheckError::IndexOutOfBounds { index, length, .. } => {
                write!(f, "Index {} out of bounds (length {})", index, length)
            }
            TypeCheckError::BuiltinFunctionError { function, message, .. } => {
                write!(f, "Builtin function `{}`: {}", function, message)
            }
            TypeCheckError::InvalidMainReturnType { found, .. } => {
                write!(f, "main 函数的返回类型不能是 `{}`，该类型无法作为退出码返回", found)
            }
            TypeCheckError::NonExhaustiveMatch { missing_patterns, .. } => {
                if missing_patterns.len() == 1 {
                    write!(f, "非穷尽 match: 缺少模式 `{}`", missing_patterns[0])
                } else if missing_patterns.len() <= 3 {
                    write!(
                        f,
                        "非穷尽 match: 缺少模式 {}",
                        missing_patterns
                            .iter()
                            .map(|p| format!("`{}`", p))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                } else {
                    write!(
                        f,
                        "非穷尽 match: 缺少 {} 个模式（{}, ...）",
                        missing_patterns.len(),
                        missing_patterns[..3]
                            .iter()
                            .map(|p| format!("`{}`", p))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            }
        }
    }
}

impl TypeCheckError {
    pub fn span(&self) -> Span {
        match self {
            TypeCheckError::UndefinedVariable { span, .. } => *span,
            | TypeCheckError::TypeMismatch { span, .. }
            | TypeCheckError::ArityMismatch { span, .. }
            | TypeCheckError::NotCallable { span, .. }
            | TypeCheckError::CannotInferType { span }
            | TypeCheckError::InfiniteType { span, .. }
            | TypeCheckError::InvalidConstructor { span, .. }
            | TypeCheckError::InvalidPattern { span, .. }
            | TypeCheckError::EmptyMatch { span }
            | TypeCheckError::MissingFields { span, .. }
            | TypeCheckError::UnknownField { span, .. }
            | TypeCheckError::NotAStruct { span, .. }
            | TypeCheckError::UndefinedType { span, .. }
            | TypeCheckError::InvalidAssignmentTarget { span }
            | TypeCheckError::ModuleInterfaceUnavailable { span, .. }
            | TypeCheckError::UndefinedModuleSymbol { span, .. }
            | TypeCheckError::DuplicateFunctionDefinition { span, .. }
            | TypeCheckError::IndexOutOfBounds { span, .. }
            | TypeCheckError::BuiltinFunctionError { span, .. }
            | TypeCheckError::InvalidMainReturnType { span, .. }
            | TypeCheckError::NonExhaustiveMatch { span, .. } => *span,
        }
    }
}
