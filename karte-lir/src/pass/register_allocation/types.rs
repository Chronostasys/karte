//! 寄存器分配的核心数据类型
//!
//! 定义了寄存器分配过程中使用的主要数据结构，
//! 如分配结果、生命周期、溢出槽等。

use crate::pass::AnalysisResult;
use crate::Register;
use std::any::Any;
use std::collections::HashMap;

/// 寄存器类型
///
/// 定义了虚拟寄存器的语义类型，用于指导寄存器分配策略
#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy)]
pub enum RegisterType {
    /// 普通数据寄存器 - 存储计算值，可以被溢出
    Data,
    /// 栈地址寄存器 - 存储栈地址，不能被溢出
    StackAddress,
    /// 函数参数寄存器 - 特殊处理，优先分配
    FunctionParameter,
    /// 特殊用途寄存器 - 如返回值、帧指针等，不能被重新分配
    Special,
}

impl RegisterType {
    /// 检查该类型的寄存器是否可以被溢出
    pub fn can_spill(&self) -> bool {
        matches!(self, RegisterType::Data)
    }

    /// 检查该类型的寄存器是否需要特殊处理
    pub fn needs_special_handling(&self) -> bool {
        matches!(
            self,
            RegisterType::StackAddress | RegisterType::FunctionParameter | RegisterType::Special
        )
    }

    /// 获取寄存器类型的字符串表示
    pub fn to_string(&self) -> &'static str {
        match self {
            RegisterType::Data => "Data",
            RegisterType::StackAddress => "StackAddress",
            RegisterType::FunctionParameter => "FunctionParameter",
            RegisterType::Special => "Special",
        }
    }
}

/// 寄存器分配结果
///
/// 封装了一次寄存器分配操作的完整产出，包括
/// 虚拟到物理寄存器的映射、被溢出到栈的寄存器列表，
///以及相关的统计数据。
#[derive(Debug, Clone)]
pub struct RegisterAllocationResult {
    /// 虚拟寄存器到物理寄存器的映射
    pub register_mapping: HashMap<Register, u8>,
    /// 溢出的寄存器及其逻辑栈槽信息
    pub spilled_registers: HashMap<Register, SpillSlot>,
    /// 🔧 新增：寄存器类型映射
    pub register_types: HashMap<Register, RegisterType>,
    /// 分配统计信息
    pub stats: AllocationStats,
}

impl AnalysisResult for RegisterAllocationResult {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// 溢出槽信息
///
/// 代表一个被溢出的虚拟寄存器在栈帧上的存储位置。
/// 只记录逻辑槽号，具体的栈偏移量由 `StackFrameLowering` Pass 计算。
#[derive(Debug, Clone)]
pub struct SpillSlot {
    pub slot_id: usize,
}

/// 分配统计信息
///
/// 用于记录和报告寄存器分配过程的性能和结果。
#[derive(Debug, Clone)]
pub struct AllocationStats {
    /// 处理的虚拟寄存器总数
    pub total_virtual_registers: usize,
    /// 成功分配到物理寄存器的数量
    pub allocated_physical_registers: usize,
    /// 被溢出到栈的寄存器数量
    pub spilled_registers: usize,
    /// 检测到的最大寄存器压力
    pub register_pressure: usize,
}

/// 寄存器生命周期
///
/// 表示一个虚拟寄存器从定义到最后一次使用的指令区间。
///
/// ## 精确生命周期 (live_ranges)
///
/// 对于复杂的控制流（如钻石形 CFG），一个寄存器的实际活跃范围可能是
/// 不连续的多个区间。`live_ranges` 字段提供了这种精确的活跃区间信息。
///
/// 例如，对于以下 CFG：
/// ```text
///      A (def reg1)
///     / \
///    B   C (use reg1)
///     \ /
///      D
/// ```
///
/// reg1 ��� `live_ranges` 应该只包含 A 和 C 块的指令，不包含 B 块。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterLifetime {
    /// 虚拟寄存器ID
    pub register: Register,
    /// 生命周期起始指令索引（保守估计：最小的活跃位置）
    pub start: usize,
    /// 生命周期结束指令索引（保守估计：最大的活跃位置）
    pub end: usize,
    /// 所有使用该寄存器的指令索引列表
    pub uses: Vec<usize>,
    /// 标记是否为函数参数
    pub is_function_parameter: bool,
    /// 如果是函数参数，记录其参数索引
    pub parameter_index: Option<usize>,
    /// 🔧 新增：寄存器类型
    pub register_type: RegisterType,
    /// 🔧 新增：精确的活跃区间列表
    ///
    /// 每个元素是一个 (start, end) 对，表示一个连续的活跃区间。
    /// 这些区间按起始位置排序，且不重叠。
    /// 如果为空，则使用 (self.start, self.end) 作为保守估计。
    pub live_ranges: Vec<(usize, usize)>,
}

impl RegisterLifetime {
    /// 创建一个新的生命周期实例（主要用于测试）
    #[cfg(test)]
    pub fn new(register: Register, start: usize, end: usize) -> Self {
        Self {
            register,
            start,
            end,
            uses: vec![],
            is_function_parameter: false,
            parameter_index: None,
            register_type: RegisterType::Data,
            live_ranges: vec![],
        }
    }

    /// 检查两个生命周期是否重叠
    ///
    /// 如果两个生命周期都有精确的 live_ranges，则使用精确比较；
    /// 否则使用保守的 [start, end] 区间比较。
    pub fn overlaps(&self, other: &Self) -> bool {
        // 如果两者都有精确的 live_ranges，使用精确比较
        if !self.live_ranges.is_empty() && !other.live_ranges.is_empty() {
            return self.overlaps_precise(other);
        }

        // 否则使用保守的区间比较
        self.start <= other.end && self.end >= other.start
    }

    /// 使用精确的 live_ranges 检查重叠
    fn overlaps_precise(&self, other: &Self) -> bool {
        // 检查任意两个区间是否重叠
        for &(s1, e1) in &self.live_ranges {
            for &(s2, e2) in &other.live_ranges {
                if s1 <= e2 && e1 >= s2 {
                    return true;
                }
            }
        }
        false
    }

    /// 检查是否在指定的指令位置活跃
    pub fn is_live_at(&self, instruction_index: usize) -> bool {
        // 如果有精确的 live_ranges，使用它
        if !self.live_ranges.is_empty() {
            return self
                .live_ranges
                .iter()
                .any(|&(s, e)| s <= instruction_index && instruction_index <= e);
        }

        // 否则使用保守的区间
        self.start <= instruction_index && instruction_index <= self.end
    }

    /// 计算生命周期的长度
    pub fn length(&self) -> usize {
        self.end - self.start
    }

    /// 检查该寄存器是否可以被溢出
    pub fn can_spill(&self) -> bool {
        self.register_type.can_spill()
    }
}

/// 调用约定接口（简化版）
///
/// 定义了函数调用时寄存器的使用规则，以避免循环依赖 `karte-common`。

pub type CallingConvention = karte_common::calling_convention::CallingConvention;
