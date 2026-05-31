//! LIR降低过程的语句处理
//!
//! 本模块负责将MIR语句转换为LIR指令。

use super::helpers::value_to_key;
use super::types::LirLoweringContext;
use crate::{AllocationType, ComparisonCondition, Instruction, Operand};
use karte_mir::{BinaryOperator, Statement, UnaryOperator, Value};

pub(super) fn lower_statement(
    ctx: &mut LirLoweringContext,
    statement: &Statement,
) -> Result<(), Vec<String>> {
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

            // 🔧 关键修复：精确检查是否是 env_ptr 字段的赋值（值为0）
            // 原逻辑过于宽泛：任何值为0的临时变量赋值都会被跳过
            // 现在改为：只有当 source 关联的结构体布局为 Closure（即 source 是从 Closure
            // 结构体字段提取的值，如 env_ptr）且值为 0 时才跳过
            // 注意：保留 lower_to_rvalue 调用以维持其副作用
            let is_env_ptr_assignment = match source {
                Value::Temp { .. } | Value::Variable { .. } => {
                    // 先调用 lower_to_rvalue（保留副作用）
                    let src_rvalue = ctx.lower_to_rvalue(source);
                    if let Operand::Immediate { value: 0 } = src_rvalue {
                        // 进一步检查 source 是否关联到 Closure 结构体
                        let is_closure_related = ctx
                            .get_struct_layout_for_value(source)
                            .map(|layout| layout.name == "Closure")
                            .unwrap_or(false);
                        if is_closure_related {
                            log::debug!(
                                "🔧 检测到 Closure 结构体关联的 0 值赋值（env_ptr），跳过以避免覆盖 function_ptr"
                            );
                            true
                        } else {
                            false
                        }
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

            // 🔧 修复 struct 返回值悬挂指针：
            // 检测当前是否在给"将被 return 返回的 temp"赋值 struct 值
            // 如果是，则强制 struct 使用堆分配，避免函数返回后栈帧释放导致悬挂指针
            if let Value::Struct { name, .. } = source {
                if name != "Closure" {
                    if let Value::Temp { id, .. } = target {
                        if ctx.returned_temp_ids.contains(&id.0) {
                            log::debug!(
                                "🔧 检测到 struct 返回逃逸: temp {:?} 将被返回，强制堆分配",
                                id
                            );
                            ctx.force_struct_heap = true;
                        }
                    }
                }
            }

            // 1. 获取源值的R-Value（值本身）
            let src_rvalue = ctx.lower_to_rvalue(source);

            // 清除堆分配标志（无论是否被使用）
            ctx.force_struct_heap = false;
            log::debug!(
                "📝 Assign: target={:?}, source={:?}, src_rvalue={:?}",
                target,
                source,
                src_rvalue
            );

            // 🔧 常量追踪：如果源值是立即数，记录到 known_constants 中
            // 这样后续 lower_to_rvalue 可以直接使用立即数，避免通过栈加载
            {
                use super::helpers::value_to_key;
                let target_key = value_to_key(target);
                if let Operand::Immediate { value } = src_rvalue {
                    log::debug!("📝 常量追踪: {} = {}", target_key, value);
                    ctx.known_constants.insert(target_key, value);
                } else {
                    // 非常量赋值，清除该变量的常量记录
                    ctx.known_constants.remove(&target_key);
                }
            }

            // 2. 获取目标的L-Value（存储位置）
            let target_lvalue = ctx.lower_to_lvalue(target);

            // 3. 执行赋值：将源值存储到目标位置
            if let Operand::Register { id: target_addr } = target_lvalue {
                log::debug!(
                    "📝 Assign Store64: target_addr={:?}, src={:?}",
                    target_addr,
                    src_rvalue
                );
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

            // 🔧 常量传播：对于算术运算（Add/Sub/Mul/Div/位运算），
            // 如果操作数是已知常量，直接使用立即数，避免通过栈加载。
            // 这解决了循环中立即数被映射到物理寄存器后与其他值冲突的问题。
            let use_const_prop = matches!(
                op,
                BinaryOperator::Add
                    | BinaryOperator::Subtract
                    | BinaryOperator::Multiply
                    | BinaryOperator::BitAnd
                    | BinaryOperator::BitOr
                    | BinaryOperator::BitXor
                    | BinaryOperator::ShiftLeft
                    | BinaryOperator::ShiftRight
            );

            let src1 = if use_const_prop {
                ctx.lower_to_rvalue_with_const_prop(left)
            } else {
                ctx.lower_to_rvalue(left)
            };
            let src2 = if use_const_prop {
                ctx.lower_to_rvalue_with_const_prop(right)
            } else {
                ctx.lower_to_rvalue(right)
            };

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

                // 逻辑、比较、除法和取模操作：在下面的第二个 match 中处理
                // 这里返回一个占位指令（会被丢弃）
                BinaryOperator::And
                | BinaryOperator::Or
                | BinaryOperator::Equal
                | BinaryOperator::NotEqual
                | BinaryOperator::LessThan
                | BinaryOperator::LessEqual
                | BinaryOperator::GreaterThan
                | BinaryOperator::GreaterEqual
                | BinaryOperator::Divide
                | BinaryOperator::Modulo => {
                    // CompareSet 在下面的第二个 match 中通过 ctx.add_instruction 添加，
                    // 这里返回一个无副作用的占位指令（会被覆盖）
                    Instruction::Move {
                        dst: temp_register,
                        src: Operand::Immediate { value: 0 },
                        span: *span,
                    }
                }

                // 位运算指令
                BinaryOperator::BitAnd => Instruction::BitAnd {
                    dst: temp_register,
                    src1,
                    src2,
                    span: *span,
                },
                BinaryOperator::BitOr => Instruction::BitOr {
                    dst: temp_register,
                    src1,
                    src2,
                    span: *span,
                },
                BinaryOperator::BitXor => Instruction::BitXor {
                    dst: temp_register,
                    src1,
                    src2,
                    span: *span,
                },
                BinaryOperator::ShiftLeft => Instruction::ShiftLeft {
                    dst: temp_register,
                    src1,
                    src2,
                    span: *span,
                },
                BinaryOperator::ShiftRight => Instruction::ShiftRight {
                    dst: temp_register,
                    src1,
                    src2,
                    span: *span,
                },
            };

            // 处理复杂的逻辑运算和比较运算
            match op {
                BinaryOperator::Equal
                | BinaryOperator::NotEqual
                | BinaryOperator::LessThan
                | BinaryOperator::LessEqual
                | BinaryOperator::GreaterThan
                | BinaryOperator::GreaterEqual => {
                    // 使用 CompareSet 指令：直接从比较条件产生 0/1 值，不产生分支
                    // x86: cmp src1, src2; setcc dst; movzbq dst, dst
                    // AArch64: cmp src1, src2; cset dst, condition
                    let condition = match op {
                        BinaryOperator::Equal => ComparisonCondition::Equal,
                        BinaryOperator::NotEqual => ComparisonCondition::NotEqual,
                        BinaryOperator::LessThan => ComparisonCondition::LessThan,
                        BinaryOperator::LessEqual => ComparisonCondition::LessEqual,
                        BinaryOperator::GreaterThan => ComparisonCondition::GreaterThan,
                        BinaryOperator::GreaterEqual => ComparisonCondition::GreaterEqual,
                        _ => unreachable!(),
                    };
                    ctx.add_instruction(Instruction::CompareSet {
                        dst: temp_register,
                        condition,
                        src1: src1_clone,
                        src2: src2_clone,
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
                BinaryOperator::Divide => {
                    // 除零检查：如果 src2 == 0，结果为 0；否则执行除法
                    let zero_label = ctx.next_internal_label("div_zero");
                    let end_label = ctx.next_internal_label("div_end");

                    // 比较 src2 与 0
                    ctx.add_instruction(Instruction::Compare {
                        src1: src2_clone.clone(),
                        src2: Operand::Immediate { value: 0 },
                        span: *span,
                    });

                    // 如果 src2 == 0，跳转到 zero_label
                    ctx.add_instruction(Instruction::JumpEqual {
                        target: zero_label,
                        span: *span,
                    });

                    // 非零路径：执行除法
                    ctx.add_instruction(Instruction::Div {
                        dst: temp_register,
                        src1: src1_clone.clone(),
                        src2: src2_clone.clone(),
                        span: *span,
                    });

                    // 跳转到 end_label
                    ctx.add_instruction(Instruction::Jump {
                        target: end_label,
                        span: *span,
                    });

                    // 零路径：结果为 0
                    ctx.add_instruction(Instruction::Label {
                        id: zero_label,
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
                BinaryOperator::Modulo => {
                    // 除零检查：如果 src2 == 0，结果为 0；否则执行取模
                    let zero_label = ctx.next_internal_label("mod_zero");
                    let end_label = ctx.next_internal_label("mod_end");

                    // 比较 src2 与 0
                    ctx.add_instruction(Instruction::Compare {
                        src1: src2_clone.clone(),
                        src2: Operand::Immediate { value: 0 },
                        span: *span,
                    });

                    // 如果 src2 == 0，跳转到 zero_label
                    ctx.add_instruction(Instruction::JumpEqual {
                        target: zero_label,
                        span: *span,
                    });

                    // 非零路径：执行取模
                    ctx.add_instruction(Instruction::Mod {
                        dst: temp_register,
                        src1: src1_clone.clone(),
                        src2: src2_clone.clone(),
                        span: *span,
                    });

                    // 跳转到 end_label
                    ctx.add_instruction(Instruction::Jump {
                        target: end_label,
                        span: *span,
                    });

                    // 零路径：结果为 0
                    ctx.add_instruction(Instruction::Label {
                        id: zero_label,
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
                    // 🔧 修复：复用 temp_register 而非分配新的 temp_reg，
                    // 减少一个虚拟寄存器，避免 SSA/Memory2Reg 在长 && 链中值追踪错误

                    // Move 1 to result register first
                    ctx.add_instruction(Instruction::Move {
                        dst: temp_register,
                        src: Operand::Immediate { value: 1 },
                        span: *span,
                    });

                    // Subtract src from 1: result = 1 - src
                    ctx.add_instruction(Instruction::Sub {
                        dst: temp_register,
                        src1: Operand::Register { id: temp_register },
                        src2: src,
                        span: *span,
                    });
                }
                UnaryOperator::BitNot => {
                    // ~x: bitwise NOT = XOR with -1 (all 1s)
                    let temp_reg = ctx.current_function_mut().new_register();
                    ctx.add_instruction(Instruction::Move {
                        dst: temp_reg,
                        src: Operand::Immediate { value: -1 },
                        span: *span,
                    });
                    ctx.add_instruction(Instruction::BitXor {
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

        Statement::TypeCast {
            target,
            source,
            dst_bits,
            signed,
            span,
        } => {
            let src_rvalue = ctx.lower_to_rvalue(source);

            if *dst_bits == 64 {
                // 64→64：无需转换，直接存储到栈
                ctx.store_value_to_stack(target, src_rvalue);
            } else {
                // 64→8/16/32：需要截断
                let temp = ctx.current_function_mut().new_register();
                ctx.add_instruction(Instruction::IntCast {
                    dst: temp,
                    src: src_rvalue,
                    src_bits: 64,
                    dst_bits: *dst_bits,
                    signed: *signed,
                    span: *span,
                });
                // Stack-First策略：将结果存储到栈
                ctx.store_value_to_stack(target, Operand::Register { id: temp });
            }

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

            // 检查是否为运行时内建函数（字符串连接、print 等）
            if let Value::Function { name, .. } = &resolved_function {
                match name.as_str() {
                    "__runtime_string_concat" => {
                        // 字符串连接：生成 StringConcat LIR 指令
                        let left_op = ctx.lower_to_rvalue(&args[0]);
                        let right_op = ctx.lower_to_rvalue(&args[1]);

                        let left_reg = match left_op {
                            Operand::Register { id } => id,
                            _ => {
                                let temp = ctx.current_function_mut().new_register();
                                ctx.add_instruction(Instruction::Move {
                                    dst: temp,
                                    src: left_op,
                                    span: *span,
                                });
                                temp
                            }
                        };
                        let right_reg = match right_op {
                            Operand::Register { id } => id,
                            _ => {
                                let temp = ctx.current_function_mut().new_register();
                                ctx.add_instruction(Instruction::Move {
                                    dst: temp,
                                    src: right_op,
                                    span: *span,
                                });
                                temp
                            }
                        };

                        let result_reg = ctx.current_function_mut().new_register();
                        ctx.add_instruction(Instruction::StringConcat {
                            dst: result_reg,
                            left: left_reg,
                            right: right_reg,
                            span: *span,
                        });

                        if let Some(target_value) = target {
                            ctx.store_value_to_stack(
                                target_value,
                                Operand::Register { id: result_reg },
                            );
                        }
                        return Ok(());
                    }
                    "__runtime_string_equal" => {
                        // 字符串内容比较：生成 StringEqual LIR 指令
                        let left_op = ctx.lower_to_rvalue(&args[0]);
                        let right_op = ctx.lower_to_rvalue(&args[1]);

                        let left_reg = match left_op {
                            Operand::Register { id } => id,
                            _ => {
                                let temp = ctx.current_function_mut().new_register();
                                ctx.add_instruction(Instruction::Move {
                                    dst: temp,
                                    src: left_op,
                                    span: *span,
                                });
                                temp
                            }
                        };
                        let right_reg = match right_op {
                            Operand::Register { id } => id,
                            _ => {
                                let temp = ctx.current_function_mut().new_register();
                                ctx.add_instruction(Instruction::Move {
                                    dst: temp,
                                    src: right_op,
                                    span: *span,
                                });
                                temp
                            }
                        };

                        let result_reg = ctx.current_function_mut().new_register();
                        ctx.add_instruction(Instruction::StringEqual {
                            dst: result_reg,
                            left: left_reg,
                            right: right_reg,
                            span: *span,
                        });

                        if let Some(target_value) = target {
                            ctx.store_value_to_stack(
                                target_value,
                                Operand::Register { id: result_reg },
                            );
                        }
                        return Ok(());
                    }
                    "__runtime_string_char_at" => {
                        let str_op = ctx.lower_to_rvalue(&args[0]);
                        let idx_op = ctx.lower_to_rvalue(&args[1]);

                        let str_reg = match str_op {
                            Operand::Register { id } => id,
                            _ => {
                                let temp = ctx.current_function_mut().new_register();
                                ctx.add_instruction(Instruction::Move {
                                    dst: temp,
                                    src: str_op,
                                    span: *span,
                                });
                                temp
                            }
                        };
                        let idx_reg = match idx_op {
                            Operand::Register { id } => id,
                            _ => {
                                let temp = ctx.current_function_mut().new_register();
                                ctx.add_instruction(Instruction::Move {
                                    dst: temp,
                                    src: idx_op,
                                    span: *span,
                                });
                                temp
                            }
                        };

                        let result_reg = ctx.current_function_mut().new_register();
                        ctx.add_instruction(Instruction::StringCharAt {
                            dst: result_reg,
                            str_ptr: str_reg,
                            index: idx_reg,
                            span: *span,
                        });

                        if let Some(target_value) = target {
                            ctx.store_value_to_stack(
                                target_value,
                                Operand::Register { id: result_reg },
                            );
                        }
                        return Ok(());
                    }
                    "__runtime_string_substring" => {
                        let str_op = ctx.lower_to_rvalue(&args[0]);
                        let start_op = ctx.lower_to_rvalue(&args[1]);
                        let len_op = ctx.lower_to_rvalue(&args[2]);

                        let str_reg = match str_op {
                            Operand::Register { id } => id,
                            _ => {
                                let temp = ctx.current_function_mut().new_register();
                                ctx.add_instruction(Instruction::Move {
                                    dst: temp,
                                    src: str_op,
                                    span: *span,
                                });
                                temp
                            }
                        };
                        let start_reg = match start_op {
                            Operand::Register { id } => id,
                            _ => {
                                let temp = ctx.current_function_mut().new_register();
                                ctx.add_instruction(Instruction::Move {
                                    dst: temp,
                                    src: start_op,
                                    span: *span,
                                });
                                temp
                            }
                        };
                        let len_reg = match len_op {
                            Operand::Register { id } => id,
                            _ => {
                                let temp = ctx.current_function_mut().new_register();
                                ctx.add_instruction(Instruction::Move {
                                    dst: temp,
                                    src: len_op,
                                    span: *span,
                                });
                                temp
                            }
                        };

                        let result_reg = ctx.current_function_mut().new_register();
                        ctx.add_instruction(Instruction::StringSubstring {
                            dst: result_reg,
                            str_ptr: str_reg,
                            start: start_reg,
                            length: len_reg,
                            span: *span,
                        });

                        if let Some(target_value) = target {
                            ctx.store_value_to_stack(
                                target_value,
                                Operand::Register { id: result_reg },
                            );
                        }
                        return Ok(());
                    }
                    "__runtime_string_contains" => {
                        let str_op = ctx.lower_to_rvalue(&args[0]);
                        let ch_op = ctx.lower_to_rvalue(&args[1]);

                        let str_reg = match str_op {
                            Operand::Register { id } => id,
                            _ => {
                                let temp = ctx.current_function_mut().new_register();
                                ctx.add_instruction(Instruction::Move {
                                    dst: temp,
                                    src: str_op,
                                    span: *span,
                                });
                                temp
                            }
                        };
                        let ch_reg = match ch_op {
                            Operand::Register { id } => id,
                            _ => {
                                let temp = ctx.current_function_mut().new_register();
                                ctx.add_instruction(Instruction::Move {
                                    dst: temp,
                                    src: ch_op,
                                    span: *span,
                                });
                                temp
                            }
                        };

                        let result_reg = ctx.current_function_mut().new_register();
                        ctx.add_instruction(Instruction::StringContains {
                            dst: result_reg,
                            str_ptr: str_reg,
                            char_code: ch_reg,
                            span: *span,
                        });

                        if let Some(target_value) = target {
                            ctx.store_value_to_stack(
                                target_value,
                                Operand::Register { id: result_reg },
                            );
                        }
                        return Ok(());
                    }
                    "__runtime_split_count" => {
                        let str_op = ctx.lower_to_rvalue(&args[0]);
                        let sep_op = ctx.lower_to_rvalue(&args[1]);

                        let str_reg = match str_op {
                            Operand::Register { id } => id,
                            _ => {
                                let temp = ctx.current_function_mut().new_register();
                                ctx.add_instruction(Instruction::Move {
                                    dst: temp,
                                    src: str_op,
                                    span: *span,
                                });
                                temp
                            }
                        };
                        let sep_reg = match sep_op {
                            Operand::Register { id } => id,
                            _ => {
                                let temp = ctx.current_function_mut().new_register();
                                ctx.add_instruction(Instruction::Move {
                                    dst: temp,
                                    src: sep_op,
                                    span: *span,
                                });
                                temp
                            }
                        };

                        let result_reg = ctx.current_function_mut().new_register();
                        ctx.add_instruction(Instruction::SplitCount {
                            dst: result_reg,
                            str_ptr: str_reg,
                            separator: sep_reg,
                            span: *span,
                        });

                        if let Some(target_value) = target {
                            ctx.store_value_to_stack(
                                target_value,
                                Operand::Register { id: result_reg },
                            );
                        }
                        return Ok(());
                    }
                    "__runtime_to_string" => {
                        let val_op = ctx.lower_to_rvalue(&args[0]);

                        let val_reg = match val_op {
                            Operand::Register { id } => id,
                            _ => {
                                let temp = ctx.current_function_mut().new_register();
                                ctx.add_instruction(Instruction::Move {
                                    dst: temp,
                                    src: val_op,
                                    span: *span,
                                });
                                temp
                            }
                        };

                        let result_reg = ctx.current_function_mut().new_register();
                        ctx.add_instruction(Instruction::ToString {
                            dst: result_reg,
                            value: val_reg,
                            span: *span,
                        });

                        if let Some(target_value) = target {
                            ctx.store_value_to_stack(
                                target_value,
                                Operand::Register { id: result_reg },
                            );
                        }
                        return Ok(());
                    }
                    "__runtime_trim" => {
                        let str_op = ctx.lower_to_rvalue(&args[0]);

                        let str_reg = match str_op {
                            Operand::Register { id } => id,
                            _ => {
                                let temp = ctx.current_function_mut().new_register();
                                ctx.add_instruction(Instruction::Move {
                                    dst: temp,
                                    src: str_op,
                                    span: *span,
                                });
                                temp
                            }
                        };

                        let result_reg = ctx.current_function_mut().new_register();
                        ctx.add_instruction(Instruction::Trim {
                            dst: result_reg,
                            str_ptr: str_reg,
                            span: *span,
                        });

                        if let Some(target_value) = target {
                            ctx.store_value_to_stack(
                                target_value,
                                Operand::Register { id: result_reg },
                            );
                        }
                        return Ok(());
                    }
                    "__runtime_print_string" => {
                        // 打印字符串：生成 PrintString LIR 指令
                        let ptr_op = ctx.lower_to_rvalue(&args[0]);
                        let ptr_reg = match ptr_op {
                            Operand::Register { id } => id,
                            _ => {
                                let temp = ctx.current_function_mut().new_register();
                                ctx.add_instruction(Instruction::Move {
                                    dst: temp,
                                    src: ptr_op,
                                    span: *span,
                                });
                                temp
                            }
                        };

                        ctx.add_instruction(Instruction::PrintString {
                            ptr: ptr_reg,
                            span: *span,
                        });

                        // print 返回 Unit (0)
                        if let Some(target_value) = target {
                            ctx.store_value_to_stack(
                                target_value,
                                Operand::Immediate { value: 0 },
                            );
                        }
                        return Ok(());
                    }
                    "__runtime_print_number" => {
                        // 打印数字：生成 PrintNumber LIR 指令
                        let val_op = ctx.lower_to_rvalue(&args[0]);
                        let val_reg = match val_op {
                            Operand::Register { id } => id,
                            _ => {
                                let temp = ctx.current_function_mut().new_register();
                                ctx.add_instruction(Instruction::Move {
                                    dst: temp,
                                    src: val_op,
                                    span: *span,
                                });
                                temp
                            }
                        };

                        ctx.add_instruction(Instruction::PrintNumber {
                            value: val_reg,
                            span: *span,
                        });

                        // print 返回 Unit (0)
                        if let Some(target_value) = target {
                            ctx.store_value_to_stack(
                                target_value,
                                Operand::Immediate { value: 0 },
                            );
                        }
                        return Ok(());
                    }
                    "__runtime_print_bool" => {
                        // 打印布尔值：生成 PrintBool LIR 指令
                        let val_op = ctx.lower_to_rvalue(&args[0]);
                        let val_reg = match val_op {
                            Operand::Register { id } => id,
                            _ => {
                                let temp = ctx.current_function_mut().new_register();
                                ctx.add_instruction(Instruction::Move {
                                    dst: temp,
                                    src: val_op,
                                    span: *span,
                                });
                                temp
                            }
                        };

                        ctx.add_instruction(Instruction::PrintBool {
                            value: val_reg,
                            span: *span,
                        });

                        // print 返回 Unit (0)
                        if let Some(target_value) = target {
                            ctx.store_value_to_stack(
                                target_value,
                                Operand::Immediate { value: 0 },
                            );
                        }
                        return Ok(());
                    }
                    _ => {} // 继续常规处理
                }
            }

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
            else if let Value::Struct { name, fields, .. } = &resolved_function {
                if name == "Closure" {
                    // 🔧 关键修复：检查env_ptr是否为0，如果是0则不添加环境参数
                    if let Some(env_ptr) = fields.get("env_ptr") {
                        if let Value::Number { value: 0, .. } = env_ptr {
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
                Value::Variable { name, .. } => ctx.current_function_params.contains(name),
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
                    Value::Function { name, .. } => name.clone(),
                    Value::Closure { function_name, .. } => function_name.clone(),
                    Value::Variable { name, .. } => {
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

        Statement::FieldAssign {
            object,
            field,
            value,
            span,
        } => {
            // 字段赋值：将 value 写入 object 的 field 字段
            // 语义是 FieldAccess 的反向操作

            // 1. 获取结构体的基地址（栈地址或堆指针）
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

            // 4. 将值存储到字段地址
            let value_operand = ctx.lower_to_rvalue(value);
            ctx.add_instruction(Instruction::Store64 {
                addr: field_addr_reg,
                offset: 0,
                src: value_operand,
                span: *span,
            });

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

            if *arg_index == 0 {
                // 第一个参数：使用 Tagged Union 管理器的数据提取（兼容旧逻辑）
                let extract_instructions = ctx
                    .tagged_union_manager
                    .generate_data_extraction_instructions(constructor_reg, temp_reg, *span);

                for instruction in extract_instructions {
                    ctx.add_instruction(instruction);
                }
            } else {
                // 多参数构造器的后续参数：直接从 offset 8 + arg_index * 8 读取
                let data_offset = (8 + arg_index * 8) as i64;
                ctx.add_instruction(Instruction::Load64 {
                    dst: temp_reg,
                    addr: constructor_reg,
                    offset: data_offset,
                    span: *span,
                });
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
            // 检查是否启用了 karte GC 模式（gc_alloc 函数被注入）
            if let Some(&gc_alloc_label) = ctx.function_labels.get("gc_alloc") {
                // karte GC 模式: 调用 gc_alloc(size) 代替 runtime bump allocator
                let result_reg = ctx.current_function_mut().new_register();

                ctx.add_instruction(Instruction::Call {
                    target: gc_alloc_label,
                    args: vec![],
                    arg_operands: vec![Operand::Immediate { value: *size as i64 }],
                    result: Some(result_reg),
                    span: *span,
                });

                ctx.store_value_to_stack(target, Operand::Register { id: result_reg });
            } else {
                // 默认模式: 使用 runtime bump allocator (Alloc 指令)
                let heap_addr_reg = ctx.current_function_mut().new_register();
                ctx.add_instruction(Instruction::Alloc {
                    dst: heap_addr_reg,
                    size: *size,
                    alignment: 8,
                    allocation_type: AllocationType::Heap,
                    span: *span,
                });
                ctx.store_value_to_stack(target, Operand::Register { id: heap_addr_reg });
            }

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

                // 注意：不再传播结构体布局到 target。
                // Store 逐字段复制了结构体数据到 target 的内存区域，
                // 但 target 本身可能只是一个指针大小的栈槽。
                // 如果传播布局，后续对 target 的 Store 会错误地逐字段复制，
                // 导致越界写入。FieldAccess 通过 global_struct_types 动态查找布局，
                // 不依赖 struct_value_layouts。
                // ctx.set_struct_layout_for_value(target, layout);

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

        Statement::UnsafeLoad {
            target,
            addr,
            byte_size,
            span,
        } => {
            let addr_operand = ctx.lower_to_rvalue(addr);
            let addr_reg = ctx.ensure_register_from_operand(addr_operand, *span);
            // 获取 target 的栈地址，用于写回结果
            let target_lvalue = ctx.lower_to_lvalue(target);
            let target_addr_reg = match target_lvalue {
                Operand::Register { id } => id,
                _ => {
                    return Err(vec![format!(
                        "UnsafeLoad: target lvalue 不是寄存器: {:?}",
                        target_lvalue
                    )]);
                }
            };
            let temp_reg = ctx.current_function_mut().new_register();
            match byte_size {
                8 => ctx.add_instruction(Instruction::Load64 {
                    dst: temp_reg,
                    addr: addr_reg,
                    offset: 0,
                    span: *span,
                }),
                4 => ctx.add_instruction(Instruction::Load32 {
                    dst: temp_reg,
                    addr: addr_reg,
                    offset: 0,
                    span: *span,
                }),
                1 => ctx.add_instruction(Instruction::Load8 {
                    dst: temp_reg,
                    addr: addr_reg,
                    offset: 0,
                    span: *span,
                }),
                _ => {
                    return Err(vec![format!(
                        "Unsupported byte_size for unsafe_load: {}",
                        byte_size
                    )]);
                }
            }
            // 把结果写回 target 的栈 slot
            ctx.add_instruction(Instruction::Store64 {
                addr: target_addr_reg,
                offset: 0,
                src: Operand::Register { id: temp_reg },
                span: *span,
            });
            Ok(())
        }

        Statement::UnsafeStore {
            addr,
            value,
            byte_size,
            span,
        } => {
            let addr_operand = ctx.lower_to_rvalue(addr);
            let addr_reg = ctx.ensure_register_from_operand(addr_operand, *span);
            let val_operand = ctx.lower_to_rvalue(value);
            match byte_size {
                8 => ctx.add_instruction(Instruction::Store64 {
                    addr: addr_reg,
                    offset: 0,
                    src: val_operand,
                    span: *span,
                }),
                4 => ctx.add_instruction(Instruction::Store32 {
                    addr: addr_reg,
                    offset: 0,
                    src: val_operand,
                    span: *span,
                }),
                1 => ctx.add_instruction(Instruction::Store8 {
                    addr: addr_reg,
                    offset: 0,
                    src: val_operand,
                    span: *span,
                }),
                _ => {
                    return Err(vec![format!(
                        "Unsupported byte_size for unsafe_store: {}",
                        byte_size
                    )]);
                }
            }
            Ok(())
        }

        Statement::RuntimeGlobal { target, global_name, span } => {
            // 获取 target 的栈地址
            let target_lvalue = ctx.lower_to_lvalue(target);
            let addr_reg = match target_lvalue {
                Operand::Register { id } => id,
                _ => {
                    return Err(vec![format!(
                        "RuntimeGlobal: target lvalue 不是寄存器: {:?}",
                        target_lvalue
                    )]);
                }
            };
            // 分配临时寄存器，从全局数据区加载值
            let temp_reg = ctx.current_function_mut().new_register();
            ctx.add_instruction(Instruction::LoadGlobal {
                dst: temp_reg,
                name: global_name.clone(),
                span: *span,
            });
            // 把结果存回 target 的栈 slot
            ctx.add_instruction(Instruction::Store64 {
                addr: addr_reg,
                offset: 0,
                src: Operand::Register { id: temp_reg },
                span: *span,
            });
            Ok(())
        }

        Statement::Phi { .. } => {
            // Phi 节点通过 lower.rs 的 phi_store_map 在前驱块处理
            Ok(())
        }

        Statement::GcRegOp { is_push, span, .. } => {
            ctx.add_instruction(Instruction::GcRegOp {
                is_push: *is_push,
                span: *span,
            });
            Ok(())
        }

        _ => Err(vec![format!(
            "Statement type not yet implemented: {:?}",
            statement
        )]),
    }
}
