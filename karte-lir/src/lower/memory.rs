//! LIR降低过程的内存和寄存器管理
//!
//! 本模块包含栈分配、寄存器分配、结构体布局管理等功能。

use super::helpers::{
    collect_function_names_from_statement, collect_function_names_from_value, value_to_key,
};
use super::types::LirLoweringContext;
use crate::{AllocationType, Instruction, Operand, Register, StructField, StructLayout};
use karte_mir::{MirFunction, Statement, TempId, Terminator, Value};
use std::collections::HashSet;

impl LirLoweringContext {
    pub(super) fn set_struct_layout_for_value(&mut self, value: &Value, layout: StructLayout) {
        self.struct_value_layouts
            .insert(value_to_key(value), layout);
    }

    /// 清理某个值的结构体布局记录
    pub(super) fn clear_struct_layout_for_value(&mut self, value: &Value) {
        self.struct_value_layouts.remove(&value_to_key(value));
    }

    /// 如果源值携带结构体布局，则将其传播到目标值
    pub(super) fn propagate_struct_layout(&mut self, target: &Value, source: &Value) {
        if let Some(layout) = self.get_struct_layout_for_value(source) {
            self.set_struct_layout_for_value(target, layout);
        } else {
            self.clear_struct_layout_for_value(target);
        }
    }

    /// 获取某个值对应的结构体布局（如有）
    pub(super) fn get_struct_layout_for_value(&self, value: &Value) -> Option<StructLayout> {
        match value {
            Value::Struct { name, .. } => self.global_struct_types.get(name).cloned(),
            Value::Temp { .. } | Value::Variable { .. } => {
                self.struct_value_layouts.get(&value_to_key(value)).cloned()
            }
            _ => None,
        }
    }

    pub(super) fn allocate_stack_slot_for_value(&mut self, value: &Value) -> Register {
        self.allocate_stack_slot_for_value_with_instruction(value, true)
    }

    /// 为值分配栈槽，可以选择是否生成alloc指令
    pub(super) fn allocate_stack_slot_for_value_with_instruction(
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
            log::debug!("💾 store_value_to_stack: value={:?}, existing_addr={:?}, src={:?}",
                value, existing_addr, src_operand);
            existing_addr
        } else {
            // 如果没有分配，现在分配
            let addr = self.allocate_stack_slot_for_value(value);
            self.stack_allocations.insert(value_key.clone(), addr);
            log::debug!("💾 store_value_to_stack: value={:?}, NEW_addr={:?}, src={:?}",
                value, addr, src_operand);
            addr
        };

        // 存储值到栈上
        log::debug!("💾 Store64: stack_addr={:?}, src={:?}", stack_addr, src_operand);
        self.add_instruction(Instruction::Store64 {
            addr: stack_addr,
            offset: 0,
            src: src_operand,
            span: karte_diagnostics::Span::dummy(),
        });
    }

    pub(super) fn allocate_register_for_value(&mut self, value: &Value) -> Register {
        // 检查是否是函数参数 - 函数参数仍然使用寄存器传递
        if let Value::Variable { name, .. } = value {
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

    pub(super) fn lower_to_lvalue(&mut self, value: &Value) -> Operand {
        let value_key = value_to_key(value);

        // 🔧 专业修复：正确处理结构体值的初始化
        match value {
            Value::Struct { name, fields, .. } => {
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
        if let Value::Variable { name, .. } = value {
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

    pub(super) fn lower_to_rvalue(&mut self, value: &Value) -> Operand {
        // 🔧 修复：特殊处理函数参数 - 直接使用参数寄存器
        if let Value::Variable { name, .. } = value {
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
            Value::Number { value, .. } => Operand::Immediate { value: *value },
            Value::Boolean { value, .. } => Operand::Immediate {
                value: if *value { 1 } else { 0 },
            },
            Value::Unit => Operand::Immediate { value: 0 },

            // 函数值
            Value::Function { name, .. } => {
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
                ..
            } => {
                // 🔧 关键修复：区分两种情况
                // 1. & x (x 是栈变量) -> 返回 x 的地址（L-Value）
                // 2. & %10000 (堆分配的临时变量) -> 返回 %10000 存储的值（R-Value），即堆地址本身

                // 检查是否是逃逸分析插入的堆地址临时变量（ID >= 10000）
                match referenced_value.as_ref() {
                    Value::Temp { id, .. } if id.0 >= 10000 => {
                        // 这是逃逸分析插入的堆地址临时变量
                        // 应该返回它的值（堆地址），而不是它的栈位置
                        self.lower_to_rvalue(referenced_value)
                    }
                    _ => {
                        // 普通引用：返回被引用值的地址
                        self.lower_to_lvalue(referenced_value)
                    }
                }
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
                            log::debug!("🔍 lower_to_rvalue: 为 {:?} 生成Load64指令: addr={:?}, dst={:?}",
                                value, addr_reg, temp_reg);
                            self.add_instruction(Instruction::Load64 {
                                dst: temp_reg,
                                addr: addr_reg,
                                offset: 0,
                                span: karte_diagnostics::Span::dummy(),
                            });
                            Operand::Register { id: temp_reg }
                        } else {
                            // 如果L-Value不是寄存器，直接返回
                            log::warn!("⚠️ lower_to_rvalue: lvalue不是Register，直接返回地址！value={:?}, lvalue={:?}",
                                value, lvalue);
                            lvalue
                        }
                    }
                }
            }
        }
    }

    /// 初始化栈上的值

    pub(super) fn initialize_stack_value(&mut self, value: &Value, stack_addr: Register) {
        match value {
            Value::Boolean { value, .. } => {
                // Bool特殊处理：直接存储0/1值，不使用Tagged Union
                let bool_value = if *value { 1 } else { 0 };
                self.add_instruction(Instruction::Store64 {
                    addr: stack_addr,
                    offset: 0,
                    src: Operand::Immediate { value: bool_value },
                    span: karte_diagnostics::Span::dummy(),
                });
            }

            Value::Constructor { name, arg, .. } => {
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
                ..
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
                ..
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

    pub(super) fn handle_struct_value(
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

    pub(super) fn get_field_offset_from_struct_layout(
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

    pub(super) fn create_tagged_union_for_constructor(
        &mut self,
        name: &str,
        arg: Option<&Value>,
    ) -> Register {
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

    pub(super) fn create_tagged_union_for_qualified_constructor(
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

    pub(super) fn resolve_value(&self, value: &Value) -> Value {
        // 简化逻辑：直接返回原值，不做复杂的映射解析
        // 这样可以避免复杂的value_mapping逻辑
        // Stack-First策略会将所有值都明确地分配到栈上，
        // 不需要复杂的间接引用解析
        value.clone()
    }

    /// 预分配函数中所有临时变量的栈槽

    pub(super) fn preallocate_temp_slots(&mut self, mir_function: &karte_mir::MirFunction) {
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
                    let temp_value = Value::Temp { id: TempId(id), ty: None };
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

    pub(super) fn collect_temp_values_from_value(
        &self,
        value: &Value,
        temp_values: &mut HashSet<String>,
    ) {
        match value {
            Value::Temp { .. } => {
                temp_values.insert(value_to_key(value));
            }
            Value::Reference { value: inner, .. } => {
                self.collect_temp_values_from_value(inner, temp_values);
            }
            _ => {}
        }
    }
}
