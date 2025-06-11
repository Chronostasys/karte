use karte_diagnostics::Span;
use std::fmt;

/// 抽象语法树节点
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    // 基础值
    Number {
        value: i64,
        span: Span,
    },
    Unit {
        span: Span,
    },
    Identifier {
        name: String,
        span: Span,
    },

    // 二元和一元操作
    BinaryOp {
        left: Box<Expr>,
        op: BinaryOperator,
        right: Box<Expr>,
        span: Span,
    },
    UnaryOp {
        op: UnaryOperator,
        operand: Box<Expr>,
        span: Span,
    },

    // 函数相关
    Lambda {
        params: Vec<String>,
        body: Box<Expr>,
        span: Span,
    },
    FunctionCall {
        function: Box<Expr>,
        args: Vec<Expr>,
        span: Span,
    },

    // 语句作为表达式
    Statement {
        stmt: Box<Statement>,
        span: Span,
    },

    // 块表达式（语句序列）
    Block {
        statements: Vec<Statement>,
        final_expr: Option<Box<Expr>>, // 最后的表达式作为返回值
        span: Span,
    },
}

/// 语句类型
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    // let语句
    Let {
        name: String,
        value: Expr,
        span: Span,
    },
    // 表达式语句（丢弃返回值）
    Expression {
        expr: Expr,
        span: Span,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOperator {
    Plus,
    Minus,
}

impl fmt::Display for BinaryOperator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BinaryOperator::Add => write!(f, "+"),
            BinaryOperator::Subtract => write!(f, "-"),
            BinaryOperator::Multiply => write!(f, "*"),
            BinaryOperator::Divide => write!(f, "/"),
        }
    }
}

impl fmt::Display for UnaryOperator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UnaryOperator::Plus => write!(f, "+"),
            UnaryOperator::Minus => write!(f, "-"),
        }
    }
}

impl fmt::Display for Statement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Statement::Let { name, value, .. } => write!(f, "let {} = {};", name, value),
            Statement::Expression { expr, .. } => write!(f, "{};", expr),
        }
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Number { value, .. } => write!(f, "{}", value),
            Expr::Unit { .. } => write!(f, "()"),
            Expr::BinaryOp {
                left, op, right, ..
            } => {
                write!(f, "({} {} {})", left, op, right)
            }
            Expr::UnaryOp { op, operand, .. } => {
                write!(f, "({}{})", op, operand)
            }
            Expr::Identifier { name, .. } => write!(f, "{}", name),
            Expr::Lambda { params, body, .. } => {
                write!(f, "lambda ({}) -> {}", params.join(", "), body)
            }
            Expr::FunctionCall { function, args, .. } => {
                write!(
                    f,
                    "{}({})",
                    function,
                    args.iter()
                        .map(|e| e.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            Expr::Statement { stmt, .. } => {
                write!(f, "{}", stmt)
            }
            Expr::Block {
                statements,
                final_expr,
                ..
            } => {
                let mut result = String::from("{ ");
                for stmt in statements {
                    result.push_str(&stmt.to_string());
                    result.push(' ');
                }
                if let Some(expr) = final_expr {
                    result.push_str(&expr.to_string());
                }
                result.push_str(" }");
                write!(f, "{}", result)
            }
        }
    }
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Number { span, .. } => *span,
            Expr::Unit { span, .. } => *span,
            Expr::BinaryOp { span, .. } => *span,
            Expr::UnaryOp { span, .. } => *span,
            Expr::Identifier { span, .. } => *span,
            Expr::Lambda { span, .. } => *span,
            Expr::FunctionCall { span, .. } => *span,
            Expr::Statement { span, .. } => *span,
            Expr::Block { span, .. } => *span,
        }
    }
}

impl Statement {
    pub fn span(&self) -> Span {
        match self {
            Statement::Let { span, .. } => *span,
            Statement::Expression { span, .. } => *span,
        }
    }
}
