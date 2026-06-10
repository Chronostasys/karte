// 穷尽性检查模块
//
// 检查 match 表达式是否穷尽所有可能的模式。
// 如果存在未覆盖的情况，生成编译器警告。
//
// 算法基于"有用性检查"(usefulness checking)：
// 1. 对于每个 match arm，检查其模式是否有用（即是否覆盖了之前 arms 未覆盖的情况）
// 2. 在所有 arms 之后，检查是否仍有未覆盖的模式

use crate::ast::Pattern;
use crate::types::{SumVariant, Type};
use karte_diagnostics::Span;
use std::collections::HashMap;

/// 穷尽性检查的结果
#[derive(Debug, Clone)]
pub struct ExhaustivenessResult {
    /// 是否穷尽
    pub is_exhaustive: bool,
    /// 未覆盖的模式描述（用于错误信息）
    pub missing_patterns: Vec<String>,
}

/// 一个简化的"模式矩阵"列，代表一个具体值或通配符
#[derive(Debug, Clone, PartialEq)]
enum PatternCol {
    /// 通配符（_ 或变量绑定）
    Wildcard,
    /// 构造器匹配，带子模式
    Constructor {
        name: String,
        arity: usize,
        sub_patterns: Vec<PatternCol>,
    },
    /// 整数字面量
    IntLiteral(i64),
    /// 布尔字面量
    BoolLiteral(bool),
}

/// 将 AST Pattern 转换为 PatternCol
fn pattern_to_col(pat: &Pattern) -> PatternCol {
    match pat {
        Pattern::Wildcard { .. } => PatternCol::Wildcard,
        Pattern::Variable { .. } => PatternCol::Wildcard,
        Pattern::Number { value, .. } => PatternCol::IntLiteral(*value),
        Pattern::Boolean { value, .. } => PatternCol::BoolLiteral(*value),
        Pattern::Constructor { name, args, .. } => {
            let sub: Vec<PatternCol> = args.iter().map(pattern_to_col).collect();
            PatternCol::Constructor {
                name: name.clone(),
                arity: sub.len(),
                sub_patterns: sub,
            }
        }
        Pattern::QualifiedConstructor {
            constructor_name,
            args,
            ..
        } => {
            let sub: Vec<PatternCol> = args.iter().map(pattern_to_col).collect();
            PatternCol::Constructor {
                name: constructor_name.clone(),
                arity: sub.len(),
                sub_patterns: sub,
            }
        }
        Pattern::Struct { .. } => {
            // struct 模式只有一个构造器（struct 本身），总是穷尽的
            // 只要有一个 struct 模式就覆盖了该类型的所有值
            PatternCol::Wildcard
        }
    }
}

/// 从类型获取所有可能的构造器
fn get_constructors(ty: &Type, custom_types: &HashMap<String, Type>) -> Vec<ConstructorInfo> {
    match ty {
        Type::Bool => vec![
            ConstructorInfo {
                name: "true".to_string(),
                arity: 0,
                sub_types: vec![],
            },
            ConstructorInfo {
                name: "false".to_string(),
                arity: 0,
                sub_types: vec![],
            },
        ],
        Type::Sum { name, variants } => {
            // 首先尝试从 custom_types 获取完整信息
            if let Some(Type::Sum {
                variants: full_variants,
                ..
            }) = custom_types.get(name)
            {
                full_variants
                    .iter()
                    .map(|v| ConstructorInfo {
                        name: v.name.clone(),
                        arity: v.data_types.len(),
                        sub_types: v.data_types.clone(),
                    })
                    .collect()
            } else {
                variants
                    .iter()
                    .map(|v| ConstructorInfo {
                        name: v.name.clone(),
                        arity: v.data_types.len(),
                        sub_types: v.data_types.clone(),
                    })
                    .collect()
            }
        }
        Type::Number | Type::Int(_) => {
            // number 类型有无限个构造器，无法穷尽检查
            vec![]
        }
        Type::Struct { .. } => {
            // struct 只有一个构造器
            vec![ConstructorInfo {
                name: "__struct".to_string(),
                arity: 0,
                sub_types: vec![],
            }]
        }
        Type::Tuple(_) | Type::Array { .. } => {
            // 元组和数组不适用穷尽性检查
            vec![]
        }
        _ => vec![],
    }
}

/// 构造器信息
#[derive(Debug, Clone)]
struct ConstructorInfo {
    name: String,
    arity: usize,
    sub_types: Vec<Type>,
}

/// 检查模式列表是否穷尽
///
/// 使用简化的穷尽性检查算法：
/// 1. 收集 match 表达式 scrutinee 的所有可能构造器
/// 2. 对于每个构造器，检查是否有 arm 覆盖它
/// 3. 如果存在未覆盖的构造器，报告缺失的模式
pub fn check_exhaustiveness(
    patterns: &[&Pattern],
    scrutinee_type: &Type,
    custom_types: &HashMap<String, Type>,
) -> ExhaustivenessResult {
    let constructors = get_constructors(scrutinee_type, custom_types);

    // 如果无法枚举构造器（如 number 类型），跳过穷尽性检查
    if constructors.is_empty() {
        return ExhaustivenessResult {
            is_exhaustive: true,
            missing_patterns: vec![],
        };
    }

    // 如果有任何通配符模式，则一定穷尽
    let has_wildcard = patterns.iter().any(|p| matches!(
        pattern_to_col(p),
        PatternCol::Wildcard
    ));
    if has_wildcard {
        return ExhaustivenessResult {
            is_exhaustive: true,
            missing_patterns: vec![],
        };
    }

    // 收集所有已被覆盖的构造器名称
    let cols: Vec<PatternCol> = patterns.iter().map(|p| pattern_to_col(p)).collect();

    // 检查每个构造器是否被覆盖
    let mut missing = Vec::new();
    for ctor in &constructors {
        let covered = cols.iter().any(|col| match col {
            PatternCol::Wildcard => true,
            PatternCol::Constructor { name, .. } => name == &ctor.name,
            PatternCol::IntLiteral(_) | PatternCol::BoolLiteral(_) => false,
        });

        if !covered {
            missing.push(ctor.name.clone());
        }
    }

    // 对于 bool 类型，检查 true/false 是否都被覆盖
    if matches!(scrutinee_type, Type::Bool) {
        let has_true = cols.iter().any(|col| matches!(col, PatternCol::BoolLiteral(true)));
        let has_false = cols.iter().any(|col| matches!(col, PatternCol::BoolLiteral(false)));
        let has_wildcard_col = cols.iter().any(|col| matches!(col, PatternCol::Wildcard));

        if has_wildcard_col || (has_true && has_false) {
            return ExhaustivenessResult {
                is_exhaustive: true,
                missing_patterns: vec![],
            };
        }

        let mut bool_missing = Vec::new();
        if !has_true && !has_wildcard_col {
            bool_missing.push("true".to_string());
        }
        if !has_false && !has_wildcard_col {
            bool_missing.push("false".to_string());
        }

        return ExhaustivenessResult {
            is_exhaustive: bool_missing.is_empty(),
            missing_patterns: bool_missing,
        };
    }

    if missing.is_empty() {
        ExhaustivenessResult {
            is_exhaustive: true,
            missing_patterns: vec![],
        }
    } else {
        // 生成未覆盖模式的描述
        let missing_patterns: Vec<String> = missing
            .iter()
            .map(|name| {
                // 查找构造器的 arity 来生成更准确的描述
                if let Some(ctor) = constructors.iter().find(|c| &c.name == name) {
                    if ctor.arity == 0 {
                        name.clone()
                    } else {
                        let args: Vec<String> = (0..ctor.arity).map(|i| format!("_{}", i)).collect();
                        format!("{}({})", name, args.join(", "))
                    }
                } else {
                    name.clone()
                }
            })
            .collect();

        ExhaustivenessResult {
            is_exhaustive: false,
            missing_patterns,
        }
    }
}

/// 为未穷尽的 match 生成建议的错误信息
pub fn format_missing_patterns(missing: &[String]) -> String {
    if missing.len() == 1 {
        format!("非穷尽 match: 缺少模式 `{}`", missing[0])
    } else if missing.len() <= 3 {
        format!(
            "非穷尽 match: 缺少模式 {}",
            missing
                .iter()
                .map(|p| format!("`{}`", p))
                .collect::<Vec<_>>()
                .join(", ")
        )
    } else {
        format!(
            "非穷尽 match: 缺少 {} 个模式（{}, ...）",
            missing.len(),
            missing[..3]
                .iter()
                .map(|p| format!("`{}`", p))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wildcard_is_exhaustive() {
        let pat = Pattern::Wildcard {
            span: Span::dummy(),
        };
        let result = check_exhaustiveness(&[&pat], &Type::Bool, &HashMap::new());
        assert!(result.is_exhaustive);
    }

    #[test]
    fn test_bool_exhaustive() {
        let pat_true = Pattern::Boolean {
            value: true,
            span: Span::dummy(),
        };
        let pat_false = Pattern::Boolean {
            value: false,
            span: Span::dummy(),
        };
        let result = check_exhaustiveness(&[&pat_true, &pat_false], &Type::Bool, &HashMap::new());
        assert!(result.is_exhaustive);
    }

    #[test]
    fn test_bool_not_exhaustive() {
        let pat_true = Pattern::Boolean {
            value: true,
            span: Span::dummy(),
        };
        let result = check_exhaustiveness(&[&pat_true], &Type::Bool, &HashMap::new());
        assert!(!result.is_exhaustive);
        assert_eq!(result.missing_patterns, vec!["false"]);
    }

    #[test]
    fn test_enum_exhaustive() {
        let custom = HashMap::new();
        let ty = Type::sum(
            "Color".to_string(),
            vec![
                SumVariant::unit("Red".to_string()),
                SumVariant::unit("Green".to_string()),
                SumVariant::unit("Blue".to_string()),
            ],
        );

        let arms: Vec<Pattern> = vec![
            Pattern::Constructor {
                name: "Red".to_string(),
                args: vec![],
                span: Span::dummy(),
            },
            Pattern::Constructor {
                name: "Green".to_string(),
                args: vec![],
                span: Span::dummy(),
            },
            Pattern::Constructor {
                name: "Blue".to_string(),
                args: vec![],
                span: Span::dummy(),
            },
        ];

        let refs: Vec<&Pattern> = arms.iter().collect();
        let result = check_exhaustiveness(&refs, &ty, &custom);
        assert!(result.is_exhaustive);
    }

    #[test]
    fn test_enum_not_exhaustive() {
        let custom = HashMap::new();
        let ty = Type::sum(
            "Color".to_string(),
            vec![
                SumVariant::unit("Red".to_string()),
                SumVariant::unit("Green".to_string()),
                SumVariant::unit("Blue".to_string()),
            ],
        );

        let arms: Vec<Pattern> = vec![
            Pattern::Constructor {
                name: "Red".to_string(),
                args: vec![],
                span: Span::dummy(),
            },
            Pattern::Constructor {
                name: "Green".to_string(),
                args: vec![],
                span: Span::dummy(),
            },
        ];

        let refs: Vec<&Pattern> = arms.iter().collect();
        let result = check_exhaustiveness(&refs, &ty, &custom);
        assert!(!result.is_exhaustive);
        assert_eq!(result.missing_patterns, vec!["Blue"]);
    }

    #[test]
    fn test_number_always_exhaustive() {
        let pat = Pattern::Number {
            value: 42,
            span: Span::dummy(),
        };
        let result = check_exhaustiveness(&[&pat], &Type::Number, &HashMap::new());
        assert!(result.is_exhaustive);
    }
}
