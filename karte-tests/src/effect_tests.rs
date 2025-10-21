#[cfg(test)]
mod effect_source_pipeline_tests {
    use crate::execute_from_string;

    #[test]
    fn test_effect_pipeline_from_source() {
        // 匹配处理器：handle 1(x) { resume(x + 1) } in perform 1(10) => 11
        let src = "handle 1(x) { resume(x + 1) } in perform 1(10)";
        let result = execute_from_string(src).expect("pipeline execute failed");
        assert_eq!(result, 11);
    }

    #[test]
    fn test_effect_upward_propagation_from_source1() {
        let src = r#"
            handle 2(x) {resume(x*2)} in
            {
                handle 1(x) { resume(x + 1) } in
                    perform 2(10)
            }
        "#;
        let result = execute_from_string(src).expect("upward propagation execute failed");
        assert_eq!(result, 20);
    }

    #[test]
    fn test_effect_upward_propagation_from_source_cross_function1() {
        let src = r#"
            let f = |x| perform 2(x);
            handle 2(x) {resume(x*2)} in
            {
                f(10)
            }
        "#;
        let result = execute_from_string(src).expect("upward propagation execute failed");
        assert_eq!(result, 20);
    }
}


