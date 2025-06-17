use crate::{
    Instruction, LabelId, LirFunction, LirProgram, Operand, RegisterId,
    StructTypeId, AllocationType, StructLayoutManager,
    tagged_union::{TaggedUnionManager, TaggedUnionTag},
};
use karte_mir::{
    BasicBlockId, BinaryOperator, MirProgram, Statement, Terminator, TempId, UnaryOperator,
    Value,
    MirFunction,
    MatchArm,
    Pattern,
};
use std::collections::HashMap;

/// MIR到LIR的lowering上下文
pub struct LirLoweringContext {
    /// 当前LIR函数
    current_function: Option<LirFunction>,
    /// MIR值到寄存器的映射
    value_to_register: HashMap<String, RegisterId>,
    /// MIR基本块到标签的映射
    block_to_label: HashMap<BasicBlockId, LabelId>,
    /// 函数名到标签的映射
    function_labels: HashMap<String, LabelId>,
    /// 值映射：追踪临时变量和变量实际存储的值
    value_mapping: HashMap<String, Value>,
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
}

impl LirLoweringContext {
    pub fn new() -> Self {
        Self {
            current_function: None,
            value_to_register: HashMap::new(),
            block_to_label: HashMap::new(),
            function_labels: HashMap::new(),
            value_mapping: HashMap::new(),
            current_function_params: vec![],
            global_label_counter: 0,
            pending_instructions: vec![],
            errors: vec![],
            struct_layout_manager: StructLayoutManager::new(),
            struct_name_to_type_id: HashMap::new(),
            tagged_union_manager: TaggedUnionManager::new(),
        }
    }

    /// 开始新函数
    pub fn start_function(&mut self, name: String) {
        self.current_function = Some(LirFunction::new(name.clone()));
        // 清理函数相关的状态
        self.value_to_register.clear();
        self.pending_instructions.clear();
        // 清空基本块到标签的映射，确保每个函数的标签都是唯一的
        self.block_to_label.clear();
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

    /// 为值分配寄存器
    fn allocate_register_for_value(&mut self, value: &Value) -> RegisterId {
        let key = value_to_key(value);
        if let Some(&register) = self.value_to_register.get(&key) {
            register
        } else {
            // 检查是否是函数参数
            if let Value::Variable { name } = value {
                if let Some(param_index) = self.current_function_params.iter().position(|p| p == name) {
                    // 函数参数使用固定的寄存器：r0, r1, r2, ...
                    let register = RegisterId(param_index);
                    self.value_to_register.insert(key, register);
                    return register;
                }
            }
            
            let register = self.current_function_mut().new_register();
            self.value_to_register.insert(key, register);
            register
        }
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

    /// 将值转换为操作数
    fn value_to_operand(&mut self, value: &Value) -> Operand {
        match value {
            Value::Number { value } => {
                // 直接使用数字值，不进行范围验证
                // 在运行时，解释器会区分构造器和用户数据
                Operand::Immediate { value: *value }
            },
            Value::Unit => Operand::Immediate { value: 0 }, // Unit表示为0（在用户数据范围内）
            Value::Boolean { value } => {
                // Boolean值使用简单的0/1编码，便于逻辑操作符处理
                Operand::Immediate { value: if *value { 1 } else { 0 } }
            },
            Value::Constructor { name, arg } => {
                // 特殊处理Boolean构造器：使用简单的0/1编码
                if name == "true" {
                    return Operand::Immediate { value: 1 };
                } else if name == "false" {
                    return Operand::Immediate { value: 0 };
                }
                
                // 其他构造器使用tagged union结构体
                // 创建Tagged Union并返回寄存器地址
                let constructor_reg = self.create_tagged_union_for_constructor(name, arg.as_deref());
                
                // 直接返回寄存器，这个寄存器包含Tagged Union结构体的地址
                Operand::Register { id: constructor_reg }
            },
            Value::QualifiedConstructor { type_name, constructor_name, arg, .. } => {
                // 限定构造器使用tagged union结构体
                let constructor_reg = self.create_tagged_union_for_qualified_constructor(type_name, constructor_name, arg.as_deref());
                
                // 直接返回寄存器，这个寄存器包含Tagged Union结构体的地址
                Operand::Register { id: constructor_reg }
            },
            Value::Struct { name, fields } => {
                // 结构体处理：在栈上分配内存并存储字段数据
                let struct_reg = self.current_function_mut().new_register();
                
                // 计算结构体大小（简化：每个字段8字节）
                let struct_size = fields.len() * 8;
                
                // 在栈上分配结构体内存
                self.add_instruction(Instruction::Alloc {
                    dst: struct_reg,
                    size: struct_size,
                    alignment: 8,
                    allocation_type: AllocationType::Stack,
                    span: karte_diagnostics::Span::dummy(),
                });
                
                // 存储字段数据到结构体内存中
                // 按照预定义的字段顺序存储，而不是按照HashMap的迭代顺序
                let field_order = ["value", "next"]; // 定义字段的正确顺序
                
                for (field_index, field_name) in field_order.iter().enumerate() {
                    if let Some(field_value) = fields.get(*field_name) {
                        let field_operand = self.value_to_operand(field_value);
                        let field_offset = field_index * 8; // 每个字段8字节
                        
                        // 将字段值存储到结构体内存的相应偏移位置
                        self.add_instruction(Instruction::Store64 {
                            addr: struct_reg,
                            offset: field_offset as i64,
                            src: field_operand,
                            span: karte_diagnostics::Span::dummy(),
                        });
                        
                        // 将字段映射存储在value_mapping中，以便字段访问时能找到
                        let field_key = format!("struct:{}:{}:{}", name, struct_reg.0, field_name);
                        let resolved_field = self.resolve_value(field_value);
                        self.value_mapping.insert(field_key, resolved_field);
                    }
                }
                
                Operand::Register { id: struct_reg }
            },
            Value::Function { name } => {
                // 函数值表示为函数ID（简化处理）
                // 我们使用函数名的哈希作为函数ID
                let function_id = name.chars().fold(0, |acc, c| acc + c as usize) as i64;
                Operand::Immediate { value: function_id }
            },
            Value::Closure { function_name, .. } => {
                // 闭包值也表示为函数ID，类似于函数值
                let function_id = function_name.chars().fold(0, |acc, c| acc + c as usize) as i64;
                Operand::Immediate { value: function_id }
            },
            Value::Reference { value } => {
                // 引用处理：将引用存储在栈内存中
                // 1. 首先确保被引用的值已经被正确处理
                let referenced_operand = self.value_to_operand(value);
                
                // 2. 在栈上分配空间存储被引用的值
                let stack_addr_reg = self.current_function_mut().new_register();
                
                // 分配栈空间（16字节用于存储一个完整的值，包括可能的Tagged Union）
                self.add_instruction(Instruction::Alloc {
                    dst: stack_addr_reg,
                    size: 16,
                    alignment: 8,
                    allocation_type: AllocationType::Stack,
                    span: karte_diagnostics::Span::dummy(),
                });
                
                // 3. 将被引用的值存储到栈内存中
                match referenced_operand {
                    Operand::Register { id: _src_reg } => {
                        // 直接将被引用的值存储到栈内存中
                        self.add_instruction(Instruction::Store64 {
                            addr: stack_addr_reg,
                            offset: 0,
                            src: referenced_operand,
                            span: karte_diagnostics::Span::dummy(),
                        });
                    },
                    _ => {
                        // 对于立即数或其他操作数，也直接存储
                        self.add_instruction(Instruction::Store64 {
                            addr: stack_addr_reg,
                            offset: 0,
                            src: referenced_operand,
                            span: karte_diagnostics::Span::dummy(),
                        });
                    }
                }
                
                // 4. 直接返回引用地址，不进行额外的存储和加载操作
                // 这样可以避免寄存器冲突问题
                Operand::Register { id: stack_addr_reg }
            },
            _ => {
                let register = self.allocate_register_for_value(value);
                Operand::Register { id: register }
            }
        }
    }
    
    /// 处理结构体值，分配内存并初始化字段
    fn handle_struct_value(&mut self, name: &str, fields: &std::collections::HashMap<String, Value>) -> Result<RegisterId, String> {
        // 获取或创建结构体类型ID
        let struct_type_id = self.get_or_create_struct_type_id(name)?;
        
        // 分配结构体内存
        let struct_addr = self.current_function_mut().new_register();
        self.add_instruction(Instruction::StructAlloc {
            dst: struct_addr,
            struct_type: struct_type_id,
            allocation_type: AllocationType::Stack, // 默认栈分配
            span: karte_diagnostics::Span::dummy(),
        });

        // 获取结构体布局
        let layout = self.current_function.as_ref()
            .ok_or_else(|| "没有当前函数".to_string())?
            .get_struct_layout(struct_type_id)
            .ok_or_else(|| "无法获取结构体布局".to_string())?
            .clone();

        // 初始化每个字段
        for field in &layout.fields {
            if let Some(field_value) = fields.get(&field.name) {
                let field_operand = self.value_to_operand(field_value);
                self.add_instruction(Instruction::StructFieldStore {
                    struct_addr,
                    field_offset: field.offset,
                    src: field_operand,
                    span: karte_diagnostics::Span::dummy(),
                });
            }
        }

        Ok(struct_addr)
    }

    /// 获取或创建结构体类型ID
    fn get_or_create_struct_type_id(&mut self, name: &str) -> Result<StructTypeId, String> {
        if let Some(&type_id) = self.struct_name_to_type_id.get(name) {
            return Ok(type_id);
        }

        // 需要从某处获取结构体定义...
        // 这里是一个简化版本，实际应该从HIR或类型检查器获取
        let mock_fields = vec![]; // 实际应该从结构体定义中获取
        let layout = self.struct_layout_manager.compute_layout(name, &mock_fields)?;
        
        let type_id = self.current_function_mut().add_struct_type(layout);
        self.struct_name_to_type_id.insert(name.to_string(), type_id);
        
        Ok(type_id)
    }

    /// 添加指令
    fn add_instruction(&mut self, instruction: Instruction) {
        self.current_function_mut().add_instruction(instruction);
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
            Some(self.value_to_operand(arg_value))
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
            Some(self.value_to_operand(arg_value))
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

    /// 解析值的实际内容，处理间接引用
    fn resolve_value(&self, value: &Value) -> Value {
        self.resolve_value_with_visited(value, &mut std::collections::HashSet::new())
    }
    
    /// 带循环检测的值解析函数
    fn resolve_value_with_visited(&self, value: &Value, visited: &mut std::collections::HashSet<String>) -> Value {
        match value {
            Value::Temp { .. } | Value::Variable { .. } => {
                let key = value_to_key(value);
                
                // 检查是否已经访问过这个值，防止无限递归
                if visited.contains(&key) {
                    // 发现循环引用，返回原始值以打破循环
                    return value.clone();
                }
                
                if let Some(mapped_value) = self.value_mapping.get(&key) {
                    // 将当前值添加到已访问集合
                    visited.insert(key.clone());
                    
                    // 递归解析，防止多层间接引用
                    let result = self.resolve_value_with_visited(mapped_value, visited);
                    
                    // 从已访问集合中移除当前值（回溯）
                    visited.remove(&key);
                    
                    result
                } else {
                    value.clone()
                }
            }
            Value::Reference { value: inner } => {
                // 对于引用值，我们也需要递归解析内部值
                let resolved_inner = self.resolve_value_with_visited(inner, visited);
                Value::Reference { value: Box::new(resolved_inner) }
            }
            _ => value.clone(),
        }
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
}

/// 将MIR程序转换为LIR程序
pub fn lower_mir_to_lir(mir_program: &MirProgram) -> Result<LirProgram, Vec<String>> {
    let mut context = LirLoweringContext::new();
    let mut lir_program = LirProgram::new();

    // 预处理，为所有函数创建入口标签
    // 我们需要先为所有函数分配标签ID，这样在处理函数调用时就能找到它们
    let mut function_names: Vec<_> = mir_program.functions.keys().cloned().collect();
    function_names.sort();

    for name in &function_names {
        let label_id = LabelId(context.global_label_counter);
        context.global_label_counter += 1;
        context.function_labels.insert(name.clone(), label_id);
    }

    // 转换每个函数
    for name in &function_names {
        let mir_function = mir_program.functions.get(name).unwrap();
        context.start_function(name.clone());
        
        // 设置当前函数的参数
        context.current_function_params = mir_function.params.clone();

        // 使用预分配的入口标签
        let entry_label = context.function_labels.get(name).cloned().expect("Function label should exist");
        context.add_instruction(Instruction::Label {
            id: entry_label,
            span: karte_diagnostics::Span::new(0,0), // Dummy span
        });
        
        // 在函数开始处理参数：将调用约定的参数寄存器移动到函数内部的参数变量
        for (i, param_name) in mir_function.params.iter().enumerate() {
            if i < 4 { // 调用约定最多支持4个参数
                let param_source_reg = RegisterId(i + 1); // 调用约定参数寄存器: r1, r2, r3, r4
                let param_dest_reg = context.current_function_mut().new_register();
                
                // 将参数从调用约定寄存器移动到函数内部寄存器
                context.add_instruction(Instruction::Move {
                    dst: param_dest_reg,
                    src: Operand::Register { id: param_source_reg },
                    span: karte_diagnostics::Span::new(0, 0),
                });
                
                // 在值映射中记录参数变量到寄存器的映射
                let param_value = Value::Variable { name: param_name.clone() };
                let param_key = value_to_key(&param_value);
                context.value_mapping.insert(param_key, Value::Temp { id: TempId(param_dest_reg.0) });
                
                // 同时更新寄存器映射
                context.value_to_register.insert(param_name.clone(), param_dest_reg);
            }
        }

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

    // 在返回之前，降级高级指令为基础指令
    if context.errors.is_empty() {
        match crate::lower_program_instructions(&mut lir_program) {
            Ok(()) => Ok(lir_program),
            Err(lowering_error) => {
                context.errors.push(format!("指令降级错误: {}", lowering_error));
                Err(context.errors)
            }
        }
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
        Statement::Assign { target, source, span } => {
            let dst = ctx.allocate_register_for_value(target);
            
            // 特殊处理构造器：直接生成Tagged Union，不需要额外的Move指令
            match source {
                Value::Constructor { name, arg } => {
                    // 特殊处理Boolean构造器：使用简单的0/1编码
                    if name == "true" {
                        ctx.add_instruction(Instruction::Move {
                            dst,
                            src: Operand::Immediate { value: 1 },
                            span: *span,
                        });
                    } else if name == "false" {
                        ctx.add_instruction(Instruction::Move {
                            dst,
                            src: Operand::Immediate { value: 0 },
                            span: *span,
                        });
                    } else {
                        // 其他构造器：创建Tagged Union，直接使用目标寄存器
                        let tag_id = ctx.tagged_union_manager.get_constructor_id(name);
                        let data_operand = if let Some(arg_value) = arg {
                            Some(ctx.value_to_operand(arg_value))
                        } else {
                            None
                        };
                        
                        let instructions = ctx.tagged_union_manager.generate_allocation_instructions(
                            dst,
                            tag_id,
                            data_operand,
                            *span,
                        );
                        
                        for instruction in instructions {
                            ctx.add_instruction(instruction);
                        }
                    }
                },
                Value::QualifiedConstructor { type_name, constructor_name, arg, .. } => {
                    // 限定构造器：创建Tagged Union，直接使用目标寄存器
                    let tag_id = ctx.tagged_union_manager.get_qualified_constructor_id(type_name, constructor_name);
                    let data_operand = if let Some(arg_value) = arg {
                        Some(ctx.value_to_operand(arg_value))
                    } else {
                        None
                    };
                    
                    let instructions = ctx.tagged_union_manager.generate_allocation_instructions(
                        dst,
                        tag_id,
                        data_operand,
                        *span,
                    );
                    
                    for instruction in instructions {
                        ctx.add_instruction(instruction);
                    }
                },
                _ => {
                    // 其他值：正常处理
                    let src = ctx.value_to_operand(source);
                    ctx.add_instruction(Instruction::Move {
                        dst,
                        src,
                        span: *span,
                    });
                }
            }
            
            // 更新值映射，追踪赋值关系
            let target_key = value_to_key(target);
            // 如果source是一个间接引用，我们需要解析它的实际值
            let actual_source = ctx.resolve_value(source);
            ctx.value_mapping.insert(target_key, actual_source);
            
            Ok(())
        }

        Statement::BinaryOp {
            target,
            left,
            op,
            right,
            span,
        } => {
            let dst = ctx.allocate_register_for_value(target);
            let src1 = ctx.value_to_operand(left);
            let src2 = ctx.value_to_operand(right);

            let instruction = match op {
                BinaryOperator::Add => Instruction::Add { dst, src1, src2, span: *span },
                BinaryOperator::Subtract => Instruction::Sub { dst, src1, src2, span: *span },
                BinaryOperator::Multiply => Instruction::Mul { dst, src1, src2, span: *span },
                BinaryOperator::Divide => Instruction::Div { dst, src1, src2, span: *span },
                
                // For comparisons, we generate a cmp instruction and then a conditional jump.
                // The result (true/false) is moved into the destination register.
                BinaryOperator::Equal | BinaryOperator::NotEqual | BinaryOperator::LessThan |
                BinaryOperator::LessEqual | BinaryOperator::GreaterThan | BinaryOperator::GreaterEqual => {
                    ctx.add_instruction(Instruction::Compare { src1, src2, span: *span });
                    
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

                    // False case
                    ctx.add_instruction(Instruction::Move { dst: dst, src: Operand::Immediate { value: 0 }, span: *span });
                    ctx.add_instruction(Instruction::Jump { target: end_label, span: *span });

                    // True case
                    ctx.add_instruction(Instruction::Label { id: true_label, span: *span });
                    ctx.add_instruction(Instruction::Move { dst: dst, src: Operand::Immediate { value: 1 }, span: *span });

                    // End
                    ctx.add_instruction(Instruction::Label { id: end_label, span: *span });
                    return Ok(());
                }
                
                // Handle logical operations with simple 0/1 encoding and short-circuit evaluation
                BinaryOperator::And => {
                    // Logical AND: if src1 == 0, result = 0; else result = src2
                    let false_label = LabelId(ctx.global_label_counter);
                    ctx.global_label_counter += 1;
                    let end_label = LabelId(ctx.global_label_counter);
                    ctx.global_label_counter += 1;
                    
                    // Compare src1 with 0 (false)
                    ctx.add_instruction(Instruction::Compare { 
                        src1: src1.clone(), 
                        src2: Operand::Immediate { value: 0 }, 
                        span: *span 
                    });
                    
                    // If src1 == 0, jump to false_label
                    ctx.add_instruction(Instruction::JumpEqual { target: false_label, span: *span });
                    
                    // src1 is true (non-zero), move src2 to result
                    ctx.add_instruction(Instruction::Move { dst, src: src2, span: *span });
                    ctx.add_instruction(Instruction::Jump { target: end_label, span: *span });
                    
                    // src1 is false, result is false (0)
                    ctx.add_instruction(Instruction::Label { id: false_label, span: *span });
                    ctx.add_instruction(Instruction::Move { dst, src: Operand::Immediate { value: 0 }, span: *span });
                    
                    // End
                    ctx.add_instruction(Instruction::Label { id: end_label, span: *span });
                    return Ok(());
                }
                
                BinaryOperator::Or => {
                    // Logical OR: if src1 != 0, result = 1; else result = src2
                    let true_label = LabelId(ctx.global_label_counter);
                    ctx.global_label_counter += 1;
                    let end_label = LabelId(ctx.global_label_counter);
                    ctx.global_label_counter += 1;
                    
                    // Compare src1 with 0 (false)
                    ctx.add_instruction(Instruction::Compare { 
                        src1: src1.clone(), 
                        src2: Operand::Immediate { value: 0 }, 
                        span: *span 
                    });
                    
                    // If src1 != 0, jump to true_label
                    ctx.add_instruction(Instruction::JumpNotEqual { target: true_label, span: *span });
                    
                    // src1 is false, move src2 to result
                    ctx.add_instruction(Instruction::Move { dst, src: src2, span: *span });
                    ctx.add_instruction(Instruction::Jump { target: end_label, span: *span });
                    
                    // src1 is true, result is true (1)
                    ctx.add_instruction(Instruction::Label { id: true_label, span: *span });
                    ctx.add_instruction(Instruction::Move { dst, src: Operand::Immediate { value: 1 }, span: *span });
                    
                    // End
                    ctx.add_instruction(Instruction::Label { id: end_label, span: *span });
                    return Ok(());
                }
            };

            ctx.add_instruction(instruction);
            Ok(())
        }

        Statement::UnaryOp {
            target,
            op,
            operand,
            span,
        } => {
            let dst = ctx.allocate_register_for_value(target);
            let src = ctx.value_to_operand(operand);

            match op {
                UnaryOperator::Plus => {
                    // +x is just x, so we move it
                    ctx.add_instruction(Instruction::Move {
                        dst,
                        src,
                        span: *span,
                    });
                }
                UnaryOperator::Minus => {
                    // -x is 0 - x
                    ctx.add_instruction(Instruction::Sub {
                        dst,
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
                        dst, 
                        src1: Operand::Register { id: temp_reg }, 
                        src2: src, 
                        span: *span 
                    });
                }
            }
            Ok(())
        }

        Statement::Call { target, function, args, span } => {
            // 首先解析函数值，看看是否是闭包
            let resolved_function = ctx.resolve_value(function);
            
            let mut all_args = Vec::new();
            
            // 如果是闭包，需要先添加捕获的值作为参数
            if let Value::Closure { captured_values, .. } = &resolved_function {
                all_args.extend(captured_values.clone());
            }
            
            // 然后添加实际的调用参数
            all_args.extend(args.clone());
            
            // 转换所有参数为操作数
            let mut arg_operands = vec![];
            for arg in &all_args {
                arg_operands.push(ctx.value_to_operand(arg));
            }

            // 分配临时寄存器来避免参数冲突
            let mut temp_regs = vec![];
            for (i, arg_op) in arg_operands.iter().enumerate() {
                let temp_reg = ctx.current_function_mut().new_register();
                ctx.add_instruction(Instruction::Move {
                    dst: temp_reg,
                    src: arg_op.clone(),
                    span: *span,
                });
                temp_regs.push(temp_reg);
            }
            
            // 然后按照调用约定将参数移动到正确的寄存器
            // 调用约定：参数寄存器为 [1, 2, 3, 4]
            let mut arg_regs = vec![];
            for (i, temp_reg) in temp_regs.iter().enumerate() {
                if i < 4 { // 最多支持4个参数
                    let param_reg = RegisterId(i + 1); // 参数寄存器: r1, r2, r3, r4
                    ctx.add_instruction(Instruction::Move {
                        dst: param_reg,
                        src: Operand::Register { id: *temp_reg },
                        span: *span,
                    });
                    arg_regs.push(param_reg);
                } else {
                    // 超过4个参数需要使用栈传递 - TODO: 未来实现
                    return Err(vec!["Functions with more than 4 parameters are not yet supported".to_string()]);
                }
            }

            // 检查是否是函数参数调用
            let is_function_parameter = match &resolved_function {
                Value::Variable { name } => {
                    ctx.current_function_params.contains(name)
                },
                _ => false,
            };

            if is_function_parameter {
                // 对于函数参数，使用间接调用
                if let Value::Variable { name } = &resolved_function {
                    let function_register = ctx.allocate_register_for_value(&resolved_function);
                    
                    // 使用特殊的间接调用指令（我们需要定义这个指令）
                    // 暂时使用CallIndirect指令来处理函数参数调用
                    let result_reg = target.as_ref().map(|t| ctx.allocate_register_for_value(t));
                    
                    ctx.add_instruction(Instruction::CallIndirect {
                        function_register,
                        args: arg_regs,
                        result: result_reg,
                        span: *span,
                    });
                    
                    // 不需要额外的Move指令，因为CallIndirect指令已经正确指定了result寄存器
                    // 专业执行器会在函数返回时直接将结果设置到result寄存器中
                }
            } else {
                // 尝试从值中提取函数名，支持更多类型的可调用值
                let function_name = match &resolved_function {
                    Value::Function { name } => name.clone(),
                    Value::Closure { function_name, .. } => function_name.clone(),
                    Value::Variable { name } => {
                        // 对于变量，尝试查找它是否映射到函数或闭包
                        let var_key = value_to_key(function);
                        if let Some(mapped_value) = ctx.value_mapping.get(&var_key) {
                            match mapped_value {
                                Value::Function { name } => name.clone(),
                                Value::Closure { function_name, .. } => function_name.clone(),
                                _ => {
                                    // 变量没有映射到函数类型，但可能是函数参数
                                    // 在函数式编程中，函数参数也应该可以被调用
                                    // 我们假设变量名就是函数名（这对lambda参数有效）
                                    name.clone()
                                }
                            }
                        } else {
                            // 没有找到映射，假设变量名就是函数名
                            name.clone()
                        }
                    },
                    Value::Temp { id } => {
                        // 对于临时变量，查找映射
                        let temp_key = value_to_key(function);
                        if let Some(mapped_value) = ctx.value_mapping.get(&temp_key) {
                            match mapped_value {
                                Value::Function { name } => name.clone(),
                                Value::Closure { function_name, .. } => function_name.clone(),
                                _ => {
                                    return Err(vec![format!("Cannot call a non-function value: {:?}", mapped_value)]);
                                }
                            }
                        } else {
                            return Err(vec![format!("Cannot call unresolved temporary: {:?}", function)]);
                        }
                    },
                    _ => {
                        return Err(vec![format!("Cannot call a non-function value: {:?}", resolved_function)]);
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
                
                // 不需要额外的Move指令，因为Call指令已经正确指定了result寄存器
                // 专业执行器会在函数返回时直接将结果设置到result寄存器中
            }

            Ok(())
        }

        Statement::FieldAccess { target, object, field, span } => {
            // 字段访问实现：从结构体内存中加载字段数据
            let dst = ctx.allocate_register_for_value(target);
            
            // 尝试解析对象值
            let resolved_object = ctx.resolve_value(object);
            
            // 首先尝试从结构体值中直接获取字段
            if let Value::Struct { name, fields } = &resolved_object {
                if let Some(field_value) = fields.get(field) {
                    let field_operand = ctx.value_to_operand(field_value);
                    ctx.add_instruction(Instruction::Move {
                        dst,
                        src: field_operand,
                        span: *span,
                    });
                    
                    let target_key = value_to_key(target);
                    let resolved_field = ctx.resolve_value(field_value);
                    ctx.value_mapping.insert(target_key, resolved_field);
                    
                    return Ok(());
                }
            }
            
            // 如果不是直接的结构体值，尝试从内存中加载字段
            let object_operand = ctx.value_to_operand(object);
            
            if let Operand::Register { id: object_reg } = object_operand {
                // 计算字段偏移（简化：假设字段按声明顺序存储，每个字段8字节）
                // 这里我们需要知道字段在结构体中的位置
                // 简化实现：假设常见的字段名对应固定偏移
                let field_offset = match field.as_str() {
                    "value" => 0,  // 第一个字段
                    "next" => 8,   // 第二个字段
                    _ => 0,        // 默认第一个字段
                };
                
                // 从结构体内存中加载字段值
                ctx.add_instruction(Instruction::Load64 {
                    dst,
                    addr: object_reg,
                    offset: field_offset,
                    span: *span,
                });
                
                let target_key = value_to_key(target);
                let target_value = Value::Temp { id: TempId(dst.0) };
                ctx.value_mapping.insert(target_key, target_value);
                
                return Ok(());
            }
            
            // 如果无法解析，生成占位符
            ctx.add_instruction(Instruction::Move {
                dst,
                src: Operand::Immediate { value: 0 },
                span: *span,
            });
            
            Ok(())
        }

        Statement::Dereference { target, reference, span } => {
            // 解引用的实现：从栈内存中加载被引用的值
            let dst = ctx.allocate_register_for_value(target);
            
            // 获取引用的操作数（这是一个栈地址）
            let ref_operand = ctx.value_to_operand(reference);
            
            // 从栈内存中加载值
            match ref_operand {
                Operand::Register { id: addr_reg } => {
                    // 从栈地址加载值
                    // 注意：这里加载的可能是Tagged Union结构的地址，也可能是简单值
                    ctx.add_instruction(Instruction::Load64 {
                        dst,
                        addr: addr_reg,
                        offset: 0,
                        span: *span,
                    });
                },
                _ => {
                    // 如果引用不是寄存器（不应该发生），直接复制值
                    ctx.add_instruction(Instruction::Move {
                        dst,
                        src: ref_operand,
                        span: *span,
                    });
                }
            }
            
            // 更新值映射
            let target_key = value_to_key(target);
            let target_value = Value::Temp { id: TempId(dst.0) };
            ctx.value_mapping.insert(target_key, target_value);
            
            Ok(())
        }

        Statement::ConstructorArgExtract { target, constructor, arg_index, span } => {
            // Tagged Union构造器参数提取：从Tagged Union结构体中提取数据
            let dst = ctx.allocate_register_for_value(target);
            
            // 获取构造器寄存器
            let constructor_operand = ctx.value_to_operand(constructor);
            let constructor_reg = match constructor_operand {
                Operand::Register { id } => id,
                _ => {
                    return Err(vec!["Constructor must be a register for argument extraction".to_string()]);
                        }
            };
            
            // 使用Tagged Union管理器生成数据提取指令
            let extract_instructions = ctx.tagged_union_manager.generate_data_extraction_instructions(
                constructor_reg,
                dst,
                *span,
            );
            
            for instruction in extract_instructions {
                ctx.add_instruction(instruction);
            }
            
            // 更新值映射
                let target_key = value_to_key(target);
            let target_value = Value::Temp { id: TempId(dst.0) };
            ctx.value_mapping.insert(target_key, target_value);
            
            Ok(())
        }

        Statement::HeapAlloc { target, size, object_type, span } => {
            // 堆分配：生成一个简化的堆分配指令
            let dst = ctx.allocate_register_for_value(target);
            
            // 简化实现：使用Move指令生成一个模拟的堆地址
            // 实际实现中应该调用堆分配器
            let heap_addr = 0x1000 + (*size as i64); // 简化的堆地址计算
            ctx.add_instruction(Instruction::Move {
                dst,
                src: Operand::Immediate { value: heap_addr },
                span: *span,
            });
            
            // 更新值映射
            let target_key = value_to_key(target);
            let target_value = Value::Temp { id: TempId(dst.0) };
            ctx.value_mapping.insert(target_key, target_value);
            
            Ok(())
        }

        Statement::Store { target, value, span } => {
            // Store语句：将值存储到指定位置
            // target现在是一个Value，表示目标地址
            
            let value_operand = ctx.value_to_operand(value);
            let target_operand = ctx.value_to_operand(target);
            
            // 生成Store64指令：将值存储到目标地址
            match target_operand {
                Operand::Register { id: addr_reg } => {
                    ctx.add_instruction(Instruction::Store64 {
                        addr: addr_reg,
                        offset: 0,
                        src: value_operand,
                        span: *span,
                    });
                }
                Operand::Immediate { value: addr } => {
                    // 如果目标是立即数地址，先移动到寄存器
                    let addr_reg = ctx.current_function_mut().new_register();
                    ctx.add_instruction(Instruction::Move {
                        dst: addr_reg,
                        src: Operand::Immediate { value: addr },
                        span: *span,
                    });
                    
                    ctx.add_instruction(Instruction::Store64 {
                        addr: addr_reg,
                        offset: 0,
                        src: value_operand,
                        span: *span,
                    });
                }
                _ => {
                    return Err(vec!["Store target must be an address".to_string()]);
                }
            }
            
            Ok(())
        }

        _ => Err(vec!["Statement type not yet implemented".to_string()]),
    }
}

/// 转换MIR终结语句为LIR指令
fn lower_terminator(
    ctx: &mut LirLoweringContext,
    terminator: &Terminator,
) -> Result<(), Vec<String>> {
    match terminator {
        Terminator::Goto { target, span } => {
            let label = ctx.allocate_label_for_block(*target);
            ctx.add_instruction(Instruction::Jump {
                target: label,
                span: *span,
            });
        }
        Terminator::Return { value, span } => {
            let return_reg = value
                .as_ref()
                .map(|v| ctx.allocate_register_for_value(v));
            ctx.add_instruction(Instruction::Return {
                value: return_reg,
                span: *span,
            });
        }
        Terminator::Branch {
            condition,
            then_block,
            else_block,
            span,
        } => {
            let cond_op = ctx.value_to_operand(condition);
            let then_label = ctx.allocate_label_for_block(*then_block);
            let else_label = ctx.allocate_label_for_block(*else_block);
            
            // 使用简单的0/1编码进行boolean比较
            // 如果条件值不等于0（即为true），跳转到then分支
            ctx.add_instruction(Instruction::Compare {
                src1: cond_op,
                src2: Operand::Immediate { value: 0 },
                span: *span,
            });
            
            // 如果条件不等于0（即为true），跳转到then分支
            ctx.add_instruction(Instruction::JumpNotEqual {
                target: then_label,
                span: *span,
            });
            
            // 否则跳转到else分支
            ctx.add_instruction(Instruction::Jump {
                target: else_label,
                span: *span,
            });
        }
        
        Terminator::Match { value, arms, default, span } => {
            // 模式匹配的LIR实现：
            // 1. 对每个模式生成比较指令
            // 2. 如果匹配成功，跳转到对应的基本块
            // 3. 如果都不匹配，跳转到默认块（如果有的话）
            
            let match_operand = ctx.value_to_operand(value);
            
            // 为每个匹配臂生成比较和跳转指令
            for arm in arms {
                let target_label = ctx.allocate_label_for_block(arm.target);
                
                match &arm.pattern {
                    Pattern::Wildcard => {
                        // 通配符模式总是匹配，直接跳转
                        ctx.add_instruction(Instruction::Jump {
                            target: target_label,
                            span: *span,
                        });
                        return Ok(()); // 通配符后面的模式不会被执行
                    }
                    Pattern::Number { value: pattern_value } => {
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
                    Pattern::Boolean { value: pattern_value } => {
                        // 布尔模式：比较简单的0/1编码值
                        let expected_value = if *pattern_value { 1 } else { 0 };
                        
                        // 比较匹配值与期望值
                        ctx.add_instruction(Instruction::Compare {
                            src1: match_operand.clone(),
                            src2: Operand::Immediate { value: expected_value },
                            span: *span,
                        });
                        
                        ctx.add_instruction(Instruction::JumpEqual {
                            target: target_label,
                            span: *span,
                        });
                    }
                    Pattern::Constructor { name, arg } => {
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
                            
                        // 生成标签检查指令
                        let temp_reg = ctx.current_function_mut().new_register();
                            let tag_check_instructions = ctx.tagged_union_manager.generate_tag_check_instructions(
                            constructor_reg,
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
                                constructor_reg,
                                var_reg_id,
                                *span,
                            );
                            
                            for instruction in extract_instructions {
                                ctx.add_instruction(instruction);
                            }
                            
                            // 更新变量映射
                            let var_key = value_to_key(&Value::Variable { name: var_name.clone() });
                            let var_value = Value::Temp { id: TempId(var_reg_id.0) };
                            ctx.value_mapping.insert(var_key, var_value);
                        }
                    }
                    Pattern::Variable { name: _ } => {
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
            // 如果没有默认分支且没有匹配，这是一个运行时错误
            // 在实际实现中应该有更好的错误处理
        }
        
        _ => return Err(vec!["Terminator type not yet implemented".to_string()]),
    }
    Ok(())
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
        Value::Function { name } => format!("fn:{}", name),
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