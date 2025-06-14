use karte_diagnostics::Span;
use std::collections::HashMap;

/// 寄存器标识符
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RegisterId(pub usize);

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
        result: Option<RegisterId>,
        span: Span,
    },

    /// 间接函数调用指令（通过寄存器存储的函数地址调用）
    CallIndirect {
        function_register: RegisterId,
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
}

impl LirFunction {
    pub fn new(name: String) -> Self {
        Self {
            name,
            instructions: Vec::new(),
            next_register: 0,
            next_label: 0,
            struct_types: HashMap::new(),
            stack_frame_size: 0,
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