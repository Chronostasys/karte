use crate::ast::{Expr, Statement};
use crate::errors::TypeCheckError;
use crate::types::{Type, TypeValue, TypeVar};
use ena::unify::InPlaceUnificationTable;
use karte_diagnostics::DiagnosticBag;
use std::collections::HashMap;

/// 类型环境 - 存储变量的类型信息
type TypeEnvironment = HashMap<String, Type>;

/// 约束条件
#[derive(Debug, Clone)]
pub struct Constraint {
    pub left: Type,
    pub right: Type,
    pub span: karte_diagnostics::Span,
}

/// 改进的类型检查器，支持类型推断
pub struct TypeChecker {
    diagnostics: DiagnosticBag,
    unification_table: InPlaceUnificationTable<TypeVar>,
    next_type_var: u32,
    constraints: Vec<Constraint>,
    custom_types: HashMap<String, Type>, // 存储自定义类型
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeChecker {
    pub fn new() -> Self {
        Self {
            diagnostics: DiagnosticBag::new(),
            unification_table: InPlaceUnificationTable::new(),
            next_type_var: 0,
            constraints: Vec::new(),
            custom_types: HashMap::new(),
        }
    }

    /// 生成新的类型变量
    fn fresh_type_var(&mut self) -> TypeVar {
        let var = TypeVar(self.next_type_var);
        self.next_type_var += 1;
        self.unification_table.new_key(TypeValue(None));
        var
    }

    /// 添加约束条件
    ///
    /// 一般来说，期望的类型在左边，实际的类型在右边
    fn add_constraint(&mut self, left: Type, right: Type, span: karte_diagnostics::Span) {
        self.constraints.push(Constraint { left, right, span });
    }

    /// 统一两个类型
    fn unify(
        &mut self,
        t1: &Type,
        t2: &Type,
        span: karte_diagnostics::Span,
        orig_t1: &Type,
        orig_t2: &Type,
    ) -> Result<(), ()> {
        match (t1, t2) {
            (Type::Number, Type::Number) => Ok(()),
            (Type::Unit, Type::Unit) => Ok(()),

            (Type::Var(v1), Type::Var(v2)) if v1 == v2 => Ok(()),

            (Type::Var(var), ty) | (ty, Type::Var(var)) => {
                // 检查是否会产生无限类型
                if ty.free_vars().contains(var) {
                    self.add_error(TypeCheckError::InfiniteType {
                        var: *var,
                        ty: ty.clone(),
                        span,
                    });
                    return Err(());
                }

                // 尝试统一
                let current_value = self.unification_table.probe_value(*var);
                match &current_value.0 {
                    None => {
                        self.unification_table
                            .union_value(*var, TypeValue(Some(ty.clone())));
                        Ok(())
                    }
                    Some(existing) => self.unify(existing, ty, span, orig_t1, orig_t2),
                }
            }

            (
                Type::Function {
                    params: p1,
                    return_type: r1,
                },
                Type::Function {
                    params: p2,
                    return_type: r2,
                },
            ) => {
                if p1.len() != p2.len() {
                    self.add_error(TypeCheckError::ArityMismatch {
                        expected: p1.len(),
                        found: p2.len(),
                        span,
                    });
                    return Err(());
                }

                // 统一参数类型
                for (param1, param2) in p1.iter().zip(p2.iter()) {
                    self.unify(param1, param2, span, orig_t1, orig_t2)?;
                }

                // 统一返回类型
                self.unify(r1, r2, span, orig_t1, orig_t2)
            }

            (
                Type::Sum {
                    name: n1,
                    variants: v1,
                },
                Type::Sum {
                    name: n2,
                    variants: v2,
                },
            ) => {
                if n1 == n2 && v1.len() == v2.len() && v1.iter().zip(v2.iter()).all(|(v1, v2)| {
                    v1.name == v2.name && match (&v1.data_type, &v2.data_type) {
                        (None, None) => true,
                        (Some(t1), Some(t2)) => t1.structural_eq(t2),
                        _ => false,
                    }
                }) {
                    Ok(())
                } else {
                    let expected = self.apply_substitution(orig_t1.clone());
                    let found = self.apply_substitution(orig_t2.clone());
                    self.add_error(TypeCheckError::TypeMismatch {
                        expected,
                        found,
                        span,
                    });
                    Err(())
                }
            }

            (Type::Unknown, _) | (_, Type::Unknown) => Ok(()),

            // 特殊处理：当尝试统一非函数类型与函数类型时
            (non_func, Type::Function { .. }) | (Type::Function { .. }, non_func) => {
                if !matches!(non_func, Type::Var(_) | Type::Unknown) {
                    // 报告类型不匹配错误，而不是"not callable"错误
                    // 因为这里的问题是类型无法统一，而不是直接的函数调用问题
                    let expected = self.apply_substitution(orig_t1.clone());
                    let found = self.apply_substitution(orig_t2.clone());
                    self.add_error(TypeCheckError::TypeMismatch {
                        expected,
                        found,
                        span,
                    });
                    return Err(());
                }
                Err(())
            }

            _ => {
                let expected = self.apply_substitution(orig_t1.clone());
                let found = self.apply_substitution(orig_t2.clone());
                self.add_error(TypeCheckError::TypeMismatch {
                    expected,
                    found,
                    span,
                });
                Err(())
            }
        }
    }

    /// 检查整个程序
    pub fn check_program(&mut self, expr: &Expr) -> Type {
        let env = TypeEnvironment::new();
        let result_type = self.infer_expr(expr, &env);

        // 解决所有约束
        self.solve_constraints();

        // 应用统一化结果
        let final_type = self.apply_substitution(result_type);

        // 如果有错误且类型仍然是变量，返回 Unknown
        if self.diagnostics.has_errors() && matches!(final_type, Type::Var(_)) {
            Type::Unknown
        } else {
            final_type
        }
    }

    /// 推断表达式的类型
    fn infer_expr(&mut self, expr: &Expr, env: &TypeEnvironment) -> Type {
        match expr {
            Expr::Number { .. } => Type::Number,

            Expr::Unit { .. } => Type::Unit,

            Expr::Identifier { name, span } => {
                if let Some(ty) = env.get(name) {
                    ty.clone()
                } else {
                    self.add_error(TypeCheckError::UndefinedVariable {
                        name: name.clone(),
                        span: *span,
                    });
                    Type::Unknown
                }
            }

            Expr::BinaryOp {
                left,
                op: _,
                right,
                span: _,
            } => {
                let left_type = self.infer_expr(left, env);
                let right_type = self.infer_expr(right, env);

                // 约束：左右操作数都必须是数字类型
                self.add_constraint(left_type, Type::Number, left.span());
                self.add_constraint(right_type, Type::Number, right.span());

                Type::Number
            }

            Expr::UnaryOp { operand, .. } => {
                let operand_type = self.infer_expr(operand, env);

                // 约束：操作数必须是数字类型
                self.add_constraint(Type::Number, operand_type, operand.span());

                Type::Number
            }

            Expr::Lambda {
                params,
                body,
                span: _,
            } => {
                let mut new_env = env.clone();
                let param_types: Vec<Type> = params
                    .iter()
                    .map(|param| {
                        let param_type = Type::Var(self.fresh_type_var());
                        new_env.insert(param.clone(), param_type.clone());
                        param_type
                    })
                    .collect();

                let return_type = self.infer_expr(body, &new_env);

                Type::function(param_types, return_type)
            }

            Expr::FunctionCall {
                function,
                args,
                span,
            } => {
                let func_type = self.infer_expr(function, env);
                let arg_types: Vec<Type> =
                    args.iter().map(|arg| self.infer_expr(arg, env)).collect();

                // 检查函数类型是否可调用
                match &func_type {
                    Type::Function {
                        params,
                        return_type,
                    } => {
                        // 检查参数数量
                        if params.len() != args.len() {
                            self.add_error(TypeCheckError::ArityMismatch {
                                expected: params.len(),
                                found: args.len(),
                                span: *span,
                            });
                            // 即使参数数量不匹配，仍然返回函数的返回类型
                            return *return_type.clone();
                        }

                        // 统一参数类型
                        for (param_type, arg_type) in params.iter().zip(arg_types.iter()) {
                            self.add_constraint(param_type.clone(), arg_type.clone(), *span);
                        }

                        *return_type.clone()
                    }
                    Type::Var(_) => {
                        // 对于类型变量，创建约束
                        let return_type = Type::Var(self.fresh_type_var());
                        let expected_func_type = Type::function(arg_types, return_type.clone());
                        self.add_constraint(func_type, expected_func_type, *span);
                        return_type
                    }
                    Type::Unknown => Type::Unknown,
                    _ => {
                        // 不可调用的类型
                        self.add_error(TypeCheckError::NotCallable {
                            found_type: func_type,
                            span: *span,
                        });
                        Type::Unknown
                    }
                }
            }

            Expr::Statement { stmt, .. } => {
                let mut env_copy = env.clone();
                self.infer_statement(stmt, &mut env_copy);
                Type::Unit
            }

            Expr::Block {
                statements,
                final_expr,
                ..
            } => {
                let mut current_env = env.clone();

                // 推断所有语句
                for stmt in statements {
                    self.infer_statement(stmt, &mut current_env);
                }

                // 推断最终表达式
                if let Some(expr) = final_expr {
                    self.infer_expr(expr, &current_env)
                } else {
                    Type::Unit
                }
            }

            Expr::Boolean { .. } => Type::bool(),

            Expr::Constructor { name, arg, span } => {
                // 首先检查是否是已知的内置构造器
                match name.as_str() {
                    "True" | "False" => Type::bool(),
                    "Some" | "None" => {
                        // 根据参数推断Option的内部类型
                        if let Some(arg_expr) = arg {
                            let arg_type = self.infer_expr(arg_expr, env);
                            Type::option(arg_type)
                        } else {
                            Type::option(Type::Var(self.fresh_type_var()))
                        }
                    }
                    _ => {
                        // 检查是否是环境中的构造器
                        if let Some(constructor_type) = env.get(name) {
                            // 如果是函数类型（有参数的构造器），需要应用参数
                            match constructor_type {
                                Type::Function { params, return_type } => {
                                    if let Some(arg_expr) = arg {
                                        if params.len() == 1 {
                                            let arg_type = self.infer_expr(arg_expr, env);
                                            self.add_constraint(arg_type, params[0].clone(), arg_expr.span());
                                            (**return_type).clone()
                                        } else {
                                            self.add_error(TypeCheckError::InvalidConstructor {
                                                name: name.clone(),
                                                span: *span,
                                            });
                                            Type::Unknown
                                        }
                                    } else {
                                        self.add_error(TypeCheckError::InvalidConstructor {
                                            name: name.clone(),
                                            span: *span,
                                        });
                                        Type::Unknown
                                    }
                                }
                                _ => {
                                    // 无参数构造器
                                    if arg.is_some() {
                                        self.add_error(TypeCheckError::InvalidConstructor {
                                            name: name.clone(),
                                            span: *span,
                                        });
                                        Type::Unknown
                                    } else {
                                        constructor_type.clone()
                                    }
                                }
                            }
                        } else {
                            // 对于未知构造器，创建一个类型变量
                            self.add_error(TypeCheckError::InvalidConstructor {
                                name: name.clone(),
                                span: *span,
                            });
                            Type::Unknown
                        }
                    }
                }
            }

            Expr::QualifiedConstructor { type_name, constructor_name, arg, span } => {
                // 检查类型是否存在
                if let Some(sum_type) = self.custom_types.get(type_name).cloned() {
                    if let Type::Sum { name: _, variants } = &sum_type {
                        // 查找对应的构造器
                        if let Some(variant) = variants.iter().find(|v| v.name == *constructor_name) {
                            if let Some(arg_expr) = arg {
                                // 有参数的构造器
                                if let Some(expected_type) = &variant.data_type {
                                    let arg_type = self.infer_expr(arg_expr, env);
                                    let expected_type_clone = expected_type.clone();
                                    self.add_constraint(arg_type, expected_type_clone, arg_expr.span());
                                    sum_type.clone()
                                } else {
                                    self.add_error(TypeCheckError::InvalidConstructor {
                                        name: format!("{}::{}", type_name, constructor_name),
                                        span: *span,
                                    });
                                    Type::Unknown
                                }
                            } else {
                                // 无参数的构造器
                                if variant.data_type.is_none() {
                                    sum_type.clone()
                                } else {
                                    self.add_error(TypeCheckError::InvalidConstructor {
                                        name: format!("{}::{}", type_name, constructor_name),
                                        span: *span,
                                    });
                                    Type::Unknown
                                }
                            }
                        } else {
                            self.add_error(TypeCheckError::InvalidConstructor {
                                name: format!("{}::{}", type_name, constructor_name),
                                span: *span,
                            });
                            Type::Unknown
                        }
                    } else {
                        self.add_error(TypeCheckError::InvalidConstructor {
                            name: format!("{}::{}", type_name, constructor_name),
                            span: *span,
                        });
                        Type::Unknown
                    }
                } else {
                    self.add_error(TypeCheckError::InvalidConstructor {
                        name: format!("{}::{}", type_name, constructor_name),
                        span: *span,
                    });
                    Type::Unknown
                }
            }

            Expr::Match { expr, arms, span } => {
                let expr_type = self.infer_expr(expr, env);
                
                if arms.is_empty() {
                    self.add_error(TypeCheckError::EmptyMatch { span: *span });
                    return Type::Unknown;
                }

                // 推断第一个分支的类型作为返回类型
                let first_arm = &arms[0];
                let mut result_env = env.clone();
                self.check_pattern(&first_arm.pattern, &expr_type, &mut result_env);
                let result_type = self.infer_expr(&first_arm.body, &result_env);

                // 检查所有其他分支的类型是否兼容
                for arm in &arms[1..] {
                    let mut arm_env = env.clone();
                    self.check_pattern(&arm.pattern, &expr_type, &mut arm_env);
                    let arm_type = self.infer_expr(&arm.body, &arm_env);
                    
                    // 约束：所有分支的类型必须兼容
                    self.add_constraint(result_type.clone(), arm_type, arm.body.span());
                }

                result_type
            }

            Expr::If { condition, then_branch, else_branch, span } => {
                // 推断条件的类型，条件必须是布尔类型
                let condition_type = self.infer_expr(condition, env);
                self.add_constraint(condition_type, Type::bool(), condition.span());

                // 推断then分支的类型
                let then_type = self.infer_expr(then_branch, env);

                // 推断else分支的类型（如果存在）
                if let Some(else_branch) = else_branch {
                    let else_type = self.infer_expr(else_branch, env);
                    // 约束：then和else分支的类型必须兼容
                    self.add_constraint(then_type.clone(), else_type, else_branch.span());
                    then_type
                } else {
                    // 如果没有else分支，if表达式返回unit类型
                    // 但不强制then分支必须是unit（允许表达式求值但丢弃结果）
                    Type::Unit
                }
            }

            Expr::While { condition, body, span: _ } => {
                // 推断条件的类型，条件必须是布尔类型
                let condition_type = self.infer_expr(condition, env);
                self.add_constraint(condition_type, Type::bool(), condition.span());

                // 推断循环体的类型（可以是任何类型，但while表达式本身返回unit）
                let _body_type = self.infer_expr(body, env);

                // while表达式总是返回unit类型
                Type::Unit
            }
        }
    }

    /// 推断语句
    fn infer_statement(&mut self, stmt: &Statement, env: &mut TypeEnvironment) {
        match stmt {
            Statement::Let { name, value, .. } => {
                let value_type = self.infer_expr(value, env);
                env.insert(name.clone(), value_type);
            }
            Statement::Expression { expr, .. } => {
                self.infer_expr(expr, env);
            }
            Statement::TypeDef { name, variants, .. } => {
                // 将变体转换为SumVariant
                let sum_variants: Vec<crate::types::SumVariant> = variants
                    .iter()
                    .map(|v| crate::types::SumVariant {
                        name: v.name.clone(),
                        data_type: v.data_type.as_ref().map(|type_name| {
                            // 简单起见，这里只支持基本类型名字映射
                            match type_name.as_str() {
                                "number" => Type::Number,
                                "unit" => Type::Unit,
                                _ => {
                                    // 检查是否是已定义的自定义类型
                                    if let Some(custom_type) = self.custom_types.get(type_name) {
                                        custom_type.clone()
                                    } else {
                                        // 未知类型，暂时用Unknown表示
                                        Type::Unknown
                                    }
                                }
                            }
                        }),
                    })
                    .collect();

                let sum_type = Type::Sum {
                    name: name.clone(),
                    variants: sum_variants,
                };

                // 将新类型添加到自定义类型环境中
                self.custom_types.insert(name.clone(), sum_type.clone());

                // 将每个构造器作为函数添加到类型环境中
                for variant in variants {
                    if let Some(data_type) = &variant.data_type {
                        // 有参数的构造器：data_type -> sum_type
                        let param_type = match data_type.as_str() {
                            "number" => Type::Number,
                            "unit" => Type::Unit,
                            _ => {
                                if let Some(custom_type) = self.custom_types.get(data_type) {
                                    custom_type.clone()
                                } else {
                                    Type::Unknown
                                }
                            }
                        };
                        
                        let constructor_type = Type::Function {
                            params: vec![param_type],
                            return_type: Box::new(sum_type.clone()),
                        };
                        env.insert(variant.name.clone(), constructor_type);
                    } else {
                        // 没有参数的构造器：直接是sum_type
                        env.insert(variant.name.clone(), sum_type.clone());
                    }
                }
            }
        }
    }

    /// 检查模式并更新环境
    fn check_pattern(&mut self, pattern: &crate::ast::Pattern, expected_type: &Type, env: &mut TypeEnvironment) {
        match pattern {
            crate::ast::Pattern::Wildcard { .. } => {
                // 通配符模式匹配任何类型，不绑定变量
            }
            crate::ast::Pattern::Variable { name, .. } => {
                // 变量模式绑定整个值
                env.insert(name.clone(), expected_type.clone());
            }
            crate::ast::Pattern::Number { value: _, span } => {
                // 数字模式必须匹配数字类型
                self.add_constraint(expected_type.clone(), Type::Number, *span);
            }
            crate::ast::Pattern::Boolean { value: _, span } => {
                // 布尔模式必须匹配布尔类型
                self.add_constraint(expected_type.clone(), Type::bool(), *span);
            }
            crate::ast::Pattern::Constructor { name, arg, span } => {
                match name.as_str() {
                    "True" | "False" => {
                        self.add_constraint(expected_type.clone(), Type::bool(), *span);
                    }
                    "Some" => {
                        if let Some(arg_pattern) = arg {
                            // Some(x) 模式，从 expected_type 中提取内部类型
                            match expected_type {
                                Type::Sum { name, variants } if name == "Option" => {
                                    // 期望是 Option<T>，找到 Some 变体的类型
                                    if let Some(some_variant) = variants.iter().find(|v| v.name == "Some") {
                                        if let Some(inner_type) = &some_variant.data_type {
                                            self.check_pattern(arg_pattern, inner_type, env);
                                        } else {
                                            self.add_error(TypeCheckError::InvalidPattern {
                                                message: "Some variant should have data type".to_string(),
                                                span: *span,
                                            });
                                        }
                                    } else {
                                        self.add_error(TypeCheckError::InvalidPattern {
                                            message: "Expected Option type with Some variant".to_string(),
                                            span: *span,
                                        });
                                    }
                                }
                                _ => {
                                    // 如果 expected_type 不是具体的 Option，创建一个约束
                                    let inner_type = Type::Var(self.fresh_type_var());
                                    let option_type = Type::option(inner_type.clone());
                                    self.add_constraint(expected_type.clone(), option_type, *span);
                                    self.check_pattern(arg_pattern, &inner_type, env);
                                }
                            }
                        } else {
                            self.add_error(TypeCheckError::InvalidPattern {
                                message: "Some constructor requires an argument".to_string(),
                                span: *span,
                            });
                        }
                    }
                    "None" => {
                        // None 模式，确保 expected_type 是 Option 类型
                        match expected_type {
                            Type::Sum { name, .. } if name == "Option" => {
                                // 已经是 Option 类型，直接匹配
                            }
                            _ => {
                                // 如果不是具体的 Option，创建约束
                                let inner_type = Type::Var(self.fresh_type_var());
                                let option_type = Type::option(inner_type);
                                self.add_constraint(expected_type.clone(), option_type, *span);
                            }
                        }
                    }
                    _ => {
                        self.add_error(TypeCheckError::InvalidPattern {
                            message: format!("Unknown constructor: {}", name),
                            span: *span,
                        });
                    }
                }
            }

            crate::ast::Pattern::QualifiedConstructor { type_name, constructor_name, arg, span } => {
                // 检查类型是否存在
                if let Some(sum_type) = self.custom_types.get(type_name).cloned() {
                    if let Type::Sum { name: _, variants } = &sum_type {
                        // 查找对应的构造器
                        if let Some(variant) = variants.iter().find(|v| v.name == *constructor_name) {
                            // 约束expected_type必须是这个sum type
                            self.add_constraint(expected_type.clone(), sum_type.clone(), *span);
                            
                            if let Some(arg_pattern) = arg {
                                // 有参数的构造器模式
                                if let Some(expected_arg_type) = &variant.data_type {
                                    self.check_pattern(arg_pattern, expected_arg_type, env);
                                } else {
                                    self.add_error(TypeCheckError::InvalidPattern {
                                        message: format!("{}::{} doesn't take arguments", type_name, constructor_name),
                                        span: *span,
                                    });
                                }
                            } else {
                                // 无参数的构造器模式
                                if variant.data_type.is_some() {
                                    self.add_error(TypeCheckError::InvalidPattern {
                                        message: format!("{}::{} requires an argument", type_name, constructor_name),
                                        span: *span,
                                    });
                                }
                            }
                        } else {
                            self.add_error(TypeCheckError::InvalidPattern {
                                message: format!("Constructor {}::{} not found", type_name, constructor_name),
                                span: *span,
                            });
                        }
                    } else {
                        self.add_error(TypeCheckError::InvalidPattern {
                            message: format!("{} is not a sum type", type_name),
                            span: *span,
                        });
                    }
                } else {
                    self.add_error(TypeCheckError::InvalidPattern {
                        message: format!("Type {} not found", type_name),
                        span: *span,
                    });
                }
            }
        }
    }

    /// 解决所有约束条件
    fn solve_constraints(&mut self) {
        for constraint in self.constraints.clone() {
            let _ = self.unify(
                &constraint.left,
                &constraint.right,
                constraint.span,
                &constraint.left,
                &constraint.right,
            );
        }
    }

    /// 应用统一化结果到类型上
    fn apply_substitution(&mut self, ty: Type) -> Type {
        match ty {
            Type::Var(var) => {
                let value = self.unification_table.probe_value(var);
                match value.0 {
                    Some(resolved_type) => self.apply_substitution(resolved_type),
                    None => Type::Var(var), // 保持未解析的类型变量
                }
            }
            Type::Function {
                params,
                return_type,
            } => Type::Function {
                params: params
                    .into_iter()
                    .map(|p| self.apply_substitution(p))
                    .collect(),
                return_type: Box::new(self.apply_substitution(*return_type)),
            },
            Type::Sum { name, variants } => Type::Sum {
                name,
                variants: variants
                    .into_iter()
                    .map(|v| crate::types::SumVariant {
                        name: v.name,
                        data_type: v.data_type.map(|t| self.apply_substitution(t)),
                    })
                    .collect(),
            },
            _ => ty,
        }
    }

    /// 添加类型检查错误
    fn add_error(&mut self, error: TypeCheckError) {
        let message = error.to_string();
        let span = error.span();
        self.diagnostics.add_error(message, span);
    }

    /// 获取诊断信息
    pub fn diagnostics(&self) -> &DiagnosticBag {
        &self.diagnostics
    }

    /// 消费并返回诊断信息
    pub fn into_diagnostics(self) -> DiagnosticBag {
        self.diagnostics
    }
}

/// 便捷函数：对表达式进行类型检查
pub fn type_check(expr: &Expr) -> (Type, DiagnosticBag) {
    let mut checker = TypeChecker::new();
    let result_type = checker.check_program(expr);
    (result_type, checker.into_diagnostics())
}
