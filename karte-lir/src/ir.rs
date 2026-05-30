use karte_common::calling_convention::CallingConvention;
pub use karte_common::calling_convention::{PhysicalRegister, Register};
use karte_diagnostics::Span;
use karte_ir_derive::IrCodec;
use std::{collections::HashMap, sync::atomic::AtomicUsize};

/// 标签标识符（用于跳转目标）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, IrCodec, Default)]
#[ir_codec(token = "L")]
pub struct LabelId(#[ir_codec(args)] pub usize);

/// 结构体类型标识符
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, IrCodec)]
pub struct StructTypeId(pub usize);

/// 内存地址标识符
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, IrCodec, Default)]
pub struct MemoryId(pub usize);

/// 结构体字段定义
#[derive(Debug, Clone, PartialEq, IrCodec)]
pub struct StructField {
    pub name: String,
    pub offset: usize,
    pub size: usize,
    pub alignment: usize,
}

/// 结构体布局信息
#[derive(Debug, Clone, PartialEq, IrCodec)]
pub struct StructLayout {
    pub name: String,
    pub fields: Vec<StructField>,
    pub total_size: usize,
    pub alignment: usize,
}

/// 比较条件类型
/// 用于 CompareSet 指令，指定比较的方式
#[derive(Debug, Clone, PartialEq, Eq, Hash, IrCodec)]
pub enum ComparisonCondition {
    /// 等于 (x86: SETE, AArch64: EQ)
    #[ir_codec(token = "eq")]
    Equal,
    /// 不等于 (x86: SETNE, AArch64: NE)
    #[ir_codec(token = "ne")]
    NotEqual,
    /// 小于 (x86: SETL, AArch64: LT)
    #[ir_codec(token = "lt")]
    LessThan,
    /// 小于等于 (x86: SETLE, AArch64: LE)
    #[ir_codec(token = "le")]
    LessEqual,
    /// 大于 (x86: SETG, AArch64: GT)
    #[ir_codec(token = "gt")]
    GreaterThan,
    /// 大于等于 (x86: SETGE, AArch64: GE)
    #[ir_codec(token = "ge")]
    GreaterEqual,
}

/// 内存分配类型
#[derive(Debug, Clone, PartialEq, IrCodec)]
pub enum AllocationType {
    /// 栈分配
    Stack,
    /// 堆分配
    Heap,
    /// 静态分配
    Static,
}

/// 指令元数据
///
/// 包含与指令关联的额外信息，如调用位置活跃寄存器、调试信息等。
/// 通过 LirFunction.instruction_metadata 侧表存储，避免修改 Instruction enum。
#[derive(Debug, Clone, PartialEq)]
pub struct InstructionMetadata {
    /// 调用位置活跃寄存器信息
    pub live_register_info: Option<LiveRegisterInfo>,
}

/// 调用位置活跃寄存器信息
///
/// 在调用指令位置（Call, CallIndirect, Alloc, Free, Retain, Release, Safepoint），
/// 记录活跃的寄存器列表。这些信息由 CallsiteLiveRegisterPass 在编译时分析生成，
/// 供 codegen 使用，用于确定在调用前需要保存哪些寄存器。
#[derive(Debug, Clone, PartialEq)]
pub struct LiveRegisterInfo {
    /// 活跃寄存器列表
    ///
    /// 在此调用位置活跃的所有寄存器。
    /// 由 lifetime analysis 计算得出。
    pub live_registers: Vec<Register>,
}

/// LIR操作数
#[derive(Debug, Clone, PartialEq, IrCodec)]
pub enum Operand {
    /// 寄存器 (直接显示寄存器，无需前缀)
    Register {
        #[ir_codec(args)]
        id: Register,
    },

    /// 立即数（整数）
    #[ir_codec(token = "#")]
    Immediate {
        #[ir_codec(args)]
        value: i64,
    },

    /// 标签引用
    #[ir_codec(token = "@")]
    Label {
        #[ir_codec(args)]
        id: LabelId,
    },

    /// 内存地址（基址 + 偏移）
    #[ir_codec(token = "mem")]
    Memory {
        #[ir_codec(args)]
        base: Register,
        #[ir_codec(args)]
        offset: i64,
    },

    /// 结构体字段地址
    StructField {
        struct_addr: Register,
        field_offset: usize,
    },

    /// 内存ID引用
    MemoryRef { id: MemoryId },
}

impl Default for Operand {
    fn default() -> Self {
        Operand::Immediate { value: 0 }
    }
}

/// LIR指令
#[derive(Debug, Clone, PartialEq, IrCodec)]
pub enum Instruction {
    /// 移动指令：mov dst, src
    #[ir_codec(token = "mov")]
    Move {
        #[ir_codec(args)]
        dst: Register,
        #[ir_codec(args)]
        src: Operand,
        #[ir_codec(skip)]
        span: Span,
    },

    /// 算术指令：add dst, src1, src2
    #[ir_codec(token = "add")]
    Add {
        #[ir_codec(args)]
        dst: Register,
        #[ir_codec(args)]
        src1: Operand,
        #[ir_codec(args)]
        src2: Operand,
        #[ir_codec(skip)]
        span: Span,
    },

    /// 减法指令：sub dst, src1, src2
    #[ir_codec(token = "sub")]
    Sub {
        #[ir_codec(args)]
        dst: Register,
        #[ir_codec(args)]
        src1: Operand,
        #[ir_codec(args)]
        src2: Operand,
        #[ir_codec(skip)]
        span: Span,
    },

    /// 乘法指令：mul dst, src1, src2
    #[ir_codec(token = "mul")]
    Mul {
        #[ir_codec(args)]
        dst: Register,
        #[ir_codec(args)]
        src1: Operand,
        #[ir_codec(args)]
        src2: Operand,
        #[ir_codec(skip)]
        span: Span,
    },

    /// 除法指令：div dst, src1, src2
    #[ir_codec(token = "div")]
    Div {
        #[ir_codec(args)]
        dst: Register,
        #[ir_codec(args)]
        src1: Operand,
        #[ir_codec(args)]
        src2: Operand,
        #[ir_codec(skip)]
        span: Span,
    },

    /// 取余指令：mod dst, src1, src2
    #[ir_codec(token = "mod")]
    Mod {
        #[ir_codec(args)]
        dst: Register,
        #[ir_codec(args)]
        src1: Operand,
        #[ir_codec(args)]
        src2: Operand,
        #[ir_codec(skip)]
        span: Span,
    },

    /// 位与指令：& dst, src1, src2
    #[ir_codec(token = "&")]
    BitAnd {
        #[ir_codec(args)]
        dst: Register,
        #[ir_codec(args)]
        src1: Operand,
        #[ir_codec(args)]
        src2: Operand,
        #[ir_codec(skip)]
        span: Span,
    },

    /// 位或指令：| dst, src1, src2
    #[ir_codec(token = "|")]
    BitOr {
        #[ir_codec(args)]
        dst: Register,
        #[ir_codec(args)]
        src1: Operand,
        #[ir_codec(args)]
        src2: Operand,
        #[ir_codec(skip)]
        span: Span,
    },

    /// 位异或指令：^ dst, src1, src2
    #[ir_codec(token = "^")]
    BitXor {
        #[ir_codec(args)]
        dst: Register,
        #[ir_codec(args)]
        src1: Operand,
        #[ir_codec(args)]
        src2: Operand,
        #[ir_codec(skip)]
        span: Span,
    },

    /// 左移指令：<< dst, src1, src2
    #[ir_codec(token = "<<")]
    ShiftLeft {
        #[ir_codec(args)]
        dst: Register,
        #[ir_codec(args)]
        src1: Operand,
        #[ir_codec(args)]
        src2: Operand,
        #[ir_codec(skip)]
        span: Span,
    },

    /// 右移指令：>> dst, src1, src2
    #[ir_codec(token = ">>")]
    ShiftRight {
        #[ir_codec(args)]
        dst: Register,
        #[ir_codec(args)]
        src1: Operand,
        #[ir_codec(args)]
        src2: Operand,
        #[ir_codec(skip)]
        span: Span,
    },

    /// 位非指令：~ dst, src
    #[ir_codec(token = "~")]
    BitNot {
        #[ir_codec(args)]
        dst: Register,
        #[ir_codec(args)]
        src: Operand,
        #[ir_codec(skip)]
        span: Span,
    },

    /// 比较指令：cmp src1, src2（只设置 flags，不写结果）
    #[ir_codec(token = "cmp")]
    Compare {
        #[ir_codec(args)]
        src1: Operand,
        #[ir_codec(args)]
        src2: Operand,
        #[ir_codec(skip)]
        span: Span,
    },

    /// 比较并设置布尔结果：setcc dst, condition, src1, src2
    /// 直接从比较条件产生 0/1 值到 dst 寄存器，不产生分支。
    /// x86: cmp src1, src2; setcc dst_byte; movzbq dst, dst_byte
    /// AArch64: cmp src1, src2; cset dst, condition
    #[ir_codec(token = "setcc")]
    CompareSet {
        #[ir_codec(args)]
        dst: Register,
        #[ir_codec(args)]
        condition: ComparisonCondition,
        #[ir_codec(args)]
        src1: Operand,
        #[ir_codec(args)]
        src2: Operand,
        #[ir_codec(skip)]
        span: Span,
    },

    /// 无条件跳转：Jump label
    #[ir_codec(token = "Jump")]
    Jump { target: LabelId, span: Span },

    /// 条件跳转：je label (jump if equal)
    JumpEqual { target: LabelId, span: Span },

    /// 条件跳转：jne label (jump if not equal)
    JumpNotEqual { target: LabelId, span: Span },

    /// 条件跳转：jg label (jump if greater)
    JumpGreater { target: LabelId, span: Span },

    /// 条件跳转：jge label (jump if greater or equal)
    JumpGreaterEqual { target: LabelId, span: Span },

    /// 条件跳转：jl label (jump if less)
    JumpLess { target: LabelId, span: Span },

    /// 条件跳转：jle label (jump if less or equal)
    JumpLessEqual { target: LabelId, span: Span },

    /// 函数调用指令
    Call {
        target: LabelId,
        args: Vec<Register>,
        /// 🔧 新增：参数操作数，用于指令降级
        arg_operands: Vec<Operand>,
        result: Option<Register>,
        span: Span,
    },

    /// 间接函数调用指令（通过寄存器存储的函数地址调用）
    CallIndirect {
        function_register: Register,
        args: Vec<Register>,
        /// 🔧 新增：参数操作数，用于指令降级
        arg_operands: Vec<Operand>,
        result: Option<Register>,
        span: Span,
    },

    /// 间接函数调用指令（带链接）
    /// 通过寄存器地址进行间接函数调用，会保存返回地址到LR
    JumpIndirect {
        function_register: Register,
        span: Span,
    },

    /// 寄存器跳转指令（无链接）
    /// 通过寄存器地址进行无条件跳转（用于continuation/resume）
    JumpRegister {
        target_register: Register,
        span: Span,
    },

    /// 返回指令：ret [register]
    Return { value: Option<Register>, span: Span },

    /// 标签定义（不是真正的指令，用于标记位置）
    Label { id: LabelId, span: Span },

    /// 空操作
    Nop { span: Span },

    // ====== 代数效应伪指令（在指令降级阶段展开为基础指令） ======
    /// 入栈一个效应处理器帧
    EffectPushHandler {
        /// 效应类型标识（支持立即数或寄存器）
        tag: Operand,
        /// 处理器入口label
        handler_label: LabelId,
        span: Span,
    },

    /// 出栈一个效应处理器帧
    EffectPopHandler { span: Span },

    /// 触发效应：在降级中查找匹配处理器，写入resume点并跳转到handler
    EffectPerform {
        tag: Operand, // 支持立即数或寄存器
        payload: Operand,
        result: Option<Register>,
        span: Span,
    },

    /// 在处理器中恢复到perform点：将value写入r0并跳回保存的resume地址
    EffectResume { value: Operand, span: Span },

    // ====== 新增的结构体操作指令 ======
    /// 分配结构体内存
    StructAlloc {
        dst: Register,
        struct_type: StructTypeId,
        allocation_type: AllocationType,
        span: Span,
    },

    /// 加载结构体字段
    StructFieldLoad {
        dst: Register,
        struct_addr: Register,
        field_offset: usize,
        span: Span,
    },

    /// 存储结构体字段
    StructFieldStore {
        struct_addr: Register,
        field_offset: usize,
        src: Operand,
        span: Span,
    },

    /// 获取结构体字段地址
    StructFieldAddr {
        dst: Register,
        struct_addr: Register,
        field_offset: usize,
        span: Span,
    },

    /// 内存拷贝指令（用于结构体赋值）
    MemCopy {
        dst: Register,
        src: Register,
        size: usize,
        span: Span,
    },

    /// 内存分配指令
    Alloc {
        dst: Register,
        size: usize,
        alignment: usize,
        allocation_type: AllocationType,
        span: Span,
    },

    /// 内存释放指令
    Free { addr: Register, span: Span },

    /// ARC/GC retain 调用占位
    Retain { value: Register, span: Span },

    /// ARC/GC release 调用占位
    Release { value: Register, span: Span },

    /// GC 安全点
    ///
    /// 在循环回边、长时间运行的函数等位置插入，允许 GC 暂停程序执行。
    /// 在安全点时，虚拟栈的状态必须是已知的，所有 GC 对象的引用都应该是可见的。
    #[ir_codec(token = "safepoint")]
    Safepoint {
        #[ir_codec(skip)]
        span: Span,
    },

    /// 字符串连接：dst = concat(left, right)
    /// 调用运行时 karte_jit_runtime_string_concat(left_ptr, right_ptr) -> new_ptr
    #[ir_codec(token = "string_concat")]
    StringConcat {
        #[ir_codec(args)]
        dst: Register,
        #[ir_codec(args)]
        left: Register,
        #[ir_codec(args)]
        right: Register,
        #[ir_codec(skip)]
        span: Span,
    },

    /// 打印字符串：print(ptr)
    /// 调用运行时 karte_jit_runtime_print_string(str_ptr) -> 0
    #[ir_codec(token = "print_string")]
    PrintString {
        #[ir_codec(args)]
        ptr: Register,
        #[ir_codec(skip)]
        span: Span,
    },

    /// 加载内存值（8字节）
    #[ir_codec(token = "load64")]
    Load64 {
        #[ir_codec(args)]
        dst: Register,
        #[ir_codec(args)]
        addr: Register,
        #[ir_codec(args)]
        offset: i64,
        #[ir_codec(skip)]
        span: Span,
    },

    /// 存储内存值（8字节）
    #[ir_codec(token = "store64")]
    Store64 {
        #[ir_codec(args)]
        addr: Register,
        #[ir_codec(args)]
        offset: i64,
        #[ir_codec(args)]
        src: Operand,
        #[ir_codec(skip)]
        span: Span,
    },

    /// 加载内存值（4字节）
    #[ir_codec(token = "load32")]
    Load32 {
        #[ir_codec(args)]
        dst: Register,
        #[ir_codec(args)]
        addr: Register,
        #[ir_codec(args)]
        offset: i64,
        #[ir_codec(skip)]
        span: Span,
    },

    /// 存储内存值（4字节）
    #[ir_codec(token = "store32")]
    Store32 {
        #[ir_codec(args)]
        addr: Register,
        #[ir_codec(args)]
        offset: i64,
        #[ir_codec(args)]
        src: Operand,
        #[ir_codec(skip)]
        span: Span,
    },

    /// 加载内存值（1字节）
    #[ir_codec(token = "load8")]
    Load8 {
        #[ir_codec(args)]
        dst: Register,
        #[ir_codec(args)]
        addr: Register,
        #[ir_codec(args)]
        offset: i64,
        #[ir_codec(skip)]
        span: Span,
    },

    /// 存储内存值（1字节）
    #[ir_codec(token = "store8")]
    Store8 {
        #[ir_codec(args)]
        addr: Register,
        #[ir_codec(args)]
        offset: i64,
        #[ir_codec(args)]
        src: Operand,
        #[ir_codec(skip)]
        span: Span,
    },

    /// 存储寄存器对到内存（AArch64 STP指令的LIR表示）
    #[ir_codec(token = "stp")]
    StorePair {
        #[ir_codec(args)]
        addr: Register,
        #[ir_codec(args)]
        offset: i64,
        #[ir_codec(args)]
        src1: Register,
        #[ir_codec(args)]
        src2: Register,
        #[ir_codec(skip)]
        span: Span,
    },

    /// 从内存加载寄存器对（AArch64 LDP指令的LIR表示）
    #[ir_codec(token = "ldp")]
    LoadPair {
        #[ir_codec(args)]
        dst1: Register,
        #[ir_codec(args)]
        dst2: Register,
        #[ir_codec(args)]
        addr: Register,
        #[ir_codec(args)]
        offset: i64,
        #[ir_codec(skip)]
        span: Span,
    },

    /// 加载 runtime 全局变量 (通过名称)
    /// 在 AOT 中被编译为 RIP-relative load
    #[ir_codec(token = "load_global")]
    LoadGlobal {
        #[ir_codec(args)]
        dst: Register,
        #[ir_codec(args)]
        name: String,
        #[ir_codec(skip)]
        span: Span,
    },

    /// GC 寄存器保存/恢复 - 把所有 callee-saved 寄存器 dump 到虚拟栈
    /// gc_push_regs: sub r10, N*8; mov [r10+0], rbx; mov [r10+8], rcx; ...
    /// gc_pop_regs:  mov rbx, [r10+0]; mov rcx, [r10+8]; ...; add r10, N*8
    #[ir_codec(token = "gc_reg_op")]
    GcRegOp {
        #[ir_codec(args)]
        is_push: bool, // true = push, false = pop
        #[ir_codec(skip)]
        span: Span,
    },

    /// φ(Phi)节点 - SSA形式的控制流汇合
    /// 在控制流汇合点选择来自不同前驱块的值
    Phi {
        dst: Register,
        /// 来自不同前驱块的值：(前驱块标签, 值)
        incoming: Vec<(LabelId, Operand)>,
        span: Span,
    },
}

// 🔧 新增：指令唯一ID系统
static NEXT_INSTRUCTION_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InstructionId(pub u64);

impl Default for InstructionId {
    fn default() -> Self {
        Self::new()
    }
}

impl InstructionId {
    pub fn new() -> Self {
        InstructionId(NEXT_INSTRUCTION_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst))
    }
}

impl Instruction {
    /// 获取指令定义的寄存器（目标寄存器）
    pub fn get_def_register(&self) -> Option<Register> {
        match self {
            Instruction::Move { dst, .. }
            | Instruction::Add { dst, .. }
            | Instruction::Sub { dst, .. }
            | Instruction::Mul { dst, .. }
            | Instruction::Div { dst, .. }
            | Instruction::Mod { dst, .. }
            | Instruction::BitAnd { dst, .. }
            | Instruction::BitOr { dst, .. }
            | Instruction::BitXor { dst, .. }
            | Instruction::ShiftLeft { dst, .. }
            | Instruction::ShiftRight { dst, .. }
            | Instruction::BitNot { dst, .. }
            | Instruction::Load64 { dst, .. }
            | Instruction::Load32 { dst, .. }
            | Instruction::Load8 { dst, .. }
            | Instruction::LoadGlobal { dst, .. }
            | Instruction::StructAlloc { dst, .. }
            | Instruction::StructFieldLoad { dst, .. }
            | Instruction::StructFieldAddr { dst, .. }
            | Instruction::Alloc { dst, .. }
            | Instruction::MemCopy { dst, .. }
            | Instruction::CompareSet { dst, .. }
            | Instruction::StringConcat { dst, .. } => Some(*dst),
            Instruction::LoadPair { dst1, .. } => Some(*dst1),
            Instruction::Call { result, .. } | Instruction::CallIndirect { result, .. } => *result,
            Instruction::Phi { dst, .. } => Some(*dst),
            Instruction::LoadGlobal { dst, .. } => Some(*dst),
            // EffectPerform 的 result 是定义寄存器（如果存在）
            Instruction::EffectPerform { result, .. } => *result,
            _ => None,
        }
    }

    /// 替换指令定义的寄存器
    pub fn replace_def_register(&mut self, old_reg: Register, new_reg: Register) {
        match self {
            Instruction::Move { dst, .. }
            | Instruction::Add { dst, .. }
            | Instruction::Sub { dst, .. }
            | Instruction::Mul { dst, .. }
            | Instruction::Div { dst, .. }
            | Instruction::Mod { dst, .. }
            | Instruction::BitAnd { dst, .. }
            | Instruction::BitOr { dst, .. }
            | Instruction::BitXor { dst, .. }
            | Instruction::ShiftLeft { dst, .. }
            | Instruction::ShiftRight { dst, .. }
            | Instruction::BitNot { dst, .. }
            | Instruction::Load64 { dst, .. }
            | Instruction::Load32 { dst, .. }
            | Instruction::Load8 { dst, .. }
            | Instruction::StructAlloc { dst, .. }
            | Instruction::StructFieldLoad { dst, .. }
            | Instruction::StructFieldAddr { dst, .. }
            | Instruction::Alloc { dst, .. }
            | Instruction::MemCopy { dst, .. }
            | Instruction::CompareSet { dst, .. }
            | Instruction::StringConcat { dst, .. } => {
                if *dst == old_reg {
                    *dst = new_reg;
                }
            }
            Instruction::LoadPair { dst1, dst2, .. } => {
                if *dst1 == old_reg {
                    *dst1 = new_reg;
                }
                if *dst2 == old_reg {
                    *dst2 = new_reg;
                }
            }
            Instruction::Call { result, .. } | Instruction::CallIndirect { result, .. } => {
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
            // EffectPerform 的 result 也需要替换
            Instruction::EffectPerform { result, .. } => {
                if let Some(ref mut res) = result {
                    if *res == old_reg {
                        *res = new_reg;
                    }
                }
            }
            _ => {}
        }
    }

    /// 获取指令使用的寄存器
    pub fn get_used_registers(&self) -> Vec<Register> {
        let mut used = Vec::new();

        match self {
            Instruction::Move { src, .. } => {
                self.add_operand_registers(src, &mut used);
            }
            Instruction::Add { src1, src2, .. }
            | Instruction::Sub { src1, src2, .. }
            | Instruction::Mul { src1, src2, .. }
            | Instruction::Div { src1, src2, .. }
            | Instruction::Mod { src1, src2, .. }
            | Instruction::BitAnd { src1, src2, .. }
            | Instruction::BitOr { src1, src2, .. }
            | Instruction::BitXor { src1, src2, .. }
            | Instruction::ShiftLeft { src1, src2, .. }
            | Instruction::ShiftRight { src1, src2, .. } => {
                self.add_operand_registers(src1, &mut used);
                self.add_operand_registers(src2, &mut used);
            }
            Instruction::BitNot { src, .. } => {
                self.add_operand_registers(src, &mut used);
            }
            Instruction::Compare { src1, src2, .. } => {
                self.add_operand_registers(src1, &mut used);
                self.add_operand_registers(src2, &mut used);
            }
            Instruction::CompareSet { src1, src2, .. } => {
                self.add_operand_registers(src1, &mut used);
                self.add_operand_registers(src2, &mut used);
            }
            Instruction::Load64 { addr, .. }
            | Instruction::Load32 { addr, .. }
            | Instruction::Load8 { addr, .. } => {
                used.push(*addr);
            }
            Instruction::Store64 { addr, src, .. }
            | Instruction::Store32 { addr, src, .. }
            | Instruction::Store8 { addr, src, .. } => {
                used.push(*addr);
                self.add_operand_registers(src, &mut used);
            }
            Instruction::StorePair {
                addr, src1, src2, ..
            } => {
                used.push(*addr);
                used.push(*src1);
                used.push(*src2);
            }
            Instruction::LoadPair { addr, .. } => {
                used.push(*addr);
            }
            Instruction::StructFieldLoad { struct_addr, .. } => {
                used.push(*struct_addr);
            }
            Instruction::StructFieldStore {
                struct_addr, src, ..
            } => {
                used.push(*struct_addr);
                self.add_operand_registers(src, &mut used);
            }
            Instruction::StructFieldAddr { struct_addr, .. } => {
                used.push(*struct_addr);
            }
            Instruction::Call {
                args, arg_operands, ..
            } => {
                used.extend_from_slice(args);
                // 添加参数操作数中使用的寄存器
                for operand in arg_operands {
                    self.add_operand_registers(operand, &mut used);
                }
            }
            Instruction::CallIndirect {
                function_register,
                args,
                arg_operands,
                ..
            } => {
                used.push(*function_register);
                used.extend_from_slice(args);
                // 🔧 修复：添加参数操作数中使用的寄存器
                for operand in arg_operands {
                    self.add_operand_registers(operand, &mut used);
                }
            }
            Instruction::Return { value, .. } => {
                if let Some(reg) = value {
                    used.push(*reg);
                }
            }
            // EffectPerform: payload 是使用的寄存器，result 是定义（如有）
            Instruction::EffectPerform { tag, payload, .. } => {
                self.add_operand_registers(tag, &mut used);
                self.add_operand_registers(payload, &mut used);
            }
            // EffectPushHandler 的 tag 是使用的寄存器
            Instruction::EffectPushHandler { tag, .. } => {
                self.add_operand_registers(tag, &mut used);
            }
            // NEW: EffectResume value is a used register
            Instruction::EffectResume { value, .. } => {
                // FIX: value is an Operand; delegate to add_operand_registers instead of pushing directly
                self.add_operand_registers(value, &mut used);
            }
            // EffectPopHandler has no explicit register operands
            Instruction::MemCopy { src, .. } => {
                used.push(*src);
            }
            Instruction::Free { addr, .. } => {
                used.push(*addr);
            }
            Instruction::Retain { value, .. } | Instruction::Release { value, .. } => {
                used.push(*value);
            }
            Instruction::StringConcat { left, right, .. } => {
                used.push(*left);
                used.push(*right);
            }
            Instruction::PrintString { ptr, .. } => {
                used.push(*ptr);
            }
            Instruction::Phi { incoming, .. } => {
                for (_, value) in incoming {
                    self.add_operand_registers(value, &mut used);
                }
            }
            Instruction::JumpIndirect {
                function_register, ..
            } => {
                used.push(*function_register);
            }
            Instruction::JumpRegister {
                target_register, ..
            } => {
                used.push(*target_register);
                // push all caller-saved registers
                used.push(Register::Physical(0));
                used.push(Register::Physical(1));
                used.push(Register::Physical(2));
                used.push(Register::Physical(3));
                used.push(Register::Physical(4));
            }
            _ => {}
        }

        used
    }

    /// 替换指令中的寄存器
    pub fn replace_register(&mut self, old_reg: Register, new_reg: Register) {
        match self {
            Instruction::Move { dst, src, .. } => {
                // 🔧 关键修复：替换目标寄存器
                if *dst == old_reg {
                    *dst = new_reg;
                }
                Self::replace_operand_register(src, old_reg, new_reg);
            }
            Instruction::Add {
                dst, src1, src2, ..
            }
            | Instruction::Sub {
                dst, src1, src2, ..
            }
            | Instruction::Mul {
                dst, src1, src2, ..
            }
            | Instruction::Div {
                dst, src1, src2, ..
            }
            | Instruction::Mod {
                dst, src1, src2, ..
            }
            | Instruction::BitAnd {
                dst, src1, src2, ..
            }
            | Instruction::BitOr {
                dst, src1, src2, ..
            }
            | Instruction::BitXor {
                dst, src1, src2, ..
            }
            | Instruction::ShiftLeft {
                dst, src1, src2, ..
            }
            | Instruction::ShiftRight {
                dst, src1, src2, ..
            } => {
                if *dst == old_reg {
                    *dst = new_reg;
                }
                Self::replace_operand_register(src1, old_reg, new_reg);
                Self::replace_operand_register(src2, old_reg, new_reg);
            }
            Instruction::BitNot { dst, src, .. } => {
                if *dst == old_reg {
                    *dst = new_reg;
                }
                Self::replace_operand_register(src, old_reg, new_reg);
            }
            Instruction::Compare { src1, src2, .. } => {
                Self::replace_operand_register(src1, old_reg, new_reg);
                Self::replace_operand_register(src2, old_reg, new_reg);
            }
            Instruction::CompareSet { dst, src1, src2, .. } => {
                if *dst == old_reg {
                    *dst = new_reg;
                }
                Self::replace_operand_register(src1, old_reg, new_reg);
                Self::replace_operand_register(src2, old_reg, new_reg);
            }
            Instruction::Load64 { dst, addr, .. }
            | Instruction::Load32 { dst, addr, .. }
            | Instruction::Load8 { dst, addr, .. } => {
                if *dst == old_reg {
                    *dst = new_reg;
                }
                if *addr == old_reg {
                    *addr = new_reg;
                }
            }
            Instruction::LoadGlobal { dst, .. } => {
                if *dst == old_reg {
                    *dst = new_reg;
                }
            }
            Instruction::Store64 { addr, src, .. }
            | Instruction::Store32 { addr, src, .. }
            | Instruction::Store8 { addr, src, .. } => {
                if *addr == old_reg {
                    *addr = new_reg;
                }
                Self::replace_operand_register(src, old_reg, new_reg);
            }
            Instruction::StorePair {
                addr, src1, src2, ..
            } => {
                if *addr == old_reg {
                    *addr = new_reg;
                }
                if *src1 == old_reg {
                    *src1 = new_reg;
                }
                if *src2 == old_reg {
                    *src2 = new_reg;
                }
            }
            Instruction::LoadPair {
                dst1, dst2, addr, ..
            } => {
                // 替换目标寄存器
                if *dst1 == old_reg {
                    *dst1 = new_reg;
                }
                if *dst2 == old_reg {
                    *dst2 = new_reg;
                }
                // 替换地址寄存器
                if *addr == old_reg {
                    *addr = new_reg;
                }
            }
            Instruction::StructFieldLoad {
                dst, struct_addr, ..
            } => {
                // 🔧 关键修复：替换目标寄存器
                if *dst == old_reg {
                    *dst = new_reg;
                }
                if *struct_addr == old_reg {
                    *struct_addr = new_reg;
                }
            }
            Instruction::StructFieldStore {
                struct_addr, src, ..
            } => {
                if *struct_addr == old_reg {
                    *struct_addr = new_reg;
                }
                Self::replace_operand_register(src, old_reg, new_reg);
            }
            Instruction::StructFieldAddr {
                dst, struct_addr, ..
            } => {
                // 🔧 关键修复：替换目标寄存器
                if *dst == old_reg {
                    *dst = new_reg;
                }
                if *struct_addr == old_reg {
                    *struct_addr = new_reg;
                }
            }
            Instruction::Call {
                args,
                arg_operands,
                result,
                ..
            } => {
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
            Instruction::CallIndirect {
                function_register,
                args,
                arg_operands,
                result,
                ..
            } => {
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
            // EffectPerform: payload 使用的寄存器需要替换，result(若有)为定义寄存器
            Instruction::EffectPerform {
                tag,
                payload,
                result,
                ..
            } => {
                Self::replace_operand_register(tag, old_reg, new_reg);
                Self::replace_operand_register(payload, old_reg, new_reg);
                if let Some(ref mut res) = result {
                    if *res == old_reg {
                        *res = new_reg;
                    }
                }
            }
            Instruction::EffectPushHandler { tag, .. } => {
                Self::replace_operand_register(tag, old_reg, new_reg);
            }
            Instruction::EffectResume { value, .. } => {
                Self::replace_operand_register(value, old_reg, new_reg);
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
            Instruction::Retain { value, .. } | Instruction::Release { value, .. } => {
                if *value == old_reg {
                    *value = new_reg;
                }
            }
            Instruction::Alloc { dst, .. } | Instruction::StructAlloc { dst, .. } => {
                // 🔧 关键修复：替换目标寄存器
                if *dst == old_reg {
                    *dst = new_reg;
                }
            }
            Instruction::StringConcat { dst, left, right, .. } => {
                if *dst == old_reg {
                    *dst = new_reg;
                }
                if *left == old_reg {
                    *left = new_reg;
                }
                if *right == old_reg {
                    *right = new_reg;
                }
            }
            Instruction::PrintString { ptr, .. } => {
                if *ptr == old_reg {
                    *ptr = new_reg;
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
            Instruction::JumpIndirect {
                function_register, ..
            } => {
                if *function_register == old_reg {
                    *function_register = new_reg;
                }
            }
            Instruction::JumpRegister {
                target_register, ..
            } => {
                if *target_register == old_reg {
                    *target_register = new_reg;
                }
            }
            _ => {}
        }
    }

    /// 辅助方法：从操作数中提取寄存器
    fn add_operand_registers(&self, operand: &Operand, registers: &mut Vec<Register>) {
        match operand {
            Operand::Register { id } => registers.push(*id),
            Operand::Memory { base, .. } => registers.push(*base),
            Operand::StructField { struct_addr, .. } => registers.push(*struct_addr),
            _ => {}
        }
    }

    /// 辅助方法：替换操作数中的寄存器
    fn replace_operand_register(operand: &mut Operand, old_reg: Register, new_reg: Register) {
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
            Instruction::Move { span, .. }
            | Instruction::Add { span, .. }
            | Instruction::Sub { span, .. }
            | Instruction::Mul { span, .. }
            | Instruction::Div { span, .. }
            | Instruction::Mod { span, .. }
            | Instruction::Store64 { span, .. }
            | Instruction::Load64 { span, .. }
            | Instruction::Compare { span, .. }
            | Instruction::CompareSet { span, .. }
            | Instruction::CallIndirect { span, .. }
            | Instruction::Call { span, .. }
            | Instruction::StructFieldStore { span, .. }
            | Instruction::Phi { span, .. } => *span,
            _ => Span::dummy(),
        }
    }

    /// 获取指令定义和使用的寄存器
    ///
    /// 返回元组 (定义的寄存器, 使用的寄存器)
    pub(crate) fn get_defined_and_used_registers(&self) -> (Vec<Register>, Vec<Register>) {
        let mut defined = vec![];
        let mut used = vec![];

        match self {
            Instruction::Move { dst, src, .. } => {
                defined.push(*dst);
                self.add_operand_registers(src, &mut used);
            }
            Instruction::Add {
                dst, src1, src2, ..
            }
            | Instruction::Sub {
                dst, src1, src2, ..
            }
            | Instruction::Mul {
                dst, src1, src2, ..
            }
            | Instruction::Div {
                dst, src1, src2, ..
            }
            | Instruction::Mod {
                dst, src1, src2, ..
            }
            | Instruction::BitAnd {
                dst, src1, src2, ..
            }
            | Instruction::BitOr {
                dst, src1, src2, ..
            }
            | Instruction::BitXor {
                dst, src1, src2, ..
            }
            | Instruction::ShiftLeft {
                dst, src1, src2, ..
            }
            | Instruction::ShiftRight {
                dst, src1, src2, ..
            } => {
                defined.push(*dst);
                self.add_operand_registers(src1, &mut used);
                self.add_operand_registers(src2, &mut used);
            }
            Instruction::BitNot { dst, src, .. } => {
                defined.push(*dst);
                self.add_operand_registers(src, &mut used);
            }
            Instruction::Store64 { addr, src, .. }
            | Instruction::Store32 { addr, src, .. }
            | Instruction::Store8 { addr, src, .. } => {
                used.push(*addr);
                self.add_operand_registers(src, &mut used);
            }
            Instruction::Load64 { dst, addr, .. }
            | Instruction::Load32 { dst, addr, .. }
            | Instruction::Load8 { dst, addr, .. } => {
                defined.push(*dst);
                used.push(*addr);
            }
            Instruction::LoadGlobal { dst, .. } => {
                defined.push(*dst);
            }
            Instruction::Compare { src1, src2, .. } => {
                if let Operand::Register { id } = src1 {
                    used.push(*id);
                }
                if let Operand::Register { id } = src2 {
                    used.push(*id);
                }
            }
            Instruction::CompareSet { src1, src2, dst, .. } => {
                if let Operand::Register { id } = src1 {
                    used.push(*id);
                }
                if let Operand::Register { id } = src2 {
                    used.push(*id);
                }
                defined.push(*dst);
            }
            Instruction::Return { value, .. } => {
                if let Some(reg) = value {
                    used.push(*reg);
                }
            }
            Instruction::JumpRegister {
                target_register,
                span: _,
            } => {
                used.push(*target_register);
                // push all caller-saved registers
                used.push(Register::Virtual(1));
                used.push(Register::Virtual(2));
                used.push(Register::Virtual(3));
                used.push(Register::Virtual(4));
            }
            // JumpRegister 已移除
            Instruction::CallIndirect {
                function_register,
                args,
                arg_operands,
                result,
                ..
            } => {
                used.push(*function_register);
                used.extend_from_slice(args);
                // 添加参数操作数中使用的寄存器
                for operand in arg_operands {
                    if let Operand::Register { id } = operand {
                        used.push(*id);
                    }
                }
                if let Some(result_reg) = result {
                    defined.push(*result_reg);
                }
            }
            Instruction::Call {
                args,
                arg_operands,
                result,
                ..
            } => {
                used.extend_from_slice(args);
                // 添加参数操作数中使用的寄存器
                for operand in arg_operands {
                    if let Operand::Register { id } = operand {
                        used.push(*id);
                    }
                }
                if let Some(result_reg) = result {
                    defined.push(*result_reg);
                }
            }
            Instruction::Alloc { dst, .. } | Instruction::StructAlloc { dst, .. } => {
                defined.push(*dst);
            }
            Instruction::StringConcat { dst, left, right, .. } => {
                defined.push(*dst);
                used.push(*left);
                used.push(*right);
            }
            Instruction::PrintString { ptr, .. } => {
                used.push(*ptr);
            }
            Instruction::StructFieldStore {
                struct_addr, src, ..
            } => {
                used.push(*struct_addr);
                self.add_operand_registers(src, &mut used);
            }
            Instruction::StructFieldLoad {
                dst, struct_addr, ..
            } => {
                defined.push(*dst);
                used.push(*struct_addr);
            }
            Instruction::Phi { dst, incoming, .. } => {
                defined.push(*dst);
                for (_, operand) in incoming {
                    self.add_operand_registers(operand, &mut used);
                }
            }
            // EffectPerform: payload 是使用，result（若有）是定义
            Instruction::EffectPerform {
                payload,
                result,
                tag,
                ..
            } => {
                // FIX: 之前遗漏了 tag 操作数，导致寄存器分配阶段未认为其存活，
                // 使得 tag 与 payload 被分配到同一个物理寄存器，执行前 payload 覆盖 tag。
                // 这里加入 tag 的寄存器使用集合，确保分配不同寄存器或保持正确活跃区间。
                self.add_operand_registers(tag, &mut used);
                self.add_operand_registers(payload, &mut used);
                if let Some(result_reg) = result {
                    defined.push(*result_reg);
                }
            }
            // EffectResume: value 是使用
            Instruction::EffectResume { value, .. } => {
                self.add_operand_registers(value, &mut used);
            }
            _ => {} // 其他指令不涉及寄存器
        }

        (defined, used)
    }

    /// 检测指令是否使用虚拟寄存器
    pub fn uses_virtual_register(&self) -> bool {
        // 检查定义的寄存器
        if let Some(reg) = self.get_def_register() {
            if matches!(reg, Register::Virtual(_)) {
                return true;
            }
        }

        // 检查使用的寄存器
        for reg in self.get_used_registers() {
            if matches!(reg, Register::Virtual(_)) {
                return true;
            }
        }

        false
    }
}

/// LIR函数
#[derive(Debug, Clone, PartialEq, IrCodec)]
pub struct LirFunction {
    pub name: String,
    #[ir_codec(body, label = "body")]
    pub instructions: Vec<Instruction>,
    #[ir_codec(skip)]
    pub next_register: usize,
    #[ir_codec(skip)]
    pub struct_types: HashMap<StructTypeId, StructLayout>,
    #[ir_codec(skip)]
    pub stack_frame_size: usize,
    #[ir_codec(label = "params")]
    pub parameter_count: usize,
    #[ir_codec(label = "param_regs")]
    pub parameter_registers: Vec<Register>,
    /// 实际使用的寄存器列表（AArch64）
    #[ir_codec(skip)]
    pub used_regs: Vec<PhysicalRegister>,
    /// 降级后的指令的生命周期信息（用于JIT编译器优化寄存器保存）
    #[ir_codec(skip)]
    pub lowered_lifetimes: Option<Vec<crate::pass::register_allocation::RegisterLifetime>>,
    /// 降级后的寄存器映射（虚拟寄存器 -> 物理寄存器）
    #[ir_codec(skip)]
    pub lowered_register_mapping: Option<HashMap<Register, u8>>,
    /// 指令元数据侧表（指令索引 -> 元数据）
    ///
    /// 存储指令的额外信息（如 GC safepoint 的活跃寄存器）。
    /// 使用侧表而非修改 Instruction enum，保持最小化修改。
    #[ir_codec(skip)]
    pub instruction_metadata: HashMap<usize, InstructionMetadata>,
    /// 目标架构（用于 cross-compile 时选择正确的调用约定）
    /// None 表示使用编译主机默认架构
    #[ir_codec(skip)]
    pub target_arch: Option<String>,
}

impl LirFunction {
    pub fn new(name: String) -> Self {
        Self {
            name,
            instructions: Vec::new(),
            next_register: 0,
            struct_types: HashMap::new(),
            stack_frame_size: 0,
            parameter_count: 0,
            parameter_registers: Vec::new(),
            used_regs: Vec::new(),
            lowered_lifetimes: None,
            lowered_register_mapping: None,
            instruction_metadata: HashMap::new(),
            target_arch: None,
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
                function.parameter_registers.push(Register::Virtual(i + 1));
            }
        }

        function
    }

    /// 获取目标架构对应的调用约定
    pub fn get_calling_convention(&self) -> CallingConvention {
        CallingConvention::for_target(self.target_arch.as_deref().unwrap_or("x86_64"))
    }

    /// 获取实际使用的 callee-saved 寄存器列表
    pub fn get_used_regs(&self) -> &[PhysicalRegister] {
        &self.used_regs
    }

    /// 设置使用的 callee-saved 寄存器
    pub fn set_used_regs(&mut self, regs: Vec<PhysicalRegister>) {
        self.used_regs = regs;
    }

    /// 分配一个新的寄存器，跳过栈指针寄存器、帧指针寄存器和函数参数寄存器
    pub fn new_register(&mut self) -> Register {
        let cc = self.get_calling_convention();
        let stack_pointer_reg: usize = cc.stack_pointer as usize;
        let frame_pointer_reg: usize = cc.frame_pointer as usize;
        // 🔧 修复：跳过已分配的函数参数寄存器
        let parameter_registers: Vec<usize> =
            self.parameter_registers.iter().map(|r| r.id()).collect();

        loop {
            let id = Register::Virtual(self.next_register);
            self.next_register += 1;

            // 🔧 关键修复：跳过栈指针和帧指针寄存器
            if id.id() == stack_pointer_reg || id.id() == frame_pointer_reg {
                continue; // 跳过这些特殊寄存器
            }

            // 🔧 修复：如果分配到了函数参数寄存器，跳过它
            if !parameter_registers.contains(&id.id()) {
                return id;
            }
        }
    }

    /// 专门用于栈操作的寄存器分配（只返回栈指针寄存器）
    pub fn get_stack_pointer_register(&self) -> Register {
        Register::Physical(self.get_calling_convention().stack_pointer)
    }

    /// 检查一个寄存器是否是栈指针寄存器
    pub fn is_stack_pointer_register(&self, reg: &Register) -> bool {
        reg.id() == self.get_calling_convention().stack_pointer as usize
    }

    /// 检查一个寄存器是否是帧指针寄存器
    pub fn is_frame_pointer_register(&self, reg: &Register) -> bool {
        reg.id() == self.get_calling_convention().frame_pointer as usize
    }

    /// 验证指令是否违反栈指针寄存器使用规则
    /// 栈指针寄存器(RegisterId(6))只能用于栈操作和栈帧管理
    pub fn validate_stack_pointer_usage(&self) -> crate::Result<()> {
        // 🔧 修复：支持基于帧指针的栈帧管理代码
        for (index, instruction) in self.instructions.iter().enumerate() {
            match instruction {
                // 允许的栈操作
                Instruction::Alloc { .. } => {
                    // 栈分配操作允许使用栈指针
                }
                Instruction::Sub {
                    dst, src1, src2, ..
                } => {
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
                                ).into());
                            }
                        }
                    }
                }
                Instruction::Add {
                    dst, src1, src2, ..
                } => {
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
                                ).into());
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
                                ).into());
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
                        if self.is_stack_pointer_register(id)
                            && !self.is_stack_pointer_register(dst)
                        {
                            // 允许：将栈指针值复制到其他寄存器
                        }
                    }
                }
                Instruction::Store64 { addr, .. } => {
                    // 🔧 新增：允许存储到栈指针地址（函数序言保存调用者帧指针）
                    if self.is_stack_pointer_register(addr) {
                        // 允许：store [SP], FP 或其他值
                    }
                }
                Instruction::Load64 { addr, .. } => {
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
        LabelId(NEXT_LABEL.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
    }

    pub fn add_instruction(&mut self, instruction: Instruction) {
        // 如果是alloc且不是heap，放在开头
        if let Instruction::Alloc {
            allocation_type, ..
        } = &instruction
        {
            if *allocation_type != AllocationType::Heap {
                self.instructions.insert(1, instruction);
            } else {
                self.instructions.push(instruction);
            }
        } else {
            self.instructions.push(instruction);
        }
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
#[derive(Debug, Clone, PartialEq, IrCodec)]
#[ir_codec(program)]
pub struct LirProgram {
    pub functions: HashMap<String, LirFunction>,
    pub main_function: Option<String>,
    /// 全局结构体类型定义
    pub global_struct_types: HashMap<String, StructLayout>,
    /// 全局变量定义
    pub global_variables: HashMap<String, MemoryId>,
    /// 目标架构（用于 cross-compile 时选择正确的调用约定）
    /// 空字符串表示使用编译主机的默认架构
    #[ir_codec(skip)]
    pub target: String,
}

impl Default for LirProgram {
    fn default() -> Self {
        Self::new()
    }
}

static NEXT_LABEL: AtomicUsize = AtomicUsize::new(100000);

impl LirProgram {
    pub fn new() -> Self {
        Self {
            functions: HashMap::new(),
            main_function: None,
            global_struct_types: HashMap::new(),
            global_variables: HashMap::new(),
            target: String::new(),
        }
    }

    /// 设置目标架构
    pub fn set_target(&mut self, target: String) {
        self.target = target;
    }

    /// 获取目标架构
    pub fn target(&self) -> &str {
        if self.target.is_empty() {
            // 默认使用编译主机的架构
            if cfg!(target_arch = "x86_64") {
                "x86_64"
            } else if cfg!(target_arch = "aarch64") {
                "aarch64"
            } else {
                "x86_64"
            }
        } else {
            &self.target
        }
    }

    /// 获取当前目标架构的调用约定
    pub fn calling_convention(&self) -> karte_common::calling_convention::CallingConvention {
        karte_common::calling_convention::CallingConvention::for_target(self.target())
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

    /// 检测LIR是否包含虚拟寄存器（未优化）
    pub fn contains_virtual_registers(&self) -> bool {
        for function in self.functions.values() {
            for instruction in &function.instructions {
                if instruction.uses_virtual_register() {
                    return true;
                }
            }
        }
        false
    }
}
