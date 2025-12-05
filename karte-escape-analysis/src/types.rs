//! 逃逸分析核心数据类型

use karte_diagnostics::Span;
use karte_mir::BasicBlockId;
use std::collections::HashSet;

/// 函数标识符
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionId(pub String);

/// 变量标识符
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VariableId(pub usize);

/// 逃逸状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EscapeState {
    /// 不逃逸 - 可以栈分配
    NoEscape,

    /// 参数逃逸 - 传递给函数参数，但不返回
    ArgEscape,

    /// 返回逃逸 - 从函数返回，生命周期超出当前函数
    ReturnEscape,

    /// 全局逃逸 - 存储到全局变量或堆对象中
    GlobalEscape,
}

impl EscapeState {
    /// 判断是否可以栈分配
    pub fn can_stack_allocate(&self) -> bool {
        matches!(self, EscapeState::NoEscape)
    }

    /// 合并两个逃逸状态（取更严格的）
    pub fn merge(&self, other: &EscapeState) -> EscapeState {
        match (self, other) {
            (EscapeState::GlobalEscape, _) | (_, EscapeState::GlobalEscape) => {
                EscapeState::GlobalEscape
            }
            (EscapeState::ReturnEscape, _) | (_, EscapeState::ReturnEscape) => {
                EscapeState::ReturnEscape
            }
            (EscapeState::ArgEscape, _) | (_, EscapeState::ArgEscape) => EscapeState::ArgEscape,
            _ => EscapeState::NoEscape,
        }
    }
}

/// 变量逃逸信息
#[derive(Debug, Clone)]
pub struct VariableEscapeInfo {
    /// 变量ID
    pub variable_id: VariableId,

    /// 逃逸状态
    pub escape_state: EscapeState,

    /// 逃逸点列表
    pub escape_points: Vec<EscapePoint>,

    /// 生命周期约束
    pub lifetime_constraints: Vec<LifetimeConstraint>,

    /// 分配建议
    pub allocation_suggestion: AllocationSuggestion,

    /// 依赖的变量
    pub depends_on: HashSet<VariableId>,

    /// 被依赖的变量
    pub depended_by: HashSet<VariableId>,
}

impl VariableEscapeInfo {
    /// 创建默认的不逃逸变量信息
    pub fn new_no_escape(variable_id: VariableId) -> Self {
        Self {
            variable_id,
            escape_state: EscapeState::NoEscape,
            escape_points: Vec::new(),
            lifetime_constraints: Vec::new(),
            allocation_suggestion: AllocationSuggestion::StackAlloc,
            depends_on: HashSet::new(),
            depended_by: HashSet::new(),
        }
    }

    /// 标记为逃逸
    pub fn mark_escape(&mut self, escape_state: EscapeState, escape_point: EscapePoint) {
        self.escape_state = self.escape_state.merge(&escape_state);
        self.escape_points.push(escape_point);

        // 更新分配建议
        if !self.escape_state.can_stack_allocate() {
            self.allocation_suggestion = AllocationSuggestion::HeapAlloc;
        }
    }

    /// 添加依赖关系
    pub fn add_dependency(&mut self, depends_on_var: VariableId) {
        self.depends_on.insert(depends_on_var);
    }

    /// 添加被依赖关系
    pub fn add_dependent(&mut self, dependent_var: VariableId) {
        self.depended_by.insert(dependent_var);
    }
}

/// 逃逸点 - 变量逃逸的具体位置
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EscapePoint {
    /// 函数参数传递
    FunctionArgument {
        function_id: FunctionId,
        argument_index: usize,
        call_site: Span,
    },

    /// 函数返回
    FunctionReturn {
        function_id: FunctionId,
        return_site: Span,
    },

    /// 闭包捕获
    ClosureCapture {
        closure_id: String,
        capture_index: usize,
        capture_site: Span,
    },

    /// 赋值给全局变量
    GlobalAssignment { global_name: String, site: Span },

    /// 存储到堆对象的字段中
    HeapFieldStore {
        object_var: VariableId,
        field_name: String,
        store_site: Span,
    },

    /// 存储到数组/切片中
    ArrayStore {
        array_var: VariableId,
        store_site: Span,
    },

    /// 取地址操作
    AddressOf {
        var_id: VariableId,
        address_site: Span,
    },
}

/// 生命周期约束
#[derive(Debug, Clone)]
pub struct LifetimeConstraint {
    /// 约束类型
    pub constraint_type: ConstraintType,

    /// 约束位置
    pub location: Span,
}

/// 约束类型
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConstraintType {
    /// 变量 a 的生命周期必须不超过变量 b
    /// a outlives b: lifetime(a) <= lifetime(b)
    Outlives {
        shorter: VariableId,
        longer: VariableId,
    },

    /// 变量 a 和 b 具有相同生命周期
    SameLifetime {
        var_a: VariableId,
        var_b: VariableId,
    },

    /// 变量的生命周期受函数调用限制
    CallLifetime {
        variable: VariableId,
        function_id: FunctionId,
        min_lifetime: LifetimeBound,
    },
}

/// 生命周期边界
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifetimeBound {
    /// 当前函数作用域
    FunctionScope(FunctionId),

    /// 基本块作用域
    BlockScope(BasicBlockId),

    /// 静态生命周期（全局）
    Static,
}

/// 分配建议
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AllocationSuggestion {
    /// 栈分配（推荐）
    StackAlloc,

    /// 堆分配（通过 GC）
    HeapAlloc,

    /// 寄存器分配（临时变量）
    RegisterAlloc,

    /// 内联分配（编译时常量）
    InlineAlloc,
}

impl AllocationSuggestion {
    /// 是否需要 GC 管理
    pub fn needs_gc(&self) -> bool {
        matches!(self, AllocationSuggestion::HeapAlloc)
    }
}
