# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Karte is a functional programming language compiler implemented in Rust. It's a multi-stage compiler with a rich type system, supporting features like algebraic data types, pattern matching, references, and a module system.

The project uses a Rust workspace with 16 crates, implementing a complete compiler pipeline from lexing through type checking to code generation (JIT compilation).

## Development Commands

### Building and Running

```bash
# Build the entire workspace
cargo build --release

# Run the CLI (basic expression evaluation)
cargo run -- "let x = 5; x + 10"

# Run in project mode with module system
cargo run -p karte-cli -- run --mode project test_project/src/main.karte

# Enable verbose output to see compilation pipeline details
cargo run -- run --verbose <input>

# Export IR at different stages
cargo run -- export examples/demo.karte --output demo.lir
cargo run -- export "let x = 2; x * 3" --stage mir

# Execute from IR files
cargo run -- execute --stage mir demo.mir
cargo run -- execute demo.lir
```

### Testing

```bash
# Run all tests in workspace
cargo test

# Run tests for a specific package
cargo test -p karte-lexer
cargo test -p karte-parser
cargo test -p karte-hir
cargo test -p karte-mir
cargo test -p karte-lir
cargo test -p karte-codegen
cargo test -p karte-tests

# Run specific test patterns
cargo test test_type_check
cargo test sum_types
cargo test lir_parse
cargo test lir_roundtrip

# Run integration tests
cargo test -p karte-tests

# Run module system tests
cargo test -p karte-tests cli_integration
cargo test -p karte-module-system

# Run LIR-specific tests
cargo test -p karte-lir lir_roundtrip
cargo test -p karte-lir lir_parse_unit
```

## Architecture

### Compilation Pipeline

The compiler follows a traditional multi-stage pipeline:

1. **Lexer** (`karte-lexer`) - Tokenization
2. **Parser** (`karte-parser`) - Parses tokens into AST, handles module/import declarations
3. **HIR** (`karte-hir`) - High-level IR with type checking and semantic analysis
4. **MIR** (`karte-mir`) - Mid-level IR using control flow graphs (CFG) with basic blocks
5. **LIR** (`karte-lir`) - Low-level IR with linear instruction sequences (assembly-like)
6. **Codegen** (`karte-codegen`) - JIT execution

Each stage lowers the representation, making it progressively more explicit and closer to machine code.

### Module System Architecture

The module system (`karte-module-system`) implements a sophisticated multi-module compilation pipeline:

- **Manifest-based**: Projects use `karte.mod.toml` to declare modules and dependencies
- **Incremental compilation**: Uses fingerprint-based caching in `target/.karte-cache/`
- **Interface artifacts**: Each module exports `.interface.json` files containing type information
- **Topological compilation**: Modules are compiled in dependency order
- **Canonical naming**: All symbols use `module::name` format (e.g., `main::main`, `utils::add`)

#### Module System Entry Points

- `compile_entry_file()` - Main entry point for project-mode compilation
- `compile_module_in_layer()` - Compiles a single module with its dependencies
- Module graph is built from `karte.mod.toml` and validates for cycles

### Parser Modes

- **Script mode** (`ParserMode::Script`): Top-level statements/expressions are implicitly wrapped in `main` function
- **Project mode** (`ParserMode::Project`): Top-level only allows declarations (`fn`, `struct`, `enum`, `let`), requires explicit `main` function

### Key Type System Features

- Static type checking with inference
- Sum types (algebraic data types): `enum Color { Red, Green, Blue }`
- Product types (structs): `struct Point { x: number, y: number }`
- Reference types: `&T` with explicit dereferencing (`*ref`)
- Pattern matching with exhaustiveness checking
- Built-in types: `Bool`, `Option<T>`

### IR Serialization

The project uses custom derive macros (`karte-ir-derive`) and codec (`karte-ir-codec`) for serializing/deserializing intermediate representations:

- `IrDisplay` trait - Converts IR to text format
- `IrParse` trait - Parses IR from text format
- Supports roundtrip testing: source → IR → text → IR → execution

### JIT Architecture

The JIT compiler (`karte-rt`) uses a continuous memory allocation strategy:

- Pre-allocates 128MB of contiguous virtual address space on startup
- On-demand physical memory commitment via `mprotect`
- Functions are tightly packed in continuous memory for cache efficiency
- Uses relative jumps (AArch64 `BL` instruction with ±128MB range)
- 16-byte function alignment for optimal AArch64 instruction access

## Code Organization Principles

From `.cursor/rules/karte-rule.mdc`:
- This is a professional compiler project - all modifications should follow best practices
- No hacks or shortcuts in the code
- When making changes, first explain the approach, then modify the code
- Do not try to fix warnings unless you are asked to
- Use chinese to write comments and documentation

From `.cursor/rules/karte.mdc`:
- Keep changes incremental (200-300 lines at a time for large modifications)
- Update TODOS.md after code changes
- Add or update necessary documentation

## Important Implementation Details

### Memory Management

The compiler tracks ownership semantics via `OwnershipKind` (in `karte-common`):
- Used throughout HIR, MIR, and LIR for proper reference handling
- Critical for struct field access and reference operations

### Struct Layout

LIR includes a `StructLayoutManager` that:
- Computes field offsets with proper alignment
- Manages both stack and heap allocation
- Provides specialized instructions: `StructAlloc`, `StructFieldLoad`, `StructFieldStore`

### Control Flow Lowering

HIR control flow (`if`, `while`, `match`) is converted to explicit basic blocks in MIR:
- Each basic block has a sequence of statements and a terminator
- Terminators include: `Return`, `Jump`, `Branch`, `Switch` (for pattern matching)
- This CFG representation enables optimization passes before LIR lowering

### Optimization Pipeline

LIR supports optimization levels via `OptimizationLevel`:
- `None`, `Balanced`, `Aggressive`
- Controlled by `--optimization` CLI flag
- Pipeline defined in `karte-lir/src/optimization_pipeline.rs`

## Testing Infrastructure

The `karte-tests` crate contains:
- CLI integration tests (`cli_integration_tests.rs`)
- Type checker tests (`type_checker_tests.rs`)
- LIR roundtrip tests (`lir_roundtrip_tests.rs` in `karte-lir/tests/`)
- LIR parser unit tests (`lir_parse_unit_tests.rs` in `karte-lir/tests/`)

Integration tests validate the entire compilation pipeline from source to execution.

## Notable Recent Changes

Based on git status, recent work includes:
- Refactored module system to support project mode
- Moved cache implementation from CLI to `karte-module-system`
- Added LIR parsing and roundtrip testing infrastructure
- Enhanced integration tests for module system
- Runner module extracted from main CLI for better organization
