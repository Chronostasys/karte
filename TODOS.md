# 目前的问题

- [ ] 目前的caller saved register好像处理有问题，虽然暂时测试还没错
- [x] register allocator暂时处理不了effect相关的caller saved register
- [ ] 现在的effect实现写死了相关的register（r0-r4），要重构。涉及文件：transform.rs和ir.rs中的获取inst的reg相关api
- [x] 多层perform测试失败
- [ ] 生命周期分析中将argument假设为永久的生命周期，这个需要优化
- [ ] 更新代数效应文档

- [x] 2025-11-14：同步 `karte-mir` 中 IR Codec 相关测试（`codec_integration_test.rs`、`compact_display_test.rs`、`debug_parse_test.rs`、`hashmap_parse_test.rs`、`manual_parse.rs`）到新的自动 Display/Parse 文本格式，确保 roundtrip 行为覆盖 `%` 前缀与 `body` 布局
