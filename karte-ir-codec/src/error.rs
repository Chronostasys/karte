use thiserror::Error;

/// IR 解析错误
#[derive(Error, Debug, Clone)]
pub enum ParseError {
    #[error("Expected {expected}, found {found}")]
    UnexpectedToken { expected: String, found: String },

    #[error("Unexpected end of input")]
    UnexpectedEof,

    #[error("Invalid variant: {0}")]
    InvalidVariant(String),

    #[error("Invalid field: {0}")]
    InvalidField(String),

    #[error("Invalid value: {0}")]
    InvalidValue(String),

    #[error("Parse error: {0}")]
    Custom(String),

    #[error("Nom error: {0}")]
    Nom(String),
}

/// 解析结果类型
pub type ParseResult<T> = Result<T, ParseError>;

impl<'a> From<nom::Err<nom::error::Error<&'a str>>> for ParseError {
    fn from(err: nom::Err<nom::error::Error<&'a str>>) -> Self {
        ParseError::Nom(format!("{:?}", err))
    }
}
