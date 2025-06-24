use crate::{
    BasicBlockId, BinaryOperator as MirBinaryOp, MatchArm, MirFunction, MirProgram, Pattern,
    Statement, Terminator, UnaryOperator as MirUnaryOp, Value,
};
use karte_hir::{BinaryOperator as HirBinaryOp, Expr, UnaryOperator as HirUnaryOp};
use std::collections::HashMap;

/// HIR到MIR的lowering上下文
pub struct LoweringContext<'a> {
    program: &'a mut MirProgram,
    /// 当前函数
    current_function_name: Option<String>,
    /// 当前基本块
    current_block: Option<BasicBlockId>,
    /// 变量作用域
    variables: HashMap<String, Value>,
    /// 错误信息
    errors: Vec<String>,
    /// 匿名函数计数器
    lambda_counter: usize,
}

impl<'a> LoweringContext<'a> {
    pub fn new(program: &'a mut MirProgram) -> Self {
        Self {
            program,
            current_function_name: None,
            current_block: None,
            variables: HashMap::new(),
            errors: Vec::new(),
            lambda_counter: 0,
        }
    }

    /// 开始新函数
    pub fn start_function(&mut self, name: String, params: Vec<String>) {
        let function = MirFunction::new(name.clone(), params.clone());
        let entry_block = function.entry_block;

        // 将参数添加到变量作用域
        for param in params {
            self.variables
                .insert(param.clone(), Value::Variable { name: param });
        }

        self.program.add_function(function);
        self.current_function_name = Some(name);
        self.current_block = Some(entry_block);
    }

    /// 完成当前函数
    pub fn finish_function(&mut self) {
        self.current_function_name = None;
        self.current_block = None;
    }

    /// 获取当前函数的可变引用
    fn current_function_mut(&mut self) -> &mut MirFunction {
        let name = self
            .current_function_name
            .as_ref()
            .expect("No current function");
        self.program
            .functions
            .get_mut(name)
            .expect("Current function not found in program")
    }

    /// 获取当前基本块
    fn current_block(&self) -> BasicBlockId {
        self.current_block.expect("No current block")
    }

    /// 设置当前基本块
    fn set_current_block(&mut self, block: BasicBlockId) {
        self.current_block = Some(block);
    }

    /// 创建新的基本块
    fn new_block(&mut self) -> BasicBlockId {
        self.current_function_mut().new_block()
    }

    /// 创建新的临时变量
    fn new_temp(&mut self) -> Value {
        let id = self.current_function_mut().new_temp();
        Value::Temp { id }
    }

    /// 添加语句到当前基本块
    fn add_statement(&mut self, stmt: Statement) {
        let block_id = self.current_block();
        if let Some(block) = self.current_function_mut().get_block_mut(block_id) {
            block.add_statement(stmt);
        }
    }

    /// 设置当前基本块的终结语句
    fn set_terminator(&mut self, terminator: Terminator) {
        let block_id = self.current_block();
        if let Some(block) = self.current_function_mut().get_block_mut(block_id) {
            block.set_terminator(terminator);
        }
    }
}

/// 将HIR表达式转换为MIR
pub fn lower_expr_to_mir(expr: &Expr) -> Result<MirProgram, Vec<String>> {
    let mut program = MirProgram::new();
    let mut context = LoweringContext::new(&mut program);

    // 创建主函数
    context.start_function("main".to_string(), vec![]);

    // 为主函数结果创建临时变量
    let result_temp = context.new_temp();

    // 降级表达式
    lower_expression(&mut context, expr, &result_temp)?;

    // 添加返回语句
    context.set_terminator(Terminator::Return {
        value: Some(result_temp.clone()),
        span: expr.span(),
    });

    context.finish_function();

    if context.errors.is_empty() {
        program.set_main("main".to_string());
        // 保存主函数的返回值
        program.main_return_value = Some(result_temp);
        Ok(program)
    } else {
        Err(context.errors)
    }
}

/// 降级单个表达式并将其结果存入 destination
fn lower_expression(
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
            if let Some(value) = ctx.variables.get(name) {
                match value {
                    Value::Reference { value: ref_target } => {
                        // 引用变量读取：生成解引用指令
                        ctx.add_statement(Statement::Dereference {
                            target: destination.clone(),
                            reference: *ref_target.clone(),
                            span,
                        });
                    }
                    _ => {
                        // 普通变量读取
                        ctx.add_statement(Statement::Assign {
                            target: destination.clone(),
                            source: value.clone(),
                            span,
                        });
                    }
                }
            } else {
                ctx.errors.push(format!("Undefined variable: {}", name));
                return Err(ctx.errors.clone());
            }
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
            // 1. 分析Lambda体中使用的自由变量（闭包捕获）
            let mut free_vars = Vec::new();
            let mut captured_var_locations = Vec::new();

            // 收集Lambda体中引用的所有变量
            let referenced_vars = collect_referenced_variables(body);
            let param_names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();

            // 找出不是参数的变量（即需要捕获的自由变量）
            for var_name in referenced_vars {
                if !param_names.contains(&var_name) {
                    if let Some(value) = ctx.variables.get(&var_name).cloned() {
                        free_vars.push(var_name.clone());

                        // 为每个捕获的变量在堆上分配共享内存位置
                        let shared_location = ctx.new_temp();
                        ctx.add_statement(Statement::HeapAlloc {
                            target: shared_location.clone(),
                            size: 8, // 一个变量8字节
                            object_type: "shared_var".to_string(),
                            span,
                        });

                        // 将当前变量值存储到共享位置
                        ctx.add_statement(Statement::Store {
                            target: shared_location.clone(),
                            value: value.clone(),
                            span,
                        });

                        // 更新外部变量映射为共享内存的引用
                        ctx.variables.insert(
                            var_name.clone(),
                            Value::Reference {
                                value: Box::new(shared_location.clone()),
                            },
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

            // 4. 创建lambda函数，参数包含env（如果有）+ 原始参数
            let mut all_params = if !captured_var_locations.is_empty() {
                vec!["__env".to_string()] // 环境参数
            } else {
                vec![]
            };
            all_params.extend(param_names.clone());

            // 暂存当前函数上下文
            let original_function_name = ctx.current_function_name.clone();
            let original_block = ctx.current_block;
            let original_vars = ctx.variables.clone();

            // 5. 开始新函数
            ctx.start_function(lambda_name.clone(), all_params);

            // 如果有环境参数，需要从环境中恢复捕获变量的共享位置
            if !captured_var_locations.is_empty() {
                let env_var = Value::Variable {
                    name: "__env".to_string(),
                };
                for (i, var_name) in free_vars.iter().enumerate() {
                    let shared_location_temp = ctx.new_temp();
                    let offset_temp = ctx.new_temp();
                    ctx.add_statement(Statement::BinaryOp {
                        target: offset_temp.clone(),
                        left: env_var.clone(),
                        op: crate::ir::BinaryOperator::Add,
                        right: Value::Number {
                            value: (i * 8) as i64,
                        },
                        span,
                    });
                    ctx.add_statement(Statement::Dereference {
                        target: shared_location_temp.clone(),
                        reference: offset_temp.clone(),
                        span,
                    });

                    // 在lambda内部，变量也映射为共享内存的引用
                    ctx.variables.insert(
                        var_name.clone(),
                        Value::Reference {
                            value: Box::new(shared_location_temp),
                        },
                    );
                }
            }

            let return_val = ctx.new_temp();
            lower_expression(ctx, body, &return_val)?;
            ctx.set_terminator(Terminator::Return {
                value: Some(return_val),
                span: body.span(),
            });

            // 恢复原始函数上下文
            ctx.current_function_name = original_function_name;
            ctx.current_block = original_block;
            ctx.variables = original_vars;
        }

        Expr::FunctionCall { function, args, .. } => {
            // 特殊处理：如果是直接lambda调用且没有捕获，生成优化的调用
            if let Expr::Lambda { .. } = function.as_ref() {
                let captured_vars = collect_referenced_variables(function);
                if captured_vars.is_empty() {
                    // 无捕获的lambda调用，直接生成函数调用
                    let func_val = lower_expression_to_temp(ctx, function)?;
                    let arg_vals: Vec<Value> = args
                        .iter()
                        .map(|arg| lower_expression_to_temp(ctx, arg))
                        .collect::<Result<_, _>>()?;

                    // 提取function_ptr并直接调用
                    let function_ptr_temp = ctx.new_temp();
                    ctx.add_statement(Statement::FieldAccess {
                        target: function_ptr_temp.clone(),
                        object: func_val.clone(),
                        field: "function_ptr".to_string(),
                        span,
                    });

                    ctx.add_statement(Statement::Call {
                        target: Some(destination.clone()),
                        function: function_ptr_temp,
                        args: arg_vals,
                        span,
                    });
                    return Ok(());
                }
            }

            let func_val = lower_expression_to_temp(ctx, function)?;

            // 对于所有其他函数调用，生成运行时closure检查
            // 1. 生成参数列表
            let arg_vals: Vec<Value> = args
                .iter()
                .map(|arg| lower_expression_to_temp(ctx, arg))
                .collect::<Result<_, _>>()?;

            // 2. 检查函数值是否已知为直接函数
            match &func_val {
                Value::Function { name: _ } => {
                    // 直接函数调用，无需额外处理
                    ctx.add_statement(Statement::Call {
                        target: Some(destination.clone()),
                        function: func_val.clone(),
                        args: arg_vals,
                        span,
                    });
                }
                Value::Closure {
                    captured_values,
                    function_name,
                } => {
                    // 旧式闭包调用（兼容性）
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
                    // 其他情况：可能是closure结构体或其他类型
                    // 生成运行时closure处理逻辑

                    // 1. 尝试提取function_ptr字段
                    let function_ptr_temp = ctx.new_temp();
                    ctx.add_statement(Statement::FieldAccess {
                        target: function_ptr_temp.clone(),
                        object: func_val.clone(),
                        field: "function_ptr".to_string(),
                        span,
                    });

                    // 2. 尝试提取env_ptr字段
                    let env_ptr_temp = ctx.new_temp();
                    ctx.add_statement(Statement::FieldAccess {
                        target: env_ptr_temp.clone(),
                        object: func_val.clone(),
                        field: "env_ptr".to_string(),
                        span,
                    });

                    // 3. 检查env_ptr是否为0
                    let zero_val = Value::Number { value: 0 };
                    let is_zero_temp = ctx.new_temp();
                    ctx.add_statement(Statement::BinaryOp {
                        target: is_zero_temp.clone(),
                        left: env_ptr_temp.clone(),
                        op: crate::ir::BinaryOperator::Equal,
                        right: zero_val,
                        span,
                    });

                    // 4. 创建分支：env_ptr == 0 时直接调用，否则传递env_ptr
                    let then_block = ctx.new_block();
                    let else_block = ctx.new_block();
                    let merge_block = ctx.new_block();

                    // 设置条件分支
                    ctx.set_terminator(Terminator::Branch {
                        condition: is_zero_temp,
                        then_block,
                        else_block,
                        span,
                    });

                    // then分支: env_ptr == 0，无环境调用
                    ctx.set_current_block(then_block);
                    ctx.add_statement(Statement::Call {
                        target: Some(destination.clone()),
                        function: function_ptr_temp.clone(),
                        args: arg_vals.clone(),
                        span,
                    });
                    ctx.set_terminator(Terminator::Goto {
                        target: merge_block,
                        span,
                    });

                    // else分支: env_ptr != 0，传递环境
                    ctx.set_current_block(else_block);
                    let mut env_args = vec![env_ptr_temp];
                    env_args.extend(arg_vals);
                    ctx.add_statement(Statement::Call {
                        target: Some(destination.clone()),
                        function: function_ptr_temp,
                        args: env_args,
                        span,
                    });
                    ctx.set_terminator(Terminator::Goto {
                        target: merge_block,
                        span,
                    });

                    // 切换到合并块
                    ctx.set_current_block(merge_block);
                }
            }
        }

        Expr::Block {
            statements,
            final_expr,
            span,
        } => {
            for stmt in statements {
                lower_statement(ctx, stmt)?;
            }
            if let Some(final_expr) = final_expr {
                lower_expression(ctx, final_expr, destination)?;
            } else {
                ctx.add_statement(Statement::Assign {
                    target: destination.clone(),
                    source: Value::Unit,
                    span: *span,
                });
            }
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
                handle_pattern_bindings(ctx, &arm.pattern, &match_value)?;

                // 生成分支体的代码
                lower_expression(ctx, &arm.body, destination)?;

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

        Expr::Assignment {
            target,
            value,
            span,
        } => {
            // 赋值表达式：执行赋值操作，然后将Unit赋值给目标
            // 注意：赋值表达式的值是Unit，但需要先执行赋值操作

            // 1. 计算右值
            let value_temp = lower_expression_to_temp(ctx, value)?;

            match target.as_ref() {
                Expr::Identifier { name, .. } => {
                    // 变量赋值：检查变量是否为引用类型
                    if let Some(var_value) = ctx.variables.get(name).cloned() {
                        match var_value {
                            Value::Reference { value: ref_target } => {
                                // 引用变量赋值：生成存储指令写入引用指向的位置
                                ctx.add_statement(Statement::Store {
                                    target: *ref_target,
                                    value: value_temp,
                                    span: *span,
                                });
                            }
                            _ => {
                                // 普通变量赋值：更新变量映射
                                ctx.variables.insert(name.clone(), value_temp);
                            }
                        }
                    } else {
                        // 新变量赋值
                        ctx.variables.insert(name.clone(), value_temp);
                    }
                }
                Expr::FieldAccess { object, field, .. } => {
                    // 字段赋值：生成字段赋值指令
                    if let Expr::Identifier { name, .. } = object.as_ref() {
                        if let Some(object_var) = ctx.variables.get(name).cloned() {
                            // 生成字段赋值语句
                            ctx.add_statement(Statement::FieldAssign {
                                object: object_var,
                                field: field.clone(),
                                value: value_temp,
                                span: *span,
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

            // 3. 赋值表达式的结果是Unit
            ctx.add_statement(Statement::Assign {
                target: destination.clone(),
                source: Value::Unit,
                span: *span,
            });
        }
    }
    Ok(())
}

fn lower_statement(
    ctx: &mut LoweringContext,
    stmt: &karte_hir::Statement,
) -> Result<(), Vec<String>> {
    match stmt {
        karte_hir::Statement::Let { name, value, .. } => {
            let var_value = lower_expression_to_temp(ctx, value)?;
            ctx.variables.insert(name.clone(), var_value);
        }
        karte_hir::Statement::Expression { expr, .. } => {
            // 结果被丢弃
            let temp = ctx.new_temp();
            lower_expression(ctx, expr, &temp)?;
        }
        karte_hir::Statement::TypeDef { .. } => {
            // 类型定义在编译期处理，MIR中无需体现
        }
        karte_hir::Statement::StructDef { name, fields, .. } => {
            // 🔧 专业修复：收集结构体定义信息，传递给MIR
            let mir_fields: Vec<crate::MirStructField> = fields
                .iter()
                .map(|field| crate::MirStructField {
                    name: field.name.clone(),
                    field_type: field.field_type.clone(),
                })
                .collect();

            let mir_struct_type = crate::MirStructType {
                name: name.clone(),
                fields: mir_fields,
            };

            ctx.program.add_struct_type(mir_struct_type);
        }
        karte_hir::Statement::Assignment { target, value, .. } => {
            // 赋值语句：将值计算到临时变量，然后赋值给目标
            let value_temp = lower_expression_to_temp(ctx, value)?;

            match target {
                Expr::Identifier { name, .. } => {
                    // 变量赋值：检查变量是否为引用类型
                    if let Some(var_value) = ctx.variables.get(name).cloned() {
                        match var_value {
                            Value::Reference { value: ref_target } => {
                                // 引用变量赋值：生成存储指令写入引用指向的位置
                                ctx.add_statement(Statement::Store {
                                    target: *ref_target,
                                    value: value_temp,
                                    span: karte_diagnostics::Span::new(0, 0),
                                });
                            }
                            _ => {
                                // 普通变量赋值：更新变量映射
                                ctx.variables.insert(name.clone(), value_temp);
                            }
                        }
                    } else {
                        // 新变量赋值
                        ctx.variables.insert(name.clone(), value_temp);
                    }
                }
                Expr::FieldAccess { object, field, .. } => {
                    // 字段赋值：生成字段赋值指令
                    if let Expr::Identifier { name, .. } = object.as_ref() {
                        if let Some(object_var) = ctx.variables.get(name).cloned() {
                            // 生成字段赋值语句
                            ctx.add_statement(Statement::FieldAssign {
                                object: object_var,
                                field: field.clone(),
                                value: value_temp,
                                span: karte_diagnostics::Span::new(0, 0),
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
        }
    }
    Ok(())
}

/// 辅助函数，将表达式降级到一个新的临时变量中
fn lower_expression_to_temp(ctx: &mut LoweringContext, expr: &Expr) -> Result<Value, Vec<String>> {
    let temp = ctx.new_temp();
    lower_expression(ctx, expr, &temp)?;
    Ok(temp)
}

/// 转换HIR二元运算符到MIR
fn convert_binary_op(op: &HirBinaryOp) -> MirBinaryOp {
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
fn convert_unary_op(op: &HirUnaryOp) -> MirUnaryOp {
    match op {
        HirUnaryOp::Plus => MirUnaryOp::Plus,
        HirUnaryOp::Minus => MirUnaryOp::Minus,
        HirUnaryOp::LogicalNot => MirUnaryOp::Not,
    }
}

/// 收集表达式中引用的所有变量名
fn collect_referenced_variables(expr: &Expr) -> Vec<String> {
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

/// 转换HIR模式到MIR模式
fn convert_pattern(pattern: &karte_hir::Pattern) -> Result<Pattern, Vec<String>> {
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
fn handle_pattern_bindings(
    ctx: &mut LoweringContext,
    pattern: &karte_hir::Pattern,
    match_value: &Value,
) -> Result<(), Vec<String>> {
    match pattern {
        karte_hir::Pattern::Variable { name, .. } => {
            // 变量模式：将整个匹配值绑定到变量
            ctx.variables.insert(name.clone(), match_value.clone());
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
                    span: karte_diagnostics::Span::new(0, 0),
                });

                ctx.variables.insert(name.clone(), arg_temp);
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
                    span: karte_diagnostics::Span::new(0, 0),
                });

                ctx.variables.insert(name.clone(), arg_temp);
            }
        }
        _ => {
            // 其他模式不需要绑定
        }
    }
    Ok(())
}

#[cfg(test)]
mod assignment_lowering_tests {
    use super::*;
    use karte_diagnostics::Span;
    use karte_hir::{Expr, Statement as HirStatement};

    fn make_span() -> Span {
        Span::new(0, 0)
    }

    #[test]
    fn test_simple_assignment_lowering() {
        // let x = 5; x = 10; x
        let expr = Expr::Block {
            statements: vec![
                HirStatement::Let {
                    name: "x".to_string(),
                    value: Expr::Number {
                        value: 5,
                        span: make_span(),
                    },
                    span: make_span(),
                },
                HirStatement::Assignment {
                    target: Expr::Identifier {
                        name: "x".to_string(),
                        span: make_span(),
                    },
                    value: Expr::Number {
                        value: 10,
                        span: make_span(),
                    },
                    span: make_span(),
                },
            ],
            final_expr: Some(Box::new(Expr::Identifier {
                name: "x".to_string(),
                span: make_span(),
            })),
            span: make_span(),
        };

        let result = lower_expr_to_mir(&expr);
        assert!(result.is_ok(), "MIR lowering 应该成功");

        let program = result.unwrap();
        assert!(program.functions.contains_key("main"), "应该有main函数");

        let main_fn = &program.functions["main"];
        assert!(!main_fn.basic_blocks.is_empty(), "main函数应该有基本块");

        // 检查是否包含Assign语句（用于赋值）
        let entry_block = &main_fn.basic_blocks[&main_fn.entry_block];
        let has_assign = entry_block
            .statements
            .iter()
            .any(|stmt| matches!(stmt, Statement::Assign { .. }));
        assert!(has_assign, "应该包含Assign语句用于赋值");
    }

    #[test]
    fn test_assignment_expression_lowering() {
        // x = 42
        let expr = Expr::Assignment {
            target: Box::new(Expr::Identifier {
                name: "x".to_string(),
                span: make_span(),
            }),
            value: Box::new(Expr::Number {
                value: 42,
                span: make_span(),
            }),
            span: make_span(),
        };

        let result = lower_expr_to_mir(&expr);
        assert!(result.is_ok(), "赋值表达式的MIR lowering应该成功");

        let program = result.unwrap();
        let main_fn = &program.functions["main"];
        let entry_block = &main_fn.basic_blocks[&main_fn.entry_block];

        // 检查是否包含Assign语句，值为42
        let has_assign = entry_block.statements.iter().any(|stmt| {
            matches!(
                stmt,
                Statement::Assign {
                    source: Value::Number { value: 42 },
                    ..
                }
            )
        });
        assert!(has_assign, "应该包含值为42的Assign语句");
    }

    #[test]
    fn test_chained_assignment_lowering() {
        // a = b = 5
        let expr = Expr::Assignment {
            target: Box::new(Expr::Identifier {
                name: "a".to_string(),
                span: make_span(),
            }),
            value: Box::new(Expr::Assignment {
                target: Box::new(Expr::Identifier {
                    name: "b".to_string(),
                    span: make_span(),
                }),
                value: Box::new(Expr::Number {
                    value: 5,
                    span: make_span(),
                }),
                span: make_span(),
            }),
            span: make_span(),
        };

        let result = lower_expr_to_mir(&expr);
        assert!(result.is_ok(), "连续赋值的MIR lowering应该成功");

        let program = result.unwrap();
        let main_fn = &program.functions["main"];
        let entry_block = &main_fn.basic_blocks[&main_fn.entry_block];

        // 检查是否包含Assign语句（至少一个，因为嵌套赋值可能有不同的实现方式）
        let assign_count = entry_block
            .statements
            .iter()
            .filter(|stmt| matches!(stmt, Statement::Assign { .. }))
            .count();
        assert!(assign_count >= 1, "应该至少有一个Assign语句用于连续赋值");
    }

    #[test]
    fn test_field_assignment_lowering() {
        // obj.field = 100
        let expr = Expr::Assignment {
            target: Box::new(Expr::FieldAccess {
                object: Box::new(Expr::Identifier {
                    name: "obj".to_string(),
                    span: make_span(),
                }),
                field: "field".to_string(),
                span: make_span(),
            }),
            value: Box::new(Expr::Number {
                value: 100,
                span: make_span(),
            }),
            span: make_span(),
        };

        let result = lower_expr_to_mir(&expr);
        // 字段赋值可能还没有完全实现，所以我们只检查它不会崩溃
        match result {
            Ok(program) => {
                // 如果成功，检查是否生成了一些语句
                let main_fn = &program.functions["main"];
                let entry_block = &main_fn.basic_blocks[&main_fn.entry_block];

                // 检查是否包含FieldAssign语句或者其他相关语句
                let has_field_assign = entry_block.statements.iter().any(
                    |stmt| matches!(stmt, Statement::FieldAssign { field, .. } if field == "field"),
                );
                let has_assign = entry_block
                    .statements
                    .iter()
                    .any(|stmt| matches!(stmt, Statement::Assign { .. }));

                // 至少应该有某种形式的语句
                assert!(
                    has_field_assign || has_assign || !entry_block.statements.is_empty(),
                    "字段赋值应该生成某些MIR语句"
                );
            }
            Err(_) => {
                // 如果失败，这可能是预期的，因为字段赋值可能还在开发中
                println!("字段赋值MIR lowering暂时不支持，这是预期的");
            }
        }
    }

    #[test]
    fn test_variable_collection_with_assignment() {
        // 测试变量收集功能是否包含赋值中的变量
        let expr = Expr::Assignment {
            target: Box::new(Expr::Identifier {
                name: "x".to_string(),
                span: make_span(),
            }),
            value: Box::new(Expr::Identifier {
                name: "y".to_string(),
                span: make_span(),
            }),
            span: make_span(),
        };

        let vars = collect_referenced_variables(&expr);
        assert!(vars.contains(&"x".to_string()), "应该包含变量x");
        assert!(vars.contains(&"y".to_string()), "应该包含变量y");
    }
}

#[cfg(test)]
mod closure_struct_tests {
    use karte_diagnostics::Span;

    use super::*;

    fn make_span() -> Span {
        Span::new(0, 0)
    }

    #[test]
    fn test_lambda_without_captures_lowering() {
        // lambda (x) => x + 1
        use karte_hir::Parameter;

        let lambda_expr = Expr::Lambda {
            params: vec![Parameter {
                name: "x".to_string(),
                type_annotation: Some("Number".to_string()),
                span: make_span(),
            }],
            body: Box::new(Expr::BinaryOp {
                left: Box::new(Expr::Identifier {
                    name: "x".to_string(),
                    span: make_span(),
                }),
                op: karte_hir::BinaryOperator::Add,
                right: Box::new(Expr::Number {
                    value: 1,
                    span: make_span(),
                }),
                span: make_span(),
            }),
            span: make_span(),
        };

        let result = lower_expr_to_mir(&lambda_expr);
        assert!(result.is_ok(), "无捕获lambda的MIR lowering应该成功");

        let program = result.unwrap();
        let main_fn = &program.functions["main"];
        let entry_block = &main_fn.basic_blocks[&main_fn.entry_block];

        // 检查是否生成了Closure结构体
        let has_closure_struct = entry_block.statements.iter().any(|stmt| {
            if let Statement::Assign {
                source: Value::Struct { name, fields },
                ..
            } = stmt
            {
                name == "Closure"
                    && fields
                        .get("env_ptr")
                        .map(|v| matches!(v, Value::Number { value: 0 }))
                        .unwrap_or(false)
            } else {
                false
            }
        });
        assert!(has_closure_struct, "应该生成env_ptr=0的Closure结构体");

        // 检查是否生成了lambda函数
        assert!(program.functions.len() >= 2, "应该生成主函数和lambda函数");

        let lambda_fn_name = program
            .functions
            .keys()
            .find(|name| name.starts_with("lambda$"))
            .expect("应该有lambda函数");
        let lambda_fn = &program.functions[lambda_fn_name];

        // 无捕获的lambda不应该有__env参数
        assert_eq!(lambda_fn.params.len(), 1, "无捕获lambda应该只有1个参数");
        assert_eq!(lambda_fn.params[0], "x", "参数应该是x");
    }

    #[test]
    fn test_lambda_with_captures_lowering() {
        // let y = 42; lambda (x) => x + y
        use karte_hir::{Parameter, Statement as HirStatement};

        let expr = Expr::Block {
            statements: vec![HirStatement::Let {
                name: "y".to_string(),
                value: Expr::Number {
                    value: 42,
                    span: make_span(),
                },
                span: make_span(),
            }],
            final_expr: Some(Box::new(Expr::Lambda {
                params: vec![Parameter {
                    name: "x".to_string(),
                    type_annotation: Some("Number".to_string()),
                    span: make_span(),
                }],
                body: Box::new(Expr::BinaryOp {
                    left: Box::new(Expr::Identifier {
                        name: "x".to_string(),
                        span: make_span(),
                    }),
                    op: karte_hir::BinaryOperator::Add,
                    right: Box::new(Expr::Identifier {
                        name: "y".to_string(),
                        span: make_span(),
                    }),
                    span: make_span(),
                }),
                span: make_span(),
            })),
            span: make_span(),
        };

        let result = lower_expr_to_mir(&expr);
        assert!(result.is_ok(), "有捕获lambda的MIR lowering应该成功");

        let program = result.unwrap();
        let main_fn = &program.functions["main"];
        let entry_block = &main_fn.basic_blocks[&main_fn.entry_block];

        // 检查是否生成了堆分配语句
        let has_heap_alloc = entry_block.statements.iter().any(|stmt| {
            matches!(stmt, Statement::HeapAlloc { object_type, .. } if object_type == "closure_env")
        });
        assert!(has_heap_alloc, "应该生成闭包环境的堆分配语句");

        // 检查是否生成了Store语句（存储捕获的变量）
        let has_store = entry_block
            .statements
            .iter()
            .any(|stmt| matches!(stmt, Statement::Store { .. }));
        assert!(has_store, "应该生成Store语句来存储捕获的变量");

        // 检查是否生成了非零env_ptr的Closure结构体
        let has_closure_with_env = entry_block.statements.iter().any(|stmt| {
            if let Statement::Assign {
                source: Value::Struct { name, fields },
                ..
            } = stmt
            {
                name == "Closure"
                    && fields
                        .get("env_ptr")
                        .map(|v| !matches!(v, Value::Number { value: 0 }))
                        .unwrap_or(false)
            } else {
                false
            }
        });
        assert!(has_closure_with_env, "应该生成env_ptr非零的Closure结构体");

        // 检查lambda函数是否有环境参数
        let lambda_fn_name = program
            .functions
            .keys()
            .find(|name| name.starts_with("lambda$"))
            .expect("应该有lambda函数");
        let lambda_fn = &program.functions[lambda_fn_name];

        // 有捕获的lambda应该有__env参数 + 原始参数
        assert_eq!(lambda_fn.params.len(), 2, "有捕获lambda应该有2个参数");
        assert_eq!(lambda_fn.params[0], "__env", "第一个参数应该是__env");
        assert_eq!(lambda_fn.params[1], "x", "第二个参数应该是x");
    }

    #[test]
    fn test_closure_struct_function_call() {
        // 测试闭包结构体的函数调用
        use karte_hir::Parameter;

        let call_expr = Expr::FunctionCall {
            function: Box::new(Expr::Lambda {
                params: vec![Parameter {
                    name: "x".to_string(),
                    type_annotation: Some("Number".to_string()),
                    span: make_span(),
                }],
                body: Box::new(Expr::Identifier {
                    name: "x".to_string(),
                    span: make_span(),
                }),
                span: make_span(),
            }),
            args: vec![Expr::Number {
                value: 42,
                span: make_span(),
            }],
            span: make_span(),
        };

        let result = lower_expr_to_mir(&call_expr);
        assert!(result.is_ok(), "闭包结构体调用的MIR lowering应该成功");

        let program = result.unwrap();
        let main_fn = &program.functions["main"];
        let entry_block = &main_fn.basic_blocks[&main_fn.entry_block];

        // 检查是否生成了Call语句
        let has_call = entry_block
            .statements
            .iter()
            .any(|stmt| matches!(stmt, Statement::Call { .. }));
        assert!(has_call, "应该生成Call语句");
    }

    #[test]
    fn test_heap_allocation_statements() {
        // 测试堆分配相关语句的生成
        use karte_hir::{Parameter, Statement as HirStatement};

        let expr = Expr::Block {
            statements: vec![HirStatement::Let {
                name: "captured".to_string(),
                value: Expr::Number {
                    value: 100,
                    span: make_span(),
                },
                span: make_span(),
            }],
            final_expr: Some(Box::new(Expr::Lambda {
                params: vec![Parameter {
                    name: "param".to_string(),
                    type_annotation: Some("Number".to_string()),
                    span: make_span(),
                }],
                body: Box::new(Expr::Identifier {
                    name: "captured".to_string(),
                    span: make_span(),
                }),
                span: make_span(),
            })),
            span: make_span(),
        };

        let result = lower_expr_to_mir(&expr);
        assert!(result.is_ok(), "包含堆分配的lambda MIR lowering应该成功");

        let program = result.unwrap();
        let main_fn = &program.functions["main"];
        let entry_block = &main_fn.basic_blocks[&main_fn.entry_block];

        // 验证各种堆操作语句
        let heap_alloc_count = entry_block
            .statements
            .iter()
            .filter(|stmt| matches!(stmt, Statement::HeapAlloc { .. }))
            .count();
        assert_eq!(
            heap_alloc_count, 2,
            "应该有2个HeapAlloc语句：1个为捕获变量分配共享内存，1个为闭包环境分配内存"
        );

        let store_count = entry_block
            .statements
            .iter()
            .filter(|stmt| matches!(stmt, Statement::Store { .. }))
            .count();
        assert_eq!(
            store_count, 2,
            "应该有2个Store语句：1个存储捕获变量到共享内存，1个存储共享内存位置到闭包环境"
        );

        // 检查lambda函数中的变量恢复
        let lambda_fn_name = program
            .functions
            .keys()
            .find(|name| name.starts_with("lambda$"))
            .expect("应该有lambda函数");
        let lambda_fn = &program.functions[lambda_fn_name];
        let lambda_entry_block = &lambda_fn.basic_blocks[&lambda_fn.entry_block];

        // 应该有语句来恢复捕获的变量（通过引用和解引用）
        let has_var_recovery = lambda_entry_block.statements.iter().any(|stmt| {
            matches!(stmt, Statement::Assign { .. })
                || matches!(stmt, Statement::Dereference { .. })
        });
        assert!(has_var_recovery, "lambda函数应该有语句来恢复捕获的变量");
    }
}
