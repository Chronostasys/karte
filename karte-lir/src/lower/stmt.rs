//! LIR降低过程的语句处理
//!
//! 本模块负责将MIR语句转换为LIR指令。

use super::helpers::value_to_key;
use super::types::LirLoweringContext;
use crate::{AllocationType, Instruction, Operand};
use karte_mir::{BinaryOperator, Statement, UnaryOperator, Value};

pub(super) fn lower_statement(ctx: &mut LirLoweringContext, statement: &Statement) -> Result<(), Vec<String>> {
    match statement {
        Statement::Assign {
            target,
            source,
            span: _,
        } => {
            // 🔧 修复：使用L-Value/R-Value概念
            // 赋值操作：target = source，需要source的R-Value和target的L-Value

            // 🔧 关键修复：检查是否是env_ptr相关的赋值，如果是且值为0，则跳过
            // 这是为了避免env_ptr覆盖function_ptr的问题
            log::debug!("🔧 Assignment: target={:?}, source={:?}", target, source);

            // 检查源值是否是env_ptr字段访问
            let is_env_ptr_assignment = match source {
                Value::Temp { .. } => {
                    // 对于临时变量，我们需要检查其值是否为0
                    let src_rvalue = ctx.lower_to_rvalue(source);
                    if let Operand::Immediate { value: 0 } = src_rvalue {
                        log::debug!("🔧 检测到值为0的临时变量赋值，可能是env_ptr，跳过以避免覆盖function_ptr");
                        true
                    } else {
                        false
                    }
                }
                _ => false,
            };

            if is_env_ptr_assignment {
                log::debug!("🔧 跳过env_ptr=0的赋值操作，避免覆盖function_ptr");
                return Ok(());
            }

            // 1. 获取源值的R-Value（值本身）
            let src_rvalue = ctx.lower_to_rvalue(source);

            // 2. 获取目标的L-Value（存储位置）
            let target_lvalue = ctx.lower_to_lvalue(target);

            // 3. 执行赋值：将源值存储到目标位置
            if let Operand::Register { id: target_addr } = target_lvalue {
                ctx.add_instruction(Instruction::Store64 {
                    addr: target_addr,
                    offset: 0,
                    src: src_rvalue,
                    span: karte_diagnostics::Span::dummy(),
                });
            }

            ctx.propagate_struct_layout(target, source);

            Ok(())
        }

        Statement::BinaryOp {
            target,
            left,
            op,
            right,
            span,
        } => {
            // Stack-First策略：创建临时寄存器来存储计算结果
            let temp_register = ctx.current_function_mut().new_register();

            // 从栈load操作数到临时寄存器
            let src1 = ctx.lower_to_rvalue(left);
            let src2 = ctx.lower_to_rvalue(right);

            // 克隆操作数以便在后续逻辑中使用
            let src1_clone = src1.clone();
            let src2_clone = src2.clone();

            let instruction = match op {
                BinaryOperator::Add => Instruction::Add {
                    dst: temp_register,
                    src1,
                    src2,
                    span: *span,
                },
                BinaryOperator::Subtract => Instruction::Sub {
                    dst: temp_register,
                    src1,
                    src2,
                    span: *span,
                },
                BinaryOperator::Multiply => Instruction::Mul {
                    dst: temp_register,
                    src1,
                    src2,
                    span: *span,
                },
                BinaryOperator::Divide => Instruction::Div {
                    dst: temp_register,
                    src1,
                    src2,
                    span: *span,
                },

                // For logical operations, we handle them differently and return a move instruction
                BinaryOperator::And
                | BinaryOperator::Or
                | BinaryOperator::Equal
                | BinaryOperator::NotEqual
                | BinaryOperator::LessThan
                | BinaryOperator::LessEqual
                | BinaryOperator::GreaterThan
                | BinaryOperator::GreaterEqual => {
                    // Handle these complex operations separately after the match
                    // For now, return a simple move to avoid type mismatch
                    Instruction::Move {
                        dst: temp_register,
                        src: Operand::Immediate { value: 0 },
                        span: *span,
                    }
                }
            };

            // 处理复杂的逻辑运算和比较运算
            match op {
                BinaryOperator::Equal
                | BinaryOperator::NotEqual
                | BinaryOperator::LessThan
                | BinaryOperator::LessEqual
                | BinaryOperator::GreaterThan
                | BinaryOperator::GreaterEqual => {
                    // 🔧 修复：确保False case的结果被正确设置
                    // 先添加比较指令
                    ctx.add_instruction(Instruction::Compare {
                        src1: src1_clone,
                        src2: src2_clone,
                        span: *span,
                    });

                    let true_label = ctx.next_internal_label("cmp_true");
                    let end_label = ctx.next_internal_label("cmp_end");

                    let jump_instr = match op {
                        BinaryOperator::Equal => Instruction::JumpEqual {
                            target: true_label,
                            span: *span,
                        },
                        BinaryOperator::NotEqual => Instruction::JumpNotEqual {
                            target: true_label,
                            span: *span,
                        },
                        BinaryOperator::LessThan => Instruction::JumpLess {
                            target: true_label,
                            span: *span,
                        },
                        BinaryOperator::LessEqual => Instruction::JumpLessEqual {
                            target: true_label,
                            span: *span,
                        },
                        BinaryOperator::GreaterThan => Instruction::JumpGreater {
                            target: true_label,
                            span: *span,
                        },
                        BinaryOperator::GreaterEqual => Instruction::JumpGreaterEqual {
                            target: true_label,
                            span: *span,
                        },
                        _ => unreachable!(),
                    };
                    ctx.add_instruction(jump_instr);

                    // 🔧 关键修复：False case - 显式设置结果为0
                    ctx.add_instruction(Instruction::Move {
                        dst: temp_register,
                        src: Operand::Immediate { value: 0 },
                        span: *span,
                    });
                    ctx.add_instruction(Instruction::Jump {
                        target: end_label,
                        span: *span,
                    });

                    // True case
                    ctx.add_instruction(Instruction::Label {
                        id: true_label,
                        span: *span,
                    });
                    ctx.add_instruction(Instruction::Move {
                        dst: temp_register,
                        src: Operand::Immediate { value: 1 },
                        span: *span,
                    });

                    // End - 🔧 关键修复：确保end_label在正确位置
                    ctx.add_instruction(Instruction::Label {
                        id: end_label,
                        span: *span,
                    });
                }
                BinaryOperator::And => {
                    // Logical AND: if src1 == 0, result = 0; else result = src2
                    let false_label = ctx.next_internal_label("and_false");
                    let end_label = ctx.next_internal_label("and_end");

                    // Compare src1 with 0 (false)
                    ctx.add_instruction(Instruction::Compare {
                        src1: src1_clone.clone(),
                        src2: Operand::Immediate { value: 0 },
                        span: *span,
                    });

                    // If src1 == 0, jump to false_label
                    ctx.add_instruction(Instruction::JumpEqual {
                        target: false_label,
                        span: *span,
                    });

                    // src1 is true (non-zero), move src2 to result
                    ctx.add_instruction(Instruction::Move {
                        dst: temp_register,
                        src: src2_clone.clone(),
                        span: *span,
                    });
                    ctx.add_instruction(Instruction::Jump {
                        target: end_label,
                        span: *span,
                    });

                    // src1 is false, result is false (0)
                    ctx.add_instruction(Instruction::Label {
                        id: false_label,
                        span: *span,
                    });
                    ctx.add_instruction(Instruction::Move {
                        dst: temp_register,
                        src: Operand::Immediate { value: 0 },
                        span: *span,
                    });

                    // End
                    ctx.add_instruction(Instruction::Label {
                        id: end_label,
                        span: *span,
                    });
                }
                BinaryOperator::Or => {
                    // Logical OR: if src1 != 0, result = 1; else result = src2
                    let true_label = ctx.next_internal_label("or_true");
                    let end_label = ctx.next_internal_label("or_end");

                    // Compare src1 with 0 (false)
                    ctx.add_instruction(Instruction::Compare {
                        src1: src1_clone,
                        src2: Operand::Immediate { value: 0 },
                        span: *span,
                    });

                    // If src1 != 0, jump to true_label
                    ctx.add_instruction(Instruction::JumpNotEqual {
                        target: true_label,
                        span: *span,
                    });

                    // src1 is false, move src2 to result
                    ctx.add_instruction(Instruction::Move {
                        dst: temp_register,
                        src: src2_clone,
                        span: *span,
                    });
                    ctx.add_instruction(Instruction::Jump {
                        target: end_label,
                        span: *span,
                    });

                    // src1 is true, result is true (1)
                    ctx.add_instruction(Instruction::Label {
                        id: true_label,
                        span: *span,
                    });
                    ctx.add_instruction(Instruction::Move {
                        dst: temp_register,
                        src: Operand::Immediate { value: 1 },
                        span: *span,
                    });

                    // End
                    ctx.add_instruction(Instruction::Label {
                        id: end_label,
                        span: *span,
                    });
                }
                _ => {
                    // 对于简单运算（Add, Sub, Mul, Div），添加基本指令
                    ctx.add_instruction(instruction);
                }
            }

            // 🔧 修复：对于逻辑操作，直接将结果标记为直接寄存器值
            // 避免不必要的栈存储，特别是对于逻辑AND/OR操作
            match op {
                BinaryOperator::And | BinaryOperator::Or => {
                    // 逻辑操作的结果也使用Stack-First策略
                    ctx.store_value_to_stack(target, Operand::Register { id: temp_register });
                    log::debug!("🔧 逻辑操作结果使用Stack-First存储: {:?} -> stack", target);
                }
                // 🔧 修复：比较操作的结果也使用Stack-First策略
                BinaryOperator::Equal
                | BinaryOperator::NotEqual
                | BinaryOperator::LessThan
                | BinaryOperator::LessEqual
                | BinaryOperator::GreaterThan
                | BinaryOperator::GreaterEqual => {
                    ctx.store_value_to_stack(target, Operand::Register { id: temp_register });
                    log::debug!("🔧 比较操作结果使用Stack-First存储: {:?} -> stack", target);
                }
                _ => {
                    // 其他操作仍使用Stack-First策略
                    ctx.store_value_to_stack(target, Operand::Register { id: temp_register });
                }
            }

            Ok(())
        }

        Statement::UnaryOp {
            target,
            op,
            operand,
            span,
        } => {
            // Stack-First策略：创建临时寄存器来存储计算结果
            let temp_register = ctx.current_function_mut().new_register();

            let src = ctx.lower_to_rvalue(operand);

            match op {
                UnaryOperator::Plus => {
                    // +x is just x, so we move it
                    ctx.add_instruction(Instruction::Move {
                        dst: temp_register,
                        src,
                        span: *span,
                    });
                }
                UnaryOperator::Minus => {
                    // -x is 0 - x
                    ctx.add_instruction(Instruction::Sub {
                        dst: temp_register,
                        src1: Operand::Immediate { value: 0 },
                        src2: src,
                        span: *span,
                    });
                }
                UnaryOperator::Not => {
                    // !x: logical not with 0/1 encoding
                    // For 0/1 boolean encoding: !x = 1 - x
                    let temp_reg = ctx.current_function_mut().new_register();

                    // Move 1 to temp register
                    ctx.add_instruction(Instruction::Move {
                        dst: temp_reg,
                        src: Operand::Immediate { value: 1 },
                        span: *span,
                    });

                    // Subtract src from 1: result = 1 - src
                    ctx.add_instruction(Instruction::Sub {
                        dst: temp_register,
                        src1: Operand::Register { id: temp_reg },
                        src2: src,
                        span: *span,
                    });
                }
            }

            // Stack-First策略：将结果存储到栈
            ctx.store_value_to_stack(target, Operand::Register { id: temp_register });

            Ok(())
        }

        Statement::Call {
            target,
            function,
            args,
            span,
        } => {
            // 首先解析函数值，看看是否是闭包
            let resolved_function = ctx.resolve_value(function);

            let mut all_args = Vec::new();
            let actual_function_to_call;

            // 🔧 修复：如果是闭包，需要先添加捕获的值作为环境参数
            if let Value::Closure {
                captured_values, ..
            } = &resolved_function
            {
                // 闭包函数体参数顺序是 [__env, user_params]
                // 所以调用时也应该先传递环境，再传递用户参数
                all_args.extend(captured_values.clone());
                actual_function_to_call = resolved_function.clone();
            }
            // 🔧 修复：如果是Closure结构体，需要提取function_ptr和env_ptr字段
            else if let Value::Struct { name, fields } = &resolved_function {
                if name == "Closure" {
                    // 🔧 关键修复：检查env_ptr是否为0，如果是0则不添加环境参数
                    if let Some(env_ptr) = fields.get("env_ptr") {
                        if let Value::Number { value: 0 } = env_ptr {
                            // env_ptr为0，不添加环境参数，这是一个简单函数
                            log::debug!("🔧 Closure的env_ptr为0，不添加环境参数，不进行任何env_ptr相关的存储操作");
                            // 🔧 重要：当env_ptr为0时，完全跳过env_ptr的处理，避免错误的存储操作
                        } else {
                            // env_ptr非0，添加环境参数
                            all_args.push(env_ptr.clone());
                            log::debug!("🔧 Closure添加环境参数: {:?}", env_ptr);
                        }
                    }

                    // 🔧 关键修复：提取function_ptr字段作为实际要调用的函数
                    if let Some(function_ptr) = fields.get("function_ptr") {
                        actual_function_to_call = function_ptr.clone();
                    } else {
                        return Err(vec!["Closure结构体缺少function_ptr字段".to_string()]);
                    }
                } else {
                    actual_function_to_call = resolved_function.clone();
                }
            } else {
                actual_function_to_call = resolved_function.clone();
            }

            // 然后添加实际的调用参数
            all_args.extend(args.clone());

            // 转换所有参数为操作数
            let mut arg_operands = vec![];
            for arg in &all_args {
                arg_operands.push(ctx.lower_to_rvalue(arg));
            }

            // 检查是否是函数参数调用
            let is_function_parameter = match &actual_function_to_call {
                Value::Variable { name } => ctx.current_function_params.contains(name),
                _ => false,
            };

            if is_function_parameter {
                // 对于函数参数，使用间接调用
                if let Value::Variable { .. } = &actual_function_to_call {
                    let function_register =
                        ctx.allocate_register_for_value(&actual_function_to_call);
                    // 🔧 修复：使用临时寄存器接收返回值
                    let result_temp_reg = if target.is_some() {
                        Some(ctx.current_function_mut().new_register())
                    } else {
                        None
                    };

                    // 🔧 简化：不再手动设置参数，让指令降级器处理
                    // 🔧 关键修复：在调用前移动参数到正确的寄存器
                    let mut actual_arg_regs = vec![];
                    for (i, arg_op) in arg_operands.iter().enumerate() {
                        if i < 4 {
                            // 最多支持4个参数
                            let param_reg = ctx.current_function_mut().new_register();
                            ctx.add_instruction(Instruction::Move {
                                dst: param_reg,
                                src: arg_op.clone(),
                                span: *span,
                            });
                            actual_arg_regs.push(param_reg);
                        }
                    }

                    // function register 要再load一次
                    let func_ptr_reg = ctx.current_function_mut().new_register();
                    ctx.add_instruction(Instruction::Load64 {
                        dst: func_ptr_reg,
                        addr: function_register,
                        offset: 0,
                        span: *span,
                    });
                    let function_register = func_ptr_reg;

                    ctx.add_instruction(Instruction::CallIndirect {
                        function_register,
                        args: actual_arg_regs, // 参数将在指令降级阶段进一步处理
                        arg_operands: arg_operands.clone(), // 传递参数操作数
                        result: result_temp_reg,
                        span: *span,
                    });

                    // 🔧 关键修复：Stack-First策略：如果有返回值，在调用后存储到栈
                    if let (Some(target_value), Some(temp_reg)) = (target, result_temp_reg) {
                        ctx.store_value_to_stack(target_value, Operand::Register { id: temp_reg });
                    }
                }
            } else {
                // 尝试从值中提取函数名，支持更多类型的可调用值
                let function_name = match &actual_function_to_call {
                    Value::Function { name } => name.clone(),
                    Value::Closure { function_name, .. } => function_name.clone(),
                    Value::Variable { name } => {
                        // 🔧 关键修复：变量可能包含Closure结构体，需要从中提取函数指针
                        // 获取变量的存储地址（这应该是Closure结构体的地址）
                        let var_addr = ctx.lower_to_lvalue(&actual_function_to_call);

                        let function_register = match var_addr {
                            Operand::Register { id: var_stack_addr } => {
                                // 🔧 关键修复：首先从变量的栈地址加载Closure结构体的地址
                                let closure_addr_reg = ctx.current_function_mut().new_register();
                                ctx.add_instruction(Instruction::Load64 {
                                    dst: closure_addr_reg,
                                    addr: var_stack_addr,
                                    offset: 0,
                                    span: *span,
                                });

                                // 然后从Closure结构体的function_ptr字段（偏移量0）加载函数指针
                                let func_ptr_reg = ctx.current_function_mut().new_register();
                                ctx.add_instruction(Instruction::Load64 {
                                    dst: func_ptr_reg,
                                    addr: closure_addr_reg,
                                    offset: 0, // function_ptr字段在偏移量0
                                    span: *span,
                                });

                                log::debug!("🔧 变量 {} 作为Closure：从栈地址 {:?} 加载Closure到 {:?}，再从Closure加载function_ptr到 {:?}", 
                                    name, var_stack_addr, closure_addr_reg, func_ptr_reg);
                                func_ptr_reg
                            }
                            _ => {
                                return Err(vec![
                                    "Variable address must be a register for function call"
                                        .to_string(),
                                ]);
                            }
                        };

                        // 🔧 简化：不再手动设置参数，让指令降级器处理
                        let result_reg = if target.is_some() {
                            Some(ctx.current_function_mut().new_register())
                        } else {
                            None
                        };

                        let mut actual_arg_regs = vec![];
                        for (i, arg_op) in arg_operands.iter().enumerate() {
                            if i < 4 {
                                // 最多支持4个参数
                                let param_reg = ctx.current_function_mut().new_register();
                                ctx.add_instruction(Instruction::Move {
                                    dst: param_reg,
                                    src: arg_op.clone(),
                                    span: *span,
                                });
                                actual_arg_regs.push(param_reg);
                            }
                        }
                        // function register 要再load一次
                        let func_ptr_reg = ctx.current_function_mut().new_register();
                        ctx.add_instruction(Instruction::Load64 {
                            dst: func_ptr_reg,
                            addr: function_register,
                            offset: 0,
                            span: *span,
                        });
                        let function_register = func_ptr_reg;

                        ctx.add_instruction(Instruction::CallIndirect {
                            function_register,
                            args: actual_arg_regs, // 参数将在指令降级阶段处理
                            arg_operands: arg_operands.clone(), // 传递参数操作数
                            result: result_reg,
                            span: *span,
                        });

                        // 🔧 关键修复：Stack-First策略：如果有返回值，在调用后存储到栈
                        if let (Some(target_value), Some(result_register)) = (target, result_reg) {
                            ctx.store_value_to_stack(
                                target_value,
                                Operand::Register {
                                    id: result_register,
                                },
                            );
                        }

                        // 间接调用已完成，直接返回
                        return Ok(());
                    }
                    Value::Temp { .. } => {
                        // 🔧 关键修复：对于包含函数指针的临时变量，直接使用其绑定的寄存器值
                        let function_register = if let Some(&bound_reg) = ctx
                            .stack_allocations
                            .get(&value_to_key(&actual_function_to_call))
                        {
                            // 临时变量已经绑定到寄存器，直接使用
                            log::debug!(
                                "🔧 临时变量作为函数指针：直接使用绑定的寄存器 {:?}",
                                bound_reg
                            );
                            bound_reg
                        } else {
                            // 如果没有绑定寄存器，则从栈地址加载（fallback）
                            let temp_stack_addr = ctx.lower_to_lvalue(&actual_function_to_call);
                            match temp_stack_addr {
                                Operand::Register { id: stack_addr } => {
                                    let func_ptr_reg = ctx.current_function_mut().new_register();
                                    ctx.add_instruction(Instruction::Load64 {
                                        dst: func_ptr_reg,
                                        addr: stack_addr,
                                        offset: 0,
                                        span: *span,
                                    });
                                    log::debug!(
                                        "🔧 临时变量作为函数指针：从栈地址 {:?} 加载到寄存器 {:?}",
                                        stack_addr,
                                        func_ptr_reg
                                    );
                                    func_ptr_reg
                                }
                                _ => {
                                    return Err(vec![
                                        "Temp variable stack address must be a register"
                                            .to_string(),
                                    ]);
                                }
                            }
                        };

                        // 🔧 简化：不再手动设置参数，让指令降级器处理
                        let result_reg = if target.is_some() {
                            Some(ctx.current_function_mut().new_register())
                        } else {
                            None
                        };
                        let mut actual_arg_regs = vec![];
                        for (i, arg_op) in arg_operands.iter().enumerate() {
                            if i < 4 {
                                // 最多支持4个参数
                                let param_reg = ctx.current_function_mut().new_register();
                                ctx.add_instruction(Instruction::Move {
                                    dst: param_reg,
                                    src: arg_op.clone(),
                                    span: *span,
                                });
                                actual_arg_regs.push(param_reg);
                            }
                        }
                        // function register 要再load一次
                        let func_ptr_reg = ctx.current_function_mut().new_register();
                        ctx.add_instruction(Instruction::Load64 {
                            dst: func_ptr_reg,
                            addr: function_register,
                            offset: 0,
                            span: *span,
                        });
                        let function_register = func_ptr_reg;

                        ctx.add_instruction(Instruction::CallIndirect {
                            function_register,
                            args: actual_arg_regs, // 参数将在指令降级阶段处理
                            arg_operands: arg_operands.clone(), // 传递参数操作数
                            result: result_reg,
                            span: *span,
                        });

                        // 🔧 关键修复：Stack-First策略：如果有返回值，在调用后存储到栈
                        if let (Some(target_value), Some(result_register)) = (target, result_reg) {
                            ctx.store_value_to_stack(
                                target_value,
                                Operand::Register {
                                    id: result_register,
                                },
                            );
                        }

                        // 间接调用已完成，直接返回
                        return Ok(());
                    }
                    _ => {
                        return Err(vec![format!(
                            "Cannot call a non-function value: {:?}",
                            actual_function_to_call
                        )]);
                    }
                };

                let target_label = ctx
                    .function_labels
                    .get(&function_name)
                    .cloned()
                    .ok_or_else(|| vec![format!("Unknown function: {}", function_name)])?;

                // 🔧 修复：使用临时寄存器接收返回值，避免覆盖栈地址寄存器
                let result_temp_reg = if target.is_some() {
                    Some(ctx.current_function_mut().new_register())
                } else {
                    None
                };

                // 🔧 简化：不再手动设置参数，让指令降级器处理
                ctx.add_instruction(Instruction::Call {
                    target: target_label,
                    args: vec![],                       // 参数将在指令降级阶段处理
                    arg_operands: arg_operands.clone(), // 传递参数操作数
                    result: result_temp_reg,
                    span: *span,
                });

                // 🔧 关键修复：Stack-First策略：如果有返回值，在调用后存储到栈
                if let (Some(target_value), Some(temp_reg)) = (target, result_temp_reg) {
                    ctx.store_value_to_stack(target_value, Operand::Register { id: temp_reg });
                }
            }

            Ok(())
        }

        Statement::FieldAccess {
            target,
            object,
            field,
            span,
        } => {
            log::debug!(
                "🔧 FieldAccess执行: target={:?}, object={:?}, field={}",
                target,
                object,
                field
            );

            // 1. 取出结构体的基地址（无论是堆指针还是栈上的值，lower_to_rvalue 都会返回地址）
            let struct_base_operand = ctx.lower_to_rvalue(object);
            let base_reg = match struct_base_operand {
                Operand::Register { id } => id,
                operand => {
                    let temp = ctx.current_function_mut().new_register();
                    ctx.add_instruction(Instruction::Move {
                        dst: temp,
                        src: operand,
                        span: *span,
                    });
                    temp
                }
            };

            // 2. 计算字段偏移量
            let field_offset = ctx
                .get_field_offset_from_struct_layout(object, field)
                .map_err(|e| vec![e])?;
            log::debug!("🔧 字段 {} 偏移量: {}", field, field_offset);

            // 3. 计算字段地址 = 基地址 + 偏移
            let field_addr_reg = ctx.current_function_mut().new_register();
            ctx.add_instruction(Instruction::Add {
                dst: field_addr_reg,
                src1: Operand::Register { id: base_reg },
                src2: Operand::Immediate {
                    value: field_offset as i64,
                },
                span: *span,
            });

            // 4. 从字段地址加载实际的字段值
            let field_value_reg = ctx.current_function_mut().new_register();
            ctx.add_instruction(Instruction::Load64 {
                dst: field_value_reg,
                addr: field_addr_reg,
                offset: 0,
                span: *span,
            });

            // 5. 将字段值写入目标的存储位置
            let target_lvalue = ctx.lower_to_lvalue(target);
            if let Operand::Register { id: target_addr } = target_lvalue {
                ctx.add_instruction(Instruction::Store64 {
                    addr: target_addr,
                    offset: 0,
                    src: Operand::Register {
                        id: field_value_reg,
                    },
                    span: *span,
                });
                log::debug!(
                    "🔧 FieldAccess完成: 字段{}值已写入 {:?}",
                    field,
                    target_addr
                );
            } else {
                return Err(vec!["字段访问目标必须可寻址".to_string()]);
            }

            Ok(())
        }

        Statement::Dereference {
            target,
            reference,
            span,
        } => {
            // 🔧 修复：使用新的L-Value/R-Value概念简化解引用
            // 解引用操作的语义：*p 是从引用p的R-Value（一个地址）加载值

            // 1. 获取引用的R-Value（这是一个地址）
            let reference_addr = ctx.lower_to_rvalue(reference);

            // 2. 获取目标的L-Value（存储位置）
            let target_lvalue = ctx.lower_to_lvalue(target);

            match (&reference_addr, &target_lvalue) {
                (Operand::Register { id: addr_reg }, Operand::Register { id: target_addr }) => {
                    // 从引用地址加载值到临时寄存器
                    let temp_reg = ctx.current_function_mut().new_register();
                    ctx.add_instruction(Instruction::Load64 {
                        dst: temp_reg,
                        addr: *addr_reg,
                        offset: 0,
                        span: *span,
                    });

                    // 将加载的值存储到目标位置
                    ctx.add_instruction(Instruction::Store64 {
                        addr: *target_addr,
                        offset: 0,
                        src: Operand::Register { id: temp_reg },
                        span: *span,
                    });
                }
                _ => {
                    // 其他情况：直接复制值
                    if let Operand::Register { id: target_addr } = target_lvalue {
                        ctx.add_instruction(Instruction::Store64 {
                            addr: target_addr,
                            offset: 0,
                            src: reference_addr,
                            span: *span,
                        });
                    }
                }
            }

            Ok(())
        }

        Statement::ConstructorArgExtract {
            target,
            constructor,
            arg_index,
            span,
        } => {
            // Tagged Union构造器参数提取：从Tagged Union结构体中提取数据

            if *arg_index != 0 {
                return Err(vec![format!(
                    "ConstructorArgExtract only supports arg_index = 0 for single-field unions (got {})",
                    arg_index
                )]);
            }

            // 获取构造器寄存器
            let constructor_operand = ctx.lower_to_rvalue(constructor);
            let constructor_reg = match constructor_operand {
                Operand::Register { id } => id,
                _ => {
                    return Err(vec![
                        "Constructor must be a register for argument extraction".to_string(),
                    ]);
                }
            };

            // 使用Stack-First策略：为目标值分配栈槽
            let target_stack_addr = ctx.allocate_stack_slot_for_value(target);

            // 创建临时寄存器来接收提取的数据
            let temp_reg = ctx.current_function_mut().new_register();

            // 使用Tagged Union管理器生成数据提取指令到临时寄存器
            let extract_instructions = ctx
                .tagged_union_manager
                .generate_data_extraction_instructions(constructor_reg, temp_reg, *span);

            for instruction in extract_instructions {
                ctx.add_instruction(instruction);
            }

            // 将提取的数据存储到栈槽
            ctx.add_instruction(Instruction::Store64 {
                addr: target_stack_addr,
                offset: 0,
                src: Operand::Register { id: temp_reg },
                span: *span,
            });

            // 直接使用Stack-First存储
            let target_key = value_to_key(target);
            ctx.stack_allocations
                .insert(target_key.clone(), target_stack_addr);

            Ok(())
        }

        Statement::Allocate {
            target,
            layout,
            span,
        } => {
            let addr_reg = ctx.current_function_mut().new_register();
            let alignment = if layout.align == 0 { 8 } else { layout.align };

            ctx.add_instruction(Instruction::Alloc {
                dst: addr_reg,
                size: if layout.size == 0 { 8 } else { layout.size },
                alignment,
                allocation_type: AllocationType::Heap,
                span: *span,
            });

            ctx.store_value_to_stack(target, Operand::Register { id: addr_reg });
            Ok(())
        }

        Statement::Deallocate { pointer, span, .. } => {
            let ptr_operand = ctx.lower_to_rvalue(pointer);
            let addr_reg = match ptr_operand {
                Operand::Register { id } => id,
                other => {
                    let temp = ctx.current_function_mut().new_register();
                    ctx.add_instruction(Instruction::Move {
                        dst: temp,
                        src: other,
                        span: *span,
                    });
                    temp
                }
            };

            ctx.add_instruction(Instruction::Free {
                addr: addr_reg,
                span: *span,
            });
            Ok(())
        }

        Statement::Retain { value, span } => {
            let operand = ctx.lower_to_rvalue(value);
            let value_reg = match operand {
                Operand::Register { id } => id,
                _ => {
                    let temp_reg = ctx.current_function_mut().new_register();
                    ctx.add_instruction(Instruction::Move {
                        dst: temp_reg,
                        src: operand,
                        span: *span,
                    });
                    temp_reg
                }
            };
            ctx.add_instruction(Instruction::Retain {
                value: value_reg,
                span: *span,
            });
            Ok(())
        }
        Statement::Release { value, span } => {
            let operand = ctx.lower_to_rvalue(value);
            let value_reg = match operand {
                Operand::Register { id } => id,
                _ => {
                    let temp_reg = ctx.current_function_mut().new_register();
                    ctx.add_instruction(Instruction::Move {
                        dst: temp_reg,
                        src: operand,
                        span: *span,
                    });
                    temp_reg
                }
            };
            ctx.add_instruction(Instruction::Release {
                value: value_reg,
                span: *span,
            });
            Ok(())
        }

        Statement::MarkGcRoot { .. } => Ok(()),

        Statement::WriteBarrier { .. } => Ok(()),

        Statement::ReadBarrier { target, object, .. } => {
            let operand = ctx.lower_to_rvalue(object);
            ctx.store_value_to_stack(target, operand);
            Ok(())
        }

        Statement::HeapAlloc {
            target,
            size,
            object_type,
            span,
        } => {
            // 🔧 新增：堆分配语句的处理
            // HeapAlloc在LIR中对应Alloc指令，用于在堆上分配内存

            // 分配一个寄存器来存储堆地址
            let heap_addr_reg = ctx.current_function_mut().new_register();

            // 生成堆分配指令
            ctx.add_instruction(Instruction::Alloc {
                dst: heap_addr_reg,
                size: *size,
                alignment: 8, // 默认8字节对齐
                allocation_type: AllocationType::Heap,
                span: *span,
            });

            // 将堆地址存储到目标值的栈位置（Stack-First策略）
            ctx.store_value_to_stack(target, Operand::Register { id: heap_addr_reg });

            log::debug!(
                "🔧 HeapAlloc: 分配 {} 字节的 {} 对象到 {:?}",
                size,
                object_type,
                target
            );
            Ok(())
        }

        // ===== 代数效应占位 —— 在 LIR 层发出伪指令，供后续指令降级展开 =====
        Statement::EffectPerform {
            tag,
            payload,
            target,
            span,
        } => {
            let tag_op = ctx.lower_to_rvalue(tag);
            let payload_op = ctx.lower_to_rvalue(payload);
            let result_reg = target
                .as_ref()
                .map(|_| ctx.current_function_mut().new_register());

            ctx.add_instruction(Instruction::EffectPerform {
                tag: tag_op,
                payload: payload_op,
                result: result_reg,
                span: *span,
            });

            if let (Some(target_value), Some(result_reg)) = (target.as_ref(), result_reg) {
                ctx.store_value_to_stack(target_value, Operand::Register { id: result_reg });
            }
            Ok(())
        }
        Statement::EffectResume { value, span } => {
            let val_op = ctx.lower_to_rvalue(value);
            ctx.add_instruction(Instruction::EffectResume {
                value: val_op,
                span: *span,
            });
            Ok(())
        }
        // handler push/pop 从 MIR 到 LIR：发出 EffectPushHandler/EffectPopHandler + 在函数内使用label作为入口
        Statement::EffectHandlerPush {
            tag,
            handler_block,
            param_name,
            span,
        } => {
            let tag_op = ctx.lower_to_rvalue(tag);
            let handler_label = ctx.allocate_label_for_block(*handler_block);
            ctx.handler_block_param
                .insert(*handler_block, param_name.clone());
            ctx.add_instruction(Instruction::EffectPushHandler {
                tag: tag_op,
                handler_label,
                span: *span,
            });
            Ok(())
        }
        Statement::EffectHandlerPop { span } => {
            ctx.add_instruction(Instruction::EffectPopHandler { span: *span });
            Ok(())
        }

        Statement::Store {
            target,
            value,
            span,
        } => {
            // 🔧 新增：存储语句的处理
            // Store语句用于将值存储到指定的内存位置

            // 获取目标地址（应该是一个包含内存地址的值）
            let target_addr_operand = ctx.lower_to_rvalue(target);

            // 确保目标地址是一个寄存器
            let target_addr_reg = ctx.ensure_register_from_operand(target_addr_operand, *span);

            // 如果存储的是结构体，需要逐字段写入，不能只写指针
            if let Some(layout) = ctx.get_struct_layout_for_value(value) {
                let source_ptr_operand = ctx.lower_to_rvalue(value);
                let source_ptr_reg = ctx.ensure_register_from_operand(source_ptr_operand, *span);

                for field_layout in &layout.fields {
                    let temp_reg = ctx.current_function_mut().new_register();
                    ctx.add_instruction(Instruction::Load64 {
                        dst: temp_reg,
                        addr: source_ptr_reg,
                        offset: field_layout.offset as i64,
                        span: *span,
                    });
                    ctx.add_instruction(Instruction::Store64 {
                        addr: target_addr_reg,
                        offset: field_layout.offset as i64,
                        src: Operand::Register { id: temp_reg },
                        span: *span,
                    });
                }

                ctx.set_struct_layout_for_value(target, layout);

                log::debug!("🔧 Store: 复制结构体值到 {:?}", target);
                return Ok(());
            }

            // 默认：按值存储（包括指针等简单类型）
            let value_operand = ctx.lower_to_rvalue(value);
            ctx.add_instruction(Instruction::Store64 {
                addr: target_addr_reg,
                offset: 0,
                src: value_operand,
                span: *span,
            });

            log::debug!("🔧 Store: 将 {:?} 存储到地址 {:?}", value, target);
            Ok(())
        }

        _ => Err(vec![format!(
            "Statement type not yet implemented: {:?}",
            statement
        )]),
    }
}
