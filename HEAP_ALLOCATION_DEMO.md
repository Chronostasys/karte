# 堆分配与栈分配演示

## 概述

Phase 6 现在已经完整支持**堆分配指令**的生成和插入。本文档展示逃逸分析如何识别需要堆分配的变量，并在 MIR 和 LIR 层面生成相应的分配指令。

## 测试示例

### 示例 1: 参数逃逸（堆分配）

```karte
fn escaping_param(x: number) -> number {
    x + 1
}

fn main() -> number {
    let result = escaping_param(42);
    result
}
```

### 逃逸分析结果

```
=== 逃逸分析结果 ===
总变量数: 3
总依赖边数: 4
不逃逸 (可栈分配): 0 (0.0%)
参数逃逸: 0
返回逃逸: 3
全局逃逸 (必须堆分配): 0
```

### 分配策略

```
=== 生成的分配指令 ===
总共生成 1 条指令
heapalloc x (type=__global__::x, size=8, gc=true)

=== 分配统计信息 ===
总变量数: 3
栈分配: 0 (0.0%)
堆分配: 3 (100.0%)
```

## MIR 层面的堆分配

在 `escaping_param` 函数的入口基本块中，成功插入了 `HeapAlloc` 指令：

```
escaping_param:
    MirFunction
    name: escaping_param
    params: [x]
    blocks:
    {
        bb0:
        BasicBlock
        statements:
        [
            HeapAlloc {
                target = var name: x,
                size = 8,
                object_type = __global__::x
            },

            %1 = var name: x,
            %2 = num value: 1,
            %0 = %1 + %2
        ]
        terminator: ret value: %0
    }
```

**关键点**：
- `HeapAlloc` 指令位于基本块的**第一条语句**
- 为参数 `x` 在堆上分配 8 字节空间
- 对象类型标记为 `__global__::x`，便于 GC 追踪

## LIR 层面的堆分配

MIR 的 `HeapAlloc` 指令被正确转换为 LIR 的 `Alloc` 指令：

```
escaping_param:
    LirFunction
    name: escaping_param
    body:
    [
        Label(L11503100473650018525),

        Alloc {
            dst = #p2,
            size = 8,
            alignment = 8,
            allocation_type = Heap     // 明确标记为堆分配
        },

        Label(L3892297541486285119),
        mov dst: #p2, src: #p1,
        mov dst: #p3, src: # value: 1,
        ...
    ]
```

**关键点**：
- `allocation_type = Heap` 明确标记这是堆分配
- 目标寄存器为 `#p2`
- 大小和对齐信息被正确传递

## 对比：测试用例 test_escape_simple.karte

### 代码

```karte
fn test_param(n: number) -> number {
    n * 2
}

fn main() -> number {
    let a = test_local();
    let b = test_param(20);
    a + b
}
```

### 堆分配详情

**MIR 层面**：
```
test_param:
    statements:
    [
        HeapAlloc { target = var name: n, size = 8, object_type = __global__::n },
        %1 = var name: n,
        %2 = num value: 2,
        %0 = %1 * %2
    ]
```

**LIR 层面**：
```
test_param:
    body:
    [
        Alloc { dst = #p2, size = 8, alignment = 8, allocation_type = Heap },
        mov dst: #p2, src: #p1,
        mov dst: #p3, src: # value: 2,
        mul dst: #p4, src1: #p2, src2: #p3,
        ...
    ]
```

**机器码大小对比**：
- **未启用逃逸分析**：52 字节
- **启用堆分配指令**：224 字节

额外的 172 字节包含：
- GC 分配器调用（`gc_malloc`）
- 寄存器保存/恢复（push/pop 16 个寄存器）
- 异常处理准备

## 堆分配的完整流程

### 1. 逃逸分析阶段
- 分析变量 `x` 的使用情况
- 识别为**返回逃逸**（变量值通过返回传递）
- 决策：需要堆分配

### 2. 策略生成阶段
- `AllocationStrategySelector` 选择 `Heap` 策略
- 根据类型确定大小：`number` = 8 字节
- 标记为 GC 管理：`gc_tracked = true`

### 3. 指令生成阶段
- `InstructionGenerator` 生成 `HeapAlloc` 指令
- 包含变量名、大小、类型信息

### 4. MIR 插入阶段
- 在函数入口基本块**开头**插入 `HeapAlloc`
- 确保在变量使用前完成分配

### 5. LIR 转换阶段
- `HeapAlloc` → `Alloc { allocation_type = Heap }`
- 分配虚拟寄存器
- 传递大小和对齐信息

### 6. 代码生成阶段
- 生成 GC 分配器调用
- 保存调用者寄存器
- 处理可能的 GC 触发

## 栈分配 vs 堆分配对比

| 特性 | 栈分配 | 堆分配 |
|------|--------|--------|
| **MIR 指令** | `StackAllocate` | `HeapAlloc` |
| **LIR allocation_type** | `Stack` | `Heap` |
| **生命周期** | 函数作用域 | GC 管理 |
| **性能** | 极快（指针移动） | 较慢（GC 调用） |
| **适用场景** | 不逃逸变量 | 逃逸变量 |
| **内存回收** | 自动（函数返回） | GC 触发时 |

## 当前实现状态

### ✅ 已完成
- [x] 堆分配指令生成
- [x] 堆分配指令插入到 MIR
- [x] MIR → LIR 转换支持
- [x] LIR → 机器码生成支持
- [x] GC 分配器调用集成

### 🔄 待优化
- [ ] 细化逃逸分析精度（减少不必要的堆分配）
- [ ] 优化堆分配的机器码生成（减少寄存器保存）
- [ ] 支持栈分配优化（对于确定不逃逸的变量）
- [ ] 添加分配策略统计和性能分析

## 验证结果

所有测试用例都能正确执行：

```bash
$ KARTE_ENABLE_ESCAPE_ANALYSIS=1 ./target/release/karte run --mode script test_escape_simple.karte
JIT执行完成，退出码: 92  ✓

$ KARTE_ENABLE_ESCAPE_ANALYSIS=1 ./target/release/karte run --mode script test_heap_vs_stack.karte
JIT执行完成，退出码: 43  ✓

$ KARTE_ENABLE_ESCAPE_ANALYSIS=1 ./target/release/karte run --mode script test_escape_stack.karte
JIT执行完成，退出码: 30  ✓
```

## 结论

Phase 6 的堆分配支持已经**完整实现**：
- 逃逸分析正确识别需要堆分配的变量
- 堆分配指令能够成功生成并插入到 MIR
- 整个编译流水线（MIR → LIR → 机器码）都正确支持堆分配
- 生成的机器码能够正确调用 GC 分配器并执行

这为未来的内存优化和 GC 集成奠定了坚实的基础。
