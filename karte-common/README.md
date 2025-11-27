# karte-common

Common types and conventions shared across the Karte compiler infrastructure.

## Overview

`karte-common` provides foundational types and conventions used throughout the Karte compilation pipeline, particularly for code generation and runtime execution.

## Modules

### Calling Convention (`calling_convention`)

Defines the function calling convention for the Karte virtual machine and JIT compiler. Based on System V ABI principles, adapted for RISC architectures.

#### Key Types

- **`Register`** - Virtual or physical register representation
  - `Virtual(usize)` - Virtual register (pre-allocation)
  - `Physical(u8)` - Physical machine register

- **`CallingConvention`** - Defines register usage rules
  - Argument registers: `r1-r4` (up to 4 arguments)
  - Return value register: `r0`
  - Stack pointer: `r6`
  - Frame pointer: `r7`
  - Return address: `r5`
  - Effect stack pointer: `r12`
  - Caller-saved registers: `r0-r4`
  - Callee-saved registers: `r5-r7, r12`

- **`CallContext`** - Function call execution context
  - Tracks argument count and return value
  - Manages register preservation across calls

#### Register Allocation

The standard calling convention provides:
- **4 argument registers** (`r1-r4`) for parameter passing
- **1 return value register** (`r0`)
- **Reserved registers** for stack management (`r5-r7`, `r12`)
- **Allocatable registers** (`r2`, `r3`, `r4`) for general use

#### Algebraic Effects Support

Special registers for algebraic effects:
- `REG_EFFECT_STACK_POINTER` (`r12`) - Effect handler stack
- `REG_EFFECT_PAYLOAD` (`r1`) - Effect value passing
- `REG_EFFECT_TAG` (`r10`) - Effect tag matching
- `REG_EFFECT_RESUME_TMP` (`r15`) - Resume continuation target

#### Usage Example

```rust
use karte_common::calling_convention::CallingConvention;

let convention = CallingConvention::standard();

// Get registers for a 3-argument function call
let arg_regs = convention.get_argument_registers(3);
assert_eq!(arg_regs, vec![1, 2, 3]);

// Check if a register needs to be saved by caller
assert!(convention.is_caller_saved(0)); // r0 (return value)
assert!(convention.is_callee_saved(6)); // r6 (stack pointer)
```

### Memory Management (`memory`)

Defines memory ownership models for heap allocations.

#### Key Types

- **`OwnershipKind`** - Heap allocation ownership strategy
  - `Manual` - Explicit memory management (malloc/free style)
  - `RefCounted` - Reference-counted memory management (retain/release)

#### Usage

The `OwnershipKind` is used throughout the IR to track how heap-allocated values (structs, closures) should be managed:

```rust
use karte_common::memory::OwnershipKind;

// For manual memory management
let manual_ownership = OwnershipKind::Manual;

// For reference-counted memory (future GC support)
let rc_ownership = OwnershipKind::RefCounted;
```

This information flows through:
- **HIR** - Semantic analysis tracks ownership
- **MIR** - Control flow preserves ownership semantics
- **LIR** - Code generation emits appropriate retain/release or manual free instructions
- **Codegen** - JIT handles actual memory operations

## Integration

`karte-common` is used throughout the compiler:

```
karte-hir ──┐
karte-mir ──┼─→ karte-common ←─┬── karte-lir
karte-lir ──┘                   └── karte-codegen
```

### IR Serialization

Both `Register` and `OwnershipKind` derive `IrCodec`, enabling text serialization:

```rust
use karte_common::calling_convention::Register;
use karte_ir_codec::IrDisplay;

let reg = Register::Virtual(42);
assert_eq!(reg.to_ir_string(), "#v 42");

let phys = Register::Physical(3);
assert_eq!(phys.to_ir_string(), "#p 3");
```

## Design Rationale

### Why These Conventions?

- **Limited arguments (4)**: Simplifies calling convention, encourages struct-based parameter passing for complex functions
- **Caller/Callee-saved split**: Balances register pressure across call boundaries
- **Dedicated stack registers**: Ensures stack integrity and simplifies frame management
- **Effect registers**: Enables efficient algebraic effect implementation without stack manipulation overhead

### Future Extensions

- **More argument registers** for optimization passes
- **SIMD/vector registers** for future numeric optimizations
- **Custom calling conventions** per-function for internal compiler optimizations

## Dependencies

- `karte-ir-derive` - Provides `IrCodec` derive macro for serialization
- `karte-ir-codec` - IR serialization runtime
