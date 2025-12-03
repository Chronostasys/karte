# 类型信息传递修复计划

## 问题分析

### 根本原因
在HIR->MIR lowering过程中，类型信息丢失导致无法正确处理可调用对象作为参数的情况。

### 具体表现
1. `apply(wrong_return, 5)` - 普通函数作为参数 → 之前Bus error，现在修复后可以工作
2. `apply(add_one, 5)` - 闭包作为参数 → 修复后反而Bus error了

### 深层问题
当前的修复策略过于简单：
- 对类型变量（`Type::Var`）默认按函数指针处理
- 但实际上，lambda参数 `f` 的类型在定义时是类型变量，它既可能接收函数指针，也可能接收闭包结构体
- 我们需要在**调用点**根据实际传入的参数类型决定如何处理，而不是在lambda定义点

## 核心问题

在lambda `apply = |f, x| { f(x) }` 内部调用 `f(x)` 时：
- **静态类型**：`f` 的类型是 `Var(TypeVar(0))`（类型变量）
- **运行时值**：
  - 如果调用 `apply(wrong_return, 5)`，`f` 的值是函数指针
  - 如果调用 `apply(add_one, 5)`，`f` 的值是闭包结构体指针

当前实现无法处理运行时多态，因为MIR lowering是静态的。

## 解决方案选项

### 方案A：统一表示形式（推荐）✅
**核心思想**：所有可调用对象（函数和闭包）都使用统一的闭包结构体表示。

**实现细节**：
1. 普通函数在被引用时，包装成闭包结构体 `{function_ptr: func_addr, env_ptr: 0}`
2. 闭包本身已经是闭包结构体
3. 在lambda内部调用参数时，统一按闭包结构体处理（提取function_ptr和env_ptr）

**优点**：
- 解决了运行时多态问题
- 调用约定统一，代码简洁
- 不需要运行时类型判断

**缺点**：
- 直接函数调用会有轻微性能开销（需要包装/解包装）
- 需要修改函数引用的lowering逻辑

### 方案B：传入统一化上下文
**核心思想**：在lowering时传入HIR type checker的统一化表（unification table），将类型变量解析为具体类型。

**实现细节**：
1. 修改`LoweringOptions`，添加`unification_table: &UnificationTable`
2. 在处理类型变量时，通过统一化表解析为具体类型
3. 根据解析后的具体类型决定处理方式

**优点**：
- 更精确的类型信息
- 可能优化某些调用场景

**缺点**：
- 需要大量重构
- 类型变量可能仍然无法完全解析（如高阶函数）
- 增加了lowering的复杂度

### 方案C：运行时类型标记
**核心思想**：在值中携带运行时类型标记。

**缺点**：
- 违反了编译时类型系统的原则
- 增加运行时开销
- 不推荐

## 选定方案：方案A（统一表示形式）

## 详细实现计划

### 阶段1：修改函数引用的lowering
**位置**：`karte-mir/src/lower/expr.rs` - `Expr::Identifier` 处理

**当前行为**：
```rust
// 普通函数
Value::Function { name: "wrong_return", ty: None }
```

**目标行为**：
```rust
// 包装成闭包结构体
Value::Struct {
    name: "Closure",
    fields: {
        "function_ptr": Value::Function { name: "wrong_return", ty: ... },
        "env_ptr": Value::Number { value: 0, ty: None }
    },
    ty: Some(...)
}
```

### 阶段2：修改函数参数调用逻辑
**位置**：`karte-mir/src/lower/expr.rs` - `lower_function_call`

**移除当前的类型变量特殊处理**：
```rust
// 删除这段代码
karte_hir::Type::Var(_) | karte_hir::Type::Unknown => {
    func_val = Value::Function {
        name: format!("__param_{}", name),
        ty: Some(param_type),
    };
    ...
}
```

**统一处理逻辑**：
对所有闭包参数，统一按闭包结构体处理（提取function_ptr和env_ptr字段）。

### 阶段3：回归测试修改
**位置**：现有测试和集成测试

需要更新预期结果，因为函数引用现在会生成闭包结构体。

## 测试计划

### 测试用例1：普通函数作为参数
```karte
fn add_one(n:number) -> number { n + 1 }
let apply = |f, x| { f(x) };
apply(add_one, 5)
```
**预期结果**：6

### 测试用例2：闭包作为参数
```karte
let apply = |f, x| { f(x) };
let add_one = |n| { n + 1 };
apply(add_one, 5)
```
**预期结果**：6

### 测试用例3：带类型标注的函数参数
```karte
fn wrong_return(n:number) -> number { n + 1 }
let apply = |f, x| { f(x) };
apply(wrong_return, 5)
```
**预期结果**：6

### 测试用例4：高阶函数
```karte
fn make_adder(n:number) -> (number -> number) {
    |x| { x + n }
}
let apply = |f, x| { f(x) };
let add_five = make_adder(5);
apply(add_five, 10)
```
**预期结果**：15

### 测试用例5：直接函数调用（确保不破坏）
```karte
fn add(a:number, b:number) -> number { a + b }
add(3, 4)
```
**预期结果**：7

### 测试用例6：闭包直接调用（确保不破坏）
```karte
let add = |a, b| { a + b };
add(3, 4)
```
**预期结果**：7

## 实施步骤

1. ✅ **步骤1**：编写测试用例文件（6个测试）
2. **步骤2**：运行测试，记录当前状态（哪些通过，哪些失败）
3. **步骤3**：实现阶段1 - 修改函数引用的lowering
4. **步骤4**：实现阶段2 - 统一参数调用逻辑
5. **步骤5**：运行所有测试，验证修复
6. **步骤6**：如有失败，分析并调整
7. **步骤7**：运行完整的集成测试套件
8. **步骤8**：清理调试代码，提交修复

## 风险评估

### 高风险项
- 修改函数引用的lowering可能影响大量现有代码
- 需要仔细测试直接函数调用场景

### 缓解措施
- 逐步实施，每步都验证
- 保持详细的测试覆盖
- 在修改前确保所有现有测试通过

## 成功标准

1. 所有6个测试用例通过
2. 现有的集成测试套件全部通过
3. 性能测试显示开销可接受（<5%）
