//! Trait 系统的 Monomorphization pass
//!
//! 在类型检查之后、MIR lowering 之前执行。
//! 1. 将 impl 方法注入为普通 FunctionDef
//! 2. 重写 trait 方法调用（eq(1,2) → Eq$number$eq(1,2)）
//! 3. 泛型函数特化（same(1,2) → same$number(1,2)）

use karte_hir::ast::{BinaryOperator, Expr, Parameter, Statement};
use karte_hir::types::{ImplBlock, Type};
use karte_diagnostics::Span;
use std::collections::HashMap;

/// 操作符 → trait 方法名映射
/// 用户定义 trait 时，方法名必须匹配这些约定名
fn operator_method_name(op: &BinaryOperator) -> Option<&'static str> {
    match op {
        BinaryOperator::Equal => Some("eq"),
        BinaryOperator::NotEqual => Some("ne"),
        BinaryOperator::Less => Some("lt"),
        BinaryOperator::Greater => Some("gt"),
        BinaryOperator::LessEqual => Some("le"),
        BinaryOperator::GreaterEqual => Some("ge"),
        BinaryOperator::Add => Some("add"),
        BinaryOperator::Subtract => Some("sub"),
        BinaryOperator::Multiply => Some("mul"),
        BinaryOperator::Divide => Some("div"),
        BinaryOperator::Modulo => Some("mod"),
        // 逻辑和位运算暂不支持操作符重载
        _ => None,
    }
}

/// 获取类型名用于 mangled name（不依赖 Display trait）
fn type_name_for_mangle(ty: &Type) -> String {
    match ty {
        Type::Number => "number".to_string(),
        Type::Bool => "bool".to_string(),
        Type::String => "string".to_string(),
        Type::Unit => "unit".to_string(),
        Type::Struct { name, .. } => name.clone(),
        Type::SelfType => "Self".to_string(),
        other => format!("{}", other),
    }
}

/// 判断类型是否为原始类型（不需要操作符重载）
fn is_primitive_type(ty: &Type) -> bool {
    match ty {
        Type::Number | Type::Bool | Type::String | Type::Unit => true,
        _ => false,
    }
}

/// 类型匹配：对 Struct 类型只比较 name（因为 parser 创建的空骨架和完整类型字段数不同）
fn type_match(t1: &Type, t2: &Type) -> bool {
    match (t1, t2) {
        (Type::Struct { name: n1, .. }, Type::Struct { name: n2, .. }) => n1 == n2,
        _ => t1.structural_eq(t2),
    }
}

/// 将类型中的 Self 和 trait 泛型参数替换为具体类型
fn substitute_type_full(
    ty: &Type,
    target: &Type,
    type_param_names: &[String],
    trait_args: &[Type],
) -> Type {
    match ty {
        Type::SelfType => target.clone(),
        // 空 Struct 骨架可能是 trait 泛型参数引用
        Type::Struct { name, fields } if fields.is_empty() => {
            if let Some(idx) = type_param_names.iter().position(|n| n == name) {
                if let Some(arg) = trait_args.get(idx) {
                    return arg.clone();
                }
            }
            ty.clone()
        }
        Type::Function { params, return_type } => Type::Function {
            params: params.iter().map(|p| substitute_type_full(p, target, type_param_names, trait_args)).collect(),
            return_type: Box::new(substitute_type_full(return_type, target, type_param_names, trait_args)),
        },
        _ => ty.clone(),
    }
}

/// 将 impl 方法注入为普通 FunctionDef，用 trait 定义填充省略的类型标注
pub fn inject_impl_methods(body: &mut Expr, impl_blocks: &[ImplBlock], trait_defs: &[karte_hir::types::TraitDef]) {
    let mut new_functions = Vec::new();

    for ib in impl_blocks {
        let type_str = type_name_for_mangle(&ib.target_type);
        // 查找对应的 trait 定义
        let trait_def = trait_defs.iter().find(|td| td.name == ib.trait_name);

        // 1. 处理 impl 中显式提供的方法
        for method in &ib.methods {
            let mangled = format!("{}${}${}", ib.trait_name, type_str, method.name);

            // 从 trait 定义获取方法签名，替换 Self → target_type 和泛型参数 → trait_args
            let resolved_params: Vec<(String, Type)> = if let Some(td) = trait_def {
                if let Some(trait_method) = td.methods.iter().find(|m| m.name == method.name) {
                    trait_method.params.iter().map(|(n, t)| {
                        let resolved = substitute_type_full(t, &ib.target_type, &td.type_params, &ib.trait_args);
                        (n.clone(), resolved)
                    }).collect()
                } else {
                    method.params.clone()
                }
            } else {
                method.params.clone()
            };

            let resolved_return: Option<Type> = if let Some(td) = trait_def {
                if let Some(trait_method) = td.methods.iter().find(|m| m.name == method.name) {
                    let resolved = substitute_type_full(&trait_method.return_type, &ib.target_type, &td.type_params, &ib.trait_args);
                    Some(resolved)
                } else {
                    None
                }
            } else {
                None
            };

            let params: Vec<Parameter> = resolved_params.iter().map(|(name, ty)| Parameter {
                name: name.clone(),
                type_annotation: Some(ty.clone()),
                span: karte_diagnostics::Span::dummy(),
            }).collect();

            new_functions.push(Statement::FunctionDef {
                name: mangled,
                params,
                return_type: resolved_return,
                body: method.body.clone(),
                is_pub: false,
                type_constraints: vec![],
                span: karte_diagnostics::Span::dummy(),
            });
        }

        // 2. 处理有默认实现但 impl 中没有提供的方法
        if let Some(td) = trait_def {
            for trait_method in &td.methods {
                // 检查 impl 是否提供了这个方法
                let provided = ib.methods.iter().any(|m| m.name == trait_method.name);
                if !provided {
                    if let Some(default_body) = &trait_method.default_body {
                        let mangled = format!("{}${}${}", ib.trait_name, type_str, trait_method.name);

                        let resolved_params: Vec<(String, Type)> = trait_method.params.iter().map(|(n, t)| {
                            let resolved = substitute_type_full(t, &ib.target_type, &td.type_params, &ib.trait_args);
                            (n.clone(), resolved)
                        }).collect();

                        let resolved_return = substitute_type_full(&trait_method.return_type, &ib.target_type, &td.type_params, &ib.trait_args);

                        let params: Vec<Parameter> = resolved_params.iter().map(|(name, ty)| Parameter {
                            name: name.clone(),
                            type_annotation: Some(ty.clone()),
                            span: karte_diagnostics::Span::dummy(),
                        }).collect();

                        // 默认方法体中可能引用了其他 trait 方法（如 eq），
                        // 这些方法名在 monomorphization 期间会被替换
                        let body = default_body.clone();

                        new_functions.push(Statement::FunctionDef {
                            name: mangled,
                            params,
                            return_type: Some(resolved_return),
                            body,
                            is_pub: false,
                            type_constraints: vec![],
                            span: karte_diagnostics::Span::dummy(),
                        });
                    }
                }
            }
        }
    }

    if let Expr::Block { statements, .. } = body {
        statements.extend(new_functions);
    }
}

/// 构建 trait 方法名 → [(target_type, mangled_name)] 的映射
pub fn build_trait_impl_map(impl_blocks: &[ImplBlock], trait_defs: &[karte_hir::types::TraitDef]) -> HashMap<String, Vec<(Type, String)>> {
    let mut map: HashMap<String, Vec<(Type, String)>> = HashMap::new();
    for ib in impl_blocks {
        let type_str = type_name_for_mangle(&ib.target_type);
        // 1. 从 impl 显式方法构建
        for method in &ib.methods {
            let mangled = format!("{}${}${}", ib.trait_name, type_str, method.name);
            map.entry(method.name.clone())
                .or_default()
                .push((ib.target_type.clone(), mangled));
        }
        // 2. 从 trait 默认方法构建（如果 impl 没有提供）
        if let Some(td) = trait_defs.iter().find(|t| t.name == ib.trait_name) {
            for trait_method in &td.methods {
                let provided = ib.methods.iter().any(|m| m.name == trait_method.name);
                if !provided && trait_method.default_body.is_some() {
                    let mangled = format!("{}${}${}", ib.trait_name, type_str, trait_method.name);
                    map.entry(trait_method.name.clone())
                        .or_default()
                        .push((ib.target_type.clone(), mangled));
                }
            }
        }
    }
    map
}

/// 重写表达式中的 trait 方法调用（基于 expr_types 匹配具体类型）
/// eq(1, 2) → Eq$number$eq(1, 2)
pub fn rewrite_trait_calls(
    expr: &mut Expr,
    impl_map: &HashMap<String, Vec<(Type, String)>>,
    expr_types: &HashMap<usize, Type>,
) {
    match expr {
        Expr::Block { statements, final_expr, .. } => {
            for stmt in statements.iter_mut() {
                rewrite_stmt_trait(stmt, impl_map, expr_types);
            }
            if let Some(fe) = final_expr {
                rewrite_trait_calls(fe, impl_map, expr_types);
            }
        }
        Expr::FunctionCall { function, args, .. } => {
            rewrite_trait_calls(function, impl_map, expr_types);
            for arg in args.iter_mut() {
                rewrite_trait_calls(arg, impl_map, expr_types);
            }
            // 检查是否是 trait 方法调用
            if let Expr::Identifier { name, .. } = function.as_ref() {
                if let Some(impls) = impl_map.get(name) {
                    let mut resolved_name: Option<String> = None;

                    // 策略1: 用第一个参数匹配 target_type（Self 参数）
                    if let Some(first_arg) = args.first() {
                        let arg_ptr = first_arg as *const Expr as usize;
                        if let Some(arg_type) = expr_types.get(&arg_ptr) {
                            for (target_type, mangled_name) in impls {
                                if type_match(target_type, arg_type) {
                                    resolved_name = Some(mangled_name.clone());
                                    break;
                                }
                            }
                        }
                    }

                    // 策略2: 如果只有一个 impl，直接使用
                    if resolved_name.is_none() && impls.len() == 1 {
                        resolved_name = Some(impls[0].1.clone());
                    }

                    // 策略3: 尝试所有参数匹配 impl 参数
                    if resolved_name.is_none() {
                        for (_target_type, mangled_name) in impls {
                            for arg in args.iter() {
                                let arg_ptr = arg as *const Expr as usize;
                                if let Some(arg_type) = expr_types.get(&arg_ptr) {
                                    for (target_type, _) in impls {
                                        if type_match(target_type, arg_type) {
                                            resolved_name = Some(mangled_name.clone());
                                            break;
                                        }
                                    }
                                    if resolved_name.is_some() { break; }
                                }
                            }
                            if resolved_name.is_some() { break; }
                        }
                    }

                    if let Some(mangled) = resolved_name {
                        if let Expr::Identifier { name: ref mut n, .. } = function.as_mut() {
                            *n = mangled;
                        }
                    }
                }
            }
        }
        Expr::If { condition, then_branch, else_branch, .. } => {
            rewrite_trait_calls(condition, impl_map, expr_types);
            rewrite_trait_calls(then_branch, impl_map, expr_types);
            if let Some(eb) = else_branch {
                rewrite_trait_calls(eb, impl_map, expr_types);
            }
        }
        Expr::While { condition, body, .. } => {
            rewrite_trait_calls(condition, impl_map, expr_types);
            rewrite_trait_calls(body, impl_map, expr_types);
        }
        Expr::TraitVtable { trait_name, target_type, span } => {
            // 将 vtable 构造替换为具体 impl 方法的引用
            let type_name = type_name_for_mangle(target_type);
            // 查找所有匹配的 impl 方法
            let mut methods = Vec::new();
            for (method_name, impls) in impl_map.iter() {
                for (impl_target, mangled_name) in impls {
                    if type_match(impl_target, target_type) {
                        // 检查这个方法是否属于当前 trait
                        if mangled_name.starts_with(&format!("{}${}", trait_name, type_name)) {
                            methods.push((method_name.clone(), mangled_name.clone()));
                        }
                    }
                }
            }
            if methods.len() == 1 {
                // 单方法 trait: vtable = 函数指针
                let (_, mangled_name) = methods.into_iter().next().unwrap();
                *expr = Expr::Identifier { name: mangled_name, span: *span };
            } else if !methods.is_empty() {
                let (_, mangled_name) = methods.into_iter().next().unwrap();
                *expr = Expr::Identifier { name: mangled_name, span: *span };
            } else {
            }
        }
        Expr::VtableMethodCall { vtable, method: _, args, span } => {
            rewrite_trait_calls(vtable, impl_map, expr_types);
            for arg in args.iter_mut() {
                rewrite_trait_calls(arg, impl_map, expr_types);
            }
            // vtable 已经被重写为具体的函数引用（Identifier）
            // 替换为普通 FunctionCall: func(args)
            let vtable_expr = std::mem::replace(&mut **vtable, Expr::Unit { span: *span });
            let span_val = *span;
            *expr = Expr::FunctionCall {
                function: Box::new(vtable_expr),
                args: std::mem::take(args),
                span: span_val,
            };
        }
        Expr::BinaryOp { left, right, op, span: _ } => {
            // 操作符重载：先检查是否需要替换为 trait 方法调用（在递归之前）
            // 原因：递归会把内层 BinaryOp 替换为 FunctionCall，导致地址变化，
            // 外层的 expr_types 查找会失败
            if let Some(method_name) = operator_method_name(op) {
                if let Some(impls) = impl_map.get(method_name) {
                    let left_ptr = &**left as *const Expr as usize;
                    if let Some(left_type) = expr_types.get(&left_ptr) {
                        if !is_primitive_type(left_type) {
                            for (target_type, mangled_name) in impls {
                                if type_match(target_type, left_type) {
                                    let old = std::mem::replace(expr, Expr::Number { value: 0, span: Span::dummy() });
                                    if let Expr::BinaryOp { left: l, right: r, span: s, .. } = old {
                                        let mut new_left = *l;
                                        let mut new_right = *r;
                                        rewrite_trait_calls(&mut new_left, impl_map, expr_types);
                                        rewrite_trait_calls(&mut new_right, impl_map, expr_types);
                                        *expr = Expr::FunctionCall {
                                            function: Box::new(Expr::Identifier {
                                                name: mangled_name.clone(),
                                                span: s,
                                            }),
                                            args: vec![new_left, new_right],
                                            span: s,
                                        };
                                    }
                                    return;
                                }
                            }
                        }
                    }
                }
            }

            rewrite_trait_calls(left, impl_map, expr_types);
            rewrite_trait_calls(right, impl_map, expr_types);
        }
        Expr::UnaryOp { operand, .. } => {
            rewrite_trait_calls(operand, impl_map, expr_types);
        }
        Expr::Lambda { body, .. } => {
            rewrite_trait_calls(body, impl_map, expr_types);
        }
        Expr::Return { value, .. } => {
            if let Some(v) = value {
                rewrite_trait_calls(v, impl_map, expr_types);
            }
        }
        Expr::Assignment { value, .. } => {
            rewrite_trait_calls(value, impl_map, expr_types);
        }
        Expr::TupleLiteral { elements, .. } | Expr::ArrayLiteral { elements, .. } => {
            for e in elements.iter_mut() {
                rewrite_trait_calls(e, impl_map, expr_types);
            }
        }
        Expr::Index { array, index, .. } => {
            rewrite_trait_calls(array, impl_map, expr_types);
            rewrite_trait_calls(index, impl_map, expr_types);
        }
        Expr::FieldAccess { object, .. } => {
            rewrite_trait_calls(object, impl_map, expr_types);
        }
        Expr::StructLiteral { fields, .. } => {
            for field in fields.iter_mut() {
                rewrite_trait_calls(&mut field.value, impl_map, expr_types);
            }
        }
        Expr::Match { expr: scrutinee, arms, .. } => {
            rewrite_trait_calls(scrutinee, impl_map, expr_types);
            for arm in arms.iter_mut() {
                rewrite_trait_calls(&mut arm.body, impl_map, expr_types);
            }
        }
        Expr::ForIn { start, end, body, .. } => {
            rewrite_trait_calls(start, impl_map, expr_types);
            rewrite_trait_calls(end, impl_map, expr_types);
            rewrite_trait_calls(body, impl_map, expr_types);
        }
        Expr::ForArray { array, body, .. } => {
            rewrite_trait_calls(array, impl_map, expr_types);
            rewrite_trait_calls(body, impl_map, expr_types);
        }
        Expr::Reference { expr: inner, .. } | Expr::Dereference { expr: inner, .. } => {
            rewrite_trait_calls(inner, impl_map, expr_types);
        }
        Expr::Constructor { args, .. } | Expr::QualifiedConstructor { args, .. } => {
            for arg in args.iter_mut() {
                rewrite_trait_calls(arg, impl_map, expr_types);
            }
        }
        Expr::Statement { stmt, .. } => {
            rewrite_stmt_trait(stmt, impl_map, expr_types);
        }
        Expr::Abs { value, .. } | Expr::CharToString { expr: value, .. } | Expr::ToString { expr: value, .. }
        | Expr::ArrayLen { array: value, .. } | Expr::Trim { string: value, .. }
        | Expr::HeapFree { pointer: value, .. } | Expr::Retain { pointer: value, .. }
        | Expr::Release { pointer: value, .. } => {
            rewrite_trait_calls(value, impl_map, expr_types);
        }
        Expr::Min { left, right, .. } | Expr::Max { left, right, .. } => {
            rewrite_trait_calls(left, impl_map, expr_types);
            rewrite_trait_calls(right, impl_map, expr_types);
        }
        Expr::Clamp { value, min_val, max_val, .. } => {
            rewrite_trait_calls(value, impl_map, expr_types);
            rewrite_trait_calls(min_val, impl_map, expr_types);
            rewrite_trait_calls(max_val, impl_map, expr_types);
        }
        Expr::StrIndex { string, index, .. } | Expr::CharAt { string, index, .. } => {
            rewrite_trait_calls(string, impl_map, expr_types);
            rewrite_trait_calls(index, impl_map, expr_types);
        }
        Expr::Substring { string, start, length, .. } => {
            rewrite_trait_calls(string, impl_map, expr_types);
            rewrite_trait_calls(start, impl_map, expr_types);
            rewrite_trait_calls(length, impl_map, expr_types);
        }
        Expr::StrContains { string, char_code, .. } | Expr::SplitCount { string, separator: char_code, .. } => {
            rewrite_trait_calls(string, impl_map, expr_types);
            rewrite_trait_calls(char_code, impl_map, expr_types);
        }
        Expr::HeapAllocate { value, .. } => {
            rewrite_trait_calls(value, impl_map, expr_types);
        }
        Expr::UnsafeStore { addr, value, .. } => {
            rewrite_trait_calls(addr, impl_map, expr_types);
            rewrite_trait_calls(value, impl_map, expr_types);
        }
        Expr::UnsafeLoad { addr, .. } => {
            rewrite_trait_calls(addr, impl_map, expr_types);
        }
        Expr::TupleAccess { object, .. } => {
            rewrite_trait_calls(object, impl_map, expr_types);
        }
        Expr::ModuleSymbolAccess { .. } => {}
        Expr::EffectPerform { tag, payload, .. } => {
            rewrite_trait_calls(tag, impl_map, expr_types);
            rewrite_trait_calls(payload, impl_map, expr_types);
        }
        Expr::EffectResume { value, .. } => {
            rewrite_trait_calls(value, impl_map, expr_types);
        }
        Expr::EffectHandle { handler, body, .. } => {
            rewrite_trait_calls(handler, impl_map, expr_types);
            rewrite_trait_calls(body, impl_map, expr_types);
        }
        Expr::TypeCast { expr: inner, .. } => {
            rewrite_trait_calls(inner, impl_map, expr_types);
        }
        _ => {}
    }
}

fn rewrite_stmt_trait(
    stmt: &mut Statement,
    impl_map: &HashMap<String, Vec<(Type, String)>>,
    expr_types: &HashMap<usize, Type>,
) {
    match stmt {
        Statement::Expression { expr, .. } => {
            rewrite_trait_calls(expr, impl_map, expr_types);
        }
        Statement::Let { value, .. } => {
            rewrite_trait_calls(value, impl_map, expr_types);
        }
        Statement::FunctionDef { body, .. } => {
            rewrite_trait_calls(body, impl_map, expr_types);
        }
        Statement::TypeDef { .. } | Statement::StructDef { .. } => {}
        Statement::Assignment { value, .. } => {
            rewrite_trait_calls(value, impl_map, expr_types);
        }
    }
}

// ==================== Phase 3b: 泛型函数特化 ====================

/// 从 type_constraints 和具体调用类型构建约束名 → 具体类型的映射
/// 例如函数 fn same<T: Eq>(a: T, b: T)，调用 same(1, 2)
/// type_constraints = [("T", "Eq")]
/// concrete_types = [number, number]
/// 结果: {"T" → number}
fn build_constraint_type_map(
    func_name: &str,
    body: &Expr,
    concrete_types: &[Type],
) -> HashMap<String, Type> {
    let mut result = HashMap::new();
    if let Expr::Block { statements, .. } = body {
        for stmt in statements {
            if let Statement::FunctionDef { name, params, type_constraints, .. } = stmt {
                if name == func_name && !type_constraints.is_empty() {
                    // 找到参数的位置，匹配具体类型
                    // type_constraints 的第一个元素是类型参数名（如 "T"）
                    // 对应 params 中没有类型标注（Unknown）的参数
                    // 简化：用第一个参数的类型推断约束类型参数的值
                    for (constraint_param, _trait_name) in type_constraints {
                        // 找到使用这个约束类型参数的参数
                        for (i, param) in params.iter().enumerate() {
                            if let Some(Type::Struct { name: ref param_type_name, .. }) = &param.type_annotation {
                                if param_type_name == constraint_param {
                                    if let Some(concrete) = concrete_types.get(i) {
                                        result.insert(constraint_param.clone(), concrete.clone());
                                    }
                                }
                            }
                            // 也检查没有标注的参数（泛型参数）
                            if param.type_annotation.is_none() {
                                // 使用第一个未标注参数的约束类型
                                if let Some(concrete) = concrete_types.get(i) {
                                    result.entry(constraint_param.clone()).or_insert_with(|| concrete.clone());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    result
}

/// 在特化体中无条件替换 trait 方法调用为 impl 函数调用
/// 因为特化后所有类型参数都是具体的，trait 方法可以直接静态解析
fn rewrite_trait_calls_unconditional(
    expr: &mut Expr,
    impl_map: &HashMap<String, Vec<(Type, String)>>,
    constraint_type_map: &HashMap<String, Type>,
) {
    match expr {
        Expr::Block { statements, final_expr, .. } => {
            for stmt in statements.iter_mut() {
                rewrite_stmt_unconditional(stmt, impl_map, constraint_type_map);
            }
            if let Some(fe) = final_expr {
                rewrite_trait_calls_unconditional(fe, impl_map, constraint_type_map);
            }
        }
        Expr::FunctionCall { function, args, .. } => {
            rewrite_trait_calls_unconditional(function, impl_map, constraint_type_map);
            for arg in args.iter_mut() {
                rewrite_trait_calls_unconditional(arg, impl_map, constraint_type_map);
            }
            if let Expr::Identifier { name, .. } = function.as_ref() {
                if let Some(impls) = impl_map.get(name) {
                    // 策略：先尝试按参数类型匹配，如果只有一个 impl 直接用
                    let resolved_name = if impls.len() == 1 {
                        Some(impls[0].1.clone())
                    } else {
                        // 多个 impl 时尝试匹配参数类型
                        let mut found = None;
                        if let Some(first_arg) = args.first() {
                            if let Some(arg_type) = resolve_arg_type(first_arg, constraint_type_map) {
                                for (target_type, mangled_name) in impls {
                                    if type_match(target_type, &arg_type) {
                                        found = Some(mangled_name.clone());
                                        break;
                                    }
                                }
                            }
                        }
                        // 如果约束类型映射中有条目，使用第一个约束类型的 impl
                        if found.is_none() {
                            if let Some(first_concrete_type) = constraint_type_map.values().next() {
                                for (target_type, mangled_name) in impls {
                                    if type_match(target_type, first_concrete_type) {
                                        found = Some(mangled_name.clone());
                                        break;
                                    }
                                }
                            }
                        }
                        found
                    };
                    if let Some(mangled_name) = resolved_name {
                        if let Expr::Identifier { name: ref mut n, .. } = function.as_mut() {
                            *n = mangled_name;
                        }
                    }
                }
            }
        }
        Expr::If { condition, then_branch, else_branch, .. } => {
            rewrite_trait_calls_unconditional(condition, impl_map, constraint_type_map);
            rewrite_trait_calls_unconditional(then_branch, impl_map, constraint_type_map);
            if let Some(eb) = else_branch {
                rewrite_trait_calls_unconditional(eb, impl_map, constraint_type_map);
            }
        }
        Expr::While { condition, body, .. } => {
            rewrite_trait_calls_unconditional(condition, impl_map, constraint_type_map);
            rewrite_trait_calls_unconditional(body, impl_map, constraint_type_map);
        }
        Expr::BinaryOp { left, right, op, span: _ } => {
            rewrite_trait_calls_unconditional(left, impl_map, constraint_type_map);
            rewrite_trait_calls_unconditional(right, impl_map, constraint_type_map);

            // 操作符重载（特化体版本：无条件匹配，使用约束类型映射）
            if let Some(method_name) = operator_method_name(op) {
                if let Some(impls) = impl_map.get(method_name) {
                    let resolved_name = if impls.len() == 1 {
                        Some(impls[0].1.clone())
                    } else {
                        // 多个 impl 时用约束类型映射
                        if let Some(concrete_type) = constraint_type_map.values().next() {
                            impls.iter()
                                .find(|(target_type, _)| type_match(target_type, concrete_type))
                                .map(|(_, mangled_name)| mangled_name.clone())
                        } else {
                            None
                        }
                    };
                    if let Some(mangled_name) = resolved_name {
                        let old = std::mem::replace(expr, Expr::Number { value: 0, span: Span::dummy() });
                        if let Expr::BinaryOp { left: l, right: r, span: s, .. } = old {
                            *expr = Expr::FunctionCall {
                                function: Box::new(Expr::Identifier {
                                    name: mangled_name,
                                    span: s,
                                }),
                                args: vec![*l, *r],
                                span: s,
                            };
                        }
                        return;
                    }
                }
            }
        }
        Expr::UnaryOp { operand, .. } => {
            rewrite_trait_calls_unconditional(operand, impl_map, constraint_type_map);
        }
        Expr::Lambda { body, .. } => {
            rewrite_trait_calls_unconditional(body, impl_map, constraint_type_map);
        }
        Expr::Return { value, .. } => {
            if let Some(v) = value {
                rewrite_trait_calls_unconditional(v, impl_map, constraint_type_map);
            }
        }
        Expr::Assignment { value, .. } => {
            rewrite_trait_calls_unconditional(value, impl_map, constraint_type_map);
        }
        Expr::TupleLiteral { elements, .. } | Expr::ArrayLiteral { elements, .. } => {
            for e in elements.iter_mut() {
                rewrite_trait_calls_unconditional(e, impl_map, constraint_type_map);
            }
        }
        Expr::Match { expr: scrutinee, arms, .. } => {
            rewrite_trait_calls_unconditional(scrutinee, impl_map, constraint_type_map);
            for arm in arms.iter_mut() {
                rewrite_trait_calls_unconditional(&mut arm.body, impl_map, constraint_type_map);
            }
        }
        Expr::Statement { stmt, .. } => {
            rewrite_stmt_unconditional(stmt, impl_map, constraint_type_map);
        }
        _ => {}
    }
}

fn rewrite_stmt_unconditional(
    stmt: &mut Statement,
    impl_map: &HashMap<String, Vec<(Type, String)>>,
    constraint_type_map: &HashMap<String, Type>,
) {
    match stmt {
        Statement::Expression { expr, .. } => {
            rewrite_trait_calls_unconditional(expr, impl_map, constraint_type_map);
        }
        Statement::Let { value, .. } => {
            rewrite_trait_calls_unconditional(value, impl_map, constraint_type_map);
        }
        Statement::FunctionDef { body, .. } => {
            rewrite_trait_calls_unconditional(body, impl_map, constraint_type_map);
        }
        Statement::Assignment { value, .. } => {
            rewrite_trait_calls_unconditional(value, impl_map, constraint_type_map);
        }
        _ => {}
    }
}

/// 尝试解析参数表达式的类型
fn resolve_arg_type(arg: &Expr, constraint_type_map: &HashMap<String, Type>) -> Option<Type> {
    match arg {
        Expr::Identifier { name, .. } => {
            // 如果标识符是约束类型参数名，返回对应的具体类型
            constraint_type_map.get(name).cloned()
        }
        _ => None,
    }
}

/// 泛型函数特化（迭代式：处理嵌套泛型调用）
pub fn specialize_generic_functions(
    body: &mut Expr,
    trait_impl_map: &HashMap<String, Vec<(Type, String)>>,
    expr_types: &HashMap<usize, Type>,
    func_sigs: &HashMap<String, (Vec<Type>, Type)>,
) {
    let mut all_sites: Vec<(String, Vec<Type>, String)> = Vec::new();
    let mut iteration = 0;
    loop {
        iteration += 1;
        if iteration > 10 { break; }

        // 收集原始 AST 中的特化点（依赖 expr_types）
        let mut new_sites: Vec<(String, Vec<Type>, String)> = Vec::new();
        collect_specialization_sites(body, trait_impl_map, expr_types, func_sigs, &mut new_sites);

        // 同时收集已生成的特化函数体中的特化点
        if let Expr::Block { statements, .. } = body {
            for stmt in statements.iter() {
                if let Statement::FunctionDef { name, params, body: fn_body, type_constraints, .. } = stmt {
                    if type_constraints.is_empty() && name.contains('$') {
                        let param_types: Vec<Type> = params.iter()
                            .filter_map(|p| p.type_annotation.clone())
                            .collect();
                        collect_specialization_sites_from_body(fn_body, &param_types, func_sigs, trait_impl_map, &mut new_sites);
                    }
                }
            }
        }

        // 过滤掉已经特化过的
        let fresh: Vec<_> = new_sites.into_iter()
            .filter(|s| !all_sites.iter().any(|e| e.2 == s.2))
            .collect();

        if fresh.is_empty() { break; }

        // 为新的特化点生成特化函数
        let mut new_functions = Vec::new();
        for (func_name, concrete_types, mangled_name) in &fresh {
            if let Some(original_body) = find_function_body(body, func_name) {
                let mut specialized_body = original_body.clone();
                let constraint_type_map = build_constraint_type_map(func_name, body, concrete_types);
                rewrite_trait_calls_unconditional(&mut specialized_body, trait_impl_map, &constraint_type_map);
                rewrite_inner_generic_calls(&mut specialized_body, &all_sites, func_name);

                let param_names = find_function_param_names(body, func_name);
                let params: Vec<Parameter> = concrete_types.iter().enumerate().map(|(i, ty)| {
                    Parameter {
                        name: param_names.get(i).cloned().unwrap_or_else(|| format!("__p{}", i)),
                        type_annotation: Some(ty.clone()),
                        span: karte_diagnostics::Span::dummy(),
                    }
                }).collect();

                new_functions.push(Statement::FunctionDef {
                    name: mangled_name.clone(),
                    params,
                    return_type: None,
                    body: specialized_body,
                    is_pub: false,
                    type_constraints: vec![],
                    span: karte_diagnostics::Span::dummy(),
                });
            }
        }

        if let Expr::Block { statements, .. } = body {
            statements.extend(new_functions);
        }

        rewrite_specialization_calls(body, &fresh);
        all_sites.extend(fresh);
    }
}

/// 从已特化函数的函数体中收集需要特化的调用点
/// 使用函数的参数类型推断调用参数的具体类型
fn collect_specialization_sites_from_body(
    expr: &Expr,
    param_types: &[Type],
    func_sigs: &HashMap<String, (Vec<Type>, Type)>,
    trait_impl_map: &HashMap<String, Vec<(Type, String)>>,
    sites: &mut Vec<(String, Vec<Type>, String)>,
) {
    match expr {
        Expr::Block { statements, final_expr, .. } => {
            for stmt in statements {
                if let Statement::Expression { expr: e, .. } = stmt {
                    collect_specialization_sites_from_body(e, param_types, func_sigs, trait_impl_map, sites);
                } else if let Statement::Let { value: e, .. } = stmt {
                    collect_specialization_sites_from_body(e, param_types, func_sigs, trait_impl_map, sites);
                } else if let Statement::Assignment { value: e, .. } = stmt {
                    collect_specialization_sites_from_body(e, param_types, func_sigs, trait_impl_map, sites);
                }
            }
            if let Some(fe) = final_expr {
                collect_specialization_sites_from_body(fe, param_types, func_sigs, trait_impl_map, sites);
            }
        }
        Expr::FunctionCall { function, args, .. } => {
            if let Expr::Identifier { name, .. } = function.as_ref() {
                if !trait_impl_map.contains_key(name) && !is_builtin(name) {
                    if let Some((sig_param_types, _)) = func_sigs.get(name) {
                        let has_type_var = sig_param_types.iter().any(|t| has_var(t));
                        if has_type_var {
                            // 使用参数位置推断具体类型
                            // 如果 arg 是 Identifier 且是函数参数，用 param_types
                            let concrete_types: Vec<Type> = args.iter().enumerate().map(|(i, arg)| {
                                if let Expr::Identifier { name: _arg_name, .. } = arg {
                                    // 查找 arg_name 在当前函数参数中的位置
                                    // 简化：直接用参数位置对应的 param_type
                                    param_types.get(i).cloned().unwrap_or(Type::Number)
                                } else {
                                    param_types.get(i).cloned().unwrap_or(Type::Number)
                                }
                            }).collect();

                            if concrete_types.iter().all(|t| !has_var(t)) {
                                let mangled = mangle_function(name, &concrete_types);
                                sites.push((name.clone(), concrete_types, mangled));
                            }
                        }
                    }
                }
            }
            for arg in args {
                collect_specialization_sites_from_body(arg, param_types, func_sigs, trait_impl_map, sites);
            }
        }
        Expr::If { condition, then_branch, else_branch, .. } => {
            collect_specialization_sites_from_body(condition, param_types, func_sigs, trait_impl_map, sites);
            collect_specialization_sites_from_body(then_branch, param_types, func_sigs, trait_impl_map, sites);
            if let Some(eb) = else_branch {
                collect_specialization_sites_from_body(eb, param_types, func_sigs, trait_impl_map, sites);
            }
        }
        Expr::BinaryOp { left, right, .. } => {
            collect_specialization_sites_from_body(left, param_types, func_sigs, trait_impl_map, sites);
            collect_specialization_sites_from_body(right, param_types, func_sigs, trait_impl_map, sites);
        }
        _ => {}
    }
}

fn collect_specialization_sites(
    expr: &Expr,
    trait_impl_map: &HashMap<String, Vec<(Type, String)>>,
    expr_types: &HashMap<usize, Type>,
    func_sigs: &HashMap<String, (Vec<Type>, Type)>,
    sites: &mut Vec<(String, Vec<Type>, String)>,
) {
    match expr {
        Expr::Block { statements, final_expr, .. } => {
            for stmt in statements {
                collect_from_stmt(stmt, trait_impl_map, expr_types, func_sigs, sites);
            }
            if let Some(fe) = final_expr {
                collect_specialization_sites(fe, trait_impl_map, expr_types, func_sigs, sites);
            }
        }
        Expr::FunctionCall { function, args, .. } => {
            if let Expr::Identifier { name, .. } = function.as_ref() {
                if !trait_impl_map.contains_key(name) && !is_builtin(name) {
                    if let Some((param_types, _)) = func_sigs.get(name) {
                        let has_type_var = param_types.iter().any(|t| has_var(t));
                        if has_type_var {
                            let concrete_types: Vec<Type> = args.iter()
                                .filter_map(|arg| expr_types.get(&(arg as *const Expr as usize)).cloned())
                                .collect();
                            if concrete_types.len() == args.len() && concrete_types.iter().all(|t| !has_var(t)) {
                                let mangled = mangle_function(name, &concrete_types);
                                sites.push((name.clone(), concrete_types, mangled));
                            }
                        }
                    }
                }
            }
            collect_specialization_sites(function, trait_impl_map, expr_types, func_sigs, sites);
            for arg in args {
                collect_specialization_sites(arg, trait_impl_map, expr_types, func_sigs, sites);
            }
        }
        Expr::If { condition, then_branch, else_branch, .. } => {
            collect_specialization_sites(condition, trait_impl_map, expr_types, func_sigs, sites);
            collect_specialization_sites(then_branch, trait_impl_map, expr_types, func_sigs, sites);
            if let Some(eb) = else_branch {
                collect_specialization_sites(eb, trait_impl_map, expr_types, func_sigs, sites);
            }
        }
        Expr::While { condition, body, .. } => {
            collect_specialization_sites(condition, trait_impl_map, expr_types, func_sigs, sites);
            collect_specialization_sites(body, trait_impl_map, expr_types, func_sigs, sites);
        }
        Expr::BinaryOp { left, right, .. } => {
            collect_specialization_sites(left, trait_impl_map, expr_types, func_sigs, sites);
            collect_specialization_sites(right, trait_impl_map, expr_types, func_sigs, sites);
        }
        _ => {}
    }
}

fn collect_from_stmt(
    stmt: &Statement,
    trait_impl_map: &HashMap<String, Vec<(Type, String)>>,
    expr_types: &HashMap<usize, Type>,
    func_sigs: &HashMap<String, (Vec<Type>, Type)>,
    sites: &mut Vec<(String, Vec<Type>, String)>,
) {
    match stmt {
        Statement::Expression { expr, .. } | Statement::Let { value: expr, .. } |
        Statement::Assignment { value: expr, .. } => {
            collect_specialization_sites(expr, trait_impl_map, expr_types, func_sigs, sites);
        }
        Statement::FunctionDef { body, .. } => {
            collect_specialization_sites(body, trait_impl_map, expr_types, func_sigs, sites);
        }
        _ => {}
    }
}

fn has_var(ty: &Type) -> bool {
    match ty {
        Type::Var(_) => true,
        Type::Function { params, return_type, .. } => {
            params.iter().any(has_var) || has_var(return_type)
        }
        _ => false,
    }
}

fn is_builtin(name: &str) -> bool {
    matches!(name, "abs" | "min" | "max" | "clamp" | "len" |
        "print_number" | "print_string" | "println" | "println_number" |
        "unsafe_cast" | "gc_alloc" | "mem_load64" | "mem_store64" |
        "char_to_string" | "str_equal" | "str_compare" | "raw_syscall6" |
        "string_index" | "string_char_at" | "string_substring" |
        "string_contains" | "string_split_count" | "to_string" |
        "heap_allocate" | "heap_free" | "retain" | "release")
}

fn mangle_function(name: &str, types: &[Type]) -> String {
    let type_strs: Vec<String> = types.iter().map(|t| {
        match t {
            Type::Number => "number".to_string(),
            Type::Bool => "bool".to_string(),
            Type::String => "string".to_string(),
            Type::Unit => "unit".to_string(),
            other => format!("{}", other).replace(' ', "_"),
        }
    }).collect();
    format!("{}${}", name, type_strs.join("$"))
}

fn find_function_body(expr: &Expr, func_name: &str) -> Option<Expr> {
    if let Expr::Block { statements, .. } = expr {
        for stmt in statements {
            if let Statement::FunctionDef { name, body, .. } = stmt {
                if name == func_name {
                    return Some(body.clone());
                }
            }
        }
    }
    None
}

fn find_function_param_names(expr: &Expr, func_name: &str) -> Vec<String> {
    if let Expr::Block { statements, .. } = expr {
        for stmt in statements {
            if let Statement::FunctionDef { name, params, .. } = stmt {
                if name == func_name {
                    return params.iter().map(|p| p.name.clone()).collect();
                }
            }
        }
    }
    Vec::new()
}

fn rewrite_specialization_calls(
    expr: &mut Expr,
    sites: &[(String, Vec<Type>, String)],
) {
    match expr {
        Expr::Block { statements, final_expr, .. } => {
            for stmt in statements.iter_mut() {
                rewrite_specialization_stmt(stmt, sites);
            }
            if let Some(fe) = final_expr {
                rewrite_specialization_calls(fe, sites);
            }
        }
        Expr::FunctionCall { function, args, .. } => {
            rewrite_specialization_calls(function, sites);
            for arg in args.iter_mut() {
                rewrite_specialization_calls(arg, sites);
            }
            if let Expr::Identifier { name, .. } = function.as_ref() {
                for (orig_name, _, mangled) in sites {
                    if name == orig_name {
                        if let Expr::Identifier { name: ref mut n, .. } = function.as_mut() {
                            *n = mangled.clone();
                        }
                        break;
                    }
                }
            }
        }
        Expr::If { condition, then_branch, else_branch, .. } => {
            rewrite_specialization_calls(condition, sites);
            rewrite_specialization_calls(then_branch, sites);
            if let Some(eb) = else_branch {
                rewrite_specialization_calls(eb, sites);
            }
        }
        Expr::While { condition, body, .. } => {
            rewrite_specialization_calls(condition, sites);
            rewrite_specialization_calls(body, sites);
        }
        Expr::BinaryOp { left, right, .. } => {
            rewrite_specialization_calls(left, sites);
            rewrite_specialization_calls(right, sites);
        }
        Expr::Assignment { value, .. } => {
            rewrite_specialization_calls(value, sites);
        }
        Expr::Return { value, .. } => {
            if let Some(v) = value {
                rewrite_specialization_calls(v, sites);
            }
        }
        Expr::Lambda { body, .. } => {
            rewrite_specialization_calls(body, sites);
        }
        Expr::UnaryOp { operand, .. } => {
            rewrite_specialization_calls(operand, sites);
        }
        Expr::TupleLiteral { elements, .. } | Expr::ArrayLiteral { elements, .. } => {
            for e in elements.iter_mut() {
                rewrite_specialization_calls(e, sites);
            }
        }
        Expr::Statement { stmt, .. } => {
            rewrite_specialization_stmt(stmt, sites);
        }
        _ => {}
    }
}

fn rewrite_specialization_stmt(
    stmt: &mut Statement,
    sites: &[(String, Vec<Type>, String)],
) {
    match stmt {
        Statement::Expression { expr, .. } | Statement::Let { value: expr, .. } |
        Statement::Assignment { value: expr, .. } => {
            rewrite_specialization_calls(expr, sites);
        }
        Statement::FunctionDef { body, .. } => {
            rewrite_specialization_calls(body, sites);
        }
        _ => {}
    }
}

/// 在特化体中替换内部的泛型函数调用
/// 例如 check$number$number 的体中调用 same(a,b)，需要替换为 same$number$number(a,b)
fn rewrite_inner_generic_calls(
    expr: &mut Expr,
    all_sites: &[(String, Vec<Type>, String)],
    _current_func: &str,
) {
    match expr {
        Expr::Block { statements, final_expr, .. } => {
            for stmt in statements.iter_mut() {
                rewrite_inner_generic_calls_stmt(stmt, all_sites);
            }
            if let Some(fe) = final_expr {
                rewrite_inner_generic_calls(fe, all_sites, "");
            }
        }
        Expr::FunctionCall { function, args, .. } => {
            rewrite_inner_generic_calls(function, all_sites, "");
            for arg in args.iter_mut() {
                rewrite_inner_generic_calls(arg, all_sites, "");
            }
            // 如果 function 是 Identifier 且匹配某个 site，替换
            if let Expr::Identifier { name, .. } = function.as_ref() {
                for (orig_name, _, mangled) in all_sites {
                    if name == orig_name {
                        if let Expr::Identifier { name: ref mut n, .. } = function.as_mut() {
                            *n = mangled.clone();
                        }
                        break;
                    }
                }
            }
        }
        Expr::If { condition, then_branch, else_branch, .. } => {
            rewrite_inner_generic_calls(condition, all_sites, "");
            rewrite_inner_generic_calls(then_branch, all_sites, "");
            if let Some(eb) = else_branch {
                rewrite_inner_generic_calls(eb, all_sites, "");
            }
        }
        Expr::While { condition, body, .. } => {
            rewrite_inner_generic_calls(condition, all_sites, "");
            rewrite_inner_generic_calls(body, all_sites, "");
        }
        Expr::BinaryOp { left, right, .. } => {
            rewrite_inner_generic_calls(left, all_sites, "");
            rewrite_inner_generic_calls(right, all_sites, "");
        }
        Expr::UnaryOp { operand, .. } => {
            rewrite_inner_generic_calls(operand, all_sites, "");
        }
        Expr::Return { value, .. } => {
            if let Some(v) = value {
                rewrite_inner_generic_calls(v, all_sites, "");
            }
        }
        Expr::Assignment { value, .. } => {
            rewrite_inner_generic_calls(value, all_sites, "");
        }
        Expr::Match { expr: scrutinee, arms, .. } => {
            rewrite_inner_generic_calls(scrutinee, all_sites, "");
            for arm in arms.iter_mut() {
                rewrite_inner_generic_calls(&mut arm.body, all_sites, "");
            }
        }
        Expr::Statement { stmt, .. } => {
            rewrite_inner_generic_calls_stmt(stmt, all_sites);
        }
        _ => {}
    }
}

fn rewrite_inner_generic_calls_stmt(
    stmt: &mut Statement,
    all_sites: &[(String, Vec<Type>, String)],
) {
    match stmt {
        Statement::Expression { expr, .. } | Statement::Let { value: expr, .. } |
        Statement::Assignment { value: expr, .. } => {
            rewrite_inner_generic_calls(expr, all_sites, "");
        }
        Statement::FunctionDef { body, .. } => {
            rewrite_inner_generic_calls(body, all_sites, "");
        }
        _ => {}
    }
}

/// VTable 解析 pass：将 vtable() 和 vtable_method() 调用替换为具体 impl 函数引用
/// 作为 rewrite_trait_calls 之后的独立 pass 执行
pub fn resolve_vtables(expr: &mut Expr, impl_map: &HashMap<String, Vec<(Type, String)>>) {
    // 先递归处理子节点
    match expr {
        Expr::Block { statements, final_expr, .. } => {
            for stmt in statements.iter_mut() {
                resolve_vtables_stmt(stmt, impl_map);
            }
            if let Some(fe) = final_expr {
                resolve_vtables(fe, impl_map);
            }
        }
        Expr::FunctionCall { function, args, .. } => {
            resolve_vtables(function, impl_map);
            for arg in args.iter_mut() {
                resolve_vtables(arg, impl_map);
            }
            // 检查是否是 vtable/vtable_method 调用并替换
            let replacement = check_vtable_replacement(function, args, impl_map);
            if let Some(new_expr) = replacement {
                *expr = new_expr;
            }
        }
        Expr::If { condition, then_branch, else_branch, .. } => {
            resolve_vtables(condition, impl_map);
            resolve_vtables(then_branch, impl_map);
            if let Some(eb) = else_branch {
                resolve_vtables(eb, impl_map);
            }
        }
        Expr::While { condition, body, .. } => {
            resolve_vtables(condition, impl_map);
            resolve_vtables(body, impl_map);
        }
        Expr::BinaryOp { left, right, .. } => {
            resolve_vtables(left, impl_map);
            resolve_vtables(right, impl_map);
        }
        Expr::UnaryOp { operand, .. } => {
            resolve_vtables(operand, impl_map);
        }
        Expr::Lambda { body, .. } => {
            resolve_vtables(body, impl_map);
        }
        Expr::Statement { stmt, .. } => {
            resolve_vtables_stmt(stmt, impl_map);
        }
        Expr::StructLiteral { fields, .. } => {
            for f in fields.iter_mut() {
                resolve_vtables(&mut f.value, impl_map);
            }
        }
        Expr::Match { expr: match_expr, arms, .. } => {
            resolve_vtables(match_expr, impl_map);
            for arm in arms.iter_mut() {
                resolve_vtables(&mut arm.body, impl_map);
            }
        }
        _ => {}
    }
}

/// 检查 FunctionCall 是否是 vtable/vtable_method 调用，返回替换表达式
fn check_vtable_replacement(
    function: &Expr,
    args: &[Expr],
    impl_map: &HashMap<String, Vec<(Type, String)>>,
) -> Option<Expr> {
    if let Expr::Identifier { name, .. } = function {
        // vtable("Trait", "Type") → 查找第一个匹配的 impl 方法
        if name == "vtable" && args.len() == 2 {
            let trait_name = extract_string_arg(args, 0)?;
            let type_str = extract_string_arg(args, 1)?;
            let target_type = parse_type_from_str(&type_str);
            let type_name = type_name_for_mangle(&target_type);
            for (_method_name, impls) in impl_map.iter() {
                for (impl_target, mangled_name) in impls {
                    if type_match(impl_target, &target_type) &&
                        mangled_name.starts_with(&format!("{}${}", trait_name, type_name)) {
                        return Some(Expr::Identifier { name: mangled_name.clone(), span: Span::dummy() });
                    }
                }
            }
        }
        // vtable_method("Trait", "Type", "method") → 查找指定方法
        if name == "vtable_method" && args.len() == 3 {
            let trait_name = extract_string_arg(args, 0)?;
            let type_str = extract_string_arg(args, 1)?;
            let method_str = extract_string_arg(args, 2)?;
            let target_type = parse_type_from_str(&type_str);
            let type_name = type_name_for_mangle(&target_type);
            let mangled = format!("{}${}${}", trait_name, type_name, method_str);
            return Some(Expr::Identifier { name: mangled, span: Span::dummy() });
        }
    }
    None
}

fn extract_string_arg(args: &[Expr], index: usize) -> Option<String> {
    match args.get(index) {
        Some(Expr::StringLiteral { value, .. }) => Some(value.clone()),
        Some(Expr::Identifier { name, .. }) => Some(name.clone()),
        _ => None,
    }
}

fn parse_type_from_str(s: &str) -> Type {
    match s {
        "number" => Type::Number,
        "bool" => Type::Bool,
        "string" => Type::String,
        n => Type::Struct { name: n.to_string(), fields: vec![] },
    }
}

fn resolve_vtables_stmt(stmt: &mut Statement, impl_map: &HashMap<String, Vec<(Type, String)>>) {
    match stmt {
        Statement::Expression { expr, .. } | Statement::Let { value: expr, .. } |
        Statement::Assignment { value: expr, .. } => {
            resolve_vtables(expr, impl_map);
        }
        Statement::FunctionDef { body, .. } => {
            resolve_vtables(body, impl_map);
        }
        _ => {}
    }
}
