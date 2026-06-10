//! Parser类型定义模块
//!
//! 本模块包含了Parser相关的所有类型定义，包括：
//! - 解析模式（ParserMode）
//! - 模块和导入相关类型（ModuleDecl、ImportDecl等）
//! - 解析结果类型（ParsedProgram）
//! - 错误类型（ParseError）

use karte_diagnostics::Span;
use karte_hir::Expr;
use karte_lexer::Token;
use std::fmt;

/// 解析模式，控制顶层语法的解析行为
///
/// Karte支持两种解析模式：
/// - 脚本模式：适合快速实验和单文件执行
/// - 项目模式：适合多模块项目，需要明确的模块结构
///
/// # 示例
///
/// ```ignore
/// // 脚本模式
/// let x = 42;  // 自动包装在main函数中
/// x * 2
///
/// // 项目模式
/// module main
/// fn main() -> number {  // 必须显式定义main
///     let x = 42;
///     x * 2
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParserMode {
    /// 脚本模式：允许顶层语句和表达式，隐式包装在 main 中
    ///
    /// 顶层可以直接写表达式和语句，解析器会自动将它们包装在一个
    /// 隐式的main函数中。适合REPL和快速脚本执行。
    Script,

    /// 项目模式：顶层只允许声明（fn, struct, enum, let），必须显式定义 main
    ///
    /// 顶层只能包含声明（函数、结构体、枚举等），必须显式定义一个
    /// main函数作为入口点。适合多模块项目和库开发。
    Project,
}

/// 模块声明（`module` 语句）
///
/// 在项目模式中，每个源文件必须以模块声明开头，
/// 声明该文件属于哪个模块。
///
/// # 示例
///
/// ```karte
/// module utils
///
/// pub fn add(x: number, y: number) -> number {
///     x + y
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleDecl {
    /// 模块名称
    pub name: String,
    /// 源代码位置
    pub span: Span,
}

/// import 语句的导入条目
///
/// 表示从模块导入的单个符号，可以选择性地使用别名。
///
/// # 示例
///
/// ```karte
/// import utils::{add, multiply as mul}
/// //             ^^^  ^^^^^^^^^^^^^^^^
/// //             |    符号名: multiply, 别名: mul
/// //             符号名: add, 无别名
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportSymbol {
    /// 符号名称
    pub name: String,
    /// 可选的别名（用于重命名导入的符号）
    pub alias: Option<String>,
    /// 源代码位置
    pub span: Span,
}

/// import 语句的选择器
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportSpecifier {
    /// 导入整个模块（默认）
    EntireModule,
    /// 仅导入部分符号
    Symbols(Vec<ImportSymbol>),
}

/// `import` 声明
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportDecl {
    pub path: Vec<String>,
    pub alias: Option<String>,
    pub specifier: ImportSpecifier,
    pub span: Span,
}

/// 解析得到的完整程序信息
///
/// 包含模块声明、导入语句和程序主体。
/// 在脚本模式中，module为None；在项目模式中，module必须存在。
///
/// # 字段
///
/// - `module`: 可选的模块声明（项目模式必需）
/// - `imports`: 所有import语句的列表
/// - `body`: 程序主体表达式（包含所有语句和声明）
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedProgram {
    /// 模块声明（项目模式中必需）
    pub module: Option<ModuleDecl>,
    /// 导入声明列表
    pub imports: Vec<ImportDecl>,
    /// 程序主体
    pub body: Box<Expr>,
}

/// 解析器错误
#[derive(Debug, Clone)]
pub enum ParseError {
    UnexpectedToken {
        expected: String,
        found: Token,
        span: Span,
    },
    UnexpectedEof {
        expected: String,
    },
    InvalidExpression {
        message: String,
        span: Span,
    },
    /// 缺少右括号
    MissingClosingParen {
        opening_span: Span,
        current_span: Span,
    },
    /// 缺少操作数
    MissingOperand {
        operator: String,
        operator_span: Span,
    },
    /// 表达式过于复杂（防止栈溢出）
    ExpressionTooDeep {
        max_depth: usize,
        span: Span,
    },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::UnexpectedToken {
                expected, found, ..
            } => {
                // 尝试提供拼写建议
                let found_str = format!("{}", found);
                let suggestion = Self::suggest_keyword(&found_str);
                if let Some(sug) = suggestion {
                    write!(f, "期望 {}, 实际 {}。你是否想写 '{}'?", expected, found, sug)
                } else {
                    write!(f, "期望 {}, 实际 {}", expected, found)
                }
            }
            ParseError::UnexpectedEof { expected } => {
                write!(f, "意外的输入结束, 期望 {}", expected)
            }
            ParseError::InvalidExpression { message, .. } => {
                write!(f, "无效的表达式: {}", message)
            }
            ParseError::MissingClosingParen {
                opening_span: _,
                current_span: _,
            } => {
                write!(f, "缺少右括号")
            }
            ParseError::MissingOperand {
                operator,
                operator_span: _,
            } => {
                write!(
                    f,
                    "运算符 '{}' 缺少操作数",
                    operator
                )
            }
            ParseError::ExpressionTooDeep { max_depth, span: _ } => {
                write!(
                    f,
                    "表达式嵌套过深 (最大深度: {})",
                    max_depth
                )
            }
        }
    }
}

/// 关键字拼写建议
impl ParseError {
    /// 所有关键字列表
    const KEYWORDS: &'static [&'static str] = &[
        "fn", "let", "return", "if", "else", "while", "for", "in",
        "match", "struct", "enum", "true", "false", "import", "from",
        "pub", "number", "string", "bool", "char", "Some", "None",
        "Ok", "Err", "Option", "Result", "Unit",
    ];

    /// 根据输入字符串建议可能的关键字
    fn suggest_keyword(input: &str) -> Option<String> {
        if input.len() < 2 || input.len() > 10 {
            return None;
        }
        if input.chars().next().map(|c| !c.is_alphabetic()).unwrap_or(true) {
            return None;
        }

        let mut best: Option<(&str, usize)> = None;
        for keyword in Self::KEYWORDS {
            let dist = Self::levenshtein(input, keyword);
            if dist > 0 && dist <= input.len() / 2 + 1 {
                if best.is_none() || dist < best.unwrap().1 {
                    best = Some((*keyword, dist));
                }
            }
        }

        best.map(|(k, _)| k.to_string())
    }

    /// Levenshtein 编辑距离
    fn levenshtein(a: &str, b: &str) -> usize {
        let a_len = a.chars().count();
        let b_len = b.chars().count();
        if a_len == 0 { return b_len; }
        if b_len == 0 { return a_len; }

        let mut matrix = vec![vec![0; b_len + 1]; a_len + 1];
        for (i, row) in matrix.iter_mut().enumerate() {
            row[0] = i;
        }
        for j in 0..=b_len {
            matrix[0][j] = j;
        }
        for (i, a_char) in a.chars().enumerate() {
            for (j, b_char) in b.chars().enumerate() {
                let cost = if a_char == b_char { 0 } else { 1 };
                matrix[i + 1][j + 1] = (matrix[i][j + 1] + 1)
                    .min(matrix[i + 1][j] + 1)
                    .min(matrix[i][j] + cost);
            }
        }
        matrix[a_len][b_len]
    }
}
