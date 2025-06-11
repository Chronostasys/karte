use ena::unify::{NoError, UnifyKey, UnifyValue};
use std::fmt;

/// 类型变量ID，用于类型推断
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeVar(pub u32);

/// 包装类型以实现UnifyValue
#[derive(Debug, Clone)]
pub struct TypeValue(pub Option<Type>);

impl UnifyValue for TypeValue {
    type Error = NoError;

    fn unify_values(value1: &Self, value2: &Self) -> Result<Self, Self::Error> {
        match (&value1.0, &value2.0) {
            (None, None) => Ok(TypeValue(None)),
            (Some(t), None) | (None, Some(t)) => Ok(TypeValue(Some(t.clone()))),
            (Some(t1), Some(t2)) => {
                if t1.structural_eq(t2) {
                    Ok(TypeValue(Some(t1.clone())))
                } else {
                    // 不能统一，但我们使用NoError，所以这里需要处理
                    // 实际上，我们应该在类型检查器中处理这种情况
                    Ok(TypeValue(Some(t1.clone())))
                }
            }
        }
    }
}

impl UnifyKey for TypeVar {
    type Value = TypeValue;

    fn index(&self) -> u32 {
        self.0
    }

    fn from_index(u: u32) -> Self {
        TypeVar(u)
    }

    fn tag() -> &'static str {
        "TypeVar"
    }
}

/// 类型系统
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Number,
    Unit,
    Function {
        params: Vec<Type>,
        return_type: Box<Type>,
    },
    /// 类型变量，用于类型推断
    Var(TypeVar),
    /// 未知类型，用于错误恢复
    Unknown,
}

impl Type {
    /// 结构相等比较（用于统一化）
    pub fn structural_eq(&self, other: &Type) -> bool {
        match (self, other) {
            (Type::Number, Type::Number) => true,
            (Type::Unit, Type::Unit) => true,
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
                p1.len() == p2.len()
                    && p1
                        .iter()
                        .zip(p2.iter())
                        .all(|(t1, t2)| t1.structural_eq(t2))
                    && r1.structural_eq(r2)
            }
            (Type::Var(v1), Type::Var(v2)) => v1 == v2,
            (Type::Unknown, Type::Unknown) => true,
            _ => false,
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Number => write!(f, "number"),
            Type::Unit => write!(f, "()"),
            Type::Function {
                params,
                return_type,
            } => {
                write!(
                    f,
                    "fn({}) -> {}",
                    params
                        .iter()
                        .map(|t| t.to_string())
                        .collect::<Vec<_>>()
                        .join(", "),
                    return_type
                )
            }
            Type::Var(var) => write!(f, "t{}", var.0),
            Type::Unknown => write!(f, "?"),
        }
    }
}

impl Type {
    /// 检查两个类型是否兼容（旧版本兼容性）
    pub fn is_compatible_with(&self, other: &Type) -> bool {
        match (self, other) {
            (_, Type::Unknown) | (Type::Unknown, _) => true,
            (Type::Var(_), _) | (_, Type::Var(_)) => true, // 类型变量总是兼容的
            (a, b) => a == b,
        }
    }

    /// 创建函数类型
    pub fn function(params: Vec<Type>, return_type: Type) -> Self {
        Type::Function {
            params,
            return_type: Box::new(return_type),
        }
    }

    /// 替换类型中的类型变量
    pub fn substitute(&self, subst: &[(TypeVar, Type)]) -> Type {
        match self {
            Type::Var(var) => subst
                .iter()
                .find(|(v, _)| v == var)
                .map(|(_, t)| t.clone())
                .unwrap_or_else(|| self.clone()),
            Type::Function {
                params,
                return_type,
            } => Type::Function {
                params: params.iter().map(|p| p.substitute(subst)).collect(),
                return_type: Box::new(return_type.substitute(subst)),
            },
            _ => self.clone(),
        }
    }

    /// 获取类型中所有的自由类型变量
    pub fn free_vars(&self) -> Vec<TypeVar> {
        match self {
            Type::Var(var) => vec![*var],
            Type::Function {
                params,
                return_type,
            } => {
                let mut vars = Vec::new();
                for param in params {
                    vars.extend(param.free_vars());
                }
                vars.extend(return_type.free_vars());
                vars.sort_unstable_by_key(|v| v.0);
                vars.dedup();
                vars
            }
            _ => Vec::new(),
        }
    }
}
