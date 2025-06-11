use karte_hir::{BinaryOperator, Expr, Statement, UnaryOperator};
use std::collections::HashMap;

/// 值类型 - 运行时的值表示
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Number(i64),
    Unit, // 空值类型，对应Rust的()
    Function {
        params: Vec<String>,
        body: Expr,
        closure: HashMap<String, Value>, // 闭包捕获的变量
    },
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Number(n) => write!(f, "{}", n),
            Value::Unit => write!(f, "()"),
            Value::Function { params, .. } => write!(f, "fn({})", params.join(", ")),
        }
    }
}

/// 环境 - 变量绑定的上下文
type Environment = HashMap<String, Value>;

/// 表达式求值器 - 负责执行/求值抽象语法树
pub fn evaluate(expr: &Expr) -> Result<Value, String> {
    let env = HashMap::new();
    evaluate_with_env(expr, &env)
}

/// 带环境的求值
fn evaluate_with_env(expr: &Expr, env: &Environment) -> Result<Value, String> {
    match expr {
        Expr::Number { value, .. } => Ok(Value::Number(*value)),

        Expr::Unit { .. } => Ok(Value::Unit),

        Expr::Identifier { name, .. } => env
            .get(name)
            .cloned()
            .ok_or_else(|| format!("Undefined variable: {}", name)),

        Expr::BinaryOp {
            left, op, right, ..
        } => {
            let left_val = evaluate_with_env(left, env)?;
            let right_val = evaluate_with_env(right, env)?;

            match (left_val, right_val) {
                (Value::Number(l), Value::Number(r)) => {
                    let result = match op {
                        BinaryOperator::Add => l + r,
                        BinaryOperator::Subtract => l - r,
                        BinaryOperator::Multiply => l * r,
                        BinaryOperator::Divide => {
                            if r == 0 {
                                return Err("Division by zero".to_string());
                            }
                            l / r
                        }
                    };
                    Ok(Value::Number(result))
                }
                _ => Err("Binary operations are only supported on numbers".to_string()),
            }
        }

        Expr::UnaryOp { op, operand, .. } => {
            let operand_val = evaluate_with_env(operand, env)?;

            match operand_val {
                Value::Number(n) => {
                    let result = match op {
                        UnaryOperator::Plus => n,
                        UnaryOperator::Minus => -n,
                    };
                    Ok(Value::Number(result))
                }
                _ => Err("Unary operations are only supported on numbers".to_string()),
            }
        }

        Expr::Lambda { params, body, .. } => {
            Ok(Value::Function {
                params: params.clone(),
                body: (**body).clone(),
                closure: env.clone(), // 捕获当前环境
            })
        }

        Expr::FunctionCall { function, args, .. } => {
            let func_val = evaluate_with_env(function, env)?;
            let arg_vals: Result<Vec<Value>, String> =
                args.iter().map(|arg| evaluate_with_env(arg, env)).collect();
            let arg_vals = arg_vals?;

            match func_val {
                Value::Function {
                    params,
                    body,
                    closure,
                } => {
                    if params.len() != arg_vals.len() {
                        return Err(format!(
                            "Function expects {} arguments, got {}",
                            params.len(),
                            arg_vals.len()
                        ));
                    }

                    // 创建新的环境，包含闭包和参数绑定
                    let mut new_env = closure;
                    for (param, arg_val) in params.iter().zip(arg_vals.iter()) {
                        new_env.insert(param.clone(), arg_val.clone());
                    }

                    evaluate_with_env(&body, &new_env)
                }
                _ => Err("Cannot call non-function value".to_string()),
            }
        }

        Expr::Statement { stmt, .. } => {
            let mut env_copy = env.clone();
            evaluate_statement(stmt, &mut env_copy)
        }

        Expr::Block {
            statements,
            final_expr,
            ..
        } => {
            let mut current_env = env.clone();

            // 执行所有语句
            for statement in statements {
                evaluate_statement(statement, &mut current_env)?;
            }

            // 返回最终表达式的值，如果没有则返回Unit
            if let Some(expr) = final_expr {
                evaluate_with_env(expr, &current_env)
            } else {
                Ok(Value::Unit)
            }
        }
    }
}

/// 语句求值器 - 语句总是返回Unit，但可能修改环境
fn evaluate_statement(stmt: &Statement, env: &mut Environment) -> Result<Value, String> {
    match stmt {
        Statement::Let { name, value, .. } => {
            let val = evaluate_with_env(value, env)?;
            env.insert(name.clone(), val);
            Ok(Value::Unit)
        }
        Statement::Expression { expr, .. } => {
            // 表达式语句：求值但丢弃结果
            evaluate_with_env(expr, env)?;
            Ok(Value::Unit)
        }
    }
}

/// 便捷函数：将Value转换为数字（如果可能）
pub fn value_to_number(value: &Value) -> Result<i64, String> {
    match value {
        Value::Number(n) => Ok(*n),
        Value::Unit => Err("Unit value cannot be converted to number".to_string()),
        _ => Err(format!("Expected number, got {}", value)),
    }
}

/// 兼容性函数：保持原有的API
pub fn evaluate_legacy(expr: &Expr) -> Result<i64, String> {
    evaluate(expr).and_then(|v| value_to_number(&v))
}
