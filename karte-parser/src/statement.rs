//! 语句和声明解析模块
//!
//! 本模块包含所有与语句和声明解析相关的功能，包括：
//! - 语句解析（let、enum、struct、fn等）
//! - 块表达式解析
//! - 类型表达式解析
//! - 程序结构解析

use karte_diagnostics::Span;
use karte_hir::{Expr, IntKind, Statement, Type};
use karte_lexer::Token;

use crate::types::{ParseError, ParserMode};
use crate::Parser;

impl<'a> Parser<'a> {
    /// 检查当前位置是否是语句的开始
    pub(crate) fn starts_with_statement(&self) -> bool {
        if let Some(token) = self.peek() {
            if let Token::Identifier(name) = &token.token {
                return matches!(name.as_str(), "let" | "enum" | "struct" | "fn");
            }
            if matches!(token.token, Token::KwPub) {
                // pub fn / pub struct / pub enum
                if self.position + 1 < self.tokens.len() {
                    if let Some(next) = self.tokens.get(self.position + 1) {
                        if let Token::Identifier(name) = &next.token {
                            return matches!(name.as_str(), "fn" | "struct" | "enum");
                        }
                    }
                }
            }
        }
        false
    }

    /// 解析程序（语句序列）
    pub(crate) fn parse_program(&mut self) -> Result<Expr, ParseError> {
        let start_span = self.peek().unwrap().span;
        let mut statements = Vec::new();
        let mut final_expr = None;

        while let Some(token) = self.peek() {
            // 检查是否为语句
            if let Token::Identifier(name) = &token.token {
                if matches!(name.as_str(), "let" | "enum" | "struct" | "fn") {
                    statements.push(self.parse_statement()?);
                    continue;
                }

                // 检查是否为赋值语句 (identifier = ...) 或复合赋值语句 (identifier += ...)
                if self.position + 1 < self.tokens.len() {
                    if let Some(next_token) = self.tokens.get(self.position + 1) {
                        if matches!(
                            next_token.token,
                            Token::Equal
                                | Token::PlusEqual
                                | Token::MinusEqual
                                | Token::StarEqual
                                | Token::SlashEqual
                        ) {
                            // 这是一个赋值语句
                            statements.push(self.parse_statement()?);
                            continue;
                        }
                    }
                }
            }

            // 检查是否为 pub 声明
            if matches!(token.token, Token::KwPub) {
                if self.position + 1 < self.tokens.len() {
                    if let Some(next) = self.tokens.get(self.position + 1) {
                        if let Token::Identifier(name) = &next.token {
                            if matches!(name.as_str(), "fn" | "struct" | "enum") {
                                statements.push(self.parse_statement()?);
                                continue;
                            }
                        }
                    }
                }
            }

            // 不是语句，尝试解析表达式
            let expr = self.parse_expression()?;

            // 检查是否有分号 - 如果有分号，这是一个表达式语句
            if let Some(token) = self.peek() {
                if matches!(token.token, Token::Semicolon) {
                    self.advance(); // consume ';'
                                    // 这是一个表达式语句，加入statements
                    let expr_span = expr.span();
                    statements.push(Statement::Expression {
                        expr,
                        span: expr_span,
                    });
                    continue;
                }
            }

            // 没有分号，这是最终表达式
            final_expr = Some(Box::new(expr));
            break;
        }

        // 验证项目模式约束
        if self.mode == ParserMode::Project {
            if let Some(expr) = &final_expr {
                return Err(ParseError::InvalidExpression {
                    message: "Top-level expressions are not allowed in Project mode".to_string(),
                    span: expr.span(),
                });
            }

            for stmt in &statements {
                match stmt {
                    Statement::FunctionDef { .. }
                    | Statement::StructDef { .. }
                    | Statement::TypeDef { .. }
                    | Statement::Let { .. } => {
                        // 允许的声明
                    }
                    Statement::Expression { span, .. } | Statement::Assignment { span, .. } => {
                        return Err(ParseError::InvalidExpression {
                            message: "Top-level statements must be declarations in Project mode"
                                .to_string(),
                            span: *span,
                        });
                    }
                }
            }
        }

        let end_span = if let Some(expr) = &final_expr {
            expr.span()
        } else if let Some(last_stmt) = statements.last() {
            last_stmt.span()
        } else {
            start_span
        };

        let span = Span::new(start_span.start, end_span.end);
        Ok(Expr::Block {
            statements,
            final_expr,
            span,
        })
    }

    /// 解析语句
    pub(crate) fn parse_statement(&mut self) -> Result<Statement, ParseError> {
        if let Some(token) = self.peek() {
            if let Token::Identifier(name) = &token.token {
                if name == "let" {
                    return self.parse_let_statement();
                } else if name == "enum" {
                    return self.parse_enum_statement(false);
                } else if name == "struct" {
                    return self.parse_struct_statement(false);
                } else if name == "fn" {
                    return self.parse_function_definition(false);
                }
            }
            // pub 声明
            if matches!(token.token, Token::KwPub) {
                self.advance(); // consume 'pub'
                if let Some(next) = self.peek() {
                    if let Token::Identifier(name) = &next.token {
                        if name == "fn" {
                            return self.parse_function_definition(true);
                        } else if name == "struct" {
                            return self.parse_struct_statement(true);
                        } else if name == "enum" {
                            return self.parse_enum_statement(true);
                        }
                    }
                }
                return Err(ParseError::UnexpectedToken {
                    expected: "'fn', 'struct', or 'enum' after 'pub'".to_string(),
                    found: self.peek().unwrap().token.clone(),
                    span: self.peek().unwrap().span,
                });
            }
        }

        // 表达式语句 - 这里可能是赋值或其他表达式
        let expr = self.parse_expression()?;

        // 如果是赋值表达式，将其转换为赋值语句
        match expr {
            Expr::Assignment {
                target,
                value,
                span,
            } => {
                // 期望分号
                if let Some(token) = self.peek() {
                    if matches!(token.token, Token::Semicolon) {
                        self.advance();
                    } else {
                        return Err(ParseError::UnexpectedToken {
                            expected: "';'".to_string(),
                            found: token.token.clone(),
                            span: token.span,
                        });
                    }
                } else {
                    return Err(ParseError::UnexpectedEof {
                        expected: "';'".to_string(),
                    });
                }

                Ok(Statement::Assignment {
                    target: *target,
                    value: *value,
                    span,
                })
            }
            _ => {
                // 普通表达式语句
                let span = expr.span();

                // 期望分号
                if let Some(token) = self.peek() {
                    if matches!(token.token, Token::Semicolon) {
                        self.advance();
                    } else {
                        return Err(ParseError::UnexpectedToken {
                            expected: "';'".to_string(),
                            found: token.token.clone(),
                            span: token.span,
                        });
                    }
                } else {
                    return Err(ParseError::UnexpectedEof {
                        expected: "';'".to_string(),
                    });
                }

                Ok(Statement::Expression { expr, span })
            }
        }
    }

    /// 解析let语句
    pub(crate) fn parse_let_statement(&mut self) -> Result<Statement, ParseError> {
        let start_span = self.peek().unwrap().span;
        self.advance(); // consume 'let'

        // 解析变量名
        let name = if let Some(token) = self.peek() {
            if let Token::Identifier(name) = &token.token {
                let name = name.clone();
                self.advance();
                name
            } else {
                return Err(ParseError::UnexpectedToken {
                    expected: "identifier".to_string(),
                    found: token.token.clone(),
                    span: token.span,
                });
            }
        } else {
            return Err(ParseError::UnexpectedEof {
                expected: "identifier".to_string(),
            });
        };

        // 期望 '='
        if let Some(token) = self.peek() {
            if matches!(token.token, Token::Equal) {
                self.advance();
            } else {
                return Err(ParseError::UnexpectedToken {
                    expected: "'='".to_string(),
                    found: token.token.clone(),
                    span: token.span,
                });
            }
        } else {
            return Err(ParseError::UnexpectedEof {
                expected: "'='".to_string(),
            });
        }

        // 解析值表达式
        let value = self.parse_expression()?;

        // 期望分号
        if let Some(token) = self.peek() {
            if matches!(token.token, Token::Semicolon) {
                self.advance();
            } else {
                return Err(ParseError::UnexpectedToken {
                    expected: "';'".to_string(),
                    found: token.token.clone(),
                    span: token.span,
                });
            }
        } else {
            return Err(ParseError::UnexpectedEof {
                expected: "';'".to_string(),
            });
        }

        let end_span = value.span();
        let span = Span::new(start_span.start, end_span.end);

        Ok(Statement::Let { name, value, span })
    }

    /// 解析enum语句
    pub(crate) fn parse_enum_statement(&mut self, is_pub: bool) -> Result<Statement, ParseError> {
        let start_span = self.peek().unwrap().span;
        self.advance(); // consume 'enum'

        // 解析类型名
        let type_name = if let Some(token) = self.peek() {
            if let Token::Identifier(name) = &token.token {
                let name = name.clone();
                self.advance();
                name
            } else {
                return Err(ParseError::UnexpectedToken {
                    expected: "type name".to_string(),
                    found: token.token.clone(),
                    span: token.span,
                });
            }
        } else {
            return Err(ParseError::UnexpectedEof {
                expected: "type name".to_string(),
            });
        };

        // 期望 '{'
        if let Some(token) = self.peek() {
            if matches!(token.token, Token::LeftBrace) {
                self.advance();
            } else {
                return Err(ParseError::UnexpectedToken {
                    expected: "'{'".to_string(),
                    found: token.token.clone(),
                    span: token.span,
                });
            }
        } else {
            return Err(ParseError::UnexpectedEof {
                expected: "'{'".to_string(),
            });
        }

        // 解析变体列表
        let mut variants = Vec::new();
        while let Some(token) = self.peek() {
            if matches!(token.token, Token::RightBrace) {
                break;
            }

            // 解析变体名
            let variant_name = if let Token::Identifier(name) = &token.token {
                let name = name.clone();
                let variant_span = token.span;
                self.advance();

                // 检查是否有数据类型（解析为结构化 Type，支持多参数）
                let data_types = if let Some(next_token) = self.peek() {
                    if matches!(next_token.token, Token::LeftParen) {
                        self.advance(); // consume '('

                        let mut types = vec![];
                        // 解析逗号分隔的类型列表
                        loop {
                            let ty = self.parse_type_expression()?;
                            types.push(ty);

                            if let Some(next) = self.peek() {
                                if matches!(next.token, Token::Comma) {
                                    self.advance(); // consume ','
                                } else if matches!(next.token, Token::RightParen) {
                                    self.advance(); // consume ')'
                                    break;
                                } else {
                                    return Err(ParseError::UnexpectedToken {
                                        expected: "',' or ')'".to_string(),
                                        found: next.token.clone(),
                                        span: next.span,
                                    });
                                }
                            } else {
                                return Err(ParseError::UnexpectedEof {
                                    expected: "',' or ')'".to_string(),
                                });
                            }
                        }
                        types
                    } else {
                        vec![]
                    }
                } else {
                    vec![]
                };

                karte_hir::ast::TypeVariant {
                    name,
                    data_types,
                    span: variant_span,
                }
            } else {
                return Err(ParseError::UnexpectedToken {
                    expected: "variant name".to_string(),
                    found: token.token.clone(),
                    span: token.span,
                });
            };

            variants.push(variant_name);

            // 检查是否有逗号
            if let Some(token) = self.peek() {
                if matches!(token.token, Token::Comma) {
                    self.advance(); // consume ','
                } else if matches!(token.token, Token::RightBrace) {
                    break;
                } else {
                    return Err(ParseError::UnexpectedToken {
                        expected: "',' or '}'".to_string(),
                        found: token.token.clone(),
                        span: token.span,
                    });
                }
            }
        }

        // 期望 '}'
        if let Some(token) = self.peek() {
            if matches!(token.token, Token::RightBrace) {
                let end_span = token.span;
                self.advance();

                // 期望分号（在块表达式中）
                if let Some(token) = self.peek() {
                    if matches!(token.token, Token::Semicolon) {
                        self.advance();
                    }
                    // 注意：不强制要求分号，因为enum可能是程序的最后一个语句
                }

                let span = Span::new(start_span.start, end_span.end);

                Ok(Statement::TypeDef {
                    name: type_name,
                    variants,
                    is_pub,
                    span,
                })
            } else {
                Err(ParseError::UnexpectedToken {
                    expected: "'}'".to_string(),
                    found: token.token.clone(),
                    span: token.span,
                })
            }
        } else {
            Err(ParseError::UnexpectedEof {
                expected: "'}'".to_string(),
            })
        }
    }

    /// 解析struct语句
    pub(crate) fn parse_struct_statement(&mut self, is_pub: bool) -> Result<Statement, ParseError> {
        let start_span = self.peek().unwrap().span;
        self.advance(); // consume 'struct'

        // 解析结构体名
        let struct_name = if let Some(token) = self.peek() {
            if let Token::Identifier(name) = &token.token {
                let name = name.clone();
                self.advance();
                name
            } else {
                return Err(ParseError::UnexpectedToken {
                    expected: "struct name".to_string(),
                    found: token.token.clone(),
                    span: token.span,
                });
            }
        } else {
            return Err(ParseError::UnexpectedEof {
                expected: "struct name".to_string(),
            });
        };

        // 期望 '{'
        if let Some(token) = self.peek() {
            if matches!(token.token, Token::LeftBrace) {
                self.advance();
            } else {
                return Err(ParseError::UnexpectedToken {
                    expected: "'{'".to_string(),
                    found: token.token.clone(),
                    span: token.span,
                });
            }
        } else {
            return Err(ParseError::UnexpectedEof {
                expected: "'{'".to_string(),
            });
        }

        // 解析字段列表
        let mut fields = Vec::new();
        while let Some(token) = self.peek() {
            if matches!(token.token, Token::RightBrace) {
                break;
            }

            // 解析字段名
            let field_name = if let Token::Identifier(name) = &token.token {
                let name = name.clone();
                let field_span = token.span;
                self.advance();

                // 期望 ":"
                if let Some(colon_token) = self.peek() {
                    if matches!(colon_token.token, Token::Colon) {
                        self.advance(); // consume ':'
                    } else {
                        return Err(ParseError::UnexpectedToken {
                            expected: "':'".to_string(),
                            found: colon_token.token.clone(),
                            span: colon_token.span,
                        });
                    }
                } else {
                    return Err(ParseError::UnexpectedEof {
                        expected: "':'".to_string(),
                    });
                }

                // 解析字段类型（支持引用类型）
                let field_type = self.parse_field_type_name()?;

                karte_hir::ast::FieldDef {
                    name,
                    field_type,
                    span: field_span,
                }
            } else {
                return Err(ParseError::UnexpectedToken {
                    expected: "field name".to_string(),
                    found: token.token.clone(),
                    span: token.span,
                });
            };

            fields.push(field_name);

            // 检查是否有逗号
            if let Some(token) = self.peek() {
                if matches!(token.token, Token::Comma) {
                    self.advance(); // consume ','
                } else if matches!(token.token, Token::RightBrace) {
                    break;
                } else {
                    return Err(ParseError::UnexpectedToken {
                        expected: "',' or '}'".to_string(),
                        found: token.token.clone(),
                        span: token.span,
                    });
                }
            }
        }

        // 期望 '}'
        if let Some(token) = self.peek() {
            if matches!(token.token, Token::RightBrace) {
                let end_span = token.span;
                self.advance();

                // 期望分号（在块表达式中）
                if let Some(token) = self.peek() {
                    if matches!(token.token, Token::Semicolon) {
                        self.advance();
                    }
                    // 注意：不强制要求分号，因为struct可能是程序的最后一个语句
                }

                let span = Span::new(start_span.start, end_span.end);

                Ok(Statement::StructDef {
                    name: struct_name,
                    fields,
                    is_pub,
                    span,
                })
            } else {
                Err(ParseError::UnexpectedToken {
                    expected: "'}'".to_string(),
                    found: token.token.clone(),
                    span: token.span,
                })
            }
        } else {
            Err(ParseError::UnexpectedEof {
                expected: "'}'".to_string(),
            })
        }
    }

    /// 解析函数定义
    pub(crate) fn parse_function_definition(&mut self, is_pub: bool) -> Result<Statement, ParseError> {
        let start_span = self.peek().unwrap().span;
        self.advance(); // consume 'fn'

        // 解析函数名
        let name = if let Some(token) = self.peek() {
            if let Token::Identifier(name) = &token.token {
                let name = name.clone();
                self.advance();
                name
            } else {
                return Err(ParseError::UnexpectedToken {
                    expected: "function name".to_string(),
                    found: token.token.clone(),
                    span: token.span,
                });
            }
        } else {
            return Err(ParseError::UnexpectedEof {
                expected: "function name".to_string(),
            });
        };

        // 解析参数列表 (...)
        if let Some(token) = self.peek() {
            if matches!(token.token, Token::LeftParen) {
                self.advance();
            } else {
                return Err(ParseError::UnexpectedToken {
                    expected: "'('".to_string(),
                    found: token.token.clone(),
                    span: token.span,
                });
            }
        } else {
            return Err(ParseError::UnexpectedEof {
                expected: "'('".to_string(),
            });
        }

        let mut params = Vec::new();
        while let Some(token) = self.peek() {
            if matches!(token.token, Token::RightParen) {
                break;
            }

            // 解析参数名
            let param_name = if let Token::Identifier(name) = &token.token {
                let name = name.clone();
                let param_span = token.span;
                self.advance();

                // 检查是否有类型注解（可选）
                let type_annotation = if let Some(colon_token) = self.peek() {
                    if matches!(colon_token.token, Token::Colon) {
                        self.advance(); // consume ':'
                        Some(self.parse_field_type_name()?)
                    } else {
                        None
                    }
                } else {
                    None
                };

                karte_hir::Parameter {
                    name,
                    type_annotation,
                    span: param_span,
                }
            } else {
                return Err(ParseError::UnexpectedToken {
                    expected: "parameter name".to_string(),
                    found: token.token.clone(),
                    span: token.span,
                });
            };

            params.push(param_name);

            // 检查是否有逗号
            if let Some(token) = self.peek() {
                if matches!(token.token, Token::Comma) {
                    self.advance(); // consume ','
                } else if matches!(token.token, Token::RightParen) {
                    break;
                } else {
                    return Err(ParseError::UnexpectedToken {
                        expected: "',' or ')'".to_string(),
                        found: token.token.clone(),
                        span: token.span,
                    });
                }
            }
        }

        // 期望 ')'
        if let Some(token) = self.peek() {
            if matches!(token.token, Token::RightParen) {
                self.advance();
            } else {
                return Err(ParseError::UnexpectedToken {
                    expected: "')'".to_string(),
                    found: token.token.clone(),
                    span: token.span,
                });
            }
        } else {
            return Err(ParseError::UnexpectedEof {
                expected: "')'".to_string(),
            });
        }

        // 解析返回类型 -> Type (可选)
        let return_type = if let Some(token) = self.peek() {
            if matches!(token.token, Token::Arrow) {
                self.advance(); // consume '->'
                Some(self.parse_field_type_name()?)
            } else {
                None
            }
        } else {
            None
        };

        // 解析函数体 { ... }
        let body = self.parse_block_expression()?;
        let end_span = body.span();
        let span = Span::new(start_span.start, end_span.end);

        Ok(Statement::FunctionDef {
            name,
            params,
            return_type,
            body,
            is_pub,
            span,
        })
    }

    /// 解析块表达式 { ... }
    pub(crate) fn parse_block_expression(&mut self) -> Result<Expr, ParseError> {
        let start_span = if let Some(token) = self.peek() {
            if matches!(token.token, Token::LeftBrace) {
                token.span
            } else {
                return Err(ParseError::UnexpectedToken {
                    expected: "'{'".to_string(),
                    found: token.token.clone(),
                    span: token.span,
                });
            }
        } else {
            return Err(ParseError::UnexpectedEof {
                expected: "'{'".to_string(),
            });
        };

        self.advance(); // consume '{'

        let mut statements = Vec::new();
        let mut final_expr = None;

        while let Some(token) = self.peek() {
            if matches!(token.token, Token::RightBrace) {
                break;
            }

            // 检查是否为语句
            if let Token::Identifier(name) = &token.token {
                if matches!(name.as_str(), "let" | "enum" | "struct" | "fn") {
                    statements.push(self.parse_statement()?);
                    continue;
                }
            }

            // 尝试解析表达式
            let expr = self.parse_expression()?;

            // 检查是否有分号
            if let Some(next_token) = self.peek() {
                if matches!(next_token.token, Token::Semicolon) {
                    self.advance(); // consume ';'
                                    // 这是一个表达式语句，加入statements
                    let expr_span = expr.span();
                    statements.push(Statement::Expression {
                        expr,
                        span: expr_span,
                    });
                } else if matches!(next_token.token, Token::RightBrace) {
                    // 没有分号，这是最终表达式
                    final_expr = Some(Box::new(expr));
                    break;
                } else {
                    return Err(ParseError::UnexpectedToken {
                        expected: "';' or '}'".to_string(),
                        found: next_token.token.clone(),
                        span: next_token.span,
                    });
                }
            } else {
                return Err(ParseError::UnexpectedEof {
                    expected: "';' or '}'".to_string(),
                });
            }
        }

        // 期望 '}'
        if let Some(token) = self.peek() {
            if matches!(token.token, Token::RightBrace) {
                let end_span = token.span;
                self.advance(); // consume '}'
                let span = Span::new(start_span.start, end_span.end);
                Ok(Expr::Block {
                    statements,
                    final_expr,
                    span,
                })
            } else {
                Err(ParseError::UnexpectedToken {
                    expected: "'}'".to_string(),
                    found: token.token.clone(),
                    span: token.span,
                })
            }
        } else {
            Err(ParseError::UnexpectedEof {
                expected: "'}'".to_string(),
            })
        }
    }

    /// 解析字段类型名，支持引用类型和泛型类型语法
    pub(crate) fn parse_field_type_name(&mut self) -> Result<Type, ParseError> {
        self.parse_type_expression()
    }

    /// 解析类型表达式，返回结构化 Type
    pub(crate) fn parse_type_expression(&mut self) -> Result<Type, ParseError> {
        if let Some(token) = self.peek() {
            match &token.token {
                Token::Ampersand => {
                    // 引用类型: &TypeName 或 &GenericType<T>
                    self.advance(); // consume '&'
                    let inner_type = self.parse_type_expression()?;
                    Ok(Type::reference(inner_type))
                }
                Token::Identifier(type_name) => {
                    let type_name = type_name.clone();
                    self.advance();

                    // 检查是否有泛型参数
                    if let Some(next_token) = self.peek() {
                        if matches!(next_token.token, Token::Less) {
                            self.advance(); // consume '<'

                            // 解析泛型参数
                            let mut generic_args = Vec::new();

                            loop {
                                let arg_type = self.parse_type_expression()?;
                                generic_args.push(arg_type);

                                if let Some(comma_token) = self.peek() {
                                    if matches!(comma_token.token, Token::Comma) {
                                        self.advance(); // consume ','
                                        continue;
                                    } else if matches!(comma_token.token, Token::Greater) {
                                        self.advance(); // consume '>'
                                        break;
                                    } else {
                                        return Err(ParseError::UnexpectedToken {
                                            expected: "',' or '>'".to_string(),
                                            found: comma_token.token.clone(),
                                            span: comma_token.span,
                                        });
                                    }
                                } else {
                                    return Err(ParseError::UnexpectedEof {
                                        expected: "'>'".to_string(),
                                    });
                                }
                            }

                            // 构造泛型类型
                            match type_name.as_str() {
                                "Option" => {
                                    if generic_args.len() == 1 {
                                        Ok(Type::option(generic_args[0].clone()))
                                    } else {
                                        Ok(Type::Unknown)
                                    }
                                }
                                _ => {
                                    // 其他泛型类型暂不支持
                                    Ok(Type::Unknown)
                                }
                            }
                        } else {
                            // 普通类型名
                            Ok(Self::type_name_to_type(&type_name))
                        }
                    } else {
                        Ok(Self::type_name_to_type(&type_name))
                    }
                }
                _ => Err(ParseError::UnexpectedToken {
                    expected: "type name".to_string(),
                    found: token.token.clone(),
                    span: token.span,
                }),
            }
        } else {
            Err(ParseError::UnexpectedEof {
                expected: "type name".to_string(),
            })
        }
    }

    /// 将类型名称字符串转换为 Type enum
    /// 在 Parser 阶段只做基本映射，自定义类型名创建 Struct 骨架占位
    fn type_name_to_type(name: &str) -> Type {
        match name {
            "number" => Type::Number,
            "string" => Type::string(),
            "unit" => Type::Unit,
            "bool" => Type::bool(),
            "i8" => Type::Int(IntKind::I8),
            "i16" => Type::Int(IntKind::I16),
            "i32" => Type::Int(IntKind::I32),
            "i64" => Type::Int(IntKind::I64),
            "u8" => Type::Int(IntKind::U8),
            "u16" => Type::Int(IntKind::U16),
            "u32" => Type::Int(IntKind::U32),
            "u64" => Type::Int(IntKind::U64),
            "usize" => Type::Int(IntKind::USize),
            _ => {
                // 自定义类型名（struct/enum），创建 Struct 骨架作为占位符
                // type_checker 会在 process_struct_definitions 中替换为实际类型
                // 如果实际是 enum 类型，type_checker 也会正确处理
                Type::struct_type(name.to_string(), vec![])
            }
        }
    }

    /// 检查接下来的token是否看起来像struct字面量
    /// 通过向前看来判断 { 后面是否跟着 identifier: value 模式
    pub(crate) fn is_likely_struct_literal(&self) -> bool {
        // 向前看，跳过当前的 '{'
        if self.position + 1 < self.tokens.len() {
            // 检查 { 后面的第一个token
            if let Some(first_token) = self.tokens.get(self.position + 1) {
                match &first_token.token {
                    Token::Identifier(_) => {
                        // 如果第一个token是标识符，检查它后面是否是 ':'
                        if self.position + 2 < self.tokens.len() {
                            if let Some(second_token) = self.tokens.get(self.position + 2) {
                                matches!(second_token.token, Token::Colon)
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    }
                    Token::RightBrace => {
                        // 空的 {} 也可能是struct字面量
                        true
                    }
                    _ => {
                        // 其他token（如 _、数字、match等）不是struct字面量
                        false
                    }
                }
            } else {
                false
            }
        } else {
            false
        }
    }
}
