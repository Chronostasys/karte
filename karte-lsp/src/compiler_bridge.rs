// 编译器桥接
//
// 负责与 Karte 编译器集成，提供语法分析、类型检查和诊断功能
// 增强版：支持类型信息查询、符号定义位置、智能补全、标识符引用收集

use karte_diagnostics::Span;
use karte_hir::type_check_for_lsp;
use karte_hir::types::Type;
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
    Parameter,
    Type,
    Enum,
    Struct,
    EnumVariant,
    Module,
}

/// 编译分析结果
#[derive(Debug, Clone, Default)]
pub struct AnalysisResult {
    pub diagnostics: Vec<LspDiagnostic>,
    pub symbols: Vec<SymbolInfo>,
    /// 标识符使用位置 (start,end) -> 定义位置 (start,end)（支持 go-to-definition 和 find-references）
    pub identifier_uses: HashMap<(usize, usize), (usize, usize)>,
    /// 定义位置 (start,end) -> 名称（反向映射，用于 find-references）
    pub definition_names: HashMap<(usize, usize), String>,
    /// 标识符位置 -> 类型字符串
    pub identifier_type_strings: HashMap<(usize, usize), String>,
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
    EnumVariant,
    Module,
    Field,
    Snippet,
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

/// 名称到定义位置的简单环境（用于标识符引用解析）
/// 使用 (start, end) 作为位置标识符
#[derive(Debug, Clone, Default)]
struct NameEnv {
    /// 各作用域层：index 0 是最内层
    scopes: Vec<HashMap<String, (usize, usize)>>,
}

impl NameEnv {
    fn new() -> Self {
        Self { scopes: vec![HashMap::new()] }
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    fn insert(&mut self, name: &str, pos: (usize, usize)) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string(), pos);
        }
    }

    fn lookup(&self, name: &str) -> Option<(usize, usize)> {
        for scope in self.scopes.iter().rev() {
            if let Some(pos) = scope.get(name) {
                return Some(*pos);
            }
        }
        None
    }
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

        // 1. 词法分析
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

        // 2. 语法分析
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

        // 3. 使用 LSP 专用类型检查，获取完整的类型信息
        let type_info = type_check_for_lsp(&parsed_program.body);

        for diag in &type_info.diagnostics.diagnostics {
            let severity = if diag.message.contains("warning") || diag.message.contains("redundant") {
                KarteDiagnosticSeverity::Warning
            } else {
                KarteDiagnosticSeverity::Error
            };
            result.diagnostics.push(LspDiagnostic {
                range: span_to_range(source, diag.span),
                message: diag.message.clone(),
                severity,
            });
        }

        // 4. 收集符号信息（第一遍：收集所有定义）
        let mut env = NameEnv::new();
        self.collect_symbols_and_defs(&parsed_program.body, &mut result, &mut env);

        // 5. 收集标识符引用关系（第二遍：解析所有使用→定义）
        self.collect_identifier_references(&parsed_program.body, &env, &mut result);

        // 6. 收集标识符类型信息（用于悬停）
        self.collect_identifier_types(&parsed_program.body, &type_info.expr_types, &mut result);

        result
    }

    /// 收集符号定义信息，同时构建名称环境
    fn collect_symbols_and_defs(
        &self,
        expr: &karte_hir::Expr,
        result: &mut AnalysisResult,
        env: &mut NameEnv,
    ) {
        use karte_hir::Expr;
        match expr {
            Expr::Block { statements, final_expr, .. } => {
                for stmt in statements {
                    self.collect_symbols_from_stmt(stmt, result, env);
                }
                if let Some(fe) = final_expr {
                    self.collect_symbols_and_defs(fe, result, env);
                }
            }
            _ => {}
        }
    }

    fn collect_symbols_from_stmt(
        &self,
        stmt: &karte_hir::Statement,
        result: &mut AnalysisResult,
        env: &mut NameEnv,
    ) {
        use karte_hir::Statement;
        match stmt {
            Statement::FunctionDef { name, params, return_type, span, body, .. } => {
                let ret_str = return_type.as_ref().map(|t| format!(" -> {}", t)).unwrap_or_default();
                let param_strs: Vec<String> = params.iter().map(|p| {
                    p.type_annotation.as_ref().map(|t| format!("{}: {}", p.name, t)).unwrap_or_else(|| p.name.clone())
                }).collect();
                let sig = format!("fn {}({}){}", name, param_strs.join(", "), ret_str);

                let def_pos = (span.start, span.end);
                result.symbols.push(SymbolInfo {
                    name: name.clone(),
                    kind: KarteSymbolKind::Function,
                    span: *span,
                    type_signature: Some(sig),
                });
                env.insert(name, def_pos);
                result.definition_names.insert(def_pos, name.clone());

                // 函数参数
                env.push_scope();
                for p in params {
                    let p_pos = (p.span.start, p.span.end);
                    env.insert(&p.name, p_pos);
                    result.symbols.push(SymbolInfo {
                        name: p.name.clone(),
                        kind: KarteSymbolKind::Parameter,
                        span: p.span,
                        type_signature: p.type_annotation.as_ref().map(|t| format!("{}: {}", p.name, t)),
                    });
                    result.definition_names.insert(p_pos, p.name.clone());
                }
                self.collect_symbols_and_defs(body, result, env);
                env.pop_scope();
            }
            Statement::Let { name, pattern, span, .. } => {
                let sym_name = if let Some(pat) = pattern {
                    if let karte_hir::Pattern::Variable { name: vn, .. } = pat.as_ref() {
                        vn.clone()
                    } else {
                        name.clone()
                    }
                } else {
                    name.clone()
                };
                if let Some(pat) = pattern {
                    self.collect_pattern_bindings(pat, result, env);
                }
                let def_pos = (span.start, span.end);
                env.insert(&sym_name, def_pos);
                result.definition_names.insert(def_pos, sym_name.clone());
                result.symbols.push(SymbolInfo {
                    name: sym_name,
                    kind: KarteSymbolKind::Variable,
                    span: *span,
                    type_signature: None,
                });
            }
            Statement::TypeDef { name, variants, span, .. } => {
                let def_pos = (span.start, span.end);
                result.symbols.push(SymbolInfo {
                    name: name.clone(),
                    kind: KarteSymbolKind::Enum,
                    span: *span,
                    type_signature: None,
                });
                env.insert(name, def_pos);
                result.definition_names.insert(def_pos, name.clone());

                for variant in variants {
                    let variant_name = &variant.name;
                    result.symbols.push(SymbolInfo {
                        name: format!("{}::{}", name, variant_name),
                        kind: KarteSymbolKind::EnumVariant,
                        span: *span,
                        type_signature: None,
                    });
                    env.insert(variant_name, def_pos);
                }
            }
            Statement::StructDef { name, fields, span, .. } => {
                let field_strs: Vec<String> = fields.iter()
                    .map(|f| format!("{}: {}", f.name, f.field_type))
                    .collect();
                let def_pos = (span.start, span.end);
                result.symbols.push(SymbolInfo {
                    name: name.clone(),
                    kind: KarteSymbolKind::Struct,
                    span: *span,
                    type_signature: Some(format!("struct {{ {} }}", field_strs.join(", "))),
                });
                env.insert(name, def_pos);
                result.definition_names.insert(def_pos, name.clone());
            }
            Statement::Expression { expr, .. } => {
                self.collect_symbols_and_defs(expr, result, env);
            }
            _ => {}
        }
    }

    /// 从模式中提取变量绑定
    fn collect_pattern_bindings(
        &self,
        pattern: &karte_hir::Pattern,
        result: &mut AnalysisResult,
        env: &mut NameEnv,
    ) {
        use karte_hir::Pattern;
        match pattern {
            Pattern::Variable { name, span } => {
                let pos = (span.start, span.end);
                env.insert(name, pos);
                result.definition_names.insert(pos, name.clone());
            }
            Pattern::Constructor { args, .. } => {
                for arg in args {
                    self.collect_pattern_bindings(arg, result, env);
                }
            }
            Pattern::QualifiedConstructor { args, .. } => {
                for arg in args {
                    self.collect_pattern_bindings(arg, result, env);
                }
            }
            Pattern::Struct { fields, .. } => {
                for field in fields {
                    self.collect_pattern_bindings(&field.pattern, result, env);
                }
            }
            _ => {}
        }
    }

    /// 收集标识符引用关系（使用位置 → 定义位置）
    fn collect_identifier_references(
        &self,
        expr: &karte_hir::Expr,
        env: &NameEnv,
        result: &mut AnalysisResult,
    ) {
        use karte_hir::Expr;
        match expr {
            Expr::Identifier { name, span } => {
                if let Some(def_pos) = env.lookup(name) {
                    result.identifier_uses.insert((span.start, span.end), def_pos);
                }
            }
            Expr::Block { statements, final_expr, .. } => {
                for stmt in statements {
                    self.collect_stmt_refs(stmt, env, result);
                }
                if let Some(fe) = final_expr {
                    self.collect_identifier_references(fe, env, result);
                }
            }
            Expr::BinaryOp { left, right, .. } => {
                self.collect_identifier_references(left, env, result);
                self.collect_identifier_references(right, env, result);
            }
            Expr::UnaryOp { operand, .. } => {
                self.collect_identifier_references(operand, env, result);
            }
            Expr::FunctionCall { function, args, .. } => {
                self.collect_identifier_references(function, env, result);
                for arg in args {
                    self.collect_identifier_references(arg, env, result);
                }
            }
            Expr::If { condition, then_branch, else_branch, .. } => {
                self.collect_identifier_references(condition, env, result);
                self.collect_identifier_references(then_branch, env, result);
                if let Some(eb) = else_branch {
                    self.collect_identifier_references(eb, env, result);
                }
            }
            Expr::While { condition, body, .. } => {
                self.collect_identifier_references(condition, env, result);
                self.collect_identifier_references(body, env, result);
            }
            Expr::Match { expr: scrutinee, arms, .. } => {
                self.collect_identifier_references(scrutinee, env, result);
                for arm in arms {
                    self.collect_identifier_references(&arm.body, env, result);
                }
            }
            Expr::Lambda { body, .. } => {
                self.collect_identifier_references(body, env, result);
            }
            Expr::Assignment { target, value, .. } => {
                self.collect_identifier_references(target, env, result);
                self.collect_identifier_references(value, env, result);
            }
            Expr::ArrayLiteral { elements, .. } => {
                for elem in elements {
                    self.collect_identifier_references(elem, env, result);
                }
            }
            Expr::Index { array, index, .. } => {
                self.collect_identifier_references(array, env, result);
                self.collect_identifier_references(index, env, result);
            }
            Expr::StructLiteral { fields, .. } => {
                for field in fields {
                    self.collect_identifier_references(&field.value, env, result);
                }
            }
            Expr::FieldAccess { object, .. } => {
                self.collect_identifier_references(object, env, result);
            }
            Expr::TupleLiteral { elements, .. } => {
                for elem in elements {
                    self.collect_identifier_references(elem, env, result);
                }
            }
            Expr::TupleAccess { object, .. } => {
                self.collect_identifier_references(object, env, result);
            }
            Expr::Constructor { args, .. } => {
                for arg in args {
                    self.collect_identifier_references(arg, env, result);
                }
            }
            Expr::QualifiedConstructor { args, .. } => {
                for arg in args {
                    self.collect_identifier_references(arg, env, result);
                }
            }
            Expr::Reference { expr: inner, .. } => {
                self.collect_identifier_references(inner, env, result);
            }
            Expr::Dereference { expr: inner, .. } => {
                self.collect_identifier_references(inner, env, result);
            }
            Expr::ModuleSymbolAccess { .. } => {}
            Expr::Return { value, .. } => {
                if let Some(v) = value {
                    self.collect_identifier_references(v, env, result);
                }
            }
            Expr::ForIn { start, end, body, .. } => {
                self.collect_identifier_references(start, env, result);
                self.collect_identifier_references(end, env, result);
                self.collect_identifier_references(body, env, result);
            }
            Expr::ForArray { array, body, .. } => {
                self.collect_identifier_references(array, env, result);
                self.collect_identifier_references(body, env, result);
            }
            _ => {}
        }
    }

    fn collect_stmt_refs(
        &self,
        stmt: &karte_hir::Statement,
        env: &NameEnv,
        result: &mut AnalysisResult,
    ) {
        use karte_hir::Statement;
        match stmt {
            Statement::Expression { expr, .. } => {
                self.collect_identifier_references(expr, env, result);
            }
            Statement::Let { value, .. } => {
                self.collect_identifier_references(value, env, result);
            }
            Statement::FunctionDef { body, .. } => {
                self.collect_identifier_references(body, env, result);
            }
            Statement::Assignment { target, value, .. } => {
                self.collect_identifier_references(target, env, result);
                self.collect_identifier_references(value, env, result);
            }
            _ => {}
        }
    }

    /// 收集标识符的类型信息（用于悬停提示）
    fn collect_identifier_types(
        &self,
        expr: &karte_hir::Expr,
        expr_types: &HashMap<usize, Type>,
        result: &mut AnalysisResult,
    ) {
        use karte_hir::Expr;
        let ptr = expr as *const Expr as usize;
        if let Some(ty) = expr_types.get(&ptr) {
            let type_str = format_type(ty);
            result.identifier_type_strings.insert((expr.span().start, expr.span().end), type_str);
        }

        match expr {
            Expr::Block { statements, final_expr, .. } => {
                for stmt in statements {
                    self.collect_stmt_types(stmt, expr_types, result);
                }
                if let Some(fe) = final_expr {
                    self.collect_identifier_types(fe, expr_types, result);
                }
            }
            Expr::BinaryOp { left, right, .. } => {
                self.collect_identifier_types(left, expr_types, result);
                self.collect_identifier_types(right, expr_types, result);
            }
            Expr::UnaryOp { operand, .. } => {
                self.collect_identifier_types(operand, expr_types, result);
            }
            Expr::FunctionCall { function, args, .. } => {
                self.collect_identifier_types(function, expr_types, result);
                for arg in args {
                    self.collect_identifier_types(arg, expr_types, result);
                }
            }
            Expr::If { condition, then_branch, else_branch, .. } => {
                self.collect_identifier_types(condition, expr_types, result);
                self.collect_identifier_types(then_branch, expr_types, result);
                if let Some(eb) = else_branch {
                    self.collect_identifier_types(eb, expr_types, result);
                }
            }
            Expr::While { condition, body, .. } => {
                self.collect_identifier_types(condition, expr_types, result);
                self.collect_identifier_types(body, expr_types, result);
            }
            Expr::Match { expr: scrutinee, arms, .. } => {
                self.collect_identifier_types(scrutinee, expr_types, result);
                for arm in arms {
                    self.collect_identifier_types(&arm.body, expr_types, result);
                }
            }
            Expr::Lambda { body, .. } => {
                self.collect_identifier_types(body, expr_types, result);
            }
            Expr::Assignment { target, value, .. } => {
                self.collect_identifier_types(target, expr_types, result);
                self.collect_identifier_types(value, expr_types, result);
            }
            Expr::ArrayLiteral { elements, .. } => {
                for elem in elements {
                    self.collect_identifier_types(elem, expr_types, result);
                }
            }
            Expr::Index { array, index, .. } => {
                self.collect_identifier_types(array, expr_types, result);
                self.collect_identifier_types(index, expr_types, result);
            }
            Expr::StructLiteral { fields, .. } => {
                for field in fields {
                    self.collect_identifier_types(&field.value, expr_types, result);
                }
            }
            Expr::FieldAccess { object, .. } => {
                self.collect_identifier_types(object, expr_types, result);
            }
            Expr::TupleLiteral { elements, .. } => {
                for elem in elements {
                    self.collect_identifier_types(elem, expr_types, result);
                }
            }
            Expr::TupleAccess { object, .. } => {
                self.collect_identifier_types(object, expr_types, result);
            }
            Expr::Constructor { args, .. } => {
                for arg in args {
                    self.collect_identifier_types(arg, expr_types, result);
                }
            }
            Expr::QualifiedConstructor { args, .. } => {
                for arg in args {
                    self.collect_identifier_types(arg, expr_types, result);
                }
            }
            Expr::Reference { expr: inner, .. } => {
                self.collect_identifier_types(inner, expr_types, result);
            }
            Expr::Dereference { expr: inner, .. } => {
                self.collect_identifier_types(inner, expr_types, result);
            }
            Expr::Return { value, .. } => {
                if let Some(v) = value {
                    self.collect_identifier_types(v, expr_types, result);
                }
            }
            Expr::ForIn { start, end, body, .. } => {
                self.collect_identifier_types(start, expr_types, result);
                self.collect_identifier_types(end, expr_types, result);
                self.collect_identifier_types(body, expr_types, result);
            }
            Expr::ForArray { array, body, .. } => {
                self.collect_identifier_types(array, expr_types, result);
                self.collect_identifier_types(body, expr_types, result);
            }
            _ => {}
        }
    }

    fn collect_stmt_types(
        &self,
        stmt: &karte_hir::Statement,
        expr_types: &HashMap<usize, Type>,
        result: &mut AnalysisResult,
    ) {
        use karte_hir::Statement;
        match stmt {
            Statement::Expression { expr, .. } => {
                self.collect_identifier_types(expr, expr_types, result);
            }
            Statement::Let { value, .. } => {
                self.collect_identifier_types(value, expr_types, result);
            }
            Statement::FunctionDef { body, .. } => {
                self.collect_identifier_types(body, expr_types, result);
            }
            Statement::Assignment { target, value, .. } => {
                self.collect_identifier_types(target, expr_types, result);
                self.collect_identifier_types(value, expr_types, result);
            }
            _ => {}
        }
    }

    /// 查找指定位置的悬停信息，返回 (类型字符串, 精确范围)
    pub fn get_hover_info(&self, position: Position) -> Option<(String, Range)> {
        let source = self.cached_source.as_ref()?;
        let result = self.cached_result.as_ref()?;
        let offset = position_to_offset(source, position);

        // 优先查找标识符类型信息
        for (&(start, end), type_str) in &result.identifier_type_strings {
            if offset >= start && offset <= end {
                let range = Range {
                    start: offset_to_position(source, start),
                    end: offset_to_position(source, end),
                };
                return Some((type_str.clone(), range));
            }
        }

        // 查找符号定义
        for sym in &result.symbols {
            if offset >= sym.span.start && offset <= sym.span.end {
                if let Some(sig) = &sym.type_signature {
                    let range = Range {
                        start: offset_to_position(source, sym.span.start),
                        end: offset_to_position(source, sym.span.end),
                    };
                    return Some((sig.clone(), range));
                }
            }
        }

        None
    }

    /// 查找指定位置的定义（支持标识符使用处 → 定义处跳转）
    pub fn get_definition(&self, position: Position) -> Option<Span> {
        let source = self.cached_source.as_ref()?;
        let result = self.cached_result.as_ref()?;
        let offset = position_to_offset(source, position);

        // 先查找标识符使用→定义
        for (&(use_start, use_end), &(def_start, def_end)) in &result.identifier_uses {
            if offset >= use_start && offset <= use_end {
                return Some(Span::new(def_start, def_end));
            }
        }

        // 回退：光标是否在定义自身上
        for sym in &result.symbols {
            if offset >= sym.span.start && offset <= sym.span.end {
                return Some(sym.span);
            }
        }

        None
    }

    /// 查找指定位置的所有引用
    pub fn find_references(&self, position: Position) -> Vec<Span> {
        let source = match self.cached_source.as_ref() {
            Some(s) => s,
            None => return Vec::new(),
        };
        let result = match self.cached_result.as_ref() {
            Some(r) => r,
            None => return Vec::new(),
        };
        let offset = position_to_offset(source, position);
        let mut refs = Vec::new();

        // 找到目标定义位置
        let mut target_def: Option<(usize, usize)> = None;

        // 检查是否在某个定义上
        for sym in &result.symbols {
            if offset >= sym.span.start && offset <= sym.span.end {
                target_def = Some((sym.span.start, sym.span.end));
                break;
            }
        }

        // 检查是否在某个使用处上
        if target_def.is_none() {
            for (&(use_start, use_end), &def_pos) in &result.identifier_uses {
                if offset >= use_start && offset <= use_end {
                    target_def = Some(def_pos);
                    break;
                }
            }
        }

        if let Some(def_pos) = target_def {
            // 定义自身
            refs.push(Span::new(def_pos.0, def_pos.1));
            // 所有引用到该定义的使用处
            for (&(use_start, use_end), &dp) in &result.identifier_uses {
                if dp.0 == def_pos.0 && dp.1 == def_pos.1 {
                    refs.push(Span::new(use_start, use_end));
                }
            }
        }

        refs
    }

    /// 获取补全建议（上下文感知）
    pub fn get_completions(&self, position: Position) -> Vec<KarteCompletionItem> {
        let source = self.cached_source.as_ref().map(|s| s.as_str()).unwrap_or("");
        let offset = position_to_offset(source, position);

        let mut items = Vec::new();

        // 获取当前位置的前缀（用于过滤）
        let before = if offset <= source.len() { &source[..offset] } else { "" };
        let prefix = Self::get_completion_prefix(before);

        // 关键词补全
        let keywords = [
            ("let", "Variable binding", "let $1 = $0;"),
            ("fn", "Function definition", "fn $1($2) {\n    $0\n}"),
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
            if kw.starts_with(prefix.as_str()) || prefix.is_empty() {
                items.push(KarteCompletionItem {
                    label: kw.to_string(),
                    kind: KarteCompletionKind::Keyword,
                    detail: Some(detail.to_string()),
                    insert_text: Some(snippet.to_string()),
                });
            }
        }

        // 符号补全（函数、变量、类型等）
        if let Some(result) = &self.cached_result {
            for sym in &result.symbols {
                if !sym.name.starts_with(prefix.as_str()) && !prefix.is_empty() {
                    continue;
                }
                let kind = match sym.kind {
                    KarteSymbolKind::Function => KarteCompletionKind::Function,
                    KarteSymbolKind::Variable => KarteCompletionKind::Variable,
                    KarteSymbolKind::Parameter => KarteCompletionKind::Variable,
                    KarteSymbolKind::Type => KarteCompletionKind::Class,
                    KarteSymbolKind::Enum => KarteCompletionKind::Enum,
                    KarteSymbolKind::Struct => KarteCompletionKind::Struct,
                    KarteSymbolKind::EnumVariant => KarteCompletionKind::EnumVariant,
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

    /// 从源码中提取补全前缀（当前正在输入的标识符前缀）
    fn get_completion_prefix(before: &str) -> String {
        let mut prefix = String::new();
        for ch in before.chars().rev() {
            if ch.is_alphanumeric() || ch == '_' {
                prefix.push(ch);
            } else {
                break;
            }
        }
        prefix.chars().rev().collect()
    }

    /// 获取文档中的所有符号
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

/// 格式化类型为可读字符串
fn format_type(ty: &karte_hir::types::Type) -> String {
    use karte_hir::types::Type;
    match ty {
        Type::Number => "number".to_string(),
        Type::Bool => "bool".to_string(),
        Type::String => "string".to_string(),
        Type::Unit => "()".to_string(),
        Type::Var(_) => "_".to_string(),
        Type::Unknown => "unknown".to_string(),
        Type::Function { params, return_type } => {
            let param_strs: Vec<String> = params.iter().map(format_type).collect();
            format!("fn({}) -> {}", param_strs.join(", "), format_type(return_type))
        }
        Type::Closure { params, return_type } => {
            let param_strs: Vec<String> = params.iter().map(format_type).collect();
            format!("closure({}) -> {}", param_strs.join(", "), format_type(return_type))
        }
        Type::Array { element } => format!("[{}]", format_type(element)),
        Type::Sum { name, .. } => name.clone(),
        Type::Struct { name, .. } => name.clone(),
        Type::Tuple(types) => {
            let type_strs: Vec<String> = types.iter().map(format_type).collect();
            format!("({})", type_strs.join(", "))
        }
        Type::Reference { inner } => format!("&{}", format_type(inner)),
        Type::Int(_) => "number".to_string(),
        Type::Generic { name, .. } => name.clone(),
        _ => format!("{:?}", ty),
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
