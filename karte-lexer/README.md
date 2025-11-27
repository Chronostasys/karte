# karte-lexer

Lexical analyzer for the Karte programming language.

## Overview

`karte-lexer` is the tokenization component of the Karte compiler. It converts source code text into a stream of tokens with position information, which are then consumed by the parser.

The lexer is built on top of the [Logos](https://github.com/maciejhirsz/logos) library for efficient and maintainable tokenization.

## Features

- **Fast tokenization** using Logos regex-based lexing
- **Position tracking** with `Span` information for error reporting
- **Comprehensive token set** supporting:
  - Literals: numbers
  - Operators: arithmetic (`+`, `-`, `*`, `/`), comparison (`==`, `<`, `>`, etc.), logical (`&&`, `||`, `!`)
  - Delimiters: parentheses, braces, brackets
  - Lambda syntax: `|` and `->`
  - Reference operator: `&`
  - Module syntax: `::`
  - Pattern matching: `_`
  - Algebraic effects keywords: `perform`, `resume`, `handle`, `in`
- **Error collection** via `DiagnosticBag` for reporting lexical errors

## Token Types

The `Token` enum defines all token types recognized by the lexer:

### Literals
- `Number(i64)` - Integer literals (e.g., `42`, `123`)

### Operators
- Arithmetic: `+`, `-`, `*`, `/`
- Comparison: `==`, `<`, `>`, `<=`, `>=`
- Logical: `&&`, `||`, `!`
- Assignment: `=`

### Delimiters
- Parentheses: `(`, `)`
- Braces: `{`, `}`
- Brackets: `[`, `]`

### Punctuation
- `,` (Comma)
- `;` (Semicolon)
- `.` (Dot)
- `:` (Colon)
- `::` (DoubleColon)
- `_` (Underscore)

### Special Syntax
- `|` (Pipe) - Lambda parameter delimiter
- `->` (Arrow) - Lambda arrow
- `&` (Ampersand) - Reference operator

### Keywords
- Algebraic effects: `perform`, `resume`, `handle`, `in`
- Other keywords like `let`, `match`, `enum`, `struct`, etc. are initially lexed as identifiers and distinguished in the parser

### Identifiers
- `Identifier(String)` - Variable and function names matching `[a-zA-Z_][a-zA-Z0-9_]*`

## Usage

### Basic Tokenization

```rust
use karte_lexer::tokenize;

let source = "let x = 42";
let (tokens, diagnostics) = tokenize(source);

for token_with_span in tokens {
    println!("{:?} at {:?}", token_with_span.token, token_with_span.span);
}

if diagnostics.has_errors() {
    eprintln!("Lexical errors: {:?}", diagnostics);
}
```

### Using the Lexer Struct

```rust
use karte_lexer::Lexer;

let source = "x + y * 2";
let mut lexer = Lexer::new(source);
let tokens = lexer.tokenize();

if lexer.diagnostics().has_errors() {
    eprintln!("Errors: {:?}", lexer.diagnostics());
}
```

## Token with Span

Each token is wrapped in `TokenWithSpan`, which contains:
- `token: Token` - The actual token
- `span: Span` - Position information (start and end offsets) in the source

This enables precise error reporting and syntax highlighting.

## Error Handling

The lexer collects lexical errors (unexpected characters) in a `DiagnosticBag` rather than stopping at the first error. This allows reporting multiple errors in a single pass.

```rust
use karte_lexer::tokenize;

let source = "let x = 42 @@@";  // @ is not a valid token
let (tokens, diagnostics) = tokenize(source);

assert!(diagnostics.has_errors());
```

## Integration

`karte-lexer` is the first stage in the Karte compilation pipeline:

```
Source Code → karte-lexer → Tokens → karte-parser → AST → ...
```

The token stream is consumed by [`karte-parser`](../karte-parser) to construct the abstract syntax tree.

## Dependencies

- [`logos`](https://crates.io/crates/logos) - Fast lexer generator
- [`karte-diagnostics`](../karte-diagnostics) - Error reporting infrastructure
