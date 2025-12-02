/// 语句降低模块
///
/// 本模块负责将HIR语句降低为MIR：
/// - Let语句：变量声明和初始化
/// - Expression语句：表达式语句
/// - Assignment：赋值语句（变量赋值、字段赋值）
/// - TypeDef：类型定义
/// - StructDef：结构体定义
/// - FunctionDef：函数定义
use super::helpers::{infer_expr_ownership, lower_expression_to_temp, maybe_retain_for_expr};
use super::types::LoweringContext;
use crate::{MirStructField, MirStructType, Statement, Terminator, Value};
use karte_common::memory::OwnershipKind;
use karte_diagnostics::Span;
use karte_hir::Expr;

/// 降低单个语句
pub(crate) fn lower_statement(
    ctx: &mut LoweringContext,
    stmt: &karte_hir::Statement,
) -> Result<(), Vec<String>> {
    match stmt {
        karte_hir::Statement::Let { name, value, .. } => {
            // 求值表达式
            let temp_value = lower_expression_to_temp(ctx, value)?;
            // 解析临时变量的实际值（如果是函数/闭包）
            let var_value = ctx.resolve_value(&temp_value);

            let ownership = infer_expr_ownership(ctx, value);
            if matches!(ownership, Some(OwnershipKind::RefCounted)) {
                maybe_retain_for_expr(ctx, value, &var_value);
            }
            ctx.bind_variable(name.clone(), var_value, ownership);
        }
        karte_hir::Statement::Expression { expr, .. } => {
            // 结果被丢弃
            let temp = ctx.new_temp();
            super::expr::lower_expression(ctx, expr, &temp)?;
        }
        karte_hir::Statement::TypeDef { .. } => {
            // 类型定义在编译期处理，MIR中无需体现
        }
        karte_hir::Statement::StructDef { name, fields, .. } => {
            // 收集结构体定义信息，传递给MIR
            let mir_fields: Vec<MirStructField> = fields
                .iter()
                .map(|field| MirStructField {
                    name: field.name.clone(),
                    field_type: field.field_type.clone(),
                })
                .collect();

            let mir_struct_type = MirStructType {
                name: name.clone(),
                fields: mir_fields,
            };

            ctx.program.add_struct_type(mir_struct_type);
        }
        karte_hir::Statement::Assignment {
            target,
            value,
            span,
        } => {
            handle_assignment(ctx, target, value, *span)?;
        }
        karte_hir::Statement::FunctionDef {
            name,
            params,
            body,
            return_type,
            span,
        } => {
            // 如果有返回类型注解，解析并注册
            if let Some(return_type_str) = return_type {
                if let Some(parsed_type) = ctx.parse_type_annotation(return_type_str) {
                    ctx.register_function_return_type(name.clone(), parsed_type);
                }
            }

            // 保存当前上下文状态
            let old_function_name = ctx.current_function_name.clone();
            let old_block = ctx.current_block;
            let old_scopes = ctx.clone_scopes();

            // 提取参数名
            let param_names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();

            ctx.start_function(name.clone(), param_names);

            // Lower 函数体
            match lower_expression_to_temp(ctx, body) {
                Ok(return_value) => {
                    // 添加返回指令
                    if let Some(block_id) = ctx.current_block {
                        if let Some(block) =
                            ctx.current_function_mut().basic_blocks.get_mut(&block_id)
                        {
                            if block.terminator.is_none() {
                                block.terminator = Some(Terminator::Return {
                                    value: Some(return_value),
                                    span: *span,
                                });
                            }
                        }
                    }
                }
                Err(e) => {
                    ctx.errors.push(format!(
                        "Error lowering function body for {}: {}",
                        name, e[0]
                    ));
                }
            }

            ctx.finish_function();

            // 恢复上下文
            ctx.current_function_name = old_function_name;
            ctx.current_block = old_block;
            ctx.restore_scopes(old_scopes);
        }
    }
    Ok(())
}

/// 处理赋值语句
///
/// 支持以下赋值目标：
/// - 普通变量（Identifier）
/// - 字段访问（FieldAccess）
pub(crate) fn handle_assignment(
    ctx: &mut LoweringContext,
    target: &Expr,
    value: &Expr,
    span: Span,
) -> Result<(), Vec<String>> {
    let value_temp = lower_expression_to_temp(ctx, value)?;
    let ownership = infer_expr_ownership(ctx, value);
    if matches!(ownership, Some(OwnershipKind::RefCounted)) {
        maybe_retain_for_expr(ctx, value, &value_temp);
    }

    match target {
        Expr::Identifier { name, .. } => {
            if let Some(binding) = ctx.lookup_variable(name).cloned() {
                if let Value::Reference { value: ref_target, .. } = binding.value {
                    ctx.add_statement(Statement::Store {
                        target: *ref_target,
                        value: value_temp,
                        span,
                    });
                } else {
                    if let Some(old_binding) =
                        ctx.update_variable(name, value_temp.clone(), ownership)
                    {
                        ctx.release_binding(&old_binding, span);
                    }
                }
            } else {
                ctx.bind_variable(name.clone(), value_temp, ownership);
            }
        }
        Expr::FieldAccess { object, field, .. } => {
            if let Expr::Identifier { name, .. } = object.as_ref() {
                if let Some(binding) = ctx.lookup_variable(name).cloned() {
                    ctx.add_statement(Statement::FieldAssign {
                        object: binding.value,
                        field: field.clone(),
                        value: value_temp,
                        span,
                    });
                } else {
                    return Err(vec![format!(
                        "Undefined variable in field assignment: {}",
                        name
                    )]);
                }
            } else {
                return Err(vec![
                    "Complex field assignment not yet supported in MIR".to_string()
                ]);
            }
        }
        _ => {
            return Err(vec!["Invalid assignment target in MIR lowering".to_string()]);
        }
    }

    Ok(())
}
