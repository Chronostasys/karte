use karte_common::memory::OwnershipKind;
use karte_diagnostics::Span;
use std::fmt;

use crate::types::Type;

/// 格式化模式用于显示
fn format_pattern(pattern: &Pattern) -> String {
    match pattern {
        Pattern::Wildcard { .. } => "_".to_string(),
        Pattern::Variable { name, .. } => name.clone(),
        Pattern::Constructor { name, args, .. } => {
            if !args.is_empty() {
                let args_str = args.iter().map(|a| format_pattern(a)).collect::<Vec<_>>().join(", ");
                format!("{}({})", name, args_str)
            } else {
                name.clone()
            }
        }
        Pattern::QualifiedConstructor {
            type_name,
            constructor_name,
            args,
            ..
        } => {
            if !args.is_empty() {
                let args_str = args.iter().map(|a| format_pattern(a)).collect::<Vec<_>>().join(", ");
                format!(
                    "{}::{}({})",
                    type_name,
                    constructor_name,
                    args_str
                )
            } else {
                format!("{}::{}", type_name, constructor_name)
            }
        }
        Pattern::Number { value, .. } => value.to_string(),
        Pattern::Boolean { value, .. } => value.to_string(),
        Pattern::Struct { name, fields, .. } => {
            let fields_str = fields
                .iter()
                .map(|f| {
                    let pat_str = format_pattern(&f.pattern);
                    if f.field == pat_str {
                        f.field.clone()
                    } else {
                        format!("{}: {}", f.field, pat_str)
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("{} {{ {} }}", name, fields_str)
        }
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
    /// 字符串字面量
    StringLiteral {
        value: String,
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
        return_type: Option<Type>,
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
        args: Vec<Expr>,
        span: Span,
    },

    /// 限定构造器调用 (例如: Color::Red, Option::Some(42))
    QualifiedConstructor {
        type_name: String,
        constructor_name: String,
        args: Vec<Expr>,
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

    /// ForIn表达式 - 范围循环 for ident in start..end { body } 或 for ident in start..=end { body }
    ForIn {
        var: String,
        start: Box<Expr>,
        end: Box<Expr>,
        body: Box<Expr>,
        inclusive: bool, // true 表示 ..= (inclusive), false 表示 .. (exclusive)
        span: Span,
    },

    /// ForArray表达式 - 数组遍历 for ident in array_expr { body }
    ForArray {
        var: String,
        array: Box<Expr>,
        body: Box<Expr>,
        span: Span,
    },

    /// Break表达式 - 跳出当前循环
    Break {
        span: Span,
    },

    /// Continue表达式 - 跳到当前循环的条件检查
    Continue {
        span: Span,
    },

    /// Return表达式 - 从当前函数返回
    Return {
        value: Option<Box<Expr>>,
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
    /// 绝对值 abs(x)
    Abs {
        value: Box<Expr>,
        span: Span,
    },
    /// 最小值 min(a, b)
    Min {
        left: Box<Expr>,
        right: Box<Expr>,
        span: Span,
    },
    /// 最大值 max(a, b)
    Max {
        left: Box<Expr>,
        right: Box<Expr>,
        span: Span,
    },
    /// 限制值范围 clamp(value, min_val, max_val)
    Clamp {
        value: Box<Expr>,
        min_val: Box<Expr>,
        max_val: Box<Expr>,
        span: Span,
    },
    /// 字符串索引 str_index(s, i) — 返回第 i 个字节的 ASCII 值
    StrIndex {
        string: Box<Expr>,
        index: Box<Expr>,
        span: Span,
    },
    /// 字符取值 char_at(s, i) — 返回第 i 个字节位置的单字节字符串
    CharAt {
        string: Box<Expr>,
        index: Box<Expr>,
        span: Span,
    },
    /// 子字符串截取 substring(s, start, len)
    Substring {
        string: Box<Expr>,
        start: Box<Expr>,
        length: Box<Expr>,
        span: Span,
    },
    /// 字符串包含检测 str_contains(s, ch) — ch 为 ASCII 字节值，返回 0 或 1
    StrContains {
        string: Box<Expr>,
        char_code: Box<Expr>,
        span: Span,
    },
    /// 字符串分割计数 split_count(s, sep) — 返回按分隔符分割后的字段数量
    SplitCount {
        string: Box<Expr>,
        separator: Box<Expr>,
        span: Span,
    },
    /// 去除字符串首尾空格 trim(s)
    Trim {
        string: Box<Expr>,
        span: Span,
    },
    /// ASCII 码转字符串 char_to_string(expr) — 将 number(ASCII码) 转换为单字符字符串
    CharToString {
        expr: Box<Expr>,
        span: Span,
    },
    /// 数字转字符串 to_string(expr) — 将 number 转换为字符串
    ToString {
        expr: Box<Expr>,
        span: Span,
    },
    /// 元组字面量
    TupleLiteral {
        elements: Vec<Expr>,
        span: Span,
    },
    /// 元组字段访问 (t.0, t.1)
    TupleAccess {
        object: Box<Expr>,
        index: usize,
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

    /// runtime 内建函数 - 读取 runtime 全局变量
    RuntimeGlobal {
        name: String, // "heap_start", "heap_limit", "vstack_bottom" 等
        span: Span,
    },

    /// GC 寄存器保存/恢复内建函数
    GcRegOp {
        is_push: bool, // true = gc_push_regs, false = gc_pop_regs
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

    // ===== 类型转换 (as 表达式) =====
    /// 显式类型转换: expr as TargetType
    TypeCast {
        expr: Box<Expr>,
        target_type: Type,
        span: Span,
    },
}

/// 语句类型
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    // let语句
    Let {
        name: String,
        pattern: Option<Box<Pattern>>,
        type_annotation: Option<Type>,
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
        is_pub: bool,
        span: Span,
        /// 泛型类型参数名称列表，如 enum Option<T> { ... } 中为 vec!["T"]
        type_params: Vec<String>,
    },

    // 结构体定义语句
    StructDef {
        name: String,
        fields: Vec<FieldDef>,
        is_pub: bool,
        span: Span,
        /// 泛型类型参数名称列表，如 struct Pair<T> { ... } 中为 vec!["T"]
        type_params: Vec<String>,
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
        return_type: Option<Type>,
        body: Expr,
        is_pub: bool,
        span: Span,
    },
}

/// 模式匹配的分支
#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub guard: Option<Box<Expr>>,
    pub body: Expr,
    pub span: Span,
}


/// 结构体字段模式 (用于 struct 解构)
#[derive(Debug, Clone, PartialEq)]
pub struct StructFieldPattern {
    /// 字段名（struct 定义中的名称）
    pub field: String,
    /// 绑定的模式（可以是 Variable、Wildcard、Number、Struct 等）
    pub pattern: Box<Pattern>,
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
        args: Vec<Pattern>,
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
        args: Vec<Pattern>,
        span: Span,
    },
    /// 结构体解构模式 (例如: Point { x, y }, Point { x: a, y: 0 })
    Struct {
        name: String,
        fields: Vec<StructFieldPattern>,
        span: Span,
    },
}

/// 类型定义中的变体
#[derive(Debug, Clone, PartialEq)]
pub struct TypeVariant {
    pub name: String,
    pub data_types: Vec<Type>,
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
    pub field_type: Type, // 结构化类型
    pub span: Span,
}

/// Lambda参数定义（支持类型注解）
#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    pub name: String,
    pub type_annotation: Option<Type>, // 结构化类型注解
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
    pub fn typed(name: String, type_annotation: Type) -> Self {
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
    Modulo,
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
            BinaryOperator::Modulo => write!(f, "%"),
            BinaryOperator::Equal => write!(f, "=="),
            BinaryOperator::NotEqual => write!(f, "!="),
            BinaryOperator::GreaterEqual => write!(f, ">="),
            BinaryOperator::LessEqual => write!(f, "<="),
            BinaryOperator::Greater => write!(f, ">"),
            BinaryOperator::Less => write!(f, "<"),
            BinaryOperator::LogicalAnd => write!(f, "&&"),
            BinaryOperator::LogicalOr => write!(f, "||"),
            BinaryOperator::BitAnd => write!(f, "&"),
            BinaryOperator::BitOr => write!(f, "|"),
            BinaryOperator::BitXor => write!(f, "^"),
            BinaryOperator::ShiftLeft => write!(f, "<<"),
            BinaryOperator::ShiftRight => write!(f, ">>"),
        }
    }
}

impl fmt::Display for UnaryOperator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UnaryOperator::Plus => write!(f, "+"),
            UnaryOperator::Minus => write!(f, "-"),
            UnaryOperator::LogicalNot => write!(f, "!"),
            UnaryOperator::BitNot => write!(f, "~"),
        }
    }
}

impl fmt::Display for Statement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Statement::Let { name, pattern, type_annotation, value, .. } => {
                match pattern {
                    Some(pat) => {
                        write!(f, "let {} = {};", format_pattern(pat), value)
                    }
                    None => {
                        if let Some(annot) = type_annotation {
                            write!(f, "let {}: {} = {};", name, annot, value)
                        } else {
                            write!(f, "let {} = {};", name, value)
                        }
                    }
                }
            }
            Statement::Expression { expr, .. } => write!(f, "{};", expr),
            Statement::TypeDef { name, variants, is_pub, .. } => {
                let pub_str = if *is_pub { "pub " } else { "" };
                let variants_str = variants
                    .iter()
                    .map(|v| {
                        if !v.data_types.is_empty() {
                            let types_str = v.data_types.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(", ");
                            format!("{}({})", v.name, types_str)
                        } else {
                            v.name.clone()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" | ");
                write!(f, "{}enum {} = {};", pub_str, name, variants_str)
            }
            Statement::StructDef { name, fields, is_pub, .. } => {
                let pub_str = if *is_pub { "pub " } else { "" };
                let fields_str = fields
                    .iter()
                    .map(|f| format!("{}: {}", f.name, f.field_type))
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "{}struct {} = {{ {} }};", pub_str, name, fields_str)
            }
            Statement::Assignment { target, value, .. } => {
                write!(f, "{} = {};", target, value)
            }
            Statement::FunctionDef {
                name,
                params,
                return_type,
                body,
                is_pub,
                ..
            } => {
                let pub_str = if *is_pub { "pub " } else { "" };
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
                write!(f, "{}fn {}({}){} {}", pub_str, name, params_str, ret_str, body)
            }
        }
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Number { value, .. } => write!(f, "{}", value),
            Expr::StringLiteral { value, .. } => write!(f, "\"{}\"", value),
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
            Expr::Lambda { params, body, return_type, .. } => {
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
                if let Some(ref ret) = return_type {
                    write!(f, "|{}| -> {} {}", params_str, ret, body)
                } else {
                    write!(f, "|{}| {}", params_str, body)
                }
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
                    let guard_str = if let Some(ref g) = arm.guard { format!(" if {}", g) } else { String::new() };
                    write!(f, "{}{} -> {}, ", format_pattern(&arm.pattern), guard_str, arm.body)?;
                }
                write!(f, "}}")
            }
            Expr::Constructor { name, args, .. } => {
                if !args.is_empty() {
                    let args_str = args.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(", ");
                    write!(f, "{}({})", name, args_str)
                } else {
                    write!(f, "{}", name)
                }
            }
            Expr::QualifiedConstructor {
                type_name,
                constructor_name,
                args,
                ..
            } => {
                if !args.is_empty() {
                    let args_str = args.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(", ");
                    write!(f, "{}::{}({})", type_name, constructor_name, args_str)
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
            Expr::ForIn {
                var, start, end, body, inclusive, ..
            } => {
                let range_op = if *inclusive { "..=" } else { ".." };
                write!(f, "for {} in {}{}{} {{ {} }}", var, start, range_op, end, body)
            }
            Expr::ForArray { var, array, body, .. } => {
                write!(f, "for {} in {} {{ {} }}", var, array, body)
            }
            Expr::Break { .. } => write!(f, "break"),
            Expr::Continue { .. } => write!(f, "continue"),
            Expr::Return { value, .. } => {
                if let Some(v) = value {
                    write!(f, "return {}", v)
                } else {
                    write!(f, "return")
                }
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
            Expr::Abs { value, .. } => {
                write!(f, "abs {}", value)
            }
            Expr::Min { left, right, .. } => {
                write!(f, "min({}, {})", left, right)
            }
            Expr::Max { left, right, .. } => {
                write!(f, "max({}, {})", left, right)
            }
            Expr::Clamp { value, min_val, max_val, .. } => {
                write!(f, "clamp({}, {}, {})", value, min_val, max_val)
            }
            Expr::StrIndex { string, index, .. } => {
                write!(f, "str_index({}, {})", string, index)
            }
            Expr::CharAt { string, index, .. } => {
                write!(f, "char_at({}, {})", string, index)
            }
            Expr::Substring { string, start, length, .. } => {
                write!(f, "substring({}, {}, {})", string, start, length)
            }
            Expr::StrContains { string, char_code, .. } => {
                write!(f, "str_contains({}, {})", string, char_code)
            }
            Expr::SplitCount { string, separator, .. } => {
                write!(f, "split_count({}, {})", string, separator)
            }
            Expr::Trim { string, .. } => {
                write!(f, "trim({})", string)
            }
            Expr::CharToString { expr, .. } => {
                write!(f, "char_to_string({})", expr)
            }
            Expr::ToString { expr, .. } => {
                write!(f, "to_string({})", expr)
            }
            Expr::TupleLiteral { elements, .. } => {
                let elems = elements
                    .iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "({})", elems)
            }
            Expr::TupleAccess { object, index, .. } => {
                write!(f, "{}.{}", object, index)
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
            Expr::RuntimeGlobal { name, .. } => {
                write!(f, "runtime_{}()", name)
            }
            Expr::GcRegOp { is_push, .. } => {
                if *is_push {
                    write!(f, "gc_push_regs()")
                } else {
                    write!(f, "gc_pop_regs()")
                }
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
            Expr::TypeCast { expr, target_type, .. } => {
                write!(f, "({} as {})", expr, target_type)
            }
        }
    }
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Number { span, .. } => *span,
            Expr::StringLiteral { span, .. } => *span,
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
            Expr::ForIn { span, .. } => *span,
            Expr::ForArray { span, .. } => *span,
            Expr::Break { span, .. } => *span,
            Expr::Continue { span, .. } => *span,
            Expr::Return { span, .. } => *span,
            Expr::StructLiteral { span, .. } => *span,
            Expr::FieldAccess { span, .. } => *span,
            Expr::ArrayLiteral { span, .. } => *span,
            Expr::Index { span, .. } => *span,
            Expr::ArrayLen { span, .. } => *span,
            Expr::Abs { span, .. } => *span,
            Expr::Min { span, .. } => *span,
            Expr::Max { span, .. } => *span,
            Expr::Clamp { span, .. } => *span,
            Expr::StrIndex { span, .. } => *span,
            Expr::CharAt { span, .. } => *span,
            Expr::Substring { span, .. } => *span,
            Expr::StrContains { span, .. } => *span,
            Expr::SplitCount { span, .. } => *span,
            Expr::Trim { span, .. } => *span,
            Expr::CharToString { span, .. } => *span,
            Expr::ToString { span, .. } => *span,
            Expr::TupleLiteral { span, .. } => *span,
            Expr::TupleAccess { span, .. } => *span,
            Expr::Reference { span, .. } => *span,
            Expr::Dereference { span, .. } => *span,
            Expr::HeapAllocate { span, .. } => *span,
            Expr::HeapFree { span, .. } => *span,
            Expr::Retain { span, .. } => *span,
            Expr::Release { span, .. } => *span,
            Expr::UnsafeLoad { span, .. } => *span,
            Expr::UnsafeStore { span, .. } => *span,
            Expr::RuntimeGlobal { span, .. } => *span,
            Expr::GcRegOp { span, .. } => *span,
            Expr::Assignment { span, .. } => *span,
            Expr::EffectPerform { span, .. } => *span,
            Expr::EffectResume { span, .. } => *span,
            Expr::EffectHandle { span, .. } => *span,
            Expr::TypeCast { span, .. } => *span,
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
            Pattern::Struct { span, .. } => *span,
        }
    }
}
