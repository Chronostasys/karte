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
    BasicBlockId, BinaryOperator as MirBinaryOp, EscapeState, HeapLayout, MatchArm, MirFunction, Statement,
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
            let expr_ptr = expr as *const Expr as usize;

            // 检查是否为字符串连接：Add 且操作数类型为 String
            let is_string_concat = *op == karte_hir::BinaryOperator::Add
                && ctx
                    .expr_types
                    .get(&expr_ptr)
                    .map(|t| matches!(t, karte_hir::Type::String))
                    .unwrap_or(false);

            // 检查是否为字符串比较：Equal/NotEqual 且操作数类型为 String
            // 注意：Equal/NotEqual 的表达式类型是 bool，不是 String
            // 所以需要检查左操作数的类型
            let left_ptr = left.as_ref() as *const Expr as usize;
            let is_string_compare = matches!(op, karte_hir::BinaryOperator::Equal | karte_hir::BinaryOperator::NotEqual)
                && ctx
                    .expr_types
                    .get(&left_ptr)
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
            } else if is_string_compare {
                // 字符串比较：调用运行时 string_equal 函数
                let left_val = lower_expression_to_temp(ctx, left)?;
                let right_val = lower_expression_to_temp(ctx, right)?;

                // 调用 string_equal 得到 0/1 结果
                let eq_result = ctx.new_temp();
                ctx.add_statement(Statement::Call {
                    target: Some(eq_result.clone()),
                    function: Value::Function {
                        name: "__runtime_string_equal".to_string(),
                        ty: None,
                    },
                    args: vec![left_val, right_val],
                    span,
                });

                if *op == karte_hir::BinaryOperator::Equal {
                    // Equal: 直接使用 string_equal 的结果 (Xor 0 = 恒等)
                    ctx.add_statement(Statement::BinaryOp {
                        target: destination.clone(),
                        left: eq_result,
                        op: MirBinaryOp::BitXor,
                        right: Value::Number { value: 0, ty: None },
                        span,
                    });
                } else {
                    // NotEqual: 对 string_equal 的结果取反 (Xor 1)
                    ctx.add_statement(Statement::BinaryOp {
                        target: destination.clone(),
                        left: eq_result,
                        op: MirBinaryOp::BitXor,
                        right: Value::Number { value: 1, ty: None },
                        span,
                    });
                }
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
            // 无论是否有 else 分支，都需要恢复绑定到 if 之前的状态
            // 否则无 else 时 else_bindings 会错误继承 then 分支的绑定
            for scope in ctx.scopes.iter_mut() {
                for (name, binding) in scope.bindings.iter_mut() {
                    if let Some(pre_val) = pre_if_bindings.get(name) {
                        binding.value = pre_val.clone();
                    }
                }
            }
            let actual_else_block;
            if let Some(else_branch) = else_branch {
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
            // R8-2 修复：当闭包在一个分支中捕获变量时（值变为 Reference），
            // 另一个分支的值是普通 Temp。Phi 不能直接合并不同类型，
            // 需要为 Reference 值插入 Dereference 获取实际值，
            // 并为非 Reference 值创建 shared_location 统一类型。
            // 最终 Phi 合并 shared_location 指针，变量绑定更新为 Reference。
            ctx.set_current_block(merge_block);
            for (name, pre_value) in &pre_if_bindings {
                let then_value = then_bindings.get(name).cloned().unwrap_or_else(|| pre_value.clone());
                let else_value = else_bindings.get(name).cloned().unwrap_or_else(|| pre_value.clone());

                let then_changed = then_value != *pre_value;
                let else_changed = else_value != *pre_value;

                if then_changed || else_changed {
                    // R8-2 修复：检查 incoming 值是否混合了 Reference 和非 Reference
                    let then_is_ref = matches!(&then_value, Value::Reference { .. });
                    let else_is_ref = matches!(&else_value, Value::Reference { .. });

                    if then_is_ref || else_is_ref {
                        // 至少一个分支有闭包捕获（Reference），需要统一为 shared_location
                        // 提取或创建 shared_location，Phi 合并指针
                        let then_loc = if let Value::Reference { value: ref_inner, .. } = &then_value {
                            ref_inner.as_ref().clone()
                        } else {
                            // 非Reference 值：创建 shared_location 并存储值
                            let loc = ctx.new_temp();
                            if !ctx.analysis_mode {
                                ctx.add_statement(Statement::HeapAlloc {
                                    target: loc.clone(),
                                    size: 8,
                                    object_type: "shared_var".to_string(),
                                    span,
                                });
                                ctx.add_statement(Statement::Store {
                                    target: loc.clone(),
                                    value: then_value.clone(),
                                    span,
                                });
                            }
                            loc
                        };

                        let else_loc = if let Value::Reference { value: ref_inner, .. } = &else_value {
                            ref_inner.as_ref().clone()
                        } else {
                            let loc = ctx.new_temp();
                            if !ctx.analysis_mode {
                                ctx.add_statement(Statement::HeapAlloc {
                                    target: loc.clone(),
                                    size: 8,
                                    object_type: "shared_var".to_string(),
                                    span,
                                });
                                ctx.add_statement(Statement::Store {
                                    target: loc.clone(),
                                    value: else_value.clone(),
                                    span,
                                });
                            }
                            loc
                        };

                        let phi_temp = ctx.new_temp();
                        if !ctx.analysis_mode {
                            ctx.add_statement(Statement::Phi {
                                target: phi_temp.clone(),
                                incoming: vec![
                                    (actual_then_block, then_loc),
                                    (actual_else_block, else_loc),
                                ],
                                span,
                            });
                        }
                        // 变量绑定更新为 Reference，指向 Phi 选出的 shared_location
                        ctx.update_variable(name, Value::Reference {
                            value: Box::new(phi_temp),
                            ty: None,
                        }, None);
                    } else {
                        // 正常情况：都不是 Reference
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

            // 快照所有作用域的变量绑定（不仅仅是当前 scope）
            let pre_loop_bindings: std::collections::HashMap<String, (Value, Option<OwnershipKind>)> =
                ctx.scopes
                    .iter()
                    .rev()
                    .flat_map(|scope| scope.bindings.iter().map(|(k, v)| (k.clone(), (v.value.clone(), v.ownership))))
                    .collect();

            // === 第一步：预分析循环体，找出被更新的变量 ===
            // 推入循环上下文，使 break/continue 在预分析阶段也能正常工作
            ctx.loop_stack.push(super::types::LoopContext {
                continue_target: loop_head,
                break_target: loop_exit,
                continue_sources: Vec::new(),
                break_sources: Vec::new(),
            });
            // 预分析模式：不生成 Phi 节点，仅收集变量绑定变化
            ctx.analysis_mode = true;
            let pre_analysis_block_count = ctx.current_function_mut().basic_blocks.len();
            let saved_block = ctx.current_block();
            let analysis_block = ctx.new_block();
            ctx.set_current_block(analysis_block);
            let temp_result = ctx.new_temp();
            let _ = lower_expression(ctx, body, &temp_result);
            ctx.analysis_mode = false;
            // 弹出预分析用的循环上下文
            ctx.loop_stack.pop();

            // 收集所有作用域中变量更新后的绑定
            let post_loop_bindings: std::collections::HashMap<String, (Value, Option<OwnershipKind>)> =
                ctx.scopes
                    .iter()
                    .rev()
                    .flat_map(|scope| scope.bindings.iter().map(|(k, v)| (k.clone(), (v.value.clone(), v.ownership))))
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

            // === 第 2.5 步：预转换结构体变量 ===
            // 对于在循环体内被闭包捕获的结构体变量，预先将其转换为 Reference，
            // 确保 loop_head Phi 能合并相同类型的值（shared_var 指针），
            // 避免迭代间 Struct VALUE 与堆指针类型不一致导致垃圾值。
            // 切换到 saved_block（循环前的块），确保堆分配语句插入到正确位置
            ctx.set_current_block(saved_block);
            let mut struct_ref_vars: std::collections::HashSet<String> = std::collections::HashSet::new();
            for (name, initial_value, loop_value) in &mut updated_vars {
                // 通过 scope 中的 binding.struct_name 判断是否为结构体变量
                let is_struct = ctx.scopes.iter().rev()
                    .find_map(|scope| scope.bindings.get(name))
                    .and_then(|b| b.struct_name.clone())
                    .is_some();
                let becomes_ref = matches!(loop_value, Value::Reference { .. });
                if is_struct && becomes_ref {
                    // 从 scope 获取结构体名称，从 program 获取大小
                    let struct_name = ctx.scopes.iter().rev()
                        .find_map(|scope| scope.bindings.get(name))
                        .and_then(|b| b.struct_name.clone())
                        .unwrap();
                    let struct_size = ctx.program.get_struct_type(&struct_name)
                        .map(|t| t.fields.len().max(1) * 8)
                        .unwrap_or(8);

                    // 在 pre_loop_block 中插入堆分配（在 Goto loop_head 之前）
                    let heap_copy = ctx.new_temp();
                    ctx.add_statement(Statement::HeapAlloc {
                        target: heap_copy.clone(),
                        size: struct_size,
                        object_type: "struct_copy".to_string(),
                        span: *span,
                    });
                    ctx.add_statement(Statement::Store {
                        target: heap_copy.clone(),
                        value: initial_value.clone(),
                        span: *span,
                    });

                    let shared_location = ctx.new_temp();
                    ctx.add_statement(Statement::HeapAlloc {
                        target: shared_location.clone(),
                        size: 8,
                        object_type: "shared_var".to_string(),
                        span: *span,
                    });
                    ctx.add_statement(Statement::Store {
                        target: shared_location.clone(),
                        value: heap_copy,
                        span: *span,
                    });

                    // 更新 initial_value 为 Reference，Phi 将合并 shared_var 指针
                    let ref_value = Value::Reference {
                        value: Box::new(shared_location),
                        ty: None,
                    };
                    *initial_value = ref_value;
                    struct_ref_vars.insert(name.clone());
                    // 更新变量绑定
                    ctx.update_variable(name, initial_value.clone(), None);
                }
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
            // 对于预转换的结构体变量，绑定保持为 Reference(phi_temp)
            for (name, phi_val) in &phi_values {
                if struct_ref_vars.contains(name) {
                    ctx.update_variable(name, Value::Reference {
                        value: Box::new(phi_val.clone()),
                        ty: None,
                    }, None);
                } else {
                    ctx.update_variable(name, phi_val.clone(), None);
                }
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
                continue_sources: Vec::new(),
                break_sources: Vec::new(),
            });

            let temp_body_result = ctx.new_temp();
            lower_expression(ctx, body, &temp_body_result)?;

            // 弹出循环上下文，取出 continue_sources 和 break_sources
            let popped_loop_ctx = ctx.loop_stack.pop().unwrap();
            let while_continue_sources = popped_loop_ctx.continue_sources;
            let while_break_sources = popped_loop_ctx.break_sources;

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

            // R7-1 修复：对于被闭包捕获的变量（binding 变为 Reference），
            // 在 back-edge 块中插入 Dereference 获取实际值，避免 Phi incoming
            // 收到 Reference 而非数值
            let mut actual_backedge_values: std::collections::HashMap<String, Value> = std::collections::HashMap::new();
            let mut actual_continue_values: std::collections::HashMap<(String, crate::BasicBlockId), Value> = std::collections::HashMap::new();
            let mut actual_break_values: std::collections::HashMap<(String, crate::BasicBlockId), Value> = std::collections::HashMap::new();
            for (name, initial_value, _loop_value) in &updated_vars {
                // 处理 back-edge（循环体正常结束）的 Reference 值
                if let Some((final_value, _)) = final_bindings.get(name) {
                    if let Value::Reference { value: ref_target, .. } = final_value {
                        if struct_ref_vars.contains(name) {
                            // 结构体变量：提取内部 shared_var 指针，不做 Dereference
                            // （Dereference 会得到 struct_ptr，而非 struct VALUE）
                            actual_backedge_values.insert(name.clone(), ref_target.as_ref().clone());
                        } else {
                            // 基本类型变量：Dereference 获取实际值
                            let derefed = ctx.new_temp();
                            ctx.add_statement(Statement::Dereference {
                                target: derefed.clone(),
                                reference: *ref_target.clone(),
                                span: body.span(),
                            });
                            actual_backedge_values.insert(name.clone(), derefed);
                        }
                    }
                }
                // 处理 continue 路径的 Reference 值
                for (source_block, cont_bindings) in &while_continue_sources {
                    if let Some(cont_value) = cont_bindings.get(name) {
                        if let Value::Reference { value: ref_target, .. } = cont_value {
                            // 需要在 continue 的来源块中插入 Dereference
                            // 但此时已经离开了来源块，所以在 back-edge 块中处理
                            let derefed = ctx.new_temp();
                            ctx.add_statement(Statement::Dereference {
                                target: derefed.clone(),
                                reference: *ref_target.clone(),
                                span: body.span(),
                            });
                            actual_continue_values.insert((name.clone(), *source_block), derefed);
                        }
                    }
                }
                // 处理 break 路径的 Reference 值
                for (source_block, break_bindings) in &while_break_sources {
                    if let Some(break_value) = break_bindings.get(name) {
                        if let Value::Reference { value: ref_target, .. } = break_value {
                            let derefed = ctx.new_temp();
                            ctx.add_statement(Statement::Dereference {
                                target: derefed.clone(),
                                reference: *ref_target.clone(),
                                span: body.span(),
                            });
                            actual_break_values.insert((name.clone(), *source_block), derefed);
                        }
                    }
                }
            }

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
                // 对于预转换的结构体变量，Phi incoming 使用 Reference 内部的 shared_var 指针
                let phi_initial = if struct_ref_vars.contains(name) {
                    if let Value::Reference { value: inner, .. } = initial_value {
                        inner.as_ref().clone()
                    } else {
                        initial_value.clone()
                    }
                } else {
                    initial_value.clone()
                };
                // 获取循环体正常结束后的值（优先使用解引用后的值）
                let final_value = if let Some(actual) = actual_backedge_values.get(name) {
                    actual.clone()
                } else {
                    final_bindings.get(name)
                        .map(|(v, _)| v.clone())
                        .unwrap_or_else(|| phi_initial.clone())
                };
                // 构建 incoming 列表：初始值 + 正常结束值 + 所有 continue 路径的值
                let mut incoming = vec![
                    (pre_loop_block, phi_initial),
                    (loop_back_edge_block, final_value),
                ];
                // 为每个 continue 来源添加 incoming（优先使用解引用后的值）
                for (source_block, cont_bindings) in &while_continue_sources {
                    let cont_value = if let Some(actual) = actual_continue_values.get(&(name.clone(), *source_block)) {
                        actual.clone()
                    } else {
                        cont_bindings.get(name)
                            .cloned()
                            .unwrap_or_else(|| initial_value.clone())
                    };
                    incoming.push((*source_block, cont_value));
                }
                ctx.add_statement(Statement::Phi {
                    target: phi_temp,
                    incoming,
                    span: *span,
                });
            }

            // 更新 context 指向 phi 结果
            // 对于预转换的结构体变量，绑定保持为 Reference(phi_temp)
            for (name, phi_val) in &phi_values {
                if struct_ref_vars.contains(name) {
                    ctx.update_variable(name, Value::Reference {
                        value: Box::new(phi_val.clone()),
                        ty: None,
                    }, None);
                } else {
                    ctx.update_variable(name, phi_val.clone(), None);
                }
            }

            // 条件求值
            let cond_val = lower_expression_to_temp(ctx, condition)?;
            ctx.set_terminator(Terminator::Branch {
                condition: cond_val,
                then_block: loop_body,
                else_block: loop_exit,
                span: condition.span(),
            });

            // === 第六步：R7-2 修复 — 处理 break 路径的 Phi 节点 ===
            // 如果有 break 路径，需要在 loop_exit 中为被修改的变量创建 Phi 节点，
            // 合并正常退出值（来自 loop_head 的 Phi temp）和 break 路径值
            if !while_break_sources.is_empty() {
                ctx.set_current_block(loop_exit);

                // 收集当前变量绑定（此时指向 loop_head 的 Phi temp）
                let normal_exit_bindings: std::collections::HashMap<String, Value> = ctx.scopes
                    .iter()
                    .rev()
                    .flat_map(|scope| scope.bindings.iter().map(|(k, v)| (k.clone(), v.value.clone())))
                    .collect();

                for (name, initial_value, _loop_value) in &updated_vars {
                    let normal_value = phi_values.get(name)
                        .map(|v| v.clone())
                        .unwrap_or_else(|| initial_value.clone());

                    // 检查是否有 break 路径的值与正常退出值不同
                    let mut need_phi = false;
                    for (source_block, break_bindings) in &while_break_sources {
                        let break_value = if let Some(actual) = actual_break_values.get(&(name.clone(), *source_block)) {
                            actual.clone()
                        } else {
                            break_bindings.get(name)
                                .cloned()
                                .unwrap_or_else(|| normal_value.clone())
                        };
                        if break_value != normal_value {
                            need_phi = true;
                            break;
                        }
                    }

                    if need_phi {
                        // 在 loop_exit 中创建 Phi 节点
                        let exit_phi_temp = ctx.new_temp();
                        let mut incoming = vec![
                            (loop_head, normal_value.clone()),
                        ];
                        for (source_block, break_bindings) in &while_break_sources {
                            let break_value = if let Some(actual) = actual_break_values.get(&(name.clone(), *source_block)) {
                                actual.clone()
                            } else {
                                break_bindings.get(name)
                                    .cloned()
                                    .unwrap_or_else(|| normal_value.clone())
                            };
                            incoming.push((*source_block, break_value));
                        }
                        ctx.add_statement(Statement::Phi {
                            target: exit_phi_temp.clone(),
                            incoming,
                            span: *span,
                        });
                        // 更新变量绑定指向 loop_exit 的 Phi 结果
                        ctx.update_variable(name, exit_phi_temp, None);
                    }
                }
            }

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
            inclusive,
            span,
        } => {
            // for var in start..end { body }
            // 展开为：
            //   let __for_start = start
            //   let __for_end = end
            //   let mut __for_var = __for_start
            //   loop {
            //       if __for_var < __for_end {
            //           let var = __for_var
            //           body         ← break → loop_exit, continue → increment_block
            //           __for_var = __for_var + 1   ← increment_block
            //       } else {
            //           break  → loop_exit
            //       }
            //   }

            let loop_head = ctx.new_block();
            let loop_body = ctx.new_block();
            let increment_block = ctx.new_block(); // 递增块（continue 的目标）
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

            // 快照所有作用域的变量绑定
            let pre_loop_bindings: std::collections::HashMap<String, (Value, Option<OwnershipKind>)> =
                ctx.scopes
                    .iter()
                    .rev()
                    .flat_map(|scope| scope.bindings.iter().map(|(k, v)| (k.clone(), (v.value.clone(), v.ownership))))
                    .collect();

            // === 第一步：预分析循环体，找出被更新的变量 ===
            // 推入循环上下文，使 break/continue 在预分析阶段也能正常工作
            ctx.loop_stack.push(super::types::LoopContext {
                continue_target: increment_block,
                break_target: loop_exit,
                continue_sources: Vec::new(),
                break_sources: Vec::new(),
            });

            ctx.analysis_mode = true;
            let pre_analysis_block_count = ctx.current_function_mut().basic_blocks.len();
            let saved_block = ctx.current_block();
            let analysis_block = ctx.new_block();
            ctx.set_current_block(analysis_block);

            // 在分析块中绑定循环变量
            let analysis_var_temp = ctx.new_temp();
            ctx.bind_variable(var.clone(), analysis_var_temp, None);
            // 模拟递增
            let analysis_inc_temp = ctx.new_temp();
            ctx.add_statement(Statement::Assign {
                target: analysis_inc_temp.clone(),
                source: Value::Number { value: 1, ty: None },
                span: *span,
            });

            let temp_result = ctx.new_temp();
            let _ = lower_expression(ctx, body, &temp_result);
            ctx.analysis_mode = false;

            // 弹出预分析用的循环上下文
            ctx.loop_stack.pop();

            // 收集所有作用域中变量更新后的绑定
            let post_loop_bindings: std::collections::HashMap<String, (Value, Option<OwnershipKind>)> =
                ctx.scopes
                    .iter()
                    .rev()
                    .flat_map(|scope| scope.bindings.iter().map(|(k, v)| (k.clone(), (v.value.clone(), v.ownership))))
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

            // === 第二步：恢复状态 ===
            ctx.remove_block(analysis_block);
            for (name, (value, ownership)) in &pre_loop_bindings {
                ctx.update_variable(name, value.clone(), *ownership);
            }

            // 确保循环变量和 __for_var 在绑定中
            ctx.bind_variable(for_var_name.clone(), for_var_temp.clone(), None);

            // === 第 2.5 步：预转换结构体变量 ===
            // 对于在循环体内被闭包捕获的结构体变量，预先将其转换为 Reference，
            // 确保 loop_head Phi 能合并相同类型的值（shared_var 指针），
            // 避免迭代间 Struct VALUE 与堆指针类型不一致导致垃圾值。
            // 切换到 saved_block（循环前的块），确保堆分配语句插入到正确位置
            ctx.set_current_block(saved_block);
            let mut struct_ref_vars: std::collections::HashSet<String> = std::collections::HashSet::new();
            for (name, initial_value, loop_value) in &mut updated_vars {
                // 通过 scope 中的 binding.struct_name 判断是否为结构体变量
                let is_struct = ctx.scopes.iter().rev()
                    .find_map(|scope| scope.bindings.get(name))
                    .and_then(|b| b.struct_name.clone())
                    .is_some();
                let becomes_ref = matches!(loop_value, Value::Reference { .. });
                if is_struct && becomes_ref {
                    // 从 scope 获取结构体名称，从 program 获取大小
                    let struct_name = ctx.scopes.iter().rev()
                        .find_map(|scope| scope.bindings.get(name))
                        .and_then(|b| b.struct_name.clone())
                        .unwrap();
                    let struct_size = ctx.program.get_struct_type(&struct_name)
                        .map(|t| t.fields.len().max(1) * 8)
                        .unwrap_or(8);

                    // 在 pre_loop_block 中插入堆分配（在 Goto loop_head 之前）
                    let heap_copy = ctx.new_temp();
                    ctx.add_statement(Statement::HeapAlloc {
                        target: heap_copy.clone(),
                        size: struct_size,
                        object_type: "struct_copy".to_string(),
                        span: *span,
                    });
                    ctx.add_statement(Statement::Store {
                        target: heap_copy.clone(),
                        value: initial_value.clone(),
                        span: *span,
                    });

                    let shared_location = ctx.new_temp();
                    ctx.add_statement(Statement::HeapAlloc {
                        target: shared_location.clone(),
                        size: 8,
                        object_type: "shared_var".to_string(),
                        span: *span,
                    });
                    ctx.add_statement(Statement::Store {
                        target: shared_location.clone(),
                        value: heap_copy,
                        span: *span,
                    });

                    // 更新 initial_value 为 Reference，Phi 将合并 shared_var 指针
                    let ref_value = Value::Reference {
                        value: Box::new(shared_location),
                        ty: None,
                    };
                    *initial_value = ref_value;
                    struct_ref_vars.insert(name.clone());
                    // 更新变量绑定
                    ctx.update_variable(name, initial_value.clone(), None);
                }
            }

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
            // 对于预转换的结构体变量，绑定保持为 Reference(phi_temp)
            for (name, phi_val) in &phi_values {
                if struct_ref_vars.contains(name) {
                    ctx.update_variable(name, Value::Reference {
                        value: Box::new(phi_val.clone()),
                        ty: None,
                    }, None);
                } else {
                    ctx.update_variable(name, phi_val.clone(), None);
                }
            }

            // Jump to loop head
            ctx.set_current_block(saved_block);
            ctx.set_terminator(Terminator::Goto {
                target: loop_head,
                span: *span,
            });

            // === 第四步：生成循环头（phi + 条件判断）===
            ctx.set_current_block(loop_head);

            // Phi 节点（用户变量）
            for (name, initial_value, _loop_value) in &updated_vars {
                let phi_temp = phi_values.get(name).unwrap().clone();
                // 对于预转换的结构体变量，Phi incoming 使用 Reference 内部的 shared_var 指针
                let phi_initial = if struct_ref_vars.contains(name) {
                    if let Value::Reference { value: inner, .. } = initial_value {
                        inner.as_ref().clone()
                    } else {
                        initial_value.clone()
                    }
                } else {
                    initial_value.clone()
                };
                // back edge 的值稍后填入（先占位，第五步更新）
                ctx.add_statement(Statement::Phi {
                    target: phi_temp,
                    incoming: vec![
                        (pre_loop_block, phi_initial.clone()),
                        // 占位：increment_block 的值在循环体生成后更新
                        (increment_block, phi_initial),
                    ],
                    span: *span,
                });
            }
            // __for_var 的 phi（同样先占位）
            ctx.add_statement(Statement::Phi {
                target: for_var_phi.clone(),
                incoming: vec![
                    (pre_loop_block, for_var_temp.clone()),
                    (increment_block, for_var_temp.clone()),
                ],
                span: *span,
            });

            // 更新 context 中所有被循环修改的变量指向 phi 结果
            // 对于预转换的结构体变量，绑定保持为 Reference(phi_temp)
            for (name, phi_val) in &phi_values {
                if struct_ref_vars.contains(name) {
                    ctx.update_variable(name, Value::Reference {
                        value: Box::new(phi_val.clone()),
                        ty: None,
                    }, None);
                } else {
                    ctx.update_variable(name, phi_val.clone(), None);
                }
            }

            // 条件: __for_var < end (exclusive) 或 __for_var <= end (inclusive)
            let for_var_phi_val = phi_values.get(&for_var_name).unwrap().clone();
            let end_val_in_header = lower_expression_to_temp(ctx, end)?;
            let cond_temp = ctx.new_temp();
            let cmp_op = if *inclusive {
                crate::BinaryOperator::LessEqual
            } else {
                crate::BinaryOperator::LessThan
            };
            ctx.add_statement(Statement::BinaryOp {
                op: cmp_op,
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

            // === 第五步：生成循环体 ===
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
            // continue → increment_block（确保递增不会跳过）
            // break → loop_exit
            ctx.loop_stack.push(super::types::LoopContext {
                continue_target: increment_block,
                break_target: loop_exit,
                continue_sources: Vec::new(),
                break_sources: Vec::new(),
            });

            let temp_body_result = ctx.new_temp();
            lower_expression(ctx, body, &temp_body_result)?;

            // 弹出循环上下文，取出 continue_sources 和 break_sources
            let popped_for_ctx = ctx.loop_stack.pop().unwrap();
            let for_continue_sources = popped_for_ctx.continue_sources;
            let for_break_sources = popped_for_ctx.break_sources;

            // 收集循环体正常结束后的变量绑定
            let normal_end_bindings: std::collections::HashMap<String, (Value, Option<OwnershipKind>)> =
                ctx.scopes
                    .iter()
                    .rev()
                    .flat_map(|scope| {
                        scope.bindings.iter().map(|(k, v)| (k.clone(), (v.value.clone(), v.ownership)))
                    })
                    .collect();

            // 循环体正常结束 → 跳转到 increment_block
            ctx.set_terminator(Terminator::Goto {
                target: increment_block,
                span: body.span(),
            });

            // 记录循环体正常结束的块（用于 increment_block 的 phi）
            let normal_end_block = ctx.current_block();

            // === 第六步：生成递增块 ===
            ctx.set_current_block(increment_block);

            // 如果有 continue 路径，需要在 increment_block 中为用户变量添加 phi
            // 合并正常结束路径和 continue 路径的变量值
            let mut inc_phi_values: std::collections::HashMap<String, Value> = std::collections::HashMap::new();
            if !for_continue_sources.is_empty() {
                for (name, _initial_value, _loop_value) in &updated_vars {
                    let inc_phi_temp = ctx.new_temp();
                    // 正常结束路径的值
                    let normal_value = normal_end_bindings.get(name)
                        .map(|(v, _)| v.clone())
                        .unwrap_or_else(|| _initial_value.clone());
                    // 构建 incoming：正常结束块 + 所有 continue 来源块
                    let mut incoming = vec![(normal_end_block, normal_value)];
                    for (source_block, cont_bindings) in &for_continue_sources {
                        let cont_value = cont_bindings.get(name)
                            .cloned()
                            .unwrap_or_else(|| _initial_value.clone());
                        incoming.push((*source_block, cont_value));
                    }
                    ctx.add_statement(Statement::Phi {
                        target: inc_phi_temp.clone(),
                        incoming,
                        span: *span,
                    });
                    inc_phi_values.insert(name.clone(), inc_phi_temp);
                }
                // 更新 context 中的变量绑定指向 increment_block 的 phi 结果
                for (name, inc_phi_val) in &inc_phi_values {
                    ctx.update_variable(name, inc_phi_val.clone(), None);
                }
            }

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
            ctx.update_variable(&for_var_name, inc_temp.clone(), None);

            // 收集 increment_block 中所有变量更新后的值（用于修正 loop_head 的 phi）
            let final_bindings: std::collections::HashMap<String, (Value, Option<OwnershipKind>)> =
                ctx.scopes
                    .iter()
                    .rev()
                    .flat_map(|scope| {
                        scope.bindings.iter().map(|(k, v)| (k.clone(), (v.value.clone(), v.ownership)))
                    })
                    .collect();

            // R8-1 修复：对被闭包捕获的变量（binding 变为 Reference），
            // 在 increment_block 中插入 Dereference 获取实际值，避免 Phi incoming
            // 收到 Reference 而非数值
            // 对于预转换的结构体变量，直接提取 Reference 内部的 shared_var 指针，
            // 不做 Dereference（因为 Dereference 会得到 struct_ptr，而非 struct VALUE）
            let mut actual_backedge_values: std::collections::HashMap<String, Value> = std::collections::HashMap::new();
            for (name, _initial_value, _loop_value) in &updated_vars {
                // 处理正常结束路径的 Reference 值
                if let Some((final_value, _)) = final_bindings.get(name) {
                    if let Value::Reference { value: ref_target, .. } = final_value {
                        if struct_ref_vars.contains(name) {
                            // 结构体变量：提取内部 shared_var 指针，不做 Dereference
                            actual_backedge_values.insert(name.clone(), ref_target.as_ref().clone());
                        } else {
                            // 基本类型变量：Dereference 获取实际值
                            let derefed = ctx.new_temp();
                            ctx.add_statement(Statement::Dereference {
                                target: derefed.clone(),
                                reference: *ref_target.clone(),
                                span: body.span(),
                            });
                            actual_backedge_values.insert(name.clone(), derefed);
                        }
                    }
                }

            }

            // 递增块 → loop_head
            ctx.set_terminator(Terminator::Goto {
                target: loop_head,
                span: *span,
            });

            // === 第七步：修正 loop_head 中 phi 的 back-edge incoming 值 ===
            // 遍历 loop_head 的语句，找到占位的 phi 节点，用实际值替换
            // 使用 R8-1 修复后的解引用值（如果变量被闭包捕获）
            let loop_head_block = ctx.current_function_mut().basic_blocks.get_mut(&loop_head).unwrap();
            for stmt in &mut loop_head_block.statements {
                if let Statement::Phi { target, incoming, .. } = stmt {
                    // 找到 increment_block 对应的 incoming，用实际值替换
                    for (block, value) in incoming.iter_mut() {
                        if *block == increment_block {
                            // 查找 target 对应的变量名
                            if *target == for_var_phi {
                                // __for_var 的 phi back edge
                                *value = final_bindings.get(&for_var_name)
                                    .map(|(v, _)| v.clone())
                                    .unwrap_or_else(|| for_var_temp.clone());
                            } else {
                                // 用户变量的 phi back edge
                                // 通过 phi_values 反查变量名
                                let var_name = phi_values.iter()
                                    .find(|(_, v)| **v == *target)
                                    .map(|(k, _)| k.clone());
                                if let Some(name) = var_name {
                                    // R8-1 修复：优先使用解引用后的值
                                    if let Some(actual) = actual_backedge_values.get(&name) {
                                        *value = actual.clone();
                                    } else {
                                        *value = final_bindings.get(&name)
                                            .map(|(v, _)| v.clone())
                                            .unwrap_or_else(|| {
                                                pre_loop_bindings.get(&name)
                                                    .map(|(v, _)| v.clone())
                                                    .unwrap_or_else(|| value.clone())
                                            });
                                    }
                                }
                            }
                        }
                    }

                }
            }

            // 修复 R12-1：恢复变量绑定指向 loop_head 的 phi temp
            // 循环体 lowering 可能修改了绑定（包括 break 后死块中的赋值），
            // 需要恢复到 phi temp 以确保 break phi 和循环后的代码使用正确的值。
            // 这与 while 循环在 line 822-833 的逻辑对应。
            for (name, phi_val) in &phi_values {
                if struct_ref_vars.contains(name) {
                    ctx.update_variable(name, Value::Reference {
                        value: Box::new(phi_val.clone()),
                        ty: None,
                    }, None);
                } else {
                    ctx.update_variable(name, phi_val.clone(), None);
                }
            }

            // R7-2 修复：处理 for-in 循环 break 路径的 Phi 节点
            if !for_break_sources.is_empty() {
                ctx.set_current_block(loop_exit);

                for (name, _initial_value, _loop_value) in &updated_vars {
                    // for-in 的正常退出值来自 loop_head 的 Phi temp
                    let normal_value = phi_values.get(name)
                        .map(|v| v.clone())
                        .unwrap_or_else(|| _initial_value.clone());

                    // 检查是否有 break 路径值与正常退出值不同
                    let mut need_phi = false;
                    for (source_block, break_bindings) in &for_break_sources {
                        let break_value = break_bindings.get(name)
                            .cloned()
                            .unwrap_or_else(|| normal_value.clone());
                        if break_value != normal_value {
                            need_phi = true;
                            break;
                        }
                    }

                    if need_phi {
                        let exit_phi_temp = ctx.new_temp();
                        let mut incoming = vec![
                            (loop_head, normal_value.clone()),
                        ];
                        for (source_block, break_bindings) in &for_break_sources {
                            let break_value = break_bindings.get(name)
                                .cloned()
                                .unwrap_or_else(|| normal_value.clone());
                            incoming.push((*source_block, break_value));
                        }
                        ctx.add_statement(Statement::Phi {
                            target: exit_phi_temp.clone(),
                            incoming,
                            span: *span,
                        });
                        ctx.update_variable(name, exit_phi_temp, None);
                    }
                }
            }

            // Continue from exit block
            ctx.set_current_block(loop_exit);
            ctx.add_statement(Statement::Assign {
                target: destination.clone(),
                source: Value::Unit,
                span: *span,
            });
        }


        Expr::Break { span, .. } => {
            // break: 跳转到当前循环的退出块，同时记录当前变量绑定
            if ctx.loop_stack.last().is_some() {
                // 记录 break 来源块 ID 和当时的变量绑定（与 continue 一致）
                let source_block = ctx.current_block();
                let bindings: std::collections::HashMap<String, Value> = ctx.scopes
                    .iter()
                    .rev()
                    .flat_map(|scope| scope.bindings.iter().map(|(k, v)| (k.clone(), v.value.clone())))
                    .collect();
                let break_target = ctx.loop_stack.last().unwrap().break_target;
                ctx.loop_stack.last_mut().unwrap().break_sources.push((source_block, bindings));

                ctx.set_terminator(Terminator::Goto {
                    target: break_target,
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
            // continue: 跳转到当前循环的 continue 目标块
            if ctx.loop_stack.last().is_some() {
                // 记录 continue 来源块 ID 和当时的变量绑定
                let source_block = ctx.current_block();
                let bindings: std::collections::HashMap<String, Value> = ctx.scopes
                    .iter()
                    .rev()
                    .flat_map(|scope| scope.bindings.iter().map(|(k, v)| (k.clone(), v.value.clone())))
                    .collect();
                let continue_target = ctx.loop_stack.last().unwrap().continue_target;
                // 记录到循环上下文中
                ctx.loop_stack.last_mut().unwrap().continue_sources.push((source_block, bindings));
                ctx.set_terminator(Terminator::Goto {
                    target: continue_target,
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

        Expr::Constructor { name, args, .. } => {
            let arg_values: Vec<Value> = args
                .iter()
                .map(|a| lower_expression_to_temp(ctx, a))
                .collect::<Result<Vec<_>, _>>()?;
            let constructor_value = Value::Constructor {
                name: name.clone(),
                args: arg_values,
                ty: None,
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
            args,
            ..
        } => {
            let arg_values: Vec<Value> = args
                .iter()
                .map(|a| lower_expression_to_temp(ctx, a))
                .collect::<Result<Vec<_>, _>>()?;
            let constructor_value = Value::QualifiedConstructor {
                type_name: type_name.clone(),
                constructor_name: constructor_name.clone(),
                args: arg_values,
                ty: None,
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

            // 保存 match 之前的变量绑定快照（遍历所有作用域）
            let pre_match_bindings: std::collections::HashMap<String, Value> = ctx
                .scopes
                .iter()
                .rev()
                .flat_map(|scope| {
                    scope.bindings.iter().map(|(k, v)| (k.clone(), v.value.clone()))
                })
                .collect();

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

            // 5. 为每个分支生成代码，并收集 Phi 信息
            let mut arm_actual_blocks: Vec<BasicBlockId> = Vec::new();
            let mut arm_bindings_list: Vec<std::collections::HashMap<String, Value>> = Vec::new();
            // 记录每个 arm 是否提前终止（continue/break/return）
            let mut arm_terminated_early: Vec<bool> = Vec::new();

            for (i, arm) in arms.iter().enumerate() {
                let arm_block = arm_blocks[i];
                ctx.set_current_block(arm_block);

                // 处理模式绑定（如果有的话）
                ctx.enter_scope();
                handle_pattern_bindings(ctx, &arm.pattern, &match_value)?;

                // 生成分支体的代码
                lower_expression(ctx, &arm.body, destination)?;
                ctx.exit_scope(arm.span);

                // 检测 arm 体是否提前终止（continue/break/return 创建了 dead_block）
                // 判断方式：如果当前块不是 arm_block，且 arm_block 已有终结器（不是 Goto merge），
                // 说明 arm 体中的 continue/break/return 已经设置了终结器并创建了死块
                let actual_block = ctx.current_block();
                let terminated_early = if actual_block != arm_block {
                    // 当前块与初始 arm_block 不同，检查 arm_block 的终结器
                    // 先提取终结器信息（释放对 ctx 的可变借用），再检查 loop_stack
                    let terminator_info = {
                        let func = ctx.current_function_mut();
                        func.basic_blocks.get(&arm_block)
                            .and_then(|b| b.terminator.clone())
                    };
                    match terminator_info {
                        // Return 是真正的提前终止
                        Some(Terminator::Return { .. }) => true,
                        // Goto 需要区分：如果目标是外层循环的 break/continue 目标，
                        // 则是提前终止；如果是内部 while/for 的 loop_head，
                        // 则是嵌套控制流，不是提前终止
                        Some(Terminator::Goto { target, .. }) => {
                            ctx.loop_stack.iter().any(|lc| {
                                lc.continue_target == target || lc.break_target == target
                            })
                        }
                        // Match、Branch 等是嵌套控制流，不是提前终止
                        Some(_) => false,
                        None => false,
                    }
                } else {
                    false
                };
                arm_terminated_early.push(terminated_early);

                if !terminated_early {
                    // 正常 arm：捕获实际跳转到 merge 的块
                    arm_actual_blocks.push(actual_block);
                    ctx.set_terminator(Terminator::Goto {
                        target: merge_block,
                        span: arm.span,
                    });

                    // 记录此 arm 后的变量绑定
                    let arm_bindings: std::collections::HashMap<String, Value> = ctx
                        .scopes
                        .iter()
                        .rev()
                        .flat_map(|scope| {
                            scope.bindings.iter().map(|(k, v)| (k.clone(), v.value.clone()))
                        })
                        .collect();
                    arm_bindings_list.push(arm_bindings);
                }
                // 提前终止的 arm（continue/break/return）不会到达 merge_block，
                // 不参与 Phi，也不设置 Goto merge（死块不应有到达 merge 的边）

                // 恢复绑定到 match 之前的状态（下一个 arm 需从原始状态开始）
                for scope in ctx.scopes.iter_mut() {
                    for (name, binding) in scope.bindings.iter_mut() {
                        if let Some(pre_val) = pre_match_bindings.get(name) {
                            binding.value = pre_val.clone();
                        }
                    }
                }
            }

            // 6. 切换到合并块，为被修改的变量插入 N 路 Phi 节点
            ctx.set_current_block(merge_block);

            for (name, pre_value) in &pre_match_bindings {
                // 收集每个 arm 的值
                let arm_values: Vec<Value> = arm_bindings_list
                    .iter()
                    .map(|bindings| {
                        bindings.get(name).cloned().unwrap_or_else(|| pre_value.clone())
                    })
                    .collect();

                // 检查是否有任何 arm 修改了此变量
                let any_changed = arm_values.iter().any(|v| v != pre_value);

                if any_changed {
                    // 检查 incoming 值是否混合了 Reference 和非 Reference
                    let any_is_ref = arm_values.iter().any(|v| matches!(v, Value::Reference { .. }));

                    if any_is_ref {
                        // 至少一个 arm 有闭包捕获（Reference），需要统一为 shared_location
                        let mut incoming: Vec<(BasicBlockId, Value)> = Vec::new();

                        for (arm_idx, arm_value) in arm_values.iter().enumerate() {
                            let arm_actual_block = arm_actual_blocks[arm_idx];
                            let loc = if let Value::Reference { value: ref_inner, .. } = arm_value {
                                ref_inner.as_ref().clone()
                            } else {
                                // 非 Reference 值：创建 shared_location 并存储值
                                let loc = ctx.new_temp();
                                if !ctx.analysis_mode {
                                    ctx.add_statement(Statement::HeapAlloc {
                                        target: loc.clone(),
                                        size: 8,
                                        object_type: "shared_var".to_string(),
                                        span,
                                    });
                                    ctx.add_statement(Statement::Store {
                                        target: loc.clone(),
                                        value: arm_value.clone(),
                                        span,
                                    });
                                }
                                loc
                            };
                            incoming.push((arm_actual_block, loc));
                        }

                        let phi_temp = ctx.new_temp();
                        if !ctx.analysis_mode {
                            ctx.add_statement(Statement::Phi {
                                target: phi_temp.clone(),
                                incoming,
                                span,
                            });
                        }
                        // 变量绑定更新为 Reference，指向 Phi 选出的 shared_location
                        ctx.update_variable(name, Value::Reference {
                            value: Box::new(phi_temp),
                            ty: None,
                        }, None);
                    } else {
                        // 正常情况：都不是 Reference
                        let incoming: Vec<(BasicBlockId, Value)> = arm_actual_blocks
                            .iter()
                            .zip(arm_values.iter())
                            .map(|(block, value): (&BasicBlockId, &Value)| (*block, value.clone()))
                            .collect();

                        let phi_temp = ctx.new_temp();
                        if !ctx.analysis_mode {
                            ctx.add_statement(Statement::Phi {
                                target: phi_temp.clone(),
                                incoming,
                                span,
                            });
                        }
                        ctx.update_variable(name, phi_temp, None);
                    }
                }
            }
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
            // 计算元素大小：从数组表达式的类型中获取元素类型，计算其字节大小
            let element_size = if let Type::Array { element } = ctx.get_expr_type(expr) {
                element.byte_size().max(8) // 至少8字节步幅，保证对齐
            } else {
                8 // 默认回退
            };
            let slot_count = elements.len() + 1; // length slot + elements
            let array_data_size = elements.len() * element_size;
            let layout = HeapLayout {
                type_id: format!("array:{}", elements.len()),
                size: 8 + array_data_size,
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
                        value: (8 + idx * element_size) as i64,
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

            // 从数组类型获取元素类型，计算元素大小
            let el_type = ctx.get_expr_type(array);
            let element_size = if let Type::Array { element } = &el_type {
                element.byte_size().max(8)
            } else {
                8
            };

            let scaled_index = ctx.new_temp();
            ctx.add_statement(Statement::BinaryOp {
                target: scaled_index.clone(),
                left: index_value,
                op: MirBinaryOp::Multiply,
                right: Value::Number { value: element_size as i64, ty: None },
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

            // 对于结构体/元组元素，直接返回元素指针（不解引用），
            // 后续 FieldAccess 才能正确使用基地址计算字段偏移
            let is_struct = match &el_type {
                Type::Array { element } => matches!(element.as_ref(),
                    Type::Struct { .. } | Type::Tuple(_)
                ),
                _ => false,
            };

            if is_struct {
                // 结构体元素：直接赋值元素指针（地址）
                ctx.add_statement(Statement::Assign {
                    target: destination.clone(),
                    source: element_ptr,
                    span: *span,
                });
            } else {
                // 简单类型元素：解引用获取值
                ctx.add_statement(Statement::Dereference {
                    target: destination.clone(),
                    reference: element_ptr,
                    span: *span,
                });
            }
        }

        Expr::ArrayLen { array, span } => {
            let array_value = lower_expression_to_temp(ctx, array)?;
            ctx.add_statement(Statement::Dereference {
                target: destination.clone(),
                reference: array_value,
                span: *span,
            });
        }

        Expr::TupleLiteral { elements, span } => {
            // 将元组转换为匿名结构体: (a, b, c) → Struct { name: "__tuple_3", fields: { _0: a, _1: b, _2: c } }
            let n = elements.len();
            let struct_name = format!("__tuple_{}", n);

            let mut mir_fields = std::collections::BTreeMap::new();
            for (i, elem) in elements.iter().enumerate() {
                let elem_value = lower_expression_to_temp(ctx, elem)?;
                mir_fields.insert(format!("_{}", i), elem_value);
            }

            let struct_value = Value::Struct {
                name: struct_name,
                fields: mir_fields,
                ty: None,
            };

            ctx.add_statement(Statement::Assign {
                target: destination.clone(),
                source: struct_value,
                span: *span,
            });
        }

        Expr::TupleAccess { object, index, span } => {
            // 元组索引访问转为字段访问: t.1 → field_access(t, "_1")
            let object_value = lower_expression_to_temp(ctx, object)?;
            ctx.add_statement(Statement::FieldAccess {
                target: destination.clone(),
                object: object_value,
                field: format!("_{}", index),
                span: *span,
            });
        }

        Expr::Reference { expr, span } => {
            // R7-3 修复：对于简单变量引用，直接使用变量的绑定值作为引用目标，
            // 避免创建副本导致引用和原变量不共享存储。
            // 当变量通过 FieldAssign 修改时，引用能正确看到更新。
            let reference_target = match expr.as_ref() {
                karte_hir::Expr::Identifier { name, .. } => {
                    if let Some(binding) = ctx.lookup_variable(name) {
                        // 变量已绑定为 Reference（如被闭包捕获），直接透传
                        if let Value::Reference { .. } = &binding.value {
                            // 已是引用类型，直接赋值（不再嵌套包装）
                            let existing_ref = binding.value.clone();
                            ctx.add_statement(Statement::Assign {
                                target: destination.clone(),
                                source: existing_ref,
                                span: *span,
                            });
                            return Ok(());
                        }
                        // 普通变量：直接使用绑定值，不创建副本
                        binding.value.clone()
                    } else {
                        // 未找到变量，走默认路径（会报错）
                        lower_expression_to_temp(ctx, expr)?
                    }
                }
                _ => {
                    // 非简单变量表达式，走默认路径
                    lower_expression_to_temp(ctx, expr)?
                }
            };

            // 创建引用值并赋值给目标
            let reference_value = Value::Reference {
                value: Box::new(reference_target),
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

                // 解析绑定值，检查变量是否已被外层闭包捕获
                let resolved_value = ctx.resolve_value(&binding.value);

                match &resolved_value {
                    // 变量已被外层闭包捕获（值为 Reference）
                    // 此时 Reference 内部的 Temp 已经持有 shared_location 的地址，
                    // 直接复用，不需要创建新的堆分配和间接层
                    Value::Reference { value: inner, .. } => {
                        captured_var_locations.push(inner.as_ref().clone());
                    }
                    // 变量首次被捕获，创建新的 shared_location（堆分配）
                    _ => {
                        // 结构体变量的闭包捕获需要特殊处理：
                        // 基本类型（number, bool）：shared_location 直接存储值（8字节）
                        // 结构体类型：需要先在堆上创建结构体副本，shared_location 存储
                        // 指向堆副本的指针。因为 lambda 中通过 Dereference + FieldAccess
                        // 访问字段，Dereference 返回的必须是指向结构体的指针（不是值）。
                        if let Some(ref struct_name) = binding.struct_name {
                            // 结构体变量：两步堆分配
                            let struct_type = ctx.program.get_struct_type(struct_name);
                            let struct_size = struct_type
                                .map(|t| t.fields.len().max(1) * 8)
                                .unwrap_or(8);
                            
                            // 第一步：在堆上分配结构体副本
                            let heap_copy = ctx.new_temp();
                            ctx.add_statement(Statement::HeapAlloc {
                                target: heap_copy.clone(),
                                size: struct_size,
                                object_type: "struct_copy".to_string(),
                                span,
                            });
                            // 将结构体值复制到堆副本
                            ctx.add_statement(Statement::Store {
                                target: heap_copy.clone(),
                                value: binding.value.clone(),
                                span,
                            });
                            
                            // 第二步：shared_location 存储指向堆副本的指针（8字节）
                            let shared_location = ctx.new_temp();
                            ctx.add_statement(Statement::HeapAlloc {
                                target: shared_location.clone(),
                                size: 8,
                                object_type: "shared_var".to_string(),
                                span,
                            });
                            // 存储堆副本的地址到 shared_location
                            ctx.add_statement(Statement::Store {
                                target: shared_location.clone(),
                                value: heap_copy,
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
                        } else {
                            // 基本类型变量：shared_location 直接存储值
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
/// ~~旧版~~：如果函数返回类型是 Function 或 Closure，往 temp_value_map 插入虚假占位符。
/// 这导致运行时的真实 Closure 结构体被覆盖为 Value::Function（名字是虚假的），
/// 后续调用生成 Statement::Call 指向不存在的函数标签，JIT 报 "未定义的标签"。
///
/// 正确做法：不做任何覆盖。函数返回的实际运行时值（Closure 结构体或函数指针）
/// 已由 MIR lowering 正确生成（lower_lambda_expression / 函数体 lowering）。
/// 保持 Value::Temp 不被覆盖，后续 resolve_value 返回 Temp 本身，
/// 调用路径走通用的 Closure 结构体间接调用（FieldAccess + CallIndirect）。
fn annotate_function_return_type(
    _ctx: &mut LoweringContext,
    _function_name: &str,
    _destination: &Value,
) {
    // 故意为空：不再往 temp_value_map 插入虚假占位符。
    // 运行时值由 MIR lowering 正确生成，不需要类型注解覆盖。
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

            // 根据参数的实际推断类型选择运行时函数
            let arg_type = ctx.get_expr_type(&args[0]);
            let runtime_fn = match &arg_type {
                karte_hir::Type::String => "__runtime_print_string",
                karte_hir::Type::Bool => "__runtime_print_bool",
                _ => "__runtime_print_number",
            };

            ctx.add_statement(Statement::Call {
                target: Some(destination.clone()),
                function: Value::Function {
                    name: runtime_fn.to_string(),
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
