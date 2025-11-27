# karte-hir

High-level Intermediate Representation (HIR) with type checking and semantic analysis for the Karte compiler.

## Overview

`karte-hir` is the first semantic analysis stage in the Karte compilation pipeline. It transforms the parser's abstract syntax tree (AST) into a type-annotated, semantically-validated representation.

HIR bridges the gap between syntax (what the code looks like) and semantics (what the code means), ensuring type safety before code generation begins.

## Key Responsibilities

1. **Type Checking** - Verify type correctness and infer types
2. **Semantic Analysis** - Validate program semantics (variable bindings, scoping, etc.)
3. **Type Annotation** - Attach type information to every expression
4. **Error Reporting** - Collect and report type errors and semantic issues

## Modules

### AST (`ast`)

Defines the HIR abstract syntax tree structures:

- **Expressions** (`HirExpr`) - Typed expressions (literals, variables, binary operations, function calls, lambdas, etc.)
- **Statements** (`HirStmt`) - Statements (let bindings, return, expressions)
- **Patterns** (`HirPattern`) - Pattern matching (identifiers, constructors, wildcards, references)
- **Declarations** (`HirDecl`) - Top-level declarations (functions, structs, enums, modules)
- **Programs** (`HirProgram`) - Complete program representation

Each HIR node carries type information and span data for precise error reporting.

### Type System (`types`)

Defines the type representation:

- **`Type`** - Core type enum
  - `Number` - Numeric type
  - `Bool` - Boolean type
  - `Function` - Function types with parameter and return types
  - `Struct` - Struct types with fields
  - `Enum` - Sum types (algebraic data types)
  - `Ref` - Reference types (`&T`)
  - `TypeVar` - Type variables for inference

- **`TypeVar`** - Type variable representation for polymorphism

The type system supports:
- Product types (structs)
- Sum types (enums/algebraic data types)
- Reference types with explicit dereferencing
- Generic types (via type variables)

### Type Checker (`type_checker`)

The core type checking engine:

- **`TypeChecker`** - Main type checker struct
  - Maintains type environment for variables and functions
  - Performs type inference via constraint solving
  - Validates pattern exhaustiveness
  - Checks reference usage correctness

- **`ModuleContext`** - Tracks module-level information
  - Exported symbols
  - Type definitions
  - Cross-module type checking

**Key Functions:**
- `type_check(program, mode)` - Type check a complete program
- `type_check_with_context(program, mode, context)` - Type check with external module context

### Errors (`errors`)

Type error reporting:

- **`TypeError`** - Type checking errors
  - Type mismatches
  - Undefined variables
  - Arity errors
  - Pattern match errors
  - Reference errors

## Type Checking Process

The type checking process follows these steps:

1. **Environment Setup** - Initialize type environment with built-in types
2. **Declaration Processing** - Process top-level declarations (structs, enums, functions)
3. **Type Inference** - Infer types for expressions using bidirectional type checking
4. **Constraint Solving** - Unify type constraints to resolve type variables
5. **Validation** - Verify reference usage, pattern exhaustiveness, etc.
6. **Annotation** - Attach resolved types to all AST nodes

## Usage

### Basic Type Checking

```rust
use karte_hir::{type_check, ModuleContext};
use karte_parser::{ParserMode, parse};

let source = "let x = 42; x + 10";
let ast = parse(source, ParserMode::Script).expect("Parse failed");

let hir = type_check(ast, ParserMode::Script)
    .expect("Type check failed");

// HIR now contains fully type-annotated expressions
```

### Module-Aware Type Checking

```rust
use karte_hir::{type_check_with_context, ModuleContext};

// Create context with external module types
let mut context = ModuleContext::new();
context.add_external_function("utils::add", /* function type */);

let hir = type_check_with_context(ast, ParserMode::Project, Some(context))
    .expect("Type check failed");
```

## Type System Features

### Sum Types (Algebraic Data Types)

```karte
enum Color {
    Red,
    Green,
    Blue,
    RGB { r: number, g: number, b: number }
}

match color {
    Color::Red => 0,
    Color::Green => 1,
    Color::Blue => 2,
    Color::RGB{r, g, b} => r + g + b,
}
```

### Product Types (Structs)

```karte
struct Point {
    x: number,
    y: number
}

let p = Point { x: 10, y: 20 };
p.x + p.y
```

### Reference Types

```karte
let x = 42;
let r = &x;   // r : &number
let y = *r;   // Explicit dereference
```

### Function Types

```karte
// Function type: (number, number) -> number
fn add(a: number, b: number) -> number {
    a + b
}

// Lambda type inferred from usage
let double = |x| x * 2;
```

## Integration in Compilation Pipeline

```
Source Code
    ↓
[karte-lexer] → Tokens
    ↓
[karte-parser] → AST
    ↓
[karte-hir] → Type-checked HIR  ← You are here
    ↓
[karte-mir] → Control Flow Graph
    ↓
[karte-lir] → Linear IR
    ↓
[karte-codegen] → Machine Code / Bytecode
```

HIR output feeds into [`karte-mir`](../karte-mir), where control flow structures are lowered to explicit basic blocks.

## Design Philosophy

- **Type Safety First** - All type errors must be caught before code generation
- **Precise Error Reporting** - Every error includes span information for helpful messages
- **Modular Type Checking** - Support for multi-module projects with interface files
- **Explicit References** - No implicit boxing or dereferencing
- **Algebraic Types** - First-class support for sum and product types

## Parser Mode Support

HIR respects two parser modes:

- **Script Mode** - Top-level expressions wrapped in implicit `main` function
- **Project Mode** - Explicit function declarations, requires `main` function

Type checking behavior adapts to the mode to provide appropriate validation.

## Dependencies

- `karte-parser` - Provides AST structures
- `karte-diagnostics` - Error reporting infrastructure
