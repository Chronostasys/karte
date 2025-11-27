/// 表达式降低模块
///
/// 本模块负责将HIR表达式降低为MIR，是lowering过程的核心部分。
/// 支持的表达式类型包括：
/// - 基础值：Number, Unit, Boolean, Identifier
/// - 运算：BinaryOp, UnaryOp
/// - 控制流：If, While, Block
/// - 函数：Lambda, FunctionCall
/// - 代数效应：EffectPerform, EffectResume, EffectHandle
/// - 模式匹配：Match, Constructor, QualifiedConstructor
/// - 结构体：StructLiteral, FieldAccess
/// - 数组：ArrayLiteral, Index, ArrayLen
/// - 引用：Reference, Dereference
/// - 堆操作：HeapAllocate, HeapFree, Retain, Release
/// - 赋值：Assignment

use super::helpers::{
    collect_referenced_variables, convert_binary_op, convert_unary_op, convert_pattern,
    expr_creates_new_ref, handle_pattern_bindings, infer_expr_ownership,
    infer_heap_layout_from_expr, lower_expression_to_temp, maybe_retain_for_escape,
    unknown_heap_layout,
};
use super::stmt::{handle_assignment, lower_statement};
use super::types::LoweringContext;
use crate::{
    BinaryOperator as MirBinaryOp, EscapeState, HeapLayout, MatchArm, MirFunction, Statement,
    Terminator, TempId, Value,
};
use karte_common::memory::OwnershipKind;
use karte_hir::Expr;

/// 降低单个表达式并将其结果存入 destination
pub(crate) fn lower_expression(
    ctx: &mut LoweringContext,
    expr: &Expr,
    destination: &Value,
) -> Result<(), Vec<String>> {
    let span = expr.span();
    match expr {
        Expr::Number { value, .. } => {
            ctx.add_statement(Statement::Assign {
                target: destination.clone(),
                source: Value::Number { value: *value },
                span,
            });
        }

        Expr::Unit { .. } => {
            ctx.add_statement(Statement::Assign {
                target: destination.clone(),
                source: Value::Unit,
                span,
            });
        }

        Expr::Boolean { value, .. } => {
            // 使用新的Boolean值表示，用于简化逻辑操作符处理
            ctx.add_statement(Statement::Assign {
                target: destination.clone(),
                source: Value::Boolean { value: *value },
                span,
            });
        }

        Expr::Identifier { name, .. } => {
            if let Some(binding) = ctx.lookup_variable(name) {
                match &binding.value {
                    Value::Reference { value: ref_target } => {
                        ctx.add_statement(Statement::Dereference {
                            target: destination.clone(),
                            reference: *ref_target.clone(),
                            span,
                        });
                    }
                    _ => {
                        ctx.add_statement(Statement::Assign {
                            target: destination.clone(),
                            source: binding.value.clone(),
                            span,
                        });
                    }
                }
            } else if ctx.is_known_function(name) {
                // 如果是函数名，返回函数值
                ctx.add_statement(Statement::Assign {
                    target: destination.clone(),
                    source: Value::Function { name: name.clone() },
                    span,
                });
            } else {
                ctx.errors.push(format!("Undefined variable: {}", name));
                return Err(ctx.errors.clone());
            }
        }

        Expr::ModuleSymbolAccess {
            module_path,
            symbol,
            ..
        } => {
            let canonical = ctx.canonical_module_symbol(module_path, symbol);
            ctx.add_statement(Statement::Assign {
                target: destination.clone(),
                source: Value::Function { name: canonical },
                span,
            });
        }

        Expr::BinaryOp {
            left, op, right, ..
        } => {
            let left_val = lower_expression_to_temp(ctx, left)?;
            let right_val = lower_expression_to_temp(ctx, right)?;

            ctx.add_statement(Statement::BinaryOp {
                target: destination.clone(),
                left: left_val,
                op: convert_binary_op(op),
                right: right_val,
                span,
            });
        }

        Expr::UnaryOp { op, operand, .. } => {
            let operand_val = lower_expression_to_temp(ctx, operand)?;

            ctx.add_statement(Statement::UnaryOp {
                target: destination.clone(),
                op: convert_unary_op(op),
                operand: operand_val,
                span,
            });
        }

        Expr::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            let condition_val = lower_expression_to_temp(ctx, condition)?;

            let then_block = ctx.new_block();
            let else_block = ctx.new_block();
            let merge_block = ctx.new_block();

            ctx.set_terminator(Terminator::Branch {
                condition: condition_val,
                then_block,
                else_block,
                span,
            });

            // then 分支
            ctx.set_current_block(then_block);
            lower_expression(ctx, then_branch, destination)?;
            ctx.set_terminator(Terminator::Goto {
                target: merge_block,
                span: then_branch.span(),
            });

            // else 分支
            if let Some(else_branch) = else_branch {
                ctx.set_current_block(else_block);
                lower_expression(ctx, else_branch, destination)?;
                ctx.set_terminator(Terminator::Goto {
                    target: merge_block,
                    span: else_branch.span(),
                });
            } else {
                // 没有else分支时，else路径应该返回Unit
                ctx.set_current_block(else_block);
                ctx.add_statement(Statement::Assign {
                    target: destination.clone(),
                    source: Value::Unit,
                    span,
                });
                ctx.set_terminator(Terminator::Goto {
                    target: merge_block,
                    span,
                });
            }

            ctx.set_current_block(merge_block);
        }

        Expr::While {
            condition,
            body,
            span,
        } => {
            let loop_head = ctx.new_block();
            let loop_body = ctx.new_block();
            let loop_exit = ctx.new_block();

            // Jump to loop head
            ctx.set_terminator(Terminator::Goto {
                target: loop_head,
                span: *span,
            });

            // In loop head, check condition
            ctx.set_current_block(loop_head);
            let cond_val = lower_expression_to_temp(ctx, condition)?;
            ctx.set_terminator(Terminator::Branch {
                condition: cond_val,
                then_block: loop_body,
                else_block: loop_exit,
                span: condition.span(),
            });

            // In loop body, execute and jump back to head
            ctx.set_current_block(loop_body);
            let temp_body_result = ctx.new_temp();
            lower_expression(ctx, body, &temp_body_result)?;
            ctx.set_terminator(Terminator::Goto {
                target: loop_head,
                span: body.span(),
            });

            // Continue from exit block
            ctx.set_current_block(loop_exit);
            // while loops evaluate to Unit
            ctx.add_statement(Statement::Assign {
                target: destination.clone(),
                source: Value::Unit,
                span: *span,
            });
        }

        Expr::Lambda { params, body, .. } => {
            lower_lambda_expression(ctx, params, body, destination, span)?;
        }

        Expr::FunctionCall { function, args, .. } => {
            lower_function_call(ctx, function, args, destination, span)?;
        }

        // ===== 代数效应 =====
        Expr::EffectPerform { tag, payload, .. } => {
            let tag_val = lower_expression_to_temp(ctx, tag)?;
            let payload_val = lower_expression_to_temp(ctx, payload)?;
            // 在MIR里生成占位语句，最终在 MIR->LIR 时处理为 LIR 伪指令
            ctx.add_statement(Statement::EffectPerform {
                tag: tag_val,
                payload: payload_val,
                target: Some(destination.clone()),
                span,
            });
        }
        Expr::EffectResume { value, .. } => {
            let v = lower_expression_to_temp(ctx, value)?;
            ctx.add_statement(Statement::EffectResume { value: v, span });
            // resume 表达式结果Unknown，这里置Unit占位
            ctx.add_statement(Statement::Assign {
                target: destination.clone(),
                source: Value::Unit,
                span,
            });
        }
        Expr::EffectHandle {
            tag,
            param,
            handler,
            body,
            ..
        } => {
            // 1) 创建 handler 所在的基本块（与当前函数同体，非独立函数）
            let handler_block = ctx.new_block();

            // 2) push handler（记录tag与handler入口块）
            let tag_val = lower_expression_to_temp(ctx, tag)?;
            ctx.add_statement(Statement::EffectHandlerPush {
                tag: tag_val,
                handler_block,
                param_name: param.clone(),
                span,
            });

            // 3) lower body（handler 安装期间生效）
            lower_expression(ctx, body, destination)?;

            // 4) pop handler
            ctx.add_statement(Statement::EffectHandlerPop { span });

            // 5) 切换到 handler_block，绑定形参名到变量环境，lower handler 代码
            let current_block = ctx.current_block;
            ctx.set_current_block(handler_block);
            // 在handler块内声明参数变量
            ctx.bind_variable(
                param.clone(),
                Value::Variable {
                    name: param.clone(),
                },
                None,
            );

            // handler 表达式不需要结果，因为它通常通过 resume 返回
            let dummy_temp = Value::Temp { id: TempId(0) };
            lower_expression(ctx, handler, &dummy_temp)?;

            // 处理器里通常通过 resume 返回；若未 resume，这里不强制添加跳转
            ctx.set_current_block(current_block.unwrap());
        }

        Expr::Block {
            statements,
            final_expr,
            span,
        } => {
            ctx.enter_scope();

            // Hoisting Pass: 预先注册当前块中的函数定义，支持相互递归
            for stmt in statements {
                if let karte_hir::Statement::FunctionDef { name, params, .. } = stmt {
                    // 仅当函数尚未定义时注册
                    if !ctx.program.functions.contains_key(name) {
                        let param_names: Vec<String> =
                            params.iter().map(|p| p.name.clone()).collect();
                        let function = MirFunction::new(name.clone(), param_names);
                        ctx.program.add_function(function);
                    }
                }
            }

            for stmt in statements {
                lower_statement(ctx, stmt)?;
            }
            if let Some(final_expr) = final_expr {
                lower_expression(ctx, final_expr, destination)?;
                maybe_retain_for_escape(ctx, final_expr, destination);
            } else {
                ctx.add_statement(Statement::Assign {
                    target: destination.clone(),
                    source: Value::Unit,
                    span: *span,
                });
            }
            ctx.exit_scope(*span);
        }

        Expr::Statement { stmt, .. } => {
            lower_statement(ctx, stmt)?;
            // Statements used as expressions evaluate to Unit
            ctx.add_statement(Statement::Assign {
                target: destination.clone(),
                source: Value::Unit,
                span,
            });
        }

        Expr::Constructor { name, arg, .. } => {
            let constructor_value = if let Some(arg) = arg {
                let arg_val = lower_expression_to_temp(ctx, arg)?;
                Value::Constructor {
                    name: name.clone(),
                    arg: Some(Box::new(arg_val)),
                }
            } else {
                Value::Constructor {
                    name: name.clone(),
                    arg: None,
                }
            };

            ctx.add_statement(Statement::Assign {
                target: destination.clone(),
                source: constructor_value,
                span,
            });
        }

        Expr::QualifiedConstructor {
            type_name,
            constructor_name,
            arg,
            ..
        } => {
            let constructor_value = if let Some(arg) = arg {
                let arg_val = lower_expression_to_temp(ctx, arg)?;
                Value::QualifiedConstructor {
                    type_name: type_name.clone(),
                    constructor_name: constructor_name.clone(),
                    arg: Some(Box::new(arg_val)),
                }
            } else {
                Value::QualifiedConstructor {
                    type_name: type_name.clone(),
                    constructor_name: constructor_name.clone(),
                    arg: None,
                }
            };

            ctx.add_statement(Statement::Assign {
                target: destination.clone(),
                source: constructor_value,
                span,
            });
        }

        Expr::Match { expr, arms, .. } => {
            // 1. 计算匹配表达式的值
            let match_value = lower_expression_to_temp(ctx, expr)?;

            // 2. 为每个匹配分支创建基本块
            let mut mir_arms = Vec::new();
            let mut arm_blocks = Vec::new();

            for arm in arms {
                let arm_block = ctx.new_block();
                arm_blocks.push(arm_block);

                // 转换HIR模式到MIR模式
                let mir_pattern = convert_pattern(&arm.pattern)?;
                mir_arms.push(MatchArm {
                    pattern: mir_pattern,
                    target: arm_block,
                });
            }

            // 3. 创建合并块（所有分支的结果汇聚到这里）
            let merge_block = ctx.new_block();

            // 4. 设置当前块的终结器为Match
            ctx.set_terminator(Terminator::Match {
                value: match_value.clone(),
                arms: mir_arms,
                default: None, // 暂时不支持默认分支
                span,
            });

            // 5. 为每个分支生成代码
            for (i, arm) in arms.iter().enumerate() {
                let arm_block = arm_blocks[i];
                ctx.set_current_block(arm_block);

                // 处理模式绑定（如果有的话）
                ctx.enter_scope();
                handle_pattern_bindings(ctx, &arm.pattern, &match_value)?;

                // 生成分支体的代码
                lower_expression(ctx, &arm.body, destination)?;
                ctx.exit_scope(arm.span);

                // 跳转到合并块
                ctx.set_terminator(Terminator::Goto {
                    target: merge_block,
                    span: arm.span,
                });
            }

            // 6. 切换到合并块
            ctx.set_current_block(merge_block);
        }

        Expr::StructLiteral { name, fields, span } => {
            // 1. 计算所有字段的值
            let mut mir_fields = std::collections::BTreeMap::new();
            for field in fields {
                let field_value = lower_expression_to_temp(ctx, &field.value)?;
                mir_fields.insert(field.name.clone(), field_value);
            }

            // 2. 创建结构体值并赋值给目标
            let struct_value = Value::Struct {
                name: name.clone(),
                fields: mir_fields,
            };

            ctx.add_statement(Statement::Assign {
                target: destination.clone(),
                source: struct_value,
                span: *span,
            });
        }

        Expr::FieldAccess {
            object,
            field,
            span,
        } => {
            // 1. 计算对象表达式的值
            let object_value = lower_expression_to_temp(ctx, object)?;

            // 2. 创建字段访问语句
            ctx.add_statement(Statement::FieldAccess {
                target: destination.clone(),
                object: object_value,
                field: field.clone(),
                span: *span,
            });
        }

        Expr::ArrayLiteral { elements, span } => {
            let slot_count = elements.len() + 1; // length slot + elements
            let layout = HeapLayout {
                type_id: format!("array:{}", elements.len()),
                size: slot_count.max(1) * 8,
                align: 8,
                mutable: true,
                escape: EscapeState::Global,
                ownership: OwnershipKind::Manual,
            };

            let array_ptr = ctx.new_temp();
            ctx.add_statement(Statement::Allocate {
                target: array_ptr.clone(),
                layout,
                span: *span,
            });

            // 写入长度信息
            ctx.add_statement(Statement::Store {
                target: array_ptr.clone(),
                value: Value::Number {
                    value: elements.len() as i64,
                },
                span: *span,
            });

            for (idx, element) in elements.iter().enumerate() {
                let element_value = lower_expression_to_temp(ctx, element)?;
                let element_ptr = ctx.new_temp();
                ctx.add_statement(Statement::BinaryOp {
                    target: element_ptr.clone(),
                    left: array_ptr.clone(),
                    op: MirBinaryOp::Add,
                    right: Value::Number {
                        value: ((idx + 1) * 8) as i64,
                    },
                    span: *span,
                });
                ctx.add_statement(Statement::Store {
                    target: element_ptr,
                    value: element_value,
                    span: *span,
                });
            }

            ctx.add_statement(Statement::Assign {
                target: destination.clone(),
                source: array_ptr,
                span: *span,
            });
        }

        Expr::Index { array, index, span } => {
            let array_value = lower_expression_to_temp(ctx, array)?;
            let index_value = lower_expression_to_temp(ctx, index)?;

            let scaled_index = ctx.new_temp();
            ctx.add_statement(Statement::BinaryOp {
                target: scaled_index.clone(),
                left: index_value,
                op: MirBinaryOp::Multiply,
                right: Value::Number { value: 8 },
                span: *span,
            });

            let data_base = ctx.new_temp();
            ctx.add_statement(Statement::BinaryOp {
                target: data_base.clone(),
                left: array_value.clone(),
                op: MirBinaryOp::Add,
                right: Value::Number { value: 8 },
                span: *span,
            });

            let element_ptr = ctx.new_temp();
            ctx.add_statement(Statement::BinaryOp {
                target: element_ptr.clone(),
                left: data_base,
                op: MirBinaryOp::Add,
                right: scaled_index,
                span: *span,
            });

            ctx.add_statement(Statement::Dereference {
                target: destination.clone(),
                reference: element_ptr,
                span: *span,
            });
        }

        Expr::ArrayLen { array, span } => {
            let array_value = lower_expression_to_temp(ctx, array)?;
            ctx.add_statement(Statement::Dereference {
                target: destination.clone(),
                reference: array_value,
                span: *span,
            });
        }

        Expr::Reference { expr, span } => {
            // 1. 计算被引用表达式的值
            let referenced_value = lower_expression_to_temp(ctx, expr)?;

            // 2. 创建引用值并赋值给目标
            let reference_value = Value::Reference {
                value: Box::new(referenced_value),
            };

            ctx.add_statement(Statement::Assign {
                target: destination.clone(),
                source: reference_value,
                span: *span,
            });
        }

        Expr::Dereference { expr, span } => {
            // 1. 计算被解引用表达式的值
            let reference_value = lower_expression_to_temp(ctx, expr)?;

            // 2. 添加解引用语句
            ctx.add_statement(Statement::Dereference {
                target: destination.clone(),
                reference: reference_value,
                span: *span,
            });
        }

        Expr::HeapAllocate {
            value,
            ownership,
            span,
        } => {
            let mut layout = infer_heap_layout_from_expr(value);
            layout.ownership = *ownership;
            let heap_ptr = ctx.new_temp();

            ctx.add_statement(Statement::Allocate {
                target: heap_ptr.clone(),
                layout: layout.clone(),
                span: *span,
            });

            let stored_value = lower_expression_to_temp(ctx, value)?;
            ctx.add_statement(Statement::Store {
                target: heap_ptr.clone(),
                value: stored_value,
                span: *span,
            });

            ctx.add_statement(Statement::Assign {
                target: destination.clone(),
                source: heap_ptr,
                span: *span,
            });
        }

        Expr::HeapFree { pointer, span } => {
            let pointer_value = lower_expression_to_temp(ctx, pointer)?;
            ctx.add_statement(Statement::Deallocate {
                pointer: pointer_value,
                layout: unknown_heap_layout(),
                span: *span,
            });

            ctx.add_statement(Statement::Assign {
                target: destination.clone(),
                source: Value::Unit,
                span: *span,
            });
        }

        Expr::Retain { pointer, span } => {
            let pointer_value = lower_expression_to_temp(ctx, pointer)?;
            ctx.add_statement(Statement::Retain {
                value: pointer_value,
                span: *span,
            });
            ctx.add_statement(Statement::Assign {
                target: destination.clone(),
                source: Value::Unit,
                span: *span,
            });
        }

        Expr::Release { pointer, span } => {
            let pointer_value = lower_expression_to_temp(ctx, pointer)?;
            ctx.add_statement(Statement::Release {
                value: pointer_value,
                span: *span,
            });
            ctx.add_statement(Statement::Assign {
                target: destination.clone(),
                source: Value::Unit,
                span: *span,
            });
        }

        Expr::Assignment {
            target,
            value,
            span,
        } => {
            handle_assignment(ctx, target, value, *span)?;
            ctx.add_statement(Statement::Assign {
                target: destination.clone(),
                source: Value::Unit,
                span: *span,
            });
        }
    }
    Ok(())
}

/// 降低Lambda表达式
///
/// Lambda表达式被转换为：
/// 1. 闭包结构体（包含function_ptr和env_ptr）
/// 2. 独立的MIR函数（带有__env参数）
fn lower_lambda_expression(
    ctx: &mut LoweringContext,
    params: &[karte_hir::Parameter],
    body: &Expr,
    destination: &Value,
    span: karte_diagnostics::Span,
) -> Result<(), Vec<String>> {
    // 1. 分析Lambda体中使用的自由变量（闭包捕获）
    let mut free_vars = Vec::new();
    let mut captured_var_locations = Vec::new();

    // 收集Lambda体中引用的所有变量
    let referenced_vars = collect_referenced_variables(body);
    let param_names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();

    // 找出不是参数的变量（即需要捕获的自由变量）
    for var_name in referenced_vars {
        if !param_names.contains(&var_name) {
            if let Some(binding) = ctx.lookup_variable(&var_name).cloned() {
                free_vars.push(var_name.clone());

                let shared_location = ctx.new_temp();
                ctx.add_statement(Statement::HeapAlloc {
                    target: shared_location.clone(),
                    size: 8,
                    object_type: "shared_var".to_string(),
                    span,
                });

                ctx.add_statement(Statement::Store {
                    target: shared_location.clone(),
                    value: binding.value.clone(),
                    span,
                });

                ctx.update_variable(
                    &var_name,
                    Value::Reference {
                        value: Box::new(shared_location.clone()),
                    },
                    None,
                );

                captured_var_locations.push(shared_location);
            }
        }
    }

    // 2. 生成唯一的函数名
    let lambda_name = format!("lambda${}", ctx.lambda_counter);
    ctx.lambda_counter += 1;

    // 3. 创建闭包结构体
    if captured_var_locations.is_empty() {
        // 无捕获变量，创建简单的函数闭包
        let mut closure_fields = std::collections::BTreeMap::new();
        closure_fields.insert(
            "function_ptr".to_string(),
            Value::Function {
                name: lambda_name.clone(),
            },
        );
        closure_fields.insert("env_ptr".to_string(), Value::Number { value: 0 }); // 空环境

        ctx.add_statement(Statement::Assign {
            target: destination.clone(),
            source: Value::Struct {
                name: "Closure".to_string(),
                fields: closure_fields,
            },
            span,
        });
    } else {
        // 有捕获变量，需要分配堆环境存储共享位置指针
        let env_temp = ctx.new_temp();
        ctx.add_statement(Statement::HeapAlloc {
            target: env_temp.clone(),
            size: captured_var_locations.len() * 8, // 每个位置指针8字节
            object_type: "closure_env".to_string(),
            span,
        });

        // 将共享变量位置存储到环境中
        for (i, shared_location) in captured_var_locations.iter().enumerate() {
            let offset_temp = ctx.new_temp();
            ctx.add_statement(Statement::BinaryOp {
                target: offset_temp.clone(),
                left: env_temp.clone(),
                op: crate::ir::BinaryOperator::Add,
                right: Value::Number {
                    value: (i * 8) as i64,
                },
                span,
            });
            ctx.add_statement(Statement::Store {
                target: offset_temp.clone(),
                value: shared_location.clone(),
                span,
            });
        }

        // 创建闭包结构体
        let mut closure_fields = std::collections::BTreeMap::new();
        closure_fields.insert(
            "function_ptr".to_string(),
            Value::Function {
                name: lambda_name.clone(),
            },
        );
        closure_fields.insert("env_ptr".to_string(), env_temp);

        ctx.add_statement(Statement::Assign {
            target: destination.clone(),
            source: Value::Struct {
                name: "Closure".to_string(),
                fields: closure_fields,
            },
            span,
        });
    }

    // 4. 创建lambda函数，参数包含统一的__env + 原始参数（即使无捕获也保留__env以匹配统一ABI）
    let mut all_params = vec!["__env".to_string()];
    all_params.extend(param_names.clone());

    // 暂存当前函数上下文
    let original_function_name = ctx.current_function_name.clone();
    let original_block = ctx.current_block;
    let original_scopes = ctx.clone_scopes();

    // 5. 开始新函数
    ctx.start_function(lambda_name.clone(), all_params);

    if !free_vars.is_empty() {
        if let Some(env_binding) = ctx.lookup_variable("__env").cloned() {
            let env_value = env_binding.value.clone();
            for (index, captured_name) in free_vars.iter().enumerate() {
                let slot_ptr = ctx.new_temp();
                ctx.add_statement(Statement::BinaryOp {
                    target: slot_ptr.clone(),
                    left: env_value.clone(),
                    op: MirBinaryOp::Add,
                    right: Value::Number {
                        value: (index * 8) as i64,
                    },
                    span,
                });

                let shared_location = ctx.new_temp();
                ctx.add_statement(Statement::Dereference {
                    target: shared_location.clone(),
                    reference: slot_ptr,
                    span,
                });

                ctx.bind_variable(
                    captured_name.clone(),
                    Value::Reference {
                        value: Box::new(shared_location),
                    },
                    None,
                );
            }
        }
    }

    let return_val = ctx.new_temp();
    lower_expression(ctx, body, &return_val)?;
    maybe_retain_for_escape(ctx, body, &return_val);
    ctx.exit_scope(body.span());
    ctx.set_terminator(Terminator::Return {
        value: Some(return_val),
        span: body.span(),
    });

    // 恢复原始函数上下文
    ctx.current_function_name = original_function_name;
    ctx.current_block = original_block;
    ctx.restore_scopes(original_scopes);

    Ok(())
}

/// 降低函数调用表达式
///
/// 支持三种调用模式：
/// 1. 直接全局函数调用
/// 2. 模块符号调用
/// 3. 闭包调用（通过结构体）
fn lower_function_call(
    ctx: &mut LoweringContext,
    function: &Expr,
    args: &[Expr],
    destination: &Value,
    span: karte_diagnostics::Span,
) -> Result<(), Vec<String>> {
    // Special handling for direct calls to global functions
    if let Expr::Identifier { name, .. } = function {
        // If it's a global function and not shadowed by a local variable
        if ctx.is_known_function(name) && ctx.lookup_variable(name).is_none() {
            let arg_vals: Vec<Value> = args
                .iter()
                .map(|a| lower_expression_to_temp(ctx, a))
                .collect::<Result<_, _>>()?;

            for (arg_expr, arg_val) in args.iter().zip(arg_vals.iter()) {
                if matches!(
                    infer_expr_ownership(ctx, arg_expr),
                    Some(OwnershipKind::RefCounted)
                ) && !expr_creates_new_ref(arg_expr)
                {
                    ctx.add_statement(Statement::Retain {
                        value: arg_val.clone(),
                        span: arg_expr.span(),
                    });
                }
            }

            ctx.add_statement(Statement::Call {
                target: Some(destination.clone()),
                function: Value::Function { name: name.clone() },
                args: arg_vals,
                span,
            });
            return Ok(());
        } else {
            eprintln!(
                "DEBUG: Not a global function or shadowed: {} (in functions: {}, in vars: {})",
                name,
                ctx.program.functions.contains_key(name),
                ctx.lookup_variable(name).is_some()
            );
        }
    }

    // Special-case: direct module symbol access (module::symbol(...))
    if let Expr::ModuleSymbolAccess {
        module_path,
        symbol,
        ..
    } = function
    {
        let canonical = ctx.canonical_module_symbol(module_path, symbol);
        let arg_vals: Vec<Value> = args
            .iter()
            .map(|a| lower_expression_to_temp(ctx, a))
            .collect::<Result<_, _>>()?;

        for (arg_expr, arg_val) in args.iter().zip(arg_vals.iter()) {
            if matches!(
                infer_expr_ownership(ctx, arg_expr),
                Some(OwnershipKind::RefCounted)
            ) && !expr_creates_new_ref(arg_expr)
            {
                ctx.add_statement(Statement::Retain {
                    value: arg_val.clone(),
                    span: arg_expr.span(),
                });
            }
        }

        ctx.add_statement(Statement::Call {
            target: Some(destination.clone()),
            function: Value::Function { name: canonical },
            args: arg_vals,
            span,
        });
        return Ok(());
    }

    // 统一闭包调用策略：
    // 1. 先将被调用表达式降级为值 func_val（可能是函数指针或Closure结构）
    // 2. 如果是直接函数（Value::Function），直接调用（与之前一致）
    // 3. 否则一律视为 Closure 结构体：提取 function_ptr 与 env_ptr，生成 call，参数序列为 (env_ptr, 原始参数...)
    //    即使 env_ptr == 0 也不做分支；保持统一 ABI，便于后端优化。
    let func_val = lower_expression_to_temp(ctx, function)?;
    let arg_vals: Vec<Value> = args
        .iter()
        .map(|a| lower_expression_to_temp(ctx, a))
        .collect::<Result<_, _>>()?;

    for (arg_expr, arg_val) in args.iter().zip(arg_vals.iter()) {
        if matches!(
            infer_expr_ownership(ctx, arg_expr),
            Some(OwnershipKind::RefCounted)
        ) && !expr_creates_new_ref(arg_expr)
        {
            ctx.add_statement(Statement::Retain {
                value: arg_val.clone(),
                span: arg_expr.span(),
            });
        }
    }

    match &func_val {
        Value::Function { name } => {
            // 直接函数：无需 env
            ctx.add_statement(Statement::Call {
                target: Some(destination.clone()),
                function: Value::Function { name: name.clone() },
                args: arg_vals,
                span,
            });
        }
        Value::Closure {
            captured_values,
            function_name,
        } => {
            // 旧式 Closure 表示：captured_values 作为 env 展开到前面（保持兼容）。
            let mut all_args = captured_values.clone();
            all_args.extend(arg_vals);
            ctx.add_statement(Statement::Call {
                target: Some(destination.clone()),
                function: Value::Function {
                    name: function_name.clone(),
                },
                args: all_args,
                span,
            });
        }
        _ => {
            // 视为标准 Closure 结构体：必须含有 function_ptr / env_ptr 字段。
            let function_ptr_temp = ctx.new_temp();
            ctx.add_statement(Statement::FieldAccess {
                target: function_ptr_temp.clone(),
                object: func_val.clone(),
                field: "function_ptr".to_string(),
                span,
            });
            let env_ptr_temp = ctx.new_temp();
            ctx.add_statement(Statement::FieldAccess {
                target: env_ptr_temp.clone(),
                object: func_val.clone(),
                field: "env_ptr".to_string(),
                span,
            });
            // 统一：env 作为第一个参数传入
            let mut final_args = vec![env_ptr_temp];
            final_args.extend(arg_vals);
            ctx.add_statement(Statement::Call {
                target: Some(destination.clone()),
                function: function_ptr_temp,
                args: final_args,
                span,
            });
        }
    }

    Ok(())
}
