/// 类型定义模块
///
/// 本模块包含HIR到MIR lowering过程中使用的所有类型定义：
/// - VariableBinding: 变量绑定信息（值、所有权、是否已移动）
/// - ScopeFrame: 作用域帧（变量绑定的映射和顺序）
/// - LoweringOptions: Lowering选项配置
/// - LoweringContext: Lowering上下文结构体定义
use crate::{BasicBlockId, MirProgram, Value};
use karte_common::memory::OwnershipKind;
use karte_hir::ModuleContext;
use std::collections::{HashMap, HashSet};

/// 循环上下文，记录 break/continue 的目标基本块
#[derive(Clone, Debug)]
pub(crate) struct LoopContext {
    /// continue 目标（循环条件检查块）
    pub(crate) continue_target: BasicBlockId,
    /// break 目标（循环退出块）
    pub(crate) break_target: BasicBlockId,
    /// 记录每次 continue 的来源块 ID 和当时的变量绑定
    pub(crate) continue_sources: Vec<(BasicBlockId, std::collections::HashMap<String, Value>)>,
    /// 记录每次 break 的来源块 ID 和当时的变量绑定
    pub(crate) break_sources: Vec<(BasicBlockId, std::collections::HashMap<String, Value>)>,
}

/// 脚本模式入口点函数名
pub const SCRIPT_ENTRY_POINT: &str = "__script_entry__";

/// 变量绑定信息
#[derive(Clone, Debug)]
pub(crate) struct VariableBinding {
    /// 变量对应的MIR值
    pub(crate) value: Value,
    /// 所有权类型（如果适用）
    pub(crate) ownership: Option<OwnershipKind>,
    /// 是否已移动
    pub(crate) moved: bool,
    /// 结构体类型名称（如果值是结构体类型）
    /// 用于闭包捕获时确定正确的堆分配大小
    pub(crate) struct_name: Option<String>,
}

/// 作用域帧
///
/// 表示一个词法作用域，包含该作用域内的所有变量绑定
#[derive(Clone)]
pub(crate) struct ScopeFrame {
    /// 变量名到绑定的映射
    pub(crate) bindings: HashMap<String, VariableBinding>,
    /// 变量声明顺序（用于正确的作用域退出时清理）
    pub(crate) order: Vec<String>,
}

impl ScopeFrame {
    pub(crate) fn new() -> Self {
        Self {
            bindings: HashMap::new(),
            order: Vec::new(),
        }
    }
}

/// Lowering选项配置
#[derive(Default, Clone)]
pub struct LoweringOptions {
    /// 已知的函数名集合（用于区分函数调用和变量引用）
    pub known_functions: HashSet<String>,
    /// 模块上下文（用于解析模块符号）
    pub module_context: Option<ModuleContext>,
    /// 表达式类型映射（从HIR type checker传递）
    /// 键是Expr的指针，值是推断出的类型
    pub expr_types: HashMap<usize, karte_hir::Type>,
}

/// HIR到MIR的lowering上下文
///
/// 维护lowering过程中的所有状态信息，包括：
/// - 当前正在处理的函数和基本块
/// - 变量作用域栈
/// - 错误信息收集
/// - 匿名函数计数器
/// - 外部函数声明
/// - 临时变量值追踪
/// - 函数返回类型追踪（用于正确处理返回函数的调用）
/// - 表达式类型信息（从HIR type checker传递）
pub struct LoweringContext<'a> {
    /// MIR程序（正在构建中）
    pub(crate) program: &'a mut MirProgram,
    /// 当前函数名称
    pub(crate) current_function_name: Option<String>,
    /// 当前基本块ID
    pub(crate) current_block: Option<BasicBlockId>,
    /// 变量作用域栈
    pub(crate) scopes: Vec<ScopeFrame>,
    /// 错误信息列表
    pub(crate) errors: Vec<String>,
    /// 匿名函数（lambda）计数器
    pub(crate) lambda_counter: usize,
    /// 通过import提前声明的外部函数
    pub(crate) external_functions: HashSet<String>,
    /// 模块上下文（用于解析模块符号）
    pub(crate) module_context: Option<ModuleContext>,
    /// 临时变量到实际值的映射（用于追踪函数值）
    /// 当临时变量被赋予 Value::Function 或 Value::Struct(Closure) 时记录
    pub(crate) temp_value_map: HashMap<crate::TempId, Value>,
    /// 函数名到其返回类型的映射
    /// 用于在函数调用时确定返回值是否为函数/闭包类型
    pub(crate) function_return_types: HashMap<String, karte_hir::Type>,
    /// 临时变量的类型映射
    pub(crate) temp_types: HashMap<crate::TempId, karte_hir::Type>,
    /// 表达式类型映射（从HIR type checker传递）
    pub(crate) expr_types: HashMap<usize, karte_hir::Type>,
    /// 预分析模式：while 循环预分析时不生成 Phi 节点
    /// 仅用于收集变量更新信息
    pub(crate) analysis_mode: bool,
    /// 循环上下文栈：break/continue 跳转目标
    pub(crate) loop_stack: Vec<LoopContext>,
    /// 返回跳转目标块（用于 return 语句）
    /// 当前函数的 epilogue 块，None 表示不在函数中或尚未创建
    pub(crate) return_target: Option<BasicBlockId>,
}
