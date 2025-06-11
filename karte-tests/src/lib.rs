pub mod codegen_tests;
pub mod integration_tests;
pub mod lexer_tests;
pub mod parser_tests;
pub mod type_checker_tests;

use karte_diagnostics::Span;

/// 测试辅助函数
pub fn dummy_span() -> Span {
    Span::new(0, 1)
}
