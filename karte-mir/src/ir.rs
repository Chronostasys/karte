use karte_diagnostics::Span;
use std::collections::HashMap;

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
    Constructor {
        name: String,
        arg: Option<Box<Value>>,
    },
    /// 限定构造器值
    QualifiedConstructor {
        type_name: String,
        constructor_name: String,
        arg: Option<Box<Value>>,
    },
    /// 结构体值
    Struct {
        name: String,
        fields: std::collections::BTreeMap<String, Value>,
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
        target: Value,
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
    /// 堆分配语句
    HeapAlloc {
        target: Value,
        size: usize,
        object_type: String,
        span: Span,
    },
    /// Phi 节点 - SSA 形式中的值选择
    Phi {
        target: Value,
        incoming: Vec<(BasicBlockId, Value)>, // (前驱块ID, 值)
        span: Span,
    },
    // 代数效应（MIR占位，不进入后端）：
    EffectPerform {
        tag: Value,
        payload: Value,
        target: Option<Value>,
        span: Span,
    },
    EffectResume {
        value: Value,
        span: Span,
    },

    /// 安装效应处理器：在当前点生效，直到对应的 Pop
    EffectHandlerPush {
        tag: Value,
        /// 处理器所在的基本块（同一函数内的一个块，非独立函数）
        handler_block: BasicBlockId,
        /// 处理器参数名（在 handler_block 内可见，绑定到 payload 寄存器约定）
        param_name: String,
        span: Span,
    },

    /// 卸载效应处理器（与最近的 Push 匹配）
    EffectHandlerPop { span: Span },
}

/// 终结语句 - 控制基本块的跳转
#[derive(Debug, Clone, PartialEq)]
pub enum Terminator {
    /// 无条件跳转
    Goto { target: BasicBlockId, span: Span },
    /// 条件跳转
    Branch {
        condition: Value,
        then_block: BasicBlockId,
        else_block: BasicBlockId,
        span: Span,
    },
    /// 返回
    Return { value: Option<Value>, span: Span },
    /// 匹配跳转
    Match {
        value: Value,
        arms: Vec<MatchArm>,
        default: Option<BasicBlockId>,
        span: Span,
    },
    // /// 代数效应：Resume 终结当前基本块并跳转回 Perform 的继续点
    // EffectResume { value: Value, span: Span },
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

/// SSA 形式的值定义信息
#[derive(Debug, Clone, PartialEq)]
pub struct ValueDefinition {
    /// 定义这个值的语句
    pub defining_statement: Option<usize>, // 在基本块中的语句索引
    /// 定义这个值的基本块
    pub defining_block: BasicBlockId,
    /// 值的版本号（SSA 中每个值只被定义一次）
    pub version: usize,
}

/// 基本块（原始版本，保持向后兼容性）
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

/// SSA 形式的基本块
#[derive(Debug, Clone, PartialEq)]
pub struct SsaBlock {
    pub id: BasicBlockId,
    pub statements: Vec<Statement>,
    pub terminator: Option<Terminator>,
    /// 前驱块
    pub predecessors: Vec<BasicBlockId>,
    /// 后继块
    pub successors: Vec<BasicBlockId>,
    /// 这个块中定义的值
    pub definitions: HashMap<String, ValueDefinition>,
    /// 这个块需要的 Phi 节点
    pub phi_nodes: Vec<Statement>,
}

impl SsaBlock {
    pub fn new(id: BasicBlockId) -> Self {
        Self {
            id,
            statements: Vec::new(),
            terminator: None,
            predecessors: Vec::new(),
            successors: Vec::new(),
            definitions: HashMap::new(),
            phi_nodes: Vec::new(),
        }
    }

    /// 从 BasicBlock 转换为 SsaBlock
    pub fn from_basic_block(block: BasicBlock) -> Self {
        Self {
            id: block.id,
            statements: block.statements,
            terminator: block.terminator,
            predecessors: Vec::new(),
            successors: Vec::new(),
            definitions: HashMap::new(),
            phi_nodes: Vec::new(),
        }
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

/// 结构体字段定义 (MIR级别)
#[derive(Debug, Clone, PartialEq)]
pub struct MirStructField {
    pub name: String,
    pub field_type: String, // 简化的类型名称
}

/// 结构体类型定义 (MIR级别)
#[derive(Debug, Clone, PartialEq)]
pub struct MirStructType {
    pub name: String,
    pub fields: Vec<MirStructField>,
}

/// MIR程序
#[derive(Debug, Clone, PartialEq)]
pub struct MirProgram {
    pub functions: HashMap<String, MirFunction>,
    pub main_function: Option<String>,
    pub main_return_value: Option<Value>,
    pub temp_values: HashMap<TempId, Value>,
    /// 结构体类型定义
    pub struct_types: HashMap<String, MirStructType>,
}

impl Default for MirProgram {
    fn default() -> Self {
        Self::new()
    }
}

impl MirProgram {
    pub fn new() -> Self {
        Self {
            functions: HashMap::new(),
            main_function: None,
            main_return_value: None,
            temp_values: HashMap::new(),
            struct_types: HashMap::new(),
        }
    }

    pub fn add_struct_type(&mut self, struct_type: MirStructType) {
        self.struct_types
            .insert(struct_type.name.clone(), struct_type);
    }

    pub fn get_struct_type(&self, name: &str) -> Option<&MirStructType> {
        self.struct_types.get(name)
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
