# Karte Trait 系统

> 这是给我自己的语言。设计目标：能写出 `fn sort<T: Ord>(arr)` 然后它真的能跑。

---

## 一、我为什么需要 Trait

现在的 Karte 标准库有几个问题：

**1. 每个类型要写一套 `eq`/`compare`/`to_string`**
```
std.string 里:  fn eq(a: string, b: string) -> bool { ... }
std.core 里:    fn abs(x: number) -> number { ... }
std.result 里:  fn unwrap(r: Result<number, string>) -> number { ... }
```
`unwrap` 只能处理 `Result<number, string>`，不是泛型的。

**2. 没法写泛型数据结构**
`std.hashmap` 只支持 `string → string`。想要 `number → string` 就要重写一遍。

**3. 没法写泛型算法**
`std.core` 的 `min`/`max`/`clamp` 只能处理 `number`。想要字符串的 `max`？自己写。

Trait 解决这些问题。写一次 `sort`，`number`/`string`/自定义类型都能用。

---

## 二、语法

### Trait 定义

```karte
trait Eq {
    fn eq(a: Self, b: Self) -> bool;
}
```

- `Self` = impl 目标类型的占位符
- 方法只有签名，以 `;` 结尾，无方法体
- 纯函数签名，不含 `self`/`this`：`eq(a, b)` 不是 `a.eq(b)`

### 带泛型参数的 Trait

```karte
trait From<T> {
    fn from(val: T) -> Self;
}

trait Convert<T, U> {
    fn convert(val: T) -> U;
}
```

### Impl 块

```karte
impl Eq for number {
    fn eq(a, b) { a == b }
}

impl Eq for bool {
    fn eq(a, b) { if a { if b { true } else { false } } else { if b { false } else { true } } }
}

impl From<number> for bool {
    fn from(val) { val != 0 }
}
```

**关键设计：impl 方法参数类型可以省略。** 编译器从 trait 定义推断：
- `trait Eq` 的方法 `eq(a: Self, b: Self) -> bool`
- `impl Eq for number` → `Self = number`
- 所以 `fn eq(a, b) { ... }` 里 `a: number, b: number, 返回 bool`

也允许写全（冗余但不报错）：
```karte
impl Eq for number {
    fn eq(a: number, b: number) -> bool { a == b }
}
```

### 泛型约束

```karte
fn find<T: Eq>(arr: &[T], val: T) -> bool {
    let i = 0;
    while i < len(arr) {
        if eq(index(arr, i), val) { return true };
        i = i + 1
    };
    false
}

fn max<T: Ord>(a: T, b: T) -> T {
    if compare(a, b) >= 0 { a } else { b }
}
```

- `<T: TraitName>` 声明泛型参数的约束
- 函数体内可以直接调用 trait 方法（如 `eq(x, y)`）
- 编译器根据实际类型查找对应的 impl

### 在非泛型代码中直接调用

```karte
fn main() -> bool {
    eq(1, 2)        // 编译器推断 1,2 是 number，找 impl Eq for number
}
```

当参数类型都是具体的（不涉及类型变量），直接解析到对应 impl。

---

## 三、解析规则

### 3.1 Trait 定义解析

```
"trait" <Identifier> ("<" <Identifier> ("," <Identifier>)* ">")? "{"
    ("fn" <Identifier> "(" <typed_params> ")" ("->" <type>)? ";")*
"}"
```

- `<T>` 是 trait 自己的泛型参数，如 `From<T>` 的 `T`
- 方法签名中可出现 `Self`（映射为 `Type::SelfType`）和 trait 泛型参数名
- trait 泛型参数在方法签名中等同于普通类型使用

### 3.2 Impl 块解析

```
"impl" <Identifier> ("<" <type> ("," <type>)* ">")? "for" <type> "{"
    ("fn" <Identifier> "(" <params> ")" ("->" <type>)? "{" <expr> "}")*
"}"
```

- `<type>` 是 trait 泛型参数的实例化（如 `From<number>` 的 `number`）
- `for <type>` 是目标类型（如 `number`、`bool`、`Point`）
- 方法参数类型**可以省略**，编译器从 trait 定义推断
- 方法返回类型**可以省略**，编译器从方法体推断

### 3.3 泛型约束解析

```
"fn" <Identifier> "<" <Identifier> ":" <Identifier> ">"
    "(" <typed_params> ")" ("->" <type>)? "{" <expr> "}"
```

- `<T: Eq>` 声明类型参数 T 必须实现 Eq trait
- 多个约束 `<T: Eq + Ord>` — Phase 2
- 约束里的 trait 名只能是当前作用域中已定义的 trait

### 3.4 Self 类型

Parser 的 `type_name_to_type()` 添加映射：
```
"Self" => Type::SelfType
```

`Type::SelfType` 仅出现在 trait 定义的方法签名中。在其他位置出现是类型错误。

---

## 四、语义

### 4.1 Impl 验证

注册 impl 时做以下检查：

1. **trait 存在性** — `impl Foo for ...` 中 `Foo` 必须是已定义的 trait
2. **方法完整性** — impl 必须实现 trait 声明的所有方法（不能多也不能少）
3. **签名匹配** — 将 trait 方法签名中的 `Self` 替换为 target_type、将 trait 泛型参数替换为 trait_args，然后与 impl 方法签名做 structural_eq
4. **方法体类型检查** — 用推断出的参数类型构造环境，推断方法体类型，验证与声明返回类型一致
5. **Coherence** — 同一 (trait, target_type) 只能有一个 impl

### 4.2 泛型函数的类型推断

当函数声明 `<T: Eq>` 时：

```
1. T 绑定到一个新的 TypeVar（如 t42）
2. 记录约束：t42 必须实现 Eq
3. 函数体内调用 eq(x, y) 时：
   a. 推断 x 和 y 的类型都是 T（即 t42）
   b. 查找当前约束中是否有 trait 含方法 eq
   c. 找到 Eq.eq(Self, Self) -> bool
   d. 用 t42 替换 Self → eq(t42, t42) -> bool
   e. 添加约束：x 的类型 == t42, y 的类型 == t42
   f. 返回类型 bool
4. generalize 时将约束打包进 TypeScheme
```

### 4.3 调用点检查

调用 `find([1,2,3], 2)` 时：

```
1. 推断参数类型：[1,2,3] → number[], 2 → number
2. 实例化 find 的 TypeScheme：
   - T' = fresh TypeVar（如 t87）
   - 约束：t87 : Eq
   - 函数类型：fn(&[t87], t87) -> bool
3. 统一参数类型：t87 = number
4. 检查约束：number 是否有 impl Eq？
   - 查 impl_registry["Eq"] → 找到 (number, ...)
   - ✅ 通过
5. 如果没有对应 impl → 编译错误 "number does not implement trait Eq"
```

### 4.4 方法解析优先级

当遇到 `eq(x, y)` 调用时：

```
1. 普通函数  — function_signatures 中有 "eq" → 走普通调用路径
2. Trait 方法 — 当前环境有 trait bounds 涉及 "eq" → trait 解析路径
3. 模块函数  — apply_module_context 注入的函数 → 已在步骤 1 覆盖
```

同名时普通函数优先。这意味着如果你定义了 `fn eq(a, b)`，它会遮盖 `trait Eq` 的 `eq`。

### 4.5 Impl 方法如何编译

impl 中的方法体会被编译为独立函数，名称 mangle 为 `TraitName$TargetType$methodName`。

```karte
impl Eq for number {
    fn eq(a, b) { a == b }
}
```

等价于生成了一个函数 `Eq$number$eq(a: number, b: number) -> bool`。

在 monomorphization 阶段，trait 方法调用被替换为对应的 mangle 函数调用。

---

## 五、Monomorphization

泛型函数不能直接编译——编译器不知道 `T` 的大小和操作。Monomorphization 在类型检查之后、MIR lowering 之前执行：

```
源码
 ↓ 类型检查（含 trait 约束验证）
 ↓ monomorphization pass ← 新增
 ↓ MIR lowering
 ↓ LIR
 ↓ codegen
```

### 算法

```
输入: 类型检查后的 AST + trait/impl 注册表
输出: 无泛型的特化 AST

1. 扫描所有泛型函数的调用点
   对每个调用 find([1,2,3], 2)：
   - 推断出 T = number
   
2. 为每个 (函数名, 具体类型组合) 生成特化版本
   - find$number(arr: &[number], val: number) -> bool { ... }
   
3. 在特化函数体内替换：
   - 类型变量 → 具体类型
   - trait 方法调用 → 具体 impl 函数调用
     eq(arr[0], val) → Eq$number$eq(arr[0], val)

4. 替换调用点：
   find([1,2,3], 2) → find$number([1,2,3], 2)
```

---

## 六、数据结构

### 6.1 Type 枚举扩展（karte-hir/src/types.rs）

```rust
pub enum Type {
    // ... 现有变体 ...
    SelfType,   // 仅在 trait 定义中出现
}
```

已实现 ✅（structural_eq/substitute/free_vars/contains_var/Display/size_of 全部适配）

### 6.2 Trait 相关结构体（karte-hir/src/types.rs）

```rust
pub struct TraitMethodDef {
    pub name: String,
    pub params: Vec<(String, Type)>,   // 可含 SelfType
    pub return_type: Type,              // 可含 SelfType
}

pub struct TraitDef {
    pub name: String,
    pub type_params: Vec<String>,       // ["T"] for From<T>
    pub methods: Vec<TraitMethodDef>,
}

pub struct ImplMethodDef {
    pub name: String,
    pub params: Vec<(String, Type)>,   // SelfType 已替换
    pub return_type: Type,
    pub body: crate::ast::Expr,
}

pub struct ImplBlock {
    pub trait_name: String,
    pub trait_args: Vec<Type>,          // [number] for From<number>
    pub target_type: Type,
    pub methods: Vec<ImplMethodDef>,
}
```

已实现 ✅

### 6.3 ParseResult 扩展（karte-parser/src/lib.rs）

```rust
pub struct ParseResult {
    // ... 现有字段 ...
    pub trait_defs: Vec<TraitDef>,      // 新增
    pub impl_blocks: Vec<ImplBlock>,    // 新增
}
```

### 6.4 TypeChecker 扩展（karte-hir/src/type_checker.rs）

```rust
pub struct TypeChecker {
    // ... 现有字段 ...

    // 新增
    trait_defs: HashMap<String, TraitDef>,
    impl_registry: HashMap<String, Vec<ImplEntry>>,
}

struct ImplEntry {
    target_type: Type,
    trait_args: Vec<Type>,
    methods: Vec<ImplMethodDef>,
}
```

### 6.5 TypeScheme 扩展（Phase 2）

```rust
pub struct TypeScheme {
    pub bound_vars: Vec<TypeVar>,
    pub body: Type,
    // Phase 2 新增
    // pub constraints: Vec<TraitConstraint>,
}

// Phase 2 新增
// pub struct TraitConstraint {
//     pub type_var_id: u32,
//     pub trait_name: String,
// }
```

---

## 七、实施计划

### Phase 1：Parse + Register + Validate（~250 行，5 文件）

**目标**：能定义 trait、写 impl，编译器验证正确性。不能运行泛型代码。

| 步骤 | 文件 | 内容 | 行数 |
|------|------|------|------|
| 1 | `karte-parser/src/lib.rs` | ParseResult 添加 trait_defs/impl_blocks；Parser 添加对应字段 | 8 |
| 2 | `karte-parser/src/statement.rs` | parse_trait_definition()、parse_impl_block()、Self 映射 | 120 |
| 3 | `karte-hir/src/type_checker.rs` | trait_defs/impl_registry + register + validate | 80 |
| 4 | 调用点接入 | CLI/module-system 调用 register_traits_and_impls | 10 |
| 5 | `karte-tests/` | 测试用例 | 80 |

**验收标准**：
```karte
// ✅ 编译通过
trait Eq { fn eq(a: Self, b: Self) -> bool; }
impl Eq for number { fn eq(a, b) { a == b } }
fn main() -> number { 0 }

// ❌ 编译错误：缺少方法
trait Eq { fn eq(a: Self, b: Self) -> bool; fn neq(a: Self, b: Self) -> bool; }
impl Eq for number { fn eq(a, b) { a == b } }

// ❌ 编译错误：重复 impl
impl Eq for number { fn eq(a, b) { a == b } }
impl Eq for number { fn eq(a, b) { a != b } }

// ❌ 编译错误：未定义 trait
impl Ord for number { fn compare(a, b) { 0 } }
```

### Phase 2：约束 + 类型推断 + 调用点检查（~150 行，3 文件）

**目标**：`<T: Eq>` 编译通过，调用点验证 impl 存在。仍不能运行。

| 步骤 | 文件 | 内容 | 行数 |
|------|------|------|------|
| 6 | `karte-hir/src/types.rs` | TypeScheme.constraints、TraitConstraint | 10 |
| 7 | `karte-parser/src/statement.rs` | 解析 `<T: Eq>` 约束语法 | 40 |
| 8 | `karte-hir/src/type_checker.rs` | generalize 收集约束、instantiate 传播约束、调用点检查 | 100 |

**验收标准**：
```karte
// ✅ 编译通过
fn same<T: Eq>(a: T, b: T) -> bool { eq(a, b) }
fn main() -> number { 0 }

// ❌ 编译错误：number does not implement Ord
fn needs_ord<T: Ord>(x: T) -> T { x }
fn main() -> number { needs_ord(42) }
```

### Phase 3：Monomorphization（~200 行，2 文件）

**目标**：泛型函数生成特化版本，trait 方法调用替换为具体 impl。代码能跑。

| 步骤 | 文件 | 内容 | 行数 |
|------|------|------|------|
| 9 | `karte-module-system/src/project.rs` | monomorphization pass | 150 |
| 10 | `karte-hir/src/type_checker.rs` | impl 方法体编译支持 | 50 |

**验收标准**：
```karte
trait Eq { fn eq(a: Self, b: Self) -> bool; }
impl Eq for number { fn eq(a, b) { a == b } }

fn same<T: Eq>(a: T, b: T) -> bool { eq(a, b) }
fn main() -> number {
    let r1 = same(1, 2);    // false → 0
    let r2 = same(3, 3);    // true  → 1
    if r1 { 0 } else { if r2 { 1 } else { 0 } }
}
// 返回 1，exit code 1
```

---

## 八、设计决策

| 决策 | 选择 | 不选 | 理由 |
|------|------|------|------|
| 实例化方式 | Self 占位符 | 显式泛型 `<T>` | `fn eq(a: Self, b: Self)` 比 `fn eq<T>(a: T, b: T) where Self=T` 简洁 |
| 调用风格 | `eq(a, b)` | `a.eq(b)` | Karte 是函数式语言，没有隐式 this |
| 分发方式 | Monomorphization | vtable/动态分发 | 零开销，适合系统语言 |
| impl 类型推断 | 可以省略参数类型 | 必须写全 | 减少重复，从 trait 定义推断即可 |
| 数据通道 | 独立于 Statement | 加入 Statement 枚举 | 避免改动 MIR/LIR 的 13+ 文件 |
| 约束位置 | inline `<T: Eq>` | where 子句 | Karte 是小语言，不需要 where |
| bounds 存储 | TypeScheme.constraints | TypeVar.bounds | 不破坏 TypeVar 的 Copy 语义 |

---

## 九、文件影响

```
Phase 1: 5 文件改动，~250 行。MIR/LIR/Codegen/Lexer 不动。
Phase 2: 3 文件改动，~150 行。
Phase 3: 2 文件改动，~200 行。
─────────────────────────────────────
总计:     7 文件，~600 行。
```

> **完成标志**：Phase 3 验收测试通过 + 879/879 现有测试无回归。
