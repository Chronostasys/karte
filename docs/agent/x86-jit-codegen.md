# x86_64 JIT Codegen

## Calling Convention

Karte uses its own calling convention (not C ABI). Key registers:

| Register | ID | Role | Notes |
|----------|-----|------|-------|
| RAX | 0 | Return register | Also effect_payload_register |
| RCX | 1 | Caller-saved | General purpose |
| RDX | 2 | Caller-saved | General purpose |
| RBX | 3 | Callee-saved | effect_resume_temp |
| RSP | 4 | System stack | NOT usable for virtual regs |
| RBP | 5 | Callee-saved | General purpose |
| RSI | 6 | Caller-saved | General purpose |
| RDI | 7 | Caller-saved | General purpose |
| R8 | 8 | Caller-saved | effect_tag_register |
| R9 | 9 | Caller-saved | return_address (unused on x86) |
| R10 | 10 | vm_sp | Virtual stack pointer — RESERVED |
| R11 | 11 | vm_fp | Virtual frame pointer — RESERVED |
| R12 | 12 | Callee-saved | effect_stack_pointer |
| R13 | 13 | Callee-saved | General purpose |
| R14 | 14 | Callee-saved | General purpose |
| R15 | 15 | Callee-saved | General purpose |

**Rule**: `effect_tag_register`, `return_address`, `effect_resume_temp`, `effect_stack_pointer` must all differ from `vm_sp` (R10) and `vm_fp` (R11).

## Save/Restore Around C Function Calls

`save_call_clobbered_registers` saves ALL caller-saved registers (including vm_sp=R10, vm_fp=R11) to the system stack before calls, restores after.

## Epilogue Frame Management

LIR's `Sub/Add vm_sp, N` pairs handle frame space. JIT epilogue must NOT add extra frame skip.

## Effect Handler Compilation

`EffectLoweringPass` converts effect instructions into ordinary LIR using a handler stack and tag comparison loop. Uses: R12 (effect_stack), R8 (tag), RBX (resume temp), RAX (payload).
