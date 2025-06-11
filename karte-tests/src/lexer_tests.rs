#[cfg(test)]
mod tests {
    use karte_lexer::*;

    #[test]
    fn test_tokenize_simple_expression() {
        let input = "1 + 2 * 3";
        let (tokens, diagnostics) = tokenize(input);

        assert!(diagnostics.is_empty());
        assert_eq!(tokens.len(), 5);

        assert_eq!(tokens[0].token, Token::Number(1));
        assert_eq!(tokens[1].token, Token::Plus);
        assert_eq!(tokens[2].token, Token::Number(2));
        assert_eq!(tokens[3].token, Token::Multiply);
        assert_eq!(tokens[4].token, Token::Number(3));
    }

    #[test]
    fn test_tokenize_with_parentheses() {
        let input = "(1 + 2) * 3";
        let (tokens, diagnostics) = tokenize(input);

        assert!(diagnostics.is_empty());
        assert_eq!(tokens.len(), 7);

        assert_eq!(tokens[0].token, Token::LeftParen);
        assert_eq!(tokens[1].token, Token::Number(1));
        assert_eq!(tokens[2].token, Token::Plus);
        assert_eq!(tokens[3].token, Token::Number(2));
        assert_eq!(tokens[4].token, Token::RightParen);
        assert_eq!(tokens[5].token, Token::Multiply);
        assert_eq!(tokens[6].token, Token::Number(3));
    }

    #[test]
    fn test_tokenize_error() {
        let input = "1 + @";
        let (tokens, diagnostics) = tokenize(input);

        assert!(diagnostics.has_errors());
        assert_eq!(tokens.len(), 2); // 只有 1 和 + 被正确识别

        assert_eq!(tokens[0].token, Token::Number(1));
        assert_eq!(tokens[1].token, Token::Plus);
    }

    #[test]
    fn test_tokenize_lambda() {
        let input = "|x, y| x + y";
        let (tokens, diagnostics) = tokenize(input);

        assert!(diagnostics.is_empty());
        assert_eq!(tokens.len(), 8);

        assert_eq!(tokens[0].token, Token::Pipe);
        assert_eq!(tokens[1].token, Token::Identifier("x".to_string()));
        assert_eq!(tokens[2].token, Token::Comma);
        assert_eq!(tokens[3].token, Token::Identifier("y".to_string()));
        assert_eq!(tokens[4].token, Token::Pipe);
        assert_eq!(tokens[5].token, Token::Identifier("x".to_string()));
        assert_eq!(tokens[6].token, Token::Plus);
        assert_eq!(tokens[7].token, Token::Identifier("y".to_string()));
    }

    #[test]
    fn test_tokenize_function_call() {
        let input = "add(1, 2)";
        let (tokens, diagnostics) = tokenize(input);

        assert!(diagnostics.is_empty());
        assert_eq!(tokens.len(), 6);

        assert_eq!(tokens[0].token, Token::Identifier("add".to_string()));
        assert_eq!(tokens[1].token, Token::LeftParen);
        assert_eq!(tokens[2].token, Token::Number(1));
        assert_eq!(tokens[3].token, Token::Comma);
        assert_eq!(tokens[4].token, Token::Number(2));
        assert_eq!(tokens[5].token, Token::RightParen);
    }
}
