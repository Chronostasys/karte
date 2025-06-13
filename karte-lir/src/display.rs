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
        }
    }
}

impl fmt::Display for LirFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}:", self.name)?;
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