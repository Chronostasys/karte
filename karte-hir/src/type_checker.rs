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
