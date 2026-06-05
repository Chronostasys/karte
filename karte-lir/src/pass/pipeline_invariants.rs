//! Pipeline invariant 检查
//!
//! 在关键 pass 执行后验证 IR 状态的正确性。
//! 所有检查仅在 debug 模式下编译，通过 log::warn! 输出违规信息（不 panic）。

use super::{AnalysisManager, FunctionPass, PassResult};
use crate::{Instruction, LirFunction, Operand, Register};
use std::collections::{HashMap, HashSet};

/// SSA 构造后的 invariant 检查
///
/// 检查项：
/// - 所有 Phi 的参数寄存器在同一函数中被定义
/// - 没有重复定义的非-Phi 寄存器（SSA 性质）
#[cfg(debug_assertions)]
pub fn check_ssa_post_invariants(function: &LirFunction) {
    let fn_name = &function.name;

    // 收集所有被定义的寄存器（区分 Phi 和非 Phi）
    let mut all_defined: HashMap<Register, bool> = HashMap::new(); // Register -> is_phi_def

    // 参数寄存器视为在入口处已定义
    let mut param_regs: HashSet<Register> = HashSet::new();
    for &reg in &function.parameter_registers {
        param_regs.insert(reg);
    }

    for instr in &function.instructions {
        // Phi 节点的定义
        if let Instruction::Phi { dst, incoming, .. } = instr {
            // 检查所有 incoming 操作数中的寄存器是否有定义
            for (label, operand) in incoming {
                if let Operand::Register { id: reg } = operand {
                    // 参数寄存器视为在入口处已定义
                    if !param_regs.contains(reg)
                        && !all_defined.contains_key(reg)
                        && !reg.is_physical()
                    {
                        log::warn!(
                            "[SSA invariant] 函数 {}: Phi 节点引用了未定义的寄存器 {:?} (来自标签 {:?})",
                            fn_name, reg, label
                        );
                    }
                }
            }

            // 检查 Phi dst 是否重复定义
            if let Some(&is_phi) = all_defined.get(dst) {
                if !is_phi {
                    log::warn!(
                        "[SSA invariant] 函数 {}: Phi 目标寄存器 {:?} 已被非 Phi 指令定义过",
                        fn_name, dst
                    );
                }
            }
            all_defined.insert(*dst, true);
        }

        // 非 Phi 指令的定义
        if let Some(def_reg) = instr.get_def_register() {
            // 跳过 Phi（上面已处理）
            if !matches!(instr, Instruction::Phi { .. }) {
                if let Some(&is_phi) = all_defined.get(&def_reg) {
                    // 非 Phi 寄存器重复定义违反 SSA
                    log::warn!(
                        "[SSA invariant] 函数 {}: 非 Phi 寄存器 {:?} 被重复定义 (之前是 {})",
                        fn_name,
                        def_reg,
                        if is_phi { "Phi" } else { "非 Phi" }
                    );
                }
                all_defined.insert(def_reg, false);
            }
        }
    }
}

/// Memory2Reg 之后的 invariant 检查
///
/// 检查项：
/// - 没有悬空的 Store64/Load64 对（addr 寄存器被 Alloc 定义但 Alloc 已被移除）
/// - 所有 mov 指令的 src 寄存器都被正确定义
#[cfg(debug_assertions)]
pub fn check_memory2reg_post_invariants(function: &LirFunction) {
    let fn_name = &function.name;

    // 收集所有 Alloc 指令定义的寄存器
    let mut alloc_defined: HashSet<Register> = HashSet::new();
    let mut all_defined: HashSet<Register> = HashSet::new();

    // 参数寄存器视为已定义
    for &reg in &function.parameter_registers {
        all_defined.insert(reg);
    }

    for instr in &function.instructions {
        // 记录 Alloc 定义的寄存器
        if let Instruction::Alloc { dst, .. } = instr {
            alloc_defined.insert(*dst);
        }

        // 记录所有定义
        if let Some(def_reg) = instr.get_def_register() {
            all_defined.insert(def_reg);
        }
    }

    // 检查 Store64/Load64 的 addr 是否被 Alloc 定义
    // Memory2Reg 应该消除大部分 Alloc-Load/Store 模式，
    // 但如果残留的 Load64/Store64 使用了被移除的 Alloc 寄存器，那就是悬空的
    for instr in &function.instructions {
        match instr {
            Instruction::Load64 { addr, .. } => {
                if alloc_defined.contains(addr) {
                    // addr 寄存器仍然由 Alloc 定义，检查这个 Alloc 是否还在指令列表中
                    // 如果 addr 被标记为 alloc 定义但指令中没有 Alloc，说明 Alloc 被移除了但引用残留
                    let alloc_still_exists = function.instructions.iter().any(|i| {
                        if let Instruction::Alloc { dst, .. } = i {
                            dst == addr
                        } else {
                            false
                        }
                    });
                    if !alloc_still_exists {
                        log::warn!(
                            "[Memory2Reg invariant] 函数 {}: Load64 引用了被移除的 Alloc 寄存器 {:?}",
                            fn_name, addr
                        );
                    }
                }
                // 检查 addr 是否被定义
                if !all_defined.contains(addr) && !addr.is_physical() {
                    log::warn!(
                        "[Memory2Reg invariant] 函数 {}: Load64 的 addr 寄存器 {:?} 未被定义",
                        fn_name, addr
                    );
                }
            }
            Instruction::Store64 { addr, src, .. } => {
                if alloc_defined.contains(addr) {
                    let alloc_still_exists = function.instructions.iter().any(|i| {
                        if let Instruction::Alloc { dst, .. } = i {
                            dst == addr
                        } else {
                            false
                        }
                    });
                    if !alloc_still_exists {
                        log::warn!(
                            "[Memory2Reg invariant] 函数 {}: Store64 引用了被移除的 Alloc 寄存器 {:?}",
                            fn_name, addr
                        );
                    }
                }
                if !all_defined.contains(addr) && !addr.is_physical() {
                    log::warn!(
                        "[Memory2Reg invariant] 函数 {}: Store64 的 addr 寄存器 {:?} 未被定义",
                        fn_name, addr
                    );
                }
                // 检查 src 操作数中的寄存器
                check_operand_defined_in_set(src, &all_defined, fn_name, "Store64 src");
            }
            Instruction::Move { src, .. } => {
                check_operand_defined_in_set(src, &all_defined, fn_name, "Move src");
            }
            _ => {}
        }
    }
}

/// 寄存器分配后的 invariant 检查
///
/// 检查项：
/// - 没有残留的 Virtual 寄存器在 Data 操作数中
///   （addr 寄存器可以是 StackAddress 类型，允许通过 Operand::Memory 表示）
#[cfg(debug_assertions)]
pub fn check_register_allocation_post_invariants(function: &LirFunction) {
    let fn_name = &function.name;

    for (idx, instr) in function.instructions.iter().enumerate() {
        // 检查指令定义的寄存器
        if let Some(def_reg) = instr.get_def_register() {
            if def_reg.is_virtual() {
                log::warn!(
                    "[RegAlloc invariant] 函数 {}: 指令 {} (idx={}) 定义了虚拟寄存器 {:?}",
                    fn_name,
                    instr_name(instr),
                    idx,
                    def_reg
                );
            }
        }

        // 检查指令使用的寄存器操作数
        check_instruction_operands_no_virtual(instr, fn_name, idx);
    }
}

/// 检查操作数中的寄存器是否在已定义集合中
#[cfg(debug_assertions)]
fn check_operand_defined_in_set(
    operand: &Operand,
    defined: &HashSet<Register>,
    fn_name: &str,
    context: &str,
) {
    match operand {
        Operand::Register { id: reg } => {
            if !defined.contains(reg) && !reg.is_physical() {
                log::warn!(
                    "[Memory2Reg invariant] 函数 {}: {} 操作数引用了未定义的寄存器 {:?}",
                    fn_name, context, reg
                );
            }
        }
        Operand::Memory { base, .. } => {
            if !defined.contains(base) && !base.is_physical() {
                log::warn!(
                    "[Memory2Reg invariant] 函数 {}: {} 操作数引用了未定义的基址寄存器 {:?}",
                    fn_name, context, base
                );
            }
        }
        _ => {}
    }
}

/// 检查指令的操作数中不应包含虚拟寄存器
///
/// 对于 Data 类型的操作数（即非地址操作数），寄存器分配后应该全部是物理寄存器。
/// 地址类操作数可以通过 Operand::Memory 表示（此时基址寄存器应是物理的或 FP/SP）。
#[cfg(debug_assertions)]
fn check_instruction_operands_no_virtual(instr: &Instruction, fn_name: &str, idx: usize) {
    let check_operand = |operand: &Operand, context: &str| {
        match operand {
            Operand::Register { id: reg } => {
                if reg.is_virtual() {
                    log::warn!(
                        "[RegAlloc invariant] 函数 {}: 指令 {} (idx={}) 的 {} 包含虚拟寄存器 {:?}",
                        fn_name,
                        instr_name(instr),
                        idx,
                        context,
                        reg
                    );
                }
            }
            Operand::Memory { base, .. } => {
                // Memory 操作数的基址寄存器也应是物理的
                if base.is_virtual() {
                    log::warn!(
                        "[RegAlloc invariant] 函数 {}: 指令 {} (idx={}) 的 {} Memory 基址包含虚拟寄存器 {:?}",
                        fn_name,
                        instr_name(instr),
                        idx,
                        context,
                        base
                    );
                }
            }
            // Immediate, Label, StructField, MemoryRef 不包含寄存器引用（或不是 Data 类型）
            _ => {}
        }
    };

    match instr {
        Instruction::Move { src, dst, .. } => {
            check_operand(src, "Move src");
            if dst.is_virtual() {
                log::warn!(
                    "[RegAlloc invariant] 函数 {}: Move dst (idx={}) 包含虚拟寄存器 {:?}",
                    fn_name, idx, dst
                );
            }
        }
        Instruction::Add { dst, src1, src2, .. }
        | Instruction::Sub { dst, src1, src2, .. }
        | Instruction::Mul { dst, src1, src2, .. }
        | Instruction::Div { dst, src1, src2, .. }
        | Instruction::Mod { dst, src1, src2, .. } => {
            check_operand(src1, "Arith src1");
            check_operand(src2, "Arith src2");
            if dst.is_virtual() {
                log::warn!(
                    "[RegAlloc invariant] 函数 {}: Arith dst (idx={}) 包含虚拟寄存器 {:?}",
                    fn_name, idx, dst
                );
            }
        }
        Instruction::Compare { src1, src2, .. }
        | Instruction::CompareSet { src1, src2, .. } => {
            check_operand(src1, "Cmp src1");
            check_operand(src2, "Cmp src2");
        }
        Instruction::Load64 { dst, addr, .. } => {
            if addr.is_virtual() {
                log::warn!(
                    "[RegAlloc invariant] 函数 {}: Load64 addr (idx={}) 包含虚拟寄存器 {:?}",
                    fn_name, idx, addr
                );
            }
        }
        Instruction::LoadGlobal { dst, .. } => {
            // LoadGlobal 不使用地址寄存器，只有 dst
        }
        Instruction::Store64 { addr, src, .. } => {
            if addr.is_virtual() {
                log::warn!(
                    "[RegAlloc invariant] 函数 {}: Store64 addr (idx={}) 包含虚拟寄存器 {:?}",
                    fn_name, idx, addr
                );
            }
            check_operand(src, "Store64 src");
        }
        Instruction::Call {
            args, arg_operands, result, ..
        } => {
            for (i, arg) in args.iter().enumerate() {
                if arg.is_virtual() {
                    log::warn!(
                        "[RegAlloc invariant] 函数 {}: Call args[{}] (idx={}) 包含虚拟寄存器 {:?}",
                        fn_name, i, idx, arg
                    );
                }
            }
            for (i, operand) in arg_operands.iter().enumerate() {
                check_operand(operand, &format!("Call arg_operands[{}]", i));
            }
            if let Some(res) = result {
                if res.is_virtual() {
                    log::warn!(
                        "[RegAlloc invariant] 函数 {}: Call result (idx={}) 包含虚拟寄存器 {:?}",
                        fn_name, idx, res
                    );
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
            if function_register.is_virtual() {
                log::warn!(
                    "[RegAlloc invariant] 函数 {}: CallIndirect function_register (idx={}) 包含虚拟寄存器 {:?}",
                    fn_name, idx, function_register
                );
            }
            for (i, arg) in args.iter().enumerate() {
                if arg.is_virtual() {
                    log::warn!(
                        "[RegAlloc invariant] 函数 {}: CallIndirect args[{}] (idx={}) 包含虚拟寄存器 {:?}",
                        fn_name, i, idx, arg
                    );
                }
            }
            for (i, operand) in arg_operands.iter().enumerate() {
                check_operand(operand, &format!("CallIndirect arg_operands[{}]", i));
            }
            if let Some(res) = result {
                if res.is_virtual() {
                    log::warn!(
                        "[RegAlloc invariant] 函数 {}: CallIndirect result (idx={}) 包含虚拟寄存器 {:?}",
                        fn_name, idx, res
                    );
                }
            }
        }
        Instruction::Return { value, .. } => {
            if let Some(reg) = value {
                if reg.is_virtual() {
                    log::warn!(
                        "[RegAlloc invariant] 函数 {}: Return value (idx={}) 包含虚拟寄存器 {:?}",
                        fn_name, idx, reg
                    );
                }
            }
        }
        Instruction::Phi { dst, incoming, .. } => {
            if dst.is_virtual() {
                log::warn!(
                    "[RegAlloc invariant] 函数 {}: Phi dst (idx={}) 包含虚拟寄存器 {:?}",
                    fn_name, idx, dst
                );
            }
            for (_, operand) in incoming {
                check_operand(operand, "Phi incoming");
            }
        }
        Instruction::StructFieldLoad {
            dst, struct_addr, ..
        } => {
            if struct_addr.is_virtual() {
                log::warn!(
                    "[RegAlloc invariant] 函数 {}: StructFieldLoad struct_addr (idx={}) 包含虚拟寄存器 {:?}",
                    fn_name, idx, struct_addr
                );
            }
        }
        Instruction::StructFieldStore {
            struct_addr, src, ..
        } => {
            if struct_addr.is_virtual() {
                log::warn!(
                    "[RegAlloc invariant] 函数 {}: StructFieldStore struct_addr (idx={}) 包含虚拟寄存器 {:?}",
                    fn_name, idx, struct_addr
                );
            }
            check_operand(src, "StructFieldStore src");
        }
        Instruction::StructFieldAddr {
            dst, struct_addr, ..
        } => {
            if struct_addr.is_virtual() {
                log::warn!(
                    "[RegAlloc invariant] 函数 {}: StructFieldAddr struct_addr (idx={}) 包含虚拟寄存器 {:?}",
                    fn_name, idx, struct_addr
                );
            }
        }
        Instruction::MemCopy { dst, src, .. } => {
            if src.is_virtual() {
                log::warn!(
                    "[RegAlloc invariant] 函数 {}: MemCopy src (idx={}) 包含虚拟寄存器 {:?}",
                    fn_name, idx, src
                );
            }
        }
        Instruction::Free { addr, .. } => {
            if addr.is_virtual() {
                log::warn!(
                    "[RegAlloc invariant] 函数 {}: Free addr (idx={}) 包含虚拟寄存器 {:?}",
                    fn_name, idx, addr
                );
            }
        }
        Instruction::Retain { value, .. } | Instruction::Release { value, .. } => {
            if value.is_virtual() {
                log::warn!(
                    "[RegAlloc invariant] 函数 {}: RC 操作 (idx={}) 包含虚拟寄存器 {:?}",
                    fn_name, idx, value
                );
            }
        }
        Instruction::JumpIndirect {
            function_register, ..
        } => {
            if function_register.is_virtual() {
                log::warn!(
                    "[RegAlloc invariant] 函数 {}: JumpIndirect (idx={}) 包含虚拟寄存器 {:?}",
                    fn_name, idx, function_register
                );
            }
        }
        Instruction::JumpRegister {
            target_register, ..
        } => {
            if target_register.is_virtual() {
                log::warn!(
                    "[RegAlloc invariant] 函数 {}: JumpRegister (idx={}) 包含虚拟寄存器 {:?}",
                    fn_name, idx, target_register
                );
            }
        }
        Instruction::EffectPerform {
            tag, payload, result, ..
        } => {
            check_operand(tag, "EffectPerform tag");
            check_operand(payload, "EffectPerform payload");
            if let Some(res) = result {
                if res.is_virtual() {
                    log::warn!(
                        "[RegAlloc invariant] 函数 {}: EffectPerform result (idx={}) 包含虚拟寄存器 {:?}",
                        fn_name, idx, res
                    );
                }
            }
        }
        Instruction::EffectResume { value, .. } => {
            check_operand(value, "EffectResume value");
        }
        Instruction::EffectPushHandler { tag, .. } => {
            check_operand(tag, "EffectPushHandler tag");
        }
        // Label, Jump*, Nop, Alloc, EffectPopHandler, Safepoint 等不涉及 Data 操作数中的寄存器
        _ => {}
    }
}

/// 获取指令的可读名称
#[cfg(debug_assertions)]
fn instr_name(instr: &Instruction) -> &'static str {
    match instr {
        Instruction::Move { .. } => "mov",
        Instruction::Add { .. } => "add",
        Instruction::Sub { .. } => "sub",
        Instruction::Mul { .. } => "mul",
        Instruction::Div { .. } => "div",
        Instruction::Mod { .. } => "mod",
        Instruction::Compare { .. } => "cmp",
        Instruction::CompareSet { .. } => "setcc",
        Instruction::Jump { .. } => "Jump",
        Instruction::JumpEqual { .. } => "je",
        Instruction::JumpNotEqual { .. } => "jne",
        Instruction::JumpGreater { .. } => "jg",
        Instruction::JumpGreaterEqual { .. } => "jge",
        Instruction::JumpLess { .. } => "jl",
        Instruction::JumpLessEqual { .. } => "jle",
        Instruction::Call { .. } => "Call",
        Instruction::CallIndirect { .. } => "CallIndirect",
        Instruction::JumpIndirect { .. } => "JumpIndirect",
        Instruction::JumpRegister { .. } => "JumpRegister",
        Instruction::Return { .. } => "Return",
        Instruction::Label { .. } => "Label",
        Instruction::Nop { .. } => "Nop",
        Instruction::EffectPushHandler { .. } => "EffectPushHandler",
        Instruction::EffectPopHandler { .. } => "EffectPopHandler",
        Instruction::EffectPerform { .. } => "EffectPerform",
        Instruction::EffectResume { .. } => "EffectResume",
        Instruction::StructAlloc { .. } => "StructAlloc",
        Instruction::StructFieldLoad { .. } => "StructFieldLoad",
        Instruction::StructFieldStore { .. } => "StructFieldStore",
        Instruction::StructFieldAddr { .. } => "StructFieldAddr",
        Instruction::MemCopy { .. } => "MemCopy",
        Instruction::Alloc { .. } => "Alloc",
        Instruction::Free { .. } => "Free",
        Instruction::Retain { .. } => "Retain",
        Instruction::Release { .. } => "Release",
        Instruction::Safepoint { .. } => "Safepoint",
        Instruction::StringConcat { .. } => "StringConcat",
        Instruction::StringEqual { .. } => "StringEqual",
        Instruction::StringCharAt { .. } => "StringCharAt",
        Instruction::StringSubstring { .. } => "StringSubstring",
        Instruction::StringContains { .. } => "StringContains",
        Instruction::SplitCount { .. } => "SplitCount",
        Instruction::Trim { .. } => "Trim",
        Instruction::CharToString { .. } => "CharToString",
            Instruction::ToString { .. } => "ToString",
        Instruction::PrintString { .. } => "PrintString",
        Instruction::PrintNumber { .. } => "PrintNumber",
        Instruction::PrintBool { .. } => "PrintBool",
        Instruction::Load64 { .. } => "Load64",
        Instruction::LoadGlobal { .. } => "LoadGlobal",
        Instruction::GcRegOp { .. } => "GcRegOp",
        Instruction::Store64 { .. } => "Store64",
        Instruction::Load32 { .. } => "Load32",
        Instruction::Store32 { .. } => "Store32",
        Instruction::Load8 { .. } => "Load8",
        Instruction::Store8 { .. } => "Store8",
        Instruction::StorePair { .. } => "StorePair",
        Instruction::LoadPair { .. } => "LoadPair",
        Instruction::Phi { .. } => "Phi",
        Instruction::BitAnd { .. } => "&",
        Instruction::BitOr { .. } => "|",
        Instruction::BitXor { .. } => "^",
        Instruction::ShiftLeft { .. } => "<<",
        Instruction::ShiftRight { .. } => ">>",
        Instruction::BitNot { .. } => "~",
        Instruction::IntCast { .. } => "intcast",
        Instruction::Panic { .. } => "Panic",
    }
}

/// Pipeline invariant 验证 Pass
///
/// 在关键 pass 后插入，调用对应的检查函数。
/// 仅在 debug 模式下生效，通过 log::warn! 输出违规信息（不 panic）。
#[derive(Debug)]
pub struct PipelineVerifyPass {
    /// 检查点类型
    checkpoint: VerifyCheckpoint,
}

/// 检查点类型
#[derive(Debug, Clone, Copy)]
pub enum VerifyCheckpoint {
    /// SSA 构造后
    AfterSsa,
    /// Memory2Reg 后
    AfterMemory2Reg,
    /// 寄存器分配后
    AfterRegisterAllocation,
}

impl PipelineVerifyPass {
    /// 创建 SSA 后验证 Pass
    pub fn after_ssa() -> Self {
        Self {
            checkpoint: VerifyCheckpoint::AfterSsa,
        }
    }

    /// 创建 Memory2Reg 后验证 Pass
    pub fn after_memory2reg() -> Self {
        Self {
            checkpoint: VerifyCheckpoint::AfterMemory2Reg,
        }
    }

    /// 创建寄存器分配后验证 Pass
    pub fn after_register_allocation() -> Self {
        Self {
            checkpoint: VerifyCheckpoint::AfterRegisterAllocation,
        }
    }
}

impl FunctionPass for PipelineVerifyPass {
    fn name(&self) -> &str {
        match self.checkpoint {
            VerifyCheckpoint::AfterSsa => "pipeline-verify-ssa",
            VerifyCheckpoint::AfterMemory2Reg => "pipeline-verify-mem2reg",
            VerifyCheckpoint::AfterRegisterAllocation => "pipeline-verify-regalloc",
        }
    }

    fn description(&self) -> &str {
        match self.checkpoint {
            VerifyCheckpoint::AfterSsa => "SSA 构造后的 invariant 检查",
            VerifyCheckpoint::AfterMemory2Reg => "Memory2Reg 后的 invariant 检查",
            VerifyCheckpoint::AfterRegisterAllocation => "寄存器分配后的 invariant 检查",
        }
    }

    fn run_on_function(
        &mut self,
        function: &mut LirFunction,
        _analyses: &mut AnalysisManager,
    ) -> PassResult {
        #[cfg(debug_assertions)]
        {
            match self.checkpoint {
                VerifyCheckpoint::AfterSsa => {
                    check_ssa_post_invariants(function);
                }
                VerifyCheckpoint::AfterMemory2Reg => {
                    check_memory2reg_post_invariants(function);
                }
                VerifyCheckpoint::AfterRegisterAllocation => {
                    check_register_allocation_post_invariants(function);
                }
            }
        }

        // 验证 pass 不修改 IR
        PassResult::Unchanged
    }
}
