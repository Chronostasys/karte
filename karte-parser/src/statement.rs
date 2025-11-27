//! 语句和声明解析模块
//!
//! 本模块包含所有与语句和声明解析相关的功能，包括：
//! - 语句解析（let、enum、struct、fn等）
//! - 块表达式解析
//! - 类型表达式解析
//! - 程序结构解析

use karte_diagnostics::Span;
use karte_hir::{Expr, Statement};
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

                // 检查是否为赋值语句 (identifier = ...)
                if self.position + 1 < self.tokens.len() {
                    if let Some(next_token) = self.tokens.get(self.position + 1) {
                        if matches!(next_token.token, Token::Equal) {
                            // 这是一个赋值语句
                            statements.push(self.parse_statement()?);
                            continue;
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
                    return self.parse_enum_statement();
                } else if name == "struct" {
                    return self.parse_struct_statement();
                } else if name == "fn" {
                    return self.parse_function_definition();
                }
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
    pub(crate) fn parse_enum_statement(&mut self) -> Result<Statement, ParseError> {
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

                // 检查是否有数据类型 (暂时只支持单个类型名)
                let data_type = if let Some(next_token) = self.peek() {
                    if matches!(next_token.token, Token::LeftParen) {
                        self.advance(); // consume '('

                        if let Some(type_token) = self.peek() {
                            if let Token::Identifier(type_name) = &type_token.token {
                                let type_name = type_name.clone();
                                self.advance();

                                // 期望 ')'
                                if let Some(close_token) = self.peek() {
                                    if matches!(close_token.token, Token::RightParen) {
                                        self.advance();
                                        Some(type_name)
                                    } else {
                                        return Err(ParseError::UnexpectedToken {
                                            expected: "')'".to_string(),
                                            found: close_token.token.clone(),
                                            span: close_token.span,
                                        });
                                    }
                                } else {
                                    return Err(ParseError::UnexpectedEof {
                                        expected: "')'".to_string(),
                                    });
                                }
                            } else {
                                return Err(ParseError::UnexpectedToken {
                                    expected: "type name".to_string(),
                                    found: type_token.token.clone(),
                                    span: type_token.span,
                                });
                            }
                        } else {
                            return Err(ParseError::UnexpectedEof {
                                expected: "type name".to_string(),
                            });
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };

                karte_hir::ast::TypeVariant {
                    name,
                    data_type,
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
    pub(crate) fn parse_struct_statement(&mut self) -> Result<Statement, ParseError> {
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
    pub(crate) fn parse_function_definition(&mut self) -> Result<Statement, ParseError> {
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

                // 解析参数类型
                let param_type = self.parse_field_type_name()?;

                karte_hir::Parameter {
                    name,
                    type_annotation: Some(param_type),
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
    pub(crate) fn parse_field_type_name(&mut self) -> Result<String, ParseError> {
        self.parse_type_expression()
    }

    /// 解析类型表达式，支持嵌套的泛型类型
    pub(crate) fn parse_type_expression(&mut self) -> Result<String, ParseError> {
        if let Some(token) = self.peek() {
            match &token.token {
                Token::Ampersand => {
                    // 引用类型: &TypeName 或 &GenericType<T>
                    self.advance(); // consume '&'
                    let inner_type = self.parse_type_expression()?;
                    Ok(format!("&{}", inner_type))
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
                                // 解析一个泛型参数
                                let arg_type = self.parse_type_expression()?;
                                generic_args.push(arg_type);

                                // 检查是否有更多参数
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

                            // 构造泛型类型字符串
                            Ok(format!("{}<{}>", type_name, generic_args.join(", ")))
                        } else {
                            // 普通类型名
                            Ok(type_name)
                        }
                    } else {
                        Ok(type_name)
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
