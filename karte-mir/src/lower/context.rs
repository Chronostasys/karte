/// 上下文管理模块
///
/// 本模块包含LoweringContext的所有实现方法：
/// - 上下文创建和初始化
/// - 函数管理（开始、完成、获取当前函数）
/// - 基本块管理（创建、切换）
/// - 作用域管理（进入、退出、恢复）
/// - 变量绑定管理（绑定、查找、更新）
/// - 语句和终结器添加
use super::types::{LoweringContext, ScopeFrame, VariableBinding};
use crate::{BasicBlockId, MirFunction, Statement, Terminator, Value};
use karte_common::memory::OwnershipKind;
use karte_diagnostics::Span;

impl<'a> LoweringContext<'a> {
    /// 创建新的LoweringContext
    pub fn new(program: &'a mut crate::MirProgram) -> Self {
        let mut ctx = Self {
            program,
            current_function_name: None,
            current_block: None,
            scopes: Vec::new(),
            errors: Vec::new(),
            lambda_counter: 0,
            external_functions: Default::default(),
            module_context: None,
            temp_value_map: std::collections::HashMap::new(),
            function_return_types: std::collections::HashMap::new(),
            temp_types: std::collections::HashMap::new(),
            expr_types: std::collections::HashMap::new(),
            analysis_mode: false,
        };
        ctx.enter_scope();
        ctx
    }

    /// 解析模块标识符的规范化名称
    ///
    /// 将模块路径转换为规范化的模块标识符，考虑别名和依赖关系
    pub(crate) fn canonical_module_symbol(&self, module_path: &[String], symbol: &str) -> String {
        let module_identifier = self
            .resolve_module_identifier(module_path)
            .unwrap_or_else(|| module_path.join("."));
        format!("{}::{}", module_identifier, symbol)
    }

    /// 解析模块路径为模块标识符
    ///
    /// 处理直接路径和别名引用
    pub(crate) fn resolve_module_identifier(&self, module_path: &[String]) -> Option<String> {
        if module_path.is_empty() {
            return None;
        }

        let direct = module_path.join(".");
        if let Some(ctx) = &self.module_context {
            if ctx.dependency_interfaces.contains_key(&direct) {
                return Some(direct);
            }

            let alias = module_path.first()?.as_str();
            if let Some(binding) = ctx
                .imports
                .iter()
                .find(|binding| binding.symbol == "*" && binding.alias == alias)
            {
                let mut resolved = binding.module_path.clone();
                if module_path.len() > 1 {
                    resolved.extend_from_slice(&module_path[1..]);
                }
                return Some(resolved.join("."));
            }
        }

        Some(direct)
    }

    /// 检查是否为已知函数
    pub(crate) fn is_known_function(&self, name: &str) -> bool {
        self.program.functions.contains_key(name) || self.external_functions.contains(name)
    }

    /// 开始新函数
    ///
    /// 创建新的MirFunction并初始化其作用域
    pub fn start_function(&mut self, name: String, params: Vec<String>) {
        let function = MirFunction::new(name.clone(), params.clone());
        let entry_block = function.entry_block;

        self.scopes.clear();
        self.temp_value_map.clear(); // 清空临时变量映射，避免不同函数间的TempId冲突
        self.enter_scope();

        // 将参数添加到变量作用域
        for param in params {
            self.bind_variable(
                param.clone(),
                Value::Variable {
                    name: param.clone(),
                    ty: None,
                },
                None,
            );
        }

        self.program.add_function(function);
        self.current_function_name = Some(name);
        self.current_block = Some(entry_block);
    }

    /// 完成当前函数
    pub fn finish_function(&mut self) {
        self.current_function_name = None;
        self.current_block = None;
    }

    /// 获取当前函数的可变引用
    pub(crate) fn current_function_mut(&mut self) -> &mut MirFunction {
        let name = self
            .current_function_name
            .as_ref()
            .expect("No current function");
        self.program
            .functions
            .get_mut(name)
            .expect("Current function not found in program")
    }

    /// 获取当前基本块ID
    pub(crate) fn current_block(&self) -> BasicBlockId {
        self.current_block.expect("No current block")
    }

    /// 设置当前基本块
    pub(crate) fn set_current_block(&mut self, block: BasicBlockId) {
        self.current_block = Some(block);
    }

    /// 创建新的基本块
    pub(crate) fn new_block(&mut self) -> BasicBlockId {
        self.current_function_mut().new_block()
    }

    pub(crate) fn remove_block(&mut self, id: BasicBlockId) {
        self.current_function_mut().remove_block(id);
    }

    /// 创建新的临时变量
    pub(crate) fn new_temp(&mut self) -> Value {
        let id = self.current_function_mut().new_temp();
        Value::Temp { id, ty: None }
    }

    /// 进入新的作用域
    pub(crate) fn enter_scope(&mut self) {
        self.scopes.push(ScopeFrame::new());
    }

    /// 退出当前作用域
    ///
    /// 释放作用域内所有引用计数的变量
    pub(crate) fn exit_scope(&mut self, span: Span) {
        if let Some(frame) = self.scopes.pop() {
            for name in frame.order.iter().rev() {
                if let Some(binding) = frame.bindings.get(name) {
                    self.release_binding(binding, span);
                }
            }
        }
    }

    /// 获取当前作用域的可变引用
    pub(crate) fn current_scope_mut(&mut self) -> &mut ScopeFrame {
        self.scopes
            .last_mut()
            .expect("at least one scope must exist")
    }

    /// 获取当前作用域的不可变引用
    pub(crate) fn current_scope(&self) -> &ScopeFrame {
        self.scopes
            .last()
            .expect("at least one scope must exist")
    }

    /// 绑定变量
    ///
    /// 在当前作用域中添加新的变量绑定
    pub(crate) fn bind_variable(
        &mut self,
        name: String,
        value: Value,
        ownership: Option<OwnershipKind>,
    ) {
        let frame = self.current_scope_mut();
        frame.order.push(name.clone());
        frame.bindings.insert(
            name,
            VariableBinding {
                value,
                ownership,
                moved: false,
            },
        );
    }

    /// 更新变量
    ///
    /// 在作用域链中查找变量并更新其值，返回旧的绑定
    pub(crate) fn update_variable(
        &mut self,
        name: &str,
        value: Value,
        ownership: Option<OwnershipKind>,
    ) -> Option<VariableBinding> {
        for frame in self.scopes.iter_mut().rev() {
            if let Some(binding) = frame.bindings.get_mut(name) {
                let old = binding.clone();
                binding.value = value;
                binding.ownership = ownership;
                binding.moved = false;
                return Some(old);
            }
        }
        None
    }

    /// 查找变量
    ///
    /// 在作用域链中查找变量绑定
    pub(crate) fn lookup_variable(&self, name: &str) -> Option<&VariableBinding> {
        for frame in self.scopes.iter().rev() {
            if let Some(binding) = frame.bindings.get(name) {
                return Some(binding);
            }
        }
        None
    }

    /// 释放变量绑定
    ///
    /// 对于引用计数的变量，生成Release语句
    pub(crate) fn release_binding(&mut self, binding: &VariableBinding, span: Span) {
        if binding.moved {
            return;
        }
        if matches!(binding.ownership, Some(OwnershipKind::RefCounted)) {
            self.add_statement(Statement::Release {
                value: binding.value.clone(),
                span,
            });
        }
    }

    /// 克隆作用域栈
    ///
    /// 用于保存上下文状态（如在处理嵌套函数时）
    pub(crate) fn clone_scopes(&self) -> Vec<ScopeFrame> {
        self.scopes.clone()
    }

    /// 恢复作用域栈
    ///
    /// 用于恢复之前保存的上下文状态
    pub(crate) fn restore_scopes(&mut self, scopes: Vec<ScopeFrame>) {
        self.scopes = scopes;
    }

    /// 添加语句到当前基本块
    pub(crate) fn add_statement(&mut self, stmt: Statement) {
        // 追踪函数值和闭包值的赋值
        if let Statement::Assign { target, source, .. } = &stmt {
            if let Value::Temp { id, .. } = target {
                let should_track = match source {
                    Value::Function { .. } => true,
                    Value::Closure { .. } => true,
                    Value::Struct { name, .. } => name == "Closure",
                    _ => false,
                };
                if should_track {
                    self.temp_value_map.insert(*id, source.clone());
                }
            }
        }

        let block_id = self.current_block();
        if let Some(block) = self.current_function_mut().get_block_mut(block_id) {
            block.add_statement(stmt);
        }
    }

    /// 解析值的实际类型
    ///
    /// 如果值是临时变量且映射到函数/闭包，返回实际的函数/闭包值
    pub(crate) fn resolve_value(&self, value: &Value) -> Value {
        if let Value::Temp { id, .. } = value {
            if let Some(actual_value) = self.temp_value_map.get(id) {
                return actual_value.clone();
            }
        }
        value.clone()
    }

    /// 设置当前基本块的终结语句
    pub(crate) fn set_terminator(&mut self, terminator: Terminator) {
        let block_id = self.current_block();
        if let Some(block) = self.current_function_mut().get_block_mut(block_id) {
            block.set_terminator(terminator);
        }
    }

    /// 解析类型注解字符串为HIR Type
    ///
    /// 支持的类型注解：
    /// - "number" -> Type::Number
    /// - "()" -> Type::Unit
    /// - "fn(...) -> ..." -> Type::Function
    /// - "&T" -> Type::Reference
    pub(crate) fn parse_type_annotation(&self, type_str: &str) -> Option<karte_hir::Type> {
        let trimmed = type_str.trim();

        // 基本类型
        match trimmed {
            "number" => return Some(karte_hir::Type::Number),
            "()" | "unit" => return Some(karte_hir::Type::Unit),
            _ => {}
        }

        // 引用类型：&T
        if let Some(inner_str) = trimmed.strip_prefix('&') {
            if let Some(inner_type) = self.parse_type_annotation(inner_str) {
                return Some(karte_hir::Type::reference(inner_type));
            }
        }

        // 函数类型：fn(...) -> ...
        // 简化实现：只支持基本的函数类型语法
        if trimmed.starts_with("fn") {
            // 对于复杂的函数类型，暂时返回None
            // 完整实现需要一个完整的类型解析器
            return None;
        }

        // 检查是否是自定义类型（如struct或enum）
        // 这里可以查询已知的自定义类型
        None
    }

    /// 注册函数的返回类型
    ///
    /// 从函数定义的返回类型注解中提取类型信息并存储
    pub(crate) fn register_function_return_type(
        &mut self,
        func_name: String,
        return_type: karte_hir::Type,
    ) {
        self.function_return_types.insert(func_name, return_type);
    }

    /// 查询函数的返回类型
    ///
    /// 如果函数返回类型已知，返回其类型；否则返回None
    pub(crate) fn get_function_return_type(&self, func_name: &str) -> Option<&karte_hir::Type> {
        self.function_return_types.get(func_name)
    }

    /// 检查类型是否为函数或闭包类型
    ///
    /// 用于判断函数调用的返回值是否需要特殊处理
    pub(crate) fn is_callable_type(ty: &karte_hir::Type) -> bool {
        matches!(
            ty,
            karte_hir::Type::Function { .. } | karte_hir::Type::Closure { .. }
        )
    }

    /// 获取表达式的推断类型
    ///
    /// 从expr_types映射中查询表达式的类型，如果找不到返回None
    pub(crate) fn get_expr_type(&self, expr: &karte_hir::Expr) -> karte_hir::Type {
        let expr_ptr = expr as *const karte_hir::Expr;
        let key = expr_ptr as usize;
        self.expr_types
            .get(&key)
            .cloned()
            .unwrap_or(karte_hir::Type::Unknown)
    }

    /// 获取Lambda表达式的推断类型
    ///
    /// 专门用于Lambda表达式，返回函数或闭包类型
    pub(crate) fn get_lambda_type(&self, expr: &karte_hir::Expr) -> Option<karte_hir::Type> {
        let expr_ptr = expr as *const karte_hir::Expr;
        let key = expr_ptr as usize;
        self.expr_types.get(&key).cloned()
    }
}
