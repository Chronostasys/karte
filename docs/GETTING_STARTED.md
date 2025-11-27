# Getting Started with Karte

A 5-minute quick start guide to building and using the Karte compiler.

## Prerequisites

- **Rust**: 1.70 or later
- **Cargo**: Comes with Rust

Verify your installation:
```bash
rustc --version
cargo --version
```

## Installation

### Clone the Repository

```bash
git clone https://github.com/your-org/karte.git
cd karte
```

### Build the Compiler

```bash
# Debug build (faster compilation)
cargo build

# Release build (optimized)
cargo build --release
```

The CLI tool will be available at `target/debug/karte-cli` (or `target/release/karte-cli`).

## Quick Examples

### 1. Basic Expression Evaluation

```bash
# Simple arithmetic
cargo run -- "2 + 3 * 4"
# Output: 14

# Variable bindings
cargo run -- "let x = 42; x * 2"
# Output: 84
```

### 2. Functions and Lambdas

```bash
# Define and call a lambda
cargo run -- "let double = |x| x * 2; double(21)"
# Output: 42

# Multi-parameter function
cargo run -- "let add = |a, b| a + b; add(10, 32)"
# Output: 42
```

### 3. Pattern Matching

```bash
# Match on Option type
cargo run -- "match Some(42) { Some(x) -> x + 1, None -> 0 }"
# Output: 43

# Match on booleans
cargo run -- "match true { true -> 100, false -> 0 }"
# Output: 100
```

### 4. Structs and References

```bash
# Create and use a struct
cargo run -- "struct Point { x: number, y: number }; let p = Point { x: 3, y: 4 }; p.x + p.y"
# Output: 7

# Use references
cargo run -- "let x = 42; let r = &x; *r + 10"
# Output: 52
```

## Interactive REPL

Start the REPL for interactive exploration:

```bash
cargo run
```

Then enter expressions:
```
karte> let x = 10
()
karte> x * 2
20
karte> exit
```

## Multi-Module Projects

### Create a Project

1. Create a project structure:
```bash
mkdir my_project
cd my_project
mkdir src
```

2. Create `karte.mod.toml`:
```toml
[[modules]]
id = "utils"
sources = ["src/utils.karte"]

[[modules]]
id = "main"
sources = ["src/main.karte"]
deps = ["utils"]
```

3. Create `src/utils.karte`:
```karte
module utils

pub fn add(x: number, y: number) -> number {
    x + y
}

pub fn double(x: number) -> number {
    x * 2
}
```

4. Create `src/main.karte`:
```karte
module main

import utils::{add, double}

fn main() -> number {
    let x = add(10, 20);
    double(x)
}
```

5. Run your project:
```bash
cargo run -p karte-cli -- run --mode project src/main.karte
# Output: 60
```

### Incremental Compilation

Karte automatically caches compiled modules in `target/.karte-cache/`. Edit `src/utils.karte` and run again - only changed modules recompile!

## Viewing IR (Intermediate Representations)

### Export IR to Files

```bash
# Export to LIR (Low-level IR)
cargo run -- export "let x = 2; x * 3" --output demo.lir

# Export to MIR (Mid-level IR)
cargo run -- export "let x = 2; x * 3" --stage mir --output demo.mir
```

### Execute from IR

```bash
# Execute LIR directly
cargo run -- execute demo.lir

# Execute MIR (automatically lowered to LIR)
cargo run -- execute --stage mir demo.mir
```

### View IR in Console

```bash
# Show LIR output
cargo run -- run --emit-lir "let x = 42; x + 10"

# Show MIR output
cargo run -- run --emit-mir "let x = 42; x + 10"
```

## Optimization Levels

Control optimization during compilation:

```bash
# No optimization (fastest compilation)
cargo run -- run --optimization none examples/demo.karte

# Balanced optimization (default)
cargo run -- run --optimization balanced examples/demo.karte

# Aggressive optimization
cargo run -- run --optimization aggressive examples/demo.karte
```

Aggressive mode enables:
- Memory-to-register promotion (mem2reg)
- Constant propagation
- Dead code elimination
- Register coalescing

## Debugging

### Verbose Output

See detailed compilation pipeline:
```bash
cargo run -- run --verbose --mode project test_project/src/main.karte
```

This shows:
- Module fingerprints
- Interface file hashes
- Compilation order
- Canonical symbol names

### Heap Statistics

View memory allocation stats:
```bash
cargo run -- run --heap-stats "let x = 42; x + 10"
```

### Type Information

See inferred types:
```bash
cargo run -- run --verbose "let f = |x| x * 2; f(21)"
```

## Running Tests

```bash
# Run all tests
cargo test

# Test specific components
cargo test -p karte-parser    # Parser tests
cargo test -p karte-hir       # Type checking tests
cargo test -p karte-lir       # LIR tests
cargo test -p karte-tests     # Integration tests

# Test patterns
cargo test lir_roundtrip      # LIR serialization tests
cargo test sum_types          # Algebraic data types tests
```

## Next Steps

Now that you've got the basics:

1. **Explore Language Features**: See [README.md](../README.md) for complete language syntax
2. **Understand Architecture**: Read [ARCHITECTURE.md](./ARCHITECTURE.md) for compiler internals
3. **Write Your Own Modules**: Create multi-module projects using `karte.mod.toml`
4. **Inspect IR**: Use `--emit-mir` and `--emit-lir` to understand compilation stages
5. **Optimize Code**: Experiment with `--optimization aggressive`

## Common Issues

### Build Errors

If you see compilation errors:
```bash
# Clean and rebuild
cargo clean
cargo build
```

### Module Not Found

In project mode, ensure:
- `karte.mod.toml` exists in project root
- Module IDs match between manifest and source files
- Dependencies are listed in `deps` array

### Type Errors

Karte is statically typed. Common mistakes:
```karte
// ❌ Wrong: Can't call a number
let x = 42; x(10)

// ✅ Correct: Define a function
let f = |x| x * 2; f(10)

// ❌ Wrong: Reference without dereference
let x = 42; let r = &x; r + 10

// ✅ Correct: Explicit dereference
let x = 42; let r = &x; *r + 10
```

## Getting Help

- **Documentation**: See `docs/` directory for detailed guides
- **Examples**: Check `examples/` and `test_project/` for sample code
- **Tests**: Browse `karte-tests/` for usage examples
- **Issues**: Report bugs at https://github.com/your-org/karte/issues

Happy hacking with Karte! 🚀
