# 目前的问题

- [ ] 目前的caller saved register好像处理有问题，虽然暂时测试还没错
- [x] register allocator暂时处理不了effect相关的caller saved register
- [ ] 现在的effect实现写死了相关的register（r0-r4），要重构。涉及文件：transform.rs和ir.rs中的获取inst的reg相关api
- [x] 多层perform测试失败
- [ ] 生命周期分析中将argument假设为永久的生命周期，这个需要优化
- [ ] 更新代数效应文档


