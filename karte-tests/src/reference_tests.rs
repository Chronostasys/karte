#[cfg(test)]
mod reference_tests {

    use crate::execute_from_string;
    use karte_hir::type_checker::type_check;
    use karte_lexer::tokenize;
    use karte_parser::parse;

    fn compile_and_run(input: &str) -> (String, Result<i64, String>) {
        let (tokens, _) = tokenize(input);
        let (ast_opt, _) = parse(&tokens);
        let ast = ast_opt.unwrap();
        let (ty, _) = type_check(&ast);
        let result = execute_from_string(input);
        (ty.to_string(), result)
    }

    #[test]
    fn test_simple_reference() {
        let input = r#"
            let x = 42;
            let ref_x = &x;
            *ref_x
        "#;

        let (ty, result) = compile_and_run(input);
        assert_eq!(ty, "number");
        assert_eq!(result.unwrap().to_string(), "42");
    }

    #[test]
    fn test_reference_arithmetic() {
        let input = r#"
            let x = 42;
            let y = 10;
            let ref_x = &x;
            let ref_y = &y;
            *ref_x + *ref_y
        "#;

        let (ty, result) = compile_and_run(input);
        assert_eq!(ty, "number");
        assert_eq!(result.unwrap().to_string(), "52");
    }

    #[test]
    fn test_struct_with_reference_field() {
        let input = r#"
            struct RefStruct {
                data: number,
                ref_data: &number
            }
            
            let x = 42;
            let ref_x = &x;
            let s = RefStruct { data: 10, ref_data: ref_x };
            s.data + *s.ref_data
        "#;

        let (ty, result) = compile_and_run(input);
        assert_eq!(ty, "number");
        assert_eq!(result.unwrap().to_string(), "52");
    }

    #[test]
    fn test_nested_references() {
        let input =
            "let x = 42; let ref_x = &x; let ref_ref_x = &ref_x; let inner = *ref_ref_x; *inner";

        let (ty, result) = compile_and_run(input);
        assert_eq!(ty, "number");
        assert_eq!(result.unwrap().to_string(), "42");
    }

    #[test]
    fn test_reference_type_display() {
        let input = r#"
            let x = 42;
            &x
        "#;

        let (ty, _) = compile_and_run(input);
        assert_eq!(ty, "&number");
    }

    // 暂时跳过这个测试，因为lambda参数类型注解还未实现
    // #[test]
    // fn test_reference_in_function() {
    //     let input = r#"
    //         let f = |ref_x: &number| *ref_x + 1;
    //         let x = 42;
    //         f(&x)
    //     "#;
    //
    //     let (ty, result) = compile_and_run(input);
    //     assert_eq!(ty, "number");
    //     assert_eq!(result.unwrap().to_string(), "43");
    // }

    #[test]
    fn test_invalid_dereference_should_fail() {
        let input = "let x = 42; *x";

        let (tokens, _) = tokenize(input);
        let (ast_opt, _) = parse(&tokens);
        let ast = ast_opt.unwrap();
        let (_, diagnostics) = type_check(&ast);

        // 应该有类型错误
        assert!(diagnostics.has_errors());
    }

    #[test]
    fn test_reference_without_dereference_should_fail() {
        let input = "let x = 42; let ref_x = &x; ref_x + 10";

        let (tokens, _) = tokenize(input);
        let (ast_opt, _) = parse(&tokens);
        if let Some(ast) = ast_opt {
            let (_, diagnostics) = type_check(&ast);
            // 应该有类型错误
            assert!(diagnostics.has_errors());
        } else {
            panic!("解析失败");
        }
    }
}
