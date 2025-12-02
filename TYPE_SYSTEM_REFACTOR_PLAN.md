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

## 实施计划

### Step 1: HIR类型信息增强 (2-3小时)
**文件修改**：
- `karte-hir/src/lib.rs`：扩展Expr::Lambda结构
- `karte-hir/src/type_checker.rs`：实现完整的Lambda类型推断

**任务清单**：
- [ ] 为Lambda表达式添加 `inferred_type` 字段
- [ ] 实现 `infer_lambda_type()` 函数
- [ ] 处理identity函数的类型推断
- [ ] 处理高阶函数的类型推断
- [ ] 添加类型推断的单元测试

### Step 2: MIR数据结构扩展 (1-2小时)
**文件修改**：
- `karte-mir/src/ir.rs`：扩展Value和MirFunction
- `karte-mir/src/lower/types.rs`：扩展LoweringContext

**任务清单**：
- [ ] 为Value的所有variants添加 `ty: Option<Type>` 字段
- [ ] 为MirFunction添加参数类型和返回类型字段
- [ ] 在LoweringContext中添加type_env和temp_types
- [ ] 实现类型查询辅助方法

### Step 3: Lowering逻辑更新 (3-4小时)
**文件修改**：
- `karte-mir/src/lower/expr.rs`：更新所有表达式lowering
- `karte-mir/src/lower/stmt.rs`：更新所有语句lowering
- `karte-mir/src/lower/helpers.rs`：添加类型辅助函数

**任务清单**：
- [ ] 更新 `lower_lambda`：从HIR提取并保存类型信息
- [ ] 更新 `lower_call`：根据函数类型标注返回值
- [ ] 更新 `lower_identifier`：从type_env查询类型
- [ ] 更新 `lower_function_def`：保存完整函数签名
- [ ] 更新所有创建Temp的地方，添加类型标注
- [ ] 实现 `resolve_type(value: &Value) -> Option<Type>`
- [ ] 实现 `annotate_temp_type(temp_id, type)`

### Step 4: 移除Hack代码 (30分钟)
**文件修改**：
- `karte-mir/src/lower/expr.rs`：移除启发式方案
- `karte-mir/src/lower/context.rs`：清理temp_value_map相关代码

**任务清单**：
- [ ] 删除 `annotate_closure_return_value()` 函数
- [ ] 删除启发式类型猜测逻辑
- [ ] 简化 `resolve_value()` 实现
- [ ] 可能完全移除 `temp_value_map`（用temp_types替代）

### Step 5: 测试和验证 (2小时)
**任务清单**：
- [ ] 验证现有测试仍然通过
- [ ] 添加identity闭包测试（简单情况）
- [ ] 添加高阶函数测试（`|f, x| { f(x) }`）
- [ ] 添加嵌套Lambda测试
- [ ] 添加函数返回函数的测试
- [ ] 性能测试（确保类型标注不影响性能）

### Step 6: 文档更新 (1小时)
**任务清单**：
- [ ] 更新TYPE_SYSTEM_IMPROVEMENTS.md
- [ ] 添加类型推断算法文档
- [ ] 更新CLAUDE.md中的架构说明
- [ ] 添加代码注释

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

## 预期效果

### 功能改进
1. ✅ **Identity闭包**：`|x| {x}` 完全正确处理
2. ✅ **高阶函数**：`|f, x| { f(x) }` 正确处理
3. ✅ **函数返回函数**：所有情况都正确
4. ✅ **嵌套Lambda**：多层嵌套正确处理
5. ✅ **类型安全**：编译期检查更严格

### 代码质量
1. ✅ **系统性**：不再是hack，而是系统的类型传递
2. ✅ **可维护性**：类型信息明确，易于理解和修改
3. ✅ **可扩展性**：为未来的类型系统增强打下基础
4. ✅ **专业性**：符合编译器设计的最佳实践

### 性能影响
- 类型信息主要在编译期使用，运行时开销极小
- 可能略微增加编译时间（类型推断和传播）
- 但会减少运行时错误，提高代码生成质量

## 风险评估

### 主要风险
1. **破坏性变更**：修改核心数据结构，可能影响大量代码
2. **类型推断复杂度**：某些情况可能无法推断（需要fallback策略）
3. **测试覆盖**：需要大量测试确保正确性

### 缓解策略
1. **增量实施**：分阶段进行，每个阶段都确保测试通过
2. **保留后备方案**：类型推断失败时使用Unknown类型
3. **充分测试**：每个阶段都添加全面的测试

## 时间估算

- **Step 1 (HIR增强)**：2-3小时
- **Step 2 (MIR扩展)**：1-2小时
- **Step 3 (Lowering更新)**：3-4小时
- **Step 4 (清理hack)**：30分钟
- **Step 5 (测试)**：2小时
- **Step 6 (文档)**：1小时

**总计**：约10-13小时

## 后续优化

完成基础重构后，可以考虑：
1. **类型推断优化**：更智能的推断算法
2. **泛型支持**：为未来的泛型系统做准备
3. **类型错误诊断**：更好的错误信息
4. **性能优化**：减少类型查询开销

## 参考资料

- **双向类型检查**：Dunfield & Krishnaswami (2013) "Complete and Easy Bidirectional Typechecking for Higher-Rank Polymorphism"
- **Lambda演算类型推断**：Hindley-Milner type system
- **编译器设计**：Modern Compiler Implementation in ML (Andrew Appel)
