# 计划：AOT Runtime 重构——遵循 Runtime vs 标准库设计原则

> 生成时间：2026-06-07
> 状态：待确认

## 背景与目标

**现状**：`runtime_x86.rs` 有 3352 行手写 x86 机器码，AOT 二进制 504KB，其中 runtime 占 498KB (97%)，用户代码仅 2.1KB。大量手写函数重复了 `std/gc.karte`、`std/string.karte` 已有的 Karte 实现。

**目标**：
1. AOT runtime 精简到只包含底层 OS 抽象（syscall、内存分配），符合项目设计原则
2. AOT 二进制大小显著缩减
3. 所有现有测试通过，零回归

## 现状分析

### 调用约定瓶颈

**核心约束**：字符串 intrinsic 通过 System V ABI 调用（`movabs rax, <ptr>; call rax`，参数在 RDI/RSI/RDX），而编译的 std 库函数使用 Karte 虚拟栈调用约定（参数 push 到虚拟栈，R10=vm_sp）。两者不兼容。

**因此字符串函数不能直接替换为编译的 std 代码**——需要 wrapper 或改变 codegen。

### 分类

| 函数 | 行数 | 决定 | 理由 |
|------|------|------|------|
| `emit_start` | 340 | **保留** | 进程入口、mmap，无法在 Karte 中实现 |
| `emit_gc_alloc` | 148 | **保留，简化** | bump allocator 是底层操作，但移除 GC 触发逻辑 |
| `emit_gc_alloc_simple` | 14 | **保留** | gc_alloc 的尾调用包装 |
| `emit_gc_collect` | 286 | **删除** | std/gc.karte 有 Karte 实现；AOT 用 bump-only |
| `emit_gc_safepoint` | 54 | **保留，改为 no-op** | std/gc.karte 自行管理 GC |
| `emit_gc_update_stack_top` | 5 | **保留** | 接口占位 |
| `emit_free` | 4 | **保留** | no-op |
| `emit_raw_syscall6` | 33 | **保留** | syscall 封装 |
| `emit_retain/release` | 7 | **保留** | no-op |
| `emit_mem_load64/store64` | 19 | **保留** | 底层内存原语 |
| `emit_panic` | 33 | **保留** | 错误处理 |
| `emit_string_compare` | 81 | **删除** | 按需裁剪 |
| `emit_string_equal` | 108 | **删除** | 按需裁剪 |
| `emit_string_concat` | 128 | **删除** | 按需裁剪 |
| `emit_string_char_at` | 93 | **删除** | 按需裁剪 |
| `emit_char_to_string` | 60 | **删除** | 按需裁剪 |
| `emit_string_substring` | 221 | **删除** | 按需裁剪 |
| `emit_string_contains` | 93 | **删除** | 按需裁剪 |
| `emit_split_count` | 99 | **删除** | 按需裁剪 |
| `emit_trim` | 318 | **删除** | 按需裁剪 |
| `emit_to_string` | 236 | **删除** | 按需裁剪 |
| `emit_print_string` | 46 | **删除** | 按需裁剪 |
| `emit_print_number` | 175 | **删除** | 按需裁剪 |
| `emit_print_bool` | 59 | **删除** | 按需裁剪 |

### 方案：按需裁剪 Runtime

**核心思路**：扫描编译后的 Karte 代码，找出实际调用了哪些 `RuntimeIntrinsic`，只 emit 用到的 runtime 函数。

**阶段一（本次）**：
1. 给 `X86Runtime` 添加 `needed_intrinsics: HashSet<String>` 字段
2. 在 `compiler.rs` 中，编译完所有 Karte 函数后，扫描机器码提取所有 `movabs rax, <jit_ptr>` 中的 intrinsic 指针
3. 将用到的 intrinsic 指针映射回 `RuntimeIntrinsic` 变体名
4. `X86Runtime.emit()` 只 emit `needed_intrinsics` 中的函数
5. `compiler.rs` 中 `runtime_ptr_map` 和 `global_labels` 也只注册被 emit 的函数

**阶段二（后续）**：
1. 将 `emit_gc_collect` 完全移除——AOT 程序使用 bump-only 分配
2. gc_alloc 溢出时返回 0（OOM），不触发 GC
3. 需要的程序可以增大初始堆大小（从 4MB 到更大）

**效果预估**：
- test_cc 只使用 `raw_syscall6, mem_load64, mem_store64, gc_alloc, string_compare` 5 个 intrinsic
- runtime 从 ~498KB 降到 ~10KB
- AOT 二进制从 504KB 降到 ~15KB

## 详细计划

### 阶段一：按需裁剪 Runtime

- [ ] 步骤 1.1：在 `runtime_x86.rs` 的 `X86Runtime::new()` 中添加 `needed_intrinsics: HashSet<String>` 参数
- [ ] 步骤 1.2：`emit()` 方法中，每个 `emit_*` 前检查 `needed_intrinsics.contains(name)`，不需要则跳过
- [ ] 步骤 1.3：`emit_start` 和 `emit_gc_alloc` 始终 emit（基础必需）
- [ ] 步骤 1.4：`emit_gc_alloc` 移除 GC 触发逻辑（不再调用 `gc_collect`），溢出返回 0
- [ ] 步骤 1.5：删除 `emit_gc_collect`（286 行）
- [ ] 步骤 1.6：`compiler.rs` 中扫描编译后的 Karte 代码，收集所有 `movabs rax, <ptr>` 中的 JIT 指针
- [ ] 步骤 1.7：通过 `runtime_ptr_map` 反查指针→函数名，构建 `needed_intrinsics`
- [ ] 步骤 1.8：`runtime_x86::new()` 接收 `needed_intrinsics`，只 emit 需要的函数
- [ ] 步骤 1.9：`global_labels` 和 `runtime_ptr_map` 只注册被 emit 的函数
- [ ] 步骤 1.10：处理 `patch_internal_calls`——只修补被 emit 函数的内部调用

### 阶段二：验证

- [ ] 步骤 2.1：`cargo nextest run --workspace` 全量测试通过
- [ ] 步骤 2.2：AOT 编译 test_cc 并验证 fib(10)=55, abs(-42)+sum(10)=87
- [ ] 步骤 2.3：验证 AOT 二进制大小显著缩减
- [ ] 步骤 2.4：AOT 编译简单程序 (`"42"`, `"let x = 5; x + 10"`) 验证基本功能

## 风险点

1. **`patch_internal_calls` 依赖函数名**：删除函数后，其他函数的内部 call 占位可能引用已删除函数的偏移。需要确保 `needed_intrinsics` 包含被依赖的函数。
   - 缓解：`gc_alloc` 依赖 `gc_collect`，但删除 `gc_collect` 后 `gc_alloc` 不再调用它
2. **字符串函数间的依赖**：`string_concat` 依赖 `gc_alloc`，`gc_alloc` 始终被 emit
3. **`emit_start` 的 `_start` 调用 `main`**：不受 `needed_intrinsics` 影响
4. **`emit_start` 内的 print 逻辑**：`_start` 中有将 main 返回值转 ASCII 输出的逻辑。如果 `print_number` 不被 emit，这部分不受影响（因为它是内联在 _start 中的）

## 回滚策略

所有修改在 feature 分支，可直接 `git revert`。
