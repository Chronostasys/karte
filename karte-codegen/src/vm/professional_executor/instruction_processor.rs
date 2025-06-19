//! 指令处理器
//! 
//! 负责处理所有LIR指令的执行，提供完整的指令集支持

use super::{ExecutionEngine, ProgramManager, InstructionResult};
use karte_lir::{Instruction, RegisterId, Operand, LabelId};
use std::collections::HashMap;

/// 指令处理器
/// 
/// 处理所有类型的LIR指令，将它们转换为执行引擎操作
#[derive(Debug)]
pub struct InstructionProcessor {
    // 指令处理器目前不需要状态，所有状态都在执行引擎中
}

impl InstructionProcessor {
    /// 创建新的指令处理器
    pub fn new() -> Self {
        Self {}
    }

    /// 处理单条指令
    pub fn process_instruction(
        &mut self,
        instruction: &Instruction,
        engine: &mut ExecutionEngine,
        program_manager: &ProgramManager,
    ) -> Result<InstructionResult, String> {
        match instruction {
            // 数据移动指令
            Instruction::Move { dst, src, .. } => {
                self.handle_move(dst, src, engine)
            }

            // 算术运算指令
            Instruction::Add { dst, src1, src2, .. } => {
                self.handle_add(dst, src1, src2, engine)
            }
            Instruction::Sub { dst, src1, src2, .. } => {
                self.handle_sub(dst, src1, src2, engine)
            }
            Instruction::Mul { dst, src1, src2, .. } => {
                self.handle_mul(dst, src1, src2, engine)
            }
            Instruction::Div { dst, src1, src2, .. } => {
                self.handle_div(dst, src1, src2, engine)
            }

            // 比较指令
            Instruction::Compare { src1, src2, .. } => {
                self.handle_compare(src1, src2, engine)
            }

            // 跳转指令
            Instruction::Jump { target, .. } => {
                self.handle_jump(target, program_manager)
            }
            Instruction::JumpEqual { target, .. } => {
                self.handle_conditional_jump(target, engine.check_equal(), program_manager)
            }
            Instruction::JumpNotEqual { target, .. } => {
                self.handle_conditional_jump(target, !engine.check_equal(), program_manager)
            }
            Instruction::JumpLess { target, .. } => {
                self.handle_conditional_jump(target, engine.check_less(), program_manager)
            }
            Instruction::JumpLessEqual { target, .. } => {
                self.handle_conditional_jump(target, engine.check_less_equal(), program_manager)
            }
            Instruction::JumpGreater { target, .. } => {
                self.handle_conditional_jump(target, engine.check_greater(), program_manager)
            }
            Instruction::JumpGreaterEqual { target, .. } => {
                self.handle_conditional_jump(target, engine.check_greater_equal(), program_manager)
            }

            // 函数调用和返回
            Instruction::Call { target, args, result, .. } => {
                self.handle_call(target, args, result.as_ref(), engine, program_manager)
            }
            Instruction::CallIndirect { function_register, args, result, .. } => {
                self.handle_call_indirect(function_register, args, result.as_ref(), engine, program_manager)
            }
            Instruction::Return { value, .. } => {
                self.handle_return(value.as_ref(), engine)
            }

            // 标签（无操作）
            Instruction::Label { .. } => {
                Ok(InstructionResult::Continue)
            }

            // 无操作指令
            Instruction::Nop { .. } => {
                Ok(InstructionResult::Continue)
            }

            // 内存操作指令
            Instruction::Store64 { addr, offset, src, .. } => {
                self.handle_store64(addr, *offset, src, engine)
            }
            Instruction::Load64 { dst, addr, offset, .. } => {
                self.handle_load64(dst, addr, *offset, engine)
            }
            Instruction::Alloc { dst, size, alignment, allocation_type, .. } => {
                self.handle_alloc(dst, *size, *alignment, engine)
            }
            Instruction::StructFieldLoad { dst, struct_addr, field_offset, .. } => {
                self.handle_struct_field_load(dst, struct_addr, *field_offset, engine)
            }

            // 其他指令暂时返回错误
            _ => {
                Err(format!("Unsupported instruction: {:?}", instruction))
            }
        }
    }

    /// 处理移动指令
    fn handle_move(
        &mut self,
        dst: &RegisterId,
        src: &Operand,
        engine: &mut ExecutionEngine,
    ) -> Result<InstructionResult, String> {
        // 检查是否是特殊的存储操作
        // 如果src是特殊标记寄存器(999)，这表示是一个存储操作
        if let Operand::Register { id } = src {
            if id.0 == 999 {
                // 这是一个存储操作：dst是内存地址，src是要存储的值
                let addr = engine.get_register(dst)? as usize;
                let value = engine.get_register(id)?;
                
                println!("执行存储操作: 地址={}, 值={}", addr, value);
                
                // 执行内存存储
                engine.store_memory(addr, value)?;
                return Ok(InstructionResult::Continue);
            }
        }
        
        // 普通的移动操作
        let value = engine.get_operand_value(src)?;
        engine.set_register(dst, value)?;
        Ok(InstructionResult::Continue)
    }

    /// 处理加法指令
    fn handle_add(
        &mut self,
        dst: &RegisterId,
        src1: &Operand,
        src2: &Operand,
        engine: &mut ExecutionEngine,
    ) -> Result<InstructionResult, String> {
        let val1 = engine.get_operand_value(src1)?;
        let val2 = engine.get_operand_value(src2)?;
        engine.set_register(dst, val1 + val2)?;
        Ok(InstructionResult::Continue)
    }

    /// 处理减法指令
    fn handle_sub(
        &mut self,
        dst: &RegisterId,
        src1: &Operand,
        src2: &Operand,
        engine: &mut ExecutionEngine,
    ) -> Result<InstructionResult, String> {
        let val1 = engine.get_operand_value(src1)?;
        let val2 = engine.get_operand_value(src2)?;
        engine.set_register(dst, val1 - val2)?;
        Ok(InstructionResult::Continue)
    }

    /// 处理乘法指令
    fn handle_mul(
        &mut self,
        dst: &RegisterId,
        src1: &Operand,
        src2: &Operand,
        engine: &mut ExecutionEngine,
    ) -> Result<InstructionResult, String> {
        let val1 = engine.get_operand_value(src1)?;
        let val2 = engine.get_operand_value(src2)?;
        engine.set_register(dst, val1 * val2)?;
        Ok(InstructionResult::Continue)
    }

    /// 处理除法指令
    fn handle_div(
        &mut self,
        dst: &RegisterId,
        src1: &Operand,
        src2: &Operand,
        engine: &mut ExecutionEngine,
    ) -> Result<InstructionResult, String> {
        let val1 = engine.get_operand_value(src1)?;
        let val2 = engine.get_operand_value(src2)?;
        if val2 == 0 {
            return Err("Division by zero".to_string());
        }
        engine.set_register(dst, val1 / val2)?;
        Ok(InstructionResult::Continue)
    }

    /// 处理比较指令
    fn handle_compare(
        &mut self,
        src1: &Operand,
        src2: &Operand,
        engine: &mut ExecutionEngine,
    ) -> Result<InstructionResult, String> {
        let val1 = engine.get_operand_value(src1)?;
        let val2 = engine.get_operand_value(src2)?;
        engine.compare(val1, val2);
        Ok(InstructionResult::Continue)
    }

    /// 处理无条件跳转
    fn handle_jump(
        &mut self,
        target: &LabelId,
        program_manager: &ProgramManager,
    ) -> Result<InstructionResult, String> {
        let target_pc = program_manager.get_label_pc(target)?;
        Ok(InstructionResult::Jump(target_pc))
    }

    /// 处理条件跳转
    fn handle_conditional_jump(
        &mut self,
        target: &LabelId,
        condition: bool,
        program_manager: &ProgramManager,
    ) -> Result<InstructionResult, String> {
        if condition {
            let target_pc = program_manager.get_label_pc(target)?;
            Ok(InstructionResult::Jump(target_pc))
        } else {
            Ok(InstructionResult::Continue)
        }
    }

    /// 处理函数调用
    fn handle_call(
        &mut self,
        target: &LabelId,
        args: &[RegisterId],
        result: Option<&RegisterId>,
        engine: &mut ExecutionEngine,
        program_manager: &ProgramManager,
    ) -> Result<InstructionResult, String> {
        // 1. 准备参数 - 将参数值传递到参数寄存器
        let mut arg_values = Vec::new();
        for (i, arg_reg) in args.iter().enumerate() {
            let arg_value = engine.get_register(arg_reg)?;
            arg_values.push(arg_value);
            
            // 如果有足够的参数寄存器，设置参数寄存器
            if i < engine.get_calling_convention().argument_registers.len() {
                let param_reg = engine.get_calling_convention().argument_registers[i];
                engine.set_register(&RegisterId(param_reg as usize), arg_value)?;
            }
        }

        // 2. 保存调用者状态（简化版本）
        let current_pc = engine.get_pc();
        
        // 3. 跳转到目标函数
        let target_pc = program_manager.get_label_pc(target)?;
        
        // 4. 标记这是一个函数调用，需要在返回时恢复状态
        engine.push_call_frame(current_pc + 1, result.cloned())?;
        
        Ok(InstructionResult::Jump(target_pc))
    }

    /// 处理间接函数调用
    fn handle_call_indirect(
        &mut self,
        function_register: &RegisterId,
        args: &[RegisterId],
        result: Option<&RegisterId>,
        engine: &mut ExecutionEngine,
        program_manager: &ProgramManager,
    ) -> Result<InstructionResult, String> {
        // 🔧 实现完整的间接函数调用机制
        
        // 1. 获取函数地址（标签ID）
        let function_address = engine.get_register(function_register)?;
        let target_label = karte_lir::LabelId(function_address as usize);
        
        println!("CallIndirect 调试信息:");
        println!("  function_register: {:?}", function_register);
        println!("  function_address: {}", function_address);
        println!("  target_label: {:?}", target_label);
        
        // 2. 准备参数 - 将参数值传递到参数寄存器
        let mut arg_values = Vec::new();
        for (i, arg_reg) in args.iter().enumerate() {
            let arg_value = engine.get_register(arg_reg)?;
            arg_values.push(arg_value);
            
            println!("  参数{}: 寄存器{:?} = {}", i, arg_reg, arg_value);
            
            // 如果有足够的参数寄存器，设置参数寄存器
            if i < engine.get_calling_convention().argument_registers.len() {
                let param_reg = engine.get_calling_convention().argument_registers[i];
                engine.set_register(&RegisterId(param_reg as usize), arg_value)?;
                println!("  -> 设置参数寄存器r{} = {}", param_reg, arg_value);
            }
        }
 
        // 5. 保存调用者状态
        let current_pc = engine.get_pc();
        
        // 6. 查找目标函数PC
        let target_pc = match program_manager.get_label_pc(&target_label) {
            Ok(pc) => {
                println!("  -> 找到目标函数PC: {}", pc);
                pc
            }
            Err(e) => {
                println!("  -> 错误：无法找到函数地址 {:?}: {}", target_label, e);
                println!("  -> 可用标签: {:?}", program_manager.get_all_labels());
                return Err(format!("Function address {:?} not found: {}", target_label, e));
            }
        };
        
        // 7. 标记这是一个函数调用，需要在返回时恢复状态
        engine.push_call_frame(current_pc + 1, result.cloned())?;
        
        println!("  -> 成功跳转到PC: {} (标签: {:?})", target_pc, target_label);
        
        Ok(InstructionResult::Jump(target_pc))
    }

    /// 处理函数返回
    fn handle_return(
        &mut self,
        value: Option<&RegisterId>,
        engine: &mut ExecutionEngine,
    ) -> Result<InstructionResult, String> {
        let return_value = if let Some(reg) = value {
            engine.get_register(reg)?
        } else {
            0 // 无返回值的函数返回0
        };
        
        Ok(InstructionResult::Return(return_value))
    }

    /// 处理Store64指令
    fn handle_store64(
        &mut self,
        addr: &RegisterId,
        offset: i64,
        src: &Operand,
        engine: &mut ExecutionEngine,
    ) -> Result<InstructionResult, String> {
        let base_addr = engine.get_register(addr)? as usize;
        let store_addr = (base_addr as i64 + offset) as usize;
        let value = engine.get_operand_value(src)?;
        
        println!("Store64: storing {} to address {} (base {} + offset {})", 
            value, store_addr, base_addr, offset);
        
        engine.store_memory(store_addr, value)?;
        Ok(InstructionResult::Continue)
    }

    /// 处理Load64指令
    fn handle_load64(
        &mut self,
        dst: &RegisterId,
        addr: &RegisterId,
        offset: i64,
        engine: &mut ExecutionEngine,
    ) -> Result<InstructionResult, String> {
        let base_addr = engine.get_register(addr)? as usize;
        let load_addr = (base_addr as i64 + offset) as usize;
        let value = engine.load_memory(load_addr)?;
        
        println!("Load64: loaded {} from address {} (base {} + offset {})", 
            value, load_addr, base_addr, offset);
        
        engine.set_register(dst, value)?;
        Ok(InstructionResult::Continue)
    }

    /// 处理Alloc指令
    fn handle_alloc(
        &mut self,
        dst: &RegisterId,
        size: usize,
        _alignment: usize,
        engine: &mut ExecutionEngine,
    ) -> Result<InstructionResult, String> {
        // 简化的内存分配：使用递增地址
        static mut NEXT_ADDR: usize = 1048544; // 从栈之后开始分配
        
        let addr = unsafe {
            let current = NEXT_ADDR;
            NEXT_ADDR += size;
            current
        };
        
        println!("Alloc: allocated {} bytes at address {}", size, addr);
        
        engine.set_register(dst, addr as i64)?;
        Ok(InstructionResult::Continue)
    }

    /// 处理结构体字段加载
    fn handle_struct_field_load(
        &mut self,
        dst: &RegisterId,
        struct_addr: &RegisterId,
        field_offset: usize,
        engine: &mut ExecutionEngine,
    ) -> Result<InstructionResult, String> {
        // 获取结构体的基地址
        let base_addr = engine.get_register(struct_addr)? as usize;
        
        // 计算字段的实际地址
        let field_addr = base_addr + field_offset;
        
        // 从字段地址加载值
        let value = engine.load_memory(field_addr)?;
        
        println!("StructFieldLoad: loaded {} from field at address {} (base {} + offset {})", 
            value, field_addr, base_addr, field_offset);
        
        // 将值存储到目标寄存器
        engine.set_register(dst, value)?;
        
        Ok(InstructionResult::Continue)
    }
}

impl Default for InstructionProcessor {
    fn default() -> Self {
        Self::new()
    }
} 