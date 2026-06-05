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

## 相关文件

| 文件 | 关键行 | 作用 |
|------|--------|------|
| `karte-parser/src/expression.rs:2792` | `desugar_nested_match_patterns` | 主入口函数 |
| `karte-parser/src/expression.rs:2844` | `indices.len() == 0` | 单 arm 递归守卫 |
| `karte-parser/src/expression.rs:2899-2906` | wildcard 复制 | 回退 arm 注入 |
| `karte-parser/src/expression.rs:2907` | 递归调用 | 深层嵌套处理 |
