# TODOS

- [ ] 优化codegen模块的结构，加回解释器支持
- [ ] 目前的实现没有重用stack slot，导致stack分配的很大，需要实现重用逻辑
- [ ] simple reg allocator过于简陋，需要重构
- [ ] 在mir层实现pass（是否有办法让pass manager独立于ir？）
- [ ] x86 JIT支持完善
- [ ] 支持C ffi，进一步实现堆分配

...
