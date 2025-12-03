# 逃逸点插入方案设计（Escape Point Insertion）

**设计日期**: 2025-12-03
**方案来源**: 借鉴 Golang 编译器的逃逸分析实现
**替代方案**: 替换之前的"全局堆变量转换"方案

---

## 1. 核心思想

### 1.1 方案对比

**旧方案（已废弃）**：
- 将逃逸变量在整个函数中都当作"堆指针"处理
- 所有访问都需要 Load/Store 操作
- 复杂、低效、容易出错

**新方案（Golang-like）**：
- 变量在作用域内保持**正常栈语义**
- 只在**逃逸点**插入堆分配和复制代码
- 简单、高效、易于理解和维证

### 1.2 关键原则

1. **栈优先**：变量默认使用栈分配和栈语义
2. **按需逃逸**：只在确实需要逃逸的地方才处理
3. **局部修改**：只修改逃逸点的代码，不影响整个函数
4. **语义清晰**：栈变量就是栈变量，直到它需要逃逸

---

## 2. 逃逸点识别

### 2.1 什么是逃逸点（Escape Point）？

逃逸点是变量**从栈语义转换到堆语义的临界点**。

### 2.2 逃逸点类型

#### 类型1：取地址并返回/传递

```karte
fn foo() -> &number {
    let d = 1;
    &d          // 逃逸点：取地址并返回
}
```

**MIR模式**：
```mir
%d = num 1
%ref = & %d
return %ref
```

**逃逸点**：`& %d` 语句，如果 `%ref` 会逃逸（返回、存储到堆等）

#### 类型2：闭包捕获

```karte
fn foo() -> || -> number {
    let d = 1;
    || { d }    // 逃逸点：d被闭包捕获
}
```

**MIR模式**：
```mir
%d = num 1
%closure = Closure { captured: [%d] }
return %closure
```

**逃逸点**：创建闭包时捕获变量

#### 类型3：存储到堆结构

```karte
fn foo() {
    let d = 1;
    global_vec.push(d);  // 逃逸点：存储到全局/堆结构
}
```

#### 类型4：通过参数逃逸

```karte
fn foo() {
    let d = 1;
    bar(&d);    // 逃逸点：如果bar会保存这个引用
}
```

---

## 3. 转换逻辑

### 3.1 基本转换模式

**原始MIR**：
```mir
%var = value
%ref = & %var
return %ref
```

**转换后MIR**（如果 `%var` 逃逸）：
```mir
%var = value                // 保持栈语义不变
%heap = HeapAlloc(8)        // 在逃逸点分配堆内存
Store { %heap, %var }       // 复制栈值到堆
%ref = %heap                // 直接使用堆地址（不需要&操作）
return %ref
```

### 3.2 详细转换规则

#### 规则1：Reference 表达式的转换

**识别条件**：
- 语句形式：`%result = & %target`
- 逃逸分析显示 `%target` 逃逸

**转换**：
```rust
// 原始：
Statement::Assign {
    target: %result,
    source: Value::Reference { value: %target },
}

// 转换为：
Statement::HeapAlloc { target: %heap_temp, size: 8 }
Statement::Store { target: %heap_temp, value: %target }
Statement::Assign { target: %result, source: %heap_temp }
```

#### 规则2：闭包捕获的转换

**识别条件**：
- 创建闭包时捕获变量
- 捕获的变量逃逸分析显示需要堆分配

**转换**：
```rust
// 原始：
Value::Closure {
    function_name: "lambda$0",
    captured_values: [%var1, %var2],
}

// 转换为（在创建闭包之前）：
%heap1 = HeapAlloc(8)
Store { %heap1, %var1 }
%heap2 = HeapAlloc(8)
Store { %heap2, %var2 }
Value::Closure {
    function_name: "lambda$0",
    captured_values: [%heap1, %heap2],  // 使用堆地址
}
```

#### 规则3：返回语句的转换

**识别条件**：
- 返回语句：`return %var`
- `%var` 是栈变量但逃逸分析显示会逃逸
- `%var` 不是简单值类型（number, bool等）

**转换**：
```rust
// 原始：
Terminator::Return { value: Some(%var) }

// 转换为：
Statement::HeapAlloc { target: %heap_temp, size: 8 }
Statement::Store { target: %heap_temp, value: %var }
Terminator::Return { value: Some(%heap_temp) }
```

---

## 4. 实现步骤

### 4.1 步骤1：构建逃逸点检测器

**文件**：`karte-mir/src/escape_point_detector.rs`

```rust
pub struct EscapePointDetector {
    /// 逃逸分析结果
    escape_info: HashMap<VariableId, VariableEscapeInfo>,

    /// 变量名到ID的映射
    var_name_to_id: HashMap<String, VariableId>,
}

impl EscapePointDetector {
    /// 检查一个Value是否需要在当前位置逃逸到堆
    pub fn needs_escape_at_point(&self, value: &Value, point: &EscapePoint) -> bool {
        // 检查逻辑：
        // 1. value必须是栈变量（Variable或Temp）
        // 2. value的逃逸状态是ReturnEscape或GlobalEscape
        // 3. 当前点确实是一个逃逸点（&操作、闭包捕获、返回等）
    }

    /// 识别语句中的所有逃逸点
    pub fn find_escape_points(&self, stmt: &Statement) -> Vec<EscapePoint> {
        match stmt {
            Statement::Assign { source: Value::Reference { value }, .. } => {
                // & 操作是潜在逃逸点
                vec![EscapePoint::AddressOf { value: value.clone() }]
            }
            _ => vec![]
        }
    }
}

pub enum EscapePoint {
    /// 取地址操作
    AddressOf { value: Value },
    /// 闭包捕获
    ClosureCapture { value: Value },
    /// 返回
    Return { value: Value },
}
```

### 4.2 步骤2：实现逃逸点转换器

**文件**：`karte-mir/src/escape_point_transformer.rs`

```rust
pub struct EscapePointTransformer {
    detector: EscapePointDetector,
    next_temp_id: usize,
}

impl EscapePointTransformer {
    /// 转换单个函数
    pub fn transform_function(&mut self, func: &mut MirFunction) {
        for (_, block) in &mut func.basic_blocks {
            let mut new_statements = Vec::new();

            for stmt in &block.statements {
                match stmt {
                    Statement::Assign { target, source: Value::Reference { value }, span } => {
                        // 检查是否需要在此处逃逸
                        if self.detector.needs_escape(value) {
                            // 插入逃逸代码
                            let heap_temp = self.alloc_temp();

                            // HeapAlloc
                            new_statements.push(Statement::HeapAlloc {
                                target: heap_temp.clone(),
                                size: 8,
                                object_type: "escaped".to_string(),
                                span: *span,
                            });

                            // Store（复制栈值到堆）
                            new_statements.push(Statement::Store {
                                target: heap_temp.clone(),
                                value: *value.clone(),
                                span: *span,
                            });

                            // 修改赋值语句，使用堆地址
                            new_statements.push(Statement::Assign {
                                target: target.clone(),
                                source: heap_temp,
                                span: *span,
                            });
                        } else {
                            // 不需要逃逸，保持原样
                            new_statements.push(stmt.clone());
                        }
                    }

                    _ => {
                        new_statements.push(stmt.clone());
                    }
                }
            }

            block.statements = new_statements;
        }
    }

    fn alloc_temp(&mut self) -> Value {
        let id = TempId(self.next_temp_id);
        self.next_temp_id += 1;
        Value::Temp { id, ty: None }
    }
}
```

### 4.3 步骤3：集成到编译管道

**文件**：`karte-module-system/src/project.rs`

**位置**：在逃逸分析之后，LIR lowering之前

```rust
// 1. 运行逃逸分析（已有）
let escape_info = analyzer.get_all_escape_info();

// 2. 构建逃逸点检测器
let detector = EscapePointDetector::new(
    escape_info.clone(),
    variable_names.clone(),
    temp_id_mapping.clone(),
);

// 3. 应用逃逸点转换
let mut transformer = EscapePointTransformer::new(detector);
transformer.transform_program(&mut mir_program);

// 4. 继续 LIR lowering
```

---

## 5. 测试用例

### 5.1 测试1：基本取地址逃逸

**源代码**：`test_escape_basic.karte`
```karte
fn main() -> number {
    let a = || {
        let d = 1;
        &d
    };
    *(a())
}
```

**期望MIR**（转换后）：
```mir
lambda$0:
    %1 = num 1                  // d 正常栈分配
    %heap = HeapAlloc(8)        // 在&d之前分配堆
    Store { %heap, %1 }         // 复制d到堆
    %0 = %heap                  // 返回堆地址（不需要&操作）
    return %0
```

**期望结果**：返回1，不崩溃

### 5.2 测试2：多个逃逸点

**源代码**：`test_escape_multiple.karte`
```karte
fn main() -> number {
    let a = || {
        let d = 1;
        &d
    };
    let b = || {
        let e = 999;
        e
    };
    let ptr = a();
    let tmp = b();
    *ptr
}
```

**期望行为**：
- lambda$0 中的 `d` 需要逃逸（取地址并返回）
- lambda$1 中的 `e` **不需要**逃逸（直接返回值）

**期望MIR**：
```mir
lambda$0:
    %1 = num 1
    %heap = HeapAlloc(8)        // 需要逃逸
    Store { %heap, %1 }
    %0 = %heap
    return %0

lambda$1:
    %1 = num 999
    %0 = %1                     // 不需要逃逸，正常赋值
    return %0
```

**期望结果**：返回1，不崩溃

### 5.3 测试3：条件逃逸

**源代码**：`test_escape_conditional.karte`
```karte
fn foo(b: bool) -> &number {
    let x = 1;
    let y = 2;
    if b {
        &x
    } else {
        &y
    }
}
```

**期望行为**：
- `x` 和 `y` 都可能逃逸
- 在各自的分支中插入逃逸代码

---

## 6. 性能对比

### 6.1 旧方案（全局堆变量）

**函数内变量访问次数**：假设10次
- HeapAlloc: 1次（函数入口）
- Store/Load: 20次（每次访问都需要Load/Store）
- **总开销**：21次内存操作

### 6.2 新方案（逃逸点插入）

**函数内变量访问次数**：假设10次
- 栈访问: 10次（正常mov指令）
- HeapAlloc: 1次（逃逸点）
- Store: 1次（复制到堆）
- **总开销**：2次内存操作 + 10次寄存器操作

**性能提升**：~10倍

---

## 7. 实现计划

### 时间估计

1. **步骤1**：实现 EscapePointDetector（2-3小时）
2. **步骤2**：实现 EscapePointTransformer（3-4小时）
3. **步骤3**：集成到编译管道（1-2小时）
4. **步骤4**：测试和调试（2-3小时）
5. **步骤5**：清理旧代码（1小时）

**总计**：约1个工作日

### 优先级

**高优先级**：
- [ ] 实现基本的 Reference 表达式转换（测试1）
- [ ] 测试 test_closure_escape_bug.karte

**中优先级**：
- [ ] 处理闭包捕获
- [ ] 处理返回语句
- [ ] 测试 test_closure_escape_ub.karte

**低优先级**：
- [ ] 优化：合并连续的 HeapAlloc
- [ ] 优化：内联小对象复制

---

## 8. 与旧方案的对比

| 维度 | 旧方案（全局转换） | 新方案（逃逸点插入） |
|------|-------------------|---------------------|
| **复杂度** | 高（需要追踪整个函数） | 低（只处理逃逸点） |
| **性能** | 差（所有访问都Load/Store） | 好（栈访问很快） |
| **正确性** | 容易出错（转换逻辑复杂） | 易于验证（局部修改） |
| **代码量** | 大（500+ 行） | 小（200-300 行） |
| **可维护性** | 差 | 好 |

---

## 9. 风险和缓解

### 风险1：逃逸点识别遗漏

**风险**：如果遗漏某个逃逸点，会导致UB
**缓解**：
- 详细的测试用例覆盖所有逃逸场景
- 保守策略：有疑问时选择堆分配

### 风险2：与现有代码冲突

**风险**：新方案可能与现有MIR生成代码冲突
**缓解**：
- 在单独的模块中实现，便于回滚
- 保留旧方案代码，通过feature flag切换

### 风险3：闭包捕获处理复杂

**风险**：闭包捕获的转换可能比预期复杂
**缓解**：
- 先实现简单的 Reference 转换
- 闭包捕获作为第二阶段

---

## 10. 总结

### 为什么新方案更好？

1. **简单**：只在需要的地方插入代码
2. **高效**：栈访问快，只在必要时分配堆
3. **正确**：局部修改，容易验证
4. **可维护**：代码量少，逻辑清晰

### 实施路径

1. ✅ 完成设计文档（当前文件）
2. ⏳ 实现 EscapePointDetector
3. ⏳ 实现 EscapePointTransformer
4. ⏳ 集成和测试
5. ⏳ 清理旧代码

**预计完成时间**：1个工作日
