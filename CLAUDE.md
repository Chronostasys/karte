# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Karte is a functional programming language compiler implemented in Rust. It's a multi-stage compiler with a rich type system, supporting features like algebraic data types, pattern matching, references, and a module system.

The project uses a Rust workspace with 18 crates, implementing a complete compiler pipeline from lexing through type checking to code generation (JIT and AOT compilation).

**Main Crates**:
- `karte-lexer`, `karte-parser`, `karte-hir`, `karte-mir`, `karte-lir`, `karte-codegen` - Compilation pipeline
- `karte-module-system` - Multi-module compilation and caching
- `karte-escape-analysis` - Compile-time memory optimization
- `karte-rt` - Runtime and JIT memory management
- `karte-aot` - AOT compilation (generates standalone ELF executables, no glibc dependency)
- `karte-syscall` - Raw syscall wrappers (x86_64 + AArch64, no libc)
- `karte-cli` - Command-line interface
- `karte-tests` - Integration test suite
- `karte-ir-codec`, `karte-ir-derive` - IR serialization infrastructure
- `karte-diagnostics` - Error reporting

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

# AOT compile to standalone executable (no glibc dependency)
cargo run -- aot "42" -o test_binary
cargo run -- aot input.karte -o output_binary
./test_binary  # Run directly, returns exit code as result
```

### Testing

```bash
# Run all tests in workspace
cargo test
# With release mode (faster, recommended for full test suite)
cargo test --workspace --release

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

# Run specific integration test
cargo test -p karte-tests --lib cli_integration_tests::cli_tests::test_function_as_value

# Run tests matching a pattern
cargo test -p karte-tests --lib higher_order

# Run module system tests
cargo test -p karte-tests cli_integration
cargo test -p karte-module-system

# Run LIR-specific tests
cargo test -p karte-lir lir_roundtrip
cargo test -p karte-lir lir_parse_unit

# Quiet mode (only show summary)
cargo test --workspace --release --quiet
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
- **Generic functions (let-polymorphism)**: Functions with unannotated parameters are generalized into type schemes, instantiated per call site (see below)
- Sum types (algebraic data types): `enum Color { Red, Green, Blue }`
- Product types (structs): `struct Point { x: number, y: number }`
- Reference types: `&T` with explicit dereferencing (`*ref`)
- Pattern matching with exhaustiveness checking
- Built-in types: `Bool`, `Option<T>`

#### Function and Closure Representation

**Unified Calling Convention**: All callable objects (functions and closures) are represented uniformly as closure structures in MIR/LIR:
- **Closure structure**: `{ function_ptr: pointer, env_ptr: pointer }`
- **Plain functions**: Wrapped with adapter functions (`function$wrapper`) that accept the closure calling convention
- **Calling convention**: `function_ptr(env_ptr, arg1, arg2, ...)`

This unified representation enables:
- Passing functions as parameters to higher-order functions
- Treating functions and closures interchangeably
- Simplified type checking for polymorphic function parameters

**Type Information Flow**:
- HIR type checker stores all expression types in `expr_types: HashMap<*const Expr, Type>`
- ParseResult includes `expr_types` field
- LoweringOptions must receive `expr_types` from ParseResult for correct function/closure handling
- If integration tests fail with function parameter issues, check that `expr_types` is being passed correctly

#### Generic Functions (Let-Polymorphism)

Karte 支持基于 **let-polymorphism** 的泛型函数。当函数参数省略类型标注时，类型检查器自动将其泛化为类型方案（`TypeScheme`），在每次调用时实例化为具体类型。

**核心数据结构** (`karte-hir/src/types.rs`):
- `TypeScheme { bound_vars: Vec<TypeVar>, body: Type }` — 将函数类型中的自由类型变量量化
- `bound_vars` 为被量化的类型变量列表，`body` 为原始函数类型

**工作机制**:
1. **Parser 层** (`karte-parser/src/statement.rs:659`): 函数参数类型标注从强制改为可选，允许 `fn id(x) { x }` 形式
2. **TypeChecker generalize** (`karte-hir/src/type_checker.rs:1985-1997`): 函数定义完成后，调用 `free_vars()` 收集函数类型中的自由类型变量，若非空则创建 `TypeScheme` 存入 `function_schemes`
3. **TypeChecker instantiate** (`karte-hir/src/type_checker.rs:150-158`): 引用泛型函数时（`Identifier` 节点），从 `function_schemes` 取出对应 `TypeScheme`，为每个 `bound_var` 生成 fresh `TypeVar` 并替换，得到该次调用的具体类型
4. **单态限制**: 当前支持单态使用（同一函数以一种类型调用）；多态调用（同一函数以不同类型调用）需要后续实现 MIR monomorphization pass

**示例**:
```karte
fn id(x) { x }
fn main() -> number {
    let a = id(42);
    let b = id(true);
    if b { a } else { 0 }
}
```
- `id` 的类型被 generalize 为 `∀a. a → a`
- `id(42)` 实例化为 `number → number`
- `id(true)` 实例化为 `bool → bool`（注：多态调用需要 monomorphization 支持）

**相关文件**:
| 文件 | 关键行 | 作用 |
|------|--------|------|
| `karte-hir/src/types.rs:602-611` | `TypeScheme` 定义 | 类型方案结构体 |
| `karte-hir/src/type_checker.rs:99` | `function_schemes` 字段 | 存储所有泛型函数的 TypeScheme |
| `karte-hir/src/type_checker.rs:150-158` | `instantiate()` | 实例化类型方案 |
| `karte-hir/src/type_checker.rs:902-905` | Identifier 推断 | 引用泛型函数时自动实例化 |
| `karte-hir/src/type_checker.rs:1985-1997` | generalize 逻辑 | 函数定义后生成 TypeScheme |
| `karte-parser/src/statement.rs:659` | 类型标注可选 | Parser 允许省略参数类型 |

### IR Serialization

The project uses custom derive macros (`karte-ir-derive`) and codec (`karte-ir-codec`) for serializing/deserializing intermediate representations:

- `IrDisplay` trait - Converts IR to text format
- `IrParse` trait - Parses IR from text format
- Supports roundtrip testing: source → IR → text → IR → execution

### JIT Architecture

**Recent Update (2025)**: The JIT compiler (`karte-rt`) was redesigned with a continuous memory allocation strategy that significantly improves performance and simplifies code generation.

**Continuous Memory Allocation**:
- Pre-allocates 128MB of contiguous virtual address space on startup using `mmap(PROT_NONE)`
- Virtual memory reservation doesn't consume physical memory until actually used
- On-demand physical memory commitment via `mprotect` when functions are compiled
- Functions are tightly packed in continuous memory for cache efficiency
- Uses relative jumps (AArch64 `BL` instruction with ±128MB range)
- 16-byte function alignment for optimal AArch64 instruction access

**Performance Benefits**:
- Eliminates complex address patching overhead for cross-function calls
- Reduces TLB pressure with continuous memory layout
- Improves I-Cache hit rate (adjacent functions are cache-friendly)
- Simplifies AArch64 compiler logic significantly

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

The compiler implements a sophisticated memory management system combining reference counting and garbage collection capabilities.

#### **Karte 虚拟栈架构** ⚠️ **重要**

Karte 使用**自定义调用约定**和**自分配虚拟栈**，这与传统的 C 语言系统栈有本质区别：

**虚拟栈实现**：
- `ExecutionEngine.virtual_stack`: `Vec<i64>` (64KB, 8192个8字节元素)
- 栈在**堆上分配**，不是系统栈
- 通过 `StackManager` 管理栈帧、局部变量和调用链
- 自定义的 `CallingConvention` 定义参数传递和寄存器保存规则

**GC 集成的关键点**：
1. **不能使用 C 栈遍历**：Karte 的调用约定与 C ABI 不兼容
2. **不需要 LLVM stackmap**：stackmap 是为 C 调用约定设计的
3. **直接扫描虚拟栈区间**：将整个 `virtual_stack` Vec 作为根区间
4. **保守扫描**：检查栈中的每个字（word），识别可能的 GC 对象指针

**正确的根扫描方式**：
```rust
// ✅ 正确：直接获取虚拟栈区间
let stack_start = engine.virtual_stack.as_ptr() as *const u8;
let stack_end = unsafe { stack_start.add(engine.virtual_stack.len() * 8) };
gc_register_stack_range(stack_start, stack_end);

// ❌ 错误：使用 C 栈遍历
// gc_malloc_fast_unwind(size, obj_type, rsp)  // rsp 指向 C 系统栈，不是 Karte 虚拟栈
```

详见 `GC_AND_ESCAPE_ANALYSIS_DESIGN.md` 第 1.3 节。

#### **Current RC Implementation**
- **Active RC system**: Full reference counting implementation with `retain()` and `release()` operations
- **Registry-based tracking**: Centralized registry tracks all allocations with their reference counts
- **Automatic cleanup**: Objects are automatically freed when reference count reaches zero
- **FFI interface**: C-compatible functions for JIT code: `karte_jit_runtime_retain()` and `karte_jit_runtime_release()`

#### **Immix GC Integration**
- **High-performance GC**: Based on the Immix algorithm with mark-sweep collection
- **128KB block structure**: Memory is divided into 32KB blocks with 128-byte lines for efficient allocation
- **Thread-local allocation**: Fast per-thread allocators reduce contention
- **Conservative stack scanning**: Automatically finds roots in stack and registers without complex registration
- **Object evacuation**: Optional feature to reduce memory fragmentation by moving live objects
- **Large object handling**: Separate allocator for objects >32KB
- **Platform support**: Works on Linux, macOS, and Windows

#### **Escape Analysis Integration**
- **Compile-time optimization**: Analyzes variable lifetimes to determine optimal allocation strategy
- **Stack allocation**: Non-escaping variables allocated on call stack for maximum performance
- **Heap allocation**: Escaping variables allocated via GC with automatic lifetime management
- **Closure support**: Sophisticated analysis of captured variables in closures
- **Loop-aware**: Handles variable lifetimes correctly in loop constructs

#### **Memory Allocation API**
```rust
// RC-based allocation (current)
let obj = runtime.allocate(size, ObjectType::Complex);
runtime.retain(obj);
runtime.release(obj);

// GC-based allocation (future)
let obj = gc_malloc(size, ObjectType::Complex.into());
// No manual retain/release needed - GC handles it automatically
```

#### **Ownership Semantics**
- **OwnershipKind enum**: Tracks ownership through HIR, MIR, and LIR stages
- **Reference types**: `&T` with explicit dereferencing via `*ref`
- **Stack-allocated objects**: Fast allocation with automatic cleanup on function return
- **Heap-allocated objects**: GC-managed with automatic lifetime tracking

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

### Writing Integration Tests

When adding integration tests in `karte-tests/src/cli_integration_tests.rs`:

```rust
// 1. Parse with type checking
let (parse_result, diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);

// 2. Extract AST and expr_types
let parse_result = parse_result.expect("No parse result");
let ast = parse_result.expr();

// 3. IMPORTANT: Pass expr_types to LoweringOptions
let options = LoweringOptions {
    known_functions: HashSet::new(),
    module_context: None,
    expr_types: parse_result.expr_types.clone(),  // Required for function/closure handling
};

// 4. Lower to MIR
let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");
```

**Critical**: Always pass `parse_result.expr_types.clone()` to LoweringOptions. Forgetting this will cause function-as-parameter tests to fail.

## Notable Recent Changes

Recent work includes:
- **Generic functions / let-polymorphism (2025-05-30)**: Added `TypeScheme` for generic function support. Parser allows optional parameter type annotations; TypeChecker generalizes functions with free type variables into type schemes and instantiates them per call site. Added 3 integration tests (identity, first, apply). Current limitation: monomorphic use only; polymorphic calls require future MIR monomorphization pass.
- **Function-as-parameter fix (2025-12-02)**: Implemented unified representation for functions and closures with wrapper functions to handle calling convention differences
- Refactored module system to support project mode
- Moved cache implementation from CLI to `karte-module-system`
- Added LIR parsing and roundtrip testing infrastructure
- Enhanced integration tests for module system (now 222 tests)
- Runner module extracted from main CLI for better organization
- Added 4 regression tests for higher-order function parameter passing

## Debugging Best Practices and Common Mistakes

### Case Study: Closure Call Bus Error (2025-12-02)

**Problem**: Bus error when executing nested closure calls like `let apply = |f, x| { f(x) }; let add_one = |n| { n + 1 }; apply(add_one, 5)`.

**Wrong Approach (What NOT to do)**:
1. ❌ **Assuming the bug is in the lowest layer** - Initially suspected the JIT compiler (AArch64 code generation) and spent time debugging assembly code and function prologue/epilogue
2. ❌ **Over-focusing on implementation details** - Analyzed stack pointer initialization, frame pointer setup, and calling conventions without verifying the higher-level semantics
3. ❌ **Skipping IR validation** - Jumped directly to debugging machine code without first checking if the LIR itself was correct
4. ❌ **Not using incremental testing** - Tried to debug the full pipeline end-to-end instead of testing each stage independently

**Correct Approach (What to do)**:
1. ✅ **Start from the highest abstraction level** - Check the generated IR first before diving into codegen
2. ✅ **Use IR execute commands for validation** - Test LIR directly with `cargo run -- execute <file>.lir` to isolate whether the bug is in IR generation or execution
3. ✅ **Modify IR manually to test hypotheses** - Create simplified test cases by hand-editing LIR files
4. ✅ **Understand the semantics** - Read and understand what the IR *should* be doing before debugging what it *is* doing
5. ✅ **Work backwards from symptoms** - When you see a bus error, check the memory address being accessed and trace back to what IR instruction generated it

**The Actual Bug**:
- In `1.lir` line 25-28, when creating a closure structure for parameter `f` in the `apply` function, the code incorrectly stored the function's own label (`@ id: L10903932721424011425`) instead of using the parameter value directly
- Should have been: `mov dst: #p4, src: #p2` (copy parameter to register)
- Was actually: Creating a new closure structure with self-reference

**Lesson Learned**:
- **Always validate your assumptions layer by layer** - Don't assume lower layers are buggy when higher layers might be wrong
- **Use the right tools for each layer** - Use `execute` command for IR bugs, use lldb only for codegen bugs
- **Simplify and isolate** - Create minimal test cases and modify them incrementally
- **Trust the existing code** - Well-tested components (like the JIT compiler) are less likely to be buggy than new features

**Debugging Methodology for Compiler Bugs**:
1. Reproduce the error
2. Export the failing case to IR (`--emit-lir` or check generated files)
3. Read and understand what the IR should be doing
4. Manually fix the IR and test with `execute` command
5. Once confirmed, find where in the compiler pipeline the wrong IR was generated
6. Fix the source of the bug (usually in HIR->MIR or MIR->LIR lowering)

### Case Study: Function-as-Parameter Type Information Loss (2025-12-02)

**Problem**: When passing plain functions as parameters (e.g., `apply(wrong_return, 5)` where `wrong_return` is a regular function), the code would crash with Bus error or return incorrect values because type information wasn't being passed from HIR to MIR.

**Root Cause Analysis**:
1. **Type Information Loss**: HIR type checker only stored Lambda expression types, not all expression types
2. **Runtime Polymorphism Challenge**: Lambda parameters like `f` in `|f, x| { f(x) }` have type variable (`Type::Var`), which could hold either:
   - A plain function pointer
   - A closure structure
3. **Calling Convention Mismatch**: Plain functions and closures have different calling conventions:
   - Plain function: `function(arg1, arg2, ...)`
   - Closure: `function_ptr(env_ptr, arg1, arg2, ...)`

**Solution: Unified Representation with Wrapper Functions**:
1. **Store All Expression Types**: Modified HIR type checker to store types for ALL expressions in `expr_types` HashMap
2. **Wrap Functions as Closures**: When a plain function is referenced, wrap it in a closure structure:
   ```
   Closure {
       function_ptr: wrapper_function,  // Points to wrapper, not original
       env_ptr: 0
   }
   ```
3. **Generate Wrapper Functions**: Create adapter lambdas that bridge calling conventions:
   ```rust
   // Original function
   fn add_one(n: number) -> number { n + 1 }

   // Generated wrapper
   fn add_one$wrapper(__env, __arg0) {
       add_one(__arg0)  // Don't pass __env to original function
   }
   ```
4. **Unified Call Logic**: All callable objects are now treated uniformly as closure structures

**Implementation Files**:
- `karte-hir/src/type_checker.rs:86-98`: Added `expr_types` field
- `karte-hir/src/type_checker.rs:1581-1587`: Store all expression types
- `karte-mir/src/lower/expr.rs:84-182`: Wrapper function generation logic
- `karte-parser/src/lib.rs:215-216`: Added `expr_types` to ParseResult
- `karte-tests/src/cli_integration_tests.rs`: Updated tests to pass `expr_types` from ParseResult to LoweringOptions

**Key Lessons**:
- ✅ **Type Information Must Flow Through Pipeline**: Integration tests failed because they weren't passing `expr_types` from ParseResult to LoweringOptions - a reminder to check the full pipeline
- ✅ **Unified Representation Simplifies Logic**: Using the same closure structure for all callables eliminates special cases
- ✅ **Test-Driven Development Works**: Created 6 comprehensive test cases before implementing, which caught regressions immediately
- ✅ **Incremental Implementation**: Broke the fix into 3 stages (store types → wrap functions → generate wrappers), validating each stage
- ✅ **Document As You Go**: Created detailed plan (TYPE_SYSTEM_FIX_PLAN.md) and summary (TYPE_SYSTEM_FIX_SUMMARY.md) documents

**Testing Strategy**:
- Created 6 targeted test cases covering: function as param, closure as param, typed function param, higher-order functions, and direct calls
- Ran full integration test suite (218 tests) to catch regressions
- Fixed example code that used outdated Lambda AST structure

**Performance Considerations**:
- Wrapper functions add one level of indirection (~<5% overhead)
- Future optimization: inline simple wrappers in LIR stage

### Case Study: CallIndirect Register Restore Bug (2025-12-03)

**Problem**: After calling lambda functions through indirect calls, heap-allocated variables were being dereferenced with incorrect addresses, causing wrong return values or crashes.

**Symptoms**:
- `test_debug_heap.karte`: Returns wrong value but doesn't crash
- `test_closure_escape_ub.karte`: Crashes with SIGSEGV (exit code 139)
- Pattern: Two closure calls followed by dereference operation

**Root Cause**: In `karte-lir/src/lower_instructions.rs:882-889`, the `CallIndirect` instruction implementation was incorrectly popping the return address from stack before restoring caller-saved registers, causing stack misalignment.

**Analysis**:
```
Correct stack layout after function call:
  [return address] ← SP should be here
  [saved #p4]
  [saved #p3]      ← Contains heap address
  [saved #p2]
  [saved #p1]

Wrong restore sequence (with extra pop):
  SP += 8          ← Incorrectly pop return address first
  Load #p4 from [SP]   ← Actually loading #p3's slot!
  Load #p3 from [SP+8] ← Actually loading #p2's slot!
  ...
  Result: #p3 gets wrong value, heap address lost
```

**Fix**: Comment out the premature return address pop in `CallIndirect` instruction lowering, matching the `Call` instruction behavior where `compile_return` already handles the return address.

```rust
// 从栈上弹出返回地址（丢弃）
// 🔧 修复：compile_return 已经负责弹出返回地址，这里不需要再次弹出
// 否则会导致栈不平衡，进而导致 caller-saved 寄存器恢复错误
// new_instructions.push(Instruction::Add { ... });
```

**Files Modified**:
- `karte-lir/src/lower_instructions.rs:881-891`: Fixed register restore logic

**Lesson Learned**:
- ✅ **Verify stack operations carefully**: Stack pointer manipulations must be symmetric
- ✅ **Check both Call and CallIndirect**: Indirect calls should match direct call conventions
- ✅ **Test with multiple call patterns**: Single calls may pass but multiple calls expose bugs

### Case Study: Parameter Register Allocation Bug in Function Calls (2025-12-04)

**Problem**: When passing multiple parameters to a function (both Call and CallIndirect), the parameter preparation code directly moved argument values to parameter registers sequentially. This caused register overwrites when later parameters referenced earlier parameter registers.

**Symptoms**:
- `test_debug_heap.karte`: Returns wrong value (pointer address instead of 22)
- Only affects calls with 3+ parameters where parameter N references a register that will be overwritten by parameter N+1

**Root Cause Analysis**:
The bug was in `karte-lir/src/lower_instructions.rs` in the parameter passing logic:
```rust
// ❌ Wrong approach - direct sequential moves cause register conflicts
for (i, op) in arg_operands.iter().enumerate() {
    if let Some(phys_reg) = self.calling_convention.argument_registers.get(i) {
        new_instructions.push(Instruction::Move {
            dst: Register::Physical(*phys_reg),  // #p2, #p3, #p4...
            src: op.clone(),  // May reference #p2, #p4, etc.
            span: *span,
        });
    }
}

// Example causing the bug:
// mov #p2, #p4  (param2 = value from #p4)
// mov #p3, #p2  (param3 = value from #p2, but #p2 was just overwritten!)
```

**Solution**: Use stack as intermediate storage to avoid register conflicts:
```rust
// ✅ Correct approach - use stack to preserve all values
// Step 1: Push all arguments onto stack
for op in arg_operands.iter() {
    new_instructions.push(Instruction::Sub {
        dst: self.stack_pointer_reg,
        src1: Operand::Register { id: self.stack_pointer_reg },
        src2: Operand::Immediate { value: 8 },
        span: *span,
    });
    new_instructions.push(Instruction::Store64 {
        addr: self.stack_pointer_reg,
        offset: 0,
        src: op.clone(),
        span: *span,
    });
}

// Step 2: Pop from stack to parameter registers (reverse order due to LIFO)
for i in (0..arg_operands.len()).rev() {
    if let Some(phys_reg) = self.calling_convention.argument_registers.get(i) {
        new_instructions.push(Instruction::Load64 {
            dst: Register::Physical(*phys_reg),
            addr: self.stack_pointer_reg,
            offset: 0,
            span: *span,
        });
        new_instructions.push(Instruction::Add {
            dst: self.stack_pointer_reg,
            src1: Operand::Register { id: self.stack_pointer_reg },
            src2: Operand::Immediate { value: 8 },
            span: *span,
        });
    }
}
```

**Why Stack-Based Approach?**:
- Cannot create new virtual registers at instruction lowering stage (register allocation already complete)
- Stack provides reliable intermediate storage
- Minimal performance overhead (a few extra stack operations)

**Files Modified**:
- `karte-lir/src/lower_instructions.rs:672-707`: Fixed Call parameter passing
- `karte-lir/src/lower_instructions.rs:837-872`: Fixed CallIndirect parameter passing

**Test Added**:
- `karte-tests/src/cli_integration_tests.rs::test_register_allocation_bug_multiple_closure_calls`: Regression test with multiple closure calls

**Lesson Learned**:
- ✅ **Instruction lowering runs after register allocation**: Cannot create new virtual registers at this stage
- ✅ **Stack is a reliable intermediate storage**: Use it when register conflicts are possible
- ✅ **Test with parameter-heavy scenarios**: Simple 1-2 parameter calls may pass while 3+ parameter calls fail
- ✅ **Always verify LIR output**: When debugging, check the generated LIR for correctness before blaming codegen

## Escape Analysis and Heap Allocation

Karte implements compile-time escape analysis to optimize memory allocation:

### Escape Analysis

**Location**: `karte-escape-analysis` crate

**Features**:
- **Escape point detection**: Detects when variables escape their scope (return, address-of, closure capture)
- **Heap allocation insertion**: Automatically inserts heap allocation at escape points
- **Stack optimization**: Non-escaping variables remain on stack

**Enabling Escape Analysis**:
```bash
KARTE_ENABLE_ESCAPE_ANALYSIS=1 ./target/release/karte run test.karte
KARTE_ENABLE_ESCAPE_ANALYSIS=1 ./target/release/karte build --emit-mir test.karte
```

**Example**:
```karte
fn main() -> number {
    let a = || {
        let d = 1;
        &d      // 🔍 Escape point: address-of operator
    };
    let ptr = a();
    *ptr        // ✅ Safe: d is heap-allocated
}
```

**Generated MIR**:
```mir
HeapAlloc { target = %10000, size = 8, object_type = escaped_value }
Store { target = %10000, value = %2 }
%0 = & value: %10000
```

**Key Implementation Details**:
- Escape point detection: `karte-escape-analysis/src/escape_point_detector.rs`
- Heap allocation transformation: `karte-escape-analysis/src/escape_point_transformer.rs`
- MIR to LIR lowering: `karte-lir/src/lower/memory.rs:314-335` handles `Value::Reference` with ID >= 10000

**Important Notes**:
- karte目前不支持注释，不要在karte源代码中加入任何注释
- bin不一定是最新的，执行命令之前一定先重新编译一下bin
- 如果想看逃逸分析的日志，请用类似 `karte --verbose <subcommand>` 这种格式
- build指令默认就会生成lir，用tail可以看到命令打印的lir位置
- 测试的时候请确保exit code是对的，而不是只看输出内容似乎正确
- 永远不要quit lldb的mcp，否则就需要我重新启动
- 不要release模式测试，直接debug
- 为了方便debug问题，我们故意添加了机制使得debug模式下gc会尽可能的触发回收并且每次都全量evacuation
- 任何测试的时候不要build --release之后测试，这样只会掩盖问题，而且大大延长编译时长
- 任何情况，除非我要求否则禁止build --release，release只会掩盖问题
- 不要在不是问题的行为上浪费时间，比如debug每次都gc就是设计好的行为，并不少它导致了错误，它只是拒绝掩盖错误。不要为了快速掩盖问题解决提出问题的人
- 禁止运行 `cargo build --release`命令，必须去掉--release
- 不允许cargo命令使用 --release flag除非我要求
- karte目前不支持注释，任何测试代码不要加测试
- 禁止任何时间对项目进行release编译，除非我要求
- **x86_64 GOTCHA**: `effect_tag_register`、`return_address` 等专用寄存器绝不能与 `vm_sp(R10)` 或 `vm_fp(R11)` 冲突，否则 EffectPerform 会直接破坏虚拟栈指针
- **SSA GOTCHA**: SSA rename_block_recursive 必须使用支配树子节点遍历（而不是 CFG 后继 + idom 检查），否则合并块会被遗漏导致寄存器使用未重命名
- **PHI GOTCHA**: MIR while/if-else 的 Phi 节点通过 `phi_store_map` 在 LIR 中用 Store64/Load64 传递值（span={MAX,MAX} 标记）。Memory2Reg 的 `transform_with_phi_support` fallback 在回溯 CFG 前驱链时必须在每个块检查已插入的 phi 节点，而非仅仅查找 Store——否则循环头中的 phi target 栈槽不会被正确替换，导致寄存器分配器将地址寄存器映射到值寄存器而 SIGSEGV。while 循环 Phi 的 incoming predecessor 必须是实际持有 Goto 终结符的块（if-else 的 merge_block），而非原始的 loop_body。
- **IF-ELSE PHI GOTCHA**: if-else 变量变异需要在 merge_block 插入 Phi 节点。预分析（while 循环第一步）期间必须跳过 Phi 生成（analysis_mode=true），且分析完成后必须清理孤立的分析块。变量绑定必须遍历所有作用域（ctx.scopes），因为 if-else 的 phi 更新可能在嵌套作用域中。
- **⚠️ 测试铁律**：
  - **禁止使用 `cargo test`**，必须且只能使用 `cargo nextest run` 运行测试
  - nextest 会为每个测试创建独立进程，SIGSEGV 不会中断整个测试套件，能真实反映所有失败
  - `cargo test` 在 SIGSEGV 时直接崩溃，grep 过滤 SEGV 后说"测试通过"是完全错误的
  - **任何代码修改后必须 `cargo nextest run` 全部通过后才能 commit**
  - 如果 nextest 有任何 FAIL 或 SIGSEGV，必须修复后才能提交，绝不许跳过

## Knowledge Files

- `docs/agent/x86-jit-codegen.md` — x86_64 JIT register conventions, save/restore, effect handler compilation
- `docs/agent/ssa-construction.md` — SSA construction pass, dominator tree traversal
- `docs/agent/aot-compilation.md` — AOT compilation architecture, ELF generation, runtime, syscall wrappers

## AOT Compilation Architecture

### Overview

The `karte-aot` crate generates standalone ELF64 executables from compiled Karte programs. The generated binaries have **no glibc dependency** — they use raw Linux syscalls for all OS operations.

### Architecture

```
Source → Lexer → Parser → HIR → MIR → LIR → [JIT: Execute] / [AOT: ELF Binary]
                                                      ↑                 ↑
                                              karte-codegen       karte-aot
                                              (compiles LIR        (packages into
                                               to machine code)    ELF executable)
```

**Key Design Decisions**:
1. **Reuses existing JIT backends** — `X86Compiler`/`AArch64Compiler` compile LIR to machine code, same as JIT
2. **Minimal runtime** — `_start` entry point, bump allocator, no-GC (for now), all using raw syscalls
3. **Direct ELF generation** — No external linker needed, writes ELF64 directly
4. **Cross-platform foundation** — `karte-syscall` provides raw syscall wrappers for both x86_64 and AArch64

### karte-syscall

Raw syscall wrappers with **no libc dependency**:
- `sys_write(fd, buf, count)` — write to file descriptor
- `sys_read(fd, buf, count)` — read from file descriptor
- `sys_exit(code)` — exit process
- `sys_mmap(...)` — memory mapping (used for virtual stack and heap)
- `sys_munmap(addr, len)` — unmap memory
- `sys_brk(addr)` — set program break
- x86_64: Uses `syscall` instruction
- AArch64: Uses `svc #0` instruction

### karte-aot

**Files**:
- `elf.rs` — ELF64 executable writer (headers, program headers, code/data segments)
- `runtime_x86.rs` — x86_64 runtime code generator (_start, bump allocator, runtime stubs)
- `runtime_aarch64.rs` — AArch64 runtime (placeholder)
- `compiler.rs` — AOT compiler orchestration (compile LIR → machine code → patch → ELF)

**Runtime Functions** (generated as raw machine code bytes):
- `_start` — Entry point: mmaps virtual stack (64KB) + heap (4MB), sets R10=vm_sp/R11=vm_fp, calls main, exits
- `__karte_alloc_aligned(size, align)` — Bump allocator using pre-mapped heap
- `__karte_free` — No-op (GC manages memory lifecycle)
- `__karte_retain/release/gc_safepoint/update_stack_top` — No-ops

**Compilation Flow**:
1. Generate runtime machine code (hand-coded x86_64 bytes)
2. Compile each Karte function using `X86Compiler` (same backend as JIT)
3. Patch runtime calls (replace JIT function pointers with AOT runtime addresses)
4. Patch cross-function jumps (resolve pending_jumps with absolute addresses)
5. Patch label addresses (resolve pending_label_addresses)
6. Patch _start's CALL main (set rel32 to main function offset)
7. Generate ELF with code segment (R+W+X) containing runtime + Karte functions

**ELF Layout**:
```
ELF Header (64 bytes)
Program Headers (1-2 PT_LOAD entries)
Padding to page boundary (0x1000)
Code Segment:
  Runtime code (_start, alloc, free, ...)
  Runtime global data (bump_ptr, heap_limit)
  Karte function code (main, add, ...)
```

### AOT Gotchas

- **AOT 跳转修补**: `emit_jump(Call)` 使用 JMP (E9) 而不是 CALL (E8) — Karte 的调用约定通过虚拟栈管理返回地址
- **运行时调用修补**: JIT 生成的运行时调用使用 `MOV RAX, imm64; CALL RAX` 模式 — 扫描并替换为 AOT 运行时地址
- **R10/R11 保存**: 第二个 mmap (堆) 会覆盖 R10/R11 (vm_sp/vm_fp) — 必须在 mmap 之间 push/pop
- **代码段必须可写**: 运行时全局变量 (bump_ptr) 使用 RIP-relative 寻址存储在代码段中
- **Extended registers**: R8-R15 需要 REX.B 前缀 — 运行时代码生成必须正确处理扩展寄存器编码

## Debugging Best Practices

### ⚠️ 遇到 AOT/JIT 执行结果不正确时，用 GDB 或反汇编工具看机器码，不要凭空推理

**原则**：当程序返回错误值时，不要猜测"可能是某个 pass 做了错误优化"或"可能是 x86 编码有问题"。直接反汇编 AOT binary 看实际生成的机器码。

**工具**：
```bash
# AOT binary 反汇编（ELF 无标准 section header，需要 capstone/ndisasm）
python3 -c "
from capstone import Cs, CS_ARCH_X86, CS_MODE_64
data = open('/tmp/test_bin','rb').read()
# 代码在 offset 0x1000, vaddr 0x401000
code = data[0x1000:0x1000+0x430]
md = Cs(CS_ARCH_X86, CS_MODE_64)
for i in md.disasm(code, 0x401000):
    print(f'0x{i.address:x}:  {i.mnemonic}  {i.op_str}')
"

# 找到 main 函数：在 _start 中搜索 call 指令
python3 -c "
from capstone import Cs, CS_ARCH_X86, CS_MODE_64
data = open('/tmp/test_bin','rb').read()
code = data[0x1000:0x1000+0x430]
md = Cs(CS_ARCH_X86, CS_MODE_64)
instrs = list(md.disasm(code, 0x401000))
for i in instrs:
    if i.mnemonic == 'call':
        target = int(i.op_str, 16)
        target_off = target - 0x401000
        print(f'main at 0x{target:x}')
        for j in md.disasm(code[target_off:target_off+128], target):
            print(f'0x{j.address:x}:  {j.mnemonic}  {j.op_str}')
        break
"
```

### Case Study: Bitwise AND 返回错误值 (2025-05-26)

**问题**: `12 bitand 10` 返回 10 而不是 8。

**错误方法（耗时 2 小时）**：
1. ❌ 猜测 ConstantFolding 没有正确工作，加了多个 debug print
2. ❌ 猜测 x86 AND 指令编码错误，反复检查 opcode
3. ❌ 在多个文件中加 eprintln! 追踪 ConstantFolding 的输入
4. ❌ 在 LIR optimization pipeline 中查找问题

**正确方法（耗时 5 分钟）**：
1. ✅ `aot` 编译生成 binary
2. ✅ 用 capstone 反汇编 main 函数
3. ✅ 直接看到问题：四条 `movabs rcx, imm` 全部写到 RCX（同一寄存器），然后 `and rcx, rcx`
4. ✅ 对比 `5 + 3` 的正确代码：`movabs rcx, 5` / `movabs rdx, 3` — src1/src2 在不同寄存器
5. ✅ 结论：SimpleStack fallback allocator 把 BitAnd 的两个操作数分配到了同一物理寄存器

**教训**：
- **反汇编是 debugging 机器码问题的第一工具**，不是最后手段
- 不要猜测编译器 pass 的行为——直接看最终生成的机器码
- 对比正确和错误的 case（Add 正确 vs BitAnd 错误）可以快速定位差异
- AOT binary 没有 section header，`objdump` 无法工作——用 capstone 或 ndisasm 的 raw 模式
