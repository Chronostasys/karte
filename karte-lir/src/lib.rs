use karte_diagnostics::Span;
use std::collections::HashMap;
use std::fmt;

pub mod lower;

/// 寄存器标识符
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RegisterId(pub usize);

impl fmt::Display for RegisterId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "r{}", self.0)
    }
}

/// 标签标识符（用于跳转目标）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LabelId(pub usize);

impl fmt::Display for LabelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "L{}", self.0)
    }
}

/// LIR操作数
#[derive(Debug, Clone, PartialEq)]
pub enum Operand {
    /// 寄存器
    Register { id: RegisterId },
    /// 立即数（整数）
    Immediate { value: i64 },
    /// 标签引用
    Label { id: LabelId },
    /// 内存地址（简化版）
    Memory { base: RegisterId, offset: i64 },
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

/// LIR指令
#[derive(Debug, Clone, PartialEq)]
pub enum Instruction {
    /// 移动指令：mov dst, src
    Move {
        dst: RegisterId,
        src: Operand,
        span: Span,
    },
    
    /// 算术指令：add dst, src1, src2
    Add {
        dst: RegisterId,
        src1: Operand,
        src2: Operand,
        span: Span,
    },
    
    /// 减法指令：sub dst, src1, src2
    Sub {
        dst: RegisterId,
        src1: Operand,
        src2: Operand,
        span: Span,
    },
    
    /// 乘法指令：mul dst, src1, src2
    Mul {
        dst: RegisterId,
        src1: Operand,
        src2: Operand,
        span: Span,
    },
    
    /// 除法指令：div dst, src1, src2
    Div {
        dst: RegisterId,
        src1: Operand,
        src2: Operand,
        span: Span,
    },
    
    /// 比较指令：cmp src1, src2
    Compare {
        src1: Operand,
        src2: Operand,
        span: Span,
    },
    
    /// 无条件跳转：jmp label
    Jump {
        target: LabelId,
        span: Span,
    },
    
    /// 条件跳转：je label (jump if equal)
    JumpEqual {
        target: LabelId,
        span: Span,
    },
    
    /// 条件跳转：jne label (jump if not equal)
    JumpNotEqual {
        target: LabelId,
        span: Span,
    },
    
    /// 函数调用：call label
    Call {
        target: LabelId,
        args: Vec<RegisterId>,
        result: Option<RegisterId>,
        span: Span,
    },
    
    /// 返回指令：ret [register]
    Return {
        value: Option<RegisterId>,
        span: Span,
    },
    
    /// 标签定义（不是真正的指令，用于标记位置）
    Label {
        id: LabelId,
        span: Span,
    },
    
    /// 空操作
    Nop {
        span: Span,
    },
}

impl fmt::Display for Instruction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Instruction::Move { dst, src, .. } => {
                write!(f, "mov {}, {}", dst, src)
            }
            Instruction::Add { dst, src1, src2, .. } => {
                write!(f, "add {}, {}, {}", dst, src1, src2)
            }
            Instruction::Sub { dst, src1, src2, .. } => {
                write!(f, "sub {}, {}, {}", dst, src1, src2)
            }
            Instruction::Mul { dst, src1, src2, .. } => {
                write!(f, "mul {}, {}, {}", dst, src1, src2)
            }
            Instruction::Div { dst, src1, src2, .. } => {
                write!(f, "div {}, {}, {}", dst, src1, src2)
            }
            Instruction::Compare { src1, src2, .. } => {
                write!(f, "cmp {}, {}", src1, src2)
            }
            Instruction::Jump { target, .. } => {
                write!(f, "jmp {}", target)
            }
            Instruction::JumpEqual { target, .. } => {
                write!(f, "je {}", target)
            }
            Instruction::JumpNotEqual { target, .. } => {
                write!(f, "jne {}", target)
            }
            Instruction::Call { target, args, result, .. } => {
                if let Some(result) = result {
                    write!(f, "{} = call {}({})", result, target, 
                           args.iter().map(|r| r.to_string()).collect::<Vec<_>>().join(", "))
                } else {
                    write!(f, "call {}({})", target,
                           args.iter().map(|r| r.to_string()).collect::<Vec<_>>().join(", "))
                }
            }
            Instruction::Return { value, .. } => {
                if let Some(value) = value {
                    write!(f, "ret {}", value)
                } else {
                    write!(f, "ret")
                }
            }
            Instruction::Label { id, .. } => {
                write!(f, "{}:", id)
            }
            Instruction::Nop { .. } => {
                write!(f, "nop")
            }
        }
    }
}

/// LIR函数
#[derive(Debug, Clone, PartialEq)]
pub struct LirFunction {
    pub name: String,
    pub instructions: Vec<Instruction>,
    pub next_register: usize,
    pub next_label: usize,
}

impl LirFunction {
    pub fn new(name: String) -> Self {
        Self {
            name,
            instructions: Vec::new(),
            next_register: 0,
            next_label: 0,
        }
    }
    
    pub fn new_register(&mut self) -> RegisterId {
        let id = RegisterId(self.next_register);
        self.next_register += 1;
        id
    }
    
    pub fn new_label(&mut self) -> LabelId {
        let id = LabelId(self.next_label);
        self.next_label += 1;
        id
    }
    
    pub fn add_instruction(&mut self, instruction: Instruction) {
        self.instructions.push(instruction);
    }
}

impl fmt::Display for LirFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}:", self.name)?;
        for instr in &self.instructions {
            writeln!(f, "  {}", instr)?;
        }
        Ok(())
    }
}

/// LIR程序
#[derive(Debug, Clone, PartialEq)]
pub struct LirProgram {
    pub functions: HashMap<String, LirFunction>,
    pub main_function: Option<String>,
}

impl LirProgram {
    pub fn new() -> Self {
        Self {
            functions: HashMap::new(),
            main_function: None,
        }
    }
    
    pub fn add_function(&mut self, function: LirFunction) {
        self.functions.insert(function.name.clone(), function);
    }
    
    pub fn set_main(&mut self, name: String) {
        self.main_function = Some(name);
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
