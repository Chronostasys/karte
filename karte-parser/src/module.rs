//! 模块和导入声明解析
//!
//! 本模块包含解析Karte模块系统相关语法的功能，包括：
//! - `module` 声明解析
//! - `import` 声明解析
//! - 模块符号访问（如 `utils.math::add`）
//!
//! 这些功能主要用于项目模式，但也支持脚本模式中的import。

use crate::types::{ImportDecl, ImportSpecifier, ImportSymbol, ModuleDecl, ParseError};
use crate::Parser;
use karte_diagnostics::Span;
use karte_hir::Expr;
use karte_lexer::Token;

impl<'a> Parser<'a> {
    /// 解析模块前导部分（module和import声明）
    pub(crate) fn parse_module_prelude(&mut self) {
        if self.prelude_parsed {
            return;
        }
        self.prelude_parsed = true;

        loop {
            let token = match self.peek() {
                Some(token) => token.clone(),
                None => break,
            };

            let keyword = match &token.token {
                Token::Identifier(name) => name.clone(),
                _ => break,
            };

            match keyword.as_str() {
                "module" => {
                    if !self.next_token_is_identifier() {
                        break;
                    }
                    if let Err(err) = self.parse_module_decl() {
                        self.add_parse_error(err);
                        self.recover_from_error();
                    }
                }
                "import" => {
                    if !self.next_token_is_identifier() {
                        break;
                    }
                    if let Err(err) = self.parse_import_decl() {
                        self.add_parse_error(err);
                        self.recover_from_error();
                    }
                }
                _ => break,
            }
        }
    }

    /// 解析module声明
    pub(crate) fn parse_module_decl(&mut self) -> Result<(), ParseError> {
        let keyword_span = self
            .peek()
            .ok_or(ParseError::UnexpectedEof {
                expected: "module name".to_string(),
            })?
            .span;
        self.advance(); // consume 'module'

        if self.module_decl.is_some() {
            return Err(ParseError::InvalidExpression {
                message: "Duplicate module declaration".to_string(),
                span: keyword_span,
            });
        }

        let (parts, last_span) = self.parse_module_path("module name")?;
        let span = Span::new(keyword_span.start, last_span.end);
        self.module_decl = Some(ModuleDecl {
            name: parts.join("."),
            span,
        });

        if let Some(token) = self.peek() {
            if matches!(token.token, Token::Semicolon) {
                self.advance();
            }
        }
        Ok(())
    }

    /// 解析import声明
    pub(crate) fn parse_import_decl(&mut self) -> Result<(), ParseError> {
        let start_span = self
            .peek()
            .ok_or(ParseError::UnexpectedEof {
                expected: "import path".to_string(),
            })?
            .span;
        self.advance(); // consume 'import'

        let (path, mut end_span) = self.parse_module_path("import path")?;

        let alias = if self.peek_keyword("as") {
            self.advance();
            let (alias_name, alias_span) = self.expect_identifier("import alias")?;
            end_span = alias_span;
            Some(alias_name)
        } else {
            None
        };

        let mut specifier = ImportSpecifier::EntireModule;

        if self.peek_is_double_colon() {
            self.advance(); // consume '::'
            let brace = self.peek().ok_or(ParseError::UnexpectedEof {
                expected: "'{'".to_string(),
            })?;
            if !matches!(brace.token, Token::LeftBrace) {
                return Err(ParseError::UnexpectedToken {
                    expected: "'{'".to_string(),
                    found: brace.token.clone(),
                    span: brace.span,
                });
            }
            self.advance(); // consume '{'
            let mut symbols = Vec::new();
            loop {
                let token = self.peek().ok_or(ParseError::UnexpectedEof {
                    expected: "import symbol or '}'".to_string(),
                })?;
                match &token.token {
                    Token::RightBrace => {
                        end_span = token.span;
                        self.advance();
                        break;
                    }
                    Token::Identifier(_) => {
                        let (symbol_name, mut symbol_span) =
                            self.expect_identifier("import symbol")?;
                        let alias = if self.peek_keyword("as") {
                            self.advance();
                            let (alias_name, alias_span) =
                                self.expect_identifier("import alias")?;
                            symbol_span = Span::new(symbol_span.start, alias_span.end);
                            Some(alias_name)
                        } else {
                            None
                        };
                        symbols.push(ImportSymbol {
                            name: symbol_name,
                            alias,
                            span: symbol_span,
                        });

                        if let Some(next) = self.peek() {
                            match next.token {
                                Token::Comma => {
                                    self.advance();
                                }
                                Token::RightBrace => {}
                                _ => {
                                    return Err(ParseError::UnexpectedToken {
                                        expected: "',' or '}'".to_string(),
                                        found: next.token.clone(),
                                        span: next.span,
                                    })
                                }
                            }
                        } else {
                            return Err(ParseError::UnexpectedEof {
                                expected: "',' or '}'".to_string(),
                            });
                        }
                    }
                    _ => {
                        return Err(ParseError::UnexpectedToken {
                            expected: "import symbol or '}'".to_string(),
                            found: token.token.clone(),
                            span: token.span,
                        })
                    }
                }
            }
            specifier = ImportSpecifier::Symbols(symbols);
        }

        if let Some(token) = self.peek() {
            if matches!(token.token, Token::Semicolon) {
                end_span = token.span;
                self.advance();
            }
        }

        let span = Span::new(start_span.start, end_span.end);
        self.imports.push(ImportDecl {
            path,
            alias,
            specifier,
            span,
        });
        Ok(())
    }

    /// 解析模块路径（如 `utils.math`）
    pub(crate) fn parse_module_path(
        &mut self,
        context: &str,
    ) -> Result<(Vec<String>, Span), ParseError> {
        let (first_ident, mut last_span) = self.expect_identifier(context)?;
        let start_span = last_span;
        let mut parts = vec![first_ident];

        loop {
            let token = match self.peek() {
                Some(token) => token.clone(),
                None => break,
            };
            match token.token {
                Token::Dot => {
                    self.advance();
                    let (next_ident, span) = self.expect_identifier(context)?;
                    last_span = span;
                    parts.push(next_ident);
                }
                _ => break,
            }
        }

        Ok((parts, Span::new(start_span.start, last_span.end)))
    }

    /// 期望一个标识符token
    pub(crate) fn expect_identifier(
        &mut self,
        context: &str,
    ) -> Result<(String, Span), ParseError> {
        if let Some(token) = self.peek() {
            if let Token::Identifier(name) = &token.token {
                let span = token.span;
                let name = name.clone();
                self.advance();
                return Ok((name, span));
            }
            return Err(ParseError::UnexpectedToken {
                expected: context.to_string(),
                found: token.token.clone(),
                span: token.span,
            });
        }
        Err(ParseError::UnexpectedEof {
            expected: context.to_string(),
        })
    }

    /// 检查下一个token是否为标识符
    pub(crate) fn next_token_is_identifier(&self) -> bool {
        self.tokens
            .get(self.position + 1)
            .map(|token| matches!(token.token, Token::Identifier(_)))
            .unwrap_or(false)
    }

    /// 检查当前token是否为特定关键字
    pub(crate) fn peek_keyword(&self, keyword: &str) -> bool {
        matches!(self.peek(), Some(token) if matches!(&token.token, Token::Identifier(name) if name == keyword))
    }

    /// 检查当前token是否为双冒号`::`
    pub(crate) fn peek_is_double_colon(&self) -> bool {
        matches!(self.peek(), Some(token) if matches!(token.token, Token::DoubleColon))
    }

    /// 尝试解析模块符号访问表达式（如 `utils.math::add`）
    pub(crate) fn try_parse_module_symbol_access(
        &mut self,
        first_ident: &str,
        start_span: Span,
    ) -> Option<Expr> {
        let (module_path, symbol, symbol_span, tokens_to_consume) =
            self.preview_module_symbol_access(first_ident)?;
        for _ in 0..tokens_to_consume {
            self.advance();
        }
        let span = Span::new(start_span.start, symbol_span.end);
        Some(Expr::ModuleSymbolAccess {
            module_path,
            symbol,
            span,
        })
    }

    /// 预览模块符号访问，不消耗token
    pub(crate) fn preview_module_symbol_access(
        &self,
        first_ident: &str,
    ) -> Option<(Vec<String>, String, Span, usize)> {
        let mut module_path = vec![first_ident.to_string()];
        let mut index = self.position;
        let mut tokens_to_consume = 0usize;
        let mut saw_dot = false;

        while let Some(token) = self.tokens.get(index) {
            match &token.token {
                Token::Dot => {
                    let next = self.tokens.get(index + 1)?;
                    if let Token::Identifier(segment) = &next.token {
                        module_path.push(segment.clone());
                        index += 2;
                        tokens_to_consume += 2;
                        saw_dot = true;
                    } else {
                        return None;
                    }
                }
                Token::DoubleColon => {
                    if !saw_dot && self.is_constructor(first_ident) {
                        return None;
                    }
                    let next = self.tokens.get(index + 1)?;
                    if let Token::Identifier(symbol) = &next.token {
                        return Some((
                            module_path,
                            symbol.clone(),
                            next.span,
                            tokens_to_consume + 2,
                        ));
                    } else {
                        return None;
                    }
                }
                _ => return None,
            }
        }
        None
    }

    /// 检查标识符是否为构造器（首字母大写或内置构造器）
    pub(crate) fn is_constructor(&self, name: &str) -> bool {
        // 识别内置构造器和可能的自定义构造器（首字母大写）
        matches!(
            name,
            "None" | "Some" | "Left" | "Right" | "Ok" | "Err" | "True" | "False"
        ) || (name.chars().next().is_some_and(|c| c.is_uppercase()))
    }
}
