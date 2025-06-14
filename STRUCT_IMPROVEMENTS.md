# LIR结构体处理改进方案

## 问题分析

当前的LIR实现在处理结构体时存在以下严重问题：

### 1. 结构体表示不当
- **问题**：结构体被简化为哈希ID立即数，丢失了所有内存布局信息
- **影响**：无法正确进行字段访问，不支持复杂的结构体操作

### 2. 内存管理缺失
- **问题**：只支持立即数，无法表示真实的内存结构
- **影响**：无法分配实际的结构体内存，不支持指针和引用

### 3. 字段访问机制简陋
- **问题**：依赖固定的偏移表（`field_name_to_offset`），不支持动态类型
- **影响**：无法处理用户定义的结构体，扩展性差

### 4. 缺乏内存对齐支持
- **问题**：没有考虑内存对齐要求
- **影响**：可能导致性能问题和在某些平台上的错误

## 改进方案

### 1. 结构体布局管理系统

#### 新增类型和结构
```rust
/// 结构体类型标识符
pub struct StructTypeId(pub usize);

/// 结构体字段定义
pub struct StructField {
    pub name: String,
    pub offset: usize,
    pub size: usize,
    pub alignment: usize,
}

/// 结构体布局信息
pub struct StructLayout {
    pub name: String,
    pub fields: Vec<StructField>,
    pub total_size: usize,
    pub alignment: usize,
}

/// 结构体布局管理器
pub struct StructLayoutManager {
    layout_cache: HashMap<String, StructLayout>,
    type_sizes: HashMap<String, usize>,
    type_alignments: HashMap<String, usize>,
}
```

#### 核心功能
- **自动布局计算**：根据字段类型自动计算偏移和对齐
- **缓存机制**：避免重复计算相同结构体的布局
- **递归支持**：支持包含其他结构体的复杂结构体
- **布局验证**：验证布局的正确性和一致性
- **性能分析**：分析内存使用效率，提供优化建议

### 2. 扩展的LIR指令集

#### 内存分配类型
```rust
pub enum AllocationType {
    Stack,   // 栈分配
    Heap,    // 堆分配
    Static,  // 静态分配
}
```

#### 新增操作数类型
```rust
pub enum Operand {
    // ... 现有类型 ...
    StructField { struct_addr: RegisterId, field_offset: usize },
    MemoryRef { id: MemoryId },
}
```

#### 结构体专用指令
```rust
/// 分配结构体内存
StructAlloc {
    dst: RegisterId,
    struct_type: StructTypeId,
    allocation_type: AllocationType,
    span: Span,
},

/// 加载结构体字段
StructFieldLoad {
    dst: RegisterId,
    struct_addr: RegisterId,
    field_offset: usize,
    span: Span,
},

/// 存储结构体字段
StructFieldStore {
    struct_addr: RegisterId,
    field_offset: usize,
    src: Operand,
    span: Span,
},

/// 获取结构体字段地址
StructFieldAddr {
    dst: RegisterId,
    struct_addr: RegisterId,
    field_offset: usize,
    span: Span,
},

/// 内存拷贝指令（用于结构体赋值）
MemCopy {
    dst: RegisterId,
    src: RegisterId,
    size: usize,
    span: Span,
},
```

#### 通用内存操作指令
```rust
/// 内存分配指令
Alloc {
    dst: RegisterId,
    size: usize,
    alignment: usize,
    allocation_type: AllocationType,
    span: Span,
},

/// 64位内存加载/存储
Load64 { dst: RegisterId, addr: RegisterId, offset: i64, span: Span },
Store64 { addr: RegisterId, offset: i64, src: Operand, span: Span },
```

### 3. 改进的Lower实现

#### 结构体值处理
```rust
fn handle_struct_value(&mut self, name: &str, fields: &HashMap<String, Value>) -> Result<RegisterId, String> {
    // 1. 获取或创建结构体类型ID
    let struct_type_id = self.get_or_create_struct_type_id(name)?;
    
    // 2. 分配结构体内存
    let struct_addr = self.current_function_mut().new_register();
    self.add_instruction(Instruction::StructAlloc {
        dst: struct_addr,
        struct_type: struct_type_id,
        allocation_type: AllocationType::Stack,
        span: Span::dummy(),
    });

    // 3. 初始化每个字段
    let layout = self.current_function.as_ref()
        .and_then(|f| f.get_struct_layout(struct_type_id))
        .ok_or("无法获取结构体布局")?;

    for field in &layout.fields {
        if let Some(field_value) = fields.get(&field.name) {
            let field_operand = self.value_to_operand(field_value);
            self.add_instruction(Instruction::StructFieldStore {
                struct_addr,
                field_offset: field.offset,
                src: field_operand,
                span: Span::dummy(),
            });
        }
    }

    Ok(struct_addr)
}
```

#### 字段访问处理
```rust
// 新的字段访问实现使用专门的结构体指令
match resolved_object {
    Value::Struct { name, fields } => {
        let struct_type_id = ctx.get_or_create_struct_type_id(&name)?;
        let layout = ctx.current_function.as_ref()
            .and_then(|f| f.get_struct_layout(struct_type_id))?;
        
        if let Some(field_info) = layout.fields.iter().find(|f| f.name == *field) {
            let object_reg = ctx.allocate_register_for_value(object);
            
            ctx.add_instruction(Instruction::StructFieldLoad {
                dst,
                struct_addr: object_reg,
                field_offset: field_info.offset,
                span: *span,
            });
        }
    },
    // ... 其他情况 ...
}
```

### 4. 增强的显示系统

#### 结构体布局显示
```rust
impl fmt::Display for StructLayout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "struct {} (size: {}, align: {}) {{", self.name, self.total_size, self.alignment)?;
        for field in &self.fields {
            writeln!(f, "  {} @ {} (size: {}, align: {})", 
                     field.name, field.offset, field.size, field.alignment)?;
        }
        writeln!(f, "}}")
    }
}
```

#### 函数显示增强
```rust
impl fmt::Display for LirFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "function {} (stack_frame: {}):", self.name, self.stack_frame_size)?;
        
        // 显示结构体类型定义
        if !self.struct_types.is_empty() {
            writeln!(f, "  # Struct types:")?;
            for (type_id, layout) in &self.struct_types {
                writeln!(f, "  # {}: {}", type_id, layout.name)?;
            }
        }
        
        // ... 指令显示 ...
    }
}
```

## 实现优势

### 1. 类型安全
- **强类型系统**：每个结构体都有明确的类型ID和布局信息
- **编译时检查**：字段访问在lowering阶段就可以验证
- **内存安全**：防止越界访问和类型混淆

### 2. 性能优化
- **内存对齐**：自动处理内存对齐，优化访问性能
- **布局优化**：提供布局分析和优化建议
- **缓存机制**：避免重复计算布局信息

### 3. 可扩展性
- **动态类型支持**：支持任意用户定义的结构体
- **嵌套结构体**：支持复杂的嵌套结构体
- **多种分配方式**：支持栈、堆、静态分配

### 4. 调试友好
- **详细信息**：显示完整的布局和类型信息
- **清晰指令**：专门的结构体操作指令，易于理解
- **错误诊断**：提供详细的错误信息和建议

## 示例对比

### 改进前（问题版本）
```rust
// 结构体被简化为哈希ID
Value::Struct { name, fields } => {
    let struct_id = self.generate_struct_id(name, fields);
    Operand::Immediate { value: struct_id }  // 丢失所有结构信息！
}

// 字段访问使用固定偏移
let field_offset = field_name_to_offset(field);  // 硬编码！
ctx.add_instruction(Instruction::Add {
    dst, src1: object_reg, src2: Immediate { value: field_offset }
});
```

### 改进后（专业版本）
```rust
// 结构体分配真实内存
let struct_addr = self.handle_struct_value(name, fields)?;
Operand::Register { id: struct_addr }  // 返回真实内存地址

// 字段访问使用类型化指令
ctx.add_instruction(Instruction::StructFieldLoad {
    dst,
    struct_addr: object_reg,
    field_offset: field_info.offset,  // 来自布局计算
    span: *span,
});
```

## 测试验证

### 1. 布局计算测试
```rust
#[test]
fn test_basic_struct_layout() {
    let mut manager = StructLayoutManager::new();
    let fields = vec![
        HirStructField { name: "x".to_string(), field_type: Type::Number },
        HirStructField { name: "y".to_string(), field_type: Type::Number },
    ];
    
    let layout = manager.compute_layout("Point", &fields).unwrap();
    
    assert_eq!(layout.total_size, 16);  // 两个8字节字段
    assert_eq!(layout.alignment, 8);    // 8字节对齐
    assert_eq!(layout.fields[0].offset, 0);
    assert_eq!(layout.fields[1].offset, 8);
}
```

### 2. 混合类型测试
```rust
#[test]
fn test_mixed_type_struct() {
    let fields = vec![
        HirStructField { name: "flag".to_string(), field_type: Type::Boolean },
        HirStructField { name: "value".to_string(), field_type: Type::Number },
    ];
    
    let layout = manager.compute_layout("Mixed", &fields).unwrap();
    
    // bool (1 byte) + 7 bytes padding + i64 (8 bytes) = 16 bytes
    assert_eq!(layout.total_size, 16);
    assert_eq!(layout.fields[1].offset, 8);  // 对齐到8字节边界
}
```

### 3. 指令生成测试
```rust
#[test]
fn test_struct_instruction_generation() {
    let mut function = LirFunction::new("test".to_string());
    let struct_type_id = function.add_struct_type(layout);
    
    function.add_instruction(Instruction::StructAlloc {
        dst: addr,
        struct_type: struct_type_id,
        allocation_type: AllocationType::Stack,
        span: Span::dummy(),
    });
    
    assert!(matches!(function.instructions[0], Instruction::StructAlloc { .. }));
}
```

## 结论

这个改进方案将LIR从一个简陋的结构体处理系统升级为工业级的专业实现：

1. **从哈希ID到真实内存**：彻底解决了结构体表示问题
2. **从固定偏移到动态布局**：支持任意用户定义的结构体
3. **从简单指令到专用指令集**：提供了完整的结构体操作能力
4. **从无对齐到自动对齐**：确保了内存访问的正确性和性能

这个改进不仅解决了当前的问题，还为未来的扩展（如GC支持、SIMD优化等）打下了坚实的基础，是一个真正专业的编译器后端实现。 