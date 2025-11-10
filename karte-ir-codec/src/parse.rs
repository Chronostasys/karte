use crate::error::{ParseError, ParseResult};
use nom::{
    branch::alt,
    bytes::complete::{tag, take_while, take_while1},
    character::complete::{char, digit1, multispace0},
    combinator::{map, map_res, opt, recognize},
    multi::separated_list0,
    sequence::{delimited, pair},
    IResult,
};

/// IR Parse trait - 用于从文本格式解析回 IR 类型
pub trait IrParse: Sized {
    /// 从字符串解析
    fn parse_ir(input: &str) -> ParseResult<Self>;

    /// 使用 nom 解析器（供内部使用）
    fn parse_nom(input: &str) -> IResult<&str, Self>;
}

/// 解析上下文 - 提供解析时的上下文信息
#[derive(Debug, Clone)]
pub struct ParseContext {
    /// 当前解析位置
    pub position: usize,
}

impl Default for ParseContext {
    fn default() -> Self {
        Self::new()
    }
}

impl ParseContext {
    pub fn new() -> Self {
        Self { position: 0 }
    }
}

// ============================================================================
// 基础解析器
// ============================================================================

/// 解析空白字符
pub fn ws<'a, F, O>(inner: F) -> impl FnMut(&'a str) -> IResult<&'a str, O>
where
    F: FnMut(&'a str) -> IResult<&'a str, O>,
{
    delimited(multispace0, inner, multispace0)
}

/// 解析标识符
pub fn identifier(input: &str) -> IResult<&str, &str> {
    recognize(pair(
        take_while1(|c: char| c.is_alphabetic() || c == '_'),
        take_while(|c: char| c.is_alphanumeric() || c == '_' || c == '$'),
    ))(input)
}

/// 解析整数
pub fn integer(input: &str) -> IResult<&str, i64> {
    map_res(recognize(pair(opt(char('-')), digit1)), |s: &str| {
        s.parse::<i64>()
    })(input)
}

/// 解析无符号整数
pub fn unsigned_integer(input: &str) -> IResult<&str, usize> {
    map_res(digit1, |s: &str| s.parse::<usize>())(input)
}

/// 解析布尔值
pub fn boolean(input: &str) -> IResult<&str, bool> {
    alt((map(tag("true"), |_| true), map(tag("false"), |_| false)))(input)
}

/// 解析关键字 (keyword with trailing whitespace)
pub fn keyword<'a>(kw: &'static str) -> impl FnMut(&'a str) -> IResult<&'a str, &'a str> {
    move |input: &'a str| ws(tag(kw))(input)
}

/// 解析token前缀 (only leading whitespace, no trailing whitespace)
/// 用于解析像 "bb0" 这样的token，其中 "bb" 是前缀，后面紧跟数字
pub fn token_prefix<'a>(prefix: &'static str) -> impl FnMut(&'a str) -> IResult<&'a str, &'a str> {
    use nom::sequence::preceded;
    move |input: &'a str| preceded(multispace0, tag(prefix))(input)
}

/// 解析括号包裹的内容
pub fn parens<'a, F, O>(inner: F) -> impl FnMut(&'a str) -> IResult<&'a str, O>
where
    F: FnMut(&'a str) -> IResult<&'a str, O>,
{
    delimited(ws(char('(')), inner, ws(char(')')))
}

/// 解析逗号分隔的列表
pub fn comma_separated<'a, F, O>(item: F) -> impl FnMut(&'a str) -> IResult<&'a str, Vec<O>>
where
    F: FnMut(&'a str) -> IResult<&'a str, O>,
{
    separated_list0(ws(char(',')), item)
}

/// 解析参数列表（带括号）
pub fn args_list<'a, F, O>(item: F) -> impl FnMut(&'a str) -> IResult<&'a str, Vec<O>>
where
    F: FnMut(&'a str) -> IResult<&'a str, O>,
{
    parens(comma_separated(item))
}

/// 解析 body 风格的字段 (label: value)
pub fn body_field<'a, F, O>(label: &'static str, mut inner: F) -> impl FnMut(&'a str) -> IResult<&'a str, O>
where
    F: FnMut(&'a str) -> IResult<&'a str, O>,
{
    move |input: &'a str| {
        // Allow an optional leading comma before a body field to be tolerant of
        // separators produced by nested collections (e.g. HashMap entries).
        let (input, _) = opt(ws(char(',')))(input)?;
        let (input, _) = ws(tag(label))(input)?;
        let (input, _) = ws(tag(":"))(input)?;
        let (input, _) = multispace0(input)?;
        let (input, result) = inner(input)?;
        Ok((input, result))
    }
}

// ============================================================================
// 为基础类型实现 IrParse
// ============================================================================

impl IrParse for String {
    fn parse_ir(input: &str) -> ParseResult<Self> {
        let (_, result) = Self::parse_nom(input).map_err(|e| ParseError::from(e))?;
        Ok(result)
    }

    fn parse_nom(input: &str) -> IResult<&str, Self> {
        map(identifier, |s: &str| s.to_string())(input)
    }
}

impl IrParse for i64 {
    fn parse_ir(input: &str) -> ParseResult<Self> {
        let (_, result) = Self::parse_nom(input).map_err(|e| ParseError::from(e))?;
        Ok(result)
    }

    fn parse_nom(input: &str) -> IResult<&str, Self> {
        integer(input)
    }
}

impl IrParse for usize {
    fn parse_ir(input: &str) -> ParseResult<Self> {
        let (_, result) = Self::parse_nom(input).map_err(|e| ParseError::from(e))?;
        Ok(result)
    }

    fn parse_nom(input: &str) -> IResult<&str, Self> {
        unsigned_integer(input)
    }
}

impl IrParse for u8 {
    fn parse_ir(input: &str) -> ParseResult<Self> {
        let (_, result) = Self::parse_nom(input).map_err(|e| ParseError::from(e))?;
        Ok(result)
    }

    fn parse_nom(input: &str) -> IResult<&str, Self> {
        map_res(digit1, |s: &str| s.parse::<u8>())(input)
    }
}

impl IrParse for bool {
    fn parse_ir(input: &str) -> ParseResult<Self> {
        let (_, result) = Self::parse_nom(input).map_err(|e| ParseError::from(e))?;
        Ok(result)
    }

    fn parse_nom(input: &str) -> IResult<&str, Self> {
        boolean(input)
    }
}

impl<T: IrParse> IrParse for Option<T> {
    fn parse_ir(input: &str) -> ParseResult<Self> {
        let (_, result) = Self::parse_nom(input).map_err(|e| ParseError::from(e))?;
        Ok(result)
    }

    fn parse_nom(input: &str) -> IResult<&str, Self> {
        alt((map(tag("none"), |_| None), map(T::parse_nom, Some)))(input)
    }
}

impl<T: IrParse> IrParse for Box<T> {
    fn parse_ir(input: &str) -> ParseResult<Self> {
        let (_, result) = Self::parse_nom(input).map_err(|e| ParseError::from(e))?;
        Ok(result)
    }

    fn parse_nom(input: &str) -> IResult<&str, Self> {
        map(T::parse_nom, Box::new)(input)
    }
}

impl<T: IrParse> IrParse for Vec<T> {
    fn parse_ir(input: &str) -> ParseResult<Self> {
        let (_, result) = Self::parse_nom(input).map_err(|e| ParseError::from(e))?;
        Ok(result)
    }

    fn parse_nom(input: &str) -> IResult<&str, Self> {
        delimited(ws(char('[')), comma_separated(T::parse_nom), ws(char(']')))(input)
    }
}
