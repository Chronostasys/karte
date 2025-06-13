# Karte 语言 Sum Type 改进

## 改进概述

根据用户反馈，我们对Karte语言的sum type实现进行了两项重要改进：

1. **解决构造器重名问题**: 引入限定构造器语法 `TypeName::Constructor`
2. **支持嵌套sum type**: 允许sum type的变体包含其他sum type

## 问题一：构造器重名

### 问题描述
原来的实现中，sum type的构造器直接暴露在全局环境中，容易产生重名冲突：

```rust
// 问题示例 - 构造器重名
enum Color { Red, Green, Blue }
enum Status { Red, Active, Inactive }  // Red与Color::Red冲突
```

### 解决方案
引入限定构造器语法，必须通过类型名来访问构造器：

```rust
// 新语法 - 无重名冲突
enum Color { Red, Green, Blue }
enum Status { Red, Active, Inactive }

let color = Color::Red
let status = Status::Red  // 明确区分
```

## 问题二：嵌套sum type支持

### 问题描述
原来的实现只支持基本类型作为变体的数据类型，不能包含其他sum type。

### 解决方案
修改类型系统以支持完整的类型作为变体的数据类型：

```rust
// 嵌套sum type示例
enum Color { Red, Green, Blue }
enum Result { Ok(Color), Err }

let success = Result::Ok(Color::Green)
match success {
    Result::Ok(Color::Red) -> 1,
    Result::Ok(Color::Green) -> 2,
    Result::Ok(Color::Blue) -> 3,
    Result::Err -> 0
}
```

## 技术实现

### 1. AST修改
- 添加 `QualifiedConstructor` 表达式类型
- 添加 `QualifiedConstructor` 模式类型
- 支持 `TypeName::Constructor` 语法

### 2. 解析器修改
- 识别 `::` token作为限定符
- 解析限定构造器表达式和模式
- 向后兼容原有语法

### 3. 类型检查器修改
- 支持限定构造器的类型检查
- 修改 `SumVariant` 以支持完整的 `Type` 而不只是字符串
- 增强模式匹配的类型检查

### 4. 代码生成器修改
- 支持限定构造器的求值
- 支持限定构造器模式的匹配

## 使用示例

### 基本限定构造器
```rust
{
    enum Color { Red, Green, Blue };
    Color::Red
}
```

### 带参数的限定构造器
```rust
{
    enum Option { Some(number), None };
    Option::Some(42)
}
```

### 模式匹配
```rust
{
    enum Color { Red, Green, Blue };
    match Color::Red {
        Color::Red -> 1,
        Color::Green -> 2,
        Color::Blue -> 3
    }
}
```

### 嵌套sum type
```rust
{
    enum Color { Red, Green, Blue };
    enum Result { Ok(Color), Err };
    
    let result = Result::Ok(Color::Green);
    match result {
        Result::Ok(Color::Red) -> 1,
        Result::Ok(Color::Green) -> 2,
        Result::Ok(Color::Blue) -> 3,
        Result::Err -> 0
    }
}
```

## 向后兼容性

- 内置构造器 (`true`, `false`, `Some`, `None`) 保持原有语法
- 现有代码可以继续工作
- 新代码建议使用限定构造器语法以避免冲突

## 优势

1. **消除命名冲突**: 不同类型的构造器不会相互冲突
2. **提高代码可读性**: 明确表示构造器属于哪个类型  
3. **支持复杂数据结构**: 可以构建嵌套的sum type
4. **类型安全**: 编译时检查构造器的正确使用
5. **符合现代语言习惯**: 与Rust、Scala等语言的语法一致 