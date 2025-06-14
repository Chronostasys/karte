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
                return matches!(name.as_str(), "let" | "enum" | "struct");
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
                if matches!(name.as_str(), "let" | "enum" | "struct") {
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
                } else if name == "enum" {
                    return self.parse_enum_statement();
                } else if name == "struct" {
                    return self.parse_struct_statement();
                }
            }
        }

        // 表达式语句 - 这里可能是赋值或其他表达式
        let expr = self.parse_expression()?;
        
        // 如果是赋值表达式，将其转换为赋值语句
        match expr {
            Expr::Assignment { target, value, span } => {
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

    /// 解析enum语句
    fn parse_enum_statement(&mut self) -> Result<Statement, ParseError> {
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
    fn parse_struct_statement(&mut self) -> Result<Statement, ParseError> {
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

    /// 检查标识符是否为已知的构造器
    fn is_constructor(&self, name: &str) -> bool {
        // 识别内置构造器和可能的自定义构造器（首字母大写）
        matches!(name, "None" | "Some" | "Left" | "Right" | "Ok" | "Err" | "True" | "False") ||
        (name.chars().next().map_or(false, |c| c.is_uppercase()) && name.chars().all(|c| c.is_alphanumeric()))
    }
    
    /// 解析字段类型名，支持引用类型和泛型类型语法
    fn parse_field_type_name(&mut self) -> Result<String, ParseError> {
        self.parse_type_expression()
    }

    /// 解析类型表达式，支持嵌套的泛型类型
    fn parse_type_expression(&mut self) -> Result<String, ParseError> {
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
                _ => {
                    Err(ParseError::UnexpectedToken {
                        expected: "type name".to_string(),
                        found: token.token.clone(),
                        span: token.span,
                    })
                }
            }
        } else {
            Err(ParseError::UnexpectedEof {
                expected: "type name".to_string(),
            })
        }
    }

    /// 检查接下来的token是否看起来像struct字面量
    /// 通过向前看来判断 { 后面是否跟着 identifier: value 模式
    fn is_likely_struct_literal(&self) -> bool {
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
                    },
                    Token::RightBrace => {
                        // 空的 {} 也可能是struct字面量
                        true
                    },
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

    // expression = assignment
    fn parse_expression(&mut self) -> Result<Expr, ParseError> {
        self.parse_assignment()
    }

    // assignment = logical_or ('=' assignment)?
    fn parse_assignment(&mut self) -> Result<Expr, ParseError> {
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
    fn parse_logical_or(&mut self) -> Result<Expr, ParseError> {
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
    fn parse_logical_and(&mut self) -> Result<Expr, ParseError> {
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
    fn parse_comparison(&mut self) -> Result<Expr, ParseError> {
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
    fn parse_additive(&mut self) -> Result<Expr, ParseError> {
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

    // factor = ('+' | '-' | '&' | '*' | '!')? primary
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
                    } else {
                        // 检查是否为限定构造器 TypeName::Constructor
                        if let Some(next_token) = self.peek() {
                            if matches!(next_token.token, Token::DoubleColon) {
                                self.advance(); // consume '::'
                                
                                // 期望构造器名
                                if let Some(constructor_token) = self.peek() {
                                    if let Token::Identifier(constructor_name) = &constructor_token.token {
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
                                                    if matches!(close_token.token, Token::RightParen) {
                                                        let end_span = close_token.span;
                                                        self.advance(); // consume ')'
                                                        let full_span = Span::new(span.start, end_span.end);
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
                                                let full_span = Span::new(span.start, constructor_span.end);
                                                Ok(Expr::QualifiedConstructor {
                                                    type_name: name,
                                                    constructor_name,
                                                    arg: None,
                                                    span: full_span,
                                                })
                                            }
                                        } else {
                                            // 无参数限定构造器
                                            let full_span = Span::new(span.start, constructor_span.end);
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
                            } else if matches!(next_token.token, Token::LeftParen) && self.is_constructor(&name) {
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
                                        let field_name = if let Token::Identifier(field_name) = &token.token {
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
                Token::LeftBrace => {
                    // 解析块表达式: { stmt1; stmt2; expr }
                    let start_span = token.span;
                    self.advance(); // consume '{'

                    let mut statements = Vec::new();
                    let mut final_expr = None;

                    while let Some(token) = self.peek() {
                        if matches!(token.token, Token::RightBrace) {
                            break;
                        }

                        // 检查是否为语句
                        if let Token::Identifier(name) = &token.token {
                            if matches!(name.as_str(), "let" | "enum") {
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
                                // 这是一个表达式语句
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
                Token::LogicalNot => {
                    // 处理连续的否定操作符，如 !!true, !!!false
                    self.parse_factor()
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

        // 处理字段访问，可能有连续的访问
        while let Some(token) = self.peek() {
            if matches!(token.token, Token::Dot) {
                self.advance(); // consume '.'
                
                // 期望字段名
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
            } else {
                break;
            }
        }

        Ok(expr)
    }

    /// 解析match表达式: match expr { pattern1 -> expr1, pattern2 -> expr2, ... }
    fn parse_match(&mut self) -> Result<Expr, ParseError> {
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

    /// 解析模式
    fn parse_pattern(&mut self) -> Result<karte_hir::Pattern, ParseError> {
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
                                                let arg = if let Some(peeked) = self.peek() {
                                                    if matches!(peeked.token, Token::RightParen) {
                                                        None
                                                    } else {
                                                        Some(Box::new(self.parse_pattern()?))
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
                                                        let full_span = Span::new(span.start, end_span.end);
                                                        Ok(karte_hir::Pattern::QualifiedConstructor {
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
                                                // 无参数限定构造器模式
                                                let full_span = Span::new(span.start, constructor_span.end);
                                                Ok(karte_hir::Pattern::QualifiedConstructor {
                                                    type_name: name,
                                                    constructor_name,
                                                    arg: None,
                                                    span: full_span,
                                                })
                                            }
                                        } else {
                                            // 无参数限定构造器模式
                                            let full_span = Span::new(span.start, constructor_span.end);
                                            Ok(karte_hir::Pattern::QualifiedConstructor {
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
                            } else if matches!(next_token.token, Token::LeftParen) {
                                // 检查是否为构造器模式
                                self.advance(); // consume '('
                                let arg = if let Some(peeked) = self.peek() {
                                    if matches!(peeked.token, Token::RightParen) {
                                        None
                                    } else {
                                        Some(Box::new(self.parse_pattern()?))
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
                                        let full_span = Span::new(span.start, end_span.end);
                                        Ok(karte_hir::Pattern::Constructor {
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
                            } else {
                                // 变量模式或简单构造器
                                Ok(karte_hir::Pattern::Variable { name, span })
                            }
                        } else {
                            Ok(karte_hir::Pattern::Variable { name, span })
                        }
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

    /// 解析空参数lambda表达式: || body
    fn parse_lambda_no_params(&mut self) -> Result<Expr, ParseError> {
        let start_span = self.peek().unwrap().span;
        self.advance(); // consume '||'

        // 解析lambda体
        let body = self.parse_expression()?;
        let end_span = body.span();
        let span = Span::new(start_span.start, end_span.end);

        Ok(Expr::Lambda {
            params: Vec::new(), // 空参数列表
            body: Box::new(body),
            span,
        })
    }

    /// 解析lambda表达式: |param1, param2| body 或 |param1: type1, param2: type2| body
    fn parse_lambda(&mut self) -> Result<Expr, ParseError> {
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
            span,
        })
    }

    /// 解析if表达式: if condition then branch else branch
    fn parse_if(&mut self) -> Result<Expr, ParseError> {
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
    fn parse_while(&mut self) -> Result<Expr, ParseError> {
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

#[cfg(test)]
mod assignment_tests {
    use super::*;
    use karte_lexer::tokenize;
    use karte_hir::*;

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
                Expr::Assignment { target: inner_target, value: inner_value, .. } => {
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
                Expr::BinaryOp { op: BinaryOperator::Add, .. } => {
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
                Expr::BinaryOp { op: BinaryOperator::Add, left, right, .. } => {
                    match left.as_ref() {
                        Expr::Identifier { name, .. } => {
                            assert_eq!(name, "y");
                        }
                        _ => panic!("加法左侧应该是标识符 y"),
                    }
                    match right.as_ref() {
                        Expr::BinaryOp { op: BinaryOperator::Multiply, .. } => {
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
