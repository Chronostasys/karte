# Karte 模块化 2.0 实施计划（实现前文档）

> 目的：在保持“无解释器（JIT-only）”路线的前提下，为 `karte.mod.toml` 模块系统补齐 import 语法、跨模块符号解析以及 CLI/Cache/测试一体化支持，确保 CLI 构建路径、MIR/LIR pipeline、运行时调用链条都能正确识别模块边界。

## 1. 现状速记

1. `karte-cli` 已能自 entry 向上查找 `karte.mod.toml`，解析 `path`/`sources`/`dir` 并构建 `ModuleGraph`，可输出拓扑序与 fingerprint。
2. `compile_entry_file` 逐模块串行编译，依赖 `ModuleInterfaceAccumulator` 生成 interface hash；`build` 子命令还提供 Rayon 分层编译雏形。
3. 语义层尚未提供 `import` 语法，也没有跨模块符号表；`test_project/` 仍通过复制函数体来“模拟共享”。
4. CLI 执行统一走 JIT，不再维护解释器路径——所有设计需面向 MIR→LIR→JIT 流程。

## 2. 语法设计

### 2.1 Manifest（`karte.mod.toml`）

```toml
[[modules]]
id   = "runtime"
path = "std/runtime.karte"

[[modules]]
id   = "main"
# sources / dir / path 三选一（可混合）
sources = ["src/main.karte", "src/app/part2.karte"]
dir      = "src/common"    # 递归收集 *.karte
deps     = ["runtime", "std.core"]
expose   = ["main", "AppState"] # 可选：显式导出函数/类型
```

约定：

- `id` 采用 `snake.case` 或 `dotted.case`（如 `std.runtime`），用于 import 命名空间。
- `deps` 必须出现在 manifest，其顺序不影响拓扑。
- 未来扩展 `features`/`cfg` 均通过 manifest 表达。

### 2.2 源码级 module + import

```karte
module main

import std.runtime
import std.array as array
import utils::{add, Subtractor}

fn main() -> i32 {
    array::len([1, 2, 3]) + add(1, 2)
}
```

语法要点：

1. 每个源文件可选写 `module <id>`，用于静态检查（必须和 manifest 中的模块匹配）。
2. `import <module_path>` 引用整个模块命名空间；允许 `as alias` 与 `{symbol list}` 选择性导入。
3. 跨模块调用使用 `module::symbol(...)`；若使用 `{symbol}` 形式导入，则可直接以 `symbol` 调用。
4. 模块路径统一使用 `.` 分隔（与 manifest 保持一致），`::` 仅用于“模块内符号”或 `import foo::{bar}` 选择子，避免混用。
5. 仅允许从依赖模块导入（`deps` 白名单）。

### 2.3 导出规则

- 默认导出：所有顶层 `fn`、`struct`、`enum` 均视为 public。
- 将来可加 `pub`/`priv` 标记；本阶段不加访问控制，但 `ModuleInterfaceAccumulator` 必须记录符号表以供校验。

## 3. 语义与分辨率

| 层级 | 职责 | 实施要点 |
| --- | --- | --- |
| Parser | 解析 `module`/`import` 语句，生成 AST 节点（`ImportDecl`）。 | 产生 `ModulePath`（`Vec<Ident>`）、别名表、选择性导入清单。 |
| HIR/Type Checker | 构建模块符号表，验证依赖、消解 `module::symbol` 与 `import alias`。 | TypeChecker 需接收“依赖模块接口 map”，支持延迟解析/先声明后定义。 |
| MIR | 记录模块名以便后续链接；确保引用的函数在编译时能映射到正确的 `FunctionId`。 | MIR 不需跨模块内联，本阶段仍以“公共符号 stub”解决。 |
| LIR/JIT | 不感知模块，但需在 `karte-cli` 合并 LIR 时避免符号冲突（`module::fn` → 全局唯一名）。 | 可通过 `module_id::fn_name` 命名策略。 |

## 4. CLI/Pipeline 调整

1. **多层并发编译**：`compile_entry_file` 已改为按层并发；后续需让每个模块编译结果（MIR/LIR + interface json）可以被依赖模块消费。
2. **接口工件**：
   - 输出 `target/.karte-cache/<module>.interface.json`，包含导出符号签名。
   - 依赖模块在解析 import 时加载接口，形成 stub（类型信息来自接口，而非源码）。
3. **缓存键**：扩展为 `source_fingerprint + deps.interface_hash + manifest_hash`，确保 import 变化触发重新编译。
4. **错误报告**：CLI `--verbose` 显示 `[module] import resolved: main -> std.array::{len}`。
5. **Prelude 注入**：通过环境变量 `KARTE_PRELUDE_PATH`（写法：`module_id=/abs/path/to/prelude`，若省略 id 则默认 `std.prelude`）声明外部 Prelude 模组位置，CLI 在构建 `ModuleGraph` 时自动注入对应模块，便于共享/覆盖标准库实现。

## 5. 实现步骤（科学顺序）

1. **Parser/HIR**
   - [x] 新增 `ModuleDecl` / `ImportDecl` AST。
   - [x] `ParserMode::Project` 默认要求 `module` 语句，`Script` 可省略（CLI 会在缺失声明时直接报错）。
   - [x] 为 `karte-hir` 引入 `ModuleContext`，挂载 import 别名表，并在 MIR 中标注 canonical symbol。

2. **Type Checker**
   - [x] 加载依赖模块接口（JSON/IR）。
   - [x] 校验 `import` 是否出现在 manifest `deps` 中（目前由 CLI 编译阶段强制执行，后续可与 Type Checker 联动）。
   - [x] 在 `Resolve` 阶段支持 `alias::symbol`、`module::symbol`、选择性导入。

3. **ModuleGraph / CLI**
   - [x] `ModuleMetadata` 增加 `manifest_hash`、`module_name`。
   - [x] `compile_entry_file` 在每层编译完成后写出接口文件，供后续层读取。
   - [x] 并在编译阶段缓存依赖接口，校验 `import` 所引用的模块与符号是否存在（缺失即报错）；校验逻辑封装在 `karte-module-system::validate_module_imports` 中，CLI/LSP 可共享。
   - [x] 入口模块汇总依赖接口，生成最终 MIR/LIR（保持 JIT-only 路线，不回退解释器）。

4. **LIR 合并 / 名称整形**
   - [x] 采用 `format!("{}::{}", module_id, fn_name)` 统一命名，避免重名。
   - [x] 更新 JIT 调用图，确保新名字可解析。

5. **测试与样例**
   - [x] `test_project/`：加入 `module`/`import` 示例，确保跨模组调用通过。
   - [ ] `kil` CLI 集成测试：新增 `build_imported_functions` case。
   - [ ] 示例：`examples/modular_demo` 补充 `import` 写法并加入 `karte.mod.toml`。

6. **文档同步**
   - [x] 更新 `docs/DEVELOPMENT_HUB.md`：记录“模块化 2.0 计划进行中 + import 语法”。
   - [x] 在 `README.md` / `docs/IR_CODEC_GUIDE.md` 添加跨模块案例。

## 8. 进度快照（2025-11-26）

- Parser/HIR/MIR 现已完整携带 `ModuleContext`：解析阶段记录 module/import，Type Checker 暴露模块上下文，MIR Lowering 将 canonical symbol 写入 `MirProgram::function_symbols` / `external_function_symbols`，为后续 LIR/JIT 提供确定性标签。
- Type Checker 的 `Resolve` 流程已经能根据 import alias、模块前缀或选择性导入来定位依赖符号，缺失接口或导出会即时给出 `ModuleInterfaceUnavailable`/`UndefinedModuleSymbol` 诊断。
- Parser 在调用 Type Checker 之前会注入依赖接口映射（源自 `target/.karte-cache/<module>.interface.json`），Type Checker 现在能在载入模块上下文时预填充每个导入 alias 的函数签名。
- CLI 的模块图编译路径可产出含模块元数据的 MIR/LIR；`test_project/` 示例和 `cli_integration_tests::test_compile_and_run_project_mode` 通过同步的 lowering 选项验证跨模块调用（`main` -> `utils::add`）能在 JIT 里稳定运行。
- LIR 降级阶段已经确保所有函数使用 canonical `module::symbol` 命名，JIT 执行引擎也改为读取 `program.main_function`（例如 `main::main`），从而可以直接执行带命名空间的入口函数并通过 CLI 集成测试验证。
- MIR→LIR 降级阶段会把每个函数和主入口命名成 `module_id::symbol`，并在 LIR 合并时保持这些 canonical 名称，避免跨模块函数覆盖；新增的单元测试锁定该行为。
- 模块图装配逻辑已抽离为独立 crate `karte-module-system`，`karte-cli` 与 `karte-tests` 均通过该 crate 共享 `ModuleGraph`/调度/计划生成；CLI 主流程已经移除旧的 `modules` 内联代码。
- `karte-tests::cli_integration_tests::test_compile_and_run_project_mode` 现直接通过 `ModuleGraph::plan_for_entry` 驱动例程，验证 manifest 解析与调度逻辑；下一步可在该 crate 内补充更细粒度的单元测试与错误分支验证。
- 模块接口构建（`ModuleInterfaceAccumulator`、hash 计算与接口文件写入）现由 `karte-module-system` 暴露；CLI 仅负责将编译结果喂给该接口层，后续 LSP/测试可直接依赖同一实现以保持 cache/file contract 一致。
- CLI 分层编译会缓存依赖接口摘要并复用 `target/.karte-cache/<module>.interface.json`，在编译期就验证每个 `import` 是否声明于 manifest 且引用的符号确实由依赖导出，缺失即立刻 fail。
- `karte-module-system` 提供共享的 `validate_module_imports` API（包含接口读取、manifest 对比逻辑），CLI 只需传入 `ImportDecl` 列表即可完成校验，后续 LSP/Type Checker 可以直接复用。
- `karte-cli` crate 补充了 import 校验单元测试，覆盖“未声明依赖”与“缺失导出符号”等场景，防止回归。
- 文档侧已在 `docs/DEVELOPMENT_HUB.md` 记录模块 2.0 当前能力，并在 `README.md` 及 `docs/IR_CODEC_GUIDE.md` 增加 manifest / canonical 命名示例，便于 CLI/LSP/测试协作成员快速了解跨模块调用的输入输出格式。
- CLI `compile_entry_file` 现会在返回前将各模块产出的 MIR/LIR 聚合到入口模块工件中（保持 canonical `module::symbol`），确保 `--emit mir/lir` 以及后续执行路径都能直接消费完整的跨模块程序。
- 下一步重点：将接口 JSON 真正喂给 Type Checker 进行符号分辨、扩展 CLI cache key 以纳入接口版本，并推动 LSP/测试消费统一接口格式。

### 最近修复（2025-11-26）

- **修复**: 在 `karte-lir/src/lower.rs` 中为外部函数同时注册别名与 canonical 名称（例如 `add` 与 `utils::add`），并增加对 MIR 的扫描以预分配引用的函数标签（例如 `utils.sub::multiply`）。
- **验证**: 复现导致 panic 的用例不再在 LIR 降级阶段触发 panic；日志显示已为 canonical 名称分配并注册标签，LIR → JIT 流程能够继续。
- **遗留问题**: 运行时在 JIT/执行阶段出现 Bus Error (exit code 138)。怀疑为 JIT 代码贴片或内存对齐/访问问题，需要收集 `RUST_BACKTRACE=1` 与更详细的 JIT patch 日志进行调查。
- **下一步**: (1) 用 `RUST_BACKTRACE=1` 重现并记录崩溃栈； (2) 在 JIT 贴片前后断言/打印标签地址有效性与对齐； (3) 添加回归测试覆盖 `module::symbol` 与 `import` 两种访问路径，防止回归。

## 6. 风险与对策

- **接口不一致**：通过 interface hash + manifest hash 解决；遇到缺失符号，报错“模块未导出 symbol”。
- **并行读写接口文件**：同层只读，不写；跨层写入完成后再解锁，必要时用 `std::fs::write` + 原子 rename。
- **语法歧义**：`import foo::{bar}` 需确保和 block 区分；解析器在关键字位置强制解析为 import。

## 7. 时间预估

| 阶段 | 工作量 | 备注 |
| --- | --- | --- |
| 语法+HIR | 1.5 d | 涉及 parser + type checker 重构 |
| CLI+缓存 | 1 d | interface 工件 + hash |
| LIR 命名 & 测试 | 0.5 d | 主要是 glue code |
| 文档 | 0.5 d | 更新 HUB + 新示例 |

发布顺序：语法/HIR → 接口/CLI → 示例/测试 → 文档收尾。
