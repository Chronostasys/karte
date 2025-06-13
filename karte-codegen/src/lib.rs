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
    /// 加法类型的值 (Sum Type Value)
    Constructor {
        name: String,
        value: Option<Box<Value>>,
    },
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Number(n) => write!(f, "{}", n),
            Value::Unit => write!(f, "()"),
            Value::Function { params, .. } => write!(f, "fn({})", params.join(", ")),
            Value::Constructor { name, value } => {
                if let Some(v) = value {
                    write!(f, "{}({})", name, v)
                } else {
                    write!(f, "{}", name)
                }
            }
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

        Expr::Boolean { value, .. } => Ok(Value::Constructor {
            name: if *value { "True".to_string() } else { "False".to_string() },
            value: None,
        }),

        Expr::Constructor { name, arg, .. } => {
            if let Some(arg_expr) = arg {
                let arg_value = evaluate_with_env(arg_expr, env)?;
                Ok(Value::Constructor {
                    name: name.clone(),
                    value: Some(Box::new(arg_value)),
                })
            } else {
                Ok(Value::Constructor {
                    name: name.clone(),
                    value: None,
                })
            }
        }

        Expr::QualifiedConstructor { type_name: _, constructor_name, arg, .. } => {
            // 对于求值来说，限定构造器和普通构造器的行为相同
            // 类型检查已经确保了构造器的正确性
            if let Some(arg_expr) = arg {
                let arg_value = evaluate_with_env(arg_expr, env)?;
                Ok(Value::Constructor {
                    name: constructor_name.clone(),
                    value: Some(Box::new(arg_value)),
                })
            } else {
                Ok(Value::Constructor {
                    name: constructor_name.clone(),
                    value: None,
                })
            }
        }

        Expr::If { condition, then_branch, else_branch, .. } => {
            let condition_val = evaluate_with_env(condition, env)?;
            
            // 检查条件是否为真
            let is_true = match condition_val {
                Value::Constructor { name, .. } => name == "True",
                Value::Number(n) => n != 0, // 数字非0为真
                Value::Unit => false, // Unit为假
                _ => false,
            };
            
            if is_true {
                evaluate_with_env(then_branch, env)
            } else if let Some(else_branch) = else_branch {
                evaluate_with_env(else_branch, env)
            } else {
                Ok(Value::Unit)
            }
        }

        Expr::While { condition, body, .. } => {
            loop {
                let condition_val = evaluate_with_env(condition, env)?;
                
                // 检查条件是否为真
                let is_true = match condition_val {
                    Value::Constructor { name, .. } => name == "True",
                    Value::Number(n) => n != 0, // 数字非0为真
                    Value::Unit => false, // Unit为假
                    _ => false,
                };
                
                if !is_true {
                    break;
                }
                
                // 执行循环体
                evaluate_with_env(body, env)?;
            }
            
            // while表达式返回unit
            Ok(Value::Unit)
        }

        Expr::Match { expr, arms, .. } => {
            let value = evaluate_with_env(expr, env)?;
            
            for arm in arms {
                let mut match_env = env.clone();
                if pattern_matches(&arm.pattern, &value, &mut match_env) {
                    return evaluate_with_env(&arm.body, &match_env);
                }
            }
            
            Err("No pattern matched in match expression".to_string())
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
        Statement::TypeDef { name: _, variants, .. } => {
            // 将枚举构造器添加到环境中
            for variant in variants {
                if variant.data_type.is_some() {
                    // 带参数的构造器，创建一个函数值
                    let constructor_fn = Value::Function {
                        params: vec!["arg".to_string()],
                        body: karte_hir::Expr::Constructor {
                            name: variant.name.clone(),
                            arg: Some(Box::new(karte_hir::Expr::Identifier {
                                name: "arg".to_string(),
                                span: variant.span,
                            })),
                            span: variant.span,
                        },
                        closure: HashMap::new(),
                    };
                    env.insert(variant.name.clone(), constructor_fn);
                } else {
                    // 无参数的构造器，直接创建构造器值
                    let constructor_value = Value::Constructor {
                        name: variant.name.clone(),
                        value: None,
                    };
                    env.insert(variant.name.clone(), constructor_value);
                }
            }
            Ok(Value::Unit)
        }
    }
}

/// 检查模式是否匹配值，并更新环境
fn pattern_matches(
    pattern: &karte_hir::Pattern,
    value: &Value,
    env: &mut Environment,
) -> bool {
    match pattern {
        karte_hir::Pattern::Wildcard { .. } => true,
        
        karte_hir::Pattern::Variable { name, .. } => {
            env.insert(name.clone(), value.clone());
            true
        }
        
        karte_hir::Pattern::Number { value: pattern_value, .. } => {
            matches!(value, Value::Number(n) if n == pattern_value)
        }
        
        karte_hir::Pattern::Boolean { value: pattern_value, .. } => {
            match value {
                Value::Constructor { name, value: None } => {
                    (name == "True" && *pattern_value) || (name == "False" && !*pattern_value)
                }
                _ => false,
            }
        }
        
        karte_hir::Pattern::Constructor { name: pattern_name, arg: pattern_arg, .. } => {
            // Constructor patterns
            match value {
                Value::Constructor { name, value } => {
                    if name == pattern_name {
                        match (pattern_arg, value) {
                            (Some(arg_pattern), Some(arg_value)) => {
                                pattern_matches(arg_pattern, arg_value, env)
                            }
                            (None, None) => true,
                            _ => false,
                        }
                    } else {
                        false
                    }
                }
                _ => false,
            }
        }

        karte_hir::Pattern::QualifiedConstructor { type_name: _, constructor_name, arg: pattern_arg, .. } => {
            // 限定构造器模式，行为与普通构造器模式相同
            // 类型检查阶段已经确保了类型的正确性
            match value {
                Value::Constructor { name, value } => {
                    if name == constructor_name {
                        match (pattern_arg, value) {
                            (Some(arg_pattern), Some(arg_value)) => {
                                pattern_matches(arg_pattern, arg_value, env)
                            }
                            (None, None) => true,
                            _ => false,
                        }
                    } else {
                        false
                    }
                }
                _ => false,
            }
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
