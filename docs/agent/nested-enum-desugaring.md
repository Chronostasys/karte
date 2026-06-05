# Nested Enum Pattern Desugaring

## Overview

`karte-parser/src/expression.rs` 中的 `desugar_nested_match_patterns` 函数负责将嵌套的 enum constructor pattern 展开为多层 match 表达式。这是 HIR 层的 desugar 方案，将语法糖转换为编译器可以处理的简单形式。

## 算法

对于形如 `Some(Some(x))` 的嵌套 pattern，desugar 过程如下：

```
// 输入
match expr {
    Some(Some(x)) => body1,
    Some(None) => body2,
    None => body3,
    _ => body4,
}

// 输出（简化）
match expr {
    Some(__karte__0) => match __karte__0 {
        Some(x) => body1,
        None => body2,
        _ => body4,       // ← wildcard 回退 arm
    },
    None => body3,
    _ => body4,
}
```

### 关键步骤

1. **分组**: 按外层 constructor key 将 arms 分组（`outer_constructor_key`）
2. **检测嵌套**: 找到第一个嵌套 arg 的索引（`find_first_nested_arg_index`）
3. **验证覆盖**: 检查同组内所有 arm 是否都有嵌套 constructor pattern（`all_covered`）
4. **生成临时变量**: 创建 `__karte__N` 变量绑定外层值
5. **构建内层 match**: 提取内层 pattern 作为新 match 的 arms
6. **递归 desugar**: 对 inner_arms 递归调用 `desugar_nested_match_patterns`
7. **添加 wildcard 回退**: 将原始 match 的非同组 wildcard arm 复制到内层 match

## 🐛 历史 Bug：三层嵌套导致 MIR lowering 错误

### 问题

在三层及以上嵌套（如 `Some(Some(Some(x)))`）时，两次连续的 `desugar_nested_match_patterns` 调用产生两个非穷尽的内层 match，导致 MIR lowering 阶段生成错误的 basic block 跳转代码。

### 根因

```rust
// 出错的守卫条件（已修复）
if indices.len() <= 1 {
    // 当 indices 只有一个元素时，跳过 desugar
    // 但这对嵌套 pattern 是错误的！
    for &idx in &indices {
        result.push(arms[idx].clone());
    }
    continue;
}
```

当外层 match 只有 1 个同 constructor 的 arm 时（如外层 `Some(...)` 只有一个 arm），原有的 `indices.len() <= 1` 守卫会直接跳过 desugar 并原样保留 arm。在三层嵌套场景下，这意味着内层 match 的 arm 未被展开，产生非穷尽 match。

### 修复三要素

1. **移除单 arm 守卫**（`expression.rs:2844`）：
   ```rust
   // 旧：if indices.len() <= 1 { ... continue; }
   // 新：if indices.len() == 0 { continue; }
   ```
   允许单 arm 递归，确保即使只有 1 个同 constructor arm，嵌套 pattern 仍能被展开。

2. **复制 wildcard 回退 arm**（`expression.rs:2899-2906`）：
   ```rust
   for j in 0..arms.len() {
       if !indices.contains(&j) {
           let fallback_arm = &arms[j];
           if matches!(&fallback_arm.pattern, Pattern::Wildcard { .. }) {
               inner_arms.push(fallback_arm.clone());
           }
       }
   }
   ```
   将原始外层 match 的 wildcard arm 复制到内层 match，使内层 match 变为穷尽。

3. **递归 desugar**（`expression.rs:2907`）：
   ```rust
   inner_arms = Self::desugar_nested_match_patterns(inner_arms);
   ```
   对构建的内层 arms 递归调用 desugar，处理更深层的嵌套。

### 案例

```karte
enum Triple { One, Two, Three }

fn main() -> number {
    let v = One;
    match v {
        Three => 3,
        Two => 2,
        One => 1,
    }
}
```

对于三层嵌套 `Outer(Middle(Inner(x)))`：
1. 第一轮 desugar：外层 `Outer(...)` → 内层 match on `__karte__0`
2. 内层 arms 递归 desugar：`Middle(Inner(x))` → 更深层 match on `__karte__1`
3. 每层内层 match 都有 wildcard 回退 arm，保证穷尽性

## 🐛 历史 Bug：Struct 字段嵌套构造器模式 MIR lowering 失败

### 问题

Parser desugar 将嵌套 enum pattern 展开为多层 match 后，内层 match arm 的 struct 字段中仍可能包含嵌套的构造器模式。例如：

```karte
enum A { A1(number) }
enum B { B1(A) }
struct S { field: B, tag: number }

fn main() -> number {
    let s = S { field: B::B1(A::A1(42)), tag: 10 };
    match s {
        S { field: B::B1(A::A1(n)), tag: t } => n + t,
        _ => 0,
    }
}
```

这里 struct 字段 `field` 的模式 `B::B1(A::A1(n))` 是嵌套构造器模式——外层 `B::B1(...)` 内含 `A::A1(n)`。

### 根因

`karte-mir/src/lower/helpers.rs` 的 `handle_pattern_bindings` 函数在处理 `Constructor` 和 `QualifiedConstructor` 的 args 时，对子模式只处理了 `Variable`、`Wildcard` 两种，遇到嵌套的 `Constructor`/`QualifiedConstructor`/`Struct` 模式时直接返回错误：

```rust
// 修复前：不支持嵌套构造器模式
_ => {
    return Err(vec![
        format!("Unsupported nested pattern in constructor argument at position {}", i)
    ]);
}
```

### 修复 (commit 2184885)

在 `Constructor` 和 `QualifiedConstructor` 两处，为子模式是 `Constructor`/`QualifiedConstructor`/`Struct` 的情况添加递归处理：

```rust
karte_hir::Pattern::Constructor { .. }
| karte_hir::Pattern::QualifiedConstructor { .. }
| karte_hir::Pattern::Struct { .. } => {
    let arg_temp = ctx.new_temp();
    ctx.add_statement(Statement::ConstructorArgExtract {
        target: arg_temp.clone(),
        constructor: match_value.clone(),
        arg_index: i,
        span: Span::new(0, 0),
    });
    let resolved = ctx.resolve_value(&arg_temp);
    handle_pattern_bindings(ctx, arg_pattern, &resolved)?;
}
```

核心逻辑：对嵌套构造器模式的每个子模式，先用 `ConstructorArgExtract` 提取对应位置的字段值到临时变量，然后递归调用 `handle_pattern_bindings` 处理该子模式——与 Struct 字段的处理方式一致（见 `helpers.rs:665-667`）。

### 与 Parser Desugar 的关系

Parser desugar 将嵌套 enum 展开为多层 match，每层 match arm 可能是 struct 模式（如果被匹配的值是 struct）。Struct 字段中的嵌套构造器模式不会被 parser desugar 消除——这需要 MIR lowering 层来处理。

```
Parser desugar:  match Outer { Middle(Inner(x)) => ... }
  →  match Outer {
       Middle(__karte__0) => match __karte__0 {
         Inner(x) => ...,    ← 构造器子模式，parser 不展开
         _ => ...
       }
     }

MIR lowering:    handle_pattern_bindings 处理 Struct 字段中的 Constructor 子模式
  →  递归调用自身处理嵌套的 Constructor/QualifiedConstructor/Struct
```

两层缺一不可：parser 层负责展开多层嵌套为多层 match，MIR 层负责递归处理 struct 字段中的嵌套构造器模式。

## 相关文件

| 文件 | 关键行 | 作用 |
|------|--------|------|
| `karte-parser/src/expression.rs:2792` | `desugar_nested_match_patterns` | Parser 层主入口函数 |
| `karte-parser/src/expression.rs:2844` | `indices.len() == 0` | 单 arm 递归守卫 |
| `karte-parser/src/expression.rs:2899-2906` | wildcard 复制 | 回退 arm 注入 |
| `karte-parser/src/expression.rs:2907` | 递归调用 | 深层嵌套处理 |
| `karte-mir/src/lower/helpers.rs:569` | `handle_pattern_bindings` | MIR 层模式绑定处理 |
| `karte-mir/src/lower/helpers.rs:596-609` | Constructor 递归 | 构造器子模式递归处理 |
| `karte-mir/src/lower/helpers.rs:635-648` | QualifiedConstructor 递归 | 限定名构造器子模式递归处理 |
