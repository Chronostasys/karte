pub fn add(left: u64, right: u64) -> u64 {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}

use karte_diagnostics::Span;
use std::collections::HashMap;
use std::fmt;

pub mod lower;

/// 基本块标识符
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BasicBlockId(pub usize);

impl fmt::Display for BasicBlockId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bb{}", self.0)
    }
}

/// 临时变量标识符
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TempId(pub usize);

impl fmt::Display for TempId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "_t{}", self.0)
    }
}

/// MIR值 - 可以是变量、常量或临时值
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// 变量引用
    Variable { name: String },
    /// 数字常量
    Number { value: i64 },
    /// 布尔常量
    Boolean { value: bool },
    /// 单元值
    Unit,
    /// 临时变量
    Temp { id: TempId },
    /// 构造器值
    Constructor { name: String, arg: Option<Box<Value>> },
    /// 限定构造器值
    QualifiedConstructor { 
        type_name: String, 
        constructor_name: String, 
        arg: Option<Box<Value>> 
    },
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Variable { name } => write!(f, "{}", name),
            Value::Number { value } => write!(f, "{}", value),
            Value::Boolean { value } => write!(f, "{}", if *value { "true" } else { "false" }),
            Value::Unit => write!(f, "()"),
            Value::Temp { id } => write!(f, "{}", id),
            Value::Constructor { name, arg } => {
                if let Some(arg) = arg {
                    write!(f, "{}({})", name, arg)
                } else {
                    write!(f, "{}", name)
                }
            }
            Value::QualifiedConstructor { type_name, constructor_name, arg } => {
                if let Some(arg) = arg {
                    write!(f, "{}::{}({})", type_name, constructor_name, arg)
                } else {
                    write!(f, "{}::{}", type_name, constructor_name)
                }
            }
        }
    }
}

/// MIR语句 - 低级操作
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    /// 赋值语句
    Assign {
        target: Value,
        source: Value,
        span: Span,
    },
    /// 二元运算
    BinaryOp {
        target: Value,
        left: Value,
        op: BinaryOperator,
        right: Value,
        span: Span,
    },
    /// 一元运算
    UnaryOp {
        target: Value,
        op: UnaryOperator,
        operand: Value,
        span: Span,
    },
    /// 函数调用
    Call {
        target: Option<Value>,
        function: Value,
        args: Vec<Value>,
        span: Span,
    },
    /// 存储语句（用于赋值）
    Store {
        target: String,
        value: Value,
        span: Span,
    },
}

/// 终结语句 - 控制基本块的跳转
#[derive(Debug, Clone, PartialEq)]
pub enum Terminator {
    /// 无条件跳转
    Goto {
        target: BasicBlockId,
        span: Span,
    },
    /// 条件跳转
    Branch {
        condition: Value,
        then_block: BasicBlockId,
        else_block: BasicBlockId,
        span: Span,
    },
    /// 返回
    Return {
        value: Option<Value>,
        span: Span,
    },
    /// 匹配跳转
    Match {
        value: Value,
        arms: Vec<MatchArm>,
        default: Option<BasicBlockId>,
        span: Span,
    },
}

/// 匹配臂
#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub target: BasicBlockId,
}

/// 模式（简化版）
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    /// 通配符
    Wildcard,
    /// 变量绑定
    Variable { name: String },
    /// 构造器模式
    Constructor { name: String, arg: Option<String> },
    /// 数字模式
    Number { value: i64 },
    /// 布尔模式
    Boolean { value: bool },
}

/// 基本块
#[derive(Debug, Clone, PartialEq)]
pub struct BasicBlock {
    pub id: BasicBlockId,
    pub statements: Vec<Statement>,
    pub terminator: Option<Terminator>,
}

impl BasicBlock {
    pub fn new(id: BasicBlockId) -> Self {
        Self {
            id,
            statements: Vec::new(),
            terminator: None,
        }
    }
    
    pub fn add_statement(&mut self, stmt: Statement) {
        self.statements.push(stmt);
    }
    
    pub fn set_terminator(&mut self, terminator: Terminator) {
        self.terminator = Some(terminator);
    }
}

/// MIR函数
#[derive(Debug, Clone, PartialEq)]
pub struct MirFunction {
    pub name: String,
    pub params: Vec<String>,
    pub basic_blocks: HashMap<BasicBlockId, BasicBlock>,
    pub entry_block: BasicBlockId,
    pub next_block_id: usize,
    pub next_temp_id: usize,
}

impl MirFunction {
    pub fn new(name: String, params: Vec<String>) -> Self {
        let entry_block = BasicBlockId(0);
        let mut basic_blocks = HashMap::new();
        basic_blocks.insert(entry_block, BasicBlock::new(entry_block));
        
        Self {
            name,
            params,
            basic_blocks,
            entry_block,
            next_block_id: 1,
            next_temp_id: 0,
        }
    }
    
    pub fn new_block(&mut self) -> BasicBlockId {
        let id = BasicBlockId(self.next_block_id);
        self.next_block_id += 1;
        self.basic_blocks.insert(id, BasicBlock::new(id));
        id
    }
    
    pub fn new_temp(&mut self) -> TempId {
        let id = TempId(self.next_temp_id);
        self.next_temp_id += 1;
        id
    }
    
    pub fn get_block_mut(&mut self, id: BasicBlockId) -> Option<&mut BasicBlock> {
        self.basic_blocks.get_mut(&id)
    }
    
    pub fn get_block(&self, id: BasicBlockId) -> Option<&BasicBlock> {
        self.basic_blocks.get(&id)
    }
}

/// MIR程序
#[derive(Debug, Clone, PartialEq)]
pub struct MirProgram {
    pub functions: HashMap<String, MirFunction>,
    pub main_function: Option<String>,
}

impl MirProgram {
    pub fn new() -> Self {
        Self {
            functions: HashMap::new(),
            main_function: None,
        }
    }
    
    pub fn add_function(&mut self, function: MirFunction) {
        self.functions.insert(function.name.clone(), function);
    }
    
    pub fn set_main(&mut self, name: String) {
        self.main_function = Some(name);
    }
}

/// 二元运算符
#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
    Equal,
    NotEqual,
    LessThan,
    LessEqual,
    GreaterThan,
    GreaterEqual,
}

/// 一元运算符
#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOperator {
    Plus,
    Minus,
    Not,
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
            BinaryOperator::LessThan => write!(f, "<"),
            BinaryOperator::LessEqual => write!(f, "<="),
            BinaryOperator::GreaterThan => write!(f, ">"),
            BinaryOperator::GreaterEqual => write!(f, ">="),
        }
    }
}

impl fmt::Display for UnaryOperator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UnaryOperator::Plus => write!(f, "+"),
            UnaryOperator::Minus => write!(f, "-"),
            UnaryOperator::Not => write!(f, "!"),
        }
    }
}

impl fmt::Display for MirFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "fn {}({}):", self.name, self.params.join(", "))?;
        
        // 按ID顺序显示基本块
        let mut blocks: Vec<_> = self.basic_blocks.values().collect();
        blocks.sort_by_key(|b| b.id.0);
        
        for block in blocks {
            writeln!(f, "  {}:", block.id)?;
            for stmt in &block.statements {
                writeln!(f, "    {}", format_statement(stmt))?;
            }
            if let Some(term) = &block.terminator {
                writeln!(f, "    {}", format_terminator(term))?;
            }
        }
        Ok(())
    }
}

fn format_statement(stmt: &Statement) -> String {
    match stmt {
        Statement::Assign { target, source, .. } => {
            format!("{} = {}", target, source)
        }
        Statement::BinaryOp { target, left, op, right, .. } => {
            format!("{} = {} {} {}", target, left, op, right)
        }
        Statement::UnaryOp { target, op, operand, .. } => {
            format!("{} = {}{}", target, op, operand)
        }
        Statement::Call { target, function, args, .. } => {
            if let Some(target) = target {
                format!("{} = {}({})", target, function, args.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(", "))
            } else {
                format!("{}({})", function, args.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(", "))
            }
        }
        Statement::Store { target, value, .. } => {
            format!("{} := {}", target, value)
        }
    }
}

fn format_terminator(term: &Terminator) -> String {
    match term {
        Terminator::Goto { target, .. } => {
            format!("goto {}", target)
        }
        Terminator::Branch { condition, then_block, else_block, .. } => {
            format!("branch {} ? {} : {}", condition, then_block, else_block)
        }
        Terminator::Return { value, .. } => {
            if let Some(value) = value {
                format!("return {}", value)
            } else {
                "return".to_string()
            }
        }
        Terminator::Match { value, arms, default, .. } => {
            let arms_str = arms.iter()
                .map(|arm| format!("{:?} => {}", arm.pattern, arm.target))
                .collect::<Vec<_>>()
                .join(", ");
            if let Some(default) = default {
                format!("match {} {{ {} _ => {} }}", value, arms_str, default)
            } else {
                format!("match {} {{ {} }}", value, arms_str)
            }
        }
    }
}
