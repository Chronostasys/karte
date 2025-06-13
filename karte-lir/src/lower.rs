use crate::{Instruction, LabelId, LirFunction, LirProgram, Operand, RegisterId};
use karte_mir::{
    BasicBlockId, BinaryOperator, MirFunction, MirProgram, Statement, Terminator, UnaryOperator,
    Value,
};
use std::collections::HashMap;

/// MIR到LIR的lowering上下文
pub struct LirLoweringContext {
    /// 当前LIR函数
    current_function: Option<LirFunction>,
    /// MIR值到寄存器的映射
    value_to_register: HashMap<String, RegisterId>,
    /// MIR基本块到标签的映射
    block_to_label: HashMap<BasicBlockId, LabelId>,
    /// 错误信息
    errors: Vec<String>,
}

impl LirLoweringContext {
    pub fn new() -> Self {
        Self {
            current_function: None,
            value_to_register: HashMap::new(),
            block_to_label: HashMap::new(),
            errors: Vec::new(),
        }
    }

    /// 开始新函数
    pub fn start_function(&mut self, name: String) {
        self.current_function = Some(LirFunction::new(name));
        self.value_to_register.clear();
        self.block_to_label.clear();
    }

    /// 完成当前函数
    pub fn finish_function(&mut self) -> Option<LirFunction> {
        self.current_function.take()
    }

    /// 获取当前函数的可变引用
    fn current_function_mut(&mut self) -> &mut LirFunction {
        self.current_function.as_mut().expect("No current function")
    }

    /// 为值分配寄存器
    fn allocate_register_for_value(&mut self, value: &Value) -> RegisterId {
        let key = value_to_key(value);
        if let Some(&register) = self.value_to_register.get(&key) {
            register
        } else {
            let register = self.current_function_mut().new_register();
            self.value_to_register.insert(key, register);
            register
        }
    }

    /// 为基本块分配标签
    fn allocate_label_for_block(&mut self, block_id: BasicBlockId) -> LabelId {
        if let Some(&label) = self.block_to_label.get(&block_id) {
            label
        } else {
            let label = self.current_function_mut().new_label();
            self.block_to_label.insert(block_id, label);
            label
        }
    }

    /// 将值转换为操作数
    fn value_to_operand(&mut self, value: &Value) -> Operand {
        match value {
            Value::Number { value } => Operand::Immediate { value: *value },
            Value::Unit => Operand::Immediate { value: 0 }, // Unit表示为0
            Value::Boolean { value } => Operand::Immediate {
                value: if *value { 1 } else { 0 },
            },
            _ => {
                let register = self.allocate_register_for_value(value);
                Operand::Register { id: register }
            }
        }
    }

    /// 添加指令
    fn add_instruction(&mut self, instruction: Instruction) {
        self.current_function_mut().add_instruction(instruction);
    }
}

/// 将MIR程序转换为LIR程序
pub fn lower_mir_to_lir(mir_program: &MirProgram) -> Result<LirProgram, Vec<String>> {
    let mut context = LirLoweringContext::new();
    let mut lir_program = LirProgram::new();

    // 转换每个函数
    for (name, mir_function) in &mir_program.functions {
        context.start_function(name.clone());

        // 预分配所有基本块的标签
        for &block_id in mir_function.basic_blocks.keys() {
            context.allocate_label_for_block(block_id);
        }

        // 转换每个基本块
        // 按ID顺序处理基本块
        let mut block_ids: Vec<_> = mir_function.basic_blocks.keys().copied().collect();
        block_ids.sort_by_key(|id| id.0);

        for block_id in block_ids {
            if let Some(block) = mir_function.basic_blocks.get(&block_id) {
                // 添加基本块标签
                let label = context.allocate_label_for_block(block_id);
                context.add_instruction(Instruction::Label {
                    id: label,
                    span: karte_diagnostics::Span::new(0, 0), // 简化span处理
                });

                // 转换基本块中的语句
                for statement in &block.statements {
                    if let Err(errors) = lower_statement(&mut context, statement) {
                        context.errors.extend(errors);
                    }
                }

                // 转换终结语句
                if let Some(terminator) = &block.terminator {
                    if let Err(errors) = lower_terminator(&mut context, terminator) {
                        context.errors.extend(errors);
                    }
                }
            }
        }

        // 完成函数并添加到程序
        if let Some(lir_function) = context.finish_function() {
            lir_program.add_function(lir_function);
        }
    }

    // 设置主函数
    if let Some(main_name) = &mir_program.main_function {
        lir_program.set_main(main_name.clone());
    }

    if context.errors.is_empty() {
        Ok(lir_program)
    } else {
        Err(context.errors)
    }
}

/// 转换MIR语句为LIR指令
fn lower_statement(
    ctx: &mut LirLoweringContext,
    statement: &Statement,
) -> Result<(), Vec<String>> {
    match statement {
        Statement::Assign { target, source, span } => {
            let dst = ctx.allocate_register_for_value(target);
            let src = ctx.value_to_operand(source);
            ctx.add_instruction(Instruction::Move {
                dst,
                src,
                span: *span,
            });
            Ok(())
        }

        Statement::BinaryOp {
            target,
            left,
            op,
            right,
            span,
        } => {
            let dst = ctx.allocate_register_for_value(target);
            let src1 = ctx.value_to_operand(left);
            let src2 = ctx.value_to_operand(right);

            let instruction = match op {
                BinaryOperator::Add => Instruction::Add {
                    dst,
                    src1,
                    src2,
                    span: *span,
                },
                BinaryOperator::Subtract => Instruction::Sub {
                    dst,
                    src1,
                    src2,
                    span: *span,
                },
                BinaryOperator::Multiply => Instruction::Mul {
                    dst,
                    src1,
                    src2,
                    span: *span,
                },
                BinaryOperator::Divide => Instruction::Div {
                    dst,
                    src1,
                    src2,
                    span: *span,
                },
                _ => {
                    return Err(vec![format!("Unsupported binary operator: {:?}", op)]);
                }
            };

            ctx.add_instruction(instruction);
            Ok(())
        }

        Statement::UnaryOp {
            target,
            op,
            operand,
            span,
        } => {
            let dst = ctx.allocate_register_for_value(target);
            let src = ctx.value_to_operand(operand);

            match op {
                UnaryOperator::Plus => {
                    // +x is just x, so we move it
                    ctx.add_instruction(Instruction::Move {
                        dst,
                        src,
                        span: *span,
                    });
                }
                UnaryOperator::Minus => {
                    // -x is 0 - x
                    ctx.add_instruction(Instruction::Sub {
                        dst,
                        src1: Operand::Immediate { value: 0 },
                        src2: src,
                        span: *span,
                    });
                }
                _ => {
                    return Err(vec![format!("Unsupported unary operator: {:?}", op)]);
                }
            }
            Ok(())
        }

        _ => Err(vec!["Statement type not yet implemented".to_string()]),
    }
}

/// 转换MIR终结语句为LIR指令
fn lower_terminator(
    ctx: &mut LirLoweringContext,
    terminator: &Terminator,
) -> Result<(), Vec<String>> {
    match terminator {
        Terminator::Goto { target, span } => {
            let label = ctx.allocate_label_for_block(*target);
            ctx.add_instruction(Instruction::Jump {
                target: label,
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
            let condition_operand = ctx.value_to_operand(condition);
            let then_label = ctx.allocate_label_for_block(*then_block);
            let else_label = ctx.allocate_label_for_block(*else_block);

            // 比较条件与0（假值）
            ctx.add_instruction(Instruction::Compare {
                src1: condition_operand,
                src2: Operand::Immediate { value: 0 },
                span: *span,
            });

            // 如果不等于0（真值），跳转到then_block
            ctx.add_instruction(Instruction::JumpNotEqual {
                target: then_label,
                span: *span,
            });

            // 否则跳转到else_block
            ctx.add_instruction(Instruction::Jump {
                target: else_label,
                span: *span,
            });

            Ok(())
        }

        Terminator::Return { value, span } => {
            let register = if let Some(value) = value {
                Some(ctx.allocate_register_for_value(value))
            } else {
                None
            };

            ctx.add_instruction(Instruction::Return {
                value: register,
                span: *span,
            });
            Ok(())
        }

        _ => Err(vec!["Terminator type not yet implemented".to_string()]),
    }
}

/// 将值转换为字符串键用于映射
fn value_to_key(value: &Value) -> String {
    match value {
        Value::Variable { name } => format!("var:{}", name),
        Value::Temp { id } => format!("temp:{}", id.0),
        Value::Number { value } => format!("num:{}", value),
        Value::Boolean { value } => format!("bool:{}", value),
        Value::Unit => "unit".to_string(),
        Value::Constructor { name, .. } => format!("ctor:{}", name),
        Value::QualifiedConstructor {
            type_name,
            constructor_name,
            ..
        } => format!("qctor:{}::{}", type_name, constructor_name),
    }
} 