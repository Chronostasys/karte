mod expression;
mod module;
mod pattern;
mod statement;
mod types;

use karte_diagnostics::{DiagnosticBag, Span};
use karte_hir::type_checker::ExternalModuleInterface;
use karte_lexer::{Token, TokenWithSpan};
use std::collections::HashMap;

// 重新导出types模块的公共类型
pub use types::{
    ImportDecl, ImportSpecifier, ImportSymbol, ModuleDecl, ParseError, ParsedProgram, ParserMode,
};

// 重新导出HIR中的类型，保持向后兼容性
pub use karte_hir::{
    type_check, type_check_with_context, type_check_with_context_and_maps, BinaryOperator, Expr,
    ModuleContext, Statement, Type, UnaryOperator,
};

/// 语法分析器
pub struct Parser<'a> {
    tokens: &'a [TokenWithSpan],
    position: usize,
    diagnostics: DiagnosticBag,
    mode: ParserMode,
    module_decl: Option<ModuleDecl>,
    imports: Vec<ImportDecl>,
    prelude_parsed: bool,
}

impl<'a> Parser<'a> {
    pub fn new(tokens: &'a [TokenWithSpan]) -> Self {
        Self {
            tokens,
            position: 0,
            diagnostics: DiagnosticBag::new(),
            mode: ParserMode::Script, // 默认为脚本模式
            module_decl: None,
            imports: Vec::new(),
            prelude_parsed: false,
        }
    }

    /// 设置解析模式
    pub fn with_mode(mut self, mode: ParserMode) -> Self {
        self.mode = mode;
        self
    }

    /// 解析程序 - 可以是单个表达式或包含语句的块，带 module/import 前导
    pub fn parse(&mut self) -> Option<ParsedProgram> {
        if self.tokens.is_empty() {
            self.diagnostics.add_error("Empty input", Span::new(0, 0));
            return None;
        }

        self.parse_module_prelude();

        if self.peek().is_none() {
            self.diagnostics.add_error(
                "Expected declarations or expressions after module/import prelude",
                Span::new(0, 0),
            );
            return None;
        }

        let body_expr = if self.starts_with_statement() {
            match self.parse_program() {
                Ok(expr) => expr,
                Err(err) => {
                    self.add_parse_error(err);
                    return None;
                }
            }
        } else {
            // 单个表达式模式
            match self.parse_expression() {
                Ok(expr) => {
                    if self.position < self.tokens.len() {
                        let token = &self.tokens[self.position];
                        self.diagnostics
                            .add_error(format!("Unexpected token: {}", token.token), token.span);
                    }
                    expr
                }
                Err(err) => {
                    self.add_parse_error(err);
                    return None;
                }
            }
        };

        Some(ParsedProgram {
            module: self.module_decl.clone(),
            imports: self.imports.clone(),
            body: Box::new(body_expr),
        })
    }

    pub fn diagnostics(&self) -> &DiagnosticBag {
        &self.diagnostics
    }

    pub fn into_diagnostics(self) -> DiagnosticBag {
        self.diagnostics
    }

    fn add_parse_error(&mut self, error: ParseError) {
        match error {
            ParseError::UnexpectedToken {
                expected: _,
                found: _,
                span,
            }
            | ParseError::InvalidExpression { message: _, span } => {
                self.diagnostics.add_error(error.to_string(), span);
            }
            ParseError::UnexpectedEof { expected: _ } => {
                let span = if self.tokens.is_empty() {
                    Span::new(0, 0)
                } else {
                    let last = &self.tokens[self.tokens.len() - 1];
                    Span::new(last.span.end, last.span.end)
                };
                self.diagnostics.add_error(error.to_string(), span);
            }
            ParseError::MissingClosingParen {
                opening_span: _,
                current_span,
            } => {
                self.diagnostics.add_error(error.to_string(), current_span);
            }
            ParseError::MissingOperand {
                operator: _,
                operator_span,
            } => {
                self.diagnostics.add_error(error.to_string(), operator_span);
            }
            ParseError::ExpressionTooDeep { max_depth: _, span } => {
                self.diagnostics.add_error(error.to_string(), span);
            }
        }
    }

    pub(crate) fn peek(&self) -> Option<&TokenWithSpan> {
        self.tokens.get(self.position)
    }

    pub(crate) fn advance(&mut self) {
        if self.position < self.tokens.len() {
            self.position += 1;
        }
    }

    pub(crate) fn expect_token(&mut self, expected: Token) -> Result<(), ParseError> {
        if let Some(token) = self.peek() {
            if token.token == expected {
                self.advance();
                return Ok(());
            }
            return Err(ParseError::UnexpectedToken {
                expected: format!("{:?}", expected),
                found: token.token.clone(),
                span: token.span,
            });
        }
        Err(ParseError::UnexpectedEof {
            expected: format!("{:?}", expected),
        })
    }

    pub(crate) fn current_span(&self) -> Span {
        if self.position > 0 && self.position <= self.tokens.len() {
            self.tokens[self.position - 1].span
        } else {
            Span::new(0, 0)
        }
    }

    /// 错误恢复：跳过当前token并尝试继续解析
    fn recover_from_error(&mut self) {
        // 跳过当前错误的token
        self.advance();

        // 尝试找到下一个安全的同步点
        while let Some(token) = self.peek() {
            match token.token {
                // 在这些token处可以安全地重新开始解析
                Token::Plus
                | Token::Minus
                | Token::Multiply
                | Token::Divide
                | Token::LeftParen
                | Token::RightParen => break,
                _ => self.advance(),
            }
        }
    }

    /// 改进的解析方法，支持错误恢复
    pub fn parse_with_recovery(&mut self) -> Vec<Expr> {
        let mut expressions = Vec::new();

        while self.position < self.tokens.len() {
            match self.parse_expression() {
                Ok(expr) => {
                    expressions.push(expr);
                    // 如果还有token，期望是分隔符或结束
                    if self.position < self.tokens.len() {
                        // 这里可以处理多个表达式的情况
                        break;
                    }
                }
                Err(err) => {
                    self.add_parse_error(err);
                    self.recover_from_error();

                    // 如果能恢复，继续尝试解析
                    if self.position < self.tokens.len() {
                        continue;
                    } else {
                        break;
                    }
                }
            }
        }

        expressions
    }
}

/// 解析结果，包含 AST、module/import 元数据以及类型信息
pub struct ParseResult {
    pub program: ParsedProgram,
    pub result_type: Type,
    pub module_context: ModuleContext,
    /// 表达式类型映射，用于从HIR传递类型信息到MIR
    pub expr_types: HashMap<usize, Type>,
}

impl ParseResult {
    pub fn expr(&self) -> &Expr {
        &self.program.body
    }

    pub fn module(&self) -> Option<&ModuleDecl> {
        self.program.module.as_ref()
    }

    pub fn imports(&self) -> &[ImportDecl] {
        &self.program.imports
    }

    pub fn module_context(&self) -> &ModuleContext {
        &self.module_context
    }
}

/// 便捷的解析函数（兼容性版本）
pub fn parse(tokens: &[TokenWithSpan]) -> (Option<Expr>, DiagnosticBag) {
    let mut parser = Parser::new(tokens);
    let program = parser.parse();
    (program.map(|p| *p.body), parser.into_diagnostics())
}

/// 返回包含 module/import 信息的解析结果
pub fn parse_program_with_metadata(
    tokens: &[TokenWithSpan],
    mode: ParserMode,
) -> (Option<ParsedProgram>, DiagnosticBag) {
    let mut parser = Parser::new(tokens).with_mode(mode);
    let program = parser.parse();
    (program, parser.into_diagnostics())
}

/// 带类型检查的解析函数
pub fn parse_with_type_check(
    tokens: &[TokenWithSpan],
    mode: ParserMode,
    dependency_interfaces: Option<&HashMap<String, ExternalModuleInterface>>,
) -> (Option<ParseResult>, DiagnosticBag) {
    let mut parser = Parser::new(tokens).with_mode(mode);
    let program = parser.parse();
    let mut diagnostics = parser.into_diagnostics();

    if let Some(program) = program {
        let mut module_context = build_module_context(&program);
        if let Some(interfaces) = dependency_interfaces {
            module_context.set_dependency_interfaces(interfaces.clone());
        }
        // 进行类型检查，获取lambda_types映射
        let (result_type, expr_types, type_diagnostics) =
            type_check_with_context_and_maps(&program.body, module_context.clone());

        // 合并诊断信息
        for error in &type_diagnostics.diagnostics {
            diagnostics.add_error(error.message.clone(), error.span);
        }

        (
            Some(ParseResult {
                program,
                result_type,
                module_context,
                expr_types,
            }),
            diagnostics,
        )
    } else {
        (None, diagnostics)
    }
}

fn build_module_context(program: &ParsedProgram) -> ModuleContext {
    let mut context = ModuleContext::default();
    if let Some(module_decl) = &program.module {
        context.module_name = Some(module_decl.name.clone());
    }
    for import in &program.imports {
        match &import.specifier {
            ImportSpecifier::Symbols(symbols) => {
                for symbol in symbols {
                    let alias = symbol.alias.clone().unwrap_or_else(|| symbol.name.clone());
                    context.add_import_symbol(import.path.clone(), symbol.name.clone(), alias);
                }
            }
            ImportSpecifier::EntireModule => {
                if let Some(alias) = &import.alias {
                    context.add_import_symbol(import.path.clone(), "*".to_string(), alias.clone());
                }
            }
        }
    }
    context
}

#[cfg(test)]
mod assignment_tests {
    use super::*;

    use karte_lexer::tokenize;

    #[test]
    fn test_assignment_expression() {
        let input = "x = 42";
        let (tokens, _) = tokenize(input);
        let (expr, diagnostics) = parse(&tokens);

        assert!(diagnostics.is_empty(), "解析应该没有错误");
        assert!(expr.is_some(), "应该成功解析表达式");

        if let Some(Expr::Assignment { target, value, .. }) = expr {
            match target.as_ref() {
                Expr::Identifier { name, .. } => {
                    assert_eq!(name, "x");
                }
                _ => panic!("赋值目标应该是标识符"),
            }
            match value.as_ref() {
                Expr::Number { value, .. } => {
                    assert_eq!(*value, 42);
                }
                _ => panic!("赋值值应该是数字"),
            }
        } else {
            panic!("应该解析为赋值表达式");
        }
    }

    #[test]
    fn test_chained_assignment() {
        let input = "a = b = 5";
        let (tokens, _) = tokenize(input);
        let (expr, diagnostics) = parse(&tokens);

        assert!(diagnostics.is_empty(), "解析应该没有错误");
        assert!(expr.is_some(), "应该成功解析表达式");

        if let Some(Expr::Assignment { target, value, .. }) = expr {
            // 外层赋值：a = (b = 5)
            match target.as_ref() {
                Expr::Identifier { name, .. } => {
                    assert_eq!(name, "a");
                }
                _ => panic!("外层赋值目标应该是标识符 a"),
            }

            // 内层赋值：b = 5
            match value.as_ref() {
                Expr::Assignment {
                    target: inner_target,
                    value: inner_value,
                    ..
                } => {
                    match inner_target.as_ref() {
                        Expr::Identifier { name, .. } => {
                            assert_eq!(name, "b");
                        }
                        _ => panic!("内层赋值目标应该是标识符 b"),
                    }
                    match inner_value.as_ref() {
                        Expr::Number { value, .. } => {
                            assert_eq!(*value, 5);
                        }
                        _ => panic!("内层赋值值应该是数字 5"),
                    }
                }
                _ => panic!("右侧应该是另一个赋值表达式"),
            }
        } else {
            panic!("应该解析为赋值表达式");
        }
    }

    #[test]
    fn test_assignment_with_expression() {
        let input = "x = y + 1";
        let (tokens, _) = tokenize(input);
        let (expr, diagnostics) = parse(&tokens);

        assert!(diagnostics.is_empty(), "解析应该没有错误");
        assert!(expr.is_some(), "应该成功解析表达式");

        if let Some(Expr::Assignment { target, value, .. }) = expr {
            match target.as_ref() {
                Expr::Identifier { name, .. } => {
                    assert_eq!(name, "x");
                }
                _ => panic!("赋值目标应该是标识符"),
            }
            match value.as_ref() {
                Expr::BinaryOp {
                    op: BinaryOperator::Add,
                    ..
                } => {
                    // 正确，是加法表达式
                }
                _ => panic!("赋值值应该是加法表达式"),
            }
        } else {
            panic!("应该解析为赋值表达式");
        }
    }

    #[test]
    fn test_field_assignment_syntax() {
        let input = "obj.field = 100";
        let (tokens, _) = tokenize(input);
        let (expr, diagnostics) = parse(&tokens);

        assert!(diagnostics.is_empty(), "解析应该没有错误");
        assert!(expr.is_some(), "应该成功解析表达式");

        if let Some(Expr::Assignment { target, value, .. }) = expr {
            match target.as_ref() {
                Expr::FieldAccess { object, field, .. } => {
                    match object.as_ref() {
                        Expr::Identifier { name, .. } => {
                            assert_eq!(name, "obj");
                        }
                        _ => panic!("字段访问的对象应该是标识符"),
                    }
                    assert_eq!(field, "field");
                }
                _ => panic!("赋值目标应该是字段访问"),
            }
            match value.as_ref() {
                Expr::Number { value, .. } => {
                    assert_eq!(*value, 100);
                }
                _ => panic!("赋值值应该是数字"),
            }
        } else {
            panic!("应该解析为赋值表达式");
        }
    }

    #[test]
    fn test_assignment_precedence() {
        let input = "x = y + z * 2";
        let (tokens, _) = tokenize(input);
        let (expr, diagnostics) = parse(&tokens);

        assert!(diagnostics.is_empty(), "解析应该没有错误");
        assert!(expr.is_some(), "应该成功解析表达式");

        if let Some(Expr::Assignment { target, value, .. }) = expr {
            match target.as_ref() {
                Expr::Identifier { name, .. } => {
                    assert_eq!(name, "x");
                }
                _ => panic!("赋值目标应该是标识符"),
            }
            // 右侧应该是 y + (z * 2)，不是 (y + z) * 2
            match value.as_ref() {
                Expr::BinaryOp {
                    op: BinaryOperator::Add,
                    left,
                    right,
                    ..
                } => {
                    match left.as_ref() {
                        Expr::Identifier { name, .. } => {
                            assert_eq!(name, "y");
                        }
                        _ => panic!("加法左侧应该是标识符 y"),
                    }
                    match right.as_ref() {
                        Expr::BinaryOp {
                            op: BinaryOperator::Multiply,
                            ..
                        } => {
                            // 正确，乘法有更高优先级
                        }
                        _ => panic!("加法右侧应该是乘法表达式"),
                    }
                }
                _ => panic!("赋值值应该是加法表达式"),
            }
        } else {
            panic!("应该解析为赋值表达式");
        }
    }
}

#[cfg(test)]
mod module_context_tests {
    use super::*;
    use karte_hir::type_checker::ExternalFunctionSignature;
    use karte_lexer::tokenize;
    use std::collections::HashMap;

    #[test]
    fn parse_with_type_check_attaches_dependency_interfaces() {
        let input = "module main fn main() -> number { 0 }";
        let (tokens, _) = tokenize(input);

        let mut interfaces: HashMap<String, ExternalModuleInterface> = HashMap::new();
        let mut module_interface = ExternalModuleInterface::default();
        module_interface.functions.insert(
            "add".into(),
            ExternalFunctionSignature {
                name: "add".into(),
                params: 2,
            },
        );
        interfaces.insert("utils".into(), module_interface);

        let (result, diagnostics) =
            parse_with_type_check(&tokens, ParserMode::Project, Some(&interfaces));

        assert!(
            diagnostics.is_empty(),
            "Expected no diagnostics: {:?}",
            diagnostics
        );
        let parsed = result.expect("expected parse result");
        assert!(
            parsed
                .module_context()
                .dependency_interfaces()
                .contains_key("utils"),
            "module context should retain dependency interfaces"
        );
    }

    #[test]
    fn parses_alias_module_symbol_expression() {
        let (tokens, _) = tokenize("array::len");
        let mut parser = Parser::new(&tokens);
        let expr = parser
            .parse_expression()
            .expect("module symbol expression should parse");
        match expr {
            Expr::ModuleSymbolAccess {
                module_path,
                symbol,
                ..
            } => {
                assert_eq!(module_path, vec!["array".to_string()]);
                assert_eq!(symbol, "len");
            }
            other => panic!("expected ModuleSymbolAccess, got {:?}", other),
        }
    }

    #[test]
    fn parses_dotted_module_symbol_expression() {
        let (tokens, _) = tokenize("std.array::len");
        let mut parser = Parser::new(&tokens);
        let expr = parser
            .parse_expression()
            .expect("module symbol expression should parse");
        match expr {
            Expr::ModuleSymbolAccess {
                module_path,
                symbol,
                ..
            } => {
                assert_eq!(module_path, vec!["std".to_string(), "array".to_string()]);
                assert_eq!(symbol, "len");
            }
            other => panic!("expected ModuleSymbolAccess, got {:?}", other),
        }
    }
}

#[cfg(test)]
mod parser_mode_tests {
    use super::*;
    use karte_lexer::tokenize;

    #[test]
    fn test_script_mode_allows_top_level_statements() {
        let input = "let x = 1; x + 1;";
        let (tokens, _) = tokenize(input);
        let mut parser = Parser::new(&tokens).with_mode(ParserMode::Script);
        let result = parser.parse_program();
        assert!(result.is_ok());
    }

    #[test]
    fn test_project_mode_disallows_top_level_statements() {
        let input = "1 + 1;";
        let (tokens, _) = tokenize(input);
        let mut parser = Parser::new(&tokens).with_mode(ParserMode::Project);
        let result = parser.parse_program();
        assert!(result.is_err());
        match result.unwrap_err() {
            ParseError::InvalidExpression { message, .. } => {
                assert!(message.contains("Top-level statements must be declarations"));
            }
            _ => panic!("Expected InvalidExpression error"),
        }
    }

    #[test]
    fn test_project_mode_disallows_top_level_expressions() {
        let input = "1 + 2";
        let (tokens, _) = tokenize(input);
        let mut parser = Parser::new(&tokens).with_mode(ParserMode::Project);
        let result = parser.parse_program();
        assert!(result.is_err());
        match result.unwrap_err() {
            ParseError::InvalidExpression { message, .. } => {
                assert!(message.contains("Top-level expressions are not allowed"));
            }
            _ => panic!("Expected InvalidExpression error"),
        }
    }

    #[test]
    fn test_project_mode_allows_declarations() {
        let input = "fn main() {} struct Point { x: i32 } let CONST = 1;";
        let (tokens, _) = tokenize(input);
        let mut parser = Parser::new(&tokens).with_mode(ParserMode::Project);
        let result = parser.parse_program();
        assert!(result.is_ok());
    }
}
