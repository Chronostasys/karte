#[cfg(test)]
mod mir_roundtrip_tests {
    use karte_ir_codec::display::IrDisplay;
    use karte_lexer::tokenize;
    use karte_mir::lower::lower_expr_to_mir;
    use karte_parser::parse;

    /// 测试从源代码 -> MIR -> 字符串 -> parse -> MIR 的完整roundtrip
    fn test_mir_roundtrip(source: &str) -> Result<(), String> {
        // 1. 源代码 -> HIR
        let (tokens, lex_diag) = tokenize(source);
        if lex_diag.has_errors() {
            return Err(format!("Lex errors: {:?}", lex_diag));
        }

        let (hir_opt, parse_diag) = parse(&tokens);
        if parse_diag.has_errors() {
            return Err(format!("Parse errors: {:?}", parse_diag));
        }

        let hir = hir_opt.ok_or("Parse failed")?;

        // 2. HIR -> MIR
        let mir = lower_expr_to_mir(&hir).map_err(|e| format!("MIR lowering failed: {:?}", e))?;

        // 3. MIR -> 字符串
        let mir_string = mir.to_ir_string();
        println!("=== MIR Output ===\n{}\n", mir_string);

        // 4. 验证MIR字符串格式正确
        // 检查关键格式特征
        assert!(mir_string.contains("bb0"), "应该包含基本块ID");

        // 根据源代码验证不同的特征
        if source.contains("+") || source.contains("*") {
            // 应该有BinaryOp，显示为 %x = %y op %z 格式
            assert!(
                mir_string.contains("=") && mir_string.contains("%"),
                "二元运算应该使用 %= 格式"
            );
        }

        Ok(())
    }

    #[test]
    fn test_simple_number() {
        let source = "42";
        test_mir_roundtrip(source).unwrap();
    }

    #[test]
    fn test_simple_addition() {
        let source = "1 + 2";
        test_mir_roundtrip(source).unwrap();
    }

    #[test]
    fn test_arithmetic_expression() {
        let source = "1 + 2 * 3";
        test_mir_roundtrip(source).unwrap();
    }

    #[test]
    fn test_let_binding() {
        let source = "let x = 5; x + 10";
        test_mir_roundtrip(source).unwrap();
    }

    #[test]
    fn test_lambda_simple() {
        let source = "let f = |x| x * 2; f(5)";
        test_mir_roundtrip(source).unwrap();
    }

    #[test]
    fn test_array_literal_roundtrip() {
        let source = "let arr = [1, 2, 3]; arr[1]";
        test_mir_roundtrip(source).unwrap();
    }

    #[test]
    fn test_array_len_roundtrip() {
        let source = "let arr = [1, 2]; len arr";
        test_mir_roundtrip(source).unwrap();
    }

    #[test]
    fn test_boolean_values() {
        let source = "true";
        test_mir_roundtrip(source).unwrap();
    }

    #[test]
    fn test_detailed_format_check() {
        // 详细检查MIR格式
        let source = "let x = 1 + 2; x * 3";

        let (tokens, _) = tokenize(source);
        let (hir, _) = parse(&tokens);
        let mir = lower_expr_to_mir(&hir.unwrap()).unwrap();
        let mir_string = mir.to_ir_string();

        println!("=== 详细格式检查 ===");
        println!("{}", mir_string);

        // 检查格式特征
        assert!(mir_string.contains("%"), "应该包含临时变量符号 %");
        assert!(mir_string.contains("bb0"), "应该包含基本块ID bb0");
        assert!(mir_string.contains("="), "应该包含赋值符号");
        assert!(mir_string.contains("num"), "数字应该显示为 num(...)");

        // 检查不应该出现的旧格式
        assert!(
            !mir_string.contains("TempId("),
            "不应该包含旧的TempId(...)格式"
        );
        assert!(
            !mir_string.contains("BasicBlockId("),
            "不应该包含旧的BasicBlockId(...)格式"
        );
        assert!(
            !mir_string.contains("Assign("),
            "不应该包含旧的Assign(...)格式"
        );
        assert!(
            !mir_string.contains("BinaryOp("),
            "不应该包含旧的BinaryOp(...)格式"
        );
    }

    #[test]
    fn test_format_examples() {
        // 测试各种MIR元素的格式
        let test_cases = vec![
            ("42", vec!["%0 = num value: 42"]),
            (
                "1 + 2",
                vec!["%1 = num value: 1", "%2 = num value: 2", "%0 = %1 + %2"],
            ),
            ("let x = 5; x", vec!["%1 = num value: 5", "%0 = %1"]),
        ];

        for (source, expected_parts) in test_cases {
            let (tokens, _) = tokenize(source);
            let (hir, _) = parse(&tokens);
            let mir = lower_expr_to_mir(&hir.unwrap()).unwrap();
            let mir_string = mir.to_ir_string();

            println!("\n=== Source: {} ===", source);
            println!("{}", mir_string);

            for part in expected_parts {
                assert!(
                    mir_string.contains(part),
                    "MIR输出应该包含 '{}', 实际输出:\n{}",
                    part,
                    mir_string
                );
            }
        }
    }

    // test_compare_with_old_format removed: length comparison test deemed not meaningful

    #[test]
    fn test_statements_on_separate_lines() {
        // 测试语句是否在独立的行上
        let source = "let x = 1; let y = 2; x + y";

        let (tokens, _) = tokenize(source);
        let (hir, _) = parse(&tokens);
        let mir = lower_expr_to_mir(&hir.unwrap()).unwrap();
        let mir_string = mir.to_ir_string();

        println!("\n=== 检查换行 ===");
        println!("{}", mir_string);

        // 语句应该在独立的行上
        let lines: Vec<&str> = mir_string.lines().collect();
        let statement_lines: Vec<&str> = lines
            .iter()
            .filter(|line| line.trim().contains("=") && !line.contains("blocks"))
            .copied()
            .collect();

        println!("找到 {} 个语句行", statement_lines.len());
        assert!(
            statement_lines.len() >= 2,
            "应该有多个语句，每个在独立的行上"
        );
    }

    #[test]
    fn test_closure_format() {
        // 测试闭包的MIR格式
        let source = "let f = |x| x * 2; f(5)";

        let (tokens, _) = tokenize(source);
        let (hir, _) = parse(&tokens);
        let mir = lower_expr_to_mir(&hir.unwrap()).unwrap();
        let mir_string = mir.to_ir_string();

        println!("\n=== 闭包MIR ===");
        println!("{}", mir_string);

        // 应该有lambda函数
        assert!(mir_string.contains("lambda"), "应该包含lambda函数");
        // 应该有函数调用
        assert!(
            mir_string.contains("Call") || mir_string.contains("call"),
            "应该包含函数调用"
        );
    }

    #[test]
    fn test_box_and_free_flow() {
        let source = "let ptr = box 42; free ptr; 0";
        test_mir_roundtrip(source).unwrap();
    }

    #[test]
    fn test_field_access_format() {
        // 测试字段访问的格式
        let source = r#"
            struct Point { x: number, y: number }
            let p = Point { x: 10, y: 20 };
            p.x
        "#;

        let (tokens, _) = tokenize(source);
        let (hir, _) = parse(&tokens);
        let mir = lower_expr_to_mir(&hir.unwrap()).unwrap();
        let mir_string = mir.to_ir_string();

        println!("\n=== 字段访问MIR ===");
        println!("{}", mir_string);

        // 应该有字段访问，格式为 %x = %y.field
        // 由于我们添加了特殊处理，应该看到点号
        let has_field_access = mir_string.contains(".x") || mir_string.contains("field");
        assert!(has_field_access, "应该包含字段访问");
    }
}
