use karte_diagnostics::{Diagnostic, DiagnosticLevel, Span};

fn test_span() -> Span {
    Span { start: 0, end: 1 }
}

#[test]
fn test_diagnostic_error() {
    let diag = Diagnostic::error("测试错误消息", test_span());
    assert_eq!(diag.message, "测试错误消息");
    assert_eq!(diag.level, DiagnosticLevel::Error);
}

#[test]
fn test_diagnostic_warning() {
    let diag = Diagnostic::warning("测试警告消息", test_span());
    assert_eq!(diag.message, "测试警告消息");
    assert_eq!(diag.level, DiagnosticLevel::Warning);
}

#[test]
fn test_diagnostic_info() {
    let diag = Diagnostic::info("测试信息消息", test_span());
    assert_eq!(diag.message, "测试信息消息");
    assert_eq!(diag.level, DiagnosticLevel::Info);
}

#[test]
fn test_diagnostic_level_display() {
    assert_eq!(format!("{}", DiagnosticLevel::Error), "错误");
    assert_eq!(format!("{}", DiagnosticLevel::Warning), "警告");
    assert_eq!(format!("{}", DiagnosticLevel::Info), "信息");
}

#[test]
fn test_diagnostic_with_source() {
    let diag = Diagnostic::error("类型不匹配", test_span())
        .with_source("fn main() -> number { \"hello\" }");
    assert_eq!(diag.message, "类型不匹配");
    assert!(diag.source.is_some());
}

#[test]
fn test_diagnostic_bag_empty() {
    let bag = karte_diagnostics::DiagnosticBag::new();
    assert!(!bag.has_errors());
    assert!(bag.diagnostics.is_empty());
}

#[test]
fn test_diagnostic_bag_with_errors() {
    let mut bag = karte_diagnostics::DiagnosticBag::new();
    bag.add(Diagnostic::error("错误1", test_span()));
    bag.add(Diagnostic::warning("警告1", test_span()));
    assert!(bag.has_errors());
    assert_eq!(bag.diagnostics.len(), 2);
}

#[test]
fn test_diagnostic_bag_only_warnings() {
    let mut bag = karte_diagnostics::DiagnosticBag::new();
    bag.add(Diagnostic::warning("警告1", test_span()));
    bag.add(Diagnostic::warning("警告2", test_span()));
    assert!(!bag.has_errors());
    assert_eq!(bag.diagnostics.len(), 2);
}
