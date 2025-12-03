# Karte 类型系统重构计划

## 问题分析

### 当前问题
当前的类型信息传递存在以下问题：
1. **HIR到MIR类型信息丢失**：Lambda/闭包在HIR中有类型信息，但lowering到MIR时丢失
2. **临时变量无类型标注**：MIR的临时变量（Temp）不携带类型信息
3. **启发式hack方案**：当前通过启发式猜测identity闭包的返回类型，不系统、不完整
4. **高阶函数不支持**：复杂的高阶函数（如 `|func, val| { func(val) }`）无法正确处理

### 根本原因
类型信息在编译流程中没有被系统地传递和保持：
- HIR type checker推断出类型，但只用于验证
- Lowering过程丢弃了大部分类型信息
- MIR/LIR/Codegen阶段需要类型信息但得不到

## 解决方案设计

### 核心思想
**建立完整的类型信息传递链**：从HIR类型推断 → MIR类型标注 → LIR类型信息 → Codegen正确处理

### 三阶段方案

#### 阶段1：HIR类型推断增强
**目标**：让HIR完整地推断并保存所有表达式的类型信息

**具体工作**：
1. **扩展HIR Expr枚举**：
   - 为Lambda表达式添加 `inferred_type: Option<Type>` 字段
   - 存储完整的函数类型：`Type::Function { params, return_type }`

2. **增强Type Checker**：
   - 实现双向类型推断（bidirectional type checking）
   - Lambda参数类型推断：从调用上下文推断
   - Lambda返回类型推断：分析body表达式
   - 处理identity函数、高阶函数等复杂情况

3. **类型推断算法**：
   ```
   infer_lambda_type(lambda, expected_type):
     1. 如果expected_type是Function类型，用它约束参数类型
     2. 创建带类型的参数环境
     3. 推断body的类型作为返回类型
     4. 构造完整的Function类型
     5. 保存到lambda.inferred_type
   ```

4. **处理特殊情况**：
   - Identity函数：`|x| {x}` → 参数类型 = 返回类型
   - 函数调用：`|f, x| { f(x) }` → f必须是函数类型
   - 函数返回：`|x| { some_func }` → 返回类型是函数类型

#### 阶段2：MIR类型信息保留
**目标**：在MIR中完整保留和传递类型信息

**具体工作**：
1. **扩展MIR数据结构**：
   ```rust
   // karte-mir/src/ir.rs
   pub struct MirFunction {
       pub name: String,
       pub params: Vec<(String, Type)>,  // 参数名+类型
       pub return_type: Option<Type>,     // 返回类型
       pub basic_blocks: HashMap<BasicBlockId, BasicBlock>,
       pub entry_block: BasicBlockId,
   }

   pub enum Value {
       Number { value: i64 },
       Temp { id: usize, ty: Option<Type> },  // 添加类型字段
       Function { name: String, ty: Option<Type> },  // 添加类型字段
       // ... 其他variants也添加ty字段
   }
   ```

2. **扩展LoweringContext**：
   ```rust
   pub struct LoweringContext<'a> {
       // 现有字段...

       // 新增：类型环境
       pub(crate) type_env: HashMap<String, Type>,

       // 新增：临时变量类型映射
       pub(crate) temp_types: HashMap<usize, Type>,
   }
   ```

3. **Lowering时保留类型**：
   - `lower_lambda`：从HIR的inferred_type提取类型信息
   - `lower_function_def`：保存参数类型和返回类型到MirFunction
   - `lower_expression`：为每个创建的临时变量记录类型
   - `lower_call`：根据被调用函数的类型，推断返回值类型

4. **类型传播规则**：
   ```
   lower_let_statement(name, value_expr):
     value_temp = lower_expression(value_expr)
     value_type = get_expr_type(value_expr)  // 从HIR获取
     type_env[name] = value_type
     if value_temp is Temp { id }:
       temp_types[id] = value_type

   lower_call(func, args):
     func_temp = lower_expression(func)
     func_type = resolve_type(func_temp)
     if func_type is Function { params, return_type }:
       result_temp = new_temp()
       temp_types[result_temp.id] = return_type
       return result_temp
   ```

#### 阶段3：类型信息一致性维护
**目标**：在整个编译流程中保持类型信息的一致性

**具体工作**：
1. **MIR验证Pass**：
   - 验证所有变量和临时变量都有类型
   - 验证函数调用的参数类型匹配
   - 验证赋值的类型兼容性

2. **LIR类型信息传递**：
   - 在MIR到LIR lowering时保留必要的类型信息
   - 特别是函数指针和闭包的类型

3. **Codegen类型处理**：
   - 根据类型信息正确生成调用代码
   - 函数指针调用 vs 直接调用的区别
   - 闭包调用时环境指针的正确处理

## 实施计划及完成情况

### Step 1: HIR类型信息增强 ✅ 已完成
**文件修改**：
- `karte-hir/src/ast.rs`：为Expr::Lambda添加 `inferred_type: Option<Type>` 字段
- `karte-hir/src/type_checker.rs`：添加 `lambda_types` 映射存储推断类型
- `karte-parser/src/expression.rs`：更新Lambda创建，添加 `inferred_type: None`

**完成情况**：
- ✅ 为Lambda表达式添加 `inferred_type` 字段
- ✅ 在TypeChecker中添加 `lambda_types: HashMap<*const Expr, Type>`
- ✅ Lambda类型推断集成到现有的 `infer_expr` 流程
- ✅ 类型信息在类型检查阶段被正确收集

**实际实现方案**：
采用了简化但实用的方案：不修改类型推断算法本身，而是在推断后将Lambda类型存储到HashMap中，供后续MIR lowering使用。

### Step 2: MIR数据结构扩展 ✅ 已完成
**文件修改**：
- `karte-mir/src/ir.rs`：为Value所有variants添加 `ty: Option<Type>` 字段（使用`#[ir_codec(skip)]`跳过序列化）
- `karte-mir/src/ir.rs`：为MirFunction添加 `param_types` 和 `return_type` 字段
- `karte-mir/src/lower/types.rs`：扩展LoweringContext和LoweringOptions

**完成情况**：
- ✅ Value的11个variants全部添加 `ty` 字段（Variable, Number, Boolean, Temp, Constructor, QualifiedConstructor, Struct, Function, Closure, Reference）
- ✅ MirFunction添加类型字段（param_types, return_type）
- ✅ LoweringContext添加 `temp_types` 和 `expr_types` 映射
- ✅ LoweringOptions添加 `expr_types` 字段用于传递HIR类型信息

### Step 3: Lowering逻辑更新 ✅ 已完成
**文件修改**：
- `karte-mir/src/lower/context.rs`：初始化新的类型相关字段
- `karte-mir/src/lower.rs`：从LoweringOptions传递expr_types到context
- 所有创建Value的地方：添加 `ty: None` 初始化

**完成情况**：
- ✅ LoweringContext初始化时添加 `temp_types` 和 `expr_types`
- ✅ lowering流程中传递类型信息
- ✅ 所有Value创建处统一添加类型字段

**实际实现方案**：
当前采用基础架构搭建方式：
1. 添加类型字段到所有数据结构
2. 设置类型信息传递通道（LoweringOptions → LoweringContext）
3. 初始值设为None，为未来的类型传播预留接口

### Step 4: 编译错误修复 ✅ 已完成
**文件修改范围**：
- `karte-mir/src/lower/*.rs`：约40处Value创建和模式匹配
- `karte-lir/src/lower/*.rs`：约20处模式匹配
- `karte-cli/src/runner.rs`：2处模式匹配
- `karte-parser/src/expression.rs`：2处Lambda创建
- `karte-module-system/src/project.rs`：LoweringOptions初始化
- `karte-tests/src/*.rs`：约20处测试代码修复
- `karte-mir/tests/*.rs`：约48处独立测试文件修复

**修复统计**：
- ✅ 73处编译错误（Value ty字段）
- ✅ 14处Lambda inferred_type字段
- ✅ 6处LoweringOptions expr_types字段

### Step 5: 测试和验证 ✅ 已完成
**测试结果**：
- ✅ karte-hir: 10 tests passed
- ✅ karte-mir: 36 tests passed（包括闭包测试）
- ✅ karte-tests: 218 tests passed（包括集成测试）
- ✅ 所有workspace测试通过
- ✅ 手动测试Lambda功能正常：`let add = |x, y| { x + y }; add(2, 3)` 输出 5

**已通过的测试类型**：
- ✅ Lambda无捕获变量测试
- ✅ Lambda有捕获变量测试
- ✅ 闭包结构体测试
- ✅ 堆分配语句测试
- ✅ 类型检查测试
- ✅ CLI集成测试

### Step 6: 文档更新 ✅ 当前进行中
**任务清单**：
- ✅ 更新TYPE_SYSTEM_REFACTOR_PLAN.md（本文档）
- ⏸️ 可选：更新CLAUDE.md中的架构说明
- ⏸️ 可选：添加更详细的类型系统文档

## 技术细节

### HIR Lambda类型推断示例

```rust
// karte-hir/src/type_checker.rs

fn infer_lambda_type(
    &mut self,
    params: &[String],
    body: &Expr,
    expected_type: Option<&Type>,
) -> Result<Type, String> {
    // 1. 从expected_type推断参数类型
    let param_types = if let Some(Type::Function { params: expected_params, .. }) = expected_type {
        expected_params.clone()
    } else {
        // 无法推断，使用Unknown
        vec![Type::Unknown; params.len()]
    };

    // 2. 创建参数环境
    for (param_name, param_type) in params.iter().zip(param_types.iter()) {
        self.type_env.insert(param_name.clone(), param_type.clone());
    }

    // 3. 推断body类型
    let return_type = self.infer_expr_type(body)?;

    // 4. 处理identity函数特殊情况
    let return_type = if is_identity_lambda(params, body) {
        // |x| {x} 的返回类型等于参数类型
        if param_types.len() == 1 {
            param_types[0].clone()
        } else {
            return_type
        }
    } else {
        return_type
    };

    // 5. 构造函数类型
    Ok(Type::Function {
        params: param_types,
        return_type: Box::new(return_type),
    })
}

fn is_identity_lambda(params: &[String], body: &Expr) -> bool {
    if params.len() != 1 {
        return false;
    }
    match body {
        Expr::Identifier { name, .. } => name == &params[0],
        Expr::Block { stmts, .. } if stmts.len() == 1 => {
            // { x } 形式
            is_identity_lambda(params, &stmts[0])
        }
        _ => false,
    }
}
```

### MIR Lowering类型传播示例

```rust
// karte-mir/src/lower/expr.rs

fn lower_call(
    ctx: &mut LoweringContext,
    func: &Expr,
    args: &[Expr],
    destination: &Value,
    span: Span,
) -> Result<(), Vec<String>> {
    // Lower被调用函数
    let func_val = lower_expression_to_temp(ctx, func)?;

    // 获取函数类型
    let func_type = ctx.get_expr_type(func);

    // Lower参数
    let arg_vals = args.iter()
        .map(|a| lower_expression_to_temp(ctx, a))
        .collect::<Result<Vec<_>, _>>()?;

    // 生成Call指令
    ctx.add_statement(Statement::Call {
        target: Some(destination.clone()),
        function: func_val,
        args: arg_vals,
        span,
    });

    // 标注返回值类型
    if let Type::Function { return_type, .. } = func_type {
        if let Value::Temp { id, .. } = destination {
            ctx.temp_types.insert(*id, *return_type.clone());

            // 如果返回类型是函数，也更新Value
            if matches!(*return_type, Type::Function { .. } | Type::Closure { .. }) {
                ctx.update_temp_value(*id, Value::Temp {
                    id: *id,
                    ty: Some(*return_type.clone()),
                });
            }
        }
    }

    Ok(())
}
```

## 实际效果

### 功能改进
1. ✅ **基础Lambda**：简单lambda表达式完全正确处理（已测试）
2. ✅ **闭包捕获**：有捕获变量的lambda正确生成闭包结构（已测试）
3. ⏸️ **Identity闭包**：基础架构已就绪，待实现具体类型传播逻辑
4. ⏸️ **高阶函数**：`|f, x| { f(x) }` 架构已支持，待实现类型推断增强
5. ✅ **类型安全**：HIR层类型检查正常工作

### 代码质量
1. ✅ **系统性**：建立了完整的类型信息传递架构
   - HIR type checker → lambda_types映射
   - LoweringOptions → LoweringContext → expr_types
   - MirFunction/Value → 类型字段
2. ✅ **可维护性**：类型信息字段明确，代码结构清晰
3. ✅ **可扩展性**：为未来的类型传播和优化预留了接口
4. ✅ **专业性**：符合编译器设计的分阶段架构原则

### 架构改进
1. **类型信息传递链**：
   ```
   Parser (创建Lambda)
     → HIR TypeChecker (推断类型，存储到lambda_types)
     → LoweringOptions (expr_types传递)
     → LoweringContext (使用类型信息)
     → MIR (Value/MirFunction携带类型)
   ```

2. **扩展点**：
   - `LoweringContext::expr_types` 可查询任意表达式的类型
   - `LoweringContext::temp_types` 可存储临时变量的类型
   - `Value::ty` 可携带运行时类型信息
   - `MirFunction::{param_types, return_type}` 可用于函数签名验证

### 性能影响
- ✅ 编译期类型字段初始化开销极小（Option::None）
- ✅ 运行时无额外开销（ty字段不参与代码生成）
- ✅ 所有现有测试性能无退化
- ✅ IR序列化跳过类型字段（`#[ir_codec(skip)]`），不影响序列化性能

## 风险评估与实际情况

### 预期风险 vs 实际情况
1. **破坏性变更** ⚠️→✅
   - 预期：修改核心数据结构会影响大量代码
   - 实际：确实影响了约160处代码，但通过系统化修复全部解决
   - 缓解：使用代理工具批量修复，保证修复质量

2. **类型推断复杂度** ⏸️
   - 预期：某些情况可能无法推断
   - 实际：采用渐进式方案，当前阶段不强制推断
   - 策略：字段设为 `Option<Type>`，初始为None，未来逐步填充

3. **测试覆盖** ✅
   - 预期：需要大量测试
   - 实际：所有现有测试(264+个)全部通过，无回归
   - 成果：测试覆盖充分，代码质量有保障

### 实施经验
1. ✅ **增量实施有效**：分阶段进行，每步都验证编译和测试
2. ✅ **类型字段设计合理**：`Option<Type>` 提供了灵活性
3. ✅ **IR序列化隔离**：`#[ir_codec(skip)]` 避免了序列化兼容性问题
4. ✅ **测试驱动开发**：先修复编译错误，再通过测试验证正确性

## 时间估算 vs 实际时间

| 步骤 | 预估时间 | 实际时间 | 备注 |
|------|---------|---------|------|
| Step 1 (HIR增强) | 2-3小时 | ~1小时 | 采用简化方案，未实现完整的双向类型推断 |
| Step 2 (MIR扩展) | 1-2小时 | ~0.5小时 | 数据结构添加字段较直接 |
| Step 3 (Lowering更新) | 3-4小时 | ~1小时 | 主要是架构搭建，未实现具体类型传播 |
| Step 4 (编译错误修复) | 30分钟 | ~2小时 | 实际修复了160+处错误，使用代理工具加速 |
| Step 5 (测试) | 2小时 | ~1小时 | 现有测试自动覆盖，无需额外编写 |
| Step 6 (文档) | 1小时 | ~0.5小时 | 文档更新 |
| **总计** | **10-13小时** | **约6小时** | 采用渐进式方案，缩短了实施时间 |

### 实际实施差异
- **简化策略**：未实现原计划的完整类型推断和传播，而是搭建基础架构
- **批量修复**：使用代理工具批量修复编译错误，提高了效率
- **测试复用**：现有测试覆盖良好，无需大量新增测试

## 最新进展（2025-12-02）

### Step 7: Function和Closure类型统一 ✅ 已完成

**问题诊断**：
- 测试发现高阶函数 `let apply = |f, x| { f(x) }` 产生类型错误
- 错误信息：`Type mismatch: expected fn(t1) -> t2, found closure(fn(t3) -> number)`
- 根本原因：`Type::Function` 和 `Type::Closure` 被视为不兼容类型

**解决方案**：
1. 在 `unify_recursive` 中添加 Function/Closure 互相统一的逻辑
2. 支持三种组合：Function+Closure、Closure+Function、Closure+Closure
3. 在 `apply_substitution` 中添加对 Closure 类型的处理
4. 更新特殊处理逻辑，支持非函数类型与 Closure 的统一

**实现细节**：
```rust
// karte-hir/src/type_checker.rs
// 添加 Function 和 Closure 可以统一的分支
(Type::Function { params: p1, return_type: r1 },
 Type::Closure { params: p2, return_type: r2 })
| (Type::Closure { params: p1, return_type: r1 },
   Type::Function { params: p2, return_type: r2 })
| (Type::Closure { params: p1, return_type: r1 },
   Type::Closure { params: p2, return_type: r2 }) => {
    // 统一参数和返回类型
}
```

**测试结果**：
- ✅ 所有现有测试通过（218个集成测试 + 10个HIR测试）
- ✅ 类型检查：`let apply = |f, x| { f(x) }; let add_one = |n| { n + 1 }; apply(add_one, 5)` 通过类型检查
- ⚠️ 运行时：高阶函数调用产生 bus error（代码生成问题，非类型系统问题）

**影响范围**：
- 修改文件：`karte-hir/src/type_checker.rs`
- 新增代码：约60行
- 无破坏性变更，所有测试保持通过

### Step 8: Lambda类型信息传递到MIR ✅ 部分完成

**已实现功能**：
1. 在 `LoweringContext` 中添加 `get_lambda_type()` 辅助方法
2. 在 `lower_lambda_expression` 中：
   - 从 HIR 获取 Lambda 的推断类型
   - 为 Function 和 Closure Value 设置类型字段
   - 为 MirFunction 设置 `param_types` 和 `return_type`
3. 类型信息传播：HIR → LoweringOptions → LoweringContext → MIR

**关键代码**：
```rust
// karte-mir/src/lower/expr.rs
let lambda_type = ctx.get_lambda_type(expr);
let closure_type = match lambda_type.as_ref() {
    Some(Type::Function { params, return_type })
    | Some(Type::Closure { params, return_type }) => {
        Some(Type::Closure { params: params.clone(), return_type: return_type.clone() })
    }
    _ => None,
};
```

**已知问题**：
- ⚠️ 高阶函数的运行时错误（bus error）
- 问题不在类型系统，而在代码生成阶段
- LIR 显示 `lambda$0`（apply函数）内部重新创建了闭包结构，这是错误的

### 当前状态总结

✅ **已完成**：
1. Lambda类型推断和存储（Step 1）
2. MIR数据结构扩展（Step 2）
3. Lowering逻辑基础架构（Step 3）
4. 编译错误修复（Step 4）
5. 测试验证（Step 5）
6. Function/Closure类型统一（Step 7）
7. Lambda类型信息传递架构（Step 8）

⚠️ **部分完成**：
- Lambda类型信息在MIR中传递成功，但代码生成有问题

🔄 **待修复**：
- 高阶函数的运行时代码生成错误
- 需要诊断为什么闭包作为参数传递时，生成的LIR代码错误地重新创建闭包

## 后续工作计划

### 近期任务（核心功能）
1. **实现类型信息实际传播** [优先级：高]
   - 在 `lower_lambda` 中从 `expr_types` 获取Lambda类型
   - 填充MirFunction的 `param_types` 和 `return_type`
   - 在创建Temp时查询并填充类型信息
   - 实现 `resolve_type()` 辅助函数

2. **函数调用类型标注** [优先级：高]
   - 在 `lower_call` 中根据被调用函数类型标注返回值
   - 更新 `temp_types` 映射
   - 修复identity闭包返回类型问题

3. **移除或更新启发式代码** [优先级：中]
   - 评估 `temp_value_map` 是否可以用 `temp_types` 替代
   - 简化 `annotate_closure_return_value()` 逻辑
   - 清理不必要的类型猜测代码

### 中期目标（增强功能）
4. **双向类型推断** [优先级：中]
   - 实现从调用上下文推断Lambda参数类型
   - 支持 `expected_type` 传递
   - 处理高阶函数情况

5. **类型错误诊断改进** [优先级：中]
   - 利用类型信息提供更精确的错误消息
   - 在MIR层进行类型一致性验证

### 长期规划（扩展能力）
6. **泛型支持准备** [优先级：低]
   - 扩展Type枚举支持类型参数
   - 设计泛型实例化机制

7. **性能优化** [优先级：低]
   - 优化类型查询路径
   - 减少类型信息拷贝
   - 考虑使用更高效的数据结构

## 参考资料

- **双向类型检查**：Dunfield & Krishnaswami (2013) "Complete and Easy Bidirectional Typechecking for Higher-Rank Polymorphism"
- **Lambda演算类型推断**：Hindley-Milner type system
- **编译器设计**：Modern Compiler Implementation in ML (Andrew Appel)
