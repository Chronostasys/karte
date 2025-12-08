//! 逃逸分析上下文

use crate::types::{FunctionId, VariableId};
use karte_mir::BasicBlockId;
use std::collections::HashSet;

/// 分析上下文
#[derive(Debug, Clone, Default)]
pub struct AnalysisContext {
    /// 当前函数
    pub current_function: Option<FunctionId>,

    /// 当前基本块
    pub current_block: Option<BasicBlockId>,

    /// 已访问的变量（防止循环分析）
    pub visited_variables: HashSet<VariableId>,

    /// 循环上下文栈
    pub loop_contexts: Vec<LoopContext>,

    /// 闭包上下文栈
    pub closure_contexts: Vec<ClosureContext>,

    /// 分析深度（防止递归过深）
    pub analysis_depth: usize,

    /// 最大分析深度
    pub max_depth: usize,
}

impl AnalysisContext {
    /// 创建新的分析上下文
    pub fn new() -> Self {
        Self {
            current_function: None,
            current_block: None,
            visited_variables: HashSet::new(),
            loop_contexts: Vec::new(),
            closure_contexts: Vec::new(),
            analysis_depth: 0,
            max_depth: 100, // 默认最大深度
        }
    }

    /// 进入函数
    pub fn enter_function(&mut self, function_id: FunctionId) {
        self.current_function = Some(function_id);
        self.visited_variables.clear();
        self.analysis_depth = 0;
    }

    /// 退出函数
    pub fn exit_function(&mut self) {
        self.current_function = None;
        self.visited_variables.clear();
        self.loop_contexts.clear();
        self.closure_contexts.clear();
        self.analysis_depth = 0;
    }

    /// 进入基本块
    pub fn enter_block(&mut self, block_id: BasicBlockId) {
        self.current_block = Some(block_id);
    }

    /// 退出基本块
    pub fn exit_block(&mut self) {
        self.current_block = None;
    }

    /// 进入循环
    pub fn enter_loop(&mut self, loop_header: BasicBlockId) {
        self.loop_contexts.push(LoopContext {
            loop_header,
            escaping_variables: HashSet::new(),
            loop_invariant_variables: HashSet::new(),
        });
    }

    /// 退出循环
    pub fn exit_loop(&mut self) -> Option<LoopContext> {
        self.loop_contexts.pop()
    }

    /// 当前是否在循环中
    pub fn is_in_loop(&self) -> bool {
        !self.loop_contexts.is_empty()
    }

    /// 获取当前循环上下文
    pub fn current_loop(&mut self) -> Option<&mut LoopContext> {
        self.loop_contexts.last_mut()
    }

    /// 进入闭包
    pub fn enter_closure(&mut self, closure_id: String) {
        self.closure_contexts.push(ClosureContext {
            closure_id,
            captured_variables: HashSet::new(),
        });
    }

    /// 退出闭包
    pub fn exit_closure(&mut self) -> Option<ClosureContext> {
        self.closure_contexts.pop()
    }

    /// 当前是否在闭包中
    pub fn is_in_closure(&self) -> bool {
        !self.closure_contexts.is_empty()
    }

    /// 获取当前闭包上下文
    pub fn current_closure(&mut self) -> Option<&mut ClosureContext> {
        self.closure_contexts.last_mut()
    }

    /// 标记变量已访问
    pub fn mark_visited(&mut self, var_id: VariableId) -> bool {
        !self.visited_variables.insert(var_id)
    }

    /// 检查变量是否已访问
    pub fn is_visited(&self, var_id: &VariableId) -> bool {
        self.visited_variables.contains(var_id)
    }

    /// 增加分析深度
    pub fn increase_depth(&mut self) -> Result<(), String> {
        self.analysis_depth += 1;
        if self.analysis_depth > self.max_depth {
            Err(format!("分析深度超过最大限制: {}", self.max_depth))
        } else {
            Ok(())
        }
    }

    /// 减少分析深度
    pub fn decrease_depth(&mut self) {
        if self.analysis_depth > 0 {
            self.analysis_depth -= 1;
        }
    }
}

/// 循环上下文
#[derive(Debug, Clone)]
pub struct LoopContext {
    /// 循环头基本块
    pub loop_header: BasicBlockId,

    /// 在循环中逃逸的变量
    pub escaping_variables: HashSet<VariableId>,

    /// 循环不变量
    pub loop_invariant_variables: HashSet<VariableId>,
}

impl LoopContext {
    /// 标记变量在循环中逃逸
    pub fn mark_escaping(&mut self, var_id: VariableId) {
        self.escaping_variables.insert(var_id);
    }

    /// 标记变量为循环不变量
    pub fn mark_invariant(&mut self, var_id: VariableId) {
        self.loop_invariant_variables.insert(var_id);
    }

    /// 检查变量是否在循环中逃逸
    pub fn is_escaping(&self, var_id: &VariableId) -> bool {
        self.escaping_variables.contains(var_id)
    }

    /// 检查变量是否为循环不变量
    pub fn is_invariant(&self, var_id: &VariableId) -> bool {
        self.loop_invariant_variables.contains(var_id)
    }
}

/// 闭包上下文
#[derive(Debug, Clone)]
pub struct ClosureContext {
    /// 闭包ID
    pub closure_id: String,

    /// 捕获的变量
    pub captured_variables: HashSet<VariableId>,
}

impl ClosureContext {
    /// 标记变量被捕获
    pub fn capture(&mut self, var_id: VariableId) {
        self.captured_variables.insert(var_id);
    }

    /// 检查变量是否被捕获
    pub fn is_captured(&self, var_id: &VariableId) -> bool {
        self.captured_variables.contains(var_id)
    }
}
