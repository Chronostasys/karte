# 计划：用 Karte 实现 GC + RISC-V AOT 后端

> 生成时间：2026-05-26 23:15 CST
> 状态：执行中

## 背景与目标

1. **用 Karte 语言实现 GC**：将 AOT runtime 中手写 x86 机器码的 GC（mark-compact）改用 Karte 语言实现，编译为 AOT binary 的一部分。要求 AOT 编译不依赖 libc，完全自包含。
2. **RISC-V AOT 后端**：添加 RV64GC 后端，生成的 ELF 能在 `qemu-riscv64` 中运行。

## 现状分析

### GC 现状
- AOT GC 已用 x86 机器码实现（`runtime_x86.rs`），算法是 bump + mark-compact
- 对象头 8 字节：`[color:u8][obj_type:u8][pad:u16][total_size:u32]`
- 全局状态 6 个 u64：bump_ptr, heap_start, heap_limit, vstack_bottom, alloc_count, gc_threshold
- GC 函数：gc_alloc_aligned, gc_collect, gc_safepoint

### AOT 后端现状
- `JitCompiler` trait 是后端接口，`X86Compiler` 和 `AArch64Compiler` 是实现
- `AotCompiler` 协调 runtime 生成 + 函数编译 + 多轮 patching + ELF 组装
- `ElfWriter` 生成最小 ELF64，code_base=0x400000
- `runtime_x86.rs` 用 `CodeBuilder` 直接生成 x86 机器码字节

### RISC-V 需求
- RV64G（64-bit，IMAFD 扩展）
- Linux syscall ABI：a7=syscall_nr, a0-a5=args, ecall 指令
- 寄存器 x0-x31，指令固定 32 位
- QEMU 用户态：`qemu-riscv64 ./binary`

## 详细计划

### 阶段一：基础设施 — karte-syscall RISC-V 支持

- [ ] 1.1 创建 `karte-syscall/src/riscv64.rs` — RISC-V syscall 封装
  - `ecall` 指令触发 syscall
  - syscall 编号：exit=93, exit_group=94, mmap=222, munmap=215, brk=214, write=64, read=63
  - 寄存器：a7=nr, a0-a5=args, 返回 a0
- [ ] 1.2 修改 `karte-syscall/src/lib.rs` — 添加 `#[cfg(target_arch = "riscv64")]`

### 阶段二：RISC-V 寄存器约定和 CallingConvention

- [ ] 2.1 修改 `karte-common/src/calling_convention.rs` — 添加 RISC-V LP64D ABI
  - 参数寄存器：a0-a7 (x10-x17)
  - 返回值：a0 (x10)
  - callee-saved：s0-s11 (x8, x9, x18-x27)
  - 专用寄存器映射：vm_sp → x2(sp), vm_fp → x8(s0/fp)
  - temp：t0-t6 (x5-x7, x28-x31)

### 阶段三：RISC-V JIT 编译器

- [ ] 3.1 创建 `karte-codegen/src/vm/professional_executor/jit/riscv_compiler.rs`
  - 实现 `JitCompiler` trait
  - 编译每条 LIR 指令为 RISC-V 机器码
  - 关键指令编码：
    - ADDI/SUBI（立即数运算）
    - LD/SD（内存访问，基于 vm_sp/vm_fp）
    - BEQ/BNE/BLT/BGE（条件分支）
    - JAL/JALR（函数调用和返回）
    - ECALL（syscall）
  - 函数 prologue/epilogue
  - CompareSet → CMP + CSET（RISC-V 无 CSET，需要 SLT/SLTU + XOR 或 SLTI）
- [ ] 3.2 在 `mod.rs` 中注册 RISC-V 编译器

### 阶段四：RISC-V AOT Runtime

- [ ] 4.1 创建 `karte-aot/src/runtime_riscv.rs`
  - `_start`：mmap 虚拟栈 + 堆、设置 vm_sp/vm_fp、调用 main、exit
  - bump allocator（`__karte_gc_alloc_aligned`）
  - no-op stubs（free, retain, release, update_stack_top）
  - gc_safepoint
  - 全局数据段（bump_ptr, heap_start, heap_limit, vstack_bottom, alloc_count, gc_threshold）
- [ ] 4.2 修改 `karte-aot/src/compiler.rs` — 添加 RISC-V 编译路径
  - 条件选择 runtime 和 compiler
  - Patch 逻辑适配 RISC-V 的跳转编码（B-type, J-type, AUIPC+JALR）
- [ ] 4.3 修改 `karte-aot/src/elf.rs` — 添加 `ElfArch::Riscv64`
  - ELF Machine ID: EM_RISCV = 243
  - Class: ELFCLASS64
  - Data: ELFDATA2LSB (little-endian)
  - Flags: 0x5 (RVC 压缩指令可选)

### 阶段五：用 Karte 语言重写 AOT GC

**策略**：GC 算法（mark-compact）用 Karte 语言实现，编译后作为 AOT binary 的一部分。需要添加少量 intrinsic 来访问底层硬件能力。

- [ ] 5.1 设计 GC Karte 代码的接口
  - `gc_alloc(size: number) -> number` — 分配（bump + 溢出时 gc_collect）
  - `gc_collect()` — mark-compact 回收
  - `gc_safepoint()` — 检查阈值
  - 需要的 intrinsic：
    - `raw_read(addr: number, offset: number) -> number` — 从地址读取 u64
    - `raw_write(addr: number, offset: number, value: number)` — 写入 u64
    - `get_vm_sp() -> number` / `get_vm_fp() -> number`
    - `raw_memcpy(dst: number, src: number, size: number)`
- [ ] 5.2 在 LIR 中添加 intrinsic 指令（RawRead, RawWrite, GetVmSp, RawMemcpy）
- [ ] 5.3 实现 intrinsic 的 codegen（x86, aarch64, riscv）
- [ ] 5.4 用 Karte 编写 gc_alloc, gc_collect, gc_safepoint
- [ ] 5.5 修改 AOT 编译流程，将 GC 的 Karte 代码编译并链接到 runtime

### 阶段六：测试验证

- [ ] 6.1 安装 `qemu-riscv64`（`apt install qemu-user`）
- [ ] 6.2 测试基本 RISC-V AOT：`fn main() -> number { 42 }`
- [ ] 6.3 测试 RISC-V AOT 算术运算、if-else、函数调用
- [ ] 6.4 测试 GC 相关功能（alloc, safepoint, collect）
- [ ] 6.5 运行全量 nextest 确认无回归

## 验证方案

- `cargo nextest run --workspace` — 505 tests 全部通过
- `./target/debug/karte aot test.karte -o test_rv && qemu-riscv64 ./test_rv` — RISC-V binary 能运行
- GC 测试：分配大量对象后触发 gc_collect，确认存活对象不被回收

## 注意事项

- RISC-V 指令固定 32 位，立即数需要多条指令（LUI + ADDI 或 AUIPC + ADDI）
- RISC-V 没有 flags 寄存器，条件分支直接比较两个寄存器（BEQ rs1, rs2, offset）
- CompareSet 在 RISC-V 中：`SLT dst, src1, src2`（set less than）或 `SLTI` + `XOR` 组合
- AOT GC 用 Karte 实现时，需要解决自举问题（GC 自身不能触发 GC）
