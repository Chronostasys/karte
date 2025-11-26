# Karte 开发中心文档

> 目标：在暂停进一步特性开发前，统一研发信息、补足基础语言能力，并做好引用计数（ARC）阶段的预备工作。即使失去上下文，也可依此文档恢复下一步执行计划。

## 1. 全局目标

1. **稳固基础语言功能**：补齐数组、切片、字符串等核心内建类型，确保 HIR → MIR → LIR → JIT 流程具备一致语义。
2. **为 ARC 做准备**：在语义、IR、运行时三层逐步引入 `retain/release` 钩子、Lifetime/escape 信息和栈图，保证未来切换 ARC/GC 时无需大改。
3. **降低缺陷密度**：通过聚焦 lint、测试和 profiling，减少“TODO 驱动开发”积压，保持专业级别的代码质量。

## 2. 文档索引

| 路径 | 说明 | 负责人/下一步 |
| --- | --- | --- |
| `docs/DYNAMIC_MEMORY_PLAN.md` | 动态内存/GC 长期路线。P1 已完成（JIT allocator + 统一 FFI）。 | 未来 P2-P3 参考该文档。 |
| `docs/IR_CODEC_GUIDE.md` / `docs/IR_CODEC_QUICK_START.md` | IR Display/Parse 体系与约定。 | 扩展新的 IR 类型时务必更新示例。 |
| `docs/DEVELOPMENT_HUB.md`（本文件） | 中心导航、基础功能 backlog、ARC 准备。 | 每次架构级决策/阶段收尾时更新。 |
| `TODOS.md` | 日常任务列表（按日期/主题）。 | 每次改动后同步打勾/新增事项。 |
| `examples/*.karte` | CLI/JIT 样例（如 `box_free_example.karte`）。 | 为新语法（数组/引用）补充 runnable 示例。 |
| `docs/plans/module_system_phase2.md` | 模块化 2.0：import 语法、跨模块调用与 CLI 缓存设计。 | 2025-11-25：实现前计划已提交，按计划推进。 |

> 规范：新增专题文档时，需要在此表补充索引；已有文档完成阶段性目标，也需在“负责人/下一步”列记录状态。

## 3. 基础语言功能缺口

| 模块 | 当前状况 | 缺口 | 建议实施步骤 |
| --- | --- | --- | --- |
| **数组/切片 (Array/Slice)** | ✅ `[]` 字面量、`arr[idx]`、`len arr` 均已落地（2025-11-18） | 尚缺变长数组 API、写操作、越界检查 | 1) MIR/LIR 补充 bounds check；2) JIT/Runtime 提供 memcpy/memset；3) CLI/Stdlib 暴露 `len`/`push` 等安全 API；4) 与 ARC/Retain 规则打通。 |
| **可变绑定/借用** | `let` 默认为不可变，缺乏 `let mut` 与借用语义 | 编译器生命周期分析不足 | 与 ARC/线性区域联动：记录 `mutable` 标志，扩展 `EscapeState`；提供 `borrow`/`deref` 语法糖。 |
| **标准库基元** | 字符串/向量分散于测试代码 | 无统一模块 | 搭建 `std/prelude` 树，示例：`std/array.karte`、`std/rc.karte`。 |

> 优先次序：数组（支撑大部分测试与示例）→ 变量可变性 → Prelude。具体 Issue 与负责人请登记到 `TODOS.md`。

## 4. ARC 准备路线

1. **语义层**：在 HIR/MIR AST 中保留 `retain/release` 节点（P0 已完成），为数组/结构体等复合类型推导逃逸级别。
2. **IR 层**：
   - MIR：确保每个 `Value` 可追溯 `HeapLayout`、`EscapeState`。
   - LIR：在 `RuntimeCall` 中加入 `retain/release` 占位符，JIT 统一通过 FFI 调用未来的 `karte_rt::rc::*`。
3. **运行时层**：
   - `karte-rt`：扩展 `Allocator` trait，准备引用计数表/调试接口 (`heap_stats` 已提供)。
   - 预留 `RcHeader { strong, weak }` 结构和 CAS 操作封装。
   - 2025-11-18：JIT 已可调用 `karte_jit_runtime_retain/release`（当时为 no-op 占位），LIR 指令与 RuntimeCall 已通路，便于后续接 wire ARC。
   - 2025-11-19：`karte-rt` 内置 RC 表与 `heap_stats` 计数，`retain/release` 会更新引用计数并在降为 0 时自动调用运行时分配器释放内存，CLI 可用 `RUST_LOG=trace karte_rt::ffi` 追踪 retain/release。
   - 2025-11-19：前端新增 `retain expr` / `release expr` 语法，MIR → LIR 会插入对应指令；`examples/rc_demo.karte` 搭配 `--heap-stats` 可观察 retain/release Δ 值。
   - 2025-11-19：补充 `arc expr` 堆分配语法并在 HIR/MIR 中标记 `OwnershipKind::RefCounted`，作为自动 retain/release 的前置条件。

### 4.1 Rust 风格 `Rc<T>` 升级计划

| 步骤 | 目标 | 说明 |
| --- | --- | --- |
| A | `Rc<T>` 类型描述 | 在类型检查阶段引入 `OwnershipKind::Rc`（或等价 metadata），`box`/`struct` 字段若标记为 `rc`，就会自动携带引用计数元信息。 |
| B | 自动 `retain` 插桩 | MIR Builder 依据 `OwnershipKind::Rc` 在别名创建（`let alias = ptr`、参数传递、记录字段复制）时插入 `retain`，禁止用户层手写。 |
| C | 块级 drop 表 | 在 HIR → MIR 过程中生成块内 `Rc` 本地变量栈，作用域退出和重绑定前自动插入 `release`，以逆序 drop，效果等价 Rust `Rc` 的 `Drop`. |
| D | `std/rc.karte` API | 提供 `Rc::new`, `Rc::clone`, `Rc::get` 等函数/方法作为唯一入口，向后兼容现有 `box` 指针，`retain/release` intrinsic 改为 `intrinsics::retain` 调试用途。 |
| E | 示例与测试 | 重写 `examples/rc_demo.karte`、`karte-tests` 内相关 case，验证在没有手写 release 的情况下堆统计回落为 0；`--heap-stats` 需额外展示强引用计数。 |

> 里程碑：A+B 为编译期准备（2025-11-22 前完成），C 完成后可宣布“RC 与 Rust 行为对齐”；D+E 可分阶段落地，但需在本文件与 `docs/DYNAMIC_MEMORY_PLAN.md` 同步进展。

- **调试工具**：`RUST_LOG=karte_rt::alloc=trace` + `heap_stats`，配合 CLI `--verbose` 输出，快速定位泄漏。

### 4.2 Heap Stats 指标说明

启用 `cargo run -p karte-cli -- run <file> --heap-stats` 后，CLI 会在 JIT 结束时打印运行期内存与 ARC 计数器的差分，便于快速判断 retain/release 是否配对：

| 指标 | 释义 | 典型判断 | ARC 示例（`arc_auto_cleanup.karte`）|
| --- | --- | --- | --- |
| `active_allocations: a -> b (Δx)` | 当前仍存活的堆分配块数量。Δ>0 表示有内存未释放。 | 期望 Δ0；若非 0 需排查对应 `HeapAllocate` / drop 路径。 | `0 -> 0 (Δ0)`：堆对象在作用域结束前已释放。 |
| `rc_tracked_objects: a -> b (Δx)` | 运行时引用计数表中被追踪的对象数量。 | ARC 下 Δ0 说明所有 `Rc` 资源都被释放。 | `0 -> 0 (Δ0)`：`arc` 创建的资源被自动清理。 |
| `retain/release ops: +R/+F (rc_zero Δ z)` | 本次执行中触发的 retain 与 release 次数，以及引用计数降至 0 的次数。 | R、F 应与 ARC 语义一致；`rc_zero Δ` > 0 表示有对象真正被回收。 | `+2/+3 (rc_zero Δ 1)`：两次 clone，三次 release，其中一次将计数降到 0 以触发析构。 |
| `bytes_in_use: a -> b (Δx)` | 运行时分配器正在使用的字节数。 | 期望 Δ0，若为正表示仍有内存占用。 | `0 -> 0 (Δ0)`：ARC 清理后没有残留占用。 |

> 诊断建议：若 `retain/release` 不平衡导致 `rc_tracked_objects` 或 `bytes_in_use` 出现正 Δ，可结合 `RUST_LOG=trace karte_rt::ffi` 观察每一次 retain/release 调用栈，定位缺失的自动释放点。

## 5. 质量保障计划

1. **Lint & Dead Code**：对 `karte-ir-derive`、`karte-lir` 等出现的 dead_code、unused warnings 建立追踪，将修复任务拆分到 `TODOS.md` 的“质量”板块。
2. **测试矩阵**：
   - 单测：`karte-rt`（allocator/FFI）、parser/type checker。
   - 集成：`karte-tests` → `codegen_tests`, `mir_roundtrip_tests`。
   - 基准：为数组/box 操作加入 microbenchmark（Rust `criterion` 或自研计时）。
3. **文档同步**：任何公共 API 调整先更新本文件及对应专题文档，然后再改代码，避免“文档滞后”。

## 6. 增量编译与模块化

1. **当前能力（2025-11-18）**：CLI 在编译文件输入时会将 MIR/LIR 文本缓存到 `target/.karte-cache/`，依据 `source+opt level+版本` 生成 key，命中时直接反序列化 `MirProgram` / `LirProgram`，达到“单文件增量编译”效果。示例：`cargo run -p karte-cli -- run examples/array_len_example.karte` 第一次生成缓存，第二次秒级复用。
2. **缓存管理**：设置 `KARTE_CACHE_TAG` 可手动失效；`rm -rf target/.karte-cache` 强制全量重建。未来可扩展为全项目 manifest（含依赖信息）。
3. **模块化 / 依赖分析（2025-11-19 进展）**：
   - CLI 现已默认沿着入口文件向上查找 `karte.mod.toml`，并能解析 `path`/`sources`/`dir` 三种声明方式；`examples/karte.mod.toml` 提供了 `runtime → main` 的演示清单。
   - `ModuleGraph` 产出的拓扑顺序驱动每个模块独立的 IR pipeline，verbose 模式会打印 source fingerprint 以及新加入的 interface hash。
   - 针对每个模块的公共接口（函数签名、结构体字段、依赖接口哈希）生成稳定摘要，保证实现层面的修改不会导致下游误重编；接口变化时会级联触发缓存失效。
   - 缓存 key 现包含 `(module_id + source_fingerprint + deps_interface_hash + opt_level)`，命中后可直接 reuse MIR/LIR；入口模块的 interface hash 会写入 `interface_hashes` 映射供依赖查询。
   - 在标准库拆分前（如 `std/array`），仍要求模块暴露显式接口描述，CLI `describe()` 输出现已包含文件列表与 interface 指纹，足以在调试/回归时进行比对。
   - 2025-11-26：Parser/HIR/MIR 已完整携带 `ModuleContext`，Type Checker 会加载 `target/.karte-cache/<module>.interface.json` 并根据 import alias / `module::symbol` / 选择性导入进行解析；LIR 降级阶段与 JIT 执行器都改为使用 canonical `module::symbol` 名称（`main::main` 等），CLI 集成测试可直接验证跨模块调用。

   ```toml
     [[modules]]
     id = "runtime"
     path = "std/runtime.karte"

     [[modules]]
     id = "main"
     sources = ["examples/app_part1.karte", "examples/app_part2.karte"]
     deps = ["runtime"]

     [[modules]]
     id = "std"
     dir = "std"
   ```

     解析后生成拓扑序列（含模块内文件列表），依次编译 `runtime -> main(part1 -> part2)`，并在 verbose 模式输出 `[module]` 描述与 fingerprint，便于调试。
4. **实施路线（更新）**：
   1. ✅ 2025-11-19：`module.karte.toml` 模式落地，示例 manifest 已收录在 `examples/`。
   2. ✅ 2025-11-19：CLI 解析 manifest 构建 `ModuleGraph`，生成拓扑序并为每个模块维护 source fingerprint + interface hash。
   3. ✅ 2025-11-19：缓存 key 由 `(module_id, fingerprint, deps_interface_hash, optimization)` 组成，依赖接口 hash 变化会触发级联失效。
   4. ⏳ 下一步：预留并发编译调度器（依赖树分层队列），当前实现仍按拓扑序串行执行并输出 `[module]` fingerprint + 接口摘要日志。

## 8. 最近关键修复 (Recent Critical Fixes)

| 日期 | 模块 | 问题描述 | 修复方案 | 影响 |
| --- | --- | --- | --- | --- |
| 2025-11-22 | JIT / LIR | `cargo run` 脚本模式下出现 `EXC_BAD_ACCESS` (SIGSEGV)，原因是 `main` 返回值被错误地作为指针解引用。 | 1. **LIR Lowering**: 修复 `Instruction::Call` 降级逻辑，使用临时寄存器接收返回值，避免覆盖栈地址寄存器；2. **Stack Balance**: 移除 LIR Lowering 中冗余的栈弹出指令（JIT epilogue 已处理）；3. **JIT**: `compile_return` 仅在返回 Host 时写入 Return Slot。 | 彻底修复了 CLI 执行脚本时的崩溃问题，验证了 Script Mode 的稳定性。 |

