# 动态内存分配支持方案

## 1. 背景与总体目标

- 语言目前缺乏统一的堆分配模型，阻碍更复杂的运行时特性（GC、闭包捕获、动态容器）。
- 目标是引入专业、可验证的动态分配体系，短期支持手动/引用计数释放，长期平滑升级到精确 GC。
- 方案需兼顾编译器各阶段（H/M/L IR）、运行时、标准库与测试工具，避免一次性超大改动。

## 2. 设计原则

- **可演进**：接口与布局前瞻 GC，需要 write barrier/stack map 钩子。
- **与 MIR/IR 深度集成**：指令层面显式建模 `Allocate/Retain/Release`，利于优化与验证。
- **平台无关**：抽象 `karte_rt::Allocator` trait，允许替换为 jemalloc/mimalloc/自研实现。
- **安全优先**：在没有 GC 前，通过线性区域、借用检查或 ARC 保障无悬挂指针。

## 3. 语言与 IR 扩展

### 3.1 语法与语义

- 新增 `box`/`new` 语法糖，对应堆对象构造；语义上生成 `Allocate + Init` 序列。
- 明确堆对象生命周期：编译器需推导逃逸信息，决定是否可栈上提升。

### 3.2 MIR/IR 指令集

- 新指令：`Allocate(layout) -> ptr`、`Deallocate(ptr, layout)`、`Retain/Release`（用于 ARC/引用语义）。
- 元指令：`MarkGcRoot`, `WriteBarrier`, `ReadBarrier`（初期为 no-op，但保留在 MIR）。
- 为每个分配指令附加：类型 ID、大小、对齐、逃逸级别、可变性标志，便于 backend 生成精确代码和 stack map。

### 3.3 类型与元数据

- 对每种复杂类型生成 `TypeDescriptor`：记录字段布局、指针 bitmap、trace 函数入口。
- MIR lower 阶段把 `TypeDescriptor` 以常量形式嵌入，供 runtime/GC 使用。

## 4. 运行时堆管理

### 4.1 Allocator 抽象

- `trait Allocator { fn alloc(&self, layout: Layout) -> *mut u8; fn dealloc(&self, ptr: *mut u8, layout: Layout); fn realloc(...) }`
- 默认实现采用分级空闲表 + bump arena 混合策略；保留特性 flag 切换到系统分配器。
- 新增 `karte-rt` crate，集中定义 `Allocator` trait、可替换的 `SystemAllocator` 以及 `karte_jit_runtime_*` FFI 入口，JIT 与 CLI 共享同一运行时。

### 4.2 对象头布局

- 建议 16 字节对齐：`[u32 type_id | u32 size] [u16 flags | u16 color | u32 extra]`。
- `flags` 预留 pinned/immutable/rc bits；`color` 服务于标记-清扫；`extra` 可指向 vtable 或 trace thunk。

### 4.3 内存安全策略

- 初始实现可选：
  - **线性区域**：函数/作用域内集中分配，作用域结束统一回收；
  - **ARC**：MIR 自动插入 retain/release，循环检测延后实现；
  - **借用检查**：重用现有生命周期分析，阻止悬挂引用。
- 与编译器逃逸分析结合，避免不必要的堆分配。

## 5. 标准库与内建类型适配

- 字符串、向量、闭包环境、哈希表统一走 allocator，消除自定义 `Vec`/`String` 的重复代码。
- 语言内置容器需暴露 `with_allocator` 构造，面向后续 arena/region API。

## 6. GC 预留能力

- **根集管理**：编译器在函数入口/出口注入 `gc_root_push/pop`，同时生成 stack map（活跃指针栈偏移表）。
- **屏障接口**：MIR 遇到指针写入时调用 `karte_rt::gc::write_barrier`，当前实现可为空函数。
- **类型追踪**：为每种 `TypeDescriptor` 生成 `trace(ObjectRef, &mut Visitor)`，GC 运行时按需调用。
- **并发/增量准备**：对象头中的 `color` + `flags` 足以表达 tri-color，不需要重写布局。

## 7. 阶段性路线图

| 阶段 | 里程碑 | 关键输出 |
| --- | --- | --- |
| P0 | 语义/IR 定稿 | 文档、MIR 指令实现、roundtrip 测试 |
| P1 | 最小 allocator | `karte_rt::Allocator`、堆 API、字符串/数组迁移 |
| P2 | ARC/区域安全层 | Retain/Release 插桩、借用分析联动 |
| P3 | GC 钩子上线 | 根集 API、屏障、trace 表 |
| P4 | 首个 GC | 标记-清扫实现、性能基准与诊断工具 |

## 8. 风险与缓解

- **ABI 变化**：通过 feature flag 与 `--emit-stack-map` 选项 gated；对旧目标保留兼容路径。
- **性能回退**：为分配密集基准构建 CI，必要时切换 arena/bump 分配策略。
- **实现复杂度**：拆分为多个 PR（每次 ≤200 行），同步更新文档+测试。
- **内存泄漏**：在 ARC/区域阶段提供 leak sanitizer 模式，集成 `miri`/`valgrind` 检查。

## 9. 开发与测试清单

- 更新 `karte-tests` 中 MIR roundtrip/codec 测试以覆盖新指令。
- 为 runtime allocator 添加基准与 property test（碎片率、对齐、并发冲突）。
- 提供 `RUST_LOG=karte_rt::alloc=trace` 调试通路，记录分配/释放与对象头摘要。
- 文档：`docs/DYNAMIC_MEMORY_PLAN.md`（本文件）、运行时 README、GC 设计草案。
- AArch64 端到端验证：`cargo test -p karte-tests --lib codegen_tests::tests::test_heap_allocate_and_free_aarch64`
- 统一 FFI codegen：`RuntimeCall/RuntimeArg` 抽象隐藏各 ISA 的参数搬运细节。

## 10. 进度跟踪

- **2025-11-14**：完成 P0 - 在 MIR 中引入 `Allocate/Deallocate/Retain/Release/MarkGcRoot/WriteBarrier/ReadBarrier` 指令以及 `HeapLayout`/`GcRootKind` 抽象，补充 roundtrip 测试覆盖。
- **2025-11-17**：前端语法新增 `box expr`/`free expr`，HIL/HIR 将其编码为堆分配/释放表达式并在 LIR 中降级为 `Alloc/Free`，`mir_roundtrip` 用例覆盖。
- **2025-11-18**：JIT 路径（x86_64 & AArch64）加载 `Alloc/Free` 指令均经 runtime FFI 完成对齐分配与释放，并在调用前后保存 caller-saved 寄存器以确保 `box/free` 语义与解释器一致。
- **2025-11-18**：完成 P1 - 引入 `karte-rt` 最小分配器与 FFI 统一封装，标准 `box/free` CLI 端到端测试覆盖，JIT 调用通过 `RuntimeCall` 统一降级。
- **2025-11-19**：ARC 预备阶段启动——运行时追踪 `retain/release` 引用计数，计数降为 0 时自动释放，并在 `HeapStats` 中暴露 RC 监控指标。
- **2025-11-19**：语言新增 `retain expr` / `release expr` 语法，CLI `--heap-stats` 输出 retain/release Δ，`examples/rc_demo.karte` 演示多次保留/释放后无需 `free` 也能回收内存。

## 11. CLI 运行示例

1. 新建源文件（例如 `examples/box_free_example.karte`）：

   ```karte
   let ptr = box 10;
   let y = *ptr + 5;
   free ptr;
   y
   ```

2. 使用 CLI 调用 JIT（默认启用，可显式设置 `KARTE_JIT=1`）：

   ```bash
   KARTE_JIT=1 cargo run -p karte-cli -- run examples/box_free_example.karte
   ```

   常用附加参数：

   - `--verbose`：打印 token / AST / MIR / LIR，以便定位分配指令。
   - `--optimization fast`：切换优化级别，便于观察 Alloc/Free 在不同阶段的保留情况。
