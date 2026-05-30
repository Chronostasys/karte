//! LIR降低过程的终结器处理
//!
//! 本模块负责将MIR终结器转换为LIR指令。

use super::types::LirLoweringContext;
use crate::{Instruction, Operand};
use karte_mir::{Terminator, Value};

pub(super) fn lower_terminator(
    ctx: &mut LirLoweringContext,
    terminator: &Terminator,
) -> Result<(), Vec<String>> {
    use karte_mir::Terminator;

    match terminator {
        Terminator::Return { value, span } => {
            if let Some(return_value) = value {
                // 获取返回值的操作数
                let return_operand = ctx.lower_to_rvalue(return_value);

                // 如果操作数是寄存器，直接使用；否则先移动到临时寄存器
                let return_register = match return_operand {
                    Operand::Register { id } => id,
                    _ => {
                        // 创建临时寄存器并移动值
                        let temp_reg = ctx.current_function_mut().new_register();
                        ctx.add_instruction(Instruction::Move {
                            dst: temp_reg,
                            src: return_operand,
                            span: *span,
                        });
                        temp_reg
                    }
                };

                // 生成返回指令
                ctx.add_instruction(Instruction::Return {
                    value: Some(return_register),
                    span: *span,
                });
            } else {
                // 无返回值的返回
                ctx.add_instruction(Instruction::Return {
                    value: None,
                    span: *span,
                });
            }
            Ok(())
        }

        Terminator::Goto { target, span } => {
            let target_label = ctx.allocate_label_for_block(*target);
            ctx.add_instruction(Instruction::Jump {
                target: target_label,
                span: *span,
            });
            Ok(())
        }

        Terminator::Branch {
            condition,
            then_block,
            else_block,
            span,
        } => {
            // 🔧 重大修复：确保条件值是从正确的源获取的
            // 检查条件是否是比较操作的结果（临时变量）
            let condition_operand = match condition {
                Value::Temp { .. } => {
                    // 临时变量：应该从栈加载其值
                    let temp_reg = ctx.current_function_mut().new_register();
                    let stack_addr = ctx.lower_to_lvalue(condition);
                    if let Operand::Register { id: addr_reg } = stack_addr {
                        ctx.add_instruction(Instruction::Load64 {
                            dst: temp_reg,
                            addr: addr_reg,
                            offset: 0,
                            span: *span,
                        });
                        Operand::Register { id: temp_reg }
                    } else {
                        // 如果不是寄存器地址，使用默认的rvalue逻辑
                        ctx.lower_to_rvalue(condition)
                    }
                }
                _ => {
                    // 其他值类型使用标准的rvalue逻辑
                    ctx.lower_to_rvalue(condition)
                }
            };

            let then_label = ctx.allocate_label_for_block(*then_block);
            let else_label = ctx.allocate_label_for_block(*else_block);

            log::debug!(
                "🔧 分支条件处理: condition={:?}, operand={:?}",
                condition,
                condition_operand
            );

            // 比较条件与0（false）
            ctx.add_instruction(Instruction::Compare {
                src1: condition_operand,
                src2: Operand::Immediate { value: 0 },
                span: *span,
            });

            // 如果条件不等于0（true），跳转到then分支
            ctx.add_instruction(Instruction::JumpNotEqual {
                target: then_label,
                span: *span,
            });

            // 否则跳转到else分支
            ctx.add_instruction(Instruction::Jump {
                target: else_label,
                span: *span,
            });

            Ok(())
        }

        Terminator::Match {
            value,
            arms,
            default,
            span,
        } => {
            // Tagged Union模式匹配的LIR实现：
            // 1. 对每个模式生成比较指令
            // 2. 如果匹配成功，跳转到对应的基本块
            // 3. 如果都不匹配，跳转到默认块（如果有的话）

            let match_operand = ctx.lower_to_rvalue(value);

            // 为每个匹配臂生成比较和跳转指令
            for arm in arms {
                let target_label = ctx.allocate_label_for_block(arm.target);

                match &arm.pattern {
                    karte_mir::Pattern::Wildcard => {
                        // 通配符模式总是匹配，直接跳转
                        ctx.add_instruction(Instruction::Jump {
                            target: target_label,
                            span: *span,
                        });
                        return Ok(()); // 通配符后面的模式不会被执行
                    }
                    karte_mir::Pattern::Number {
                        value: pattern_value,
                    } => {
                        // 数字模式：比较值是否相等
                        ctx.add_instruction(Instruction::Compare {
                            src1: match_operand.clone(),
                            src2: Operand::Immediate {
                                value: *pattern_value,
                            },
                            span: *span,
                        });
                        ctx.add_instruction(Instruction::JumpEqual {
                            target: target_label,
                            span: *span,
                        });
                    }
                    karte_mir::Pattern::Boolean {
                        value: pattern_value,
                    } => {
                        // Boolean模式：直接比较0/1值，不使用Tagged Union
                        let expected_value = if *pattern_value { 1 } else { 0 };

                        // 比较匹配值与期望值
                        ctx.add_instruction(Instruction::Compare {
                            src1: match_operand.clone(),
                            src2: Operand::Immediate {
                                value: expected_value,
                            },
                            span: *span,
                        });

                        // 如果值匹配，跳转到目标分支
                        ctx.add_instruction(Instruction::JumpEqual {
                            target: target_label,
                            span: *span,
                        });
                    }
                    karte_mir::Pattern::Constructor { name, args } => {
                        // Tagged Union构造器模式处理：检查标签并提取数据
                        let constructor_reg = match &match_operand {
                            Operand::Register { id } => *id,
                            _ => {
                                ctx.errors.push(
                                    "Match operand must be a register for constructor pattern"
                                        .to_string(),
                                );
                                continue;
                            }
                        };

                        // 获取期望的标签ID
                        let expected_tag_id = if name.contains("::") {
                            let parts: Vec<&str> = name.split("::").collect();
                            if parts.len() == 2 {
                                ctx.tagged_union_manager
                                    .get_qualified_constructor_id(parts[0], parts[1])
                            } else {
                                ctx.tagged_union_manager.get_constructor_id(name)
                            }
                        } else {
                            ctx.tagged_union_manager.get_constructor_id(name)
                        };

                        // constructor_reg直接包含Tagged Union的地址
                        // 生成标签检查指令
                        let temp_reg = ctx.current_function_mut().new_register();
                        let tag_check_instructions =
                            ctx.tagged_union_manager.generate_tag_check_instructions(
                                constructor_reg,
                                expected_tag_id,
                                temp_reg,
                                *span,
                            );

                        for instruction in tag_check_instructions {
                            ctx.add_instruction(instruction);
                        }

                        // 如果标签匹配，跳转到目标分支
                        ctx.add_instruction(Instruction::JumpEqual {
                            target: target_label,
                            span: *span,
                        });

                        // 多参数绑定：逐个提取参数
                        for (i, var_name) in args.iter().enumerate() {
                            let var_reg_id = ctx.allocate_register_for_value(&Value::Variable {
                                name: var_name.clone(),
                                ty: None,
                            });
                            // 第一个参数在 offset 8，后续在 offset 16, 24, ...
                            let data_offset = (8 + i * 8) as i64;
                            ctx.add_instruction(Instruction::Load64 {
                                dst: var_reg_id,
                                addr: constructor_reg,
                                offset: data_offset,
                                span: *span,
                            });
                        }
                    }
                    karte_mir::Pattern::Variable { name: _ } => {
                        // 变量模式总是匹配（类似通配符）
                        ctx.add_instruction(Instruction::Jump {
                            target: target_label,
                            span: *span,
                        });
                        return Ok(());
                    }
                }
            }

            // 如果有默认分支，跳转到默认分支
            if let Some(default_block) = default {
                let default_label = ctx.allocate_label_for_block(*default_block);
                ctx.add_instruction(Instruction::Jump {
                    target: default_label,
                    span: *span,
                });
            }

            Ok(())
        }
    }
}
