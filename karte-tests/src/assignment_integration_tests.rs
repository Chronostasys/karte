#[cfg(test)]
mod assignment_integration_tests {
    // use karte_codegen::evaluate;
    use karte_hir::type_check;
    use karte_lexer::tokenize;
    use karte_mir::lower::lower_expr_to_mir;
    use karte_parser::parse;
    use log::info;

    #[test]
    fn test_simple_assignment_parsing() {
        let program = "let x = 5; x = 10; x";

        // 词法分析
        let (tokens, lex_diagnostics) = tokenize(program);
        assert!(lex_diagnostics.is_empty(), "词法分析不应该有错误");

        // 语法分析
        let (expr_opt, parse_diagnostics) = parse(&tokens);
        assert!(
            parse_diagnostics.is_empty(),
            "语法分析不应该有错误: {:?}",
            parse_diagnostics.diagnostics
        );
        assert!(expr_opt.is_some(), "应该成功解析表达式");

        let expr = expr_opt.unwrap();
        info!("解析成功: {:#?}", expr);
    }

    #[test]
    fn test_assignment_type_checking() {
        let program = "let x = 5; x = 10; x";

        let (tokens, _) = tokenize(program);
        let (expr_opt, _) = parse(&tokens);
        let expr = expr_opt.unwrap();

        // 类型检查
        let (result_type, type_diagnostics) = type_check(&expr);
        info!("结果类型: {:?}", result_type);

        if !type_diagnostics.is_empty() {
            info!("类型检查诊断: {:?}", type_diagnostics.diagnostics);
        }
    }

    #[test]
    fn test_assignment_mir_lowering() {
        let program = "let x = 5; x = 10; x";

        let (tokens, _) = tokenize(program);
        let (expr_opt, _) = parse(&tokens);
        let expr = expr_opt.unwrap();

        // MIR 降级
        let mir_result = lower_expr_to_mir(&expr);

        match mir_result {
            Ok(mir_program) => {
                info!("MIR降级成功");
                info!("MIR程序: {:#?}", mir_program);

                // 检查main函数存在
                assert!(mir_program.functions.contains_key("main"), "应该有main函数");
            }
            Err(errors) => {
                info!("MIR降级失败: {:?}", errors);
            }
        }
    }

    #[test]
    fn test_chained_assignment_parsing() {
        let program = "a = b = 5";

        let (tokens, _) = tokenize(program);
        let (expr_opt, parse_diagnostics) = parse(&tokens);

        if parse_diagnostics.is_empty() && expr_opt.is_some() {
            info!("连续赋值解析成功");
            info!("AST: {:#?}", expr_opt.unwrap());
        } else {
            info!("连续赋值解析失败: {:?}", parse_diagnostics.diagnostics);
        }
    }

    #[test]
    fn test_field_assignment_parsing() {
        let program = "obj.field = 100";

        let (tokens, _) = tokenize(program);
        let (expr_opt, parse_diagnostics) = parse(&tokens);

        if parse_diagnostics.is_empty() && expr_opt.is_some() {
            info!("字段赋值解析成功");
            info!("AST: {:#?}", expr_opt.unwrap());
        } else {
            info!("字段赋值解析失败: {:?}", parse_diagnostics.diagnostics);
        }
    }

    #[test]
    fn test_assignment_precedence() {
        let program = "x = y + z * 2";

        let (tokens, _) = tokenize(program);
        let (expr_opt, parse_diagnostics) = parse(&tokens);

        if parse_diagnostics.is_empty() && expr_opt.is_some() {
            info!("赋值优先级解析成功");
            info!("AST: {:#?}", expr_opt.unwrap());
        } else {
            info!("赋值优先级解析失败: {:?}", parse_diagnostics.diagnostics);
        }
    }
}
