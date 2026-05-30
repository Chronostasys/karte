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
use karte_hir::types::Type;
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

        Expr::StringLiteral { value, span } => {
            // 字符串字面量：堆分配 [length: i64] [byte0 byte1 ... padding]
            let byte_len = value.len();
            // 对齐到 8 字节 + 8 字节头部（存储长度）
            let total_size = ((byte_len + 7) / 8) * 8 + 8;

            let str_ptr = ctx.new_temp();
            ctx.add_statement(Statement::Allocate {
                target: str_ptr.clone(),
                layout: HeapLayout {
                    type_id: format!("string:{}", byte_len),
                    size: total_size,
                    align: 8,
                    mutable: false,
                    escape: EscapeState::Global,
                    ownership: OwnershipKind::Manual,
                },
                span: *span,
            });

            // 写入长度到头部（offset 0）
            ctx.add_statement(Statement::Store {
                target: str_ptr.clone(),
                value: Value::Number {
                    value: byte_len as i64,
                    ty: None,
                },
                span: *span,
            });

            // 逐字节写入字符串数据（从 offset 8 开始）
            for (i, byte) in value.as_bytes().iter().enumerate() {
                let byte_offset = 8 + i;
                let byte_addr = ctx.new_temp();
                ctx.add_statement(Statement::BinaryOp {
                    target: byte_addr.clone(),
                    left: str_ptr.clone(),
                    op: MirBinaryOp::Add,
                    right: Value::Number {
                        value: byte_offset as i64,
                        ty: None,
                    },
                    span: *span,
                });
                ctx.add_statement(Statement::UnsafeStore {
                    addr: byte_addr,
                    value: Value::Number {
                        value: *byte as i64,
                        ty: None,
                    },
                    byte_size: 1,
                    span: *span,
                });
            }

            ctx.add_statement(Statement::Assign {
                target: destination.clone(),
                source: str_ptr,
                span: *span,
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
            // 检查是否为字符串连接：Add 且操作数类型为 String
            let expr_ptr = expr as *const Expr as usize;
            let is_string_concat = *op == karte_hir::BinaryOperator::Add
                && ctx
                    .expr_types
                    .get(&expr_ptr)
                    .map(|t| matches!(t, karte_hir::Type::String))
                    .unwrap_or(false);

            if is_string_concat {
                // 字符串连接：调用运行时 string_concat 函数
                let left_val = lower_expression_to_temp(ctx, left)?;
                let right_val = lower_expression_to_temp(ctx, right)?;
                ctx.add_statement(Statement::Call {
                    target: Some(destination.clone()),
                    function: Value::Function {
                        name: "__runtime_string_concat".to_string(),
                        ty: None,
                    },
                    args: vec![left_val, right_val],
                    span,
                });
            } else {
                // 原有数字运算逻辑
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

            // 保存 if 之前的变量绑定快照（遍历所有作用域）
            let pre_if_bindings: std::collections::HashMap<String, Value> = ctx
                .scopes
                .iter()
                .rev() // 从内到外遍历，内层优先
                .flat_map(|scope| {
                    scope.bindings.iter().map(|(k, v)| (k.clone(), v.value.clone()))
                })
                .collect();

            // then 分支
            ctx.set_current_block(then_block);
            lower_expression(ctx, then_branch, destination)?;
            // 捕获实际跳转到 merge 的块（内嵌 if-else 会改变 current_block）
            let actual_then_block = ctx.current_block();
            ctx.set_terminator(Terminator::Goto {
                target: merge_block,
                span: then_branch.span(),
            });

            // 记录 then 分支后的变量绑定（遍历所有作用域）
            let then_bindings: std::collections::HashMap<String, Value> = ctx
                .scopes
                .iter()
                .rev()
                .flat_map(|scope| {
                    scope.bindings.iter().map(|(k, v)| (k.clone(), v.value.clone()))
                })
                .collect();

            // else 分支
            let actual_else_block;
            if let Some(else_branch) = else_branch {
                // 恢复到 if 之前的绑定
                // 遍历所有作用域，恢复每个作用域中的变量绑定
                for scope in ctx.scopes.iter_mut() {
                    for (name, binding) in scope.bindings.iter_mut() {
                        if let Some(pre_val) = pre_if_bindings.get(name) {
                            binding.value = pre_val.clone();
                        }
                    }
                }
                ctx.set_current_block(else_block);
                lower_expression(ctx, else_branch, destination)?;
                // 捕获实际跳转到 merge 的块
                actual_else_block = ctx.current_block();
                ctx.set_terminator(Terminator::Goto {
                    target: merge_block,
                    span: else_branch.span(),
                });
            } else {
                // 没有else分支时，else路径应该返回Unit
                actual_else_block = else_block;
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

            // 记录 else 分支后的变量绑定（遍历所有作用域）
            let else_bindings: std::collections::HashMap<String, Value> = ctx
                .scopes
                .iter()
                .rev()
                .flat_map(|scope| {
                    scope.bindings.iter().map(|(k, v)| (k.clone(), v.value.clone()))
                })
                .collect();

            // 在 merge 块中为被修改的变量插入 Phi 节点
            // 预分析模式也追踪变量变化（创建 phi temp 更新绑定），但不生成 Phi statement
            ctx.set_current_block(merge_block);
            for (name, pre_value) in &pre_if_bindings {
                let then_value = then_bindings.get(name).cloned().unwrap_or_else(|| pre_value.clone());
                let else_value = else_bindings.get(name).cloned().unwrap_or_else(|| pre_value.clone());

                let then_changed = then_value != *pre_value;
                let else_changed = else_value != *pre_value;

                if then_changed || else_changed {
                    let phi_temp = ctx.new_temp();
                    if !ctx.analysis_mode {
                        ctx.add_statement(Statement::Phi {
                            target: phi_temp.clone(),
                            incoming: vec![
                                (actual_then_block, then_value),
                                (actual_else_block, else_value),
                            ],
                            span,
                        });
                    }
                    ctx.update_variable(name, phi_temp, None);
                }
            }
        }

        Expr::While {
            condition,
            body,
            span,
        } => {
            let loop_head = ctx.new_block();
            let loop_body = ctx.new_block();
            let loop_exit = ctx.new_block();

            // 记录循环前的块 ID
            let pre_loop_block = ctx.current_block();

            // 快照当前变量绑定
            let pre_loop_bindings: std::collections::HashMap<String, (Value, Option<OwnershipKind>)> =
                ctx.current_scope()
                    .bindings
                    .iter()
                    .map(|(k, v)| (k.clone(), (v.value.clone(), v.ownership)))
                    .collect();

            // === 第一步：预分析循环体，找出被更新的变量 ===
            // 先保存当前状态，lower 循环体到临时块来收集变量更新
            // 预分析模式：不生成 Phi 节点，仅收集变量绑定变化
            ctx.analysis_mode = true;
            let pre_analysis_block_count = ctx.current_function_mut().basic_blocks.len();
            let saved_block = ctx.current_block();
            let analysis_block = ctx.new_block();
            ctx.set_current_block(analysis_block);
            let temp_result = ctx.new_temp();
            let _ = lower_expression(ctx, body, &temp_result);
            ctx.analysis_mode = false;

            // 收集循环体中更新的变量
            let post_loop_bindings: std::collections::HashMap<String, (Value, Option<OwnershipKind>)> =
                ctx.current_scope()
                    .bindings
                    .iter()
                    .map(|(k, v)| (k.clone(), (v.value.clone(), v.ownership)))
                    .collect();

            // 清理预分析产生的临时块（包括 if-else 创建的 then/else/merge 块）
            let all_block_ids: Vec<_> = ctx.current_function_mut().basic_blocks.keys().cloned().collect();
            let analysis_blocks: Vec<_> = all_block_ids[pre_analysis_block_count..].to_vec();
            for block_id in &analysis_blocks {
                ctx.remove_block(*block_id);
            }

            let mut updated_vars: Vec<(String, Value, Value)> = Vec::new();
            for (name, (post_value, _)) in &post_loop_bindings {
                if let Some((pre_value, _)) = pre_loop_bindings.get(name) {
                    if post_value != pre_value {
                        updated_vars.push((name.clone(), pre_value.clone(), post_value.clone()));
                    }
                }
            }

            // === 第二步：删除分析用的临时块，恢复状态 ===
            ctx.remove_block(analysis_block);
            // 恢复变量绑定到循环前的状态
            for (name, (value, ownership)) in &pre_loop_bindings {
                ctx.update_variable(name, value.clone(), *ownership);
            }

            // === 第三步：创建 phi temp 并更新 context ===
            // 这样后续 lower 循环体时会使用 phi 结果
            let mut phi_values: std::collections::HashMap<String, Value> = std::collections::HashMap::new();
            for (name, initial_value, loop_value) in &updated_vars {
                let phi_temp = ctx.new_temp();
                phi_values.insert(name.clone(), phi_temp);
                // 记录 phi 信息：(initial_value, loop_value) 用于后续生成
                // 注意：先不生成 phi 语句，等循环体 lower 后再生成
            }

            // 更新 context 中的变量绑定指向 phi temp
            for (name, phi_val) in &phi_values {
                ctx.update_variable(name, phi_val.clone(), None);
            }

            // Jump to loop head (从 pre_loop 块)
            ctx.set_current_block(saved_block);
            ctx.set_terminator(Terminator::Goto {
                target: loop_head,
                span: *span,
            });

            // === 第四步：生成循环体（使用 phi 绑定后的 context）===
            ctx.set_current_block(loop_body);

            // 推入循环上下文（break/continue 需要）
            ctx.loop_stack.push(super::types::LoopContext {
                continue_target: loop_head,
                break_target: loop_exit,
            });

            let temp_body_result = ctx.new_temp();
            lower_expression(ctx, body, &temp_body_result)?;

            // 弹出循环上下文
            ctx.loop_stack.pop();

            // 收集循环体中变量更新后的值（用于 phi incoming）
            // 遍历所有作用域，因为 if-else 的 phi 更新可能在内层作用域
            let final_bindings: std::collections::HashMap<String, (Value, Option<OwnershipKind>)> =
                ctx.scopes
                    .iter()
                    .rev()
                    .flat_map(|scope| {
                        scope.bindings.iter().map(|(k, v)| (k.clone(), (v.value.clone(), v.ownership)))
                    })
                    .collect();

            ctx.set_terminator(Terminator::Goto {
                target: loop_head,
                span: body.span(),
            });

            // 记录 Goto 终结符所在的基本块（可能是 if-else 的 merge_block）
            let loop_back_edge_block = ctx.current_block();

            // === 第五步：生成循环头（包含 phi 节点）===
            ctx.set_current_block(loop_head);

            for (name, initial_value, _loop_value) in &updated_vars {
                let phi_temp = phi_values.get(name).unwrap().clone();
                // 获取循环体更新后的值
                let final_value = final_bindings.get(name)
                    .map(|(v, _)| v.clone())
                    .unwrap_or_else(|| initial_value.clone());
                ctx.add_statement(Statement::Phi {
                    target: phi_temp,
                    incoming: vec![
                        (pre_loop_block, initial_value.clone()),
                        (loop_back_edge_block, final_value),
                    ],
                    span: *span,
                });
            }

            // 更新 context 指向 phi 结果
            for (name, phi_val) in &phi_values {
                ctx.update_variable(name, phi_val.clone(), None);
            }

            // 条件求值
            let cond_val = lower_expression_to_temp(ctx, condition)?;
            ctx.set_terminator(Terminator::Branch {
                condition: cond_val,
                then_block: loop_body,
                else_block: loop_exit,
                span: condition.span(),
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

        Expr::ForIn {
            var,
            start,
            end,
            body,
            span,
        } => {
            // for var in start..end { body }
            // 展开为：
            //   let __for_start = start
            //   let __for_end = end
            //   let mut __for_var = __for_start
            //   while __for_var < __for_end {
            //       let var = __for_var
            //       body
            //       __for_var = __for_var + 1
            //   }

            let loop_head = ctx.new_block();
            let loop_body = ctx.new_block();
            let loop_exit = ctx.new_block();

            // 记录循环前的块 ID
            let pre_loop_block = ctx.current_block();

            // 计算 start 和 end
            let start_val = lower_expression_to_temp(ctx, start)?;
            let end_val = lower_expression_to_temp(ctx, end)?;

            // 创建循环变量 __for_var（使用递增计数器确保嵌套循环变量名唯一）
            let for_var_name = format!("__for_var_{}", ctx.lambda_counter);
            ctx.lambda_counter += 1;
            let for_var_temp = ctx.new_temp();
            ctx.add_statement(Statement::Assign {
                target: for_var_temp.clone(),
                source: start_val.clone(),
                span: *span,
            });
            ctx.bind_variable(for_var_name.clone(), for_var_temp.clone(), None);

            // 快照当前变量绑定
            let pre_loop_bindings: std::collections::HashMap<String, (Value, Option<OwnershipKind>)> =
                ctx.current_scope()
                    .bindings
                    .iter()
                    .map(|(k, v)| (k.clone(), (v.value.clone(), v.ownership)))
                    .collect();

            // === 第一步：预分析循环体，找出被更新的变量 ===
            ctx.analysis_mode = true;
            let pre_analysis_block_count = ctx.current_function_mut().basic_blocks.len();
            let saved_block = ctx.current_block();
            let analysis_block = ctx.new_block();
            ctx.set_current_block(analysis_block);

            // 在分析块中绑定循环变量
            let analysis_var_temp = ctx.new_temp();
            ctx.bind_variable(var.clone(), analysis_var_temp, None);
            // 模拟递增
            let inc_temp = ctx.new_temp();
            ctx.add_statement(Statement::Assign {
                target: inc_temp.clone(),
                source: Value::Number { value: 1, ty: None },
                span: *span,
            });

            let temp_result = ctx.new_temp();
            let _ = lower_expression(ctx, body, &temp_result);
            ctx.analysis_mode = false;

            // 收集循环体中更新的变量
            let post_loop_bindings: std::collections::HashMap<String, (Value, Option<OwnershipKind>)> =
                ctx.current_scope()
                    .bindings
                    .iter()
                    .map(|(k, v)| (k.clone(), (v.value.clone(), v.ownership)))
                    .collect();

            // 清理预分析产生的临时块
            let all_block_ids: Vec<_> = ctx.current_function_mut().basic_blocks.keys().cloned().collect();
            let analysis_blocks: Vec<_> = all_block_ids[pre_analysis_block_count..].to_vec();
            for block_id in &analysis_blocks {
                ctx.remove_block(*block_id);
            }

            let mut updated_vars: Vec<(String, Value, Value)> = Vec::new();
            for (name, (post_value, _)) in &post_loop_bindings {
                if let Some((pre_value, _)) = pre_loop_bindings.get(name) {
                    if post_value != pre_value {
                        updated_vars.push((name.clone(), pre_value.clone(), post_value.clone()));
                    }
                }
            }

            // === 第二步：删除分析用的临时块，恢复状态 ===
            ctx.remove_block(analysis_block);
            for (name, (value, ownership)) in &pre_loop_bindings {
                ctx.update_variable(name, value.clone(), *ownership);
            }

            // 确保循环变量和 __for_var 在绑定中
            ctx.bind_variable(for_var_name.clone(), for_var_temp.clone(), None);

            // === 第三步：创建 phi temp ===
            let mut phi_values: std::collections::HashMap<String, Value> = std::collections::HashMap::new();
            // __for_var 也需要 phi
            let for_var_phi = ctx.new_temp();
            phi_values.insert(for_var_name.clone(), for_var_phi.clone());

            for (name, initial_value, _loop_value) in &updated_vars {
                let phi_temp = ctx.new_temp();
                phi_values.insert(name.clone(), phi_temp);
            }

            // 更新 context 中的变量绑定指向 phi temp
            for (name, phi_val) in &phi_values {
                ctx.update_variable(name, phi_val.clone(), None);
            }

            // Jump to loop head
            ctx.set_current_block(saved_block);
            ctx.set_terminator(Terminator::Goto {
                target: loop_head,
                span: *span,
            });

            // === 第四步：生成循环体 ===
            ctx.set_current_block(loop_body);

            // 在循环体内绑定用户变量 = __for_var 的 phi 值
            let user_var_temp = ctx.new_temp();
            ctx.add_statement(Statement::Assign {
                target: user_var_temp.clone(),
                source: for_var_phi.clone(),
                span: *span,
            });
            ctx.bind_variable(var.clone(), user_var_temp.clone(), None);

            // 推入循环上下文（break/continue 需要）
            ctx.loop_stack.push(super::types::LoopContext {
                continue_target: loop_head,
                break_target: loop_exit,
            });

            let temp_body_result = ctx.new_temp();
            lower_expression(ctx, body, &temp_body_result)?;

            // 弹出循环上下文
            ctx.loop_stack.pop();

            // 递增 __for_var = __for_var + 1
            let inc_temp = lower_expression_to_temp(ctx, &Expr::BinaryOp {
                left: Box::new(Expr::Identifier {
                    name: for_var_name.clone(),
                    span: *span,
                }),
                op: karte_hir::BinaryOperator::Add,
                right: Box::new(Expr::Number { value: 1, span: *span }),
                span: *span,
            })?;
            // 递增结果写入新 temp 并通过 update_variable 更新绑定
            // 不能直接写入 for_var_phi（phi target），否则 phi incoming 会自引用
            ctx.update_variable(&for_var_name, inc_temp.clone(), None);

            // 收集循环体中变量更新后的值
            let final_bindings: std::collections::HashMap<String, (Value, Option<OwnershipKind>)> =
                ctx.scopes
                    .iter()
                    .rev()
                    .flat_map(|scope| {
                        scope.bindings.iter().map(|(k, v)| (k.clone(), (v.value.clone(), v.ownership)))
                    })
                    .collect();

            ctx.set_terminator(Terminator::Goto {
                target: loop_head,
                span: body.span(),
            });

            let loop_back_edge_block = ctx.current_block();

            // === 第五步：生成循环头 ===
            ctx.set_current_block(loop_head);

            // Phi 节点（包括 __for_var）
            for (name, initial_value, _loop_value) in &updated_vars {
                let phi_temp = phi_values.get(name).unwrap().clone();
                let final_value = final_bindings.get(name)
                    .map(|(v, _)| v.clone())
                    .unwrap_or_else(|| initial_value.clone());
                ctx.add_statement(Statement::Phi {
                    target: phi_temp,
                    incoming: vec![
                        (pre_loop_block, initial_value.clone()),
                        (loop_back_edge_block, final_value),
                    ],
                    span: *span,
                });
            }
            // __for_var 的 phi
            let for_var_updated = final_bindings.get(&for_var_name)
                .map(|(v, _)| v.clone())
                .unwrap_or_else(|| start_val);
            ctx.add_statement(Statement::Phi {
                target: for_var_phi,
                incoming: vec![
                    (pre_loop_block, for_var_temp),
                    (loop_back_edge_block, for_var_updated),
                ],
                span: *span,
            });
            // 更新 context 中所有被循环修改的变量指向 phi 结果
            for (name, phi_val) in &phi_values {
                ctx.update_variable(name, phi_val.clone(), None);
            }

            // 条件: __for_var < end
            // 在循环头内部重新计算 end，避免跨块引用临时值导致 memory2reg 错误提升
            let for_var_phi_val = phi_values.get(&for_var_name).unwrap().clone();
            let end_val_in_header = lower_expression_to_temp(ctx, end)?;
            let cond_temp = ctx.new_temp();
            ctx.add_statement(Statement::BinaryOp {
                op: crate::BinaryOperator::LessThan,
                left: for_var_phi_val,
                right: end_val_in_header,
                target: cond_temp.clone(),
                span: *span,
            });

            ctx.set_terminator(Terminator::Branch {
                condition: cond_temp,
                then_block: loop_body,
                else_block: loop_exit,
                span: *span,
            });

            // Continue from exit block
            ctx.set_current_block(loop_exit);
            ctx.add_statement(Statement::Assign {
                target: destination.clone(),
                source: Value::Unit,
                span: *span,
            });
        }

        Expr::Break { span, .. } => {
            // break: 跳转到当前循环的退出块
            if let Some(loop_ctx) = ctx.loop_stack.last() {
                ctx.set_terminator(Terminator::Goto {
                    target: loop_ctx.break_target,
                    span: *span,
                });
                // 创建一个死块来放置后续代码（break 后的代码不会执行）
                let dead_block = ctx.new_block();
                ctx.set_current_block(dead_block);
            } else {
                // 不在循环中，break 是错误
                return Err(vec![format!("break outside of loop at {:?}", span)]);
            }
            // break 后赋值 Unit（不会执行，但满足 destination 要求）
            ctx.add_statement(Statement::Assign {
                target: destination.clone(),
                source: Value::Unit,
                span: *span,
            });
        }

        Expr::Continue { span, .. } => {
            // continue: 跳转到当前循环的条件检查块
            if let Some(loop_ctx) = ctx.loop_stack.last() {
                ctx.set_terminator(Terminator::Goto {
                    target: loop_ctx.continue_target,
                    span: *span,
                });
                // 创建一个死块
                let dead_block = ctx.new_block();
                ctx.set_current_block(dead_block);
            } else {
                return Err(vec![format!("continue outside of loop at {:?}", span)]);
            }
            ctx.add_statement(Statement::Assign {
                target: destination.clone(),
                source: Value::Unit,
                span: *span,
            });
        }

        Expr::Return { value, span, .. } => {
            // return: 跳转到函数返回块
            // 对于脚本入口，我们创建一个提前返回块
            // 对于函数，跳转到 return 终结符
            let return_value = if let Some(v) = value {
                lower_expression_to_temp(ctx, v)?
            } else {
                // 无返回值，用 Unit
                let unit_temp = ctx.new_temp();
                ctx.add_statement(Statement::Assign {
                    target: unit_temp.clone(),
                    source: Value::Unit,
                    span: *span,
                });
                unit_temp
            };

            // 设置 Return 终结符
            ctx.set_terminator(Terminator::Return {
                value: Some(return_value),
                span: *span,
            });

            // 创建一个死块
            let dead_block = ctx.new_block();
            ctx.set_current_block(dead_block);

            // 赋值 Unit（不会执行）
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
            // 检查是否为字符串 .len 属性
            let obj_type = ctx.get_expr_type(object);
            let is_string_len = field == "len"
                && matches!(obj_type, karte_hir::Type::String);

            if is_string_len {
                // 字符串 .len：从指针 offset 0 读取 i64 长度值
                let object_value = lower_expression_to_temp(ctx, object)?;
                ctx.add_statement(Statement::Dereference {
                    target: destination.clone(),
                    reference: object_value,
                    span: *span,
                });
            } else {
                // struct field access
                let object_value = lower_expression_to_temp(ctx, object)?;
                ctx.add_statement(Statement::FieldAccess {
                    target: destination.clone(),
                    object: object_value,
                    field: field.clone(),
                    span: *span,
                });
            }
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

        Expr::RuntimeGlobal { name, span } => {
            ctx.add_statement(Statement::RuntimeGlobal {
                target: destination.clone(),
                global_name: name.clone(),
                span: *span,
            });
        }

        Expr::GcRegOp { is_push, span } => {
            ctx.add_statement(Statement::GcRegOp {
                target: destination.clone(),
                is_push: *is_push,
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

        // ===== 类型转换 (as 表达式) =====
        Expr::TypeCast { expr, target_type, span } => {
            // 先 lower 内部表达式到临时变量
            let source = lower_expression_to_temp(ctx, expr)?;
            
            // 从 target_type 提取位宽和符号性
            let (dst_bits, signed) = match target_type {
                Type::Int(kind) => ((kind.size_in_bytes() * 8) as u8, kind.is_signed()),
                Type::Number => (64, true),  // Number 等同于 i64
                Type::Bool => (8, false),    // Bool 用 U8 表示
                _ => {
                    // 不支持的转换目标类型，直接透传
                    lower_expression(ctx, expr, destination)?;
                    return Ok(());
                }
            };
            
            let target = ctx.new_temp();
            ctx.add_statement(Statement::TypeCast {
                target: target.clone(),
                source,
                dst_bits,
                signed,
                span: *span,
            });
            
            // 将结果移动到目标寄存器
            ctx.add_statement(Statement::Assign {
                target: destination.clone(),
                source: target,
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
        // 处理 print 内建函数
        if name == "print" {
            // print 只接受一个参数
            if args.len() != 1 {
                ctx.errors.push("print 函数只接受一个参数".to_string());
                return Err(ctx.errors.clone());
            }
            let arg_val = lower_expression_to_temp(ctx, &args[0])?;
            ctx.add_statement(Statement::Call {
                target: Some(destination.clone()),
                function: Value::Function {
                    name: "__runtime_print_string".to_string(),
                    ty: None,
                },
                args: vec![arg_val],
                span,
            });
            return Ok(());
        }

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
