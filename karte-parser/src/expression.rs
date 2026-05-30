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

    // assignment = logical_or (('=' | '+=' | '-=' | '*=' | '/=' | 'bitand=' | 'bitor=' | 'bitxor=' | 'shl=' | 'shr=') assignment)?
    pub(crate) fn parse_assignment(&mut self) -> Result<Expr, ParseError> {
        let expr = self.parse_logical_or()?;

        // 检测复合赋值运算符（单 token: += -= *= /=）
        let compound_op = if let Some(token) = self.peek() {
            match &token.token {
                Token::PlusEqual => Some(BinaryOperator::Add),
                Token::MinusEqual => Some(BinaryOperator::Subtract),
                Token::StarEqual => Some(BinaryOperator::Multiply),
                Token::SlashEqual => Some(BinaryOperator::Divide),
                Token::AmpersandEqual => Some(BinaryOperator::BitAnd),
                Token::PipeEqual => Some(BinaryOperator::BitOr),
                Token::CaretEqual => Some(BinaryOperator::BitXor),
                Token::ShiftLeftEqual => Some(BinaryOperator::ShiftLeft),
                Token::ShiftRightEqual => Some(BinaryOperator::ShiftRight),
                _ => {
                    // 双 token 复合赋值：bitand= bitor= bitxor= shl= shr=
                    // 需要 lookahead 一个 token
                    let tok = &token.token;
                    let next_tok = self.tokens.get(self.position + 1).map(|t| &t.token);
                    match (tok, next_tok) {
                        (Token::BitAnd, Some(Token::Equal)) => Some(BinaryOperator::BitAnd),
                        (Token::BitOr, Some(Token::Equal)) => Some(BinaryOperator::BitOr),
                        (Token::BitXor, Some(Token::Equal)) => Some(BinaryOperator::BitXor),
                        (Token::ShiftLeft, Some(Token::Equal))
                        | (Token::ShiftLeftSym, Some(Token::Equal)) => {
                            Some(BinaryOperator::ShiftLeft)
                        }
                        (Token::ShiftRight, Some(Token::Equal))
                        | (Token::ShiftRightSym, Some(Token::Equal)) => {
                            Some(BinaryOperator::ShiftRight)
                        }
                        _ => None,
                    }
                }
            }
        } else {
            None
        };

        if let Some(op) = compound_op {
            let span_start = expr.span().start;
            // 判断是单 token 还是双 token 复合赋值
            let is_single_token = matches!(
                self.peek().map(|t| &t.token),
                Some(
                    Token::PlusEqual
                        | Token::MinusEqual
                        | Token::StarEqual
                        | Token::SlashEqual
                        | Token::AmpersandEqual
                        | Token::PipeEqual
                        | Token::CaretEqual
                        | Token::ShiftLeftEqual
                        | Token::ShiftRightEqual
                )
            );

            if is_single_token {
                self.advance(); // 消费 += 等
            } else {
                self.advance(); // 消费 bitand 等
                self.advance(); // 消费 =
            }

            let right = self.parse_assignment()?; // 右结合
            let span = Span::new(span_start, right.span().end);
            let target = expr.clone();
            // 复合赋值展开为 target = target op right
            let value = Expr::BinaryOp {
                left: Box::new(target.clone()),
                op,
                right: Box::new(right),
                span,
            };
            return Ok(Expr::Assignment {
                target: Box::new(target),
                value: Box::new(value),
                span,
            });
        }

        // 普通赋值
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
        let mut left = self.parse_bitwise_or()?;

        while let Some(token) = self.peek() {
            match token.token {
                Token::EqualEqual => {
                    self.advance();
                    let right = self.parse_bitwise_or()?;
                    let span = Span::new(left.span().start, right.span().end);
                    left = Expr::BinaryOp {
                        left: Box::new(left),
                        op: BinaryOperator::Equal,
                        right: Box::new(right),
                        span,
                    };
                }
                Token::NotEqual => {
                    self.advance();
                    let right = self.parse_bitwise_or()?;
                    let span = Span::new(left.span().start, right.span().end);
                    left = Expr::BinaryOp {
                        left: Box::new(left),
                        op: BinaryOperator::NotEqual,
                        right: Box::new(right),
                        span,
                    };
                }
                Token::GreaterEqual => {
                    self.advance();
                    let right = self.parse_bitwise_or()?;
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
                    let right = self.parse_bitwise_or()?;
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
                    let right = self.parse_bitwise_or()?;
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
                    let right = self.parse_bitwise_or()?;
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

    // bitwise_or = bitwise_xor ((bitor | |) bitwise_xor)*
    // 注意：需要排除 bitor= 复合赋值的情况
    pub(crate) fn parse_bitwise_or(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_bitwise_xor()?;
        while let Some(token) = self.peek() {
            if token.token == Token::BitOr || token.token == Token::Pipe {
                // BitOr 关键字形式需要排除 bitor= 复合赋值
                // Pipe 符号形式不需要排除，因为 |= 已经是单 Token PipeEqual
                if token.token == Token::BitOr {
                    if let Some(next) = self.tokens.get(self.position + 1) {
                        if matches!(next.token, Token::Equal) {
                            break; // 交给 parse_assignment 处理
                        }
                    }
                }
                self.advance();
                let right = self.parse_bitwise_xor()?;
                let span = Span::new(left.span().start, right.span().end);
                left = Expr::BinaryOp {
                    left: Box::new(left),
                    op: BinaryOperator::BitOr,
                    right: Box::new(right),
                    span,
                };
            } else {
                break;
            }
        }
        Ok(left)
    }

    // bitwise_xor = bitwise_and ((bitxor | ^) bitwise_and)*
    // 注意：需要排除 bitxor= 复合赋值的情况
    pub(crate) fn parse_bitwise_xor(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_bitwise_and()?;
        while let Some(token) = self.peek() {
            if token.token == Token::BitXor || token.token == Token::Caret {
                // BitXor 关键字形式需要排除 bitxor= 复合赋值
                // Caret 符号形式不需要排除，因为 ^= 已经是单 Token CaretEqual
                if token.token == Token::BitXor {
                    if let Some(next) = self.tokens.get(self.position + 1) {
                        if matches!(next.token, Token::Equal) {
                            break; // 交给 parse_assignment 处理
                        }
                    }
                }
                self.advance();
                let right = self.parse_bitwise_and()?;
                let span = Span::new(left.span().start, right.span().end);
                left = Expr::BinaryOp {
                    left: Box::new(left),
                    op: BinaryOperator::BitXor,
                    right: Box::new(right),
                    span,
                };
            } else {
                break;
            }
        }
        Ok(left)
    }

    // bitwise_and = shift ((bitand | &) shift)*
    // 注意：需要排除 bitand= 复合赋值的情况
    pub(crate) fn parse_bitwise_and(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_shift()?;
        while let Some(token) = self.peek() {
            if token.token == Token::BitAnd || token.token == Token::Ampersand {
                // BitAnd 关键字形式需要排除 bitand= 复合赋值
                // Ampersand 符号形式不需要排除，因为 &= 已经是单 Token AmpersandEqual
                if token.token == Token::BitAnd {
                    if let Some(next) = self.tokens.get(self.position + 1) {
                        if matches!(next.token, Token::Equal) {
                            break; // 交给 parse_assignment 处理
                        }
                    }
                }
                self.advance();
                let right = self.parse_shift()?;
                let span = Span::new(left.span().start, right.span().end);
                left = Expr::BinaryOp {
                    left: Box::new(left),
                    op: BinaryOperator::BitAnd,
                    right: Box::new(right),
                    span,
                };
            } else {
                break;
            }
        }
        Ok(left)
    }

    // shift = additive ((shl | shr | << | >>) additive)*
    // 注意：需要排除 shl= / shr= 复合赋值的情况
    pub(crate) fn parse_shift(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_additive()?;
        while let Some(token) = self.peek() {
            match token.token {
                Token::ShiftLeft | Token::ShiftLeftSym => {
                    // 检查是否为 shl= 复合赋值（仅关键字形式）
                    if token.token == Token::ShiftLeft {
                        if let Some(next) = self.tokens.get(self.position + 1) {
                            if matches!(next.token, Token::Equal) {
                                break; // 交给 parse_assignment 处理
                            }
                        }
                    }
                    self.advance();
                    let right = self.parse_additive()?;
                    let span = Span::new(left.span().start, right.span().end);
                    left = Expr::BinaryOp {
                        left: Box::new(left),
                        op: BinaryOperator::ShiftLeft,
                        right: Box::new(right),
                        span,
                    };
                }
                Token::ShiftRight | Token::ShiftRightSym => {
                    // 检查是否为 shr= 复合赋值（仅关键字形式）
                    if token.token == Token::ShiftRight {
                        if let Some(next) = self.tokens.get(self.position + 1) {
                            if matches!(next.token, Token::Equal) {
                                break; // 交给 parse_assignment 处理
                            }
                        }
                    }
                    self.advance();
                    let right = self.parse_additive()?;
                    let span = Span::new(left.span().start, right.span().end);
                    left = Expr::BinaryOp {
                        left: Box::new(left),
                        op: BinaryOperator::ShiftRight,
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
                Token::Percent => {
                    self.advance();
                    let right = self.parse_factor()?;
                    let span = Span::new(left.span().start, right.span().end);
                    left = Expr::BinaryOp {
                        left: Box::new(left),
                        op: BinaryOperator::Modulo,
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
                }
            }

            // bitnot 一元运算符（支持 bitnot 关键字和 ~ 符号）
            if token.token == Token::BitNot || token.token == Token::Tilde {
                let start_span = token.span;
                self.advance();
                let operand = self.parse_factor()?;
                let span = Span::new(start_span.start, operand.span().end);
                return Ok(Expr::UnaryOp {
                    op: UnaryOperator::BitNot,
                    operand: Box::new(operand),
                    span,
                });
            }

            // unsafe 内存操作内建函数
            match &token.token {
                Token::UnsafeLoad | Token::UnsafeLoad8 | Token::UnsafeLoad32 => {
                    let byte_size = match &token.token {
                        Token::UnsafeLoad => 8,
                        Token::UnsafeLoad8 => 1,
                        Token::UnsafeLoad32 => 4,
                        _ => unreachable!(),
                    };
                    let start_span = token.span;
                    self.advance();
                    self.expect_token(Token::LeftParen)?;
                    let addr = self.parse_expression()?;
                    self.expect_token(Token::RightParen)?;
                    let span = Span::new(start_span.start, self.current_span().end);
                    return Ok(Expr::UnsafeLoad {
                        addr: Box::new(addr),
                        byte_size,
                        span,
                    });
                }
                Token::UnsafeStore | Token::UnsafeStore8 | Token::UnsafeStore32 => {
                    let byte_size = match &token.token {
                        Token::UnsafeStore => 8,
                        Token::UnsafeStore8 => 1,
                        Token::UnsafeStore32 => 4,
                        _ => unreachable!(),
                    };
                    let start_span = token.span;
                    self.advance();
                    self.expect_token(Token::LeftParen)?;
                    let addr = self.parse_expression()?;
                    self.expect_token(Token::Comma)?;
                    let value = self.parse_expression()?;
                    self.expect_token(Token::RightParen)?;
                    let span = Span::new(start_span.start, self.current_span().end);
                    return Ok(Expr::UnsafeStore {
                        addr: Box::new(addr),
                        value: Box::new(value),
                        byte_size,
                        span,
                    });
                }
                _ => {}
            }

            // runtime 内建函数
            match &token.token {
                Token::RuntimeHeapBase | Token::RuntimeHeapLimit | Token::RuntimeStackBottom | Token::RuntimeStackTop | Token::RuntimeVmSp => {
                    let name = match &token.token {
                        Token::RuntimeHeapBase => "heap_base",
                        Token::RuntimeHeapLimit => "heap_limit",
                        Token::RuntimeStackBottom => "stack_bottom",
                        Token::RuntimeStackTop => "stack_top",
                        Token::RuntimeVmSp => "vm_sp",
                        _ => unreachable!(),
                    };
                    let start_span = token.span;
                    self.advance();
                    self.expect_token(Token::LeftParen)?;
                    self.expect_token(Token::RightParen)?;
                    let span = Span::new(start_span.start, self.current_span().end);
                    return Ok(Expr::RuntimeGlobal { name: name.to_string(), span });
                }
                // GC 寄存器保存/恢复内建函数
                Token::GcPushRegs | Token::GcPopRegs => {
                    let is_push = matches!(&token.token, Token::GcPushRegs);
                    let start_span = token.span;
                    self.advance();
                    self.expect_token(Token::LeftParen)?;
                    self.expect_token(Token::RightParen)?;
                    let span = Span::new(start_span.start, self.current_span().end);
                    return Ok(Expr::GcRegOp { is_push, span });
                }
                _ => {}
            }

            // len 内建函数（Identifier 分支）
            if let Token::Identifier(name) = &token.token {
                if name == "len" {
                    let start_span = token.span;
                    self.advance();
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
                Token::StringLiteral(value) => {
                    let value = value.clone();
                    let span = token.span;
                    self.advance();
                    Ok(Expr::StringLiteral { value, span })
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
                Token::KwFor => {
                    self.parse_for_in()
                }
                Token::KwBreak => {
                    let span = token.span;
                    self.advance();
                    Ok(Expr::Break { span })
                }
                Token::KwContinue => {
                    let span = token.span;
                    self.advance();
                    Ok(Expr::Continue { span })
                }
                Token::KwReturn => {
                    let start_span = token.span;
                    self.advance();
                    // return 后面可以跟表达式，也可以没有
                    let value = if let Some(tok) = self.peek() {
                        // 检查下一个 token 是否可以开始一个表达式
                        match &tok.token {
                            Token::RightBrace | Token::Semicolon | Token::Comma => None,
                            _ => Some(Box::new(self.parse_expression()?)),
                        }
                    } else {
                        None
                    };
                    let end_span = if let Some(v) = &value {
                        v.span()
                    } else {
                        start_span
                    };
                    Ok(Expr::Return {
                        value,
                        span: Span::new(start_span.start, end_span.end),
                    })
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

    /// 解析 for-in 表达式: for ident in start..end { body }
    pub(crate) fn parse_for_in(&mut self) -> Result<Expr, ParseError> {
        let start_span = self.peek().unwrap().span;
        self.advance(); // consume 'for'

        // 解析循环变量名
        let var = if let Some(tok) = self.peek() {
            if let Token::Identifier(name) = &tok.token {
                name.clone()
            } else {
                return Err(ParseError::UnexpectedToken {
                    expected: "identifier".to_string(),
                    found: tok.token.clone(),
                    span: tok.span,
                });
            }
        } else {
            return Err(ParseError::UnexpectedEof {
                expected: "loop variable name".to_string(),
            });
        };
        self.advance(); // consume var name

        // 期望 'in'
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
                        expected: "'in'".to_string(),
                        found: tok.token.clone(),
                        span: tok.span,
                    });
                }
            }
        } else {
            return Err(ParseError::UnexpectedEof {
                expected: "'in'".to_string(),
            });
        }

        // 解析 start 表达式
        let start = self.parse_expression()?;

        // 期望 '..'
        if let Some(tok) = self.peek() {
            if matches!(tok.token, Token::DoubleDot) {
                self.advance(); // consume '..'
            } else {
                return Err(ParseError::UnexpectedToken {
                    expected: "'..'".to_string(),
                    found: tok.token.clone(),
                    span: tok.span,
                });
            }
        } else {
            return Err(ParseError::UnexpectedEof {
                expected: "'..'".to_string(),
            });
        }

        // 解析 end 表达式
        let end = self.parse_expression()?;

        // 期望 '{' (或 do)
        if let Some(tok) = self.peek() {
            match &tok.token {
                Token::LeftBrace => {
                    // 允许 for x in 0..10 { ... } 语法，不需要 consume
                }
                Token::Identifier(name) if name == "do" => {
                    self.advance(); // consume 'do'
                }
                _ => {
                    return Err(ParseError::UnexpectedToken {
                        expected: "'{' or 'do'".to_string(),
                        found: tok.token.clone(),
                        span: tok.span,
                    });
                }
            }
        } else {
            return Err(ParseError::UnexpectedEof {
                expected: "'{' or 'do'".to_string(),
            });
        }

        // 解析循环体
        let body = self.parse_expression()?;
        let end_span = body.span();
        let span = Span::new(start_span.start, end_span.end);

        Ok(Expr::ForIn {
            var,
            start: Box::new(start),
            end: Box::new(end),
            body: Box::new(body),
            span,
        })
    }
}
