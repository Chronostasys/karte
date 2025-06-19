//! 寄存器分配的核心数据类型
//! 
//! 定义了寄存器分配过程中使用的主要数据结构，
//! 如分配结果、生命周期、溢出槽等。

use crate::RegisterId;
use std::collections::HashMap;
use std::any::Any;
use crate::pass::AnalysisResult;

/// 寄存器分配结果
/// 
/// 封装了一次寄存器分配操作的完整产出，包括
/// 虚拟到物理寄存器的映射、被溢出到栈的寄存器列表，
///以及相关的统计数据。
#[derive(Debug, Clone)]
pub struct RegisterAllocationResult {
    /// 虚拟寄存器到物理寄存器的映射
    pub register_mapping: HashMap<RegisterId, u8>,
    /// 溢出的寄存器及其逻辑栈槽信息
    pub spilled_registers: HashMap<RegisterId, SpillSlot>,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterLifetime {
    /// 虚拟寄存器ID
    pub register: RegisterId,
    /// 生命周期起始指令索引
    pub start: usize,
    /// 生命周期结束指令索引
    pub end: usize,
    /// 所有使用该寄存器的指令索引列表
    pub uses: Vec<usize>,
    /// 标记是否为函数参数
    pub is_function_parameter: bool,
    /// 如果是函数参数，记录其参数索引
    pub parameter_index: Option<usize>,
}

impl RegisterLifetime {
    /// 创建一个新的生命周期实例（主要用于测试）
    #[cfg(test)]
    pub fn new(register: RegisterId, start: usize, end: usize) -> Self {
        Self {
            register,
            start,
            end,
            uses: vec![],
            is_function_parameter: false,
            parameter_index: None,
        }
    }

    /// 检查两个生命周期是否重叠
    pub fn overlaps(&self, other: &Self) -> bool {
        self.start <= other.end && self.end >= other.start
    }

    /// 计算生命周期的长度
    pub fn length(&self) -> usize {
        self.end - self.start
    }
}

/// 调用约定接口（简化版）
/// 
/// 定义了函数调用时寄存器的使用规则，以避免循环依赖 `karte-common`。
#[derive(Debug, Clone)]
pub struct SimpleCallingConvention {
    /// 参数传递寄存器
    pub argument_registers: Vec<u8>,
    /// 返回值寄存器
    pub return_register: u8,
    /// 栈指针寄存器
    pub stack_pointer: u8,
    /// 帧指针寄存器
    pub frame_pointer: u8,
    /// 返回地址寄存器
    pub return_address: u8,
    /// 可被分配器使用的物理寄存器列表
    pub allocatable_registers: Vec<u8>,
}

impl Default for SimpleCallingConvention {
    fn default() -> Self {
        Self {
            argument_registers: vec![1, 2, 3, 4], // r1-r4
            return_register: 0,                   // r0
            stack_pointer: 6,                     // r6
            frame_pointer: 7,                     // r7
            return_address: 5,                    // r5
            allocatable_registers: vec![0, 1, 2, 3, 4], // r0-r4
        }
    }
}

impl SimpleCallingConvention {
    /// 检查给定ID的寄存器是否为特殊用途寄存器
    pub fn is_special_register(&self, reg_id: u8) -> bool {
        reg_id == self.stack_pointer ||
        reg_id == self.frame_pointer ||
        reg_id == self.return_address ||
        reg_id == self.return_register
    }

    /// 根据参数索引获取对应的物理寄存器
    pub fn get_argument_register(&self, index: usize) -> Option<u8> {
        self.argument_registers.get(index).copied()
    }
}


/// 寄存器分配运行模式
/// 
/// ## 设计说明
/// 寄存器分配被设计为两个阶段，以解耦 **分配决策** 和 **代码改写**。
/// 
/// 1. **`DecisionOnly` (Pre-RA)**:
///    此阶段仅分析LIR，计算所有虚拟寄存器的生命周期，并运行线性扫描算法
///    来决定哪些寄存器可以映射到物理寄存器，哪些需要溢出到栈上。
///    其结果（`RegisterAllocationResult`）被存储起来，但**不修改任何代码**。
/// 
/// 2. **`FinalRewrite` (Final-RA)**:
///    此阶段在 `StackFrameLowering` Pass 之后运行。`StackFrameLowering` 
///    会使用 Pre-RA 的决策来分配栈帧，并可能引入新的临时寄存器来处理溢出。
///    Final-RA 阶段会重新进行一次完整的分配，为所有寄存器（包括那些新的临时寄存器）
///    找到最终的物理寄存器，并重写LIR代码，将虚拟寄存器替换为物理寄存器。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterAllocationMode {
    /// 决策模式 (Pre-RA): 只做分析决策，不修改代码。
    DecisionOnly,
    /// 最终改写模式 (Final-RA): 执行最终的寄存器替换。
    FinalRewrite,
} 