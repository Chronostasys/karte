use crate::ast::{BinaryOperator, Expr, FieldDef, Statement, UnaryOperator};
use crate::errors::TypeCheckError;
use crate::types::{IntKind, Type, TypeScheme, TypeValue, TypeVar};
use ena::unify::InPlaceUnificationTable;
use karte_diagnostics::DiagnosticBag;
use std::collections::HashMap;
use std::collections::HashSet;

/// 类型环境 - 存储变量的类型信息
type TypeEnvironment = HashMap<String, Type>;

/// 约束条件
#[derive(Debug, Clone)]
pub struct Constraint {
    pub left: Type,
    pub right: Type,
    pub span: karte_diagnostics::Span,
    /// 约束的上下文描述（用于改进错误消息）
    pub context: Option<String>,
}

/// 模块上下文：携带 module/import 元信息
#[derive(Debug, Clone, Default)]
pub struct ModuleContext {
    pub module_name: Option<String>,
    pub imports: Vec<ImportBinding>,
    pub dependency_interfaces: HashMap<String, ExternalModuleInterface>,
}

impl ModuleContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_import_symbol(&mut self, module_path: Vec<String>, symbol: String, alias: String) {
        self.imports.push(ImportBinding {
            module_path,
            symbol,
            alias,
        });
    }

    pub fn set_dependency_interfaces(
        &mut self,
        interfaces: HashMap<String, ExternalModuleInterface>,
    ) {
        self.dependency_interfaces = interfaces;
    }

    pub fn dependency_interfaces(&self) -> &HashMap<String, ExternalModuleInterface> {
        &self.dependency_interfaces
    }
}

/// 单个 import 绑定信息
#[derive(Debug, Clone)]
pub struct ImportBinding {
    pub module_path: Vec<String>,
    pub symbol: String,
    pub alias: String,
}

#[derive(Debug, Clone, Default)]
pub struct ExternalModuleInterface {
    pub functions: HashMap<String, ExternalFunctionSignature>,
    pub structs: HashMap<String, ExternalStructSignature>,
}

#[derive(Debug, Clone)]
pub struct ExternalFunctionSignature {
    pub name: String,
    pub params: usize,
}

#[derive(Debug, Clone)]
pub struct ExternalStructSignature {
    pub name: String,
    pub fields: Vec<ExternalStructField>,
}

#[derive(Debug, Clone)]
pub struct ExternalStructField {
    pub name: String,
    pub ty: String,
}

/// 改进的类型检查器，支持类型推断
pub struct TypeChecker {
    diagnostics: DiagnosticBag,
    unification_table: InPlaceUnificationTable<TypeVar>,
    next_type_var: u32,
    constraints: Vec<Constraint>,
    custom_types: HashMap<String, Type>, // 存储自定义类型
    /// 存储泛型类型定义模板（带类型参数的 struct/enum 定义）
    /// key = 类型名（如 "Pair"），value = (type_params, 模板类型)
    generic_type_defs: HashMap<String, GenericTypeDef>,
    module_context: ModuleContext,
    function_signatures: HashMap<String, FunctionSignature>, // 缓存函数签名，避免重复解析
    /// 存储Lambda表达式的推断类型（保持向后兼容）
    lambda_types: HashMap<*const Expr, Type>,
    /// 存储所有表达式的推断类型（用于传递给MIR lowering）
    expr_types: HashMap<*const Expr, Type>,
    /// 泛型函数的类型方案（TypeScheme），用于 let-polymorphism
    function_schemes: HashMap<String, TypeScheme>,
    /// 追踪重复函数定义的 span（与 primary 不同），用于 infer_stmt 阶段报告错误
    duplicate_function_spans: HashSet<(usize, usize)>,
    /// 追踪已在 hoisting pass 中处理过的函数 span，用于区分重复定义与二次遍历
    primary_function_spans: HashSet<(usize, usize)>,
    /// 未使用变量/函数警告追踪：记录定义的变量 (span.start, span.end) -> name
    defined_locals: Vec<HashMap<(usize, usize), String>>,
    /// 记录已使用的定义 span
    used_definitions: HashSet<(usize, usize)>,
}

/// 泛型类型定义模板
#[derive(Debug, Clone)]
struct GenericTypeDef {
    /// 类型参数名称列表，如 ["T", "E"]
    type_params: Vec<String>,
    /// 模板类型（字段/变体中使用 Type::Var 作为类型参数占位符）
    template: Type,
}

/// 函数签名，包含参数类型和返回类型
#[derive(Debug, Clone)]
struct FunctionSignature {
    param_types: Vec<Type>,
    return_type: Type,
    /// 是否有显式的返回类型标注
    has_explicit_return_type: bool,
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeChecker {
    pub fn new() -> Self {
        Self {
            diagnostics: DiagnosticBag::new(),
            unification_table: InPlaceUnificationTable::new(),
            next_type_var: 0,
            constraints: Vec::new(),
            custom_types: HashMap::new(),
            generic_type_defs: HashMap::new(),
            module_context: ModuleContext::default(),
            function_signatures: HashMap::new(),
            lambda_types: HashMap::new(),
            expr_types: HashMap::new(),
            function_schemes: HashMap::new(),
            duplicate_function_spans: HashSet::new(),
            primary_function_spans: HashSet::new(),
            defined_locals: vec![HashMap::new()],
            used_definitions: HashSet::new(),
        }
    }

    /// 获取所有表达式的类型映射，转换为usize键
    /// 注意：方法名保持为get_lambda_types以保持向后兼容，但实际返回所有表达式类型
    pub fn get_lambda_types(&self) -> HashMap<usize, Type> {
        self.expr_types
            .iter()
            .map(|(expr_ptr, ty)| (*expr_ptr as usize, ty.clone()))
            .collect()
    }

    /// 生成新的类型变量
    fn fresh_type_var(&mut self) -> TypeVar {
        let var = TypeVar(self.next_type_var);
        self.next_type_var += 1;
        self.unification_table.new_key(TypeValue(None));
        var
    }

    /// 记录变量定义
    fn define_local(&mut self, name: &str, span: karte_diagnostics::Span) {
        if let Some(scope) = self.defined_locals.last_mut() {
            scope.insert((span.start, span.end), name.to_string());
        }
    }

    /// 记录变量使用（通过变量名在所有作用域中查找）
    fn mark_used_by_name(&mut self, name: &str) {
        for scope in self.defined_locals.iter().rev() {
            for (span_key, var_name) in scope.iter() {
                if var_name == name {
                    self.used_definitions.insert(*span_key);
                    return;
                }
            }
        }
    }

    /// 调试用：打印当前 defined_locals 的状态
    #[allow(dead_code)]
    fn debug_print_locals(&self, context: &str) {
        eprintln!("[DEBUG defined_locals: {}]", context);
        for (i, scope) in self.defined_locals.iter().enumerate() {
            eprintln!("  scope[{}]: {:?}", i, scope);
        }
        eprintln!("  used: {:?}", self.used_definitions);
    }

    /// 描述类型不匹配的上下文
    fn describe_type_mismatch(&self, expected: &Type, found: &Type) -> Option<String> {
        match (expected, found) {
            (Type::Number, Type::String) => Some("算术运算需要 number 类型，但得到了 string".to_string()),
            (Type::String, Type::Number) => Some("期望 string 类型，但得到了 number".to_string()),
            (Type::Number, Type::Bool) => Some("期望 number 类型，但得到了 bool".to_string()),
            (Type::Bool, Type::Number) => Some("期望 bool 类型，但得到了 number".to_string()),
            (Type::Number, Type::Unit) => Some("期望 number 类型，但得到了空值 (())".to_string()),
            (Type::Function { .. }, Type::Number) => Some("期望函数类型，但得到了 number".to_string()),
            (Type::Function { .. }, Type::String) => Some("期望函数类型，但得到了 string".to_string()),
            (Type::Array { .. }, Type::Number) => Some("期望数组类型，但得到了 number".to_string()),
            (Type::Array { .. }, Type::String) => Some("期望数组类型，但得到了 string".to_string()),
            (Type::Sum { name, .. }, Type::Number) => Some(format!("期望 {} 类型，但得到了 number", name)),
            (Type::Sum { name, .. }, Type::String) => Some(format!("期望 {} 类型，但得到了 string", name)),
            (Type::Struct { name, .. }, Type::Number) => Some(format!("期望 struct {} 类型，但得到了 number", name)),
            _ => None,
        }
    }

    /// 从模式中提取变量定义
    fn define_pattern_locals(&mut self, pattern: &crate::ast::Pattern) {
        use crate::ast::Pattern;
        match pattern {
            Pattern::Variable { name, span } => {
                self.define_local(name, *span);
            }
            Pattern::Constructor { args, .. } => {
                for arg in args {
                    self.define_pattern_locals(arg);
                }
            }
            Pattern::QualifiedConstructor { args, .. } => {
                for arg in args {
                    self.define_pattern_locals(arg);
                }
            }
            Pattern::Struct { fields, .. } => {
                for field in fields {
                    self.define_pattern_locals(&field.pattern);
                }
            }
            _ => {}
        }
    }

    /// 进入新的作用域
    fn push_local_scope(&mut self) {
        self.defined_locals.push(HashMap::new());
    }

    /// 退出作用域，检查未使用的变量并发出警告
    fn pop_local_scope(&mut self, suppress_warnings: bool) {
        if let Some(scope) = self.defined_locals.pop() {
            if !suppress_warnings {
                for (span_key, name) in &scope {
                    // 跳过以 _ 开头的变量（约定为 intentionally unused）
                    if name.starts_with('_') {
                        continue;
                    }
                    // 跳过 main 函数（入口点，不需要被使用）
                    if name == "main" {
                        continue;
                    }
                    // 跳过函数定义（函数可能只被其他模块引用）
                    // TODO: 未来可以根据函数是否在同一模块中被引用来判断
                    if self.function_signatures.contains_key(name.as_str()) {
                        continue;
                    }
                    if !self.used_definitions.contains(span_key) {
                        // 未使用的变量，发出警告
                        self.diagnostics.diagnostics.push(
                            karte_diagnostics::Diagnostic::warning(
                                format!("未使用的变量: `{}` (如果故意不使用，请添加下划线前缀: `_{})", name, name),
                                karte_diagnostics::Span::new(span_key.0, span_key.1),
                            )
                        );
                    }
                }
            }
        }
    }

    /// 计算两个字符串之间的编辑距离（Levenshtein distance）
    fn levenshtein(a: &str, b: &str) -> usize {
        let a_len = a.chars().count();
        let b_len = b.chars().count();
        if a_len == 0 { return b_len; }
        if b_len == 0 { return a_len; }

        let mut matrix = vec![vec![0; b_len + 1]; a_len + 1];
        for (i, row) in matrix.iter_mut().enumerate() {
            row[0] = i;
        }
        for j in 0..=b_len {
            matrix[0][j] = j;
        }

        for (i, ca) in a.chars().enumerate() {
            for (j, cb) in b.chars().enumerate() {
                let cost = if ca == cb { 0 } else { 1 };
                matrix[i + 1][j + 1] = (matrix[i][j + 1] + 1)
                    .min(matrix[i + 1][j] + 1)
                    .min(matrix[i][j] + cost);
            }
        }

        matrix[a_len][b_len]
    }

    /// 为未定义的变量名找到最相似的已定义变量名
    fn suggest_variable(&self, name: &str, env: &TypeEnvironment) -> Option<String> {
        let mut best: Option<(String, usize)> = None;
        let max_dist = (name.len() / 2).max(2); // 最大允许距离

        for var_name in env.keys() {
            // 跳过完全不同的长度
            if var_name.len().abs_diff(name.len()) > max_dist {
                continue;
            }
            let dist = Self::levenshtein(name, var_name);
            if dist <= max_dist {
                match &best {
                    None => best = Some((var_name.clone(), dist)),
                    Some((_, best_dist)) if dist < *best_dist => {
                        best = Some((var_name.clone(), dist));
                    }
                    _ => {}
                }
            }
        }

        // 也检查函数名
        for fn_name in self.function_signatures.keys() {
            if fn_name.len().abs_diff(name.len()) > max_dist {
                continue;
            }
            let dist = Self::levenshtein(name, fn_name);
            if dist <= max_dist {
                match &best {
                    None => best = Some((fn_name.clone(), dist)),
                    Some((_, best_dist)) if dist < *best_dist => {
                        best = Some((fn_name.clone(), dist));
                    }
                    _ => {}
                }
            }
        }

        best.map(|(name, _)| name)
    }

    /// 实例化类型方案：将 bound_vars 替换为新的类型变量
    /// 每次调用泛型函数时，生成一组新的类型变量
    fn instantiate(&mut self, scheme: &TypeScheme) -> Type {
        if scheme.bound_vars.is_empty() {
            return scheme.body.clone();
        }
        let subst: std::collections::HashMap<TypeVar, Type> = scheme.bound_vars.iter()
            .map(|var| (*var, Type::Var(self.fresh_type_var())))
            .collect();
        scheme.body.substitute(&subst)
    }

    /// 添加约束条件
    ///
    /// 一般来说，期望的类型在左边，实际的类型在右边
    fn add_constraint(&mut self, left: Type, right: Type, span: karte_diagnostics::Span) {
        self.constraints.push(Constraint { left, right, span, context: None });
    }

    /// 添加带上下文描述的约束条件
    fn add_constraint_with_context(
        &mut self,
        left: Type,
        right: Type,
        span: karte_diagnostics::Span,
        context: impl Into<String>,
    ) {
        self.constraints.push(Constraint { left, right, span, context: Some(context.into()) });
    }

    /// 统一两个类型
    fn unify(
        &mut self,
        t1: &Type,
        t2: &Type,
        span: karte_diagnostics::Span,
        orig_t1: &Type,
        orig_t2: &Type,
    ) -> Result<(), ()> {
        let mut visited = HashSet::new();
        self.unify_recursive(t1, t2, span, orig_t1, orig_t2, &mut visited)
    }

    fn unify_recursive(
        &mut self,
        t1: &Type,
        t2: &Type,
        span: karte_diagnostics::Span,
        orig_t1: &Type,
        orig_t2: &Type,
        visited: &mut HashSet<(String, String)>,
    ) -> Result<(), ()> {
        // 解析骨架占位符：将 Struct{name, fields: []} 替换为 custom_types 中的实际类型
        // 这对递归枚举至关重要（如 enum List { Nil, Cons(number, List) } 中 List 自引用）
        // 利用 visited 集合防止递归类型的无限循环
        let resolved_t1;
        let t1 = match t1 {
            Type::Struct { name, fields } if fields.is_empty() => {
                if let Some(resolved) = self.custom_types.get(name) {
                    resolved_t1 = resolved.clone();
                    &resolved_t1
                } else {
                    t1
                }
            }
            _ => t1,
        };
        let resolved_t2;
        let t2 = match t2 {
            Type::Struct { name, fields } if fields.is_empty() => {
                if let Some(resolved) = self.custom_types.get(name) {
                    resolved_t2 = resolved.clone();
                    &resolved_t2
                } else {
                    t2
                }
            }
            _ => t2,
        };

        match (t1, t2) {
            (Type::Number, Type::Number) => Ok(()),
            // Number 与具体整数类型兼容：number 字面量可以传递给 Int 类型参数
            (Type::Number, Type::Int(_)) | (Type::Int(_), Type::Number) => Ok(()),
            (Type::Unit, Type::Unit) => Ok(()),
            (Type::Bool, Type::Bool) => Ok(()),
            (Type::String, Type::String) => Ok(()),
            (Type::Int(k1), Type::Int(k2)) if k1 == k2 => Ok(()),
            (Type::Array { element: e1 }, Type::Array { element: e2 }) => {
                self.unify_recursive(e1, e2, span, orig_t1, orig_t2, visited)
            }

            (Type::Var(v1), Type::Var(v2)) if v1 == v2 => Ok(()),

            (Type::Var(var), ty) | (ty, Type::Var(var)) => {
                // 检查是否会产生无限类型（短路求值，不分配 Vec）
                if ty.contains_var(var) {
                    self.add_error(TypeCheckError::InfiniteType {
                        var: *var,
                        ty: ty.clone(),
                        span,
                    });
                    return Err(());
                }

                // 尝试统一
                let current_value = self.unification_table.probe_value(*var);
                match &current_value.0 {
                    None => {
                        self.unification_table
                            .union_value(*var, TypeValue(Some(ty.clone())));
                        Ok(())
                    }
                    Some(existing) => {
                        self.unify_recursive(existing, ty, span, orig_t1, orig_t2, visited)
                    }
                }
            }

            (
                Type::Function {
                    params: p1,
                    return_type: r1,
                },
                Type::Function {
                    params: p2,
                    return_type: r2,
                },
            ) => {
                if p1.len() != p2.len() {
                    self.add_error(TypeCheckError::ArityMismatch {
                        expected: p1.len(),
                        found: p2.len(),
                        span,
                    });
                    return Err(());
                }

                // 统一参数类型
                for (param1, param2) in p1.iter().zip(p2.iter()) {
                    self.unify_recursive(param1, param2, span, orig_t1, orig_t2, visited)?;
                }

                // 统一返回类型
                self.unify_recursive(r1, r2, span, orig_t1, orig_t2, visited)
            }

            // Function 和 Closure 可以统一（它们在语义上是兼容的）
            (
                Type::Function {
                    params: p1,
                    return_type: r1,
                },
                Type::Closure {
                    params: p2,
                    return_type: r2,
                },
            )
            | (
                Type::Closure {
                    params: p1,
                    return_type: r1,
                },
                Type::Function {
                    params: p2,
                    return_type: r2,
                },
            )
            | (
                Type::Closure {
                    params: p1,
                    return_type: r1,
                },
                Type::Closure {
                    params: p2,
                    return_type: r2,
                },
            ) => {
                if p1.len() != p2.len() {
                    self.add_error(TypeCheckError::ArityMismatch {
                        expected: p1.len(),
                        found: p2.len(),
                        span,
                    });
                    return Err(());
                }

                // 统一参数类型
                for (param1, param2) in p1.iter().zip(p2.iter()) {
                    self.unify_recursive(param1, param2, span, orig_t1, orig_t2, visited)?;
                }

                // 统一返回类型
                self.unify_recursive(r1, r2, span, orig_t1, orig_t2, visited)
            }

            (
                Type::Sum {
                    name: n1,
                    variants: v1,
                },
                Type::Sum {
                    name: n2,
                    variants: v2,
                },
            ) => {
                let key = if n1 < n2 {
                    (n1.clone(), n2.clone())
                } else {
                    (n2.clone(), n1.clone())
                };
                if visited.contains(&key) {
                    return Ok(());
                }

                if n1 == n2 && v1.len() == v2.len() {
                    visited.insert(key);
                    // 检查每个变体的名称和数据类型是否能够统一
                    let mut all_unified = true;
                    for (variant1, variant2) in v1.iter().zip(v2.iter()) {
                        if variant1.name != variant2.name {
                            all_unified = false;
                            break;
                        }
                        match (&variant1.data_types, &variant2.data_types) {
                            (v1, v2) if v1.is_empty() && v2.is_empty() => {
                                // 两个都没有数据类型，匹配
                            }
                            (v1, v2) if v1.len() == v2.len() => {
                                // 尝试统一每个对应位置的类型
                                for (t1, t2) in v1.iter().zip(v2.iter()) {
                                    if self
                                        .unify_recursive(t1, t2, span, orig_t1, orig_t2, visited)
                                        .is_err()
                                    {
                                        all_unified = false;
                                        break;
                                    }
                                }
                            }
                            _ => {
                                // 数据类型数量不匹配
                                all_unified = false;
                                break;
                            }
                        }
                    }

                    if all_unified {
                        Ok(())
                    } else {
                        let expected = self.apply_substitution(orig_t1.clone());
                        let found = self.apply_substitution(orig_t2.clone());
                        self.add_error(TypeCheckError::TypeMismatch {
                            expected,
                            found,
                            span,
                            context: None,
                        });
                        Err(())
                    }
                } else {
                    let expected = self.apply_substitution(orig_t1.clone());
                    let found = self.apply_substitution(orig_t2.clone());
                    let context = self.describe_type_mismatch(&expected, &found);
                    self.add_error(TypeCheckError::TypeMismatch {
                        expected,
                        found,
                        span,
                        context,
                    });
                    Err(())
                }
            }

            (Type::Reference { inner: i1 }, Type::Reference { inner: i2 }) => {
                // 统一引用的内部类型
                self.unify_recursive(i1, i2, span, orig_t1, orig_t2, visited)
            }

            (Type::Tuple(ts1), Type::Tuple(ts2)) => {
                if ts1.len() != ts2.len() {
                    let expected = self.apply_substitution(orig_t1.clone());
                    let found = self.apply_substitution(orig_t2.clone());
                    self.add_error(TypeCheckError::TypeMismatch {
                        expected,
                        found,
                        span,
                        context: None,
                    });
                    return Err(());
                }
                for (t1_elem, t2_elem) in ts1.iter().zip(ts2.iter()) {
                    self.unify_recursive(t1_elem, t2_elem, span, orig_t1, orig_t2, visited)?;
                }
                Ok(())
            }

            (
                Type::Struct {
                    name: n1,
                    fields: f1,
                },
                Type::Struct {
                    name: n2,
                    fields: f2,
                },
            ) => {
                let key = if n1 < n2 {
                    (n1.clone(), n2.clone())
                } else {
                    (n2.clone(), n1.clone())
                };
                if visited.contains(&key) {
                    return Ok(());
                }

                if n1 != n2 {
                    let expected = self.apply_substitution(orig_t1.clone());
                    let found = self.apply_substitution(orig_t2.clone());
                    self.add_error(TypeCheckError::TypeMismatch {
                        expected,
                        found,
                        span,
                        context: None,
                    });
                    return Err(());
                }

                visited.insert(key);

                if f1.len() == f2.len() {
                    // 检查每个字段的类型是否匹配
                    for (field1, field2) in f1.iter().zip(f2.iter()) {
                        if field1.name != field2.name {
                            let expected = self.apply_substitution(orig_t1.clone());
                            let found = self.apply_substitution(orig_t2.clone());
                            self.add_error(TypeCheckError::TypeMismatch {
                                expected,
                                found,
                                span,
                                context: None,
                            });
                            return Err(());
                        }
                        // 递归统一字段类型
                        self.unify_recursive(
                            &field1.field_type,
                            &field2.field_type,
                            span,
                            orig_t1,
                            orig_t2,
                            visited,
                        )?;
                    }
                    Ok(())
                } else {
                    let expected = self.apply_substitution(orig_t1.clone());
                    let found = self.apply_substitution(orig_t2.clone());
                    self.add_error(TypeCheckError::TypeMismatch {
                        expected,
                        found,
                        span,
                        context: None,
                    });
                    Err(())
                }
            }

            (Type::Unknown, _) | (_, Type::Unknown) => Ok(()),

            // 特殊处理：当尝试统一非函数类型与函数/闭包类型时
            (non_func, Type::Function { .. })
            | (Type::Function { .. }, non_func)
            | (non_func, Type::Closure { .. })
            | (Type::Closure { .. }, non_func) => {
                if !matches!(non_func, Type::Var(_) | Type::Unknown) {
                    // 报告类型不匹配错误，而不是"not callable"错误
                    // 因为这里的问题是类型无法统一，而不是直接的函数调用问题
                    let expected = self.apply_substitution(orig_t1.clone());
                    let found = self.apply_substitution(orig_t2.clone());
                    self.add_error(TypeCheckError::TypeMismatch {
                        expected,
                        found,
                        span,
                        context: None,
                    });
                    return Err(());
                }
                Err(())
            }

            _ => {
                let expected = self.apply_substitution(orig_t1.clone());
                let found = self.apply_substitution(orig_t2.clone());
                self.add_error(TypeCheckError::TypeMismatch {
                    expected,
                    found,
                    span,
                    context: None,
                });
                Err(())
            }
        }
    }

    /// 检查整个程序
    pub fn check_program(&mut self, expr: &Expr) -> Type {
        self.check_program_with_context(expr, &ModuleContext::default())
    }

    pub fn check_program_with_context(&mut self, expr: &Expr, context: &ModuleContext) -> Type {
        // 首先收集所有结构体定义
        self.collect_struct_definitions(expr);

        // 在收集函数定义之前，先收集所有枚举（TypeDef）定义
        // 否则函数签名中的枚举类型引用会被 resolve_struct_field_from_parsed
        // 解析为空的 Struct 骨架，导致类型检查器看到空枚举
        self.collect_enum_definitions(expr);

        // 收集泛型类型定义模板（泛型 struct/enum）
        // 在函数签名收集之前完成，这样 resolve_struct_field_from_parsed
        // 能找到泛型类型并用 fresh type var 正确实例化
        self.collect_generic_type_defs(expr);

        let mut env = TypeEnvironment::new();

        // 🔧 Hoisting Pass: 收集顶层函数定义
        self.collect_function_definitions(expr, &mut env);

        // 预置 import 别名，避免“未定义”诊断
        self.apply_module_context(context, &mut env);

        let result_type = self.infer_expr(expr, &env);

        // 解决所有约束
        self.solve_constraints();

        // 对所有 expr_types 中的类型应用统一化结果
        // 因为 infer_expr 存储时约束尚未求解，此时类型变量未解析
        for key in self.expr_types.keys().copied().collect::<Vec<_>>() {
            if let Some(old_ty) = self.expr_types.remove(&key) {
                let resolved = self.apply_substitution(old_ty);
                self.expr_types.insert(key, resolved);
            }
        }

        // 应用统一化结果
        let final_type = self.apply_substitution(result_type);

        if let Some(main_sig) = self.function_signatures.get("main") {
            let resolved_ret = self.apply_substitution(main_sig.return_type.clone());
            match &resolved_ret {
                Type::Number | Type::Int(_) | Type::Bool | Type::Unit | Type::Var(_) => {}
                _ => {
                    self.add_error(TypeCheckError::InvalidMainReturnType {
                        found: resolved_ret,
                        span: karte_diagnostics::Span::dummy(),
                    });
                }
            }
        }

        // 检查顶层未使用的变量和函数（只在无编译错误时，避免噪音）
        if !self.diagnostics.has_errors() {
            self.pop_local_scope(false);
        }

        // 如果有错误且类型仍然是变量，返回 Unknown
        if self.diagnostics.has_errors() && matches!(final_type, Type::Var(_)) {
            Type::Unknown
        } else {
            final_type
        }
    }

    fn apply_module_context(&mut self, context: &ModuleContext, env: &mut TypeEnvironment) {
        self.module_context = context.clone();
        for binding in &context.imports {
            let ty = self
                .resolve_import_binding(context, binding)
                .unwrap_or_else(|| Type::Var(self.fresh_type_var()));
            env.entry(binding.alias.clone()).or_insert(ty);
        }

        let prelude_key = "std.prelude";
        if context.dependency_interfaces().contains_key(prelude_key) {
            let prelude_std_modules = ["std.core", "std.math", "std.io", "std.string", "std.result"];
            for module_key in &prelude_std_modules {
                if let Some(iface) = context.dependency_interfaces().get(*module_key) {
                    for (func_name, function) in &iface.functions {
                        env.entry(func_name.clone()).or_insert_with(|| {
                            let params = vec![Type::Unknown; function.params];
                            Type::function(params, Type::Unknown)
                        });
                    }
                }
            }
        }
    }

    fn resolve_import_binding(
        &self,
        context: &ModuleContext,
        binding: &ImportBinding,
    ) -> Option<Type> {
        if binding.symbol == "*" {
            return None;
        }

        let module_key = binding.module_path.join(".");
        let module_iface = context.dependency_interfaces().get(&module_key)?;
        if let Some(function) = module_iface.functions.get(&binding.symbol) {
            let params = vec![Type::Unknown; function.params];
            return Some(Type::function(params, Type::Unknown));
        }
        None
    }

    fn resolve_module_symbol(
        &mut self,
        module_path: &[String],
        symbol: &str,
        span: karte_diagnostics::Span,
    ) -> Type {
        if module_path.is_empty() {
            self.add_error(TypeCheckError::ModuleInterfaceUnavailable {
                module: "<unknown>".to_string(),
                span,
            });
            return Type::Unknown;
        }

        let module_identifier = self
            .resolve_module_identifier(module_path)
            .unwrap_or_else(|| module_path.join("."));

        let interface = if let Some(iface) = self
            .module_context
            .dependency_interfaces()
            .get(&module_identifier)
        {
            iface
        } else {
            self.add_error(TypeCheckError::ModuleInterfaceUnavailable {
                module: module_identifier,
                span,
            });
            return Type::Unknown;
        };

        if let Some(function) = interface.functions.get(symbol) {
            let params = vec![Type::Unknown; function.params];
            return Type::function(params, Type::Unknown);
        }

        self.add_error(TypeCheckError::UndefinedModuleSymbol {
            module: module_identifier,
            symbol: symbol.to_string(),
            span,
        });
        Type::Unknown
    }

    fn resolve_module_identifier(&self, module_path: &[String]) -> Option<String> {
        if module_path.is_empty() {
            return None;
        }

        let direct = module_path.join(".");
        if self
            .module_context
            .dependency_interfaces()
            .contains_key(&direct)
        {
            return Some(direct);
        }

        let alias = module_path.first()?.as_str();
        if let Some(binding) = self
            .module_context
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

        Some(direct)
    }

    /// 递归收集所有结构体定义并预处理它们
    fn collect_struct_definitions(&mut self, expr: &Expr) {
        // 先收集所有结构体定义的名称和字段信息
        let mut struct_defs = HashMap::new();
        self.gather_struct_defs(expr, &mut struct_defs);

        // 现在解析这些结构体定义，支持相互引用和自引用
        self.process_struct_definitions(struct_defs);
    }

    /// 收集顶层函数定义，支持相互递归
    ///
    /// 这个函数会：
    /// 1. 严格验证所有类型标注（如果有的话）
    /// 2. 将解析的函数签名缓存到 self.function_signatures 中
    /// 3. 将函数类型添加到环境中
    fn collect_function_definitions(&mut self, expr: &Expr, env: &mut TypeEnvironment) {
        if let Expr::Block { statements, .. } = expr {
            for stmt in statements {
                if let Statement::FunctionDef {
                    name,
                    params,
                    return_type,
                    span,
                    ..
                } = stmt
                {
                    // 如果环境里已经有了，检查是否是二次遍历（同一 span）还是真正的重复定义
                    if env.contains_key(name) {
                        // 如果是已处理过的主定义（二次遍历中的同一 span），静默跳过
                        if self.primary_function_spans.contains(&(span.start, span.end)) {
                            continue;
                        }
                        // 真正的重复定义：记录 span，后续由 infer_stmt 统一报告错误
                        self.duplicate_function_spans.insert((span.start, span.end));
                        continue;
                    }

                    let mut param_types = Vec::new();
                    let mut parse_failed = false;

                    // 检查参数名重复
                    let mut param_names = std::collections::HashSet::new();
                    for param in params {
                        if param_names.contains(&param.name) {
                            self.add_error(TypeCheckError::DuplicateFunctionDefinition {
                                name: param.name.clone(),
                                span: param.span,
                            });
                        }
                        param_names.insert(param.name.clone());
                    }

                    // 解析参数类型（严格模式）
                    for param in params {
                        let param_ty = if let Some(type_ann) = &param.type_annotation {
                            // 使用结构化类型注解，但需要解析其中的骨架占位符
                            self.resolve_struct_field_from_parsed(type_ann)
                        } else {
                            // 没有类型标注，创建类型变量
                            Type::Var(self.fresh_type_var())
                        };
                        param_types.push(param_ty);
                    }

                    // 解析返回类型（严格模式）
                    let ret_ty = if let Some(ret) = return_type {
                        // 使用结构化返回类型，但需要解析其中的骨架占位符
                        self.resolve_struct_field_from_parsed(ret)
                    } else {
                        // 没有返回类型标注，创建类型变量
                        Type::Var(self.fresh_type_var())
                    };

                    // 缓存函数签名，供后续检查函数体时使用
                    // 即使解析失败，也要缓存，以便继续类型检查发现更多错误
                    self.function_signatures.insert(
                        name.clone(),
                        FunctionSignature {
                            param_types: param_types.clone(),
                            return_type: ret_ty.clone(),
                            has_explicit_return_type: return_type.is_some(),
                        },
                    );

                    // 记录此 span 为已处理的主定义（用于后续检测二次遍历）
                    self.primary_function_spans.insert((span.start, span.end));

                    let func_type = Type::Function {
                        params: param_types,
                        return_type: Box::new(ret_ty),
                    };
                    env.insert(name.clone(), func_type);

                    // 注意：即使类型解析失败（parse_failed == true），
                    // 我们也继续处理，以便发现更多的类型错误
                }
            }
        }
    }

    /// 递归遍历AST收集结构体定义
    fn gather_struct_defs(&self, expr: &Expr, struct_defs: &mut HashMap<String, Vec<FieldDef>>) {
        match expr {
            Expr::Block {
                statements,
                final_expr,
                ..
            } => {
                for stmt in statements {
                    self.gather_struct_defs_from_statement(stmt, struct_defs);
                }
                if let Some(final_expr) = final_expr {
                    self.gather_struct_defs(final_expr, struct_defs);
                }
            }
            Expr::Statement { stmt, .. } => {
                self.gather_struct_defs_from_statement(stmt, struct_defs);
            }
            // 其他表达式可能包含嵌套的语句/块
            Expr::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                self.gather_struct_defs(condition, struct_defs);
                self.gather_struct_defs(then_branch, struct_defs);
                if let Some(else_branch) = else_branch {
                    self.gather_struct_defs(else_branch, struct_defs);
                }
            }
            Expr::While {
                condition, body, ..
            } => {
                self.gather_struct_defs(condition, struct_defs);
                self.gather_struct_defs(body, struct_defs);
            }
            Expr::Match { expr, arms, .. } => {
                self.gather_struct_defs(expr, struct_defs);
                for arm in arms {
                    self.gather_struct_defs(&arm.body, struct_defs);
                }
            }
            Expr::FunctionCall { function, args, .. } => {
                self.gather_struct_defs(function, struct_defs);
                for arg in args {
                    self.gather_struct_defs(arg, struct_defs);
                }
            }
            Expr::Lambda { body, .. } => {
                self.gather_struct_defs(body, struct_defs);
            }
            // ... 其他表达式类型（它们不包含结构体定义）
            _ => {}
        }
    }

    /// 从语句中收集结构体定义
    fn gather_struct_defs_from_statement(
        &self,
        stmt: &Statement,
        struct_defs: &mut HashMap<String, Vec<FieldDef>>,
    ) {
        if let Statement::StructDef { name, fields, type_params, .. } = stmt {
            // 泛型 struct 不在普通收集阶段处理
            if !type_params.is_empty() {
                return;
            }
            struct_defs.insert(name.clone(), fields.clone());
        }
    }

    /// 🔧 预收集枚举定义（TypeDef），在函数签名解析之前注册到 custom_types
    /// 否则 `fn eval(e: Expr)` 中的 `Expr` 类型在函数签名解析时尚未注册，
    /// resolve_struct_field_from_parsed 会返回空 Struct 骨架导致类型检查失败
    fn collect_enum_definitions(&mut self, expr: &Expr) {
        let mut enum_defs: Vec<(String, Vec<(String, Vec<Type>)>)> = Vec::new();
        self.gather_enum_defs(expr, &mut enum_defs);

        // 第一轮：注册所有枚举 Sum 到 custom_types（建立类型引用基础）
        for (name, variants) in &enum_defs {
            let sum_variants: Vec<crate::types::SumVariant> = variants
                .iter()
                .map(|(variant_name, data_types)| crate::types::SumVariant {
                    name: variant_name.clone(),
                    data_types: data_types.clone(),
                })
                .collect();

            let sum_type = Type::sum(name.clone(), sum_variants);
            self.custom_types.insert(name.clone(), sum_type);
        }

        // 第二轮：将变体 data_types 中的骨架占位符替换为实际类型
        // 处理递归枚举（如 enum List { Nil, Cons(number, List) }）和互引用枚举
        for (name, variants) in &enum_defs {
            let mut resolved_variants = Vec::new();
            for (variant_name, data_types) in variants {
                let mut resolved_data_types = Vec::new();
                for dt in data_types {
                    resolved_data_types.push(self.resolve_struct_field_from_parsed(dt));
                }
                resolved_variants.push(crate::types::SumVariant {
                    name: variant_name.clone(),
                    data_types: resolved_data_types,
                });
            }

            let sum_type = Type::sum(name.clone(), resolved_variants);
            self.custom_types.insert(name.clone(), sum_type);
        }
    }

    fn gather_enum_defs(
        &self,
        expr: &Expr,
        enum_defs: &mut Vec<(String, Vec<(String, Vec<Type>)>)>,
    ) {
        match expr {
            Expr::Block {
                statements,
                final_expr,
                ..
            } => {
                for stmt in statements {
                    self.gather_enum_defs_from_statement(stmt, enum_defs);
                }
                if let Some(final_expr) = final_expr {
                    self.gather_enum_defs(final_expr, enum_defs);
                }
            }
            _ => {}
        }
    }

    /// 收集泛型类型定义模板（泛型 struct/enum）
    /// 必须在 collect_function_definitions 之前调用，
    /// 以便 resolve_struct_field_from_parsed 能找到泛型类型并用 fresh type var 实例化
    fn collect_generic_type_defs(&mut self, expr: &Expr) {
        if let Expr::Block { statements, .. } = expr {
            for stmt in statements {
                match stmt {
                    Statement::StructDef { name, fields, type_params, .. } if !type_params.is_empty() => {
                        let struct_fields: Vec<crate::types::StructField> = fields
                            .iter()
                            .map(|field| crate::types::StructField {
                                name: field.name.clone(),
                                field_type: field.field_type.clone(),
                            })
                            .collect();
                        let template = Type::struct_type(name.clone(), struct_fields);
                        self.generic_type_defs.insert(name.clone(), GenericTypeDef {
                            type_params: type_params.clone(),
                            template,
                        });
                    }
                    Statement::TypeDef { name, variants, type_params, .. } if !type_params.is_empty() => {
                        let sum_variants: Vec<crate::types::SumVariant> = variants
                            .iter()
                            .map(|v| crate::types::SumVariant {
                                name: v.name.clone(),
                                data_types: v.data_types.clone(),
                            })
                            .collect();
                        let template = Type::sum(name.clone(), sum_variants);
                        self.generic_type_defs.insert(name.clone(), GenericTypeDef {
                            type_params: type_params.clone(),
                            template,
                        });
                    }
                    _ => {}
                }
            }
        }
    }

    fn gather_enum_defs_from_statement(
        &self,
        stmt: &Statement,
        enum_defs: &mut Vec<(String, Vec<(String, Vec<Type>)>)>,
    ) {
        if let Statement::TypeDef { name, variants, type_params, .. } = stmt {
            // 泛型枚举不在普通收集阶段处理
            if !type_params.is_empty() {
                return;
            }
            let variant_list: Vec<(String, Vec<Type>)> = variants
                .iter()
                .map(|v| (v.name.clone(), v.data_types.clone()))
                .collect();
            enum_defs.push((name.clone(), variant_list));
        }
    }

    /// 处理收集到的结构体定义，解析字段类型并检测循环引用
    fn process_struct_definitions(&mut self, struct_defs: HashMap<String, Vec<FieldDef>>) {
        // 🔧 修复：先创建所有结构体的完整骨架（只有名字，没有字段）
        // 这样在解析字段类型时，至少能通过名字引用其他结构体
        for name in struct_defs.keys() {
            let placeholder_type = Type::struct_type(name.clone(), vec![]);
            self.custom_types.insert(name.clone(), placeholder_type);
        }

        // 🔧 修复：按依赖顺序解析字段类型
        // 使用拓扑排序确保被依赖的 struct 先解析
        // 对于简单情况（无循环依赖），按源码顺序解析即可
        let struct_defs_copy = struct_defs.clone();

        // 🔧 关键修复：多轮解析，每轮尝试解析尚未完成的结构体
        // 直到没有新的结构体被解析为止
        let mut resolved: HashMap<String, Vec<crate::types::StructField>> = HashMap::new();
        let mut remaining: Vec<(String, Vec<FieldDef>)> = struct_defs.into_iter().collect();

        // 最多迭代 remaining.len() 次（每次至少解析一个）
        let max_iterations = remaining.len();
        for _ in 0..max_iterations {
            let mut next_remaining = Vec::new();
            let mut progress = false;

            for (name, fields) in remaining {
                let mut struct_fields = Vec::new();
                let mut all_resolved = true;

                for field in &fields {
                    let field_type = self.resolve_parsed_type(&field.field_type, &struct_defs_copy);

                    // 检查字段类型是否还是空骨架（未完全解析）
                    if let Type::Struct { name: ref type_name, fields: ref type_fields } = field_type {
                        if type_fields.is_empty() && struct_defs_copy.contains_key(type_name) && type_name != &name {
                            // 引用了其他尚未解析完成的 struct，跳过本轮
                            all_resolved = false;
                            break;
                        }
                    }

                    struct_fields.push(crate::types::StructField {
                        name: field.name.clone(),
                        field_type,
                    });
                }

                if all_resolved {
                    // 检查是否有非法的递归
                    if Self::has_illegal_recursion(&name, &struct_fields, &mut vec![]) {
                        self.add_error(TypeCheckError::InvalidPattern {
                            message: format!(
                                "Illegal recursion in struct {}: recursive types must use references",
                                name
                            ),
                            span: karte_diagnostics::Span::new(0, 0),
                        });
                    }
                    resolved.insert(name.clone(), struct_fields);
                    // 🔧 关键：解析完成后立即更新 custom_types，让后续 struct 能引用
                    let struct_type = Type::struct_type(name.clone(), resolved[&name].clone());
                    self.custom_types.insert(name, struct_type);
                    progress = true;
                } else {
                    next_remaining.push((name, fields));
                }
            }

            remaining = next_remaining;
            if !progress || remaining.is_empty() {
                break;
            }
        }

        // 处理剩余无法解析的（循环依赖等情况）
        for (name, fields) in remaining {
            let mut struct_fields = Vec::new();
            for field in &fields {
                let field_type = self.resolve_parsed_type(&field.field_type, &struct_defs_copy);
                struct_fields.push(crate::types::StructField {
                    name: field.name.clone(),
                    field_type,
                });
            }

            // 🔧 检查循环依赖是否为非法递归（没有通过引用的递归）
            if Self::has_illegal_recursion(&name, &struct_fields, &mut vec![]) {
                self.add_error(TypeCheckError::InvalidPattern {
                    message: format!(
                        "Illegal recursion in struct {}: recursive types must use references",
                        name
                    ),
                    span: karte_diagnostics::Span::new(0, 0),
                });
            }

            let struct_type = Type::struct_type(name.clone(), struct_fields);
            self.custom_types.insert(name, struct_type);
        }
    }

    /// 检查结构体是否有非法的递归（没有通过引用的递归）
    fn has_illegal_recursion(
        struct_name: &str,
        fields: &[crate::types::StructField],
        visited: &mut Vec<String>,
    ) -> bool {
        if visited.contains(&struct_name.to_string()) {
            return true; // 找到循环
        }

        visited.push(struct_name.to_string());

        for field in fields {
            match &field.field_type {
                Type::Reference { .. } => {
                    // 引用类型打破了递归，这是合法的
                    continue;
                }
                Type::Struct {
                    name: field_struct_name,
                    fields: field_struct_fields,
                } => {
                    if Self::has_illegal_recursion(field_struct_name, field_struct_fields, visited)
                    {
                        visited.pop();
                        return true;
                    }
                }
                Type::Array { element } => {
                    // 检查数组元素类型是否构成非法递归
                    match element.as_ref() {
                        Type::Struct {
                            name: elem_name,
                            ..
                        } if elem_name == struct_name => {
                            return true; // 非法递归：struct S { arr: [S; N] }
                        }
                        _ => continue,
                    }
                }
                _ => {
                    // 其他类型不会导致递归
                    continue;
                }
            }
        }

        visited.pop();
        false
    }

    /// 推断表达式的类型
    fn infer_expr(&mut self, expr: &Expr, env: &TypeEnvironment) -> Type {
        let inferred_type = match expr {
            Expr::Number { .. } => Type::Number,

            Expr::StringLiteral { .. } => Type::String,

            Expr::Unit { .. } => Type::Unit,

            Expr::Identifier { name, span } => {
                // 内建函数
                if name == "print" {
                    let alpha = self.fresh_type_var();
                    return Type::function(vec![Type::Var(alpha)], Type::Number);
                }
                // 字符串内容比较
                if name == "str_equal" {
                    return Type::function(vec![Type::String, Type::String], Type::Number);
                }
                // 字符串比较（有序）
                if name == "str_compare" {
                    return Type::function(vec![Type::String, Type::String], Type::Number);
                }
                // 通用内存原语 — 供标准库实现数组、哈希表等数据结构
                if name == "gc_alloc" {
                    return Type::function(vec![Type::Number], Type::Number);
                }
                if name == "mem_load64" {
                    return Type::function(vec![Type::Number], Type::Number);
                }
                if name == "unsafe_cast" {
                    let alpha = self.fresh_type_var();
                    let beta = self.fresh_type_var();
                    return Type::function(vec![Type::Var(alpha)], Type::Var(beta));
                }
                // 通用 syscall 入口（Go 风格 raw_syscall6）
                if name == "raw_syscall6" {
                    return Type::function(
                        vec![Type::Number, Type::Number, Type::Number, Type::Number, Type::Number, Type::Number, Type::Number],
                        Type::Number,
                    );
                }
                if name == "mem_store64" {
                    let alpha = self.fresh_type_var();
                    return Type::function(vec![Type::Number, Type::Var(alpha)], Type::Number);
                }
                // 优先检查泛型函数，若匹配则实例化
                if let Some(scheme) = self.function_schemes.get(name).cloned() {
                    self.mark_used_by_name(name);
                    return self.instantiate(&scheme);
                }
                if let Some(ty) = env.get(name) {
                    self.mark_used_by_name(name);
                    ty.clone()
                } else {
                    let suggestion = self.suggest_variable(name, env);
                    self.add_error(TypeCheckError::UndefinedVariable {
                        name: name.clone(),
                        span: *span,
                        suggestion,
                    });
                    Type::Unknown
                }
            }

            Expr::ModuleSymbolAccess {
                module_path,
                symbol,
                span,
            } => self.resolve_module_symbol(&module_path, &symbol, *span),

            Expr::BinaryOp {
                left,
                op,
                right,
                span: _,
            } => {
                let left_type = self.infer_expr(left, env);
                let right_type = self.infer_expr(right, env);

                // 根据操作符类型添加不同的类型约束
                match op {
                    BinaryOperator::Add => {
                        // Add 支持数字加法和字符串拼接
                        // String + String: 两侧都约束为 String
                        // String + Number / Number + String: 仅 String 侧约束，Number 侧保持 Number（由 MIR lowering 自动插入 to_string）
                        match (&left_type, &right_type) {
                            (Type::String, Type::String) => {
                                // 两侧都是 String，无需额外约束
                            }
                            (Type::String, _) => {
                                self.add_constraint_with_context(left_type.clone(), Type::String, left.span(), 
                                    format!("字符串拼接要求 string 类型, 但左侧为 `{}`", left_type));
                            }
                            (_, Type::String) => {
                                self.add_constraint_with_context(right_type.clone(), Type::String, right.span(), 
                                    format!("字符串拼接要求 string 类型, 但右侧为 `{}`", right_type));
                            }
                            _ => {
                                self.add_constraint_with_context(left_type.clone(), Type::Number, left.span(), 
                                    format!("加法运算要求 number 类型, 但左侧为 `{}`", left_type));
                                self.add_constraint_with_context(right_type.clone(), Type::Number, left.span(), 
                                    format!("加法运算要求 number 类型, 但右侧为 `{}`", right_type));
                            }
                        }
                    }
                    BinaryOperator::Equal | BinaryOperator::NotEqual => {
                        self.add_constraint_with_context(left_type.clone(), right_type.clone(), left.span(), 
                            format!("比较运算要求两边类型一致: 左侧为 `{}`, 右侧为 `{}`", left_type, right_type));
                    }
                    BinaryOperator::Subtract
                    | BinaryOperator::Multiply
                    | BinaryOperator::Divide
                    | BinaryOperator::Modulo
                    | BinaryOperator::BitAnd
                    | BinaryOperator::BitOr
                    | BinaryOperator::BitXor
                    | BinaryOperator::ShiftLeft
                    | BinaryOperator::ShiftRight => {
                        // 数字运算、位运算：左右操作数都必须是数字类型
                        self.add_constraint_with_context(left_type.clone(), Type::Number, left.span(), 
                            format!("算术运算要求 number 类型, 但左侧为 `{}`", left_type));
                        self.add_constraint_with_context(right_type.clone(), Type::Number, right.span(), 
                            format!("算术运算要求 number 类型, 但右侧为 `{}`", right_type));
                    }
                    BinaryOperator::GreaterEqual
                    | BinaryOperator::LessEqual
                    | BinaryOperator::Greater
                    | BinaryOperator::Less => {
                        // 有序比较运算符：支持 number 和 string 类型
                        // 如果任一操作数是 string，则两边都必须是 string（字典序比较）
                        // 否则两边都必须是 number
                        match (&left_type, &right_type) {
                            (Type::String, _) | (_, Type::String) => {
                                self.add_constraint_with_context(left_type.clone(), Type::String, left.span(), 
                                    format!("字符串比较要求 string 类型, 但左侧为 `{}`", left_type));
                                self.add_constraint_with_context(right_type.clone(), Type::String, right.span(), 
                                    format!("字符串比较要求 string 类型, 但右侧为 `{}`", right_type));
                            }
                            _ => {
                                self.add_constraint_with_context(left_type.clone(), Type::Number, left.span(), 
                                    format!("比较运算要求 number 类型, 但左侧为 `{}`", left_type));
                                self.add_constraint_with_context(right_type.clone(), Type::Number, right.span(), 
                                    format!("比较运算要求 number 类型, 但右侧为 `{}`", right_type));
                            }
                        }
                    }
                    BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr => {
                        // 逻辑运算符接受 bool 或 number（与 if/while 条件一致）
                        match (&left_type, &right_type) {
                            (Type::Number, _) | (_, Type::Number) => {
                                self.add_constraint_with_context(left_type.clone(), Type::Number, left.span(), 
                                    format!("逻辑运算符要求 number 类型, 但左侧为 `{}`", left_type));
                                self.add_constraint_with_context(right_type.clone(), Type::Number, right.span(), 
                                    format!("逻辑运算符要求 number 类型, 但右侧为 `{}`", right_type));
                            }
                            _ => {
                                self.add_constraint_with_context(left_type.clone(), Type::bool(), left.span(), 
                                    format!("逻辑运算符 (&&/||) 要求 bool 类型, 但左侧为 `{}`", left_type));
                                self.add_constraint_with_context(right_type.clone(), Type::bool(), right.span(), 
                                    format!("逻辑运算符 (&&/||) 要求 bool 类型, 但右侧为 `{}`", right_type));
                            }
                        }
                    }
                }

                // 根据操作符类型返回不同的结果类型
                match op {
                    BinaryOperator::Add => {
                        // Add 支持数字加法和字符串拼接
                        match (&left_type, &right_type) {
                            (Type::String, _) | (_, Type::String) => Type::String,
                            _ => Type::Number,
                        }
                    }
                    BinaryOperator::Subtract
                    | BinaryOperator::Multiply
                    | BinaryOperator::Divide
                    | BinaryOperator::Modulo
                    | BinaryOperator::BitAnd
                    | BinaryOperator::BitOr
                    | BinaryOperator::BitXor
                    | BinaryOperator::ShiftLeft
                    | BinaryOperator::ShiftRight => Type::Number,
                    BinaryOperator::Equal
                    | BinaryOperator::NotEqual
                    | BinaryOperator::GreaterEqual
                    | BinaryOperator::LessEqual
                    | BinaryOperator::Greater
                    | BinaryOperator::Less => Type::bool(),
                    BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr => {
                        // 逻辑运算返回值：number 操作数返回 number，bool 操作数返回 bool
                        match (&left_type, &right_type) {
                            (Type::Number, _) | (_, Type::Number) => Type::Number,
                            _ => Type::bool(),
                        }
                    }
                }
            }

            Expr::UnaryOp { operand, op, .. } => {
                let operand_type = self.infer_expr(operand, env);

                match op {
                    UnaryOperator::Plus | UnaryOperator::Minus => {
                        self.add_constraint_with_context(Type::Number, operand_type.clone(), operand.span(), 
                            format!("一元运算符 (+/-) 要求 number 类型, 但操作数为 `{}`", operand_type));
                        Type::Number
                    }
                    UnaryOperator::LogicalNot => {
                        self.add_constraint_with_context(Type::bool(), operand_type.clone(), operand.span(), 
                            format!("逻辑非运算符 (!) 要求 bool 类型, 但操作数为 `{}`", operand_type));
                        Type::bool()
                    }
                    UnaryOperator::BitNot => {
                        self.add_constraint_with_context(Type::Number, operand_type.clone(), operand.span(), 
                            format!("位非运算符 (~) 要求 number 类型, 但操作数为 `{}`", operand_type));
                        Type::Number
                    }
                }
            }

            Expr::Lambda {
                params,
                body,
                return_type,
                inferred_type: _,
                span: _,
            } => {
                let mut new_env = env.clone();
                let mut param_types = Vec::new();
                // 检查闭包参数名重复
                let mut seen_params = std::collections::HashSet::new();
                for param in params {
                    if seen_params.contains(&param.name) {
                        self.add_error(TypeCheckError::DuplicateFunctionDefinition {
                            name: param.name.clone(),
                            span: param.span,
                        });
                    }
                    seen_params.insert(param.name.clone());
                    let param_type = if let Some(ref type_ann) = param.type_annotation {
                        self.resolve_struct_field_from_parsed(type_ann)
                    } else {
                        Type::Var(self.fresh_type_var())
                    };
                    new_env.insert(param.name.clone(), param_type.clone());
                    param_types.push(param_type);
                }

                let body_type = self.infer_expr(body, &new_env);

                // 如果有返回类型标注，统一约束
                if let Some(ref ret_annotation) = return_type {
                    let resolved_ret_type = self.resolve_struct_field_from_parsed(ret_annotation);
                    self.add_constraint_with_context(body_type.clone(), resolved_ret_type.clone(), body.span(), 
                        format!("Lambda 返回类型与标注不匹配: 标注为 `{}`, 实际为 `{}`", resolved_ret_type, body_type));
                }

                // 存储Lambda类型到映射中
                let lambda_type = Type::closure(param_types.clone(), body_type.clone());
                self.lambda_types
                    .insert(expr as *const Expr, lambda_type.clone());

                lambda_type
            }

            Expr::FunctionCall {
                function,
                args,
                span,
            } => {
                // 检测内置函数调用（通过方法调用语法解糖产生的）
                if let Expr::Identifier { name, .. } = function.as_ref() {
                    match name.as_str() {
                        "len" => {
                            // len(x) - 返回数组/字符串长度
                            if args.len() == 1 {
                                let arg_type = self.infer_expr(&args[0], env);
                                match &arg_type {
                                    Type::Array { .. } => {
                                        self.expr_types.insert(&args[0] as *const Expr, arg_type);
                                        return Type::Number;
                                    }
                                    Type::String => {
                                        return Type::Number;
                                    }
                                    _ => {
                                        // 尝试作为数组处理
                                        return Type::Number;
                                    }
                                }
                            }
                        }
                        "abs" => {
                            if args.len() == 1 {
                                let arg_type = self.infer_expr(&args[0], env);
                                self.add_constraint_with_context(Type::Number, arg_type, *span, "abs() 要求 number 类型参数");
                                return Type::Number;
                            }
                        }
                        "min" | "max" => {
                            if args.len() == 2 {
                                let t1 = self.infer_expr(&args[0], env);
                                let t2 = self.infer_expr(&args[1], env);
                                self.add_constraint_with_context(Type::Number, t1, *span, "min()/max() 要求 number 类型参数");
                                self.add_constraint_with_context(Type::Number, t2, *span, "min()/max() 要求 number 类型参数");
                                return Type::Number;
                            }
                        }
                        "clamp" => {
                            if args.len() == 3 {
                                let t1 = self.infer_expr(&args[0], env);
                                let t2 = self.infer_expr(&args[1], env);
                                let t3 = self.infer_expr(&args[2], env);
                                self.add_constraint_with_context(Type::Number, t1, *span, "clamp() 要求 number 类型参数");
                                self.add_constraint_with_context(Type::Number, t2, *span, "clamp() 要求 number 类型参数");
                                self.add_constraint_with_context(Type::Number, t3, *span, "clamp() 要求 number 类型参数");
                                return Type::Number;
                            }
                        }
                        _ => {}
                    }
                }

                let func_type = self.infer_expr(function, env);
                let arg_types: Vec<Type> =
                    args.iter().map(|arg| self.infer_expr(arg, env)).collect();

                // 检查函数类型是否可调用
                match &func_type {
                    Type::Function {
                        params,
                        return_type,
                    } => {
                        // 检查参数数量
                        if params.len() != args.len() {
                            self.add_error(TypeCheckError::ArityMismatch {
                                expected: params.len(),
                                found: args.len(),
                                span: *span,
                            });
                            // 即使参数数量不匹配，仍然返回函数的返回类型
                            return *return_type.clone();
                        }

                        // 统一参数类型
                        for (i, (param_type, arg_type)) in params.iter().zip(arg_types.iter()).enumerate() {
                            self.add_constraint_with_context(
                                param_type.clone(),
                                arg_type.clone(),
                                *span,
                                format!("函数调用第 {} 个参数类型不匹配: 期望 `{}`, 实际 `{}`", i + 1, param_type, arg_type),
                            );
                        }

                        *return_type.clone()
                    }
                    Type::Closure {
                        params,
                        return_type,
                    } => {
                        // Closure 调用规则与普通函数相同，但在运行时会走闭包调用路径
                        if params.len() != args.len() {
                            self.add_error(TypeCheckError::ArityMismatch {
                                expected: params.len(),
                                found: args.len(),
                                span: *span,
                            });
                            return *return_type.clone();
                        }

                        for (i, (param_type, arg_type)) in params.iter().zip(arg_types.iter()).enumerate() {
                            self.add_constraint_with_context(
                                param_type.clone(),
                                arg_type.clone(),
                                *span,
                                format!("闭包调用第 {} 个参数类型不匹配: 期望 `{}`, 实际 `{}`", i + 1, param_type, arg_type),
                            );
                        }

                        *return_type.clone()
                    }
                    Type::Var(_) => {
                        // 对于类型变量，创建约束
                        let return_type = Type::Var(self.fresh_type_var());
                        let expected_func_type = Type::function(arg_types, return_type.clone());
                        self.add_constraint_with_context(func_type, expected_func_type, *span, "函数调用参数类型不匹配");
                        return_type
                    }
                    Type::Unknown => Type::Unknown,
                    _ => {
                        // 不可调用的类型
                        self.add_error(TypeCheckError::NotCallable {
                            found_type: func_type,
                            span: *span,
                        });
                        Type::Unknown
                    }
                }
            }

            Expr::Statement { stmt, .. } => {
                let mut env_copy = env.clone();
                self.infer_statement(stmt, &mut env_copy);
                Type::Unit
            }

            Expr::Block {
                statements,
                final_expr,
                ..
            } => {
                let mut current_env = env.clone();

                // 🔧 Hoisting Pass: 预先收集函数定义，支持相互递归
                self.collect_function_definitions(expr, &mut current_env);

                // 检测死代码：在 return/break/continue 后的语句不可达
                let mut found_control_flow = false;
                for (i, stmt) in statements.iter().enumerate() {
                    if found_control_flow {
                        // 死代码警告
                        let span = stmt.span();
                        let msg = "不可达代码: return/break/continue 后的代码永远不会执行".to_string();
                        self.add_warning(msg, span);
                        break; // 只报告第一个死代码警告
                    }
                    // 检查当前语句是否是控制流表达式
                    if let Statement::Expression { expr, .. } = stmt {
                        if matches!(expr, Expr::Return { .. } | Expr::Break { .. } | Expr::Continue { .. }) {
                            found_control_flow = true;
                        }
                    }
                }
                if found_control_flow && final_expr.is_some() {
                    if let Some(fe) = final_expr {
                        let span = fe.span();
                        let msg = format!("不可达代码: {} 后的代码永远不会执行", "return/break/continue");
                        self.add_warning(msg, span);
                    }
                }

                // 推断所有语句
                for stmt in statements {
                    self.infer_statement(stmt, &mut current_env);
                }

                // 推断最终表达式
                if let Some(expr) = final_expr {
                    self.infer_expr(expr, &current_env)
                } else if let Some(last_stmt) = statements.last() {
                    // 如果没有 final_expr，检查最后一个语句是否是 return/break/continue 表达式
                    // 这些控制流表达式有实际的返回类型
                    match last_stmt {
                        Statement::Expression { expr, .. } => {
                            let last_type = self.infer_expr(expr, &current_env);
                            // 检查是否是控制流表达式（return/break/continue）
                            // 这些表达式的类型就是函数的返回类型
                            if matches!(expr, Expr::Return { .. } | Expr::Break { .. } | Expr::Continue { .. }) {
                                last_type
                            } else {
                                Type::Unit
                            }
                        }
                        _ => Type::Unit,
                    }
                } else {
                    Type::Unit
                }
            }

            Expr::Boolean { .. } => Type::bool(),

            Expr::Constructor { name, args, span } => {
                // 首先检查是否是已知的内置构造器
                match name.as_str() {
                    "True" | "False" => Type::bool(),
                    "Some" | "None" => {
                        // 根据参数推断Option的内部类型
                        if let Some(arg_expr) = args.get(0) {
                            let arg_type = self.infer_expr(arg_expr, env);
                            Type::option(arg_type)
                        } else {
                            Type::option(Type::Var(self.fresh_type_var()))
                        }
                    }
                    "Ok" => {
                        // Ok(value) 构造 Result<T, E>
                        if let Some(arg_expr) = args.get(0) {
                            let ok_type = self.infer_expr(arg_expr, env);
                            Type::result(ok_type, Type::Var(self.fresh_type_var()))
                        } else {
                            self.add_error(TypeCheckError::InvalidConstructor {
                                name: name.clone(),
                                span: *span,
                            });
                            Type::Unknown
                        }
                    }
                    "Err" => {
                        // Err(error) 构造 Result<T, E>
                        if let Some(arg_expr) = args.get(0) {
                            let err_type = self.infer_expr(arg_expr, env);
                            Type::result(Type::Var(self.fresh_type_var()), err_type)
                        } else {
                            self.add_error(TypeCheckError::InvalidConstructor {
                                name: name.clone(),
                                span: *span,
                            });
                            Type::Unknown
                        }
                    }
                    _ => {
                        // 检查是否是泛型 enum 的构造器
                        let mut found_generic = None;
                        for (gen_name, gen_def) in &self.generic_type_defs {
                            if let Type::Sum { variants, .. } = &gen_def.template {
                                if variants.iter().any(|v| v.name == *name) {
                                    found_generic = Some((gen_name.clone(), gen_def.type_params.clone(), variants.clone()));
                                    break;
                                }
                            }
                        }
                        if let Some((enum_name, type_params, variants)) = found_generic {
                            let variant = variants.iter().find(|v| v.name == *name).unwrap();
                            let mut inferred_args: HashMap<String, Type> = HashMap::new();
                            
                            if !variant.data_types.is_empty() {
                                if args.len() != variant.data_types.len() {
                                    self.add_error(TypeCheckError::InvalidConstructor {
                                        name: name.clone(),
                                        span: *span,
                                    });
                                    return Type::Unknown;
                                }
                                for (i, arg_expr) in args.iter().enumerate() {
                                    let arg_type = self.infer_expr(arg_expr, env);
                                    self.infer_type_params_from_value(
                                        &variant.data_types[i],
                                        &arg_type,
                                        &type_params,
                                        &mut inferred_args,
                                    );
                                }
                            } else if !args.is_empty() {
                                self.add_error(TypeCheckError::InvalidConstructor {
                                    name: name.clone(),
                                    span: *span,
                                });
                                return Type::Unknown;
                            }

                            let concrete_args: Vec<Type> = type_params.iter().map(|param| {
                                inferred_args.get(param).cloned().unwrap_or(Type::Var(self.fresh_type_var()))
                            }).collect();

                            let instantiated = self.instantiate_generic_type(&enum_name, &concrete_args);

                            return instantiated;
                        }

                        // 检查是否是环境中的构造器
                        if let Some(constructor_type) = env.get(name) {
                            // 如果是函数类型（有参数的构造器），需要应用参数
                            match constructor_type {
                                Type::Function {
                                    params,
                                    return_type,
                                } => {
                                    if !args.is_empty() {
                                        if params.len() == args.len() {
                                            for (i, arg_expr) in args.iter().enumerate() {
                                                let arg_type = self.infer_expr(arg_expr, env);
                                                self.add_constraint_with_context(
                                                    arg_type,
                                                    params[i].clone(),
                                                    arg_expr.span(),
                                                    "函数参数类型不匹配",
                                                );
                                            }
                                            (**return_type).clone()
                                        } else {
                                            self.add_error(TypeCheckError::InvalidConstructor {
                                                name: name.clone(),
                                                span: *span,
                                            });
                                            Type::Unknown
                                        }
                                    } else {
                                        self.add_error(TypeCheckError::InvalidConstructor {
                                            name: name.clone(),
                                            span: *span,
                                        });
                                        Type::Unknown
                                    }
                                }
                                _ => {
                                    // 无参数构造器
                                    if !args.is_empty() {
                                        self.add_error(TypeCheckError::InvalidConstructor {
                                            name: name.clone(),
                                            span: *span,
                                        });
                                        Type::Unknown
                                    } else {
                                        constructor_type.clone()
                                    }
                                }
                            }
                        } else {
                            // 对于未知构造器，创建一个类型变量
                            self.add_error(TypeCheckError::InvalidConstructor {
                                name: name.clone(),
                                span: *span,
                            });
                            Type::Unknown
                        }
                    }
                }
            }

            Expr::QualifiedConstructor {
                type_name,
                constructor_name,
                args,
                span,
            } => {
                // 检查类型是否存在：先查 custom_types，再查 generic_type_defs
                let sum_type = if let Some(sum_type) = self.custom_types.get(type_name).cloned() {
                    Some(sum_type)
                } else if self.generic_type_defs.contains_key(type_name) {
                    // 泛型类型：用 fresh type var 实例化
                    let param_count = self.generic_type_defs.get(type_name).unwrap().type_params.len();
                    let fresh_args: Vec<Type> = (0..param_count)
                        .map(|_| Type::Var(self.fresh_type_var()))
                        .collect();
                    Some(self.instantiate_generic_type(type_name, &fresh_args))
                } else {
                    None
                };

                if let Some(sum_type) = sum_type {
                    if let Type::Sum { name: _, variants } = &sum_type {
                        // 查找对应的构造器
                        if let Some(variant) = variants.iter().find(|v| v.name == *constructor_name)
                        {
                            if !args.is_empty() {
                                // 有参数的构造器
                                if !variant.data_types.is_empty() {
                                    // 检查参数数量是否匹配
                                    if args.len() == variant.data_types.len() {
                                        for (i, arg_expr) in args.iter().enumerate() {
                                            let arg_type = self.infer_expr(arg_expr, env);
                                            self.add_constraint_with_context(
                                                arg_type,
                                                variant.data_types[i].clone(),
                                                arg_expr.span(),
                                                "构造器参数类型不匹配",
                                            );
                                        }
                                        sum_type.clone()
                                    } else {
                                        self.add_error(TypeCheckError::InvalidConstructor {
                                            name: format!("{}::{}", type_name, constructor_name),
                                            span: *span,
                                        });
                                        Type::Unknown
                                    }
                                } else {
                                    self.add_error(TypeCheckError::InvalidConstructor {
                                        name: format!("{}::{}", type_name, constructor_name),
                                        span: *span,
                                    });
                                    Type::Unknown
                                }
                            } else {
                                // 无参数的构造器
                                if variant.data_types.is_empty() {
                                    sum_type.clone()
                                } else {
                                    self.add_error(TypeCheckError::InvalidConstructor {
                                        name: format!("{}::{}", type_name, constructor_name),
                                        span: *span,
                                    });
                                    Type::Unknown
                                }
                            }
                        } else {
                            self.add_error(TypeCheckError::InvalidConstructor {
                                name: format!("{}::{}", type_name, constructor_name),
                                span: *span,
                            });
                            Type::Unknown
                        }
                    } else {
                        self.add_error(TypeCheckError::InvalidConstructor {
                            name: format!("{}::{}", type_name, constructor_name),
                            span: *span,
                        });
                        Type::Unknown
                    }
                } else {
                    self.add_error(TypeCheckError::InvalidConstructor {
                        name: format!("{}::{}", type_name, constructor_name),
                        span: *span,
                    });
                    Type::Unknown
                }
            }

            Expr::Match { expr, arms, span } => {
                let expr_type = self.infer_expr(expr, env);

                if arms.is_empty() {
                    self.add_error(TypeCheckError::EmptyMatch { span: *span });
                    return Type::Unknown;
                }

                // 推断第一个分支的类型作为返回类型
                let first_arm = &arms[0];
                let mut result_env = env.clone();
                self.check_pattern(&first_arm.pattern, &expr_type, &mut result_env);
                if let Some(ref guard) = first_arm.guard {
                    let guard_type = self.infer_expr(guard, &result_env);
                    self.add_constraint_with_context(guard_type.clone(), Type::bool(), guard.span(), 
                        format!("match guard 必须是 bool 类型, 实际为 `{}`", guard_type));
                }
                let result_type = self.infer_expr(&first_arm.body, &result_env);

                // 检查所有其他分支的类型是否兼容
                for arm in &arms[1..] {
                    let mut arm_env = env.clone();
                    self.check_pattern(&arm.pattern, &expr_type, &mut arm_env);
                    if let Some(ref guard) = arm.guard {
                        let guard_type = self.infer_expr(guard, &arm_env);
                        self.add_constraint_with_context(guard_type.clone(), Type::bool(), guard.span(), 
                            format!("match guard 必须是 bool 类型, 实际为 `{}`", guard_type));
                    }
                    let arm_type = self.infer_expr(&arm.body, &arm_env);

                    // 约束：所有分支的类型必须兼容
                    self.add_constraint_with_context(result_type.clone(), arm_type.clone(), arm.body.span(), 
                        format!("match 分支类型不一致: 期望 `{}`, 实际 `{}`", result_type, arm_type));
                }

                // 穷尽性检查：检查所有模式是否覆盖了所有可能的情况
                {
                    let pattern_refs: Vec<&crate::ast::Pattern> = arms.iter().map(|arm| &arm.pattern).collect();
                    let exhaust_result = crate::exhaustiveness::check_exhaustiveness(
                        &pattern_refs,
                        &expr_type,
                        &self.custom_types,
                    );
                    if !exhaust_result.is_exhaustive {
                        self.add_error(TypeCheckError::NonExhaustiveMatch {
                            missing_patterns: exhaust_result.missing_patterns,
                            span: *span,
                        });
                    }
                    // 报告冗余 arm 警告
                    for arm_idx in &exhaust_result.redundant_arms {
                        let arm = &arms[*arm_idx];
                        self.add_warning(
                            format!("冗余的 match arm：该模式被之前的 arm 完全覆盖"),
                            arm.pattern.span(),
                        );
                    }
                }

                result_type
            }

            Expr::If {
                condition,
                then_branch,
                else_branch,
                span: _,
            } => {
                // 🔧 检测 if 条件中的赋值（常见错误: if x = 5 而非 if x == 5）
                if let Expr::Assignment { .. } = condition.as_ref() {
                    self.add_warning(
                        "if 条件中使用了赋值操作 (=)，请确认是否应该使用比较操作 (==)".to_string(),
                        condition.span(),
                    );
                }

                // 推断条件的类型，条件可以是布尔或数字类型（非零为 true）
                let condition_type = self.infer_expr(condition, env);
                // 允许 number 和 bool 作为条件
                match condition_type {
                    Type::Number | Type::Int(_) | Type::Bool => {}
                    _ => self.add_constraint_with_context(condition_type.clone(), Type::bool(), condition.span(), 
                        format!("if 条件必须是 bool 或 number 类型, 实际为 `{}`", condition_type)),
                };

                // 推断then分支的类型
                let then_type = self.infer_expr(then_branch, env);

                // 推断else分支的类型（如果存在）
                if let Some(else_branch) = else_branch {
                    let else_type = self.infer_expr(else_branch, env);
                    // 约束：then和else分支的类型必须兼容
                    // 特殊处理：如果一个分支是 Unit，if-else 返回另一个分支的类型
                    match (&then_type, &else_type) {
                        (Type::Unit, _) => else_type,
                        (_, Type::Unit) => then_type.clone(),
                        _ => {
                            self.add_constraint_with_context(
                                then_type.clone(),
                                else_type.clone(),
                                else_branch.span(),
                                format!("if-else 分支类型不一致: then 分支为 `{}`, else 分支为 `{}`", then_type, else_type),
                            );
                            then_type
                        }
                    }
                } else {
                    // 如果没有else分支，if表达式返回unit类型
                    // 但不强制then分支必须是unit（允许表达式求值但丢弃结果）
                    Type::Unit
                }
            }

            Expr::While {
                condition, body, ..
            } => {
                // 🔧 检测 while 条件中的赋值
                if let Expr::Assignment { .. } = condition.as_ref() {
                    self.add_warning(
                        "while 条件中使用了赋值操作 (=)，请确认是否应该使用比较操作 (==)".to_string(),
                        condition.span(),
                    );
                }

                // 条件可以是布尔或数字类型（非零为 true）
                let condition_type = self.infer_expr(condition, env);
                match condition_type {
                    Type::Number | Type::Int(_) | Type::Bool => {}
                    _ => self.add_constraint_with_context(condition_type.clone(), Type::bool(), condition.span(), 
                        format!("while 条件必须是 bool 或 number 类型, 实际为 `{}`", condition_type)),
                };

                // while循环的body可以是任何类型，但while表达式本身返回Unit
                self.infer_expr(body, env);
                Type::Unit
            }

            Expr::StructLiteral { name, fields, span } => {
                // 首先检查是否是泛型 struct，如果是则推断类型参数
                if let Some(gen_def) = self.generic_type_defs.get(name).cloned() {
                    if let Type::Struct { fields: template_fields, .. } = &gen_def.template {
                        // 检查字段数量匹配
                        if fields.len() != template_fields.len() {
                            // 计算缺少的字段
                            let provided_names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
                            let missing: Vec<String> = template_fields.iter()
                                .filter_map(|f| {
                                    if !provided_names.contains(&f.name.as_str()) {
                                        Some(f.name.clone())
                                    } else {
                                        None
                                    }
                                })
                                .collect();
                            self.add_error(TypeCheckError::MissingFields {
                                struct_name: name.clone(),
                                expected: template_fields.len(),
                                found: fields.len(),
                                missing,
                                span: *span,
                            });
                            return Type::Unknown;
                        }

                        // 从字段值推断泛型参数
                        let mut inferred_args: HashMap<String, Type> = HashMap::new();
                        for field_init in fields {
                            if let Some(field_def) = template_fields.iter().find(|f| f.name == field_init.name) {
                                let field_value_type = self.infer_expr(&field_init.value, env);
                                // 尝试从字段类型推断泛型参数
                                self.infer_type_params_from_value(
                                    &field_def.field_type,
                                    &field_value_type,
                                    &gen_def.type_params,
                                    &mut inferred_args,
                                );
                            } else {
                                let available: Vec<String> = template_fields.iter().map(|f| f.name.clone()).collect();
                                self.add_error(TypeCheckError::UnknownField {
                                    struct_name: name.clone(),
                                    field_name: field_init.name.clone(),
                                    available_fields: available,
                                    span: field_init.span,
                                });
                            }
                        }

                        // 构建具体类型参数列表
                        let concrete_args: Vec<Type> = gen_def.type_params.iter().map(|param| {
                            inferred_args.get(param).cloned().unwrap_or(Type::Unknown)
                        }).collect();

                        // 实例化泛型类型
                        let instantiated = self.instantiate_generic_type(name, &concrete_args);

                        return instantiated;
                    }
                }

                // 查找非泛型结构体类型定义
                if let Some(struct_type) = self.custom_types.get(name).cloned() {
                    match struct_type {
                        Type::Struct {
                            name: struct_name,
                            fields: field_defs,
                        } => {
                            // 检查字段是否完整匹配
                            if fields.len() != field_defs.len() {
                                // 计算缺少的字段
                                let provided_names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
                                let missing: Vec<String> = field_defs.iter()
                                    .filter_map(|f| {
                                        if !provided_names.contains(&f.name.as_str()) {
                                            Some(f.name.clone())
                                        } else {
                                            None
                                        }
                                    })
                                    .collect();
                                self.add_error(TypeCheckError::MissingFields {
                                    struct_name: struct_name.clone(),
                                    expected: field_defs.len(),
                                    found: fields.len(),
                                    missing,
                                    span: *span,
                                });
                                return Type::Unknown;
                            }

                            // 检查每个字段的类型
                            for field_init in fields {
                                if let Some(field_def) =
                                    field_defs.iter().find(|f| f.name == field_init.name)
                                {
                                    let field_value_type = self.infer_expr(&field_init.value, env);
                                    self.add_constraint_with_context(
                                        field_def.field_type.clone(),
                                        field_value_type,
                                        field_init.span,
                                    "结构体字段类型不匹配",
                                    );
                                } else {
                                    let available: Vec<String> = field_defs.iter().map(|f| f.name.clone()).collect();
                                    self.add_error(TypeCheckError::UnknownField {
                                        struct_name: struct_name.clone(),
                                        field_name: field_init.name.clone(),
                                        available_fields: available,
                                        span: field_init.span,
                                    });
                                }
                            }

                            // 返回结构体类型
                            Type::Struct {
                                name: struct_name,
                                fields: field_defs,
                            }
                        }
                        _ => {
                            self.add_error(TypeCheckError::NotAStruct {
                                name: name.clone(),
                                span: *span,
                            });
                            Type::Unknown
                        }
                    }
                } else {
                    self.add_error(TypeCheckError::UndefinedType {
                        name: name.clone(),
                        span: *span,
                    });
                    Type::Unknown
                }
            }
            Expr::ArrayLiteral { elements, span } => {
                if elements.is_empty() {
                    let elem_type = Type::Var(self.fresh_type_var());
                    Type::array(elem_type)
                } else {
                    let element_type = self.infer_expr(&elements[0], env);
                    for element in &elements[1..] {
                        let current_type = self.infer_expr(element, env);
                        self.add_constraint_with_context(element_type.clone(), current_type.clone(), element.span(), 
                            format!("数组元素类型不一致: 期望 `{}`, 实际 `{}`", element_type, current_type));
                    }
                    Type::array(element_type)
                }
            }
            Expr::FieldAccess {
                object,
                field,
                span,
            } => {
                let object_type = self.infer_expr(object, env);

                // 如果是引用类型，提取内部类型
                let actual_type = match &object_type {
                    Type::Reference { inner } => inner.as_ref(),
                    _ => &object_type,
                };

                // 字符串 .len 属性
                if matches!(actual_type, Type::String) {
                    if field == "len" {
                        return Type::Number;
                    } else {
                        self.add_error(TypeCheckError::NotAStruct {
                            name: format!("字符串类型没有属性 '{}'", field),
                            span: *span,
                        });
                        return Type::Unknown;
                    }
                }

                match actual_type {
                    Type::Struct { fields, .. } => {
                        if let Some(field_def) = fields.iter().find(|f| f.name == *field) {
                            field_def.field_type.clone()
                        } else {
                            let available: Vec<String> = fields.iter().map(|f| f.name.clone()).collect();
                            self.add_error(TypeCheckError::UnknownField {
                                struct_name: "unknown".to_string(),
                                field_name: field.clone(),
                                available_fields: available,
                                span: *span,
                            });
                            Type::Unknown
                        }
                    }
                    Type::Var(_) => {
                        // 对于类型变量，我们不能立即确定字段类型，需要更复杂的约束求解
                        // 简化处理：返回一个新的类型变量
                        Type::Var(self.fresh_type_var())
                    }
                    Type::Unknown => Type::Unknown,
                    _ => {
                        self.add_error(TypeCheckError::NotAStruct {
                            name: "".to_string(),
                            span: *span,
                        });
                        Type::Unknown
                    }
                }
            }
            Expr::ArrayLen { array, span } => {
                let array_type = self.infer_expr(array, env);
                match array_type {
                    Type::Array { .. } => Type::Number,
                    Type::String => Type::Number,
                    Type::Reference { inner } => match *inner {
                        Type::Array { .. } => Type::Number,
                        Type::String => Type::Number,
                        other => {
                            self.add_error(TypeCheckError::TypeMismatch {
                                expected: Type::array(Type::Unknown),
                                found: Type::Reference {
                                    inner: Box::new(other),
                                },
                                span: *span,
                                context: Some("len() 的参数必须是数组或字符串".to_string()),
                            });
                            Type::Unknown
                        }
                    },
                    Type::Var(_) => {
                        let element_var = Type::Var(self.fresh_type_var());
                        let expected = Type::array(element_var);
                        self.add_constraint_with_context(array_type.clone(), expected, array.span(), 
                            format!("数组类型不匹配: 期望数组, 实际 `{}`", array_type));
                        Type::Number
                    }
                    Type::Unknown => Type::Unknown,
                    Type::Number => {
                        self.add_error(TypeCheckError::BuiltinFunctionError {
                            function: "len".to_string(),
                            message: "expects an array or string, but got a number. len() measures the length of arrays and strings, not numbers.".to_string(),
                            span: *span,
                        });
                        Type::Unknown
                    }
                    other => {
                        self.add_error(TypeCheckError::BuiltinFunctionError {
                            function: "len".to_string(),
                            message: format!("expects an array or string, but got {}", other),
                            span: *span,
                        });
                        Type::Unknown
                    }
                }
            }
            Expr::Abs { value, span } => {
                let value_type = self.infer_expr(value, env);
                match value_type {
                    Type::Number => Type::Number,
                    Type::Unknown => Type::Unknown,
                    other => {
                        self.add_error(TypeCheckError::TypeMismatch {
                            expected: Type::Number,
                            found: other,
                            span: *span,
                            context: Some("abs() 的参数必须是 number 类型".to_string()),
                        });
                        Type::Unknown
                    }
                }
            }
            Expr::Min { left, right, span } => {
                let left_type = self.infer_expr(left, env);
                let right_type = self.infer_expr(right, env);
                let mut ok = true;
                if left_type != Type::Number && left_type != Type::Unknown {
                    self.add_error(TypeCheckError::TypeMismatch {
                        expected: Type::Number,
                        found: left_type,
                        span: left.span(),
                        context: Some("min() 的左操作数必须是 number 类型".to_string()),
                    });
                    ok = false;
                }
                if right_type != Type::Number && right_type != Type::Unknown {
                    self.add_error(TypeCheckError::TypeMismatch {
                        expected: Type::Number,
                        found: right_type,
                        span: right.span(),
                        context: Some("min() 的右操作数必须是 number 类型".to_string()),
                    });
                    ok = false;
                }
                if ok { Type::Number } else { Type::Unknown }
            }
            Expr::Max { left, right, span } => {
                let left_type = self.infer_expr(left, env);
                let right_type = self.infer_expr(right, env);
                let mut ok = true;
                if left_type != Type::Number && left_type != Type::Unknown {
                    self.add_error(TypeCheckError::TypeMismatch {
                        expected: Type::Number,
                        found: left_type,
                        span: left.span(),
                        context: Some("max() 的左操作数必须是 number 类型".to_string()),
                    });
                    ok = false;
                }
                if right_type != Type::Number && right_type != Type::Unknown {
                    self.add_error(TypeCheckError::TypeMismatch {
                        expected: Type::Number,
                        found: right_type,
                        span: right.span(),
                        context: Some("max() 的右操作数必须是 number 类型".to_string()),
                    });
                    ok = false;
                }
                if ok { Type::Number } else { Type::Unknown }
            }
            Expr::Clamp { value, min_val, max_val, span } => {
                let value_type = self.infer_expr(value, env);
                let min_type = self.infer_expr(min_val, env);
                let max_type = self.infer_expr(max_val, env);
                let mut ok = true;
                if value_type != Type::Number && value_type != Type::Unknown {
                    self.add_error(TypeCheckError::TypeMismatch {
                        expected: Type::Number,
                        found: value_type,
                        span: value.span(),
                        context: Some("clamp() 的值参数必须是 number 类型".to_string()),
                    });
                    ok = false;
                }
                if min_type != Type::Number && min_type != Type::Unknown {
                    self.add_error(TypeCheckError::TypeMismatch {
                        expected: Type::Number,
                        found: min_type,
                        span: min_val.span(),
                        context: Some("clamp() 的最小值参数必须是 number 类型".to_string()),
                    });
                    ok = false;
                }
                if max_type != Type::Number && max_type != Type::Unknown {
                    self.add_error(TypeCheckError::TypeMismatch {
                        expected: Type::Number,
                        found: max_type,
                        span: max_val.span(),
                        context: Some("clamp() 的最大值参数必须是 number 类型".to_string()),
                    });
                    ok = false;
                }
                if ok { Type::Number } else { Type::Unknown }
            }
            Expr::StrIndex { string, index, span } => {
                let string_type = self.infer_expr(string, env);
                let index_type = self.infer_expr(index, env);
                let mut ok = true;
                if string_type != Type::String && string_type != Type::Unknown {
                    self.add_error(TypeCheckError::TypeMismatch {
                        expected: Type::String,
                        found: string_type,
                        span: string.span(),
                        context: Some("字符串索引操作要求 string 类型".to_string()),
                    });
                    ok = false;
                }
                if index_type != Type::Number && index_type != Type::Unknown {
                    self.add_error(TypeCheckError::TypeMismatch {
                        expected: Type::Number,
                        found: index_type,
                        span: index.span(),
                        context: Some("字符串索引要求 number 类型索引".to_string()),
                    });
                    ok = false;
                }
                if ok { Type::Number } else { Type::Unknown }
            }
            Expr::CharAt { string, index, span } => {
                let string_type = self.infer_expr(string, env);
                let index_type = self.infer_expr(index, env);
                let mut ok = true;
                if string_type != Type::String && string_type != Type::Unknown {
                    self.add_error(TypeCheckError::TypeMismatch {
                        expected: Type::String,
                        found: string_type,
                        span: string.span(),
                        context: Some("charAt() 的字符串参数必须是 string 类型".to_string()),
                    });
                    ok = false;
                }
                if index_type != Type::Number && index_type != Type::Unknown {
                    self.add_error(TypeCheckError::TypeMismatch {
                        expected: Type::Number,
                        found: index_type,
                        span: index.span(),
                        context: Some("charAt() 的索引参数必须是 number 类型".to_string()),
                    });
                    ok = false;
                }
                if ok { Type::String } else { Type::Unknown }
            }
            Expr::Substring { string, start, length, span } => {
                let string_type = self.infer_expr(string, env);
                let start_type = self.infer_expr(start, env);
                let length_type = self.infer_expr(length, env);
                let mut ok = true;
                if string_type != Type::String && string_type != Type::Unknown {
                    self.add_error(TypeCheckError::TypeMismatch {
                        expected: Type::String,
                        found: string_type,
                        span: string.span(),
                        context: Some("substring() 的字符串参数必须是 string 类型".to_string()),
                    });
                    ok = false;
                }
                if start_type != Type::Number && start_type != Type::Unknown {
                    self.add_error(TypeCheckError::TypeMismatch {
                        expected: Type::Number,
                        found: start_type,
                        span: start.span(),
                        context: Some("substring() 的起始位置必须是 number 类型".to_string()),
                    });
                    ok = false;
                }
                if length_type != Type::Number && length_type != Type::Unknown {
                    self.add_error(TypeCheckError::TypeMismatch {
                        expected: Type::Number,
                        found: length_type,
                        span: length.span(),
                        context: Some("substring() 的长度参数必须是 number 类型".to_string()),
                    });
                    ok = false;
                }
                if ok { Type::String } else { Type::Unknown }
            }
            Expr::StrContains { string, char_code, span } => {
                let string_type = self.infer_expr(string, env);
                let char_code_type = self.infer_expr(char_code, env);
                let mut ok = true;
                if string_type != Type::String && string_type != Type::Unknown {
                    self.add_error(TypeCheckError::TypeMismatch {
                        expected: Type::String,
                        found: string_type,
                        span: string.span(),
                        context: Some("字符串查找操作要求 string 类型".to_string()),
                    });
                    ok = false;
                }
                if char_code_type != Type::Number && char_code_type != Type::Unknown {
                    self.add_error(TypeCheckError::TypeMismatch {
                        expected: Type::Number,
                        found: char_code_type,
                        span: char_code.span(),
                        context: Some("字符串查找的子串参数必须是 string 或 number 类型".to_string()),
                    });
                    ok = false;
                }
                if ok { Type::Number } else { Type::Unknown }
            }
            Expr::SplitCount { string, separator, span } => {
                let string_type = self.infer_expr(string, env);
                let sep_type = self.infer_expr(separator, env);
                let mut ok = true;
                if string_type != Type::String && string_type != Type::Unknown {
                    self.add_error(TypeCheckError::TypeMismatch {
                        expected: Type::String,
                        found: string_type,
                        span: string.span(),
                        context: Some("字符串分割操作要求 string 类型".to_string()),
                    });
                    ok = false;
                }
                if sep_type != Type::Number && sep_type != Type::Unknown {
                    self.add_error(TypeCheckError::TypeMismatch {
                        expected: Type::Number,
                        found: sep_type,
                        span: separator.span(),
                        context: Some("字符串分割的分隔符参数类型不匹配".to_string()),
                    });
                    ok = false;
                }
                if ok { Type::Number } else { Type::Unknown }
            }
            Expr::Trim { string, span } => {
                let string_type = self.infer_expr(string, env);
                if string_type != Type::String && string_type != Type::Unknown {
                    self.add_error(TypeCheckError::TypeMismatch {
                        expected: Type::String,
                        found: string_type,
                        span: string.span(),
                        context: Some("trim() 的参数必须是 string 类型".to_string()),
                    });
                    Type::Unknown
                } else {
                    Type::String
                }
            }
            Expr::CharToString { expr, span } => {
                let expr_type = self.infer_expr(expr, env);
                let mut ok = true;
                if expr_type != Type::Number && expr_type != Type::Unknown {
                    self.add_error(TypeCheckError::TypeMismatch {
                        expected: Type::Number,
                        found: expr_type,
                        span: expr.span(),
                        context: Some("char_to_string() 的参数必须是 number 类型（字符编码）".to_string()),
                    });
                    ok = false;
                }
                if ok { Type::String } else { Type::Unknown }
            }
            Expr::ToString { expr, span } => {
                let expr_type = self.infer_expr(expr, env);
                let mut ok = true;
                if expr_type != Type::Number && expr_type != Type::Unknown {
                    self.add_error(TypeCheckError::TypeMismatch {
                        expected: Type::Number,
                        found: expr_type,
                        span: expr.span(),
                        context: Some("to_string() 的参数必须是 number 类型".to_string()),
                    });
                    ok = false;
                }
                if ok { Type::String } else { Type::Unknown }
            }
            Expr::Index { array, index, span } => {
                let array_type = self.infer_expr(array, env);
                let index_type = self.infer_expr(index, env);
                self.add_constraint_with_context(index_type, Type::Number, index.span(), "数组索引必须是 number 类型");

                match array_type {
                    Type::Array { element } => *element,
                    Type::Reference { inner } => match *inner {
                        Type::Array { element } => *element,
                        other => {
                            self.add_error(TypeCheckError::TypeMismatch {
                                expected: Type::array(Type::Unknown),
                                found: Type::Reference {
                                    inner: Box::new(other),
                                },
                                span: *span,
                                context: Some("数组元素类型访问：表达式必须是数组类型".to_string()),
                            });
                            Type::Unknown
                        }
                    },
                    Type::Var(_) => {
                        let element_var = Type::Var(self.fresh_type_var());
                        let expected = Type::array(element_var.clone());
                        self.add_constraint_with_context(array_type.clone(), expected, array.span(), 
                            format!("数组类型不匹配: 期望数组, 实际 `{}`", array_type));
                        element_var
                    }
                    Type::Unknown => Type::Unknown,
                    other => {
                        self.add_error(TypeCheckError::TypeMismatch {
                            expected: Type::array(Type::Unknown),
                            found: other,
                            span: *span,
                            context: Some("数组元素类型访问：表达式必须是数组类型".to_string()),
                        });
                        Type::Unknown
                    }
                }
            }

            Expr::TupleLiteral { elements, span } => {
                let elem_types: Vec<Type> = elements
                    .iter()
                    .map(|e| self.infer_expr(e, env))
                    .collect();
                Type::tuple(elem_types)
            }
            Expr::TupleAccess { object, index, span } => {
                let obj_type = self.infer_expr(object, env);
                // 自动解引用
                let actual_type = match &obj_type {
                    Type::Reference { inner } => inner.as_ref(),
                    _ => &obj_type,
                };
                match actual_type {
                    Type::Tuple(types) => {
                        if *index >= types.len() {
                            self.add_error(TypeCheckError::IndexOutOfBounds {
                                index: *index as i64,
                                length: types.len() as i64,
                                span: *span,
                            });
                            Type::Unknown
                        } else {
                            types[*index].clone()
                        }
                    }
                    Type::Var(_) => {
                        // 类型变量，无法立即确定，返回新的类型变量
                        Type::Var(self.fresh_type_var())
                    }
                    Type::Unknown => Type::Unknown,
                    _ => {
                        self.add_error(TypeCheckError::NotAStruct {
                            name: format!("不是元组类型，无法使用 .{} 索引访问", index),
                            span: *span,
                        });
                        Type::Unknown
                    }
                }
            }

            Expr::Reference { expr, .. } => {
                // 引用表达式的类型是对内部表达式类型的引用
                let inner_type = self.infer_expr(expr, env);
                Type::reference(inner_type)
            }

            Expr::Dereference { expr, span } => {
                // 解引用表达式：从引用类型中提取内部类型
                let expr_type = self.infer_expr(expr, env);
                match expr_type {
                    Type::Reference { inner } => *inner,
                    // 类型变量：添加约束，要求该类型变量必须为引用类型
                    Type::Var(_) => {
                        let inner_type = Type::Var(self.fresh_type_var());
                        let expected_ref_type = Type::reference(inner_type.clone());
                        self.add_constraint_with_context(expr_type.clone(), expected_ref_type, *span, 
                            format!("解引用操作要求引用类型 (&T), 但操作数为 `{}`", expr_type));
                        inner_type
                    }
                    _ => {
                        // 创建一个占位符引用类型用于错误报告
                        let expected_ref_type = Type::reference(Type::Unknown);
                        self.add_error(TypeCheckError::TypeMismatch {
                            expected: expected_ref_type,
                            found: expr_type.clone(),
                            span: *span,
                            context: Some(format!("解引用操作要求引用类型 (&T), 但操作数为 `{}`", expr_type)),
                        });
                        Type::Unknown
                    }
                }
            }

            Expr::HeapAllocate { value, .. } => {
                let inner_type = self.infer_expr(value, env);
                Type::reference(inner_type)
            }

            Expr::HeapFree { pointer, span } => {
                let pointer_type = self.infer_expr(pointer, env);
                match pointer_type {
                    Type::Reference { .. } => Type::Unit,
                    _ => {
                        let expected = Type::reference(Type::Unknown);
                        self.add_error(TypeCheckError::TypeMismatch {
                            expected,
                            found: pointer_type.clone(),
                            span: *span,
                            context: Some(format!("释放操作要求引用类型 (&T), 但操作数为 `{}`", pointer_type)),
                        });
                        Type::Unit
                    }
                }
            }

            Expr::Retain { pointer, span } | Expr::Release { pointer, span } => {
                let pointer_type = self.infer_expr(pointer, env);
                match pointer_type {
                    Type::Reference { .. } => Type::Unit,
                    _ => {
                        let expected = Type::reference(Type::Unknown);
                        self.add_error(TypeCheckError::TypeMismatch {
                            expected,
                            found: pointer_type,
                            span: *span,
                            context: Some("引用赋值操作要求引用类型 (&T)".to_string()),
                        });
                        Type::Unit
                    }
                }
            }

            Expr::UnsafeLoad { addr, byte_size, span } => {
                let addr_type = self.infer_expr(addr, env);
                if addr_type != Type::Number {
                    self.add_error(TypeCheckError::TypeMismatch {
                        expected: Type::Number,
                        found: addr_type,
                        span: *span,
                        context: Some("unsafe_load 地址参数必须是 number 类型".to_string()),
                    });
                }
                Type::Number
            }

            Expr::UnsafeStore { addr, value, byte_size, span } => {
                let addr_type = self.infer_expr(addr, env);
                if addr_type != Type::Number {
                    self.add_error(TypeCheckError::TypeMismatch {
                        expected: Type::Number,
                        found: addr_type,
                        span: *span,
                        context: Some("unsafe_store 地址参数必须是 number 类型".to_string()),
                    });
                }
                let val_type = self.infer_expr(value, env);
                if val_type != Type::Number {
                    self.add_error(TypeCheckError::TypeMismatch {
                        expected: Type::Number,
                        found: val_type,
                        span: *span,
                        context: Some("unsafe_store 值参数必须是 number 类型".to_string()),
                    });
                }
                Type::Number
            }

            Expr::RuntimeGlobal { span: _, .. } => {
                Type::Number
            }

            Expr::GcRegOp { span: _, .. } => {
                Type::Number
            }

            Expr::Assignment {
                target,
                value,
                span,
            } => {
                // 赋值表达式：检查目标是否可赋值，并统一类型
                let target_type = self.infer_expr(target, env);
                let value_type = self.infer_expr(value, env);

                // 检查赋值目标的有效性
                match &**target {
                    Expr::Identifier { .. } => {
                        // 变量赋值：统一类型
                        self.add_constraint_with_context(target_type.clone(), value_type.clone(), *span, 
                            format!("赋值类型不匹配: 变量为 `{}`, 值为 `{}`", target_type, value_type));
                    }
                    Expr::Constructor { args, .. } if args.is_empty() => {
                        // 大写字母开头的变量被 parser 误解析为零参数 Constructor
                        // 在赋值目标位置应视为普通变量
                        self.add_constraint_with_context(target_type.clone(), value_type.clone(), *span, 
                            format!("赋值类型不匹配: 变量为 `{}`, 值为 `{}`", target_type, value_type));
                    }
                    Expr::FieldAccess { .. } => {
                        // 字段赋值：统一类型
                        self.add_constraint_with_context(target_type.clone(), value_type.clone(), *span, 
                            format!("字段赋值类型不匹配: 字段为 `{}`, 值为 `{}`", target_type, value_type));
                    }
                    Expr::Index { .. } => {
                        // 数组下标赋值：统一类型
                        self.add_constraint_with_context(target_type.clone(), value_type.clone(), *span, 
                            format!("数组元素赋值类型不匹配: 元素为 `{}`, 值为 `{}`", target_type, value_type));
                    }
                    _ => {
                        // 其他表达式不能作为赋值目标
                        self.add_error(TypeCheckError::InvalidAssignmentTarget { span: *span });
                    }
                }

                // 赋值表达式返回单元类型
                Type::Unit
            }
            // ===== 代数效应（最小类型规则） =====
            Expr::EffectPerform { tag, payload, .. } => {
                // 简化：tag 推断为 number（或不约束），payload 任意，perform 表达式结果设为 Unknown
                let _ = self.infer_expr(tag, env);
                let _ = self.infer_expr(payload, env);
                Type::Unknown
            }
            Expr::EffectResume { value, .. } => {
                let _ = self.infer_expr(value, env);
                // resume 表达式自身结果设为 Unknown（通常不需要值）
                Type::Unknown
            }
            Expr::EffectHandle {
                tag,
                param,
                handler,
                body,
                ..
            } => {
                // 语义：在 body 的动态作用域内安装处理器。
                // 类型规则（简化）：
                // - 让 tag 推断；
                // - handler 在一个扩展环境中检查：绑定 param: α，handler 的类型约束为 (α) -> β；
                // - handle 表达式的结果类型取 body 的类型。
                let _ = self.infer_expr(tag, env);

                // 为 handler 构造期望函数类型 (param_ty -> ret_ty)
                let param_ty = Type::Var(self.fresh_type_var());
                let ret_ty = Type::Var(self.fresh_type_var());
                let expected_handler_ty = Type::function(vec![param_ty.clone()], ret_ty.clone());

                // 在扩展环境中绑定参数名，以便 handler 内引用到 param 不报未定义
                let mut handler_env = env.clone();
                handler_env.insert(param.clone(), param_ty.clone());

                let handler_ty = self.infer_expr(handler, &handler_env);
                // 约束 handler 的函数类型
                self.add_constraint_with_context(expected_handler_ty, handler_ty, handler.span(), "effect handler 类型不匹配");

                // body 在原环境中检查，作为整体类型
                self.infer_expr(body, env)
            }

            Expr::TypeCast { expr, target_type, span } => {
                let inner_type = self.infer_expr(expr, env);

                match (&inner_type, target_type) {
                    (Type::Number, Type::Int(_)) | (Type::Int(_), Type::Number) => {},
                    (Type::Number, Type::Number) => {},
                    (Type::Int(_), Type::Int(_)) => {},
                    (Type::Int(_), Type::Bool) | (Type::Number, Type::Bool) => {},
                    (Type::Bool, Type::Number) | (Type::Bool, Type::Int(_)) => {},
                    _ => {
                        self.add_error(
                            TypeCheckError::TypeMismatch {
                                expected: target_type.clone(),
                                found: inner_type,
                                span: *span,
                                context: Some("函数调用参数类型不匹配".to_string()),
                            }
                        );
                    }
                }

                target_type.clone()
            }

            Expr::ForIn {
                var, start, end, body, inclusive: _, ..
            } => {
                // start 和 end 必须是数字类型
                let start_type = self.infer_expr(start, env);
                self.add_constraint_with_context(start_type.clone(), Type::Number, start.span(), 
                    format!("for 循环范围起始必须是 number 类型, 实际为 `{}`", start_type));
                let end_type = self.infer_expr(end, env);
                self.add_constraint_with_context(end_type.clone(), Type::Number, end.span(), 
                    format!("for 循环范围结束必须是 number 类型, 实际为 `{}`", end_type));

                // 循环变量是数字类型
                let mut loop_env = env.clone();
                loop_env.insert(var.clone(), Type::Number);

                // for 循环的 body 可以是任何类型，但 for 表达式本身返回 Unit
                self.infer_expr(body, &loop_env);
                Type::Unit
            }

            Expr::ForArray { var, array, body, .. } => {
                let array_type = self.infer_expr(array, env);

                let element_type = match &array_type {
                    Type::Array { element } => element.as_ref().clone(),
                    _ => {
                        self.add_error(TypeCheckError::TypeMismatch {
                            expected: Type::array(Type::Unknown),
                            found: array_type.clone(),
                            span: array.span(),
                            context: Some(format!("for 循环的迭代对象必须是数组类型, 实际为 `{}`", array_type)),
                        });
                        Type::Number
                    }
                };

                let mut loop_env = env.clone();
                loop_env.insert(var.clone(), element_type);

                self.infer_expr(body, &loop_env);
                Type::Unit
            }

            Expr::Break { .. } => {
                // break 返回 Unit（实际上会跳转，不会使用返回值）
                Type::Unit
            }

            Expr::Continue { .. } => {
                // continue 返回 Unit（实际上会跳转，不会使用返回值）
                Type::Unit
            }

            Expr::Return { value, .. } => {
                // return 的类型取决于返回值
                if let Some(v) = value {
                    self.infer_expr(v, env)
                } else {
                    Type::Unit
                }
            }
        };

        // 存储所有表达式的类型信息（用于传递给MIR lowering）
        self.expr_types
            .insert(expr as *const Expr, inferred_type.clone());

        inferred_type
    }

    /// 推断语句并更新环境
    fn infer_statement(&mut self, stmt: &Statement, env: &mut TypeEnvironment) {
        match stmt {
            Statement::Let { name, pattern, value, type_annotation, span, .. } => {
                // 检查是否为解构模式绑定（如 let Point { x, y } = p）
                if let Some(pat) = pattern {
                    let value_type = self.infer_expr(value, env);
                    // 对解构模式进行类型检查，并绑定变量到 env
                    self.check_pattern(pat.as_ref(), &value_type, env);
                    // 记录解构模式中的变量定义
                    self.define_pattern_locals(pat.as_ref());
                } else if let Expr::Lambda { .. } = value {
                    // 检查是否为递归闭包（let f = |...| { ... f ... }）
                    let rec_type_var = self.fresh_type_var();
                    let mut rec_env = env.clone();
                    rec_env.insert(name.clone(), Type::Var(rec_type_var));

                    // 推断 lambda 体，此时 f 在环境中
                    let value_type = self.infer_expr(value, &rec_env);

                    // 统一递归类型变量和实际推断出的类型
                    let _ = self.unify(
                        &Type::Var(rec_type_var),
                        &value_type,
                        *span,
                        &Type::Var(rec_type_var),
                        &value_type,
                    );

                    // 如果有类型标注，将标注类型与推断类型统一
                    if let Some(annotated_type) = type_annotation {
                        let _ = self.unify(
                            annotated_type,
                            &value_type,
                            *span,
                            annotated_type,
                            &value_type,
                        );
                    }

                    // 检查变量遮蔽（shadow）
                    if env.contains_key(name) && !name.starts_with('_') {
                        self.add_warning(
                            format!("变量 `{}` 遮蔽了外层作用域中的同名变量", name),
                            *span,
                        );
                    }

                    env.insert(name.clone(), value_type);
                    self.define_local(name, *span);
                } else {
                    let value_type = self.infer_expr(value, env);

                    // 如果有类型标注，将标注类型与推断类型统一
                    if let Some(annotated_type) = type_annotation {
                        let _ = self.unify(
                            annotated_type,
                            &value_type,
                            *span,
                            annotated_type,
                            &value_type,
                        );
                    }

                    // 检查变量遮蔽（shadow）
                    if env.contains_key(name) && !name.starts_with('_') {
                        self.add_warning(
                            format!("变量 `{}` 遮蔽了外层作用域中的同名变量", name),
                            *span,
                        );
                    }

                    env.insert(name.clone(), value_type);
                    self.define_local(name, *span);
                }
            }
            Statement::Expression { expr, .. } => {
                self.infer_expr(expr, env);
            }
            Statement::TypeDef { name, variants, type_params, .. } => {
                // 如果是泛型枚举，存储为模板
                if !type_params.is_empty() {
                    let sum_variants: Vec<crate::types::SumVariant> = variants
                        .iter()
                        .map(|variant| {
                            crate::types::SumVariant {
                                name: variant.name.clone(),
                                data_types: variant.data_types.clone(),
                            }
                        })
                        .collect();
                    let template = Type::sum(name.clone(), sum_variants);
                    self.generic_type_defs.insert(name.clone(), GenericTypeDef {
                        type_params: type_params.clone(),
                        template,
                    });
                    return;
                }

                // 解析变体 data_types 中的骨架占位符（递归枚举自引用等场景）
                let mut sum_variants = Vec::new();
                for variant in variants {
                    let mut resolved_data_types = Vec::new();
                    for dt in &variant.data_types {
                        resolved_data_types.push(self.resolve_struct_field_from_parsed(dt));
                    }
                    sum_variants.push(crate::types::SumVariant {
                        name: variant.name.clone(),
                        data_types: resolved_data_types,
                    });
                }

                let sum_type = Type::sum(name.clone(), sum_variants.clone());

                // 将类型添加到自定义类型表中
                self.custom_types.insert(name.clone(), sum_type.clone());

                // 为每个构造器添加类型到环境中
                for variant in &sum_variants {
                    if !variant.data_types.is_empty() {
                        // 有数据的构造器是函数类型
                        let constructor_type =
                            Type::function(variant.data_types.clone(), sum_type.clone());
                        env.insert(variant.name.clone(), constructor_type);
                    } else {
                        // 无数据的构造器直接是该类型
                        env.insert(variant.name.clone(), sum_type.clone());
                    }
                }
            }
            Statement::StructDef { name, fields, type_params, .. } => {
                // 如果是泛型结构体，存储为模板
                if !type_params.is_empty() {
                    let struct_fields: Vec<crate::types::StructField> = fields
                        .iter()
                        .map(|field| {
                            crate::types::StructField {
                                name: field.name.clone(),
                                field_type: field.field_type.clone(),
                            }
                        })
                        .collect();
                    let template = Type::struct_type(name.clone(), struct_fields);
                    self.generic_type_defs.insert(name.clone(), GenericTypeDef {
                        type_params: type_params.clone(),
                        template,
                    });
                    return;
                }

                // 构建结构体类型的字段
                // field.field_type 已经是 parser 生成的结构化 Type，但自定义类型名可能是骨架占位符
                // 需要用 process_struct_definitions 中创建的完整类型替换
                let mut struct_fields = Vec::new();
                for field in fields {
                    let resolved_type = self.resolve_struct_field_from_parsed(&field.field_type);
                    struct_fields.push(crate::types::StructField {
                        name: field.name.clone(),
                        field_type: resolved_type,
                    });
                }

                let struct_type = Type::struct_type(name.clone(), struct_fields);

                // 将类型添加到自定义类型表中
                self.custom_types.insert(name.clone(), struct_type);
            }
            Statement::Assignment { target, value, .. } => {
                // 赋值语句：推断目标和值的类型，并进行约束检查
                let target_type = self.infer_expr(target, env);
                let value_type = self.infer_expr(value, env);

                // 检查赋值目标的有效性
                match target {
                    Expr::Identifier { name, .. } => {
                        // 变量赋值：更新环境中的变量类型
                        if env.contains_key(name) {
                            // 变量已存在，统一类型
                            self.add_constraint_with_context(target_type.clone(), value_type.clone(), target.span(), 
                                format!("赋值类型不匹配: 变量 `{}` 为 `{}`, 值为 `{}`", name, target_type, value_type));
                            env.insert(name.clone(), value_type);
                        } else {
                            // 变量不存在，报告错误（或者可以选择自动创建）
                            let suggestion = self.suggest_variable(name, env);
                            self.add_error(TypeCheckError::UndefinedVariable {
                                name: name.clone(),
                                span: target.span(),
                                suggestion,
                            });
                        }
                    }
                    Expr::Constructor { name, args, .. } if args.is_empty() => {
                        // 大写字母开头的变量被 parser 误解析为零参数 Constructor
                        // 在赋值目标位置应视为普通变量
                        if env.contains_key(name) {
                            self.add_constraint_with_context(target_type.clone(), value_type.clone(), target.span(), 
                                format!("赋值类型不匹配: 变量 `{}` 为 `{}`, 值为 `{}`", name, target_type, value_type));
                            env.insert(name.clone(), value_type);
                        } else {
                            let suggestion = self.suggest_variable(name, env);
                            self.add_error(TypeCheckError::UndefinedVariable {
                                name: name.clone(),
                                span: target.span(),
                                suggestion,
                            });
                        }
                    }
                    Expr::FieldAccess { .. } => {
                        // 字段赋值：统一类型
                        self.add_constraint_with_context(target_type.clone(), value_type.clone(), target.span(), 
                            format!("字段赋值类型不匹配: 字段为 `{}`, 值为 `{}`", target_type, value_type));
                    }
                    Expr::Index { .. } => {
                        // 数组下标赋值：统一类型
                        self.add_constraint_with_context(target_type.clone(), value_type.clone(), target.span(), 
                            format!("数组元素赋值类型不匹配: 元素为 `{}`, 值为 `{}`", target_type, value_type));
                    }
                    _ => {
                        // 其他表达式不能作为赋值目标
                        self.add_error(TypeCheckError::InvalidAssignmentTarget {
                            span: target.span(),
                        });
                    }
                }
            }
            Statement::FunctionDef {
                name,
                params,
                body,
                span,
                ..
            } => {
                // 如果是重复定义（已在 collect_function_definitions 中标记），报告错误并跳过函数体检查
                if self.duplicate_function_spans.contains(&(span.start, span.end)) {
                    self.add_error(TypeCheckError::DuplicateFunctionDefinition {
                        name: name.clone(),
                        span: *span,
                    });
                    return;
                }
                // 从缓存中获取函数签名（已在 collect_function_definitions 中解析和验证）
                let signature = self
                    .function_signatures
                    .get(name)
                    .cloned()
                    .expect("函数签名应该已在 collect_function_definitions 中缓存");

                // 记录函数定义
                self.define_local(name, *span);

                // 为函数体创建新的局部作用域
                self.push_local_scope();

                // 1. 创建新的作用域
                let mut func_env = env.clone();

                // 2. 将参数及其类型添加到函数环境中（使用缓存的类型）
                for (param, param_ty) in params.iter().zip(signature.param_types.iter()) {
                    func_env.insert(param.name.clone(), param_ty.clone());
                    self.define_local(&param.name, param.span);
                }

                // 3. 推断函数体类型
                let body_ty = self.infer_expr(body, &mut func_env);

                // 4. 添加约束：函数体的类型必须与声明的返回类型一致
                self.add_constraint_with_context(
                    signature.return_type.clone(),
                    body_ty,
                    *span,
                    format!("函数 `{}` 的返回值类型与函数体不一致", name),
                );

                // 5. 构造函数类型
                let func_type = Type::Function {
                    params: signature.param_types.clone(),
                    return_type: Box::new(signature.return_type.clone()),
                };

                // 6. 将函数名加入当前环境
                env.insert(name.clone(), func_type);

                // 7. 检查是否需要 generalize
                if let Some(sig) = self.function_signatures.get(name) {
                    let func_type = Type::Function {
                        params: sig.param_types.clone(),
                        return_type: Box::new(sig.return_type.clone()),
                    };
                    let free_vars = func_type.free_vars();
                    if !free_vars.is_empty() {
                        self.function_schemes.insert(name.clone(), TypeScheme::new(free_vars, func_type));
                    }
                }

                // 退出函数体作用域（抑制参数未使用警告，函数参数可能不被使用）
                self.pop_local_scope(true);
            }
        }
    }

    /// 检查模式并更新环境
    fn check_pattern(
        &mut self,
        pattern: &crate::ast::Pattern,
        expected_type: &Type,
        env: &mut TypeEnvironment,
    ) {
        match pattern {
            crate::ast::Pattern::Wildcard { .. } => {
                // 通配符模式匹配任何类型，不绑定变量
            }
            crate::ast::Pattern::Variable { name, .. } => {
                // 变量模式绑定整个值
                env.insert(name.clone(), expected_type.clone());
            }
            crate::ast::Pattern::Number { value: _, span } => {
                // 数字模式必须匹配数字类型
                self.add_constraint_with_context(expected_type.clone(), Type::Number, *span, "期望 number 类型");
            }
            crate::ast::Pattern::Boolean { value: _, span } => {
                // 布尔模式必须匹配布尔类型
                self.add_constraint_with_context(expected_type.clone(), Type::bool(), *span, "期望 bool 类型");
            }
            crate::ast::Pattern::Constructor { name, args, span } => {
                match name.as_str() {
                    "True" | "False" => {
                        self.add_constraint_with_context(expected_type.clone(), Type::bool(), *span, "期望 bool 类型");
                    }
                    "Some" => {
                        if let Some(arg_pattern) = args.get(0) {
                            // Some(x) 模式，从 expected_type 中提取内部类型
                            match expected_type {
                                Type::Sum { name, variants } if name == "Option" => {
                                    // 期望是 Option<T>，找到 Some 变体的类型
                                    if let Some(some_variant) =
                                        variants.iter().find(|v| v.name == "Some")
                                    {
                                        if let Some(inner_type) = some_variant.data_types.first() {
                                            self.check_pattern(arg_pattern, inner_type, env);
                                        } else {
                                            self.add_error(TypeCheckError::InvalidPattern {
                                                message: "Some variant should have data type"
                                                    .to_string(),
                                                span: *span,
                                            });
                                        }
                                    } else {
                                        self.add_error(TypeCheckError::InvalidPattern {
                                            message: "Expected Option type with Some variant"
                                                .to_string(),
                                            span: *span,
                                        });
                                    }
                                }
                                _ => {
                                    // 如果 expected_type 不是具体的 Option，创建一个约束
                                    let inner_type = Type::Var(self.fresh_type_var());
                                    let option_type = Type::option(inner_type.clone());
                                    self.add_constraint_with_context(expected_type.clone(), option_type, *span, "Option 类型不匹配");
                                    self.check_pattern(arg_pattern, &inner_type, env);
                                }
                            }
                        } else {
                            self.add_error(TypeCheckError::InvalidPattern {
                                message: "Some constructor requires an argument".to_string(),
                                span: *span,
                            });
                        }
                    }
                    "None" => {
                        // None 模式，确保 expected_type 是 Option 类型
                        match expected_type {
                            Type::Sum { name, .. } if name == "Option" => {
                                // 已经是 Option 类型，直接匹配
                            }
                            _ => {
                                // 如果不是具体的 Option，创建约束
                                let inner_type = Type::Var(self.fresh_type_var());
                                let option_type = Type::option(inner_type);
                                self.add_constraint_with_context(expected_type.clone(), option_type, *span, "Option 类型不匹配");
                            }
                        }
                    }
                    "Ok" => {
                        // Ok(x) 模式，从 expected_type 中提取 Ok 变体的类型
                        if let Some(arg_pattern) = args.get(0) {
                            match expected_type {
                                Type::Sum { name, variants } if name == "Result" => {
                                    if let Some(ok_variant) =
                                        variants.iter().find(|v| v.name == "Ok")
                                    {
                                        if let Some(ok_type) = ok_variant.data_types.first() {
                                            self.check_pattern(arg_pattern, ok_type, env);
                                        } else {
                                            self.add_error(TypeCheckError::InvalidPattern {
                                                message: "Ok variant should have data type"
                                                    .to_string(),
                                                span: *span,
                                            });
                                        }
                                    }
                                }
                                _ => {
                                    let ok_type = Type::Var(self.fresh_type_var());
                                    let err_type = Type::Var(self.fresh_type_var());
                                    let result_type = Type::result(ok_type.clone(), err_type);
                                    self.add_constraint_with_context(expected_type.clone(), result_type, *span, "类型不匹配",
                                    );
                                    self.check_pattern(arg_pattern, &ok_type, env);
                                }
                            }
                        } else {
                            self.add_error(TypeCheckError::InvalidPattern {
                                message: "Ok constructor requires an argument".to_string(),
                                span: *span,
                            });
                        }
                    }
                    "Err" => {
                        // Err(e) 模式，从 expected_type 中提取 Err 变体的类型
                        if let Some(arg_pattern) = args.get(0) {
                            match expected_type {
                                Type::Sum { name, variants } if name == "Result" => {
                                    if let Some(err_variant) =
                                        variants.iter().find(|v| v.name == "Err")
                                    {
                                        if let Some(err_type) = err_variant.data_types.first() {
                                            self.check_pattern(arg_pattern, err_type, env);
                                        } else {
                                            self.add_error(TypeCheckError::InvalidPattern {
                                                message: "Err variant should have data type"
                                                    .to_string(),
                                                span: *span,
                                            });
                                        }
                                    }
                                }
                                _ => {
                                    let ok_type = Type::Var(self.fresh_type_var());
                                    let err_type = Type::Var(self.fresh_type_var());
                                    let result_type = Type::result(ok_type, err_type.clone());
                                    self.add_constraint_with_context(expected_type.clone(), result_type, *span, "类型不匹配",
                                    );
                                    self.check_pattern(arg_pattern, &err_type, env);
                                }
                            }
                        } else {
                            self.add_error(TypeCheckError::InvalidPattern {
                                message: "Err constructor requires an argument".to_string(),
                                span: *span,
                            });
                        }
                    }
                    _ => {
                        // 先检查是否是泛型 enum 的构造器
                        let mut found_generic = None;
                        for (gen_name, gen_def) in &self.generic_type_defs {
                            if let Type::Sum { variants, .. } = &gen_def.template {
                                if let Some(variant) = variants.iter().find(|v| v.name == *name) {
                                    found_generic = Some((gen_name.clone(), gen_def.type_params.clone(), variant.clone()));
                                    break;
                                }
                            }
                        }
                        if let Some((_enum_name, type_params, variant)) = found_generic {
                            let mut inferred_args: HashMap<String, Type> = HashMap::new();
                            if let Type::Sum { variants: exp_variants, .. } = expected_type {
                                if let Some(exp_variant) = exp_variants.iter().find(|v| v.name == *name) {
                                    for (i, param_ty) in variant.data_types.iter().enumerate() {
                                        if let Some(exp_ty) = exp_variant.data_types.get(i) {
                                            self.infer_type_params_from_value(
                                                param_ty,
                                                exp_ty,
                                                &type_params,
                                                &mut inferred_args,
                                            );
                                        }
                                    }
                                }
                            }

                            for (i, arg_pattern) in args.iter().enumerate() {
                                if let Some(param_ty) = variant.data_types.get(i) {
                                    let subst: std::collections::HashMap<String, Type> = type_params.iter().map(|p| {
                                        (p.clone(), inferred_args.get(p).cloned().unwrap_or(Type::Unknown))
                                    }).collect();
                                    let resolved_ty = self.substitute_type_params(param_ty, &subst);
                                    self.check_pattern(arg_pattern, &resolved_ty, env);
                                }
                            }
                            return;
                        }

                        // 动态查找构造器所属的 enum 类型
                        let mut found = false;
                        let mut matched_type: Option<Type> = None;
                        let mut arg_types: Vec<Type> = Vec::new();
                        for (_, custom_type) in &self.custom_types {
                            if let Type::Sum { name: _sum_name, variants } = custom_type {
                                if let Some(variant) = variants.iter().find(|v| v.name == *name) {
                                    found = true;
                                    matched_type = Some(custom_type.clone());
                                    arg_types = variant.data_types.clone();
                                    break;
                                }
                            }
                        }
                        if found {
                            if let Some(ct) = matched_type {
                                self.add_constraint_with_context(expected_type.clone(), ct, *span, "match 分支类型与被匹配值不匹配",
                                );
                            }
                            for (i, arg_pattern) in args.iter().enumerate() {
                                if let Some(param_type) = arg_types.get(i) {
                                    self.check_pattern(arg_pattern, param_type, env);
                                }
                            }
                        } else {
                            self.add_error(TypeCheckError::InvalidPattern {
                                message: format!("Unknown constructor: {}", name),
                                span: *span,
                            });
                        }
                    }
                }
            }

            crate::ast::Pattern::QualifiedConstructor {
                type_name,
                constructor_name,
                args,
                span,
            } => {
                // 检查类型是否存在：先查 custom_types，再查 generic_type_defs
                let sum_type = if let Some(sum_type) = self.custom_types.get(type_name).cloned() {
                    Some(sum_type)
                } else if self.generic_type_defs.contains_key(type_name) {
                    let param_count = self.generic_type_defs.get(type_name).unwrap().type_params.len();
                    let fresh_args: Vec<Type> = (0..param_count)
                        .map(|_| Type::Var(self.fresh_type_var()))
                        .collect();
                    Some(self.instantiate_generic_type(type_name, &fresh_args))
                } else {
                    None
                };

                if let Some(sum_type) = sum_type {
                    if let Type::Sum { name: _, variants } = &sum_type {
                        // 查找对应的构造器
                        if let Some(variant) = variants.iter().find(|v| v.name == *constructor_name)
                        {
                            // 约束expected_type必须是这个sum type
                            self.add_constraint_with_context(expected_type.clone(), sum_type.clone(), *span, "条件表达式类型不匹配",
                            );

                            // 检查参数数量
                            if args.len() != variant.data_types.len() {
                                self.add_error(TypeCheckError::InvalidPattern {
                                    message: format!(
                                        "{}::{} expects {} argument(s), but got {}",
                                        type_name,
                                        constructor_name,
                                        variant.data_types.len(),
                                        args.len()
                                    ),
                                    span: *span,
                                });
                            } else {
                                for (i, arg_pattern) in args.iter().enumerate() {
                                    if let Some(param_type) = variant.data_types.get(i) {
                                        self.check_pattern(arg_pattern, param_type, env);
                                    }
                                }
                            }
                        } else {
                            self.add_error(TypeCheckError::InvalidPattern {
                                message: format!(
                                    "Constructor {}::{} not found",
                                    type_name, constructor_name
                                ),
                                span: *span,
                            });
                        }
                    } else {
                        self.add_error(TypeCheckError::InvalidPattern {
                            message: format!("{} is not a sum type", type_name),
                            span: *span,
                        });
                    }
                } else {
                    self.add_error(TypeCheckError::InvalidPattern {
                        message: format!("Type {} not found", type_name),
                        span: *span,
                    });
                }
            }
        crate::ast::Pattern::Struct { name, fields, span } => {
            // 结构体解构模式类型检查
            let struct_type = self.custom_types.get(name).cloned();
            match struct_type {
                Some(Type::Struct { name: _, fields: struct_fields }) => {
                    // 约束 expected_type 必须是此结构体类型
                    let full_type = Type::Struct { name: name.clone(), fields: struct_fields.clone() };
                    self.add_constraint_with_context(expected_type.clone(), full_type, *span, "match 分支类型与被匹配值不匹配",
                    );
                    // 对每个字段模式进行类型检查
                    for field_pattern in fields {
                        if let Some(field_def) = struct_fields.iter().find(|f| f.name == field_pattern.field) {
                            let field_type = field_def.field_type.clone();
                            self.check_pattern(
                                field_pattern.pattern.as_ref(),
                                &field_type,
                                env,
                            );
                        } else {
                            self.add_error(TypeCheckError::InvalidPattern {
                                message: format!("Struct {} has no field {}", name, field_pattern.field),
                                span: field_pattern.span,
                            });
                        }
                    }
                }
                Some(_) => {
                    self.add_error(TypeCheckError::InvalidPattern {
                        message: format!("{} is not a struct type", name),
                        span: *span,
                    });
                }
                None => {
                    self.add_error(TypeCheckError::InvalidPattern {
                        message: format!("Unknown struct type: {}", name),
                        span: *span,
                    });
                }
            }
        }
        }
    }

    /// 解决所有约束条件
    fn solve_constraints(&mut self) {
        let constraints = std::mem::take(&mut self.constraints);
        for constraint in constraints {
            let result = self.unify(
                &constraint.left,
                &constraint.right,
                constraint.span,
                &constraint.left,
                &constraint.right,
            );
            // 如果 unify 失败且有上下文信息，在最近的错误消息中附加上下文
            if result.is_err() {
                if let Some(ctx) = constraint.context {
                    // 找到最后一个 TypeMismatch 错误并附加上下文
                    for diag in self.diagnostics.diagnostics.iter_mut().rev() {
                        if diag.message.contains("类型不匹配") || diag.message.contains("Type mismatch") {
                            diag.message = format!("{} ({})", diag.message, ctx);
                            break;
                        }
                    }
                }
            }
        }
    }

    /// 应用统一化结果到类型上
    /// 使用 visited 集合检测类型变量循环引用，防止无限递归
    /// 应用统一化结果到类型上
    /// 使用循环展开 Type::Var 链，并用 visited 集合检测循环引用
    fn apply_substitution(&mut self, mut ty: Type) -> Type {
        // 循环展开 Type::Var 链，检测循环引用
        let mut visited = std::collections::HashSet::new();
        loop {
            match ty {
                Type::Var(var) => {
                    // 检测循环引用：如果已经访问过此类型变量，保持为 Var
                    if !visited.insert(var) {
                        return Type::Var(var);
                    }
                    let value = self.unification_table.probe_value(var);
                    match value.0 {
                        Some(resolved_type) => ty = resolved_type,
                        None => return Type::Var(var),
                    }
                }
                _ => break,
            }
        }
        // ty 不再是 Type::Var，处理复杂类型
        match ty {
            Type::Function {
                params,
                return_type,
            } => Type::Function {
                params: params
                    .into_iter()
                    .map(|p| self.apply_substitution(p))
                    .collect(),
                return_type: Box::new(self.apply_substitution(*return_type)),
            },
            Type::Closure {
                params,
                return_type,
            } => Type::Closure {
                params: params
                    .into_iter()
                    .map(|p| self.apply_substitution(p))
                    .collect(),
                return_type: Box::new(self.apply_substitution(*return_type)),
            },
            Type::Sum { name, variants } => Type::Sum {
                name,
                variants: variants
                    .into_iter()
                    .map(|v| crate::types::SumVariant {
                        name: v.name,
                        data_types: v.data_types.into_iter().map(|t| self.apply_substitution(t)).collect(),
                    })
                    .collect(),
            },
            Type::Array { element } => Type::Array {
                element: Box::new(self.apply_substitution(*element)),
            },
            Type::Generic { name, args } => Type::Generic {
                name,
                args: args.into_iter().map(|t| self.apply_substitution(t)).collect(),
            },
            _ => ty,
        }
    }

    /// 解析字段类型字符串，支持引用类型、结构体名称引用和泛型类型
    fn parse_field_type(&mut self, type_str: &str) -> Type {
        self.parse_generic_type(type_str)
    }

    /// 解析类型标注，严格模式下遇到未定义类型会报错
    ///
    /// # 参数
    /// - `type_str`: 类型字符串
    /// - `span`: 源代码位置，用于报错
    /// - `strict`: 是否启用严格模式。严格模式下遇到未定义类型会返回 Err
    ///
    /// # 返回
    /// - Ok(Type): 解析成功的类型
    /// - Err(TypeCheckError): 严格模式下遇到未定义类型
    fn parse_type_annotation(
        &mut self,
        type_str: &str,
        span: karte_diagnostics::Span,
        strict: bool,
    ) -> Result<Type, TypeCheckError> {
        self.parse_generic_type_strict(type_str, span, strict)
    }

    /// 严格模式的泛型类型解析
    fn parse_generic_type_strict(
        &mut self,
        type_str: &str,
        span: karte_diagnostics::Span,
        strict: bool,
    ) -> Result<Type, TypeCheckError> {
        // 检查是否是引用类型
        if let Some(inner_type_str) = type_str.strip_prefix('&') {
            let inner_type = self.parse_generic_type_strict(inner_type_str, span, strict)?;
            return Ok(Type::reference(inner_type));
        }

        // 检查是否是泛型类型
        if let Some(open_bracket) = type_str.find('<') {
            if let Some(close_bracket) = type_str.rfind('>') {
                let base_type = &type_str[..open_bracket];
                let args_str = &type_str[open_bracket + 1..close_bracket];

                // 解析泛型参数
                let generic_args = self.parse_generic_args_strict(args_str, span, strict)?;

                match base_type {
                    "Option" => {
                        if generic_args.len() == 1 {
                            return Ok(Type::option(generic_args[0].clone()));
                        } else {
                            return Ok(Type::Unknown); // 错误的参数数量
                        }
                    }
                    "Result" => {
                        if generic_args.len() == 2 {
                            return Ok(Type::result(generic_args[0].clone(), generic_args[1].clone()));
                        } else {
                            return Ok(Type::Unknown); // 错误的参数数量
                        }
                    }
                    _ => {
                        // 其他泛型类型可以在将来支持
                        return Ok(Type::Unknown);
                    }
                }
            } else {
                return Ok(Type::Unknown); // 没有匹配的 >
            }
        } else {
            // 非泛型类型
            match type_str {
                "number" => return Ok(Type::Number),
                "i32" => return Ok(Type::Int(IntKind::I32)),
                "i64" => return Ok(Type::Int(IntKind::I64)),
                "bool" => return Ok(Type::Bool),
                "i8" => return Ok(Type::Int(IntKind::I8)),
                "i16" => return Ok(Type::Int(IntKind::I16)),
                "u8" => return Ok(Type::Int(IntKind::U8)),
                "u16" => return Ok(Type::Int(IntKind::U16)),
                "u32" => return Ok(Type::Int(IntKind::U32)),
                "u64" => return Ok(Type::Int(IntKind::U64)),
                "usize" => return Ok(Type::Int(IntKind::USize)),
                "unit" => return Ok(Type::Unit),
                _ => {
                    // 检查是否是已定义的类型
                    if let Some(custom_type) = self.custom_types.get(type_str) {
                        return Ok(custom_type.clone());
                    } else {
                        // 未定义的类型
                        if strict {
                            return Err(TypeCheckError::UndefinedType {
                                name: type_str.to_string(),
                                span,
                            });
                        } else {
                            // 非严格模式：创建类型变量作为占位符
                            return Ok(Type::Var(self.fresh_type_var()));
                        }
                    }
                }
            }
        }
    }

    /// 严格模式的泛型参数解析
    fn parse_generic_args_strict(
        &mut self,
        args_str: &str,
        span: karte_diagnostics::Span,
        strict: bool,
    ) -> Result<Vec<Type>, TypeCheckError> {
        if args_str.trim().is_empty() {
            return Ok(vec![]);
        }

        // 简单的逗号分隔解析（不处理嵌套的泛型）
        let parts: Vec<&str> = args_str.split(',').collect();
        let mut types = Vec::new();
        for part in parts {
            let ty = self.parse_generic_type_strict(part.trim(), span, strict)?;
            types.push(ty);
        }
        Ok(types)
    }

    /// 解析泛型类型字符串，例如 "Option<&Node>" 或 "&Option<number>"
    fn parse_generic_type(&mut self, type_str: &str) -> Type {
        // 检查是否是引用类型
        if let Some(inner_type_str) = type_str.strip_prefix('&') {
            let inner_type = self.parse_generic_type(inner_type_str);
            return Type::reference(inner_type);
        }

        // 检查是否是泛型类型
        if let Some(open_bracket) = type_str.find('<') {
            if let Some(close_bracket) = type_str.rfind('>') {
                let base_type = &type_str[..open_bracket];
                let args_str = &type_str[open_bracket + 1..close_bracket];

                // 解析泛型参数
                let generic_args = self.parse_generic_args(args_str);

                match base_type {
                    "Option" => {
                        if generic_args.len() == 1 {
                            Type::option(generic_args[0].clone())
                        } else {
                            Type::Unknown // 错误的参数数量
                        }
                    }
                    "Result" => {
                        if generic_args.len() == 2 {
                            Type::result(generic_args[0].clone(), generic_args[1].clone())
                        } else {
                            Type::Unknown // 错误的参数数量
                        }
                    }
                    _ => {
                        // 其他泛型类型可以在将来支持
                        Type::Unknown
                    }
                }
            } else {
                Type::Unknown // 没有匹配的 >
            }
        } else {
            // 非泛型类型
            match type_str {
                "number" => Type::Number,
                "i32" => Type::Int(IntKind::I32),
                "i64" => Type::Int(IntKind::I64),
                "bool" => Type::Bool,
                "i8" => Type::Int(IntKind::I8),
                "i16" => Type::Int(IntKind::I16),
                "u8" => Type::Int(IntKind::U8),
                "u16" => Type::Int(IntKind::U16),
                "u32" => Type::Int(IntKind::U32),
                "u64" => Type::Int(IntKind::U64),
                "usize" => Type::Int(IntKind::USize),
                "unit" => Type::Unit,
                _ => {
                    // 检查是否是已定义的类型
                    if let Some(custom_type) = self.custom_types.get(type_str) {
                        custom_type.clone()
                    } else {
                        // 对于未知类型名称，创建一个类型变量作为占位符
                        // 这允许前向引用和相互引用
                        Type::Var(self.fresh_type_var())
                    }
                }
            }
        }
    }

    /// 解析泛型参数列表，例如 "&Node" 或 "number, string"
    fn parse_generic_args(&mut self, args_str: &str) -> Vec<Type> {
        if args_str.trim().is_empty() {
            return vec![];
        }

        // 简单的参数分割（不处理嵌套的 <> ）
        let args: Vec<&str> = args_str.split(',').map(|s| s.trim()).collect();
        let mut result = Vec::new();
        for arg in args {
            result.push(self.parse_generic_type(arg));
        }
        result
    }

    /// 解析 parser 生成的结构化类型中的自定义类型引用
    /// 解析类型注解中的骨架占位符，替换为完整类型
    /// 需要 &mut self 以便在遇到泛型类型时创建 fresh type variable
    fn resolve_struct_field_from_parsed(&mut self, ty: &Type) -> Type {
        match ty {
            Type::Struct { name, fields } if fields.is_empty() => {
                // Parser 创建的 Struct 骨架占位符
                // 1. 先查找 custom_types（非泛型类型）
                if let Some(struct_type) = self.custom_types.get(name) {
                    return struct_type.clone();
                }
                // 2. 查找泛型类型定义，用 fresh type variable 实例化类型参数
                let param_count = self.generic_type_defs.get(name)
                    .map(|gen_def| gen_def.type_params.len());
                if let Some(count) = param_count {
                    let fresh_args: Vec<Type> = (0..count)
                        .map(|_| Type::Var(self.fresh_type_var()))
                        .collect();
                    return self.instantiate_generic_type(name, &fresh_args);
                }
                // 3. 内置泛型类型
                match name.as_str() {
                    "Option" => return Type::option(Type::Var(self.fresh_type_var())),
                    "Result" => return Type::result(Type::Var(self.fresh_type_var()), Type::Var(self.fresh_type_var())),
                    _ => {}
                }
                ty.clone()
            }
            Type::Reference { inner } => {
                let resolved_inner = self.resolve_struct_field_from_parsed(inner);
                Type::reference(resolved_inner)
            }
            Type::Array { element } => {
                let resolved_element = self.resolve_struct_field_from_parsed(element);
                Type::array(resolved_element)
            }
            // Sum 类型的内部参数也需要递归解析
            Type::Sum { name: sum_name, variants } => {
                let mut resolved_variants = Vec::new();
                for v in variants {
                    let mut resolved_data = Vec::new();
                    for dt in &v.data_types {
                        resolved_data.push(self.resolve_struct_field_from_parsed(dt));
                    }
                    resolved_variants.push(crate::types::SumVariant {
                        name: v.name.clone(),
                        data_types: resolved_data,
                    });
                }
                Type::sum(sum_name.clone(), resolved_variants)
            }
            Type::Generic { name, args } => {
                // 实例化泛型类型
                self.instantiate_generic_type(name, args)
            }
            // 🔧 修复：递归解析元组类型中的元素类型（如 (Point, Point) 中的 Point 骨架）
            Type::Tuple(types) => {
                let resolved: Vec<Type> = types
                    .iter()
                    .map(|t| self.resolve_struct_field_from_parsed(t))
                    .collect();
                Type::tuple(resolved)
            }
            _ => ty.clone(),
        }
    }

    /// 实例化泛型类型：将泛型模板中的类型参数替换为具体参数
    fn instantiate_generic_type(&mut self, name: &str, args: &[Type]) -> Type {
        // 内置泛型类型
        match name {
            "Option" => {
                if args.len() == 1 {
                    return Type::option(args[0].clone());
                }
                return Type::Unknown;
            }
            "Result" => {
                if args.len() == 2 {
                    return Type::result(args[0].clone(), args[1].clone());
                }
                return Type::Unknown;
            }
            _ => {}
        }

        // 用户自定义泛型类型
        // 先克隆定义数据，避免借用冲突
        let gen_def_clone = self.generic_type_defs.get(name).cloned();
        if let Some(gen_def) = gen_def_clone {
            if gen_def.type_params.len() != args.len() {
                return Type::Unknown;
            }

            // 解析泛型参数中的类型
            let mut resolved_args = Vec::new();
            for t in args {
                resolved_args.push(self.resolve_struct_field_from_parsed(t));
            }

            // 构建替换映射：type_param -> 具体类型（HashMap O(1) 查找）
            let subst: std::collections::HashMap<String, Type> = gen_def
                .type_params
                .iter()
                .zip(resolved_args.iter())
                .map(|(param, arg)| (param.clone(), arg.clone()))
                .collect();

            // 在模板中替换类型参数为具体类型，保持 base name
            // 这样在 unify 时同名类型可以直接统一字段
            let instantiated = self.substitute_type_params(&gen_def.template, &subst);

            // 检查是否所有参数都是具体类型（非 Type::Var）
            let all_concrete = resolved_args.iter().all(|t| !matches!(t, Type::Var(_)));

            // 当所有参数都是具体类型时，用单态化名称缓存到 custom_types
            // 但 Type 本身保持 base name
            if all_concrete {
                let mono_name = format!("{}<{}>", name, resolved_args.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(", "));
                if !self.custom_types.contains_key(&mono_name) {
                    self.custom_types.insert(mono_name, instantiated.clone());
                }
            }

            instantiated
        } else {
            Type::Unknown
        }
    }

    /// 在类型模板中替换类型参数为具体类型
    /// 类型参数在模板中以 Type::Struct { name: param_name, fields: [] } 骨架形式存在
    fn substitute_type_params(&mut self, ty: &Type, subst: &std::collections::HashMap<String, Type>) -> Type {
        match ty {
            Type::Struct { name, fields } if fields.is_empty() => {
                // 检查是否是类型参数（HashMap O(1) 查找）
                if let Some(concrete_type) = subst.get(name) {
                    return concrete_type.clone();
                }
                // 不是类型参数，是实际的自定义类型引用
                if let Some(resolved) = self.custom_types.get(name) {
                    resolved.clone()
                } else {
                    ty.clone()
                }
            }
            Type::Struct { name, fields } => {
                let resolved_fields = fields
                    .iter()
                    .map(|f| crate::types::StructField {
                        name: f.name.clone(),
                        field_type: self.substitute_type_params(&f.field_type, subst),
                    })
                    .collect();
                Type::struct_type(name.clone(), resolved_fields)
            }
            Type::Sum { name, variants } => {
                let resolved_variants = variants
                    .iter()
                    .map(|v| crate::types::SumVariant {
                        name: v.name.clone(),
                        data_types: v
                            .data_types
                            .iter()
                            .map(|dt| self.substitute_type_params(dt, subst))
                            .collect(),
                    })
                    .collect();
                Type::sum(name.clone(), resolved_variants)
            }
            Type::Reference { inner } => {
                Type::reference(self.substitute_type_params(inner, subst))
            }
            Type::Array { element } => {
                Type::array(self.substitute_type_params(element, subst))
            }
            Type::Tuple(types) => {
                Type::tuple(types.iter().map(|t| self.substitute_type_params(t, subst)).collect())
            }
            Type::Function { params, return_type } => Type::Function {
                params: params.iter().map(|p| self.substitute_type_params(p, subst)).collect(),
                return_type: Box::new(self.substitute_type_params(return_type, subst)),
            },
            Type::Closure { params, return_type } => Type::Closure {
                params: params.iter().map(|p| self.substitute_type_params(p, subst)).collect(),
                return_type: Box::new(self.substitute_type_params(return_type, subst)),
            },
            Type::Generic { name, args } => {
                // 递归替换泛型参数中的类型变量，然后实例化泛型类型
                let resolved_args: Vec<Type> = args.iter()
                    .map(|a| self.substitute_type_params(a, subst))
                    .collect();
                self.instantiate_generic_type(name, &resolved_args)
            }
            _ => ty.clone(),
        }
    }

    /// 重命名类型（用于单态化）
    /// 从字段值推断泛型类型参数
    /// template_field_type 是模板中的字段类型（可能包含类型参数骨架）
    /// value_type 是实际的字段值类型
    /// type_params 是泛型参数名列表
    /// inferred_args 是推断出的参数映射
    fn infer_type_params_from_value(
        &self,
        template_field_type: &Type,
        value_type: &Type,
        type_params: &[String],
        inferred_args: &mut HashMap<String, Type>,
    ) {
        match template_field_type {
            Type::Struct { name, fields } if fields.is_empty() => {
                // 可能是类型参数骨架
                if type_params.contains(name) && !inferred_args.contains_key(name) {
                    inferred_args.insert(name.clone(), value_type.clone());
                }
            }
            _ => {}
        }
    }

    /// 解析 parser 生成的结构化类型，将 Struct 骨架等占位替换为实际自定义类型
    /// 在 process_struct_definitions 阶段使用，此时 custom_types 可能只有骨架
    fn resolve_parsed_type(
        &mut self,
        ty: &Type,
        struct_defs: &HashMap<String, Vec<FieldDef>>,
    ) -> Type {
        match ty {
            Type::Unknown => Type::Unknown,
            Type::Struct { name, fields } if fields.is_empty() => {
                // Parser 创建的 Struct 骨架占位符，检查是否是已定义的结构体
                if let Some(struct_type) = self.custom_types.get(name) {
                    struct_type.clone()
                } else {
                    // 未知类型名，保留骨架
                    ty.clone()
                }
            }
            Type::Reference { inner } => {
                let resolved_inner = self.resolve_parsed_type(inner, struct_defs);
                Type::reference(resolved_inner)
            }
            Type::Array { element } => {
                let resolved_element = self.resolve_parsed_type(element, struct_defs);
                Type::array(resolved_element)
            }
            // Sum 类型的内部参数也需要递归解析
            Type::Sum { name: sum_name, variants } => {
                let mut resolved_variants = Vec::new();
                for v in variants {
                    let mut resolved_data = Vec::new();
                    for dt in &v.data_types {
                        resolved_data.push(self.resolve_parsed_type(dt, struct_defs));
                    }
                    resolved_variants.push(crate::types::SumVariant {
                        name: v.name.clone(),
                        data_types: resolved_data,
                    });
                }
                Type::sum(sum_name.clone(), resolved_variants)
            }
            Type::Generic { name, args } => {
                // 实例化泛型类型
                let mut resolved_args = Vec::new();
                for t in args {
                    resolved_args.push(self.resolve_parsed_type(t, struct_defs));
                }
                self.instantiate_generic_type(name, &resolved_args)
            }
            Type::Struct { name, fields } => {
                // 完整的 struct 类型，递归解析字段
                let mut resolved_fields = Vec::new();
                for f in fields {
                    let field_type = self.resolve_parsed_type(&f.field_type, struct_defs);
                    resolved_fields.push(crate::types::StructField {
                        name: f.name.clone(),
                        field_type,
                    });
                }
                Type::struct_type(name.clone(), resolved_fields)
            }
            Type::Tuple(types) => {
                let mut resolved = Vec::new();
                for t in types {
                    resolved.push(self.resolve_parsed_type(t, struct_defs));
                }
                Type::tuple(resolved)
            }
            _ => ty.clone(),
        }
    }

    /// 解析结构体字段类型，支持延迟解析、循环检测和泛型类型
    fn resolve_struct_field_type(
        &self,
        type_str: &str,
        struct_defs: &HashMap<String, Vec<FieldDef>>,
    ) -> Type {
        self.resolve_generic_type(type_str, struct_defs)
    }

    /// 解析泛型类型字符串，在结构体定义解析阶段使用
    fn resolve_generic_type(
        &self,
        type_str: &str,
        struct_defs: &HashMap<String, Vec<FieldDef>>,
    ) -> Type {
        // 检查是否是引用类型
        if let Some(inner_type_str) = type_str.strip_prefix('&') {
            let inner_type = self.resolve_generic_type(inner_type_str, struct_defs);
            return Type::reference(inner_type);
        }

        // 检查是否是泛型类型
        if let Some(open_bracket) = type_str.find('<') {
            if let Some(close_bracket) = type_str.rfind('>') {
                let base_type = &type_str[..open_bracket];
                let args_str = &type_str[open_bracket + 1..close_bracket];

                // 解析泛型参数
                let generic_args = self.resolve_generic_args(args_str, struct_defs);

                match base_type {
                    "Option" => {
                        if generic_args.len() == 1 {
                            Type::option(generic_args[0].clone())
                        } else {
                            Type::Unknown // 错误的参数数量
                        }
                    }
                    "Result" => {
                        if generic_args.len() == 2 {
                            Type::result(generic_args[0].clone(), generic_args[1].clone())
                        } else {
                            Type::Unknown // 错误的参数数量
                        }
                    }
                    _ => {
                        // 其他泛型类型可以在将来支持
                        Type::Unknown
                    }
                }
            } else {
                Type::Unknown // 没有匹配的 >
            }
        } else {
            // 非泛型类型
            match type_str {
                "number" => Type::Number,
                "bool" => Type::Bool,
                "i8" => Type::Int(IntKind::I8),
                "i16" => Type::Int(IntKind::I16),
                "u8" => Type::Int(IntKind::U8),
                "u16" => Type::Int(IntKind::U16),
                "u32" => Type::Int(IntKind::U32),
                "u64" => Type::Int(IntKind::U64),
                "usize" => Type::Int(IntKind::USize),
                "unit" => Type::Unit,
                _ => {
                    // 检查是否是已定义的结构体类型
                    if struct_defs.contains_key(type_str) {
                        // 获取已经创建的结构体类型骨架
                        if let Some(struct_type) = self.custom_types.get(type_str) {
                            struct_type.clone()
                        } else {
                            // 如果还没有创建骨架，创建一个临时的
                            Type::struct_type(type_str.to_string(), vec![])
                        }
                    } else if let Some(custom_type) = self.custom_types.get(type_str) {
                        custom_type.clone()
                    } else {
                        Type::Unknown
                    }
                }
            }
        }
    }

    /// 解析泛型参数列表，在结构体定义解析阶段使用
    fn resolve_generic_args(
        &self,
        args_str: &str,
        struct_defs: &HashMap<String, Vec<FieldDef>>,
    ) -> Vec<Type> {
        if args_str.trim().is_empty() {
            return vec![];
        }

        // 简单的参数分割（不处理嵌套的 <> ）
        let args: Vec<&str> = args_str.split(',').map(|s| s.trim()).collect();
        args.into_iter()
            .map(|arg| self.resolve_generic_type(arg, struct_defs))
            .collect()
    }

    /// 添加类型检查错误
    fn add_error(&mut self, error: TypeCheckError) {
        let message = error.to_string();
        let span = error.span();
        self.diagnostics.add_error(message, span);
    }

    /// 添加编译器警告
    fn add_warning(&mut self, message: impl Into<String>, span: karte_diagnostics::Span) {
        self.diagnostics.add_warning(message, span);
    }

    /// 获取诊断信息
    pub fn diagnostics(&self) -> &DiagnosticBag {
        &self.diagnostics
    }

    /// 消费并返回诊断信息
    pub fn into_diagnostics(self) -> DiagnosticBag {
        self.diagnostics
    }
}

/// 便捷函数：对表达式进行类型检查
pub fn type_check(expr: &Expr) -> (Type, DiagnosticBag) {
    type_check_with_context(expr, ModuleContext::default())
}

pub fn type_check_with_context(expr: &Expr, context: ModuleContext) -> (Type, DiagnosticBag) {
    let mut checker = TypeChecker::new();
    let result_type = checker.check_program_with_context(expr, &context);
    (result_type, checker.into_diagnostics())
}

/// 带lambda_types的类型检查
pub fn type_check_with_context_and_maps(
    expr: &Expr,
    context: ModuleContext,
) -> (Type, HashMap<usize, Type>, DiagnosticBag) {
    let mut checker = TypeChecker::new();
    let result_type = checker.check_program_with_context(expr, &context);
    let expr_types = checker.get_lambda_types(); // 方法名保持不变以保持兼容性
    (result_type, expr_types, checker.into_diagnostics())
}

/// LSP 专用类型检查，返回完整的分析结果
pub fn type_check_for_lsp(expr: &Expr) -> LspTypeInfo {
    let mut checker = TypeChecker::new();
    let context = ModuleContext::default();
    let result_type = checker.check_program_with_context(expr, &context);
    let expr_types = checker.get_lambda_types();
    let function_signatures = checker.function_signatures.clone();
    let function_schemes = checker.function_schemes.clone();
    let diagnostics = checker.into_diagnostics();

    LspTypeInfo {
        result_type,
        expr_types,
        function_signatures,
        function_schemes,
        diagnostics,
    }
}

/// LSP 类型信息结果
#[derive(Debug)]
pub struct LspTypeInfo {
    pub result_type: Type,
    pub expr_types: HashMap<usize, Type>,
    /// 函数签名信息（函数名 -> 签名）
    pub function_signatures: HashMap<String, FunctionSignature>,
    /// 泛型函数的类型方案
    pub function_schemes: HashMap<String, TypeScheme>,
    pub diagnostics: DiagnosticBag,
}

#[cfg(test)]
mod assignment_type_check_tests {
    use super::*;
    use crate::ast::*;
    use karte_diagnostics::Span;

    fn make_span() -> Span {
        Span::new(0, 0)
    }

    #[test]
    fn test_variable_assignment_type_check() {
        let mut checker = TypeChecker::new();
        let mut env = TypeEnvironment::new();

        // let x = 5; x = 10;
        let assignment = Expr::Assignment {
            target: Box::new(Expr::Identifier {
                name: "x".to_string(),
                span: make_span(),
            }),
            value: Box::new(Expr::Number {
                value: 10,
                span: make_span(),
            }),
            span: make_span(),
        };

        // 先添加x到环境中
        env.insert("x".to_string(), Type::Number);

        let result_type = checker.infer_expr(&assignment, &env);
        assert_eq!(result_type, Type::Unit, "赋值表达式应该返回Unit类型");
        assert!(checker.diagnostics().is_empty(), "不应该有类型错误");
    }

    #[cfg(test)]
    mod module_context_type_tests {
        use super::*;

        #[test]
        fn dependency_interfaces_seed_function_types() {
            let mut context = ModuleContext::default();
            context.add_import_symbol(vec!["utils".into()], "add".into(), "add".into());

            let mut iface = ExternalModuleInterface::default();
            iface.functions.insert(
                "add".into(),
                ExternalFunctionSignature {
                    name: "add".into(),
                    params: 2,
                },
            );
            let mut interfaces = HashMap::new();
            interfaces.insert("utils".into(), iface);
            context.set_dependency_interfaces(interfaces);

            let mut checker = TypeChecker::new();
            let mut env = TypeEnvironment::new();
            checker.apply_module_context(&context, &mut env);

            match env.get("add") {
                Some(Type::Function { params, .. }) => assert_eq!(params.len(), 2),
                other => panic!(
                    "expected function type seeded from interface, got {:?}",
                    other
                ),
            }
        }

        #[test]
        fn unknown_dependencies_stay_unbound() {
            let mut context = ModuleContext::default();
            context.add_import_symbol(vec!["utils".into()], "missing".into(), "missing".into());

            let mut checker = TypeChecker::new();
            let mut env = TypeEnvironment::new();
            checker.apply_module_context(&context, &mut env);

            match env.get("missing") {
                Some(Type::Var(_)) => {} // fallback to fresh type variable
                other => panic!(
                    "expected fresh type variable for missing symbol, got {:?}",
                    other
                ),
            }
        }

        #[test]
        fn module_symbol_alias_resolves_function_type() {
            let mut context = ModuleContext::default();
            context.add_import_symbol(
                vec!["std".into(), "array".into()],
                "*".into(),
                "array".into(),
            );

            let mut iface = ExternalModuleInterface::default();
            iface.functions.insert(
                "len".into(),
                ExternalFunctionSignature {
                    name: "len".into(),
                    params: 1,
                },
            );
            let mut interfaces = HashMap::new();
            interfaces.insert("std.array".into(), iface);
            context.set_dependency_interfaces(interfaces);

            let expr = Expr::ModuleSymbolAccess {
                module_path: vec!["array".into()],
                symbol: "len".into(),
                span: Span::new(0, 0),
            };

            let (ty, diagnostics) = type_check_with_context(&expr, context);
            assert!(
                diagnostics.is_empty(),
                "unexpected diagnostics: {:?}",
                diagnostics
            );

            match ty {
                Type::Function { params, .. } => assert_eq!(params.len(), 1),
                other => panic!("expected function type, got {:?}", other),
            }
        }

        #[test]
        fn module_symbol_without_interface_reports_error() {
            let mut context = ModuleContext::default();
            context.add_import_symbol(
                vec!["std".into(), "array".into()],
                "*".into(),
                "array".into(),
            );

            let expr = Expr::ModuleSymbolAccess {
                module_path: vec!["array".into()],
                symbol: "len".into(),
                span: Span::new(0, 0),
            };

            let (_, diagnostics) = type_check_with_context(&expr, context);
            assert!(diagnostics.has_errors(), "expected availability error");
            assert!(diagnostics
                .diagnostics
                .iter()
                .any(|diag| diag.message.contains("std.array")));
        }

        #[test]
        fn module_symbol_undefined_export_reports_error() {
            let mut context = ModuleContext::default();
            context.add_import_symbol(
                vec!["std".into(), "array".into()],
                "*".into(),
                "array".into(),
            );

            let mut iface = ExternalModuleInterface::default();
            iface.functions.insert(
                "len".into(),
                ExternalFunctionSignature {
                    name: "len".into(),
                    params: 1,
                },
            );
            let mut interfaces = HashMap::new();
            interfaces.insert("std.array".into(), iface);
            context.set_dependency_interfaces(interfaces);

            let expr = Expr::ModuleSymbolAccess {
                module_path: vec!["array".into()],
                symbol: "missing".into(),
                span: Span::new(0, 0),
            };

            let (_, diagnostics) = type_check_with_context(&expr, context);
            assert!(diagnostics.has_errors(), "expected export error");
            assert!(diagnostics
                .diagnostics
                .iter()
                .any(|diag| diag.message.contains("未导出 `missing`")));
        }
    }
    #[test]
    fn test_assignment_to_undefined_variable() {
        let mut checker = TypeChecker::new();
        let env = TypeEnvironment::new();

        // y = 42; (y未定义)
        let assignment = Expr::Assignment {
            target: Box::new(Expr::Identifier {
                name: "y".to_string(),
                span: make_span(),
            }),
            value: Box::new(Expr::Number {
                value: 42,
                span: make_span(),
            }),
            span: make_span(),
        };

        let _result_type = checker.infer_expr(&assignment, &env);
        assert!(!checker.diagnostics().is_empty(), "应该有未定义变量的错误");
    }

    #[test]
    fn test_assignment_type_compatibility() {
        let mut checker = TypeChecker::new();
        let mut env = TypeEnvironment::new();

        // let x = 5; x = true; (类型不兼容)
        env.insert("x".to_string(), Type::Number);

        let field_assignment = Expr::Assignment {
            target: Box::new(Expr::FieldAccess {
                object: Box::new(Expr::Identifier {
                    name: "p".to_string(),
                    span: make_span(),
                }),
                field: "x".to_string(),
                span: make_span(),
            }),
            value: Box::new(Expr::Number {
                value: 42,
                span: make_span(),
            }),
            span: make_span(),
        };

        let result_type = checker.infer_expr(&field_assignment, &env);
        assert_eq!(result_type, Type::Unit, "字段赋值表达式应该返回Unit类型");
    }

    #[test]
    fn test_assignment_statement_type_check() {
        let mut checker = TypeChecker::new();
        let mut env = TypeEnvironment::new();

        env.insert("x".to_string(), Type::Number);

        let assignment_statement = Statement::Assignment {
            target: Expr::Identifier {
                name: "x".to_string(),
                span: make_span(),
            },
            value: Expr::Number {
                value: 100,
                span: make_span(),
            },
            span: make_span(),
        };

        checker.infer_statement(&assignment_statement, &mut env);
        assert!(checker.diagnostics().is_empty(), "赋值语句不应该有类型错误");
    }

    #[test]
    fn test_invalid_assignment_target() {
        let mut checker = TypeChecker::new();
        let env = TypeEnvironment::new();

        // 5 = 10; (数字字面量不能作为赋值目标)
        let invalid_assignment = Expr::Assignment {
            target: Box::new(Expr::Number {
                value: 5,
                span: make_span(),
            }),
            value: Box::new(Expr::Number {
                value: 10,
                span: make_span(),
            }),
            span: make_span(),
        };

        let _result_type = checker.infer_expr(&invalid_assignment, &env);
        assert!(
            !checker.diagnostics().is_empty(),
            "应该有无效赋值目标的错误"
        );
    }
}
