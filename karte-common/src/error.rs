//! Karte 编译器统一错误处理系统
//!
//! 提供了贯穿整个编译流程的统一错误类型和Result别名，
//! 简化错误传播和处理。

use std::fmt;
use std::io;
use std::path::PathBuf;

/// Karte 编译器通用错误类型
///
/// 封装了编译流程中可能出现的所有错误类型，
/// 提供统一的错误处理接口。
#[derive(Debug)]
pub enum KarteError {
    /// IO错误（文件读写、路径操作等）
    Io {
        source: io::Error,
        context: String,
    },

    /// 文件未找到
    FileNotFound {
        path: PathBuf,
    },

    /// 词法分析错误
    Lexer {
        message: String,
    },

    /// 语法分析错误
    Parse {
        message: String,
    },

    /// 类型检查错误
    TypeCheck {
        message: String,
    },

    /// 代码生成错误
    Codegen {
        message: String,
    },

    /// 模块系统错误
    Module {
        message: String,
    },

    /// 缓存操作错误
    Cache {
        message: String,
    },

    /// JSON序列化/反序列化错误
    Json {
        source: serde_json::Error,
        context: String,
    },

    /// IR解析错误
    IrParse {
        message: String,
    },

    /// 运行时错误
    Runtime {
        message: String,
    },

    /// 内部编译器错误（不应该发生的错误）
    Internal {
        message: String,
        file: &'static str,
        line: u32,
    },

    /// 多个错误的集合
    Multiple {
        errors: Vec<KarteError>,
    },
}

impl fmt::Display for KarteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KarteError::Io { source, context } => {
                write!(f, "IO错误: {} ({})", context, source)
            }
            KarteError::FileNotFound { path } => {
                write!(f, "文件未找到: {}", path.display())
            }
            KarteError::Lexer { message } => {
                write!(f, "词法分析错误: {}", message)
            }
            KarteError::Parse { message } => {
                write!(f, "语法分析错误: {}", message)
            }
            KarteError::TypeCheck { message } => {
                write!(f, "类型检查错误: {}", message)
            }
            KarteError::Codegen { message } => {
                write!(f, "代码生成错误: {}", message)
            }
            KarteError::Module { message } => {
                write!(f, "模块系统错误: {}", message)
            }
            KarteError::Cache { message } => {
                write!(f, "缓存错误: {}", message)
            }
            KarteError::Json { source, context } => {
                write!(f, "JSON错误: {} ({})", context, source)
            }
            KarteError::IrParse { message } => {
                write!(f, "IR解析错误: {}", message)
            }
            KarteError::Runtime { message } => {
                write!(f, "运行时错误: {}", message)
            }
            KarteError::Internal { message, file, line } => {
                write!(f, "内部编译器错误: {} (at {}:{})", message, file, line)
            }
            KarteError::Multiple { errors } => {
                writeln!(f, "发现 {} 个错误:", errors.len())?;
                for (i, err) in errors.iter().enumerate() {
                    writeln!(f, "  {}. {}", i + 1, err)?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for KarteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            KarteError::Io { source, .. } => Some(source),
            KarteError::Json { source, .. } => Some(source),
            _ => None,
        }
    }
}

// 便捷的错误构造函数
impl KarteError {
    /// 创建IO错误
    pub fn io(source: io::Error, context: impl Into<String>) -> Self {
        KarteError::Io {
            source,
            context: context.into(),
        }
    }

    /// 创建文件未找到错误
    pub fn file_not_found(path: impl Into<PathBuf>) -> Self {
        KarteError::FileNotFound { path: path.into() }
    }

    /// 创建词法分析错误
    pub fn lexer(message: impl Into<String>) -> Self {
        KarteError::Lexer {
            message: message.into(),
        }
    }

    /// 创建语法分析错误
    pub fn parse(message: impl Into<String>) -> Self {
        KarteError::Parse {
            message: message.into(),
        }
    }

    /// 创建类型检查错误
    pub fn type_check(message: impl Into<String>) -> Self {
        KarteError::TypeCheck {
            message: message.into(),
        }
    }

    /// 创建代码生成错误
    pub fn codegen(message: impl Into<String>) -> Self {
        KarteError::Codegen {
            message: message.into(),
        }
    }

    /// 创建模块系统错误
    pub fn module(message: impl Into<String>) -> Self {
        KarteError::Module {
            message: message.into(),
        }
    }

    /// 创建缓存错误
    pub fn cache(message: impl Into<String>) -> Self {
        KarteError::Cache {
            message: message.into(),
        }
    }

    /// 创建JSON错误
    pub fn json(source: serde_json::Error, context: impl Into<String>) -> Self {
        KarteError::Json {
            source,
            context: context.into(),
        }
    }

    /// 创建IR解析错误
    pub fn ir_parse(message: impl Into<String>) -> Self {
        KarteError::IrParse {
            message: message.into(),
        }
    }

    /// 创建运行时错误
    pub fn runtime(message: impl Into<String>) -> Self {
        KarteError::Runtime {
            message: message.into(),
        }
    }

    /// 创建内部编译器错误（使用宏更方便）
    pub fn internal(message: impl Into<String>, file: &'static str, line: u32) -> Self {
        KarteError::Internal {
            message: message.into(),
            file,
            line,
        }
    }

    /// 合并多个错误
    pub fn multiple(errors: Vec<KarteError>) -> Self {
        KarteError::Multiple { errors }
    }
}

/// 内部编译器错误宏
///
/// 用于报告不应该发生的错误（如不变量违反）
///
/// # 示例
/// ```ignore
/// if something_impossible {
///     return Err(internal_error!("这不应该发生"));
/// }
/// ```
#[macro_export]
macro_rules! internal_error {
    ($msg:expr) => {
        $crate::error::KarteError::internal($msg, file!(), line!())
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::error::KarteError::internal(format!($fmt, $($arg)*), file!(), line!())
    };
}

// 从IO错误自动转换
impl From<io::Error> for KarteError {
    fn from(err: io::Error) -> Self {
        KarteError::Io {
            source: err,
            context: "IO操作失败".to_string(),
        }
    }
}

// 从JSON错误自动转换
impl From<serde_json::Error> for KarteError {
    fn from(err: serde_json::Error) -> Self {
        KarteError::Json {
            source: err,
            context: "JSON操作失败".to_string(),
        }
    }
}

/// Karte Result类型别名
///
/// 在整个编译器中使用此类型简化错误处理
pub type Result<T> = std::result::Result<T, KarteError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = KarteError::parse("意外的token");
        assert_eq!(err.to_string(), "语法分析错误: 意外的token");

        let err = KarteError::file_not_found("main.karte");
        assert!(err.to_string().contains("main.karte"));
    }

    #[test]
    fn test_error_constructors() {
        let err = KarteError::module("模块未找到");
        assert!(matches!(err, KarteError::Module { .. }));

        let err = KarteError::cache("缓存键冲突");
        assert!(matches!(err, KarteError::Cache { .. }));
    }

    #[test]
    fn test_multiple_errors() {
        let errors = vec![
            KarteError::parse("错误1"),
            KarteError::type_check("错误2"),
        ];
        let multi = KarteError::multiple(errors);
        let display = multi.to_string();
        assert!(display.contains("发现 2 个错误"));
        assert!(display.contains("错误1"));
        assert!(display.contains("错误2"));
    }

    #[test]
    fn test_io_error_conversion() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "文件未找到");
        let karte_err: KarteError = io_err.into();
        assert!(matches!(karte_err, KarteError::Io { .. }));
    }
}
