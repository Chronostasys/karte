# LIR 优化系统使用示例

## 概述

LIR (Low-level Intermediate Representation) 优化系统采用了现代编译器的标准设计模式，包含：

1. **Pass 框架**: 可扩展的优化通道系统
2. **分析框架**: 控制流图分析、数据流分析等
3. **变换框架**: Memory2Reg、死代码消除、常量折叠等优化
4. **优化流水线**: 预配置的优化序列

## 核心设计理念

### Stack-First 策略

新的设计采用 "Stack-First" 策略：

1. **Lowering 阶段**: 将所有变量分配到栈上，简化生成逻辑
2. **Memory2Reg Pass**: 将合适的栈变量提升为寄存器，优化性能
3. **后续优化**: 在寄存器基础上进行进一步优化

这种设计有以下优势：
- **简化 Lowering**: 不需要复杂的寄存器分配逻辑
- **分离关注点**: 正确性和性能优化分离
- **可扩展性**: 易于添加新的优化 Pass

## 使用示例

### 基础使用

```rust
use karte_lir::{LirProgram, OptimizationPipeline, OptimizationPresets};

// 创建或获取 LIR 程序
let mut program = LirProgram::new();

// 使用预设的优化配置
let pipeline = OptimizationPipeline::new(OptimizationPresets::balanced());

// 运行优化
let stats = pipeline.optimize(&mut program)?;

// 查看优化结果
stats.print();
```

### 自定义优化配置

```rust
use karte_lir::{OptimizationConfig, OptimizationPipeline};

let config = OptimizationConfig {
    enable_mem2reg: true,
    enable_dce: true,
    enable_const_fold: true,
    optimization_level: 2,
    debug: true,
};

let pipeline = OptimizationPipeline::new(config);
let stats = pipeline.optimize(&mut program)?;
```

### 手动构建 Pass 序列

```rust
use karte_lir::pass::*;

let mut pass_manager = PassManager::new().with_debug();

// 添加分析 Pass
pass_manager.add_analysis_pass(Box::new(ControlFlowAnalysis::new()));
pass_manager.add_analysis_pass(Box::new(DefUseAnalysis::new()));

// 添加优化 Pass
pass_manager.add_function_pass(Box::new(ConstantFolding::new()));
pass_manager.add_function_pass(Box::new(Memory2RegPass::new()));
pass_manager.add_function_pass(Box::new(DeadCodeElimination::new()));

// 运行优化
pass_manager.run_on_program(&mut program)?;
```

## 优化配置预设

### 调试模式 (Debug)

```rust
let config = OptimizationPresets::debug();
// - 无优化
// - 启用调试输出
// - 保持代码原样
```

### 快速模式 (Fast)

```rust
let config = OptimizationPresets::fast();
// - 基本优化（常量折叠 + Memory2Reg）
// - 编译速度优先
// - 适合开发阶段
```

### 平衡模式 (Balanced)

```rust
let config = OptimizationPresets::balanced();
// - 标准优化集合
// - 平衡编译速度和执行效率
// - 适合生产环境
```

### 高性能模式 (Performance)

```rust
let config = OptimizationPresets::performance();
// - 激进优化
// - 多轮优化
// - 执行效率优先
```

## Pass 框架详解

### 分析 Pass

- **控制流图分析 (CFG)**: 构建基本块和控制流边
- **定义-使用链分析**: 追踪变量的定义和使用
- **活跃变量分析**: 确定变量的生存期

### 变换 Pass

- **Memory2Reg**: 将栈变量提升为寄存器
- **死代码消除 (DCE)**: 移除未使用的代码
- **常量折叠**: 在编译时计算常量表达式

### Pass 依赖管理

Pass 框架自动处理依赖关系：

```rust
impl FunctionPass for Memory2RegPass {
    fn required_analyses(&self) -> Vec<&'static str> {
        vec!["cfg", "def-use"]  // Memory2Reg 需要控制流图和定义使用分析
    }
    
    fn invalidated_analyses(&self) -> Vec<&'static str> {
        vec!["def-use", "cfg"]  // Memory2Reg 会使这些分析失效
    }
}
```

## Memory2Reg Pass 详解

Memory2Reg 是最重要的优化 Pass，它：

1. **识别栈分配**: 找到所有 `Alloc` 指令创建的栈变量
2. **分析使用模式**: 确定哪些栈变量可以安全地提升为寄存器
3. **执行变换**: 将 load/store 操作替换为寄存器移动

### 提升条件

栈变量只有满足以下条件才会被提升：

- 只有简单的 load/store 访问（无偏移）
- 地址没有被传递给其他函数
- 没有复杂的地址运算

### 变换示例

**优化前：**
```
%1 = alloc 8, stack
store %1, 0, 42
%2 = load %1, 0
%3 = add %2, 1
store %1, 0, %3
%4 = load %1, 0
ret %4
```

**优化后：**
```
%5 = mov 42
%6 = add %5, 1
ret %6
```

## 性能监控

优化统计信息提供详细的性能数据：

```rust
pub struct OptimizationStats {
    pub total_time_ms: u64,      // 总执行时间
    pub pass_time_ms: u64,       // Pass 执行时间
    pub total_passes: usize,     // 总 Pass 数量
    pub changed_passes: usize,   // 产生变化的 Pass 数量
    pub unchanged_passes: usize, // 未产生变化的 Pass 数量
    pub failed_passes: usize,    // 失败的 Pass 数量
}
```

## 扩展性

### 添加新的分析 Pass

```rust
#[derive(Debug)]
pub struct MyAnalysis;

impl AnalysisPass for MyAnalysis {
    fn name(&self) -> &str { "my-analysis" }
    
    fn analyze_function(&mut self, function: &LirFunction, analyses: &AnalysisManager) 
        -> Result<Box<dyn AnalysisResult>, String> 
    {
        // 实现分析逻辑
        Ok(Box::new(MyAnalysisResult { /* ... */ }))
    }
}
```

### 添加新的变换 Pass

```rust
#[derive(Debug)]
pub struct MyOptimization;

impl FunctionPass for MyOptimization {
    fn name(&self) -> &str { "my-opt" }
    
    fn run_on_function(&mut self, function: &mut LirFunction, analyses: &mut AnalysisManager) 
        -> PassResult 
    {
        // 实现优化逻辑
        PassResult::Changed
    }
}
```

## 最佳实践

1. **使用预设配置**: 对于大多数情况，预设配置已经足够
2. **启用调试输出**: 在开发阶段启用调试输出了解优化过程
3. **监控统计信息**: 关注优化的效果和性能开销
4. **渐进式优化**: 从较低的优化级别开始，逐步提升
5. **测试验证**: 确保优化后的代码行为正确

## 总结

新的 LIR 优化系统提供了：

- **现代化的 Pass 框架**：可扩展且易于使用
- **Stack-First 策略**：简化 lowering，提高可维护性
- **完整的优化流水线**：从基础到高级的全面优化
- **详细的性能监控**：帮助理解和调优优化过程

这个设计解决了原始的 `spilled_register_storage` 问题，采用了更符合编译器原理的方法。 