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
    collect_referenced_variables, convert_binary_op, convert_pattern, convert_unary_op,
    expr_creates_new_ref, handle_pattern_bindings, infer_expr_ownership,
    infer_heap_layout_from_expr, lower_expression_to_temp, maybe_retain_for_escape,
    unknown_heap_layout,
};
use super::stmt::{handle_assignment, lower_statement};
use super::types::LoweringContext;
use crate::{
    BinaryOperator as MirBinaryOp, EscapeState, HeapLayout, MatchArm, MirFunction, Statement,
    TempId, Terminator, Value,
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
                source: Value::Number {
                    value: *value,
                    ty: None,
                },
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
                source: Value::Boolean {
                    value: *value,
                    ty: None,
                },
                span,
            });
        }

        Expr::Identifier { name, .. } => {
            if let Some(binding) = ctx.lookup_variable(name) {
                // 解析临时变量的实际值
                let resolved_value = ctx.resolve_value(&binding.value);
                match &resolved_value {
                    Value::Reference {
                        value: ref_target, ..
                    } => {
                        ctx.add_statement(Statement::Dereference {
                            target: destination.clone(),
                            reference: *ref_target.clone(),
                            span,
                        });
                    }
                    _ => {
                        ctx.add_statement(Statement::Assign {
                            target: destination.clone(),
                            source: resolved_value,
                            span,
                        });
                    }
                }
            } else if ctx.is_known_function(name) {
                // 如果是函数名，需要包装成闭包结构体以统一表示形式
                // 但普通函数和闭包的调用约定不同：
                // - 闭包: function_ptr(env_ptr, ...args)
                // - 普通函数: function(...args)
                //
                // 解决方案：创建一个wrapper lambda，它接收(env_ptr, ...args)并调用原函数(...args)

                // 获取原函数的类型信息
                let function_type = ctx.get_expr_type(expr);
                let (param_types, return_type) = match &function_type {
                    karte_hir::Type::Function {
                        params,
                        return_type,
                    } => (params.clone(), return_type.clone()),
                    _ => {
                        // 如果类型未知，创建简单的wrapper
                        (vec![], Box::new(karte_hir::Type::Unknown))
                    }
                };

                // 创建wrapper lambda函数
                let wrapper_name = format!("{}$wrapper", name);
                let param_count = param_types.len();

                // wrapper的参数：__env + 原函数的参数（用通用名称）
                let mut wrapper_params = vec!["__env".to_string()];
                for i in 0..param_count {
                    wrapper_params.push(format!("__arg{}", i));
                }

                // 暂存当前函数上下文
                let original_function_name = ctx.current_function_name.clone();
                let original_block = ctx.current_block;
                let original_scopes = ctx.clone_scopes();

                // 创建wrapper函数
                ctx.start_function(wrapper_name.clone(), wrapper_params.clone());

                // 在wrapper中调用原函数，传递所有参数（除了__env）
                let wrapper_entry = ctx.current_block();
                let result_temp = ctx.new_temp();

                // 准备原函数调用的参数（跳过__env）
                let mut call_args = Vec::new();
                for i in 0..param_count {
                    let arg_name = format!("__arg{}", i);
                    if let Some(binding) = ctx.lookup_variable(&arg_name) {
                        call_args.push(binding.value.clone());
                    }
                }

                // 调用原函数
                ctx.add_statement(Statement::Call {
                    target: Some(result_temp.clone()),
                    function: Value::Function {
                        name: name.clone(),
                        ty: Some(function_type.clone()),
                    },
                    args: call_args,
                    span,
                });

                // 返回结果
                ctx.set_terminator(Terminator::Return {
                    value: Some(result_temp),
                    span,
                });

                ctx.finish_function();

                // 恢复原函数上下文
                ctx.current_function_name = original_function_name;
                ctx.current_block = original_block;
                ctx.restore_scopes(original_scopes);

                // 现在创建闭包结构体，指向wrapper而不是原函数
                let mut closure_fields = std::collections::BTreeMap::new();
                closure_fields.insert(
                    "function_ptr".to_string(),
                    Value::Function {
                        name: wrapper_name,
                        ty: Some(function_type.clone()),
                    },
                );
                closure_fields.insert("env_ptr".to_string(), Value::Number { value: 0, ty: None });

                ctx.add_statement(Statement::Assign {
                    target: destination.clone(),
                    source: Value::Struct {
                        name: "Closure".to_string(),
                        fields: closure_fields,
                        ty: Some(function_type),
                    },
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
                source: Value::Function {
                    name: canonical,
                    ty: None,
                },
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

        Expr::Lambda { .. } => {
            lower_lambda_expression(ctx, expr, destination)?;
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
                    ty: None,
                },
                None,
            );

            // handler 表达式不需要结果，因为它通常通过 resume 返回
            let dummy_temp = Value::Temp {
                id: TempId(0),
                ty: None,
            };
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
                    ty: None,
                }
            } else {
                Value::Constructor {
                    name: name.clone(),
                    arg: None,
                    ty: None,
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
                    ty: None,
                }
            } else {
                Value::QualifiedConstructor {
                    type_name: type_name.clone(),
                    constructor_name: constructor_name.clone(),
                    arg: None,
                    ty: None,
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
                ty: None,
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
                    ty: None,
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
                        ty: None,
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
                right: Value::Number { value: 8, ty: None },
                span: *span,
            });

            let data_base = ctx.new_temp();
            ctx.add_statement(Statement::BinaryOp {
                target: data_base.clone(),
                left: array_value.clone(),
                op: MirBinaryOp::Add,
                right: Value::Number { value: 8, ty: None },
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
                ty: None,
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

        Expr::UnsafeLoad { addr, byte_size, span } => {
            let addr_value = lower_expression_to_temp(ctx, addr)?;
            ctx.add_statement(Statement::UnsafeLoad {
                target: destination.clone(),
                addr: addr_value,
                byte_size: *byte_size,
                span: *span,
            });
        }

        Expr::UnsafeStore { addr, value, byte_size, span } => {
            let addr_value = lower_expression_to_temp(ctx, addr)?;
            let val_value = lower_expression_to_temp(ctx, value)?;
            ctx.add_statement(Statement::UnsafeStore {
                addr: addr_value,
                value: val_value,
                byte_size: *byte_size,
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
    expr: &karte_hir::Expr,
    destination: &Value,
) -> Result<(), Vec<String>> {
    // 提取Lambda表达式的各个部分
    let (params, body, span) = match expr {
        karte_hir::Expr::Lambda {
            params, body, span, ..
        } => (params, body, span),
        _ => unreachable!(),
    };
    let span = *span;

    // 获取Lambda的推断类型（如果存在）
    let lambda_type = ctx.get_lambda_type(expr);

    // 计算闭包结构体的类型
    let closure_type = match lambda_type.as_ref() {
        Some(karte_hir::Type::Function {
            params,
            return_type,
        })
        | Some(karte_hir::Type::Closure {
            params,
            return_type,
        }) => Some(karte_hir::Type::Closure {
            params: params.clone(),
            return_type: return_type.clone(),
        }),
        _ => None,
    };

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
                        ty: None,
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
                ty: lambda_type.clone(),
            },
        );
        closure_fields.insert("env_ptr".to_string(), Value::Number { value: 0, ty: None }); // 空环境

        ctx.add_statement(Statement::Assign {
            target: destination.clone(),
            source: Value::Struct {
                name: "Closure".to_string(),
                fields: closure_fields,
                ty: closure_type.clone(),
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
                    ty: None,
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
                ty: lambda_type.clone(),
            },
        );
        closure_fields.insert("env_ptr".to_string(), env_temp);

        ctx.add_statement(Statement::Assign {
            target: destination.clone(),
            source: Value::Struct {
                name: "Closure".to_string(),
                fields: closure_fields,
                ty: closure_type.clone(),
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

    // 设置函数的类型信息（如果已知）
    if let Some(lambda_type) = lambda_type {
        let current_fn = ctx.current_function_mut();
        match lambda_type {
            karte_hir::Type::Function {
                params: param_types,
                return_type,
            }
            | karte_hir::Type::Closure {
                params: param_types,
                return_type,
            } => {
                // 注意：all_params 包含 __env 作为第一个参数，但 param_types 不包含 __env
                // 因此我们只设置原始参数的类型，跳过 __env
                if param_types.len() == params.len() {
                    current_fn.param_types = param_types;
                    current_fn.return_type = Some(*return_type);
                }
            }
            _ => {
                // 其他类型，如 Unknown、Var 等，不设置类型信息
            }
        }
    }

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
                        ty: None,
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
                        ty: None,
                    },
                    None,
                );
            }
        }
    }

    let return_val = ctx.new_temp();
    lower_expression(ctx, body, &return_val)?;
    maybe_retain_for_escape(ctx, body, &return_val);

    // 推断Lambda的返回类型并注册
    // 对于identity闭包等情况，返回值可能是函数或闭包类型
    infer_and_register_lambda_return_type(ctx, body, &lambda_name, &return_val);

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

/// 推断并注册Lambda的返回类型
///
/// 分析Lambda body表达式，尝试推断其返回类型
/// 特别处理identity闭包等返回函数/闭包的情况
fn infer_and_register_lambda_return_type(
    ctx: &mut LoweringContext,
    body: &Expr,
    lambda_name: &str,
    return_val: &Value,
) {
    // 检查返回值是否已经解析为函数或闭包
    let resolved_return_val = ctx.resolve_value(return_val);

    let should_register_callable = match &resolved_return_val {
        Value::Function { .. } | Value::Closure { .. } => true,
        Value::Struct { name, .. } => name == "Closure",
        _ => {
            // 如果body是简单的标识符，检查该标识符
            if let Expr::Identifier { name: var_name, .. } = body {
                if let Some(binding) = ctx.lookup_variable(var_name) {
                    let resolved_binding = ctx.resolve_value(&binding.value);
                    match &resolved_binding {
                        Value::Function { .. } | Value::Closure { .. } => true,
                        Value::Struct { name, .. } => name == "Closure",
                        _ => false,
                    }
                } else {
                    false
                }
            } else {
                false
            }
        }
    };

    if should_register_callable {
        // Lambda返回可调用类型，注册为闭包类型
        // 使用一个泛型的闭包签名
        let callable_type = karte_hir::Type::Closure {
            params: vec![],                                  // 参数类型未知
            return_type: Box::new(karte_hir::Type::Unknown), // 返回类型未知
        };
        ctx.register_function_return_type(lambda_name.to_string(), callable_type);
    }
}

/// 闭包调用后检查并标注返回值类型
///
/// 对于identity闭包等运行时才能确定返回类型的情况，
/// 在调用后检查返回值实际内容并标注类型
fn annotate_closure_return_value(ctx: &mut LoweringContext, destination: &Value, args: &[Value]) {
    // 采用启发式方法：如果调用闭包时传入了函数类型参数，
    // 则该闭包可能返回该函数（比如identity闭包）
    // 单参数情况：假设返回值可能是该函数
    // 多参数情况：暂时不做假设，因为可能是高阶函数

    if args.len() == 1 {
        let arg = &args[0];
        let resolved = ctx.resolve_value(arg);

        let is_callable = matches!(&resolved, Value::Function { .. } | Value::Closure { .. })
            || matches!(&resolved, Value::Struct { name, .. } if name == "Closure");

        if is_callable {
            if let Value::Temp { id, .. } = destination {
                ctx.temp_value_map.insert(*id, resolved);
            }
        }
    }
    // 注意：对于多参数闭包如 |func, val| { func(val) }，
    // 我们不能简单假设返回值类型，因为需要更复杂的控制流分析
}

/// 在函数调用后标注返回值的类型信息
///
/// 如果函数的返回类型是Function或Closure，记录到temp_value_map中
/// 这样后续使用时能正确识别该值为可调用类型
fn annotate_function_return_type(
    ctx: &mut LoweringContext,
    function_name: &str,
    destination: &Value,
) {
    if let Value::Temp { id, .. } = destination {
        if let Some(return_type) = ctx.get_function_return_type(function_name) {
            if LoweringContext::is_callable_type(return_type) {
                // 返回类型是函数或闭包，创建一个类型标注值
                let type_marker = match return_type {
                    karte_hir::Type::Function { .. } => {
                        // 标记为函数类型（函数名未知，使用占位符）
                        Value::Function {
                            name: format!("__returned_function_{}", id),
                            ty: None,
                        }
                    }
                    karte_hir::Type::Closure { .. } => {
                        // 标记为闭包类型
                        Value::Struct {
                            name: "Closure".to_string(),
                            fields: std::collections::BTreeMap::new(),
                            ty: None,
                        }
                    }
                    _ => return,
                };
                ctx.temp_value_map.insert(*id, type_marker);
            }
        }
    }
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
    // Special handling for identifiers that might be functions or closures
    if let Expr::Identifier { name, .. } = function {
        // 首先检查变量环境中的绑定，并解析实际值
        let resolved_value = if let Some(binding) = ctx.lookup_variable(name) {
            let resolved = ctx.resolve_value(&binding.value);
            Some(resolved)
        } else if ctx.is_known_function(name) {
            Some(Value::Function {
                name: name.clone(),
                ty: None,
            })
        } else {
            None
        };

        if let Some(Value::Function {
            name: func_name, ..
        }) = resolved_value
        {
            // 如果解析后的值是函数类型，直接调用
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
                function: Value::Function {
                    name: func_name.clone(),
                    ty: None,
                },
                args: arg_vals,
                span,
            });
            // 标注返回类型（如果返回函数/闭包）
            annotate_function_return_type(ctx, &func_name, destination);
            return Ok(());
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
            function: Value::Function {
                name: canonical.clone(),
                ty: None,
            },
            args: arg_vals,
            span,
        });
        // 标注返回类型（如果返回函数/闭包）
        annotate_function_return_type(ctx, &canonical, destination);
        return Ok(());
    }

    // 统一闭包调用策略：
    // 1. 先将被调用表达式降级为值 func_val（可能是函数指针或Closure结构）
    // 2. 解析临时变量的实际值（如果是函数/闭包）
    // 3. 如果是直接函数（Value::Function），直接调用（与之前一致）
    // 4. 否则一律视为 Closure 结构体：提取 function_ptr 与 env_ptr，生成 call，参数序列为 (env_ptr, 原始参数...)
    //    即使 env_ptr == 0 也不做分支；保持统一 ABI，便于后端优化。

    // 🔧 关键修复：检查function是否是闭包类型的参数
    // 如果是参数且在当前函数的参数列表中，直接使用该参数值
    let is_closure_parameter = if let Expr::Identifier { name, .. } = function {
        // 检查是否是当前函数的参数
        if let Some(current_func_name) = &ctx.current_function_name {
            if let Some(func) = ctx.program.functions.get(current_func_name) {
                func.params.contains(name) && name != "__env" // __env不算闭包参数
            } else {
                false
            }
        } else {
            false
        }
    } else {
        false
    };

    let func_temp = lower_expression_to_temp(ctx, function)?;
    let func_val = ctx.resolve_value(&func_temp);
    // 移除了之前的类型变量特殊处理
    // 现在所有可调用对象（函数和闭包）都统一表示为闭包结构体
    // 所以不需要区分处理
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
        Value::Function { name, .. } => {
            // 直接函数调用（不是闭包参数的情况）
            ctx.add_statement(Statement::Call {
                target: Some(destination.clone()),
                function: Value::Function {
                    name: name.clone(),
                    ty: None,
                },
                args: arg_vals,
                span,
            });
            // 标注返回类型（如果返回函数/闭包）
            annotate_function_return_type(ctx, name, destination);
        }
        Value::Closure {
            captured_values,
            function_name,
            ..
        } => {
            // 旧式 Closure 表示：captured_values 作为 env 展开到前面（保持兼容）。
            // 在消费arg_vals之前，先标注返回值类型
            annotate_closure_return_value(ctx, destination, &arg_vals);

            let mut all_args = captured_values.clone();
            all_args.extend(arg_vals);
            ctx.add_statement(Statement::Call {
                target: Some(destination.clone()),
                function: Value::Function {
                    name: function_name.clone(),
                    ty: None,
                },
                args: all_args,
                span,
            });
            // 标注返回类型（如果返回函数/闭包）
            annotate_function_return_type(ctx, function_name, destination);
        }
        _ => {
            // 视为标准 Closure 结构体：必须含有 function_ptr / env_ptr 字段。
            // 🔧 关键修复：如果是闭包参数，使用func_temp（临时变量）而不是func_val（类型标记）
            let object_for_field_access = if is_closure_parameter {
                func_temp.clone()
            } else {
                func_val.clone()
            };

            let function_ptr_temp = ctx.new_temp();
            ctx.add_statement(Statement::FieldAccess {
                target: function_ptr_temp.clone(),
                object: object_for_field_access.clone(),
                field: "function_ptr".to_string(),
                span,
            });
            let env_ptr_temp = ctx.new_temp();
            ctx.add_statement(Statement::FieldAccess {
                target: env_ptr_temp.clone(),
                object: object_for_field_access.clone(),
                field: "env_ptr".to_string(),
                span,
            });

            // 在消费arg_vals之前，先标注返回值类型
            // 这对于identity闭包等返回函数的情况很重要
            annotate_closure_return_value(ctx, destination, &arg_vals);

            // 统一：env 作为第一个参数传入
            let mut final_args = vec![env_ptr_temp];
            final_args.extend(arg_vals);

            // 解析function_ptr_temp获取实际函数名
            let resolved_func_ptr = ctx.resolve_value(&function_ptr_temp);
            if let Value::Function {
                name: func_name, ..
            } = &resolved_func_ptr
            {
                ctx.add_statement(Statement::Call {
                    target: Some(destination.clone()),
                    function: Value::Function {
                        name: func_name.clone(),
                        ty: None,
                    },
                    args: final_args,
                    span,
                });
                // 标注返回类型（如果返回函数/闭包）
                annotate_function_return_type(ctx, func_name, destination);
            } else {
                ctx.add_statement(Statement::Call {
                    target: Some(destination.clone()),
                    function: function_ptr_temp,
                    args: final_args,
                    span,
                });
            }
        }
    }

    Ok(())
}
