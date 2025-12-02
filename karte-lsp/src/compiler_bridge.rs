// 编译器桥接
//
// 负责与 Karte 编译器集成，提供语法分析、类型检查和诊断功能

use karte_diagnostics::Span;
use karte_hir::type_check;
use karte_lexer::Lexer;
use karte_parser::{Parser, ParserMode};
use tower_lsp::lsp_types::{Position, Range};

/// 编译器桥接
pub struct CompilerBridge;

impl CompilerBridge {
    pub fn new() -> Self {
        Self
    }

    /// 分析源代码并返回诊断信息
    pub fn analyze(&self, source: &str) -> Vec<LspDiagnostic> {
        let mut diagnostics = Vec::new();

        // 1. 词法分析
        let mut lexer = Lexer::new(source);
        let tokens = lexer.tokenize();
        let lex_diagnostics = lexer.into_diagnostics();

        // 收集词法错误
        for diag in &lex_diagnostics.diagnostics {
            diagnostics.push(LspDiagnostic {
                range: span_to_range(source, diag.span),
                message: diag.message.clone(),
                severity: DiagnosticSeverity::Error,
            });
        }

        // 如果有词法错误，直接返回
        if lex_diagnostics.has_errors() {
            return diagnostics;
        }

        // 2. 语法分析
        let mut parser = Parser::new(&tokens).with_mode(ParserMode::Script);
        let parsed_program = parser.parse();
        let parse_diagnostics = parser.diagnostics();

        // 收集语法错误
        for diag in &parse_diagnostics.diagnostics {
            diagnostics.push(LspDiagnostic {
                range: span_to_range(source, diag.span),
                message: diag.message.clone(),
                severity: DiagnosticSeverity::Error,
            });
        }

        // 如果有语法错误或解析失败，直接返回
        if parse_diagnostics.has_errors() || parsed_program.is_none() {
            return diagnostics;
        }

        let parsed_program = parsed_program.unwrap();

        // 3. 类型检查
        let (_ty, type_diagnostics) = type_check(&parsed_program.body);

        // 收集类型错误
        for diag in &type_diagnostics.diagnostics {
            diagnostics.push(LspDiagnostic {
                range: span_to_range(source, diag.span),
                message: diag.message.clone(),
                severity: DiagnosticSeverity::Error,
            });
        }

        diagnostics
    }
}

impl Default for CompilerBridge {
    fn default() -> Self {
        Self::new()
    }
}

/// LSP 诊断信息
#[derive(Debug, Clone)]
pub struct LspDiagnostic {
    pub range: Range,
    pub message: String,
    pub severity: DiagnosticSeverity,
}

#[derive(Debug, Clone, Copy)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Information,
    Hint,
}

/// 将 Karte Span 转换为 LSP Range
fn span_to_range(source: &str, span: Span) -> Range {
    let start_pos = offset_to_position(source, span.start);
    let end_pos = offset_to_position(source, span.end);

    Range {
        start: start_pos,
        end: end_pos,
    }
}

/// 将字节偏移量转换为 LSP Position
fn offset_to_position(source: &str, offset: usize) -> Position {
    let mut line = 0;
    let mut col = 0;
    let mut current_offset = 0;

    for ch in source.chars() {
        if current_offset >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
        current_offset += ch.len_utf8();
    }

    Position {
        line: line as u32,
        character: col as u32,
    }
}
