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
    /// 加法类型 (Sum Type / Tagged Union)
    Sum {
        name: String,
        variants: Vec<SumVariant>,
    },
    /// 结构体类型 (Product Type)
    Struct {
        name: String,
        fields: Vec<StructField>,
    },
    /// 不可变引用类型
    Reference {
        inner: Box<Type>,
    },
    /// 类型变量，用于类型推断
    Var(TypeVar),
    /// 未知类型，用于错误恢复
    Unknown,
}

/// 加法类型的变体
#[derive(Debug, Clone, PartialEq)]
pub struct SumVariant {
    pub name: String,
    pub data_type: Option<Type>, // 支持完整的类型，包括嵌套的sum type
}

impl SumVariant {
    /// 创建无数据的变体
    pub fn unit(name: String) -> Self {
        Self {
            name,
            data_type: None,
        }
    }

    /// 创建带数据的变体
    pub fn with_data(name: String, data_type: Type) -> Self {
        Self {
            name,
            data_type: Some(data_type),
        }
    }
}

/// 结构体字段类型
#[derive(Debug, Clone, PartialEq)]
pub struct StructField {
    pub name: String,
    pub field_type: Type,
}

impl StructField {
    pub fn new(name: String, field_type: Type) -> Self {
        Self { name, field_type }
    }
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
                n1 == n2
                    && v1.len() == v2.len()
                    && v1.iter().zip(v2.iter()).all(|(variant1, variant2)| {
                        variant1.name == variant2.name
                            && match (&variant1.data_type, &variant2.data_type) {
                                (None, None) => true,
                                (Some(t1), Some(t2)) => t1.structural_eq(t2),
                                _ => false,
                            }
                    })
            }
            (
                Type::Struct {
                    name: n1,
                    fields: f1,
                },
                Type::Struct {
                    name: n2,
                    fields: f2,
                },
            ) => {
                n1 == n2
                    && f1.len() == f2.len()
                    && f1.iter().zip(f2.iter()).all(|(field1, field2)| {
                        field1.name == field2.name
                            && field1.field_type.structural_eq(&field2.field_type)
                    })
            }
            (Type::Reference { inner: i1 }, Type::Reference { inner: i2 }) => i1.structural_eq(i2),
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
            Type::Sum { name, variants } => {
                let variants_str = variants
                    .iter()
                    .map(|v| {
                        if let Some(data_type) = &v.data_type {
                            format!("{}({})", v.name, data_type)
                        } else {
                            v.name.clone()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" | ");
                write!(f, "{} = {}", name, variants_str)
            }
            Type::Struct { name, fields } => {
                let fields_str = fields
                    .iter()
                    .map(|f| format!("{}: {}", f.name, f.field_type))
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "{} = {{ {} }}", name, fields_str)
            }
            Type::Reference { inner } => {
                write!(f, "&{}", inner)
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

    /// 创建加法类型
    pub fn sum(name: String, variants: Vec<SumVariant>) -> Self {
        Type::Sum { name, variants }
    }

    /// 创建结构体类型
    pub fn struct_type(name: String, fields: Vec<StructField>) -> Self {
        Type::Struct { name, fields }
    }

    /// 创建引用类型
    pub fn reference(inner: Type) -> Self {
        Type::Reference {
            inner: Box::new(inner),
        }
    }

    /// 创建布尔类型
    pub fn bool() -> Self {
        Type::Sum {
            name: "Bool".to_string(),
            variants: vec![
                SumVariant {
                    name: "True".to_string(),
                    data_type: None,
                },
                SumVariant {
                    name: "False".to_string(),
                    data_type: None,
                },
            ],
        }
    }

    /// 创建Option类型
    pub fn option(inner: Type) -> Self {
        Type::Sum {
            name: "Option".to_string(),
            variants: vec![
                SumVariant {
                    name: "Some".to_string(),
                    data_type: Some(inner),
                },
                SumVariant {
                    name: "None".to_string(),
                    data_type: None,
                },
            ],
        }
    }

    /// 替换类型中的类型变量
    pub fn substitute(&self, subst: &[(TypeVar, Type)]) -> Type {
        match self {
            Type::Number => Type::Number,
            Type::Unit => Type::Unit,
            Type::Function {
                params,
                return_type,
            } => Type::Function {
                params: params.iter().map(|p| p.substitute(subst)).collect(),
                return_type: Box::new(return_type.substitute(subst)),
            },
            Type::Sum { name, variants } => Type::Sum {
                name: name.clone(),
                variants: variants
                    .iter()
                    .map(|v| SumVariant {
                        name: v.name.clone(),
                        data_type: v.data_type.as_ref().map(|t| t.substitute(subst)),
                    })
                    .collect(),
            },
            Type::Struct { name, fields } => Type::Struct {
                name: name.clone(),
                fields: fields
                    .iter()
                    .map(|f| StructField {
                        name: f.name.clone(),
                        field_type: f.field_type.substitute(subst),
                    })
                    .collect(),
            },
            Type::Reference { inner } => Type::Reference {
                inner: Box::new(inner.substitute(subst)),
            },
            Type::Var(var) => {
                for (v, t) in subst {
                    if v == var {
                        return t.clone();
                    }
                }
                self.clone()
            }
            Type::Unknown => Type::Unknown,
        }
    }

    /// 获取类型中所有的自由类型变量
    pub fn free_vars(&self) -> Vec<TypeVar> {
        match self {
            Type::Number | Type::Unit | Type::Unknown => vec![],
            Type::Function {
                params,
                return_type,
            } => {
                let mut vars = Vec::new();
                for param in params {
                    vars.extend(param.free_vars());
                }
                vars.extend(return_type.free_vars());
                vars
            }
            Type::Sum { variants, .. } => {
                let mut vars = Vec::new();
                for variant in variants {
                    if let Some(data_type) = &variant.data_type {
                        vars.extend(data_type.free_vars());
                    }
                }
                vars
            }
            Type::Struct { fields, .. } => {
                let mut vars = Vec::new();
                for field in fields {
                    vars.extend(field.field_type.free_vars());
                }
                vars
            }
            Type::Reference { inner } => inner.free_vars(),
            Type::Var(var) => vec![*var],
        }
    }
}
