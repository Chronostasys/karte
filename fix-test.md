# Fix Test 修改计划

## 背景
- 现有 `karte-mir/tests` 下多处单测仍依赖旧版手写 Display/Parse 逻辑（如 `Variable(x)`、`Temp(TempId(5))` 等格式），而运行时代码已经切换到 `karte-ir-derive` 提供的自动 `token/args/body` 语法。
- 这些断言如今直接与 proc macro 输出冲突，导致 `cargo test -p karte-mir --test codec_integration_test` 等测试套件无法通过。

## 整体策略
1. **确认基准行为**  
   - 运行 `cargo test -p karte-mir --test codec_integration_test`、`--test compact_display_test`、`--test hashmap_parse_test`，收集实际失败断言、记录新的 Display/Parse 文本格式。
2. **重写或删除过时断言**  
   - `karte-mir/tests/codec_integration_test.rs`：将对旧格式的精确匹配改为“新语法断言 + parse roundtrip”模式；对确实无法稳定断言的输出直接改成 `assert_roundtrip(value)` 帮助函数，并移除已经覆盖重复语义的片段。
   - `karte-mir/tests/hashmap_parse_test.rs` & `debug_parse_*.rs`：同步示例输入为当前自动缩进/换行规范，避免依赖旧版 `MirFunction` 文本结构。
   - `karte-mir/tests/temp_display_test.rs` 等打印型测试补充实际断言，确保 `%`/`bb` token 行为被真正校验。
3. **补充新用例**  
   - 增加一个针对 `#[ir_codec(token="...")] + #[ir_codec(args)]` 的组合示例（例如 `Value::Reference`, `BinaryOperator`）做“显示 -> 解析 -> 再显示”回合，防止回退。
   - 为 `body`/`label` 属性生成的块级结构（`BasicBlock`, `MirFunction`）添加结构化 roundtrip 测试，覆盖 `HashMap/BTreeMap` 容器解析。
4. **验证与文档**  
   - 跑通相关测试与 `cargo test -p karte-mir`.
   - 在 `TODOS.md` 记录测试更新摘要，必要时为 `docs/IR_CODEC_*` 增补一条“测试最佳实践”说明。

## MIR 缩进与层级优化计划
1. **统一多行值的缩进语义**  
   - 将 `write_multiline_suffix` 输出改为先对内部文本做一次“基于 `{`/`[` 符号的归一化缩进”，再根据父级传入的缩进量整体平移，确保每层只增加固定 4 个空格。
2. **括号对齐与可读性**  
   - 归一化函数需要在遇到行首 `}`/`]` 时先回退缩进，再打印，从而保证闭合括号与对应的开括号保持同级。
3. **验证样例**  
   - 重新生成 `1.mir`、运行 `cargo test -p karte-mir` 及 `cargo test -p karte-tests`，确认新的缩进规则不影响解析或 roundtrip 行为。

