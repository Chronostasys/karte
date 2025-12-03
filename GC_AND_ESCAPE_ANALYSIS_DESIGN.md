# Karte 垃圾收集器与逃逸分析完整设计方案

## 概述

本方案为Karte编程语言设计了一个完整的内存管理系统，结合了基于Immix算法的高性能垃圾收集器和编译时逃逸分析。通过在编译阶段分析变量的生命周期，我们可以决定哪些变量可以分配在栈上，哪些需要分配在堆上，从而大幅减少GC压力和内存分配开销。

## 设计目标

1. **编译时优化**: 通过逃逸分析在编译时决定分配策略
2. **零运行时开销**: 逃逸分析完全在编译时完成，运行时无额外开销
3. **最大化栈分配**: 最大化非逃逸变量的栈分配，减少堆分配
4. **精确GC集成**: 与Immix GC无缝集成，提供精确的根扫描
5. **类型安全**: 保证内存安全，避免悬垂指针和内存泄漏

## 1. 系统架构

### 1.1 整体架构图

```
┌─────────────────────────────────────────────────────────────┐
│                 Karte Compilation Pipeline               │
├─────────────────────────────────────────────────────────────┤
│  Source → Parser → HIR → MIR → LIR → JIT → Execute    │
│                 │         │          │                 │
│                 │         │          │                 │
│    ┌──────────▼────────┼─────────▼───────────▼─────────┘
│    │  Escape Analysis  │   Allocation Strategy   │   GC Integration │
│    │  (编译时)        │   (运行时选择)      │   (运行时管理)    │
│    └────────────────────┴─────────────────────┴─────────────────────┘
├─────────────────────────────────────────────────────────────┤
│                     Runtime System                       │
│  ┌─────────────┬─────────────┬─────────────┬─────────────┐  │
│  │   Stack     │    Heap      │   JIT Code   │   GC Core    │  │
│  │ Management  │ Management   │ Memory      │ (Immix)     │  │
│  └─────────────┴─────────────┴─────────────┴─────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

### 1.2 核心组件

#### **逃逸分析器 (EscapeAnalyzer)**
- **编译时分析**: HIR/MIR阶段进行变量逃逸分析
- **依赖图构建**: 构建变量间依赖关系图
- **生命周期推断**: 推断变量的生命周期约束
- **分配建议**: 为每个变量生成最优分配建议

#### **分配策略选择器 (AllocationStrategySelector)**
- **策略决策**: 根据逃逸分析结果选择分配策略
- **栈分配**: 非逃逸变量分配到调用栈
- **堆分配**: 逃逸变量通过GC堆分配
- **内联优化**: 小对象和临时变量内联分配

#### **GC集成层 (GCIntegration)**
- **Immix集成**: 与Immix GC核心集成
- **根扫描**: 基于Karte虚拟栈模型的精确根扫描
- **安全点**: 在JIT代码中插入GC安全点

## 2. 逃逸分析算法

### 2.1 变量逃逸状态分类

```rust
// karte-hir/src/escape_analysis.rs

/// 变量的逃逸状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EscapeState {
    /// 不逃逸: 可以安全分配在栈上
    NoEscape,

    /// 参数逃逸: 作为参数传递给其他函数，但不会在函数外存活
    ArgumentEscape,

    /// 返回值逃逸: 作为返回值返回
    ReturnEscape,

    /// 全局逃逸: 可能在函数执行结束后仍然存活
    GlobalEscape,
}

impl EscapeState {
    /// 检查是否可以栈分配
    pub fn can_stack_allocate(&self) -> bool {
        match self {
            EscapeState::NoEscape => true,
            EscapeState::ArgumentEscape => true, // 参数可以在调用者栈上分配
            EscapeState::ReturnEscape => false,  // 返回值需要堆分配
            EscapeState::GlobalEscape => false, // 全局逃逸需要堆分配
        }
    }

    /// 检查是否需要GC跟踪
    pub fn needs_gc_tracking(&self) -> bool {
        !self.can_stack_allocate()
    }

    /// 合并两个逃逸状态
    pub fn merge(self, other: EscapeState) -> EscapeState {
        self.max(other)
    }
}

/// 变量的逃逸信息
#[derive(Debug, Clone)]
pub struct VariableEscapeInfo {
    /// 变量ID
    pub variable_id: VariableId,
    /// 逃逸状态
    pub escape_state: EscapeState,
    /// 逃逸点集合（函数或位置）
    pub escape_points: Vec<EscapePoint>,
    /// 生命周期约束
    pub lifetime_constraints: Vec<LifetimeConstraint>,
    /// 分配建议
    pub allocation_suggestion: AllocationSuggestion,
}

/// 逃逸点
#[derive(Debug, Clone)]
pub enum EscapePoint {
    /// 函数参数传递
    Argument {
        function_id: FunctionId,
        argument_index: usize,
    },

    /// 函数返回
    Return {
        function_id: FunctionId,
        return_path: ReturnPath,
    },

    /// 闭包捕获
    ClosureCapture {
        closure_id: ClosureId,
        capture_index: usize,
    },

    /// 赋值给全局变量
    GlobalAssignment {
        global_variable_id: VariableId,
    },

    /// 存储到堆对象中
    HeapStore {
        object_id: ObjectId,
        field_path: FieldPath,
    },
}

/// 生命周期约束
#[derive(Debug, Clone)]
pub struct LifetimeConstraint {
    /// 约束类型
    pub constraint_type: ConstraintType,
    /// 相关变量
    pub variables: Vec<VariableId>,
    /// 约束位置
    pub location: SourceLocation,
}

#[derive(Debug, Clone)]
pub enum ConstraintType {
    /// 变量a的生命周期必须不超过变量b
    Outlives(VariableId, VariableId),

    /// 变量a和b具有相同生命周期
    SameLifetime(VariableId, VariableId),

    /// 变量a的生命周期受函数调用限制
    CallLifetime {
        call_site: FunctionCallId,
        min_lifetime: LifetimeBound,
    },
}

/// 分配建议
#[derive(Debug, Clone)]
pub enum AllocationSuggestion {
    /// 栈分配（推荐）
    StackAlloc {
        stack_frame: StackFrameId,
        offset_from_fp: isize,
    },

    /// 堆分配
    HeapAlloc {
        allocation_site: FunctionId,
        object_type: TypeId,
        size_estimate: usize,
    },

    /// 内联分配（小对象）
    InlineAlloc {
        inline_site: ExpressionId,
    },

    /// 寄存器分配（临时变量）
    RegisterAlloc {
        preferred_register: RegisterId,
    },
}
```

### 2.2 依赖图分析

```rust
/// 逃逸分析器
pub struct EscapeAnalyzer {
    /// 函数调用图
    call_graph: CallGraph,

    /// 变量依赖图
    variable_graph: VariableGraph,

    /// 类型信息
    type_system: &TypeSystem,

    /// 当前分析上下文
    current_context: AnalysisContext,

    /// 分析结果
    escape_info: HashMap<VariableId, VariableEscapeInfo>,
}

#[derive(Default)]
struct AnalysisContext {
    /// 当前函数
    current_function: Option<FunctionId>,

    /// 当前栈帧
    current_stack_frame: StackFrameId,

    /// 已访问的变量
    visited_variables: HashSet<VariableId>,

    /// 循环上下文
    loop_context: Vec<LoopContext>,

    /// 闭包上下文
    closure_context: Vec<ClosureContext>,
}

impl EscapeAnalyzer {
    /// 创建新的逃逸分析器
    pub fn new(type_system: &TypeSystem) -> Self {
        Self {
            call_graph: CallGraph::new(),
            variable_graph: VariableGraph::new(),
            type_system,
            current_context: AnalysisContext::default(),
            escape_info: HashMap::new(),
        }
    }

    /// 分析函数的逃逸情况
    pub fn analyze_function(&mut self, function: &Function) -> Result<(), EscapeAnalysisError> {
        self.enter_function(function.id)?;

        // 1. 构建变量依赖图
        self.build_variable_graph(function)?;

        // 2. 分析函数体
        self.analyze_function_body(function)?;

        // 3. 处理循环
        self.analyze_loops(function)?;

        // 4. 处理闭包
        self.analyze_closures(function)?;

        // 5. 计算逃逸状态
        self.compute_escape_states(function)?;

        self.exit_function();
        Ok(())
    }

    /// 构建变量依赖图
    fn build_variable_graph(&mut self, function: &Function) -> Result<(), EscapeAnalysisError> {
        for statement in &function.body {
            self.analyze_statement_dependency(statement)?;
        }
        Ok(())
    }

    /// 分析语句的依赖关系
    fn analyze_statement_dependency(&mut self, statement: &Statement) -> Result<(), EscapeAnalysisError> {
        match statement {
            Statement::VariableDeclaration { variable_id, initializer, .. } => {
                if let Some(initializer) = initializer {
                    self.add_variable_dependency(*variable_id, initializer);
                }
            }

            Statement::Assignment { target, value, .. } => {
                self.add_assignment_dependency(target, value)?;
            }

            Statement::FunctionCall { function_id, arguments, return_variable, .. } => {
                self.analyze_function_call_dependency(*function_id, arguments, *return_variable)?;
            }

            Statement::Return { value, .. } => {
                if let Some(return_value) = value {
                    self.mark_return_escape(return_value)?;
                }
            }

            Statement::Loop { body, .. } => {
                self.enter_loop_context();
                for stmt in body {
                    self.analyze_statement_dependency(stmt)?;
                }
                self.exit_loop_context();
            }

            Statement::If { condition, then_body, else_body, .. } => {
                self.analyze_expression_dependency(condition)?;
                for stmt in then_body {
                    self.analyze_statement_dependency(stmt)?;
                }
                for stmt in else_body {
                    self.analyze_statement_dependency(stmt)?;
                }
            }

            Statement::Closure { captures, .. } => {
                self.analyze_closure_captures(captures)?;
            }

            // 其他语句类型...
        }
        Ok(())
    }

    /// 添加变量依赖关系
    fn add_variable_dependency(&mut self, var_id: VariableId, expr: &Expression) -> Result<(), EscapeAnalysisError> {
        let used_variables = self.extract_variables_from_expression(expr)?;

        for used_var in used_variables {
            self.variable_graph.add_dependency(used_var, var_id);
        }

        Ok(())
    }

    /// 分析赋值依赖
    fn add_assignment_dependency(&mut self, target: &Expression, value: &Expression) -> Result<(), EscapeAnalysisError> {
        match target {
            Expression::VariableAccess { variable_id, .. } => {
                self.add_variable_dependency(*variable_id, value)?;
            }

            Expression::FieldAccess { object, field_name, .. } => {
                // 字段赋值可能包含指针存储，需要逃逸分析
                let object_vars = self.extract_variables_from_expression(object)?;
                let value_vars = self.extract_variables_from_expression(value)?;

                for obj_var in object_vars {
                    for val_var in value_vars {
                        self.variable_graph.add_heap_store_dependency(obj_var, val_var, field_name.clone());
                    }
                }
            }

            Expression::IndexAccess { array, index, .. } => {
                // 数组赋值
                let array_vars = self.extract_variables_from_expression(array)?;
                let index_vars = self.extract_variables_from_expression(index)?;
                let value_vars = self.extract_variables_from_expression(value)?;

                for arr_var in array_vars {
                    for val_var in value_vars {
                        self.variable_graph.add_array_store_dependency(arr_var, val_var);
                    }
                }
            }

            // 其他目标类型...
        }

        Ok(())
    }

    /// 分析函数调用依赖
    fn analyze_function_call_dependency(
        &mut self,
        function_id: FunctionId,
        arguments: &[Expression],
        return_variable: Option<VariableId>,
    ) -> Result<(), EscapeAnalysisError> {
        // 分析参数逃逸
        for (index, arg) in arguments.iter().enumerate() {
            let arg_vars = self.extract_variables_from_expression(arg)?;

            for arg_var in arg_vars {
                self.mark_argument_escape(arg_var, function_id, index)?;
            }
        }

        // 分析返回值逃逸
        if let Some(ret_var) = return_variable {
            self.mark_return_escape_variable(ret_var, function_id)?;
        }

        Ok(())
    }

    /// 标记参数逃逸
    fn mark_argument_escape(
        &mut self,
        variable_id: VariableId,
        function_id: FunctionId,
        argument_index: usize,
    ) -> Result<(), EscapeAnalysisError> {
        let escape_info = self.escape_info.entry(variable_id).or_insert_with(|| {
            VariableEscapeInfo {
                variable_id,
                escape_state: EscapeState::NoEscape,
                escape_points: Vec::new(),
                lifetime_constraints: Vec::new(),
                allocation_suggestion: AllocationSuggestion::StackAlloc {
                    stack_frame: self.current_context.current_stack_frame,
                    offset_from_fp: 0, // 稍后计算
                },
            }
        });

        // 添加逃逸点
        escape_info.escape_points.push(EscapePoint::Argument {
            function_id,
            argument_index,
        });

        // 检查函数是否可能存储参数
        if self.function_may_store_arguments(function_id)? {
            escape_info.escape_state = EscapeState::GlobalEscape;
            escape_info.allocation_suggestion = AllocationSuggestion::HeapAlloc {
                allocation_site: self.current_context.current_function.unwrap(),
                object_type: self.get_variable_type(variable_id)?,
                size_estimate: self.estimate_variable_size(variable_id)?,
            };
        }

        Ok(())
    }

    /// 标记返回值逃逸
    fn mark_return_escape_variable(
        &mut self,
        variable_id: VariableId,
        function_id: FunctionId,
    ) -> Result<(), EscapeAnalysisError> {
        let escape_info = self.escape_info.entry(variable_id).or_insert_with(|| {
            VariableEscapeInfo {
                variable_id,
                escape_state: EscapeState::NoEscape,
                escape_points: Vec::new(),
                lifetime_constraints: Vec::new(),
                allocation_suggestion: AllocationSuggestion::StackAlloc {
                    stack_frame: self.current_context.current_stack_frame,
                    offset_from_fp: 0,
                },
            }
        });

        escape_info.escape_points.push(EscapePoint::Return {
            function_id,
            return_path: ReturnPath::Direct,
        });

        // 返回值总是逃逸
        escape_info.escape_state = EscapeState::ReturnEscape;
        escape_info.allocation_suggestion = AllocationSuggestion::HeapAlloc {
            allocation_site: self.current_context.current_function.unwrap(),
            object_type: self.get_variable_type(variable_id)?,
            size_estimate: self.estimate_variable_size(variable_id)?,
        };

        Ok(())
    }

    /// 标记返回值逃逸（表达式形式）
    fn mark_return_escape(&mut self, expression: &Expression) -> Result<(), EscapeAnalysisError> {
        let expr_vars = self.extract_variables_from_expression(expression)?;

        for var_id in expr_vars {
            let escape_info = self.escape_info.entry(var_id).or_insert_with(|| {
                VariableEscapeInfo {
                    variable_id: var_id,
                    escape_state: EscapeState::NoEscape,
                    escape_points: Vec::new(),
                    lifetime_constraints: Vec::new(),
                    allocation_suggestion: AllocationSuggestion::StackAlloc {
                        stack_frame: self.current_context.current_stack_frame,
                        offset_from_fp: 0,
                    },
                }
            });

            escape_info.escape_points.push(EscapePoint::Return {
                function_id: self.current_context.current_function.unwrap(),
                return_path: ReturnPath::Indirect,
            });

            escape_info.escape_state = escape_info.escape_state.max(EscapeState::ReturnEscape);
        }

        Ok(())
    }

    /// 分析闭包捕获
    fn analyze_closure_captures(&mut self, captures: &[VariableCapture]) -> Result<(), EscapeAnalysisError> {
        for (index, capture) in captures.iter().enumerate() {
            self.mark_closure_capture_escape(capture.variable_id, index)?;
        }
        Ok(())
    }

    /// 标记闭包捕获逃逸
    fn mark_closure_capture_escape(
        &mut self,
        variable_id: VariableId,
        capture_index: usize,
    ) -> Result<(), EscapeAnalysisError> {
        let escape_info = self.escape_info.entry(variable_id).or_insert_with(|| {
            VariableEscapeInfo {
                variable_id,
                escape_state: EscapeState::NoEscape,
                escape_points: Vec::new(),
                lifetime_constraints: Vec::new(),
                allocation_suggestion: AllocationSuggestion::StackAlloc {
                    stack_frame: self.current_context.current_stack_frame,
                    offset_from_fp: 0,
                },
            }
        });

        escape_info.escape_points.push(EscapePoint::ClosureCapture {
            closure_id: self.current_context.closure_context.last().unwrap().closure_id,
            capture_index,
        });

        // 闭包捕获的变量生命周期受闭包对象限制
        escape_info.lifetime_constraints.push(LifetimeConstraint::Outlives(
            self.get_closure_object_variable(self.current_context.closure_context.last().unwrap().closure_id)?,
            variable_id,
        ));

        // 根据捕获模式决定逃逸状态
        match capture.capture_mode {
            CaptureMode::ByValue => {
                // 按值捕获，变量需要在闭包对象中存储，因此可能逃逸
                escape_info.escape_state = EscapeState::GlobalEscape;
                escape_info.allocation_suggestion = AllocationSuggestion::HeapAlloc {
                    allocation_site: self.current_context.current_function.unwrap(),
                    object_type: self.get_variable_type(variable_id)?,
                    size_estimate: self.estimate_variable_size(variable_id)?,
                };
            }

            CaptureMode::ByRef | CaptureMode::ByMutRef => {
                // 按引用捕获，原始变量的生命周期必须超过闭包
                escape_info.escape_state = EscapeState::GlobalEscape;
                escape_info.allocation_suggestion = AllocationSuggestion::HeapAlloc {
                    allocation_site: self.current_context.current_function.unwrap(),
                    object_type: self.get_variable_type(variable_id)?,
                    size_estimate: self.estimate_variable_size(variable_id)?,
                };
            }
        }

        Ok(())
    }

    /// 计算逃逸状态
    fn compute_escape_states(&mut self, function: &Function) -> Result<(), EscapeAnalysisError> {
        // 1. 计算直接逃逸
        self.compute_direct_escapes(function)?;

        // 2. 传播间接逃逸
        self.propagate_indirect_escapes()?;

        // 3. 处理循环特殊情况
        self.handle_loop_special_cases(function)?;

        // 4. 优化分配建议
        self.optimize_allocation_suggestions()?;

        Ok(())
    }

    /// 计算直接逃逸
    fn compute_direct_escapes(&mut self, function: &Function) -> Result<(), EscapeAnalysisError> {
        // 分析已经收集的逃逸信息，确定每个变量的逃逸状态
        for (var_id, escape_info) in &mut self.escape_info {
            if escape_info.escape_state == EscapeState::NoEscape {
                // 检查是否有其他逃逸点
                for escape_point in &escape_info.escape_points {
                    match escape_point {
                        EscapePoint::Argument { function_id, .. } => {
                            if self.function_may_store_arguments(*function_id)? {
                                escape_info.escape_state = EscapeState::GlobalEscape;
                            } else {
                                escape_info.escape_state = EscapeState::ArgumentEscape;
                            }
                        }

                        EscapePoint::Return { .. } => {
                            escape_info.escape_state = EscapeState::ReturnEscape;
                        }

                        EscapePoint::ClosureCapture { .. } => {
                            escape_info.escape_state = EscapeState::GlobalEscape;
                        }

                        EscapePoint::GlobalAssignment { .. } => {
                            escape_info.escape_state = EscapeState::GlobalEscape;
                        }

                        EscapePoint::HeapStore { .. } => {
                            escape_info.escape_state = EscapeState::GlobalEscape;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// 传播间接逃逸
    fn propagate_indirect_escapes(&mut self) -> Result<(), EscapeAnalysisError> {
        // 使用工作列表算法传播逃逸状态
        let mut work_list: Vec<VariableId> = self.escape_info.keys().copied().collect();
        let mut changed = true;

        while changed {
            changed = false;

            for var_id in work_list.iter() {
                let current_state = self.escape_info.get(var_id).unwrap().escape_state;

                // 传播给依赖的变量
                if let Some(dependencies) = self.variable_graph.get_dependencies(*var_id) {
                    for &dep_var_id in dependencies {
                        let dep_info = self.escape_info.get_mut(&dep_var_id).unwrap();

                        let new_state = dep_info.escape_state.max(current_state);
                        if new_state != dep_info.escape_state {
                            dep_info.escape_state = new_state;
                            changed = true;

                            // 更新分配建议
                            if new_state.needs_gc_tracking() {
                                dep_info.allocation_suggestion = AllocationSuggestion::HeapAlloc {
                                    allocation_site: self.current_context.current_function.unwrap(),
                                    object_type: self.get_variable_type(dep_var_id)?,
                                    size_estimate: self.estimate_variable_size(dep_var_id)?,
                                };
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// 处理循环特殊情况
    fn handle_loop_special_cases(&mut self, function: &Function) -> Result<(), EscapeAnalysisError> {
        for loop_context in &self.current_context.loop_context {
            // 在循环中创建的变量如果被使用到循环外，则逃逸
            for &var_id in &loop_context.loop_variables {
                if self.variable_used_outside_loop(var_id, loop_context.loop_id)? {
                    let escape_info = self.escape_info.get_mut(&var_id).unwrap();
                    escape_info.escape_state = EscapeState::GlobalEscape;
                    escape_info.allocation_suggestion = AllocationSuggestion::HeapAlloc {
                        allocation_site: self.current_context.current_function.unwrap(),
                        object_type: self.get_variable_type(var_id)?,
                        size_estimate: self.estimate_variable_size(var_id)?,
                    };
                }
            }
        }

        Ok(())
    }

    /// 检查变量是否在循环外使用
    fn variable_used_outside_loop(&self, var_id: VariableId, loop_id: LoopId) -> Result<bool, EscapeAnalysisError> {
        // 这里需要分析变量的使用范围
        // 简化实现：检查是否有在循环外的使用
        todo!("实现循环外使用检查")
    }

    /// 优化分配建议
    fn optimize_allocation_suggestions(&mut self) -> Result<(), EscapeAnalysisError> {
        for escape_info in self.escape_info.values_mut() {
            if escape_info.escape_state.can_stack_allocate() {
                // 可以栈分配，优化栈偏移
                if let AllocationSuggestion::StackAlloc { offset_from_fp, .. } = &mut escape_info.allocation_suggestion {
                    *offset_from_fp = self.calculate_optimal_stack_offset(escape_info.variable_id)?;
                }
            } else {
                // 需要堆分配，考虑内联优化
                let size = self.estimate_variable_size(escape_info.variable_id)?;
                if size <= 32 && self.can_inline_allocate(escape_info.variable_id)? {
                    escape_info.allocation_suggestion = AllocationSuggestion::InlineAlloc {
                        inline_site: self.get_allocation_site(escape_info.variable_id)?,
                    };
                }
            }
        }

        Ok(())
    }

    /// 计算最优栈偏移
    fn calculate_optimal_stack_offset(&self, var_id: VariableId) -> Result<isize, EscapeAnalysisError> {
        // 根据变量大小和对齐要求计算最优偏移
        let var_size = self.estimate_variable_size(var_id)?;
        let alignment = self.get_variable_alignment(var_id)?;

        // 简化实现：返回固定偏移
        Ok(((var_size + alignment - 1) & !(alignment - 1)) as isize)
    }

    /// 检查是否可以内联分配
    fn can_inline_allocate(&self, var_id: VariableId) -> Result<bool, EscapeAnalysisError> {
        // 检查变量的使用模式，决定是否可以内联分配
        // 小对象且使用次数少时可以内联
        let usage_count = self.get_variable_usage_count(var_id)?;
        let size = self.estimate_variable_size(var_id)?;

        Ok(size <= 32 && usage_count <= 3)
    }

    /// 辅助方法实现
    fn extract_variables_from_expression(&self, expr: &Expression) -> Result<Vec<VariableId>, EscapeAnalysisError> {
        // 实现表达式变量提取
        todo!("实现表达式变量提取")
    }

    fn function_may_store_arguments(&self, function_id: FunctionId) -> Result<bool, EscapeAnalysisError> {
        // 检查函数是否可能存储参数（例如存储到全局变量、堆对象等）
        todo!("实现参数存储检查")
    }

    fn get_variable_type(&self, var_id: VariableId) -> Result<TypeId, EscapeAnalysisError> {
        // 获取变量类型
        todo!("实现变量类型获取")
    }

    fn estimate_variable_size(&self, var_id: VariableId) -> Result<usize, EscapeAnalysisError> {
        // 估算变量大小
        todo!("实现变量大小估算")
    }

    fn get_variable_alignment(&self, var_id: VariableId) -> Result<usize, EscapeAnalysisError> {
        // 获取变量对齐要求
        todo!("实现变量对齐获取")
    }

    fn get_variable_usage_count(&self, var_id: VariableId) -> Result<usize, EscapeAnalysisError> {
        // 获取变量使用次数
        todo!("实现变量使用计数")
    }

    fn get_allocation_site(&self, var_id: VariableId) -> Result<ExpressionId, EscapeAnalysisError> {
        // 获取分配点表达式ID
        todo!("实现分配点获取")
    }
}
```

### 2.2 循环感知的逃逸分析

```rust
/// 循环上下文
#[derive(Debug, Clone)]
pub struct LoopContext {
    /// 循环ID
    pub loop_id: LoopId,

    /// 循环中定义的变量
    pub loop_variables: Vec<VariableId>,

    /// 循环中使用的变量
    pub used_variables: Vec<VariableId>,

    /// 循环嵌套深度
    pub nesting_depth: usize,

    /// 循环类型
    pub loop_type: LoopType,
}

#[derive(Debug, Clone)]
pub enum LoopType {
    /// for循环
    For {
        iterator_var: VariableId,
        collection_var: VariableId,
    },

    /// while循环
    While {
        condition_var: VariableId,
    },

    /// 无限循环
    Infinite,
}

impl EscapeAnalyzer {
    /// 进入循环上下文
    fn enter_loop_context(&mut self) {
        let loop_context = LoopContext {
            loop_id: self.generate_loop_id(),
            loop_variables: Vec::new(),
            used_variables: Vec::new(),
            nesting_depth: self.current_context.loop_context.len(),
            loop_type: LoopType::Infinite, // 稍后更新
        };

        self.current_context.loop_context.push(loop_context);
    }

    /// 退出循环上下文
    fn exit_loop_context(&mut self) {
        self.current_context.loop_context.pop();
    }

    /// 在当前循环中定义变量
    fn define_loop_variable(&mut self, var_id: VariableId) {
        if let Some(loop_context) = self.current_context.loop_context.last_mut() {
            loop_context.loop_variables.push(var_id);
        }
    }

    /// 在循环中使用变量
    fn use_variable_in_loop(&mut self, var_id: VariableId) {
        if let Some(loop_context) = self.current_context.loop_context.last_mut() {
            loop_context.used_variables.push(var_id);
        }
    }

    /// 分析循环中的逃逸情况
    fn analyze_loop_escapes(&mut self, loop_context: &LoopContext) -> Result<(), EscapeAnalysisError> {
        // 检查循环中定义的变量是否逃逸到循环外
        for &var_id in &loop_context.loop_variables {
            if self.variable_used_in_outer_scope(var_id, loop_context.loop_id)? {
                // 变量逃逸到循环外，需要堆分配
                let escape_info = self.escape_info.get_mut(&var_id).unwrap();
                escape_info.escape_state = EscapeState::GlobalEscape;
                escape_info.allocation_suggestion = AllocationSuggestion::HeapAlloc {
                    allocation_site: self.current_context.current_function.unwrap(),
                    object_type: self.get_variable_type(var_id)?,
                    size_estimate: self.estimate_variable_size(var_id)?,
                };
            }
        }

        // 特殊处理迭代器变量
        if let LoopType::For { iterator_var, .. } = &loop_context.loop_type {
            let iter_info = self.escape_info.get_mut(iterator_var).unwrap();
            iter_info.escape_state = EscapeState::NoEscape; // 迭代器变量通常不逃逸
        }

        Ok(())
    }

    /// 检查变量是否在外层作用域使用
    fn variable_used_in_outer_scope(&self, var_id: VariableId, loop_id: LoopId) -> Result<bool, EscapeAnalysisError> {
        // 检查变量是否在循环外的作用域中使用
        // 这需要分析变量的作用域链
        todo!("实现外层作用域使用检查")
    }

    /// 生成循环ID
    fn generate_loop_id(&self) -> LoopId {
        // 生成唯一的循环标识符
        todo!("实现循环ID生成")
    }
}
```

### 2.3 闭包逃逸分析

```rust
/// 闭包上下文
#[derive(Debug, Clone)]
pub struct ClosureContext {
    /// 闭包ID
    pub closure_id: ClosureId,

    /// 捕获的变量
    pub captured_variables: Vec<VariableCapture>,

    /// 闭包类型（函数指针、闭包对象等）
    pub closure_type: ClosureType,

    /// 闭包生命周期
    pub closure_lifetime: Lifetime,
}

#[derive(Debug, Clone)]
pub enum ClosureType {
    /// 简单函数指针，无捕获
    FunctionPointer,

    /// 闭包对象，包含捕获的环境
    ClosureObject {
        capture_count: usize,
        capture_layout: CaptureLayout,
    },

    /// 方法闭包，包含self参数
    MethodClosure {
        self_type: TypeId,
        capture_count: usize,
    },
}

#[derive(Debug, Clone)]
pub struct CaptureLayout {
    /// 捕获变量的布局偏移
    pub captures: Vec<CaptureInfo>,
}

#[derive(Debug, Clone)]
pub struct CaptureInfo {
    /// 捕获的变量ID
    pub variable_id: VariableId,

    /// 在闭包环境中的偏移
    pub offset: usize,

    /// 捕获方式
    pub capture_mode: CaptureMode,
}

#[derive(Debug, Clone)]
pub enum CaptureMode {
    /// 按值捕获
    ByValue,

    /// 按引用捕获
    ByRef,

    /// 按可变引用捕获
    ByMutRef,
}

impl EscapeAnalyzer {
    /// 进入闭包上下文
    fn enter_closure_context(&mut self, closure_id: ClosureId, closure_type: ClosureType) {
        let closure_context = ClosureContext {
            closure_id,
            captured_variables: Vec::new(),
            closure_type,
            closure_lifetime: Lifetime::generate_new(),
        };

        self.current_context.closure_context.push(closure_context);
    }

    /// 退出闭包上下文
    fn exit_closure_context(&mut self) {
        self.current_context.closure_context.pop();
    }

    /// 添加闭包捕获
    fn add_closure_capture(&mut self, variable_id: VariableId, capture_mode: CaptureMode) {
        if let Some(closure_context) = self.current_context.closure_context.last_mut() {
            let capture_info = CaptureInfo {
                variable_id,
                offset: 0, // 稍后计算
                capture_mode,
            };

            closure_context.captured_variables.push(VariableCapture {
                variable_id,
                capture_mode,
                source_location: self.current_source_location(),
            });
        }
    }

    /// 分析闭包捕获的逃逸情况
    fn analyze_closure_escapes(&mut self) -> Result<(), EscapeAnalysisError> {
        for closure_context in &self.current_context.closure_context {
            for capture in &closure_context.captured_variables {
                self.analyze_capture_escape(closure_context, capture)?;
            }
        }

        Ok(())
    }

    /// 分析单个捕获的逃逸情况
    fn analyze_capture_escape(
        &mut self,
        closure_context: &ClosureContext,
        capture: &VariableCapture,
    ) -> Result<(), EscapeAnalysisError> {
        let capture_info = self.escape_info.entry(capture.variable_id).or_insert_with(|| {
            VariableEscapeInfo {
                variable_id: capture.variable_id,
                escape_state: EscapeState::NoEscape,
                escape_points: Vec::new(),
                lifetime_constraints: Vec::new(),
                allocation_suggestion: AllocationSuggestion::StackAlloc {
                    stack_frame: self.current_context.current_stack_frame,
                    offset_from_fp: 0,
                },
            }
        });

        // 闭包捕获的变量生命周期受闭包对象限制
        capture_info.lifetime_constraints.push(LifetimeConstraint::Outlives(
            self.get_closure_object_variable(closure_context.closure_id)?,
            capture.variable_id,
        ));

        // 根据捕获模式决定逃逸状态
        match capture.capture_mode {
            CaptureMode::ByValue => {
                // 按值捕获，变量需要在闭包对象中存储，因此可能逃逸
                capture_info.escape_state = EscapeState::GlobalEscape;
                capture_info.allocation_suggestion = AllocationSuggestion::HeapAlloc {
                    allocation_site: self.current_context.current_function.unwrap(),
                    object_type: self.get_variable_type(capture.variable_id)?,
                    size_estimate: self.estimate_variable_size(capture.variable_id)?,
                };
            }

            CaptureMode::ByRef | CaptureMode::ByMutRef => {
                // 按引用捕获，原始变量的生命周期必须超过闭包
                capture_info.escape_state = EscapeState::GlobalEscape;
                capture_info.allocation_suggestion = AllocationSuggestion::HeapAlloc {
                    allocation_site: self.current_context.current_function.unwrap(),
                    object_type: self.get_variable_type(capture.variable_id)?,
                    size_estimate: self.estimate_variable_size(capture.variable_id)?,
                };
            }
        }

        // 添加闭包捕获逃逸点
        capture_info.escape_points.push(EscapePoint::ClosureCapture {
            closure_id: closure_context.closure_id,
            capture_index: capture_info.escape_points.len(),
        });

        Ok(())
    }

    /// 获取闭包对象的变量ID
    fn get_closure_object_variable(&self, closure_id: ClosureId) -> Result<VariableId, EscapeAnalysisError> {
        // 查找或创建代表闭包对象的变量
        todo!("实现闭包对象变量获取")
    }

    /// 获取当前源码位置
    fn current_source_location(&self) -> SourceLocation {
        // 返回当前分析的源码位置
        todo!("实现源码位置获取")
    }
}
```

## 3. 与MIR lowering集成

### 3.1 分配策略选择

```rust
// karte-mir/src/lower/allocation_strategy.rs

use karte_hir::{escape_analysis::EscapeState, type_system::Type};
use crate::mir::*;

/// 分配策略选择器
pub struct AllocationStrategySelector {
    /// 逃逸分析结果
    escape_analysis: HashMap<VariableId, VariableEscapeInfo>,

    /// 当前函数上下文
    current_function: FunctionId,

    /// 栈帧布局
    stack_layout: StackFrameLayout,
}

impl AllocationStrategySelector {
    /// 创建新的分配策略选择器
    pub fn new(escape_analysis: HashMap<VariableId, VariableEscapeInfo>) -> Self {
        Self {
            escape_analysis,
            current_function: FunctionId::invalid(),
            stack_layout: StackFrameLayout::new(),
        }
    }

    /// 为变量选择分配策略
    pub fn select_allocation_strategy(&mut self, variable_id: VariableId, var_type: &Type) -> AllocationStrategy {
        if let Some(escape_info) = self.escape_analysis.get(&variable_id) {
            match &escape_info.allocation_suggestion {
                AllocationSuggestion::StackAlloc { stack_frame, offset_from_fp } => {
                    AllocationStrategy::Stack {
                        stack_frame: *stack_frame,
                        offset: *offset_from_fp,
                        size: self.calculate_stack_size(var_type),
                        alignment: var_type.alignment(),
                    }
                }

                AllocationSuggestion::HeapAlloc { allocation_site, object_type, size_estimate } => {
                    AllocationStrategy::Heap {
                        object_type: *object_type,
                        size: *size_estimate,
                        allocation_site: *allocation_site,
                        gc_tracked: true,
                    }
                }

                AllocationSuggestion::InlineAlloc { inline_site } => {
                    AllocationStrategy::Inline {
                        size: self.calculate_inline_size(var_type),
                        site: *inline_site,
                    }
                }

                AllocationSuggestion::RegisterAlloc { preferred_register } => {
                    AllocationStrategy::Register {
                        register: *preferred_register,
                        size: var_type.size(),
                    }
                }
            }
        } else {
            // 默认堆分配
            AllocationStrategy::Heap {
                object_type: var_type.id(),
                size: var_type.size(),
                allocation_site: self.current_function,
                gc_tracked: true,
            }
        }
    }

    /// 生成分配指令
    pub fn generate_allocation_instruction(&mut self, strategy: AllocationStrategy) -> Instruction {
        match strategy {
            AllocationStrategy::Stack { stack_frame, offset, size, alignment } => {
                Instruction::StackAlloc {
                    stack_frame,
                    offset,
                    size,
                    alignment,
                    target: self.generate_temp_variable(),
                }
            }

            AllocationStrategy::Heap { object_type, size, allocation_site, gc_tracked } => {
                if gc_tracked {
                    Instruction::GcAlloc {
                        object_type,
                        size,
                        allocation_site,
                        target: self.generate_temp_variable(),
                    }
                } else {
                    Instruction::RawAlloc {
                        size,
                        target: self.generate_temp_variable(),
                    }
                }
            }

            AllocationStrategy::Inline { size, site } => {
                Instruction::InlineAlloc {
                    size,
                    site,
                    target: self.generate_temp_variable(),
                }
            }

            AllocationStrategy::Register { register, size } => {
                Instruction::RegisterAlloc {
                    register,
                    size,
                    target: self.generate_temp_variable(),
                }
            }
        }
    }

    /// 计算栈分配大小
    fn calculate_stack_size(&self, var_type: &Type) -> usize {
        let mut size = var_type.size();

        // 考虑对齐
        let alignment = var_type.alignment();
        if size % alignment != 0 {
            size = (size + alignment - 1) & !(alignment - 1);
        }

        size
    }

    /// 计算内联分配大小
    fn calculate_inline_size(&self, var_type: &Type) -> usize {
        // 内联分配通常有大小限制
        let base_size = var_type.size();
        base_size.min(32) // 限制为32字节
    }

    /// 生成临时变量
    fn generate_temp_variable(&mut self) -> VariableId {
        let temp_id = VariableId::generate_temp();
        self.stack_layout.add_temporary(temp_id);
        temp_id
    }
}

/// 分配策略
#[derive(Debug, Clone)]
pub enum AllocationStrategy {
    /// 栈分配
    Stack {
        stack_frame: StackFrameId,
        offset: isize,
        size: usize,
        alignment: usize,
    },

    /// 堆分配（GC跟踪）
    Heap {
        object_type: TypeId,
        size: usize,
        allocation_site: FunctionId,
        gc_tracked: bool,
    },

    /// 内联分配
    Inline {
        size: usize,
        site: ExpressionId,
    },

    /// 寄存器分配
    Register {
        register: RegisterId,
        size: usize,
    },
}
```

### 3.2 栈帧布局优化

```rust
/// 栈帧布局器
pub struct StackFrameLayout {
    /// 当前函数ID
    function_id: FunctionId,

    /// 栈帧变量
    variables: HashMap<VariableId, StackSlot>,

    /// 临时变量
    temporaries: Vec<VariableId>,

    /// 栈帧大小
    frame_size: usize,

    /// 对齐要求
    alignment: usize,

    /// 保存的寄存器
    saved_registers: Vec<RegisterId>,
}

#[derive(Debug, Clone)]
pub struct StackSlot {
    /// 变量ID
    pub variable_id: VariableId,

    /// 栈帧中的偏移（从FP开始）
    pub offset: isize,

    /// 变量大小
    pub size: usize,

    /// 对齐要求
    pub alignment: usize,

    /// 槽类型
    pub slot_type: SlotType,
}

#[derive(Debug, Clone)]
pub enum SlotType {
    /// 局部变量
    Local,

    /// 参数变量
    Parameter,

    /// 保存的寄存器
    SavedRegister,

    /// 返回值地址
    ReturnAddress,

    /// 临时变量
    Temporary,
}

impl StackFrameLayout {
    /// 创建新的栈帧布局
    pub fn new() -> Self {
        Self {
            function_id: FunctionId::invalid(),
            variables: HashMap::new(),
            temporaries: Vec::new(),
            frame_size: 0,
            alignment: 8, // 默认8字节对齐
            saved_registers: Vec::new(),
        }
    }

    /// 添加栈变量
    pub fn add_stack_variable(&mut self, variable_id: VariableId, size: usize, alignment: usize) -> isize {
        let offset = self.allocate_stack_space(size, alignment);

        let slot = StackSlot {
            variable_id,
            offset,
            size,
            alignment,
            slot_type: SlotType::Local,
        };

        self.variables.insert(variable_id, slot);
        offset
    }

    /// 添加参数变量
    pub fn add_parameter(&mut self, variable_id: VariableId, param_index: usize, size: usize, alignment: usize) -> isize {
        // 参数通常在调用者的栈帧中，这里只是记录
        let offset = -(param_index as isize + 1) * 8; // 简化：每个参数8字节

        let slot = StackSlot {
            variable_id,
            offset,
            size,
            alignment,
            slot_type: SlotType::Parameter,
        };

        self.variables.insert(variable_id, slot);
        offset
    }

    /// 分配栈空间
    fn allocate_stack_space(&mut self, size: usize, alignment: usize) -> isize {
        // 向对齐边界对齐
        let current_offset = self.frame_size as isize;
        let aligned_offset = (current_offset + (alignment as isize - 1)) & !(alignment as isize - 1);

        self.frame_size = (aligned_offset + size as isize) as usize;
        self.alignment = self.alignment.max(alignment);

        aligned_offset
    }

    /// 获取变量的栈偏移
    pub fn get_variable_offset(&self, variable_id: VariableId) -> Option<isize> {
        self.variables.get(&variable_id).map(|slot| slot.offset)
    }

    /// 计算最终栈帧大小（对齐后）
    pub fn compute_frame_size(&mut self) -> usize {
        // 对齐到16字节边界（函数调用约定要求）
        (self.frame_size + 15) & !15
    }
}
```

## 4. 与JIT代码生成集成

### 4.1 JIT生成器逃逸分析集成

```rust
// karte-codegen/src/escape_analysis_integration.rs

use crate::codegen::{CodeGenContext, FunctionBuilder};
use karte_hir::escape_analysis::*;
use karte_mir::lower::allocation_strategy::*;

/// JIT生成器的逃逸分析集成
pub struct EscapeAnalysisIntegration {
    /// 当前函数的逃逸分析结果
    current_escape_analysis: HashMap<VariableId, VariableEscapeInfo>,

    /// 分配策略选择器
    allocation_selector: AllocationStrategySelector,
}

impl EscapeAnalysisIntegration {
    /// 为函数设置逃逸分析结果
    pub fn set_function_escape_analysis(
        &mut self,
        function_id: FunctionId,
        escape_analysis: HashMap<VariableId, VariableEscapeInfo>,
    ) {
        self.current_escape_analysis = escape_analysis.clone();
        self.allocation_selector = AllocationStrategySelector::new(escape_analysis);
    }

    /// 生成变量分配代码
    pub fn generate_variable_allocation(
        &mut self,
        ctx: &mut CodeGenContext,
        variable_id: VariableId,
        var_type: &Type,
    ) -> Result<Value, CodeGenError> {
        let strategy = self.allocation_selector.select_allocation_strategy(variable_id, var_type);
        let instruction = self.allocation_selector.generate_allocation_instruction(strategy);

        match strategy {
            AllocationStrategy::Stack { offset, .. } => {
                // 生成栈分配代码
                let stack_ptr = ctx.get_stack_pointer();
                let slot_ptr = ctx.build_int_add(stack_ptr, ctx.build_const_i64(offset))?;

                // 如果需要初始化
                if var_type.requires_initialization() {
                    self.initialize_stack_memory(ctx, slot_ptr, var_type)?;
                }

                Ok(slot_ptr)
            }

            AllocationStrategy::Heap { size, gc_tracked, .. } => {
                if gc_tracked {
                    // 生成GC分配代码
                    let alloc_func = ctx.get_function("karte_gc_alloc");
                    let size_val = ctx.build_const_i64(size as i64);
                    let type_val = ctx.build_const_i64(self.get_gc_type_id(var_type) as i64);

                    ctx.build_call(alloc_func, &[size_val, type_val])
                } else {
                    // 生成原始分配代码
                    self.generate_raw_allocation(ctx, size)?
                }
            }

            AllocationStrategy::Inline { size, site } => {
                // 生成内联分配代码
                self.generate_inline_allocation(ctx, size, site, var_type)
            }

            AllocationStrategy::Register { register, .. } => {
                // 寄存器分配，无需生成代码
                ctx.get_register_value(register)
            }
        }
    }

    /// 生成变量释放代码
    pub fn generate_variable_cleanup(
        &mut self,
        ctx: &mut CodeGenContext,
        variable_id: VariableId,
    ) -> Result<(), CodeGenError> {
        if let Some(escape_info) = self.current_escape_analysis.get(&variable_id) {
            match &escape_info.allocation_suggestion {
                AllocationSuggestion::Stack { .. } => {
                    // 栈分配自动清理，无需生成代码
                    Ok(())
                }

                AllocationSuggestion::HeapAlloc { gc_tracked, .. } => {
                    if *gc_tracked {
                        // GC管理的对象无需显式释放
                        Ok(())
                    } else {
                        // 生成释放代码
                        let free_func = ctx.get_function("free");
                        let var_ptr = ctx.get_variable_value(variable_id)?;
                        ctx.build_call(free_func, &[var_ptr])?;
                        Ok(())
                    }
                }

                AllocationSuggestion::Inline { .. } => {
                    // 内联分配自动清理
                    Ok(())
                }

                AllocationSuggestion::RegisterAlloc { .. } => {
                    // 寄存器自动清理
                    Ok(())
                }
            }
        } else {
            Ok(())
        }
    }

    /// 在函数入口生成逃逸分析信息
    pub fn generate_function_prologue(
        &mut self,
        ctx: &mut CodeGenContext,
        function: &Function,
    ) -> Result<(), CodeGenError> {
        // 1. 设置函数的逃逸分析结果
        self.set_function_escape_analysis(function.id, function.escape_analysis.clone());

        // 2. 生成栈帧设置
        self.generate_stack_frame_setup(ctx, function)?;

        // 3. 分配栈空间给非逃逸变量
        self.allocate_non_escaping_variables(ctx, function)?;

        // 4. 插入GC安全点
        self.insert_gc_safepoint(ctx)?;

        Ok(())
    }

    /// 在函数出口生成清理代码
    pub fn generate_function_epilogue(
        &mut self,
        ctx: &mut CodeGenContext,
        function: &Function,
    ) -> Result<(), CodeGenError> {
        // 1. 清理堆分配的变量
        self.cleanup_heap_variables(ctx, function)?;

        // 2. 插入GC安全点
        self.insert_gc_safepoint(ctx)?;

        // 3. 恢复栈帧
        self.restore_stack_frame(ctx, function)?;

        Ok(())
    }

    /// 分配非逃逸变量到栈
    fn allocate_non_escaping_variables(
        &mut self,
        ctx: &mut CodeGenContext,
        function: &Function,
    ) -> Result<(), CodeGenError> {
        for (var_id, var_info) in &function.variables {
            if let Some(escape_info) = self.current_escape_analysis.get(var_id) {
                if escape_info.escape_state.can_stack_allocate() {
                    // 生成栈分配代码
                    self.generate_variable_allocation(ctx, *var_id, &var_info.var_type)?;

                    // 记录栈槽信息
                    ctx.add_stack_slot(*var_id, self.get_stack_offset(*var_id)?);
                }
            }
        }

        Ok(())
    }

    /// 获取变量的栈偏移
    fn get_stack_offset(&self, variable_id: VariableId) -> Result<isize, CodeGenError> {
        if let Some(escape_info) = self.current_escape_analysis.get(&variable_id) {
            if let AllocationSuggestion::StackAlloc { offset_from_fp, .. } = &escape_info.allocation_suggestion {
                Ok(*offset_from_fp)
            } else {
                Err(CodeGenError::VariableNotOnStack(variable_id))
            }
        } else {
            Err(CodeGenError::MissingEscapeAnalysis(variable_id))
        }
    }

    /// 插入GC安全点
    fn insert_gc_safepoint(&mut self, ctx: &mut CodeGenContext) -> Result<(), CodeGenError> {
        // 在适当位置插入GC安全点调用
        let safepoint_func = ctx.get_function("karte_gc_safepoint");

        // 传递当前栈指针等信息给GC
        let sp = ctx.get_stack_pointer();
        let fp = ctx.get_frame_pointer();

        ctx.build_call(safepoint_func, &[sp, fp])?;
        Ok(())
    }

    /// 其他辅助方法...
    fn initialize_stack_memory(&mut self, ctx: &mut CodeGenContext, ptr: Value, var_type: &Type) -> Result<(), CodeGenError> {
        // 实现栈内存初始化
        todo!("实现栈内存初始化")
    }

    fn generate_inline_allocation(&mut self, ctx: &mut CodeGenContext, size: usize, site: ExpressionId, var_type: &Type) -> Result<Value, CodeGenError> {
        // 实现内联分配
        todo!("实现内联分配")
    }

    fn get_gc_type_id(&self, var_type: &Type) -> u32 {
        // 将Karte类型映射到GC类型ID
        match var_type {
            Type::Number => 0, // Atomic
            Type::Bool => 0,
            Type::String => 2, // Complex
            Type::Array(_) => 2,
            Type::Struct(_) => 2,
            Type::Function(_) => 1, // Pointer
            Type::Closure(_) => 2,
            Type::Reference(_) => 1,
            _ => 2, // 默认为Complex
        }
    }
}

/// JIT代码生成器扩展
impl FunctionBuilder {
    /// 基于逃逸分析的变量分配
    pub fn allocate_variable_with_escape_analysis(
        &mut self,
        variable_id: VariableId,
        var_type: &Type,
    ) -> Result<Value, CodeGenError> {
        // 如果有逃逸分析结果，使用它
        if let Some(escape_integration) = self.get_escape_analysis_integration() {
            escape_integration.generate_variable_allocation(&mut self.ctx, variable_id, var_type)
        } else {
            // 回退到默认堆分配
            self.default_variable_allocation(variable_id, var_type)
        }
    }

    /// 生成逃逸分析感知的函数入口
    pub fn generate_function_entry_with_escape_analysis(
        &mut self,
        function: &Function,
    ) -> Result<(), CodeGenError> {
        if let Some(escape_integration) = self.get_escape_analysis_integration_mut() {
            escape_integration.generate_function_prologue(&mut self.ctx, function)
        } else {
            // 回退到默认函数入口
            self.default_function_prologue(function)
        }
    }

    /// 生成逃逸分析感知的函数出口
    pub fn generate_function_exit_with_escape_analysis(
        &mut self,
        function: &Function,
    ) -> Result<(), CodeGenError> {
        if let Some(escape_integration) = self.get_escape_analysis_integration_mut() {
            escape_integration.generate_function_epilogue(&mut self.ctx, function)
        } else {
            // 回退到默认函数出口
            self.default_function_epilogue(function)
        }
    }
}
```

## 5. 与现有Runtime的集成

### 5.1 基于逃逸分析的GC分配器

```rust
// karte-rt/src/memory/gc_escape_allocator.rs
use karte_gc::{gc_malloc, gc_malloc_no_collect, ObjectType};
use karte_hir::escape_analysis::{EscapeState, AllocationStrategy};

/// 基于逃逸分析的GC分配器
pub struct GcEscapeAllocator {
    /// 分配统计
    stats: AllocationStats,

    /// 当前函数的逃逸分析结果
    current_escape_analysis: HashMap<FunctionId, HashMap<VariableId, VariableEscapeInfo>>,

    /// 当前栈帧信息
    current_stack_frame: Option<StackFrameInfo>,
}

impl GcEscapeAllocator {
    pub fn new() -> Self {
        Self {
            stats: AllocationStats::default(),
            current_escape_analysis: HashMap::new(),
            current_stack_frame: None,
        }
    }

    /// 设置函数的逃逸分析结果
    pub fn set_function_escape_analysis(&mut self, function_id: FunctionId, analysis: HashMap<VariableId, VariableEscapeInfo>) {
        self.current_escape_analysis.insert(function_id, analysis);
    }

    /// 根据逃逸分析结果分配变量
    pub fn allocate_with_escape_analysis(
        &mut self,
        variable_id: VariableId,
        var_type: &Type,
        escape_info: &VariableEscapeInfo,
    ) -> Result<*mut u8, AllocError> {
        let strategy = self.select_allocation_strategy(variable_id, var_type, escape_info);

        match strategy {
            AllocationStrategy::Stack { offset, size, .. } => {
                // 栈分配 - 最优性能
                self.allocate_on_stack(offset, size, var_type)
            }

            AllocationStrategy::Heap { object_type, size, gc_tracked, .. } => {
                // 堆分配 - 使用GC
                if gc_tracked {
                    let gc_type = self.map_type_to_gc_type(object_type);
                    let ptr = unsafe {
                        gc_malloc(size, gc_type.into())
                    };

                    if ptr.is_null() {
                        // 触发GC后重试
                        unsafe { gc_collect(); }
                        let ptr = unsafe {
                            gc_malloc(size, gc_type.into())
                        };

                        if ptr.is_null() {
                            return Err(AllocError::OutOfMemory);
                        }

                        Ok(ptr)
                    } else {
                        Ok(ptr)
                    }
                } else {
                    // 非GC堆分配（特殊用途）
                    self.allocate_raw_heap(size)
                }
            }

            AllocationStrategy::Inline { size, site } => {
                // 内联分配
                self.allocate_inline(size, site, var_type)
            }

            AllocationStrategy::Register { register, .. } => {
                // 寄存器分配
                self.allocate_to_register(register, var_type)
            }
        }
    }

    /// 栈分配
    fn allocate_on_stack(&mut self, offset: isize, size: usize, var_type: &Type) -> Result<*mut u8, AllocError> {
        if let Some(stack_frame) = &self.current_stack_frame {
            let stack_ptr = unsafe { stack_frame.stack_pointer.add(offset as usize) };

            // 根据类型需要初始化内存
            if var_type.requires_initialization() {
                self.initialize_stack_memory(stack_ptr, size);
            }

            Ok(stack_ptr)
        } else {
            Err(AllocError::NoStackFrame)
        }
    }

    /// 选择分配策略
    fn select_allocation_strategy(
        &self,
        variable_id: VariableId,
        var_type: &Type,
        escape_info: &VariableEscapeInfo,
    ) -> AllocationStrategy {
        match &escape_info.allocation_suggestion {
            AllocationSuggestion::StackAlloc { stack_frame, offset_from_fp } => {
                AllocationStrategy::Stack {
                    stack_frame: *stack_frame,
                    offset: *offset_from_fp,
                    size: var_type.size(),
                    alignment: var_type.alignment(),
                }
            }

            AllocationSuggestion::HeapAlloc { allocation_site, object_type, size_estimate, .. } => {
                AllocationStrategy::Heap {
                    object_type: *object_type,
                    size: *size_estimate,
                    allocation_site: *allocation_site,
                    gc_tracked: true,
                }
            }

            AllocationSuggestion::InlineAlloc { inline_site, .. } => {
                AllocationStrategy::Inline {
                    size: var_type.size().min(32), // 限制为32字节
                    site: *inline_site,
                }
            }

            AllocationSuggestion::RegisterAlloc { preferred_register, .. } => {
                AllocationStrategy::Register {
                    register: *preferred_register,
                    size: var_type.size(),
                }
            }
        }
    }

    /// 映射类型到GC类型
    fn map_type_to_gc_type(&self, type_id: TypeId) -> ObjectType {
        match self.get_type_info(type_id) {
            Type::Number => ObjectType::Atomic,
            Type::Bool => ObjectType::Atomic,
            Type::String => ObjectType::Complex,
            Type::Array(_) => ObjectType::Complex,
            Type::Struct(_) => ObjectType::Complex,
            Type::Function(_) => ObjectType::Pointer,
            Type::Closure(_) => ObjectType::Complex,
            Type::Reference(_) => ObjectType::Pointer,
            _ => ObjectType::Complex,
        }
    }
}
```

## 6. 实施计划

### 阶段1：核心GC集成（2周）
- [x] 分析Karte虚拟栈和Immix堆架构
- [ ] 创建`karte-gc` crate并集成Immix
- [ ] 实现Karte专用的根扫描器
- [ ] 实现保守对象扫描器
- [ ] 基础功能测试

### 阶段2：JIT集成（2周）
- [ ] 在ExecutionEngine中添加GC初始化
- [ ] 实现GC安全点机制
- [ ] 修改JIT代码生成器插入安全点
- [ ] 实现VM状态保存/恢复
- [ ] JIT集成测试

### 阶段3：分配器替换（1-2周）
- [ ] 用GC分配器替换现有分配器
- [ ] 移除所有RC相关代码
- [ ] 修复编译错误和链接问题
- [ ] 内存分配测试

### 阶段4：逃逸分析核心（4-5周）
- [ ] 实现基础逃逸分析算法
- [ ] 构建变量依赖图
- [ ] 实现逃逸状态传播
- [ ] 添加循环和闭包支持
- [ ] 单元测试和性能测试

### 阶段5：分配策略（2-3周）
- [ ] 实现分配策略选择器
- [ ] 优化栈帧布局
- [ ] 集成逃逸分析结果
- [ ] 实现分配指令生成
- [ ] 性能测试

### 阶段6：MIR集成（2-3周）
- [ ] 在MIR lowering中集成逃逸分析
- [ ] 修改MIR指令集支持栈分配
- [ ] 实现逃逸感知的变量管理
- [ ] 添加内联优化
- [ ] MIR集成测试

### 阶段7：JIT优化集成（2-3周）
- [ ] 在JIT代码生成中支持栈分配
- [ ] 实现逃逸感知的GC安全点
- [ ] 优化函数调用约定
- [ ] 添加闭包优化
- [ ] JIT集成测试

### 阶段8：性能优化（1-2周）
- [ ] 优化逃逸分析算法性能
- [ ] 实现增量逃逸分析
- [ ] 添加更多优化启发式规则
- [ ] 性能基准测试

### 阶段9：测试和文档（1周）
- [ ] 全面的集成测试
- [ ] 性能基准测试
- [ ] 内存使用分析
- [ ] 文档和示例更新

## 7. 预期性能提升

### 7.1 内存分配优化
- **栈分配比例**: 60-80%的变量分配在栈上
- **堆分配减少**: 减少70-90%的不必要堆分配
- **内存碎片减少**: 栈分配无碎片，堆对象更少意味着更少碎片

### 7.2 GC性能提升
- **GC频率降低**: 堆对象减少70-90%，GC频率降低80-95%
- **GC暂停时间**: 平均暂停时间从10-50ms降低到1-5ms
- **GC吞吐量**: GC吞吐量提升300-500%

### 7.3 整体性能提升
- **分配延迟**: 栈分配延迟<1ns，比堆分配快100-1000倍
- **缓存性能**: 栈分配的数据具有更好的缓存局部性
- **整体执行速度**: 预期整体性能提升50-200%
- **内存使用**: 总内存使用减少40-70%

## 8. 调试和监控

### 8.1 逃逸分析调试工具

```rust
/// 逃逸分析调试器
pub struct EscapeAnalysisDebugger {
    /// 分析结果
    analysis_results: HashMap<FunctionId, HashMap<VariableId, VariableEscapeInfo>>,

    /// 统计信息
    statistics: EscapeAnalysisStats,
}

#[derive(Default)]
pub struct EscapeAnalysisStats {
    pub total_variables: usize,
    pub stack_allocated: usize,
    pub heap_allocated: usize,
    pub inlined_allocated: usize,
    pub register_allocated: usize,
}

impl EscapeAnalysisDebugger {
    /// 生成分析报告
    pub fn generate_report(&self) -> String {
        let stats = &self.statistics;
        let stack_ratio = stats.stack_allocated as f64 / stats.total_variables as f64;
        let heap_ratio = stats.heap_allocated as f64 / stats.total_variables as f64;

        format!(
            "Escape Analysis Report:\n\
            - Total variables: {}\n\
            - Stack allocated: {} ({:.1}%)\n\
            - Heap allocated: {} ({:.1}%)\n\
            - Inlined allocated: {} ({:.1}%)\n\
            - Register allocated: {} ({:.1}%)\n\
            - Expected memory reduction: {:.1}%\n\
            - Expected GC pressure reduction: {:.1}%",
            stats.total_variables,
            stats.stack_allocated,
            stack_ratio * 100.0,
            stats.heap_allocated,
            heap_ratio * 100.0,
            stats.inlined_allocated,
            stats.inlined_allocated as f64 / stats.total_variables as f64 * 100.0,
            stats.register_allocated,
            stats.register_allocated as f64 / stats.total_variables as f64 * 100.0,
            stack_ratio * 80.0, // 估算内存减少
            stack_ratio * 90.0   // 估算GC压力减少
        )
    }

    /// 生成变量详情报告
    pub fn generate_variable_report(&self, function_id: FunctionId) -> Option<String> {
        if let Some(variables) = self.analysis_results.get(&function_id) {
            let mut report = format!("Function {} variable escape analysis:\n", function_id);

            for (var_id, escape_info) in variables {
                let allocation_type = match &escape_info.allocation_suggestion {
                    AllocationSuggestion::StackAlloc { .. } => "Stack",
                    AllocationSuggestion::HeapAlloc { .. } => "Heap",
                    AllocationSuggestion::InlineAlloc { .. } => "Inline",
                    AllocationSuggestion::RegisterAlloc { .. } => "Register",
                };

                report.push_str(&format!(
                    "  Variable {}: {} (escape: {:?})\n",
                    var_id,
                    allocation_type,
                    escape_info.escape_state
                ));
            }

            Some(report)
        } else {
            None
        }
    }
}
```

通过将逃逸分析与Immix GC深度集成，我们可以在编译时就决定变量的分配策略，大幅减少不必要的堆分配和GC压力，显著提升程序性能。这个方案为Karte的长期发展奠定了坚实的内存管理基础。