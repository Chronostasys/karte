# karte-lir

Low-level Intermediate Representation (LIR) with linear instruction sequences for the Karte compiler.

## Overview

`karte-lir` is the final IR stage before code generation. It transforms the control flow graph (CFG) from MIR into a linear sequence of assembly-like instructions, ready for JIT compilation.

LIR is designed to be close to machine code while remaining target-independent, making the transition to native code or bytecode straightforward.

## Key Concepts

### Operand (`Operand`)

Represents instruction operands:
- **Register** - Virtual or physical registers
- **Immediate** - Constant values (numbers, booleans)
- **Memory** - Memory addresses and stack slots
- **Label** - Jump targets (basic block labels)

### Instruction (`Instruction`)

Assembly-like instructions:

**Data Movement:**
- `Mov` - Move data between registers/memory
- `LoadImmediate` - Load constant into register

**Arithmetic:**
- `Add`, `Sub`, `Mul`, `Div` - Binary arithmetic operations
- `Neg` - Negation

**Comparison:**
- `Cmp` - Compare two operands (sets flags)

**Control Flow:**
- `Jmp` - Unconditional jump
- `JmpIf` - Conditional jump (branch on condition)
- `Call` - Function call
- `Ret` - Return from function

**Memory Operations:**
- `Load` - Load from memory to register
- `Store` - Store from register to memory

**Struct Operations:**
- `StructAlloc` - Allocate struct (stack or heap)
- `StructFieldLoad` - Load struct field
- `StructFieldStore` - Store to struct field

**Reference Operations:**
- `CreateRef` - Create reference to value
- `LoadRef` - Dereference (load through reference)

### LIR Function (`LirFunction`)

A function as a linear instruction sequence:

```
LirFunction {
    name: "my_function",
    parameters: [Reg(0), Reg(1)],
    instructions: [
        Label("BB0"),
        LoadImmediate(Reg(2), 42),
        Add(Reg(3), Reg(0), Reg(2)),
        Ret(Some(Reg(3))),
    ],
    register_count: 4,
}
```

### LIR Program (`LirProgram`)

Complete program with metadata:
- Functions (linear instruction sequences)
- Struct definitions and layouts
- Enum definitions with tag layouts
- Optimization metadata

## MIR to LIR Lowering

The `lower` module flattens MIR's CFG into linear code:

### Transformations

1. **Basic Blocks → Labels** - Each basic block becomes a label
2. **CFG Edges → Jumps** - Control flow edges become jump instructions
3. **Values → Registers** - MIR values mapped to virtual registers
4. **Statements → Instructions** - MIR statements expanded to instruction sequences
5. **Terminators → Jumps** - Branch/Jump/Return terminators become control flow instructions

### Example

MIR CFG:
```
BB0:
  %1 = x > 10
  Branch(%1, BB1, BB2)

BB1:
  %2 = x * 2
  Jump(BB3)

BB2:
  %3 = x + 1
  Jump(BB3)

BB3:
  Return(%result)
```

LIR:
```
Label("BB0")
  Mov(Reg(0), Param(0))      // x
  LoadImmediate(Reg(1), 10)
  Cmp(Reg(0), Reg(1))
  JmpIf(Greater, "BB1")
  Jmp("BB2")

Label("BB1")
  LoadImmediate(Reg(2), 2)
  Mul(Reg(3), Reg(0), Reg(2))
  Mov(Reg(4), Reg(3))
  Jmp("BB3")

Label("BB2")
  LoadImmediate(Reg(5), 1)
  Add(Reg(6), Reg(0), Reg(5))
  Mov(Reg(4), Reg(6))
  Jmp("BB3")

Label("BB3")
  Ret(Some(Reg(4)))
```

## Struct Layout Management

### StructLayoutManager

Manages memory layout for structs:
- **Field Offsets** - Computes byte offsets for each field
- **Alignment** - Ensures proper field alignment (1, 2, 4, 8 bytes)
- **Padding** - Inserts padding bytes where needed
- **Total Size** - Calculates total struct size with alignment

```rust
use karte_lir::StructLayoutManager;

let manager = StructLayoutManager::new();
let layout = manager.compute_layout(&struct_type);

// Access field offset
let offset = layout.field_offset("x");
```

### Struct Instructions

- **`StructAlloc`** - Allocate struct on stack or heap
- **`StructFieldLoad`** - Load field at computed offset
- **`StructFieldStore`** - Store to field at offset

This design enables efficient struct operations without runtime overhead.

## Tagged Unions (Enums)

Enums are represented as tagged unions:
- **Tag** - Discriminator field (which variant)
- **Payload** - Union of all variant data

```
enum Color {
    Red,           // Tag = 0, no payload
    RGB(r, g, b)   // Tag = 1, payload = (number, number, number)
}

Memory layout:
[Tag: 1 byte][Padding][Payload: variant-dependent]
```

## Optimization Pipeline

LIR supports multiple optimization levels:

### Optimization Levels

- **None** - No optimizations, fastest compilation
- **Balanced** - Basic optimizations (const folding, dead code elimination)
- **Aggressive** - Advanced optimizations (mem2reg, etc.)

### Optimization Passes

Located in `pass/` module:

- **Dead Code Elimination** - Remove unused instructions
- **Constant Folding** - Evaluate constant expressions at compile time
- **Constant Propagation** - Propagate known constant values
- **Memory-to-Register (mem2reg)** - Promote stack allocations to registers
- **Register Coalescing** - Reduce register-to-register moves

### Running Optimizations

```rust
use karte_lir::{optimize_program, OptimizationLevel};

let optimized = optimize_program(lir_program, OptimizationLevel::Balanced);
```

## IR Serialization

LIR implements full text serialization:

```rust
use karte_lir::LirProgram;
use karte_ir_codec::{IrDisplay, IrParse};

// Serialize
let lir_text = lir_program.to_ir_string();

// Parse
let parsed = LirProgram::parse_ir(&lir_text).unwrap();

// Roundtrip testing
assert_eq!(parsed, lir_program);
```

### LIR Text Format

```
function main() -> number {
  BB0:
    mov #v0, #p0
    load_imm #v1, 42
    add #v2, #v0, #v1
    ret #v2
}
```

This format is used for:
- Debugging and inspection
- Snapshot testing
- Artifact persistence
- Cross-compilation

## Usage

### Lower MIR to LIR

```rust
use karte_lir::lower::lower_program;
use karte_mir::lower::lower_program as lower_to_mir;

// Lower HIR → MIR → LIR
let mir = lower_to_mir(&hir)?;
let lir = lower_program(&mir)?;

// LIR is now ready for codegen
```

### Manual LIR Construction

```rust
use karte_lir::*;

let mut function = LirFunction::new("add");
function.add_param(Register::Virtual(0));
function.add_param(Register::Virtual(1));

function.add_instruction(Instruction::Add {
    dest: Register::Virtual(2),
    left: Operand::Register(Register::Virtual(0)),
    right: Operand::Register(Register::Virtual(1)),
});

function.add_instruction(Instruction::Ret {
    value: Some(Operand::Register(Register::Virtual(2))),
});
```

## 模块结构

`karte-lir` 采用清晰的模块化设计，将 lowering 逻辑拆分为多个职责明确的模块：

```
karte-lir/
├── src/
│   ├── lib.rs                - 主模块和公共API
│   ├── ir.rs                 - LIR类型定义（Instruction、Operand等）
│   ├── struct_layout.rs      - 结构体内存布局管理
│   ├── tagged_union.rs       - 枚举/Tagged Union支持
│   ├── optimization_pipeline.rs - 优化流程编排
│   ├── pass/                 - 优化Pass
│   └── lower/                - MIR到LIR的降低模块（模块化结构）
│       ├── types.rs      (46行)   - Lowering上下文类型定义
│       ├── helpers.rs    (235行)  - 辅助函数（标签生成、函数名收集等）
│       ├── context.rs    (132行)  - 上下文管理（函数、标签）
│       ├── memory.rs     (837行)  - 内存与寄存器管理
│       ├── stmt.rs       (1149行) - 语句降低
│       ├── terminator.rs (274行)  - 终结器降低
│       └── lower.rs      (262行)  - 主模块协调器和公共API
```

### lower/ 模块详细说明

#### types.rs - 类型定义
- `LirLoweringContext` - LIR降低上下文结构体定义

#### helpers.rs - 辅助函数
- `stable_label_from_parts` - 稳定的标签ID生成
- `collect_function_names_*` - 从各种结构中收集函数名
- `value_to_key` - 值到字符串键的转换

#### context.rs - 上下文管理
- 上下文创建和初始化（Default trait、new()）
- 函数管理（start_function、finish_function）
- 标签管理（allocate_label_for_block、next_internal_label）
- 指令添加（add_instruction）

#### memory.rs - 内存与寄存器管理
- 结构体布局管理（set_struct_layout_for_value等）
- 栈分配（allocate_stack_slot_for_value等）
- 寄存器分配（allocate_register_for_value）
- L-Value/R-Value降级（lower_to_lvalue、lower_to_rvalue）
- 初始化（initialize_stack_value、handle_struct_value）
- 临时变量预分配（preallocate_temp_slots等）

#### stmt.rs - 语句降低
- `lower_statement` 函数
- 处理所有MIR Statement类型到LIR指令的转换
- 支持：Assign、BinaryOp、UnaryOp、Call、FieldAccess、Dereference等

#### terminator.rs - 终结器降低
- `lower_terminator` 函数
- 处理所有MIR Terminator类型到LIR指令的转换
- 支持：Return、Goto、Branch、Match

## Modules

- **`ir`** - LIR type definitions (Instruction, Operand, etc.)
- **`lower`** - MIR to LIR lowering transformation (modularized into types, helpers, context, memory, stmt, terminator)
- **`lower_instructions`** - Instruction lowering helpers
- **`struct_layout`** - Struct memory layout manager
- **`tagged_union`** - Enum/tagged union support
- **`optimization_pipeline`** - Optimization orchestration
- **`pass`** - Individual optimization passes

## Integration in Compilation Pipeline

```
[karte-mir] → Control Flow Graph
    ↓
[karte-lir] → Linear Instructions  ← You are here
    ↓
[karte-codegen] → Machine Code / Bytecode
```

LIR output feeds into [`karte-codegen`](../karte-codegen), which performs register allocation and generates JIT-compiled native code.

## Register Allocation

LIR uses virtual registers during lowering. The codegen stage performs register allocation to map virtual registers to physical machine registers, inserting spills/reloads as needed.

## Design Philosophy

- **Target Independence** - LIR doesn't assume specific CPU architecture
- **Optimization Ready** - Instructions designed for easy analysis and transformation
- **Explicit Memory** - All memory operations are explicit
- **Linear Simplicity** - Straightforward sequence of instructions, no hidden control flow

## Testing

LIR includes extensive roundtrip tests:

```bash
cargo test -p karte-lir lir_roundtrip
cargo test -p karte-lir lir_parse
```

These ensure that LIR serialization/deserialization preserves semantics.

## Dependencies

- `karte-mir` - Input representation
- `karte-common` - Calling convention, ownership model
- `karte-ir-codec` / `karte-ir-derive` - IR serialization
