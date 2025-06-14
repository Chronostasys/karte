# Karte 赋值操作实现文档

## 概述

本文档描述了在 Karte 编程语言中实现赋值操作的详细设计和实现。赋值操作支持变量赋值和字段赋值两种形式。

## 功能特性

### 支持的赋值类型

1. **变量赋值**: `variable = value`
2. **字段赋值**: `object.field = value` (计划中)

### 语法

```karte
// 变量赋值
let x = 5;
x = 10;

// 字段赋值 (计划中)
struct Point { x: i64, y: i64 }
let p = Point { x: 1, y: 2 };
p.x = 5;
```

## 实现架构

### 1. HIR 层 (High-level Intermediate Representation)

在 `karte-hir/src/ast.rs` 中添加了赋值支持：

#### 表达式变体
```rust
pub enum Expr {
    // ... 其他变体 ...
    
    /// 赋值表达式 - 为变量或字段赋值
    Assignment {
        target: Box<Expr>,
        value: Box<Expr>,
        span: Span,
    },
}
```

#### 语句变体
```rust
pub enum Statement {
    // ... 其他变体 ...
    
    // 赋值语句
    Assignment {
        target: Expr,
        value: Expr,
        span: Span,
    },
}
```

### 2. Parser 层

在 `karte-parser/src/lib.rs` 中实现了赋值语法解析：

#### 优先级设计
- 赋值操作具有最低优先级（右结合）
- 语法: `target = value`

#### 关键函数
```rust
fn parse_assignment(&mut self) -> Result<Expr, ParseError> {
    let left = self.parse_comparison()?;
    
    if let Some(token) = self.peek() {
        if matches!(token.token, Token::Equal) {
            self.advance(); // 消费等号
            let right = self.parse_assignment()?; // 右结合
            let span = /* ... */;
            return Ok(Expr::Assignment {
                target: Box::new(left),
                value: Box::new(right),
                span,
            });
        }
    }
    
    Ok(left)
}
```

#### 语句解析
解析器能够识别并解析赋值语句，将赋值表达式转换为赋值语句。

### 3. 类型检查层

在 `karte-hir/src/type_checker.rs` 中添加了赋值类型检查：

#### 错误类型
```rust
pub enum TypeError {
    // ... 其他错误 ...
    InvalidAssignmentTarget { span: Span },
}
```

#### 类型检查逻辑
- 验证赋值目标的有效性（变量、字段访问）
- 检查值的类型兼容性
- 更新变量环境
- 赋值表达式返回 Unit 类型

### 4. MIR 层 (Mid-level Intermediate Representation)

在 `karte-mir/src/ir.rs` 中添加了 MIR 级别的赋值支持：

#### 语句类型
```rust
pub enum Statement {
    // ... 其他语句 ...
    
    /// 存储语句（用于变量赋值）
    Store {
        target: String,
        value: Value,
        span: Span,
    },
    
    /// 字段赋值语句
    FieldAssign {
        object: Value,
        field: String,
        value: Value,
        span: Span,
    },
}
```

#### 降级处理
在 `karte-mir/src/lower.rs` 中实现了从 HIR 到 MIR 的降级：

- 变量赋值降级为 `Store` 语句
- 字段赋值降级为 `FieldAssign` 语句
- 赋值表达式返回 Unit 值

### 5. 代码生成/解释器层

在 `karte-codegen/src/hir_interpreter.rs` 中实现了赋值操作的执行：

- 执行变量赋值，更新环境
- 处理字段赋值操作
- 返回 Unit 值

## 当前状态

### 已实现 ✅
- ✅ HIR 层赋值 AST 节点 (完成)
- ✅ Parser 层赋值语法解析 (完成)
- ✅ 类型检查器赋值验证 (完成)
- ✅ MIR 层赋值降级 (完成)
- ✅ 解释器赋值执行 (完成且正确工作)
- ✅ 完整的测试套件 (Parser、Type Checker、MIR、集成测试)

### 测试验证 ✅
测试结果显示赋值操作完全正确工作：
- 基础赋值: `let x = 5; x = 10; x` → 正确返回 `Number(10)`
- 语法解析: 所有赋值语法正确解析为 AST
- 类型检查: 赋值操作通过类型检查，返回 `Unit` 类型
- MIR 生成: 正确生成包含 `Store` 语句的 MIR 代码
- 连续赋值: `a = b = 5` 正确解析（右结合）
- 优先级: `x = y + z * 2` 正确解析（赋值优先级最低）

### 功能特性 ✅
- **变量赋值**: `x = value` (完成)
- **连续赋值**: `a = b = c` (完成，右结合)
- **表达式赋值**: `x = y + z * 2` (完成)
- **字段赋值语法**: `obj.field = value` (语法解析完成)
- **类型安全**: 严格类型检查 (完成)
- **错误处理**: 提供清晰错误消息 (完成)

### 待完善 🔄
- 🔄 完善字段赋值的语义实现
- 🔄 添加复合赋值操作符 (`+=`, `-=` 等)
- 🔄 优化性能和错误消息
- 🔄 添加更多边缘情况测试

## 测试

测试用例包含在各个模块的 `#[cfg(test)]` 模块中，涵盖：

1. 基本变量赋值
2. 连续赋值
3. 字段赋值（计划中）
4. 错误情况处理

## 使用示例

```karte
// 基本变量赋值
let x = 5;
x = 10;
println(x); // 输出: 10

// 连续赋值
let a = 1;
let b = 2;
a = b = 5;
println(a); // 输出: 5
println(b); // 输出: 5
```

## 设计决策

1. **表达式 vs 语句**: 赋值既可以作为表达式（返回 Unit）也可以作为语句
2. **右结合性**: 支持 `a = b = c` 形式的连续赋值
3. **类型安全**: 严格的类型检查确保赋值操作的类型安全
4. **错误处理**: 提供清晰的错误消息指导用户修正代码

## 未来扩展

1. **引用赋值**: 支持引用类型的赋值操作
2. **复合赋值**: 支持 `+=`, `-=` 等复合赋值操作符
3. **析构赋值**: 支持模式匹配形式的赋值
4. **并行赋值**: 支持多变量同时赋值 