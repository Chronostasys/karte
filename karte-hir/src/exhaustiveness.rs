// 穷尽性检查模块
//
// 检查 match 表达式是否穷尽所有可能的模式。
// 如果存在未覆盖的情况，生成编译器错误。
// 同时检测冗余的 match arm（被之前的 arm 完全覆盖）。
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
    /// 冗余 arm 的索引（被之前的 arm 完全覆盖）
    pub redundant_arms: Vec<usize>,
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
            PatternCol::Wildcard
        }
    }
}

/// 构造器信息
#[derive(Debug, Clone)]
struct ConstructorInfo {
    name: String,
    arity: usize,
    sub_types: Vec<Type>,
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
            // 特殊处理 Option<T> 和 Result<T,E>（其构造器名是标准的）
            if name == "Option" {
                // 从 variants 中提取 Some/None
                let some_variant = variants.iter().find(|v| v.name == "Some");
                let none_variant = variants.iter().find(|v| v.name == "None");
                let mut result = Vec::new();
                if let Some(some) = some_variant {
                    result.push(ConstructorInfo {
                        name: "Some".to_string(),
                        arity: some.data_types.len(),
                        sub_types: some.data_types.clone(),
                    });
                } else {
                    // 默认 Some 构造器
                    result.push(ConstructorInfo {
                        name: "Some".to_string(),
                        arity: 1,
                        sub_types: vec![Type::Unknown],
                    });
                }
                if none_variant.is_some() || some_variant.is_none() {
                    result.push(ConstructorInfo {
                        name: "None".to_string(),
                        arity: 0,
                        sub_types: vec![],
                    });
                }
                return result;
            }
            if name == "Result" {
                let ok_variant = variants.iter().find(|v| v.name == "Ok");
                let err_variant = variants.iter().find(|v| v.name == "Err");
                let mut result = Vec::new();
                if let Some(ok) = ok_variant {
                    result.push(ConstructorInfo {
                        name: "Ok".to_string(),
                        arity: ok.data_types.len(),
                        sub_types: ok.data_types.clone(),
                    });
                } else {
                    result.push(ConstructorInfo {
                        name: "Ok".to_string(),
                        arity: 1,
                        sub_types: vec![Type::Unknown],
                    });
                }
                if let Some(err) = err_variant {
                    result.push(ConstructorInfo {
                        name: "Err".to_string(),
                        arity: err.data_types.len(),
                        sub_types: err.data_types.clone(),
                    });
                } else {
                    result.push(ConstructorInfo {
                        name: "Err".to_string(),
                        arity: 1,
                        sub_types: vec![Type::Unknown],
                    });
                }
                return result;
            }

            // 通用 Sum 类型处理
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
        Type::Number | Type::Int(_) => vec![],
        Type::Struct { .. } => {
            vec![ConstructorInfo {
                name: "__struct".to_string(),
                arity: 0,
                sub_types: vec![],
            }]
        }
        Type::Tuple(_) | Type::Array { .. } => vec![],
        _ => vec![],
    }
}

/// 检查一组模式是否已经穷尽了所有可能值
fn is_exhaustive_set(patterns: &[PatternCol], constructors: &[ConstructorInfo]) -> bool {
    // 如果有通配符，一定穷尽
    if patterns.iter().any(|p| matches!(p, PatternCol::Wildcard)) {
        return true;
    }
    
    // 对于有穷构造器的类型（如 bool、enum），检查每个构造器是否被覆盖
    if constructors.is_empty() {
        return false;
    }
    
    for ctor in constructors {
        let covered = patterns.iter().any(|p| match p {
            PatternCol::Wildcard => true,
            PatternCol::Constructor { name, .. } => name == &ctor.name,
            // bool 字面量匹配 bool 构造器
            PatternCol::BoolLiteral(b) => {
                ctor.name == "true" && *b || ctor.name == "false" && !*b
            }
            PatternCol::IntLiteral(_) => false,
        });
        if !covered {
            return false;
        }
    }
    true
}

/// 检查一个模式是否被一组之前的模式覆盖
fn is_redundant(pattern: &PatternCol, previous: &[PatternCol], constructors: &[ConstructorInfo]) -> bool {
    // 如果之前的模式已经穷尽，任何后续模式都是冗余的
    if is_exhaustive_set(previous, constructors) {
        return true;
    }
    
    // 对于构造器模式，检查是否被之前的特定构造器覆盖
    match pattern {
        PatternCol::Wildcard => {
            // 通配符只在之前已穷尽时冗余（上面已处理）
            false
        }
        PatternCol::Constructor { name, .. } => {
            // 检查之前的模式中是否有同名的构造器或通配符
            previous.iter().any(|prev| match prev {
                PatternCol::Wildcard => true,
                PatternCol::Constructor { name: prev_name, .. } => prev_name == name,
                _ => false,
            })
        }
        PatternCol::BoolLiteral(b) => {
            previous.iter().any(|prev| match prev {
                PatternCol::Wildcard => true,
                PatternCol::BoolLiteral(pb) => pb == b,
                _ => false,
            })
        }
        PatternCol::IntLiteral(n) => {
            previous.iter().any(|prev| match prev {
                PatternCol::Wildcard => true,
                PatternCol::IntLiteral(pn) => pn == n,
                _ => false,
            })
        }
    }
}

/// 检查模式列表是否穷尽
pub fn check_exhaustiveness(
    patterns: &[&Pattern],
    scrutinee_type: &Type,
    custom_types: &HashMap<String, Type>,
) -> ExhaustivenessResult {
    let constructors = get_constructors(scrutinee_type, custom_types);

    // 即使没有有限的构造器列表，也可以检测冗余 arm（如 _ 后面的模式）
    let cols: Vec<PatternCol> = patterns.iter().map(|p| pattern_to_col(p)).collect();

    // 检测冗余 arm
    let mut redundant_arms = Vec::new();
    for (i, col) in cols.iter().enumerate() {
        if i > 0 && is_redundant(col, &cols[..i], &constructors) {
            redundant_arms.push(i);
        }
    }

    if constructors.is_empty() {
        // 对于没有有限构造器的类型（如 number），只能确认穷尽性（如果有通配符）
        // 但冗余 arm 检测已经在上面完成了
        return ExhaustivenessResult {
            is_exhaustive: true,
            missing_patterns: vec![],
            redundant_arms,
        };
    }

    // 如果有任何通配符模式，则一定穷尽
    let has_wildcard = cols.iter().any(|c| matches!(c, PatternCol::Wildcard));
    if has_wildcard {
        return ExhaustivenessResult {
            is_exhaustive: true,
            missing_patterns: vec![],
            redundant_arms,
        };
    }

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

    // 对于 bool 类型，特殊处理
    if matches!(scrutinee_type, Type::Bool) {
        let has_true = cols.iter().any(|col| matches!(col, PatternCol::BoolLiteral(true)));
        let has_false = cols.iter().any(|col| matches!(col, PatternCol::BoolLiteral(false)));
        let has_wildcard_col = cols.iter().any(|col| matches!(col, PatternCol::Wildcard));

        if has_wildcard_col || (has_true && has_false) {
            return ExhaustivenessResult {
                is_exhaustive: true,
                missing_patterns: vec![],
                redundant_arms,
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
            redundant_arms,
        };
    }

    if missing.is_empty() {
        ExhaustivenessResult {
            is_exhaustive: true,
            missing_patterns: vec![],
            redundant_arms,
        }
    } else {
        let missing_patterns: Vec<String> = missing
            .iter()
            .map(|name| {
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
            redundant_arms,
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
        assert!(result.redundant_arms.is_empty());
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
        assert!(result.redundant_arms.is_empty());
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
        assert!(result.redundant_arms.is_empty());
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
    fn test_redundant_arm_detection() {
        let pat_true = Pattern::Boolean {
            value: true,
            span: Span::dummy(),
        };
        let pat_false = Pattern::Boolean {
            value: false,
            span: Span::dummy(),
        };
        let pat_wildcard = Pattern::Wildcard {
            span: Span::dummy(),
        };
        // true, false, _ -> 第三个 arm 是冗余的
        let result = check_exhaustiveness(&[&pat_true, &pat_false, &pat_wildcard], &Type::Bool, &HashMap::new());
        assert!(result.is_exhaustive);
        assert_eq!(result.redundant_arms, vec![2]);
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

    #[test]
    fn test_option_exhaustive_with_some_none() {
        let option_ty = Type::sum(
            "Option".to_string(),
            vec![
                SumVariant::with_data("Some".to_string(), Type::Number),
                SumVariant::unit("None".to_string()),
            ],
        );

        let some_pat = Pattern::Constructor {
            name: "Some".to_string(),
            args: vec![Pattern::Wildcard {
                span: Span::dummy(),
            }],
            span: Span::dummy(),
        };
        let none_pat = Pattern::Constructor {
            name: "None".to_string(),
            args: vec![],
            span: Span::dummy(),
        };

        let refs = [&some_pat, &none_pat];
        let result = check_exhaustiveness(&refs, &option_ty, &HashMap::new());
        assert!(result.is_exhaustive, "Option 的 Some + None 应该是穷尽的");
    }

    #[test]
    fn test_option_non_exhaustive_only_some() {
        let option_ty = Type::sum(
            "Option".to_string(),
            vec![
                SumVariant::with_data("Some".to_string(), Type::Number),
                SumVariant::unit("None".to_string()),
            ],
        );

        let some_pat = Pattern::Constructor {
            name: "Some".to_string(),
            args: vec![Pattern::Wildcard {
                span: Span::dummy(),
            }],
            span: Span::dummy(),
        };

        let refs = [&some_pat];
        let result = check_exhaustiveness(&refs, &option_ty, &HashMap::new());
        assert!(!result.is_exhaustive, "只有 Some 的 match 不穷尽");
        assert!(result.missing_patterns.iter().any(|p| p.contains("None")),
            "缺失的模式应该包含 None");
    }

    #[test]
    fn test_wildcard_is_always_exhaustive() {
        let wildcard = Pattern::Wildcard {
            span: Span::dummy(),
        };
        let bool_ty = Type::Bool;
        let result = check_exhaustiveness(&[&wildcard], &bool_ty, &HashMap::new());
        assert!(result.is_exhaustive, "通配符模式总是穷尽的");
    }

    #[test]
    fn test_number_wildcard_is_exhaustive() {
        let wildcard = Pattern::Wildcard { span: Span::dummy() };
        let result = check_exhaustiveness(&[&wildcard], &Type::Number, &HashMap::new());
        assert!(result.is_exhaustive, "通配符匹配 number 类型应该是穷尽的");
    }
}
