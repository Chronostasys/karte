# Karte Compiler Architecture

Comprehensive guide to the Karte compiler's internal architecture, design decisions, and implementation details.

## Overview

Karte is a multi-stage compiler for a functional programming language, implemented as a Rust workspace with 16 crates. The compiler follows a traditional pipeline architecture, progressively lowering high-level source code to executable machine code or bytecode.

### Design Philosophy

- **Explicit over Implicit**: All transformations, control flow, and memory operations are made explicit
- **Layered IR**: Multiple intermediate representations, each optimized for different analysis tasks
- **Type Safety First**: Comprehensive static type checking before code generation
- **Modular Architecture**: Clean separation of concerns across crates
- **Optimization-Friendly**: IR design supports analysis and transformation passes

## Compilation Pipeline

```
Source Code (.karte)
    ↓
┌─────────────────┐
│  karte-lexer    │  Tokenization
└─────────────────┘
    ↓ Tokens
┌─────────────────┐
│  karte-parser   │  Parsing → AST
└─────────────────┘
    ↓ AST
┌─────────────────┐
│   karte-hir     │  Type Checking → Typed HIR
└─────────────────┘
    ↓ HIR (High-level IR)
┌─────────────────┐
│   karte-mir     │  Control Flow → CFG
└─────────────────┘
    ↓ MIR (Mid-level IR, Basic Blocks)
┌─────────────────┐
│   karte-lir     │  Instruction Selection → Linear IR
└─────────────────┘
    ↓ LIR (Low-level IR, Assembly-like)
┌─────────────────┐
│ karte-codegen   │  Code Generation → Bytecode/JIT
└─────────────────┘
    ↓
Executable Code
```

### Stage 1: Lexical Analysis (`karte-lexer`)

**Input**: Source code string
**Output**: Token stream with position information

The lexer uses the [Logos](https://github.com/maciejhirsz/logos) library for efficient regex-based tokenization.

**Key Components**:
- `Token` enum - All language tokens (literals, operators, keywords, identifiers)
- `TokenWithSpan` - Token + source position (for error reporting)
- `Lexer` - Main tokenizer with diagnostic collection

**Features**:
- Whitespace skipping
- Error recovery (collects all lexical errors)
- Position tracking for every token

### Stage 2: Syntax Analysis (`karte-parser`)

**Input**: Token stream
**Output**: Abstract Syntax Tree (AST)

Recursive descent parser with operator precedence for expressions.

**Parser Modes**:
- **Script Mode**: Top-level expressions wrapped in implicit `main` function
- **Project Mode**: Only declarations allowed at top-level, explicit `main` required

**Key Components**:
- `Parser` struct - Main parsing logic
- Expression parsing with precedence climbing
- Statement and declaration parsing
- Module/import declaration handling

**AST Node Types**:
- Expressions: literals, variables, binary/unary ops, lambdas, calls, if/while, match, structs
- Statements: let bindings, returns
- Declarations: functions, structs, enums, modules

### Stage 3: Type Checking and Semantic Analysis (`karte-hir`)

**Input**: AST
**Output**: High-level Intermediate Representation (HIR) with complete type information

The type checker performs bidirectional type checking with constraint-based inference.

**Key Components**:
- `TypeChecker` - Main type checking engine
- `Type` enum - Type representation (Number, Bool, Function, Struct, Enum, Ref, TypeVar)
- `ModuleContext` - Cross-module type information
- Pattern exhaustiveness checker
- Reference usage validator

**Type System Features**:
- **Product Types** (structs): `struct Point { x: number, y: number }`
- **Sum Types** (enums): `enum Color { Red, Green, Blue }`
- **Function Types**: `fn(T1, T2) -> R`
- **Reference Types**: `&T` with explicit dereferencing
- **Type Inference**: Bidirectional with constraint solving

**Process**:
1. Build type environment from declarations
2. Infer expression types bottom-up
3. Unify type constraints
4. Check pattern exhaustiveness
5. Validate reference usage
6. Annotate AST with resolved types

### Stage 4: Control Flow Graph Construction (`karte-mir`)

**Input**: HIR (type-annotated AST)
**Output**: Mid-level IR with explicit control flow graph

MIR represents programs as graphs of basic blocks, making data flow analysis straightforward.

**Key Components**:
- `BasicBlock` - Sequence of statements + terminator
- `Statement` - Data operations (assign, binop, call, struct ops)
- `Terminator` - Control flow (return, jump, branch, switch)
- `Value` - Operands (constants, locals, function refs)

**Lowering Transformations**:
- **If expressions** → Branch terminator with true/false blocks
- **While loops** → Header block + body block + exit block
- **Match expressions** → Switch terminator with case blocks
- **Nested expressions** → Temporary variables + sequential statements

**CFG Benefits**:
- Explicit control flow edges
- Easy dominance analysis
- Straightforward data flow computation
- Natural SSA form representation

### Stage 5: Instruction Selection (`karte-lir`)

**Input**: MIR (CFG with basic blocks)
**Output**: Low-level IR as linear instruction sequence

LIR is assembly-like, target-independent, and optimized for final code generation.

**Key Components**:
- `Instruction` enum - Assembly-like instructions (mov, add, jmp, call, etc.)
- `Operand` - Registers, immediates, memory, labels
- `StructLayoutManager` - Struct memory layout computation
- Optimization pipeline

**Lowering Transformations**:
- Basic blocks → Labels
- Terminators → Jump instructions
- Values → Virtual registers
- Struct operations → Field offset calculations

**Optimization Passes**:
- **Dead Code Elimination** - Remove unreachable code
- **Constant Folding** - Evaluate constant expressions
- **Constant Propagation** - Propagate known values
- **Memory-to-Register (mem2reg)** - Promote allocations to registers
- **Register Coalescing** - Reduce unnecessary moves

**Optimization Levels**:
- `None`: No optimization (fastest compilation)
- `Balanced`: Basic optimizations (default)
- `Aggressive`: All optimizations (best performance)

### Stage 6: Code Generation (`karte-codegen`)

**Input**: LIR (linear instructions)
**Output**: JIT-compiled machine code

Execution backend:
- **JIT Compiler**: Native code generation (AArch64)

**JIT Architecture** (`karte-rt`):
- **Continuous Memory**: Pre-allocates 128MB contiguous virtual address space
- **On-Demand Commitment**: Physical memory committed via `mprotect` as needed
- **Relative Jumps**: Uses AArch64 `BL` instruction (±128MB range)
- **16-byte Alignment**: Optimal instruction cache performance
- **Function Packing**: Functions tightly packed for cache efficiency

**Register Allocation**:
- Virtual → Physical register mapping
- Spilling for register pressure
- Calling convention enforcement

## Module System

### Architecture

The module system (`karte-module-system`) enables multi-module projects with separate compilation and incremental builds.

**Key Concepts**:
- **Manifest**: `karte.mod.toml` declares modules and dependencies
- **Interface Files**: `.interface.json` contains exported types
- **Canonical Naming**: All symbols use `module::name` format
- **Topological Compilation**: Modules compiled in dependency order

### Compilation Flow

```
karte.mod.toml
    ↓
ModuleGraph
    ↓
Topological Sort
    ↓
┌─────────────────────┐
│ For each module:    │
│  1. Check cache     │
│  2. Compile if      │
│     needed          │
│  3. Generate        │
│     interface.json  │
│  4. Cache artifacts │
└─────────────────────┘
    ↓
Link and Execute
```

### Caching Strategy

**Cache Key Components**:
- Module ID
- Source file fingerprint (SHA-256)
- Dependencies' interface hashes
- Optimization level

**Cache Location**: `target/.karte-cache/`

**Cache Artifacts**:
- `{module}.interface.json` - Exported type information
- `{module}.lir` - Compiled LIR (future enhancement)

### Cross-Module References

Modules reference each other via canonical names:
```karte
// In module "utils"
pub fn add(x: number, y: number) -> number { x + y }

// In module "main"
import utils::{add}
fn main() -> number { add(10, 20) }  // Resolves to utils::add
```

The type checker uses `ModuleContext` to resolve external symbols during type checking.

## IR Serialization

All IR stages (HIR, MIR, LIR) support text serialization via custom derive macros.

**System Components**:
- `karte-ir-codec` - Runtime serialization traits (`IrDisplay`, `IrParse`)
- `karte-ir-derive` - Procedural macros (`#[derive(IrCodec)]`)

**Benefits**:
- **Debugging**: Inspect IR in human-readable format
- **Testing**: Write tests using IR text syntax
- **Roundtrip Verification**: Ensure serialization preserves semantics
- **Artifact Storage**: Save/load compiled IR

**Example**:
```rust
let mir_text = mir_program.to_ir_string();
let parsed = MirProgram::parse_ir(&mir_text).unwrap();
assert_eq!(parsed, mir_program);
```

## Memory Management

### Ownership Tracking

The compiler tracks memory ownership through all IR stages via `OwnershipKind`:
- **Manual**: Explicit memory management (malloc/free style)
- **RefCounted**: Reference-counted (future ARC/GC support)

This information flows:
1. **HIR**: Inferred during type checking
2. **MIR**: Preserved in struct operations
3. **LIR**: Emits appropriate retain/release instructions
4. **Codegen**: Generates actual memory operations

### Struct Layout

`StructLayoutManager` computes memory layout:
- Field offsets with proper alignment
- Padding insertion for alignment requirements
- Total size calculation
- Stack vs heap allocation decisions

## Diagnostics System

**Components** (`karte-diagnostics`):
- `Span` - Source position (start, end offsets)
- `Diagnostic` - Error/warning with position and message
- `DiagnosticBag` - Collection of diagnostics

**Integration**:
- Lexer: Unexpected character errors
- Parser: Syntax errors with position
- Type Checker: Type errors with spans
- Runtime: Execution errors

## Testing Infrastructure

### Test Organization

- **Unit Tests**: In each crate (lexer, parser, HIR, MIR, LIR, codegen)
- **Integration Tests**: `karte-tests` crate
  - CLI integration tests
  - Type checker tests
  - End-to-end compilation tests
- **Roundtrip Tests**: IR serialization/deserialization verification

### Test Execution

```bash
cargo test                    # All tests
cargo test -p karte-hir       # HIR tests only
cargo test lir_roundtrip      # LIR roundtrip tests
cargo test -p karte-tests     # Integration tests
```

## Common Data Structures

### Calling Convention (`karte-common`)

Defines register usage for function calls:
- **Argument registers**: r1-r4 (up to 4 args)
- **Return register**: r0
- **Stack pointer**: r6
- **Frame pointer**: r7
- **Return address**: r5
- **Caller-saved**: r0-r4
- **Callee-saved**: r5-r7, r12

### Types Shared Across Crates

- **`OwnershipKind`**: Memory management strategy
- **`Register`**: Virtual/physical register representation
- **`Span`**: Source code position
- **`OptimizationLevel`**: Optimization settings

## Future Enhancements

### Planned Features
- **Garbage Collection**: Replace manual memory management
- **More Optimizations**: Loop unrolling, inlining, LICM
- **Multi-Target Codegen**: x86-64, RISC-V support
- **LSP Server**: Language server protocol for IDE integration
- **Debugger Integration**: DWARF info generation

### Performance Improvements
- **Parallel Compilation**: Compile independent modules concurrently
- **Incremental Type Checking**: Cache type information across runs
- **Profile-Guided Optimization**: Use runtime profiles for optimization decisions

## Design Decisions Rationale

### Why Multiple IRs?

- **HIR**: Preserves high-level semantics for type checking
- **MIR**: Explicit CFG enables data flow analysis
- **LIR**: Linear form simplifies code generation

Each IR is optimized for specific compiler tasks.

### Why Explicit Dereferencing?

Explicit `*ref` prevents accidental reference usage and makes data flow obvious.

### Why Canonical Naming?

`module::symbol` format prevents naming conflicts and makes cross-module references unambiguous.

### Why Virtual Registers in LIR?

Virtual registers delay register allocation, allowing optimization passes without register constraints.

## Code Navigation

### Key Entry Points

- `karte-cli/src/main.rs` - CLI entry point
- `karte-cli/src/runner.rs` - Compilation and execution orchestration
- `karte-hir/src/type_checker.rs` - Type checking engine
- `karte-mir/src/lower.rs` - HIR → MIR lowering
- `karte-lir/src/lower.rs` - MIR → LIR lowering
- `karte-module-system/src/lib.rs` - Module graph and incremental compilation

### Critical Algorithms

- **Type Inference**: `karte-hir/src/type_checker.rs::infer_expr()`
- **CFG Construction**: `karte-mir/src/lower.rs::lower_expr()`
- **Instruction Selection**: `karte-lir/src/lower.rs::lower_basic_block()`
- **mem2reg Pass**: `karte-lir/src/pass/memory2reg.rs`

## References

- [Getting Started Guide](./GETTING_STARTED.md)
- [IR Codec Guide](./IR_CODEC_GUIDE.md)
- Individual crate READMEs in their respective directories
