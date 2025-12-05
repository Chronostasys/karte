//! 指针使用分析器
//!
//! 用于判断指针是否只在本地使用，以及验证指针生命周期安全性

use crate::error::Result;
use crate::graph::VariableGraph;
use crate::types::{EscapeState, VariableEscapeInfo, VariableId};
use karte_mir::{BasicBlockId, MirFunction};
use std::collections::HashMap;

/// 指针使用分析器
pub struct PointerUsageAnalyzer<'a> {
    /// 变量依赖图
    graph: &'a VariableGraph,

    /// 逃逸信息
    escape_info: &'a HashMap<VariableId, VariableEscapeInfo>,

    /// 当前分析的函数
    function: Option<&'a MirFunction>,
}

impl<'a> PointerUsageAnalyzer<'a> {
    /// 创建新的指针使用分析器
    pub fn new(
        graph: &'a VariableGraph,
        escape_info: &'a HashMap<VariableId, VariableEscapeInfo>,
    ) -> Self {
        Self {
            graph,
            escape_info,
            function: None,
        }
    }

    /// 设置当前分析的函数
    pub fn set_function(&mut self, function: &'a MirFunction) {
        self.function = Some(function);
    }

    /// 检查指针是否只在本地使用
    ///
    /// 一个指针被认为是"本地使用"的条件：
    /// 1. 指针从未被返回
    /// 2. 指针从未存储到堆对象
    /// 3. 指针只向下传递给不保存它的函数
    /// 4. 指针的最后使用 ≤ 被指向对象的生命周期结束
    pub fn is_pointer_local_only(&self, pointer_var: VariableId) -> bool {
        // 检查指针是否被返回
        if self.is_returned(pointer_var) {
            log::debug!("指针 {:?} 被返回，不是本地使用", pointer_var);
            return false;
        }

        // 检查指针是否存储到堆对象
        if self.is_stored_to_heap(pointer_var) {
            log::debug!("指针 {:?} 存储到堆对象，不是本地使用", pointer_var);
            return false;
        }

        // 检查指针是否传递给会保存它的函数
        if self.is_passed_to_storing_function(pointer_var) {
            log::debug!("指针 {:?} 传递给会保存它的函数，不是本地使用", pointer_var);
            return false;
        }

        log::debug!("指针 {:?} 只在本地使用", pointer_var);
        true
    }

    /// 检查指针生命周期是否安全
    ///
    /// 验证指针的生命周期不超过被指向对象的生命周期
    pub fn check_lifetime_safety(
        &self,
        pointer_var: VariableId,
        pointee_var: VariableId,
    ) -> bool {
        // 获取指针和被指向对象的元数据
        let pointer_meta = self.graph.get_node_metadata(&pointer_var);
        let pointee_meta = self.graph.get_node_metadata(&pointee_var);

        if let (Some(ptr_meta), Some(pte_meta)) = (pointer_meta, pointee_meta) {
            // 检查最后使用位置
            match (ptr_meta.last_use_block, pte_meta.last_use_block) {
                (Some(ptr_last_use), Some(pte_last_use)) => {
                    // 简化检查：如果指针的最后使用在被指向对象的最后使用之后，可能不安全
                    // 注意：这是一个保守的检查，实际的生命周期分析需要更复杂的控制流分析
                    if ptr_last_use.0 > pte_last_use.0 {
                        log::warn!(
                            "指针 {:?} 的最后使用 {:?} 晚于被指向对象 {:?} 的最后使用 {:?}",
                            pointer_var,
                            ptr_last_use,
                            pointee_var,
                            pte_last_use
                        );
                        return false;
                    }
                }
                _ => {
                    // 如果没有足够的元数据，保守地认为是安全的
                    log::debug!("缺少生命周期元数据，保守地认为安全");
                }
            }
        }

        true
    }

    /// 检查变量是否被返回
    fn is_returned(&self, var_id: VariableId) -> bool {
        if let Some(info) = self.escape_info.get(&var_id) {
            matches!(
                info.escape_state,
                EscapeState::ReturnEscape | EscapeState::GlobalEscape
            )
        } else {
            // 检查依赖图中的逃逸状态
            let state = self.graph.get_escape_state(&var_id);
            matches!(state, EscapeState::ReturnEscape | EscapeState::GlobalEscape)
        }
    }

    /// 检查变量是否存储到堆对象
    fn is_stored_to_heap(&self, var_id: VariableId) -> bool {
        if let Some(info) = self.escape_info.get(&var_id) {
            // 检查逃逸点中是否有堆存储
            info.escape_points.iter().any(|point| {
                matches!(
                    point,
                    crate::types::EscapePoint::HeapFieldStore { .. }
                        | crate::types::EscapePoint::ArrayStore { .. }
                        | crate::types::EscapePoint::GlobalAssignment { .. }
                )
            })
        } else {
            false
        }
    }

    /// 检查变量是否传递给会保存它的函数
    fn is_passed_to_storing_function(&self, var_id: VariableId) -> bool {
        if let Some(info) = self.escape_info.get(&var_id) {
            // 检查逃逸点中是否有函数参数传递，且该函数会保存参数
            info.escape_points.iter().any(|point| {
                if let crate::types::EscapePoint::FunctionArgument { function_id, .. } = point {
                    // TODO: 这里需要函数摘要来判断函数是否会保存参数
                    // 目前保守地假设所有函数调用都可能保存参数
                    // 在 Phase 2 实现函数摘要后，这里可以更精确
                    log::debug!(
                        "变量 {:?} 传递给函数 {:?}，保守地假设会被保存",
                        var_id,
                        function_id
                    );
                    true
                } else {
                    false
                }
            })
        } else {
            false
        }
    }

    /// 检查指针是否被闭包捕获
    pub fn is_captured_by_closure(&self, var_id: VariableId) -> bool {
        if let Some(info) = self.escape_info.get(&var_id) {
            info.escape_points
                .iter()
                .any(|point| matches!(point, crate::types::EscapePoint::ClosureCapture { .. }))
        } else {
            false
        }
    }

    /// 分析指针的使用模式
    ///
    /// 返回指针使用的详细信息
    pub fn analyze_pointer_usage(&self, pointer_var: VariableId) -> PointerUsagePattern {
        let is_local = self.is_pointer_local_only(pointer_var);
        let is_returned = self.is_returned(pointer_var);
        let is_stored = self.is_stored_to_heap(pointer_var);
        let is_captured = self.is_captured_by_closure(pointer_var);

        PointerUsagePattern {
            is_local_only: is_local,
            is_returned,
            is_stored_to_heap: is_stored,
            is_captured_by_closure: is_captured,
            can_stack_allocate: is_local && !is_returned && !is_stored && !is_captured,
        }
    }
}

/// 指针使用模式
#[derive(Debug, Clone)]
pub struct PointerUsagePattern {
    /// 是否只在本地使用
    pub is_local_only: bool,

    /// 是否被返回
    pub is_returned: bool,

    /// 是否存储到堆对象
    pub is_stored_to_heap: bool,

    /// 是否被闭包捕获
    pub is_captured_by_closure: bool,

    /// 是否可以栈分配
    pub can_stack_allocate: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::VariableGraph;

    #[test]
    fn test_pointer_usage_analyzer() {
        let graph = VariableGraph::new();
        let escape_info = HashMap::new();
        let analyzer = PointerUsageAnalyzer::new(&graph, &escape_info);

        let var_id = VariableId(1);

        // 默认情况下，没有逃逸信息的变量应该被认为是本地使用
        assert!(analyzer.is_pointer_local_only(var_id));
    }
}
