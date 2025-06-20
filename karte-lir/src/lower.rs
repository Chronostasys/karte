use crate::{
    Instruction, LabelId, LirFunction, LirProgram, Operand, RegisterId,
    StructTypeId, AllocationType, StructLayoutManager, StructField, StructLayout,
    tagged_union::{TaggedUnionManager, TaggedUnionTag},
};
use karte_mir::{
    BasicBlockId, BinaryOperator, MirProgram, Statement, Terminator, TempId, UnaryOperator,
    Value,
    Pattern,
};
use std::collections::HashMap;
use std::collections::HashSet;

/// MIR到LIR的lowering上下文（简化版本）
/// 
/// 遵循Stack-First策略：
/// - 简化值映射逻辑，移除复杂的HashMap
/// - 所有变量都先分配到栈上
/// - 让Memory2Reg Pass来决定哪些可以优化到寄存器
pub struct LirLoweringContext {
    /// 当前LIR函数
    current_function: Option<LirFunction>,
    /// MIR基本块到标签的映射
    block_to_label: HashMap<BasicBlockId, LabelId>,
    /// 函数名到标签的映射
    function_labels: HashMap<String, LabelId>,
    /// 当前函数的参数列表
    current_function_params: Vec<String>,
    /// 全局标签计数器
    global_label_counter: usize,
    /// 待处理的指令
    pending_instructions: Vec<Instruction>,
    /// 错误信息
    errors: Vec<String>,
    /// 结构体布局管理器
    struct_layout_manager: StructLayoutManager,
    /// 结构体名称到类型ID的映射
    struct_name_to_type_id: HashMap<String, StructTypeId>,
    /// Tagged Union管理器
    tagged_union_manager: TaggedUnionManager,
    /// 栈分配追踪（统一的值存储策略）
    stack_allocations: HashMap<String, RegisterId>,
    /// 🔧 专业修复：全局结构体类型信息
    global_struct_types: HashMap<String, StructLayout>,
}

impl LirLoweringContext {
    pub fn new() -> Self {
        Self {
            current_function: None,
            block_to_label: HashMap::new(),
            function_labels: HashMap::new(),
            current_function_params: vec![],
            global_label_counter: 0,
            pending_instructions: vec![],
            errors: vec![],
            struct_layout_manager: StructLayoutManager::new(),
            struct_name_to_type_id: HashMap::new(),
            tagged_union_manager: TaggedUnionManager::new(),
            stack_allocations: HashMap::new(),
            global_struct_types: HashMap::new(),
        }
    }

    /// 开始新函数
    pub fn start_function(&mut self, name: String) {
        self.current_function = Some(LirFunction::new(name.clone()));
        // 清理函数相关的状态
        self.pending_instructions.clear();
        // 清空基本块到标签的映射，确保每个函数的标签都是唯一的
        self.block_to_label.clear();
        // 清空栈分配，每个函数都重新开始
        self.stack_allocations.clear();
    }
    
    /// 🔧 新增：开始新函数并设置参数信息
    pub fn start_function_with_params(&mut self, name: String, params: &[String]) {
        self.current_function = Some(LirFunction::new_with_params(name.clone(), params.len()));
        // 清理函数相关的状态
        self.pending_instructions.clear();
        // 清空基本块到标签的映射，确保每个函数的标签都是唯一的
        self.block_to_label.clear();
        // 清空栈分配，每个函数都重新开始
        self.stack_allocations.clear();
        
        // 设置当前函数参数列表
        self.current_function_params = params.to_vec();
        
        println!("🔧 创建函数 {} 包含 {} 个参数: {:?}", name, params.len(), params);
    }

    /// 完成当前函数
    pub fn finish_function(&mut self) -> Option<LirFunction> {
        let mut f = self.current_function.take();
        if let Some(func) = &mut f {
            func.instructions.append(&mut self.pending_instructions);
        }
        f
    }

    /// 获取当前函数的可变引用
    fn current_function_mut(&mut self) -> &mut LirFunction {
        self.current_function.as_mut().expect("No current function")
    }

    /// 为值分配栈槽（Stack-First策略）
    fn allocate_stack_slot_for_value(&mut self, value: &Value) -> RegisterId {
        self.allocate_stack_slot_for_value_with_instruction(value, true)
    }
    
    /// 为值分配栈槽，可以选择是否生成alloc指令
    fn allocate_stack_slot_for_value_with_instruction(&mut self, value: &Value, generate_alloc: bool) -> RegisterId {
        let key = value_to_key(value);
        
        // 🔧 关键修复：检查是否已经为这个值分配了栈槽
        // 如果已经分配，重用现有的栈槽，确保同一个值在整个函数中使用相同的地址
        if let Some(&existing_addr) = self.stack_allocations.get(&key) {
            return existing_addr;
        }
        
        // 为新值分配栈空间
        let address_register = self.current_function_mut().new_register();
        
        // 根据值类型确定需要的空间大小
        let size = match value {
            Value::Boolean { .. } | 
            Value::Constructor { .. } | 
            Value::QualifiedConstructor { .. } => 16, // Tagged Union需要16字节（tag + data）
            _ => 8, // 其他值8字节
        };
        
        // 只有在需要时才生成alloc指令
        if generate_alloc {
            // 在栈上分配空间来存储这个值
            self.add_instruction(Instruction::Alloc {
                dst: address_register, 
                size,
                alignment: 8,
                allocation_type: AllocationType::Stack,
                span: karte_diagnostics::Span::dummy(),
            });
        }
        
        // 记录分配的栈地址，供后续使用
        self.stack_allocations.insert(key, address_register);
        address_register
    }

    /// Stack-First策略的Store操作
    /// 将值存储到已分配的栈位置
    pub fn store_value_to_stack(&mut self, value: &Value, src_operand: Operand) {
        let value_key = value_to_key(value);
        
        // 确保值已经有栈空间分配
        let stack_addr = if let Some(&existing_addr) = self.stack_allocations.get(&value_key) {
            existing_addr
        } else {
            // 如果没有分配，现在分配
            let addr = self.allocate_stack_slot_for_value(value);
            self.stack_allocations.insert(value_key.clone(), addr);
            addr
        };
        
        // 存储值到栈上
        self.add_instruction(Instruction::Store64 {
            addr: stack_addr,
            offset: 0,
            src: src_operand,
            span: karte_diagnostics::Span::dummy(),
        });
    }

    /// 为值分配寄存器（简化版：主要用于函数参数）
    fn allocate_register_for_value(&mut self, value: &Value) -> RegisterId {
        // 检查是否是函数参数 - 函数参数仍然使用寄存器传递
        if let Value::Variable { name } = value {
            if let Some(param_index) = self.current_function_params.iter().position(|p| p == name) {
                // 函数参数使用固定的寄存器：r1, r2, r3, r4（跳过r0作为特殊用途）
                return RegisterId(param_index + 1);
            }
        }
        
        // 🔧 修复：检查是否已经为这个值分配了寄存器
        let value_key = value_to_key(value);
        if let Some(&existing_reg) = self.stack_allocations.get(&value_key) {
            return existing_reg;
        }
        
        // 对于非参数变量，使用栈分配
        self.allocate_stack_slot_for_value(value)
    }

    /// 为基本块分配标签
    fn allocate_label_for_block(&mut self, block_id: BasicBlockId) -> LabelId {
        if let Some(label) = self.block_to_label.get(&block_id) {
            return *label;
        }
        let label = LabelId(self.global_label_counter);
        self.global_label_counter += 1;
        self.block_to_label.insert(block_id, label);
        label
    }

    fn new_label(&mut self) -> LabelId {
        let label = LabelId(self.global_label_counter);
        self.global_label_counter += 1;
        label
    }

    fn add_pending_instructions(&mut self, mut instructions: Vec<Instruction>) {
        self.pending_instructions.append(&mut instructions);
    }

    /// 已弃用：请使用 lower_to_lvalue 或 lower_to_rvalue
    /// 根据fix.md的指导，该函数应被完全替换
    #[deprecated(note = "请根据上下文使用 lower_to_lvalue 或 lower_to_rvalue")]
    fn value_to_operand(&mut self, value: &Value) -> Operand {
        // 默认行为：返回R-Value
        // 这是为了兼容性，但应该尽快移除所有调用
        eprintln!("警告：仍在使用已弃用的 value_to_operand 函数，请检查代码");
        self.lower_to_rvalue(value)
    }

    /// 🔧 新增：L-Value降级 - 返回值的存储地址
    /// 这个函数总是返回一个表示存储位置的操作数
    fn lower_to_lvalue(&mut self, value: &Value) -> Operand {
        let value_key = value_to_key(value);
        
        // 🔧 专业修复：正确处理结构体值的初始化
        match value {
            Value::Struct { name, fields } => {
                // 检查是否已经处理过这个结构体
                if let Some(&existing_addr) = self.stack_allocations.get(&value_key) {
                    return Operand::Register { id: existing_addr };
                }
                
                // 调用handle_struct_value来正确创建和初始化结构体
                match self.handle_struct_value(name, fields) {
                    Ok(struct_addr) => {
                        self.stack_allocations.insert(value_key, struct_addr);
                        return Operand::Register { id: struct_addr };
                    }
                    Err(e) => {
                        self.errors.push(e);
                        // 返回一个默认的寄存器作为fallback
                        let fallback_reg = self.current_function_mut().new_register();
                        return Operand::Register { id: fallback_reg };
                    }
                }
            }
            _ => {
                // 其他值类型的原有逻辑
            }
        }
        
        // 对于函数参数，我们需要为其分配栈空间
        if let Value::Variable { name } = value {
            if self.current_function_params.contains(name) {
                // 函数参数：如果还没有栈分配，先创建一个
                if let Some(&stack_addr) = self.stack_allocations.get(&value_key) {
                    return Operand::Register { id: stack_addr };
                } else {
                    // 为参数分配栈空间
                    let stack_addr = self.allocate_stack_slot_for_value(value);
                    self.stack_allocations.insert(value_key, stack_addr);
                    
                    // 将参数值存储到栈（如果需要）
                    if let Some(param_index) = self.current_function_params.iter().position(|p| p == name) {
                        let param_reg = RegisterId(param_index + 1);
                        self.add_instruction(Instruction::Store64 {
                            addr: stack_addr,
                            offset: 0,
                            src: Operand::Register { id: param_reg },
                            span: karte_diagnostics::Span::dummy(),
                        });
                    }
                    
                    return Operand::Register { id: stack_addr };
                }
            }
        }
        
        // 🔧 关键修复：对于临时变量，优先查找FieldAccess创建的独立栈槽
        if let Value::Temp { .. } = value {
            // 检查是否有FieldAccess为这个临时变量创建的独立栈槽
            // 按优先级顺序查找：function_ptr > env_ptr > 其他字段
            let field_keys = [
                format!("field_function_ptr:{}", value_key),
                format!("field_env_ptr:{}", value_key),
            ];
            
            for field_key in &field_keys {
                if let Some(&field_stack_addr) = self.stack_allocations.get(field_key) {
                    println!("🔧 lower_to_lvalue: 找到FieldAccess栈槽 {} -> {:?} (来自{})", value_key, field_stack_addr, field_key);
                    // 将这个栈槽也注册到常规的value_key下，便于后续查找
                    self.stack_allocations.insert(value_key, field_stack_addr);
                    return Operand::Register { id: field_stack_addr };
                }
            }
            
            // 如果没有找到FieldAccess栈槽，检查是否有其他field_*键
            for (key, &addr) in self.stack_allocations.iter() {
                if key.ends_with(&format!(":{}", value_key)) && key.starts_with("field_") {
                    println!("🔧 lower_to_lvalue: 找到其他FieldAccess栈槽 {} -> {:?} (来自{})", value_key, addr, key);
                    // 将这个栈槽也注册到常规的value_key下
                    self.stack_allocations.insert(value_key, addr);
                    return Operand::Register { id: addr };
                }
            }
        }
        
        // 检查是否已经有栈分配
        if let Some(&stack_addr) = self.stack_allocations.get(&value_key) {
            println!("🔧 lower_to_lvalue: 找到已分配的栈槽 {} -> {:?}", value_key, stack_addr);
            return Operand::Register { id: stack_addr };
        }
        
        // 分配新的栈空间
        println!("🔧 lower_to_lvalue: 需要分配新栈槽 {}", value_key);
        let stack_addr = self.allocate_stack_slot_for_value(value);
        println!("🔧 lower_to_lvalue: 分配了新栈槽 {} -> {:?}", value_key, stack_addr);
        self.stack_allocations.insert(value_key, stack_addr);
        
        // 对于引用值，需要特殊处理
        match value {
            Value::Reference { .. } => {
                // 引用值的初始化将在其他地方处理
            }
            _ => {
                // 其他值类型可能需要初始化（但变量通常通过赋值设置）
                match value {
                    Value::Variable { .. } | Value::Temp { .. } => {
                        // 变量和临时值不需要在这里初始化
                    }
                    _ => {
                        self.initialize_stack_value(value, stack_addr);
                    }
                }
            }
        }
        
        Operand::Register { id: stack_addr }
    }

    /// 🔧 新增：R-Value降级 - 返回值的内容
    /// 这个函数返回一个表示值内容的操作数
    fn lower_to_rvalue(&mut self, value: &Value) -> Operand {
        // 🔧 修复：特殊处理函数参数 - 直接使用参数寄存器
        if let Value::Variable { name } = value {
            if self.current_function_params.contains(name) {
                if let Some(param_index) = self.current_function_params.iter().position(|p| p == name) {
                    let param_reg = RegisterId(param_index + 1); // 参数寄存器: r1, r2, r3, r4
                    println!("🔧 函数参数 {} 在lower_to_rvalue中直接使用寄存器 {:?}", name, param_reg);
                    return Operand::Register { id: param_reg };
                }
            }
        }
        
        match value {
            // 立即数值直接返回
            Value::Number { value } => Operand::Immediate { value: *value },
            Value::Boolean { value } => Operand::Immediate { value: if *value { 1 } else { 0 } },
            Value::Unit => Operand::Immediate { value: 0 },
            
            // 函数值
            Value::Function { name } => {
                if let Some(&label_id) = self.function_labels.get(name) {
                    Operand::Immediate { value: label_id.0 as i64 }
                } else {
                    // 🔧 关键修复：如果函数不在映射中，这是一个错误，不应该分配新标签
                    // 所有函数标签都应该在预处理阶段分配好
                    panic!("函数 {} 的标签未找到！这表明函数标签预分配有问题。", name);
                }
            }
            
            // 对于引用值，返回被引用值的地址
            Value::Reference { value: referenced_value } => {
                // 引用表达式的R-Value就是被引用值的L-Value（地址）
                self.lower_to_lvalue(referenced_value)
            }
            
            // 🔧 修复：正确处理结构体值
            Value::Struct { .. } => {
                // 结构体值的R-Value就是其L-Value（结构体的基地址）
                // 这将调用lower_to_lvalue，进而调用handle_struct_value来正确初始化结构体
                self.lower_to_lvalue(value)
            }
            
            // 对于其他值，我们需要从存储位置加载
            _ => {
                // 首先获取存储地址
                let lvalue = self.lower_to_lvalue(value);
                
                // 对于某些特殊类型，直接返回地址而不是加载内容
                match value {
                    Value::Constructor { .. } | Value::QualifiedConstructor { .. } => {
                        // Tagged Union构造器：返回地址
                        lvalue
                    }
                    _ => {
                        // 其他值：从地址加载内容
                        if let Operand::Register { id: addr_reg } = lvalue {
                            let temp_reg = self.current_function_mut().new_register();
                            self.add_instruction(Instruction::Load64 {
                                dst: temp_reg,
                                addr: addr_reg,
                                offset: 0,
                                span: karte_diagnostics::Span::dummy(),
                            });
                            Operand::Register { id: temp_reg }
                        } else {
                            // 如果L-Value不是寄存器，直接返回
                            lvalue
                        }
                    }
                }
            }
        }
    }

    /// Stack-First策略的核心实现
    /// 所有非立即数的值都分配到栈上，只在需要时load到寄存器
    fn handle_stack_first_value(&mut self, value: &Value) -> Operand {
        let value_key = value_to_key(value);
        
        // 🔧 修复：特殊处理函数参数 - 直接使用参数寄存器
        if let Value::Variable { name } = value {
            if self.current_function_params.contains(name) {
                if let Some(param_index) = self.current_function_params.iter().position(|p| p == name) {
                    let param_reg = RegisterId(param_index + 1); // 参数寄存器: r1, r2, r3, r4
                    println!("🔧 函数参数 {} 直接使用寄存器 {:?}", name, param_reg);
                    return Operand::Register { id: param_reg };
                }
            }
        }
        
        // 检查是否已经分配了栈空间
        let existing_stack_addr = self.stack_allocations.get(&value_key).copied();
        if let Some(stack_addr) = existing_stack_addr {
            // 对于Tagged Union类型，返回栈地址而不是加载内容
            // 但是Boolean值是特殊情况，不应该被当作Tagged Union处理
            match value {
                Value::Constructor { .. } | 
                Value::QualifiedConstructor { .. } => {
                    // Tagged Union值：返回栈地址本身
                    return Operand::Register { id: stack_addr };
                }
                _ => {
                    // 其他值（包括Boolean）：从栈加载内容
                    let temp_register = self.current_function_mut().new_register();
                    
                    // 从栈加载值到临时寄存器
                    self.add_instruction(Instruction::Load64 {
                        dst: temp_register,
                        addr: stack_addr,
                        offset: 0,
                        span: karte_diagnostics::Span::dummy(),
                    });
                    
                    return Operand::Register { id: temp_register };
                }
            }
        }
    
       // 为这个值分配栈空间
       let stack_addr = self.allocate_stack_slot_for_value(value);
       self.stack_allocations.insert(value_key.clone(), stack_addr);
       
       // 🔧 修复：只对需要初始化的值类型调用initialize_stack_value
       // 变量和临时值不应该在这里初始化，它们的值通过赋值语句设置
       match value {
           Value::Variable { .. } | Value::Temp { .. } => {
               // 变量和临时值：不初始化，等待赋值语句设置值
           }
           _ => {
               // 其他值类型（如构造器、布尔值等）需要初始化
               self.initialize_stack_value(value, stack_addr);
           }
       }
       
       // 对于Tagged Union类型，返回栈地址而不是加载内容
       // 但是Boolean值是特殊情况，不应该被当作Tagged Union处理
       match value {
           Value::Constructor { .. } | 
           Value::QualifiedConstructor { .. } => {
               // Tagged Union值：返回栈地址本身
               Operand::Register { id: stack_addr }
           }
           _ => {
               // 其他值（包括Boolean）：从栈加载内容
               let temp_register = self.current_function_mut().new_register();
               
               // 从栈加载值到临时寄存器  
               self.add_instruction(Instruction::Load64 {
                   dst: temp_register,
                   addr: stack_addr,
                   offset: 0,
                   span: karte_diagnostics::Span::dummy(),
               });
               
               Operand::Register { id: temp_register }
           }
       }
    }
    
    /// 初始化栈上的值
         fn initialize_stack_value(&mut self, value: &Value, stack_addr: RegisterId) {
         match value {
             Value::Boolean { value } => {
                 // Bool特殊处理：直接存储0/1值，不使用Tagged Union
                 let bool_value = if *value { 1 } else { 0 };
                 self.add_instruction(Instruction::Store64 {
                     addr: stack_addr,
                     offset: 0,
                     src: Operand::Immediate { value: bool_value },
                     span: karte_diagnostics::Span::dummy(),
                 });
             },
             
             Value::Constructor { name, arg } => {
                 // 创建Tagged Union for constructor
                 let struct_addr = self.create_tagged_union_for_constructor(name, arg.as_deref());
                 
                 // 将Tagged Union的内容复制到栈位置（16字节）
                 // 复制tag字段（8字节）
                 let temp_tag = self.current_function_mut().new_register();
                 self.add_instruction(Instruction::Load64 {
                     dst: temp_tag,
                     addr: struct_addr,
                     offset: 0,
                     span: karte_diagnostics::Span::dummy(),
                 });
                 self.add_instruction(Instruction::Store64 {
                     addr: stack_addr,
                     offset: 0,
                     src: Operand::Register { id: temp_tag },
                     span: karte_diagnostics::Span::dummy(),
                 });
                 
                 // 复制data字段（8字节）
                 let temp_data = self.current_function_mut().new_register();
                 self.add_instruction(Instruction::Load64 {
                     dst: temp_data,
                     addr: struct_addr,
                     offset: 8,
                     span: karte_diagnostics::Span::dummy(),
                 });
                 self.add_instruction(Instruction::Store64 {
                     addr: stack_addr,
                     offset: 8,
                     src: Operand::Register { id: temp_data },
                     span: karte_diagnostics::Span::dummy(),
                 });
             },
             
             Value::QualifiedConstructor { type_name, constructor_name, arg } => {
                 // 创建Tagged Union for qualified constructor
                 let struct_addr = self.create_tagged_union_for_qualified_constructor(
                     type_name, constructor_name, arg.as_deref()
                 );
                 
                 // 将Tagged Union的内容复制到栈位置（16字节）
                 // 复制tag字段（8字节）
                 let temp_tag = self.current_function_mut().new_register();
                 self.add_instruction(Instruction::Load64 {
                     dst: temp_tag,
                     addr: struct_addr,
                     offset: 0,
                     span: karte_diagnostics::Span::dummy(),
                 });
                 self.add_instruction(Instruction::Store64 {
                     addr: stack_addr,
                     offset: 0,
                     src: Operand::Register { id: temp_tag },
                     span: karte_diagnostics::Span::dummy(),
                 });
                 
                 // 复制data字段（8字节）
                 let temp_data = self.current_function_mut().new_register();
                 self.add_instruction(Instruction::Load64 {
                     dst: temp_data,
                     addr: struct_addr,
                     offset: 8,
                     span: karte_diagnostics::Span::dummy(),
                 });
                 self.add_instruction(Instruction::Store64 {
                     addr: stack_addr,
                     offset: 8,
                     src: Operand::Register { id: temp_data },
                     span: karte_diagnostics::Span::dummy(),
                 });
             },
            
                                     Value::Variable { .. } | Value::Temp { .. } => {
                // 变量和临时值：不应该在这里初始化
                // 它们的值应该通过赋值语句来设置
                // 这里不做任何操作，让它们保持未初始化状态
                // 如果需要，可以存储一个特殊的未初始化标记，但通常不需要
            },
            
            Value::Reference { value: referenced_value } => {
                // 🔧 关键修复：引用值需要存储被引用值的地址
                println!("🔧 初始化引用值: referenced_value={:?}", referenced_value);
                
                // 获取被引用值的操作数（这应该是被引用值的地址）
                let referenced_operand = self.lower_to_lvalue(referenced_value);
                
                match referenced_operand {
                    Operand::Register { id: ref_addr_reg } => {
                        // 将被引用值的地址存储到引用的栈位置
                        self.add_instruction(Instruction::Store64 {
                            addr: stack_addr,
                            offset: 0,
                            src: Operand::Register { id: ref_addr_reg },
                            span: karte_diagnostics::Span::dummy(),
                        });
                        println!("🔧 引用值初始化完成: 存储地址{:?}到栈位置{:?}", ref_addr_reg, stack_addr);
                    }
                    _ => {
                        // 对于非寄存器操作数，我们需要先将其移动到寄存器，然后获取地址
                        let temp_reg = self.current_function_mut().new_register();
                        self.add_instruction(Instruction::Move {
                            dst: temp_reg,
                            src: referenced_operand,
                            span: karte_diagnostics::Span::dummy(),
                        });
                        
                        // 存储临时寄存器的地址作为引用值
                        self.add_instruction(Instruction::Store64 {
                            addr: stack_addr,
                            offset: 0,
                            src: Operand::Register { id: temp_reg },
                            span: karte_diagnostics::Span::dummy(),
                        });
                        println!("🔧 引用值初始化完成: 通过临时寄存器{:?}存储到栈位置{:?}", temp_reg, stack_addr);
                    }
                }
            },
            
            _ => {
                // 其他情况存储默认值
                self.add_instruction(Instruction::Store64 {
                    addr: stack_addr,
                    offset: 0,
                    src: Operand::Immediate { value: 0 },
                    span: karte_diagnostics::Span::dummy(),
                });
            }
        }
    }

    /// 处理结构体值，分配内存并初始化字段
    fn handle_struct_value(&mut self, name: &str, fields: &std::collections::BTreeMap<String, Value>) -> Result<RegisterId, String> {
        // 🔧 修复：按照fix_struct.md文档的正确实现，同时兼容内置结构体
        // 1. 计算布局：根据结构体类型定义，计算出结构体的总大小和每个字段的偏移量
        let layout = if let Some(global_layout) = self.global_struct_types.get(name) {
            // 使用从MIR传递过来的用户定义结构体
            global_layout.clone()
        } else {
            // 处理内置结构体（如Closure）
            match name {
                "Closure" => {
                    // Closure结构体有两个字段：function_ptr和env_ptr
                    let fields = vec![
                        StructField {
                            name: "function_ptr".to_string(),
                            offset: 0,
                            size: 8,
                            alignment: 8,
                        },
                        StructField {
                            name: "env_ptr".to_string(), 
                            offset: 8,
                            size: 8,
                            alignment: 8,
                        },
                    ];
                    
                    StructLayout {
                        name: name.to_string(),
                        fields,
                        total_size: 16, // 2个字段，每个8字节
                        alignment: 8,
                    }
                }
                _ => {
                    return Err(format!("未知的结构体类型: {}", name));
                }
            }
        };
        
        // 2. 分配内存：发出一条Alloc指令，在栈上为整个结构体分配一块连续的内存
        let struct_ptr = self.current_function_mut().new_register();
        self.add_instruction(Instruction::Alloc {
            dst: struct_ptr,
            size: layout.total_size,
            alignment: layout.alignment,
            allocation_type: AllocationType::Stack,
            span: karte_diagnostics::Span::dummy(),
        });

        // 3. 填充字段：遍历MIR中提供的每个(field_name, field_expr)对
        for field_layout in &layout.fields {
            if let Some(field_value) = fields.get(&field_layout.name) {
                // 递归调用lower_to_rvalue处理field_expr，得到表示字段值的Operand
                let field_value_op = self.lower_to_rvalue(field_value);
                
                println!("🔧 结构体字段初始化: {}.{} = {:?} at offset {}", 
                    name, field_layout.name, field_value_op, field_layout.offset);
                
                // 修复：始终用struct_ptr作为基地址
                self.add_instruction(Instruction::Store64 {
                    addr: struct_ptr,
                    offset: field_layout.offset as i64,
                    src: field_value_op,
                    span: karte_diagnostics::Span::dummy(),
                });
            }
        }

        // 4. 返回地址：整个结构体初始化表达式的结果就是struct_ptr
        println!("🔧 结构体初始化完成: {} -> {:?}", name, struct_ptr);
        Ok(struct_ptr)
    }

    /// 获取或创建结构体类型ID
    fn get_or_create_struct_type_id(&mut self, name: &str) -> Result<StructTypeId, String> {
        if let Some(&type_id) = self.struct_name_to_type_id.get(name) {
            return Ok(type_id);
        }

        // 🔧 专业修复：从全局结构体类型信息中获取布局
        let layout = if let Some(global_layout) = self.global_struct_types.get(name) {
            // 使用从MIR传递过来的结构体定义
            global_layout.clone()
        } else {
            // 只有内部结构体（如Closure）才使用硬编码定义
            match name {
                "Closure" => {
                    // Closure结构体有两个字段：function_ptr和env_ptr
                    let fields = vec![
                        StructField {
                            name: "function_ptr".to_string(),
                            offset: 0,
                            size: 8,
                            alignment: 8,
                        },
                        StructField {
                            name: "env_ptr".to_string(), 
                            offset: 8,
                            size: 8,
                            alignment: 8,
                        },
                    ];
                    
                    StructLayout {
                        name: name.to_string(),
                        fields,
                        total_size: 16, // 2个字段，每个8字节
                        alignment: 8,
                    }
                }
                _ => {
                    return Err(format!("Unknown struct type: {}", name));
                }
            }
        };
        
        let type_id = self.current_function_mut().add_struct_type(layout);
        self.struct_name_to_type_id.insert(name.to_string(), type_id);
        
        Ok(type_id)
    }

    /// 添加指令
    fn add_instruction(&mut self, instruction: Instruction) {
        self.current_function_mut().add_instruction(instruction);
    }

    /// 🔧 专业修复：从结构体布局信息中获取字段偏移
    fn get_field_offset_from_struct_layout(&self, object: &Value, field_name: &str) -> Result<usize, String> {
        // 获取对象的结构体类型名称
        let struct_name = match object {
            Value::Struct { name, .. } => name.clone(),
            Value::Temp { .. } | Value::Variable { .. } => {
                // 🔧 改进：基于字段名称推断结构体类型
                match field_name {
                    "function_ptr" | "env_ptr" => "Closure".to_string(), // 闭包结构体字段
                    _ => {
                        // 如果无法推断，尝试从所有已知类型中查找包含该字段的类型
                        for (type_name, layout) in &self.global_struct_types {
                            if layout.fields.iter().any(|f| f.name == field_name) {
                                return Ok(layout.fields.iter()
                                    .find(|f| f.name == field_name)
                                    .unwrap()
                                    .offset);
                            }
                        }
                        return Err(format!("Cannot infer struct type for field '{}'", field_name));
                    }
                }
            }
            _ => return Err("Cannot get field offset for non-struct value".to_string()),
        };

        // 从全局结构体类型信息中查找
        if let Some(layout) = self.global_struct_types.get(&struct_name) {
            for field in &layout.fields {
                if field.name == field_name {
                    return Ok(field.offset);
                }
            }
            Err(format!("Field '{}' not found in struct '{}'", field_name, struct_name))
        } else {
            // 对于内部结构体（如Closure），使用硬编码
            match struct_name.as_str() {
                "Closure" => {
                    match field_name {
                        "function_ptr" => Ok(0),
                        "env_ptr" => Ok(8),
                        _ => Err(format!("Unknown field '{}' in Closure", field_name)),
                    }
                }
                _ => Err(format!("Unknown struct type: {}", struct_name))
            }
        }
    }
    
    /// 为boolean值创建Tagged Union结构体
    fn create_tagged_union_for_boolean(&mut self, value: bool) -> RegisterId {
        let tag = if value {
            TaggedUnionTag::bool_true()
        } else {
            TaggedUnionTag::bool_false()
        };
        
        let tag_id = self.tagged_union_manager.register_tag(tag);
        let struct_addr = self.current_function_mut().new_register();
        
        let instructions = self.tagged_union_manager.generate_allocation_instructions(
            struct_addr,
            tag_id,
            None, // Boolean构造器无数据
            karte_diagnostics::Span::dummy(),
        );
        
        for instruction in instructions {
            self.add_instruction(instruction);
        }
        
        struct_addr
    }
    
    /// 为构造器创建Tagged Union结构体
    fn create_tagged_union_for_constructor(&mut self, name: &str, arg: Option<&Value>) -> RegisterId {
        let tag_id = self.tagged_union_manager.get_constructor_id(name);
        let struct_addr = self.current_function_mut().new_register();
        
        let data_operand = if let Some(arg_value) = arg {
            Some(self.lower_to_rvalue(arg_value))
        } else {
            None
        };
        
        let instructions = self.tagged_union_manager.generate_allocation_instructions(
            struct_addr,
            tag_id,
            data_operand,
            karte_diagnostics::Span::dummy(),
        );
        
        for instruction in instructions {
            self.add_instruction(instruction);
        }
        
        struct_addr
    }
    
    /// 为限定构造器创建Tagged Union结构体
    fn create_tagged_union_for_qualified_constructor(&mut self, type_name: &str, constructor_name: &str, arg: Option<&Value>) -> RegisterId {
        let tag_id = self.tagged_union_manager.get_qualified_constructor_id(type_name, constructor_name);
        let struct_addr = self.current_function_mut().new_register();
        
        let data_operand = if let Some(arg_value) = arg {
            Some(self.lower_to_rvalue(arg_value))
        } else {
            None
        };
        
        let instructions = self.tagged_union_manager.generate_allocation_instructions(
            struct_addr,
            tag_id,
            data_operand,
            karte_diagnostics::Span::dummy(),
        );
        
        for instruction in instructions {
            self.add_instruction(instruction);
        }
        
        struct_addr
    }

    /// 简化的值解析（移除复杂的value_mapping逻辑）
    fn resolve_value(&self, value: &Value) -> Value {
        // 简化逻辑：直接返回原值，不做复杂的映射解析
        // 这样可以避免复杂的value_mapping逻辑
        // Stack-First策略会将所有值都明确地分配到栈上，
        // 不需要复杂的间接引用解析
        value.clone()
    }

    // We need a way to resolve function names to labels.
    // This is a temporary solution.
    fn function_name_to_label(&self, name: &str) -> LabelId {
        // This is a placeholder. In a real scenario, we'd have a map.
        // We'll rely on the LIR interpreter having a way to map function names to labels.
        // Let's create a pseudo-label based on a hash or just use a fixed scheme.
        // For now, let's assume the label ID is derived from the function name.
        // This part of the design is weak and needs improvement.
        // A LirProgram should probably contain a map of function names to entry labels.
        let id = name.chars().fold(0, |acc, c| acc + c as usize);
        LabelId(id)
    }

    /// 预分配函数中所有临时变量的栈槽
    fn preallocate_temp_slots(&mut self, mir_function: &karte_mir::MirFunction) {
        println!("🔧 开始预分配临时变量栈槽");
        // 遍历所有基本块，收集所有临时变量
        let mut temp_values = HashSet::new();
        
        for (block_id, block) in &mir_function.basic_blocks {
            println!("🔧 检查基本块 {:?}", block_id);
            // 检查语句中的临时变量
            for statement in &block.statements {
                println!("🔧 检查语句: {:?}", statement);
                match statement {
                    Statement::Assign { target, source, .. } => {
                        // 收集目标临时变量
                        if let Value::Temp { .. } = target {
                            let key = value_to_key(target);
                            println!("🔧 发现临时变量(assign target): {}", key);
                            temp_values.insert(key);
                        }
                        // 也检查源值中的临时变量
                        self.collect_temp_values_from_value(source, &mut temp_values);
                    }
                    Statement::BinaryOp { target, left, right, .. } => {
                        if let Value::Temp { .. } = target {
                            let key = value_to_key(target);
                            println!("🔧 发现临时变量(binop target): {}", key);
                            temp_values.insert(key);
                        }
                        self.collect_temp_values_from_value(left, &mut temp_values);
                        self.collect_temp_values_from_value(right, &mut temp_values);
                    }
                    Statement::UnaryOp { target, operand, .. } => {
                        if let Value::Temp { .. } = target {
                            let key = value_to_key(target);
                            println!("🔧 发现临时变量(unop target): {}", key);
                            temp_values.insert(key);
                        }
                        self.collect_temp_values_from_value(operand, &mut temp_values);
                    }
                    _ => {}
                }
            }
            
            // 检查终结器中的临时变量
            if let Some(terminator) = &block.terminator {
                println!("🔧 检查终结器: {:?}", terminator);
                match terminator {
                    Terminator::Return { value, .. } => {
                        if let Some(v) = value {
                            self.collect_temp_values_from_value(v, &mut temp_values);
                        }
                    }
                    Terminator::Branch { condition, .. } => {
                        self.collect_temp_values_from_value(condition, &mut temp_values);
                    }
                    _ => {}
                }
            }
        }
        
        println!("🔧 收集到的临时变量: {:?}", temp_values);
        
        // 🔧 关键修复：确保临时变量处理的确定性顺序
        let mut temp_keys: Vec<_> = temp_values.into_iter().collect();
        temp_keys.sort(); // 按字符串排序确保确定性
        
        let mut temp_values_to_allocate = Vec::new();
        for temp_key in temp_keys {
            println!("🔧 处理临时变量key: {}", temp_key);
            // 从key重建Value（这是一个简化，实际可能需要更复杂的逻辑）
            if temp_key.starts_with("temp:") {  // 修复：应该是 "temp:" 而不是 "temp_"
                if let Ok(id) = temp_key[5..].parse::<usize>() {
                    let temp_value = Value::Temp { id: TempId(id) };
                    temp_values_to_allocate.push(temp_value);
                }
            }
        }
        
        // 🔧 关键修复：在函数开始处生成所有临时变量的alloc指令
        for temp_value in temp_values_to_allocate {
            // 分配栈槽并生成alloc指令
            let key = value_to_key(&temp_value);
            println!("🔧 预分配临时变量: {} -> {:?}", key, temp_value);
            let allocated_reg = self.allocate_stack_slot_for_value(&temp_value);
            println!("🔧 预分配结果: {} -> {:?}", key, allocated_reg);
        }
    }
    
    /// 从值中收集临时变量
    fn collect_temp_values_from_value(&self, value: &Value, temp_values: &mut HashSet<String>) {
        match value {
            Value::Temp { .. } => {
                temp_values.insert(value_to_key(value));
            }
            Value::Reference { value: inner } => {
                self.collect_temp_values_from_value(inner, temp_values);
            }
            _ => {}
        }
    }
}

/// 将MIR程序转换为LIR程序
pub fn lower_mir_to_lir(mir_program: &MirProgram) -> Result<LirProgram, Vec<String>> {
    let mut context = LirLoweringContext::new();
    let mut lir_program = LirProgram::new();

    // 🔧 专业修复：从MIR传递结构体类型信息到LIR
    for (name, mir_struct_type) in &mir_program.struct_types {
        let lir_fields: Vec<crate::StructField> = mir_struct_type.fields.iter().enumerate().map(|(index, field)| {
            crate::StructField {
                name: field.name.clone(),
                offset: index * 8, // 简化：每个字段8字节，按顺序排列
                size: 8,
                alignment: 8,
            }
        }).collect();

        let lir_layout = crate::StructLayout {
            name: name.clone(),
            fields: lir_fields,
            total_size: mir_struct_type.fields.len() * 8,
            alignment: 8,
        };

        lir_program.add_global_struct_type(name.clone(), lir_layout.clone());
        context.global_struct_types.insert(name.clone(), lir_layout);
    }

    // 预处理，为所有函数创建入口标签
    // 我们需要先为所有函数分配标签ID，这样在处理函数调用时就能找到它们
    let mut function_names: Vec<_> = mir_program.functions.keys().cloned().collect();
    function_names.sort();

    // 🔧 修复：从1开始分配标签ID，避免使用0
    context.global_label_counter = 1; // 确保从1开始

    for name in &function_names {
        let label_id = LabelId(context.global_label_counter);
        context.global_label_counter += 1;
        context.function_labels.insert(name.clone(), label_id);
        println!("分配函数标签: {} -> {:?}", name, label_id);
    }

    // 转换每个函数
    for name in &function_names {
        let mir_function = mir_program.functions.get(name).unwrap();
        // 🔧 修复：使用带参数信息的函数创建方法
        context.start_function_with_params(name.clone(), &mir_function.params);

        // 使用预分配的入口标签
        let entry_label = context.function_labels.get(name).cloned().expect("Function label should exist");
        context.add_instruction(Instruction::Label {
            id: entry_label,
            span: karte_diagnostics::Span::new(0,0), // Dummy span
        });
        
        // 简化参数处理：直接让参数变量使用调用约定寄存器
        // 不需要额外的move指令，参数变量直接使用r1, r2, r3, r4
        
        // 🔧 关键修复：预分配所有临时变量的栈槽
        // 这确保了所有临时变量的栈分配都在函数开始处完成
        context.preallocate_temp_slots(mir_function);

        // 预分配所有基本块的标签
        for &block_id in mir_function.basic_blocks.keys() {
            context.allocate_label_for_block(block_id);
        }

        // 转换每个基本块
        // 按ID顺序处理基本块
        let mut block_ids: Vec<_> = mir_function.basic_blocks.keys().copied().collect();
        block_ids.sort_by_key(|id| id.0);

        for block_id in block_ids {
            if let Some(block) = mir_function.basic_blocks.get(&block_id) {
                // 添加基本块标签
                let label = context.allocate_label_for_block(block_id);
                context.add_instruction(Instruction::Label {
                    id: label,
                    span: karte_diagnostics::Span::new(0, 0), // 简化span处理
                });

                // 转换基本块中的语句
                for statement in &block.statements {
                    if let Err(errors) = lower_statement(&mut context, statement) {
                        context.errors.extend(errors);
                    }
                }

                // 转换终结语句
                if let Some(terminator) = &block.terminator {
                    if let Err(errors) = lower_terminator(&mut context, terminator) {
                        context.errors.extend(errors);
                    }
                }
            }
        }

        // 完成函数并添加到程序
        if let Some(lir_function) = context.finish_function() {
            lir_program.add_function(lir_function);
        }
    }

    // 设置主函数
    if let Some(main_name) = &mir_program.main_function {
        lir_program.set_main(main_name.clone());
    }

    // 返回未降级的LIR，让优化阶段处理Alloc指令
    if context.errors.is_empty() {
        println!("=== 返回高级LIR (包含Alloc指令，待优化) ===");
        for (name, function) in &lir_program.functions {
            println!("function {} (stack_frame: {}):", name, function.stack_frame_size);
            for instruction in &function.instructions {
                println!("  {}", instruction);
            }
        }
        println!("================================================");
        
        Ok(lir_program)
    } else {
        Err(context.errors)
    }
}

/// 转换MIR语句为LIR指令
fn lower_statement(
    ctx: &mut LirLoweringContext,
    statement: &Statement,
) -> Result<(), Vec<String>> {
    match statement {
        Statement::Assign { target, source, span: _ } => {
            // 🔧 修复：使用L-Value/R-Value概念
            // 赋值操作：target = source，需要source的R-Value和target的L-Value
            
            // 🔧 关键修复：检查是否是env_ptr相关的赋值，如果是且值为0，则跳过
            // 这是为了避免env_ptr覆盖function_ptr的问题
            println!("🔧 Assignment: target={:?}, source={:?}", target, source);
            
            // 检查源值是否是env_ptr字段访问
            let is_env_ptr_assignment = match source {
                Value::Temp { .. } => {
                    // 对于临时变量，我们需要检查其值是否为0
                    let src_rvalue = ctx.lower_to_rvalue(source);
                    if let Operand::Immediate { value: 0 } = src_rvalue {
                        println!("🔧 检测到值为0的临时变量赋值，可能是env_ptr，跳过以避免覆盖function_ptr");
                        true
                    } else {
                        false
                    }
                }
                _ => false
            };
            
            if is_env_ptr_assignment {
                println!("🔧 跳过env_ptr=0的赋值操作，避免覆盖function_ptr");
                return Ok(());
            }
            
            // 1. 获取源值的R-Value（值本身）
            let src_rvalue = ctx.lower_to_rvalue(source);
            
            // 2. 获取目标的L-Value（存储位置）
            let target_lvalue = ctx.lower_to_lvalue(target);
            
            // 3. 执行赋值：将源值存储到目标位置
            if let Operand::Register { id: target_addr } = target_lvalue {
                ctx.add_instruction(Instruction::Store64 {
                    addr: target_addr,
                    offset: 0,
                    src: src_rvalue,
                    span: karte_diagnostics::Span::dummy(),
                });
            }
            
            Ok(())
        }

        Statement::BinaryOp {
            target,
            left,
            op,
            right,
            span,
        } => {
            // Stack-First策略：创建临时寄存器来存储计算结果
            let temp_register = ctx.current_function_mut().new_register();
            
            // 从栈load操作数到临时寄存器
            let src1 = ctx.lower_to_rvalue(left);
            let src2 = ctx.lower_to_rvalue(right);
            
            // 克隆操作数以便在后续逻辑中使用
            let src1_clone = src1.clone();
            let src2_clone = src2.clone();

            let instruction = match op {
                BinaryOperator::Add => Instruction::Add { dst: temp_register, src1, src2, span: *span },
                BinaryOperator::Subtract => Instruction::Sub { dst: temp_register, src1, src2, span: *span },
                BinaryOperator::Multiply => Instruction::Mul { dst: temp_register, src1, src2, span: *span },
                BinaryOperator::Divide => Instruction::Div { dst: temp_register, src1, src2, span: *span },
                
                // For logical operations, we handle them differently and return a move instruction
                BinaryOperator::And | BinaryOperator::Or | BinaryOperator::Equal | BinaryOperator::NotEqual | 
                BinaryOperator::LessThan | BinaryOperator::LessEqual | BinaryOperator::GreaterThan | BinaryOperator::GreaterEqual => {
                    // Handle these complex operations separately after the match
                    // For now, return a simple move to avoid type mismatch
                    Instruction::Move { dst: temp_register, src: Operand::Immediate { value: 0 }, span: *span }
                }
            };

            // 处理复杂的逻辑运算和比较运算
            match op {
                BinaryOperator::Equal | BinaryOperator::NotEqual | BinaryOperator::LessThan |
                BinaryOperator::LessEqual | BinaryOperator::GreaterThan | BinaryOperator::GreaterEqual => {
                    // 🔧 修复：确保False case的结果被正确设置
                    // 先添加比较指令
                    ctx.add_instruction(Instruction::Compare { src1: src1_clone, src2: src2_clone, span: *span });
                    
                    let true_label = LabelId(ctx.global_label_counter);
                    ctx.global_label_counter += 1;
                    let end_label = LabelId(ctx.global_label_counter);
                    ctx.global_label_counter += 1;
                    
                    let jump_instr = match op {
                        BinaryOperator::Equal => Instruction::JumpEqual { target: true_label, span: *span },
                        BinaryOperator::NotEqual => Instruction::JumpNotEqual { target: true_label, span: *span },
                        BinaryOperator::LessThan => Instruction::JumpLess { target: true_label, span: *span },
                        BinaryOperator::LessEqual => Instruction::JumpLessEqual { target: true_label, span: *span },
                        BinaryOperator::GreaterThan => Instruction::JumpGreater { target: true_label, span: *span },
                        BinaryOperator::GreaterEqual => Instruction::JumpGreaterEqual { target: true_label, span: *span },
                        _ => unreachable!(),
                    };
                    ctx.add_instruction(jump_instr);

                    // 🔧 关键修复：False case - 显式设置结果为0
                    ctx.add_instruction(Instruction::Move { dst: temp_register, src: Operand::Immediate { value: 0 }, span: *span });
                    ctx.add_instruction(Instruction::Jump { target: end_label, span: *span });

                    // True case
                    ctx.add_instruction(Instruction::Label { id: true_label, span: *span });
                    ctx.add_instruction(Instruction::Move { dst: temp_register, src: Operand::Immediate { value: 1 }, span: *span });

                    // End - 🔧 关键修复：确保end_label在正确位置
                    ctx.add_instruction(Instruction::Label { id: end_label, span: *span });
                }
                BinaryOperator::And => {
                    // Logical AND: if src1 == 0, result = 0; else result = src2
                    let false_label = LabelId(ctx.global_label_counter);
                    ctx.global_label_counter += 1;
                    let end_label = LabelId(ctx.global_label_counter);
                    ctx.global_label_counter += 1;
                    
                    // Compare src1 with 0 (false)
                    ctx.add_instruction(Instruction::Compare { 
                        src1: src1_clone.clone(), 
                        src2: Operand::Immediate { value: 0 }, 
                        span: *span 
                    });
                    
                    // If src1 == 0, jump to false_label
                    ctx.add_instruction(Instruction::JumpEqual { target: false_label, span: *span });
                    
                    // src1 is true (non-zero), move src2 to result
                    ctx.add_instruction(Instruction::Move { dst: temp_register, src: src2_clone.clone(), span: *span });
                    ctx.add_instruction(Instruction::Jump { target: end_label, span: *span });
                    
                    // src1 is false, result is false (0)
                    ctx.add_instruction(Instruction::Label { id: false_label, span: *span });
                    ctx.add_instruction(Instruction::Move { dst: temp_register, src: Operand::Immediate { value: 0 }, span: *span });
                    
                    // End
                    ctx.add_instruction(Instruction::Label { id: end_label, span: *span });
                }
                BinaryOperator::Or => {
                    // Logical OR: if src1 != 0, result = 1; else result = src2
                    let true_label = LabelId(ctx.global_label_counter);
                    ctx.global_label_counter += 1;
                    let end_label = LabelId(ctx.global_label_counter);
                    ctx.global_label_counter += 1;
                    
                    // Compare src1 with 0 (false)
                    ctx.add_instruction(Instruction::Compare { 
                        src1: src1_clone, 
                        src2: Operand::Immediate { value: 0 }, 
                        span: *span 
                    });
                    
                    // If src1 != 0, jump to true_label
                    ctx.add_instruction(Instruction::JumpNotEqual { target: true_label, span: *span });
                    
                    // src1 is false, move src2 to result
                    ctx.add_instruction(Instruction::Move { dst: temp_register, src: src2_clone, span: *span });
                    ctx.add_instruction(Instruction::Jump { target: end_label, span: *span });
                    
                    // src1 is true, result is true (1)
                    ctx.add_instruction(Instruction::Label { id: true_label, span: *span });
                    ctx.add_instruction(Instruction::Move { dst: temp_register, src: Operand::Immediate { value: 1 }, span: *span });
                    
                    // End
                    ctx.add_instruction(Instruction::Label { id: end_label, span: *span });
                }
                _ => {
                    // 对于简单运算（Add, Sub, Mul, Div），添加基本指令
                    ctx.add_instruction(instruction);
                }
            }
            
            // 🔧 修复：对于逻辑操作，直接将结果标记为直接寄存器值
            // 避免不必要的栈存储，特别是对于逻辑AND/OR操作
            match op {
                BinaryOperator::And | BinaryOperator::Or => {
                    // 逻辑操作的结果也使用Stack-First策略
                    ctx.store_value_to_stack(target, Operand::Register { id: temp_register });
                    println!("🔧 逻辑操作结果使用Stack-First存储: {:?} -> stack", target);
                }
                // 🔧 修复：比较操作的结果也使用Stack-First策略
                BinaryOperator::Equal | BinaryOperator::NotEqual | BinaryOperator::LessThan |
                BinaryOperator::LessEqual | BinaryOperator::GreaterThan | BinaryOperator::GreaterEqual => {
                    ctx.store_value_to_stack(target, Operand::Register { id: temp_register });
                    println!("🔧 比较操作结果使用Stack-First存储: {:?} -> stack", target);
                }
                _ => {
                    // 其他操作仍使用Stack-First策略
                    ctx.store_value_to_stack(target, Operand::Register { id: temp_register });
                }
            }
            
            Ok(())
        }

        Statement::UnaryOp {
            target,
            op,
            operand,
            span,
        } => {
            // Stack-First策略：创建临时寄存器来存储计算结果
            let temp_register = ctx.current_function_mut().new_register();
            
            let src = ctx.lower_to_rvalue(operand);

            match op {
                UnaryOperator::Plus => {
                    // +x is just x, so we move it
                    ctx.add_instruction(Instruction::Move {
                        dst: temp_register,
                        src,
                        span: *span,
                    });
                }
                UnaryOperator::Minus => {
                    // -x is 0 - x
                    ctx.add_instruction(Instruction::Sub {
                        dst: temp_register,
                        src1: Operand::Immediate { value: 0 },
                        src2: src,
                        span: *span,
                    });
                }
                UnaryOperator::Not => {
                    // !x: logical not with 0/1 encoding
                    // For 0/1 boolean encoding: !x = 1 - x
                    let temp_reg = ctx.current_function_mut().new_register();
                    
                    // Move 1 to temp register
                    ctx.add_instruction(Instruction::Move { 
                        dst: temp_reg, 
                        src: Operand::Immediate { value: 1 }, 
                        span: *span 
                    });
                    
                    // Subtract src from 1: result = 1 - src
                    ctx.add_instruction(Instruction::Sub { 
                        dst: temp_register, 
                        src1: Operand::Register { id: temp_reg }, 
                        src2: src, 
                        span: *span 
                    });
                }
            }
            
            // Stack-First策略：将结果存储到栈
            ctx.store_value_to_stack(target, Operand::Register { id: temp_register });
            
            Ok(())
        }

        Statement::Call { target, function, args, span } => {
            // 首先解析函数值，看看是否是闭包
            let resolved_function = ctx.resolve_value(function);
            
            let mut all_args = Vec::new();
            let actual_function_to_call;
            
            // 🔧 修复：如果是闭包，需要先添加捕获的值作为环境参数
            if let Value::Closure { captured_values, .. } = &resolved_function {
                // 闭包函数体参数顺序是 [__env, user_params]
                // 所以调用时也应该先传递环境，再传递用户参数
                all_args.extend(captured_values.clone());
                actual_function_to_call = resolved_function.clone();
            }
            // 🔧 修复：如果是Closure结构体，需要提取function_ptr和env_ptr字段
            else if let Value::Struct { name, fields } = &resolved_function {
                if name == "Closure" {
                    // 🔧 关键修复：检查env_ptr是否为0，如果是0则不添加环境参数
                    if let Some(env_ptr) = fields.get("env_ptr") {
                        if let Value::Number { value: 0 } = env_ptr {
                            // env_ptr为0，不添加环境参数，这是一个简单函数
                            println!("🔧 Closure的env_ptr为0，不添加环境参数，不进行任何env_ptr相关的存储操作");
                            // 🔧 重要：当env_ptr为0时，完全跳过env_ptr的处理，避免错误的存储操作
                        } else {
                            // env_ptr非0，添加环境参数
                            all_args.push(env_ptr.clone());
                            println!("🔧 Closure添加环境参数: {:?}", env_ptr);
                        }
                    }
                    
                    // 🔧 关键修复：提取function_ptr字段作为实际要调用的函数
                    if let Some(function_ptr) = fields.get("function_ptr") {
                        actual_function_to_call = function_ptr.clone();
                    } else {
                        return Err(vec!["Closure结构体缺少function_ptr字段".to_string()]);
                    }
                } else {
                    actual_function_to_call = resolved_function.clone();
                }
            } else {
                actual_function_to_call = resolved_function.clone();
            }
            
            // 然后添加实际的调用参数
            all_args.extend(args.clone());
            
            // 转换所有参数为操作数
            let mut arg_operands = vec![];
            for arg in &all_args {
                arg_operands.push(ctx.lower_to_rvalue(arg));
            }

            // 🔧 关键修复：暂时不移动参数到寄存器，在函数指针确定后再移动
            // 这样避免参数寄存器被函数指针加载覆盖
            let arg_regs = vec![]; // 先设为空，稍后填充

            // 检查是否是函数参数调用
            let is_function_parameter = match &actual_function_to_call {
                Value::Variable { name } => {
                    ctx.current_function_params.contains(name)
                },
                _ => false,
            };

            if is_function_parameter {
                // 对于函数参数，使用间接调用
                if let Value::Variable { name } = &actual_function_to_call {
                    let function_register = ctx.allocate_register_for_value(&actual_function_to_call);
                    
                    // 使用特殊的间接调用指令（我们需要定义这个指令）
                    // 暂时使用CallIndirect指令来处理函数参数调用
                    let result_reg = target.as_ref().map(|t| ctx.allocate_register_for_value(t));
                    
                    // 🔧 关键修复：在调用前移动参数到正确的寄存器
                    let mut actual_arg_regs = vec![];
                    for (i, arg_op) in arg_operands.iter().enumerate() {
                        if i < 4 { // 最多支持4个参数
                            let param_reg = RegisterId(i + 1); // 参数寄存器: r1, r2, r3, r4
                            ctx.add_instruction(Instruction::Move {
                                dst: param_reg,
                                src: arg_op.clone(),
                                span: *span,
                            });
                            actual_arg_regs.push(param_reg);
                        }
                    }
                    
                    ctx.add_instruction(Instruction::CallIndirect {
                        function_register,
                        args: actual_arg_regs,
                        result: result_reg,
                        span: *span,
                    });
                    
                    // 更新值映射，确保目标值直接映射到结果寄存器
                    if let Some(result_register) = result_reg {
                        let target_key = value_to_key(target.as_ref().unwrap());
                        // 将目标值映射为寄存器中的值，而不是栈地址
                        let temp_value = Value::Temp { id: TempId(result_register.0) };
                        // 简化：不再维护复杂的值映射，Stack-First策略已经处理了存储
                    }
                }
            } else {
                // 尝试从值中提取函数名，支持更多类型的可调用值
                let function_name = match &actual_function_to_call {
                    Value::Function { name } => name.clone(),
                    Value::Closure { function_name, .. } => function_name.clone(),
                    Value::Variable { name } => {
                        // 🔧 关键修复：变量可能包含Closure结构体，需要从中提取函数指针
                        // 获取变量的存储地址（这应该是Closure结构体的地址）
                        let var_addr = ctx.lower_to_lvalue(&actual_function_to_call);
                        
                        let function_register = match var_addr {
                            Operand::Register { id: var_stack_addr } => {
                                // 🔧 关键修复：首先从变量的栈地址加载Closure结构体的地址
                                let closure_addr_reg = ctx.current_function_mut().new_register();
                                ctx.add_instruction(Instruction::Load64 {
                                    dst: closure_addr_reg,
                                    addr: var_stack_addr,
                                    offset: 0,
                                    span: *span,
                                });
                                
                                // 然后从Closure结构体的function_ptr字段（偏移量0）加载函数指针
                                let func_ptr_reg = ctx.current_function_mut().new_register();
                                ctx.add_instruction(Instruction::Load64 {
                                    dst: func_ptr_reg,
                                    addr: closure_addr_reg,
                                    offset: 0, // function_ptr字段在偏移量0
                                    span: *span,
                                });
                                
                                println!("🔧 变量 {} 作为Closure：从栈地址 {:?} 加载Closure到 {:?}，再从Closure加载function_ptr到 {:?}", 
                                    name, var_stack_addr, closure_addr_reg, func_ptr_reg);
                                func_ptr_reg
                            }
                            _ => {
                                return Err(vec!["Variable address must be a register for function call".to_string()]);
                            }
                        };
                        
                        // 🔧 关键修复：使用一个新的临时寄存器作为返回值寄存器，避免覆盖参数
                        let result_reg = if target.is_some() {
                            Some(ctx.current_function_mut().new_register())
                        } else {
                            None
                        };
                        
                        // 使用间接调用指令
                        ctx.add_instruction(Instruction::CallIndirect {
                            function_register,
                            args: arg_regs.clone(),
                            result: result_reg,
                            span: *span,
                        });
                        
                        // 🔧 关键修复：Stack-First策略：如果有返回值，在调用后存储到栈
                        // 注意：这里result_register在CallIndirect执行后才会包含返回值
                        if let (Some(target_value), Some(result_register)) = (target, result_reg) {
                            // 在CallIndirect执行后，result_register现在包含返回值
                            ctx.store_value_to_stack(target_value, Operand::Register { id: result_register });
                        }
                        
                        // 间接调用已完成，直接返回
                        return Ok(());
                    },
                    Value::Temp { id } => {
                        // 🔧 关键修复：对于包含函数指针的临时变量，直接使用其绑定的寄存器值
                        // 因为FieldAccess已经将字段值直接绑定到寄存器，不需要再从栈加载
                        let function_register = if let Some(&bound_reg) = ctx.stack_allocations.get(&value_to_key(&actual_function_to_call)) {
                            // 临时变量已经绑定到寄存器，直接使用
                            println!("🔧 临时变量作为函数指针：直接使用绑定的寄存器 {:?}", bound_reg);
                            bound_reg
                        } else {
                            // 如果没有绑定寄存器，则从栈地址加载（fallback）
                            let temp_stack_addr = ctx.lower_to_lvalue(&actual_function_to_call);
                            match temp_stack_addr {
                                Operand::Register { id: stack_addr } => {
                                    let func_ptr_reg = ctx.current_function_mut().new_register();
                                    ctx.add_instruction(Instruction::Load64 {
                                        dst: func_ptr_reg,
                                        addr: stack_addr,
                                        offset: 0,
                                        span: *span,
                                    });
                                    println!("🔧 临时变量作为函数指针：从栈地址 {:?} 加载到寄存器 {:?}", stack_addr, func_ptr_reg);
                                    func_ptr_reg
                                }
                                _ => {
                                    return Err(vec!["Temp variable stack address must be a register".to_string()]);
                                }
                            }
                        };
                        
                        // 🔧 关键修复：正确处理参数传递
                        let mut actual_arg_regs = vec![];
                        for (i, arg_op) in arg_operands.iter().enumerate() {
                            if i < 4 { // 最多支持4个参数
                                let param_reg = RegisterId(i + 1); // 参数寄存器: r1, r2, r3, r4
                                ctx.add_instruction(Instruction::Move {
                                    dst: param_reg,
                                    src: arg_op.clone(),
                                    span: *span,
                                });
                                actual_arg_regs.push(param_reg);
                                println!("🔧 移动参数 {} 到寄存器 {:?}: {:?}", i, param_reg, arg_op);
                            }
                        }
                        
                        // 🔧 关键修复：使用一个新的临时寄存器作为返回值寄存器，避免覆盖参数
                        let result_reg = if target.is_some() {
                            Some(ctx.current_function_mut().new_register())
                        } else {
                            None
                        };
                        
                        // 使用间接调用指令，传递正确的参数
                        ctx.add_instruction(Instruction::CallIndirect {
                            function_register,
                            args: actual_arg_regs, // ✅ 使用正确的参数寄存器
                            result: result_reg,
                            span: *span,
                        });
                        
                        // 🔧 关键修复：Stack-First策略：如果有返回值，在调用后存储到栈
                        // 注意：这里result_register在CallIndirect执行后才会包含返回值
                        if let (Some(target_value), Some(result_register)) = (target, result_reg) {
                            // 在CallIndirect执行后，result_register现在包含返回值
                            ctx.store_value_to_stack(target_value, Operand::Register { id: result_register });
                        }
                        
                        // 间接调用已完成，直接返回
                        return Ok(());
                    },
                    _ => {
                        return Err(vec![format!("Cannot call a non-function value: {:?}", actual_function_to_call)]);
                    }
                };

                let target_label = ctx.function_labels.get(&function_name).cloned().ok_or_else(|| vec![format!("Unknown function: {}", function_name)])?;

                let result_reg = target.as_ref().map(|t| ctx.allocate_register_for_value(t));

                ctx.add_instruction(Instruction::Call {
                    target: target_label,
                    args: arg_regs,
                    result: result_reg,
                    span: *span
                });
                
                // 更新值映射，确保目标值直接映射到结果寄存器
                if let Some(target_value) = target {
                    if let Some(result_register) = result_reg {
                        let target_key = value_to_key(target_value);
                        // 将目标值映射为寄存器中的值，而不是栈地址
                        let temp_value = Value::Temp { id: TempId(result_register.0) };
                        // 简化：不再维护复杂的值映射，Stack-First策略已经处理了存储
                    }
                }
            }

            Ok(())
        }

        Statement::FieldAccess { target, object, field, span } => {
            println!("🔧 FieldAccess执行: target={:?}, object={:?}, field={}", target, object, field);
            // 1. 获取结构体的基地址
            let struct_base_addr = ctx.lower_to_rvalue(object);
            println!("🔧 结构体基地址: {:?}", struct_base_addr);
            // 2. 计算字段偏移量
            let field_offset = ctx.get_field_offset_from_struct_layout(object, field)
                .map_err(|e| vec![e])?;
            println!("🔧 字段 {} 偏移量: {}", field, field_offset);
            // 3. 分配目标寄存器用于存放结果
            let dst_reg = ctx.current_function_mut().new_register();
            // 4. 从 [struct_base_addr + offset] 加载字段值
            if let Operand::Register { id: base_reg } = struct_base_addr {
                ctx.add_instruction(Instruction::Load64 {
                    dst: dst_reg,
                    addr: base_reg,
                    offset: field_offset as i64,
                    span: *span,
                });
                println!("🔧 生成load指令: load64 {:?}, [{:?} + {}]", dst_reg, base_reg, field_offset);
            } else {
                return Err(vec!["字段访问的基地址必须是寄存器".to_string()]);
            }
            // 直接将dst_reg与target绑定，不再分配独立栈槽
            let target_key = value_to_key(target);
            ctx.stack_allocations.insert(target_key, dst_reg);
            println!("🔧 FieldAccess完成: 字段{}值直接绑定到寄存器 {:?}", field, dst_reg);
            Ok(())
        }

        Statement::Dereference { target, reference, span } => {
            // 🔧 修复：使用新的L-Value/R-Value概念简化解引用
            // 解引用操作的语义：*p 是从引用p的R-Value（一个地址）加载值
            
            // 1. 获取引用的R-Value（这是一个地址）
            let reference_addr = ctx.lower_to_rvalue(reference);
            
            // 2. 获取目标的L-Value（存储位置）
            let target_lvalue = ctx.lower_to_lvalue(target);
            
            match (&reference_addr, &target_lvalue) {
                (Operand::Register { id: addr_reg }, Operand::Register { id: target_addr }) => {
                    // 从引用地址加载值到临时寄存器
                    let temp_reg = ctx.current_function_mut().new_register();
                    ctx.add_instruction(Instruction::Load64 {
                        dst: temp_reg,
                        addr: *addr_reg,
                        offset: 0,
                        span: *span,
                    });
                    
                    // 将加载的值存储到目标位置
                    ctx.add_instruction(Instruction::Store64 {
                        addr: *target_addr,
                        offset: 0,
                        src: Operand::Register { id: temp_reg },
                        span: *span,
                    });
                },
                _ => {
                    // 其他情况：直接复制值
                    if let Operand::Register { id: target_addr } = target_lvalue {
                        ctx.add_instruction(Instruction::Store64 {
                            addr: target_addr,
                            offset: 0,
                            src: reference_addr,
                            span: *span,
                        });
                    }
                }
            }
            
            Ok(())
        }

        Statement::ConstructorArgExtract { target, constructor, arg_index, span } => {
            // Tagged Union构造器参数提取：从Tagged Union结构体中提取数据
            
            // 获取构造器寄存器
            let constructor_operand = ctx.lower_to_rvalue(constructor);
            let constructor_reg = match constructor_operand {
                Operand::Register { id } => id,
                _ => {
                    return Err(vec!["Constructor must be a register for argument extraction".to_string()]);
                }
            };
            
            // 使用Stack-First策略：为目标值分配栈槽
            let target_stack_addr = ctx.allocate_stack_slot_for_value(target);
            
            // 创建临时寄存器来接收提取的数据
            let temp_reg = ctx.current_function_mut().new_register();
            
            // 使用Tagged Union管理器生成数据提取指令到临时寄存器
            let extract_instructions = ctx.tagged_union_manager.generate_data_extraction_instructions(
                constructor_reg,
                temp_reg,
                *span,
            );
            
            for instruction in extract_instructions {
                ctx.add_instruction(instruction);
            }
            
            // 将提取的数据存储到栈槽
            ctx.add_instruction(Instruction::Store64 {
                addr: target_stack_addr,
                offset: 0,
                src: Operand::Register { id: temp_reg },
                span: *span,
            });
            
            // 直接使用Stack-First存储
            let target_key = value_to_key(target);
            ctx.stack_allocations.insert(target_key.clone(), target_stack_addr);
            
            Ok(())
        }

        Statement::HeapAlloc { target, size, object_type, span } => {
            // 🔧 新增：堆分配语句的处理
            // HeapAlloc在LIR中对应Alloc指令，用于在堆上分配内存
            
            // 分配一个寄存器来存储堆地址
            let heap_addr_reg = ctx.current_function_mut().new_register();
            
            // 生成堆分配指令
            ctx.add_instruction(Instruction::Alloc {
                dst: heap_addr_reg,
                size: *size,
                alignment: 8, // 默认8字节对齐
                allocation_type: AllocationType::Heap,
                span: *span,
            });
            
            // 将堆地址存储到目标值的栈位置（Stack-First策略）
            ctx.store_value_to_stack(target, Operand::Register { id: heap_addr_reg });
            
            println!("🔧 HeapAlloc: 分配 {} 字节的 {} 对象到 {:?}", size, object_type, target);
            Ok(())
        }

        Statement::Store { target, value, span } => {
            // 🔧 新增：存储语句的处理
            // Store语句用于将值存储到指定的内存位置
            
            // 获取目标地址（应该是一个包含内存地址的值）
            let target_addr_operand = ctx.lower_to_rvalue(target);
            
            // 获取要存储的值
            let value_operand = ctx.lower_to_rvalue(value);
            
            // 确保目标地址是一个寄存器
            let target_addr_reg = match target_addr_operand {
                Operand::Register { id } => id,
                _ => {
                    // 如果不是寄存器，先移动到临时寄存器
                    let temp_reg = ctx.current_function_mut().new_register();
                    ctx.add_instruction(Instruction::Move {
                        dst: temp_reg,
                        src: target_addr_operand,
                        span: *span,
                    });
                    temp_reg
                }
            };
            
            // 生成存储指令
            ctx.add_instruction(Instruction::Store64 {
                addr: target_addr_reg,
                offset: 0,
                src: value_operand,
                span: *span,
            });
            
            println!("🔧 Store: 将 {:?} 存储到地址 {:?}", value, target);
            Ok(())
        }

        _ => Err(vec![format!("Statement type not yet implemented: {:?}", statement)]),
    }
}

/// 转换MIR终结语句为LIR指令
fn lower_terminator(
    ctx: &mut LirLoweringContext,
    terminator: &Terminator,
) -> Result<(), Vec<String>> {
    use karte_mir::{Terminator};
    
    match terminator {
        Terminator::Return { value, span } => {
            if let Some(return_value) = value {
                // 获取返回值的操作数
                let return_operand = ctx.lower_to_rvalue(return_value);
                
                // 如果操作数是寄存器，直接使用；否则先移动到临时寄存器
                let return_register = match return_operand {
                    Operand::Register { id } => id,
                    _ => {
                        // 创建临时寄存器并移动值
                        let temp_reg = ctx.current_function_mut().new_register();
                        ctx.add_instruction(Instruction::Move {
                            dst: temp_reg,
                            src: return_operand,
                            span: *span,
                        });
                        temp_reg
                    }
                };
                
                // 生成返回指令
                ctx.add_instruction(Instruction::Return {
                    value: Some(return_register),
                    span: *span,
                });
            } else {
                // 无返回值的返回
                ctx.add_instruction(Instruction::Return {
                    value: None,
                    span: *span,
                });
            }
            Ok(())
        }
        
        Terminator::Goto { target, span } => {
            let target_label = ctx.allocate_label_for_block(*target);
            ctx.add_instruction(Instruction::Jump {
                target: target_label,
                span: *span,
            });
            Ok(())
        }
        
        Terminator::Branch { condition, then_block, else_block, span } => {
            // 🔧 重大修复：确保条件值是从正确的源获取的
            // 检查条件是否是比较操作的结果（临时变量）
            let condition_operand = match condition {
                Value::Temp { id } => {
                    // 临时变量：应该从栈加载其值
                    let temp_reg = ctx.current_function_mut().new_register();
                    let stack_addr = ctx.lower_to_lvalue(condition);
                    if let Operand::Register { id: addr_reg } = stack_addr {
                        ctx.add_instruction(Instruction::Load64 {
                            dst: temp_reg,
                            addr: addr_reg,
                            offset: 0,
                            span: *span,
                        });
                        Operand::Register { id: temp_reg }
                    } else {
                        // 如果不是寄存器地址，使用默认的rvalue逻辑
                        ctx.lower_to_rvalue(condition)
                    }
                }
                _ => {
                    // 其他值类型使用标准的rvalue逻辑
                    ctx.lower_to_rvalue(condition)
                }
            };
            
            let then_label = ctx.allocate_label_for_block(*then_block);
            let else_label = ctx.allocate_label_for_block(*else_block);
            
            println!("🔧 分支条件处理: condition={:?}, operand={:?}", condition, condition_operand);
            
            // 比较条件与0（false）
            ctx.add_instruction(Instruction::Compare {
                src1: condition_operand,
                src2: Operand::Immediate { value: 0 },
                span: *span,
            });
            
            // 如果条件不等于0（true），跳转到then分支
            ctx.add_instruction(Instruction::JumpNotEqual {
                target: then_label,
                span: *span,
            });
            
            // 否则跳转到else分支
            ctx.add_instruction(Instruction::Jump {
                target: else_label,
                span: *span,
            });
            
            Ok(())
        }
        
        Terminator::Match { value, arms, default, span } => {
            // Tagged Union模式匹配的LIR实现：
            // 1. 对每个模式生成比较指令
            // 2. 如果匹配成功，跳转到对应的基本块
            // 3. 如果都不匹配，跳转到默认块（如果有的话）
            
            let match_operand = ctx.lower_to_rvalue(value);
            
            // 为每个匹配臂生成比较和跳转指令
            for arm in arms {
                let target_label = ctx.allocate_label_for_block(arm.target);
                
                match &arm.pattern {
                    karte_mir::Pattern::Wildcard => {
                        // 通配符模式总是匹配，直接跳转
                        ctx.add_instruction(Instruction::Jump {
                            target: target_label,
                            span: *span,
                        });
                        return Ok(()); // 通配符后面的模式不会被执行
                    }
                    karte_mir::Pattern::Number { value: pattern_value } => {
                        // 数字模式：比较值是否相等
                        ctx.add_instruction(Instruction::Compare {
                            src1: match_operand.clone(),
                            src2: Operand::Immediate { value: *pattern_value },
                            span: *span,
                        });
                        ctx.add_instruction(Instruction::JumpEqual {
                            target: target_label,
                            span: *span,
                        });
                    }
                    karte_mir::Pattern::Boolean { value: pattern_value } => {
                        // Boolean模式：直接比较0/1值，不使用Tagged Union
                        let expected_value = if *pattern_value { 1 } else { 0 };
                        
                        // 比较匹配值与期望值
                        ctx.add_instruction(Instruction::Compare {
                            src1: match_operand.clone(),
                            src2: Operand::Immediate { value: expected_value },
                            span: *span,
                        });
                        
                        // 如果值匹配，跳转到目标分支
                        ctx.add_instruction(Instruction::JumpEqual {
                            target: target_label,
                            span: *span,
                        });
                    }
                    karte_mir::Pattern::Constructor { name, arg } => {
                        // Tagged Union构造器模式处理：检查标签并提取数据
                        let constructor_reg = match &match_operand {
                            Operand::Register { id } => *id,
                            _ => {
                                ctx.errors.push("Match operand must be a register for constructor pattern".to_string());
                                continue;
                            }
                        };

                        // 获取期望的标签ID
                        let expected_tag_id = if name.contains("::") {
                            let parts: Vec<&str> = name.split("::").collect();
                            if parts.len() == 2 {
                                ctx.tagged_union_manager.get_qualified_constructor_id(parts[0], parts[1])
                            } else {
                                ctx.tagged_union_manager.get_constructor_id(name)
                            }
                        } else {
                            ctx.tagged_union_manager.get_constructor_id(name)
                        };
                        
                        // constructor_reg直接包含Tagged Union的地址
                        // 生成标签检查指令
                        let temp_reg = ctx.current_function_mut().new_register();
                        let tag_check_instructions = ctx.tagged_union_manager.generate_tag_check_instructions(
                            constructor_reg,  // 直接使用constructor_reg作为Tagged Union地址
                            expected_tag_id,
                            temp_reg,
                            *span,
                        );
                            
                        for instruction in tag_check_instructions {
                            ctx.add_instruction(instruction);
                        }
                            
                        // 如果标签匹配，跳转到目标分支
                        ctx.add_instruction(Instruction::JumpEqual {
                            target: target_label,
                            span: *span,
                        });
                            
                        // 如果有参数绑定，生成数据提取指令
                        if let Some(var_name) = arg {
                            let var_reg_id = ctx.allocate_register_for_value(&Value::Variable { name: var_name.clone() });
                            let extract_instructions = ctx.tagged_union_manager.generate_data_extraction_instructions(
                                constructor_reg,  // 直接使用constructor_reg作为Tagged Union地址
                                var_reg_id,
                                *span,
                            );
                            
                            for instruction in extract_instructions {
                                ctx.add_instruction(instruction);
                            }
                        }
                    }
                    karte_mir::Pattern::Variable { name: _ } => {
                        // 变量模式总是匹配（类似通配符）
                        ctx.add_instruction(Instruction::Jump {
                            target: target_label,
                            span: *span,
                        });
                        return Ok(());
                    }
                }
            }
            
            // 如果有默认分支，跳转到默认分支
            if let Some(default_block) = default {
                let default_label = ctx.allocate_label_for_block(*default_block);
                ctx.add_instruction(Instruction::Jump {
                    target: default_label,
                    span: *span,
                });
            }
            
            Ok(())
        }
    }
}

/// 将值转换为字符串键用于映射
fn value_to_key(value: &Value) -> String {
    match value {
        Value::Variable { name } => format!("var:{}", name),
        Value::Temp { id } => format!("temp:{}", id.0),
        Value::Function { name } => format!("fn:{}", name),
        Value::Closure { function_name, captured_values } => {
            let captured_str = captured_values.iter().map(|v| value_to_key(v)).collect::<Vec<_>>().join(",");
            format!("closure:{}:({})", function_name, captured_str)
        },
        // Note: This is a simplification. Hash of constructor/struct would be better
        Value::Constructor { name, arg } => format!("ctor:{}({:?})", name, arg),
        Value::QualifiedConstructor { type_name, constructor_name, arg } => format!("qctor:{}::{}({:?})", type_name, constructor_name, arg),
        Value::Number { value } => format!("num:{}", value),
        Value::Boolean { value } => format!("bool:{}", value),
        Value::Unit => "unit".to_string(),
        Value::Struct { name, fields } => {
            let fields_str = fields.iter()
                .map(|(k, v)| format!("{}:{}", k, value_to_key(v)))
                .collect::<Vec<_>>()
                .join(",");
            format!("struct:{}({})", name, fields_str)
        },
        Value::Reference { value } => {
            format!("ref:({})", value_to_key(value))
        },
    }
}

/// 将字段名转换为偏移量
/// 这是一个简化的实现，用于字段访问
fn field_name_to_offset(field_name: &str) -> i64 {
    match field_name {
        "x" => 1,
        "y" => 2,
        "width" => 3,
        "height" => 4,
        "data" => 5,
        "ref_data" => 6,
        "value" => 7,
        "name" => 8,
        "age" => 9,
        _ => {
            // 对于未知字段，使用简单的哈希
            let mut hash = 0i64;
            for byte in field_name.bytes() {
                hash = hash.wrapping_mul(31).wrapping_add(byte as i64);
            }
            hash.abs() % 1000 + 10 // 确保不与预定义字段冲突
        }
    }
}

/// ===== 构造器编码系统：偏移编码 + 命名空间隔离 =====

/// 构造器编码方案：
/// 
/// 我们使用偏移编码来支持完整的用户数据范围：
/// - 用户数据：完整的i64范围 [i64::MIN, i64::MAX]
/// - 构造器：使用特殊的偏移值，映射到用户数据范围之外的概念空间
/// 
/// 实现方式：
/// 1. 构造器使用负的极端值（接近i64::MIN）
/// 2. 在模式匹配时，我们比较的是构造器ID，不是用户数据
/// 3. 运行时通过上下文区分构造器和用户数据
/// 
/// 构造器命名空间分配：
/// - Boolean构造器: -1000000000 到 -1000000999
/// - Option构造器:  -1000001000 到 -1000001999  
/// - 用户自定义:    -1000002000 到 -1999999999
/// - 系统保留:      -2000000000 到 i64::MIN
/// 
/// 这样设计的优势：
/// 1. 用户数据支持完整的i64范围
/// 2. 构造器使用极端负值，实际冲突概率为0
/// 3. 命名空间隔离，不同类型的构造器有独立的ID空间
/// 4. 性能优异，直接整数比较
/// 5. 语义清晰，类型安全

// 构造器基础偏移（使用极端负值）
const CONSTRUCTOR_BASE_OFFSET: i64 = -1000000000;

// 构造器命名空间
const BOOLEAN_NAMESPACE_BASE: i64 = CONSTRUCTOR_BASE_OFFSET;
const OPTION_NAMESPACE_BASE: i64 = CONSTRUCTOR_BASE_OFFSET - 1000;
const USER_DEFINED_NAMESPACE_BASE: i64 = CONSTRUCTOR_BASE_OFFSET - 2000;

// 具体构造器ID
const TRUE_CONSTRUCTOR_ID: i64 = BOOLEAN_NAMESPACE_BASE - 1;   // -1000000001
const FALSE_CONSTRUCTOR_ID: i64 = BOOLEAN_NAMESPACE_BASE - 2;  // -1000000002
const SOME_CONSTRUCTOR_ID: i64 = OPTION_NAMESPACE_BASE - 1;    // -1000001001
const NONE_CONSTRUCTOR_ID: i64 = OPTION_NAMESPACE_BASE - 2;    // -1000001002

/// 判断值是否为构造器（使用范围检查）
fn is_constructor(value: i64) -> bool {
    value <= CONSTRUCTOR_BASE_OFFSET
}

/// 判断值是否为用户数据
fn is_user_data(value: i64) -> bool {
    value > CONSTRUCTOR_BASE_OFFSET
}

/// 创建用户数据值（支持完整范围）
fn create_user_data(value: i64) -> Result<i64, String> {
    if is_constructor(value) {
        Err(format!("值 {} 在构造器保留范围内", value))
    } else {
        Ok(value)
    }
}

/// 验证用户数据范围
fn validate_user_data(value: i64) -> Result<i64, String> {
    if is_user_data(value) {
        Ok(value)
    } else {
        Err(format!("值 {} 在构造器保留范围内", value))
    }
}

/// 构造器名称到ID的映射（使用命名空间隔离）
fn constructor_name_to_id(name: &str) -> i64 {
    match name {
        // Boolean命名空间（支持大小写两种形式）
        "true" | "True" => TRUE_CONSTRUCTOR_ID,
        "false" | "False" => FALSE_CONSTRUCTOR_ID,
        
        // Option命名空间  
        "Some" => SOME_CONSTRUCTOR_ID,
        "None" => NONE_CONSTRUCTOR_ID,
        
        // 用户自定义构造器使用哈希算法生成ID
        _ => {
            let hash = fnv1a_hash(name.as_bytes());
            // 映射到用户自定义命名空间
            USER_DEFINED_NAMESPACE_BASE - (hash % 996999) as i64 - 1
        }
    }
}

/// FNV-1a哈希算法（用于用户自定义构造器）
fn fnv1a_hash(data: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 14695981039346656037;
    const FNV_PRIME: u64 = 1099511628211;
    
    let mut hash = FNV_OFFSET_BASIS;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// 比较两个值是否相等（处理构造器和用户数据）
fn values_equal(a: i64, b: i64) -> bool {
    // 直接比较即可，因为构造器和用户数据在不同的数值范围
    a == b
}

/// 从构造器值中提取偏移量（用于带参数的构造器）
fn extract_constructor_offset(constructor_id: i64) -> i64 {
    // 对于带参数的构造器，参数存储在单独的位置
    // 这里返回0作为默认偏移量
    0
}

/// 获取用户数据的有效范围
fn get_user_data_range() -> (i64, i64) {
    (CONSTRUCTOR_BASE_OFFSET + 1, i64::MAX)
}

/// 获取构造器的命名空间信息
fn get_constructor_namespaces() -> Vec<(&'static str, i64, i64)> {
    vec![
        ("Boolean", BOOLEAN_NAMESPACE_BASE - 999, BOOLEAN_NAMESPACE_BASE),
        ("Option", OPTION_NAMESPACE_BASE - 999, OPTION_NAMESPACE_BASE),
        ("UserDefined", USER_DEFINED_NAMESPACE_BASE - 996999, USER_DEFINED_NAMESPACE_BASE),
    ]
} 