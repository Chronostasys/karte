use karte_lexer::Lexer;
use karte_lexer::Token;

#[test]
fn test_lexer_number_literal() {
    let tokens = Lexer::new("42").tokenize();
    assert!(!tokens.is_empty(), "Should have at least one token");
    assert!(matches!(tokens[0].token, Token::Number(42)));
}

#[test]
fn test_lexer_negative_number() {
    let tokens = Lexer::new("-42").tokenize();
    assert!(tokens.len() >= 2);
    // -42 is parsed as minus + 42
}

#[test]
fn test_lexer_string_literal() {
    let tokens = Lexer::new(r#""hello""#).tokenize();
    assert!(!tokens.is_empty());
    assert!(matches!(tokens[0].token, Token::StringLiteral(_)));
    if let Token::StringLiteral(s) = &tokens[0].token {
        assert_eq!(s, "hello");
    }
}

#[test]
fn test_lexer_char_literal() {
    let tokens = Lexer::new("'A'").tokenize();
    assert!(!tokens.is_empty());
    assert!(matches!(tokens[0].token, Token::CharLiteral(65)));
}

#[test]
fn test_lexer_identifier() {
    let tokens = Lexer::new("foo").tokenize();
    assert!(!tokens.is_empty());
    assert!(matches!(tokens[0].token, Token::Identifier(_)));
    if let Token::Identifier(s) = &tokens[0].token {
        assert_eq!(s, "foo");
    }
}

#[test]
fn test_lexer_keywords() {
    let tokens = Lexer::new("fn let if else while for in match return struct enum").tokenize();
    assert!(tokens.len() >= 10);
    // Some keywords (fn, let, if, else, while, match, struct, enum) are parsed as Identifier
    // Others (for, return, in) are parsed as separate token types (KwFor, KwReturn, KwIn)
}

#[test]
fn test_lexer_operators() {
    let tokens = Lexer::new("+ - * / %").tokenize();
    assert!(tokens.len() >= 5);
}

#[test]
fn test_lexer_boolean_keywords() {
    let tokens = Lexer::new("true false").tokenize();
    assert!(tokens.len() >= 2);
    assert!(matches!(tokens[0].token, Token::Identifier(_)));
}

#[test]
fn test_lexer_brackets() {
    let tokens = Lexer::new("{ } ( ) [ ]").tokenize();
    assert!(tokens.len() >= 6);
}

#[test]
fn test_lexer_arrows() {
    let tokens = Lexer::new("-> =>").tokenize();
    assert!(tokens.len() >= 2);
}

#[test]
fn test_lexer_colon() {
    let tokens = Lexer::new(": ::").tokenize();
    assert!(tokens.len() >= 2);
}

#[test]
fn test_lexer_dot() {
    let tokens = Lexer::new(". ..").tokenize();
    assert!(tokens.len() >= 2);
}

#[test]
fn test_lexer_escaped_string() {
    let tokens = Lexer::new(r#""hello\nworld""#).tokenize();
    assert!(!tokens.is_empty());
    if let Token::StringLiteral(s) = &tokens[0].token {
        assert!(s.contains('\n'), "Escaped newline should be converted");
    }
}

#[test]
fn test_lexer_escaped_char() {
    let tokens = Lexer::new("'\\n'").tokenize();
    assert!(!tokens.is_empty());
    if let Token::CharLiteral(c) = tokens[0].token {
        assert_eq!(c, '\n' as i64);
    }
}

#[test]
fn test_lexer_semicolon() {
    let tokens = Lexer::new("let x = 1; x").tokenize();
    assert!(tokens.len() >= 5);
}

#[test]
fn test_lexer_comma() {
    let tokens = Lexer::new("1, 2, 3").tokenize();
    assert!(tokens.len() >= 5);
}
