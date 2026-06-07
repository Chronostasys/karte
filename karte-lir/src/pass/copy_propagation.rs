//! Copy Propagation + Store-Load Forwarding Pass
//!
//! Copy Propagation: `Move rX, rY` 后续使用 rX 的地方替换为 rY
//! Store-Load Forwarding: Store64 [addr+offset] = val 后续 Load64 rX, [addr+offset] 替换为 Move rX, val
//!
//! 安全性保证：
//! - 仅传播虚拟寄存器（Physical 寄存器不传播）
//! - 寄存器被重定义时立即清除映射
//! - Store-Load 传播时追踪地址和值的变更
//! - 控制流标签/跳转时清空映射（保守安全）
//! - Call/CallIndirect 清空 Store-Load 映射（可能修改任意内存）

use super::instruction_transformer::IndexInstructionTransformer;
use super::{AnalysisManager, FunctionPass, PassResult};
use crate::{Instruction, LirFunction, Operand, Register};

/// 判断寄存器是否为物理寄存器
fn is_physical(reg: Register) -> bool {
    matches!(reg, Register::Physical(_))
}

/// 获取指令中所有被定义（写入）的寄存器
fn defs(instr: &Instruction) -> Vec<Register> {
    match instr {
        Instruction::Move { dst, .. } => vec![*dst],
        Instruction::Load64 { dst, .. } => vec![*dst],
        Instruction::Add { dst, .. } | Instruction::Sub { dst, .. } | Instruction::Mul { dst, .. }
        | Instruction::Div { dst, .. } | Instruction::Mod { dst, .. }
        | Instruction::BitAnd { dst, .. } | Instruction::BitOr { dst, .. }
        | Instruction::BitXor { dst, .. } | Instruction::BitNot { dst, .. }
        | Instruction::IntCast { dst, .. } | Instruction::ShiftLeft { dst, .. }
        | Instruction::ShiftRight { dst, .. } | Instruction::CompareSet { dst, .. }
        | Instruction::Alloc { dst, .. } | Instruction::LoadGlobal { dst, .. }
        | Instruction::Phi { dst, .. } | Instruction::StructAlloc { dst, .. }
        | Instruction::StructFieldLoad { dst, .. } | Instruction::StructFieldAddr { dst, .. }
        | Instruction::StringConcat { dst, .. } | Instruction::StringEqual { dst, .. }
        | Instruction::StringCharAt { dst, .. } | Instruction::StringSubstring { dst, .. }
        | Instruction::StringCompare { dst, .. } | Instruction::CharToString { dst, .. }
        | Instruction::GcAlloc { dst, .. } | Instruction::MemLoad64 { dst, .. }
        | Instruction::ToString { dst, .. } => vec![*dst],
        Instruction::Call { result, .. } | Instruction::CallIndirect { result, .. } => {
            result.map(|r| vec![r]).unwrap_or_default()
        }
        Instruction::LoadPair { dst1, dst2, .. } => vec![*dst1, *dst2],
        _ => vec![],
    }
}

/// 获取指令中所有被使用（读取）的寄存器
fn uses_regs(instr: &Instruction) -> Vec<Register> {
    match instr {
        Instruction::Move { src, .. } => op_regs(src),
        Instruction::Store64 { addr, src, .. } => {
            let mut r = op_regs(src); r.push(*addr); r
        }
        Instruction::Add { src1, src2, .. } | Instruction::Sub { src1, src2, .. }
        | Instruction::Mul { src1, src2, .. } | Instruction::Div { src1, src2, .. }
        | Instruction::Mod { src1, src2, .. } | Instruction::BitAnd { src1, src2, .. }
        | Instruction::BitOr { src1, src2, .. } | Instruction::BitXor { src1, src2, .. }
        | Instruction::ShiftLeft { src1, src2, .. } | Instruction::ShiftRight { src1, src2, .. } => {
            let mut r = op_regs(src1); r.extend(op_regs(src2)); r
        }
        Instruction::BitNot { src, .. } | Instruction::IntCast { src, .. } => op_regs(src),
        Instruction::Compare { src1, src2, .. } => {
            let mut r = op_regs(src1); r.extend(op_regs(src2)); r
        }
        Instruction::Call { args, arg_operands, .. } => {
            let mut r: Vec<_> = args.iter().copied().collect();
            for op in arg_operands { r.extend(op_regs(op)); }
            r
        }
        Instruction::CallIndirect { function_register, args, arg_operands, .. } => {
            let mut r = vec![*function_register];
            r.extend(args.iter().copied());
            for op in arg_operands { r.extend(op_regs(op)); }
            r
        }
        Instruction::Return { value, .. } => value.map(|r| vec![r]).unwrap_or_default(),
        Instruction::Phi { incoming, .. } => {
            incoming.iter().flat_map(|(_, v)| op_regs(v)).collect()
        }
        Instruction::StructFieldStore { struct_addr, src, .. } => {
            let mut r = vec![*struct_addr]; r.extend(op_regs(src)); r
        }
        Instruction::StructFieldAddr { struct_addr, .. } => vec![*struct_addr],
        Instruction::MemStore64 { addr, value, .. } => vec![*addr, *value],
        Instruction::MemLoad64 { addr, .. } => vec![*addr],
        _ => vec![],
    }
}

fn op_regs(op: &Operand) -> Vec<Register> {
    match op {
        Operand::Register { id } => vec![*id],
        _ => vec![],
    }
}

/// 替换操作数中的寄存器引用
fn replace_op(op: &mut Operand, from: Register, to: &Operand) -> bool {
    if let Operand::Register { id } = op {
        if *id == from { *op = to.clone(); return true; }
    }
    false
}

/// 替换指令中 src 操作数（不碰 dst）
fn replace_uses(instr: &mut Instruction, from: Register, to: &Operand) -> bool {
    let mut c = false;
    match instr {
        Instruction::Move { src, .. } => c |= replace_op(src, from, to),
        Instruction::Store64 { src, .. } => c |= replace_op(src, from, to),
        Instruction::Add { src1, src2, .. } | Instruction::Sub { src1, src2, .. }
        | Instruction::Mul { src1, src2, .. } | Instruction::Div { src1, src2, .. }
        | Instruction::Mod { src1, src2, .. } | Instruction::BitAnd { src1, src2, .. }
        | Instruction::BitOr { src1, src2, .. } | Instruction::BitXor { src1, src2, .. }
        | Instruction::ShiftLeft { src1, src2, .. } | Instruction::ShiftRight { src1, src2, .. } => {
            c |= replace_op(src1, from, to);
            c |= replace_op(src2, from, to);
        }
        Instruction::BitNot { src, .. } | Instruction::IntCast { src, .. } => c |= replace_op(src, from, to),
        Instruction::Compare { src1, src2, .. } => {
            c |= replace_op(src1, from, to);
            c |= replace_op(src2, from, to);
        }
        Instruction::Call { arg_operands, .. } => {
            for a in arg_operands { c |= replace_op(a, from, to); }
        }
        Instruction::CallIndirect { arg_operands, .. } => {
            for a in arg_operands { c |= replace_op(a, from, to); }
        }
        Instruction::Return { value, .. } => {
            if let Some(r) = value {
                if *r == from {
                    if let Operand::Register { id } = to {
                        *value = Some(*id);
                        c = true;
                    }
                }
            }
        }
        Instruction::Phi { incoming, .. } => {
            for (_, v) in incoming.iter_mut() { c |= replace_op(v, from, to); }
        }
        Instruction::StructFieldStore { src, .. } => c |= replace_op(src, from, to),
        Instruction::MemLoad64 { addr, .. } => {
            // MemLoad64 addr 是 Register 不是 Operand，不能传播
            let _ = (addr, from, to);
        }
        Instruction::MemStore64 { .. } => {
            // MemStore64 的 value 是 Register 不是 Operand，不能传播
        }
        _ => {}
    }
    c
}

// ============================================================
// CopyPropagation
// ============================================================

pub struct CopyPropagation;

impl CopyPropagation {
    pub fn new() -> Self { Self }

    fn optimize_function(&self, function: &mut LirFunction) -> bool {
        use std::collections::HashMap;
        let mut changed = false;
        let mut transformer = IndexInstructionTransformer::new();
        let mut copy_map: HashMap<Register, Operand> = HashMap::new();

        for (i, instr) in function.instructions.iter().enumerate() {
            // 基本块边界：清空
            if matches!(instr, Instruction::Label { .. } | Instruction::Jump { .. }) {
                copy_map.clear();
            }

            // 替换使用
            let mut new_instr = instr.clone();
            let mut instr_changed = false;
            for (&from_reg, to_op) in &copy_map {
                if uses_regs(&new_instr).contains(&from_reg) && !defs(&new_instr).contains(&from_reg) {
                    instr_changed |= replace_uses(&mut new_instr, from_reg, to_op);
                }
            }
            if instr_changed {
                transformer.replace(i, new_instr);
                changed = true;
            }

            // 清除被重定义的映射
            for def_reg in defs(instr) {
                copy_map.remove(&def_reg);
                copy_map.retain(|_, v| if let Operand::Register { id } = v { *id != def_reg } else { true });
            }

            // Move: 建立映射（含传递闭包）
            if let Instruction::Move { dst, src, .. } = instr {
                if !is_physical(*dst) {
                    let eff = if let Operand::Register { id } = src {
                        copy_map.get(id).cloned().unwrap_or_else(|| src.clone())
                    } else {
                        src.clone()
                    };
                    match &eff {
                        Operand::Register { id } if !is_physical(*id) && *id != *dst => {
                            copy_map.insert(*dst, eff);
                        }
                        Operand::Immediate { .. } => {
                            copy_map.insert(*dst, eff);
                        }
                        _ => {}
                    }
                }
            }

            // Call 可能修改物理寄存器
            if matches!(instr, Instruction::Call { .. } | Instruction::CallIndirect { .. }) {
                copy_map.retain(|r, _| !is_physical(*r));
            }
        }

        let (applied, ..) = transformer.apply_to_function(function);
        changed || applied
    }
}

impl Default for CopyPropagation {
    fn default() -> Self { Self::new() }
}

impl FunctionPass for CopyPropagation {
    fn name(&self) -> &str { "copy_propagation" }
    fn description(&self) -> &str { "Copy Propagation" }
    fn run_on_function(&mut self, f: &mut LirFunction, _: &mut AnalysisManager) -> PassResult {
        if self.optimize_function(f) { PassResult::Changed } else { PassResult::Unchanged }
    }
    fn invalidated_analyses(&self) -> Vec<&'static str> { vec!["def-use"] }
}

// ============================================================
// Store-Load Forwarding
// ============================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct MemLoc { addr: Register, offset: i64 }

pub struct StoreLoadForwarding;

impl StoreLoadForwarding {
    pub fn new() -> Self { Self }

    fn optimize_function(&self, function: &mut LirFunction) -> bool {
        use std::collections::HashMap;
        let mut changed = false;
        let mut transformer = IndexInstructionTransformer::new();
        let mut store_map: HashMap<MemLoc, Operand> = HashMap::new();

        for (i, instr) in function.instructions.iter().enumerate() {
            // 控制流边界：清空
            if matches!(instr,
                Instruction::Label { .. } | Instruction::Jump { .. }
                | Instruction::JumpEqual { .. } | Instruction::JumpNotEqual { .. }
                | Instruction::JumpGreater { .. } | Instruction::JumpGreaterEqual { .. }
                | Instruction::JumpLess { .. } | Instruction::JumpLessEqual { .. }
            ) {
                store_map.clear();
                continue;
            }

            match instr {
                Instruction::Store64 { addr, offset, src, .. } => {
                    store_map.insert(MemLoc { addr: *addr, offset: *offset }, src.clone());
                }
                Instruction::Load64 { dst, addr, offset, span } => {
                    let loc = MemLoc { addr: *addr, offset: *offset };
                    if let Some(val) = store_map.get(&loc) {
                        if *dst != *addr {
                            transformer.replace(i, Instruction::Move { dst: *dst, src: val.clone(), span: *span });
                            changed = true;
                        }
                    }
                }
                _ => {}
            }

            // 清除因重定义而失效的映射
            for def_reg in defs(instr) {
                store_map.retain(|k, v| {
                    if k.addr == def_reg { return false; }
                    if let Operand::Register { id } = v { if *id == def_reg { return false; } }
                    true
                });
            }

            // Call 清空
            if matches!(instr, Instruction::Call { .. } | Instruction::CallIndirect { .. }) {
                store_map.clear();
            }
        }

        let (applied, ..) = transformer.apply_to_function(function);
        changed || applied
    }
}

impl Default for StoreLoadForwarding {
    fn default() -> Self { Self::new() }
}

impl FunctionPass for StoreLoadForwarding {
    fn name(&self) -> &str { "store_load_forwarding" }
    fn description(&self) -> &str { "Store-Load Forwarding" }
    fn run_on_function(&mut self, f: &mut LirFunction, _: &mut AnalysisManager) -> PassResult {
        if self.optimize_function(f) { PassResult::Changed } else { PassResult::Unchanged }
    }
    fn invalidated_analyses(&self) -> Vec<&'static str> { vec!["def-use", "memory"] }
}
