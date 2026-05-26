use karte_common::memory::OwnershipKind;
use karte_diagnostics::Span;
use std::fmt;

use crate::types::Type;

/// 格式化模式用于显示
fn format_pattern(pattern: &Pattern) -> String {
    match pattern {
        Pattern::Wildcard { .. } => "_".to_string(),
        Pattern::Variable { name, .. } => name.clone(),
        Pattern::Constructor { name, arg, .. } => {
            if let Some(arg) = arg {
                format!("{}({})", name, format_pattern(arg))
            } else {
                name.clone()
            }
        }
        Pattern::QualifiedConstructor {
            type_name,
            constructor_name,
            arg,
            ..
        } => {
            if let Some(arg) = arg {
                format!(
                    "{}::{}({})",
                    type_name,
                    constructor_name,
                    format_pattern(arg)
                )
            } else {
                format!("{}::{}", type_name, constructor_name)
            }
        }
        Pattern::Number { value, .. } => value.to_string(),
        Pattern::Boolean { value, .. } => value.to_string(),
    }
}

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
    /// 模块符号访问（module::symbol）
    ModuleSymbolAccess {
        module_path: Vec<String>,
        symbol: String,
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
        params: Vec<Parameter>,
        body: Box<Expr>,
        inferred_type: Option<Type>,
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

    // 加法类型相关
    /// 构造器调用 (例如: Some(42), None, Left(value))
    Constructor {
        name: String,
        arg: Option<Box<Expr>>, // Some constructors don't take arguments
        span: Span,
    },

    /// 限定构造器调用 (例如: Color::Red, Option::Some(42))
    QualifiedConstructor {
        type_name: String,
        constructor_name: String,
        arg: Option<Box<Expr>>,
        span: Span,
    },

    /// 模式匹配表达式
    Match {
        expr: Box<Expr>,
        arms: Vec<MatchArm>,
        span: Span,
    },

    /// 布尔字面量 (语法糖，实际对应True/False构造器)
    Boolean {
        value: bool,
        span: Span,
    },

    /// If表达式 - 条件表达式
    If {
        condition: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Option<Box<Expr>>,
        span: Span,
    },

    /// While表达式 - 循环表达式
    While {
        condition: Box<Expr>,
        body: Box<Expr>,
        span: Span,
    },

    /// 结构体字面量 - 创建结构体实例
    StructLiteral {
        name: String,
        fields: Vec<FieldInit>,
        span: Span,
    },

    /// 字段访问 - 访问结构体的字段
    FieldAccess {
        object: Box<Expr>,
        field: String,
        span: Span,
    },
    /// 数组字面量
    ArrayLiteral {
        elements: Vec<Expr>,
        span: Span,
    },
    /// 下标访问
    Index {
        array: Box<Expr>,
        index: Box<Expr>,
        span: Span,
    },
    /// 数组长度
    ArrayLen {
        array: Box<Expr>,
        span: Span,
    },

    /// 引用表达式 - 创建对表达式的不可变引用
    Reference {
        expr: Box<Expr>,
        span: Span,
    },

    /// 解引用表达式 - 显式解引用操作
    Dereference {
        expr: Box<Expr>,
        span: Span,
    },

    /// 堆分配表达式 - 将值移动到堆上，返回指针
    HeapAllocate {
        value: Box<Expr>,
        ownership: OwnershipKind,
        span: Span,
    },

    /// 显式释放表达式 - 对堆指针执行释放操作
    HeapFree {
        pointer: Box<Expr>,
        span: Span,
    },
    /// 引用计数保留表达式 - 保留对表达式的引用
    Retain {
        pointer: Box<Expr>,
        span: Span,
    },
    /// 引用计数释放表达式 - 释放对表达式的引用
    Release {
        pointer: Box<Expr>,
        span: Span,
    },

    /// unsafe 内存读取 - 从任意地址读取指定字节数的值
    UnsafeLoad {
        addr: Box<Expr>,
        byte_size: u8, // 1, 4, or 8
        span: Span,
    },
    /// unsafe 内存写入 - 向任意地址写入指定字节数的值
    UnsafeStore {
        addr: Box<Expr>,
        value: Box<Expr>,
        byte_size: u8, // 1, 4, or 8
        span: Span,
    },

    /// 赋值表达式 - 为变量或字段赋值
    Assignment {
        target: Box<Expr>,
        value: Box<Expr>,
        span: Span,
    },

    // ===== 代数效应 =====
    /// perform 表达式：触发某个效应，传入载荷
    EffectPerform {
        tag: Box<Expr>,
        payload: Box<Expr>,
        span: Span,
    },

    /// resume 表达式：在处理器中恢复到触发点
    EffectResume {
        value: Box<Expr>,
        span: Span,
    },

    /// handle 表达式：在 `body` 的动态作用域内安装处理器
    /// 语法建议：handle tag(param) { handler } in body
    EffectHandle {
        tag: Box<Expr>,
        param: String,
        handler: Box<Expr>,
        body: Box<Expr>,
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
    // 类型定义语句 (enum定义)
    TypeDef {
        name: String,
        variants: Vec<TypeVariant>,
        span: Span,
    },

    // 结构体定义语句
    StructDef {
        name: String,
        fields: Vec<FieldDef>,
        span: Span,
    },

    // 赋值语句
    Assignment {
        target: Expr,
        value: Expr,
        span: Span,
    },

    // 函数定义语句
    FunctionDef {
        name: String,
        params: Vec<Parameter>,
        return_type: Option<String>,
        body: Expr,
        span: Span,
    },
}

/// 模式匹配的分支
#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Expr,
    pub span: Span,
}

/// 模式
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    /// 通配符模式 _
    Wildcard { span: Span },
    /// 变量绑定模式
    Variable { name: String, span: Span },
    /// 构造器模式 (例如: Some(x), None)
    Constructor {
        name: String,
        arg: Option<Box<Pattern>>,
        span: Span,
    },
    /// 数字字面量模式
    Number { value: i64, span: Span },
    /// 布尔字面量模式
    Boolean { value: bool, span: Span },
    /// 限定构造器模式 (例如: Color::Red, Option::Some(x))
    QualifiedConstructor {
        type_name: String,
        constructor_name: String,
        arg: Option<Box<Pattern>>,
        span: Span,
    },
}

/// 类型定义中的变体
#[derive(Debug, Clone, PartialEq)]
pub struct TypeVariant {
    pub name: String,
    pub data_type: Option<String>, // 简化版本，只支持类型名字符串
    pub span: Span,
}

/// 结构体字段初始化
#[derive(Debug, Clone, PartialEq)]
pub struct FieldInit {
    pub name: String,
    pub value: Expr,
    pub span: Span,
}

/// 结构体字段定义
#[derive(Debug, Clone, PartialEq)]
pub struct FieldDef {
    pub name: String,
    pub field_type: String, // 简化版本，只支持类型名字符串
    pub span: Span,
}

/// Lambda参数定义（支持类型注解）
#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    pub name: String,
    pub type_annotation: Option<String>, // 可选的类型注解
    pub span: Span,
}

impl Parameter {
    /// 创建一个简单的参数（无类型注解）
    pub fn simple(name: String) -> Self {
        Self {
            name,
            type_annotation: None,
            span: Span::new(0, 0),
        }
    }

    /// 创建一个带类型注解的参数
    pub fn typed(name: String, type_annotation: String) -> Self {
        Self {
            name,
            type_annotation: Some(type_annotation),
            span: Span::new(0, 0),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
    Equal,
    NotEqual,
    GreaterEqual,
    LessEqual,
    Greater,
    Less,
    // 逻辑运算符
    LogicalAnd,
    LogicalOr,
    // 位运算符
    BitAnd,
    BitOr,
    BitXor,
    ShiftLeft,
    ShiftRight,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOperator {
    Plus,
    Minus,
    // 逻辑非运算符
    LogicalNot,
    // 位非运算符
    BitNot,
}

impl fmt::Display for BinaryOperator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BinaryOperator::Add => write!(f, "+"),
            BinaryOperator::Subtract => write!(f, "-"),
            BinaryOperator::Multiply => write!(f, "*"),
            BinaryOperator::Divide => write!(f, "/"),
            BinaryOperator::Equal => write!(f, "=="),
            BinaryOperator::NotEqual => write!(f, "!="),
            BinaryOperator::GreaterEqual => write!(f, ">="),
            BinaryOperator::LessEqual => write!(f, "<="),
            BinaryOperator::Greater => write!(f, ">"),
            BinaryOperator::Less => write!(f, "<"),
            BinaryOperator::LogicalAnd => write!(f, "&&"),
            BinaryOperator::LogicalOr => write!(f, "||"),
            BinaryOperator::BitAnd => write!(f, "bitand"),
            BinaryOperator::BitOr => write!(f, "bitor"),
            BinaryOperator::BitXor => write!(f, "bitxor"),
            BinaryOperator::ShiftLeft => write!(f, "shl"),
            BinaryOperator::ShiftRight => write!(f, "shr"),
        }
    }
}

impl fmt::Display for UnaryOperator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UnaryOperator::Plus => write!(f, "+"),
            UnaryOperator::Minus => write!(f, "-"),
            UnaryOperator::LogicalNot => write!(f, "!"),
            UnaryOperator::BitNot => write!(f, "bitnot"),
        }
    }
}

impl fmt::Display for Statement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Statement::Let { name, value, .. } => write!(f, "let {} = {};", name, value),
            Statement::Expression { expr, .. } => write!(f, "{};", expr),
            Statement::TypeDef { name, variants, .. } => {
                let variants_str = variants
                    .iter()
                    .map(|v| {
                        if let Some(data_type) = &v.data_type {
                            format!("{}({})", v.name, data_type)
                        } else {
                            v.name.clone()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" | ");
                write!(f, "enum {} = {};", name, variants_str)
            }
            Statement::StructDef { name, fields, .. } => {
                let fields_str = fields
                    .iter()
                    .map(|f| format!("{}: {}", f.name, f.field_type))
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "struct {} = {{ {} }};", name, fields_str)
            }
            Statement::Assignment { target, value, .. } => {
                write!(f, "{} = {};", target, value)
            }
            Statement::FunctionDef {
                name,
                params,
                return_type,
                body,
                ..
            } => {
                let params_str = params
                    .iter()
                    .map(|p| {
                        if let Some(ty) = &p.type_annotation {
                            format!("{}: {}", p.name, ty)
                        } else {
                            p.name.clone()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let ret_str = if let Some(ret) = return_type {
                    format!(" -> {}", ret)
                } else {
                    "".to_string()
                };
                write!(f, "fn {}({}){} {}", name, params_str, ret_str, body)
            }
        }
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Number { value, .. } => write!(f, "{}", value),
            Expr::Unit { .. } => write!(f, "()"),
            Expr::Identifier { name, .. } => write!(f, "{}", name),
            Expr::ModuleSymbolAccess {
                module_path,
                symbol,
                ..
            } => {
                let path = module_path.join(".");
                if path.is_empty() {
                    write!(f, "::{}", symbol)
                } else {
                    write!(f, "{}::{}", path, symbol)
                }
            }
            Expr::BinaryOp {
                left, op, right, ..
            } => write!(f, "({} {} {})", left, op, right),
            Expr::UnaryOp { op, operand, .. } => write!(f, "({} {})", op, operand),
            Expr::Lambda { params, body, .. } => {
                let params_str = params
                    .iter()
                    .map(|p| {
                        if let Some(ref type_ann) = p.type_annotation {
                            format!("{}: {}", p.name, type_ann)
                        } else {
                            p.name.clone()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "|{}| {}", params_str, body)
            }
            Expr::FunctionCall { function, args, .. } => {
                write!(f, "{}(", function)?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", arg)?;
                }
                write!(f, ")")
            }
            Expr::Statement { stmt, .. } => {
                write!(f, "{}", stmt)
            }
            Expr::Block {
                statements,
                final_expr,
                ..
            } => {
                write!(f, "{{ ")?;
                for stmt in statements {
                    write!(f, "{}; ", stmt)?;
                }
                if let Some(expr) = final_expr {
                    write!(f, "{}", expr)?;
                }
                write!(f, " }}")
            }
            Expr::Match { expr, arms, .. } => {
                write!(f, "match {} {{ ", expr)?;
                for arm in arms {
                    write!(f, "{} -> {}, ", format_pattern(&arm.pattern), arm.body)?;
                }
                write!(f, "}}")
            }
            Expr::Constructor { name, arg, .. } => {
                if let Some(arg) = arg {
                    write!(f, "{}({})", name, arg)
                } else {
                    write!(f, "{}", name)
                }
            }
            Expr::QualifiedConstructor {
                type_name,
                constructor_name,
                arg,
                ..
            } => {
                if let Some(arg) = arg {
                    write!(f, "{}::{}({})", type_name, constructor_name, arg)
                } else {
                    write!(f, "{}::{}", type_name, constructor_name)
                }
            }
            Expr::Boolean { value, .. } => {
                write!(f, "{}", if *value { "true" } else { "false" })
            }
            Expr::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                if let Some(else_branch) = else_branch {
                    write!(
                        f,
                        "if {} then {} else {}",
                        condition, then_branch, else_branch
                    )
                } else {
                    write!(f, "if {} then {}", condition, then_branch)
                }
            }
            Expr::While {
                condition, body, ..
            } => {
                write!(f, "while {} do {}", condition, body)
            }
            Expr::StructLiteral { name, fields, .. } => {
                let fields_str = fields
                    .iter()
                    .map(|f| format!("{}: {}", f.name, f.value))
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "{} = {{ {} }};", name, fields_str)
            }
            Expr::FieldAccess { object, field, .. } => {
                write!(f, "{}.{}", object, field)
            }
            Expr::ArrayLiteral { elements, .. } => {
                let elems = elements
                    .iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "[{}]", elems)
            }
            Expr::Index { array, index, .. } => {
                write!(f, "{}[{}]", array, index)
            }
            Expr::ArrayLen { array, .. } => {
                write!(f, "len {}", array)
            }
            Expr::Reference { expr, .. } => {
                write!(f, "&{}", expr)
            }
            Expr::Dereference { expr, .. } => {
                write!(f, "*{}", expr)
            }
            Expr::HeapAllocate { value, .. } => {
                write!(f, "box {}", value)
            }
            Expr::HeapFree { pointer, .. } => {
                write!(f, "free {}", pointer)
            }
            Expr::Retain { pointer, .. } => {
                write!(f, "retain {}", pointer)
            }
            Expr::Release { pointer, .. } => {
                write!(f, "release {}", pointer)
            }
            Expr::UnsafeLoad { addr, byte_size, .. } => {
                write!(f, "unsafe_load{}({})", byte_size, addr)
            }
            Expr::UnsafeStore { addr, value, byte_size, .. } => {
                write!(f, "unsafe_store{}({}, {})", byte_size, addr, value)
            }
            Expr::Assignment { target, value, .. } => {
                write!(f, "{} = {}", target, value)
            }
            Expr::EffectPerform { tag, payload, .. } => {
                write!(f, "perform {}({})", tag, payload)
            }
            Expr::EffectResume { value, .. } => {
                write!(f, "resume({})", value)
            }
            Expr::EffectHandle {
                tag,
                param,
                handler,
                body,
                ..
            } => {
                write!(f, "handle {}({}) {{ {} }} in {}", tag, param, handler, body)
            }
        }
    }
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Number { span, .. } => *span,
            Expr::Unit { span, .. } => *span,
            Expr::Identifier { span, .. } => *span,
            Expr::ModuleSymbolAccess { span, .. } => *span,
            Expr::BinaryOp { span, .. } => *span,
            Expr::UnaryOp { span, .. } => *span,
            Expr::Lambda { span, .. } => *span,
            Expr::FunctionCall { span, .. } => *span,
            Expr::Statement { span, .. } => *span,
            Expr::Block { span, .. } => *span,
            Expr::Match { span, .. } => *span,
            Expr::Constructor { span, .. } => *span,
            Expr::QualifiedConstructor { span, .. } => *span,
            Expr::Boolean { span, .. } => *span,
            Expr::If { span, .. } => *span,
            Expr::While { span, .. } => *span,
            Expr::StructLiteral { span, .. } => *span,
            Expr::FieldAccess { span, .. } => *span,
            Expr::ArrayLiteral { span, .. } => *span,
            Expr::Index { span, .. } => *span,
            Expr::ArrayLen { span, .. } => *span,
            Expr::Reference { span, .. } => *span,
            Expr::Dereference { span, .. } => *span,
            Expr::HeapAllocate { span, .. } => *span,
            Expr::HeapFree { span, .. } => *span,
            Expr::Retain { span, .. } => *span,
            Expr::Release { span, .. } => *span,
            Expr::UnsafeLoad { span, .. } => *span,
            Expr::UnsafeStore { span, .. } => *span,
            Expr::Assignment { span, .. } => *span,
            Expr::EffectPerform { span, .. } => *span,
            Expr::EffectResume { span, .. } => *span,
            Expr::EffectHandle { span, .. } => *span,
        }
    }
}

impl Statement {
    pub fn span(&self) -> Span {
        match self {
            Statement::Let { span, .. } => *span,
            Statement::Expression { span, .. } => *span,
            Statement::TypeDef { span, .. } => *span,
            Statement::StructDef { span, .. } => *span,
            Statement::Assignment { span, .. } => *span,
            Statement::FunctionDef { span, .. } => *span,
        }
    }
}

impl Pattern {
    pub fn span(&self) -> Span {
        match self {
            Pattern::Wildcard { span } => *span,
            Pattern::Variable { span, .. } => *span,
            Pattern::Constructor { span, .. } => *span,
            Pattern::Number { span, .. } => *span,
            Pattern::Boolean { span, .. } => *span,
            Pattern::QualifiedConstructor { span, .. } => *span,
        }
    }
}
