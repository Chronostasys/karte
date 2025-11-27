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

/// 脚本模式入口点函数名
pub const SCRIPT_ENTRY_POINT: &str = "__script_entry__";

/// 变量绑定信息
#[derive(Clone)]
pub(crate) struct VariableBinding {
    /// 变量对应的MIR值
    pub(crate) value: Value,
    /// 所有权类型（如果适用）
    pub(crate) ownership: Option<OwnershipKind>,
    /// 是否已移动
    pub(crate) moved: bool,
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
}

/// HIR到MIR的lowering上下文
///
/// 维护lowering过程中的所有状态信息，包括：
/// - 当前正在处理的函数和基本块
/// - 变量作用域栈
/// - 错误信息收集
/// - 匿名函数计数器
/// - 外部函数声明
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
}
