# Pass 系统重构说明

## 重构概述

本次重构对 `karte-lir/src/pass/` 系统进行了全面升级，提升了灵活性和可用性。

**关键改进**：
- Pass 的名称和描述从 Pass 实例本身获取，无需硬编码
- 新增 CLI 命令支持列出 Pass 和使用自定义管线
- 所有 Pass 都实现了描述信息

## 新增功能

### 1. PassRegistry - Pass 注册表系统

**文件**: `karte-lir/src/pass/pass_registry.rs`

**功能**:
- 集中管理所有可用的 Pass
- 支持通过字符串名称动态构建 Pass
- 预注册所有标准 Pass (分析、转换、优化)
- 列出所有可用 Pass 及其描述

**使用示例**:
```rust
use karte_lir::pass::PassRegistry;

// 创建注册表(自动注册所有标准Pass)
let registry = PassRegistry::default();

// 列出所有可用的Pass
registry.print_available_passes();

// 从字符串构建Pass管线
let manager = registry.build_pipeline_from_string("cfg,def-use,dce,const-fold")?;
```

**已注册的 Pass**:

**分析 Pass**:
- `cfg` - 控制流图分析
- `def-use` - 定义-使用链分析
- `lifetime` - 生命周期分析

**转换 Pass**:
- `dce` - 死代码消除
- `const-fold` - 常量折叠
- `peephole` - 窥孔优化
- `mem2reg` - Memory2Reg优化
- `phi-elim` - Phi节点消除
- `ssa` - SSA构造
- `effect-lower` - Effect指令降级
- `instr-lower` - 通用指令降级
- `stack-layout` - 栈帧布局
- `reg-alloc` - 简单栈寄存器分配

**Utility Pass**:
- `print-ir` - 打印IR(调试用)
- `verify` - 验证IR正确性
- `stats` - 收集IR统计信息

### 2. Utility Passes - 工具 Pass

**文件**: `karte-lir/src/pass/utils.rs`

#### PrintIRPass - 打印IR

在优化管线中的任意位置插入,观察IR状态。

```rust
// 使用标准IR格式打印函数
let print_pass = PrintIRPass::new();

// 或者自定义名称标识位置
let print_pass = PrintIRPass::with_name("优化后");
```

**输出示例**:
```
=== print-ir ===
fn main params: 0
body:
    mov #v1, 5
    mov #v2, 10
    add #v3, #v1, #v2
    ret #v3
=== print-ir 结束 ===
```

#### VerifyPass - 验证IR

检查IR的正确性,包括:
- 寄存器定义后再使用
- 跳转目标合法性
- Phi节点正确性

```rust
let verify_pass = VerifyPass::new();

// 启用严格模式(更多检查)
let verify_pass = VerifyPass::new().with_strict();
```

#### StatisticsPass - 统计信息

收集IR统计数据:
- 指令数量
- 基本块数量
- 寄存器使用情况

```rust
let stats_pass = StatisticsPass::new();
```

**输出示例**:
```
=== 函数统计: main ===
  指令数量: 124
  参数数量: 2
  标签数量: 8
  虚拟寄存器数量: 45
  物理寄存器数量: 6
```

### 3. PassManager 增强

**新增方法**:

#### print_pipeline()

打印当前管道中的所有Pass,便于调试和文档记录。

```rust
let mut manager = PassManager::new();
manager.add_analysis_pass(Box::new(ControlFlowAnalysis::new()));
manager.add_function_pass(Box::new(DeadCodeElimination::new()));

manager.print_pipeline();
```

**输出示例**:
```
=== Pass 管道 ===

分析 Pass:
  1. cfg

函数级别 Pass:
  1. dce

总计: 2 个 Pass
===================
```

#### pass_count()

返回管道中Pass的总数量。

```rust
let count = manager.pass_count();
```

### 4. OptimizationPipeline 增强

**新增方法**:

#### list_available_passes()

列出所有可用的Pass及其描述:

```rust
OptimizationPipeline::list_available_passes();
```

#### optimize_with_custom_pipeline()

使用字符串定义的自定义Pass管线优化程序:

```rust
let stats = OptimizationPipeline::optimize_with_custom_pipeline(
    &mut program,
    "cfg,def-use,print-ir,dce,const-fold,print-ir,verify",
    true  // debug模式
)?;
```

**管线字符串格式**:
- Pass名称用逗号分隔
- 可以重复使用同一个Pass
- 建议在关键位置插入 `print-ir` 观察变化
- 使用 `verify` 确保IR正确性

## 代码清理

### optimization_pipeline.rs

**删除内容**:
- 清理了所有过时的注释代码(约100行)
- 删除了注释掉的多轮优化逻辑
- 删除了注释掉的寄存器分配架构代码

**简化内容**:
- `configure_professional_passes()` 方法简化为8个清晰的阶段
- 每个阶段都有简洁的注释
- 使用 `print_pipeline()` 替代手动打印日志

## 使用示例

### 示例1: 自定义优化管线

```rust
use karte_lir::OptimizationPipeline;

// 在DCE前后观察IR变化
let stats = OptimizationPipeline::optimize_with_custom_pipeline(
    &mut program,
    "cfg,def-use,print-ir,dce,print-ir,verify",
    true
)?;

println!("优化完成: 删除了 {} 条指令",
    stats.instructions_before - stats.instructions_after);
```

### 示例2: 调试特定Pass

```rust
// 只运行特定Pass组合进行调试
let stats = OptimizationPipeline::optimize_with_custom_pipeline(
    &mut program,
    "cfg,const-fold,print-ir",
    true
)?;
```

### 示例3: 在管线中插入统计

```rust
let stats = OptimizationPipeline::optimize_with_custom_pipeline(
    &mut program,
    "stats,dce,stats,const-fold,stats",
    false
)?;
```

## 架构改进

### 1. 关注点分离

- **PassRegistry**: 负责Pass的注册和构建
- **PassManager**: 负责Pass的执行和管理
- **OptimizationPipeline**: 负责预定义的优化流程

### 2. 可扩展性

添加新Pass非常简单:

```rust
// 1. 实现Pass trait
pub struct MyCustomPass;

impl FunctionPass for MyCustomPass {
    fn name(&self) -> &str { "my-pass" }
    // ...
}

// 2. 在PassRegistry中注册
registry.register_function_pass(
    "my-pass",
    "我的自定义Pass",
    Box::new(|| Box::new(MyCustomPass::new())),
);
```

### 3. 可测试性

- 可以轻松构建测试特定Pass的管线
- `verify` Pass帮助发现Pass实现中的bug
- `print-ir` Pass帮助观察Pass的效果

## 向后兼容性

所有现有的API保持不变:
- `OptimizationPipeline::new()`
- `OptimizationPipeline::create_professional_pipeline()`
- `pipeline.optimize()`

新功能是额外的,不影响现有代码。

## CLI 接口

### 列出所有可用的 Pass

```bash
karte list-passes
```

**输出示例**:
```
=== 可用的函数级别 Pass ===
  const-fold: 常量折叠 - 计算编译时可确定的常量表达式
  dce: 死代码消除 - 移除未使用的指令和寄存器定义
  print-ir: 打印IR - 使用标准IR格式打印函数(调试用)
  ...

=== 可用的分析 Pass ===
  cfg: 控制流图分析 - 构建基本块和控制流信息
  def-use: 定义-使用链分析 - 跟踪每个寄存器的定义和使用位置
  ...
```

### 使用自定义 Pass 管线优化代码

```bash
# 基本用法
karte optimize input.lir -p "cfg,dce,const-fold" -o output.lir

# 启用调试模式（显示管线信息）
karte optimize input.lir -p "print-ir,dce,print-ir" --debug

# 不指定输出文件时，直接打印到标准输出
karte optimize input.lir -p "dce,stats"

# 查看优化前后的对比
karte optimize input.lir -p "print-ir,dce,peephole,print-ir" --debug
```

**参数说明**:
- `-p, --pipeline <PIPELINE>`: Pass管线（逗号分隔）
- `-o, --output <OUTPUT>`: 输出文件路径（可选）
- `--debug`: 启用调试模式，打印管线信息
- `--verbose`: 详细模式，打印统计信息

### 实际使用示例

#### 示例1: 观察优化效果

```bash
# 先构建生成LIR
karte build test.karte

# 使用自定义管线观察优化
karte optimize target/main.lir \
  -p "print-ir,dce,const-fold,print-ir" \
  --debug \
  -o target/optimized.lir
```

#### 示例2: 收集统计信息

```bash
karte optimize target/main.lir \
  -p "dce,peephole,stats" \
  --verbose
```

**输出**:
```
=== 函数统计: main ===
  指令数量: 42
  参数数量: 2
  标签数量: 5
  虚拟寄存器数量: 0
  物理寄存器数量: 6

=== 优化统计 ===
优化前指令数: 50
优化后指令数: 42
指令减少: 8
执行的Pass数: 3
成功优化Pass数: 2
总耗时: 5ms
```

#### 示例3: 调试特定 Pass

```bash
# 测试单个Pass的效果
karte optimize target/main.lir -p "dce" --debug

# 测试Pass组合
karte optimize target/main.lir -p "dce,peephole" --debug

# 在Pass之间插入验证
karte optimize target/main.lir -p "dce,verify,peephole,verify" --debug
```

## 后续工作建议

1. **添加更多Utility Pass**:
   - `dump-cfg` - 导出CFG图形
   - `count-instructions` - 按类型统计指令
   - `measure-complexity` - 计算圈复杂度

2. **Pass依赖验证增强**:
   - 自动检测Pass依赖关系
   - 自动插入必需的分析Pass

3. **性能分析**:
   - 记录每个Pass的执行时间
   - 生成优化性能报告

4. **Pass组合预设**:
   - 定义常用的Pass组合(如 "quick", "balanced", "aggressive")
   - 支持从配置文件加载Pass管线
