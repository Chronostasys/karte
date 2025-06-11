use karte_diagnostics::{DiagnosticBag, Span};
use karte_lexer::{Token, TokenWithSpan};
use std::fmt;

// 重新导出HIR中的类型，保持向后兼容性
pub use karte_hir::{type_check, BinaryOperator, Expr, Statement, Type, UnaryOperator};

/// 解析器错误
#[derive(Debug, Clone)]
pub enum ParseError {
    UnexpectedToken {
        expected: String,
        found: Token,
        span: Span,
    },
    UnexpectedEof {
        expected: String,
    },
    InvalidExpression {
        message: String,
        span: Span,
    },
    /// 缺少右括号
    MissingClosingParen {
        opening_span: Span,
        current_span: Span,
    },
    /// 缺少操作数
    MissingOperand {
        operator: String,
        operator_span: Span,
    },
    /// 表达式过于复杂（防止栈溢出）
    ExpressionTooDeep {
        max_depth: usize,
        span: Span,
    },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::UnexpectedToken {
                expected, found, ..
            } => {
                write!(f, "Expected {}, found {}", expected, found)
            }
            ParseError::UnexpectedEof { expected } => {
                write!(f, "Unexpected end of input, expected {}", expected)
            }
            ParseError::InvalidExpression { message, .. } => {
                write!(f, "Invalid expression: {}", message)
            }
            ParseError::MissingClosingParen {
                opening_span,
                current_span,
            } => {
                write!(
                    f,
                    "Missing closing parenthesis at {:?} (opened at {:?})",
                    current_span, opening_span
                )
            }
            ParseError::MissingOperand {
                operator,
                operator_span,
            } => {
                write!(
                    f,
                    "Missing operand for operator {} at {:?}",
                    operator, operator_span
                )
            }
            ParseError::ExpressionTooDeep { max_depth, span } => {
                write!(
                    f,
                    "Expression too deep (max depth: {}) at {:?}",
                    max_depth, span
                )
            }
        }
    }
}

/// 语法分析器
pub struct Parser<'a> {
    tokens: &'a [TokenWithSpan],
    position: usize,
    diagnostics: DiagnosticBag,
}

impl<'a> Parser<'a> {
    pub fn new(tokens: &'a [TokenWithSpan]) -> Self {
        Self {
            tokens,
            position: 0,
            diagnostics: DiagnosticBag::new(),
        }
    }

    /// 解析程序 - 可以是单个表达式或包含语句的块
    pub fn parse(&mut self) -> Option<Expr> {
        if self.tokens.is_empty() {
            self.diagnostics.add_error("Empty input", Span::new(0, 0));
            return None;
        }

        // 检查是否以let开头（语句模式）或其他（表达式模式）
        if self.starts_with_statement() {
            match self.parse_program() {
                Ok(expr) => Some(expr),
                Err(err) => {
                    self.add_parse_error(err);
                    None
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
                    Some(expr)
                }
                Err(err) => {
                    self.add_parse_error(err);
                    None
                }
            }
        }
    }

    /// 检查是否以语句开头
    fn starts_with_statement(&self) -> bool {
        if let Some(token) = self.peek() {
            if let Token::Identifier(name) = &token.token {
                return name == "let";
            }
        }
        false
    }

    /// 解析程序（语句序列）
    fn parse_program(&mut self) -> Result<Expr, ParseError> {
        let start_span = self.peek().unwrap().span;
        let mut statements = Vec::new();
        let mut final_expr = None;

        while let Some(token) = self.peek() {
            // 检查是否为语句
            if let Token::Identifier(name) = &token.token {
                if name == "let" {
                    statements.push(self.parse_statement()?);
                    continue;
                }
            }

            // 不是语句，尝试解析表达式作为最终表达式
            let expr = self.parse_expression()?;
            final_expr = Some(Box::new(expr));
            break;
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
    fn parse_statement(&mut self) -> Result<Statement, ParseError> {
        if let Some(token) = self.peek() {
            if let Token::Identifier(name) = &token.token {
                if name == "let" {
                    return self.parse_let_statement();
                }
            }
        }

        // 表达式语句
        let expr = self.parse_expression()?;
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
        }

        Ok(Statement::Expression { expr, span })
    }

    /// 解析let语句
    fn parse_let_statement(&mut self) -> Result<Statement, ParseError> {
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

    // expression = term (('+' | '-') term)*
    fn parse_expression(&mut self) -> Result<Expr, ParseError> {
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
    fn parse_term(&mut self) -> Result<Expr, ParseError> {
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

    // factor = ('+' | '-')? primary
    fn parse_factor(&mut self) -> Result<Expr, ParseError> {
        if let Some(token) = self.peek() {
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
                _ => self.parse_primary(),
            }
        } else {
            Err(ParseError::UnexpectedEof {
                expected: "expression".to_string(),
            })
        }
    }

    // primary = NUMBER | IDENTIFIER | LAMBDA | '(' expression ')' | function_call
    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        let mut expr = if let Some(token) = self.peek() {
            match &token.token {
                Token::Number(value) => {
                    let value = *value;
                    let span = token.span;
                    self.advance();
                    Ok(Expr::Number { value, span })
                }
                Token::Identifier(name) => {
                    // 在表达式上下文中，let不应该出现
                    let name = name.clone();
                    let span = token.span;
                    self.advance();
                    Ok(Expr::Identifier { name, span })
                }
                Token::Pipe => {
                    // 解析lambda表达式: |param1, param2| body
                    self.parse_lambda()
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
                _ => Err(ParseError::UnexpectedToken {
                    expected: "number, identifier, lambda, or '('".to_string(),
                    found: token.token.clone(),
                    span: token.span,
                }),
            }
        } else {
            Err(ParseError::UnexpectedEof {
                expected: "number, identifier, lambda, or '('".to_string(),
            })
        }?;

        // 处理函数调用，可能有连续的调用
        while let Some(token) = self.peek() {
            if matches!(token.token, Token::LeftParen) {
                self.advance(); // consume '('
                let mut args = Vec::new();

                // 如果不是立即遇到')'，解析参数列表
                if let Some(token) = self.peek() {
                    if !matches!(token.token, Token::RightParen) {
                        args.push(self.parse_expression()?);

                        // 解析剩余参数
                        while let Some(token) = self.peek() {
                            if matches!(token.token, Token::Comma) {
                                self.advance(); // consume ','
                                args.push(self.parse_expression()?);
                            } else {
                                break;
                            }
                        }
                    }
                }

                // 期望')'
                if let Some(token) = self.peek() {
                    if matches!(token.token, Token::RightParen) {
                        let end_span = token.span;
                        self.advance(); // consume ')'
                        let span = Span::new(expr.span().start, end_span.end);
                        expr = Expr::FunctionCall {
                            function: Box::new(expr),
                            args,
                            span,
                        };
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
            } else {
                break;
            }
        }

        Ok(expr)
    }

    /// 解析lambda表达式: |param1, param2| body
    fn parse_lambda(&mut self) -> Result<Expr, ParseError> {
        let start_span = self.peek().unwrap().span;
        self.advance(); // consume first '|'

        let mut params = Vec::new();

        // 解析参数列表
        while let Some(token) = self.peek() {
            match &token.token {
                Token::Identifier(name) => {
                    params.push(name.clone());
                    self.advance();

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
            span,
        })
    }

    fn peek(&self) -> Option<&TokenWithSpan> {
        self.tokens.get(self.position)
    }

    fn advance(&mut self) {
        if self.position < self.tokens.len() {
            self.position += 1;
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

/// 解析结果，包含AST和类型信息
pub struct ParseResult {
    pub expr: Expr,
    pub result_type: Type,
}

/// 便捷的解析函数（兼容性版本）
pub fn parse(tokens: &[TokenWithSpan]) -> (Option<Expr>, DiagnosticBag) {
    let mut parser = Parser::new(tokens);
    let expr = parser.parse();
    (expr, parser.into_diagnostics())
}

/// 带类型检查的解析函数
pub fn parse_with_type_check(tokens: &[TokenWithSpan]) -> (Option<ParseResult>, DiagnosticBag) {
    let mut parser = Parser::new(tokens);
    let expr = parser.parse();
    let mut diagnostics = parser.into_diagnostics();

    if let Some(expr) = expr {
        // 进行类型检查
        let (result_type, type_diagnostics) = type_check(&expr);

        // 合并诊断信息
        for error in &type_diagnostics.diagnostics {
            diagnostics.add_error(error.message.clone(), error.span);
        }

        (Some(ParseResult { expr, result_type }), diagnostics)
    } else {
        (None, diagnostics)
    }
}
