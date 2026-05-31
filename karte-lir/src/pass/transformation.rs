use super::instruction_transformer::IndexInstructionTransformer;
use super::{AnalysisManager, FunctionPass, PassResult};
use crate::{Instruction, LirFunction, Operand, Register};
use karte_common::calling_convention::CallingConvention;
use karte_diagnostics::Span;
use log::{debug, info};
use std::collections::{HashMap, HashSet};

/// 死代码消除 Pass
///
/// 移除未使用的指令和寄存器定义
#[derive(Debug)]
pub struct DeadCodeElimination;

impl Default for DeadCodeElimination {
    fn default() -> Self {
        Self::new()
    }
}

impl DeadCodeElimination {
    pub fn new() -> Self {
        Self
    }

    /// 运行死代码消除
    fn eliminate_dead_code(&self, function: &mut LirFunction) -> bool {
        info!("🧹 开始死代码消除");

        let mut used_registers = HashSet::new();
        let mut defined_registers = HashMap::new();

        // 第一遍：收集所有有副作用的指令使用的寄存器
        for (i, instruction) in function.instructions.iter().enumerate() {
            if self.has_side_effects(instruction) {
                let used = self.get_used_registers(instruction);
                debug!(
                    "🧹 有副作用的指令 {}: {:?} 使用寄存器: {:?}",
                    i, instruction, used
                );
                for reg in used {
                    used_registers.insert(reg);
                }
            }

            // 记录定义
            if let Some(def_reg) = self.get_defined_register(instruction) {
                defined_registers.insert(def_reg, i);
            }
        }

        info!("🧹 初始使用的寄存器: {:?}", used_registers);

        // 第二遍：传播使用关系
        let mut changed = true;
        while changed {
            changed = false;
            let mut new_used = HashSet::new();

            for (i, instruction) in function.instructions.iter().enumerate() {
                if let Some(def_reg) = self.get_defined_register(instruction) {
                    if used_registers.contains(&def_reg) {
                        let used = self.get_used_registers(instruction);
                        debug!(
                            "🧹 指令 {} 定义了使用中的寄存器 {:?}，传播使用: {:?}",
                            i, def_reg, used
                        );
                        for reg in used {
                            if used_registers.insert(reg) {
                                new_used.insert(reg);
                                changed = true;
                            }
                        }
                    }
                }
            }
        }

        info!("🧹 传播后使用的寄存器: {:?}", used_registers);

        // 第三遍：移除死代码
        let mut transformer = IndexInstructionTransformer::new();
        for (i, instruction) in function.instructions.iter().enumerate() {
            if !self.has_side_effects(instruction) {
                if let Some(def_reg) = self.get_defined_register(instruction) {
                    if !used_registers.contains(&def_reg) {
                        transformer.remove(i);
                        if matches!(instruction, Instruction::Phi { .. }) {
                            debug!("🧹 移除未使用的φ节点: {:?}", instruction);
                        } else {
                            debug!("🧹 移除死代码: {:?}", instruction);
                        }
                    } else {
                        debug!(
                            "🧹 保留指令 {} (定义寄存器 {:?} 被使用): {:?}",
                            i, def_reg, instruction
                        );
                    }
                }
            }
        }
        let (changed, _, _, _) = transformer.apply_to_function(function);
        if changed {
            info!("🧹 死代码消除完成，有代码被移除");
        } else {
            info!("🧹 死代码消除完成，无代码被移除");
        }
        changed
    }

    /// 检查指令是否有副作用
    fn has_side_effects(&self, instruction: &Instruction) -> bool {
        match instruction {
            Instruction::Store64 { .. } => true,
            Instruction::StorePair { .. } => true,
            Instruction::Alloc { .. } => true,
            Instruction::StructAlloc { .. } => true,
            Instruction::StructFieldStore { .. } => true,
            Instruction::MemCopy { .. } => true,
            Instruction::Free { .. } => true,
            Instruction::Retain { .. } | Instruction::Release { .. } => true,
            Instruction::Call { .. } => true,
            Instruction::CallIndirect { .. } => true,
            Instruction::Return { .. } => true,
            Instruction::Jump { .. } => true,
            Instruction::JumpEqual { .. } => true,
            Instruction::JumpNotEqual { .. } => true,
            Instruction::JumpLess { .. } => true,
            Instruction::JumpLessEqual { .. } => true,
            Instruction::JumpGreater { .. } => true,
            Instruction::JumpGreaterEqual { .. } => true,
            Instruction::Compare { .. } => true,
            Instruction::CompareSet { .. } => true,
            Instruction::Label { .. } => true,
            // JumpRegister 已合并为 JumpIndirect
            Instruction::EffectPushHandler { .. }
            | Instruction::EffectPopHandler { .. }
            | Instruction::EffectPerform { .. }
            | Instruction::EffectResume { .. } => true,
            Instruction::Move { .. } => false,
            Instruction::Add { .. } => false,
            Instruction::Sub { .. } => false,
            Instruction::Mul { .. } => false,
            Instruction::Div { .. } => false,
            Instruction::Mod { .. } => false,
            Instruction::BitAnd { .. }
            | Instruction::BitOr { .. }
            | Instruction::BitXor { .. }
            | Instruction::ShiftLeft { .. }
            | Instruction::ShiftRight { .. } => false,
            Instruction::BitNot { .. } => false,
            Instruction::IntCast { .. } => false,
            Instruction::Load64 { .. } => false,
            Instruction::Load32 { .. } => false,
            Instruction::Load8 { .. } => false,
            Instruction::LoadGlobal { .. } => false,
            Instruction::GcRegOp { .. } => true, // 保存/恢复寄存器有副作用
            Instruction::Store32 { .. } => true,
            Instruction::Store8 { .. } => true,
            Instruction::LoadPair { .. } => false,
            Instruction::StructFieldLoad { .. } => false,
            Instruction::StructFieldAddr { .. } => false,
            Instruction::Nop { .. } => false,
            Instruction::Phi { .. } => false,
            Instruction::BitAnd { .. } => false,
            Instruction::BitOr { .. } => false,
            Instruction::BitXor { .. } => false,
            Instruction::ShiftLeft { .. } => false,
            Instruction::ShiftRight { .. } => false,
            Instruction::BitNot { .. } => false,
            Instruction::IntCast { .. } => false,
            Instruction::Safepoint { .. } => true,
            Instruction::JumpIndirect { .. } => true,
            Instruction::JumpRegister { .. } => true,
            Instruction::Safepoint { .. } => false,
            Instruction::StringConcat { .. } => true,
            Instruction::StringEqual { .. } => true,
            Instruction::StringCharAt { .. } => true,
            Instruction::StringSubstring { .. } => true,
            Instruction::StringContains { .. } => true,
            Instruction::SplitCount { .. } => true,
            Instruction::ToString { .. } => true,
            Instruction::PrintString { .. } => true,
            Instruction::PrintNumber { .. } => true,
            Instruction::PrintBool { .. } => true,
        }
    }

    /// 获取指令使用的寄存器
    fn get_used_registers(&self, instruction: &Instruction) -> Vec<Register> {
        let mut used = Vec::new();

        match instruction {
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
            Instruction::IntCast { src, .. } => {
                self.add_operand_registers(src, &mut used);
            }
            Instruction::Compare { src1, src2, .. }
            | Instruction::CompareSet { src1, src2, .. } => {
                self.add_operand_registers(src1, &mut used);
                self.add_operand_registers(src2, &mut used);
            }
            Instruction::Load64 { addr, .. }
            | Instruction::Load32 { addr, .. }
            | Instruction::Load8 { addr, .. } => {
                used.push(*addr);
            }
            Instruction::LoadGlobal { .. } => {
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
            Instruction::Call {
                args, arg_operands, ..
            } => {
                used.extend_from_slice(args);
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
                for operand in arg_operands {
                    self.add_operand_registers(operand, &mut used);
                }
            }
            Instruction::Return { value, .. } => {
                if let Some(reg) = value {
                    used.push(*reg);
                }
            }
            Instruction::StructFieldStore {
                struct_addr, src, ..
            } => {
                used.push(*struct_addr);
                self.add_operand_registers(src, &mut used);
            }
            Instruction::StructFieldLoad { struct_addr, .. }
            | Instruction::StructFieldAddr { struct_addr, .. } => {
                used.push(*struct_addr);
            }
            Instruction::MemCopy { src, .. } => {
                used.push(*src);
            }
            Instruction::Free { addr, .. } => {
                used.push(*addr);
            }
            Instruction::Retain { value, .. } | Instruction::Release { value, .. } => {
                used.push(*value);
            }
            Instruction::Phi { incoming, .. } => {
                // φ节点使用来自各个前驱块的值
                for (_, operand) in incoming {
                    self.add_operand_registers(operand, &mut used);
                }
            }
            // 处理 Effect 指令使用的寄存器
            Instruction::EffectPushHandler { tag, .. } => {
                self.add_operand_registers(tag, &mut used);
            }
            Instruction::EffectPerform { tag, payload, .. } => {
                self.add_operand_registers(tag, &mut used);
                self.add_operand_registers(payload, &mut used);
            }
            Instruction::EffectResume { value, .. } => {
                self.add_operand_registers(value, &mut used);
            }
            Instruction::JumpRegister {
                target_register, ..
            } => {
                used.push(*target_register);
                // push all caller-saved registers
                used.push(Register::Virtual(1));
                used.push(Register::Virtual(2));
                used.push(Register::Virtual(3));
                used.push(Register::Virtual(4));
            }
            Instruction::PrintNumber { value, .. } => {
                used.push(*value);
            }
            Instruction::PrintBool { value, .. } => {
                used.push(*value);
            }
            Instruction::PrintString { ptr, .. } => {
                used.push(*ptr);
            }
            Instruction::StringEqual { left, right, .. } => {
                used.push(*left);
                used.push(*right);
            }
            Instruction::StringCharAt { str_ptr, index, .. } => {
                used.push(*str_ptr);
                used.push(*index);
            }
            Instruction::StringSubstring { str_ptr, start, length, .. } => {
                used.push(*str_ptr);
                used.push(*start);
                used.push(*length);
            }
            Instruction::StringContains { str_ptr, char_code, .. } => {
                used.push(*str_ptr);
                used.push(*char_code);
            }
            Instruction::SplitCount { str_ptr, separator, .. } => {
                used.push(*str_ptr);
                used.push(*separator);
            }
            Instruction::ToString { value, .. } => {
                used.push(*value);
            }
            _ => {}
        }

        used
    }

    /// 添加操作数中的寄存器
    fn add_operand_registers(&self, operand: &Operand, registers: &mut Vec<Register>) {
        match operand {
            Operand::Register { id } => registers.push(*id),
            Operand::Memory { base, .. } => registers.push(*base),
            Operand::StructField { struct_addr, .. } => registers.push(*struct_addr),
            _ => {}
        }
    }

    /// 获取指令定义的寄存器
    fn get_defined_register(&self, instruction: &Instruction) -> Option<Register> {
        match instruction {
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
            | Instruction::IntCast { dst, .. }
            | Instruction::Load64 { dst, .. }
            | Instruction::Load32 { dst, .. }
            | Instruction::Load8 { dst, .. }
            | Instruction::LoadGlobal { dst, .. }
            | Instruction::Alloc { dst, .. }
            | Instruction::StructAlloc { dst, .. }
            | Instruction::StructFieldLoad { dst, .. }
            | Instruction::StructFieldAddr { dst, .. } => Some(*dst),
            Instruction::StringEqual { dst, .. } => Some(*dst),
            Instruction::StringCharAt { dst, .. } => Some(*dst),
            Instruction::StringSubstring { dst, .. } => Some(*dst),
            Instruction::StringContains { dst, .. } => Some(*dst),
            Instruction::SplitCount { dst, .. } => Some(*dst),
            Instruction::ToString { dst, .. } => Some(*dst),
            Instruction::LoadPair { dst1, .. } => Some(*dst1),
            Instruction::Call {
                result: Some(dst), ..
            }
            | Instruction::CallIndirect {
                result: Some(dst), ..
            } => Some(*dst),
            Instruction::Phi { dst, .. } => Some(*dst),
            Instruction::EffectPerform {
                result: Some(dst), ..
            } => Some(*dst),
            _ => None,
        }
    }
}

impl FunctionPass for DeadCodeElimination {
    fn name(&self) -> &str {
        "dce"
    }

    fn description(&self) -> &str {
        "死代码消除 - 移除未使用的指令和寄存器定义"
    }

    fn run_on_function(
        &mut self,
        function: &mut LirFunction,
        _analyses: &mut AnalysisManager,
    ) -> PassResult {
        if self.eliminate_dead_code(function) {
            PassResult::Changed
        } else {
            PassResult::Unchanged
        }
    }
    fn invalidated_analyses(&self) -> Vec<&'static str> {
        vec!["cfg", "def-use"]
    }
}

/// 常量折叠 Pass
///
/// 计算编译时可确定的常量表达式
#[derive(Debug)]
pub struct ConstantFolding;

impl Default for ConstantFolding {
    fn default() -> Self {
        Self::new()
    }
}

impl ConstantFolding {
    pub fn new() -> Self {
        Self
    }

    /// 运行常量折叠
    fn fold_constants(&self, function: &mut LirFunction) -> bool {
        let mut changed = false;
        let mut constant_values = HashMap::new();

        for instruction in function.instructions.iter_mut() {
            match instruction {
                Instruction::Move {
                    dst,
                    src: Operand::Immediate { value },
                    ..
                } => {
                    // 记录常量定义
                    constant_values.insert(*dst, *value);
                }
                Instruction::Add {
                    dst,
                    src1,
                    src2,
                    span,
                } => {
                    let dst_reg = *dst;
                    let span_copy = *span;
                    if let (Some(&val1), Some(&val2)) = (
                        self.get_constant_value(src1, &constant_values),
                        self.get_constant_value(src2, &constant_values),
                    ) {
                        let result = val1 + val2;
                        // 折叠加法
                        *instruction = Instruction::Move {
                            dst: dst_reg,
                            src: Operand::Immediate { value: result },
                            span: span_copy,
                        };
                        constant_values.insert(dst_reg, result);
                        changed = true;
                    }
                }
                Instruction::Sub {
                    dst,
                    src1,
                    src2,
                    span,
                } => {
                    let dst_reg = *dst;
                    let span_copy = *span;
                    if let (Some(&val1), Some(&val2)) = (
                        self.get_constant_value(src1, &constant_values),
                        self.get_constant_value(src2, &constant_values),
                    ) {
                        let result = val1 - val2;
                        // 折叠减法
                        *instruction = Instruction::Move {
                            dst: dst_reg,
                            src: Operand::Immediate { value: result },
                            span: span_copy,
                        };
                        constant_values.insert(dst_reg, result);
                        changed = true;
                    }
                }
                Instruction::Mul {
                    dst,
                    src1,
                    src2,
                    span,
                } => {
                    let dst_reg = *dst;
                    let span_copy = *span;
                    if let (Some(&val1), Some(&val2)) = (
                        self.get_constant_value(src1, &constant_values),
                        self.get_constant_value(src2, &constant_values),
                    ) {
                        let result = val1 * val2;
                        // 折叠乘法
                        *instruction = Instruction::Move {
                            dst: dst_reg,
                            src: Operand::Immediate { value: result },
                            span: span_copy,
                        };
                        constant_values.insert(dst_reg, result);
                        changed = true;
                    }
                }
                Instruction::Mod {
                    dst,
                    src1,
                    src2,
                    span,
                } => {
                    let dst_reg = *dst;
                    let span_copy = *span;
                    if let (Some(&val1), Some(&val2)) = (
                        self.get_constant_value(src1, &constant_values),
                        self.get_constant_value(src2, &constant_values),
                    ) {
                        if val2 != 0 {
                            // 折叠取余运算（避免除零）
                            let result = val1 % val2;
                            *instruction = Instruction::Move {
                                dst: dst_reg,
                                src: Operand::Immediate { value: result },
                                span: span_copy,
                            };
                            constant_values.insert(dst_reg, result);
                            changed = true;
                        }
                    }
                }
                Instruction::BitAnd {
                    dst,
                    src1,
                    src2,
                    span,
                } => {
                    let dst_reg = *dst;
                    let span_copy = *span;
                    let v1 = self.get_constant_value(src1, &constant_values);
                    let v2 = self.get_constant_value(src2, &constant_values);
                    if let (Some(&val1), Some(&val2)) = (v1, v2) {
                        let result = val1 & val2;
                        *instruction = Instruction::Move {
                            dst: dst_reg,
                            src: Operand::Immediate { value: result },
                            span: span_copy,
                        };
                        constant_values.insert(dst_reg, result);
                        changed = true;
                    }
                }
                Instruction::BitOr {
                    dst,
                    src1,
                    src2,
                    span,
                }
                | Instruction::BitXor {
                    dst,
                    src1,
                    src2,
                    span,
                }
                | Instruction::ShiftLeft {
                    dst,
                    src1,
                    src2,
                    span,
                }
                | Instruction::ShiftRight {
                    dst,
                    src1,
                    src2,
                    span,
                } => {
                    let dst_reg = *dst;
                    let span_copy = *span;
                    if let (Some(&val1), Some(&val2)) = (
                        self.get_constant_value(src1, &constant_values),
                        self.get_constant_value(src2, &constant_values),
                    ) {
                        let result = match instruction {
                            Instruction::BitOr { .. } => val1 | val2,
                            Instruction::BitXor { .. } => val1 ^ val2,
                            Instruction::ShiftLeft { .. } => val1 << (val2 & 63),
                            Instruction::ShiftRight { .. } => {
                                (val1 as u64 >> (val2 as u64 & 63)) as i64
                            }
                            _ => unreachable!(),
                        };
                        *instruction = Instruction::Move {
                            dst: dst_reg,
                            src: Operand::Immediate { value: result },
                            span: span_copy,
                        };
                        constant_values.insert(dst_reg, result);
                        changed = true;
                    }
                }
                Instruction::BitNot { dst, src, span } => {
                    let dst_reg = *dst;
                    let span_copy = *span;
                    if let Some(&val) = self.get_constant_value(src, &constant_values) {
                        let result = !val;
                        *instruction = Instruction::Move {
                            dst: dst_reg,
                            src: Operand::Immediate { value: result },
                            span: span_copy,
                        };
                        constant_values.insert(dst_reg, result);
                        changed = true;
                    }
                }
                _ => {
                    // 其他指令可能使常量值失效
                    if let Some(defined_reg) = self.get_defined_register(instruction) {
                        constant_values.remove(&defined_reg);
                    }
                }
            }
        }

        changed
    }

    /// 获取操作数的常量值
    fn get_constant_value<'a>(
        &self,
        operand: &'a Operand,
        constants: &'a HashMap<Register, i64>,
    ) -> Option<&'a i64> {
        match operand {
            Operand::Immediate { value } => Some(value),
            Operand::Register { id } => constants.get(id),
            _ => None,
        }
    }

    /// 获取指令定义的寄存器
    fn get_defined_register(&self, instruction: &Instruction) -> Option<Register> {
        match instruction {
            Instruction::Move { dst, .. }
            | Instruction::Add { dst, .. }
            | Instruction::Sub { dst, .. }
            | Instruction::Mul { dst, .. }
            | Instruction::Div { dst, .. }
            | Instruction::Mod { dst, .. }
            | Instruction::Load64 { dst, .. }
            | Instruction::Alloc { dst, .. }
            | Instruction::StructAlloc { dst, .. }
            | Instruction::StructFieldLoad { dst, .. }
            | Instruction::StructFieldAddr { dst, .. } => Some(*dst),
            Instruction::LoadPair { dst1, .. } => Some(*dst1),
            Instruction::Call {
                result: Some(dst), ..
            }
            | Instruction::CallIndirect {
                result: Some(dst), ..
            } => Some(*dst),
            Instruction::Phi { dst, .. } => Some(*dst),
            _ => None,
        }
    }
}

impl FunctionPass for ConstantFolding {
    fn name(&self) -> &str {
        "const-fold"
    }

    fn description(&self) -> &str {
        "常量折叠 - 计算编译时可确定的常量表达式"
    }

    fn run_on_function(
        &mut self,
        function: &mut LirFunction,
        _analyses: &mut AnalysisManager,
    ) -> PassResult {
        if self.fold_constants(function) {
            PassResult::Changed
        } else {
            PassResult::Unchanged
        }
    }
    fn invalidated_analyses(&self) -> Vec<&'static str> {
        vec!["cfg", "def-use"]
    }
}

/// 窥孔优化 Pass
///
/// 安全、局部的指令级简化：
/// - 折叠 push/pop 模式：`sub sp,sp,#8; store [sp], Rx; load Ry, [sp]; add sp,sp,#8` -> `mov Ry, Rx`
/// - 去重相邻重复 `mov`
/// - `add dst, src, #0` => `mov dst, src`
/// - 移除 `mov dst, dst`
#[derive(Debug)]
pub struct PeepholeOptimizer(CallingConvention);

impl Default for PeepholeOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

impl PeepholeOptimizer {
    pub fn new() -> Self {
        Self(CallingConvention::default())
    }

    fn optimize_function(&self, function: &mut LirFunction) -> bool {
        let mut changed = false;
        let mut transformer = IndexInstructionTransformer::new();

        let sp = Register::Physical(self.0.stack_pointer); // 约定的 SP

        // 小工具：判断操作数是否为指定寄存器
        let is_reg = |op: &Operand, reg: Register| -> bool {
            match op {
                Operand::Register { id } => *id == reg,
                _ => false,
            }
        };

        let len = function.instructions.len();
        let mut i = 0usize;
        while i < len {
            // 规则1：push/pop 折叠
            if i + 3 < function.instructions.len() {
                if let (
                    Instruction::Sub {
                        dst: d1,
                        src1: s1,
                        src2: Operand::Immediate { value: off1 },
                        ..
                    },
                    Instruction::Store64 {
                        addr: a1,
                        offset: off_st,
                        src: Operand::Register { id: src_saved },
                        ..
                    },
                    Instruction::Load64 {
                        dst: dst_restored,
                        addr: a2,
                        offset: off_ld,
                        ..
                    },
                    Instruction::Add {
                        dst: d2,
                        src1: s2,
                        src2: Operand::Immediate { value: off2 },
                        ..
                    },
                ) = (
                    &function.instructions[i],
                    &function.instructions[i + 1],
                    &function.instructions[i + 2],
                    &function.instructions[i + 3],
                ) {
                    if *d1 == sp
                        && is_reg(s1, sp)
                        && *off1 == 8
                        && *a1 == sp
                        && *off_st == 0
                        && *a2 == sp
                        && *off_ld == 0
                        && *d2 == sp
                        && is_reg(s2, sp)
                        && *off2 == 8
                    {
                        // sub sp; store [sp], src_saved; load dst_restored, [sp]; add sp
                        // => mov dst_restored, src_saved
                        transformer.replace(
                            i,
                            Instruction::Move {
                                dst: *dst_restored,
                                src: Operand::Register { id: *src_saved },
                                span: Span::dummy(),
                            },
                        );
                        transformer.remove(i + 1);
                        transformer.remove(i + 2);
                        transformer.remove(i + 3);
                        changed = true;
                        i += 4;
                        continue;
                    }
                }
            }

            // 规则2：add dst, src, #0 -> mov dst, src
            if let Instruction::Add {
                dst,
                src1,
                src2: Operand::Immediate { value: val },
                span,
            } = &function.instructions[i]
            {
                if *val == 0 {
                    if let Operand::Register { id: reg } = src1 {
                        transformer.replace(
                            i,
                            Instruction::Move {
                                dst: *dst,
                                src: Operand::Register { id: *reg },
                                span: *span,
                            },
                        );
                        changed = true;
                        i += 1;
                        continue;
                    }
                }
            }

            // 规则3：移除 mov dst, dst
            if let Instruction::Move { dst, src, .. } = &function.instructions[i] {
                if let Operand::Register { id } = src {
                    if *id == *dst {
                        transformer.remove(i);
                        changed = true;
                        i += 1;
                        continue;
                    }
                }
            }

            // 规则4：相邻重复 mov 去重（删除后一个）
            if i + 1 < function.instructions.len() {
                if let (
                    Instruction::Move {
                        dst: d1, src: s1, ..
                    },
                    Instruction::Move {
                        dst: d2, src: s2, ..
                    },
                ) = (&function.instructions[i], &function.instructions[i + 1])
                {
                    if d1 == d2 && s1 == s2 {
                        transformer.remove(i + 1);
                        changed = true;
                        i += 2;
                        continue;
                    }
                }
            }

            // 规则5：相邻同地址双重store，保留后者（覆盖前者）
            if i + 1 < function.instructions.len() {
                if let (
                    Instruction::Store64 {
                        addr: a1,
                        offset: o1,
                        ..
                    },
                    Instruction::Store64 {
                        addr: a2,
                        offset: o2,
                        ..
                    },
                ) = (&function.instructions[i], &function.instructions[i + 1])
                {
                    if a1 == a2 && o1 == o2 {
                        transformer.remove(i); // 删除前一个store
                        changed = true;
                        i += 1;
                        continue;
                    }
                }
            }

            // 规则6：相邻同地址双重load，第二个改为mov（若自赋值则直接删除第二个）
            if i + 1 < function.instructions.len() {
                if let (
                    Instruction::Load64 {
                        dst: d1,
                        addr: a1,
                        offset: o1,
                        ..
                    },
                    Instruction::Load64 {
                        dst: d2,
                        addr: a2,
                        offset: o2,
                        span,
                    },
                ) = (&function.instructions[i], &function.instructions[i + 1])
                {
                    if a1 == a2 && o1 == o2 {
                        if *d1 == *d2 {
                            // load同一地址两次且写同一寄存器，删除第二个
                            transformer.remove(i + 1);
                        } else {
                            transformer.replace(
                                i + 1,
                                Instruction::Move {
                                    dst: *d2,
                                    src: Operand::Register { id: *d1 },
                                    span: *span,
                                },
                            );
                        }
                        changed = true;
                        i += 2;
                        continue;
                    }
                }
            }

            // 规则7：load到寄存器后，立即写回同一地址：移除store（值未变更）
            if i + 1 < function.instructions.len() {
                if let (
                    Instruction::Load64 {
                        dst: d1,
                        addr: a1,
                        offset: o1,
                        ..
                    },
                    Instruction::Store64 {
                        addr: a2,
                        offset: o2,
                        src: Operand::Register { id: sreg },
                        ..
                    },
                ) = (&function.instructions[i], &function.instructions[i + 1])
                {
                    if a1 == a2 && o1 == o2 && *d1 == *sreg {
                        transformer.remove(i + 1);
                        changed = true;
                        i += 1;
                        continue;
                    }
                }
            }

            // 规则8：store后紧跟同地址load，用mov替换load（若自赋值则直接删除load）
            if i + 1 < function.instructions.len() {
                if let (
                    Instruction::Store64 {
                        addr: a1,
                        offset: o1,
                        src: Operand::Register { id: sreg },
                        ..
                    },
                    Instruction::Load64 {
                        dst: d2,
                        addr: a2,
                        offset: o2,
                        span,
                    },
                ) = (&function.instructions[i], &function.instructions[i + 1])
                {
                    if a1 == a2 && o1 == o2 {
                        if *d2 == *sreg {
                            transformer.remove(i + 1);
                        } else {
                            transformer.replace(
                                i + 1,
                                Instruction::Move {
                                    dst: *d2,
                                    src: Operand::Register { id: *sreg },
                                    span: *span,
                                },
                            );
                        }
                        changed = true;
                        i += 2;
                        continue;
                    }
                }
            }

            i += 1;
        }

        let (applied, ..) = transformer.apply_to_function(function);
        changed || applied
    }
}

impl FunctionPass for PeepholeOptimizer {
    fn name(&self) -> &str {
        "peephole"
    }

    fn description(&self) -> &str {
        "窥孔优化 - 局部指令级优化(消除冗余mov/load/store等)"
    }

    fn run_on_function(
        &mut self,
        function: &mut LirFunction,
        _analyses: &mut AnalysisManager,
    ) -> PassResult {
        if self.optimize_function(function) {
            PassResult::Changed
        } else {
            PassResult::Unchanged
        }
    }

    fn invalidated_analyses(&self) -> Vec<&'static str> {
        vec!["cfg", "def-use"]
    }
}
