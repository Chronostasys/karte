# KART-E 编译器测试修复方案

## 1. 根本问题诊断

目前大量的测试失败（58个）都归结于同一个根本原因：**虚拟机执行时的 `Memory store out of bounds` 恐慌**。

这个错误发生在LIR执行阶段，表明我们生成的最终机器码尝试向一个非法的内存地址写入数据。日志显示，写入的目标地址通常是一个巨大的负数，这强烈暗示了问题出在**栈帧管理和地址计算**上，而不是核心的算法逻辑（如算术或控制流）。

我们的重构将寄存器分配拆分为多个模块，并与 `StackFrameLowering` Pass 进行解耦。问题很可能就出在这个解耦的接口上，导致 `StackFrameLowering` 在计算栈上变量的最终物理地址时，使用了错误的信息。

## 2. 修复方案三步走

我们将分三个步骤，从最底层、最核心的栈管理逻辑开始，逐步向上修复。

### 第 1 步：修正 `StackFrameLowering` 的栈地址计算

这是问题的核心。`StackFrameLowering` pass 负责将抽象的 `alloc` 指令和溢出槽（Spill Slot）转换为具体的 `[FP + offset]` 形式的地址。

**任务**:
1.  **审查 `StackFrameLowering::run_on_function`**:
    -   仔细检查函数序言（prologue）的生成逻辑。确保 `sub r6, r6, #frame_size` 中的 `frame_size` 计算是**完全正确**的。它必须包含所有局部变量、所有溢出槽、以及保存调用者寄存器所需的空间。
    -   验证 `mov r7, r6` 是否在正确的位置，以确保帧指针（FP）被正确设置。
2.  **审查 `layout_stack_frame` 方法**:
    -   这是最可疑的地方。检查分配局部变量和溢出槽的偏移量计算。偏移量应该是**负数**且相对于`FP`。
    -   **关键点**：确保分配每个变量时，`next_stack_offset` 的更新是正确的，特别是要考虑到每个变量的**大小（size）和对齐（alignment）**。一个常见的错误是忽略了对齐，导致偏移量计算错误。
3.  **审查 `replace_alloc_instructions` 方法**:
    -   当 `alloc` 指令被替换为 `add reg, r7, #offset` 时，要确保 `offset` 是从 `layout_stack_frame` 中获取到的正确值。

**业界最佳实践**:
- 栈向下增长。所有局部变量和溢出槽的偏移量都应该是相对于帧指针（FP）的负数。例如 `[fp-8]`, `[fp-16]`。
- 栈分配必须考虑数据对齐。一个64位（8字节）的值应该被分配在8字节对齐的地址上。偏移量计算必须是 `offset -= size; offset &= !(alignment - 1);` 的形式。

### 第 2 步：稳定 `LinearScanAllocator` 的溢出逻辑

尽管我们已经改进了溢出逻辑，但它仍然可能在某些边缘情况下做出次优选择。一个更稳定、更经典的溢出启发式是**选择下次使用最远的活跃区间进行溢出**。

**任务**:
1.  **重写 `find_spill_candidate`**:
    -   将其逻辑修改为：遍历所有`active_intervals`，对每一个`interval`，计算它在当前指令之后**下一次被使用**的指令位置。
    -   选择那个"下次使用位置"最远的 `interval` 作为溢出候选者。如果一个 `interval` 在未来再也不会被使用，那么它的下次使用位置可以认为是无穷大，是最佳的溢出对象。
    -   **特别注意**：绝对不能选择被标记为 `stack_address_registers` 的区间进行溢出。

**业界最佳实践**:
- Belady的最优页面置换算法在寄存器分配中的应用，就是溢出未来最远才会用到的寄存器。这是线性扫描算法中最经典、效果最稳定的溢出启发式之一。

### 第 3 步：完善 `PassManager` 的分析依赖与失效

您恢复的 `PassManager` 包含了依赖管理和分析失效的逻辑。虽然我们当前的重构暂时简化了这部分，但一个健壮的编译器必须正确处理它。

**任务**:
1.  **为所有Pass添加依赖声明**:
    -   在 `LinearScanRegisterAllocation::new` 中，明确声明它依赖于 `cfg` 和 `def-use` 分析。
        ```rust
        // in LinearScanRegisterAllocation
        fn required_analyses(&self) -> Vec<&'static str> {
            vec!["cfg", "def-use"]
        }
        ```
2.  **实现正确的分析失效**:
    -   在 `LinearScanRegisterAllocation::run_on_function` 的末尾，声明它可能会使哪些分析结果失效。寄存器分配本身通常不会使CFG失效，但它会彻底改变Def-Use关系。
        ```rust
        // in LinearScanRegisterAllocation
        fn invalidated_analyses(&self) -> Vec<&'static str> {
            // 寄存器分配后，寄存器的使用情况完全改变
            vec!["def-use", "liveness"] 
        }
        ```
3.  **在 `PassManager` 中强制执行检查**:
    -   确保 `PassManager` 中的 `strict_dependency_check` 和 `validate_invalidation` 标志能正常工作，在Pass的依赖不满足或失效处理不正确时报错。

**业界最佳实践**:
- LLVM的Pass Manager是这方面的黄金标准。它的核心思想就是每个Pass明确声明自己的依赖、以及它会改变（失效）哪些分析结果。Pass管理器负责按正确的顺序执行，并按需重新计算失效的分析。

## 3. 总结

通过以上三步，我相信我们可以系统性地解决当前所有测试失败的问题，不仅让测试通过，还能构建一个更健壮、更可维护的编译器后端。 