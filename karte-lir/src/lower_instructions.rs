//! LIR指令降级器
//! 
//! 将高级LIR指令（如Alloc、Load64、Store64等）降级为更基础的指令组合
//! 这样虚拟机只需要支持最基础的指令集

use crate::{Instruction, RegisterId, Operand, LirFunction, LirProgram, AllocationType};
use karte_diagnostics::Span;

/// 指令降级器
pub struct InstructionLowerer {
    /// 栈指针寄存器（固定使用寄存器6）
    stack_pointer_reg: RegisterId,
    /// 帧指针寄存器（固定使用寄存器7）
    frame_pointer_reg: RegisterId,
}

impl InstructionLowerer {
    /// 创建新的指令降级器
    pub fn new() -> Self {
        Self {
            // 使用寄存器6作为栈指针（SP），与调用约定匹配
            stack_pointer_reg: RegisterId(6),
            // 使用寄存器7作为帧指针（FP），与调用约定匹配
            frame_pointer_reg: RegisterId(7),
        }
    }

    /// 降级整个LIR程序
    pub fn lower_program(&mut self, program: &mut LirProgram) -> Result<(), String> {
        // 为每个函数降级指令
        for (_, function) in program.functions.iter_mut() {
            self.lower_function(function)?;
        }
        Ok(())
    }

    /// 降级单个函数的指令
    pub fn lower_function(&mut self, function: &mut LirFunction) -> Result<(), String> {
        let mut new_instructions = Vec::new();
        
        // 克隆指令列表以避免借用检查问题
        let instructions_to_process = function.instructions.clone();
        
        for instruction in &instructions_to_process {
            match instruction {
                // 降级 Alloc 指令
                Instruction::Alloc { dst, size, alignment, allocation_type, span } => {
                    self.lower_alloc(dst, *size, *alignment, allocation_type, span, &mut new_instructions, function)?;
                }
                
                // 降级 Load64 指令
                Instruction::Load64 { dst, addr, offset, span } => {
                    self.lower_load64(dst, addr, *offset, span, &mut new_instructions)?;
                }
                
                // 降级 Store64 指令
                Instruction::Store64 { addr, offset, src, span } => {
                    self.lower_store64(addr, *offset, src, span, &mut new_instructions)?;
                }
                
                // 降级 StructAlloc 指令
                Instruction::StructAlloc { dst, struct_type, allocation_type, span } => {
                    self.lower_struct_alloc(dst, struct_type, allocation_type, span, &mut new_instructions, function)?;
                }
                
                // 降级 StructFieldLoad 指令
                Instruction::StructFieldLoad { dst, struct_addr, field_offset, span } => {
                    self.lower_struct_field_load(dst, struct_addr, *field_offset, span, &mut new_instructions)?;
                }
                
                // 降级 StructFieldStore 指令
                Instruction::StructFieldStore { struct_addr, field_offset, src, span } => {
                    self.lower_struct_field_store(struct_addr, *field_offset, src, span, &mut new_instructions)?;
                }
                
                // 其他指令直接保留
                _ => {
                    new_instructions.push(instruction.clone());
                }
            }
        }
        
        function.instructions = new_instructions;
        Ok(())
    }

    /// 降级 Alloc 指令为栈指针操作
    fn lower_alloc(
        &mut self,
        dst: &RegisterId,
        size: usize,
        _alignment: usize,
        allocation_type: &AllocationType,
        span: &Span,
        instructions: &mut Vec<Instruction>,
        function: &mut LirFunction,
    ) -> Result<(), String> {
        match allocation_type {
            AllocationType::Stack => {
                // 栈分配：SP = SP - size; dst = SP (新的栈指针值)
                
                // 1. 获取栈指针寄存器（确保使用正确的栈指针）
                let stack_pointer = function.get_stack_pointer_register();
                
                // 2. 使用临时寄存器存储size值（确保不使用栈指针寄存器）
                let temp_reg = function.new_register(); // 这个方法现在会跳过栈指针寄存器
                
                instructions.push(Instruction::Move {
                    dst: temp_reg,
                    src: Operand::Immediate { value: size as i64 },
                    span: *span,
                });
                
                // 3. SP = SP - size (移动栈指针)
                instructions.push(Instruction::Sub {
                    dst: stack_pointer,
                    src1: Operand::Register { id: stack_pointer },
                    src2: Operand::Register { id: temp_reg },
                    span: *span,
                });
                
                // 4. dst = SP (将新的栈指针值作为分配的地址)
                instructions.push(Instruction::Move {
                    dst: *dst,
                    src: Operand::Register { id: stack_pointer },
                    span: *span,
                });
            }
            _ => {
                return Err(format!("Unsupported allocation type: {:?}", allocation_type));
            }
        }
        Ok(())
    }

    /// 降级 Load64 指令为基础内存操作
    fn lower_load64(
        &mut self,
        dst: &RegisterId,
        addr: &RegisterId,
        offset: i64,
        span: &Span,
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), String> {
        if offset == 0 {
            // 简单情况：直接从地址加载
            // 这里我们假设虚拟机支持 Memory 操作数
            instructions.push(Instruction::Move {
                dst: *dst,
                src: Operand::Memory { base: *addr, offset: 0 },
                span: *span,
            });
        } else {
            // 复杂情况：需要计算地址
            // 暂时简化为直接使用 Memory 操作数的偏移
            instructions.push(Instruction::Move {
                dst: *dst,
                src: Operand::Memory { base: *addr, offset },
                span: *span,
            });
        }
        Ok(())
    }

    /// 降级 Store64 指令为基础内存操作
    fn lower_store64(
        &mut self,
        addr: &RegisterId,
        offset: i64,
        src: &Operand,
        span: &Span,
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), String> {
        // 1. 将要存储的值移动到特殊标记寄存器
        let store_marker_reg = RegisterId(999); // 特殊标记寄存器
        instructions.push(Instruction::Move {
            dst: store_marker_reg,
            src: src.clone(),
            span: *span,
        });
        
        // 2. 计算目标地址并执行存储操作
        if offset == 0 {
            // 简单情况：直接使用地址寄存器
            // 专业执行器会识别RegisterId(999)并执行存储操作
            instructions.push(Instruction::Move {
                dst: *addr,
                src: Operand::Register { id: store_marker_reg },
                span: *span,
            });
        } else {
            // 复杂情况：需要计算 addr + offset
            let temp_offset_reg = RegisterId(998); // 使用特殊寄存器避免冲突
            let temp_addr_reg = RegisterId(997);   // 使用特殊寄存器避免冲突
            
            // temp_offset_reg = offset
            instructions.push(Instruction::Move {
                dst: temp_offset_reg,
                src: Operand::Immediate { value: offset },
                span: *span,
            });
            
            // temp_addr_reg = addr + offset
            instructions.push(Instruction::Add {
                dst: temp_addr_reg,
                src1: Operand::Register { id: *addr },
                src2: Operand::Register { id: temp_offset_reg },
                span: *span,
            });
            
            // 执行存储操作：将标记寄存器的值存储到计算出的地址
            instructions.push(Instruction::Move {
                dst: temp_addr_reg,
                src: Operand::Register { id: store_marker_reg },
                span: *span,
            });
        }
        
        Ok(())
    }

    /// 降级 StructAlloc 指令
    fn lower_struct_alloc(
        &mut self,
        dst: &RegisterId,
        struct_type: &crate::StructTypeId,
        allocation_type: &AllocationType,
        span: &Span,
        instructions: &mut Vec<Instruction>,
        function: &mut LirFunction,
    ) -> Result<(), String> {
        // 获取结构体大小
        let struct_layout = function.struct_types.get(struct_type)
            .ok_or_else(|| format!("Unknown struct type: {:?}", struct_type))?;
        
        // 降级为普通的 Alloc 指令
        self.lower_alloc(dst, struct_layout.total_size, struct_layout.alignment, allocation_type, span, instructions, function)
    }

    /// 降级 StructFieldLoad 指令
    fn lower_struct_field_load(
        &mut self,
        dst: &RegisterId,
        struct_addr: &RegisterId,
        field_offset: usize,
        span: &Span,
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), String> {
        // 降级为 Load64 指令
        self.lower_load64(dst, struct_addr, field_offset as i64, span, instructions)
    }

    /// 降级 StructFieldStore 指令
    fn lower_struct_field_store(
        &mut self,
        struct_addr: &RegisterId,
        field_offset: usize,
        src: &Operand,
        span: &Span,
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), String> {
        // 降级为 Store64 指令
        self.lower_store64(struct_addr, field_offset as i64, src, span, instructions)
    }
}

impl Default for InstructionLowerer {
    fn default() -> Self {
        Self::new()
    }
}

/// 降级整个LIR程序的指令
pub fn lower_program_instructions(program: &mut LirProgram) -> Result<(), String> {
    let mut lowerer = InstructionLowerer::new();
    
    // 降级指令
    lowerer.lower_program(program)?;
    
    // 验证栈指针寄存器使用规则
    for (function_name, function) in &program.functions {
        if let Err(validation_error) = function.validate_stack_pointer_usage() {
            return Err(format!(
                "函数 '{}' 中的栈指针使用验证失败: {}",
                function_name,
                validation_error
            ));
        }
    }
    
    Ok(())
} 