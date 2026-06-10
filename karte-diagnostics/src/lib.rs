use log::error;
use miette::{
    self, Diagnostic as MietteDiagnostic, NamedSource, Result as MietteResult, SourceSpan,
};
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

/// 源码中的位置信息
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, karte_ir_derive::IrCodec)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub fn len(&self) -> usize {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    pub fn dummy() -> Self {
        Self { start: 0, end: 0 }
    }
}

impl Default for Span {
    fn default() -> Self {
        Self::dummy()
    }
}

impl From<Span> for SourceSpan {
    fn from(span: Span) -> Self {
        SourceSpan::new(span.start.into(), span.len().into())
    }
}

/// 诊断级别
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagnosticLevel {
    Error,
    Warning,
    Info,
    Hint,
}

impl fmt::Display for DiagnosticLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DiagnosticLevel::Error => write!(f, "error"),
            DiagnosticLevel::Warning => write!(f, "warning"),
            DiagnosticLevel::Info => write!(f, "info"),
            DiagnosticLevel::Hint => write!(f, "hint"),
        }
    }
}

/// 美观的编译器错误 - 集成 miette
#[derive(Error, Debug, MietteDiagnostic)]
pub enum CompilerError {
    #[error("Lexical error")]
    #[diagnostic(code(E001))]
    LexError {
        #[label("Unexpected character")]
        span: SourceSpan,
        #[source_code]
        src: NamedSource,
        #[help]
        help: String,
    },

    #[error("Parse error")]
    #[diagnostic(code(E002))]
    ParseError {
        #[label("{message}")]
        span: SourceSpan,
        message: String,
        #[source_code]
        src: NamedSource,
        #[help]
        help: Option<String>,
    },

    #[error("Type error: {message}")]
    #[diagnostic(code(E003))]
    TypeError {
        #[label("Type mismatch occurred here")]
        span: SourceSpan,
        message: String,
        #[source_code]
        src: NamedSource,
        #[help]
        help: Option<String>,
    },

    #[error("Undefined variable: {name}")]
    #[diagnostic(code(E004))]
    UndefinedVariable {
        #[label("Variable '{name}' is not defined")]
        span: SourceSpan,
        name: String,
        #[source_code]
        src: NamedSource,
        #[help]
        help: String,
    },

    #[error("Function call error")]
    #[diagnostic(code(E005))]
    CallError {
        #[label("{message}")]
        span: SourceSpan,
        message: String,
        #[source_code]
        src: NamedSource,
        #[help]
        help: Option<String>,
    },

    #[error("Duplicate function definition: {name}")]
    #[diagnostic(code(E006))]
    DuplicateFunctionDefinition {
        #[label("Function '{name}' is already defined in this scope")]
        span: SourceSpan,
        name: String,
        #[source_code]
        src: NamedSource,
        #[help]
        help: String,
    },

    #[error("Warning: {message}")]
    #[diagnostic(code(W001), severity(Warning))]
    Warning {
        #[label("{message}")]
        span: SourceSpan,
        message: String,
        #[source_code]
        src: NamedSource,
        #[help]
        help: Option<String>,
    },

    #[error("IO error")]
    #[diagnostic(code(E999))]
    IoError {
        #[from]
        source: std::io::Error,
    },
}

/// 诊断信息 - 兼容旧接口
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    pub message: String,
    pub span: Span,
    pub code: Option<String>,
    pub source: Option<String>,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>, span: Span) -> Self {
        Self {
            level: DiagnosticLevel::Error,
            message: message.into(),
            span,
            code: None,
            source: None,
        }
    }

    pub fn warning(message: impl Into<String>, span: Span) -> Self {
        Self {
            level: DiagnosticLevel::Warning,
            message: message.into(),
            span,
            code: None,
            source: None,
        }
    }

    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    /// 转换为美观的 miette 错误
    pub fn into_compiler_error(self, source_code: &str, filename: &str) -> CompilerError {
        let src = NamedSource::new(filename, source_code.to_string());
        let span = SourceSpan::from(self.span);

        match self.level {
            DiagnosticLevel::Error => {
                if self.message.contains("Undefined variable") {
                    // 提取变量名和可能的建议
                    let name = self
                        .message
                        .strip_prefix("Undefined variable: ")
                        .unwrap_or("unknown")
                        .split(" (did you mean")
                        .next()
                        .unwrap_or("unknown")
                        .to_string();
                    
                    let help = if self.message.contains("(did you mean") {
                        // 提取建议 - 格式: "Undefined variable: xxx (did you mean 'yyy'?)"
                        let after_mean = self.message.split("(did you mean '").nth(1)
                            .unwrap_or("");
                        // 取出建议名（到下一个 ' 为止）
                        let suggestion = after_mean.split('\'').next().unwrap_or("");
                        if suggestion.is_empty() {
                            format!("Make sure '{}' is defined before use. You can define it with 'let {} = ...'", name.trim(), name.trim())
                        } else {
                            format!("Make sure '{}' is defined before use. Did you mean '{}'?", name.trim(), suggestion)
                        }
                    } else {
                        format!("Make sure '{}' is defined before use. You can define it with 'let {} = ...'", name.trim(), name.trim())
                    };
                    
                    CompilerError::UndefinedVariable {
                        span,
                        name: name.trim().to_string(),
                        src,
                        help,
                    }
                } else if self.message.contains("Type mismatch") {
                    CompilerError::TypeError {
                        span,
                        message: self.message,
                        src,
                        help: Some(
                            "The value type does not match what this operation requires. \
                             'expected' shows the required type, 'found' shows the actual type."
                                .to_string(),
                        ),
                    }
                } else if self.message.contains("Cannot call")
                    || self.message.contains("Arity mismatch")
                {
                    CompilerError::CallError {
                        span,
                        message: self.message,
                        src,
                        help: Some("Check the function signature and argument count".to_string()),
                    }
                } else if self.message.contains("Duplicate function") {
                    let name = self.message
                        .strip_prefix("Duplicate function definition: ")
                        .unwrap_or("unknown")
                        .to_string();
                    CompilerError::DuplicateFunctionDefinition {
                        span,
                        name,
                        src,
                        help: "A function with this name is already defined in this scope. Rename or remove the duplicate.".to_string(),
                    }
                } else if self.message.contains("Builtin function") {
                    CompilerError::TypeError {
                        span,
                        message: self.message,
                        src,
                        help: Some("Check the argument type for this built-in function".to_string()),
                    }
                } else if self.message.contains("非穷尽 match") {
                    CompilerError::TypeError {
                        span,
                        message: self.message,
                        src,
                        help: Some("Add a match arm for the missing pattern(s), or use a wildcard `_` to catch remaining cases.".to_string()),
                    }
                } else if self.message.contains("Expected") || self.message.contains("expected") {
                    CompilerError::ParseError {
                        span,
                        message: self.message,
                        src,
                        help: Some("Check the syntax of your expression".to_string()),
                    }
                } else {
                    CompilerError::ParseError {
                        span,
                        message: self.message,
                        src,
                        help: None,
                    }
                }
            }
            DiagnosticLevel::Warning => {
                CompilerError::Warning {
                    span,
                    message: self.message,
                    src,
                    help: None,
                }
            }
            _ => CompilerError::ParseError {
                span,
                message: format!("{}: {}", self.level, self.message),
                src,
                help: None,
            },
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(code) = &self.code {
            write!(f, "{} [{}]: {}", self.level, code, self.message)
        } else {
            write!(f, "{}: {}", self.level, self.message)
        }
    }
}

/// 诊断结果集合
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiagnosticBag {
    pub diagnostics: Vec<Diagnostic>,
}

impl DiagnosticBag {
    pub fn new() -> Self {
        Self {
            diagnostics: Vec::new(),
        }
    }

    pub fn add(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    pub fn add_error(&mut self, message: impl Into<String>, span: Span) {
        self.add(Diagnostic::error(message, span));
    }

    pub fn add_warning(&mut self, message: impl Into<String>, span: Span) {
        self.add(Diagnostic::warning(message, span));
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.level == DiagnosticLevel::Error)
    }

    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }

    pub fn len(&self) -> usize {
        self.diagnostics.len()
    }

    pub fn extend(&mut self, other: DiagnosticBag) {
        self.diagnostics.extend(other.diagnostics);
    }

    /// 使用 miette 打印美观的错误信息
    pub fn print_fancy(&self, source_code: &str, filename: &str) -> MietteResult<()> {
        for diagnostic in &self.diagnostics {
            let error = diagnostic
                .clone()
                .into_compiler_error(source_code, filename);
            error!("{:?}", miette::Report::new(error));
        }
        Ok(())
    }

    /// 获取第一个错误并转换为美观的 miette 错误
    pub fn first_error_as_miette(
        &self,
        source_code: &str,
        filename: &str,
    ) -> Option<CompilerError> {
        self.diagnostics
            .iter()
            .find(|d| d.level == DiagnosticLevel::Error)
            .map(|d| d.clone().into_compiler_error(source_code, filename))
    }
}

impl fmt::Display for DiagnosticBag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for diagnostic in &self.diagnostics {
            writeln!(f, "{}", diagnostic)?;
        }
        Ok(())
    }
}

/// 便捷的结果类型，使用 miette
pub type Result<T> = MietteResult<T>;

/// 辅助函数：创建带源码的 miette 错误
pub fn create_miette_error(
    error_type: &str,
    message: &str,
    span: Span,
    source_code: &str,
    filename: &str,
) -> CompilerError {
    let src = NamedSource::new(filename, source_code.to_string());
    let source_span = SourceSpan::from(span);

    match error_type {
        "lex" => CompilerError::LexError {
            span: source_span,
            src,
            help: "Check for invalid characters in your code".to_string(),
        },
        "parse" => CompilerError::ParseError {
            span: source_span,
            message: message.to_string(),
            src,
            help: Some("Check the syntax of your expression".to_string()),
        },
        "type" => CompilerError::TypeError {
            span: source_span,
            message: message.to_string(),
            src,
            help: Some("Make sure the types match what's expected".to_string()),
        },
        _ => CompilerError::ParseError {
            span: source_span,
            message: message.to_string(),
            src,
            help: None,
        },
    }
}

/// 创建无限类型错误的便捷函数
pub fn infinite_type_error(
    var_name: &str,
    type_name: &str,
    span: Span,
    source_code: &str,
    filename: &str,
) -> CompilerError {
    CompilerError::TypeError {
        span: SourceSpan::from(span),
        message: format!(
            "Cannot construct infinite type: {} = {}",
            var_name, type_name
        ),
        src: NamedSource::new(filename, source_code.to_string()),
        help: Some(
            "This usually happens when a type variable refers to itself recursively".to_string(),
        ),
    }
}
