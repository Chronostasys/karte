use karte_diagnostics::{DiagnosticBag, Span};
use logos::Logos;
use std::fmt;

/// Token 类型定义
#[derive(Logos, Debug, Clone, PartialEq)]
pub enum Token {
    // 数字
    #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().ok())]
    Number(i64),

    // 运算符
    #[token("+")]
    Plus,

    #[token("-")]
    Minus,

    #[token("*")]
    Multiply,

    #[token("/")]
    Divide,

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
    #[token("|")]
    Pipe,

    #[token("->")]
    Arrow,

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
    #[token(".")]
    Dot,

    // 添加冒号用于结构体字段
    #[token(":")]
    Colon,

    // 引用符号
    #[token("&")]
    Ampersand,

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

    // 代数效应语法关键字
    #[token("perform")]
    KwPerform,
    #[token("resume")]
    KwResume,
    #[token("handle")]
    KwHandle,
    #[token("in")]
    KwIn,

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
    // Error token - handled automatically by Logos 0.13+
    Error,
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Number(n) => write!(f, "{}", n),
            Token::Plus => write!(f, "+"),
            Token::Minus => write!(f, "-"),
            Token::Multiply => write!(f, "*"),
            Token::Divide => write!(f, "/"),
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
            Token::Arrow => write!(f, "->"),
            Token::Identifier(s) => write!(f, "{}", s),
            Token::Comma => write!(f, ","),
            Token::Semicolon => write!(f, ";"),
            Token::Dot => write!(f, "."),
            Token::Colon => write!(f, ":"),
            Token::Ampersand => write!(f, "&"),
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
                    tokens.push(TokenWithSpan::new(token, span));
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
