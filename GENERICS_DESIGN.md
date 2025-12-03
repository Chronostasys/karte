# Karte 泛型系统设计文档

## 目标

为 Karte 语言添加泛型（Generics）和参数化多态（Parametric Polymorphism）支持，使代码更加可复用和类型安全。

## 1. 语法设计

### 1.1 泛型函数

```karte
// 基本泛型函数
fn identity<T>(x: T) -> T {
    x
}

// 多个类型参数
fn pair<A, B>(first: A, second: B) -> (A, B) {
    (first, second)
}

// 使用示例
let x = identity(42);          // T = number
let y = identity("hello");     // T = string
let p = pair(1, "one");        // A = number, B = string
```

### 1.2 泛型结构体

```karte
// 泛型结构体定义
struct Box<T> {
    value: T
}

// 泛型方法（如果支持方法）
impl<T> Box<T> {
    fn new(value: T) -> Box<T> {
        Box { value }
    }

    fn get(&self) -> &T {
        &self.value
    }
}

// 使用示例
let int_box = Box::new(42);
let str_box = Box { value: "hello" };
```

### 1.3 泛型枚举

```karte
// Option 类型（已内置，但可以这样定义）
enum Option<T> {
    Some(T),
    None
}

// Result 类型
enum Result<T, E> {
    Ok(T),
    Err(E)
}

// 使用示例
fn divide(a: number, b: number) -> Result<number, string> {
    if b == 0 {
        Result::Err("division by zero")
    } else {
        Result::Ok(a / b)
    }
}
```

### 1.4 高阶函数与泛型

```karte
// map 函数
fn map<A, B>(f: A -> B, list: List<A>) -> List<B> {
    // 实现
}

// filter 函数
fn filter<T>(pred: T -> bool, list: List<T>) -> List<T> {
    // 实现
}

// fold 函数
fn fold<A, B>(f: (B, A) -> B, init: B, list: List<A>) -> B {
    // 实现
}
```

## 2. 类型推断与泛型

### 2.1 类型推断策略

Karte 已有基于 Hindley-Milner 的类型推断系统，泛型应该与之无缝集成：

```karte
// 不需要显式类型标注
let id = |x| { x };           // 推断为 <T>(T) -> T
let result = id(42);          // 推断为 number
let result2 = id("hello");    // 推断为 string

// 部分类型标注
fn process<T>(x: T) -> T {
    let temp = x;             // temp 类型推断为 T
    temp
}
```

### 2.2 类型参数约束（暂缓，Phase 2）

```karte
// 未来可能的语法
trait Display {
    fn to_string(&self) -> string;
}

fn print<T: Display>(value: T) -> () {
    println(value.to_string());
}
```

## 3. 实现策略

### 3.1 选项对比

#### 选项 A: Monomorphization（单态化）- 推荐 ✅

**描述**：编译时为每个类型参数的具体使用生成特化版本。

**优点**：
- 性能最优，无运行时开销
- 类型安全在编译时完全检查
- 与现有 JIT 架构兼容
- 实现相对简单直接

**缺点**：
- 代码膨胀（每个类型组合生成一份代码）
- 编译时间可能增加
- 二进制大小增加

**实现路径**：
1. HIR 阶段保留泛型信息
2. HIR->MIR lowering 时进行 monomorphization
3. 为每个泛型函数/结构体的具体使用生成特化版本
4. MIR/LIR 中不再包含泛型信息

**示例**：
```karte
fn identity<T>(x: T) -> T { x }

let a = identity(42);
let b = identity("hi");

// 生成：
fn identity$number(x: number) -> number { x }
fn identity$string(x: string) -> string { x }
```

#### 选项 B: Type Erasure（类型擦除）

**描述**：泛型类型在编译后被擦除，运行时使用统一表示。

**优点**：
- 代码大小小
- 编译快

**缺点**：
- 性能开销（需要装箱/拆箱）
- 实现复杂（需要统一值表示）
- 某些优化困难

**不推荐原因**：与 Karte 的 JIT 编译和性能目标不符。

#### 选项 C: 混合策略

**描述**：对常用类型进行 monomorphization，其他使用泛型表示。

**评估**：可作为未来优化方向，但第一版不推荐。

### 3.2 推荐实现策略：Monomorphization

## 4. 实现计划

### Phase 1: 基础泛型函数（MVP）

**目标**：支持泛型函数的基本功能

**任务**：
1. **语法扩展**
   - [ ] 扩展 Parser 支持 `<T>` 类型参数语法
   - [ ] 支持函数签名中的类型参数
   - [ ] 解析泛型函数调用

2. **类型系统扩展**
   - [ ] 在 HIR 的 `Type` 枚举中添加泛型类型表示
   - [ ] 扩展类型推断系统处理类型参数
   - [ ] 实现类型参数的统一化（unification）

3. **Monomorphization 实现**
   - [ ] 在 HIR->MIR lowering 时收集所有泛型函数的实例化
   - [ ] 为每个实例化生成特化函数
   - [ ] 替换泛型函数调用为特化版本调用

4. **测试**
   - [ ] 基础泛型函数测试（identity, swap, etc）
   - [ ] 类型推断测试
   - [ ] 高阶泛型函数测试

**预期时间**：2-3 周

### Phase 2: 泛型数据结构

**目标**：支持泛型结构体和枚举

**任务**：
1. **语法扩展**
   - [ ] 支持 `struct Box<T> { ... }` 语法
   - [ ] 支持 `enum Option<T> { ... }` 语法
   - [ ] 支持泛型类型的构造和模式匹配

2. **类型系统扩展**
   - [ ] 泛型结构体的类型检查
   - [ ] 泛型枚举的类型检查
   - [ ] 构造函数的类型推断

3. **Monomorphization 扩展**
   - [ ] 为泛型结构体生成特化版本
   - [ ] 处理泛型结构体的内存布局
   - [ ] 为泛型枚举生成特化版本

4. **测试**
   - [ ] 泛型结构体测试
   - [ ] 泛型枚举测试（Option, Result）
   - [ ] 嵌套泛型测试

**预期时间**：2-3 周

### Phase 3: 高级特性（可选）

**目标**：trait 系统和类型约束

**任务**：
- [ ] Trait 定义和实现
- [ ] 类型约束（trait bounds）
- [ ] 泛型方法和关联类型

**预期时间**：4-6 周

## 5. 技术细节

### 5.1 HIR 类型表示

```rust
// 在 karte-hir/src/ast.rs 中扩展 Type 枚举
pub enum Type {
    // 现有类型...
    Number,
    String,
    Bool,
    Function { params: Vec<Type>, return_type: Box<Type> },

    // 新增：类型参数
    TypeParam {
        name: String,
        id: TypeParamId,  // 唯一标识符
    },

    // 新增：泛型应用（已实例化的泛型类型）
    Generic {
        base: Box<Type>,           // 例如 Box
        args: Vec<Type>,           // 例如 [number]
    },
}

// 函数签名扩展
pub struct FunctionDecl {
    pub name: String,
    pub type_params: Vec<TypeParam>,  // 新增
    pub params: Vec<Parameter>,
    pub return_type: Option<Type>,
    pub body: Box<Expr>,
    pub span: Span,
}

pub struct TypeParam {
    pub name: String,
    pub id: TypeParamId,
    // 未来：constraints: Vec<TraitBound>,
}
```

### 5.2 类型推断与统一化

```rust
// 扩展类型推断器
impl TypeChecker {
    // 处理泛型函数调用
    fn infer_generic_call(
        &mut self,
        function: &FunctionDecl,
        args: &[Expr],
    ) -> Type {
        // 1. 为每个类型参数创建新的类型变量
        let type_var_map = self.fresh_type_vars_for_params(&function.type_params);

        // 2. 实例化函数签名（替换类型参数为类型变量）
        let instantiated_sig = self.instantiate_function_sig(function, &type_var_map);

        // 3. 统一参数类型
        for (arg, param_type) in args.iter().zip(&instantiated_sig.params) {
            let arg_type = self.infer_expr(arg, env);
            self.unify(arg_type, param_type.clone())?;
        }

        // 4. 返回实例化后的返回类型
        instantiated_sig.return_type
    }
}
```

### 5.3 Monomorphization 实现

```rust
// 在 karte-mir/src/monomorphize.rs（新文件）
pub struct Monomorphizer {
    // 记录所有需要特化的泛型函数
    specializations: HashMap<(FunctionId, Vec<ConcreteType>), FunctionId>,
    // 工作队列
    work_queue: VecDeque<(FunctionDecl, Vec<ConcreteType>)>,
}

impl Monomorphizer {
    pub fn monomorphize(&mut self, hir: &HirProgram) -> MirProgram {
        // 1. 从入口函数开始遍历
        self.process_function(&hir.main_function, vec![]);

        // 2. 处理工作队列中的所有函数
        while let Some((func, type_args)) = self.work_queue.pop_front() {
            self.specialize_function(func, type_args);
        }

        // 3. 生成 MIR
        self.generate_mir()
    }

    fn specialize_function(
        &mut self,
        func: &FunctionDecl,
        type_args: Vec<ConcreteType>,
    ) -> FunctionId {
        // 检查是否已经特化过
        if let Some(&id) = self.specializations.get(&(func.id, &type_args)) {
            return id;
        }

        // 创建新的特化函数
        let specialized_name = self.mangled_name(func, &type_args);
        let specialized_body = self.substitute_types(&func.body, &type_args);

        // 记录特化
        let new_id = self.allocate_function_id();
        self.specializations.insert((func.id, type_args.clone()), new_id);

        // 如果函数体中调用了其他泛型函数，加入工作队列
        self.collect_generic_calls(&specialized_body);

        new_id
    }

    fn mangled_name(&self, func: &FunctionDecl, type_args: &[ConcreteType]) -> String {
        // identity<number> -> identity$number
        // pair<number, string> -> pair$number$string
        format!("{}${}", func.name, type_args.iter()
            .map(|t| t.mangle())
            .collect::<Vec<_>>()
            .join("$"))
    }
}
```

### 5.4 类型参数作用域

```rust
// 类型环境扩展
pub struct TypeEnvironment {
    // 现有字段...
    variables: HashMap<String, Type>,

    // 新增：类型参数绑定
    type_params: HashMap<String, TypeParamId>,
}

impl TypeEnvironment {
    pub fn with_type_params(
        &self,
        params: &[TypeParam],
    ) -> TypeEnvironment {
        let mut new_env = self.clone();
        for param in params {
            new_env.type_params.insert(param.name.clone(), param.id);
        }
        new_env
    }
}
```

## 6. 测试策略

### 6.1 基础功能测试

```karte
// test_generic_identity.karte
fn identity<T>(x: T) -> T { x }

fn main() -> number {
    let a = identity(42);
    let b = identity("hello");
    a
}
// 预期: 42

// test_generic_pair.karte
fn first<A, B>(pair: (A, B)) -> A {
    let (a, b) = pair;
    a
}

fn main() -> number {
    first((42, "hello"))
}
// 预期: 42
```

### 6.2 类型推断测试

```karte
// test_inference.karte
fn map<A, B>(f: A -> B, x: A) -> B {
    f(x)
}

fn main() -> number {
    let add_one = |x| { x + 1 };
    map(add_one, 41)
}
// 预期: 42
```

### 6.3 高阶泛型测试

```karte
// test_higher_order.karte
fn compose<A, B, C>(f: B -> C, g: A -> B) -> A -> C {
    |x| { f(g(x)) }
}

fn main() -> number {
    let add_one = |x| { x + 1 };
    let double = |x| { x * 2 };
    let h = compose(double, add_one);
    h(5)  // (5 + 1) * 2 = 12
}
// 预期: 12
```

## 7. 潜在问题与解决方案

### 7.1 代码膨胀

**问题**：Monomorphization 可能导致代码大小急剧增长。

**解决方案**：
1. 在 MIR 优化阶段检测重复的特化函数并合并
2. 对于简单的泛型函数（如 identity），考虑内联
3. 未来可以实现基于使用频率的选择性 monomorphization

### 7.2 编译时间

**问题**：大量泛型可能增加编译时间。

**解决方案**：
1. 增量编译：缓存已特化的函数
2. 并行化 monomorphization 过程
3. 延迟特化：只特化实际使用的类型组合

### 7.3 类型推断复杂度

**问题**：泛型可能使类型推断更复杂，导致推断失败或性能下降。

**解决方案**：
1. 要求在必要时提供类型标注
2. 改进错误信息，提示用户添加类型标注
3. 实现更智能的类型推断算法（如 local type inference）

### 7.4 与现有类型系统的兼容性

**问题**：需要确保泛型与现有的函数/闭包统一表示兼容。

**解决方案**：
1. 泛型函数在 monomorphization 后变成普通函数
2. 闭包可以是泛型的，但在捕获时需要实例化
3. 统一表示在 MIR 层面保持不变

## 8. 未来扩展方向

### 8.1 Trait 系统

```karte
trait Display {
    fn to_string(&self) -> string;
}

impl Display for number {
    fn to_string(&self) -> string {
        // ...
    }
}

fn print<T: Display>(value: T) -> () {
    println(value.to_string());
}
```

### 8.2 关联类型

```karte
trait Container {
    type Item;
    fn get(&self) -> Self::Item;
}
```

### 8.3 高阶类型（Higher-Kinded Types）

```karte
trait Functor<F<_>> {
    fn map<A, B>(fa: F<A>, f: A -> B) -> F<B>;
}
```

## 9. 实现里程碑

### Milestone 1: 泛型函数原型（2周）
- [ ] 解析泛型函数语法
- [ ] 类型推断基本支持
- [ ] 简单的 monomorphization
- [ ] 基础测试通过

### Milestone 2: 完整泛型函数（4周）
- [ ] 完善类型推断
- [ ] 优化 monomorphization
- [ ] 高阶泛型函数支持
- [ ] 完整测试套件

### Milestone 3: 泛型数据结构（6周）
- [ ] 泛型结构体
- [ ] 泛型枚举
- [ ] 模式匹配支持

### Milestone 4: 性能优化（8周）
- [ ] 减少代码膨胀
- [ ] 编译时间优化
- [ ] 完整的集成测试

## 10. 讨论问题

以下是需要讨论和决策的关键问题：

1. **语法选择**：
   - 类型参数使用 `<T>` 还是其他语法？
   - 是否需要显式类型标注的语法（如 `identity::<number>(42)`）？

2. **类型推断**：
   - 在什么情况下要求显式类型标注？
   - 是否支持部分类型推断？

3. **实现优先级**：
   - 先实现泛型函数还是同时支持泛型数据结构？
   - 是否需要在 Phase 1 就考虑 trait 系统？

4. **性能权衡**：
   - 是否可以接受代码膨胀？
   - 编译时间增加的可接受范围？

5. **错误处理**：
   - 泛型相关的错误信息应该如何设计？
   - 如何帮助用户诊断类型推断失败？

## 11. 参考资料

- [Rust RFC: Monomorphization](https://rust-lang.github.io/rfcs/)
- [MLton: Whole-Program Compilation](http://mlton.org/)
- [OCaml Manual: Polymorphism](https://ocaml.org/manual/)
- [Type Systems for Programming Languages (Pierce)](https://www.cis.upenn.edu/~bcpierce/tapl/)

---

**文档版本**: v1.0
**创建日期**: 2025-12-02
**状态**: 待讨论
