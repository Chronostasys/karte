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
        HirBinaryOp::Equal => MirBinaryOp::Equal,
        HirBinaryOp::GreaterEqual => MirBinaryOp::GreaterEqual,
        HirBinaryOp::LessEqual => MirBinaryOp::LessEqual,
        HirBinaryOp::Greater => MirBinaryOp::GreaterThan,
        HirBinaryOp::Less => MirBinaryOp::LessThan,
        HirBinaryOp::LogicalAnd => MirBinaryOp::And,
        HirBinaryOp::LogicalOr => MirBinaryOp::Or,
    }
}

/// 转换HIR一元运算符到MIR
pub(crate) fn convert_unary_op(op: &HirUnaryOp) -> MirUnaryOp {
    match op {
        HirUnaryOp::Plus => MirUnaryOp::Plus,
        HirUnaryOp::Minus => MirUnaryOp::Minus,
        HirUnaryOp::LogicalNot => MirUnaryOp::Not,
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
fn collect_vars_recursive(expr: &Expr, vars: &mut Vec<String>) {
    match expr {
        Expr::Identifier { name, .. } => {
            vars.push(name.clone());
        }
        Expr::BinaryOp { left, right, .. } => {
            collect_vars_recursive(left, vars);
            collect_vars_recursive(right, vars);
        }
        Expr::UnaryOp { operand, .. } => {
            collect_vars_recursive(operand, vars);
        }
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
        Expr::Lambda { params, body, .. } => {
            // 对于lambda，只收集真正的外部捕获变量，排除lambda参数
            let mut lambda_vars = Vec::new();
            collect_vars_recursive(body, &mut lambda_vars);

            // 过滤掉lambda参数
            let param_names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
            for var in lambda_vars {
                if !param_names.contains(&var) {
                    vars.push(var);
                }
            }
        }
        Expr::FunctionCall { function, args, .. } => {
            collect_vars_recursive(function, vars);
            for arg in args {
                collect_vars_recursive(arg, vars);
            }
        }
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
        Expr::Statement { stmt, .. } => {
            collect_vars_in_statement(stmt, vars);
        }
        Expr::Assignment { target, value, .. } => {
            collect_vars_recursive(target, vars);
            collect_vars_recursive(value, vars);
        }
        // 其他表达式类型不包含变量引用
        _ => {}
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
pub(crate) fn infer_expr_ownership(
    ctx: &LoweringContext,
    expr: &Expr,
) -> Option<OwnershipKind> {
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

/// 转换HIR模式到MIR模式
pub(crate) fn convert_pattern(pattern: &karte_hir::Pattern) -> Result<Pattern, Vec<String>> {
    match pattern {
        karte_hir::Pattern::Wildcard { .. } => Ok(Pattern::Wildcard),
        karte_hir::Pattern::Variable { name, .. } => Ok(Pattern::Variable { name: name.clone() }),
        karte_hir::Pattern::Constructor { name, arg, .. } => {
            let mir_arg = if let Some(arg) = arg {
                // 对于构造器模式的参数，我们只支持变量绑定
                match arg.as_ref() {
                    karte_hir::Pattern::Variable { name, .. } => Some(name.clone()),
                    _ => {
                        return Err(vec![
                            "Only variable patterns are supported in constructor arguments"
                                .to_string(),
                        ])
                    }
                }
            } else {
                None
            };
            Ok(Pattern::Constructor {
                name: name.clone(),
                arg: mir_arg,
            })
        }
        karte_hir::Pattern::Number { value, .. } => Ok(Pattern::Number { value: *value }),
        karte_hir::Pattern::Boolean { value, .. } => Ok(Pattern::Boolean { value: *value }),
        karte_hir::Pattern::QualifiedConstructor {
            constructor_name,
            arg,
            ..
        } => {
            let mir_arg = if let Some(arg) = arg {
                match arg.as_ref() {
                    karte_hir::Pattern::Variable { name, .. } => Some(name.clone()),
                    _ => {
                        return Err(vec![
                        "Only variable patterns are supported in qualified constructor arguments"
                            .to_string(),
                    ])
                    }
                }
            } else {
                None
            };
            // 对于限定构造器，我们使用构造器名称
            Ok(Pattern::Constructor {
                name: constructor_name.clone(),
                arg: mir_arg,
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
            arg: Some(arg_pattern),
            ..
        } => {
            // 构造器模式带参数：需要提取构造器的参数
            if let karte_hir::Pattern::Variable { name, .. } = arg_pattern.as_ref() {
                // 创建一个临时变量来存储提取的参数
                let arg_temp = ctx.new_temp();

                // 添加一个特殊的语句来从构造器中提取参数
                // 这个语句告诉运行时从match_value构造器中提取参数
                ctx.add_statement(Statement::ConstructorArgExtract {
                    target: arg_temp.clone(),
                    constructor: match_value.clone(),
                    arg_index: 0, // 第一个参数
                    span: Span::new(0, 0),
                });

                ctx.bind_variable(name.clone(), arg_temp, None);
            }
        }
        karte_hir::Pattern::QualifiedConstructor {
            arg: Some(arg_pattern),
            ..
        } => {
            // 限定构造器模式带参数
            if let karte_hir::Pattern::Variable { name, .. } = arg_pattern.as_ref() {
                let arg_temp = ctx.new_temp();

                // 添加构造器参数提取语句
                ctx.add_statement(Statement::ConstructorArgExtract {
                    target: arg_temp.clone(),
                    constructor: match_value.clone(),
                    arg_index: 0,
                    span: Span::new(0, 0),
                });

                ctx.bind_variable(name.clone(), arg_temp, None);
            }
        }
        _ => {
            // 其他模式不需要绑定
        }
    }
    Ok(())
}
