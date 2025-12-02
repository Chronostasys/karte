//! 表达式解析模块
//!
//! 本模块负责解析所有类型的表达式，包括：
//! - 赋值表达式
//! - 逻辑运算表达式（&&, ||）
//! - 比较运算表达式（==, !=, <, >, <=, >=）
//! - 算术运算表达式（+, -, *, /）
//! - 一元运算表达式（+, -, !, &, *）
//! - 主表达式（数字、标识符、lambda、括号表达式等）
//! - 控制流表达式（if, while, match）
//! - 复合表达式（函数调用、字段访问、数组下标等）
//!
//! ## 运算符优先级（从低到高）
//! 1. 赋值 (=)
//! 2. 逻辑或 (||)
//! 3. 逻辑与 (&&)
//! 4. 比较 (==, !=, <, >, <=, >=)
//! 5. 加减 (+, -)
//! 6. 乘除 (*, /)
//! 7. 一元运算 (+, -, !, &, *, box, free, arc, retain, release, len)
//! 8. 后缀运算（函数调用、字段访问、数组下标）
//! 9. 主表达式（字面量、标识符、括号表达式等）

use karte_common::memory::OwnershipKind;
use karte_diagnostics::Span;
use karte_hir::{BinaryOperator, Expr, UnaryOperator};
use karte_lexer::Token;

use crate::types::ParseError;
use crate::Parser;

impl<'a> Parser<'a> {
    // expression = assignment
    pub(crate) fn parse_expression(&mut self) -> Result<Expr, ParseError> {
        self.parse_assignment()
    }

    // assignment = logical_or ('=' assignment)?
    pub(crate) fn parse_assignment(&mut self) -> Result<Expr, ParseError> {
        let expr = self.parse_logical_or()?;

        if let Some(token) = self.peek() {
            if matches!(token.token, Token::Equal) {
                self.advance(); // consume '='
                let value = self.parse_assignment()?; // 右结合
                let span = Span::new(expr.span().start, value.span().end);
                return Ok(Expr::Assignment {
                    target: Box::new(expr),
                    value: Box::new(value),
                    span,
                });
            }
        }

        Ok(expr)
    }

    // logical_or = logical_and ('||' logical_and)*
    pub(crate) fn parse_logical_or(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_logical_and()?;

        while let Some(token) = self.peek() {
            if matches!(token.token, Token::LogicalOr) {
                self.advance();
                let right = self.parse_logical_and()?;
                let span = Span::new(left.span().start, right.span().end);
                left = Expr::BinaryOp {
                    left: Box::new(left),
                    op: BinaryOperator::LogicalOr,
                    right: Box::new(right),
                    span,
                };
            } else {
                break;
            }
        }

        Ok(left)
    }

    // logical_and = comparison ('&&' comparison)*
    pub(crate) fn parse_logical_and(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_comparison()?;

        while let Some(token) = self.peek() {
            if matches!(token.token, Token::LogicalAnd) {
                self.advance();
                let right = self.parse_comparison()?;
                let span = Span::new(left.span().start, right.span().end);
                left = Expr::BinaryOp {
                    left: Box::new(left),
                    op: BinaryOperator::LogicalAnd,
                    right: Box::new(right),
                    span,
                };
            } else {
                break;
            }
        }

        Ok(left)
    }

    // comparison = additive (('==' | '>=' | '<=' | '>' | '<') additive)*
    pub(crate) fn parse_comparison(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_additive()?;

        while let Some(token) = self.peek() {
            match token.token {
                Token::EqualEqual => {
                    self.advance();
                    let right = self.parse_additive()?;
                    let span = Span::new(left.span().start, right.span().end);
                    left = Expr::BinaryOp {
                        left: Box::new(left),
                        op: BinaryOperator::Equal,
                        right: Box::new(right),
                        span,
                    };
                }
                Token::GreaterEqual => {
                    self.advance();
                    let right = self.parse_additive()?;
                    let span = Span::new(left.span().start, right.span().end);
                    left = Expr::BinaryOp {
                        left: Box::new(left),
                        op: BinaryOperator::GreaterEqual,
                        right: Box::new(right),
                        span,
                    };
                }
                Token::LessEqual => {
                    self.advance();
                    let right = self.parse_additive()?;
                    let span = Span::new(left.span().start, right.span().end);
                    left = Expr::BinaryOp {
                        left: Box::new(left),
                        op: BinaryOperator::LessEqual,
                        right: Box::new(right),
                        span,
                    };
                }
                Token::Greater => {
                    self.advance();
                    let right = self.parse_additive()?;
                    let span = Span::new(left.span().start, right.span().end);
                    left = Expr::BinaryOp {
                        left: Box::new(left),
                        op: BinaryOperator::Greater,
                        right: Box::new(right),
                        span,
                    };
                }
                Token::Less => {
                    self.advance();
                    let right = self.parse_additive()?;
                    let span = Span::new(left.span().start, right.span().end);
                    left = Expr::BinaryOp {
                        left: Box::new(left),
                        op: BinaryOperator::Less,
                        right: Box::new(right),
                        span,
                    };
                }
                _ => break,
            }
        }

        Ok(left)
    }

    // additive = term (('+' | '-') term)*
    pub(crate) fn parse_additive(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_term()?;

        while let Some(token) = self.peek() {
            match token.token {
                Token::Plus => {
                    self.advance();
                    let right = self.parse_term()?;
                    let span = Span::new(left.span().start, right.span().end);
                    left = Expr::BinaryOp {
                        left: Box::new(left),
                        op: BinaryOperator::Add,
                        right: Box::new(right),
                        span,
                    };
                }
                Token::Minus => {
                    self.advance();
                    let right = self.parse_term()?;
                    let span = Span::new(left.span().start, right.span().end);
                    left = Expr::BinaryOp {
                        left: Box::new(left),
                        op: BinaryOperator::Subtract,
                        right: Box::new(right),
                        span,
                    };
                }
                _ => break,
            }
        }

        Ok(left)
    }

    // term = factor (('*' | '/') factor)*
    pub(crate) fn parse_term(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_factor()?;

        while let Some(token) = self.peek() {
            match token.token {
                Token::Multiply => {
                    self.advance();
                    let right = self.parse_factor()?;
                    let span = Span::new(left.span().start, right.span().end);
                    left = Expr::BinaryOp {
                        left: Box::new(left),
                        op: BinaryOperator::Multiply,
                        right: Box::new(right),
                        span,
                    };
                }
                Token::Divide => {
                    self.advance();
                    let right = self.parse_factor()?;
                    let span = Span::new(left.span().start, right.span().end);
                    left = Expr::BinaryOp {
                        left: Box::new(left),
                        op: BinaryOperator::Divide,
                        right: Box::new(right),
                        span,
                    };
                }
                _ => break,
            }
        }

        Ok(left)
    }

    // factor = ('+' | '-' | '&' | '*' | '!' | 'box' | 'free')? primary
    pub(crate) fn parse_factor(&mut self) -> Result<Expr, ParseError> {
        if let Some(token) = self.peek() {
            if let Token::Identifier(name) = &token.token {
                if name == "box" {
                    let start_span = token.span;
                    self.advance(); // consume 'box'
                    let value = self.parse_primary()?;
                    let span = Span::new(start_span.start, value.span().end);
                    return Ok(Expr::HeapAllocate {
                        value: Box::new(value),
                        ownership: OwnershipKind::Manual,
                        span,
                    });
                } else if name == "arc" {
                    let start_span = token.span;
                    self.advance(); // consume 'arc'
                    let value = self.parse_primary()?;
                    let span = Span::new(start_span.start, value.span().end);
                    return Ok(Expr::HeapAllocate {
                        value: Box::new(value),
                        ownership: OwnershipKind::RefCounted,
                        span,
                    });
                } else if name == "free" {
                    let start_span = token.span;
                    self.advance(); // consume 'free'
                    let pointer = self.parse_primary()?;
                    let span = Span::new(start_span.start, pointer.span().end);
                    return Ok(Expr::HeapFree {
                        pointer: Box::new(pointer),
                        span,
                    });
                } else if name == "retain" {
                    let start_span = token.span;
                    self.advance();
                    let pointer = self.parse_primary()?;
                    let span = Span::new(start_span.start, pointer.span().end);
                    return Ok(Expr::Retain {
                        pointer: Box::new(pointer),
                        span,
                    });
                } else if name == "release" {
                    let start_span = token.span;
                    self.advance();
                    let pointer = self.parse_primary()?;
                    let span = Span::new(start_span.start, pointer.span().end);
                    return Ok(Expr::Release {
                        pointer: Box::new(pointer),
                        span,
                    });
                } else if name == "len" {
                    let start_span = token.span;
                    self.advance(); // consume 'len'
                    let array = self.parse_primary()?;
                    let span = Span::new(start_span.start, array.span().end);
                    return Ok(Expr::ArrayLen {
                        array: Box::new(array),
                        span,
                    });
                }
            }

            match token.token {
                Token::Plus => {
                    let op_span = token.span;
                    self.advance();
                    let operand = self.parse_primary()?;
                    let span = Span::new(op_span.start, operand.span().end);
                    Ok(Expr::UnaryOp {
                        op: UnaryOperator::Plus,
                        operand: Box::new(operand),
                        span,
                    })
                }
                Token::Minus => {
                    let op_span = token.span;
                    self.advance();
                    let operand = self.parse_primary()?;
                    let span = Span::new(op_span.start, operand.span().end);
                    Ok(Expr::UnaryOp {
                        op: UnaryOperator::Minus,
                        operand: Box::new(operand),
                        span,
                    })
                }
                Token::LogicalNot => {
                    let op_span = token.span;
                    self.advance();
                    let operand = self.parse_primary()?;
                    let span = Span::new(op_span.start, operand.span().end);
                    Ok(Expr::UnaryOp {
                        op: UnaryOperator::LogicalNot,
                        operand: Box::new(operand),
                        span,
                    })
                }
                Token::Ampersand => {
                    let op_span = token.span;
                    self.advance();
                    let expr = self.parse_primary()?;
                    let span = Span::new(op_span.start, expr.span().end);
                    Ok(Expr::Reference {
                        expr: Box::new(expr),
                        span,
                    })
                }
                Token::Multiply => {
                    let op_span = token.span;
                    self.advance();
                    let expr = self.parse_primary()?;
                    let span = Span::new(op_span.start, expr.span().end);
                    Ok(Expr::Dereference {
                        expr: Box::new(expr),
                        span,
                    })
                }
                _ => self.parse_primary(),
            }
        } else {
            Err(ParseError::UnexpectedEof {
                expected: "expression".to_string(),
            })
        }
    }

    // primary = NUMBER | IDENTIFIER | 关键字(perf/resume/handle) | LAMBDA | '(' expression ')' | function_call
    pub(crate) fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        let mut expr = if let Some(token) = self.peek() {
            match &token.token {
                Token::Number(value) => {
                    let value = *value;
                    let span = token.span;
                    self.advance();
                    Ok(Expr::Number { value, span })
                }
                Token::KwPerform => {
                    let span = token.span;
                    self.advance(); // consume 'perform'
                                    // 解析tag：为避免将后续括号吞为函数调用，这里优先仅接受简单标识符或字面量
                    let tag_expr = if let Some(tok) = self.peek() {
                        match &tok.token {
                            Token::Identifier(name) => {
                                let name2 = name.clone();
                                let s = tok.span;
                                self.advance();
                                Expr::Identifier {
                                    name: name2,
                                    span: s,
                                }
                            }
                            Token::Number(n) => {
                                let s = tok.span;
                                let v = *n;
                                self.advance();
                                Expr::Number { value: v, span: s }
                            }
                            _ => {
                                // 回退到通用表达式（可能仍会吞括号为调用，但覆盖常见用例）
                                self.parse_expression()?
                            }
                        }
                    } else {
                        return Err(ParseError::UnexpectedEof {
                            expected: "tag expression".to_string(),
                        });
                    };
                    if let Some(tok) = self.peek() {
                        if matches!(tok.token, Token::LeftParen) {
                            self.advance();
                        } else {
                            return Err(ParseError::UnexpectedToken {
                                expected: "'('".to_string(),
                                found: tok.token.clone(),
                                span: tok.span,
                            });
                        }
                    } else {
                        return Err(ParseError::UnexpectedEof {
                            expected: "'('".to_string(),
                        });
                    }
                    let payload_opt = if let Some(tok) = self.peek() {
                        if matches!(tok.token, Token::RightParen) {
                            None
                        } else {
                            Some(self.parse_expression()?)
                        }
                    } else {
                        return Err(ParseError::UnexpectedEof {
                            expected: ")".to_string(),
                        });
                    };
                    if let Some(tok) = self.peek() {
                        if matches!(tok.token, Token::RightParen) {
                            let end_span = tok.span;
                            self.advance();
                            let full_span = Span::new(span.start, end_span.end);
                            Ok(Expr::EffectPerform {
                                tag: Box::new(tag_expr),
                                payload: Box::new(
                                    payload_opt.unwrap_or(Expr::Unit { span: end_span }),
                                ),
                                span: full_span,
                            })
                        } else {
                            Err(ParseError::UnexpectedToken {
                                expected: ")".to_string(),
                                found: tok.token.clone(),
                                span: tok.span,
                            })
                        }
                    } else {
                        Err(ParseError::UnexpectedEof {
                            expected: ")".to_string(),
                        })
                    }
                }
                Token::KwResume => {
                    let span = token.span;
                    self.advance(); // consume 'resume'
                    if let Some(tok) = self.peek() {
                        if matches!(tok.token, Token::LeftParen) {
                            self.advance();
                        } else {
                            return Err(ParseError::UnexpectedToken {
                                expected: "'('".to_string(),
                                found: tok.token.clone(),
                                span: tok.span,
                            });
                        }
                    } else {
                        return Err(ParseError::UnexpectedEof {
                            expected: "'('".to_string(),
                        });
                    }
                    let value_expr = self.parse_expression()?;
                    if let Some(tok) = self.peek() {
                        if matches!(tok.token, Token::RightParen) {
                            let end_span = tok.span;
                            self.advance();
                            let full_span = Span::new(span.start, end_span.end);
                            Ok(Expr::EffectResume {
                                value: Box::new(value_expr),
                                span: full_span,
                            })
                        } else {
                            Err(ParseError::UnexpectedToken {
                                expected: ")".to_string(),
                                found: tok.token.clone(),
                                span: tok.span,
                            })
                        }
                    } else {
                        Err(ParseError::UnexpectedEof {
                            expected: ")".to_string(),
                        })
                    }
                }
                Token::KwHandle => {
                    let span = token.span;
                    self.advance(); // consume 'handle'
                                    // 同 perform：优先解析简单标识符/数字作为tag
                    let tag_expr = if let Some(tok) = self.peek() {
                        match &tok.token {
                            Token::Identifier(name) => {
                                let s = tok.span;
                                let name2 = name.clone();
                                self.advance();
                                Expr::Identifier {
                                    name: name2,
                                    span: s,
                                }
                            }
                            Token::Number(n) => {
                                let s = tok.span;
                                let v = *n;
                                self.advance();
                                Expr::Number { value: v, span: s }
                            }
                            _ => self.parse_expression()?,
                        }
                    } else {
                        return Err(ParseError::UnexpectedEof {
                            expected: "tag expression".to_string(),
                        });
                    };
                    if let Some(tok) = self.peek() {
                        if matches!(tok.token, Token::LeftParen) {
                            self.advance();
                        } else {
                            return Err(ParseError::UnexpectedToken {
                                expected: "'('".to_string(),
                                found: tok.token.clone(),
                                span: tok.span,
                            });
                        }
                    } else {
                        return Err(ParseError::UnexpectedEof {
                            expected: "'('".to_string(),
                        });
                    }
                    let param_name = if let Some(tok) = self.peek() {
                        if let Token::Identifier(s) = &tok.token {
                            let s2 = s.clone();
                            self.advance();
                            s2
                        } else {
                            return Err(ParseError::UnexpectedToken {
                                expected: "identifier".to_string(),
                                found: tok.token.clone(),
                                span: tok.span,
                            });
                        }
                    } else {
                        return Err(ParseError::UnexpectedEof {
                            expected: "identifier".to_string(),
                        });
                    };
                    if let Some(tok) = self.peek() {
                        if matches!(tok.token, Token::RightParen) {
                            self.advance();
                        } else {
                            return Err(ParseError::UnexpectedToken {
                                expected: ")".to_string(),
                                found: tok.token.clone(),
                                span: tok.span,
                            });
                        }
                    } else {
                        return Err(ParseError::UnexpectedEof {
                            expected: ")".to_string(),
                        });
                    }
                    if let Some(tok) = self.peek() {
                        if matches!(tok.token, Token::LeftBrace) {
                            self.advance();
                        } else {
                            return Err(ParseError::UnexpectedToken {
                                expected: "'{'".to_string(),
                                found: tok.token.clone(),
                                span: tok.span,
                            });
                        }
                    } else {
                        return Err(ParseError::UnexpectedEof {
                            expected: "'{'".to_string(),
                        });
                    }
                    let handler_expr = self.parse_expression()?;
                    if let Some(tok) = self.peek() {
                        if matches!(tok.token, Token::RightBrace) {
                            self.advance();
                        } else {
                            return Err(ParseError::UnexpectedToken {
                                expected: "'}'".to_string(),
                                found: tok.token.clone(),
                                span: tok.span,
                            });
                        }
                    } else {
                        return Err(ParseError::UnexpectedEof {
                            expected: "'}'".to_string(),
                        });
                    }
                    if let Some(tok) = self.peek() {
                        match &tok.token {
                            Token::KwIn => {
                                self.advance();
                            }
                            Token::Identifier(id) if id == "in" => {
                                self.advance();
                            }
                            _ => {
                                return Err(ParseError::UnexpectedToken {
                                    expected: "in".to_string(),
                                    found: tok.token.clone(),
                                    span: tok.span,
                                });
                            }
                        }
                    } else {
                        return Err(ParseError::UnexpectedEof {
                            expected: "in".to_string(),
                        });
                    }
                    let body_expr = self.parse_expression()?;
                    let full_span = Span::new(span.start, body_expr.span().end);
                    Ok(Expr::EffectHandle {
                        tag: Box::new(tag_expr),
                        param: param_name,
                        handler: Box::new(handler_expr),
                        body: Box::new(body_expr),
                        span: full_span,
                    })
                }
                Token::Identifier(name) => {
                    let name = name.clone();
                    let span = token.span;
                    self.advance();

                    // 检查是否为布尔字面量
                    if name == "true" {
                        Ok(Expr::Boolean { value: true, span })
                    } else if name == "false" {
                        Ok(Expr::Boolean { value: false, span })
                    } else if name == "match" {
                        // 回退并解析match表达式
                        self.position -= 1;
                        self.parse_match()
                    } else if name == "if" {
                        // 回退并解析if表达式
                        self.position -= 1;
                        self.parse_if()
                    } else if name == "while" {
                        // 回退并解析while表达式
                        self.position -= 1;
                        self.parse_while()
                    } else if name == "perform" {
                        let tag_expr = self.parse_expression()?;
                        if let Some(tok) = self.peek() {
                            if matches!(tok.token, Token::LeftParen) {
                                self.advance();
                            } else {
                                return Err(ParseError::UnexpectedToken {
                                    expected: "'('".to_string(),
                                    found: tok.token.clone(),
                                    span: tok.span,
                                });
                            }
                        } else {
                            return Err(ParseError::UnexpectedEof {
                                expected: "'('".to_string(),
                            });
                        }
                        let payload_opt = if let Some(tok) = self.peek() {
                            if matches!(tok.token, Token::RightParen) {
                                None
                            } else {
                                Some(self.parse_expression()?)
                            }
                        } else {
                            return Err(ParseError::UnexpectedEof {
                                expected: ")".to_string(),
                            });
                        };
                        if let Some(tok) = self.peek() {
                            if matches!(tok.token, Token::RightParen) {
                                let end_span = tok.span;
                                self.advance();
                                let full_span = Span::new(span.start, end_span.end);
                                Ok(Expr::EffectPerform {
                                    tag: Box::new(tag_expr),
                                    payload: Box::new(
                                        payload_opt.unwrap_or(Expr::Unit { span: end_span }),
                                    ),
                                    span: full_span,
                                })
                            } else {
                                Err(ParseError::UnexpectedToken {
                                    expected: ")".to_string(),
                                    found: tok.token.clone(),
                                    span: tok.span,
                                })
                            }
                        } else {
                            Err(ParseError::UnexpectedEof {
                                expected: ")".to_string(),
                            })
                        }
                    } else if name == "resume" {
                        if let Some(tok) = self.peek() {
                            if matches!(tok.token, Token::LeftParen) {
                                self.advance();
                            } else {
                                return Err(ParseError::UnexpectedToken {
                                    expected: "'('".to_string(),
                                    found: tok.token.clone(),
                                    span: tok.span,
                                });
                            }
                        } else {
                            return Err(ParseError::UnexpectedEof {
                                expected: "'('".to_string(),
                            });
                        }
                        let value_expr = self.parse_expression()?;
                        if let Some(tok) = self.peek() {
                            if matches!(tok.token, Token::RightParen) {
                                let end_span = tok.span;
                                self.advance();
                                let full_span = Span::new(span.start, end_span.end);
                                Ok(Expr::EffectResume {
                                    value: Box::new(value_expr),
                                    span: full_span,
                                })
                            } else {
                                Err(ParseError::UnexpectedToken {
                                    expected: ")".to_string(),
                                    found: tok.token.clone(),
                                    span: tok.span,
                                })
                            }
                        } else {
                            Err(ParseError::UnexpectedEof {
                                expected: ")".to_string(),
                            })
                        }
                    } else if name == "handle" {
                        let tag_expr = self.parse_expression()?;
                        if let Some(tok) = self.peek() {
                            if matches!(tok.token, Token::LeftParen) {
                                self.advance();
                            } else {
                                return Err(ParseError::UnexpectedToken {
                                    expected: "'('".to_string(),
                                    found: tok.token.clone(),
                                    span: tok.span,
                                });
                            }
                        } else {
                            return Err(ParseError::UnexpectedEof {
                                expected: "'('".to_string(),
                            });
                        }
                        let param_name = if let Some(tok) = self.peek() {
                            if let Token::Identifier(s) = &tok.token {
                                let s2 = s.clone();
                                self.advance();
                                s2
                            } else {
                                return Err(ParseError::UnexpectedToken {
                                    expected: "identifier".to_string(),
                                    found: tok.token.clone(),
                                    span: tok.span,
                                });
                            }
                        } else {
                            return Err(ParseError::UnexpectedEof {
                                expected: "identifier".to_string(),
                            });
                        };
                        if let Some(tok) = self.peek() {
                            if matches!(tok.token, Token::RightParen) {
                                self.advance();
                            } else {
                                return Err(ParseError::UnexpectedToken {
                                    expected: ")".to_string(),
                                    found: tok.token.clone(),
                                    span: tok.span,
                                });
                            }
                        } else {
                            return Err(ParseError::UnexpectedEof {
                                expected: ")".to_string(),
                            });
                        }
                        if let Some(tok) = self.peek() {
                            if matches!(tok.token, Token::LeftBrace) {
                                self.advance();
                            } else {
                                return Err(ParseError::UnexpectedToken {
                                    expected: "'{'".to_string(),
                                    found: tok.token.clone(),
                                    span: tok.span,
                                });
                            }
                        } else {
                            return Err(ParseError::UnexpectedEof {
                                expected: "'{'".to_string(),
                            });
                        }
                        let handler_expr = self.parse_expression()?;
                        if let Some(tok) = self.peek() {
                            if matches!(tok.token, Token::RightBrace) {
                                self.advance();
                            } else {
                                return Err(ParseError::UnexpectedToken {
                                    expected: "'}'".to_string(),
                                    found: tok.token.clone(),
                                    span: tok.span,
                                });
                            }
                        } else {
                            return Err(ParseError::UnexpectedEof {
                                expected: "'}'".to_string(),
                            });
                        }
                        if let Some(tok) = self.peek() {
                            match &tok.token {
                                Token::KwIn => {
                                    self.advance();
                                }
                                Token::Identifier(id) if id == "in" => {
                                    self.advance();
                                }
                                _ => {
                                    return Err(ParseError::UnexpectedToken {
                                        expected: "in".to_string(),
                                        found: tok.token.clone(),
                                        span: tok.span,
                                    });
                                }
                            }
                        } else {
                            return Err(ParseError::UnexpectedEof {
                                expected: "in".to_string(),
                            });
                        }
                        let body_expr = self.parse_expression()?;
                        let full_span = Span::new(span.start, body_expr.span().end);
                        Ok(Expr::EffectHandle {
                            tag: Box::new(tag_expr),
                            param: param_name,
                            handler: Box::new(handler_expr),
                            body: Box::new(body_expr),
                            span: full_span,
                        })
                    } else if let Some(module_access) =
                        self.try_parse_module_symbol_access(&name, span)
                    {
                        Ok(module_access)
                    } else {
                        // 检查是否为限定构造器 TypeName::Constructor
                        if let Some(next_token) = self.peek() {
                            if matches!(next_token.token, Token::DoubleColon) {
                                self.advance(); // consume '::'

                                // 期望构造器名
                                if let Some(constructor_token) = self.peek() {
                                    if let Token::Identifier(constructor_name) =
                                        &constructor_token.token
                                    {
                                        let constructor_name = constructor_name.clone();
                                        let constructor_span = constructor_token.span;
                                        self.advance();

                                        // 检查是否有参数
                                        if let Some(arg_token) = self.peek() {
                                            if matches!(arg_token.token, Token::LeftParen) {
                                                self.advance(); // consume '('
                                                let arg = if let Some(peeked) = self.peek() {
                                                    if matches!(peeked.token, Token::RightParen) {
                                                        None
                                                    } else {
                                                        Some(Box::new(self.parse_expression()?))
                                                    }
                                                } else {
                                                    return Err(ParseError::UnexpectedEof {
                                                        expected: "expression or ')'".to_string(),
                                                    });
                                                };

                                                if let Some(close_token) = self.peek() {
                                                    if matches!(
                                                        close_token.token,
                                                        Token::RightParen
                                                    ) {
                                                        let end_span = close_token.span;
                                                        self.advance(); // consume ')'
                                                        let full_span =
                                                            Span::new(span.start, end_span.end);
                                                        Ok(Expr::QualifiedConstructor {
                                                            type_name: name,
                                                            constructor_name,
                                                            arg,
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
                                                // 无参数限定构造器
                                                let full_span =
                                                    Span::new(span.start, constructor_span.end);
                                                Ok(Expr::QualifiedConstructor {
                                                    type_name: name,
                                                    constructor_name,
                                                    arg: None,
                                                    span: full_span,
                                                })
                                            }
                                        } else {
                                            // 无参数限定构造器
                                            let full_span =
                                                Span::new(span.start, constructor_span.end);
                                            Ok(Expr::QualifiedConstructor {
                                                type_name: name,
                                                constructor_name,
                                                arg: None,
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
                            } else if matches!(next_token.token, Token::LeftParen)
                                && self.is_constructor(&name)
                            {
                                // 只有已知构造器才处理构造器调用 Constructor(arg)
                                self.advance(); // consume '('
                                let arg = if let Some(peeked) = self.peek() {
                                    if matches!(peeked.token, Token::RightParen) {
                                        // 无参数构造器
                                        None
                                    } else {
                                        Some(Box::new(self.parse_expression()?))
                                    }
                                } else {
                                    return Err(ParseError::UnexpectedEof {
                                        expected: "expression or ')'".to_string(),
                                    });
                                };

                                if let Some(token) = self.peek() {
                                    if matches!(token.token, Token::RightParen) {
                                        let end_span = token.span;
                                        self.advance(); // consume ')'
                                        let full_span = Span::new(span.start, end_span.end);
                                        Ok(Expr::Constructor {
                                            name,
                                            arg,
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
                            } else if matches!(next_token.token, Token::LeftBrace) {
                                // 检查是否是结构体字面量
                                // 我们需要向前看一些token来确定这是struct字面量还是其他东西
                                if self.is_likely_struct_literal() {
                                    // 结构体字面量: StructName { field1: value1, field2: value2 }
                                    self.advance(); // consume '{'

                                    let mut fields = Vec::new();

                                    // 解析字段初始化列表
                                    while let Some(token) = self.peek() {
                                        if matches!(token.token, Token::RightBrace) {
                                            break;
                                        }

                                        // 解析字段名
                                        let field_name = if let Token::Identifier(field_name) =
                                            &token.token
                                        {
                                            let field_name = field_name.clone();
                                            let field_span = token.span;
                                            self.advance();

                                            // 期望 ':'
                                            if let Some(colon_token) = self.peek() {
                                                if matches!(colon_token.token, Token::Colon) {
                                                    self.advance(); // consume ':'

                                                    // 解析字段值
                                                    let field_value = self.parse_expression()?;

                                                    karte_hir::ast::FieldInit {
                                                        name: field_name,
                                                        value: field_value,
                                                        span: field_span,
                                                    }
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
                                            self.advance(); // consume '}'
                                            let full_span = Span::new(span.start, end_span.end);
                                            Ok(Expr::StructLiteral {
                                                name,
                                                fields,
                                                span: full_span,
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
                                } else {
                                    // 不是struct字面量，作为普通标识符处理
                                    if self.is_constructor(&name) {
                                        Ok(Expr::Constructor {
                                            name,
                                            arg: None,
                                            span,
                                        })
                                    } else {
                                        Ok(Expr::Identifier { name, span })
                                    }
                                }
                            } else {
                                // 检查是否为已知的无参数构造器
                                if self.is_constructor(&name) {
                                    Ok(Expr::Constructor {
                                        name,
                                        arg: None,
                                        span,
                                    })
                                } else {
                                    Ok(Expr::Identifier { name, span })
                                }
                            }
                        } else {
                            // 检查是否为已知的无参数构造器
                            if self.is_constructor(&name) {
                                Ok(Expr::Constructor {
                                    name,
                                    arg: None,
                                    span,
                                })
                            } else {
                                Ok(Expr::Identifier { name, span })
                            }
                        }
                    }
                }
                Token::Pipe => {
                    // 解析lambda表达式: |param1, param2| body
                    self.parse_lambda()
                }
                Token::LogicalOr => {
                    // 解析空参数lambda表达式: || body
                    self.parse_lambda_no_params()
                }
                Token::LeftParen => {
                    self.advance(); // consume '('
                    let expr = self.parse_expression()?;

                    if let Some(token) = self.peek() {
                        if matches!(token.token, Token::RightParen) {
                            self.advance(); // consume ')'
                            Ok(expr)
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
                Token::LeftBrace => self.parse_block_expression(),
                Token::LogicalNot => {
                    // 处理连续的否定操作符，如 !!true, !!!false
                    self.parse_factor()
                }
                Token::LeftBracket => {
                    let start_span = token.span;
                    self.advance(); // consume '['
                    let mut elements = Vec::new();

                    loop {
                        if let Some(next) = self.peek() {
                            if matches!(next.token, Token::RightBracket) {
                                break;
                            }
                        } else {
                            return Err(ParseError::UnexpectedEof {
                                expected: "']'".to_string(),
                            });
                        }

                        elements.push(self.parse_expression()?);

                        if let Some(next) = self.peek() {
                            if matches!(next.token, Token::Comma) {
                                self.advance();
                            } else if matches!(next.token, Token::RightBracket) {
                                break;
                            } else {
                                return Err(ParseError::UnexpectedToken {
                                    expected: "',' or ']'".to_string(),
                                    found: next.token.clone(),
                                    span: next.span,
                                });
                            }
                        }
                    }

                    if let Some(end_token) = self.peek() {
                        if matches!(end_token.token, Token::RightBracket) {
                            let end_span = end_token.span;
                            self.advance(); // consume ']'
                            let span = Span::new(start_span.start, end_span.end);
                            Ok(Expr::ArrayLiteral { elements, span })
                        } else {
                            Err(ParseError::UnexpectedToken {
                                expected: "']'".to_string(),
                                found: end_token.token.clone(),
                                span: end_token.span,
                            })
                        }
                    } else {
                        Err(ParseError::UnexpectedEof {
                            expected: "']'".to_string(),
                        })
                    }
                }
                _ => Err(ParseError::UnexpectedToken {
                    expected: "number, identifier, lambda, '{', or '('".to_string(),
                    found: token.token.clone(),
                    span: token.span,
                }),
            }
        } else {
            Err(ParseError::UnexpectedEof {
                expected: "number, identifier, lambda, or '('".to_string(),
            })
        }?;

        // 处理调用 / 下标 / 字段访问的后缀操作
        loop {
            let mut progressed = false;
            if let Some(token) = self.peek() {
                match token.token {
                    Token::LeftParen => {
                        progressed = true;
                        self.advance(); // consume '('
                        let mut args = Vec::new();

                        if let Some(next) = self.peek() {
                            if !matches!(next.token, Token::RightParen) {
                                args.push(self.parse_expression()?);
                                while let Some(next) = self.peek() {
                                    if matches!(next.token, Token::Comma) {
                                        self.advance();
                                        args.push(self.parse_expression()?);
                                    } else {
                                        break;
                                    }
                                }
                            }
                        }

                        if let Some(next) = self.peek() {
                            if matches!(next.token, Token::RightParen) {
                                let end_span = next.span;
                                self.advance();
                                let span = Span::new(expr.span().start, end_span.end);
                                expr = Expr::FunctionCall {
                                    function: Box::new(expr),
                                    args,
                                    span,
                                };
                            } else {
                                return Err(ParseError::UnexpectedToken {
                                    expected: "')'".to_string(),
                                    found: next.token.clone(),
                                    span: next.span,
                                });
                            }
                        } else {
                            return Err(ParseError::UnexpectedEof {
                                expected: "')'".to_string(),
                            });
                        }
                    }
                    Token::LeftBracket => {
                        progressed = true;
                        self.advance(); // consume '['
                        let index_expr = self.parse_expression()?;
                        if let Some(close) = self.peek() {
                            if matches!(close.token, Token::RightBracket) {
                                let end_span = close.span;
                                self.advance();
                                let span = Span::new(expr.span().start, end_span.end);
                                expr = Expr::Index {
                                    array: Box::new(expr),
                                    index: Box::new(index_expr),
                                    span,
                                };
                            } else {
                                return Err(ParseError::UnexpectedToken {
                                    expected: "']'".to_string(),
                                    found: close.token.clone(),
                                    span: close.span,
                                });
                            }
                        } else {
                            return Err(ParseError::UnexpectedEof {
                                expected: "']'".to_string(),
                            });
                        }
                    }
                    Token::Dot => {
                        progressed = true;
                        self.advance(); // consume '.'
                        if let Some(field_token) = self.peek() {
                            if let Token::Identifier(field_name) = &field_token.token {
                                let field_name = field_name.clone();
                                let end_span = field_token.span;
                                self.advance();
                                let span = Span::new(expr.span().start, end_span.end);
                                expr = Expr::FieldAccess {
                                    object: Box::new(expr),
                                    field: field_name,
                                    span,
                                };
                            } else {
                                return Err(ParseError::UnexpectedToken {
                                    expected: "field name".to_string(),
                                    found: field_token.token.clone(),
                                    span: field_token.span,
                                });
                            }
                        } else {
                            return Err(ParseError::UnexpectedEof {
                                expected: "field name".to_string(),
                            });
                        }
                    }
                    _ => {}
                }
            }

            if !progressed {
                break;
            }
        }

        Ok(expr)
    }

    /// 解析match表达式: match expr { pattern1 -> expr1, pattern2 -> expr2, ... }
    pub(crate) fn parse_match(&mut self) -> Result<Expr, ParseError> {
        let start_span = self.peek().unwrap().span;
        self.advance(); // consume 'match'

        // 解析被匹配的表达式
        let expr = Box::new(self.parse_expression()?);

        // 期望 '{'
        if let Some(token) = self.peek() {
            if matches!(token.token, Token::LeftBrace) {
                self.advance(); // consume '{'
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

        // 解析match分支
        let mut arms = Vec::new();
        while let Some(token) = self.peek() {
            if matches!(token.token, Token::RightBrace) {
                break;
            }

            // 解析模式
            let pattern = self.parse_pattern()?;

            // 期望 '->'
            if let Some(token) = self.peek() {
                if matches!(token.token, Token::Arrow) {
                    self.advance(); // consume '->'
                } else {
                    return Err(ParseError::UnexpectedToken {
                        expected: "'->'".to_string(),
                        found: token.token.clone(),
                        span: token.span,
                    });
                }
            } else {
                return Err(ParseError::UnexpectedEof {
                    expected: "'->'".to_string(),
                });
            }

            // 解析分支体
            let body = self.parse_expression()?;
            let arm_span = Span::new(pattern.span().start, body.span().end);

            arms.push(karte_hir::MatchArm {
                pattern,
                body,
                span: arm_span,
            });

            // 检查是否有更多分支
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
                self.advance(); // consume '}'
                let span = Span::new(start_span.start, end_span.end);
                Ok(Expr::Match { expr, arms, span })
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

    /// 解析空参数lambda表达式: || body
    pub(crate) fn parse_lambda_no_params(&mut self) -> Result<Expr, ParseError> {
        let start_span = self.peek().unwrap().span;
        self.advance(); // consume '||'

        // 解析lambda体
        let body = self.parse_expression()?;
        let end_span = body.span();
        let span = Span::new(start_span.start, end_span.end);

        Ok(Expr::Lambda {
            params: Vec::new(), // 空参数列表
            body: Box::new(body),
            inferred_type: None,
            span,
        })
    }

    /// 解析lambda表达式: |param1, param2| body 或 |param1: type1, param2: type2| body
    pub(crate) fn parse_lambda(&mut self) -> Result<Expr, ParseError> {
        let start_span = self.peek().unwrap().span;
        self.advance(); // consume first '|'

        let mut params = Vec::new();

        // 解析参数列表
        while let Some(token) = self.peek() {
            match &token.token {
                Token::Identifier(name) => {
                    let param_name = name.clone();
                    let param_span = token.span;
                    self.advance();

                    // 检查是否有类型注解
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

                    params.push(karte_hir::Parameter {
                        name: param_name,
                        type_annotation,
                        span: param_span,
                    });

                    // 检查是否有更多参数
                    if let Some(next_token) = self.peek() {
                        if matches!(next_token.token, Token::Comma) {
                            self.advance(); // consume ','
                            continue;
                        } else if matches!(next_token.token, Token::Pipe) {
                            break; // 结束参数列表
                        } else {
                            return Err(ParseError::UnexpectedToken {
                                expected: "',' or '|'".to_string(),
                                found: next_token.token.clone(),
                                span: next_token.span,
                            });
                        }
                    } else {
                        return Err(ParseError::UnexpectedEof {
                            expected: "'|'".to_string(),
                        });
                    }
                }
                Token::Pipe => {
                    break; // 空参数列表
                }
                _ => {
                    return Err(ParseError::UnexpectedToken {
                        expected: "parameter name or '|'".to_string(),
                        found: token.token.clone(),
                        span: token.span,
                    });
                }
            }
        }

        // 期望第二个'|'
        if let Some(token) = self.peek() {
            if matches!(token.token, Token::Pipe) {
                self.advance(); // consume second '|'
            } else {
                return Err(ParseError::UnexpectedToken {
                    expected: "'|'".to_string(),
                    found: token.token.clone(),
                    span: token.span,
                });
            }
        } else {
            return Err(ParseError::UnexpectedEof {
                expected: "'|'".to_string(),
            });
        }

        // 解析lambda体
        let body = self.parse_expression()?;
        let end_span = body.span();
        let span = Span::new(start_span.start, end_span.end);

        Ok(Expr::Lambda {
            params,
            body: Box::new(body),
            inferred_type: None,
            span,
        })
    }

    /// 解析if表达式: if condition then branch else branch
    pub(crate) fn parse_if(&mut self) -> Result<Expr, ParseError> {
        let start_span = self.peek().unwrap().span;

        // 期望 'if'
        if let Some(token) = self.peek() {
            if let Token::Identifier(name) = &token.token {
                if name == "if" {
                    self.advance(); // consume 'if'
                } else {
                    return Err(ParseError::UnexpectedToken {
                        expected: "'if'".to_string(),
                        found: token.token.clone(),
                        span: token.span,
                    });
                }
            } else {
                return Err(ParseError::UnexpectedToken {
                    expected: "'if'".to_string(),
                    found: token.token.clone(),
                    span: token.span,
                });
            }
        } else {
            return Err(ParseError::UnexpectedEof {
                expected: "'if'".to_string(),
            });
        }

        // 解析条件表达式
        let condition = self.parse_expression()?;

        // 期望 'then' 或 '{'
        if let Some(token) = self.peek() {
            if let Token::Identifier(name) = &token.token {
                if name == "then" {
                    self.advance(); // consume 'then'
                } else if matches!(token.token, Token::LeftBrace) {
                    // 允许 if condition { ... } 语法
                } else {
                    return Err(ParseError::UnexpectedToken {
                        expected: "'then' or '{'".to_string(),
                        found: token.token.clone(),
                        span: token.span,
                    });
                }
            } else if matches!(token.token, Token::LeftBrace) {
                // 允许 if condition { ... } 语法
            } else {
                return Err(ParseError::UnexpectedToken {
                    expected: "'then' or '{'".to_string(),
                    found: token.token.clone(),
                    span: token.span,
                });
            }
        } else {
            return Err(ParseError::UnexpectedEof {
                expected: "'then' or '{'".to_string(),
            });
        }

        // 解析then分支
        let then_branch = self.parse_expression()?;

        // 检查是否有else分支
        let else_branch = if let Some(token) = self.peek() {
            if let Token::Identifier(name) = &token.token {
                if name == "else" {
                    self.advance(); // consume 'else'
                    Some(Box::new(self.parse_expression()?))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let end_span = if let Some(else_branch) = &else_branch {
            else_branch.span()
        } else {
            then_branch.span()
        };

        let span = Span::new(start_span.start, end_span.end);
        Ok(Expr::If {
            condition: Box::new(condition),
            then_branch: Box::new(then_branch),
            else_branch,
            span,
        })
    }

    /// 解析while表达式: while condition do body
    pub(crate) fn parse_while(&mut self) -> Result<Expr, ParseError> {
        let start_span = self.peek().unwrap().span;

        // 期望 'while'
        if let Some(token) = self.peek() {
            if let Token::Identifier(name) = &token.token {
                if name == "while" {
                    self.advance(); // consume 'while'
                } else {
                    return Err(ParseError::UnexpectedToken {
                        expected: "'while'".to_string(),
                        found: token.token.clone(),
                        span: token.span,
                    });
                }
            } else {
                return Err(ParseError::UnexpectedToken {
                    expected: "'while'".to_string(),
                    found: token.token.clone(),
                    span: token.span,
                });
            }
        } else {
            return Err(ParseError::UnexpectedEof {
                expected: "'while'".to_string(),
            });
        }

        // 解析条件表达式
        let condition = self.parse_expression()?;

        // 期望 'do' 或 '{'
        if let Some(token) = self.peek() {
            if let Token::Identifier(name) = &token.token {
                if name == "do" {
                    self.advance(); // consume 'do'
                } else if matches!(token.token, Token::LeftBrace) {
                    // 允许 while condition { ... } 语法
                } else {
                    return Err(ParseError::UnexpectedToken {
                        expected: "'do' or '{'".to_string(),
                        found: token.token.clone(),
                        span: token.span,
                    });
                }
            } else if matches!(token.token, Token::LeftBrace) {
                // 允许 while condition { ... } 语法
            } else {
                return Err(ParseError::UnexpectedToken {
                    expected: "'do' or '{'".to_string(),
                    found: token.token.clone(),
                    span: token.span,
                });
            }
        } else {
            return Err(ParseError::UnexpectedEof {
                expected: "'do' or '{'".to_string(),
            });
        }

        // 解析循环体
        let body = self.parse_expression()?;
        let end_span = body.span();
        let span = Span::new(start_span.start, end_span.end);

        Ok(Expr::While {
            condition: Box::new(condition),
            body: Box::new(body),
            span,
        })
    }
}
