# AOT Compilation Architecture

## Overview

The Karte AOT compiler (`karte-aot` crate) generates standalone ELF64 executables from compiled Karte programs. These executables have **zero runtime dependencies** — no glibc, no dynamic libraries, just raw Linux syscalls.

## Crates

### karte-syscall
Raw syscall wrappers with no libc dependency.

**Supported platforms**:
- x86_64 Linux: Uses `syscall` instruction
- AArch64 Linux: Uses `svc #0` instruction (syscall wrappers exist, AOT runtime not yet implemented)
- RISC-V 64 Linux: Uses `ecall` instruction (AOT runtime fully implemented)

**Available syscalls**:
- `sys_write(fd, buf, count)` → SYS_write
- `sys_read(fd, buf, count)` → SYS_read
- `sys_exit(code)` → SYS_exit
- `sys_exit_group(code)` → SYS_exit_group
- `sys_mmap(addr, len, prot, flags, fd, offset)` → SYS_mmap
- `sys_munmap(addr, len)` → SYS_munmap
- `sys_brk(addr)` → SYS_brk
- `sys_arch_prctl(code, addr)` → SYS_arch_prctl (x86_64 only)
- `sys_set_fs(addr)` → Thread-local storage setup (x86_64 only)

### karte-aot
AOT compilation crate that generates executable binaries.

**Files**:
- `elf.rs` — ELF64 writer (generates minimal ELF with PT_LOAD segments)
- `runtime_x86.rs` — x86_64 runtime code generator with tri-color mark-sweep-compact GC (3352 lines)
- `runtime_aarch64.rs` — AArch64 runtime (placeholder, not yet implemented)
- `runtime_riscv.rs` — RISC-V 64 runtime code generator with mark-sweep GC (799 lines)
- `compiler.rs` — Main AOT compiler for x86_64, AArch64, and RISC-V (1180 lines)

## Compilation Pipeline

```
karte-cli (aot subcommand)
  → runner::aot_compile()
    → compile_to_lir() [existing function]
    → OptimizationPipeline::optimize()
    → karte_aot::AotCompiler::compile_to_bytes()
      → [x86_64] X86Runtime::new().generate() + patch_internal_calls()
      → [RISC-V] RiscvRuntime::new().generate() + patch_globals() + patch_internal_calls()
      → X86Compiler/RiscvCompiler::compile_function() for each function
      → Patch runtime calls (x86_64: MOV RAX,imm64; CALL RAX → AOT addresses)
      → Patch pending_label_addresses (64-bit absolute address loads)
      → Patch pending_jumps (x86_64: rel32; RISC-V: JAL/B-type)
      → Patch _start CALL/JAL main (rel32 / J-type)
      → ElfWriter::generate() with appropriate ElfArch
```

## Runtime Architecture

### _start Entry Point

The `_start` function is the first code executed when the binary starts:

1. Save callee-saved registers (RBP, RBX, R12-R15)
2. `mmap(NULL, 512KB, PROT_READ|PROT_WRITE, MAP_PRIVATE|MAP_ANON, -1, 0)` → Virtual stack
3. Set R12 = vstack_base
4. Set R10 (vm_sp) = vstack_base + 524272 (top of stack)
5. Store sentinel at [R10]
6. Set R11 (vm_fp) = R10
7. Save R10/R11 to system stack (they get clobbered by next mmap)
8. `mmap(NULL, 4MB, PROT_READ|PROT_WRITE, MAP_PRIVATE|MAP_ANON, -1, 0)` → Heap
9. Restore R10/R11
10. Store heap_base to `__bump_ptr` global (RIP-relative)
11. Store heap_base + 4MB to `__heap_limit` global (RIP-relative)
12. Set RDI = R10 (vm_sp), RSI = R12 (stack_bottom)
13. `CALL main` (rel32, patched at link time)
14. Restore callee-saved registers
15. `exit_group(RAX)` syscall

### Bump Allocator

`__karte_alloc_aligned(size, alignment)`:
1. Load bump_ptr from global
2. Calculate new_ptr = old_ptr + size, aligned to 16
3. Check if new_ptr < heap_limit
4. If overflow: return 0
5. Update bump_ptr global
6. Return old_ptr (allocated memory)

**No free()**: Memory is allocated via bump pointer and never freed. This is acceptable for programs with bounded allocation. GC support will be added later.

### Global Data

Two 64-bit values stored in the code segment (accessed via RIP-relative addressing):
- `__bump_ptr`: Current heap allocation pointer (8 bytes)
- `__heap_limit`: Heap end address (8 bytes)

## Patching Strategy

### Runtime Call Patching

JIT-compiled code uses `emit_call_absolute` for runtime calls:
```
48 B8 <imm64>   ; MOV RAX, symbol_ptr  (JIT function pointer)
FF D0           ; CALL RAX
```

The AOT compiler scans for this pattern (`48 B8 ... FF D0`) and compares the 64-bit address against known runtime function pointers (obtained via `RuntimeIntrinsic::symbol_ptr()`). When matched, it replaces the address with the AOT runtime function's virtual address.

### Cross-Function Jump Patching

The JIT compiler generates `PendingJump` entries for cross-function calls:
- `emit_jump(Call, label)`: Generates `E9 00 00 00 00` (JMP rel32), `patch_position` points to E9 opcode
- `emit_jump(Unconditional, label)`: Same as above
- `emit_jump(Conditional*, label)`: Generates `0F 8x 00 00 00 00`, `patch_position` points to first byte

**Patch calculation**:
- For JMP/CALL (1-byte opcode): rel32 at `patch_position + 1`, target = `RIP_after + rel32`
- For conditional (2-byte opcode): rel32 at `patch_position + 2`

### Label Address Patching

`PendingLabelAddress` entries store 64-bit absolute addresses:
- `emit_label_address(label)`: Generates `48 B8 00 00 00 00 00 00 00 00`, `patch_position` points to the imm64 field
- Write 8 bytes directly at `patch_position`

## ELF Generation

The ELF writer (`elf.rs`) generates a minimal ELF64 executable:

```
Offset 0x0000: ELF Header (64 bytes)
Offset 0x0040: Program Headers (56 bytes × num_segments)
Offset 0x1000: Code Segment (R+W+X)
                - Runtime code
                - Runtime global data
                - Karte function code
Offset 0x2000: Data Segment (R+W, optional)
```

**Key constants**:
- Code base address: `0x400000`
- Data base address: `0x800000`
- Page size: `0x1000`
- Entry point: Start of code segment (offset 0)

**The code segment is R+W+X** because:
- Runtime global variables (bump_ptr, heap_limit) are stored in the code segment
- RIP-relative addressing is used to access these globals
- The code needs write access to update the bump pointer

## CLI Usage

```bash
# Compile expression to binary
karte aot "42" -o test

# Compile file to binary
karte aot input.karte -o output

# The binary runs directly
./test
echo $?  # Exit code is the program result
```

## Testing

```bash
# Simple expressions
karte aot "42" -o /tmp/t1 && /tmp/t1; echo $?  # → 42
karte aot "10 + 20 * 3" -o /tmp/t2 && /tmp/t2; echo $?  # → 70

# Let bindings
karte aot "let x = 5; let y = 10; x * y + 3" -o /tmp/t3 && /tmp/t3; echo $?  # → 53

# Function calls
cat > /tmp/test.karte << 'EOF'
fn add(a: number, b: number) -> number { a + b }
fn main() -> number { add(20, 30) }
EOF
karte aot /tmp/test.karte -o /tmp/t4 && /tmp/t4; echo $?  # → 50
```

## Known Limitations

1. **GC incomplete**: x86_64 has tri-color mark-sweep-compact GC (⚠️ compact phase has REP MOVSB bug — RDI not set to destination). RISC-V has mark-sweep but doesn't reclaim memory (only clears marks). AArch64 not implemented.
2. **No complete string I/O in AOT**: `print_string`, `print_number`, `print_bool` runtime functions exist in AOT but complex string formatting may have limitations.
3. **AArch64 not implemented**: Only x86_64 and RISC-V 64 are functional.
4. **Linux only**: macOS support (Mach-O) not yet implemented.
5. **No dynamic loading**: All code must be statically compiled into the binary.
6. **Register allocation warnings**: Complex programs may trigger "超出虚拟机范围的物理寄存器" warnings.

## Future Work

1. **AArch64 AOT**: Implement AArch64 runtime code generation
2. **macOS support**: Implement Mach-O writer
3. **Fix GC compact bug**: x86_64 REP MOVSB in gc_collect compact phase doesn't set RDI=dest (runtime_x86.rs:883-897)
4. **RISC-V GC reclaim**: Implement actual memory reclamation in sweep phase
5. **Built-in I/O**: Implement print/println using write(2) syscall
6. **Cross-compilation**: Support compiling for different target architectures
7. **Optimization**: Implement dead code elimination, inline small wrappers
