//! 函数调用图
//!
//! 用于确定函数分析顺序，支持自底向上的逃逸分析

use crate::types::FunctionId;
use karte_mir::{MirProgram, Statement, Value};
use std::collections::{HashMap, HashSet, VecDeque};

/// 函数调用图
///
/// 有向图，边 A -> B 表示函数A调用函数B
#[derive(Debug, Default)]
pub struct CallGraph {
    /// 调用边：caller -> callees
    edges: HashMap<FunctionId, HashSet<FunctionId>>,

    /// 反向边：callee -> callers（用于快速查询）
    reverse_edges: HashMap<FunctionId, HashSet<FunctionId>>,

    /// 所有函数节点
    nodes: HashSet<FunctionId>,
}

impl CallGraph {
    /// 创建新的调用图
    pub fn new() -> Self {
        Self::default()
    }

    /// 从MIR程序构建调用图
    pub fn build_from_program(program: &MirProgram) -> Self {
        let mut graph = CallGraph::new();

        // 添加所有函数节点
        for func_name in program.functions.keys() {
            graph.add_node(FunctionId(func_name.clone()));
        }

        // 分析每个函数的调用关系
        for (func_name, function) in &program.functions {
            let caller_id = FunctionId(func_name.clone());

            // 遍历所有基本块和语句
            for block in function.basic_blocks.values() {
                for statement in &block.statements {
                    if let Statement::Call {
                        function: callee, ..
                    } = statement
                    {
                        // 提取被调用函数的ID
                        if let Some(callee_id) = extract_function_id(callee) {
                            graph.add_edge(caller_id.clone(), callee_id);
                        }
                    }
                }
            }
        }

        graph
    }

    /// 添加节点
    pub fn add_node(&mut self, func_id: FunctionId) {
        self.nodes.insert(func_id);
    }

    /// 添加调用边
    pub fn add_edge(&mut self, caller: FunctionId, callee: FunctionId) {
        // 添加到正向边
        self.edges
            .entry(caller.clone())
            .or_insert_with(HashSet::new)
            .insert(callee.clone());

        // 添加到反向边
        self.reverse_edges
            .entry(callee.clone())
            .or_insert_with(HashSet::new)
            .insert(caller.clone());

        // 确保两个节点都在图中
        self.nodes.insert(caller);
        self.nodes.insert(callee);
    }

    /// 获取函数调用的所有被调用函数
    pub fn get_callees(&self, func_id: &FunctionId) -> Vec<FunctionId> {
        self.edges
            .get(func_id)
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// 获取调用该函数的所有函数
    pub fn get_callers(&self, func_id: &FunctionId) -> Vec<FunctionId> {
        self.reverse_edges
            .get(func_id)
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// 检测循环依赖（递归调用）
    pub fn has_cycle(&self, start: &FunctionId) -> bool {
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();
        self.has_cycle_util(start, &mut visited, &mut rec_stack)
    }

    fn has_cycle_util(
        &self,
        func_id: &FunctionId,
        visited: &mut HashSet<FunctionId>,
        rec_stack: &mut HashSet<FunctionId>,
    ) -> bool {
        if rec_stack.contains(func_id) {
            return true; // 检测到循环
        }

        if visited.contains(func_id) {
            return false; // 已访问过且无循环
        }

        visited.insert(func_id.clone());
        rec_stack.insert(func_id.clone());

        // 检查所有被调用函数
        if let Some(callees) = self.edges.get(func_id) {
            for callee in callees {
                if self.has_cycle_util(callee, visited, rec_stack) {
                    return true;
                }
            }
        }

        rec_stack.remove(func_id);
        false
    }

    /// 拓扑排序（后序DFS）
    ///
    /// 返回自底向上的分析顺序：叶子函数在前，调用者在后
    ///
    /// 例如：如果A调用B，B调用C，则返回顺序为 [C, B, A]
    pub fn topological_order(&self) -> Vec<FunctionId> {
        let mut result = Vec::new();
        let mut visited = HashSet::new();
        let mut in_progress = HashSet::new();

        // 对每个节点进行DFS
        for func_id in &self.nodes {
            if !visited.contains(func_id) {
                self.topological_dfs(func_id, &mut visited, &mut in_progress, &mut result);
            }
        }

        result
    }

    fn topological_dfs(
        &self,
        func_id: &FunctionId,
        visited: &mut HashSet<FunctionId>,
        in_progress: &mut HashSet<FunctionId>,
        result: &mut Vec<FunctionId>,
    ) {
        if visited.contains(func_id) {
            return;
        }

        if in_progress.contains(func_id) {
            // 检测到循环，跳过
            log::warn!("检测到递归调用: {:?}", func_id);
            return;
        }

        in_progress.insert(func_id.clone());

        // 先访问所有被调用函数（深度优先）
        if let Some(callees) = self.edges.get(func_id) {
            for callee in callees {
                self.topological_dfs(callee, visited, in_progress, result);
            }
        }

        in_progress.remove(func_id);
        visited.insert(func_id.clone());

        // 后序：在所有被调用函数之后添加当前函数
        result.push(func_id.clone());
    }

    /// 获取所有叶子函数（不调用任何其他函数）
    pub fn get_leaf_functions(&self) -> Vec<FunctionId> {
        self.nodes
            .iter()
            .filter(|func_id| {
                self.edges
                    .get(func_id)
                    .map(|callees| callees.is_empty())
                    .unwrap_or(true)
            })
            .cloned()
            .collect()
    }

    /// 获取所有入口函数（不被任何函数调用）
    pub fn get_entry_functions(&self) -> Vec<FunctionId> {
        self.nodes
            .iter()
            .filter(|func_id| {
                self.reverse_edges
                    .get(func_id)
                    .map(|callers| callers.is_empty())
                    .unwrap_or(true)
            })
            .cloned()
            .collect()
    }

    /// 获取图统计信息
    pub fn stats(&self) -> CallGraphStats {
        let total_functions = self.nodes.len();
        let total_edges = self.edges.values().map(|set| set.len()).sum();
        let leaf_functions = self.get_leaf_functions().len();
        let entry_functions = self.get_entry_functions().len();

        CallGraphStats {
            total_functions,
            total_edges,
            leaf_functions,
            entry_functions,
        }
    }
}

/// 从Value中提取函数ID
fn extract_function_id(value: &Value) -> Option<FunctionId> {
    match value {
        Value::Function { name, .. } => Some(FunctionId(name.clone())),
        Value::Variable { name, .. } => {
            // 可能是函数变量
            Some(FunctionId(name.clone()))
        }
        _ => None,
    }
}

/// 调用图统计信息
#[derive(Debug, Clone)]
pub struct CallGraphStats {
    /// 函数总数
    pub total_functions: usize,

    /// 调用边总数
    pub total_edges: usize,

    /// 叶子函数数量（不调用其他函数）
    pub leaf_functions: usize,

    /// 入口函数数量（不被调用）
    pub entry_functions: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_call_graph_basic() {
        let mut graph = CallGraph::new();

        let func_a = FunctionId("A".to_string());
        let func_b = FunctionId("B".to_string());
        let func_c = FunctionId("C".to_string());

        // A -> B -> C
        graph.add_edge(func_a.clone(), func_b.clone());
        graph.add_edge(func_b.clone(), func_c.clone());

        // 检查边
        let a_callees = graph.get_callees(&func_a);
        assert_eq!(a_callees.len(), 1);
        assert!(a_callees.contains(&func_b));

        let c_callers = graph.get_callers(&func_c);
        assert_eq!(c_callers.len(), 1);
        assert!(c_callers.contains(&func_b));
    }

    #[test]
    fn test_topological_order() {
        let mut graph = CallGraph::new();

        let func_a = FunctionId("A".to_string());
        let func_b = FunctionId("B".to_string());
        let func_c = FunctionId("C".to_string());

        // A -> B -> C
        graph.add_edge(func_a.clone(), func_b.clone());
        graph.add_edge(func_b.clone(), func_c.clone());

        let order = graph.topological_order();

        // 自底向上：C应该在B之前，B应该在A之前
        let c_pos = order.iter().position(|f| f == &func_c).unwrap();
        let b_pos = order.iter().position(|f| f == &func_b).unwrap();
        let a_pos = order.iter().position(|f| f == &func_a).unwrap();

        assert!(c_pos < b_pos);
        assert!(b_pos < a_pos);
    }

    #[test]
    fn test_cycle_detection() {
        let mut graph = CallGraph::new();

        let func_a = FunctionId("A".to_string());
        let func_b = FunctionId("B".to_string());

        // A -> B -> A (循环)
        graph.add_edge(func_a.clone(), func_b.clone());
        graph.add_edge(func_b.clone(), func_a.clone());

        assert!(graph.has_cycle(&func_a));
        assert!(graph.has_cycle(&func_b));
    }

    #[test]
    fn test_leaf_and_entry_functions() {
        let mut graph = CallGraph::new();

        let func_a = FunctionId("A".to_string());
        let func_b = FunctionId("B".to_string());
        let func_c = FunctionId("C".to_string());

        // A -> B -> C
        graph.add_edge(func_a.clone(), func_b.clone());
        graph.add_edge(func_b.clone(), func_c.clone());

        let leaf_funcs = graph.get_leaf_functions();
        assert_eq!(leaf_funcs.len(), 1);
        assert!(leaf_funcs.contains(&func_c));

        let entry_funcs = graph.get_entry_functions();
        assert_eq!(entry_funcs.len(), 1);
        assert!(entry_funcs.contains(&func_a));
    }
}
