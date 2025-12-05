//! 变量依赖图
//!
//! 用于跟踪变量之间的依赖关系，支持逃逸状态的传播

use crate::error::{EscapeAnalysisError, Result};
use crate::types::{EscapeState, VariableId};
use std::collections::{HashMap, HashSet, VecDeque};

/// 依赖边类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyEdge {
    /// 直接赋值: x = y
    DirectAssignment,

    /// 字段访问: x = y.field
    FieldAccess { field_name_hash: u64 },

    /// 数组访问: x = y[i]
    ArrayAccess,

    /// 函数参数传递: f(y) where y flows to x
    FunctionArgument,

    /// 函数返回值: x = f()
    FunctionReturn,

    /// 闭包捕获: closure captures y
    ClosureCapture,

    /// 存储到堆: y.field = x
    HeapStore,
}

/// 边权重：追踪指针"深度"变化
/// 用于Go风格的逃逸分析，追踪取地址和解引用操作
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeWeight {
    /// -1: 取地址操作（增加指针层级）
    /// 例如: ptr = &value，从value到ptr的权重是AddressOf
    AddressOf,

    /// 0: 直接赋值，指针深度不变
    /// 例如: y = x，权重是Identity
    Identity,

    /// +1: 解引用操作（减少指针层级）
    /// 例如: value = *ptr，从ptr到value的权重是Dereference
    Dereference,
}

/// 带权重的依赖边
/// 结合边类型和权重，用于精确的逃逸分析
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeightedEdge {
    /// 边类型
    pub edge_type: DependencyEdge,

    /// 边权重
    pub weight: EdgeWeight,
}

/// 节点元数据
/// 存储变量节点的额外信息，用于更精确的逃逸分析
#[derive(Debug, Clone)]
pub struct NodeMetadata {
    /// 指针深度（0=值，1=&value，-1=*ptr，2=&&value等）
    pub pointer_depth: i32,

    /// 是否是函数参数
    pub is_parameter: bool,

    /// 定义位置的基本块ID
    pub definition_block: Option<karte_mir::BasicBlockId>,

    /// 最后使用位置的基本块ID
    pub last_use_block: Option<karte_mir::BasicBlockId>,
}

impl NodeMetadata {
    /// 创建默认的节点元数据
    pub fn new() -> Self {
        Self {
            pointer_depth: 0,
            is_parameter: false,
            definition_block: None,
            last_use_block: None,
        }
    }

    /// 创建参数节点的元数据
    pub fn new_parameter() -> Self {
        Self {
            pointer_depth: 0,
            is_parameter: true,
            definition_block: None,
            last_use_block: None,
        }
    }
}

impl Default for NodeMetadata {
    fn default() -> Self {
        Self::new()
    }
}

/// 变量依赖图
#[derive(Debug, Default)]
pub struct VariableGraph {
    /// 前向边: 变量 -> 依赖它的变量
    /// 例如: x = y, 则 y -> x (y 有一条边指向 x)
    forward_edges: HashMap<VariableId, Vec<(VariableId, WeightedEdge)>>,

    /// 后向边: 变量 -> 它依赖的变量
    /// 例如: x = y, 则 x -> y (x 依赖 y)
    backward_edges: HashMap<VariableId, Vec<(VariableId, WeightedEdge)>>,

    /// 变量的逃逸状态缓存
    escape_cache: HashMap<VariableId, EscapeState>,

    /// 节点元数据：存储每个变量的额外信息
    node_metadata: HashMap<VariableId, NodeMetadata>,
}

impl VariableGraph {
    /// 创建新的变量依赖图
    pub fn new() -> Self {
        Self::default()
    }

    /// 添加依赖边: from -> to
    /// 表示 `to` 依赖于 `from` (例如: to = from)
    pub fn add_edge(&mut self, from: VariableId, to: VariableId, edge_type: DependencyEdge) {
        // 使用默认权重 Identity
        let weighted_edge = WeightedEdge {
            edge_type,
            weight: EdgeWeight::Identity,
        };
        self.add_weighted_edge(from, to, weighted_edge);
    }

    /// 添加带权重的依赖边: from -> to
    /// 这是新的核心方法，支持追踪指针深度变化
    pub fn add_weighted_edge(&mut self, from: VariableId, to: VariableId, edge: WeightedEdge) {
        // 前向边: from -> to
        self.forward_edges
            .entry(from)
            .or_insert_with(Vec::new)
            .push((to, edge));

        // 后向边: to -> from
        self.backward_edges
            .entry(to)
            .or_insert_with(Vec::new)
            .push((from, edge));

        // 清除逃逸状态缓存
        self.escape_cache.remove(&from);
        self.escape_cache.remove(&to);

        // 更新指针深度（如果元数据存在）
        if let (Some(from_meta), Some(to_meta)) = (
            self.node_metadata.get(&from).cloned(),
            self.node_metadata.get_mut(&to),
        ) {
            // 根据边权重计算目标节点的指针深度
            match edge.weight {
                EdgeWeight::AddressOf => {
                    // ptr = &value: ptr的深度 = value的深度 + 1
                    to_meta.pointer_depth = from_meta.pointer_depth + 1;
                }
                EdgeWeight::Identity => {
                    // y = x: y的深度 = x的深度
                    to_meta.pointer_depth = from_meta.pointer_depth;
                }
                EdgeWeight::Dereference => {
                    // value = *ptr: value的深度 = ptr的深度 - 1
                    to_meta.pointer_depth = from_meta.pointer_depth - 1;
                }
            }
        }
    }

    /// 添加简单赋值依赖: target = source
    pub fn add_assignment(&mut self, source: VariableId, target: VariableId) {
        self.add_edge(source, target, DependencyEdge::DirectAssignment);
    }

    /// 添加字段访问依赖: target = source.field
    pub fn add_field_access(&mut self, source: VariableId, target: VariableId, field_name: &str) {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        field_name.hash(&mut hasher);
        let field_name_hash = hasher.finish();

        self.add_edge(
            source,
            target,
            DependencyEdge::FieldAccess { field_name_hash },
        );
    }

    /// 添加数组访问依赖: target = source[index]
    pub fn add_array_access(&mut self, source: VariableId, target: VariableId) {
        self.add_edge(source, target, DependencyEdge::ArrayAccess);
    }

    /// 添加函数参数依赖
    pub fn add_function_argument(&mut self, arg: VariableId, param: VariableId) {
        self.add_edge(arg, param, DependencyEdge::FunctionArgument);
    }

    /// 添加函数返回依赖
    pub fn add_function_return(&mut self, returned: VariableId, receiver: VariableId) {
        self.add_edge(returned, receiver, DependencyEdge::FunctionReturn);
    }

    /// 添加闭包捕获依赖
    pub fn add_closure_capture(&mut self, captured: VariableId, closure_var: VariableId) {
        self.add_edge(captured, closure_var, DependencyEdge::ClosureCapture);
    }

    /// 添加堆存储依赖: object.field = value
    pub fn add_heap_store(&mut self, object: VariableId, value: VariableId) {
        self.add_edge(value, object, DependencyEdge::HeapStore);
    }

    /// 获取变量的所有前向依赖（依赖该变量的其他变量）
    pub fn get_dependents(&self, var_id: &VariableId) -> Vec<(VariableId, WeightedEdge)> {
        self.forward_edges
            .get(var_id)
            .cloned()
            .unwrap_or_default()
    }

    /// 获取变量的所有后向依赖（该变量依赖的其他变量）
    pub fn get_dependencies(&self, var_id: &VariableId) -> Vec<(VariableId, WeightedEdge)> {
        self.backward_edges
            .get(var_id)
            .cloned()
            .unwrap_or_default()
    }

    /// 传播逃逸状态
    /// 当一个变量被标记为逃逸时，应该沿着依赖链反向传播
    /// 例如：如果 y = x 且 y 逃逸，则 x 也应该逃逸
    pub fn propagate_escape(
        &mut self,
        var_id: VariableId,
        escape_state: EscapeState,
    ) -> Result<Vec<VariableId>> {
        let mut affected_vars = Vec::new();
        let mut work_queue = VecDeque::new();
        let mut visited = HashSet::new();

        // 初始化工作队列
        work_queue.push_back((var_id, escape_state));

        while let Some((current_var, current_state)) = work_queue.pop_front() {
            // 更新逃逸状态缓存
            let old_state = self
                .escape_cache
                .get(&current_var)
                .copied()
                .unwrap_or(EscapeState::NoEscape);
            let new_state = old_state.merge(&current_state);

            // 避免重复访问 - 但要在更新状态后检查
            // 这样可以确保首次访问时会进行传播
            let is_first_visit = visited.insert(current_var);

            // 如果状态改变或者是首次访问，进行传播
            let should_propagate = (new_state != old_state) || is_first_visit;

            if new_state != old_state {
                self.escape_cache.insert(current_var, new_state);
                affected_vars.push(current_var);
            }

            if should_propagate {
                // 反向传播：传播到该变量依赖的变量（后向边）
                // 因为如果 y = x 且 y 逃逸，则 x 也应该逃逸
                let dependencies = self.get_dependencies(&current_var);
                log::debug!("  {:?} 的后向依赖（依赖的变量）: {:?}", current_var, dependencies);
                for (dependency_var, edge) in dependencies {
                    let propagated_state = self.compute_backward_propagated_state(new_state, edge);
                    log::debug!("    -> 反向传播到 {:?}: {:?}", dependency_var, propagated_state);
                    work_queue.push_back((dependency_var, propagated_state));
                }

                // 前向传播：传播到依赖该变量的变量（前向边）
                // 因为如果 x 逃逸且 y = x，则 y 也应该逃逸
                let dependents = self.get_dependents(&current_var);
                log::debug!("  {:?} 的前向依赖（被依赖的变量）: {:?}", current_var, dependents);
                for (dependent_var, edge) in dependents {
                    let propagated_state = self.compute_forward_propagated_state(new_state, edge);
                    log::debug!("    -> 前向传播到 {:?}: {:?}", dependent_var, propagated_state);
                    work_queue.push_back((dependent_var, propagated_state));
                }
            }
        }

        Ok(affected_vars)
    }

    /// 计算反向传播后的逃逸状态（沿着依赖链向后传播）
    /// 例如：y = x, 如果 y 逃逸，x 也应该逃逸
    fn compute_backward_propagated_state(
        &self,
        source_state: EscapeState,
        edge: WeightedEdge,
    ) -> EscapeState {
        // 根据边权重调整传播逻辑
        match edge.weight {
            EdgeWeight::AddressOf => {
                // y = &x, 如果 y 逃逸，x 也逃逸
                // 因为y持有x的地址，y逃逸意味着x的地址逃逸
                source_state
            }
            EdgeWeight::Dereference => {
                // y = *x, 如果 y 逃逸，x 不一定逃逸
                // 因为只是读取x指向的值，不影响x本身
                EscapeState::NoEscape
            }
            EdgeWeight::Identity => {
                // 按边类型处理
                self.compute_backward_propagated_state_by_edge_type(source_state, edge.edge_type)
            }
        }
    }

    /// 根据边类型计算反向传播状态
    fn compute_backward_propagated_state_by_edge_type(
        &self,
        source_state: EscapeState,
        edge_type: DependencyEdge,
    ) -> EscapeState {
        match edge_type {
            // 直接赋值: 完全传播逃逸状态
            DependencyEdge::DirectAssignment => source_state,

            // 字段访问: 如果目标逃逸，源也逃逸
            DependencyEdge::FieldAccess { .. } => source_state,

            // 数组访问: 如果目标逃逸，源也逃逸
            DependencyEdge::ArrayAccess => source_state,

            // 其他情况：保守地传播
            _ => source_state,
        }
    }

    /// 计算前向传播后的逃逸状态（沿着依赖链向前传播）
    /// 例如：y = x, 如果 x 逃逸，y 也应该逃逸
    fn compute_forward_propagated_state(
        &self,
        source_state: EscapeState,
        edge: WeightedEdge,
    ) -> EscapeState {
        // 根据边权重调整传播逻辑
        match edge.weight {
            EdgeWeight::AddressOf => {
                // ptr = &value, 如果 value 逃逸，ptr 也逃逸
                source_state
            }
            EdgeWeight::Dereference => {
                // val = *ptr, 如果 ptr 逃逸，val 不一定逃逸
                // 因为只是读取指向的值
                EscapeState::NoEscape
            }
            EdgeWeight::Identity => {
                // 按边类型处理
                self.compute_forward_propagated_state_by_edge_type(source_state, edge.edge_type)
            }
        }
    }

    /// 根据边类型计算前向传播状态
    fn compute_forward_propagated_state_by_edge_type(
        &self,
        source_state: EscapeState,
        edge_type: DependencyEdge,
    ) -> EscapeState {
        match edge_type {
            // 直接赋值: 完全传播逃逸状态
            DependencyEdge::DirectAssignment => source_state,

            // 字段访问: 如果源逃逸，目标也逃逸
            DependencyEdge::FieldAccess { .. } => source_state,

            // 数组访问: 如果源逃逸，目标也逃逸
            DependencyEdge::ArrayAccess => source_state,

            // 函数参数: 至少是参数逃逸
            DependencyEdge::FunctionArgument => {
                if source_state == EscapeState::NoEscape {
                    EscapeState::ArgEscape
                } else {
                    source_state
                }
            }

            // 函数返回: 至少是返回逃逸
            DependencyEdge::FunctionReturn => {
                if matches!(source_state, EscapeState::NoEscape | EscapeState::ArgEscape) {
                    EscapeState::ReturnEscape
                } else {
                    source_state
                }
            }

            // 闭包捕获: 全局逃逸
            DependencyEdge::ClosureCapture => EscapeState::GlobalEscape,

            // 堆存储: 全局逃逸
            DependencyEdge::HeapStore => EscapeState::GlobalEscape,
        }
    }

    /// 获取变量的逃逸状态
    pub fn get_escape_state(&self, var_id: &VariableId) -> EscapeState {
        self.escape_cache
            .get(var_id)
            .copied()
            .unwrap_or(EscapeState::NoEscape)
    }

    /// 设置变量的逃逸状态
    pub fn set_escape_state(&mut self, var_id: VariableId, state: EscapeState) {
        self.escape_cache.insert(var_id, state);
    }

    /// 检测循环依赖
    pub fn has_cycle(&self, start_var: VariableId) -> bool {
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();
        self.has_cycle_util(start_var, &mut visited, &mut rec_stack)
    }

    fn has_cycle_util(
        &self,
        var_id: VariableId,
        visited: &mut HashSet<VariableId>,
        rec_stack: &mut HashSet<VariableId>,
    ) -> bool {
        if rec_stack.contains(&var_id) {
            return true;
        }

        if visited.contains(&var_id) {
            return false;
        }

        visited.insert(var_id);
        rec_stack.insert(var_id);

        for (dependent_var, _) in self.get_dependents(&var_id) {
            if self.has_cycle_util(dependent_var, visited, rec_stack) {
                return true;
            }
        }

        rec_stack.remove(&var_id);
        false
    }

    /// 获取图的统计信息
    pub fn stats(&self) -> GraphStats {
        // 统计所有出现在图中的唯一变量
        let mut all_vars = std::collections::HashSet::new();
        for &var in self.forward_edges.keys() {
            all_vars.insert(var);
        }
        for &var in self.backward_edges.keys() {
            all_vars.insert(var);
        }
        for &var in self.escape_cache.keys() {
            all_vars.insert(var);
        }

        GraphStats {
            total_variables: all_vars.len(),
            total_edges: self
                .forward_edges
                .values()
                .map(|edges| edges.len())
                .sum(),
            no_escape_count: self
                .escape_cache
                .values()
                .filter(|&&s| s == EscapeState::NoEscape)
                .count(),
            arg_escape_count: self
                .escape_cache
                .values()
                .filter(|&&s| s == EscapeState::ArgEscape)
                .count(),
            return_escape_count: self
                .escape_cache
                .values()
                .filter(|&&s| s == EscapeState::ReturnEscape)
                .count(),
            global_escape_count: self
                .escape_cache
                .values()
                .filter(|&&s| s == EscapeState::GlobalEscape)
                .count(),
        }
    }

    /// 获取节点元数据
    pub fn get_node_metadata(&self, var_id: &VariableId) -> Option<&NodeMetadata> {
        self.node_metadata.get(var_id)
    }

    /// 设置节点元数据
    pub fn set_node_metadata(&mut self, var_id: VariableId, metadata: NodeMetadata) {
        self.node_metadata.insert(var_id, metadata);
    }

    /// 获取或创建节点元数据
    pub fn get_or_create_node_metadata(&mut self, var_id: VariableId) -> &mut NodeMetadata {
        self.node_metadata.entry(var_id).or_insert_with(NodeMetadata::new)
    }

    /// 清空图
    pub fn clear(&mut self) {
        self.forward_edges.clear();
        self.backward_edges.clear();
        self.escape_cache.clear();
        self.node_metadata.clear();
    }
}

/// 图统计信息
#[derive(Debug, Clone)]
pub struct GraphStats {
    pub total_variables: usize,
    pub total_edges: usize,
    pub no_escape_count: usize,
    pub arg_escape_count: usize,
    pub return_escape_count: usize,
    pub global_escape_count: usize,
}

impl GraphStats {
    /// 计算可栈分配变量的百分比
    pub fn stack_allocatable_percentage(&self) -> f64 {
        if self.total_variables == 0 {
            0.0
        } else {
            (self.no_escape_count as f64 / self.total_variables as f64) * 100.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_graph() {
        let mut graph = VariableGraph::new();

        let var_a = VariableId(1);
        let var_b = VariableId(2);
        let var_c = VariableId(3);

        // a = b
        graph.add_assignment(var_b, var_a);
        // c = a
        graph.add_assignment(var_a, var_c);

        // 检查依赖关系
        let a_dependents = graph.get_dependents(&var_a);
        assert_eq!(a_dependents.len(), 1);
        assert_eq!(a_dependents[0].0, var_c);

        let a_dependencies = graph.get_dependencies(&var_a);
        assert_eq!(a_dependencies.len(), 1);
        assert_eq!(a_dependencies[0].0, var_b);
    }

    #[test]
    fn test_escape_propagation() {
        let mut graph = VariableGraph::new();

        let var_a = VariableId(1);
        let var_b = VariableId(2);
        let var_c = VariableId(3);

        // a = b, c = a
        graph.add_assignment(var_b, var_a);
        graph.add_assignment(var_a, var_c);

        // 标记 b 为全局逃逸
        let affected = graph
            .propagate_escape(var_b, EscapeState::GlobalEscape)
            .unwrap();

        // a 和 c 应该也被标记为逃逸
        assert!(affected.contains(&var_b));
        assert!(affected.contains(&var_a));
        assert!(affected.contains(&var_c));

        assert_eq!(graph.get_escape_state(&var_b), EscapeState::GlobalEscape);
        assert_eq!(graph.get_escape_state(&var_a), EscapeState::GlobalEscape);
        assert_eq!(graph.get_escape_state(&var_c), EscapeState::GlobalEscape);
    }

    #[test]
    fn test_cycle_detection() {
        let mut graph = VariableGraph::new();

        let var_a = VariableId(1);
        let var_b = VariableId(2);
        let var_c = VariableId(3);

        // a -> b -> c -> a (循环)
        graph.add_assignment(var_a, var_b);
        graph.add_assignment(var_b, var_c);
        graph.add_assignment(var_c, var_a);

        assert!(graph.has_cycle(var_a));
        assert!(graph.has_cycle(var_b));
        assert!(graph.has_cycle(var_c));
    }

    #[test]
    fn test_stats() {
        let mut graph = VariableGraph::new();

        let var_a = VariableId(1);
        let var_b = VariableId(2);
        let var_c = VariableId(3);

        graph.add_assignment(var_a, var_b);
        graph.add_assignment(var_b, var_c);

        graph.set_escape_state(var_a, EscapeState::NoEscape);
        graph.set_escape_state(var_b, EscapeState::NoEscape);
        graph.set_escape_state(var_c, EscapeState::GlobalEscape);

        let stats = graph.stats();
        assert_eq!(stats.total_variables, 3);
        assert_eq!(stats.no_escape_count, 2);
        assert_eq!(stats.global_escape_count, 1);
        assert!((stats.stack_allocatable_percentage() - 66.666).abs() < 0.01);
    }
}
