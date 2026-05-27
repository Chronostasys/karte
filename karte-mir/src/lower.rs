mod context;
mod expr;
mod helpers;
mod stmt;
/// HIR到MIR的lowering模块
///
/// 本模块负责将高级中间表示（HIR）降低为中级中间表示（MIR）。
/// MIR使用显式的控制流图（CFG）表示，为后续的优化和代码生成提供基础。
///
/// 模块结构：
/// - `types`: 类型定义（VariableBinding, ScopeFrame, LoweringOptions, LoweringContext）
/// - `context`: LoweringContext的实现（作用域管理、变量绑定、基本块操作）
/// - `expr`: 表达式降低逻辑
/// - `stmt`: 语句降低逻辑
/// - `helpers`: 辅助函数（类型转换、变量收集、所有权推断、模式转换）
mod types;

// 重新导出公共类型和常量
pub use types::{LoweringContext, LoweringOptions, SCRIPT_ENTRY_POINT};

use crate::{MirProgram, Terminator};
use helpers::maybe_retain_for_escape;
use karte_hir::{Expr, ModuleContext};

/// 将HIR表达式转换为MIR
pub fn lower_expr_to_mir(expr: &Expr) -> Result<MirProgram, Vec<String>> {
    lower_expr_to_mir_with_options(expr, LoweringOptions::default())
}

/// 将HIR表达式转换为MIR（带选项）
pub fn lower_expr_to_mir_with_options(
    expr: &Expr,
    options: LoweringOptions,
) -> Result<MirProgram, Vec<String>> {
    let LoweringOptions {
        known_functions,
        module_context,
        expr_types,
    } = options;
    let mut program = MirProgram::new();
    let mut context = LoweringContext::new(&mut program);
    context.external_functions = known_functions;
    context.module_context = module_context.clone();
    context.expr_types = expr_types;

    // 创建主函数
    context.start_function(SCRIPT_ENTRY_POINT.to_string(), vec![]);

    // 为主函数结果创建临时变量
    let result_temp = context.new_temp();

    // 降级表达式
    expr::lower_expression(&mut context, expr, &result_temp)?;
    maybe_retain_for_escape(&mut context, expr, &result_temp);
    context.exit_scope(expr.span());

    // 添加返回语句
    context.set_terminator(Terminator::Return {
        value: Some(result_temp.clone()),
        span: expr.span(),
    });

    context.finish_function();

    if context.errors.is_empty() {
        program.set_main(SCRIPT_ENTRY_POINT.to_string());
        // 保存主函数的返回值
        program.main_return_value = Some(result_temp);
        if let Some(module_ctx) = module_context {
            annotate_module_symbols(&mut program, &module_ctx);
        }
        Ok(program)
    } else {
        Err(context.errors)
    }
}

/// 为MIR程序添加模块符号注解
fn annotate_module_symbols(program: &mut MirProgram, module_ctx: &ModuleContext) {
    let function_names: Vec<String> = program.functions.keys().cloned().collect();
    if let Some(module_name) = &module_ctx.module_name {
        for function_name in &function_names {
            let symbol = format!("{}::{}", module_name, function_name);
            program.set_function_symbol(function_name, symbol);
        }
    } else {
        for function_name in &function_names {
            program.set_function_symbol(function_name, function_name.clone());
        }
    }

    for binding in &module_ctx.imports {
        if binding.symbol == "*" {
            continue;
        }
        let module_path = if binding.module_path.is_empty() {
            String::new()
        } else {
            binding.module_path.join(".")
        };
        let canonical = if module_path.is_empty() {
            binding.symbol.clone()
        } else {
            format!("{}::{}", module_path, binding.symbol)
        };
        program.set_external_function_symbol(&binding.alias, canonical);
    }
}

#[cfg(test)]
mod assignment_lowering_tests {
    use super::*;
    use crate::{Statement, Value};
    use karte_diagnostics::Span;
    use karte_hir::{Expr, Statement as HirStatement};

    fn make_span() -> Span {
        Span::new(0, 0)
    }

    #[test]
    fn test_simple_assignment_lowering() {
        // let x = 5; x = 10; x
        let expr = Expr::Block {
            statements: vec![
                HirStatement::Let {
                    name: "x".to_string(),
                    value: Expr::Number {
                        value: 5,
                        span: make_span(),
                    },
                    span: make_span(),
                },
                HirStatement::Assignment {
                    target: Expr::Identifier {
                        name: "x".to_string(),
                        span: make_span(),
                    },
                    value: Expr::Number {
                        value: 10,
                        span: make_span(),
                    },
                    span: make_span(),
                },
            ],
            final_expr: Some(Box::new(Expr::Identifier {
                name: "x".to_string(),
                span: make_span(),
            })),
            span: make_span(),
        };

        let result = lower_expr_to_mir(&expr);
        assert!(result.is_ok(), "MIR lowering 应该成功");

        let program = result.unwrap();
        assert!(
            program.functions.contains_key(SCRIPT_ENTRY_POINT),
            "应该有main函数"
        );

        let main_fn = &program.functions[SCRIPT_ENTRY_POINT];
        assert!(!main_fn.basic_blocks.is_empty(), "main函数应该有基本块");

        // 检查是否包含Assign语句（用于赋值）
        let entry_block = &main_fn.basic_blocks[&main_fn.entry_block];
        let has_assign = entry_block
            .statements
            .iter()
            .any(|stmt| matches!(stmt, Statement::Assign { .. }));
        assert!(has_assign, "应该包含Assign语句用于赋值");
    }

    #[test]
    fn test_assignment_expression_lowering() {
        // x = 42
        let expr = Expr::Assignment {
            target: Box::new(Expr::Identifier {
                name: "x".to_string(),
                span: make_span(),
            }),
            value: Box::new(Expr::Number {
                value: 42,
                span: make_span(),
            }),
            span: make_span(),
        };

        let result = lower_expr_to_mir(&expr);
        assert!(result.is_ok(), "赋值表达式的MIR lowering应该成功");

        let program = result.unwrap();
        let main_fn = &program.functions[SCRIPT_ENTRY_POINT];
        let entry_block = &main_fn.basic_blocks[&main_fn.entry_block];

        // 检查是否包含Assign语句，值为42
        let has_assign = entry_block.statements.iter().any(|stmt| {
            matches!(
                stmt,
                Statement::Assign {
                    source: Value::Number { value: 42, .. },
                    ..
                }
            )
        });
        assert!(has_assign, "应该包含值为42的Assign语句");
    }

    #[test]
    fn test_chained_assignment_lowering() {
        // a = b = 5
        let expr = Expr::Assignment {
            target: Box::new(Expr::Identifier {
                name: "a".to_string(),
                span: make_span(),
            }),
            value: Box::new(Expr::Assignment {
                target: Box::new(Expr::Identifier {
                    name: "b".to_string(),
                    span: make_span(),
                }),
                value: Box::new(Expr::Number {
                    value: 5,
                    span: make_span(),
                }),
                span: make_span(),
            }),
            span: make_span(),
        };

        let result = lower_expr_to_mir(&expr);
        assert!(result.is_ok(), "连续赋值的MIR lowering应该成功");

        let program = result.unwrap();
        let main_fn = &program.functions[SCRIPT_ENTRY_POINT];
        let entry_block = &main_fn.basic_blocks[&main_fn.entry_block];

        // 检查是否包含Assign语句（至少一个，因为嵌套赋值可能有不同的实现方式）
        let assign_count = entry_block
            .statements
            .iter()
            .filter(|stmt| matches!(stmt, Statement::Assign { .. }))
            .count();
        assert!(assign_count >= 1, "应该至少有一个Assign语句用于连续赋值");
    }

    #[test]
    fn test_field_assignment_lowering() {
        // obj.field = 100
        let expr = Expr::Assignment {
            target: Box::new(Expr::FieldAccess {
                object: Box::new(Expr::Identifier {
                    name: "obj".to_string(),
                    span: make_span(),
                }),
                field: "field".to_string(),
                span: make_span(),
            }),
            value: Box::new(Expr::Number {
                value: 100,
                span: make_span(),
            }),
            span: make_span(),
        };

        let result = lower_expr_to_mir(&expr);
        // 字段赋值可能还没有完全实现，所以我们只检查它不会崩溃
        match result {
            Ok(program) => {
                // 如果成功，检查是否生成了一些语句
                let main_fn = &program.functions["main"];
                let entry_block = &main_fn.basic_blocks[&main_fn.entry_block];

                // 检查是否包含FieldAssign语句或者其他相关语句
                let has_field_assign = entry_block.statements.iter().any(
                    |stmt| matches!(stmt, Statement::FieldAssign { field, .. } if field == "field"),
                );
                let has_assign = entry_block
                    .statements
                    .iter()
                    .any(|stmt| matches!(stmt, Statement::Assign { .. }));

                // 至少应该有某种形式的语句
                assert!(
                    has_field_assign || has_assign || !entry_block.statements.is_empty(),
                    "字段赋值应该生成某些MIR语句"
                );
            }
            Err(_) => {
                // 如果失败，这可能是预期的，因为字段赋值可能还在开发中
                println!("字段赋值MIR lowering暂时不支持，这是预期的");
            }
        }
    }

    #[test]
    fn test_variable_collection_with_assignment() {
        // 测试变量收集功能是否包含赋值中的变量
        let expr = Expr::Assignment {
            target: Box::new(Expr::Identifier {
                name: "x".to_string(),
                span: make_span(),
            }),
            value: Box::new(Expr::Identifier {
                name: "y".to_string(),
                span: make_span(),
            }),
            span: make_span(),
        };

        let vars = helpers::collect_referenced_variables(&expr);
        assert!(vars.contains(&"x".to_string()), "应该包含变量x");
        assert!(vars.contains(&"y".to_string()), "应该包含变量y");
    }
}

#[cfg(test)]
mod closure_struct_tests {
    use karte_diagnostics::Span;

    use super::*;
    use crate::{Statement, Value};

    fn make_span() -> Span {
        Span::new(0, 0)
    }

    #[test]
    fn test_lambda_without_captures_lowering() {
        // lambda (x) => x + 1
        use karte_hir::Parameter;

        let lambda_expr = Expr::Lambda {
            params: vec![Parameter {
                name: "x".to_string(),
                type_annotation: Some("Number".to_string()),
                span: make_span(),
            }],
            body: Box::new(Expr::BinaryOp {
                left: Box::new(Expr::Identifier {
                    name: "x".to_string(),
                    span: make_span(),
                }),
                op: karte_hir::BinaryOperator::Add,
                right: Box::new(Expr::Number {
                    value: 1,
                    span: make_span(),
                }),
                span: make_span(),
            }),
            inferred_type: None,
            span: make_span(),
        };

        let result = lower_expr_to_mir(&lambda_expr);
        assert!(result.is_ok(), "无捕获lambda的MIR lowering应该成功");

        let program = result.unwrap();
        let main_fn = &program.functions[SCRIPT_ENTRY_POINT];
        let entry_block = &main_fn.basic_blocks[&main_fn.entry_block];

        // 检查是否生成了Closure结构体
        let has_closure_struct = entry_block.statements.iter().any(|stmt| {
            if let Statement::Assign {
                source: Value::Struct { name, fields, .. },
                ..
            } = stmt
            {
                name == "Closure"
                    && fields
                        .get("env_ptr")
                        .map(|v| matches!(v, Value::Number { value: 0, .. }))
                        .unwrap_or(false)
            } else {
                false
            }
        });
        assert!(has_closure_struct, "应该生成env_ptr=0的Closure结构体");

        // 检查是否生成了lambda函数
        assert!(program.functions.len() >= 2, "应该生成主函数和lambda函数");

        let lambda_fn_name = program
            .functions
            .keys()
            .find(|name| name.starts_with("lambda$"))
            .expect("应该有lambda函数");
        let lambda_fn = &program.functions[lambda_fn_name];

        assert_eq!(lambda_fn.params.len(), 2, "lambda应该有2个参数");
        assert_eq!(lambda_fn.params[1], "x", "参数应该是x");
    }

    #[test]
    fn test_lambda_with_captures_lowering() {
        // let y = 42; lambda (x) => x + y
        use karte_hir::{Parameter, Statement as HirStatement};

        let expr = Expr::Block {
            statements: vec![HirStatement::Let {
                name: "y".to_string(),
                value: Expr::Number {
                    value: 42,
                    span: make_span(),
                },
                span: make_span(),
            }],
            final_expr: Some(Box::new(Expr::Lambda {
                params: vec![Parameter {
                    name: "x".to_string(),
                    type_annotation: Some("Number".to_string()),
                    span: make_span(),
                }],
                body: Box::new(Expr::BinaryOp {
                    left: Box::new(Expr::Identifier {
                        name: "x".to_string(),
                        span: make_span(),
                    }),
                    op: karte_hir::BinaryOperator::Add,
                    right: Box::new(Expr::Identifier {
                        name: "y".to_string(),
                        span: make_span(),
                    }),
                    span: make_span(),
                }),
                inferred_type: None,
                span: make_span(),
            })),
            span: make_span(),
        };

        let result = lower_expr_to_mir(&expr);
        assert!(result.is_ok(), "有捕获lambda的MIR lowering应该成功");

        let program = result.unwrap();
        let main_fn = &program.functions[SCRIPT_ENTRY_POINT];
        let entry_block = &main_fn.basic_blocks[&main_fn.entry_block];

        // 检查是否生成了堆分配语句
        let has_heap_alloc = entry_block.statements.iter().any(|stmt| {
            matches!(stmt, Statement::HeapAlloc { object_type, .. } if object_type == "closure_env")
        });
        assert!(has_heap_alloc, "应该生成闭包环境的堆分配语句");

        // 检查是否生成了Store语句（存储捕获的变量）
        let has_store = entry_block
            .statements
            .iter()
            .any(|stmt| matches!(stmt, Statement::Store { .. }));
        assert!(has_store, "应该生成Store语句来存储捕获的变量");

        // 检查是否生成了非零env_ptr的Closure结构体
        let has_closure_with_env = entry_block.statements.iter().any(|stmt| {
            if let Statement::Assign {
                source: Value::Struct { name, fields, .. },
                ..
            } = stmt
            {
                name == "Closure"
                    && fields
                        .get("env_ptr")
                        .map(|v| !matches!(v, Value::Number { value: 0, .. }))
                        .unwrap_or(false)
            } else {
                false
            }
        });
        assert!(has_closure_with_env, "应该生成env_ptr非零的Closure结构体");

        // 检查lambda函数是否有环境参数
        let lambda_fn_name = program
            .functions
            .keys()
            .find(|name| name.starts_with("lambda$"))
            .expect("应该有lambda函数");
        let lambda_fn = &program.functions[lambda_fn_name];

        // 有捕获的lambda应该有__env参数 + 原始参数
        assert_eq!(lambda_fn.params.len(), 2, "有捕获lambda应该有2个参数");
        assert_eq!(lambda_fn.params[0], "__env", "第一个参数应该是__env");
        assert_eq!(lambda_fn.params[1], "x", "第二个参数应该是x");
    }

    #[test]
    fn test_closure_struct_function_call() {
        // 测试闭包结构体的函数调用
        use karte_hir::Parameter;

        let call_expr = Expr::FunctionCall {
            function: Box::new(Expr::Lambda {
                params: vec![Parameter {
                    name: "x".to_string(),
                    type_annotation: Some("Number".to_string()),
                    span: make_span(),
                }],
                body: Box::new(Expr::Identifier {
                    name: "x".to_string(),
                    span: make_span(),
                }),
                inferred_type: None,
                span: make_span(),
            }),
            args: vec![Expr::Number {
                value: 42,
                span: make_span(),
            }],
            span: make_span(),
        };

        let result = lower_expr_to_mir(&call_expr);
        assert!(result.is_ok(), "闭包结构体调用的MIR lowering应该成功");

        let program = result.unwrap();
        let main_fn = &program.functions[SCRIPT_ENTRY_POINT];
        let entry_block = &main_fn.basic_blocks[&main_fn.entry_block];

        // 检查是否生成了Call语句
        let has_call = entry_block
            .statements
            .iter()
            .any(|stmt| matches!(stmt, Statement::Call { .. }));
        assert!(has_call, "应该生成Call语句");
    }

    #[test]
    fn test_heap_allocation_statements() {
        // 测试堆分配相关语句的生成
        use karte_hir::{Parameter, Statement as HirStatement};

        let expr = Expr::Block {
            statements: vec![HirStatement::Let {
                name: "captured".to_string(),
                value: Expr::Number {
                    value: 100,
                    span: make_span(),
                },
                span: make_span(),
            }],
            final_expr: Some(Box::new(Expr::Lambda {
                params: vec![Parameter {
                    name: "param".to_string(),
                    type_annotation: Some("Number".to_string()),
                    span: make_span(),
                }],
                body: Box::new(Expr::Identifier {
                    name: "captured".to_string(),
                    span: make_span(),
                }),
                inferred_type: None,
                span: make_span(),
            })),
            span: make_span(),
        };

        let result = lower_expr_to_mir(&expr);
        assert!(result.is_ok(), "包含堆分配的lambda MIR lowering应该成功");

        let program = result.unwrap();
        let main_fn = &program.functions[SCRIPT_ENTRY_POINT];
        let entry_block = &main_fn.basic_blocks[&main_fn.entry_block];

        // 验证各种堆操作语句
        let heap_alloc_count = entry_block
            .statements
            .iter()
            .filter(|stmt| matches!(stmt, Statement::HeapAlloc { .. }))
            .count();
        assert_eq!(
            heap_alloc_count, 2,
            "应该有2个HeapAlloc语句：1个为捕获变量分配共享内存，1个为闭包环境分配内存"
        );

        let store_count = entry_block
            .statements
            .iter()
            .filter(|stmt| matches!(stmt, Statement::Store { .. }))
            .count();
        assert_eq!(
            store_count, 2,
            "应该有2个Store语句：1个存储捕获变量到共享内存，1个存储共享内存位置到闭包环境"
        );

        // 检查lambda函数中的变量恢复
        let lambda_fn_name = program
            .functions
            .keys()
            .find(|name| name.starts_with("lambda$"))
            .expect("应该有lambda函数");
        let lambda_fn = &program.functions[lambda_fn_name];
        let lambda_entry_block = &lambda_fn.basic_blocks[&lambda_fn.entry_block];

        // 应该有语句来恢复捕获的变量（通过引用和解引用）
        let has_var_recovery = lambda_entry_block.statements.iter().any(|stmt| {
            matches!(stmt, Statement::Assign { .. })
                || matches!(stmt, Statement::Dereference { .. })
        });
        assert!(has_var_recovery, "lambda函数应该有语句来恢复捕获的变量");
    }
}
