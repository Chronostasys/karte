# 类型信息传递修复总结

## 问题描述

在HIR->MIR lowering过程中，当普通函数或闭包作为参数传递时，由于类型信息丢失导致Bus error或参数传递错误。

### 原始问题
```karte
fn wrong_return(n:number) -> number { n + 1 }
let apply = |f, x| { f(x) };
let add_one = |n| { n + 1 };

apply(wrong_return, 5)  // 之前：Bus error
apply(add_one, 5)       // 之前：Bus error
```

## 解决方案：统一表示形式 + Wrapper函数

### 核心思想
1. **统一表示**：所有可调用对象（函数和闭包）都使用闭包结构体表示
2. **Wrapper函数**：为普通函数创建wrapper以适配闭包调用约定
3. **类型信息传递**：修改HIR type checker存储所有表达式的类型信息

### 实现细节

#### 1. HIR Type Checker修改 (`karte-hir/src/type_checker.rs`)
- 添加 `expr_types` 字段存储所有表达式的类型信息（不仅仅是Lambda）
- 在 `infer_expr` 方法中记录每个表达式的推断类型
- `get_lambda_types()` 现在返回所有表达式类型

#### 2. 函数引用的Lowering (`karte-mir/src/lower/expr.rs:84-182`)
当引用普通函数时：
1. 创建一个wrapper lambda函数，签名：`wrapper(env_ptr, ...args)`
2. Wrapper内部调用原函数：`original_function(...args)` (不传递env_ptr)
3. 创建闭包结构体指向wrapper：
   ```
   Closure {
       function_ptr: wrapper_function,
       env_ptr: 0
   }
   ```

#### 3. 调用约定统一 (`karte-mir/src/lower/expr.rs:1224-1228`)
- 移除了类型变量的特殊处理
- 所有闭包参数统一按闭包结构体处理
- 调用时提取 `function_ptr` 和 `env_ptr`，调用 `function_ptr(env_ptr, ...args)`

### 为什么需要Wrapper？

**调用约定差异**：
- 闭包调用：`function_ptr(env_ptr, arg1, arg2, ...)`
- 普通函数：`function(arg1, arg2, ...)`

**问题案例**：
- 原函数：`wrong_return(n)` 只接受1个参数
- 如果直接作为闭包调用：`wrong_return(env_ptr=0, n=5)`
- 结果：`n` 接收到 `env_ptr=0`，返回 `0+1=1` ❌

**Wrapper解决方案**：
```rust
// 生成的wrapper
fn wrong_return$wrapper(__env, __arg0) {
    wrong_return(__arg0)  // 不传递__env
}

// 闭包调用
wrong_return$wrapper(env_ptr=0, 5)  // → wrong_return(5) → 6 ✓
```

## 测试结果

### 全部通过 ✅

| 测试用例 | 描述 | 预期结果 | 实际结果 | 状态 |
|---------|------|---------|---------|------|
| Test 1 | 普通函数作为参数 | 6 | 6 | ✅ |
| Test 2 | 闭包作为参数 | 6 | 6 | ✅ |
| Test 3 | 带类型标注的函数参数 | 6 | 6 | ✅ |
| Test 5 | 直接函数调用 | 7 | 7 | ✅ |
| Test 6 | 直接闭包调用 | 7 | 7 | ✅ |
| 原始例子 | `examples/type_annotation_error.karte` | 6 | 6 | ✅ |

### 测试代码
```karte
// Test 1: 普通函数作为参数
fn add_one(n:number) -> number { n + 1 }
let apply = |f, x| { f(x) };
apply(add_one, 5)  // 结果: 6 ✅

// Test 2: 闭包作为参数
let apply = |f, x| { f(x) };
let add_one = |n| { n + 1 };
apply(add_one, 5)  // 结果: 6 ✅

// Test 3: 带类型标注的函数参数
fn wrong_return(n:number) -> number { n + 1 }
let apply = |f, x| { f(x) };
apply(wrong_return, 5)  // 结果: 6 ✅
```

## 性能影响

### 开销分析
1. **Wrapper函数**：每次引用普通函数时生成一个wrapper（编译时开销）
2. **间接调用**：增加一层函数调用（运行时开销）
3. **闭包结构体**：额外的内存分配（运行时开销）

### 预期影响
- 编译时间：略微增加（每个函数引用生成wrapper）
- 运行时性能：预计 <5% 开销（主要来自间接调用）
- 内存使用：每个函数引用增加一个闭包结构体（16字节）

### 优化空间
未来可以考虑：
1. **内联优化**：在LIR层检测简单wrapper并内联
2. **特化**：为常见场景生成特化代码
3. **缓存wrapper**：对同一函数的多次引用共享wrapper

## 修改的文件

### 核心修改
1. `karte-hir/src/type_checker.rs`
   - 添加 `expr_types` 字段
   - 修改 `infer_expr` 存储所有表达式类型
   - 更新 `get_lambda_types()` 返回所有类型

2. `karte-mir/src/lower/expr.rs`
   - 修改函数引用lowering（84-182行）
   - 移除类型变量特殊处理（1224-1228行）
   - 简化函数调用逻辑（1247-1259行）

### 支持代码
- 使用了现有的 `clone_scopes()` 和 `restore_scopes()` 方法
- 利用现有的闭包结构体表示

## 向后兼容性

### 保持兼容
- ✅ 直接函数调用不受影响
- ✅ 闭包直接调用不受影响
- ✅ 现有测试套件应该全部通过

### 可能的影响
- ⚠️ 生成的MIR/LIR代码结构变化（多了wrapper函数）
- ⚠️ 如果有代码依赖MIR/LIR的具体结构，可能需要更新

## 后续工作

### 必须完成
- [x] 运行完整的集成测试套件（所有222个测试通过）
- [x] 清理调试输出代码（移除了11个println!调试语句）
- [x] 更新相关文档（CLAUDE.md）
- [x] 添加回归测试到karte-tests包

### 回归测试
在 `karte-tests/src/cli_integration_tests.rs` 中添加了4个测试用例：
1. **test_plain_function_as_higher_order_param**: 测试普通函数作为高阶函数参数
2. **test_closure_as_higher_order_param**: 测试闭包作为高阶函数参数
3. **test_mixed_function_and_closure_params**: 测试混合使用函数和闭包作为参数
4. **test_typed_function_param**: 测试带类型标注的函数参数

这些测试确保未来不会出现回归，保护了这次修复的核心功能。

### 可选优化
- [ ] 实现wrapper内联优化
- [ ] 性能基准测试
- [ ] 考虑缓存机制减少重复wrapper

## 关键经验

### 成功要素
1. **系统化方法**：先分析根因，制定完整计划，再逐步实施
2. **测试驱动**：先创建测试用例，确保修复正确
3. **增量修改**：分阶段实施，每步都验证

### 遇到的挑战
1. **类型变量问题**：无法静态区分函数和闭包 → 使用统一表示
2. **调用约定差异**：函数和闭包参数不同 → 使用wrapper适配
3. **参数传递错误**：最初忘记wrapper → 通过测试发现并修复

### 设计原则
- **最小惊讶原则**：用户代码不需要改变
- **统一性优于特殊情况**：统一表示简化逻辑
- **正确性优于性能**：先保证正确，再优化性能
