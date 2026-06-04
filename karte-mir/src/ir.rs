use karte_common::memory::OwnershipKind;
use karte_diagnostics::Span;
use karte_hir::types::Type;
use karte_ir_codec::parse::{body_field, keyword};
use karte_ir_codec::{IrDisplay, IrParse, ParseError, ParseResult};
use karte_ir_derive::IrCodec;
use nom::IResult;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fmt;

/// 基本块标识符
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, IrCodec, PartialOrd, Ord)]
#[ir_codec(token = "bb")]
pub struct BasicBlockId(#[ir_codec(args)] pub usize);

impl Default for BasicBlockId {
    fn default() -> Self {
        BasicBlockId(0)
    }
}

/// 临时变量标识符
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, IrCodec, Default)]
#[ir_codec(token = "%")]
pub struct TempId(#[ir_codec(args)] pub usize);

/// MIR值 - 可以是变量、常量或临时值
#[derive(Debug, Clone, PartialEq, IrCodec, Default)]
pub enum Value {
    /// 变量引用
    #[ir_codec(token = "var")]
    Variable {
        #[ir_codec(args)]
        name: String,
        #[ir_codec(skip)]
        ty: Option<Type>,
    },

    /// 数字常量
    #[ir_codec(token = "num")]
    Number {
        #[ir_codec(args)]
        value: i64,
        #[ir_codec(skip)]
        ty: Option<Type>,
    },

    /// 布尔常量
    #[ir_codec(token = "bool")]
    Boolean {
        #[ir_codec(args)]
        value: bool,
        #[ir_codec(skip)]
        ty: Option<Type>,
    },

    /// 字符串字面量
    #[ir_codec(token = "str")]
    StringLiteral {
        #[ir_codec(args)]
        value: String,
        #[ir_codec(skip)]
        ty: Option<Type>,
    },

    /// 单元值
    #[ir_codec(token = "()")]
    #[default]
    Unit,

    /// 临时变量 (直接显示为 %)
    Temp {
        #[ir_codec(args)]
        id: TempId,
        #[ir_codec(skip)]
        ty: Option<Type>,
    },

    /// 构造器值
    Constructor {
        name: String,
        args: Vec<Value>,
        #[ir_codec(skip)]
        ty: Option<Type>,
    },

    /// 限定构造器值
    QualifiedConstructor {
        type_name: String,
        constructor_name: String,
        args: Vec<Value>,
        #[ir_codec(skip)]
        ty: Option<Type>,
    },

    /// 结构体值
    Struct {
        name: String,
        fields: std::collections::BTreeMap<String, Value>,
        #[ir_codec(skip)]
        ty: Option<Type>,
    },

    /// 函数值
    #[ir_codec(token = "fn")]
    Function {
        #[ir_codec(args)]
        name: String,
        #[ir_codec(skip)]
        ty: Option<Type>,
    },

    /// 闭包值（包含函数名和捕获的值）
    Closure {
        function_name: String,
        captured_values: Vec<Value>,
        #[ir_codec(skip)]
        ty: Option<Type>,
    },

    /// 引用值
    #[ir_codec(token = "&")]
    Reference {
        #[ir_codec(args)]
        value: Box<Value>,
        #[ir_codec(skip)]
        ty: Option<Type>,
    },
}

/// 堆对象逃逸级别
#[derive(Debug, Clone, Copy, PartialEq, Eq, IrCodec)]
pub enum EscapeState {
    /// 对象仅在当前基本块/作用域内可见，可在优化时提升到栈
    Local,
    /// 对象通过返回值或参数逃逸到调用方
    Return,
    /// 对象存入闭包/全局/堆结构，需要长期存在
    Global,
}

/// MIR 中对堆布局和安全属性的抽象
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeapLayout {
    pub type_id: String,
    pub size: usize,
    pub align: usize,
    pub mutable: bool,
    pub escape: EscapeState,
    pub ownership: OwnershipKind,
}

impl IrDisplay for HeapLayout {
    fn ir_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HeapLayout")?;
        write!(f, "\n    type_id: ")?;
        self.type_id.ir_fmt(f)?;
        write!(f, "\n    size: ")?;
        self.size.ir_fmt(f)?;
        write!(f, "\n    align: ")?;
        self.align.ir_fmt(f)?;
        write!(f, "\n    mutable: ")?;
        self.mutable.ir_fmt(f)?;
        write!(f, "\n    escape: ")?;
        self.escape.ir_fmt(f)?;
        write!(f, "\n    ownership: ")?;
        self.ownership.ir_fmt(f)?;
        Ok(())
    }
}

impl IrParse for HeapLayout {
    fn parse_ir(input: &str) -> ParseResult<Self> {
        let (_, layout) = Self::parse_nom(input).map_err(ParseError::from)?;
        Ok(layout)
    }

    fn parse_nom(input: &str) -> IResult<&str, Self> {
        let (input, _) = keyword("HeapLayout")(input)?;
        let (input, type_id) = body_field("type_id", String::parse_nom)(input)?;
        let (input, size) = body_field("size", usize::parse_nom)(input)?;
        let (input, align) = body_field("align", usize::parse_nom)(input)?;
        let (input, mutable) = body_field("mutable", bool::parse_nom)(input)?;
        let (input, escape) = body_field("escape", EscapeState::parse_nom)(input)?;
        let (input, ownership) = body_field("ownership", OwnershipKind::parse_nom)(input)?;

        Ok((
            input,
            HeapLayout {
                type_id,
                size,
                align,
                mutable,
                escape,
                ownership,
            },
        ))
    }
}

/// GC 根的分类信息
#[derive(Debug, Clone, PartialEq, Eq, IrCodec)]
pub enum GcRootKind {
    /// 栈上的根：编译器通过 stack map 管理
    StackSlot { slot: usize },
    /// 静态/全局对象
    Global { symbol: String },
    /// 运行时自定义根（例如 native 代码注册）
    Custom { label: String },
}

/// 操作数的类型信息，用于 struct/enum 的值比较
#[derive(Debug, Clone, PartialEq)]
pub enum OperandType {
    /// 结构体类型：逐字段比较
    Struct { name: String, field_count: usize },
    /// 标签联合体类型（枚举）：先比较 tag，再比较 data
    TaggedUnion,
}

/// MIR语句 - 低级操作
#[derive(Debug, Clone, PartialEq, IrCodec)]
pub enum Statement {
    /// 赋值语句
    #[ir_codec(token = "=", infix)]
    Assign {
        #[ir_codec(args, left)]
        target: Value,
        #[ir_codec(args, right)]
        source: Value,
        #[ir_codec(skip)]
        span: Span,
    },
    /// 二元运算 (显示为: target = left op right)
    #[ir_codec(binop)]
    BinaryOp {
        #[ir_codec(args, target)]
        target: Value,
        #[ir_codec(args, left)]
        left: Value,
        #[ir_codec(args, op)]
        op: BinaryOperator,
        #[ir_codec(args, right)]
        right: Value,
        #[ir_codec(skip)]
        operand_type: Option<OperandType>,
        #[ir_codec(skip)]
        span: Span,
    },
    /// 一元运算
    #[ir_codec(token = "unop", unop)]
    UnaryOp {
        #[ir_codec(args, target)]
        target: Value,
        #[ir_codec(args, op)]
        op: UnaryOperator,
        #[ir_codec(args, operand)]
        operand: Value,
        #[ir_codec(skip)]
        span: Span,
    },
    /// 类型转换 (显示为: target = cast source, dst_bits, signed)
    #[ir_codec(token = "cast")]
    TypeCast {
        #[ir_codec(args, target)]
        target: Value,
        #[ir_codec(args)]
        source: Value,
        #[ir_codec(args)]
        dst_bits: u8,
        #[ir_codec(args)]
        signed: bool,
        #[ir_codec(skip)]
        span: Span,
    },
    /// 函数调用
    #[ir_codec(token = "call")]
    Call {
        #[ir_codec(args, target)]
        target: Option<Value>,
        #[ir_codec(args)]
        function: Value,
        #[ir_codec(args)]
        args: Vec<Value>,
        #[ir_codec(skip)]
        span: Span,
    },
    /// 存储语句（用于赋值）
    Store {
        target: Value,
        value: Value,
        span: Span,
    },
    /// 字段访问语句 (显示为: target = object.field)
    #[ir_codec(token = "fieldaccess", fieldaccess)]
    FieldAccess {
        #[ir_codec(args, target)]
        target: Value,
        #[ir_codec(args, object)]
        object: Value,
        #[ir_codec(args, field_name)]
        field: String,
        #[ir_codec(skip)]
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
    /// 通用堆分配语句
    #[ir_codec(token = "alloc")]
    Allocate {
        #[ir_codec(args, target)]
        target: Value,
        #[ir_codec(args)]
        layout: HeapLayout,
        #[ir_codec(skip)]
        span: Span,
    },
    /// 通用堆释放语句
    #[ir_codec(token = "dealloc")]
    Deallocate {
        #[ir_codec(args)]
        pointer: Value,
        #[ir_codec(args)]
        layout: HeapLayout,
        #[ir_codec(skip)]
        span: Span,
    },
    /// 引用计数/资源持有增加
    #[ir_codec(token = "retain")]
    Retain {
        #[ir_codec(args)]
        value: Value,
        #[ir_codec(skip)]
        span: Span,
    },
    /// 引用计数/资源释放
    #[ir_codec(token = "release")]
    Release {
        #[ir_codec(args)]
        value: Value,
        #[ir_codec(skip)]
        span: Span,
    },
    /// 标记一个值为 GC 根
    #[ir_codec(token = "mark_gc_root")]
    MarkGcRoot {
        #[ir_codec(args)]
        value: Value,
        #[ir_codec(args)]
        root: GcRootKind,
        #[ir_codec(skip)]
        span: Span,
    },
    /// 写屏障（便于未来并发/增量 GC）
    #[ir_codec(token = "write_barrier")]
    WriteBarrier {
        #[ir_codec(args)]
        object: Value,
        /// 用于调试的字段标签，可选
        #[ir_codec(args)]
        slot: Option<String>,
        #[ir_codec(args)]
        value: Value,
        #[ir_codec(skip)]
        span: Span,
    },
    /// 读屏障
    #[ir_codec(token = "read_barrier")]
    ReadBarrier {
        #[ir_codec(args, target)]
        target: Value,
        #[ir_codec(args)]
        object: Value,
        /// 用于调试的字段标签，可选
        #[ir_codec(args)]
        slot: Option<String>,
        #[ir_codec(skip)]
        span: Span,
    },
    /// 堆分配语句
    HeapAlloc {
        target: Value,
        size: usize,
        object_type: String,
        span: Span,
    },
    /// 栈分配语句 - 基于逃逸分析的栈上分配
    #[ir_codec(token = "stackalloc")]
    StackAllocate {
        #[ir_codec(args, target)]
        target: Value,
        #[ir_codec(args)]
        size: usize,
        #[ir_codec(args)]
        alignment: usize,
        #[ir_codec(skip)]
        offset: Option<isize>,
        #[ir_codec(skip)]
        span: Span,
    },
    /// unsafe 内存读取 - 从任意地址读取指定字节大小
    #[ir_codec(token = "unsafe_load")]
    UnsafeLoad {
        #[ir_codec(args, target)]
        target: Value,
        #[ir_codec(args)]
        addr: Value,
        #[ir_codec(args)]
        byte_size: u8,
        #[ir_codec(skip)]
        span: Span,
    },
    /// unsafe 内存写入 - 向任意地址写入指定字节大小
    #[ir_codec(token = "unsafe_store")]
    UnsafeStore {
        #[ir_codec(args)]
        addr: Value,
        #[ir_codec(args)]
        value: Value,
        #[ir_codec(args)]
        byte_size: u8,
        #[ir_codec(skip)]
        span: Span,
    },
    /// runtime 内建函数 - 读取 runtime 全局变量
    #[ir_codec(token = "runtime_global")]
    RuntimeGlobal {
        #[ir_codec(args, target)]
        target: Value,
        #[ir_codec(args)]
        global_name: String,
        #[ir_codec(skip)]
        span: Span,
    },
    /// GC 寄存器保存/恢复 - 把所有 callee-saved 寄存器 dump 到虚拟栈
    #[ir_codec(token = "gc_reg_op")]
    GcRegOp {
        #[ir_codec(args, target)]
        target: Value,
        #[ir_codec(args)]
        is_push: bool, // true = push, false = pop
        #[ir_codec(skip)]
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
    EffectHandlerPop {
        span: Span,
    },
}

/// 终结语句 - 控制基本块的跳转
#[derive(Debug, Clone, PartialEq, IrCodec)]
pub enum Terminator {
    /// 无条件跳转
    #[ir_codec(token = "goto")]
    Goto {
        #[ir_codec(args)]
        target: BasicBlockId,
        #[ir_codec(skip)]
        span: Span,
    },

    /// 条件跳转
    #[ir_codec(token = "if")]
    Branch {
        #[ir_codec(args)]
        condition: Value,
        #[ir_codec(label = "then")]
        then_block: BasicBlockId,
        #[ir_codec(label = "else")]
        else_block: BasicBlockId,
        #[ir_codec(skip)]
        span: Span,
    },

    /// 返回
    #[ir_codec(token = "ret")]
    Return {
        #[ir_codec(args)]
        value: Option<Value>,
        #[ir_codec(skip)]
        span: Span,
    },

    /// 匹配跳转
    #[ir_codec(token = "match")]
    Match {
        #[ir_codec(args)]
        value: Value,
        arms: Vec<MatchArm>,
        default: Option<BasicBlockId>,
        #[ir_codec(skip)]
        span: Span,
    },
    // /// 代数效应：Resume 终结当前基本块并跳转回 Perform 的继续点
    // EffectResume { value: Value, span: Span },
}

/// 匹配臂
#[derive(Debug, Clone, PartialEq, IrCodec)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub target: BasicBlockId,
}

/// 模式（简化版）
#[derive(Debug, Clone, PartialEq, IrCodec)]
pub enum Pattern {
    /// 通配符
    Wildcard,
    /// 变量绑定
    Variable { name: String },
    /// 构造器模式
    Constructor { name: String, args: Vec<Pattern> },
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
#[derive(Debug, Clone, PartialEq, IrCodec)]
pub struct BasicBlock {
    pub id: BasicBlockId,
    #[ir_codec(body, label = "statements")]
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
#[derive(Debug, Clone, PartialEq, IrCodec)]
pub struct MirFunction {
    pub name: String,
    #[ir_codec(label = "params")]
    pub params: Vec<String>,
    /// 参数类型
    #[ir_codec(skip)]
    pub param_types: Vec<Type>,
    /// 返回类型
    #[ir_codec(skip)]
    pub return_type: Option<Type>,
    #[ir_codec(body, label = "blocks")]
    pub basic_blocks: BTreeMap<BasicBlockId, BasicBlock>,
    #[ir_codec(skip)]
    pub entry_block: BasicBlockId,
    #[ir_codec(skip)]
    pub next_block_id: usize,
    #[ir_codec(skip)]
    pub next_temp_id: usize,
}

impl MirFunction {
    pub fn new(name: String, params: Vec<String>) -> Self {
        let entry_block = BasicBlockId(0);
        let mut basic_blocks = BTreeMap::new();
        basic_blocks.insert(entry_block, BasicBlock::new(entry_block));

        Self {
            name,
            params,
            param_types: Vec::new(),
            return_type: None,
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

    pub fn remove_block(&mut self, id: BasicBlockId) {
        self.basic_blocks.remove(&id);
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
#[derive(Debug, Clone, PartialEq, IrCodec)]
pub struct MirStructField {
    pub name: String,
    pub field_type: String, // 简化的类型名称
}

/// 结构体类型定义 (MIR级别)
#[derive(Debug, Clone, PartialEq, IrCodec)]
pub struct MirStructType {
    pub name: String,
    pub fields: Vec<MirStructField>,
}

/// MIR程序
#[derive(Debug, Clone, PartialEq, IrCodec)]
#[ir_codec(program)]
pub struct MirProgram {
    #[ir_codec(body)]
    pub functions: HashMap<String, MirFunction>,
    pub main_function: Option<String>,
    #[ir_codec(extra)]
    pub main_return_value: Option<Value>,
    #[ir_codec(extra)]
    pub temp_values: HashMap<TempId, Value>,
    /// 结构体类型定义
    #[ir_codec(extra)]
    pub struct_types: HashMap<String, MirStructType>,
    #[ir_codec(skip)]
    pub function_symbols: HashMap<String, String>,
    #[ir_codec(skip)]
    pub external_function_symbols: HashMap<String, String>,
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
            function_symbols: HashMap::new(),
            external_function_symbols: HashMap::new(),
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

    pub fn set_function_symbol<S: Into<String>>(&mut self, name: &str, symbol: S) {
        self.function_symbols
            .insert(name.to_string(), symbol.into());
    }

    pub fn function_symbol(&self, name: &str) -> Option<&str> {
        self.function_symbols.get(name).map(|s| s.as_str())
    }

    pub fn set_external_function_symbol<S: Into<String>>(&mut self, alias: &str, symbol: S) {
        self.external_function_symbols
            .insert(alias.to_string(), symbol.into());
    }

    pub fn external_function_symbols(&self) -> &HashMap<String, String> {
        &self.external_function_symbols
    }
}

/// 二元运算符
#[derive(Debug, Clone, PartialEq, IrCodec)]
pub enum BinaryOperator {
    #[ir_codec(token = "+")]
    Add,
    #[ir_codec(token = "-")]
    Subtract,
    #[ir_codec(token = "*")]
    Multiply,
    #[ir_codec(token = "/")]
    Divide,
    #[ir_codec(token = "%")]
    Modulo,
    #[ir_codec(token = "==")]
    Equal,
    #[ir_codec(token = "!=")]
    NotEqual,
    #[ir_codec(token = "<")]
    LessThan,
    #[ir_codec(token = "<=")]
    LessEqual,
    #[ir_codec(token = ">")]
    GreaterThan,
    #[ir_codec(token = ">=")]
    GreaterEqual,
    // 逻辑运算符
    #[ir_codec(token = "&&")]
    And,
    #[ir_codec(token = "||")]
    Or,
    // 位运算符
    #[ir_codec(token = "&")]
    BitAnd,
    #[ir_codec(token = "|")]
    BitOr,
    #[ir_codec(token = "^")]
    BitXor,
    #[ir_codec(token = "<<")]
    ShiftLeft,
    #[ir_codec(token = ">>")]
    ShiftRight,
}

/// 一元运算符
#[derive(Debug, Clone, PartialEq, IrCodec)]
pub enum UnaryOperator {
    #[ir_codec(token = "+")]
    Plus,
    #[ir_codec(token = "-")]
    Minus,
    #[ir_codec(token = "!")]
    Not,
    #[ir_codec(token = "~")]
    BitNot,
}
