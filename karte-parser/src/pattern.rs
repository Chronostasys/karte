//! 模式匹配解析
//!
//! 本模块包含解析Karte模式匹配语法的功能，支持以下模式：
//! - 通配符模式（`_`）
//! - 数字字面量模式
//! - 布尔字面量模式（`true`、`false`）
//! - 变量模式
//! - 构造器模式（如 `Some(x)`）
//! - 限定构造器模式（如 `Option::Some(x)`）
//!
//! 模式在match表达式中使用，进行模式匹配和解构。

use crate::types::ParseError;
use crate::Parser;
use karte_diagnostics::Span;
use karte_lexer::Token;

impl<'a> Parser<'a> {
    /// 解析模式
    ///
    /// 支持的模式类型：
    /// - `_`: 通配符，匹配任何值
    /// - `42`: 数字字面量
    /// - `true`/`false`: 布尔字面量
    /// - `x`: 变量绑定
    /// - `Some(x)`: 构造器模式
    /// - `Option::Some(x)`: 限定构造器模式
    pub(crate) fn parse_pattern(&mut self) -> Result<karte_hir::Pattern, ParseError> {
        if let Some(token) = self.peek() {
            match &token.token {
                Token::Underscore => {
                    let span = token.span;
                    self.advance();
                    Ok(karte_hir::Pattern::Wildcard { span })
                }
                Token::Number(value) => {
                    let value = *value;
                    let span = token.span;
                    self.advance();
                    Ok(karte_hir::Pattern::Number { value, span })
                }
                Token::CharLiteral(value) => {
                    let value = *value;
                    let span = token.span;
                    self.advance();
                    Ok(karte_hir::Pattern::Number { value, span })
                }
                Token::Identifier(name) => {
                    let name = name.clone();
                    let span = token.span;
                    self.advance();

                    if name == "true" {
                        Ok(karte_hir::Pattern::Boolean { value: true, span })
                    } else if name == "false" {
                        Ok(karte_hir::Pattern::Boolean { value: false, span })
                    } else {
                        // 检查是否为限定构造器模式 TypeName::Constructor
                        if let Some(next_token) = self.peek() {
                            if matches!(next_token.token, Token::DoubleColon) {
                                self.parse_qualified_constructor_pattern(name, span)
                            } else if matches!(next_token.token, Token::LeftParen) {
                                self.parse_constructor_pattern(name, span)
                            } else if matches!(next_token.token, Token::LeftBrace) {
                                // 大写字母开头 + { → 结构体解构模式
                                if name.chars().next().map_or(false, |c| c.is_uppercase()) {
                                    self.parse_struct_pattern(name, span)
                                } else {
                                    Err(ParseError::UnexpectedToken {
                                        expected: "identifier".to_string(),
                                        found: next_token.token.clone(),
                                        span: next_token.span,
                                    })
                                }
                            } else {
                                // 大写字母开头的标识符视为零参数构造器模式
                                // （枚举变体如 Red、Green、Blue），小写字母开头视为变量绑定
                                if name.chars().next().map_or(false, |c| c.is_uppercase()) {
                                    Ok(karte_hir::Pattern::Constructor {
                                        name,
                                        args: vec![],
                                        span,
                                    })
                                } else {
                                    Ok(karte_hir::Pattern::Variable { name, span })
                                }
                            }
                        } else {
                            // 大写字母开头的标识符视为零参数构造器模式
                            if name.chars().next().map_or(false, |c| c.is_uppercase()) {
                                Ok(karte_hir::Pattern::Constructor {
                                    name,
                                    args: vec![],
                                    span,
                                })
                            } else {
                                Ok(karte_hir::Pattern::Variable { name, span })
                            }
                        }
                    }
                }
                Token::Minus => {
                    let minus_span = token.span;
                    self.advance();
                    if let Some(next_token) = self.peek() {
                        if let Token::Number(value) = &next_token.token {
                            let value = *value;
                            let end_span = next_token.span;
                            self.advance();
                            // 处理 i64::MIN 的特殊情况：lexer 将 9223372036854775808 解析为 i64::MIN
                            // 此时 value 已经是负值，不应再取反
                            let neg_value = if value == i64::MIN {
                                value
                            } else {
                                -value
                            };
                            let span = Span::new(minus_span.start, end_span.end);
                            Ok(karte_hir::Pattern::Number { value: neg_value, span })
                        } else {
                            Err(ParseError::UnexpectedToken {
                                expected: "number after '-'".to_string(),
                                found: next_token.token.clone(),
                                span: next_token.span,
                            })
                        }
                    } else {
                        Err(ParseError::UnexpectedEof {
                            expected: "number after '-'".to_string(),
                        })
                    }
                }
                _ => Err(ParseError::UnexpectedToken {
                    expected: "pattern".to_string(),
                    found: token.token.clone(),
                    span: token.span,
                }),
            }
        } else {
            Err(ParseError::UnexpectedEof {
                expected: "pattern".to_string(),
            })
        }
    }

    /// 解析限定构造器模式（如 `Option::Some(x)`）
    fn parse_qualified_constructor_pattern(
        &mut self,
        type_name: String,
        start_span: Span,
    ) -> Result<karte_hir::Pattern, ParseError> {
        self.advance(); // consume '::'

        // 期望构造器名
        if let Some(constructor_token) = self.peek() {
            if let Token::Identifier(constructor_name) = &constructor_token.token {
                let constructor_name = constructor_name.clone();
                let constructor_span = constructor_token.span;
                self.advance();

                // 检查是否有参数模式
                if let Some(arg_token) = self.peek() {
                    if matches!(arg_token.token, Token::LeftParen) {
                        self.advance(); // consume '('
                        let mut args = Vec::new();
                        if let Some(peeked) = self.peek() {
                            if !matches!(peeked.token, Token::RightParen) {
                                args.push(self.parse_pattern()?);
                                while let Some(next) = self.peek() {
                                    if matches!(next.token, Token::Comma) {
                                        self.advance();
                                        args.push(self.parse_pattern()?);
                                    } else {
                                        break;
                                    }
                                }
                            }
                        } else {
                            return Err(ParseError::UnexpectedEof {
                                expected: "pattern or ')'".to_string(),
                            });
                        };

                        if let Some(close_token) = self.peek() {
                            if matches!(close_token.token, Token::RightParen) {
                                let end_span = close_token.span;
                                self.advance(); // consume ')'
                                let full_span = Span::new(start_span.start, end_span.end);
                                Ok(karte_hir::Pattern::QualifiedConstructor {
                                    type_name,
                                    constructor_name,
                                    args,
                                    span: full_span,
                                })
                            } else {
                                Err(ParseError::UnexpectedToken {
                                    expected: "')'".to_string(),
                                    found: close_token.token.clone(),
                                    span: close_token.span,
                                })
                            }
                        } else {
                            Err(ParseError::UnexpectedEof {
                                expected: "')'".to_string(),
                            })
                        }
                    } else {
                        // 无参数限定构造器模式
                        let full_span = Span::new(start_span.start, constructor_span.end);
                        Ok(karte_hir::Pattern::QualifiedConstructor {
                            type_name,
                            constructor_name,
                            args: vec![],
                            span: full_span,
                        })
                    }
                } else {
                    // 无参数限定构造器模式
                    let full_span = Span::new(start_span.start, constructor_span.end);
                    Ok(karte_hir::Pattern::QualifiedConstructor {
                        type_name,
                        constructor_name,
                        args: vec![],
                        span: full_span,
                    })
                }
            } else {
                Err(ParseError::UnexpectedToken {
                    expected: "constructor name".to_string(),
                    found: constructor_token.token.clone(),
                    span: constructor_token.span,
                })
            }
        } else {
            Err(ParseError::UnexpectedEof {
                expected: "constructor name".to_string(),
            })
        }
    }

    /// 解析构造器模式（如 `Some(x)`）
    fn parse_constructor_pattern(
        &mut self,
        name: String,
        start_span: Span,
    ) -> Result<karte_hir::Pattern, ParseError> {
        self.advance(); // consume '('
        let mut args = Vec::new();
        if let Some(peeked) = self.peek() {
            if !matches!(peeked.token, Token::RightParen) {
                args.push(self.parse_pattern()?);
                while let Some(next) = self.peek() {
                    if matches!(next.token, Token::Comma) {
                        self.advance();
                        args.push(self.parse_pattern()?);
                    } else {
                        break;
                    }
                }
            }
        } else {
            return Err(ParseError::UnexpectedEof {
                expected: "pattern or ')'".to_string(),
            });
        };

        if let Some(token) = self.peek() {
            if matches!(token.token, Token::RightParen) {
                let end_span = token.span;
                self.advance(); // consume ')'
                let full_span = Span::new(start_span.start, end_span.end);
                Ok(karte_hir::Pattern::Constructor {
                    name,
                    args,
                    span: full_span,
                })
            } else {
                Err(ParseError::UnexpectedToken {
                    expected: "')'".to_string(),
                    found: token.token.clone(),
                    span: token.span,
                })
            }
        } else {
            Err(ParseError::UnexpectedEof {
                expected: "')'".to_string(),
            })
        }
    }

    /// 解析结构体解构模式（如 `Point { x, y }` 或 `Point { x: a, y: 0 }`）
    pub(crate) fn parse_struct_pattern(
        &mut self,
        name: String,
        start_span: Span,
    ) -> Result<karte_hir::Pattern, ParseError> {
        // consume {
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

        let mut fields = Vec::new();
        loop {
            // 检查是否到达 }
            if let Some(token) = self.peek() {
                if matches!(token.token, Token::RightBrace) {
                    let end_span = token.span;
                    self.advance();
                    let span = Span::new(start_span.start, end_span.end);
                    return Ok(karte_hir::Pattern::Struct {
                        name,
                        fields,
                        span,
                    });
                }
            } else {
                return Err(ParseError::UnexpectedEof {
                    expected: "'}'".to_string(),
                });
            }

            // 解析字段名
            if let Some(token) = self.peek() {
                if let Token::Identifier(id) = &token.token {
                    let id = id.clone();
                    let field_span = token.span;
                    self.advance();

                    // 检查后面是否是 : （字段重命名或字面量模式）
                    if let Some(next) = self.peek() {
                        if matches!(next.token, Token::Colon) {
                            self.advance(); // consume :
                            let pattern = self.parse_pattern()?;
                            fields.push(karte_hir::StructFieldPattern {
                                field: id,
                                pattern: Box::new(pattern),
                                span: field_span,
                            });
                        } else {
                            // shorthand: 字段名同时也是绑定变量名
                            let bind_name = id.clone();
                            fields.push(karte_hir::StructFieldPattern {
                                field: id,
                                pattern: Box::new(karte_hir::Pattern::Variable {
                                    name: bind_name,
                                    span: field_span,
                                }),
                                span: field_span,
                            });
                        }
                    } else {
                        return Err(ParseError::UnexpectedEof {
                            expected: "':' or ',' or '}'".to_string(),
                        });
                    }
                } else {
                    return Err(ParseError::UnexpectedToken {
                        expected: "field name".to_string(),
                        found: token.token.clone(),
                        span: token.span,
                    });
                }
            } else {
                return Err(ParseError::UnexpectedEof {
                    expected: "field name".to_string(),
                });
            }

            // 检查逗号分隔符
            if let Some(token) = self.peek() {
                if matches!(token.token, Token::Comma) {
                    self.advance(); // consume ,
                    // 尾随逗号后可以是 }
                    continue;
                }
                // 不是逗号也没关系，可能是 }
            }
        }
    }
}