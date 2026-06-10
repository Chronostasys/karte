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

/// 整数类型种类
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntKind {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    USize,
}

impl IntKind {
    /// 获取整数类型的字节大小
    pub fn size_in_bytes(&self) -> usize {
        match self {
            IntKind::I8 | IntKind::U8 => 1,
            IntKind::I16 | IntKind::U16 => 2,
            IntKind::I32 | IntKind::U32 => 4,
            IntKind::I64 | IntKind::U64 | IntKind::USize => 8,
        }
    }

    /// 是否为有符号整数
    pub fn is_signed(&self) -> bool {
        matches!(self, IntKind::I8 | IntKind::I16 | IntKind::I32 | IntKind::I64)
    }

    /// 检查是否可以从 other 隐式转换到 self（无损转换）
    pub fn can_implicitly_convert_from(&self, other: &IntKind) -> bool {
        if self == other {
            return true;
        }
        // 同符号：从小到大可以隐式转换
        if self.is_signed() == other.is_signed() {
            return self.size_in_bytes() >= other.size_in_bytes();
        }
        // 无符号小类型到有符号大类型
        if self.is_signed() && !other.is_signed() {
            return self.size_in_bytes() > other.size_in_bytes();
        }
        false
    }
}

impl fmt::Display for IntKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IntKind::I8 => write!(f, "i8"),
            IntKind::I16 => write!(f, "i16"),
            IntKind::I32 => write!(f, "i32"),
            IntKind::I64 => write!(f, "i64"),
            IntKind::U8 => write!(f, "u8"),
            IntKind::U16 => write!(f, "u16"),
            IntKind::U32 => write!(f, "u32"),
            IntKind::U64 => write!(f, "u64"),
            IntKind::USize => write!(f, "usize"),
        }
    }
}

/// 类型系统
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Number,
    /// 具体整数类型
    Int(IntKind),
    /// 布尔类型（原生类型）
    Bool,
    /// 字符串类型
    String,
    Unit,
    Function {
        params: Vec<Type>,
        return_type: Box<Type>,
    },
    /// Closure 类型（与 Function 区分，用于捕获环境的匿名函数）
    Closure {
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
    /// 元组类型
    Tuple(Vec<Type>),
    /// 数组类型
    Array {
        element: Box<Type>,
    },
    /// 不可变引用类型
    Reference {
        inner: Box<Type>,
    },
    /// 类型变量，用于类型推断
    Var(TypeVar),
    /// 泛型类型引用，如 Pair<number>，在类型检查阶段被实例化为具体类型
    Generic {
        name: String,
        args: Vec<Type>,
    },
    /// 未知类型，用于错误恢复
    Unknown,
}

/// 加法类型的变体
#[derive(Debug, Clone, PartialEq)]
pub struct SumVariant {
    pub name: String,
    pub data_types: Vec<Type>,
}

impl SumVariant {
    /// 创建无数据的变体
    pub fn unit(name: String) -> Self {
        Self {
            name,
            data_types: vec![],
        }
    }

    /// 创建带数据的变体
    pub fn with_data(name: String, data_type: Type) -> Self {
        Self {
            name,
            data_types: vec![data_type],
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
            (Type::Int(k1), Type::Int(k2)) => k1 == k2,
            (Type::Bool, Type::Bool) => true,
            (Type::String, Type::String) => true,
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
                Type::Closure {
                    params: p1,
                    return_type: r1,
                },
                Type::Closure {
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
                            && match (&variant1.data_types, &variant2.data_types) {
                                (v1, v2) if v1.is_empty() && v2.is_empty() => true,
                                (v1, v2) if v1.len() == v2.len() => {
                                    v1.iter().zip(v2.iter()).all(|(t1, t2)| t1.structural_eq(t2))
                                }
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
            (Type::Array { element: e1 }, Type::Array { element: e2 }) => e1.structural_eq(e2),
            (Type::Tuple(ts1), Type::Tuple(ts2)) => {
                ts1.len() == ts2.len()
                    && ts1.iter().zip(ts2.iter()).all(|(t1, t2)| t1.structural_eq(t2))
            }
            (Type::Reference { inner: i1 }, Type::Reference { inner: i2 }) => i1.structural_eq(i2),
            (Type::Var(_), Type::Var(_)) => true, // 类型变量之间总是兼容（由统一化处理）
            (
                Type::Generic { name: n1, args: a1 },
                Type::Generic { name: n2, args: a2 },
            ) => {
                n1 == n2
                    && a1.len() == a2.len()
                    && a1.iter().zip(a2.iter()).all(|(t1, t2)| t1.structural_eq(t2))
            }
            (Type::Unknown, Type::Unknown) => true,
            _ => false,
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Number => write!(f, "number"),
            Type::Int(kind) => write!(f, "{}", kind),
            Type::Bool => write!(f, "bool"),
            Type::String => write!(f, "string"),
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
            Type::Closure {
                params,
                return_type,
            } => {
                // Closure should be displayed distinctly from plain functions
                write!(
                    f,
                    "closure(fn({}) -> {})",
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
                        if !v.data_types.is_empty() {
                            let types_str = v.data_types.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(", ");
                            format!("{}({})", v.name, types_str)
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
            Type::Tuple(types) => {
                let types_str = types
                    .iter()
                    .map(|t| t.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "({})", types_str)
            }
            Type::Array { element } => {
                write!(f, "[{}]", element)
            }
            Type::Reference { inner } => {
                write!(f, "&{}", inner)
            }
            Type::Generic { name, args } => {
                write!(f, "{}", name)?;
                if !args.is_empty() {
                    write!(f, "<")?;
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{}", arg)?;
                    }
                    write!(f, ">")?;
                }
                Ok(())
            }
            Type::Var(var) => {
                // 显示为 T, U, V, ... 而非 t0, t1, t2
                let names = "TUVWXYZABCDEFGHIJKLMNOPQRS";
                let idx = var.0 as usize;
                if idx < names.len() {
                    write!(f, "{}", names.chars().nth(idx).unwrap())
                } else {
                    write!(f, "T{}", idx)
                }
            }
            Type::Unknown => write!(f, "unknown"),
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

    /// 检查是否为数值类型（Number 或具体整数类型）
    pub fn is_numeric(&self) -> bool {
        matches!(self, Type::Number | Type::Int(_))
    }

    /// 获取整数类型的字节大小，非整数类型返回 None
    pub fn int_size_bytes(&self) -> Option<usize> {
        match self {
            Type::Number => Some(8), // Number 默认 8 字节（i64）
            Type::Int(kind) => Some(kind.size_in_bytes()),
            _ => None,
        }
    }

    /// 获取类型的字节大小（用于数组元素步幅计算）
    /// 对结构体递归计算所有字段大小之和
    pub fn byte_size(&self) -> usize {
        match self {
            Type::Number => 8,
            Type::Int(kind) => kind.size_in_bytes(),
            Type::Bool => 1,
            Type::String => 8, // 字符串是指针
            Type::Unit => 0,
            Type::Struct { fields, .. } => {
                fields.iter().map(|f| f.field_type.byte_size()).sum()
            }
            Type::Tuple(types) => {
                types.iter().map(|t| t.byte_size()).sum()
            }
            Type::Reference { .. } => 8, // 引用是指针
            Type::Array { .. } => 8, // 数组是指针
            Type::Function { .. } | Type::Closure { .. } => 8, // 函数值是指针
            Type::Sum { .. } => 8, // Tagged union 是指针
            Type::Var(_) | Type::Unknown | Type::Generic { .. } => 8, // 保守估计
        }
    }


    /// 创建函数类型
    pub fn function(params: Vec<Type>, return_type: Type) -> Self {
        Type::Function {
            params,
            return_type: Box::new(return_type),
        }
    }

    /// 创建 closure 类型（与 Function 区分）
    pub fn closure(params: Vec<Type>, return_type: Type) -> Self {
        Type::Closure {
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

    /// 创建元组类型
    pub fn tuple(types: Vec<Type>) -> Self {
        Type::Tuple(types)
    }

    /// 创建数组类型
    pub fn array(inner: Type) -> Self {
        Type::Array {
            element: Box::new(inner),
        }
    }

    /// 创建引用类型
    pub fn reference(inner: Type) -> Self {
        Type::Reference {
            inner: Box::new(inner),
        }
    }

    /// 创建布尔类型（原生 Bool）
    pub fn bool() -> Self {
        Type::Bool
    }

    /// 创建字符串类型
    pub fn string() -> Self {
        Type::String
    }

    /// 检查是否为布尔类型（兼容旧的 Type::Sum { name: "Bool" }）
    pub fn is_bool(&self) -> bool {
        match self {
            Type::Bool => true,
            Type::Sum { name, .. } => name == "Bool",
            _ => false,
        }
    }

    /// 检查两个布尔类型是否兼容（Type::Bool 与 Type::Sum { name: "Bool" } 兼容）
    pub fn is_bool_compatible(&self, other: &Type) -> bool {
        self.is_bool() && other.is_bool()
    }

    /// 创建Option类型
    pub fn option(inner: Type) -> Self {
        Type::Sum {
            name: "Option".to_string(),
            variants: vec![
                SumVariant {
                    name: "Some".to_string(),
                    data_types: vec![inner],
                },
                SumVariant {
                    name: "None".to_string(),
                    data_types: vec![],
                },
            ],
        }
    }

    /// 创建Result类型
    pub fn result(ok_type: Type, err_type: Type) -> Self {
        Type::Sum {
            name: "Result".to_string(),
            variants: vec![
                SumVariant {
                    name: "Ok".to_string(),
                    data_types: vec![ok_type],
                },
                SumVariant {
                    name: "Err".to_string(),
                    data_types: vec![err_type],
                },
            ],
        }
    }

    /// 检查是否为 Result 类型
    pub fn is_result(&self) -> bool {
        matches!(self, Type::Sum { name, .. } if name == "Result")
    }

    /// 替换类型中的类型变量（HashMap O(1) 查找）
    pub fn substitute(&self, subst: &std::collections::HashMap<TypeVar, Type>) -> Type {
        match self {
            Type::Number => Type::Number,
            Type::Int(kind) => Type::Int(*kind),
            Type::Bool => Type::Bool,
            Type::String => Type::String,
            Type::Unit => Type::Unit,
            Type::Function {
                params,
                return_type,
            } => Type::Function {
                params: params.iter().map(|p| p.substitute(subst)).collect(),
                return_type: Box::new(return_type.substitute(subst)),
            },
            Type::Closure {
                params,
                return_type,
            } => Type::Closure {
                params: params.iter().map(|p| p.substitute(subst)).collect(),
                return_type: Box::new(return_type.substitute(subst)),
            },
            Type::Sum { name, variants } => Type::Sum {
                name: name.clone(),
                variants: variants
                    .iter()
                    .map(|v| SumVariant {
                        name: v.name.clone(),
                        data_types: v.data_types.iter().map(|t| t.substitute(subst)).collect(),
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
            Type::Tuple(types) => Type::Tuple(
                types.iter().map(|t| t.substitute(subst)).collect()
            ),
            Type::Array { element } => Type::Array {
                element: Box::new(element.substitute(subst)),
            },
            Type::Reference { inner } => Type::Reference {
                inner: Box::new(inner.substitute(subst)),
            },
            Type::Generic { name, args } => Type::Generic {
                name: name.clone(),
                args: args.iter().map(|t| t.substitute(subst)).collect(),
            },
            Type::Var(var) => {
                subst.get(var).cloned().unwrap_or_else(|| self.clone())
            }
            Type::Unknown => Type::Unknown,
        }
    }

    /// 获取类型中所有的自由类型变量
    pub fn free_vars(&self) -> Vec<TypeVar> {
        // ... 见上方完整实现
        self.free_vars_inner()
    }

    /// 检查类型中是否包含特定的类型变量（短路求值，比 free_vars().contains() 更高效）
    pub fn contains_var(&self, target: &TypeVar) -> bool {
        self.contains_var_inner(target)
    }

    fn contains_var_inner(&self, target: &TypeVar) -> bool {
        match self {
            Type::Number | Type::Int(_) | Type::Bool | Type::String | Type::Unit | Type::Unknown => false,
            Type::Var(var) => var == target,
            Type::Function { params, return_type } => {
                params.iter().any(|p| p.contains_var_inner(target))
                    || return_type.contains_var_inner(target)
            }
            Type::Closure { params, return_type } => {
                params.iter().any(|p| p.contains_var_inner(target))
                    || return_type.contains_var_inner(target)
            }
            Type::Sum { variants, .. } => {
                variants.iter().any(|v| {
                    v.data_types.iter().any(|dt| dt.contains_var_inner(target))
                })
            }
            Type::Struct { fields, .. } => {
                fields.iter().any(|f| f.field_type.contains_var_inner(target))
            }
            Type::Tuple(types) => {
                types.iter().any(|t| t.contains_var_inner(target))
            }
            Type::Array { element } => element.contains_var_inner(target),
            Type::Reference { inner } => inner.contains_var_inner(target),
            Type::Generic { args, .. } => {
                args.iter().any(|a| a.contains_var_inner(target))
            }
        }
    }

    fn free_vars_inner(&self) -> Vec<TypeVar> {
        match self {
            Type::Number | Type::Int(_) | Type::Bool | Type::String | Type::Unit | Type::Unknown => vec![],
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
            Type::Closure {
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
                    for data_type in &variant.data_types {
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
            Type::Tuple(types) => {
                let mut vars = Vec::new();
                for t in types {
                    vars.extend(t.free_vars());
                }
                vars
            }
            Type::Array { element } => element.free_vars(),
            Type::Reference { inner } => inner.free_vars(),
            Type::Generic { args, .. } => {
                let mut vars = Vec::new();
                for arg in args {
                    vars.extend(arg.free_vars());
                }
                vars
            }
            Type::Var(var) => vec![*var],
        }
    }
}

/// 类型方案（Type Scheme）— 用于 let-polymorphism
/// 将函数类型中的自由类型变量量化，使其可以在不同调用位点实例化为不同类型
#[derive(Debug, Clone)]
pub struct TypeScheme {
    pub bound_vars: Vec<TypeVar>,
    pub body: Type,
}

impl TypeScheme {
    pub fn new(bound_vars: Vec<TypeVar>, body: Type) -> Self {
        Self { bound_vars, body }
    }
}
