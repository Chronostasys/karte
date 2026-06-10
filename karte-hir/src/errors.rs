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
        context: Option<String>,
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
                    write!(f, "未定义的变量: {} (你是否想输入 '{}'?)", name, s)
                } else {
                    write!(f, "未定义的变量: {}", name)
                }
            }
            TypeCheckError::TypeMismatch {
                expected, found, context, ..
            } => {
                if let Some(ctx) = context {
                    write!(f, "类型不匹配: 期望 `{}`, 实际 `{}` ({})", expected, found, ctx)
                } else {
                    write!(f, "类型不匹配: 期望 `{}`, 实际 `{}`", expected, found)
                }
            }
            TypeCheckError::ArityMismatch {
                expected, found, ..
            } => {
                write!(
                    f,
                    "参数数量不匹配: 期望 {} 个参数, 实际 {} 个",
                    expected, found
                )
            }
            TypeCheckError::NotCallable { found_type, .. } => {
                write!(f, "无法调用类型为 `{}` 的值（只有函数和闭包可以被调用）", found_type)
            }
            TypeCheckError::CannotInferType { .. } => {
                write!(f, "无法推断类型，请添加类型标注")
            }
            TypeCheckError::InfiniteType { .. } => {
                write!(f, "无限类型：表达式的类型依赖于自身，请检查是否存在递归类型定义")
            }
            TypeCheckError::InvalidConstructor { name, .. } => {
                write!(f, "无效的构造器: `{}`（请检查构造器名称是否正确）", name)
            }
            TypeCheckError::InvalidPattern { message, .. } => {
                write!(f, "无效的模式: {}", message)
            }
            TypeCheckError::EmptyMatch { .. } => {
                write!(f, "空的 match 表达式，至少需要一个分支")
            }
            TypeCheckError::MissingFields {
                struct_name,
                expected,
                found,
                ..
            } => {
                write!(
                    f,
                    "结构体 `{}` 缺少字段: 期望 {} 个字段, 实际 {} 个（请检查是否遗漏了字段）",
                    struct_name, expected, found
                )
            }
            TypeCheckError::UnknownField {
                struct_name,
                field_name,
                ..
            } => {
                write!(f, "结构体 `{}` 中不存在字段 `{}`（请检查字段名称是否正确）", struct_name, field_name)
            }
            TypeCheckError::NotAStruct { name, .. } => {
                write!(f, "`{}` 不是结构体类型，无法使用构造器语法", name)
            }
            TypeCheckError::UndefinedType { name, .. } => {
                write!(f, "未定义的类型: `{}`（请检查类型名称是否正确）", name)
            }
            TypeCheckError::InvalidAssignmentTarget { .. } => {
                write!(f, "无效的赋值目标（只能对变量或结构体字段赋值）")
            }
            TypeCheckError::ModuleInterfaceUnavailable { module, .. } => {
                write!(
                    f,
                    "模块 `{}` 在当前编译单元中不可用",
                    module
                )
            }
            TypeCheckError::UndefinedModuleSymbol { module, symbol, .. } => {
                write!(f, "模块 `{}` 未导出 `{}`", module, symbol)
            }
            TypeCheckError::DuplicateFunctionDefinition { name, .. } => {
                write!(f, "重复的函数定义: `{}`（此名称已在此作用域中定义）", name)
            }
            TypeCheckError::IndexOutOfBounds { index, length, .. } => {
                write!(f, "索引 {} 超出范围（数组长度为 {}）", index, length)
            }
            TypeCheckError::BuiltinFunctionError { function, message, .. } => {
                write!(f, "内置函数 `{}`: {}", function, message)
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
