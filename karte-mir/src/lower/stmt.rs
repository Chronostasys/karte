/// 语句降低模块
///
/// 本模块负责将HIR语句降低为MIR：
/// - Let语句：变量声明和初始化
/// - Expression语句：表达式语句
/// - Assignment：赋值语句（变量赋值、字段赋值）
/// - TypeDef：类型定义
/// - StructDef：结构体定义
/// - FunctionDef：函数定义
use super::helpers::{handle_destruct_pattern, infer_expr_ownership, lower_expression_to_temp, maybe_retain_for_expr};
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
        karte_hir::Statement::Let { name, pattern, value, span, .. } => {
            // 检查是否为解构模式绑定（如 let Point { x, y } = p）
            if let Some(pat) = pattern {
                let temp_value = lower_expression_to_temp(ctx, value)?;
                let var_value = ctx.resolve_value(&temp_value);
                handle_destruct_pattern(ctx, pat.as_ref(), &var_value)?;
            } else {
            // 检查是否为递归闭包（let f = |...| { ... f ... }）
            // 如果 value 是 Lambda，先预绑定一个占位值，使 lambda 体中可以引用自身名称
            let is_recursive_lambda = matches!(value, karte_hir::Expr::Lambda { .. });

            if is_recursive_lambda {
                // 1. 预绑定占位值，使 lambda lowering 时能通过 lookup_variable 找到 name
                ctx.bind_variable(name.clone(), Value::Number { value: 0, ty: None }, None);

                // 2. 正常 lowering lambda — name 会被当作捕获变量处理
                let temp_value = lower_expression_to_temp(ctx, value)?;

                // 3. Lambda lowering 完成后，name 的绑定已被更新为 Reference(shared_location)
                //    需要把闭包结构体写入 shared_location
                if let Some(binding) = ctx.lookup_variable(name) {
                    let resolved = ctx.resolve_value(&binding.value);
                    if let Value::Reference { value: shared_location, .. } = &resolved {
                        ctx.add_statement(Statement::Store {
                            target: shared_location.as_ref().clone(),
                            value: temp_value.clone(),
                            span: *span,
                        });
                    }
                }

                // 4. 同时需要将外层作用域的 name 绑定更新为闭包值
                //    （而不是 Reference），因为外层直接引用 f 就是闭包结构体
                let var_value = ctx.resolve_value(&temp_value);
                let struct_name = match &var_value {
                    Value::Struct { name, .. } => Some(name.clone()),
                    _ => None,
                };
                let ownership = infer_expr_ownership(ctx, value);
                ctx.update_variable(name, var_value, ownership);
                // 如果有 struct_name 信息需要保留，用 update_variable 可能丢失，检查一下
                if struct_name.is_some() {
                    // update_variable 不更新 struct_name，需要直接修改
                    // 但通常闭包结构体的 struct_name 不需要特殊处理
                }
            } else {
                // 求值表达式
                let temp_value = lower_expression_to_temp(ctx, value)?;
                // 解析临时变量的实际值（如果是函数/闭包）
                let var_value = ctx.resolve_value(&temp_value);

                // 检查值是否是结构体类型，记录结构体名称用于闭包捕获分析
                let struct_name = match &var_value {
                    Value::Struct { name, .. } => Some(name.clone()),
                    _ => match value {
                        karte_hir::Expr::StructLiteral { name, .. } => Some(name.clone()),
                        _ => None,
                    },
                };

                let ownership = infer_expr_ownership(ctx, value);
                if matches!(ownership, Some(OwnershipKind::RefCounted)) {
                    maybe_retain_for_expr(ctx, value, &var_value);
                }
                ctx.bind_variable_with_struct_name(name.clone(), var_value, ownership, struct_name);
            }
            } // end else for pattern check
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
                    field_type: field.field_type.to_string(),
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
            is_pub: _,
            span,
        } => {
            // 如果有返回类型注解，直接使用结构化类型
            if let Some(return_type) = return_type {
                ctx.register_function_return_type(name.clone(), return_type.clone());
            }

            // 保存当前上下文状态
            let old_function_name = ctx.current_function_name.clone();
            let old_block = ctx.current_block;
            let old_scopes = ctx.clone_scopes();

            // 提取参数名和类型
            let param_names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
            let param_types: Vec<Option<karte_hir::types::Type>> = params
                .iter()
                .map(|p| p.type_annotation.clone())
                .collect();

            ctx.start_function(name.clone(), param_names, param_types);

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
/// - 数组下标访问（Index）
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
                if let Value::Reference {
                    value: ref_target, ..
                } = binding.value
                {
                    // 🔧 修复：赋值给被闭包捕获的 struct 变量时的安全指针更新
                    //
                    // shared_var 布局：shared_var → [ptr_to_struct_data]
                    //
                    // 直接 Store(shared_var, value_temp) 在 LIR 中会检测到 value_temp
                    // 携带 struct layout → 逐字段复制到 shared_var 的 8 字节 slot
                    // → 覆盖指针 → 后续 Dereference SIGSEGV。
                    //
                    // 修复策略：区分 value 来源。
                    // 1. 函数调用返回值（Expr::Call/MethodCall）：
                    //    value_temp 在 LIR 中是 HeapAlloc 指针（无 struct layout），
                    //    Store 不做逐字段复制 → 直接 Store64 → 正确。
                    // 2. 其他表达式（struct 字面量、match、if-else 等）：
                    //    value_temp 在 LIR 中携带 struct layout → 需要包装：
                    //    HeapAlloc 新 slot → Store(struct_data) → Store(shared_var, ptr)
                    let is_call = matches!(value, Expr::FunctionCall { .. });
                    if !is_call && binding.struct_name.is_some() {
                        // struct 类型非函数调用：需要 HeapAlloc 包装
                        let struct_name = binding.struct_name.as_ref().unwrap().clone();
                        let struct_size = ctx.program.get_struct_type(&struct_name)
                            .map(|st| st.fields.len() * 8)
                            .unwrap_or(8);
                        let new_alloc = ctx.new_temp();
                        ctx.add_statement(Statement::HeapAlloc {
                            target: new_alloc.clone(),
                            size: struct_size,
                            object_type: "struct_copy".to_string(),
                            span,
                        });
                        ctx.add_statement(Statement::Store {
                            target: new_alloc.clone(),
                            value: value_temp,
                            span,
                        });
                        ctx.add_statement(Statement::Store {
                            target: *ref_target,
                            value: new_alloc,
                            span,
                        });
                    } else {
                        // 函数调用或非 struct 类型：直接 Store
                        ctx.add_statement(Statement::Store {
                            target: *ref_target,
                            value: value_temp,
                            span,
                        });
                    }
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
        Expr::Constructor { name, args, .. } if args.is_empty() => {
            // 大写字母开头的变量被 parser 误解析为零参数 Constructor
            // 在赋值目标位置应视为普通变量
            if let Some(binding) = ctx.lookup_variable(name).cloned() {
                if let Value::Reference {
                    value: ref_target, ..
                } = binding.value
                {
                    let is_call = matches!(value, Expr::FunctionCall { .. });
                    if !is_call && binding.struct_name.is_some() {
                        let struct_name = binding.struct_name.as_ref().unwrap().clone();
                        let struct_size = ctx.program.get_struct_type(&struct_name)
                            .map(|st| st.fields.len() * 8)
                            .unwrap_or(8);
                        let new_alloc = ctx.new_temp();
                        ctx.add_statement(Statement::HeapAlloc {
                            target: new_alloc.clone(),
                            size: struct_size,
                            object_type: "struct_copy".to_string(),
                            span,
                        });
                        ctx.add_statement(Statement::Store {
                            target: new_alloc.clone(),
                            value: value_temp,
                            span,
                        });
                        ctx.add_statement(Statement::Store {
                            target: *ref_target,
                            value: new_alloc,
                            span,
                        });
                    } else {
                        ctx.add_statement(Statement::Store {
                            target: *ref_target,
                            value: value_temp,
                            span,
                        });
                    }
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
            // 对 object 表达式求值，得到结构体（或嵌套结构体）的基地址
            // 支持单级赋值 (o.val = 42) 和嵌套赋值 (o.inner.val = 42)
            let object_value = if let Expr::Identifier { name, .. } = object.as_ref() {
                // 简单变量引用：需要处理闭包捕获导致的 Reference 包装
                if let Some(binding) = ctx.lookup_variable(name).cloned() {
                    match &binding.value {
                        Value::Reference { value: ref_target, .. } => {
                            // 变量被闭包捕获：先解引用得到结构体地址
                            let derefed = ctx.new_temp();
                            ctx.add_statement(Statement::Dereference {
                                target: derefed.clone(),
                                reference: *ref_target.clone(),
                                span,
                            });
                            derefed
                        }
                        _ => binding.value.clone(),
                    }
                } else {
                    return Err(vec![format!(
                        "Undefined variable in field assignment: {}",
                        name
                    )]);
                }
            } else {
                // 嵌套字段赋值：递归对 object 表达式求值得到中间结构体地址
                // 例如 o.inner.val = 42 中，先求值 o.inner 得到 inner 的地址
                lower_expression_to_temp(ctx, object)?
            };

            ctx.add_statement(Statement::FieldAssign {
                object: object_value,
                field: field.clone(),
                value: value_temp,
                span,
            });
        }
        Expr::Index { array, index, .. } => {
            // 数组下标赋值：计算 element_ptr = array_base + 8 + index * element_size
            let array_value = lower_expression_to_temp(ctx, array)?;
            let index_value = lower_expression_to_temp(ctx, index)?;

            // 从数组类型获取元素类型，计算元素大小
            let el_type = ctx.get_expr_type(array);
            let element_size = if let karte_hir::Type::Array { element } = &el_type {
                element.byte_size().max(8)
            } else {
                8
            };

            // scaled_index = index * element_size
            let scaled_index = ctx.new_temp();
            ctx.add_statement(Statement::BinaryOp {
                target: scaled_index.clone(),
                left: index_value,
                op: crate::BinaryOperator::Multiply,
                right: Value::Number { value: element_size as i64, ty: None },
                operand_type: None,
                span,
            });

            // data_base = array + 8（跳过长度头）
            let data_base = ctx.new_temp();
            ctx.add_statement(Statement::BinaryOp {
                target: data_base.clone(),
                left: array_value,
                op: crate::BinaryOperator::Add,
                right: Value::Number { value: 8, ty: None },
                operand_type: None,
                span,
            });

            // element_ptr = data_base + scaled_index
            let element_ptr = ctx.new_temp();
            ctx.add_statement(Statement::BinaryOp {
                target: element_ptr.clone(),
                left: data_base,
                op: crate::BinaryOperator::Add,
                right: scaled_index,
                operand_type: None,
                span,
            });

            // Store value to element_ptr
            ctx.add_statement(Statement::Store {
                target: element_ptr,
                value: value_temp,
                span,
            });
        }
        _ => {
            return Err(vec!["Invalid assignment target in MIR lowering".to_string()]);
        }
    }

    Ok(())
}
