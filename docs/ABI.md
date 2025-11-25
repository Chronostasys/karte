# Karte ABI & Calling Convention

This document describes the Application Binary Interface (ABI) and Calling Convention used by the Karte language runtime and compiler.

## Register Usage (AArch64)

Karte uses a custom calling convention optimized for its algebraic effect system.

| Register | Alias | Usage | Preserved? |
|----------|-------|-------|------------|
| `x0` | `REG_RETURN` | Return value / Scratch | Caller-saved |
| `x1` | `REG_ARG0` / `REG_EFFECT_PAYLOAD` | 1st Argument / Effect Payload | Caller-saved |
| `x2` | `REG_ARG1` | 2nd Argument / Scratch | Caller-saved |
| `x3` | `REG_ARG2` | 3rd Argument / Scratch | Caller-saved |
| `x4` | `REG_ARG3` | 4th Argument / Scratch | Caller-saved |
| `x5` | `REG_RETURN_ADDRESS` | Return Address (Link Register) | Callee-saved |
| `x6` | `REG_STACK_POINTER` | Stack Pointer | Callee-saved |
| `x7` | `REG_FRAME_POINTER` | Frame Pointer | Callee-saved |
| `x10` | `REG_EFFECT_TAG` | Effect Tag | Caller-saved |
| `x12` | `REG_EFFECT_STACK_POINTER` | Effect Stack Pointer | Callee-saved |
| `x15` | `REG_EFFECT_RESUME_TMP` | Effect Resume Temporary | Caller-saved |

### Reserved Registers

* **`x1` (REG_EFFECT_PAYLOAD)**: Reserved for passing the payload when performing an effect operation. It is also used as the first argument register (`REG_ARG0`) for standard function calls. The register allocator does **not** use this register for general purpose allocation to ensure it is available for effect handling.
* **`x12` (REG_EFFECT_STACK_POINTER)**: Dedicated pointer to the effect handler stack.

### Scratch Registers

The following registers are used as temporary scratch registers by the register allocator for spill/reload operations:

* `x0` (REG_RETURN)
* `x2` (REG_ARG1)
* `x3` (REG_ARG2)
* `x4` (REG_ARG3)

Note that `x1` is excluded from the scratch pool.

## Stack Layout

The stack frame is organized as follows (growing downwards):

```text
+----------------------+ <- High Address
| Caller's Stack Frame |
+----------------------+
| Return Address       |
+----------------------+
| Saved Frame Pointer  |
+----------------------+ <- FP (x7)
| Local Variables      |
| ...                  |
+----------------------+
| Spill Slots          |
| ...                  |
+----------------------+
| Scratch Slots        |
| (for temp regs)      |
+----------------------+ <- SP (x6)
```

## Effect System ABI

When an effect is performed:

1. The effect tag is placed in `REG_EFFECT_TAG` (`x10`).
2. The effect payload (if any) is placed in `REG_EFFECT_PAYLOAD` (`x1`).
3. Control is transferred to the handler.

The `REG_EFFECT_PAYLOAD` register (`x1`) is strictly reserved from general allocation to facilitate this mechanism without complex shuffling.

### Return Convention Details

* **Internal Calls (JIT to JIT)**:
    * The return value is passed directly in `x0` (`REG_RETURN`).
    * The return address is passed in `x5` (`REG_RETURN_ADDRESS`).
    * The caller is responsible for retrieving the value from `x0` immediately after the call returns.

* **Host Calls (JIT to Host / Host to JIT)**:
    * When the JIT code returns to the Host (Rust Runtime), it checks if `REG_RETURN_ADDRESS` (`x5`) is 0.
    * If `x5 == 0`, it treats the return as a "Return to Host".
    * In this case, the return value in `x0` is written to the **Host Return Slot** (a pointer managed by the VM entry trampoline).
    * This distinction ensures that internal recursive calls don't accidentally overwrite the host's return buffer.
