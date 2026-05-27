//! x86-64 JIT编译器
//!
//! 将LIR指令编译为x86-64机器码
//! 使用 System V ABI 调用约定
//!
//! 寄存器编号（与 karte-common calling_convention x86_64 版一致）：
//! 0=RAX, 1=RCX, 2=RDX, 3=RBX, 4=RSP, 5=RBP, 6=RSI, 7=RDI,
//! 8=R8, 9=R9, 10=R10, 11=R11, 12=R12, 13=R13, 14=R14, 15=R15

use super::code_buffer::{CodeBuilder, JumpType};
use super::compiler_trait::*;
use super::ffi::{RuntimeArg, RuntimeCall};
use karte_common::calling_convention::{CallingConvention, CC};
use karte_lir::{ComparisonCondition, Instruction, LirFunction, LirProgram, Operand, Register};
use std::collections::HashMap;

/// x86-64编译器
#[derive(Debug)]
pub struct X86Compiler {
    /// 调试模式
    debug_mode: bool,
    /// 当前函数使用的 callee-saved 寄存器列表
    current_function_used_regs: Vec<u8>,
    /// 当前函数的栈帧大小（用于 FP 偏移量）
    current_stack_frame_size: usize,
    /// 内部函数 epilogue 需要跳过的栈帧大小
    /// LIR 指令（Sub vm_sp, N）分配帧空间，prologue 不分配，
    /// 但 epilogue 需要知道帧大小才能正确恢复 callee-saved
    stack_frame_size_for_epilogue: usize,
}

impl X86Compiler {
    /// 创建新的x86编译器
    pub fn new(debug_mode: bool) -> crate::Result<Self> {
        Ok(Self {
            debug_mode: true,
            current_function_used_regs: Vec::new(),
            current_stack_frame_size: 0,
            stack_frame_size_for_epilogue: 0,
        })
    }

    // 寄存器映射：x86_64 硬件特殊寄存器需要映射
    // Physical(4=RSP) → R10 (虚拟栈指针，不能改硬件RSP)
    // Physical(5=RBP) → R11 (虚拟帧指针，不能改硬件RBP)
    // 其他寄存器保持不变
    /// LIR 物理寄存器到 x86_64 硬件寄存器的映射
    /// 由于 CallingConvention 现在直接使用 REG_R10 和 REG_R11 作为
    /// stack_pointer 和 frame_pointer，不再需要重映射
    fn map_register(&self, reg: u8) -> u8 {
        reg  // 1:1 映射，不需要重映射
    }

    /// 反向映射：x86_64 硬件寄存器 → LIR Physical 编号
    fn unmap_register(&self, hw_reg: u8) -> u8 {
        hw_reg  // 1:1 映射，不需要重映射
    }

    /// 获取物理寄存器编号（映射后的 x86_64 硬件寄存器）
    fn get_physical_register(&self, reg: &Register) -> crate::Result<u8> {
        match reg {
            Register::Physical(id) => {
                if *id >= 16 {
                    Err(format!("x86_64 不支持寄存器编号 {}: 最多16个通用寄存器", id).into())
                } else {
                    Ok(self.map_register(*id))
                }
            }
            Register::Virtual(id) => {
                // 虚拟寄存器不应出现在 JIT 阶段，寄存器分配必须在 JIT 之前完成
                panic!("JIT 编译器遇到虚拟寄存器 Virtual({})，寄存器分配应在 JIT 之前完成", id);
            }
        }
    }

    /// 获取未映射的 LIR 物理寄存器编号
    fn get_lir_register(&self, reg: &Register) -> crate::Result<u8> {
        match reg {
            Register::Physical(id) => {
                if *id >= 16 {
                    Err(format!("x86_64 不支持寄存器编号 {}", id).into())
                } else {
                    Ok(*id)
                }
            }
            Register::Virtual(id) => {
                Err(format!("x86_64 JIT 遇到虚拟寄存器 v{}", id).into())
            }
        }
    }

    /// 编译单个指令
    fn compile_instruction(
        &mut self,
        instruction: &Instruction,
        code_builder: &mut CodeBuilder,
        _program: &LirProgram,
        is_main_function: bool,
    ) -> crate::Result<()> {
        if self.debug_mode {
            log::debug!("x86 编译指令: {:?}", instruction);
        }

        match instruction {
            Instruction::Move { dst, src, .. } => self.compile_move(dst, src, code_builder),
            Instruction::Add { dst, src1, src2, .. } => {
                self.compile_add(dst, src1, src2, code_builder)
            }
            Instruction::Sub { dst, src1, src2, .. } => {
                self.compile_sub(dst, src1, src2, code_builder)
            }
            Instruction::Mul { dst, src1, src2, .. } => {
                self.compile_mul(dst, src1, src2, code_builder)
            }
            Instruction::Div { dst, src1, src2, .. } => {
                self.compile_div(dst, src1, src2, code_builder)
            }
            Instruction::BitAnd { dst, src1, src2, .. } => {
                self.compile_bitand(dst, src1, src2, code_builder)
            }
            Instruction::BitOr { dst, src1, src2, .. } => {
                self.compile_bitor(dst, src1, src2, code_builder)
            }
            Instruction::BitXor { dst, src1, src2, .. } => {
                self.compile_bitxor(dst, src1, src2, code_builder)
            }
            Instruction::ShiftLeft { dst, src1, src2, .. } => {
                self.compile_shift_left(dst, src1, src2, code_builder)
            }
            Instruction::ShiftRight { dst, src1, src2, .. } => {
                self.compile_shift_right(dst, src1, src2, code_builder)
            }
            Instruction::BitNot { dst, src, .. } => {
                self.compile_bitnot(dst, src, code_builder)
            }
            Instruction::Compare { src1, src2, .. } => {
                self.compile_compare(src1, src2, code_builder)
            }
            Instruction::CompareSet { dst, condition, src1, src2, .. } => {
                self.compile_compare_set(dst, condition, src1, src2, code_builder)
            }
            Instruction::Jump { target, .. } => self.compile_jump(target, code_builder),
            Instruction::JumpEqual { target, .. } => {
                self.compile_conditional_jump(JumpType::ConditionalEqual, target, code_builder)
            }
            Instruction::JumpNotEqual { target, .. } => self.compile_conditional_jump(
                JumpType::ConditionalNotEqual,
                target,
                code_builder,
            ),
            Instruction::JumpLess { target, .. } => {
                self.compile_conditional_jump(JumpType::ConditionalLess, target, code_builder)
            }
            Instruction::JumpLessEqual { target, .. } => self.compile_conditional_jump(
                JumpType::ConditionalLessEqual,
                target,
                code_builder,
            ),
            Instruction::JumpGreater { target, .. } => self.compile_conditional_jump(
                JumpType::ConditionalGreater,
                target,
                code_builder,
            ),
            Instruction::JumpGreaterEqual { target, .. } => self.compile_conditional_jump(
                JumpType::ConditionalGreaterEqual,
                target,
                code_builder,
            ),
            Instruction::Call { target, .. } => self.compile_call(target, code_builder),
            Instruction::JumpIndirect { function_register, .. } => {
                self.compile_jump_indirect(function_register, code_builder)
            }
            Instruction::JumpRegister { target_register, .. } => {
                self.compile_jump_register(target_register, code_builder)
            }
            Instruction::Return { value, .. } => {
                self.compile_return(value.as_ref(), code_builder, is_main_function)
            }
            Instruction::Label { id, .. } => {
                let label_name = format!("label_{}", id.0);
                code_builder.define_label(&label_name)?;
                Ok(())
            }
            Instruction::Load64 { dst, addr, offset, .. } => {
                self.compile_load64(dst, addr, *offset, code_builder)
            }
            Instruction::Store64 { addr, offset, src, .. } => {
                self.compile_store64(addr, *offset, src, code_builder)
            }
            Instruction::Load32 { dst, addr, offset, .. } => {
                self.compile_load32(dst, addr, *offset, code_builder)
            }
            Instruction::Store32 { addr, offset, src, .. } => {
                self.compile_store32(addr, *offset, src, code_builder)
            }
            Instruction::Load8 { dst, addr, offset, .. } => {
                self.compile_load8(dst, addr, *offset, code_builder)
            }
            Instruction::Store8 { addr, offset, src, .. } => {
                self.compile_store8(addr, *offset, src, code_builder)
            }
            // StorePair 拆分为两条 Store64
            Instruction::StorePair { addr, offset, src1, src2, .. } => {
                self.compile_store_pair(addr, *offset, src1, src2, code_builder)
            }
            // LoadPair 拆分为两条 Load64
            Instruction::LoadPair { dst1, dst2, addr, offset, .. } => {
                self.compile_load_pair(dst1, dst2, addr, *offset, code_builder)
            }
            Instruction::Alloc { dst, size, alignment, allocation_type, .. } => {
                self.compile_alloc(dst, *size, *alignment, allocation_type, code_builder)
            }
            Instruction::Free { addr, .. } => self.compile_free(addr, code_builder),
            Instruction::Retain { value, .. } => self.compile_retain(value, code_builder),
            Instruction::Release { value, .. } => self.compile_release(value, code_builder),
            Instruction::Safepoint { .. } => self.compile_safepoint(code_builder),
            Instruction::Nop { .. } => {
                // x86 NOP
                code_builder.emit_byte(0x90);
                Ok(())
            }
            Instruction::StructAlloc { .. } => {
                // StructAlloc 在指令降级后应该是 Alloc
                Err("StructAlloc 应该已经被降级为 Alloc".into())
            }
            Instruction::StructFieldLoad { .. }
            | Instruction::StructFieldStore { .. }
            | Instruction::StructFieldAddr { .. } => {
                // 这些应该已经被降级为 Load64/Store64
                Err(format!("Struct 操作应该已经被降级: {:?}", instruction).into())
            }
            Instruction::MemCopy { .. } => {
                // MemCopy 应该已经被降级为多条 Load64/Store64
                Err("MemCopy 应该已经被降级".into())
            }
            Instruction::LoadGlobal { dst, name, .. } => {
                self.compile_load_global(dst, name, code_builder)
            }
            Instruction::Phi { .. } => {
                // Phi 应该已经被消除
                log::warn!("Phi 指令出现在 JIT 编译阶段，这表明 SSA 降级不完整");
                Ok(())
            }
            _ => Err(format!("不支持的指令类型: {:?}", instruction).into()),
        }
    }
}

// ============================================================================
// 指令编译方法
// ============================================================================
impl X86Compiler {
    fn compile_move(
        &self,
        dst: &Register,
        src: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let dst_reg = self.get_physical_register(dst)?;

        match src {
            Operand::Register { id } => {
                let src_reg = self.get_physical_register(id)?;
                if dst_reg != src_reg {
                    self.emit_mov_reg_reg(code_builder, dst_reg, src_reg);
                }
            }
            Operand::Immediate { value } => {
                self.emit_mov_reg_imm64(code_builder, dst_reg, *value);
            }
            Operand::Label { id } => {
                // mov reg, label - 使用 lea 加载标签地址
                let label_name = format!("label_{}", id.0);
                code_builder.emit_label_address(&label_name);
                // 然后 mov reg, [rip + 0] 加载8字节地址
                self.emit_mov_reg_rip_rel(code_builder, dst_reg, 0);
            }
            _ => {
                return Err(format!("mov指令不支持的操作数类型: {:?}", src).into());
            }
        }
        Ok(())
    }

    fn compile_add(
        &self,
        dst: &Register,
        src1: &Operand,
        src2: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let dst_reg = self.get_physical_register(dst)?;

        match src1 {
            Operand::Register { id } => {
                let src1_reg = self.get_physical_register(id)?;
                if dst_reg != src1_reg {
                    self.emit_mov_reg_reg(code_builder, dst_reg, src1_reg);
                }
            }
            Operand::Immediate { value } => {
                self.emit_mov_reg_imm64(code_builder, dst_reg, *value);
            }
            _ => {
                return Err(format!("add指令不支持的src1类型: {:?}", src1).into());
            }
        }

        match src2 {
            Operand::Register { id } => {
                let src2_reg = self.get_physical_register(id)?;
                self.emit_add_reg_reg(code_builder, dst_reg, src2_reg);
            }
            Operand::Immediate { value } => {
                self.emit_add_reg_imm32(code_builder, dst_reg, *value as i32);
            }
            _ => {
                return Err(format!("add指令不支持的src2类型: {:?}", src2).into());
            }
        }
        Ok(())
    }

    fn compile_sub(
        &self,
        dst: &Register,
        src1: &Operand,
        src2: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let dst_reg = self.get_physical_register(dst)?;

        match src1 {
            Operand::Register { id } => {
                let src1_reg = self.get_physical_register(id)?;
                if dst_reg != src1_reg {
                    self.emit_mov_reg_reg(code_builder, dst_reg, src1_reg);
                }
            }
            Operand::Immediate { value } => {
                self.emit_mov_reg_imm64(code_builder, dst_reg, *value);
            }
            _ => {
                return Err(format!("sub指令不支持的src1类型: {:?}", src1).into());
            }
        }

        match src2 {
            Operand::Register { id } => {
                let src2_reg = self.get_physical_register(id)?;
                self.emit_sub_reg_reg(code_builder, dst_reg, src2_reg);
            }
            Operand::Immediate { value } => {
                self.emit_sub_reg_imm32(code_builder, dst_reg, *value as i32);
            }
            _ => {
                return Err(format!("sub指令不支持的src2类型: {:?}", src2).into());
            }
        }
        Ok(())
    }

    fn compile_mul(
        &self,
        dst: &Register,
        src1: &Operand,
        src2: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let dst_reg = self.get_physical_register(dst)?;

        match src1 {
            Operand::Register { id } => {
                let src1_reg = self.get_physical_register(id)?;
                if dst_reg != src1_reg {
                    self.emit_mov_reg_reg(code_builder, dst_reg, src1_reg);
                }
            }
            Operand::Immediate { value } => {
                self.emit_mov_reg_imm64(code_builder, dst_reg, *value);
            }
            _ => {
                return Err(format!("mul指令不支持的src1类型: {:?}", src1).into());
            }
        }

        match src2 {
            Operand::Register { id } => {
                let src2_reg = self.get_physical_register(id)?;
                self.emit_imul_reg_reg(code_builder, dst_reg, src2_reg);
            }
            Operand::Immediate { value } => {
                self.emit_imul_reg_imm32(code_builder, dst_reg, *value as i32);
            }
            _ => {
                return Err(format!("mul指令不支持的src2类型: {:?}", src2).into());
            }
        }
        Ok(())
    }

    fn compile_div(
        &self,
        dst: &Register,
        src1: &Operand,
        src2: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let dst_reg = self.get_physical_register(dst)?;
        let rax: u8 = 0; // RAX
        let rdx: u8 = 2; // RDX
        let rcx: u8 = 1; // RCX (安全临时寄存器)

        // 先处理 src2（除数），因为它可能在 RDX 中，CQO 会覆盖 RDX
        // 同时需要确保临时寄存器不与 src1 冲突
        let src1_reg = match src1 {
            Operand::Register { id } => self.get_physical_register(id)?,
            _ => 0xFF, // 立即数，不会与寄存器冲突
        };

        let div_src_reg: u8;
        match src2 {
            Operand::Register { id } => {
                let src2_reg = self.get_physical_register(id)?;
                if src2_reg == rdx {
                    // src2 在 RDX 中，CQO 会覆盖它
                    // 使用 RCX 作为临时寄存器（但如果 src1 在 RCX 中就不能用）
                    if src1_reg == rcx {
                        // src1 在 RCX，不能用 RCX 作临时，用 R8 代替
                        let r8: u8 = 8;
                        self.emit_mov_reg_reg(code_builder, r8, rdx);
                        div_src_reg = r8;
                    } else {
                        self.emit_mov_reg_reg(code_builder, rcx, rdx);
                        div_src_reg = rcx;
                    }
                } else {
                    div_src_reg = src2_reg;
                }
            }
            Operand::Immediate { value } => {
                // 立即数，加载到安全寄存器（不能是 RAX, RDX, src1 寄存器）
                let temp_reg = if src1_reg == rcx { 8 } else { rcx }; // R8 or RCX
                self.emit_mov_reg_imm64(code_builder, temp_reg, *value);
                div_src_reg = temp_reg;
            }
            _ => {
                return Err(format!("div指令不支持的src2类型: {:?}", src2).into());
            }
        }

        // 将 src1 加载到 RAX
        match src1 {
            Operand::Register { id } => {
                let src1_phys = self.get_physical_register(id)?;
                if rax != src1_phys {
                    self.emit_mov_reg_reg(code_builder, rax, src1_phys);
                }
            }
            Operand::Immediate { value } => {
                self.emit_mov_reg_imm64(code_builder, rax, *value);
            }
            _ => {
                return Err(format!("div指令不支持的src1类型: {:?}", src1).into());
            }
        }

        // CQO (将 RAX 符号扩展到 RDX:RAX)
        self.emit_rex_prefix(code_builder, true, 0, 0, 0);
        code_builder.emit_byte(0x99);

        // IDIV div_src_reg
        self.emit_idiv_reg(code_builder, div_src_reg);

        // 商在 RAX，移动到 dst
        if dst_reg != rax {
            self.emit_mov_reg_reg(code_builder, dst_reg, rax);
        }
        Ok(())
    }

    fn compile_compare(
        &self,
        src1: &Operand,
        src2: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        match (src1, src2) {
            (Operand::Register { id: id1 }, Operand::Register { id: id2 }) => {
                let reg1 = self.get_physical_register(id1)?;
                let reg2 = self.get_physical_register(id2)?;
                self.emit_cmp_reg_reg(code_builder, reg1, reg2);
            }
            (Operand::Register { id }, Operand::Immediate { value }) => {
                let reg = self.get_physical_register(id)?;
                self.emit_cmp_reg_imm32(code_builder, reg, *value as i32);
            }
            _ => {
                return Err(format!(
                    "compare指令不支持的操作数组合: {:?}, {:?}",
                    src1, src2
                ).into());
            }
        }
        Ok(())
    }

    /// 编译 CompareSet 指令：cmp src1, src2; setcc dst_byte; movzbq dst, dst_byte
    /// 直接从比较条件产生 0/1 值到 dst 寄存器，不产生分支。
    fn compile_compare_set(
        &self,
        dst: &Register,
        condition: &ComparisonCondition,
        src1: &Operand,
        src2: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let dst_reg = self.get_physical_register(dst)?;

        // ⚠️ 先 XOR 清零 dst（必须在 CMP 之前，否则 XOR 会破坏 CMP 的 flags）
        if dst_reg >= 8 {
            code_builder.emit_byte(0x49); // REX.W + REX.B
        } else {
            code_builder.emit_byte(0x48); // REX.W
        }
        code_builder.emit_byte(0x31); // XOR r/m, reg
        code_builder.emit_byte(0xC0 | ((dst_reg & 0x07) << 3) | (dst_reg & 0x07));

        // CMP 设置 flags
        match (src1, src2) {
            (Operand::Register { id: id1 }, Operand::Register { id: id2 }) => {
                let reg1 = self.get_physical_register(id1)?;
                let reg2 = self.get_physical_register(id2)?;
                self.emit_cmp_reg_reg(code_builder, reg1, reg2);
            }
            (Operand::Register { id }, Operand::Immediate { value }) => {
                let reg = self.get_physical_register(id)?;
                self.emit_cmp_reg_imm32(code_builder, reg, *value as i32);
            }
            _ => {
                return Err(format!(
                    "setcc指令不支持的操作数组合: {:?}, {:?}",
                    src1, src2
                ).into());
            }
        }

        // SETcc dst_byte: 根据条件设置低位字节为 0 或 1
        let opcode2: u8 = match condition {
            ComparisonCondition::Equal => 0x94,        // SETE
            ComparisonCondition::NotEqual => 0x95,     // SETNE
            ComparisonCondition::LessThan => 0x9C,     // SETL
            ComparisonCondition::LessEqual => 0x9E,    // SETLE
            ComparisonCondition::GreaterThan => 0x9F,  // SETG
            ComparisonCondition::GreaterEqual => 0x9D, // SETGE
        };
        // x86-64 字节寄存器编码陷阱：
        // 无 REX 前缀时，r/m 字段 4-7 映射到 AH/CH/DH/BH（高字节）
        // 加 REX 前缀后，r/m 字段 4-7 映射到 SPL/BPL/SIL/DIL（低字节）
        // SETcc 操作数是字节寄存器，必须确保使用正确的低字节
        if dst_reg >= 8 {
            code_builder.emit_byte(0x41); // REX.B（扩展 R8-R15）
        } else if dst_reg >= 4 {
            code_builder.emit_byte(0x40); // REX（无扩展位，仅启用 SPL/BPL/SIL/DIL）
        }
        code_builder.emit_byte(0x0F);
        code_builder.emit_byte(opcode2);
        code_builder.emit_byte(0xC0 | (dst_reg & 0x07)); // ModRM: mod=11, reg=0, r/m=dst

        Ok(())
    }

    fn compile_jump(
        &self,
        target: &karte_lir::LabelId,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let label_name = format!("label_{}", target.0);
        code_builder.emit_jump(JumpType::Unconditional, &label_name);
        Ok(())
    }

    fn compile_conditional_jump(
        &self,
        jump_type: JumpType,
        target: &karte_lir::LabelId,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let label_name = format!("label_{}", target.0);
        code_builder.emit_jump(jump_type, &label_name);
        Ok(())
    }

    fn compile_call(
        &self,
        target: &karte_lir::LabelId,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let label_name = format!("label_{}", target.0);
        code_builder.emit_jump(JumpType::Call, &label_name);
        Ok(())
    }

    /// 编译间接跳转（无链接）：jmp reg
    fn compile_jump_indirect(
        &self,
        function_register: &Register,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let reg = self.get_physical_register(function_register)?;
        self.emit_jmp_reg(code_builder, reg);
        Ok(())
    }

    /// 编译寄存器跳转
    fn compile_jump_register(
        &self,
        target_register: &Register,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let reg = self.get_physical_register(target_register)?;
        self.emit_jmp_reg(code_builder, reg);
        Ok(())
    }

    fn compile_return(
        &self,
        value: Option<&Register>,
        code_builder: &mut CodeBuilder,
        is_main_function: bool,
    ) -> crate::Result<()> {
        // 将返回值移动到 RAX
        if let Some(reg) = value {
            let src_reg = self.get_physical_register(reg)?;
            if src_reg != 0 {
                // 0 = RAX
                self.emit_mov_reg_reg(code_builder, 0, src_reg);
            }
        } else {
            self.emit_mov_reg_imm64(code_builder, 0, 0);
        }

        if is_main_function {
            // Main函数尾声
            self.emit_main_function_epilogue(code_builder)?;
        } else {
            // 内部函数尾声：恢复 callee-saved，加载返回地址并跳转
            // 注意：与 AArch64 保持一致，callee 不弹出返回地址（caller 负责弹出）
            self.emit_internal_function_epilogue(code_builder)?;

            // 此时 vm_sp 指向返回地址所在的栈位置
            // 使用 RCX 作为临时寄存器加载返回地址（不能和 vm_sp 一样用 R10）
            let vm_sp: u8 = 10; // R10 = 虚拟栈指针
            let tmp_reg: u8 = 1;  // RCX = 临时寄存器
            // 先保存返回值（RAX），因为 RCX 可能被用作参数
            // 但返回值已经在 RAX 中了，RCX 可以安全使用
            self.emit_mov_reg_mem(code_builder, tmp_reg, vm_sp, 0);
            // 不弹出返回地址！caller 的 lower_call 会负责 add vm_sp, 16
            // 跳转到返回地址
            self.emit_jmp_reg(code_builder, tmp_reg);
        }

        Ok(())
    }

    /// 编译 LoadGlobal 指令 - 从 runtime 全局数据区加载值
    /// 生成: movabs rax, <global_addr>  (占位，AOT 修补)
    ///       mov dst, rax
    ///       mov dst, [dst]              (从地址加载值)
    fn compile_load_global(
        &self,
        dst: &Register,
        name: &str,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let dst_reg = self.get_physical_register(dst)?;
        
        // 生成占位 movabs rax, <global_addr>
        let global_label = format!("__global_{}", name);
        code_builder.emit_movabs_to_rax_with_label(&global_label);
        // mov dst, rax (dst = 全局变量的地址)
        self.emit_mov_reg_reg(code_builder, dst_reg, 0);
        // mov dst, [dst] (从地址加载值)
        self.emit_mov_reg_mem(code_builder, dst_reg, dst_reg, 0);
        
        Ok(())
    }

    fn compile_load64(
        &self,
        dst: &Register,
        addr: &Register,
        offset: i64,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let dst_reg = self.get_physical_register(dst)?;
        let addr_reg = self.get_physical_register(addr)?;
        self.emit_mov_reg_mem(code_builder, dst_reg, addr_reg, offset as i32);
        Ok(())
    }

    fn compile_store64(
        &self,
        addr: &Register,
        offset: i64,
        src: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let addr_reg = self.get_physical_register(addr)?;

        match src {
            Operand::Register { id } => {
                let src_reg = self.get_physical_register(id)?;
                self.emit_mov_mem_reg(code_builder, addr_reg, offset as i32, src_reg);
            }
            Operand::Immediate { value } => {
                self.emit_mov_mem_imm32(code_builder, addr_reg, offset as i32, *value as i32);
            }
            Operand::Label { id } => {
                let label_name = format!("label_{}", id.0);
                code_builder.emit_movabs_to_rax_with_label(&label_name);
                self.emit_mov_mem_reg(code_builder, addr_reg, offset as i32, 0); // rax = 0
            }
            _ => {
                return Err(format!("store64指令不支持的src类型: {:?}", src).into());
            }
        }
        Ok(())
    }

    // === 位运算编译函数 ===

    fn compile_bitand(
        &self,
        dst: &Register,
        src1: &Operand,
        src2: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let dst_reg = self.get_physical_register(dst)?;
        // 编译期常量折叠：两个都是 immediate 时直接算结果
        if let (Operand::Immediate { value: v1 }, Operand::Immediate { value: v2 }) = (src1, src2)
        {
            self.emit_mov_reg_imm64(code_builder, dst_reg, v1 & v2);
            return Ok(());
        }
        match src1 {
            Operand::Register { id } => {
                let src1_reg = self.get_physical_register(id)?;
                if dst_reg != src1_reg {
                    self.emit_mov_reg_reg(code_builder, dst_reg, src1_reg);
                }
            }
            Operand::Immediate { value } => {
                self.emit_mov_reg_imm64(code_builder, dst_reg, *value);
            }
            _ => return Err(format!("bitand不支持的src1: {:?}", src1).into()),
        }
        match src2 {
            Operand::Register { id } => {
                let src2_reg = self.get_physical_register(id)?;
                // AND r64, r64: REX 0x21 ModRM — 使用 emit_rex_prefix 处理扩展寄存器
                self.emit_rex_prefix(code_builder, true, src2_reg, 0, dst_reg);
                code_builder.emit_byte(0x21);
                self.emit_modrm(code_builder, 0b11, src2_reg, dst_reg);
            }
            Operand::Immediate { value } => {
                // AND r64, imm32: REX 0x81 ModRM imm32
                self.emit_rex_prefix(code_builder, true, 0, 0, dst_reg);
                code_builder.emit_byte(0x81);
                self.emit_modrm(code_builder, 0b11, 4, dst_reg); // /4 = AND
                code_builder.emit_i32(*value as i32);
            }
            _ => return Err(format!("bitand不支持的src2: {:?}", src2).into()),
        }
        Ok(())
    }

    fn compile_bitor(
        &self,
        dst: &Register,
        src1: &Operand,
        src2: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let dst_reg = self.get_physical_register(dst)?;
        if let (Operand::Immediate { value: v1 }, Operand::Immediate { value: v2 }) = (src1, src2)
        {
            self.emit_mov_reg_imm64(code_builder, dst_reg, v1 | v2);
            return Ok(());
        }
        match src1 {
            Operand::Register { id } => {
                let src1_reg = self.get_physical_register(id)?;
                if dst_reg != src1_reg {
                    self.emit_mov_reg_reg(code_builder, dst_reg, src1_reg);
                }
            }
            Operand::Immediate { value } => {
                self.emit_mov_reg_imm64(code_builder, dst_reg, *value);
            }
            _ => return Err(format!("bitor不支持的src1: {:?}", src1).into()),
        }
        match src2 {
            Operand::Register { id } => {
                let src2_reg = self.get_physical_register(id)?;
                // OR r64, r64: REX 0x09 ModRM — 使用 emit_rex_prefix 处理扩展寄存器
                self.emit_rex_prefix(code_builder, true, src2_reg, 0, dst_reg);
                code_builder.emit_byte(0x09);
                self.emit_modrm(code_builder, 0b11, src2_reg, dst_reg);
            }
            Operand::Immediate { value } => {
                // OR r64, imm32: REX 0x81 ModRM imm32
                self.emit_rex_prefix(code_builder, true, 0, 0, dst_reg);
                code_builder.emit_byte(0x81);
                self.emit_modrm(code_builder, 0b11, 1, dst_reg); // /1 = OR
                code_builder.emit_i32(*value as i32);
            }
            _ => return Err(format!("bitor不支持的src2: {:?}", src2).into()),
        }
        Ok(())
    }

    fn compile_bitxor(
        &self,
        dst: &Register,
        src1: &Operand,
        src2: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let dst_reg = self.get_physical_register(dst)?;
        if let (Operand::Immediate { value: v1 }, Operand::Immediate { value: v2 }) = (src1, src2)
        {
            self.emit_mov_reg_imm64(code_builder, dst_reg, v1 ^ v2);
            return Ok(());
        }
        match src1 {
            Operand::Register { id } => {
                let src1_reg = self.get_physical_register(id)?;
                if dst_reg != src1_reg {
                    self.emit_mov_reg_reg(code_builder, dst_reg, src1_reg);
                }
            }
            Operand::Immediate { value } => {
                self.emit_mov_reg_imm64(code_builder, dst_reg, *value);
            }
            _ => return Err(format!("bitxor不支持的src1: {:?}", src1).into()),
        }
        match src2 {
            Operand::Register { id } => {
                let src2_reg = self.get_physical_register(id)?;
                // XOR r64, r64: REX 0x31 ModRM — 使用 emit_rex_prefix 处理扩展寄存器
                self.emit_rex_prefix(code_builder, true, src2_reg, 0, dst_reg);
                code_builder.emit_byte(0x31);
                self.emit_modrm(code_builder, 0b11, src2_reg, dst_reg);
            }
            Operand::Immediate { value } => {
                // XOR r64, imm32: REX 0x81 ModRM imm32
                self.emit_rex_prefix(code_builder, true, 0, 0, dst_reg);
                code_builder.emit_byte(0x81);
                self.emit_modrm(code_builder, 0b11, 6, dst_reg); // /6 = XOR
                code_builder.emit_i32(*value as i32);
            }
            _ => return Err(format!("bitxor不支持的src2: {:?}", src2).into()),
        }
        Ok(())
    }

    /// 编码 ModRM byte，处理扩展寄存器 (r8-r15) 的 REX.B 前缀
    /// base_modrm: 基础 ModRM byte（不含 rm 字段的低位）
    /// reg: 寄存器编号 (0-15)
    /// 返回 (rex_byte, modrm_byte)，rex_byte 为 0 表示不需要额外 REX 前缀
    fn encode_modrm_reg_extension(&self, base_modrm: u8, reg: u8) -> (u8, u8) {
        let modrm = base_modrm | (reg & 0x07);
        if reg >= 8 {
            // 需要 REX.B=1。由于外层已经有 0x48 (REX.W)，合并为 0x49 (REX.WB)
            // 返回 0x49 让调用者替换 0x48 为 0x49
            (0x49, modrm)
        } else {
            (0, modrm)
        }
    }

    fn compile_shift_left(
        &self,
        dst: &Register,
        src1: &Operand,
        src2: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let dst_reg = self.get_physical_register(dst)?;
        if let (Operand::Immediate { value: v1 }, Operand::Immediate { value: v2 }) = (src1, src2)
        {
            self.emit_mov_reg_imm64(code_builder, dst_reg, v1 << (v2 & 63));
            return Ok(());
        }
        match src1 {
            Operand::Register { id } => {
                let src1_reg = self.get_physical_register(id)?;
                if dst_reg != src1_reg {
                    self.emit_mov_reg_reg(code_builder, dst_reg, src1_reg);
                }
            }
            Operand::Immediate { value } => {
                self.emit_mov_reg_imm64(code_builder, dst_reg, *value);
            }
            _ => return Err(format!("shl不支持的src1: {:?}", src1).into()),
        }
        match src2 {
            Operand::Register { id } if *id == Register::Physical(1) || *id == Register::Physical(0) => {
                let src2_reg = self.get_physical_register(id)?;
                let (rex, modrm) = self.encode_modrm_reg_extension(0xE0, dst_reg);
                code_builder.emit_bytes(&[if rex != 0 { rex } else { 0x48 }, 0xD3, modrm]);
            }
            Operand::Immediate { value } => {
                let (rex, modrm) = self.encode_modrm_reg_extension(0xE0, dst_reg);
                code_builder.emit_bytes(&[if rex != 0 { rex } else { 0x48 }, 0xC1, modrm]);
                code_builder.emit_bytes(&[(*value as u8) & 0x3F]);
            }
            Operand::Register { id } => {
                let src2_reg = self.get_physical_register(id)?;
                if src2_reg != 1 {
                    self.emit_mov_reg_reg(code_builder, 1, src2_reg);
                }
                let (rex, modrm) = self.encode_modrm_reg_extension(0xE0, dst_reg);
                code_builder.emit_bytes(&[if rex != 0 { rex } else { 0x48 }, 0xD3, modrm]);
            }
            _ => return Err(format!("shl不支持的src2: {:?}", src2).into()),
        }
        Ok(())
    }

    fn compile_shift_right(
        &self,
        dst: &Register,
        src1: &Operand,
        src2: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let dst_reg = self.get_physical_register(dst)?;
        if let (Operand::Immediate { value: v1 }, Operand::Immediate { value: v2 }) = (src1, src2)
        {
            // 无符号右移（逻辑右移）
            self.emit_mov_reg_imm64(code_builder, dst_reg, ((*v1 as u64) >> (*v2 as u64 & 63)) as i64);
            return Ok(());
        }
        match src1 {
            Operand::Register { id } => {
                let src1_reg = self.get_physical_register(id)?;
                if dst_reg != src1_reg {
                    self.emit_mov_reg_reg(code_builder, dst_reg, src1_reg);
                }
            }
            Operand::Immediate { value } => {
                self.emit_mov_reg_imm64(code_builder, dst_reg, *value);
            }
            _ => return Err(format!("shr不支持的src1: {:?}", src1).into()),
        }
        match src2 {
            Operand::Immediate { value } => {
                let (rex, modrm) = self.encode_modrm_reg_extension(0xE8, dst_reg);
                code_builder.emit_bytes(&[if rex != 0 { rex } else { 0x48 }, 0xC1, modrm]);
                code_builder.emit_bytes(&[(*value as u8) & 0x3F]);
            }
            Operand::Register { id } => {
                let src2_reg = self.get_physical_register(id)?;
                if src2_reg != 1 {
                    self.emit_mov_reg_reg(code_builder, 1, src2_reg);
                }
                let (rex, modrm) = self.encode_modrm_reg_extension(0xE8, dst_reg);
                code_builder.emit_bytes(&[if rex != 0 { rex } else { 0x48 }, 0xD3, modrm]);
            }
            _ => return Err(format!("shr不支持的src2: {:?}", src2).into()),
        }
        Ok(())
    }

    fn compile_bitnot(
        &self,
        dst: &Register,
        src: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let dst_reg = self.get_physical_register(dst)?;
        if let Operand::Immediate { value } = src {
            self.emit_mov_reg_imm64(code_builder, dst_reg, !value);
            return Ok(());
        }
        match src {
            Operand::Register { id } => {
                let src_reg = self.get_physical_register(id)?;
                if dst_reg != src_reg {
                    self.emit_mov_reg_reg(code_builder, dst_reg, src_reg);
                }
            }
            Operand::Immediate { value } => {
                self.emit_mov_reg_imm64(code_builder, dst_reg, *value);
            }
            _ => return Err(format!("bitnot不支持的src: {:?}", src).into()),
        }
        // NOT r64: 0x48 0xF7 ModRM(0xD0 + reg)
        code_builder.emit_bytes(&[0x48, 0xF7, 0xD0 | dst_reg]);
        Ok(())
    }

    // === Load/Store 变体 (32-bit, 8-bit) ===

    fn compile_load32(
        &self,
        dst: &Register,
        addr: &Register,
        offset: i64,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let dst_reg = self.get_physical_register(dst)?;
        let addr_reg = self.get_physical_register(addr)?;
        // MOV r32, [addr + offset]: 使用 32-bit 操作数（自动零扩展到 64 位）
        // 0x8B ModRM(disp32) or REX prefix for extended regs
        let offset_bytes = (offset as i32).to_le_bytes();
        if dst_reg < 8 && addr_reg < 8 {
            code_builder.emit_bytes(&[
                0x8B,
                0x80 | (dst_reg << 3) | addr_reg,
            ]);
        } else {
            // REX prefix needed
            let rex = 0x48
                | if dst_reg >= 8 { 0x04 } else { 0 }
                | if addr_reg >= 8 { 0x01 } else { 0 };
            code_builder.emit_bytes(&[
                rex,
                0x8B,
                0x80 | ((dst_reg & 7) << 3) | (addr_reg & 7),
            ]);
        }
        code_builder.emit_bytes(&offset_bytes);
        Ok(())
    }

    fn compile_store32(
        &self,
        addr: &Register,
        offset: i64,
        src: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let addr_reg = self.get_physical_register(addr)?;
        match src {
            Operand::Register { id } => {
                let src_reg = self.get_physical_register(id)?;
                // MOV [addr + disp32], r32
                if src_reg < 8 && addr_reg < 8 {
                    code_builder.emit_bytes(&[
                        0x89,
                        0x80 | (src_reg << 3) | addr_reg,
                    ]);
                } else {
                    let rex = 0x48
                        | if src_reg >= 8 { 0x04 } else { 0 }
                        | if addr_reg >= 8 { 0x01 } else { 0 };
                    code_builder.emit_bytes(&[
                        rex,
                        0x89,
                        0x80 | ((src_reg & 7) << 3) | (addr_reg & 7),
                    ]);
                }
                code_builder.emit_i32(offset as i32);
            }
            Operand::Immediate { value } => {
                // MOV [addr + disp32], imm32
                if addr_reg < 8 {
                    code_builder.emit_bytes(&[0xC7, 0x80 | addr_reg]);
                } else {
                    code_builder.emit_bytes(&[0x41, 0xC7, 0x80 | (addr_reg & 7)]);
                }
                code_builder.emit_i32(offset as i32);
                code_builder.emit_i32(*value as i32);
            }
            _ => return Err(format!("store32不支持的src: {:?}", src).into()),
        }
        Ok(())
    }

    fn compile_load8(
        &self,
        dst: &Register,
        addr: &Register,
        offset: i64,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let dst_reg = self.get_physical_register(dst)?;
        let addr_reg = self.get_physical_register(addr)?;
        // MOVZX r64, byte [addr + disp32]: 0x0F 0xB6 ModRM
        let offset_bytes = (offset as i32).to_le_bytes();
        if dst_reg < 8 && addr_reg < 8 {
            code_builder.emit_bytes(&[
                0x0F, 0xB6,
                0x80 | (dst_reg << 3) | addr_reg,
            ]);
        } else {
            let rex = 0x48
                | if dst_reg >= 8 { 0x04 } else { 0 }
                | if addr_reg >= 8 { 0x01 } else { 0 };
            code_builder.emit_bytes(&[
                rex, 0x0F, 0xB6,
                0x80 | ((dst_reg & 7) << 3) | (addr_reg & 7),
            ]);
        }
        code_builder.emit_bytes(&offset_bytes);
        Ok(())
    }

    fn compile_store8(
        &self,
        addr: &Register,
        offset: i64,
        src: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let addr_reg = self.get_physical_register(addr)?;
        match src {
            Operand::Register { id } => {
                let src_reg = self.get_physical_register(id)?;
                // MOV byte [addr + disp32], r8
                // x86_64: SPL/BPL/SIL/DIL (reg 4-7) 需要 REX prefix
                // 没有 REX 时 reg 4-7 是 AH/CH/DH/BH，有 REX 时是 SPL/BPL/SIL/DIL
                let need_rex = src_reg >= 4 || addr_reg >= 8 || src_reg >= 8;
                if need_rex {
                    let rex = 0x40  // REX base (不需要 REX.W，byte 操作)
                        | if src_reg >= 8 { 0x04 } else { 0 }
                        | if addr_reg >= 8 { 0x01 } else { 0 };
                    code_builder.emit_bytes(&[
                        rex,
                        0x88,
                        0x80 | ((src_reg & 7) << 3) | (addr_reg & 7),
                    ]);
                } else {
                    code_builder.emit_bytes(&[
                        0x88,
                        0x80 | (src_reg << 3) | addr_reg,
                    ]);
                }
                code_builder.emit_i32(offset as i32);
            }
            Operand::Immediate { value } => {
                // MOV byte [addr + disp32], imm8
                if addr_reg < 8 {
                    code_builder.emit_bytes(&[0xC6, 0x80 | addr_reg]);
                } else {
                    code_builder.emit_bytes(&[0x41, 0xC6, 0x80 | (addr_reg & 7)]);
                }
                code_builder.emit_i32(offset as i32);
                code_builder.emit_bytes(&[(*value as u8) & 0xFF]);
            }
            _ => return Err(format!("store8不支持的src: {:?}", src).into()),
        }
        Ok(())
    }

    /// StorePair 拆分为两条 Store64
    fn compile_store_pair(
        &self,
        addr: &Register,
        offset: i64,
        src1: &Register,
        src2: &Register,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        // store64 [addr + offset], src1
        self.compile_store64(
            addr,
            offset,
            &Operand::Register { id: *src1 },
            code_builder,
        )?;
        // store64 [addr + offset + 8], src2
        self.compile_store64(
            addr,
            offset + 8,
            &Operand::Register { id: *src2 },
            code_builder,
        )
    }

    /// LoadPair 拆分为两条 Load64
    fn compile_load_pair(
        &self,
        dst1: &Register,
        dst2: &Register,
        addr: &Register,
        offset: i64,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        // load64 dst1, [addr + offset]
        self.compile_load64(dst1, addr, offset, code_builder)?;
        // load64 dst2, [addr + offset + 8]
        self.compile_load64(dst2, addr, offset + 8, code_builder)
    }

    fn compile_alloc(
        &self,
        dst: &Register,
        size: usize,
        alignment: usize,
        allocation_type: &karte_lir::AllocationType,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        match allocation_type {
            karte_lir::AllocationType::Heap => {
                let call = RuntimeCall::alloc(size, alignment);
                self.emit_runtime_call(code_builder, call, Some(dst))
            }
            _ => Err(format!(
                "Alloc instruction with unsupported allocation type: {:?}",
                allocation_type
            ).into()),
        }
    }

    fn compile_free(
        &self,
        addr: &Register,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let call = RuntimeCall::free(*addr);
        self.emit_runtime_call(code_builder, call, None)
    }

    fn compile_retain(
        &self,
        value: &Register,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let call = RuntimeCall::retain(*value);
        self.emit_runtime_call(code_builder, call, None)
    }

    fn compile_release(
        &self,
        value: &Register,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let call = RuntimeCall::release(*value);
        self.emit_runtime_call(code_builder, call, None)
    }

    fn compile_safepoint(&self, code_builder: &mut CodeBuilder) -> crate::Result<()> {
        let call = RuntimeCall::gc_safepoint();
        self.emit_runtime_call(code_builder, call, None)
    }

    fn emit_runtime_call(
        &self,
        code_builder: &mut CodeBuilder,
        call: RuntimeCall,
        result: Option<&Register>,
    ) -> crate::Result<()> {
        let return_reg: u8 = 0; // RAX
        let exclude: Vec<u8> = if result.is_some() && call.expects_result() {
            vec![return_reg]
        } else {
            Vec::new()
        };
        let (saved_regs, stack_space) =
            self.save_call_clobbered_registers(code_builder, &exclude);

        // System V ABI 参数寄存器: RDI, RSI, RDX, RCX, R8, R9
        let arg_regs = [7u8, 6, 2, 1, 8, 9]; // RDI=7, RSI=6, RDX=2, RCX=1, R8=8, R9=9

        for (idx, arg) in call.args.iter().enumerate() {
            if idx >= arg_regs.len() {
                return Err(format!(
                    "runtime call {} 超过支持的参数数量(最多 {})",
                    call.intrinsic.name(),
                    arg_regs.len()
                ).into());
            }
            let target_reg = arg_regs[idx];
            match arg {
                RuntimeArg::Immediate(value) => {
                    self.emit_mov_reg_imm64(code_builder, target_reg, *value);
                }
                RuntimeArg::Register(reg) => {
                    let src_reg = self.get_physical_register(reg)?;
                    if src_reg != target_reg {
                        self.emit_mov_reg_reg(code_builder, target_reg, src_reg);
                    }
                }
            }
        }

        self.emit_call_absolute(code_builder, call.intrinsic.symbol_ptr() as u64);
        self.restore_call_clobbered_registers(code_builder, &saved_regs, stack_space);

        if let (Some(dst), true) = (result, call.expects_result()) {
            let dst_reg = self.get_physical_register(dst)?;
            if dst_reg != return_reg {
                self.emit_mov_reg_reg(code_builder, dst_reg, return_reg);
            }
        }

        Ok(())
    }
}

// ============================================================================
// 函数序言和尾声
// ============================================================================
impl X86Compiler {
    /// 判断是否为入口函数
    fn is_entry_function(&self, name: &str, program: &LirProgram) -> bool {
        program.main_function.as_deref() == Some(name)
    }

    /// 获取当前函数使用的 callee-saved 寄存器（使用映射后的编号）
    fn get_callee_saved_registers(&self) -> Vec<u8> {
        let cc = CallingConvention::standard();
        self.current_function_used_regs
            .iter()
            .filter(|&&reg| {
                cc.is_callee_saved(reg)
                && reg != cc.stack_pointer  // RSP 特殊处理
                && reg != cc.frame_pointer  // RBP 特殊处理
            })
            .map(|&reg| self.map_register(reg))
            .collect()
    }

    /// 生成主函数序言
    fn emit_main_function_prologue(&self, code_builder: &mut CodeBuilder) -> crate::Result<()> {
        // x86-64 System V ABI 入口:
        // 参数通过 RDI(7), RSI(6) 传递
        // RDI = 虚拟栈顶地址, RSI = 虚拟栈底地址
        //
        // 虚拟栈架构（与 AArch64 设计相同）：
        // 1. 保存 callee-saved 到系统栈
        // 2. 将虚拟栈参数移动到虚拟栈指针寄存器
        // 3. 在虚拟栈上保存系统 RSP
        // 4. 为返回值槽分配空间

        let vm_sp: u8 = 10; // R10 = 虚拟栈指针 (Physical(10))
        let vm_fp: u8 = 11; // R11 = 虚拟帧指针 (Physical(11))

        // 保存 callee-saved 寄存器到系统栈
        // RBX(3), RBP(5), R12(12), R13(13), R14(14), R15(15)
        // 注意：必须保存 RBP 因为 System V ABI 要求
        //
        // 栈对齐计算：
        //   入口时 RSP % 16 == 8（caller 的 CALL 压入 8 字节返回地址）
        //   push rbx → RSP -= 8 → RSP % 16 == 0
        //   push rbp → RSP -= 8 → RSP % 16 == 8
        //   sub rsp, N → 需要 N % 16 == 8 才能使最终 RSP % 16 == 0
        //   所以 sub rsp, 40（总调整量 = 8+8+40 = 56，56%16 == 8 ✓）
        // push rbx
        code_builder.emit_byte(0x53);
        // push rbp
        code_builder.emit_byte(0x55);
        // 使用 sub rsp 预留空间保存 R12-R15（32 字节数据 + 8 字节对齐填充）
        self.emit_sub_reg_imm32(code_builder, 4, 40); // RSP -= 40
        // mov [rsp+0], r12
        self.emit_mov_mem_reg(code_builder, 4, 0, 12);
        // mov [rsp+8], r13
        self.emit_mov_mem_reg(code_builder, 4, 8, 13);
        // mov [rsp+16], r14
        self.emit_mov_mem_reg(code_builder, 4, 16, 14);
        // mov [rsp+24], r15
        self.emit_mov_mem_reg(code_builder, 4, 24, 15);

        // 将虚拟栈参数移动到虚拟栈寄存器
        // RDI(7) → R10(虚拟SP), RSI(6) → R11(虚拟FP)
        self.emit_mov_reg_reg(code_builder, vm_sp, 7); // R10 = RDI (虚拟栈顶)
        self.emit_mov_reg_reg(code_builder, vm_fp, 6); // R11 = RSI (虚拟栈底)

        // 在虚拟栈上保存系统 RSP
        // sub vm_sp, 16 (虚拟栈分配16字节)
        self.emit_sub_reg_imm32(code_builder, vm_sp, 16);
        // mov [vm_sp+0], rsp (保存系统SP)
        // 注意：这里需要用当前的硬件RSP值
        // lea rax, [rsp + 0] → mov [vm_sp], rax
        self.emit_mov_reg_reg(code_builder, 0, 4); // RAX = RSP
        self.emit_mov_mem_reg(code_builder, vm_sp, 0, 0); // [vm_sp+0] = RSP
        // mov [vm_sp+8], 0 (保留，16字节对齐)
        self.emit_mov_mem_imm32(code_builder, vm_sp, 8, 0);

        // 为返回值槽分配空间
        // sub vm_sp, 16
        self.emit_sub_reg_imm32(code_builder, vm_sp, 16);
        // mov [vm_sp+0], rax (返回值槽指针，暂时用0占位)
        self.emit_mov_mem_imm32(code_builder, vm_sp, 0, 0);
        self.emit_mov_mem_imm32(code_builder, vm_sp, 8, 0);

        // 设置帧指针：vm_fp = vm_sp + frame_size
        // StackFrameLayoutPass 使用 FP + 负偏移量访问栈槽
        // 分配栈帧空间后设置 FP，使得 FP - 8, FP - 16 等位于已分配区域
        if self.current_stack_frame_size > 0 {
            self.emit_sub_reg_imm32(code_builder, vm_sp, self.current_stack_frame_size as i32);
            self.emit_mov_reg_reg(code_builder, vm_fp, vm_sp);
            self.emit_add_reg_imm32(code_builder, vm_fp, self.current_stack_frame_size as i32);
        } else {
            self.emit_mov_reg_reg(code_builder, vm_fp, vm_sp);
        }

        if self.debug_mode {
            log::debug!("x86_64: 主函数序言生成完成");
        }

        Ok(())
    }

    /// 生成内部函数序言（用于虚拟机内部函数调用）
    fn emit_internal_function_prologue(&self, code_builder: &mut CodeBuilder) -> crate::Result<()> {
        // 内部函数使用虚拟栈，保存 callee-saved 到虚拟栈
        let vm_sp: u8 = 10; // R10 = 虚拟栈指针
        let vm_fp: u8 = 11; // R11 = 虚拟帧指针
        let tmp: u8 = 0;    // RAX = 临时寄存器

        // 保存旧 vm_sp 值到临时寄存器
        self.emit_mov_reg_reg(code_builder, tmp, vm_sp);

        // 在虚拟栈上分配空间保存 FP 和 SP
        // sub vm_sp, 16
        self.emit_sub_reg_imm32(code_builder, vm_sp, 16);
        // mov [vm_sp + 8], vm_fp (保存帧指针)
        self.emit_mov_mem_reg(code_builder, vm_sp, 8, vm_fp);
        // mov [vm_sp + 0], tmp (保存旧的 SP)
        self.emit_mov_mem_reg(code_builder, vm_sp, 0, tmp);

        // 保存 callee-saved 寄存器（映射后）
        // 在 x86_64 上，callee-saved: RBX(3), R12(12), R13(13), R14(14), R15(15)
        // RSP(4) 和 RBP(5) 不需要保存（它们映射到 R10/R11，已经在上面保存了）
        let callee_saved = self.get_callee_saved_registers();
        for &reg in &callee_saved {
            self.emit_sub_reg_imm32(code_builder, vm_sp, 16);
            self.emit_mov_mem_reg(code_builder, vm_sp, 0, reg);
        }

        if self.debug_mode {
            log::debug!("x86_64: 内部函数序言保存了 {} 个 callee-saved 寄存器", callee_saved.len());
        }

        // 分配栈帧空间（用于 StackFrameLayoutPass 的 FP + 负偏移量访问）
        // StackFrameLayoutPass 生成 Add dst, FP, -offset 指令
        // 我们需要确保 FP - offset 位于虚拟栈的已分配区域，不会与函数调用的 push 冲突
        if self.current_stack_frame_size > 0 {
            self.emit_sub_reg_imm32(code_builder, vm_sp, self.current_stack_frame_size as i32);
        }

        // 设置帧指针：vm_fp = vm_sp + frame_size
        // 这样 FP - 8, FP - 16 等地址位于已分配的栈帧区域，不会被后续的 push 覆盖
        if self.current_stack_frame_size > 0 {
            self.emit_mov_reg_reg(code_builder, vm_fp, vm_sp);
            self.emit_add_reg_imm32(code_builder, vm_fp, self.current_stack_frame_size as i32);
        } else {
            self.emit_mov_reg_reg(code_builder, vm_fp, vm_sp);
        }

        Ok(())
    }

    /// 生成内部函数尾声
    fn emit_internal_function_epilogue(&self, code_builder: &mut CodeBuilder) -> crate::Result<()> {
        let vm_sp: u8 = 10; // R10 = 虚拟栈指针
        let tmp: u8 = 1;    // RCX = 临时寄存器

        // 内部函数的虚拟栈布局（从高地址到低地址）：
        //   [old_sp, old_fp]     ← prologue 保存
        //   [callee-saved]       ← prologue 保存
        //   ← vm_sp = vm_fp 在这里（prologue 后的位置）
        //   [帧空间]             ← LIR Sub/Add vm_sp, N 管理（LIR 自行恢复）
        //   ← vm_sp 当前位置（LIR 已恢复）
        //
        // LIR 指令中有配对的 Sub/Add vm_sp，Return 时 vm_sp 回到 prologue 后位置。
        // epilogue 直接恢复 callee-saved 和 old_sp/old_fp。

        // 恢复 callee-saved 寄存器（逆序）
        let callee_saved = self.get_callee_saved_registers();
        for &reg in callee_saved.iter().rev() {
            self.emit_mov_reg_mem(code_builder, reg, vm_sp, 0);
            self.emit_add_reg_imm32(code_builder, vm_sp, 16);
        }

        // 恢复 FP 和 SP
        self.emit_mov_reg_mem(code_builder, tmp, vm_sp, 8); // tmp = old_fp
        self.emit_mov_reg_mem(code_builder, vm_sp, vm_sp, 0); // vm_sp = old_sp
        self.emit_mov_reg_reg(code_builder, 11, tmp); // vm_fp(R11) = old_fp

        Ok(())
    }

    /// 生成主函数尾声（恢复系统栈并返回）
    fn emit_main_function_epilogue(&self, code_builder: &mut CodeBuilder) -> crate::Result<()> {
        let vm_sp: u8 = 10; // R10 = 虚拟栈指针

        // main 函数的虚拟栈布局（从高地址到低地址）：
        //   [系统RSP, 0]            ← prologue 保存，epilogue 目标位置
        //   [返回值槽, 16字节]      ← prologue 分配
        //   [栈帧空间, N字节]       ← LIR 的 Sub/Add vm_sp 管理（LIR 自行恢复）
        //   ← vm_sp 当前位置（LIR 已恢复帧空间）
        //
        // 注意：LIR 指令中有 Sub vm_sp, N 和对应的 Add vm_sp, N，
        // 所以到 Return 时 vm_sp 已经回到了返回值槽位置。
        // main epilogue 只需跳过返回值槽即可到达系统RSP保存位置。

        // 1. 弹出返回值槽
        self.emit_add_reg_imm32(code_builder, vm_sp, 16);

        // 2. 现在 vm_sp 指向保存系统 RSP 的位置
        self.emit_mov_mem_reg(code_builder, 4, 32, 0); // [rsp+32] = RAX (保存返回值)

        // 3. 从虚拟栈恢复系统 RSP
        self.emit_mov_reg_mem(code_builder, 1, vm_sp, 0); // RCX = [vm_sp+0] = 系统RSP
        self.emit_mov_reg_reg(code_builder, 4, 1); // RSP = RCX

        // 4. 恢复所有 callee-saved 寄存器（从系统栈）
        self.emit_mov_reg_mem(code_builder, 15, 4, 24); // R15 = [rsp+24]
        self.emit_mov_reg_mem(code_builder, 14, 4, 16); // R14 = [rsp+16]
        self.emit_mov_reg_mem(code_builder, 13, 4, 8);  // R13 = [rsp+8]
        self.emit_mov_reg_mem(code_builder, 12, 4, 0);  // R12 = [rsp+0]

        // 5. 从 padding 区域读回返回值
        self.emit_mov_reg_mem(code_builder, 0, 4, 32); // RAX = [rsp+32]

        // 6. 释放 sub rsp, 40 的空间
        self.emit_add_reg_imm32(code_builder, 4, 40);

        // 7. pop rbp
        code_builder.emit_byte(0x5D);

        // 8. pop rbx
        code_builder.emit_byte(0x5B);

        // 9. ret
        code_builder.emit_byte(0xC3);

        Ok(())
    }
}

// ============================================================================
// x86-64 指令编码
// ============================================================================
impl X86Compiler {
    fn emit_rex_prefix(&self, code_builder: &mut CodeBuilder, w: bool, r: u8, x: u8, b: u8) {
        let rex = 0x40
            | (if w { 0x08 } else { 0x00 })
            | ((r & 0x08) >> 1)
            | ((x & 0x08) >> 2)
            | ((b & 0x08) >> 3);
        code_builder.emit_byte(rex);
    }

    fn emit_modrm(&self, code_builder: &mut CodeBuilder, mode: u8, reg: u8, rm: u8) {
        let modrm = (mode << 6) | ((reg & 0x07) << 3) | (rm & 0x07);
        code_builder.emit_byte(modrm);
    }

    /// mov reg, reg (64位)
    fn emit_mov_reg_reg(&self, code_builder: &mut CodeBuilder, dst: u8, src: u8) {
        self.emit_rex_prefix(code_builder, true, src, 0, dst);
        code_builder.emit_byte(0x89);
        self.emit_modrm(code_builder, 0b11, src, dst);
    }

    /// mov reg, imm64
    fn emit_mov_reg_imm64(&self, code_builder: &mut CodeBuilder, dst: u8, imm: i64) {
        self.emit_rex_prefix(code_builder, true, 0, 0, dst);
        code_builder.emit_byte(0xB8 + (dst & 0x07));
        code_builder.emit_i64(imm);
    }

    /// mov reg, [reg + offset] (64位)
    fn emit_mov_reg_mem(&self, code_builder: &mut CodeBuilder, dst: u8, base: u8, offset: i32) {
        // 如果 base 是 RSP(4) 或 R12(12)，需要 SIB 字节
        let base_low = base & 0x07;
        let needs_sib = base_low == 4; // RSP/R12 的低3位是 100

        self.emit_rex_prefix(code_builder, true, dst, 0, base);
        code_builder.emit_byte(0x8B); // MOV r64, r/m64

        if needs_sib {
            if offset == 0 {
                self.emit_modrm(code_builder, 0b00, dst, 4); // r/m=100 means SIB follows
                code_builder.emit_byte(0x24); // SIB: scale=00, index=100(none), base=100(RSP)
            } else if (-128..=127).contains(&offset) {
                self.emit_modrm(code_builder, 0b01, dst, 4);
                code_builder.emit_byte(0x24); // SIB
                code_builder.emit_byte(offset as u8);
            } else {
                self.emit_modrm(code_builder, 0b10, dst, 4);
                code_builder.emit_byte(0x24); // SIB
                code_builder.emit_i32(offset);
            }
        } else if base_low == 5 {
            // RBP/R13: 即使 offset=0 也需要 mod=01 + disp8=0
            if offset == 0 {
                self.emit_modrm(code_builder, 0b01, dst, base);
                code_builder.emit_byte(0);
            } else if (-128..=127).contains(&offset) {
                self.emit_modrm(code_builder, 0b01, dst, base);
                code_builder.emit_byte(offset as u8);
            } else {
                self.emit_modrm(code_builder, 0b10, dst, base);
                code_builder.emit_i32(offset);
            }
        } else {
            if offset == 0 {
                self.emit_modrm(code_builder, 0b00, dst, base);
            } else if (-128..=127).contains(&offset) {
                self.emit_modrm(code_builder, 0b01, dst, base);
                code_builder.emit_byte(offset as u8);
            } else {
                self.emit_modrm(code_builder, 0b10, dst, base);
                code_builder.emit_i32(offset);
            }
        }
    }

    /// mov [reg + offset], reg (64位)
    fn emit_mov_mem_reg(&self, code_builder: &mut CodeBuilder, base: u8, offset: i32, src: u8) {
        let base_low = base & 0x07;
        let needs_sib = base_low == 4;

        self.emit_rex_prefix(code_builder, true, src, 0, base);
        code_builder.emit_byte(0x89); // MOV r/m64, r64

        if needs_sib {
            if offset == 0 {
                self.emit_modrm(code_builder, 0b00, src, 4);
                code_builder.emit_byte(0x24);
            } else if (-128..=127).contains(&offset) {
                self.emit_modrm(code_builder, 0b01, src, 4);
                code_builder.emit_byte(0x24);
                code_builder.emit_byte(offset as u8);
            } else {
                self.emit_modrm(code_builder, 0b10, src, 4);
                code_builder.emit_byte(0x24);
                code_builder.emit_i32(offset);
            }
        } else if base_low == 5 {
            if offset == 0 {
                self.emit_modrm(code_builder, 0b01, src, base);
                code_builder.emit_byte(0);
            } else if (-128..=127).contains(&offset) {
                self.emit_modrm(code_builder, 0b01, src, base);
                code_builder.emit_byte(offset as u8);
            } else {
                self.emit_modrm(code_builder, 0b10, src, base);
                code_builder.emit_i32(offset);
            }
        } else {
            if offset == 0 {
                self.emit_modrm(code_builder, 0b00, src, base);
            } else if (-128..=127).contains(&offset) {
                self.emit_modrm(code_builder, 0b01, src, base);
                code_builder.emit_byte(offset as u8);
            } else {
                self.emit_modrm(code_builder, 0b10, src, base);
                code_builder.emit_i32(offset);
            }
        }
    }

    /// mov [reg + offset], imm32
    fn emit_mov_mem_imm32(&self, code_builder: &mut CodeBuilder, base: u8, offset: i32, imm: i32) {
        let base_low = base & 0x07;
        let needs_sib = base_low == 4;

        self.emit_rex_prefix(code_builder, true, 0, 0, base);
        code_builder.emit_byte(0xC7);

        if needs_sib {
            if offset == 0 {
                self.emit_modrm(code_builder, 0b00, 0, 4);
                code_builder.emit_byte(0x24);
            } else if (-128..=127).contains(&offset) {
                self.emit_modrm(code_builder, 0b01, 0, 4);
                code_builder.emit_byte(0x24);
                code_builder.emit_byte(offset as u8);
            } else {
                self.emit_modrm(code_builder, 0b10, 0, 4);
                code_builder.emit_byte(0x24);
                code_builder.emit_i32(offset);
            }
        } else if base_low == 5 {
            if offset == 0 {
                self.emit_modrm(code_builder, 0b01, 0, base);
                code_builder.emit_byte(0);
            } else if (-128..=127).contains(&offset) {
                self.emit_modrm(code_builder, 0b01, 0, base);
                code_builder.emit_byte(offset as u8);
            } else {
                self.emit_modrm(code_builder, 0b10, 0, base);
                code_builder.emit_i32(offset);
            }
        } else {
            if offset == 0 {
                self.emit_modrm(code_builder, 0b00, 0, base);
            } else if (-128..=127).contains(&offset) {
                self.emit_modrm(code_builder, 0b01, 0, base);
                code_builder.emit_byte(offset as u8);
            } else {
                self.emit_modrm(code_builder, 0b10, 0, base);
                code_builder.emit_i32(offset);
            }
        }
        code_builder.emit_i32(imm);
    }

    /// mov reg, [rip + offset]
    fn emit_mov_reg_rip_rel(&self, code_builder: &mut CodeBuilder, dst: u8, offset: i32) {
        self.emit_rex_prefix(code_builder, true, dst, 0, 0);
        code_builder.emit_byte(0x8B);
        let modrm = ((dst & 0x07) << 3) | 0x05; // mod=00, reg=dst, rm=101 (RIP-relative)
        code_builder.emit_byte(modrm);
        code_builder.emit_i32(offset);
    }

    /// add reg, reg
    fn emit_add_reg_reg(&self, code_builder: &mut CodeBuilder, dst: u8, src: u8) {
        self.emit_rex_prefix(code_builder, true, src, 0, dst);
        code_builder.emit_byte(0x01);
        self.emit_modrm(code_builder, 0b11, src, dst);
    }

    /// add reg, imm32
    fn emit_add_reg_imm32(&self, code_builder: &mut CodeBuilder, dst: u8, imm: i32) {
        self.emit_rex_prefix(code_builder, true, 0, 0, dst);
        code_builder.emit_byte(0x81);
        self.emit_modrm(code_builder, 0b11, 0, dst);
        code_builder.emit_i32(imm);
    }

    /// sub reg, reg
    fn emit_sub_reg_reg(&self, code_builder: &mut CodeBuilder, dst: u8, src: u8) {
        self.emit_rex_prefix(code_builder, true, src, 0, dst);
        code_builder.emit_byte(0x29);
        self.emit_modrm(code_builder, 0b11, src, dst);
    }

    /// sub reg, imm32
    fn emit_sub_reg_imm32(&self, code_builder: &mut CodeBuilder, dst: u8, imm: i32) {
        self.emit_rex_prefix(code_builder, true, 0, 0, dst);
        code_builder.emit_byte(0x81);
        self.emit_modrm(code_builder, 0b11, 5, dst);
        code_builder.emit_i32(imm);
    }

    /// imul reg, reg
    fn emit_imul_reg_reg(&self, code_builder: &mut CodeBuilder, dst: u8, src: u8) {
        self.emit_rex_prefix(code_builder, true, dst, 0, src);
        code_builder.emit_bytes(&[0x0F, 0xAF]);
        self.emit_modrm(code_builder, 0b11, dst, src);
    }

    /// imul reg, imm32
    fn emit_imul_reg_imm32(&self, code_builder: &mut CodeBuilder, dst: u8, imm: i32) {
        self.emit_rex_prefix(code_builder, true, dst, 0, dst);
        code_builder.emit_byte(0x69);
        self.emit_modrm(code_builder, 0b11, dst, dst);
        code_builder.emit_i32(imm);
    }

    /// idiv reg (有符号除法)
    fn emit_idiv_reg(&self, code_builder: &mut CodeBuilder, src: u8) {
        self.emit_rex_prefix(code_builder, true, 0, 0, src);
        code_builder.emit_byte(0xF7);
        self.emit_modrm(code_builder, 0b11, 7, src); // /7 = IDIV
    }

    /// cmp reg, reg
    fn emit_cmp_reg_reg(&self, code_builder: &mut CodeBuilder, reg1: u8, reg2: u8) {
        self.emit_rex_prefix(code_builder, true, reg2, 0, reg1);
        code_builder.emit_byte(0x39);
        self.emit_modrm(code_builder, 0b11, reg2, reg1);
    }

    /// cmp reg, imm32
    fn emit_cmp_reg_imm32(&self, code_builder: &mut CodeBuilder, reg: u8, imm: i32) {
        self.emit_rex_prefix(code_builder, true, 0, 0, reg);
        code_builder.emit_byte(0x81);
        self.emit_modrm(code_builder, 0b11, 7, reg);
        code_builder.emit_i32(imm);
    }

    /// jmp reg
    fn emit_jmp_reg(&self, code_builder: &mut CodeBuilder, reg: u8) {
        if reg >= 8 {
            code_builder.emit_byte(0x41); // REX.B
        }
        code_builder.emit_bytes(&[0xFF, 0xE0 | (reg & 0x07)]);
    }

    /// call absolute address (通过 rax 间接调用)
    fn emit_call_absolute(&self, code_builder: &mut CodeBuilder, func: u64) {
        let rax: u8 = 0;
        self.emit_mov_reg_imm64(code_builder, rax, func as i64);
        code_builder.emit_byte(0xFF);
        self.emit_modrm(code_builder, 0b11, 0b010, rax); // CALL r/m64
    }

    /// 保存 caller-saved 寄存器（用于运行时函数调用）
    fn save_call_clobbered_registers(
        &self,
        code_builder: &mut CodeBuilder,
        exclude: &[u8],
    ) -> (Vec<u8>, usize) {
        // x86-64 System V ABI caller-saved: RAX, RCX, RDX, RSI, RDI, R8, R9, R10, R11
        let regs: Vec<u8> = [0u8, 1, 2, 6, 7, 8, 9, 10, 11]
            .into_iter()
            .filter(|reg| !exclude.contains(reg))
            .collect();

        if regs.is_empty() {
            return (regs, 0);
        }

        let stack_space = align_to(regs.len() * 8, 16);
        let vm_sp: u8 = 10;

        // 1. 在系统栈上保存 vm_sp（用于恢复 R10，因为 C 函数会破坏它）
        //    sub rsp, 16（保持对齐）
        self.emit_sub_reg_imm32(code_builder, 4, 16);
        //    mov [rsp], vm_sp
        self.emit_mov_mem_reg(code_builder, 4, 0, vm_sp);

        // 2. 保存所有 caller-saved 寄存器到虚拟栈（GC 可以扫描并更新）
        //    sub vm_sp, stack_space
        self.emit_sub_reg_imm32(code_builder, vm_sp, stack_space as i32);

        for (idx, reg) in regs.iter().enumerate() {
            self.emit_mov_mem_reg(code_builder, vm_sp, (idx * 8) as i32, *reg);
        }

        (regs, stack_space)
    }

    fn restore_call_clobbered_registers(
        &self,
        code_builder: &mut CodeBuilder,
        regs: &[u8],
        stack_space: usize,
    ) {
        if regs.is_empty() {
            return;
        }

        let vm_sp: u8 = 10;

        // 1. 从系统栈恢复 vm_sp（C 函数可能破坏了 R10）
        //    mov vm_sp, [rsp]
        self.emit_mov_reg_mem(code_builder, vm_sp, 4, 0);
        //    add rsp, 16（释放保存 vm_sp 的空间）
        self.emit_add_reg_imm32(code_builder, 4, 16);

        // 2. 此时 vm_sp 指向虚拟栈保存区域的起始位置之前（因为保存时 vm_sp 先 sub 了 stack_space，
        //    保存的值是 sub 之前的 vm_sp 值）。虚拟栈保存区在 vm_sp_saved - stack_space。
        //    所以需要 sub vm_sp, stack_space 来指向保存区。
        self.emit_sub_reg_imm32(code_builder, vm_sp, stack_space as i32);

        // 3. 从虚拟栈恢复所有寄存器（GC 可能已更新堆指针）
        for (idx, reg) in regs.iter().enumerate() {
            self.emit_mov_reg_mem(code_builder, *reg, vm_sp, (idx * 8) as i32);
        }

        // 4. add vm_sp, stack_space（恢复虚拟栈位置）
        self.emit_add_reg_imm32(code_builder, vm_sp, stack_space as i32);
    }
}

fn align_to(value: usize, alignment: usize) -> usize {
    ((value + alignment - 1) / alignment) * alignment
}

/// 计算函数需要的栈帧空间（从 StackFrameLayoutPass 生成的 Add FP, offset 指令推断）
fn compute_stack_frame_size(function: &LirFunction, frame_pointer_reg: u8) -> usize {
    let mut max_offset = 0i64;
    for inst in &function.instructions {
        if let Instruction::Add { src1, src2, .. } = inst {
            if let Operand::Register { id: Register::Physical(fp) } = src1 {
                if *fp == frame_pointer_reg {
                    if let Operand::Immediate { value } = src2 {
                        if *value < 0 {
                            max_offset = max_offset.max(-*value);
                        }
                    }
                }
            }
        }
    }
    // 对齐到 16 字节
    align_to(max_offset as usize, 16)
}

// ============================================================================
// 实现 JitCompiler trait
// ============================================================================
impl JitCompiler for X86Compiler {
    fn compile_function(
        &mut self,
        function: &LirFunction,
        program: &LirProgram,
    ) -> crate::Result<CompiledFunction> {
        if self.debug_mode {
            log::debug!("x86_64: 开始编译函数 '{}'", function.name);
        }

        // 缓存当前函数的 callee-saved 信息
        self.current_function_used_regs = function.get_used_regs().to_vec();

        // 计算栈帧大小
        // 注意：prologue 不分配帧空间——由 LIR 的 Sub vm_sp, N 指令分配
        // compute_stack_frame_size 从 Add dst, FP, offset 推断偏移量，
        // 但这个值不用于 prologue 分配（避免与 LIR 的 Sub vm_sp, N 双重分配）
        let cc = CallingConvention::standard();
        let computed_frame = compute_stack_frame_size(function, cc.frame_pointer);
        self.current_stack_frame_size = 0; // prologue 不分配帧空间

        // epilogue 需要知道帧大小来跳过帧区域
        // 使用 LIR 的 function.stack_frame_size（这是实际由 LIR 指令分配的量）
        self.stack_frame_size_for_epilogue = function.stack_frame_size as usize;

        let mut code_builder = CodeBuilder::new();

        // 生成函数标签
        let function_label = format!("func_{}", function.name);
        code_builder.define_label(&function_label)?;

        let is_main_function = self.is_entry_function(&function.name, program);

        // 只有 main 函数生成完整序言
        if is_main_function {
            self.emit_main_function_prologue(&mut code_builder)?;
        }

        // 内部函数不需要 main prologue，但需要保存 callee-saved
        // 注意：旧测试可能没有 Label 作为第一个指令，这里做容错处理
        let has_first_label = matches!(function.instructions.first(), Some(Instruction::Label { .. }));
        if has_first_label {
            if let Some(Instruction::Label { id, .. }) = function.instructions.first() {
                code_builder.define_label(&format!("label_{}", id.0))?;
            }
        }

        // 内部函数生成简化序言
        if !is_main_function {
            self.emit_internal_function_prologue(&mut code_builder)?;
        }

        // 编译所有指令
        let skip = if has_first_label { 1 } else { 0 };
        for instruction in function.instructions.iter().skip(skip) {
            self.compile_instruction(instruction, &mut code_builder, program, is_main_function)?;
        }

        // 获取label和修补信息
        let labels = code_builder.exported_labels().clone();
        let pending_jumps = code_builder.exported_pending_jumps().clone();
        let pending_label_addresses = code_builder.exported_pending_label_addresses().clone();
        let pending_adrs = code_builder.exported_pending_adrs().clone();

        let machine_code = code_builder.finalize()?;

        let mut compiled_function = CompiledFunction::new(
            function.name.clone(),
            machine_code,
            0,
        );

        compiled_function.labels = labels;
        compiled_function.pending_jumps = pending_jumps;
        compiled_function.pending_label_addresses = pending_label_addresses;
        compiled_function.pending_adrs = pending_adrs;

        log::info!(
            "x86_64: 函数 '{}' 编译完成，机器码大小: {} 字节",
            function.name,
            compiled_function.code_size()
        );

        Ok(compiled_function)
    }

    fn target_architecture(&self) -> &'static str {
        "x86-64"
    }

    fn get_register_mapping(&self) -> &HashMap<Register, u8> {
        // 不再需要映射表，直接使用 calling_convention 的编号
        static EMPTY: std::sync::OnceLock<HashMap<Register, u8>> = std::sync::OnceLock::new();
        EMPTY.get_or_init(HashMap::new)
    }

    fn supports_debug_info(&self) -> bool {
        true
    }

    fn get_calling_convention(&self) -> CallingConventionInfo {
        CallingConventionInfo {
            parameter_registers: vec![7, 6, 2, 1, 8, 9], // RDI, RSI, RDX, RCX, R8, R9
            return_register: 0,  // RAX
            stack_pointer: 4,    // RSP
            frame_pointer: 5,    // RBP
            caller_saved: vec![0, 1, 2, 6, 7, 8, 9, 10, 11],
            callee_saved: vec![3, 5, 12, 13, 14, 15],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use karte_lir::{LirFunction, LirProgram, Register};

    #[test]
    fn test_basic_compilation() {
        let mut function = LirFunction::new("test_basic".to_string());
        let target_label = function.new_label();
        function.add_instruction(Instruction::Label {
            id: target_label,
            span: karte_diagnostics::Span::dummy(),
        });
        function.add_instruction(Instruction::Move {
            dst: Register::Physical(0),
            src: Operand::Immediate { value: 42 },
            span: karte_diagnostics::Span::dummy(),
        });
        function.add_instruction(Instruction::Return {
            value: Some(Register::Physical(0)),
            span: karte_diagnostics::Span::dummy(),
        });

        let mut program = LirProgram::new();
        program.add_function(function);

        let mut compiler = X86Compiler::new(true).unwrap();
        let result = compiler.compile_function(&program.functions["test_basic"], &program);
        assert!(result.is_ok(), "编译失败: {:?}", result.err());

        let compiled = result.unwrap();
        assert!(compiled.code_size() > 0, "编译后的机器码为空");
    }

    #[test]
    fn test_store_pair_compilation() {
        let mut function = LirFunction::new("test_store_pair".to_string());
        let target_label = function.new_label();
        function.add_instruction(Instruction::Label {
            id: target_label,
            span: karte_diagnostics::Span::dummy(),
        });
        function.add_instruction(Instruction::StorePair {
            addr: Register::Physical(4), // RSP
            offset: -16,
            src1: Register::Physical(0),
            src2: Register::Physical(1),
            span: karte_diagnostics::Span::dummy(),
        });
        function.add_instruction(Instruction::Return {
            value: None,
            span: karte_diagnostics::Span::dummy(),
        });

        let mut program = LirProgram::new();
        program.add_function(function);

        let mut compiler = X86Compiler::new(true).unwrap();
        let result = compiler.compile_function(&program.functions["test_store_pair"], &program);
        assert!(result.is_ok(), "StorePair编译失败: {:?}", result.err());
    }
}
