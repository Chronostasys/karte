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
    /// 结构体值 (Product Type Value)
    Struct {
        name: String,
        fields: HashMap<String, Value>,
    },
    /// 引用值 - 指向另一个值的不可变引用
    Reference {
        value: Box<Value>,
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
            Value::Struct { name, fields } => {
                let fields_str = fields
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k, v))
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "{} {{ {} }}", name, fields_str)
            }
            Value::Reference { value } => {
                write!(f, "&{}", value)
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
            // 处理逻辑运算符的短路求值
            match op {
                BinaryOperator::LogicalAnd => {
                    let left_val = evaluate_with_env(left, env)?;
                    let left_is_true = match left_val {
                        Value::Constructor { name, .. } => name == "True",
                        _ => return Err("Logical AND requires boolean operands".to_string()),
                    };

                    if left_is_true {
                        // 左操作数为真，计算右操作数
                        let right_val = evaluate_with_env(right, env)?;
                        match &right_val {
                            Value::Constructor { name, .. }
                                if name == "True" || name == "False" =>
                            {
                                Ok(right_val)
                            }
                            _ => Err("Logical AND requires boolean operands".to_string()),
                        }
                    } else {
                        // 左操作数为假，短路返回false
                        Ok(Value::Constructor {
                            name: "False".to_string(),
                            value: None,
                        })
                    }
                }
                BinaryOperator::LogicalOr => {
                    let left_val = evaluate_with_env(left, env)?;
                    let left_is_true = match left_val {
                        Value::Constructor { name, .. } => name == "True",
                        _ => return Err("Logical OR requires boolean operands".to_string()),
                    };

                    if left_is_true {
                        // 左操作数为真，短路返回true
                        Ok(Value::Constructor {
                            name: "True".to_string(),
                            value: None,
                        })
                    } else {
                        // 左操作数为假，计算右操作数
                        let right_val = evaluate_with_env(right, env)?;
                        match &right_val {
                            Value::Constructor { name, .. }
                                if name == "True" || name == "False" =>
                            {
                                Ok(right_val)
                            }
                            _ => Err("Logical OR requires boolean operands".to_string()),
                        }
                    }
                }
                _ => {
                    // 其他运算符需要计算两个操作数
                    let left_val = evaluate_with_env(left, env)?;
                    let right_val = evaluate_with_env(right, env)?;

                    match (left_val, right_val) {
                        (Value::Number(l), Value::Number(r)) => {
                            match op {
                                // 算术操作符返回数字
                                BinaryOperator::Add => Ok(Value::Number(l + r)),
                                BinaryOperator::Subtract => Ok(Value::Number(l - r)),
                                BinaryOperator::Multiply => Ok(Value::Number(l * r)),
                                BinaryOperator::Divide => {
                                    if r == 0 {
                                        return Err("Division by zero".to_string());
                                    }
                                    Ok(Value::Number(l / r))
                                }
                                // 比较操作符返回布尔值
                                BinaryOperator::Equal => {
                                    let result = l == r;
                                    Ok(Value::Constructor {
                                        name: if result {
                                            "True".to_string()
                                        } else {
                                            "False".to_string()
                                        },
                                        value: None,
                                    })
                                }
                                BinaryOperator::GreaterEqual => {
                                    let result = l >= r;
                                    Ok(Value::Constructor {
                                        name: if result {
                                            "True".to_string()
                                        } else {
                                            "False".to_string()
                                        },
                                        value: None,
                                    })
                                }
                                BinaryOperator::LessEqual => {
                                    let result = l <= r;
                                    Ok(Value::Constructor {
                                        name: if result {
                                            "True".to_string()
                                        } else {
                                            "False".to_string()
                                        },
                                        value: None,
                                    })
                                }
                                BinaryOperator::Greater => {
                                    let result = l > r;
                                    Ok(Value::Constructor {
                                        name: if result {
                                            "True".to_string()
                                        } else {
                                            "False".to_string()
                                        },
                                        value: None,
                                    })
                                }
                                BinaryOperator::Less => {
                                    let result = l < r;
                                    Ok(Value::Constructor {
                                        name: if result {
                                            "True".to_string()
                                        } else {
                                            "False".to_string()
                                        },
                                        value: None,
                                    })
                                }
                                BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr => {
                                    unreachable!("Logical operators should be handled above")
                                }
                            }
                        }
                        _ => Err("Binary operations are only supported on numbers".to_string()),
                    }
                }
            }
        }

        Expr::UnaryOp { op, operand, .. } => {
            let operand_val = evaluate_with_env(operand, env)?;

            match op {
                UnaryOperator::Plus | UnaryOperator::Minus => match operand_val {
                    Value::Number(n) => {
                        let result = match op {
                            UnaryOperator::Plus => n,
                            UnaryOperator::Minus => -n,
                            UnaryOperator::LogicalNot => unreachable!(),
                        };
                        Ok(Value::Number(result))
                    }
                    _ => Err("Unary +/- operations are only supported on numbers".to_string()),
                },
                UnaryOperator::LogicalNot => match operand_val {
                    Value::Constructor { name, .. } => {
                        let result = match name.as_str() {
                            "True" => "False",
                            "False" => "True",
                            _ => return Err("Logical NOT requires boolean operand".to_string()),
                        };
                        Ok(Value::Constructor {
                            name: result.to_string(),
                            value: None,
                        })
                    }
                    _ => Err("Logical NOT requires boolean operand".to_string()),
                },
            }
        }

        Expr::Lambda { params, body, .. } => {
            Ok(Value::Function {
                params: params.iter().map(|p| p.name.clone()).collect(),
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

        Expr::Assignment { target, value, .. } => {
            // 赋值表达式：对于解释器来说，赋值通常返回被赋的值或单元类型
            // 但是由于我们的环境是不可变的，这里只能检查赋值的有效性
            let _target_val = evaluate_with_env(target, env)?;
            let _value_val = evaluate_with_env(value, env)?;

            // 在实际的解释器中，这里需要修改环境，但我们当前的设计不支持
            // 暂时返回单元类型
            Ok(Value::Unit)
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
            name: if *value {
                "True".to_string()
            } else {
                "False".to_string()
            },
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

        Expr::QualifiedConstructor {
            type_name: _,
            constructor_name,
            arg,
            ..
        } => {
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

        Expr::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            let condition_val = evaluate_with_env(condition, env)?;

            // 检查条件是否为真
            let is_true = match condition_val {
                Value::Constructor { name, .. } => name == "True",
                Value::Number(n) => n != 0, // 数字非0为真
                Value::Unit => false,       // Unit为假
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

        Expr::While {
            condition, body, ..
        } => {
            loop {
                let condition_val = evaluate_with_env(condition, env)?;

                let is_true = match condition_val {
                    Value::Constructor { name, .. } => name == "True",
                    Value::Number(n) => n != 0,
                    Value::Unit => false,
                    _ => false,
                };

                if is_true {
                    evaluate_with_env(body, env)?;
                } else {
                    break;
                }
            }
            Ok(Value::Unit)
        }

        Expr::Match { expr, arms, .. } => {
            let value_to_match = evaluate_with_env(expr, env)?;
            for arm in arms {
                let mut new_env = env.clone();
                if pattern_matches(&arm.pattern, &value_to_match, &mut new_env) {
                    return evaluate_with_env(&arm.body, &new_env);
                }
            }
            Err("No pattern matched".to_string())
        }

        Expr::StructLiteral { name, fields, .. } => {
            let mut field_values = HashMap::new();
            for field_init in fields {
                let field_value = evaluate_with_env(&field_init.value, env)?;
                field_values.insert(field_init.name.clone(), field_value);
            }
            Ok(Value::Struct {
                name: name.clone(),
                fields: field_values,
            })
        }

        Expr::FieldAccess { object, field, .. } => {
            let struct_val = evaluate_with_env(object, env)?;
            match struct_val {
                Value::Struct { fields, .. } => fields
                    .get(field)
                    .cloned()
                    .ok_or_else(|| format!("Field {} not found on struct", field)),
                _ => Err(format!(
                    "Cannot access field '{}' on non-struct type",
                    field
                )),
            }
        }

        Expr::Reference { expr, .. } => {
            let value = evaluate_with_env(expr, env)?;
            Ok(Value::Reference {
                value: Box::new(value),
            })
        }

        Expr::Dereference { expr, .. } => {
            let ref_val = evaluate_with_env(expr, env)?;
            match ref_val {
                Value::Reference { value } => Ok(*value),
                _ => Err("Cannot dereference non-reference value".to_string()),
            }
        }
        // 代数效应：解释器路径暂不支持，返回错误
        Expr::EffectPerform { .. } | Expr::EffectResume { .. } | Expr::EffectHandle { .. } => {
            Err("Algebraic effects are JIT-only in this build".to_string())
        }
    }
}

fn evaluate_statement(stmt: &Statement, env: &mut Environment) -> Result<Value, String> {
    match stmt {
        Statement::Let { name, value, .. } => {
            let value = evaluate_with_env(value, env)?;
            env.insert(name.clone(), value);
            Ok(Value::Unit)
        }
        Statement::Expression { expr, .. } => {
            evaluate_with_env(expr, env)?;
            Ok(Value::Unit)
        }
        Statement::StructDef { .. } => {
            // 在求值时，结构体定义不产生任何值或行为
            // 类型检查阶段已经处理了定义
            Ok(Value::Unit)
        }
        Statement::TypeDef { .. } => {
            // 在求值时，枚举定义不产生任何值或行为
            Ok(Value::Unit)
        }
        Statement::Assignment { target, value, .. } => {
            // 赋值语句：计算右值并更新目标
            let value_result = evaluate_with_env(value, env)?;

            match target {
                Expr::Identifier { name, .. } => {
                    // 变量赋值：更新环境
                    env.insert(name.clone(), value_result);
                    Ok(Value::Unit)
                }
                Expr::FieldAccess { object, field, .. } => {
                    // 字段赋值：需要修改结构体
                    // 注意：这是一个简化实现，真实的赋值需要更复杂的逻辑
                    match object.as_ref() {
                        Expr::Identifier { name, .. } => {
                            if let Some(Value::Struct {
                                name: struct_name,
                                mut fields,
                            }) = env.get(name).cloned()
                            {
                                fields.insert(field.clone(), value_result);
                                let updated_struct = Value::Struct {
                                    name: struct_name,
                                    fields,
                                };
                                env.insert(name.clone(), updated_struct);
                                Ok(Value::Unit)
                            } else {
                                Err(format!(
                                    "Cannot assign to field '{}' of non-struct variable '{}'",
                                    field, name
                                ))
                            }
                        }
                        _ => Err("Complex field assignment not yet supported".to_string()),
                    }
                }
                _ => Err("Invalid assignment target".to_string()),
            }
        }
    }
}

fn pattern_matches(pattern: &karte_hir::Pattern, value: &Value, env: &mut Environment) -> bool {
    match pattern {
        karte_hir::Pattern::Wildcard { .. } => true,
        karte_hir::Pattern::Number { value: pat_val, .. } => {
            matches!(value, Value::Number(val) if val == pat_val)
        }
        karte_hir::Pattern::Boolean { value: pat_val, .. } => {
            matches!(value, Value::Constructor { name, .. } if (name == "True" && *pat_val) || (name == "False" && !*pat_val))
        }
        karte_hir::Pattern::Variable { name, .. } => {
            env.insert(name.clone(), value.clone());
            true
        }
        karte_hir::Pattern::Constructor { name, arg, .. } => {
            if let Value::Constructor {
                name: val_name,
                value: val_arg,
            } = value
            {
                if name == val_name {
                    return match (arg, val_arg) {
                        (Some(pat_arg), Some(val_arg)) => pattern_matches(pat_arg, val_arg, env),
                        (None, None) => true,
                        _ => false,
                    };
                }
            }
            false
        }
        karte_hir::Pattern::QualifiedConstructor {
            constructor_name,
            arg,
            ..
        } => {
            if let Value::Constructor {
                name: val_name,
                value: val_arg,
            } = value
            {
                if constructor_name == val_name {
                    return match (arg, val_arg) {
                        (Some(pat_arg), Some(val_arg)) => pattern_matches(pat_arg, val_arg, env),
                        (None, None) => true,
                        _ => false,
                    };
                }
            }
            false
        }
    }
}

pub fn value_to_number(value: &Value) -> Result<i64, String> {
    match value {
        Value::Number(n) => Ok(*n),
        _ => Err(format!("Expected number, found {}", value)),
    }
}

// 兼容旧的测试
pub fn evaluate_legacy(expr: &Expr) -> Result<i64, String> {
    evaluate(expr).and_then(|v| value_to_number(&v))
}
