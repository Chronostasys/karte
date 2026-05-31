//! LIR降低过程的类型定义
//!
//! 本模块包含LIR降低过程中使用的核心数据结构。

use crate::{tagged_union::TaggedUnionManager, Instruction, LabelId, LirFunction, StructLayout};
use karte_mir::BasicBlockId;
use std::collections::{HashMap, HashSet};

use crate::Register;

/// MIR到LIR的lowering上下文（简化版本）
///
/// 遵循Stack-First策略：
/// - 简化值映射逻辑，移除复杂的HashMap
/// - 所有变量都先分配到栈上
/// - 让Memory2Reg Pass来决定哪些可以优化到寄存器
pub struct LirLoweringContext {
    /// 当前LIR函数
    pub(super) current_function: Option<LirFunction>,
    /// MIR基本块到标签的映射
    pub(super) block_to_label: HashMap<BasicBlockId, LabelId>,
    /// 函数名到标签的映射
    pub(super) function_labels: HashMap<String, LabelId>,
    /// 函数名到规范符号的映射
    pub(super) function_symbols: HashMap<String, String>,
    /// 当前函数的规范符号
    pub(super) current_function_symbol: Option<String>,
    /// 当前函数的参数列表
    pub(super) current_function_params: Vec<String>,
    /// 每个函数内部生成标签的计数器
    pub(super) label_seed: u64,
    /// 待处理的指令
    pub(super) pending_instructions: Vec<Instruction>,
    /// 错误信息
    pub(super) errors: Vec<String>,
    /// Tagged Union管理器
    pub(super) tagged_union_manager: TaggedUnionManager,
    /// 栈分配追踪（统一的值存储策略）
    pub(super) stack_allocations: HashMap<String, Register>,
    /// 🔧 专业修复：全局结构体类型信息
    pub(super) global_struct_types: HashMap<String, StructLayout>,
    /// 跟踪持有特定结构体布局的值（包括临时值与变量）
    pub(super) struct_value_layouts: HashMap<String, StructLayout>,
    /// 代数效应：handler入口块的参数名映射（用于在块标签处把payload写入变量）
    pub(super) handler_block_param: HashMap<BasicBlockId, String>,
    /// 已知常量值追踪：记录被赋值为常量的变量/临时变量
    /// key = value_to_key(value), value = 常量值
    /// 用于在 BinaryOp 等指令中直接使用立即数，避免通过栈加载导致的寄存器分配冲突
    pub(super) known_constants: HashMap<String, i64>,
    /// 当前函数中，通过 return 语句返回的临时变量 ID 集合
    /// 用于在 LIR lowering 时判断 struct 是否需要堆分配
    pub(super) returned_temp_ids: HashSet<usize>,
    /// 强制下一个 struct 分配使用堆（由逃逸分析触发）
    /// 当 struct 值被赋给一个将通过 return 返回的 temp 时设置此标志
    pub(super) force_struct_heap: bool,
}
