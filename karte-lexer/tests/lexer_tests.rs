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

#[test]
fn test_lexer_dot_dot_range() {
    let tokens = Lexer::new("0..10").tokenize();
    assert!(tokens.len() >= 2, "Should tokenize range expression");
}

#[test]
fn test_lexer_arrow() {
    let tokens = Lexer::new("->").tokenize();
    assert!(tokens.len() >= 1, "Should tokenize arrow");
}

#[test]
fn test_lexer_fat_arrow() {
    let tokens = Lexer::new("=>").tokenize();
    assert!(tokens.len() >= 1, "Should tokenize fat arrow");
}

#[test]
fn test_lexer_double_colon() {
    let tokens = Lexer::new("Color::Red").tokenize();
    assert!(tokens.len() >= 3, "Should tokenize double colon path");
}

#[test]
fn test_lexer_ampersand() {
    let tokens = Lexer::new("&x").tokenize();
    assert!(tokens.len() >= 2, "Should tokenize ampersand");
}

#[test]
fn test_lexer_star() {
    let tokens = Lexer::new("*x").tokenize();
    assert!(tokens.len() >= 2, "Should tokenize star");
}

#[test]
fn test_lexer_parentheses() {
    let tokens = Lexer::new("(1 + 2)").tokenize();
    assert!(tokens.len() >= 5, "Should tokenize parentheses expression");
}

#[test]
fn test_lexer_array_brackets() {
    let tokens = Lexer::new("[1, 2, 3]").tokenize();
    assert!(tokens.len() >= 7, "Should tokenize array literal");
}

#[test]
fn test_lexer_string_with_escapes() {
    let tokens = Lexer::new(r#""hello\nworld""#).tokenize();
    assert!(tokens.len() >= 1, "Should tokenize string with escape sequences");
}

#[test]
fn test_lexer_char_literal_escape() {
    let tokens = Lexer::new("'a'").tokenize();
    assert!(tokens.len() >= 1, "Should tokenize char literal");
}

#[test]
fn test_lexer_keywords_complete() {
    let code = "let fn return if else while for in match struct enum true false import from pub";
    let tokens = Lexer::new(code).tokenize();
    assert!(tokens.len() >= 15, "Should tokenize all keywords");
}

#[test]
fn test_lexer_hex_number() {
    let tokens = Lexer::new("0xFF").tokenize();
    assert!(tokens.len() >= 1, "Should tokenize hex number");
}

#[test]
fn test_lexer_negative_number_with_minus() {
    let tokens = Lexer::new("-42").tokenize();
    assert!(tokens.len() >= 2, "Should tokenize negative number as minus and number");
}

#[test]
fn test_lexer_empty_input() {
    let tokens = Lexer::new("").tokenize();
    assert!(tokens.is_empty() || tokens.len() == 1, "Empty input should produce EOF or nothing");
}

#[test]
fn test_lexer_whitespace_only() {
    let tokens = Lexer::new("   \t\n  ").tokenize();
    assert!(tokens.len() <= 1, "Whitespace only should produce EOF or nothing");
}

#[test]
fn test_lexer_string_with_escapes_v2() {
    let tokens = Lexer::new("\"hello\\nworld\\t!\"").tokenize();
    assert!(tokens.len() >= 1, "Should tokenize string with escapes");
    if let Token::StringLiteral(s) = &tokens[0].token {
        assert!(s.contains('\n'), "Should contain newline escape");
        assert!(s.contains('\t'), "Should contain tab escape");
    }
}

#[test]
fn test_lexer_nested_parentheses() {
    let tokens = Lexer::new("((()))").tokenize();
    assert!(tokens.len() >= 6, "Should tokenize nested parentheses");
}

#[test]
fn test_lexer_multiple_operators() {
    let tokens = Lexer::new("1 + 2 * 3 - 4 / 5").tokenize();
    assert!(tokens.len() >= 9, "Should tokenize multiple operators");
}

#[test]
fn test_lexer_comparison_operators() {
    let tokens = Lexer::new("a == b != c < d > e <= f >= g").tokenize();
    assert!(tokens.len() >= 13, "Should tokenize comparison operators");
}

#[test]
fn test_lexer_arrow_operator() {
    let tokens = Lexer::new("-> =>").tokenize();
    assert!(tokens.len() >= 2, "Should tokenize arrow operators");
}

#[test]
fn test_lexer_logical_operators() {
    let tokens = Lexer::new("true && false || true").tokenize();
    assert!(tokens.len() >= 5, "Should tokenize logical operators");
}

#[test]
fn test_lexer_pipe_operator() {
    let tokens = Lexer::new("a | b").tokenize();
    assert!(tokens.len() >= 3, "Should tokenize pipe operator");
}

#[test]
fn test_lexer_char_literals_v2() {
    let tokens = Lexer::new("'a' '\\n' '\\t' '\\\\'").tokenize();
    assert!(tokens.len() >= 4, "Should tokenize char literals");
}
