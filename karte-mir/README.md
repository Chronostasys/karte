# karte-mir

Mid-level Intermediate Representation (MIR) based on Control Flow Graph (CFG) for the Karte compiler.

## Overview

`karte-mir` is the second IR stage in the Karte compilation pipeline. It represents programs as explicit control flow graphs (CFGs), making data flow analysis and optimization straightforward.

MIR sits between the high-level type-checked representation (HIR) and the low-level linear instruction representation (LIR), providing a sweet spot for optimization passes.

## Key Concepts

### Value (`Value`)

Represents operands in MIR:
- **Constants** - Literal numbers, booleans
- **Locals** - Local variables and temporaries
- **Function references** - References to callable functions

### Statement (`Statement`)

Non-control-flow instructions that transform data:
- **Assign** - Variable assignments
- **BinaryOp** - Arithmetic and logical operations (`+`, `-`, `*`, `/`, `==`, `<`, etc.)
- **UnaryOp** - Unary operations (negation, logical NOT, dereference `*`)
- **Call** - Function calls
- **ConstructStruct** - Struct instantiation
- **ConstructEnum** - Enum variant construction
- **FieldAccess** - Struct field reads
- **CreateRef** - Reference creation (`&x`)

Each statement within a basic block executes sequentially without branching.

### Terminator (`Terminator`)

Control-flow-changing instructions at the end of basic blocks:
- **Return** - Exit function with value
- **Jump** - Unconditional jump to target basic block
- **Branch** - Conditional branch (if-then-else)
- **Switch** - Multi-way branch for pattern matching

### Basic Block (`BasicBlock`)

A sequence of statements followed by a terminator:

```
BasicBlock {
    statements: [Stmt1, Stmt2, Stmt3, ...],
    terminator: Jump(TargetBlock) | Branch(...) | Return(...) | Switch(...),
}
```

Basic blocks are atomic units of execution - once entered, all statements execute before the terminator takes effect.

### MIR Function (`MirFunction`)

A function represented as a collection of basic blocks:

```
MirFunction {
    name: "my_function",
    parameters: ["param1", "param2"],
    basic_blocks: [BB0, BB1, BB2, ...],
    entry_block: BB0,
}
```

### MIR Program (`MirProgram`)

Complete program representation:
- Collection of MIR functions
- Struct definitions
- Enum definitions
- Global metadata

## Control Flow Graph (CFG)

MIR makes control flow explicit through CFGs:

```
      BB0 (entry)
       |
      [stmt1, stmt2]
       |
    Branch(cond)
      /  \
    BB1  BB2
     |    |
  Jump  Jump
     \  /
      BB3
       |
    Return
```

### Benefits of CFG Representation

1. **Explicit Control Flow** - All branches and jumps are visible
2. **Easier Data Flow Analysis** - Compute reaching definitions, live variables, etc.
3. **Optimization Opportunities** - Dead code elimination, constant propagation, etc.
4. **Simpler Lowering to LIR** - CFG maps naturally to jump-based low-level code

## HIR to MIR Lowering

The `lower` module transforms HIR expressions into MIR basic blocks:

- **If expressions** → Branch terminator with true/false basic blocks
- **While loops** → Loop header, loop body, and exit blocks
- **Match expressions** → Switch terminator with case-specific blocks
- **Function calls** → Call statements + value captures
- **Nested expressions** → Temporary variables + sequential statements

### Example Lowering

HIR:
```karte
if x > 10 {
    x * 2
} else {
    x + 1
}
```

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
  %result = Phi(%2, %3)
  Return(%result)
```

## IR Serialization

MIR implements `IrDisplay` and `IrParse` for text serialization:

```rust
use karte_mir::MirProgram;
use karte_ir_codec::{IrDisplay, IrParse};

// Serialize to text
let mir_text = mir_program.to_ir_string();

// Parse from text
let parsed = MirProgram::parse_ir(&mir_text).unwrap();

// Roundtrip property
assert_eq!(parsed, mir_program);
```

This enables:
- **Debugging** - Inspect MIR in human-readable format
- **Testing** - Write tests using MIR text syntax
- **Persistence** - Save/load compiled artifacts

## Usage

### Lowering HIR to MIR

```rust
use karte_mir::lower::lower_program;
use karte_hir::type_check;

// Type-check program first
let hir = type_check(ast, ParserMode::Script)?;

// Lower to MIR
let mir = lower_program(&hir)?;

// MIR is now in CFG form
for function in &mir.functions {
    println!("Function: {}", function.name);
    for (bb_id, bb) in &function.basic_blocks {
        println!("  BB{}: {} stmts", bb_id, bb.statements.len());
    }
}
```

### Manual MIR Construction

```rust
use karte_mir::*;

// Build basic block
let mut bb = BasicBlock::new();
bb.push(Statement::Assign {
    target: LocalId(0),
    value: Value::Number(42),
});
bb.set_terminator(Terminator::Return {
    value: Some(Value::Local(LocalId(0))),
});

// Build function
let mut function = MirFunction::new("main");
function.add_basic_block(BasicBlockId(0), bb);
```

## 模块结构

`karte-mir` 采用清晰的模块化设计，将 lowering 逻辑拆分为多个职责明确的模块：

```
karte-mir/
├── src/
│   ├── lib.rs           - 主模块和公共API
│   ├── ir.rs            - MIR类型定义（Value、Statement、Terminator等）
│   ├── codec.rs         - IR序列化实现
│   └── lower/           - HIR到MIR的降低模块（模块化结构）
│       ├── types.rs     (82行)  - Lowering上下文类型定义
│       ├── context.rs   (267行) - 上下文管理（作用域、变量绑定）
│       ├── helpers.rs   (385行) - 辅助函数（运算符转换、所有权推断等）
│       ├── stmt.rs      (181行) - 语句降低
│       ├── expr.rs      (1062行) - 表达式降低
│       └── lower.rs     (594行) - 主模块协调器和公共API
```

### lower/ 模块详细说明

#### types.rs - 类型定义
- `VariableBinding` - 变量绑定信息（值、所有权、移动状态）
- `ScopeFrame` - 作用域帧
- `LoweringOptions` - Lowering选项
- `LoweringContext` - Lowering上下文

#### context.rs - 上下文管理
- 上下文创建和初始化
- 函数管理（开始、完成、获取当前函数）
- 基本块管理（创建、切换）
- 作用域管理（进入、退出、恢复）
- 变量绑定管理（绑定、查找、更新）
- 模块符号解析

#### helpers.rs - 辅助函数
- 运算符转换（HIR → MIR）
- 变量收集（用于闭包捕获分析）
- 所有权推断
- 模式转换（HIR Pattern → MIR Pattern）
- 堆布局推断

#### stmt.rs - 语句降低
支持的语句类型：
- Let语句
- Expression语句
- Assignment（变量赋值、字段赋值）
- TypeDef、StructDef、FunctionDef

#### expr.rs - 表达式降低
支持的表达式（最大模块）：
- 基础值、运算符
- 控制流（If, While, Block）
- 函数（Lambda, FunctionCall）
- 代数效应
- 模式匹配
- 结构体、数组
- 引用和堆操作

## Modules

- **`ir`** - MIR type definitions (Value, Statement, Terminator, BasicBlock, etc.)
- **`lower`** - HIR to MIR lowering transformation (modularized into types, context, helpers, stmt, expr)
- **`codec`** - IR serialization implementation

## Integration in Compilation Pipeline

```
[karte-hir] → Type-checked HIR
    ↓
[karte-mir] → Control Flow Graph  ← You are here
    ↓
[karte-lir] → Linear Assembly-like IR
    ↓
[karte-codegen] → Machine Code
```

MIR output feeds into [`karte-lir`](../karte-lir), where basic blocks are flattened into linear instruction sequences with explicit labels and jumps.

## Future Optimization Passes

MIR's CFG structure enables future optimizations:

- **Dead Code Elimination** - Remove unreachable basic blocks
- **Constant Propagation** - Propagate known values through CFG
- **Common Subexpression Elimination** - Deduplicate computations
- **Loop Optimizations** - Loop unrolling, invariant code motion
- **Inlining** - Inline small functions by merging CFGs

## Design Philosophy

- **Explicit Everything** - No hidden control flow or implicit operations
- **SSA-Ready** - CFG structure naturally supports SSA form extensions
- **Analysis-Friendly** - Designed for data flow and control flow analysis
- **Clean Lowering** - Straightforward transformation to/from HIR and LIR

## Dependencies

- `karte-hir` - Input representation (HIR)
- `karte-common` - Common types (`OwnershipKind`, etc.)
- `karte-ir-codec` / `karte-ir-derive` - IR serialization
