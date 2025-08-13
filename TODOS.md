# TODOS

- [ ] 优化codegen模块的结构，加回解释器支持
- [ ] 目前的实现没有重用stack slot，导致stack分配的很大，需要实现重用逻辑
- [ ] simple reg allocator过于简陋，需要重构
- [ ] 在mir层实现pass（是否有办法让pass manager独立于ir？）
- [ ] x86 JIT支持完善
- [ ] 支持C ffi，进一步实现堆分配

...

2025-08-13
- [x] StackFrameLayout: 修复地址纯寻址分析。若 `Store64` 的 `src` 为某个栈槽地址寄存器，说明地址值作为数据被使用（地址逃逸），必须将该地址寄存器标记为“非纯寻址用途”。这样在下沉阶段不会删除其 `Alloc`，而是保留/替换为 `add addr, FP, base_off`，避免 `Option<&T>` 构造时地址被错误折叠导致的值破坏。
- [x] 运行 `cargo test test_option_constructor_encoding_bug -- --nocapture` 验证修复通过。
- [x] RA: 修复临时寄存器入栈顺序，采用 `sub sp, #8; store [sp]` 语义，而非 `store [sp-8]`，避免越界；撤回显式 `Alloc(Stack)` 的spill方案，改为继续用 SP 偏移的稳定策略（确保 push/pop 合法），避免和框架布局重复管理。
