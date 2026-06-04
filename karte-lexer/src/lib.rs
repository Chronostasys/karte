use karte_diagnostics::{DiagnosticBag, Span};
use logos::Logos;
use std::fmt;

/// 处理字符串字面量中的转义序列
/// 支持: \n, \t, \r, \\, \", \0
/// 未知转义序列返回 Err，由 Logos 回退到 Error token
fn process_string_escapes(s: &str) -> Result<String, String> {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some('r') => result.push('\r'),
                Some('\\') => result.push('\\'),
                Some('"') => result.push('"'),
                Some('0') => result.push('\0'),
                Some(c) => return Err(format!("unknown escape: \\{}", c)),
                None => return Err("trailing backslash in string".to_string()),
            }
        } else {
            result.push(ch);
        }
    }
    Ok(result)
}

/// Token 类型定义
#[derive(Logos, Debug, Clone, PartialEq)]
pub enum Token {
    // 浮点数字面量（不支持，提前捕获给出友好错误；必须在 Number 和 Dot 之前定义）
    #[regex(r"[0-9]+\.[0-9]+")]
    FloatLiteral,

    // 数字（十六进制和二进制必须在十进制之前，Logos 最长匹配）
    #[regex(r"0x[0-9a-fA-F]+", |lex| i64::from_str_radix(&lex.slice()[2..], 16).ok())]
    #[regex(r"0b[01]+", |lex| i64::from_str_radix(&lex.slice()[2..], 2).ok())]
    #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().ok())]
    Number(i64),

    // 复合赋值运算符（必须在对应单字符运算符之前，Logos 最长匹配）
    #[token("+=")]
    PlusEqual,

    #[token("-=")]
    MinusEqual,

    #[token("*=")]
    StarEqual,

    #[token("/=")]
    SlashEqual,

    // 运算符
    #[token("+")]
    Plus,

    #[token("-")]
    Minus,

    #[token("*")]
    Multiply,

    #[token("/")]
    Divide,

    #[token("%")]
    Percent,

    #[token("=")]
    Equal,

    // 比较操作符（需要在单字符比较符号之前定义）
    #[token("==")]
    EqualEqual,

    #[token("!=")]
    NotEqual,

    #[token(">=")]
    GreaterEqual,

    #[token("<=")]
    LessEqual,

    // 移位复合赋值（必须在 << 和 >> 之前定义，Logos 最长匹配）
    #[token("<<=")]
    ShiftLeftEqual,

    #[token(">>=")]
    ShiftRightEqual,

    // 移位符号（双字符，必须在 > 和 < 之前定义以避免被截断匹配）
    #[token("<<")]
    ShiftLeftSym,

    #[token(">>")]
    ShiftRightSym,

    #[token(">")]
    Greater,

    #[token("<")]
    Less,

    #[token("[")]
    LeftBracket,

    #[token("]")]
    RightBracket,

    // 括号
    #[token("(")]
    LeftParen,

    #[token(")")]
    RightParen,

    #[token("{")]
    LeftBrace,

    #[token("}")]
    RightBrace,

    // Lambda 语法
    // 位或复合赋值（必须在 | 之前定义，Logos 最长匹配）
    #[token("|=")]
    PipeEqual,

    #[token("|")]
    Pipe,

    #[token("=>")]
    FatArrow,

    #[token("->")]
    Arrow,

    // 字符串字面量（支持转义字符）
    #[regex(r#""(?:[^"\\]|\\.)*""#, |lex| {
        let s = lex.slice();
        let inner = &s[1..s.len()-1];
        process_string_escapes(inner).ok()
    })]
    StringLiteral(String),

    // 标识符和关键字
    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice().to_string())]
    Identifier(String),

    // 关键字 - 这些会在后面的解析阶段区分
    // 我们先作为标识符处理，然后在parser中转换

    // 标点符号
    #[token(",")]
    Comma,

    #[token(";")]
    Semicolon,

    // 添加点操作符用于字段访问
    // 范围运算符（必须在 Dot 之前定义，Logos 最长匹配）
    #[token("..=")]
    DotDotEqual,

    #[token("..")]
    DoubleDot,

    #[token(".")]
    Dot,

    // 添加冒号用于结构体字段
    #[token(":")]
    Colon,

    // 引用符号
    // 位与复合赋值（必须在 & 之前定义，Logos 最长匹配）
    #[token("&=")]
    AmpersandEqual,

    #[token("&")]
    Ampersand,

    // 异或符号
    // 异或复合赋值（必须在 ^ 之前定义，Logos 最长匹配）
    #[token("^=")]
    CaretEqual,

    #[token("^")]
    Caret,

    // 按位取反符号
    #[token("~")]
    Tilde,

    // 添加逻辑运算符
    #[token("&&")]
    LogicalAnd,

    #[token("||")]
    LogicalOr,

    #[token("!")]
    LogicalNot,

    // 泛型类型符号（单独定义，避免冲突）
    #[token("::")]
    DoubleColon,

    // 模式匹配相关
    #[token("_")]
    Underscore,

    #[token("for")]
    KwFor,
    #[token("break")]
    KwBreak,
    #[token("continue")]
    KwContinue,
    #[token("return")]
    KwReturn,

    // 代数效应语法关键字
    #[token("perform")]
    KwPerform,
    #[token("resume")]
    KwResume,
    #[token("handle")]
    KwHandle,
    #[token("in")]
    KwIn,

    // 可见性关键字
    #[token("pub")]
    KwPub,

    // 位运算关键字（避免与 & | 符号冲突）
    #[token("bitand")]
    BitAnd,
    #[token("bitor")]
    BitOr,
    #[token("bitxor")]
    BitXor,
    #[token("bitnot")]
    BitNot,
    #[token("shl")]
    ShiftLeft,
    #[token("shr")]
    ShiftRight,

    // unsafe 内存操作内建函数
    #[token("unsafe_load")]
    UnsafeLoad,
    #[token("unsafe_store")]
    UnsafeStore,
    #[token("unsafe_load8")]
    UnsafeLoad8,
    #[token("unsafe_store8")]
    UnsafeStore8,
    #[token("unsafe_load32")]
    UnsafeLoad32,
    #[token("unsafe_store32")]
    UnsafeStore32,

    // runtime 内建函数
    #[token("runtime_heap_base")]
    RuntimeHeapBase,
    #[token("runtime_heap_limit")]
    RuntimeHeapLimit,
    #[token("runtime_stack_bottom")]
    RuntimeStackBottom,
    #[token("runtime_stack_top")]
    RuntimeStackTop,
    #[token("runtime_vm_sp")]
    RuntimeVmSp,

    // GC 寄存器保存/恢复内建函数
    #[token("gc_push_regs")]
    GcPushRegs,
    #[token("gc_pop_regs")]
    GcPopRegs,

    // 跳过空白字符
    #[regex(r"[ \t\n\f]+", logos::skip)]

    // 跳过行注释（// 到行尾）
    #[regex(r"//[^\n]*", logos::skip)]

    // Error token - handled automatically by Logos 0.13+
    Error,
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Number(n) => write!(f, "{}", n),
            Token::PlusEqual => write!(f, "+="),
            Token::MinusEqual => write!(f, "-="),
            Token::StarEqual => write!(f, "*="),
            Token::SlashEqual => write!(f, "/="),
            Token::ShiftLeftEqual => write!(f, "<<="),
            Token::ShiftRightEqual => write!(f, ">>="),
            Token::PipeEqual => write!(f, "|="),
            Token::AmpersandEqual => write!(f, "&="),
            Token::CaretEqual => write!(f, "^="),
            Token::Plus => write!(f, "+"),
            Token::Minus => write!(f, "-"),
            Token::Multiply => write!(f, "*"),
            Token::Divide => write!(f, "/"),
            Token::Percent => write!(f, "%"),
            Token::Equal => write!(f, "="),
            Token::EqualEqual => write!(f, "=="),
            Token::NotEqual => write!(f, "!="),
            Token::GreaterEqual => write!(f, ">="),
            Token::LessEqual => write!(f, "<="),
            Token::Greater => write!(f, ">"),
            Token::Less => write!(f, "<"),
            Token::LeftParen => write!(f, "("),
            Token::RightParen => write!(f, ")"),
            Token::LeftBrace => write!(f, "{{"),
            Token::RightBrace => write!(f, "}}"),
            Token::LeftBracket => write!(f, "["),
            Token::RightBracket => write!(f, "]"),
            Token::Pipe => write!(f, "|"),
            Token::FatArrow => write!(f, "=>"),
            Token::Arrow => write!(f, "->"),
            Token::Identifier(s) => write!(f, "{}", s),
            Token::StringLiteral(s) => write!(f, "\"{}\"", s),
            Token::Comma => write!(f, ","),
            Token::Semicolon => write!(f, ";"),
            Token::DotDotEqual => write!(f, "..="),
            Token::DoubleDot => write!(f, ".."),
            Token::Dot => write!(f, "."),
            Token::Colon => write!(f, ":"),
            Token::Ampersand => write!(f, "&"),
            Token::Caret => write!(f, "^"),
            Token::Tilde => write!(f, "~"),
            Token::ShiftLeftSym => write!(f, "<<"),
            Token::ShiftRightSym => write!(f, ">>"),
            Token::LogicalAnd => write!(f, "&&"),
            Token::LogicalOr => write!(f, "||"),
            Token::LogicalNot => write!(f, "!"),

            Token::BitAnd => write!(f, "bitand"),
            Token::BitOr => write!(f, "bitor"),
            Token::BitXor => write!(f, "bitxor"),
            Token::BitNot => write!(f, "bitnot"),
            Token::ShiftLeft => write!(f, "shl"),
            Token::ShiftRight => write!(f, "shr"),

            Token::UnsafeLoad => write!(f, "unsafe_load"),
            Token::UnsafeStore => write!(f, "unsafe_store"),
            Token::UnsafeLoad8 => write!(f, "unsafe_load8"),
            Token::UnsafeStore8 => write!(f, "unsafe_store8"),
            Token::UnsafeLoad32 => write!(f, "unsafe_load32"),
            Token::UnsafeStore32 => write!(f, "unsafe_store32"),
            Token::RuntimeHeapBase => write!(f, "runtime_heap_base"),
            Token::RuntimeHeapLimit => write!(f, "runtime_heap_limit"),
            Token::RuntimeStackBottom => write!(f, "runtime_stack_bottom"),
            Token::RuntimeVmSp => write!(f, "runtime_vm_sp"),
            Token::RuntimeStackTop => write!(f, "runtime_stack_top"),
            Token::GcPushRegs => write!(f, "gc_push_regs"),
            Token::GcPopRegs => write!(f, "gc_pop_regs"),
            Token::DoubleColon => write!(f, "::"),
            Token::Underscore => write!(f, "_"),
            Token::KwPerform => write!(f, "perform"),
            Token::KwResume => write!(f, "resume"),
            Token::KwHandle => write!(f, "handle"),
            Token::KwIn => write!(f, "in"),
            Token::KwPub => write!(f, "pub"),
            Token::KwFor => write!(f, "for"),
            Token::KwBreak => write!(f, "break"),
            Token::KwContinue => write!(f, "continue"),
            Token::KwReturn => write!(f, "return"),
            Token::FloatLiteral => write!(f, "<float literal>"),
            Token::Error => write!(f, "<error>"),
        }
    }
}

/// 带位置信息的Token
#[derive(Debug, Clone, PartialEq)]
pub struct TokenWithSpan {
    pub token: Token,
    pub span: Span,
}

impl TokenWithSpan {
    pub fn new(token: Token, span: Span) -> Self {
        Self { token, span }
    }
}

/// 词法分析器
pub struct Lexer<'a> {
    lexer: logos::Lexer<'a, Token>,
    diagnostics: DiagnosticBag,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            lexer: Token::lexer(input),
            diagnostics: DiagnosticBag::new(),
        }
    }

    pub fn tokenize(&mut self) -> Vec<TokenWithSpan> {
        let mut tokens = Vec::new();

        while let Some(result) = self.lexer.next() {
            let span = Span::new(self.lexer.span().start, self.lexer.span().end);

            match result {
                Ok(token) => {
                    if matches!(token, Token::FloatLiteral) {
                        self.diagnostics.add_error(
                            "不支持浮点数字面量".to_string(),
                            span,
                        );
                    } else {
                        tokens.push(TokenWithSpan::new(token, span));
                    }
                }
                Err(_) => {
                    self.diagnostics.add_error(
                        format!("Unexpected character: '{}'", self.lexer.slice()),
                        span,
                    );
                }
            }
        }

        tokens
    }

    pub fn diagnostics(&self) -> &DiagnosticBag {
        &self.diagnostics
    }

    pub fn into_diagnostics(self) -> DiagnosticBag {
        self.diagnostics
    }
}

/// 关键字识别器
pub fn is_keyword(ident: &str) -> bool {
    matches!(
        ident,
        "let" | "match" | "enum" | "struct" | "true" | "false" | "if" | "else" | "while"
            | "for" | "in" | "break" | "continue" | "return"
    )
}

/// 从标识符创建关键字或标识符token
pub fn keyword_or_identifier(s: String) -> Token {
    Token::Identifier(s)
}

/// 便捷的词法分析函数
pub fn tokenize(input: &str) -> (Vec<TokenWithSpan>, DiagnosticBag) {
    let mut lexer = Lexer::new(input);
    let tokens = lexer.tokenize();
    (tokens, lexer.into_diagnostics())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_string() {
        let (tokens, diag) = tokenize(r#""hello""#);
        assert!(!diag.has_errors());
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].token, Token::StringLiteral("hello".to_string()));
    }

    #[test]
    fn test_escape_newline_tab() {
        let (tokens, diag) = tokenize(r#""\n\t""#);
        assert!(!diag.has_errors());
        assert_eq!(tokens.len(), 1);
        assert!(matches!(&tokens[0].token, Token::StringLiteral(s) if s == "\n\t"));
    }

    #[test]
    fn test_escape_carriage_return() {
        let (tokens, diag) = tokenize(r#""\r""#);
        assert!(!diag.has_errors());
        assert_eq!(tokens.len(), 1);
        assert!(matches!(&tokens[0].token, Token::StringLiteral(s) if s == "\r"));
    }

    #[test]
    fn test_escape_backslash() {
        let (tokens, diag) = tokenize(r#""\\""#);
        assert!(!diag.has_errors());
        assert_eq!(tokens.len(), 1);
        assert!(matches!(&tokens[0].token, Token::StringLiteral(s) if s == "\\"));
    }

    #[test]
    fn test_escape_double_quote() {
        let (tokens, diag) = tokenize(r#""say \"hello\"""#);
        assert!(!diag.has_errors());
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].token, Token::StringLiteral("say \"hello\"".to_string()));
    }

    #[test]
    fn test_escape_null() {
        let (tokens, diag) = tokenize(r#""\0""#);
        assert!(!diag.has_errors());
        assert_eq!(tokens.len(), 1);
        assert!(matches!(&tokens[0].token, Token::StringLiteral(s) if s == "\0"));
    }

    #[test]
    fn test_all_escapes_combined() {
        let (tokens, diag) = tokenize(r#""\n\t\r\\\"\0""#);
        assert!(!diag.has_errors());
        assert_eq!(tokens.len(), 1);
        assert!(matches!(&tokens[0].token, Token::StringLiteral(s) if s == "\n\t\r\\\"\0"));
    }

    #[test]
    fn test_empty_string() {
        let (tokens, diag) = tokenize(r#""""#);
        assert!(!diag.has_errors());
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].token, Token::StringLiteral("".to_string()));
    }

    #[test]
    fn test_mixed_content_with_escapes() {
        let (tokens, diag) = tokenize(r#""line1\nline2\ttab""#);
        assert!(!diag.has_errors());
        assert_eq!(tokens.len(), 1);
        assert!(matches!(&tokens[0].token, Token::StringLiteral(s) if s == "line1\nline2\ttab"));
    }

    #[test]
    fn test_invalid_escape_reports_error() {
        let (tokens, diag) = tokenize(r#""\x""#);
        assert!(diag.has_errors());
    }

    #[test]
    fn test_string_in_expression_context() {
        let (tokens, diag) = tokenize(r#"let s = "hello\nworld""#);
        assert!(!diag.has_errors());
        let string_tokens: Vec<_> = tokens.iter()
            .filter(|t| matches!(&t.token, Token::StringLiteral(_)))
            .collect();
        assert_eq!(string_tokens.len(), 1);
        assert!(matches!(&string_tokens[0].token, Token::StringLiteral(s) if s == "hello\nworld"));
    }

    #[test]
    fn test_escaped_backslash_at_end() {
        let (tokens, diag) = tokenize(r#""abc\\""#);
        assert!(!diag.has_errors());
        assert_eq!(tokens.len(), 1);
        assert!(matches!(&tokens[0].token, Token::StringLiteral(s) if s == "abc\\"));
    }

    #[test]
    fn test_unterminated_with_escape() {
        let (tokens, diag) = tokenize(r#""abc\"#);
        assert!(diag.has_errors());
    }
}
