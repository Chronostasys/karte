use crate::{
    BasicBlock, BasicBlockId, BinaryOperator as MirBinaryOp, MirFunction, MirProgram,
    Pattern as MirPattern, Statement, Terminator, UnaryOperator as MirUnaryOp,
    Value, MatchArm, TempId,
};
use karte_diagnostics::Span;
use karte_hir::{Expr, BinaryOperator as HirBinaryOp, UnaryOperator as HirUnaryOp, Pattern as HirPattern};
use std::collections::HashMap;

/// HIR到MIR的lowering上下文
pub struct LoweringContext {
    /// 当前函数
    current_function: Option<MirFunction>,
    /// 当前基本块
    current_block: Option<BasicBlockId>,
    /// 变量作用域
    variables: HashMap<String, Value>,
    /// 错误信息
    errors: Vec<String>,
}

impl LoweringContext {
    pub fn new() -> Self {
        Self {
            current_function: None,
            current_block: None,
            variables: HashMap::new(),
            errors: Vec::new(),
        }
    }

    /// 开始新函数
    pub fn start_function(&mut self, name: String, params: Vec<String>) {
        let mut function = MirFunction::new(name, params.clone());
        self.current_block = Some(function.entry_block);
        
        // 将参数添加到变量作用域
        for param in params {
            self.variables.insert(param.clone(), Value::Variable { name: param });
        }
        
        self.current_function = Some(function);
    }

    /// 完成当前函数
    pub fn finish_function(&mut self) -> Option<MirFunction> {
        self.current_function.take()
    }

    /// 获取当前函数的可变引用
    fn current_function_mut(&mut self) -> &mut MirFunction {
        self.current_function.as_mut().expect("No current function")
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
    fn new_temp(&mut self) -> TempId {
        self.current_function_mut().new_temp()
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
    let mut context = LoweringContext::new();
    let mut program = MirProgram::new();
    
    // 创建主函数
    context.start_function("main".to_string(), vec![]);
    
    // 降级表达式
    let result_value = lower_expression(&mut context, expr)?;
    
    // 添加返回语句
    context.set_terminator(Terminator::Return {
        value: Some(result_value),
        span: expr.span(),
    });
    
    // 完成函数并添加到程序
    if let Some(main_func) = context.finish_function() {
        program.add_function(main_func);
        program.set_main("main".to_string());
    }
    
    if context.errors.is_empty() {
        Ok(program)
    } else {
        Err(context.errors)
    }
}

/// 降级单个表达式
fn lower_expression(ctx: &mut LoweringContext, expr: &Expr) -> Result<Value, Vec<String>> {
    match expr {
        Expr::Number { value, .. } => {
            Ok(Value::Number { value: *value })
        }
        
        Expr::Unit { .. } => {
            Ok(Value::Unit)
        }
        
        Expr::Boolean { value, .. } => {
            Ok(Value::Boolean { value: *value })
        }
        
        Expr::Identifier { name, .. } => {
            if let Some(value) = ctx.variables.get(name) {
                Ok(value.clone())
            } else {
                ctx.errors.push(format!("Undefined variable: {}", name));
                Err(ctx.errors.clone())
            }
        }
        
        Expr::BinaryOp { left, op, right, span } => {
            // 降级左右操作数
            let left_val = lower_expression(ctx, left)?;
            let right_val = lower_expression(ctx, right)?;
            
            // 创建临时变量存储结果
            let temp_id = ctx.new_temp();
            let result_val = Value::Temp { id: temp_id };
            
            // 添加二元运算语句
            ctx.add_statement(Statement::BinaryOp {
                target: result_val.clone(),
                left: left_val,
                op: convert_binary_op(op),
                right: right_val,
                span: *span,
            });
            
            Ok(result_val)
        }
        
        Expr::UnaryOp { op, operand, span } => {
            // 降级操作数
            let operand_val = lower_expression(ctx, operand)?;
            
            // 创建临时变量存储结果
            let temp_id = ctx.new_temp();
            let result_val = Value::Temp { id: temp_id };
            
            // 添加一元运算语句
            ctx.add_statement(Statement::UnaryOp {
                target: result_val.clone(),
                op: convert_unary_op(op),
                operand: operand_val,
                span: *span,
            });
            
            Ok(result_val)
        }
        
        Expr::If { condition, then_branch, else_branch, span } => {
            // 降级条件表达式
            let condition_val = lower_expression(ctx, condition)?;
            
            // 创建基本块
            let then_block = ctx.new_block();
            let else_block = ctx.new_block();
            let merge_block = ctx.new_block();
            
            // 添加条件分支终结语句
            ctx.set_terminator(Terminator::Branch {
                condition: condition_val,
                then_block,
                else_block,
                span: *span,
            });
            
            // 处理then分支
            ctx.set_current_block(then_block);
            let then_val = lower_expression(ctx, then_branch)?;
            let then_temp = ctx.new_temp();
            let then_result = Value::Temp { id: then_temp };
            ctx.add_statement(Statement::Assign {
                target: then_result.clone(),
                source: then_val,
                span: then_branch.span(),
            });
            ctx.set_terminator(Terminator::Goto {
                target: merge_block,
                span: then_branch.span(),
            });
            
            // 处理else分支
            ctx.set_current_block(else_block);
            let else_result = if let Some(else_branch) = else_branch {
                let else_val = lower_expression(ctx, else_branch)?;
                let else_temp = ctx.new_temp();
                let else_result = Value::Temp { id: else_temp };
                ctx.add_statement(Statement::Assign {
                    target: else_result.clone(),
                    source: else_val,
                    span: else_branch.span(),
                });
                else_result
            } else {
                // 如果没有else分支，默认返回unit
                Value::Unit
            };
            ctx.set_terminator(Terminator::Goto {
                target: merge_block,
                span: else_branch.as_ref().map(|e| e.span()).unwrap_or(*span),
            });
            
            // 切换到合并块
            ctx.set_current_block(merge_block);
            
            // 创建结果临时变量（这里简化处理，实际应该使用PHI节点）
            let result_temp = ctx.new_temp();
            let result_val = Value::Temp { id: result_temp };
            
            // 这里应该是PHI节点，但为了简化，我们使用then分支的结果
            // 在实际实现中，需要更复杂的合并逻辑
            ctx.add_statement(Statement::Assign {
                target: result_val.clone(),
                source: then_result,
                span: *span,
            });
            
            Ok(result_val)
        }
        
        Expr::While { condition, body, span } => {
            // 创建基本块
            let header_block = ctx.new_block();
            let body_block = ctx.new_block();
            let exit_block = ctx.new_block();
            
            // 跳转到循环头部
            ctx.set_terminator(Terminator::Goto {
                target: header_block,
                span: *span,
            });
            
            // 处理循环头部（条件检查）
            ctx.set_current_block(header_block);
            let condition_val = lower_expression(ctx, condition)?;
            ctx.set_terminator(Terminator::Branch {
                condition: condition_val,
                then_block: body_block,
                else_block: exit_block,
                span: condition.span(),
            });
            
            // 处理循环体
            ctx.set_current_block(body_block);
            let _body_val = lower_expression(ctx, body)?;
            // 循环体执行完后跳回头部
            ctx.set_terminator(Terminator::Goto {
                target: header_block,
                span: body.span(),
            });
            
            // 设置当前块为退出块
            ctx.set_current_block(exit_block);
            
            // while表达式返回unit
            Ok(Value::Unit)
        }
        
        // 其他表达式类型将在后续步骤实现
        _ => {
            ctx.errors.push("Expression type not yet implemented".to_string());
            Err(ctx.errors.clone())
        }
    }
}

/// 转换HIR二元运算符到MIR
fn convert_binary_op(op: &HirBinaryOp) -> MirBinaryOp {
    match op {
        HirBinaryOp::Add => MirBinaryOp::Add,
        HirBinaryOp::Subtract => MirBinaryOp::Subtract,
        HirBinaryOp::Multiply => MirBinaryOp::Multiply,
        HirBinaryOp::Divide => MirBinaryOp::Divide,
    }
}

/// 转换HIR一元运算符到MIR
fn convert_unary_op(op: &HirUnaryOp) -> MirUnaryOp {
    match op {
        HirUnaryOp::Plus => MirUnaryOp::Plus,
        HirUnaryOp::Minus => MirUnaryOp::Minus,
    }
} 