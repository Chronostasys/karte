//! 分配策略选择器
//!
//! 根据逃逸分析结果为变量选择最优的分配策略

use crate::types::{
    AllocationSuggestion as EscapeAllocationSuggestion, EscapeState, VariableEscapeInfo,
    VariableId as EscapeVariableId,
};
use std::collections::HashMap;

/// 分配策略
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AllocationStrategy {
    /// 栈分配
    Stack {
        /// 栈帧偏移（相对于帧指针）
        offset: isize,
        /// 分配大小
        size: usize,
        /// 对齐要求
        alignment: usize,
    },

    /// 堆分配（GC管理）
    Heap {
        /// 对象类型ID
        object_type: String,
        /// 分配大小
        size: usize,
        /// 是否需要GC跟踪
        gc_tracked: bool,
    },

    /// 内联分配（小对象优化）
    Inline {
        /// 内联大小（限制为32字节）
        size: usize,
    },

    /// 寄存器分配（临时变量）
    Register {
        /// 寄存器ID（可选，由寄存器分配器决定）
        register_hint: Option<usize>,
        /// 数据大小
        size: usize,
    },
}

impl AllocationStrategy {
    /// 检查是否可以栈分配
    pub fn is_stack_allocated(&self) -> bool {
        matches!(self, AllocationStrategy::Stack { .. })
    }

    /// 检查是否需要GC跟踪
    pub fn needs_gc_tracking(&self) -> bool {
        matches!(
            self,
            AllocationStrategy::Heap { gc_tracked: true, .. }
        )
    }

    /// 获取分配大小
    pub fn size(&self) -> usize {
        match self {
            AllocationStrategy::Stack { size, .. } => *size,
            AllocationStrategy::Heap { size, .. } => *size,
            AllocationStrategy::Inline { size } => *size,
            AllocationStrategy::Register { size, .. } => *size,
        }
    }
}

/// 分配策略选择器
pub struct AllocationStrategySelector {
    /// 逃逸分析结果
    escape_analysis: HashMap<EscapeVariableId, VariableEscapeInfo>,

    /// 栈帧布局
    stack_layout: StackFrameLayout,

    /// 当前函数名
    current_function: String,

    /// 变量大小估算
    variable_sizes: HashMap<String, usize>,

    /// 变量对齐要求
    variable_alignments: HashMap<String, usize>,
}

impl AllocationStrategySelector {
    /// 创建新的分配策略选择器
    pub fn new(escape_analysis: HashMap<EscapeVariableId, VariableEscapeInfo>) -> Self {
        Self {
            escape_analysis,
            stack_layout: StackFrameLayout::new(),
            current_function: String::new(),
            variable_sizes: HashMap::new(),
            variable_alignments: HashMap::new(),
        }
    }

    /// 设置当前函数
    pub fn set_current_function(&mut self, function_name: String) {
        self.current_function = function_name;
        self.stack_layout.reset();
    }

    /// 设置变量大小
    pub fn set_variable_size(&mut self, var_name: String, size: usize) {
        self.variable_sizes.insert(var_name, size);
    }

    /// 设置变量对齐
    pub fn set_variable_alignment(&mut self, var_name: String, alignment: usize) {
        self.variable_alignments.insert(var_name, alignment);
    }

    /// 为变量选择分配策略
    pub fn select_allocation_strategy(
        &mut self,
        var_id: &EscapeVariableId,
        var_name: &str,
    ) -> AllocationStrategy {
        // 获取逃逸分析信息（克隆以避免借用检查问题）
        if let Some(escape_info) = self.escape_analysis.get(var_id).cloned() {
            self.select_strategy_from_escape_info(&escape_info, var_name)
        } else {
            // 没有逃逸分析信息，默认堆分配
            log::warn!(
                "变量 {} ({:?}) 没有逃逸分析信息，使用默认堆分配",
                var_name,
                var_id
            );
            self.default_heap_allocation(var_name)
        }
    }

    /// 根据逃逸分析信息选择策略
    fn select_strategy_from_escape_info(
        &mut self,
        escape_info: &VariableEscapeInfo,
        var_name: &str,
    ) -> AllocationStrategy {
        match &escape_info.allocation_suggestion {
            EscapeAllocationSuggestion::StackAlloc => {
                // 栈分配：可以安全地分配在栈上
                let size = self.get_variable_size(var_name);
                let alignment = self.get_variable_alignment(var_name);
                let offset = self.stack_layout.allocate(size, alignment);

                AllocationStrategy::Stack {
                    offset,
                    size,
                    alignment,
                }
            }

            EscapeAllocationSuggestion::HeapAlloc => {
                // 堆分配：需要GC管理
                let size = self.get_variable_size(var_name);
                AllocationStrategy::Heap {
                    object_type: format!("{}::{}", self.current_function, var_name),
                    size,
                    gc_tracked: true,
                }
            }

            EscapeAllocationSuggestion::InlineAlloc => {
                // 内联分配：小对象优化
                let size = self.get_variable_size(var_name).min(32);
                AllocationStrategy::Inline { size }
            }

            EscapeAllocationSuggestion::RegisterAlloc => {
                // 寄存器分配：临时变量
                let size = self.get_variable_size(var_name);
                AllocationStrategy::Register {
                    register_hint: None,
                    size,
                }
            }
        }
    }

    /// 默认堆分配策略
    fn default_heap_allocation(&self, var_name: &str) -> AllocationStrategy {
        let size = self.get_variable_size(var_name);
        AllocationStrategy::Heap {
            object_type: format!("{}::{}", self.current_function, var_name),
            size,
            gc_tracked: true,
        }
    }

    /// 获取变量大小
    fn get_variable_size(&self, var_name: &str) -> usize {
        self.variable_sizes.get(var_name).copied().unwrap_or(8)
    }

    /// 获取变量对齐
    fn get_variable_alignment(&self, var_name: &str) -> usize {
        self.variable_alignments
            .get(var_name)
            .copied()
            .unwrap_or(8)
    }

    /// 获取栈帧布局
    pub fn stack_layout(&self) -> &StackFrameLayout {
        &self.stack_layout
    }

    /// 获取栈帧总大小
    pub fn stack_frame_size(&self) -> usize {
        self.stack_layout.total_size()
    }

    /// 生成分配统计
    pub fn generate_statistics(&self) -> AllocationStatistics {
        let mut stats = AllocationStatistics::default();

        for (var_id, escape_info) in &self.escape_analysis {
            stats.total_variables += 1;

            match escape_info.escape_state {
                EscapeState::NoEscape => {
                    stats.stack_allocated += 1;
                }
                EscapeState::ArgEscape => {
                    stats.arg_escape += 1;
                    // 参数逃逸可以栈分配
                    stats.stack_allocated += 1;
                }
                EscapeState::ReturnEscape => {
                    stats.heap_allocated += 1;
                }
                EscapeState::GlobalEscape => {
                    stats.heap_allocated += 1;
                }
            }

            // 统计分配建议
            match escape_info.allocation_suggestion {
                EscapeAllocationSuggestion::StackAlloc => {
                    stats.suggested_stack += 1;
                }
                EscapeAllocationSuggestion::HeapAlloc => {
                    stats.suggested_heap += 1;
                }
                EscapeAllocationSuggestion::InlineAlloc => {
                    stats.suggested_inline += 1;
                }
                EscapeAllocationSuggestion::RegisterAlloc => {
                    stats.suggested_register += 1;
                }
            }
        }

        stats
    }
}

/// 栈帧布局管理器
#[derive(Debug, Clone)]
pub struct StackFrameLayout {
    /// 当前栈偏移（从帧指针开始，向下增长）
    current_offset: isize,

    /// 最大栈偏移（栈帧大小）
    max_offset: isize,

    /// 栈槽信息
    slots: Vec<StackSlot>,

    /// 对齐要求
    alignment: usize,
}

impl StackFrameLayout {
    /// 创建新的栈帧布局
    pub fn new() -> Self {
        Self {
            current_offset: 0,
            max_offset: 0,
            slots: Vec::new(),
            alignment: 8, // 默认8字节对齐
        }
    }

    /// 重置栈帧布局
    pub fn reset(&mut self) {
        self.current_offset = 0;
        self.max_offset = 0;
        self.slots.clear();
    }

    /// 分配栈空间
    pub fn allocate(&mut self, size: usize, alignment: usize) -> isize {
        // 向对齐边界对齐
        let aligned_offset = align_down(self.current_offset, alignment as isize);

        // 分配空间（栈向下增长，所以减去size）
        let new_offset = aligned_offset - size as isize;

        // 记录栈槽
        let slot = StackSlot {
            offset: new_offset,
            size,
            alignment,
        };
        self.slots.push(slot);

        // 更新当前偏移和最大偏移
        self.current_offset = new_offset;
        if new_offset < self.max_offset {
            self.max_offset = new_offset;
        }

        // 更新全局对齐要求
        self.alignment = self.alignment.max(alignment);

        new_offset
    }

    /// 获取栈帧总大小（对齐到16字节边界）
    pub fn total_size(&self) -> usize {
        let size = (-self.max_offset) as usize;
        align_up(size, 16) // AArch64要求16字节对齐
    }

    /// 获取栈槽数量
    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }

    /// 获取所有栈槽
    pub fn slots(&self) -> &[StackSlot] {
        &self.slots
    }
}

impl Default for StackFrameLayout {
    fn default() -> Self {
        Self::new()
    }
}

/// 栈槽信息
#[derive(Debug, Clone)]
pub struct StackSlot {
    /// 栈偏移（相对于帧指针）
    pub offset: isize,

    /// 槽大小
    pub size: usize,

    /// 对齐要求
    pub alignment: usize,
}

/// 分配统计
#[derive(Debug, Clone, Default)]
pub struct AllocationStatistics {
    /// 总变量数
    pub total_variables: usize,

    /// 栈分配数量
    pub stack_allocated: usize,

    /// 堆分配数量
    pub heap_allocated: usize,

    /// 参数逃逸数量
    pub arg_escape: usize,

    /// 建议栈分配
    pub suggested_stack: usize,

    /// 建议堆分配
    pub suggested_heap: usize,

    /// 建议内联分配
    pub suggested_inline: usize,

    /// 建议寄存器分配
    pub suggested_register: usize,
}

impl AllocationStatistics {
    /// 计算栈分配百分比
    pub fn stack_allocation_percentage(&self) -> f64 {
        if self.total_variables == 0 {
            0.0
        } else {
            (self.stack_allocated as f64 / self.total_variables as f64) * 100.0
        }
    }

    /// 打印统计信息
    pub fn print_summary(&self) {
        println!("=== 分配策略统计 ===");
        println!("总变量数: {}", self.total_variables);
        println!("栈分配: {} ({:.1}%)", self.stack_allocated, self.stack_allocation_percentage());
        println!("堆分配: {}", self.heap_allocated);
        println!("参数逃逸: {}", self.arg_escape);
        println!("\n建议统计:");
        println!("  栈分配: {}", self.suggested_stack);
        println!("  堆分配: {}", self.suggested_heap);
        println!("  内联分配: {}", self.suggested_inline);
        println!("  寄存器分配: {}", self.suggested_register);
    }
}

/// 向上对齐
fn align_up(value: usize, alignment: usize) -> usize {
    (value + alignment - 1) & !(alignment - 1)
}

/// 向下对齐（支持负数，栈向下增长）
fn align_down(value: isize, alignment: isize) -> isize {
    let alignment = alignment.abs();
    let remainder = value.rem_euclid(alignment);
    if remainder == 0 {
        value
    } else {
        value - remainder
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stack_frame_layout() {
        let mut layout = StackFrameLayout::new();

        // 分配第一个变量（8字节）
        let offset1 = layout.allocate(8, 8);
        assert_eq!(offset1, -8);

        // 分配第二个变量（16字节）
        let offset2 = layout.allocate(16, 8);
        assert_eq!(offset2, -24);

        // 分配第三个变量（4字节，4字节对齐）
        let offset3 = layout.allocate(4, 4);
        assert_eq!(offset3, -28);

        // 检查栈帧大小（应该对齐到16字节）
        let total_size = layout.total_size();
        assert_eq!(total_size, 32); // 28对齐到16 = 32

        // 检查栈槽数量
        assert_eq!(layout.slot_count(), 3);
    }

    #[test]
    fn test_allocation_strategy_stack() {
        let mut escape_analysis = HashMap::new();
        let var_id = EscapeVariableId(0);

        let escape_info = VariableEscapeInfo {
            variable_id: var_id,
            escape_state: EscapeState::NoEscape,
            escape_points: Vec::new(),
            lifetime_constraints: Vec::new(),
            allocation_suggestion: EscapeAllocationSuggestion::StackAlloc,
            depends_on: Default::default(),
            depended_by: Default::default(),
        };

        escape_analysis.insert(var_id, escape_info);

        let mut selector = AllocationStrategySelector::new(escape_analysis);
        selector.set_current_function("test".to_string());
        selector.set_variable_size("x".to_string(), 8);
        selector.set_variable_alignment("x".to_string(), 8);

        let strategy = selector.select_allocation_strategy(&var_id, "x");

        match strategy {
            AllocationStrategy::Stack { offset, size, alignment } => {
                assert_eq!(offset, -8);
                assert_eq!(size, 8);
                assert_eq!(alignment, 8);
            }
            _ => panic!("Expected stack allocation"),
        }
    }

    #[test]
    fn test_allocation_strategy_heap() {
        let mut escape_analysis = HashMap::new();
        let var_id = EscapeVariableId(0);

        let escape_info = VariableEscapeInfo {
            variable_id: var_id,
            escape_state: EscapeState::ReturnEscape,
            escape_points: Vec::new(),
            lifetime_constraints: Vec::new(),
            allocation_suggestion: EscapeAllocationSuggestion::HeapAlloc,
            depends_on: Default::default(),
            depended_by: Default::default(),
        };

        escape_analysis.insert(var_id, escape_info);

        let mut selector = AllocationStrategySelector::new(escape_analysis);
        selector.set_current_function("test".to_string());
        selector.set_variable_size("y".to_string(), 16);

        let strategy = selector.select_allocation_strategy(&var_id, "y");

        match strategy {
            AllocationStrategy::Heap { object_type, size, gc_tracked } => {
                assert_eq!(object_type, "test::y");
                assert_eq!(size, 16);
                assert!(gc_tracked);
            }
            _ => panic!("Expected heap allocation"),
        }
    }

    #[test]
    fn test_allocation_statistics() {
        let mut escape_analysis = HashMap::new();

        // 添加不逃逸变量
        let var1 = EscapeVariableId(0);
        escape_analysis.insert(
            var1,
            VariableEscapeInfo {
                variable_id: var1,
                escape_state: EscapeState::NoEscape,
                escape_points: Vec::new(),
                lifetime_constraints: Vec::new(),
                allocation_suggestion: EscapeAllocationSuggestion::StackAlloc,
                depends_on: Default::default(),
                depended_by: Default::default(),
            },
        );

        // 添加返回逃逸变量
        let var2 = EscapeVariableId(1);
        escape_analysis.insert(
            var2,
            VariableEscapeInfo {
                variable_id: var2,
                escape_state: EscapeState::ReturnEscape,
                escape_points: Vec::new(),
                lifetime_constraints: Vec::new(),
                allocation_suggestion: EscapeAllocationSuggestion::HeapAlloc,
                depends_on: Default::default(),
                depended_by: Default::default(),
            },
        );

        let selector = AllocationStrategySelector::new(escape_analysis);
        let stats = selector.generate_statistics();

        assert_eq!(stats.total_variables, 2);
        assert_eq!(stats.stack_allocated, 1);
        assert_eq!(stats.heap_allocated, 1);
        assert_eq!(stats.stack_allocation_percentage(), 50.0);
    }

    #[test]
    fn test_align_functions() {
        assert_eq!(align_up(10, 8), 16);
        assert_eq!(align_up(16, 8), 16);
        assert_eq!(align_up(17, 8), 24);

        assert_eq!(align_down(-10, -8), -16);
        assert_eq!(align_down(-16, -8), -16);
        assert_eq!(align_down(-17, -8), -24);
    }
}
