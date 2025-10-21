use crate::{
    tagged_union::TaggedUnionManager, AllocationType, Instruction, LabelId, LirFunction,
    LirProgram, Operand, Register, StructField, StructLayout, StructLayoutManager, StructTypeId,
};
use karte_mir::{
    BasicBlockId, BinaryOperator, MirProgram, Statement, TempId, Terminator, UnaryOperator, Value,
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
    stack_allocations: HashMap<String, Register>,
    /// 🔧 专业修复：全局结构体类型信息
    global_struct_types: HashMap<String, StructLayout>,
    /// 代数效应：handler入口块的参数名映射（用于在块标签处把payload写入变量）
    handler_block_param: HashMap<BasicBlockId, String>,
}

impl Default for LirLoweringContext {
    fn default() -> Self {
        Self::new()
    }
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
            handler_block_param: HashMap::new(),
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

        log::debug!(
            "🔧 创建函数 {} 包含 {} 个参数: {:?}",
            name,
            params.len(),
            params
        );
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
    fn allocate_stack_slot_for_value(&mut self, value: &Value) -> Register {
        self.allocate_stack_slot_for_value_with_instruction(value, true)
    }

    /// 为值分配栈槽，可以选择是否生成alloc指令
    fn allocate_stack_slot_for_value_with_instruction(
        &mut self,
        value: &Value,
        generate_alloc: bool,
    ) -> Register {
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
            Value::Boolean { .. }
            | Value::Constructor { .. }
            | Value::QualifiedConstructor { .. } => 16, // Tagged Union需要16字节（tag + data）
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
    fn allocate_register_for_value(&mut self, value: &Value) -> Register {
        // 检查是否是函数参数 - 函数参数仍然使用寄存器传递
        if let Value::Variable { name } = value {
            if let Some(param_index) = self.current_function_params.iter().position(|p| p == name) {
                // 函数参数使用固定的寄存器：r1, r2, r3, r4（跳过r0作为特殊用途）
                return Register::Virtual(param_index + 1);
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
                    if let Some(param_index) =
                        self.current_function_params.iter().position(|p| p == name)
                    {
                        let param_reg = Register::Virtual(param_index + 1);
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
                    log::debug!(
                        "🔧 lower_to_lvalue: 找到FieldAccess栈槽 {} -> {:?} (来自{})",
                        value_key,
                        field_stack_addr,
                        field_key
                    );
                    // 将这个栈槽也注册到常规的value_key下，便于后续查找
                    self.stack_allocations.insert(value_key, field_stack_addr);
                    return Operand::Register {
                        id: field_stack_addr,
                    };
                }
            }

            // 如果没有找到FieldAccess栈槽，检查是否有其他field_*键
            for (key, &addr) in self.stack_allocations.iter() {
                if key.ends_with(&format!(":{}", value_key)) && key.starts_with("field_") {
                    log::debug!(
                        "🔧 lower_to_lvalue: 找到其他FieldAccess栈槽 {} -> {:?} (来自{})",
                        value_key,
                        addr,
                        key
                    );
                    // 将这个栈槽也注册到常规的value_key下
                    self.stack_allocations.insert(value_key, addr);
                    return Operand::Register { id: addr };
                }
            }
        }

        // 检查是否已经有栈分配
        if let Some(&stack_addr) = self.stack_allocations.get(&value_key) {
            log::debug!(
                "🔧 lower_to_lvalue: 找到已分配的栈槽 {} -> {:?}",
                value_key,
                stack_addr
            );
            return Operand::Register { id: stack_addr };
        }

        // 分配新的栈空间
        log::debug!("🔧 lower_to_lvalue: 需要分配新栈槽 {}", value_key);
        let stack_addr = self.allocate_stack_slot_for_value(value);
        log::debug!(
            "🔧 lower_to_lvalue: 分配了新栈槽 {} -> {:?}",
            value_key,
            stack_addr
        );
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
                if let Some(param_index) =
                    self.current_function_params.iter().position(|p| p == name)
                {
                    let param_reg = Register::Virtual(param_index + 1); // 参数寄存器: r1, r2, r3, r4
                    log::debug!(
                        "🔧 函数参数 {} 在lower_to_rvalue中直接使用寄存器 {:?}",
                        name,
                        param_reg
                    );
                    return Operand::Register { id: param_reg };
                }
            }
        }

        match value {
            // 立即数值直接返回
            Value::Number { value } => Operand::Immediate { value: *value },
            Value::Boolean { value } => Operand::Immediate {
                value: if *value { 1 } else { 0 },
            },
            Value::Unit => Operand::Immediate { value: 0 },

            // 函数值
            Value::Function { name } => {
                if let Some(&label_id) = self.function_labels.get(name) {
                    Operand::Label { id: label_id }
                } else {
                    // 🔧 关键修复：如果函数不在映射中，这是一个错误，不应该分配新标签
                    // 所有函数标签都应该在预处理阶段分配好
                    panic!("函数 {} 的标签未找到！这表明函数标签预分配有问题。", name);
                }
            }

            // 对于引用值，返回被引用值的地址
            Value::Reference {
                value: referenced_value,
            } => {
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

    /// 初始化栈上的值
    fn initialize_stack_value(&mut self, value: &Value, stack_addr: Register) {
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
            }

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
            }

            Value::QualifiedConstructor {
                type_name,
                constructor_name,
                arg,
            } => {
                // 创建Tagged Union for qualified constructor
                let struct_addr = self.create_tagged_union_for_qualified_constructor(
                    type_name,
                    constructor_name,
                    arg.as_deref(),
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
            }

            Value::Variable { .. } | Value::Temp { .. } => {
                // 变量和临时值：不应该在这里初始化
                // 它们的值应该通过赋值语句来设置
                // 这里不做任何操作，让它们保持未初始化状态
                // 如果需要，可以存储一个特殊的未初始化标记，但通常不需要
            }

            Value::Reference {
                value: referenced_value,
            } => {
                // 🔧 关键修复：引用值需要存储被引用值的地址
                log::debug!("🔧 初始化引用值: referenced_value={:?}", referenced_value);

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
                        log::debug!(
                            "🔧 引用值初始化完成: 存储地址{:?}到栈位置{:?}",
                            ref_addr_reg,
                            stack_addr
                        );
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
                        log::debug!(
                            "🔧 引用值初始化完成: 通过临时寄存器{:?}存储到栈位置{:?}",
                            temp_reg,
                            stack_addr
                        );
                    }
                }
            }

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
    fn handle_struct_value(
        &mut self,
        name: &str,
        fields: &std::collections::BTreeMap<String, Value>,
    ) -> Result<Register, String> {
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

                log::debug!(
                    "🔧 结构体字段初始化: {}.{} = {:?} at offset {}",
                    name,
                    field_layout.name,
                    field_value_op,
                    field_layout.offset
                );

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
        log::debug!("🔧 结构体初始化完成: {} -> {:?}", name, struct_ptr);
        Ok(struct_ptr)
    }

    /// 添加指令
    fn add_instruction(&mut self, instruction: Instruction) {
        let is_conditional_jmp = match &instruction {
            Instruction::JumpEqual { .. }
            | Instruction::JumpGreater { .. }
            | Instruction::JumpGreaterEqual { .. }
            | Instruction::JumpIndirect { .. }
            | Instruction::JumpLess { .. }
            | Instruction::JumpLessEqual { .. }
            | Instruction::JumpNotEqual { .. } => true,
            _ => false,
        };
        self.current_function_mut().add_instruction(instruction);
        // 如果是条件jmp，则自动在后面插入一个label
        if is_conditional_jmp {
            let label = self.current_function_mut().new_label();
            self.current_function_mut()
                .add_instruction(Instruction::Label {
                    id: label,
                    span: karte_diagnostics::Span::dummy(),
                });
        }
    }

    /// 🔧 专业修复：从结构体布局信息中获取字段偏移
    fn get_field_offset_from_struct_layout(
        &self,
        object: &Value,
        field_name: &str,
    ) -> Result<usize, String> {
        // 获取对象的结构体类型名称
        let struct_name = match object {
            Value::Struct { name, .. } => name.clone(),
            Value::Temp { .. } | Value::Variable { .. } => {
                // 🔧 改进：基于字段名称推断结构体类型
                match field_name {
                    "function_ptr" | "env_ptr" => "Closure".to_string(), // 闭包结构体字段
                    _ => {
                        // 如果无法推断，尝试从所有已知类型中查找包含该字段的类型
                        for layout in self.global_struct_types.values() {
                            if layout.fields.iter().any(|f| f.name == field_name) {
                                return Ok(layout
                                    .fields
                                    .iter()
                                    .find(|f| f.name == field_name)
                                    .unwrap()
                                    .offset);
                            }
                        }
                        return Err(format!(
                            "Cannot infer struct type for field '{}'",
                            field_name
                        ));
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
            Err(format!(
                "Field '{}' not found in struct '{}'",
                field_name, struct_name
            ))
        } else {
            // 对于内部结构体（如Closure），使用硬编码
            match struct_name.as_str() {
                "Closure" => match field_name {
                    "function_ptr" => Ok(0),
                    "env_ptr" => Ok(8),
                    _ => Err(format!("Unknown field '{}' in Closure", field_name)),
                },
                _ => Err(format!("Unknown struct type: {}", struct_name)),
            }
        }
    }

    /// 为构造器创建Tagged Union结构体
    fn create_tagged_union_for_constructor(&mut self, name: &str, arg: Option<&Value>) -> Register {
        let tag_id = self.tagged_union_manager.get_constructor_id(name);
        let struct_addr = self.current_function_mut().new_register();

        let data_operand = arg.map(|arg_value| self.lower_to_rvalue(arg_value));

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
    fn create_tagged_union_for_qualified_constructor(
        &mut self,
        type_name: &str,
        constructor_name: &str,
        arg: Option<&Value>,
    ) -> Register {
        let tag_id = self
            .tagged_union_manager
            .get_qualified_constructor_id(type_name, constructor_name);
        let struct_addr = self.current_function_mut().new_register();

        let data_operand = arg.map(|arg_value| self.lower_to_rvalue(arg_value));

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

    /// 预分配函数中所有临时变量的栈槽
    fn preallocate_temp_slots(&mut self, mir_function: &karte_mir::MirFunction) {
        log::debug!("🔧 开始预分配临时变量栈槽");
        // 遍历所有基本块，收集所有临时变量
        let mut temp_values = HashSet::new();

        for (block_id, block) in &mir_function.basic_blocks {
            log::debug!("🔧 检查基本块 {:?}", block_id);
            // 检查语句中的临时变量
            for statement in &block.statements {
                log::debug!("🔧 检查语句: {:?}", statement);
                match statement {
                    Statement::Assign { target, source, .. } => {
                        // 收集目标临时变量
                        if let Value::Temp { .. } = target {
                            let key = value_to_key(target);
                            log::debug!("🔧 发现临时变量(assign target): {}", key);
                            temp_values.insert(key);
                        }
                        // 也检查源值中的临时变量
                        self.collect_temp_values_from_value(source, &mut temp_values);
                    }
                    Statement::BinaryOp {
                        target,
                        left,
                        right,
                        ..
                    } => {
                        if let Value::Temp { .. } = target {
                            let key = value_to_key(target);
                            log::debug!("🔧 发现临时变量(binop target): {}", key);
                            temp_values.insert(key);
                        }
                        self.collect_temp_values_from_value(left, &mut temp_values);
                        self.collect_temp_values_from_value(right, &mut temp_values);
                    }
                    Statement::UnaryOp {
                        target, operand, ..
                    } => {
                        if let Value::Temp { .. } = target {
                            let key = value_to_key(target);
                            log::debug!("🔧 发现临时变量(unop target): {}", key);
                            temp_values.insert(key);
                        }
                        self.collect_temp_values_from_value(operand, &mut temp_values);
                    }
                    _ => {}
                }
            }

            // 检查终结器中的临时变量
            if let Some(terminator) = &block.terminator {
                log::debug!("🔧 检查终结器: {:?}", terminator);
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

        log::debug!("🔧 收集到的临时变量: {:?}", temp_values);

        // 🔧 关键修复：确保临时变量处理的确定性顺序
        let mut temp_keys: Vec<_> = temp_values.into_iter().collect();
        temp_keys.sort(); // 按字符串排序确保确定性

        let mut temp_values_to_allocate = Vec::new();
        for temp_key in temp_keys {
            log::debug!("🔧 处理临时变量key: {}", temp_key);
            // 从key重建Value（这是一个简化，实际可能需要更复杂的逻辑）
            if temp_key.starts_with("temp:") {
                // 修复：应该是 "temp:" 而不是 "temp_"
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
            log::debug!("🔧 预分配临时变量: {} -> {:?}", key, temp_value);
            let allocated_reg = self.allocate_stack_slot_for_value(&temp_value);
            log::debug!("🔧 预分配结果: {} -> {:?}", key, allocated_reg);
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
        let lir_fields: Vec<crate::StructField> = mir_struct_type
            .fields
            .iter()
            .enumerate()
            .map(|(index, field)| {
                crate::StructField {
                    name: field.name.clone(),
                    offset: index * 8, // 简化：每个字段8字节，按顺序排列
                    size: 8,
                    alignment: 8,
                }
            })
            .collect();

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
        log::debug!("分配函数标签: {} -> {:?}", name, label_id);
    }

    // 转换每个函数
    for name in &function_names {
        let mir_function = mir_program.functions.get(name).unwrap();
        // 🔧 修复：使用带参数信息的函数创建方法
        context.start_function_with_params(name.clone(), &mir_function.params);

        // 使用预分配的入口标签
        let entry_label = context
            .function_labels
            .get(name)
            .cloned()
            .expect("Function label should exist");
        context.add_instruction(Instruction::Label {
            id: entry_label,
            span: karte_diagnostics::Span::new(0, 0), // Dummy span
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
                // 如果这是一个handler入口块，绑定payload到变量（通过将r1写入变量的栈槽）
                if let Some(param_name) = context.handler_block_param.get(&block_id) {
                    // 把 r1 写入变量 param_name 的栈槽
                    let var_value = Value::Variable { name: param_name.clone() };
                    let var_addr = context.lower_to_lvalue(&var_value);
                    // 确保目标是寄存器地址
                    let addr_reg = match var_addr {
                        Operand::Register { id } => id,
                        _ => context.current_function_mut().new_register(),
                    };
                    context.add_instruction(Instruction::Store64 {
                        addr: addr_reg,
                        offset: 0,
                        src: Operand::Register { id: Register::Virtual(1) }, // r1
                        span: karte_diagnostics::Span::dummy(),
                    });
                }

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
        log::debug!("=== 返回高级LIR (包含Alloc指令，待优化) ===");
        for (name, function) in &lir_program.functions {
            log::debug!(
                "function {} (stack_frame: {}):",
                name,
                function.stack_frame_size
            );
            for instruction in &function.instructions {
                log::debug!("  {}", instruction);
            }
        }
        log::debug!("================================================");

        Ok(lir_program)
    } else {
        Err(context.errors)
    }
}

/// 转换MIR语句为LIR指令
fn lower_statement(ctx: &mut LirLoweringContext, statement: &Statement) -> Result<(), Vec<String>> {
    match statement {
        Statement::Assign {
            target,
            source,
            span: _,
        } => {
            // 🔧 修复：使用L-Value/R-Value概念
            // 赋值操作：target = source，需要source的R-Value和target的L-Value

            // 🔧 关键修复：检查是否是env_ptr相关的赋值，如果是且值为0，则跳过
            // 这是为了避免env_ptr覆盖function_ptr的问题
            log::debug!("🔧 Assignment: target={:?}, source={:?}", target, source);

            // 检查源值是否是env_ptr字段访问
            let is_env_ptr_assignment = match source {
                Value::Temp { .. } => {
                    // 对于临时变量，我们需要检查其值是否为0
                    let src_rvalue = ctx.lower_to_rvalue(source);
                    if let Operand::Immediate { value: 0 } = src_rvalue {
                        log::debug!("🔧 检测到值为0的临时变量赋值，可能是env_ptr，跳过以避免覆盖function_ptr");
                        true
                    } else {
                        false
                    }
                }
                _ => false,
            };

            if is_env_ptr_assignment {
                log::debug!("🔧 跳过env_ptr=0的赋值操作，避免覆盖function_ptr");
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
                BinaryOperator::Add => Instruction::Add {
                    dst: temp_register,
                    src1,
                    src2,
                    span: *span,
                },
                BinaryOperator::Subtract => Instruction::Sub {
                    dst: temp_register,
                    src1,
                    src2,
                    span: *span,
                },
                BinaryOperator::Multiply => Instruction::Mul {
                    dst: temp_register,
                    src1,
                    src2,
                    span: *span,
                },
                BinaryOperator::Divide => Instruction::Div {
                    dst: temp_register,
                    src1,
                    src2,
                    span: *span,
                },

                // For logical operations, we handle them differently and return a move instruction
                BinaryOperator::And
                | BinaryOperator::Or
                | BinaryOperator::Equal
                | BinaryOperator::NotEqual
                | BinaryOperator::LessThan
                | BinaryOperator::LessEqual
                | BinaryOperator::GreaterThan
                | BinaryOperator::GreaterEqual => {
                    // Handle these complex operations separately after the match
                    // For now, return a simple move to avoid type mismatch
                    Instruction::Move {
                        dst: temp_register,
                        src: Operand::Immediate { value: 0 },
                        span: *span,
                    }
                }
            };

            // 处理复杂的逻辑运算和比较运算
            match op {
                BinaryOperator::Equal
                | BinaryOperator::NotEqual
                | BinaryOperator::LessThan
                | BinaryOperator::LessEqual
                | BinaryOperator::GreaterThan
                | BinaryOperator::GreaterEqual => {
                    // 🔧 修复：确保False case的结果被正确设置
                    // 先添加比较指令
                    ctx.add_instruction(Instruction::Compare {
                        src1: src1_clone,
                        src2: src2_clone,
                        span: *span,
                    });

                    let true_label = LabelId(ctx.global_label_counter);
                    ctx.global_label_counter += 1;
                    let end_label = LabelId(ctx.global_label_counter);
                    ctx.global_label_counter += 1;

                    let jump_instr = match op {
                        BinaryOperator::Equal => Instruction::JumpEqual {
                            target: true_label,
                            span: *span,
                        },
                        BinaryOperator::NotEqual => Instruction::JumpNotEqual {
                            target: true_label,
                            span: *span,
                        },
                        BinaryOperator::LessThan => Instruction::JumpLess {
                            target: true_label,
                            span: *span,
                        },
                        BinaryOperator::LessEqual => Instruction::JumpLessEqual {
                            target: true_label,
                            span: *span,
                        },
                        BinaryOperator::GreaterThan => Instruction::JumpGreater {
                            target: true_label,
                            span: *span,
                        },
                        BinaryOperator::GreaterEqual => Instruction::JumpGreaterEqual {
                            target: true_label,
                            span: *span,
                        },
                        _ => unreachable!(),
                    };
                    ctx.add_instruction(jump_instr);

                    // 🔧 关键修复：False case - 显式设置结果为0
                    ctx.add_instruction(Instruction::Move {
                        dst: temp_register,
                        src: Operand::Immediate { value: 0 },
                        span: *span,
                    });
                    ctx.add_instruction(Instruction::Jump {
                        target: end_label,
                        span: *span,
                    });

                    // True case
                    ctx.add_instruction(Instruction::Label {
                        id: true_label,
                        span: *span,
                    });
                    ctx.add_instruction(Instruction::Move {
                        dst: temp_register,
                        src: Operand::Immediate { value: 1 },
                        span: *span,
                    });

                    // End - 🔧 关键修复：确保end_label在正确位置
                    ctx.add_instruction(Instruction::Label {
                        id: end_label,
                        span: *span,
                    });
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
                        span: *span,
                    });

                    // If src1 == 0, jump to false_label
                    ctx.add_instruction(Instruction::JumpEqual {
                        target: false_label,
                        span: *span,
                    });

                    // src1 is true (non-zero), move src2 to result
                    ctx.add_instruction(Instruction::Move {
                        dst: temp_register,
                        src: src2_clone.clone(),
                        span: *span,
                    });
                    ctx.add_instruction(Instruction::Jump {
                        target: end_label,
                        span: *span,
                    });

                    // src1 is false, result is false (0)
                    ctx.add_instruction(Instruction::Label {
                        id: false_label,
                        span: *span,
                    });
                    ctx.add_instruction(Instruction::Move {
                        dst: temp_register,
                        src: Operand::Immediate { value: 0 },
                        span: *span,
                    });

                    // End
                    ctx.add_instruction(Instruction::Label {
                        id: end_label,
                        span: *span,
                    });
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
                        span: *span,
                    });

                    // If src1 != 0, jump to true_label
                    ctx.add_instruction(Instruction::JumpNotEqual {
                        target: true_label,
                        span: *span,
                    });

                    // src1 is false, move src2 to result
                    ctx.add_instruction(Instruction::Move {
                        dst: temp_register,
                        src: src2_clone,
                        span: *span,
                    });
                    ctx.add_instruction(Instruction::Jump {
                        target: end_label,
                        span: *span,
                    });

                    // src1 is true, result is true (1)
                    ctx.add_instruction(Instruction::Label {
                        id: true_label,
                        span: *span,
                    });
                    ctx.add_instruction(Instruction::Move {
                        dst: temp_register,
                        src: Operand::Immediate { value: 1 },
                        span: *span,
                    });

                    // End
                    ctx.add_instruction(Instruction::Label {
                        id: end_label,
                        span: *span,
                    });
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
                    log::debug!("🔧 逻辑操作结果使用Stack-First存储: {:?} -> stack", target);
                }
                // 🔧 修复：比较操作的结果也使用Stack-First策略
                BinaryOperator::Equal
                | BinaryOperator::NotEqual
                | BinaryOperator::LessThan
                | BinaryOperator::LessEqual
                | BinaryOperator::GreaterThan
                | BinaryOperator::GreaterEqual => {
                    ctx.store_value_to_stack(target, Operand::Register { id: temp_register });
                    log::debug!("🔧 比较操作结果使用Stack-First存储: {:?} -> stack", target);
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
                        span: *span,
                    });

                    // Subtract src from 1: result = 1 - src
                    ctx.add_instruction(Instruction::Sub {
                        dst: temp_register,
                        src1: Operand::Register { id: temp_reg },
                        src2: src,
                        span: *span,
                    });
                }
            }

            // Stack-First策略：将结果存储到栈
            ctx.store_value_to_stack(target, Operand::Register { id: temp_register });

            Ok(())
        }

        Statement::Call {
            target,
            function,
            args,
            span,
        } => {
            // 首先解析函数值，看看是否是闭包
            let resolved_function = ctx.resolve_value(function);

            let mut all_args = Vec::new();
            let actual_function_to_call;

            // 🔧 修复：如果是闭包，需要先添加捕获的值作为环境参数
            if let Value::Closure {
                captured_values, ..
            } = &resolved_function
            {
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
                            log::debug!("🔧 Closure的env_ptr为0，不添加环境参数，不进行任何env_ptr相关的存储操作");
                            // 🔧 重要：当env_ptr为0时，完全跳过env_ptr的处理，避免错误的存储操作
                        } else {
                            // env_ptr非0，添加环境参数
                            all_args.push(env_ptr.clone());
                            log::debug!("🔧 Closure添加环境参数: {:?}", env_ptr);
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

            // 检查是否是函数参数调用
            let is_function_parameter = match &actual_function_to_call {
                Value::Variable { name } => ctx.current_function_params.contains(name),
                _ => false,
            };

            if is_function_parameter {
                // 对于函数参数，使用间接调用
                if let Value::Variable { .. } = &actual_function_to_call {
                    let function_register =
                        ctx.allocate_register_for_value(&actual_function_to_call);
                    let result_reg = target.as_ref().map(|t| ctx.allocate_register_for_value(t));

                    // 🔧 简化：不再手动设置参数，让指令降级器处理
                    // 🔧 关键修复：在调用前移动参数到正确的寄存器
                    let mut actual_arg_regs = vec![];
                    for (i, arg_op) in arg_operands.iter().enumerate() {
                        if i < 4 {
                            // 最多支持4个参数
                            let param_reg = ctx.current_function_mut().new_register();
                            ctx.add_instruction(Instruction::Move {
                                dst: param_reg,
                                src: arg_op.clone(),
                                span: *span,
                            });
                            actual_arg_regs.push(param_reg);
                        }
                    }

                    // function register 要再load一次
                    let func_ptr_reg = ctx.current_function_mut().new_register();
                    ctx.add_instruction(Instruction::Load64 {
                        dst: func_ptr_reg,
                        addr: function_register,
                        offset: 0,
                        span: *span,
                    });
                    let function_register = func_ptr_reg;

                    ctx.add_instruction(Instruction::CallIndirect {
                        function_register,
                        args: actual_arg_regs, // 参数将在指令降级阶段进一步处理
                        arg_operands: arg_operands.clone(), // 传递参数操作数
                        result: result_reg,
                        span: *span,
                    });
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

                                log::debug!("🔧 变量 {} 作为Closure：从栈地址 {:?} 加载Closure到 {:?}，再从Closure加载function_ptr到 {:?}", 
                                    name, var_stack_addr, closure_addr_reg, func_ptr_reg);
                                func_ptr_reg
                            }
                            _ => {
                                return Err(vec![
                                    "Variable address must be a register for function call"
                                        .to_string(),
                                ]);
                            }
                        };

                        // 🔧 简化：不再手动设置参数，让指令降级器处理
                        let result_reg = if target.is_some() {
                            Some(ctx.current_function_mut().new_register())
                        } else {
                            None
                        };

                        let mut actual_arg_regs = vec![];
                        for (i, arg_op) in arg_operands.iter().enumerate() {
                            if i < 4 {
                                // 最多支持4个参数
                                let param_reg = ctx.current_function_mut().new_register();
                                ctx.add_instruction(Instruction::Move {
                                    dst: param_reg,
                                    src: arg_op.clone(),
                                    span: *span,
                                });
                                actual_arg_regs.push(param_reg);
                            }
                        }
                        // function register 要再load一次
                        let func_ptr_reg = ctx.current_function_mut().new_register();
                        ctx.add_instruction(Instruction::Load64 {
                            dst: func_ptr_reg,
                            addr: function_register,
                            offset: 0,
                            span: *span,
                        });
                        let function_register = func_ptr_reg;

                        ctx.add_instruction(Instruction::CallIndirect {
                            function_register,
                            args: actual_arg_regs, // 参数将在指令降级阶段处理
                            arg_operands: arg_operands.clone(), // 传递参数操作数
                            result: result_reg,
                            span: *span,
                        });

                        // 🔧 关键修复：Stack-First策略：如果有返回值，在调用后存储到栈
                        if let (Some(target_value), Some(result_register)) = (target, result_reg) {
                            ctx.store_value_to_stack(
                                target_value,
                                Operand::Register {
                                    id: result_register,
                                },
                            );
                        }

                        // 间接调用已完成，直接返回
                        return Ok(());
                    }
                    Value::Temp { .. } => {
                        // 🔧 关键修复：对于包含函数指针的临时变量，直接使用其绑定的寄存器值
                        let function_register = if let Some(&bound_reg) = ctx
                            .stack_allocations
                            .get(&value_to_key(&actual_function_to_call))
                        {
                            // 临时变量已经绑定到寄存器，直接使用
                            log::debug!(
                                "🔧 临时变量作为函数指针：直接使用绑定的寄存器 {:?}",
                                bound_reg
                            );
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
                                    log::debug!(
                                        "🔧 临时变量作为函数指针：从栈地址 {:?} 加载到寄存器 {:?}",
                                        stack_addr,
                                        func_ptr_reg
                                    );
                                    func_ptr_reg
                                }
                                _ => {
                                    return Err(vec![
                                        "Temp variable stack address must be a register"
                                            .to_string(),
                                    ]);
                                }
                            }
                        };

                        // 🔧 简化：不再手动设置参数，让指令降级器处理
                        let result_reg = if target.is_some() {
                            Some(ctx.current_function_mut().new_register())
                        } else {
                            None
                        };
                        let mut actual_arg_regs = vec![];
                        for (i, arg_op) in arg_operands.iter().enumerate() {
                            if i < 4 {
                                // 最多支持4个参数
                                let param_reg = ctx.current_function_mut().new_register();
                                ctx.add_instruction(Instruction::Move {
                                    dst: param_reg,
                                    src: arg_op.clone(),
                                    span: *span,
                                });
                                actual_arg_regs.push(param_reg);
                            }
                        }
                        // function register 要再load一次
                        let func_ptr_reg = ctx.current_function_mut().new_register();
                        ctx.add_instruction(Instruction::Load64 {
                            dst: func_ptr_reg,
                            addr: function_register,
                            offset: 0,
                            span: *span,
                        });
                        let function_register = func_ptr_reg;

                        ctx.add_instruction(Instruction::CallIndirect {
                            function_register,
                            args: actual_arg_regs, // 参数将在指令降级阶段处理
                            arg_operands: arg_operands.clone(), // 传递参数操作数
                            result: result_reg,
                            span: *span,
                        });

                        // 🔧 关键修复：Stack-First策略：如果有返回值，在调用后存储到栈
                        if let (Some(target_value), Some(result_register)) = (target, result_reg) {
                            ctx.store_value_to_stack(
                                target_value,
                                Operand::Register {
                                    id: result_register,
                                },
                            );
                        }

                        // 间接调用已完成，直接返回
                        return Ok(());
                    }
                    _ => {
                        return Err(vec![format!(
                            "Cannot call a non-function value: {:?}",
                            actual_function_to_call
                        )]);
                    }
                };

                let target_label = ctx
                    .function_labels
                    .get(&function_name)
                    .cloned()
                    .ok_or_else(|| vec![format!("Unknown function: {}", function_name)])?;

                let result_reg = target.as_ref().map(|t| ctx.allocate_register_for_value(t));

                // 🔧 简化：不再手动设置参数，让指令降级器处理
                ctx.add_instruction(Instruction::Call {
                    target: target_label,
                    args: vec![],                       // 参数将在指令降级阶段处理
                    arg_operands: arg_operands.clone(), // 传递参数操作数
                    result: result_reg,
                    span: *span,
                });
            }

            Ok(())
        }

        Statement::FieldAccess {
            target,
            object,
            field,
            span,
        } => {
            log::debug!(
                "🔧 FieldAccess执行: target={:?}, object={:?}, field={}",
                target,
                object,
                field
            );
            // 1. 获取结构体的基地址
            let struct_base_addr = ctx.lower_to_rvalue(object);
            log::debug!("🔧 结构体基地址: {:?}", struct_base_addr);
            // 2. 计算字段偏移量
            let field_offset = ctx
                .get_field_offset_from_struct_layout(object, field)
                .map_err(|e| vec![e])?;
            log::debug!("🔧 字段 {} 偏移量: {}", field, field_offset);
            // 3. 分配目标寄存器用于存放结果
            let dst_reg = ctx.current_function_mut().new_register();
            // 4. 从 [struct_base_addr + offset] 加载字段值
            if let Operand::Register { id: base_reg } = struct_base_addr {
                ctx.add_instruction(Instruction::Add {
                    dst: dst_reg,
                    src1: Operand::Register { id: base_reg },
                    src2: Operand::Immediate {
                        value: field_offset as i64,
                    },
                    span: *span,
                });
                log::debug!(
                    "🔧 生成add指令: add {:?}, [{:?} + {}]",
                    dst_reg,
                    base_reg,
                    field_offset
                );
            } else {
                return Err(vec!["字段访问的基地址必须是寄存器".to_string()]);
            }
            // 直接将dst_reg与target绑定，不再分配独立栈槽
            let target_key = value_to_key(target);
            log::debug!(
                "🔧 FieldAccess完成: 字段{}值直接绑定到寄存器 {:?}, {}",
                field,
                dst_reg,
                target_key
            );
            ctx.stack_allocations.insert(target_key, dst_reg);
            Ok(())
        }

        Statement::Dereference {
            target,
            reference,
            span,
        } => {
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
                }
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

        Statement::ConstructorArgExtract {
            target,
            constructor,
            arg_index,
            span,
        } => {
            // Tagged Union构造器参数提取：从Tagged Union结构体中提取数据

            // 获取构造器寄存器
            let constructor_operand = ctx.lower_to_rvalue(constructor);
            let constructor_reg = match constructor_operand {
                Operand::Register { id } => id,
                _ => {
                    return Err(vec![
                        "Constructor must be a register for argument extraction".to_string(),
                    ]);
                }
            };

            // 使用Stack-First策略：为目标值分配栈槽
            let target_stack_addr = ctx.allocate_stack_slot_for_value(target);

            // 创建临时寄存器来接收提取的数据
            let temp_reg = ctx.current_function_mut().new_register();

            // 使用Tagged Union管理器生成数据提取指令到临时寄存器
            let extract_instructions = ctx
                .tagged_union_manager
                .generate_data_extraction_instructions(constructor_reg, temp_reg, *span);

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
            ctx.stack_allocations
                .insert(target_key.clone(), target_stack_addr);

            Ok(())
        }

        Statement::HeapAlloc {
            target,
            size,
            object_type,
            span,
        } => {
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

            log::debug!(
                "🔧 HeapAlloc: 分配 {} 字节的 {} 对象到 {:?}",
                size,
                object_type,
                target
            );
            Ok(())
        }

        // ===== 代数效应占位 —— 在 LIR 层发出伪指令，供后续指令降级展开 =====
        Statement::EffectPerform { tag, payload, target, span } => {
            let tag_op = ctx.lower_to_rvalue(tag);
            let payload_op = ctx.lower_to_rvalue(payload);
            let result_reg = if let Some(t) = target { Some(ctx.current_function_mut().new_register()) } else { None };

            ctx.add_instruction(Instruction::EffectPerform { tag: tag_op, payload: payload_op, result: result_reg, span: *span });

            if let Some(t) = target {
                ctx.store_value_to_stack(
                    t,
                    Operand::Register { id: result_reg.unwrap() },
                );
            }
            Ok(())
        }
        Statement::EffectResume { value, span } => {
            let val_op = ctx.lower_to_rvalue(value);
            ctx.add_instruction(Instruction::EffectResume { value: val_op, span: *span });
            Ok(())
        }
        // handler push/pop 从 MIR 到 LIR：发出 EffectPushHandler/EffectPopHandler + 在函数内使用label作为入口
        Statement::EffectHandlerPush { tag, handler_block, param_name, .. } => {
            let tag_op = ctx.lower_to_rvalue(tag);
            let handler_label = ctx.allocate_label_for_block(*handler_block);
            ctx.handler_block_param.insert(*handler_block, param_name.clone());
            ctx.add_instruction(Instruction::EffectPushHandler { tag: tag_op, handler_label, span: karte_diagnostics::Span::dummy() });
            Ok(())
        }
        Statement::EffectHandlerPop { .. } => {
            ctx.add_instruction(Instruction::EffectPopHandler { span: karte_diagnostics::Span::dummy() });
            Ok(())
        }

        Statement::Store {
            target,
            value,
            span,
        } => {
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

            log::debug!("🔧 Store: 将 {:?} 存储到地址 {:?}", value, target);
            Ok(())
        }

        _ => Err(vec![format!(
            "Statement type not yet implemented: {:?}",
            statement
        )]),
    }
}

/// 转换MIR终结语句为LIR指令
fn lower_terminator(
    ctx: &mut LirLoweringContext,
    terminator: &Terminator,
) -> Result<(), Vec<String>> {
    use karte_mir::Terminator;

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

        Terminator::Branch {
            condition,
            then_block,
            else_block,
            span,
        } => {
            // 🔧 重大修复：确保条件值是从正确的源获取的
            // 检查条件是否是比较操作的结果（临时变量）
            let condition_operand = match condition {
                Value::Temp { .. } => {
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

            log::debug!(
                "🔧 分支条件处理: condition={:?}, operand={:?}",
                condition,
                condition_operand
            );

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

        Terminator::Match {
            value,
            arms,
            default,
            span,
        } => {
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
                    karte_mir::Pattern::Number {
                        value: pattern_value,
                    } => {
                        // 数字模式：比较值是否相等
                        ctx.add_instruction(Instruction::Compare {
                            src1: match_operand.clone(),
                            src2: Operand::Immediate {
                                value: *pattern_value,
                            },
                            span: *span,
                        });
                        ctx.add_instruction(Instruction::JumpEqual {
                            target: target_label,
                            span: *span,
                        });
                    }
                    karte_mir::Pattern::Boolean {
                        value: pattern_value,
                    } => {
                        // Boolean模式：直接比较0/1值，不使用Tagged Union
                        let expected_value = if *pattern_value { 1 } else { 0 };

                        // 比较匹配值与期望值
                        ctx.add_instruction(Instruction::Compare {
                            src1: match_operand.clone(),
                            src2: Operand::Immediate {
                                value: expected_value,
                            },
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
                                ctx.errors.push(
                                    "Match operand must be a register for constructor pattern"
                                        .to_string(),
                                );
                                continue;
                            }
                        };

                        // 获取期望的标签ID
                        let expected_tag_id = if name.contains("::") {
                            let parts: Vec<&str> = name.split("::").collect();
                            if parts.len() == 2 {
                                ctx.tagged_union_manager
                                    .get_qualified_constructor_id(parts[0], parts[1])
                            } else {
                                ctx.tagged_union_manager.get_constructor_id(name)
                            }
                        } else {
                            ctx.tagged_union_manager.get_constructor_id(name)
                        };

                        // constructor_reg直接包含Tagged Union的地址
                        // 生成标签检查指令
                        let temp_reg = ctx.current_function_mut().new_register();
                        let tag_check_instructions =
                            ctx.tagged_union_manager.generate_tag_check_instructions(
                                constructor_reg, // 直接使用constructor_reg作为Tagged Union地址
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
                            let var_reg_id = ctx.allocate_register_for_value(&Value::Variable {
                                name: var_name.clone(),
                            });
                            let extract_instructions = ctx
                                .tagged_union_manager
                                .generate_data_extraction_instructions(
                                    constructor_reg, // 直接使用constructor_reg作为Tagged Union地址
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
        Value::Closure {
            function_name,
            captured_values,
        } => {
            let captured_str = captured_values
                .iter()
                .map(value_to_key)
                .collect::<Vec<_>>()
                .join(",");
            format!("closure:{}:({})", function_name, captured_str)
        }
        // Note: This is a simplification. Hash of constructor/struct would be better
        Value::Constructor { name, arg } => format!("ctor:{}({:?})", name, arg),
        Value::QualifiedConstructor {
            type_name,
            constructor_name,
            arg,
        } => format!("qctor:{}::{}({:?})", type_name, constructor_name, arg),
        Value::Number { value } => format!("num:{}", value),
        Value::Boolean { value } => format!("bool:{}", value),
        Value::Unit => "unit".to_string(),
        Value::Struct { name, fields } => {
            let fields_str = fields
                .iter()
                .map(|(k, v)| format!("{}:{}", k, value_to_key(v)))
                .collect::<Vec<_>>()
                .join(",");
            format!("struct:{}({})", name, fields_str)
        }
        Value::Reference { value } => {
            format!("ref:({})", value_to_key(value))
        }
    }
}
