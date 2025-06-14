use karte_diagnostics::Span;
use std::collections::HashMap;
use std::fmt;

/// 基本块标识符
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BasicBlockId(pub usize);

/// 临时变量标识符
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TempId(pub usize);

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
        arg: Option<Box<Value>>,
    },
    /// 结构体值
    Struct {
        name: String,
        fields: std::collections::HashMap<String, Value>,
    },
    /// 函数值
    Function { name: String },
    /// 闭包值（包含函数名和捕获的值）
    Closure {
        function_name: String,
        captured_values: Vec<Value>,
    },
    /// 引用值
    Reference { value: Box<Value> },
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
    /// 字段访问语句
    FieldAccess {
        target: Value,
        object: Value,
        field: String,
        span: Span,
    },
    /// 解引用语句
    Dereference {
        target: Value,
        reference: Value,
        span: Span,
    },
    /// 构造器参数提取语句
    ConstructorArgExtract {
        target: Value,
        constructor: Value,
        arg_index: usize,
        span: Span,
    },
    /// 字段赋值语句
    FieldAssign {
        object: Value,
        field: String,
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
    pub main_return_value: Option<Value>,
    pub temp_values: HashMap<TempId, Value>,
}

impl MirProgram {
    pub fn new() -> Self {
        Self {
            functions: HashMap::new(),
            main_function: None,
            main_return_value: None,
            temp_values: HashMap::new(),
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
    // 逻辑运算符
    And,
    Or,
}

/// 一元运算符
#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOperator {
    Plus,
    Minus,
    Not,
} 