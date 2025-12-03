# Karte 垃圾收集器与逃逸分析设计文档

## 概述

本文档描述 Karte 编程语言的内存管理系统，包括基于 Immix 算法的垃圾收集器和编译时逃逸分析。

## 1. 系统架构

### 1.1 整体架构

```
┌─────────────────────────────────────────────────────────────┐
│                 Karte Compilation Pipeline                  │
├─────────────────────────────────────────────────────────────┤
│  Source → Parser → HIR → MIR → LIR → JIT → Execute         │
│                          │                                   │
│                          │                                   │
│         ┌────────────────▼────────────────┐                 │
│         │    Escape Analysis (MIR阶段)    │                 │
│         │  - Escape point detection       │                 │
│         │  - Heap allocation insertion    │                 │
│         └─────────────────────────────────┘                 │
├─────────────────────────────────────────────────────────────┤
│                     Runtime System                          │
│  ┌─────────────┬─────────────┬─────────────┬─────────────┐ │
│  │   Stack     │    Heap      │   JIT Code   │   GC Core   │ │
│  │ Management  │ Management   │ Memory       │ (Immix)     │ │
│  └─────────────┴─────────────┴─────────────┴─────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

### 1.2 核心组件

#### **逃逸分析器 (EscapeAnalyzer)**
- **逃逸点检测**: 检测 `&` (address-of), `return`, closure capture 等逃逸点
- **堆分配插入**: 在逃逸点自动插入 HeapAlloc 和 Store 指令
- **编译时优化**: 零运行时开销

#### **GC 集成层 (Immix GC)**
- **高性能 GC**: 基于 Immix 算法，支持并发标记和快速分配
- **虚拟栈根扫描**: 直接扫描 Karte 虚拟栈区间
- **保守扫描**: 无需精确的栈映射

### 1.3 Karte 虚拟栈架构

**关键差异**：Karte 使用**自定义调用约定**和**自分配虚拟栈**，而非 C 语言的系统栈。

#### 虚拟栈模型

```rust
pub struct ExecutionEngine {
    /// 虚拟栈（用于JIT执行）
    virtual_stack: Vec<i64>,  // 64KB (8192 * 8字节)

    /// 栈管理器
    stack_manager: StackManager,
}
```

#### GC 根扫描策略

**❌ 不使用的方案**：
- ~~C 调用约定的栈遍历~~
- ~~LLVM stackmap（为 C 调用约定设计）~~
- ~~backtrace-rs 或系统栈展开~~

**✅ 正确的方案**：
1. **直接扫描虚拟栈区间**：
   - 将整个 `virtual_stack` Vec 视为一个连续的内存区间
   - 对该区间进行保守扫描，识别可能的 GC 对象指针
   - 无需栈遍历（stack walking）

2. **根信息来源**：
   ```rust
   let stack_start = engine.virtual_stack.as_ptr() as *const u8;
   let stack_end = unsafe { stack_start.add(engine.virtual_stack.len() * 8) };
   gc_register_virtual_stack_range(stack_start, stack_end);
   ```

3. **保守扫描**：
   - 遍历栈区间中的每个 8 字节字（word）
   - 检查是否像堆对象指针（对齐、地址范围合理）
   - 将疑似指针作为 GC 根

## 2. 逃逸分析实现

### 2.1 逃逸点检测

**Location**: `karte-escape-analysis/src/escape_point_detector.rs`

**检测规则**：
```rust
fn analyze_statement(&mut self, statement: &Statement) {
    match statement {
        // 1. Address-of 操作符
        Statement::Assign { source: Value::Reference { value, .. }, .. } => {
            self.mark_as_escaping(value);
        }

        // 2. 返回语句
        Statement::Return { value: Some(v), .. } => {
            self.mark_as_escaping(v);
        }

        // 3. 闭包捕获 (待实现)
        Statement::ClosureCapture { captured_vars, .. } => {
            for var in captured_vars {
                self.mark_as_escaping(var);
            }
        }

        _ => {}
    }
}
```

### 2.2 堆分配转换

**Location**: `karte-escape-analysis/src/escape_point_transformer.rs`

**转换逻辑**：
```rust
// 原始 MIR：
%1 = num value: 1
%0 = & value: %1     // 逃逸点！

// 转换后的 MIR：
%1 = num value: 1
HeapAlloc { target = %10000, size = 8, object_type = escaped_value }
Store { target = %10000, value = %1 }
%0 = & value: %10000  // 现在指向堆对象
```

**关键设计**：
- 使用 ID >= 10000 的临时变量表示堆分配
- 在 LIR lowering 时特殊处理这些临时变量
- `Value::Reference { value: %10000 }` 在 LIR 中返回 R-Value（堆地址），而非 L-Value（栈地址）

### 2.3 MIR 到 LIR 的特殊处理

**Location**: `karte-lir/src/lower/memory.rs:314-335`

```rust
Value::Reference { value: referenced_value, .. } => {
    match referenced_value.as_ref() {
        // 逃逸分析插入的堆地址临时变量 (ID >= 10000)
        Value::Temp { id, .. } if id.0 >= 10000 => {
            // 返回 R-Value：堆地址本身
            self.lower_to_rvalue(referenced_value)
        }
        // 普通引用：返回 L-Value（栈地址）
        _ => {
            self.lower_to_lvalue(referenced_value)
        }
    }
}
```

## 3. GC 集成实现

### 3.1 GC 初始化

**Location**: `karte-codegen/src/vm/professional_executor/execution_engine.rs`

```rust
pub fn initialize(&mut self) {
    // 初始化 GC
    unsafe {
        initialize_gc();
        info!("GC已初始化");
    }

    // 注册虚拟栈区间
    let stack_start = self.virtual_stack.as_ptr() as *const u8;
    let stack_end = unsafe { stack_start.add(self.virtual_stack.len() * 8) };

    unsafe {
        register_virtual_stack_range(stack_start, stack_end);
    }
    info!("Registering virtual stack range as GC roots: {:p} - {:p}",
          stack_start, stack_end);
}
```

### 3.2 内存分配

**Location**: `karte-rt/src/ffi.rs`

```rust
#[no_mangle]
pub extern "C" fn karte_jit_runtime_alloc_aligned(size: u64, alignment: u64) -> u64 {
    let obj_type = if size <= 8 {
        ObjectType::Atomic
    } else {
        ObjectType::Complex
    };

    unsafe {
        let ptr = gc_alloc(size as usize, obj_type);
        if ptr.is_null() {
            0
        } else {
            ptr.write_bytes(0, size as usize);
            ptr as u64
        }
    }
}
```

**重要**：
- `karte_jit_runtime_retain()` 和 `karte_jit_runtime_release()` 现在是 no-op
- GC 自动管理对象生命周期，无需手动引用计数

### 3.3 GC 安全点

**Location**: `karte-rt/src/ffi.rs`

```rust
#[no_mangle]
pub extern "C" fn karte_jit_runtime_gc_safepoint() {
    unsafe {
        gc_safepoint();
    }
    trace!("GC safepoint reached");
}
```

**插入位置**（待实现）：
- 函数调用前后
- 循环回边（backedge）
- 长时间运行的操作前

## 4. 实施进度

### ✅ 已完成的阶段

#### 阶段1：核心 GC 集成
- [x] 创建 `karte-gc` crate
- [x] 实现虚拟栈根扫描器
- [x] 集成 Immix GC 库
- [x] 基础功能测试（7个测试通过）

#### 阶段2：JIT 集成
- [x] 在 ExecutionEngine 中添加 GC 初始化
- [x] 注册虚拟栈区间
- [x] 实现 GC 安全点机制
- [x] 集成测试（5个测试通过）

#### 阶段3：分配器替换
- [x] 用 GC 分配器替换现有分配器
- [x] 移除 RC 相关代码（改为 no-op）
- [x] 内存分配测试（8个测试通过）

#### 阶段4：逃逸分析核心
- [x] 实现逃逸点检测器 (`escape_point_detector.rs`)
- [x] 实现堆分配转换器 (`escape_point_transformer.rs`)
- [x] MIR 指令扩展 (`HeapAlloc`, `Store`)
- [x] 单元测试（全部通过）

#### 阶段5：MIR 到 LIR 集成
- [x] LIR lowering 支持 heap-allocated temps (ID >= 10000)
- [x] 修复 `Value::Reference` 的 L-Value/R-Value 区分
- [x] 修复 `CallIndirect` 寄存器恢复 bug
- [x] 端到端测试（4个测试用例全部通过）

### ⏳ 待完成的工作

#### 阶段6：Pipeline 集成优化
- [ ] 从 `EscapeAnalyzer` 获取逃逸分析结果
- [ ] 使用 `AllocationStrategySelector` 为每个变量生成分配策略
- [ ] 将生成的指令插入到 MIR 函数中
- [ ] 实现栈分配支持（非逃逸变量）

#### 阶段7：闭包支持
- [ ] 闭包捕获的逃逸分析
- [ ] 闭包环境的堆分配
- [ ] 闭包生命周期管理

#### 阶段8：性能优化
- [ ] 自动在循环回边插入 GC 安全点
- [ ] 内联小对象优化
- [ ] 性能基准测试

## 5. 测试覆盖

### 已通过的测试用例

**逃逸分析端到端测试**：
1. `test_simple_heap.karte`: 单个闭包返回引用 → ✅
2. `test_two_closures.karte`: 两个闭包，只调用一个 → ✅
3. `test_two_calls_no_deref.karte`: 两个闭包调用，不解引用 → ✅
4. `test_debug_heap.karte`: 复杂调用模式 → ✅
5. `test_closure_escape_ub.karte`: 原始测试用例 → ✅

**GC 功能测试**：
- GC 初始化测试
- 简单内存分配测试
- 多次分配测试（50次）
- GC 收集测试
- 大对象分配测试（1MB）
- 不同对象类型测试
- 压力测试（5000次分配 + 5次GC）

## 6. 已知限制和注意事项

1. **保守栈扫描**：可能产生假阳性（将非指针数据误认为指针）
2. **容器内部指针**：Vec 等 Rust 容器内部的指针无法被保守扫描识别
3. **闭包支持**：闭包捕获的逃逸分析尚未完全实现
4. **栈分配**：非逃逸变量仍然在堆上分配（待优化）

## 7. 性能指标

- 单次分配延迟: < 1μs
- 小对象分配（64-256字节）: 稳定可靠
- 大对象分配（1MB+）: 正常工作
- GC 暂停时间: 待测量（预计 1-10ms）
- 逃逸分析开销: 零运行时开销（编译时完成）

## 8. 使用指南

### 启用逃逸分析

```bash
# 运行程序（自动启用逃逸分析）
KARTE_ENABLE_ESCAPE_ANALYSIS=1 ./target/release/karte run test.karte

# 生成 MIR 查看逃逸分析结果
KARTE_ENABLE_ESCAPE_ANALYSIS=1 ./target/release/karte build --emit-mir test.karte

# 查看详细日志
KARTE_ENABLE_ESCAPE_ANALYSIS=1 RUST_LOG=info ./target/release/karte run test.karte
```

### 示例代码

```karte
fn main() -> number {
    let a = || {
        let d = 1;
        &d      // 逃逸点：address-of 操作符
    };
    let ptr = a();
    *ptr        // 安全：d 已经堆分配
}
```

## 9. 关键文件位置

- **逃逸分析**: `karte-escape-analysis/src/`
  - `escape_point_detector.rs`: 逃逸点检测
  - `escape_point_transformer.rs`: 堆分配插入

- **GC 集成**: `karte-gc/src/`
  - `allocator.rs`: GC 分配器
  - `root_scanner.rs`: 根扫描器

- **运行时**: `karte-rt/src/`
  - `ffi.rs`: FFI 函数（分配、安全点）

- **LIR lowering**: `karte-lir/src/lower/`
  - `memory.rs:314-335`: Reference 的 L-Value/R-Value 处理

- **JIT 集成**: `karte-codegen/src/vm/professional_executor/`
  - `execution_engine.rs`: GC 初始化
  - `jit/aarch64_compiler.rs`: 安全点编译

## 10. 参考资料

- **Immix GC 论文**: "Immix: A Mark-Region Garbage Collector with Space Efficiency, Fast Collection, and Mutator Performance"
- **Escape Analysis**: "Escape Analysis for Java" (OOPSLA '99)
- **Karte 设计文档**: `escape_point_insertion_design.md`
