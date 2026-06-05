/// 辅助函数模块
///
/// 本模块包含lowering过程中使用的各种辅助函数：
/// - 运算符转换（HIR -> MIR）
/// - 变量收集（用于闭包捕获分析）
/// - 所有权推断
/// - 模式转换（HIR Pattern -> MIR Pattern）
/// - 堆布局推断
use super::types::LoweringContext;
use crate::{
    BinaryOperator as MirBinaryOp, EscapeState, HeapLayout, Pattern, Statement,
    UnaryOperator as MirUnaryOp, Value,
};
use karte_common::memory::OwnershipKind;
use karte_diagnostics::Span;
use karte_hir::{BinaryOperator as HirBinaryOp, Expr, UnaryOperator as HirUnaryOp};

/// 将表达式降级到一个新的临时变量中
pub(crate) fn lower_expression_to_temp(
    ctx: &mut LoweringContext,
    expr: &Expr,
) -> Result<Value, Vec<String>> {
    let temp = ctx.new_temp();
    super::expr::lower_expression(ctx, expr, &temp)?;
    Ok(temp)
}

/// 转换HIR二元运算符到MIR
pub(crate) fn convert_binary_op(op: &HirBinaryOp) -> MirBinaryOp {
    match op {
        HirBinaryOp::Add => MirBinaryOp::Add,
        HirBinaryOp::Subtract => MirBinaryOp::Subtract,
        HirBinaryOp::Multiply => MirBinaryOp::Multiply,
        HirBinaryOp::Divide => MirBinaryOp::Divide,
        HirBinaryOp::Modulo => MirBinaryOp::Modulo,
        HirBinaryOp::Equal => MirBinaryOp::Equal,
        HirBinaryOp::NotEqual => MirBinaryOp::NotEqual,
        HirBinaryOp::GreaterEqual => MirBinaryOp::GreaterEqual,
        HirBinaryOp::LessEqual => MirBinaryOp::LessEqual,
        HirBinaryOp::Greater => MirBinaryOp::GreaterThan,
        HirBinaryOp::Less => MirBinaryOp::LessThan,
        HirBinaryOp::LogicalAnd => MirBinaryOp::And,
        HirBinaryOp::LogicalOr => MirBinaryOp::Or,
        HirBinaryOp::BitAnd => MirBinaryOp::BitAnd,
        HirBinaryOp::BitOr => MirBinaryOp::BitOr,
        HirBinaryOp::BitXor => MirBinaryOp::BitXor,
        HirBinaryOp::ShiftLeft => MirBinaryOp::ShiftLeft,
        HirBinaryOp::ShiftRight => MirBinaryOp::ShiftRight,
    }
}

/// 转换HIR一元运算符到MIR
pub(crate) fn convert_unary_op(op: &HirUnaryOp) -> MirUnaryOp {
    match op {
        HirUnaryOp::Plus => MirUnaryOp::Plus,
        HirUnaryOp::Minus => MirUnaryOp::Minus,
        HirUnaryOp::LogicalNot => MirUnaryOp::Not,
        HirUnaryOp::BitNot => MirUnaryOp::BitNot,
    }
}

/// 收集表达式中引用的所有变量名
pub(crate) fn collect_referenced_variables(expr: &Expr) -> Vec<String> {
    let mut vars = Vec::new();
    collect_vars_recursive(expr, &mut vars);
    vars.sort();
    vars.dedup();
    vars
}

/// 递归收集变量引用
///
/// 完整遍历所有表达式类型，确保不遗漏任何子表达式中的变量引用。
/// 这对闭包捕获分析至关重要——如果遗漏了某个表达式类型的遍历，
/// 闭包就无法正确捕获该表达式中引用的外部变量。
fn collect_vars_recursive(expr: &Expr, vars: &mut Vec<String>) {
    match expr {
        // 基础值：直接收集变量名
        Expr::Identifier { name, .. } => {
            vars.push(name.clone());
        }
        // 不含变量的字面量
        Expr::Number { .. }
        | Expr::StringLiteral { .. }
        | Expr::Unit { .. }
        | Expr::Boolean { .. }
        | Expr::ModuleSymbolAccess { .. }
        | Expr::RuntimeGlobal { .. }
        | Expr::GcRegOp { .. }
        | Expr::Break { .. }
        | Expr::Continue { .. } => {}

        // 二元/一元操作
        Expr::BinaryOp { left, right, .. } => {
            collect_vars_recursive(left, vars);
            collect_vars_recursive(right, vars);
        }
        Expr::UnaryOp { operand, .. } => {
            collect_vars_recursive(operand, vars);
        }

        // 条件表达式
        Expr::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            collect_vars_recursive(condition, vars);
            collect_vars_recursive(then_branch, vars);
            if let Some(else_branch) = else_branch {
                collect_vars_recursive(else_branch, vars);
            }
        }
        Expr::While {
            condition, body, ..
        } => {
            collect_vars_recursive(condition, vars);
            collect_vars_recursive(body, vars);
        }

        // Lambda：收集自由变量（排除lambda自身参数）
        Expr::Lambda { params, body, .. } => {
            let mut lambda_vars = Vec::new();
            collect_vars_recursive(body, &mut lambda_vars);
            let param_names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
            for var in lambda_vars {
                if !param_names.contains(&var) {
                    vars.push(var);
                }
            }
        }

        // 函数调用
        Expr::FunctionCall { function, args, .. } => {
            collect_vars_recursive(function, vars);
            for arg in args {
                collect_vars_recursive(arg, vars);
            }
        }

        // 语句和块
        Expr::Statement { stmt, .. } => {
            collect_vars_in_statement(stmt, vars);
        }
        Expr::Block {
            statements,
            final_expr,
            ..
        } => {
            for stmt in statements {
                collect_vars_in_statement(stmt, vars);
            }
            if let Some(final_expr) = final_expr {
                collect_vars_recursive(final_expr, vars);
            }
        }
        Expr::Assignment { target, value, .. } => {
            collect_vars_recursive(target, vars);
            collect_vars_recursive(value, vars);
        }

        // 数组相关
        Expr::ArrayLiteral { elements, .. } => {
            for element in elements {
                collect_vars_recursive(element, vars);
            }
        }
        Expr::Index { array, index, .. } => {
            collect_vars_recursive(array, vars);
            collect_vars_recursive(index, vars);
        }
        Expr::ArrayLen { array, .. } => {
            collect_vars_recursive(array, vars);
        }

        Expr::Abs { value, .. } => {
            collect_vars_recursive(value, vars);
        }

        Expr::Min { left, right, .. } => {
            collect_vars_recursive(left, vars);
            collect_vars_recursive(right, vars);
        }

        Expr::Max { left, right, .. } => {
            collect_vars_recursive(left, vars);
            collect_vars_recursive(right, vars);
        }

        Expr::Clamp { value, min_val, max_val, .. } => {
            collect_vars_recursive(value, vars);
            collect_vars_recursive(min_val, vars);
            collect_vars_recursive(max_val, vars);
        }

        Expr::StrIndex { string, index, .. } => {
            collect_vars_recursive(string, vars);
            collect_vars_recursive(index, vars);
        }

        Expr::CharAt { string, index, .. } => {
            collect_vars_recursive(string, vars);
            collect_vars_recursive(index, vars);
        }

        Expr::Substring { string, start, length, .. } => {
            collect_vars_recursive(string, vars);
            collect_vars_recursive(start, vars);
            collect_vars_recursive(length, vars);
        }

        Expr::StrContains { string, char_code, .. } => {
            collect_vars_recursive(string, vars);
            collect_vars_recursive(char_code, vars);
        }

        Expr::SplitCount { string, separator, .. } => {
            collect_vars_recursive(string, vars);
            collect_vars_recursive(separator, vars);
        }

        Expr::Trim { string, .. } => {
            collect_vars_recursive(string, vars);
        }

        Expr::CharToString { expr, .. } => {
            collect_vars_recursive(expr, vars);
        }

        Expr::ToString { expr, .. } => {
            collect_vars_recursive(expr, vars);
        }

        // 元组相关
        Expr::TupleLiteral { elements, .. } => {
            for element in elements {
                collect_vars_recursive(element, vars);
            }
        }
        Expr::TupleAccess { object, .. } => {
            collect_vars_recursive(object, vars);
        }

        // 结构体相关
        Expr::StructLiteral { fields, .. } => {
            for field_init in fields {
                collect_vars_recursive(&field_init.value, vars);
            }
        }
        Expr::FieldAccess { object, .. } => {
            collect_vars_recursive(object, vars);
        }

        // 引用与解引用
        Expr::Reference { expr, .. } => {
            collect_vars_recursive(expr, vars);
        }
        Expr::Dereference { expr, .. } => {
            collect_vars_recursive(expr, vars);
        }

        // 代数数据类型：构造器
        Expr::Constructor { args, .. } => {
            for arg in args {
                collect_vars_recursive(arg, vars);
            }
        }
        Expr::QualifiedConstructor { args, .. } => {
            for arg in args {
                collect_vars_recursive(arg, vars);
            }
        }

        // 模式匹配
        Expr::Match { expr, arms, .. } => {
            collect_vars_recursive(expr, vars);
            for arm in arms {
                // arm.body 中可能引用外部变量；arm.pattern 中绑定的变量是局部的
                collect_vars_recursive(&arm.body, vars);
            }
        }

        // 循环
        Expr::ForIn {
            start, end, body, inclusive: _, ..
        } => {
            collect_vars_recursive(start, vars);
            collect_vars_recursive(end, vars);
            collect_vars_recursive(body, vars);
        }

        Expr::ForArray { array, body, .. } => {
            collect_vars_recursive(array, vars);
            collect_vars_recursive(body, vars);
        }

        // 返回
        Expr::Return { value, .. } => {
            if let Some(value) = value {
                collect_vars_recursive(value, vars);
            }
        }

        // 内存管理
        Expr::HeapAllocate { value, .. } => {
            collect_vars_recursive(value, vars);
        }
        Expr::HeapFree { pointer, .. } => {
            collect_vars_recursive(pointer, vars);
        }
        Expr::Retain { pointer, .. } => {
            collect_vars_recursive(pointer, vars);
        }
        Expr::Release { pointer, .. } => {
            collect_vars_recursive(pointer, vars);
        }
        Expr::UnsafeLoad { addr, .. } => {
            collect_vars_recursive(addr, vars);
        }
        Expr::UnsafeStore {
            addr, value, ..
        } => {
            collect_vars_recursive(addr, vars);
            collect_vars_recursive(value, vars);
        }

        // 代数效应
        Expr::EffectPerform { tag, payload, .. } => {
            collect_vars_recursive(tag, vars);
            collect_vars_recursive(payload, vars);
        }
        Expr::EffectResume { value, .. } => {
            collect_vars_recursive(value, vars);
        }
        Expr::EffectHandle {
            tag,
            handler,
            body,
            ..
        } => {
            collect_vars_recursive(tag, vars);
            collect_vars_recursive(handler, vars);
            collect_vars_recursive(body, vars);
        }

        // 类型转换
        Expr::TypeCast { expr, .. } => {
            collect_vars_recursive(expr, vars);
        }
    }
}

/// 收集语句中的变量引用
fn collect_vars_in_statement(stmt: &karte_hir::Statement, vars: &mut Vec<String>) {
    match stmt {
        karte_hir::Statement::Let { value, .. } => {
            collect_vars_recursive(value, vars);
        }
        karte_hir::Statement::Expression { expr, .. } => {
            collect_vars_recursive(expr, vars);
        }
        karte_hir::Statement::Assignment { target, value, .. } => {
            collect_vars_recursive(target, vars);
            collect_vars_recursive(value, vars);
        }
        _ => {}
    }
}

/// 推断表达式的所有权类型
pub(crate) fn infer_expr_ownership(ctx: &LoweringContext, expr: &Expr) -> Option<OwnershipKind> {
    match expr {
        Expr::HeapAllocate { ownership, .. } => Some(*ownership),
        Expr::Identifier { name, .. } => ctx.lookup_variable(name).and_then(|b| b.ownership),
        Expr::Block { final_expr, .. } => final_expr
            .as_ref()
            .and_then(|inner| infer_expr_ownership(ctx, inner)),
        Expr::If {
            then_branch,
            else_branch,
            ..
        } => {
            if let Some(else_branch) = else_branch {
                let then_kind = infer_expr_ownership(ctx, then_branch);
                let else_kind = infer_expr_ownership(ctx, else_branch);
                if then_kind.is_some() && then_kind == else_kind {
                    then_kind
                } else {
                    None
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

/// 检查表达式是否创建新的引用
pub(crate) fn expr_creates_new_ref(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::HeapAllocate {
            ownership: OwnershipKind::RefCounted,
            ..
        }
    )
}

/// 如果表达式需要，为其生成Retain语句
pub(crate) fn maybe_retain_for_expr(ctx: &mut LoweringContext, expr: &Expr, value: &Value) {
    if matches!(
        infer_expr_ownership(ctx, expr),
        Some(OwnershipKind::RefCounted)
    ) && !expr_creates_new_ref(expr)
    {
        ctx.add_statement(Statement::Retain {
            value: value.clone(),
            span: expr.span(),
        });
    }
}

/// 为逃逸值生成Retain语句
pub(crate) fn maybe_retain_for_escape(ctx: &mut LoweringContext, expr: &Expr, value: &Value) {
    if matches!(
        infer_expr_ownership(ctx, expr),
        Some(OwnershipKind::RefCounted)
    ) {
        ctx.add_statement(Statement::Retain {
            value: value.clone(),
            span: expr.span(),
        });
    }
}

/// 从表达式推断堆布局
pub(crate) fn infer_heap_layout_from_expr(expr: &Expr) -> HeapLayout {
    let (type_id, slots) = match expr {
        Expr::StructLiteral { name, fields, .. } => {
            (format!("struct:{}", name), fields.len().max(1))
        }
        Expr::Lambda { .. } => ("closure_env".to_string(), 2),
        Expr::Number { .. } => ("number".to_string(), 1),
        Expr::Boolean { .. } => ("bool".to_string(), 1),
        Expr::ArrayLiteral { elements, .. } => (
            format!("array:{}", elements.len()),
            elements.len().max(1) + 1,
        ),
        Expr::TupleLiteral { elements, .. } => (
            format!("tuple:{}", elements.len()),
            elements.len().max(1),
        ),
        _ => ("opaque".to_string(), 1),
    };

    HeapLayout {
        type_id,
        size: slots * 8,
        align: 8,
        mutable: true,
        escape: EscapeState::Global,
        ownership: OwnershipKind::Manual,
    }
}

/// 创建未知堆布局
pub(crate) fn unknown_heap_layout() -> HeapLayout {
    HeapLayout {
        type_id: "unknown".to_string(),
        size: 0,
        align: 8,
        mutable: true,
        escape: EscapeState::Global,
        ownership: OwnershipKind::Manual,
    }
}
/// 处理 let 解构模式绑定
/// 从结构体值中逐字段提取，并绑定到子模式指定的变量名
pub(crate) fn handle_destruct_pattern(
    ctx: &mut LoweringContext,
    pattern: &karte_hir::Pattern,
    value: &Value,
) -> Result<(), Vec<String>> {
    match pattern {
        karte_hir::Pattern::Variable { name, .. } => {
            ctx.bind_variable(name.clone(), value.clone(), None);
        }
        karte_hir::Pattern::Struct { fields, .. } => {
            for field_pattern in fields {
                let field_temp = ctx.new_temp();
                ctx.add_statement(Statement::FieldAccess {
                    target: field_temp.clone(),
                    object: value.clone(),
                    field: field_pattern.field.clone(),
                    span: karte_diagnostics::Span::new(0, 0),
                });
                let resolved = ctx.resolve_value(&field_temp);
                handle_destruct_pattern(ctx, field_pattern.pattern.as_ref(), &resolved)?;
            }
        }
        karte_hir::Pattern::Wildcard { .. } => {
            // 不绑定
        }
        karte_hir::Pattern::Number { .. } | karte_hir::Pattern::Boolean { .. } => {
            // 在 let 解构中，字面量模式不绑定变量
        }
        _ => {
            return Err(vec![format!(
                "Unsupported pattern in let destructuring"
            )]);
        }
    }
    Ok(())
}

/// 转换HIR模式到MIR模式
pub(crate) fn convert_pattern(pattern: &karte_hir::Pattern) -> Result<Pattern, Vec<String>> {
    match pattern {
        karte_hir::Pattern::Wildcard { .. } => Ok(Pattern::Wildcard),
        karte_hir::Pattern::Variable { name, .. } => Ok(Pattern::Variable { name: name.clone() }),
        karte_hir::Pattern::Constructor { name, args, .. } => {
            let mir_args: Vec<Pattern> = args
                .iter()
                .map(|arg| convert_pattern(arg))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Pattern::Constructor {
                name: name.clone(),
                args: mir_args,
            })
        }
        karte_hir::Pattern::Number { value, .. } => Ok(Pattern::Number { value: *value }),
        karte_hir::Pattern::Boolean { value, .. } => Ok(Pattern::Boolean { value: *value }),
        karte_hir::Pattern::QualifiedConstructor {
            type_name,
            constructor_name,
            args,
            ..
        } => {
            let mir_args: Vec<Pattern> = args
                .iter()
                .map(|arg| convert_pattern(arg))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Pattern::Constructor {
                name: format!("{}::{}", type_name, constructor_name),
                args: mir_args,
            })
        }
        karte_hir::Pattern::Struct { name, fields, .. } => {
            let mir_fields: Vec<crate::StructFieldPattern> = fields
                .iter()
                .map(|f| {
                    convert_pattern(&f.pattern).map(|p| crate::StructFieldPattern {
                        field: f.field.clone(),
                        pattern: p,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Pattern::Struct {
                name: name.clone(),
                fields: mir_fields,
            })
        }
    }
}

/// 处理模式绑定，将匹配的值绑定到变量
pub(crate) fn handle_pattern_bindings(
    ctx: &mut LoweringContext,
    pattern: &karte_hir::Pattern,
    match_value: &Value,
) -> Result<(), Vec<String>> {
    match pattern {
        karte_hir::Pattern::Variable { name, .. } => {
            // 变量模式：将整个匹配值绑定到变量
            ctx.bind_variable(name.clone(), match_value.clone(), None);
        }
        karte_hir::Pattern::Constructor {
            args,
            ..
        } => {
            for (i, arg_pattern) in args.iter().enumerate() {
                match arg_pattern {
                    karte_hir::Pattern::Variable { name, .. } => {
                        let arg_temp = ctx.new_temp();
                        ctx.add_statement(Statement::ConstructorArgExtract {
                            target: arg_temp.clone(),
                            constructor: match_value.clone(),
                            arg_index: i,
                            span: Span::new(0, 0),
                        });
                        ctx.bind_variable(name.clone(), arg_temp, None);
                    }
                    karte_hir::Pattern::Wildcard { .. } => {
                    }
                    karte_hir::Pattern::Constructor { .. }
                    | karte_hir::Pattern::QualifiedConstructor { .. }
                    | karte_hir::Pattern::Struct { .. } => {
                        let arg_temp = ctx.new_temp();
                        ctx.add_statement(Statement::ConstructorArgExtract {
                            target: arg_temp.clone(),
                            constructor: match_value.clone(),
                            arg_index: i,
                            span: Span::new(0, 0),
                        });
                        let resolved = ctx.resolve_value(&arg_temp);
                        handle_pattern_bindings(ctx, arg_pattern, &resolved)?;
                    }
                    _ => {
                        return Err(vec![
                            format!("Unsupported nested pattern in constructor argument at position {}", i)
                        ]);
                    }
                }
            }
        }
        karte_hir::Pattern::QualifiedConstructor {
            args,
            ..
        } => {
            for (i, arg_pattern) in args.iter().enumerate() {
                match arg_pattern {
                    karte_hir::Pattern::Variable { name, .. } => {
                        let arg_temp = ctx.new_temp();
                        ctx.add_statement(Statement::ConstructorArgExtract {
                            target: arg_temp.clone(),
                            constructor: match_value.clone(),
                            arg_index: i,
                            span: Span::new(0, 0),
                        });
                        ctx.bind_variable(name.clone(), arg_temp, None);
                    }
                    karte_hir::Pattern::Wildcard { .. } => {
                    }
                    karte_hir::Pattern::Constructor { .. }
                    | karte_hir::Pattern::QualifiedConstructor { .. }
                    | karte_hir::Pattern::Struct { .. } => {
                        let arg_temp = ctx.new_temp();
                        ctx.add_statement(Statement::ConstructorArgExtract {
                            target: arg_temp.clone(),
                            constructor: match_value.clone(),
                            arg_index: i,
                            span: Span::new(0, 0),
                        });
                        let resolved = ctx.resolve_value(&arg_temp);
                        handle_pattern_bindings(ctx, arg_pattern, &resolved)?;
                    }
                    _ => {
                        return Err(vec![
                            format!("Unsupported nested pattern in qualified constructor argument at position {}", i)
                        ]);
                    }
                }
            }
        }
        karte_hir::Pattern::Struct { fields, .. } => {
            for field_pattern in fields {
                let field_temp = ctx.new_temp();
                ctx.add_statement(Statement::FieldAccess {
                    target: field_temp.clone(),
                    object: match_value.clone(),
                    field: field_pattern.field.clone(),
                    span: Span::new(0, 0),
                });
                let resolved = ctx.resolve_value(&field_temp);
                handle_pattern_bindings(ctx, field_pattern.pattern.as_ref(), &resolved)?;
            }
        }
        _ => {
            // 其他模式不需要绑定
        }
    }
    Ok(())
}
