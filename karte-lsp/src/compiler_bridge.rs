// 编译器桥接
//
// 负责与 Karte 编译器集成，提供语法分析、类型检查和诊断功能
// 增强版：支持类型信息查询、符号定义位置、智能补全

use karte_diagnostics::Span;
use karte_hir::type_check;
use karte_lexer::Lexer;
use karte_parser::{Parser, ParserMode};
use tower_lsp::lsp_types::{Position, Range};
use std::collections::HashMap;

/// 符号信息
#[derive(Debug, Clone)]
pub struct SymbolInfo {
    pub name: String,
    pub kind: KarteSymbolKind,
    pub span: Span,
    pub type_signature: Option<String>,
}

/// 符号类型
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KarteSymbolKind {
    Function,
    Variable,
    Type,
    Enum,
    Struct,
    Module,
}

/// 编译分析结果
#[derive(Debug, Clone, Default)]
pub struct AnalysisResult {
    pub diagnostics: Vec<LspDiagnostic>,
    pub symbols: Vec<SymbolInfo>,
}

/// 补全项
#[derive(Debug, Clone)]
pub struct KarteCompletionItem {
    pub label: String,
    pub kind: KarteCompletionKind,
    pub detail: Option<String>,
    pub insert_text: Option<String>,
}

/// 补全项类型
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KarteCompletionKind {
    Keyword,
    Function,
    Variable,
    Class,
    Enum,
    Struct,
    Module,
}

/// LSP 诊断信息
#[derive(Debug, Clone)]
pub struct LspDiagnostic {
    pub range: Range,
    pub message: String,
    pub severity: KarteDiagnosticSeverity,
}

#[derive(Debug, Clone, Copy)]
pub enum KarteDiagnosticSeverity {
    Error,
    Warning,
    Information,
    Hint,
}

/// 编译器桥接
pub struct CompilerBridge {
    cached_result: Option<AnalysisResult>,
    cached_source: Option<String>,
}

impl CompilerBridge {
    pub fn new() -> Self {
        Self {
            cached_result: None,
            cached_source: None,
        }
    }

    /// 分析源代码并返回诊断信息
    pub fn analyze(&mut self, source: &str) -> Vec<LspDiagnostic> {
        let result = self.full_analysis(source);
        let diagnostics = result.diagnostics.clone();
        self.cached_result = Some(result);
        self.cached_source = Some(source.to_string());
        diagnostics
    }

    fn full_analysis(&self, source: &str) -> AnalysisResult {
        let mut result = AnalysisResult::default();

        let mut lexer = Lexer::new(source);
        let tokens = lexer.tokenize();
        let lex_diagnostics = lexer.into_diagnostics();

        for diag in &lex_diagnostics.diagnostics {
            result.diagnostics.push(LspDiagnostic {
                range: span_to_range(source, diag.span),
                message: diag.message.clone(),
                severity: KarteDiagnosticSeverity::Error,
            });
        }

        if lex_diagnostics.has_errors() {
            return result;
        }

        let mut parser = Parser::new(&tokens).with_mode(ParserMode::Script);
        let parsed_program = parser.parse();
        let parse_diagnostics = parser.diagnostics();

        for diag in &parse_diagnostics.diagnostics {
            result.diagnostics.push(LspDiagnostic {
                range: span_to_range(source, diag.span),
                message: diag.message.clone(),
                severity: KarteDiagnosticSeverity::Error,
            });
        }

        if parse_diagnostics.has_errors() || parsed_program.is_none() {
            return result;
        }

        let parsed_program = parsed_program.unwrap();

        let (_ty, type_diagnostics) = type_check(&parsed_program.body);

        for diag in &type_diagnostics.diagnostics {
            result.diagnostics.push(LspDiagnostic {
                range: span_to_range(source, diag.span),
                message: diag.message.clone(),
                severity: KarteDiagnosticSeverity::Error,
            });
        }

        self.collect_symbols(&parsed_program.body, &mut result);

        result
    }

    fn collect_symbols(&self, expr: &karte_hir::Expr, result: &mut AnalysisResult) {
        use karte_hir::Expr;
        match expr {
            Expr::Block { statements, final_expr, .. } => {
                for stmt in statements {
                    self.collect_symbols_from_stmt(stmt, result);
                }
                if let Some(fe) = final_expr {
                    self.collect_symbols(fe, result);
                }
            }
            _ => {}
        }
    }

    fn collect_symbols_from_stmt(&self, stmt: &karte_hir::Statement, result: &mut AnalysisResult) {
        use karte_hir::Statement;
        match stmt {
            Statement::FunctionDef { name, params, return_type, span, .. } => {
                let ret_str = return_type.as_ref().map(|t| format!(" -> {}", t)).unwrap_or_default();
                let param_strs: Vec<String> = params.iter().map(|p| {
                    p.type_annotation.as_ref().map(|t| format!("{}: {}", p.name, t)).unwrap_or_else(|| p.name.clone())
                }).collect();
                let sig = format!("fn {}({}){}", name, param_strs.join(", "), ret_str);
                result.symbols.push(SymbolInfo {
                    name: name.clone(),
                    kind: KarteSymbolKind::Function,
                    span: *span,
                    type_signature: Some(sig),
                });
            }
            Statement::Let { name, pattern, .. } => {
                let sym_name = if let Some(pat) = pattern {
                    if let karte_hir::Pattern::Variable { name: vn, .. } = pat.as_ref() {
                        vn.clone()
                    } else {
                        name.clone()
                    }
                } else {
                    name.clone()
                };
                result.symbols.push(SymbolInfo {
                    name: sym_name,
                    kind: KarteSymbolKind::Variable,
                    span: stmt.span(),
                    type_signature: None,
                });
            }
            Statement::TypeDef { name, span, .. } => {
                result.symbols.push(SymbolInfo {
                    name: name.clone(),
                    kind: KarteSymbolKind::Enum,
                    span: *span,
                    type_signature: None,
                });
            }
            Statement::StructDef { name, span, .. } => {
                result.symbols.push(SymbolInfo {
                    name: name.clone(),
                    kind: KarteSymbolKind::Struct,
                    span: *span,
                    type_signature: None,
                });
            }
            Statement::Expression { expr, .. } => {
                self.collect_symbols(expr, result);
            }
            _ => {}
        }
    }

    pub fn get_hover_info(&self, position: Position) -> Option<String> {
        let source = self.cached_source.as_ref()?;
        let result = self.cached_result.as_ref()?;
        let offset = position_to_offset(source, position);

        for sym in &result.symbols {
            if offset >= sym.span.start && offset <= sym.span.end {
                if let Some(sig) = &sym.type_signature {
                    return Some(sig.clone());
                }
                return Some(sym.name.clone());
            }
        }

        None
    }

    pub fn get_definition(&self, position: Position) -> Option<Span> {
        let source = self.cached_source.as_ref()?;
        let result = self.cached_result.as_ref()?;
        let offset = position_to_offset(source, position);

        for sym in &result.symbols {
            if offset >= sym.span.start && offset <= sym.span.end {
                return Some(sym.span);
            }
        }

        None
    }

    pub fn get_completions(&self, _position: Position) -> Vec<KarteCompletionItem> {
        let mut items = Vec::new();

        let keywords = [
            ("let", "Variable binding", "let $1 = $0;"),
            ("fn", "Function definition", "fn $1($2) -> $3 {\n    $0\n}"),
            ("if", "Conditional", "if $1 {\n    $0\n}"),
            ("else", "Else branch", "else {\n    $0\n}"),
            ("while", "While loop", "while $1 {\n    $0\n}"),
            ("match", "Pattern matching", "match $1 {\n    $2 => $0\n}"),
            ("enum", "Enum definition", "enum $1 {\n    $0\n}"),
            ("struct", "Struct definition", "struct $1 {\n    $0\n}"),
            ("return", "Return value", "return $0;"),
            ("true", "Boolean true", "true"),
            ("false", "Boolean false", "false"),
        ];

        for (kw, detail, snippet) in &keywords {
            items.push(KarteCompletionItem {
                label: kw.to_string(),
                kind: KarteCompletionKind::Keyword,
                detail: Some(detail.to_string()),
                insert_text: Some(snippet.to_string()),
            });
        }

        if let Some(result) = &self.cached_result {
            for sym in &result.symbols {
                let kind = match sym.kind {
                    KarteSymbolKind::Function => KarteCompletionKind::Function,
                    KarteSymbolKind::Variable => KarteCompletionKind::Variable,
                    KarteSymbolKind::Type => KarteCompletionKind::Class,
                    KarteSymbolKind::Enum => KarteCompletionKind::Enum,
                    KarteSymbolKind::Struct => KarteCompletionKind::Struct,
                    KarteSymbolKind::Module => KarteCompletionKind::Module,
                };
                items.push(KarteCompletionItem {
                    label: sym.name.clone(),
                    kind,
                    detail: sym.type_signature.clone(),
                    insert_text: None,
                });
            }
        }

        items
    }

    pub fn get_document_symbols(&self) -> Vec<SymbolInfo> {
        self.cached_result
            .as_ref()
            .map(|r| r.symbols.clone())
            .unwrap_or_default()
    }
}

impl Default for CompilerBridge {
    fn default() -> Self {
        Self::new()
    }
}

/// 将 Karte Span 转换为 LSP Range
pub fn span_to_range(source: &str, span: Span) -> Range {
    let start_pos = offset_to_position(source, span.start);
    let end_pos = offset_to_position(source, span.end);
    Range { start: start_pos, end: end_pos }
}

/// 将字节偏移量转换为 LSP Position
pub fn offset_to_position(source: &str, offset: usize) -> Position {
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

    Position { line: line as u32, character: col as u32 }
}

/// 将 LSP Position 转换为字节偏移量
pub fn position_to_offset(source: &str, position: Position) -> usize {
    let mut current_line = 0;
    let mut current_col = 0;
    let mut offset = 0;

    for ch in source.chars() {
        if current_line == position.line as usize && current_col >= position.character as usize {
            break;
        }
        if ch == '\n' {
            current_line += 1;
            current_col = 0;
        } else {
            current_col += 1;
        }
        offset += ch.len_utf8();
    }

    offset
}
