use crate::ir::*;
use std::fmt;

impl fmt::Display for RegisterId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "r{}", self.0)
    }
}

impl fmt::Display for LabelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "L{}", self.0)
    }
}

impl fmt::Display for StructTypeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "struct{}", self.0)
    }
}

impl fmt::Display for MemoryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "mem{}", self.0)
    }
}

impl fmt::Display for AllocationType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AllocationType::Stack => write!(f, "stack"),
            AllocationType::Heap => write!(f, "heap"),
            AllocationType::Static => write!(f, "static"),
        }
    }
}

impl fmt::Display for Operand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Operand::Register { id } => write!(f, "{}", id),
            Operand::Immediate { value } => write!(f, "#{}", value),
            Operand::Label { id } => write!(f, "{}", id),
            Operand::Memory { base, offset } => {
                if *offset == 0 {
                    write!(f, "[{}]", base)
                } else {
                    write!(f, "[{} + {}]", base, offset)
                }
            }
            Operand::StructField { struct_addr, field_offset } => {
                write!(f, "[{} + field{}]", struct_addr, field_offset)
            }
            Operand::MemoryRef { id } => write!(f, "{}", id),
        }
    }
}

impl fmt::Display for Instruction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Instruction::Move { dst, src, .. } => write!(f, "mov {}, {}", dst, src),
            Instruction::Add {
                dst, src1, src2, ..
            } => {
                write!(f, "add {}, {}, {}", dst, src1, src2)
            }
            Instruction::Sub {
                dst, src1, src2, ..
            } => {
                write!(f, "sub {}, {}, {}", dst, src1, src2)
            }
            Instruction::Mul {
                dst, src1, src2, ..
            } => {
                write!(f, "mul {}, {}, {}", dst, src1, src2)
            }
            Instruction::Div {
                dst, src1, src2, ..
            } => {
                write!(f, "div {}, {}, {}", dst, src1, src2)
            }
            Instruction::Compare { src1, src2, .. } => write!(f, "cmp {}, {}", src1, src2),
            Instruction::Jump { target, .. } => write!(f, "jmp {}", target),
            Instruction::JumpEqual { target, .. } => write!(f, "je {}", target),
            Instruction::JumpNotEqual { target, .. } => write!(f, "jne {}", target),
            Instruction::JumpGreater { target, .. } => write!(f, "jg {}", target),
            Instruction::JumpGreaterEqual { target, .. } => write!(f, "jge {}", target),
            Instruction::JumpLess { target, .. } => write!(f, "jl {}", target),
            Instruction::JumpLessEqual { target, .. } => write!(f, "jle {}", target),
            Instruction::Call {
                target,
                args,
                result,
                ..
            } => {
                if let Some(result) = result {
                    write!(
                        f,
                        "{} = call {}({})",
                        result,
                        target,
                        args.iter().map(|r| r.to_string()).collect::<Vec<_>>().join(", ")
                    )
                } else {
                    write!(
                        f,
                        "call {}({})",
                        target,
                        args.iter().map(|r| r.to_string()).collect::<Vec<_>>().join(", ")
                    )
                }
            }
            Instruction::CallIndirect {
                function_register,
                args,
                result,
                ..
            } => {
                if let Some(result) = result {
                    write!(
                        f,
                        "{} = call_indirect {}({})",
                        result,
                        function_register,
                        args.iter().map(|r| r.to_string()).collect::<Vec<_>>().join(", ")
                    )
                } else {
                    write!(
                        f,
                        "call_indirect {}({})",
                        function_register,
                        args.iter().map(|r| r.to_string()).collect::<Vec<_>>().join(", ")
                    )
                }
            }
            Instruction::Return { value, .. } => {
                if let Some(value) = value {
                    write!(f, "ret {}", value)
                } else {
                    write!(f, "ret")
                }
            }
            Instruction::Label { id, .. } => write!(f, "{}:", id),
            Instruction::Nop { .. } => write!(f, "nop"),
            
            // 结构体相关指令
            Instruction::StructAlloc { dst, struct_type, allocation_type, .. } => {
                write!(f, "alloc_struct {}, {}, {}", dst, struct_type, allocation_type)
            }
            Instruction::StructFieldLoad { dst, struct_addr, field_offset, .. } => {
                write!(f, "load_field {}, [{}].{}", dst, struct_addr, field_offset)
            }
            Instruction::StructFieldStore { struct_addr, field_offset, src, .. } => {
                write!(f, "store_field [{}].{}, {}", struct_addr, field_offset, src)
            }
            Instruction::StructFieldAddr { dst, struct_addr, field_offset, .. } => {
                write!(f, "field_addr {}, [{}].{}", dst, struct_addr, field_offset)
            }
            Instruction::MemCopy { dst, src, size, .. } => {
                write!(f, "memcpy {}, {}, #{}", dst, src, size)
            }
            Instruction::Alloc { dst, size, alignment, allocation_type, .. } => {
                write!(f, "alloc {}, #{}, #{}, {}", dst, size, alignment, allocation_type)
            }
            Instruction::Free { addr, .. } => {
                write!(f, "free {}", addr)
            }
            Instruction::Load64 { dst, addr, offset, .. } => {
                if *offset == 0 {
                    write!(f, "load64 {}, [{}]", dst, addr)
                } else {
                    write!(f, "load64 {}, [{} + {}]", dst, addr, offset)
                }
            }
            Instruction::Store64 { addr, offset, src, .. } => {
                if *offset == 0 {
                    write!(f, "store64 [{}], {}", addr, src)
                } else {
                    write!(f, "store64 [{} + {}], {}", addr, offset, src)
                }
            }
        }
    }
}

impl fmt::Display for StructLayout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "struct {} (size: {}, align: {}) {{", self.name, self.total_size, self.alignment)?;
        for field in &self.fields {
            writeln!(f, "  {} @ {} (size: {}, align: {})", field.name, field.offset, field.size, field.alignment)?;
        }
        writeln!(f, "}}")
    }
}

impl fmt::Display for LirFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "function {} (stack_frame: {}):", self.name, self.stack_frame_size)?;
        
        // 显示结构体类型定义
        if !self.struct_types.is_empty() {
            writeln!(f, "  # Struct types:")?;
            for (type_id, layout) in &self.struct_types {
                writeln!(f, "  # {}: {}", type_id, layout.name)?;
            }
            writeln!(f)?;
        }
        
        for instr in &self.instructions {
            if let Instruction::Label { .. } = instr {
                writeln!(f, "{}", instr)?;
            } else {
                writeln!(f, "  {}", instr)?;
            }
        }
        Ok(())
    }
}

impl fmt::Display for LirProgram {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for function in self.functions.values() {
            writeln!(f, "{}", function)?;
        }
        Ok(())
    }
} 