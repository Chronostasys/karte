use karte_diagnostics::Span;
use std::collections::HashMap;
pub use karte_common::calling_convention::RegisterId;

/// 标签标识符（用于跳转目标）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LabelId(pub usize);

/// 结构体类型标识符
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StructTypeId(pub usize);

/// 内存地址标识符
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryId(pub usize);

/// 结构体字段定义
#[derive(Debug, Clone, PartialEq)]
pub struct StructField {
    pub name: String,
    pub offset: usize,
    pub size: usize,
    pub alignment: usize,
}

/// 结构体布局信息
#[derive(Debug, Clone, PartialEq)]
pub struct StructLayout {
    pub name: String,
    pub fields: Vec<StructField>,
    pub total_size: usize,
    pub alignment: usize,
}

/// 内存分配类型
#[derive(Debug, Clone, PartialEq)]
pub enum AllocationType {
    /// 栈分配
    Stack,
    /// 堆分配
    Heap,
    /// 静态分配
    Static,
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
    /// 内存地址（基址 + 偏移）
    Memory { base: RegisterId, offset: i64 },
    /// 结构体字段地址
    StructField { 
        struct_addr: RegisterId, 
        field_offset: usize 
    },
    /// 内存ID引用
    MemoryRef { id: MemoryId },
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

    /// 条件跳转：jg label (jump if greater)
    JumpGreater {
        target: LabelId,
        span: Span,
    },

    /// 条件跳转：jge label (jump if greater or equal)
    JumpGreaterEqual {
        target: LabelId,
        span: Span,
    },

    /// 条件跳转：jl label (jump if less)
    JumpLess {
        target: LabelId,
        span: Span,
    },

    /// 条件跳转：jle label (jump if less or equal)
    JumpLessEqual {
        target: LabelId,
        span: Span,
    },

    /// 函数调用指令
    Call {
        target: LabelId,
        args: Vec<RegisterId>,
        /// 🔧 新增：参数操作数，用于指令降级
        arg_operands: Vec<Operand>,
        result: Option<RegisterId>,
        span: Span,
    },

    /// 间接函数调用指令（通过寄存器存储的函数地址调用）
    CallIndirect {
        function_register: RegisterId,
        args: Vec<RegisterId>,
        /// 🔧 新增：参数操作数，用于指令降级
        arg_operands: Vec<Operand>,
        result: Option<RegisterId>,
        span: Span,
    },

    /// 间接跳转指令

    JumpIndirect {
        function_register: RegisterId,
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

    // ====== 新增的结构体操作指令 ======

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

    /// 内存分配指令
    Alloc {
        dst: RegisterId,
        size: usize,
        alignment: usize,
        allocation_type: AllocationType,
        span: Span,
    },

    /// 内存释放指令
    Free {
        addr: RegisterId,
        span: Span,
    },

    /// 加载内存值（8字节）
    Load64 {
        dst: RegisterId,
        addr: RegisterId,
        offset: i64,
        span: Span,
    },

    /// 存储内存值（8字节）
    Store64 {
        addr: RegisterId,
        offset: i64,
        src: Operand,
        span: Span,
    },

    /// φ(Phi)节点 - SSA形式的控制流汇合
    /// 在控制流汇合点选择来自不同前驱块的值
    Phi {
        dst: RegisterId,
        /// 来自不同前驱块的值：(前驱块标签, 值)
        incoming: Vec<(LabelId, Operand)>,
        span: Span,
    },
}

// 🔧 新增：指令唯一ID系统
static NEXT_INSTRUCTION_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InstructionId(pub u64);

impl InstructionId {
    pub fn new() -> Self {
        InstructionId(NEXT_INSTRUCTION_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst))
    }
}

impl Instruction {
    /// 获取指令定义的寄存器（目标寄存器）
    pub fn get_def_register(&self) -> Option<RegisterId> {
        match self {
            Instruction::Move { dst, .. } |
            Instruction::Add { dst, .. } |
            Instruction::Sub { dst, .. } |
            Instruction::Mul { dst, .. } |
            Instruction::Div { dst, .. } |
            Instruction::Load64 { dst, .. } |
            Instruction::StructAlloc { dst, .. } |
            Instruction::StructFieldLoad { dst, .. } |
            Instruction::StructFieldAddr { dst, .. } |
            Instruction::Alloc { dst, .. } |
            Instruction::MemCopy { dst, .. } => Some(*dst),
            Instruction::Call { result, .. } |
            Instruction::CallIndirect { result, .. } => *result,
            Instruction::Phi { dst, .. } => Some(*dst),
            _ => None,
        }
    }

    /// 替换指令定义的寄存器
    pub fn replace_def_register(&mut self, old_reg: RegisterId, new_reg: RegisterId) {
        match self {
            Instruction::Move { dst, .. } |
            Instruction::Add { dst, .. } |
            Instruction::Sub { dst, .. } |
            Instruction::Mul { dst, .. } |
            Instruction::Div { dst, .. } |
            Instruction::Load64 { dst, .. } |
            Instruction::StructAlloc { dst, .. } |
            Instruction::StructFieldLoad { dst, .. } |
            Instruction::StructFieldAddr { dst, .. } |
            Instruction::Alloc { dst, .. } |
            Instruction::MemCopy { dst, .. } => {
                if *dst == old_reg {
                    *dst = new_reg;
                }
            }
            Instruction::Call { result, .. } |
            Instruction::CallIndirect { result, .. } => {
                if let Some(ref mut res) = result {
                    if *res == old_reg {
                        *res = new_reg;
                    }
                }
            }
            Instruction::Phi { dst, .. } => {
                if *dst == old_reg {
                    *dst = new_reg;
                }
            }
            _ => {}
        }
    }

    /// 获取指令使用的寄存器
    pub fn get_used_registers(&self) -> Vec<RegisterId> {
        let mut used = Vec::new();
        
        match self {
            Instruction::Move { src, .. } => {
                self.add_operand_registers(src, &mut used);
            }
            Instruction::Add { src1, src2, .. } |
            Instruction::Sub { src1, src2, .. } |
            Instruction::Mul { src1, src2, .. } |
            Instruction::Div { src1, src2, .. } => {
                self.add_operand_registers(src1, &mut used);
                self.add_operand_registers(src2, &mut used);
            }
            Instruction::Compare { src1, src2, .. } => {
                self.add_operand_registers(src1, &mut used);
                self.add_operand_registers(src2, &mut used);
            }
            Instruction::Load64 { addr, .. } => {
                used.push(*addr);
            }
            Instruction::Store64 { addr, src, .. } => {
                used.push(*addr);
                self.add_operand_registers(src, &mut used);
            }
            Instruction::StructFieldLoad { struct_addr, .. } => {
                used.push(*struct_addr);
            }
            Instruction::StructFieldStore { struct_addr, src, .. } => {
                used.push(*struct_addr);
                self.add_operand_registers(src, &mut used);
            }
            Instruction::StructFieldAddr { struct_addr, .. } => {
                used.push(*struct_addr);
            }
            Instruction::Call { args, arg_operands, .. } => {
                used.extend_from_slice(args);
                // 添加参数操作数中使用的寄存器
                for operand in arg_operands {
                    self.add_operand_registers(operand, &mut used);
                }
            }
            Instruction::CallIndirect { function_register, args, arg_operands, .. } => {
                used.push(*function_register);
                used.extend_from_slice(args);
                // 添加参数操作数中使用的寄存器
                for operand in arg_operands {
                    if let Operand::Register { id } = operand { used.push(*id); }
                }
            }
            Instruction::Return { value, .. } => {
                if let Some(reg) = value {
                    used.push(*reg);
                }
            }
            Instruction::MemCopy { src, .. } => {
                used.push(*src);
            }
            Instruction::Free { addr, .. } => {
                used.push(*addr);
            }
            Instruction::Phi { incoming, .. } => {
                for (_, value) in incoming {
                    self.add_operand_registers(value, &mut used);
                }
            }
            _ => {}
        }
        
        used
    }

    /// 替换指令中的寄存器
    pub fn replace_register(&mut self, old_reg: RegisterId, new_reg: RegisterId) {
        match self {
            Instruction::Move { dst, src, .. } => {
                // 🔧 关键修复：替换目标寄存器
                if *dst == old_reg {
                    *dst = new_reg;
                }
                Self::replace_operand_register(src, old_reg, new_reg);
            }
            Instruction::Add { dst, src1, src2, .. } |
            Instruction::Sub { dst, src1, src2, .. } |
            Instruction::Mul { dst, src1, src2, .. } |
            Instruction::Div { dst, src1, src2, .. } => {
                // 🔧 关键修复：替换目标寄存器
                if *dst == old_reg {
                    *dst = new_reg;
                }
                Self::replace_operand_register(src1, old_reg, new_reg);
                Self::replace_operand_register(src2, old_reg, new_reg);
            }
            Instruction::Compare { src1, src2, .. } => {
                Self::replace_operand_register(src1, old_reg, new_reg);
                Self::replace_operand_register(src2, old_reg, new_reg);
            }
            Instruction::Load64 { dst, addr, .. } => {
                // 🔧 关键修复：替换目标寄存器
                if *dst == old_reg {
                    *dst = new_reg;
                }
                if *addr == old_reg {
                    *addr = new_reg;
                }
            }
            Instruction::Store64 { addr, src, .. } => {
                if *addr == old_reg {
                    *addr = new_reg;
                }
                Self::replace_operand_register(src, old_reg, new_reg);
            }
            Instruction::StructFieldLoad { dst, struct_addr, .. } => {
                // 🔧 关键修复：替换目标寄存器
                if *dst == old_reg {
                    *dst = new_reg;
                }
                if *struct_addr == old_reg {
                    *struct_addr = new_reg;
                }
            }
            Instruction::StructFieldStore { struct_addr, src, .. } => {
                if *struct_addr == old_reg {
                    *struct_addr = new_reg;
                }
                Self::replace_operand_register(src, old_reg, new_reg);
            }
            Instruction::StructFieldAddr { dst, struct_addr, .. } => {
                // 🔧 关键修复：替换目标寄存器
                if *dst == old_reg {
                    *dst = new_reg;
                }
                if *struct_addr == old_reg {
                    *struct_addr = new_reg;
                }
            }
            Instruction::Call { args, arg_operands, result, .. } => {
                for arg in args {
                    if *arg == old_reg {
                        *arg = new_reg;
                    }
                }
                // 替换参数操作数中的寄存器
                for operand in arg_operands {
                    Self::replace_operand_register(operand, old_reg, new_reg);
                }
                // 🔧 关键修复：替换返回值寄存器
                if let Some(ref mut result_reg) = result {
                    if *result_reg == old_reg {
                        *result_reg = new_reg;
                    }
                }
            }
            Instruction::CallIndirect { function_register, args, arg_operands, result, .. } => {
                if *function_register == old_reg {
                    *function_register = new_reg;
                }
                for arg in args {
                    if *arg == old_reg {
                        *arg = new_reg;
                    }
                }
                // 替换参数操作数中的寄存器
                for operand in arg_operands {
                    Self::replace_operand_register(operand, old_reg, new_reg);
                }
                // 🔧 关键修复：替换返回值寄存器
                if let Some(ref mut result_reg) = result {
                    if *result_reg == old_reg {
                        *result_reg = new_reg;
                    }
                }
            }
            Instruction::Return { value, .. } => {
                if let Some(ref mut reg) = value {
                    if *reg == old_reg {
                        *reg = new_reg;
                    }
                }
            }
            Instruction::MemCopy { dst, src, .. } => {
                // 🔧 关键修复：替换目标寄存器
                if *dst == old_reg {
                    *dst = new_reg;
                }
                if *src == old_reg {
                    *src = new_reg;
                }
            }
            Instruction::Free { addr, .. } => {
                if *addr == old_reg {
                    *addr = new_reg;
                }
            }
            Instruction::Alloc { dst, .. } |
            Instruction::StructAlloc { dst, .. } => {
                // 🔧 关键修复：替换目标寄存器
                if *dst == old_reg {
                    *dst = new_reg;
                }
            }
            Instruction::Phi { dst, incoming, .. } => {
                // 🔧 关键修复：替换目标寄存器
                if *dst == old_reg {
                    *dst = new_reg;
                }
                for (_, value) in incoming {
                    Self::replace_operand_register(value, old_reg, new_reg);
                }
            }
            _ => {}
        }
    }

    /// 辅助方法：从操作数中提取寄存器
    fn add_operand_registers(&self, operand: &Operand, registers: &mut Vec<RegisterId>) {
        match operand {
            Operand::Register { id } => registers.push(*id),
            Operand::Memory { base, .. } => registers.push(*base),
            Operand::StructField { struct_addr, .. } => registers.push(*struct_addr),
            _ => {}
        }
    }

    /// 辅助方法：替换操作数中的寄存器
    fn replace_operand_register(operand: &mut Operand, old_reg: RegisterId, new_reg: RegisterId) {
        match operand {
            Operand::Register { id } => {
                if *id == old_reg {
                    *id = new_reg;
                }
            }
            Operand::Memory { base, .. } => {
                if *base == old_reg {
                    *base = new_reg;
                }
            }
            Operand::StructField { struct_addr, .. } => {
                if *struct_addr == old_reg {
                    *struct_addr = new_reg;
                }
            }
            _ => {}
        }
    }

    /// 获取指令的跨度信息
    pub(crate) fn get_span(&self) -> Span {
        match self {
            Instruction::Move { span, .. } |
            Instruction::Add { span, .. } |
            Instruction::Sub { span, .. } |
            Instruction::Mul { span, .. } |
            Instruction::Div { span, .. } |
            Instruction::Store64 { span, .. } |
            Instruction::Load64 { span, .. } |
            Instruction::Compare { span, .. } |
            Instruction::CallIndirect { span, .. } |
            Instruction::Call { span, .. } |
            Instruction::StructFieldStore { span, .. } |
            Instruction::Phi { span, .. } => *span,
            _ => Span::dummy(),
        }
    }
    
    /// 获取指令定义和使用的寄存器
    /// 
    /// 返回元组 (定义的寄存器, 使用的寄存器)
    pub(crate) fn get_defined_and_used_registers(&self) -> (Vec<RegisterId>, Vec<RegisterId>) {
        let mut defined = vec![];
        let mut used = vec![];

        match self {
            Instruction::Move { dst, src, .. } => {
                defined.push(*dst);
                self.add_operand_registers(src, &mut used);
            }
            Instruction::Add { dst, src1, src2, .. } |
            Instruction::Sub { dst, src1, src2, .. } |
            Instruction::Mul { dst, src1, src2, .. } |
            Instruction::Div { dst, src1, src2, .. } => {
                defined.push(*dst);
                self.add_operand_registers(src1, &mut used);
                self.add_operand_registers(src2, &mut used);
            }
            Instruction::Store64 { addr, src, .. } => {
                used.push(*addr);
                self.add_operand_registers(src, &mut used);
            }
            Instruction::Load64 { dst, addr, .. } => {
                defined.push(*dst);
                used.push(*addr);
            }
            Instruction::Compare { src1, src2, .. } => {
                if let Operand::Register { id } = src1 { used.push(*id); }
                if let Operand::Register { id } = src2 { used.push(*id); }
            }
            Instruction::Return { value, .. } => {
                if let Some(reg) = value { used.push(*reg); }
            }
            Instruction::CallIndirect { function_register, args, arg_operands, result, .. } => {
                used.push(*function_register);
                used.extend_from_slice(args);
                // 添加参数操作数中使用的寄存器
                for operand in arg_operands {
                    if let Operand::Register { id } = operand { used.push(*id); }
                }
                if let Some(result_reg) = result { defined.push(*result_reg); }
            }
            Instruction::Call { args, arg_operands, result, .. } => {
                used.extend_from_slice(args);
                // 添加参数操作数中使用的寄存器
                for operand in arg_operands {
                    if let Operand::Register { id } = operand { used.push(*id); }
                }
                if let Some(result_reg) = result { defined.push(*result_reg); }
            }
            Instruction::Alloc { dst, .. } |
            Instruction::StructAlloc { dst, .. } => {
                defined.push(*dst);
            }
            Instruction::StructFieldStore { struct_addr, src, .. } => {
                used.push(*struct_addr);
                self.add_operand_registers(src, &mut used);
            }
            Instruction::StructFieldLoad { dst, struct_addr, .. } => {
                defined.push(*dst);
                used.push(*struct_addr);
            }
            Instruction::Phi { dst, incoming, .. } => {
                defined.push(*dst);
                for (_, operand) in incoming {
                    self.add_operand_registers(operand, &mut used);
                }
            }
            _ => {} // 其他指令不涉及寄存器
        }

        (defined, used)
    }
}

/// LIR函数
#[derive(Debug, Clone, PartialEq)]
pub struct LirFunction {
    pub name: String,
    pub instructions: Vec<Instruction>,
    pub next_register: usize,
    pub next_label: usize,
    /// 函数使用的结构体类型
    pub struct_types: HashMap<StructTypeId, StructLayout>,
    /// 栈帧大小（用于局部变量分配）
    pub stack_frame_size: usize,
    /// 🔧 新增：函数参数数量
    pub parameter_count: usize,
    /// 🔧 新增：函数参数寄存器列表
    pub parameter_registers: Vec<RegisterId>,
}

impl LirFunction {
    pub fn new(name: String) -> Self {
        Self {
            name,
            instructions: Vec::new(),
            next_register: 0,
            next_label: 1000,
            struct_types: HashMap::new(),
            stack_frame_size: 0,
            parameter_count: 0,
            parameter_registers: Vec::new(),
        }
    }
    
    /// 🔧 新增：创建带参数信息的函数
    pub fn new_with_params(name: String, param_count: usize) -> Self {
        let mut function = Self::new(name);
        function.parameter_count = param_count;
        
        // 根据调用约定设置参数寄存器
        for i in 0..param_count {
            // 调用约定：r1-r4 是参数寄存器
            if i < 4 {
                function.parameter_registers.push(RegisterId(i + 1));
            }
        }
        
        function
    }

    /// 分配一个新的寄存器，跳过栈指针寄存器(RegisterId(6))、帧指针寄存器(RegisterId(7))和函数参数寄存器
    pub fn new_register(&mut self) -> RegisterId {
        // 栈指针寄存器是RegisterId(6)，帧指针寄存器是RegisterId(7)
        const STACK_POINTER_REG: usize = 6;
        const FRAME_POINTER_REG: usize = 7;
        // 🔧 修复：跳过已分配的函数参数寄存器
        let parameter_registers: Vec<usize> = self.parameter_registers.iter().map(|r| r.0).collect();
        
        loop {
            let id = RegisterId(self.next_register);
            self.next_register += 1;
            
            // 🔧 关键修复：跳过栈指针和帧指针寄存器
            if id.0 == STACK_POINTER_REG || id.0 == FRAME_POINTER_REG {
                continue; // 跳过这些特殊寄存器
            }
            
            // 🔧 修复：如果分配到了函数参数寄存器，跳过它
            if !parameter_registers.contains(&id.0) {
                return id;
            }
        }
    }
    
    /// 专门用于栈操作的寄存器分配（只返回栈指针寄存器）
    pub fn get_stack_pointer_register(&self) -> RegisterId {
        RegisterId(6) // 栈指针寄存器
    }
    
    /// 检查一个寄存器是否是栈指针寄存器
    pub fn is_stack_pointer_register(&self, reg: &RegisterId) -> bool {
        reg.0 == 6
    }
    
    /// 检查一个寄存器是否是帧指针寄存器
    pub fn is_frame_pointer_register(&self, reg: &RegisterId) -> bool {
        reg.0 == 7
    }

    /// 验证指令是否违反栈指针寄存器使用规则
    /// 栈指针寄存器(RegisterId(6))只能用于栈操作和栈帧管理
    pub fn validate_stack_pointer_usage(&self) -> Result<(), String> {
        // 🔧 修复：支持基于帧指针的栈帧管理代码
        for (index, instruction) in self.instructions.iter().enumerate() {
            match instruction {
                // 允许的栈操作
                Instruction::Alloc { .. } => {
                    // 栈分配操作允许使用栈指针
                }
                Instruction::Sub { dst, src1, src2, .. } => {
                    // 栈指针相关的减法操作
                    if self.is_stack_pointer_register(dst) {
                        match (src1, src2) {
                            (Operand::Register { id }, _) if self.is_stack_pointer_register(id) => {
                                // 允许：SP = SP - size (栈分配或函数序言)
                            }
                            _ => {
                                return Err(format!(
                                    "指令 {} 违反栈指针使用规则: 栈指针寄存器只能用于栈操作",
                                    index
                                ));
                            }
                        }
                    }
                }
                Instruction::Add { dst, src1, src2, .. } => {
                    // 🔧 新增：允许栈指针的加法操作（函数尾声恢复栈指针）
                    if self.is_stack_pointer_register(dst) {
                        match (src1, src2) {
                            (Operand::Register { id }, _) if self.is_stack_pointer_register(id) => {
                                // 允许：SP = SP + size (函数尾声恢复栈指针)
                            }
                            _ => {
                                return Err(format!(
                                    "指令 {} 违反栈指针使用规则: 栈指针寄存器只能用于栈操作",
                                    index
                                ));
                            }
                        }
                    }
                }
                Instruction::Move { dst, src, .. } => {
                    // 🔧 修复：支持栈帧管理中的寄存器移动
                    if self.is_stack_pointer_register(dst) {
                        match src {
                            Operand::Register { id } if self.is_stack_pointer_register(id) => {
                                // 允许：dst = SP (栈分配结果)
                            }
                            Operand::Register { id } if self.is_frame_pointer_register(id) => {
                                // 🔧 新增：允许：SP = FP (函数尾声恢复栈指针)
                            }
                            _ => {
                                return Err(format!(
                                    "指令 {} 违反栈指针使用规则: 栈指针寄存器只能用于栈操作",
                                    index
                                ));
                            }
                        }
                    }
                    
                    // 🔧 新增：允许帧指针相关的操作
                    if self.is_frame_pointer_register(dst) {
                        match src {
                            Operand::Register { id } if self.is_stack_pointer_register(id) => {
                                // 允许：FP = SP (函数序言设置帧指针)
                            }
                            _ => {
                                // 其他对帧指针的赋值也允许（例如恢复调用者的帧指针）
                            }
                        }
                    }
                    
                    // 允许将栈指针值复制到其他寄存器（栈分配结果）
                    if let Operand::Register { id } = src {
                        if self.is_stack_pointer_register(id) && !self.is_stack_pointer_register(dst) {
                            // 允许：将栈指针值复制到其他寄存器
                        }
                    }
                }
                Instruction::Store64 { addr, src, .. } => {
                    // 🔧 新增：允许存储到栈指针地址（函数序言保存调用者帧指针）
                    if self.is_stack_pointer_register(addr) {
                        // 允许：store [SP], FP 或其他值
                    }
                }
                Instruction::Load64 { dst, addr, .. } => {
                    // 🔧 新增：允许从栈指针地址加载（函数尾声恢复调用者帧指针）
                    if self.is_stack_pointer_register(addr) {
                        // 允许：load FP, [SP] 或其他值
                    }
                }
                // 其他指令暂时不进行严格验证，避免复杂性
                _ => {
                    // 简化：不进行复杂的寄存器使用检查
                }
            }
        }
        Ok(())
    }

    pub fn new_label(&mut self) -> LabelId {
        let id = LabelId(self.next_label);
        self.next_label += 1;
        id
    }

    pub fn add_instruction(&mut self, instruction: Instruction) {
        self.instructions.push(instruction);
    }

    pub fn add_struct_type(&mut self, struct_type: StructLayout) -> StructTypeId {
        let id = StructTypeId(self.struct_types.len());
        self.struct_types.insert(id, struct_type);
        id
    }

    pub fn get_struct_layout(&self, type_id: StructTypeId) -> Option<&StructLayout> {
        self.struct_types.get(&type_id)
    }

    pub fn reserve_stack_space(&mut self, size: usize, alignment: usize) -> usize {
        // 对齐栈帧大小
        let aligned_offset = (self.stack_frame_size + alignment - 1) & !(alignment - 1);
        self.stack_frame_size = aligned_offset + size;
        aligned_offset
    }
}

/// LIR程序
#[derive(Debug, Clone, PartialEq)]
pub struct LirProgram {
    pub functions: HashMap<String, LirFunction>,
    pub main_function: Option<String>,
    /// 全局结构体类型定义
    pub global_struct_types: HashMap<String, StructLayout>,
    /// 全局变量定义
    pub global_variables: HashMap<String, MemoryId>,
}

impl LirProgram {
    pub fn new() -> Self {
        Self {
            functions: HashMap::new(),
            main_function: None,
            global_struct_types: HashMap::new(),
            global_variables: HashMap::new(),
        }
    }

    pub fn add_function(&mut self, function: LirFunction) {
        self.functions.insert(function.name.clone(), function);
    }

    pub fn set_main(&mut self, name: String) {
        self.main_function = Some(name);
    }

    pub fn add_global_struct_type(&mut self, name: String, layout: StructLayout) {
        self.global_struct_types.insert(name, layout);
    }

    pub fn get_global_struct_layout(&self, name: &str) -> Option<&StructLayout> {
        self.global_struct_types.get(name)
    }

    pub fn add_global_variable(&mut self, name: String, memory_id: MemoryId) {
        self.global_variables.insert(name, memory_id);
    }
} 