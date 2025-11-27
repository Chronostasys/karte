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
    pub body: Expr,
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
                write!(f, "Expected {}, found {}", expected, found)
            }
            ParseError::UnexpectedEof { expected } => {
                write!(f, "Unexpected end of input, expected {}", expected)
            }
            ParseError::InvalidExpression { message, .. } => {
                write!(f, "Invalid expression: {}", message)
            }
            ParseError::MissingClosingParen {
                opening_span,
                current_span,
            } => {
                write!(
                    f,
                    "Missing closing parenthesis at {:?} (opened at {:?})",
                    current_span, opening_span
                )
            }
            ParseError::MissingOperand {
                operator,
                operator_span,
            } => {
                write!(
                    f,
                    "Missing operand for operator {} at {:?}",
                    operator, operator_span
                )
            }
            ParseError::ExpressionTooDeep { max_depth, span } => {
                write!(
                    f,
                    "Expression too deep (max depth: {}) at {:?}",
                    max_depth, span
                )
            }
        }
    }
}
