//! RISC-V 64-bit JIT 编译器
//!
//! 将 LIR 指令编译为 RISC-V 64-bit (RV64GC) 机器码
//! 这是一个交叉编译器：运行在 x86_64/AArch64 主机上，生成 RISC-V 机器码
//!
//! 寄存器映射（Karte 虚拟编号 → RISC-V 硬件寄存器）：
//!   0=x10(a0), 1=x11(a1), 2=x12(a2), 3=x13(a3), 4=x14(a4),
//!   5=x15(a5), 6=x16(a6), 7=x17(a7), 8=x5(t0), 9=x6(t1),
//!   10=x2(sp)=vm_sp, 11=x8(s0/fp)=vm_fp, 12=x9(s1)=effect_sp,
//!   13-22=x18-x27(s2-s11)=callee-saved, 23-26=x28-x31(t3-t6)=temp,
//!   27=x1(ra)=return_address

use super::code_buffer::{CodeBuilder, JumpType, TargetArch};
use super::compiler_trait::*;
use super::ffi::{RuntimeArg, RuntimeCall};
use karte_common::calling_convention::{CallingConvention, CC};
use karte_lir::{ComparisonCondition, Instruction, LirFunction, LirProgram, Operand, Register};
use std::collections::HashMap;

use std::cell::Cell;

/// RISC-V 64-bit 编译器
#[derive(Debug)]
pub struct RiscvCompiler {
    /// 调试模式
    debug_mode: bool,
    /// 当前函数使用的 callee-saved 寄存器列表
    current_function_used_regs: Vec<u8>,
    /// 当前函数的栈帧大小
    current_stack_frame_size: usize,
    /// epilogue 需要跳过的栈帧大小
    stack_frame_size_for_epilogue: usize,
    /// 最近一次 Compare 指令的操作数（rs1, rs2 物理寄存器）
    /// RISC-V 没有标志寄存器，条件分支需要直接比较寄存器
    last_compare_operands: Cell<Option<(u8, u8)>>,
}

// ============================================================================
// RISC-V 寄存器编号映射
// ============================================================================
impl RiscvCompiler {
    /// 创建新的 RISC-V 编译器
    pub fn new(debug_mode: bool) -> crate::Result<Self> {
        Ok(Self {
            debug_mode,
            current_function_used_regs: Vec::new(),
            current_stack_frame_size: 0,
            stack_frame_size_for_epilogue: 0,
            last_compare_operands: Cell::new(None),
        })
    }

    /// Karte 虚拟编号 → RISC-V 硬件寄存器编号
    fn map_register(&self, reg: u8) -> u8 {
        match reg {
            0 => 10,  // a0
            1 => 11,  // a1
            2 => 12,  // a2
            3 => 13,  // a3
            4 => 14,  // a4
            5 => 15,  // a5
            6 => 16,  // a6
            7 => 17,  // a7
            8 => 5,   // t0
            9 => 6,   // t1
            10 => 2,  // sp (vm_sp)
            11 => 8,  // s0/fp (vm_fp)
            12 => 9,  // s1 (effect_sp)
            13 => 18, // s2
            14 => 19, // s3
            15 => 20, // s4
            16 => 21, // s5
            17 => 22, // s6
            18 => 23, // s7
            19 => 24, // s8
            20 => 25, // s9
            21 => 26, // s10
            22 => 27, // s11
            23 => 28, // t3
            24 => 29, // t4
            25 => 30, // t5
            26 => 31, // t6
            27 => 1,  // ra
            28 => 3,  // gp (not used)
            29 => 4,  // tp (not used)
            _ => panic!("RISC-V: 未知的寄存器编号 {}", reg),
        }
    }

    /// 获取物理寄存器编号（映射后的 RISC-V 硬件寄存器）
    fn get_physical_register(&self, reg: &Register) -> crate::Result<u8> {
        match reg {
            Register::Physical(id) => {
                if *id >= 30 {
                    Err(format!("RISC-V 不支持寄存器编号 {}", id).into())
                } else {
                    Ok(self.map_register(*id))
                }
            }
            Register::Virtual(id) => {
                panic!(
                    "RISC-V JIT 遇到虚拟寄存器 Virtual({})，寄存器分配应在 JIT 之前完成",
                    id
                );
            }
        }
    }
}

// ============================================================================
// RISC-V 指令编码辅助
// ============================================================================
impl RiscvCompiler {
    /// 编码 R-type 指令
    fn emit_r_type(&self, cb: &mut CodeBuilder, funct7: u8, rs2: u8, rs1: u8, funct3: u8, rd: u8, opcode: u8) {
        let instr = ((funct7 as u32) << 25)
            | ((rs2 as u32 & 0x1F) << 20)
            | ((rs1 as u32 & 0x1F) << 15)
            | ((funct3 as u32 & 0x7) << 12)
            | ((rd as u32 & 0x1F) << 7)
            | (opcode as u32 & 0x7F);
        cb.emit_u32(instr);
    }

    /// 编码 I-type 指令
    fn emit_i_type(&self, cb: &mut CodeBuilder, imm: i32, rs1: u8, funct3: u8, rd: u8, opcode: u8) {
        let instr = ((imm as u32 & 0xFFF) << 20)
            | ((rs1 as u32 & 0x1F) << 15)
            | ((funct3 as u32 & 0x7) << 12)
            | ((rd as u32 & 0x1F) << 7)
            | (opcode as u32 & 0x7F);
        cb.emit_u32(instr);
    }

    /// 编码 S-type 指令
    fn emit_s_type(&self, cb: &mut CodeBuilder, imm: i32, rs2: u8, rs1: u8, funct3: u8, opcode: u8) {
        let imm_val = imm as u32;
        let instr = (((imm_val >> 5) & 0x7F) << 25)
            | ((rs2 as u32 & 0x1F) << 20)
            | ((rs1 as u32 & 0x1F) << 15)
            | ((funct3 as u32 & 0x7) << 12)
            | ((imm_val & 0x1F) << 7)
            | (opcode as u32 & 0x7F);
        cb.emit_u32(instr);
    }

    /// 编码 B-type 指令（条件分支）
    fn emit_b_type(&self, cb: &mut CodeBuilder, imm: i32, rs2: u8, rs1: u8, funct3: u8, opcode: u8) {
        let imm_val = imm as u32;
        let instr = (((imm_val >> 12) & 0x1) << 31)
            | (((imm_val >> 5) & 0x3F) << 25)
            | ((rs2 as u32 & 0x1F) << 20)
            | ((rs1 as u32 & 0x1F) << 15)
            | ((funct3 as u32 & 0x7) << 12)
            | (((imm_val >> 1) & 0xF) << 8)
            | (((imm_val >> 11) & 0x1) << 7)
            | (opcode as u32 & 0x7F);
        cb.emit_u32(instr);
    }

    /// 编码 U-type 指令
    fn emit_u_type(&self, cb: &mut CodeBuilder, imm: u32, rd: u8, opcode: u8) {
        let instr = (imm & 0xFFFFF000) | ((rd as u32 & 0x1F) << 7) | (opcode as u32 & 0x7F);
        cb.emit_u32(instr);
    }

    /// 编码 J-type 指令 (JAL)
    fn emit_j_type(&self, cb: &mut CodeBuilder, imm: i32, rd: u8, opcode: u8) {
        let imm_val = imm as u32;
        let instr = (((imm_val >> 20) & 0x1) << 31)
            | (((imm_val >> 1) & 0x3FF) << 21)
            | (((imm_val >> 11) & 0x1) << 20)
            | (((imm_val >> 12) & 0xFF) << 12)
            | ((rd as u32 & 0x1F) << 7)
            | (opcode as u32 & 0x7F);
        cb.emit_u32(instr);
    }
}

// ============================================================================
// RISC-V 指令发射（高层）
// ============================================================================
impl RiscvCompiler {
    /// ADD rd, rs1, rs2
    fn emit_add(&self, cb: &mut CodeBuilder, rd: u8, rs1: u8, rs2: u8) {
        self.emit_r_type(cb, 0x00, rs2, rs1, 0x0, rd, 0x33);
    }

    /// SUB rd, rs1, rs2
    fn emit_sub(&self, cb: &mut CodeBuilder, rd: u8, rs1: u8, rs2: u8) {
        self.emit_r_type(cb, 0x20, rs2, rs1, 0x0, rd, 0x33);
    }

    /// ADDI rd, rs1, imm
    fn emit_addi(&self, cb: &mut CodeBuilder, rd: u8, rs1: u8, imm: i32) {
        self.emit_i_type(cb, imm, rs1, 0x0, rd, 0x13);
    }

    /// ADDIW rd, rs1, imm (RV64 only, opcode=0x1B)
    fn emit_addiw(&self, cb: &mut CodeBuilder, rd: u8, rs1: u8, imm: i32) {
        self.emit_i_type(cb, imm, rs1, 0x0, rd, 0x1B);
    }

    /// MUL rd, rs1, rs2
    fn emit_mul(&self, cb: &mut CodeBuilder, rd: u8, rs1: u8, rs2: u8) {
        self.emit_r_type(cb, 0x01, rs2, rs1, 0x0, rd, 0x33);
    }

    /// DIV rd, rs1, rs2 (有符号除法)
    fn emit_div(&self, cb: &mut CodeBuilder, rd: u8, rs1: u8, rs2: u8) {
        self.emit_r_type(cb, 0x01, rs2, rs1, 0x4, rd, 0x33);
    }

    /// REM rd, rs1, rs2 (有符号取余)
    fn emit_rem(&self, cb: &mut CodeBuilder, rd: u8, rs1: u8, rs2: u8) {
        self.emit_r_type(cb, 0x01, rs2, rs1, 0x6, rd, 0x33);
    }

    /// AND rd, rs1, rs2
    fn emit_and(&self, cb: &mut CodeBuilder, rd: u8, rs1: u8, rs2: u8) {
        self.emit_r_type(cb, 0x00, rs2, rs1, 0x7, rd, 0x33);
    }

    /// OR rd, rs1, rs2
    fn emit_or(&self, cb: &mut CodeBuilder, rd: u8, rs1: u8, rs2: u8) {
        self.emit_r_type(cb, 0x00, rs2, rs1, 0x6, rd, 0x33);
    }

    /// XOR rd, rs1, rs2
    fn emit_xor(&self, cb: &mut CodeBuilder, rd: u8, rs1: u8, rs2: u8) {
        self.emit_r_type(cb, 0x00, rs2, rs1, 0x4, rd, 0x33);
    }

    /// ANDI rd, rs1, imm
    fn emit_andi(&self, cb: &mut CodeBuilder, rd: u8, rs1: u8, imm: i32) {
        self.emit_i_type(cb, imm, rs1, 0x7, rd, 0x13);
    }

    /// ORI rd, rs1, imm
    fn emit_ori(&self, cb: &mut CodeBuilder, rd: u8, rs1: u8, imm: i32) {
        self.emit_i_type(cb, imm, rs1, 0x6, rd, 0x13);
    }

    /// XORI rd, rs1, imm
    fn emit_xori(&self, cb: &mut CodeBuilder, rd: u8, rs1: u8, imm: i32) {
        self.emit_i_type(cb, imm, rs1, 0x4, rd, 0x13);
    }

    /// SLL rd, rs1, rs2
    fn emit_sll(&self, cb: &mut CodeBuilder, rd: u8, rs1: u8, rs2: u8) {
        self.emit_r_type(cb, 0x00, rs2, rs1, 0x1, rd, 0x33);
    }

    /// SRL rd, rs1, rs2 (逻辑右移)
    fn emit_srl(&self, cb: &mut CodeBuilder, rd: u8, rs1: u8, rs2: u8) {
        self.emit_r_type(cb, 0x00, rs2, rs1, 0x5, rd, 0x33);
    }

    /// SLLI rd, rs1, shamt (RV64: 6-bit shamt)
    fn emit_slli(&self, cb: &mut CodeBuilder, rd: u8, rs1: u8, shamt: u32) {
        let imm12 = (shamt & 0x3F) << 6;
        let instr = (imm12 << 20) | ((rs1 as u32 & 0x1F) << 15) | ((rd as u32 & 0x1F) << 7) | 0x13;
        cb.emit_u32(instr);
    }

    /// SRLI rd, rs1, shamt (逻辑右移立即数, RV64: 6-bit shamt)
    fn emit_srli(&self, cb: &mut CodeBuilder, rd: u8, rs1: u8, shamt: u32) {
        let imm12 = (shamt & 0x3F) << 6;
        let instr = (imm12 << 20) | ((rs1 as u32 & 0x1F) << 15) | (0x5 << 12) | ((rd as u32 & 0x1F) << 7) | 0x13;
        cb.emit_u32(instr);
    }

    /// SLT rd, rs1, rs2 (有符号小于)
    fn emit_slt(&self, cb: &mut CodeBuilder, rd: u8, rs1: u8, rs2: u8) {
        self.emit_r_type(cb, 0x00, rs2, rs1, 0x2, rd, 0x33);
    }

    /// SLTU rd, rs1, rs2 (无符号小于)
    fn emit_sltu(&self, cb: &mut CodeBuilder, rd: u8, rs1: u8, rs2: u8) {
        self.emit_r_type(cb, 0x00, rs2, rs1, 0x3, rd, 0x33);
    }

    /// SLTIU rd, rs1, imm (无符号小于立即数)
    fn emit_sltiu(&self, cb: &mut CodeBuilder, rd: u8, rs1: u8, imm: i32) {
        self.emit_i_type(cb, imm, rs1, 0x3, rd, 0x13);
    }

    /// LD rd, offset(rs1) — 64-bit 加载
    fn emit_ld(&self, cb: &mut CodeBuilder, rd: u8, rs1: u8, offset: i32) {
        self.emit_i_type(cb, offset, rs1, 0x3, rd, 0x03);
    }

    /// LW rd, offset(rs1) — 32-bit 有符号加载
    fn emit_lw(&self, cb: &mut CodeBuilder, rd: u8, rs1: u8, offset: i32) {
        self.emit_i_type(cb, offset, rs1, 0x2, rd, 0x03);
    }

    /// LBU rd, offset(rs1) — 8-bit 无符号加载
    fn emit_lbu(&self, cb: &mut CodeBuilder, rd: u8, rs1: u8, offset: i32) {
        self.emit_i_type(cb, offset, rs1, 0x4, rd, 0x03);
    }

    /// SD rs2, offset(rs1) — 64-bit 存储
    fn emit_sd(&self, cb: &mut CodeBuilder, src: u8, base: u8, offset: i32) {
        self.emit_s_type(cb, offset, src, base, 0x3, 0x23);
    }

    /// SW rs2, offset(rs1) — 32-bit 存储
    fn emit_sw(&self, cb: &mut CodeBuilder, src: u8, base: u8, offset: i32) {
        self.emit_s_type(cb, offset, src, base, 0x2, 0x23);
    }

    /// SB rs2, offset(rs1) — 8-bit 存储
    fn emit_sb(&self, cb: &mut CodeBuilder, src: u8, base: u8, offset: i32) {
        self.emit_s_type(cb, offset, src, base, 0x0, 0x23);
    }

    /// JAL rd, offset
    fn emit_jal(&self, cb: &mut CodeBuilder, rd: u8, offset: i32) {
        self.emit_j_type(cb, offset, rd, 0x6F);
    }

    /// JALR rd, rs1, imm
    fn emit_jalr(&self, cb: &mut CodeBuilder, rd: u8, rs1: u8, imm: i32) {
        self.emit_i_type(cb, imm, rs1, 0x0, rd, 0x67);
    }

    /// LUI rd, imm
    fn emit_lui(&self, cb: &mut CodeBuilder, rd: u8, imm: u32) {
        self.emit_u_type(cb, imm, rd, 0x37);
    }

    /// AUIPC rd, imm
    fn emit_auipc(&self, cb: &mut CodeBuilder, rd: u8, imm: u32) {
        self.emit_u_type(cb, imm, rd, 0x17);
    }

    /// NOP (ADDI x0, x0, 0)
    fn emit_nop(&self, cb: &mut CodeBuilder) {
        self.emit_addi(cb, 0, 0, 0);
    }
}

// ============================================================================
// 高层操作：加载 64 位立即数
// ============================================================================
impl RiscvCompiler {
    /// 检查值是否可以用 12 位有符号立即数表示
    fn as_i12(&self, value: i64) -> Option<i32> {
        if (-2048i64..=2047i64).contains(&value) {
            Some(value as i32)
        } else {
            None
        }
    }

    /// 加载 64 位立即数到寄存器
    /// 使用"数据内联 + 跳过"方案：
    ///   AUIPC rd, 0            ; rd = PC
    ///   LD    rd, rd, 12       ; 从 rd+12 加载（跳过 3 条指令 = 12 字节）
    ///   JALR  x0, t0, 16       ; 跳过 16 字节（3 条指令 + 8 字节数据）
    ///   .quad value             ; 8 字节内联常量
    fn emit_load_imm64(&self, cb: &mut CodeBuilder, rd: u8, value: i64) {
        // 快速路径：小立即数 [-2048, 2047]
        if let Some(imm12) = self.as_i12(value) {
            self.emit_addi(cb, rd, 0, imm12);
            return;
        }

        // 快速路径：可以用 LUI + ADDIW 表示（值高 32 位全为 0）
        let uval = value as u64;
        if uval < 0x8000_0000 && uval >= 0 {
            let upper = ((uval >> 12) & 0xFFFFF) as u32;
            let lower = (uval & 0xFFF) as u32;
            if lower != 0 {
                self.emit_lui(cb, rd, upper << 12);
                self.emit_addiw(cb, rd, rd, lower as i32);
            } else {
                self.emit_lui(cb, rd, upper << 12);
            }
            return;
        }

        // 通用路径：在代码段嵌入 8 字节常量
        self.emit_auipc(cb, rd, 0);      // rd = PC (当前 AUIPC 的地址)
        self.emit_ld(cb, rd, rd, 12);    // 从 rd+12 加载（跳过 AUIPC+LD+JAL = 12 字节）
        self.emit_jal(cb, 0, 12);        // 跳过内联数据（12 字节 = 本指令到 .quad 之后）
        cb.emit_i64(value);               // 内联 8 字节常量
    }

    /// 将操作数值加载到寄存器
    fn load_operand_to_reg(&self, cb: &mut CodeBuilder, rd: u8, operand: &Operand) -> crate::Result<()> {
        match operand {
            Operand::Register { id } => {
                let src = self.get_physical_register(id)?;
                if rd != src {
                    self.emit_addi(cb, rd, src, 0); // MV rd, src
                }
            }
            Operand::Immediate { value } => {
                self.emit_load_imm64(cb, rd, *value);
            }
            Operand::Label { id } => {
                // 将标签地址加载到 rd
                // 使用数据内联方案：
                //   AUIPC rd, 0; LD rd, rd, 12; JAL x0, 12; .quad <label_addr>
                let label_name = format!("label_{}", id.0);
                self.emit_auipc(cb, rd, 0);
                self.emit_ld(cb, rd, rd, 12);
                self.emit_jal(cb, 0, 12);
                // 8 字节占位，通过 emit_label_address 机制 patch
                cb.emit_label_address(&label_name);
            }
            _ => {
                return Err(format!("RISC-V: 不支持的操作数类型: {:?}", operand).into());
            }
        }
        Ok(())
    }

    /// 将操作数加载到寄存器，如果操作数是寄存器则直接返回其映射后的编号
    /// 如果需要加载则使用临时寄存器 tmp
    fn resolve_operand_reg(&self, cb: &mut CodeBuilder, operand: &Operand, tmp: u8) -> crate::Result<u8> {
        match operand {
            Operand::Register { id } => Ok(self.get_physical_register(id)?),
            Operand::Immediate { value } => {
                self.emit_load_imm64(cb, tmp, *value);
                Ok(tmp)
            }
            Operand::Label { id } => {
                let label_name = format!("label_{}", id.0);
                self.emit_auipc(cb, tmp, 0);
                self.emit_ld(cb, tmp, tmp, 12);
                self.emit_jal(cb, 0, 12);
                cb.emit_label_address(&label_name);
                Ok(tmp)
            }
            _ => Err(format!("RISC-V: 不支持的操作数类型: {:?}", operand).into()),
        }
    }
}

// ============================================================================
// 指令编译方法
// ============================================================================
impl RiscvCompiler {
    fn compile_instruction(
        &mut self,
        instruction: &Instruction,
        cb: &mut CodeBuilder,
        program: &LirProgram,
        is_main_function: bool,
    ) -> crate::Result<()> {
        if self.debug_mode {
            log::debug!("RISC-V 编译指令: {:?}", instruction);
        }

        match instruction {
            Instruction::Move { dst, src, .. } => self.compile_move(dst, src, cb),
            Instruction::Add { dst, src1, src2, .. } => self.compile_add(dst, src1, src2, cb),
            Instruction::Sub { dst, src1, src2, .. } => self.compile_sub(dst, src1, src2, cb),
            Instruction::Mul { dst, src1, src2, .. } => self.compile_mul(dst, src1, src2, cb),
            Instruction::Div { dst, src1, src2, .. } => self.compile_div(dst, src1, src2, cb),
            Instruction::BitAnd { dst, src1, src2, .. } => self.compile_bitand(dst, src1, src2, cb),
            Instruction::BitOr { dst, src1, src2, .. } => self.compile_bitor(dst, src1, src2, cb),
            Instruction::BitXor { dst, src1, src2, .. } => self.compile_bitxor(dst, src1, src2, cb),
            Instruction::ShiftLeft { dst, src1, src2, .. } => self.compile_shift_left(dst, src1, src2, cb),
            Instruction::ShiftRight { dst, src1, src2, .. } => self.compile_shift_right(dst, src1, src2, cb),
            Instruction::BitNot { dst, src, .. } => self.compile_bitnot(dst, src, cb),
            Instruction::Compare { src1, src2, .. } => self.compile_compare(src1, src2, cb),
            Instruction::CompareSet { dst, condition, src1, src2, .. } => {
                self.compile_compare_set(dst, condition, src1, src2, cb)
            }
            Instruction::Jump { target, .. } => self.compile_jump(target, cb),
            Instruction::JumpEqual { target, .. } => {
                self.compile_conditional_jump(JumpType::ConditionalEqual, target, cb)
            }
            Instruction::JumpNotEqual { target, .. } => {
                self.compile_conditional_jump(JumpType::ConditionalNotEqual, target, cb)
            }
            Instruction::JumpLess { target, .. } => {
                self.compile_conditional_jump(JumpType::ConditionalLess, target, cb)
            }
            Instruction::JumpLessEqual { target, .. } => {
                self.compile_conditional_jump(JumpType::ConditionalLessEqual, target, cb)
            }
            Instruction::JumpGreater { target, .. } => {
                self.compile_conditional_jump(JumpType::ConditionalGreater, target, cb)
            }
            Instruction::JumpGreaterEqual { target, .. } => {
                self.compile_conditional_jump(JumpType::ConditionalGreaterEqual, target, cb)
            }
            Instruction::Call { target, .. } => self.compile_call(target, cb),
            Instruction::JumpIndirect { function_register, .. } => {
                self.compile_jump_indirect(function_register, cb)
            }
            Instruction::JumpRegister { target_register, .. } => {
                self.compile_jump_register(target_register, cb)
            }
            Instruction::Return { value, .. } => {
                self.compile_return(value.as_ref(), cb, is_main_function)
            }
            Instruction::Label { id, .. } => {
                let label_name = format!("label_{}", id.0);
                cb.define_label(&label_name)?;
                Ok(())
            }
            Instruction::Load64 { dst, addr, offset, .. } => self.compile_load64(dst, addr, *offset, cb),
            Instruction::Store64 { addr, offset, src, .. } => self.compile_store64(addr, *offset, src, cb),
            Instruction::Load32 { dst, addr, offset, .. } => self.compile_load32(dst, addr, *offset, cb),
            Instruction::Store32 { addr, offset, src, .. } => self.compile_store32(addr, *offset, src, cb),
            Instruction::Load8 { dst, addr, offset, .. } => self.compile_load8(dst, addr, *offset, cb),
            Instruction::Store8 { addr, offset, src, .. } => self.compile_store8(addr, *offset, src, cb),
            Instruction::StorePair { addr, offset, src1, src2, .. } => {
                self.compile_store_pair(addr, *offset, src1, src2, cb)
            }
            Instruction::LoadPair { dst1, dst2, addr, offset, .. } => {
                self.compile_load_pair(dst1, dst2, addr, *offset, cb)
            }
            Instruction::Alloc { dst, size, alignment, allocation_type, .. } => {
                self.compile_alloc(dst, *size, *alignment, allocation_type, cb)
            }
            Instruction::Free { addr, .. } => self.compile_free(addr, cb),
            Instruction::Retain { value, .. } => self.compile_retain(value, cb),
            Instruction::Release { value, .. } => self.compile_release(value, cb),
            Instruction::Safepoint { .. } => self.compile_safepoint(cb),
            Instruction::Nop { .. } => {
                self.emit_nop(cb);
                Ok(())
            }
            Instruction::StructAlloc { .. } => Err("StructAlloc 应该已经被降级为 Alloc".into()),
            Instruction::StructFieldLoad { .. }
            | Instruction::StructFieldStore { .. }
            | Instruction::StructFieldAddr { .. } => {
                Err(format!("Struct 操作应该已经被降级: {:?}", instruction).into())
            }
            Instruction::MemCopy { .. } => Err("MemCopy 应该已经被降级".into()),
            Instruction::LoadGlobal { dst, name, .. } => self.compile_load_global(dst, name, cb),
            Instruction::GcRegOp { is_push, .. } => self.compile_gc_reg_op(*is_push, cb),
            Instruction::Phi { .. } => {
                log::warn!("Phi 指令出现在 JIT 编译阶段，这表明 SSA 降级不完整");
                Ok(())
            }
            _ => Err(format!("RISC-V: 不支持的指令类型: {:?}", instruction).into()),
        }
    }
}

// ============================================================================
// 各指令的编译实现
// ============================================================================
impl RiscvCompiler {
    fn compile_move(&self, dst: &Register, src: &Operand, cb: &mut CodeBuilder) -> crate::Result<()> {
        let dst_rv = self.get_physical_register(dst)?;
        self.load_operand_to_reg(cb, dst_rv, src)
    }

    fn compile_add(&self, dst: &Register, src1: &Operand, src2: &Operand, cb: &mut CodeBuilder) -> crate::Result<()> {
        let dst_rv = self.get_physical_register(dst)?;
        self.load_operand_to_reg(cb, dst_rv, src1)?;
        let src2_rv = self.resolve_operand_reg(cb, src2, 5)?;
        self.emit_add(cb, dst_rv, dst_rv, src2_rv);
        Ok(())
    }

    fn compile_sub(&self, dst: &Register, src1: &Operand, src2: &Operand, cb: &mut CodeBuilder) -> crate::Result<()> {
        let dst_rv = self.get_physical_register(dst)?;
        self.load_operand_to_reg(cb, dst_rv, src1)?;
        let src2_rv = self.resolve_operand_reg(cb, src2, 5)?;
        self.emit_sub(cb, dst_rv, dst_rv, src2_rv);
        Ok(())
    }

    fn compile_mul(&self, dst: &Register, src1: &Operand, src2: &Operand, cb: &mut CodeBuilder) -> crate::Result<()> {
        let dst_rv = self.get_physical_register(dst)?;
        self.load_operand_to_reg(cb, dst_rv, src1)?;
        let src2_rv = self.resolve_operand_reg(cb, src2, 5)?;
        self.emit_mul(cb, dst_rv, dst_rv, src2_rv);
        Ok(())
    }

    fn compile_div(&self, dst: &Register, src1: &Operand, src2: &Operand, cb: &mut CodeBuilder) -> crate::Result<()> {
        let dst_rv = self.get_physical_register(dst)?;
        self.load_operand_to_reg(cb, dst_rv, src1)?;
        let src2_rv = self.resolve_operand_reg(cb, src2, 5)?;
        self.emit_div(cb, dst_rv, dst_rv, src2_rv);
        Ok(())
    }

    fn compile_bitand(&self, dst: &Register, src1: &Operand, src2: &Operand, cb: &mut CodeBuilder) -> crate::Result<()> {
        let dst_rv = self.get_physical_register(dst)?;
        if let (Operand::Immediate { value: v1 }, Operand::Immediate { value: v2 }) = (src1, src2) {
            self.emit_load_imm64(cb, dst_rv, v1 & v2);
            return Ok(());
        }
        self.load_operand_to_reg(cb, dst_rv, src1)?;
        let src2_rv = self.resolve_operand_reg(cb, src2, 5)?;
        self.emit_and(cb, dst_rv, dst_rv, src2_rv);
        Ok(())
    }

    fn compile_bitor(&self, dst: &Register, src1: &Operand, src2: &Operand, cb: &mut CodeBuilder) -> crate::Result<()> {
        let dst_rv = self.get_physical_register(dst)?;
        if let (Operand::Immediate { value: v1 }, Operand::Immediate { value: v2 }) = (src1, src2) {
            self.emit_load_imm64(cb, dst_rv, v1 | v2);
            return Ok(());
        }
        self.load_operand_to_reg(cb, dst_rv, src1)?;
        let src2_rv = self.resolve_operand_reg(cb, src2, 5)?;
        self.emit_or(cb, dst_rv, dst_rv, src2_rv);
        Ok(())
    }

    fn compile_bitxor(&self, dst: &Register, src1: &Operand, src2: &Operand, cb: &mut CodeBuilder) -> crate::Result<()> {
        let dst_rv = self.get_physical_register(dst)?;
        if let (Operand::Immediate { value: v1 }, Operand::Immediate { value: v2 }) = (src1, src2) {
            self.emit_load_imm64(cb, dst_rv, v1 ^ v2);
            return Ok(());
        }
        self.load_operand_to_reg(cb, dst_rv, src1)?;
        let src2_rv = self.resolve_operand_reg(cb, src2, 5)?;
        self.emit_xor(cb, dst_rv, dst_rv, src2_rv);
        Ok(())
    }

    fn compile_shift_left(&self, dst: &Register, src1: &Operand, src2: &Operand, cb: &mut CodeBuilder) -> crate::Result<()> {
        let dst_rv = self.get_physical_register(dst)?;
        if let (Operand::Immediate { value: v1 }, Operand::Immediate { value: v2 }) = (src1, src2) {
            self.emit_load_imm64(cb, dst_rv, v1 << (v2 & 63));
            return Ok(());
        }
        self.load_operand_to_reg(cb, dst_rv, src1)?;
        match src2 {
            Operand::Immediate { value } => {
                self.emit_slli(cb, dst_rv, dst_rv, (*value as u32) & 0x3F);
            }
            _ => {
                let src2_rv = self.resolve_operand_reg(cb, src2, 5)?;
                self.emit_sll(cb, dst_rv, dst_rv, src2_rv);
            }
        }
        Ok(())
    }

    fn compile_shift_right(&self, dst: &Register, src1: &Operand, src2: &Operand, cb: &mut CodeBuilder) -> crate::Result<()> {
        let dst_rv = self.get_physical_register(dst)?;
        if let (Operand::Immediate { value: v1 }, Operand::Immediate { value: v2 }) = (src1, src2) {
            self.emit_load_imm64(cb, dst_rv, ((*v1 as u64) >> (*v2 as u64 & 63)) as i64);
            return Ok(());
        }
        self.load_operand_to_reg(cb, dst_rv, src1)?;
        match src2 {
            Operand::Immediate { value } => {
                self.emit_srli(cb, dst_rv, dst_rv, (*value as u32) & 0x3F);
            }
            _ => {
                let src2_rv = self.resolve_operand_reg(cb, src2, 5)?;
                self.emit_srl(cb, dst_rv, dst_rv, src2_rv);
            }
        }
        Ok(())
    }

    fn compile_bitnot(&self, dst: &Register, src: &Operand, cb: &mut CodeBuilder) -> crate::Result<()> {
        let dst_rv = self.get_physical_register(dst)?;
        if let Operand::Immediate { value } = src {
            self.emit_load_imm64(cb, dst_rv, !value);
            return Ok(());
        }
        self.load_operand_to_reg(cb, dst_rv, src)?;
        self.emit_xori(cb, dst_rv, dst_rv, -1); // NOT = XORI rd, rd, -1
        Ok(())
    }

    fn compile_compare(&self, src1: &Operand, src2: &Operand, cb: &mut CodeBuilder) -> crate::Result<()> {
        // RISC-V 没有标志寄存器，条件分支需要直接比较寄存器。
        // 在这里解析操作数到物理寄存器，保存起来供后续条件分支使用。
        let rs1 = self.resolve_operand_reg(cb, src1, 5)?;
        let rs2 = self.resolve_operand_reg(cb, src2, 6)?;
        self.last_compare_operands.set(Some((rs1, rs2)));
        Ok(())
    }

    fn compile_compare_set(
        &self, dst: &Register, condition: &ComparisonCondition, src1: &Operand, src2: &Operand, cb: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let dst_rv = self.get_physical_register(dst)?;

        // 加载两个操作数
        self.load_operand_to_reg(cb, dst_rv, src1)?;
        let src2_rv = self.resolve_operand_reg(cb, src2, 5)?;

        match condition {
            ComparisonCondition::Equal => {
                // SUB dst, dst, src2; SLTIU dst, dst, 1
                self.emit_sub(cb, dst_rv, dst_rv, src2_rv);
                self.emit_sltiu(cb, dst_rv, dst_rv, 1);
            }
            ComparisonCondition::NotEqual => {
                // SUB dst, dst, src2; SLTU dst, x0, dst
                self.emit_sub(cb, dst_rv, dst_rv, src2_rv);
                self.emit_sltu(cb, dst_rv, 0, dst_rv);
            }
            ComparisonCondition::LessThan => {
                self.emit_slt(cb, dst_rv, dst_rv, src2_rv);
            }
            ComparisonCondition::GreaterThan => {
                self.emit_slt(cb, dst_rv, src2_rv, dst_rv);
            }
            ComparisonCondition::LessEqual => {
                // !(src2 < src1) = SLT dst, src2, src1; XORI dst, dst, 1
                self.emit_slt(cb, dst_rv, src2_rv, dst_rv);
                self.emit_xori(cb, dst_rv, dst_rv, 1);
            }
            ComparisonCondition::GreaterEqual => {
                // !(src1 < src2) = SLT dst, src1, src2; XORI dst, dst, 1
                self.emit_slt(cb, dst_rv, dst_rv, src2_rv);
                self.emit_xori(cb, dst_rv, dst_rv, 1);
            }
        }
        Ok(())
    }

    fn compile_jump(&self, target: &karte_lir::LabelId, cb: &mut CodeBuilder) -> crate::Result<()> {
        let label_name = format!("label_{}", target.0);
        // 使用 CodeBuilder 的 emit_jump 机制（与 x86 一致）
        cb.emit_jump(JumpType::Unconditional, &label_name);
        Ok(())
    }

    fn compile_conditional_jump(
        &self, jump_type: JumpType, target: &karte_lir::LabelId, cb: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let label_name = format!("label_{}", target.0);

        // 获取最近 Compare 指令的操作数
        let (rs1, rs2) = self.last_compare_operands.get()
            .ok_or_else(|| format!("条件分支前没有 Compare 指令: {:?}", jump_type))?;

        // 确定分支指令的 funct3 和操作数顺序
        // RISC-V B-type: BEQ=000, BNE=001, BLT=100, BGE=101, BLTU=110, BGEU=111
        let (funct3, actual_rs1, actual_rs2) = match jump_type {
            JumpType::ConditionalEqual => (0b000, rs1, rs2),         // BEQ rs1, rs2
            JumpType::ConditionalNotEqual => (0b001, rs1, rs2),      // BNE rs1, rs2
            JumpType::ConditionalLess => (0b100, rs1, rs2),           // BLT rs1, rs2 (signed)
            JumpType::ConditionalGreaterEqual => (0b101, rs1, rs2),   // BGE rs1, rs2 (signed)
            JumpType::ConditionalGreater => (0b100, rs2, rs1),        // BLT rs2, rs1 (即 rs1 > rs2)
            JumpType::ConditionalLessEqual => (0b101, rs2, rs1),      // BGE rs2, rs1 (即 rs1 <= rs2)
            _ => return Err(format!("不支持的条件分支类型: {:?}", jump_type).into()),
        };

        // 记录当前位置用于后续 pending_jump 修补
        let patch_position = cb.position();

        // 生成 B-type 占位指令（offset=0），后续由 AOT patch 修正
        self.emit_b_type(cb, 0, actual_rs2, actual_rs1, funct3, 0x63);

        // 添加 pending_jump 用于后续地址修补
        cb.add_pending_jump_raw(patch_position, label_name, jump_type);

        Ok(())
    }

    fn compile_call(&self, target: &karte_lir::LabelId, cb: &mut CodeBuilder) -> crate::Result<()> {
        let label_name = format!("label_{}", target.0);
        // Karte 虚拟机使用 Jump 而不是硬件 CALL，返回地址通过虚拟栈管理
        cb.emit_jump(JumpType::Call, &label_name);
        Ok(())
    }

    fn compile_jump_indirect(&self, function_register: &Register, cb: &mut CodeBuilder) -> crate::Result<()> {
        let reg = self.get_physical_register(function_register)?;
        // JALR x0, reg, 0 — 跳转不保存返回地址
        self.emit_jalr(cb, 0, reg, 0);
        Ok(())
    }

    fn compile_jump_register(&self, target_register: &Register, cb: &mut CodeBuilder) -> crate::Result<()> {
        let reg = self.get_physical_register(target_register)?;
        self.emit_jalr(cb, 0, reg, 0);
        Ok(())
    }

    fn compile_return(
        &self, value: Option<&Register>, cb: &mut CodeBuilder, is_main_function: bool,
    ) -> crate::Result<()> {
        // 返回值 → a0 (Karte #0 → x10)
        let a0 = self.map_register(0);
        if let Some(reg) = value {
            let src_rv = self.get_physical_register(reg)?;
            if src_rv != a0 {
                self.emit_addi(cb, a0, src_rv, 0);
            }
        } else {
            self.emit_addi(cb, a0, 0, 0);
        }

        if is_main_function {
            self.emit_main_function_epilogue(cb)?;
        } else {
            self.emit_internal_function_epilogue(cb)?;
            // 从虚拟栈加载返回地址并跳转
            let vm_sp = self.map_register(10); // x2
            let tmp = self.map_register(8);    // t0 = x5
            self.emit_ld(cb, tmp, vm_sp, 0);
            // 不弹出返回地址（caller 负责）
            self.emit_jalr(cb, 0, tmp, 0);
        }
        Ok(())
    }

    fn compile_load_global(&self, dst: &Register, name: &str, cb: &mut CodeBuilder) -> crate::Result<()> {
        let dst_rv = self.get_physical_register(dst)?;

        // vm_sp = RISC-V x2 (sp)，直接读取
        if name == "vm_sp" {
            // mv dst, sp
            self.emit_addi(cb, dst_rv, 2, 0);
            return Ok(());
        }

        // stack_top = vstack_bottom + 65520，但 RISC-V 中 vstack_bottom 没有固定寄存器
        // 需要从全局变量加载
        if name == "stack_top" {
            let global_label = "__global_stack_bottom".to_string();
            self.emit_auipc(cb, dst_rv, 0);
            self.emit_ld(cb, dst_rv, dst_rv, 12);
            self.emit_jal(cb, 0, 12);
            cb.emit_label_address(&global_label);
            // dst = vstack_bottom 地址，加载值
            self.emit_ld(cb, dst_rv, dst_rv, 0);
            // dst += 65520
            self.emit_load_imm64(cb, 5, 65520); // t0 = 65520
            self.emit_add(cb, dst_rv, dst_rv, 5);
            return Ok(());
        }

        let global_label = format!("__global_{}", name);

        // 使用数据内联 + emit_label_address 加载地址，然后从地址加载值。
        self.emit_auipc(cb, dst_rv, 0);
        self.emit_ld(cb, dst_rv, dst_rv, 12);
        self.emit_jal(cb, 0, 12);
        cb.emit_label_address(&global_label);
        // dst 现在是全局变量的地址，加载值
        self.emit_ld(cb, dst_rv, dst_rv, 0);
        Ok(())
    }

    /// GC 寄存器保存/恢复 - 把所有可能持有堆指针的寄存器 dump 到虚拟栈
    /// 保存: a0-a7(x10-x17), s2-s11(x18-x27), t0-t5(x5-x7,x28-x30) = 20 个 × 8 字节 = 160 字节
    /// 不保存: zero(x0), sp(x2)=vm_sp, gp(x3), tp(x4), s0(x8)=vm_fp, s1(x9)
    fn compile_gc_reg_op(&self, is_push: bool, cb: &mut CodeBuilder) -> crate::Result<()> {
        // 需要保存的 RISC-V 物理寄存器编号
        const REGS: [u8; 20] = [5, 6, 7, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26];
        // 注意: 没存 x27(s11), x28(t3), x29(t4), x30(t5) — 如果需要可以加
        const NUM_REGS: i64 = 20;
        const FRAME_SIZE: i64 = NUM_REGS * 8; // 160

        // vm_sp 在 RISC-V 中是 x2 (sp)
        if is_push {
            // addi sp, sp, -160
            self.emit_addi(cb, 2, 2, -(FRAME_SIZE as i32));
            // sd reg, offset(sp) 逐个保存
            for (i, &reg) in REGS.iter().enumerate() {
                let offset = (i as i32) * 8;
                self.emit_sd(cb, reg, 2, offset);
            }
        } else {
            // ld reg, offset(sp) 逐个恢复
            for (i, &reg) in REGS.iter().enumerate() {
                let offset = (i as i32) * 8;
                self.emit_ld(cb, reg, 2, offset);
            }
            // addi sp, sp, 160
            self.emit_addi(cb, 2, 2, FRAME_SIZE as i32);
        }

        Ok(())
    }

    fn compile_load64(&self, dst: &Register, addr: &Register, offset: i64, cb: &mut CodeBuilder) -> crate::Result<()> {
        let dst_rv = self.get_physical_register(dst)?;
        let addr_rv = self.get_physical_register(addr)?;
        if let Some(imm12) = self.as_i12(offset) {
            self.emit_ld(cb, dst_rv, addr_rv, imm12);
        } else {
            let tmp = 5u8;
            self.emit_load_imm64(cb, tmp, offset);
            self.emit_add(cb, tmp, addr_rv, tmp);
            self.emit_ld(cb, dst_rv, tmp, 0);
        }
        Ok(())
    }

    fn compile_store64(&self, addr: &Register, offset: i64, src: &Operand, cb: &mut CodeBuilder) -> crate::Result<()> {
        let addr_rv = self.get_physical_register(addr)?;

        // 计算有效地址（大偏移量需要临时计算）
        let (base, eff_offset) = if let Some(imm12) = self.as_i12(offset) {
            (addr_rv, imm12)
        } else {
            let tmp = 5u8;
            self.emit_load_imm64(cb, tmp, offset);
            self.emit_add(cb, tmp, addr_rv, tmp);
            (tmp, 0)
        };

        match src {
            Operand::Register { id } => {
                let src_rv = self.get_physical_register(id)?;
                self.emit_sd(cb, src_rv, base, eff_offset);
            }
            Operand::Immediate { value } => {
                let tmp2 = 7u8; // t2 — 避免与 effect_resume_temp(t1=x6) 冲突
                self.emit_load_imm64(cb, tmp2, *value);
                self.emit_sd(cb, tmp2, base, eff_offset);
            }
            Operand::Label { id } => {
                let label_name = format!("label_{}", id.0);
                // 用数据内联方式加载标签地址
                // 注意：不能用 x6(t1)，因为它是 effect_resume_temp (Karte #9)
                // CallIndirect 序列中 Store64(Label) 之后紧跟 JumpIndirect(#9=t1)，
                // 如果这里也用 t1 作为临时寄存器，会覆盖掉之前存入的函数指针
                let tmp2 = 7u8; // t2 — 不与 effect_resume_temp(t1) 冲突
                self.emit_auipc(cb, tmp2, 0);
                self.emit_ld(cb, tmp2, tmp2, 12);
                self.emit_jal(cb, 0, 12);
                cb.emit_label_address(&label_name);
                self.emit_sd(cb, tmp2, base, eff_offset);
            }
            _ => return Err(format!("store64 不支持的 src: {:?}", src).into()),
        }
        Ok(())
    }

    fn compile_load32(&self, dst: &Register, addr: &Register, offset: i64, cb: &mut CodeBuilder) -> crate::Result<()> {
        let dst_rv = self.get_physical_register(dst)?;
        let addr_rv = self.get_physical_register(addr)?;
        if let Some(imm12) = self.as_i12(offset) {
            self.emit_lw(cb, dst_rv, addr_rv, imm12);
        } else {
            let tmp = 5u8;
            self.emit_load_imm64(cb, tmp, offset);
            self.emit_add(cb, tmp, addr_rv, tmp);
            self.emit_lw(cb, dst_rv, tmp, 0);
        }
        Ok(())
    }

    fn compile_store32(&self, addr: &Register, offset: i64, src: &Operand, cb: &mut CodeBuilder) -> crate::Result<()> {
        let addr_rv = self.get_physical_register(addr)?;
        let (base, eff_offset) = if let Some(imm12) = self.as_i12(offset) {
            (addr_rv, imm12)
        } else {
            let tmp = 5u8;
            self.emit_load_imm64(cb, tmp, offset);
            self.emit_add(cb, tmp, addr_rv, tmp);
            (tmp, 0)
        };

        match src {
            Operand::Register { id } => {
                let src_rv = self.get_physical_register(id)?;
                self.emit_sw(cb, src_rv, base, eff_offset);
            }
            Operand::Immediate { value } => {
                let tmp2 = 6u8;
                self.emit_load_imm64(cb, tmp2, *value);
                self.emit_sw(cb, tmp2, base, eff_offset);
            }
            _ => return Err(format!("store32 不支持的 src: {:?}", src).into()),
        }
        Ok(())
    }

    fn compile_load8(&self, dst: &Register, addr: &Register, offset: i64, cb: &mut CodeBuilder) -> crate::Result<()> {
        let dst_rv = self.get_physical_register(dst)?;
        let addr_rv = self.get_physical_register(addr)?;
        if let Some(imm12) = self.as_i12(offset) {
            self.emit_lbu(cb, dst_rv, addr_rv, imm12);
        } else {
            let tmp = 5u8;
            self.emit_load_imm64(cb, tmp, offset);
            self.emit_add(cb, tmp, addr_rv, tmp);
            self.emit_lbu(cb, dst_rv, tmp, 0);
        }
        Ok(())
    }

    fn compile_store8(&self, addr: &Register, offset: i64, src: &Operand, cb: &mut CodeBuilder) -> crate::Result<()> {
        let addr_rv = self.get_physical_register(addr)?;
        let (base, eff_offset) = if let Some(imm12) = self.as_i12(offset) {
            (addr_rv, imm12)
        } else {
            let tmp = 5u8;
            self.emit_load_imm64(cb, tmp, offset);
            self.emit_add(cb, tmp, addr_rv, tmp);
            (tmp, 0)
        };

        match src {
            Operand::Register { id } => {
                let src_rv = self.get_physical_register(id)?;
                self.emit_sb(cb, src_rv, base, eff_offset);
            }
            Operand::Immediate { value } => {
                let tmp2 = 6u8;
                self.emit_load_imm64(cb, tmp2, *value);
                self.emit_sb(cb, tmp2, base, eff_offset);
            }
            _ => return Err(format!("store8 不支持的 src: {:?}", src).into()),
        }
        Ok(())
    }

    fn compile_store_pair(
        &self, addr: &Register, offset: i64, src1: &Register, src2: &Register, cb: &mut CodeBuilder,
    ) -> crate::Result<()> {
        self.compile_store64(addr, offset, &Operand::Register { id: *src1 }, cb)?;
        self.compile_store64(addr, offset + 8, &Operand::Register { id: *src2 }, cb)
    }

    fn compile_load_pair(
        &self, dst1: &Register, dst2: &Register, addr: &Register, offset: i64, cb: &mut CodeBuilder,
    ) -> crate::Result<()> {
        self.compile_load64(dst1, addr, offset, cb)?;
        self.compile_load64(dst2, addr, offset + 8, cb)
    }

    fn compile_alloc(
        &self, dst: &Register, size: usize, alignment: usize,
        allocation_type: &karte_lir::AllocationType, cb: &mut CodeBuilder,
    ) -> crate::Result<()> {
        match allocation_type {
            karte_lir::AllocationType::Heap => {
                let call = RuntimeCall::alloc(size, alignment);
                self.emit_runtime_call(cb, call, Some(dst))
            }
            _ => Err(format!("Alloc with unsupported allocation type: {:?}", allocation_type).into()),
        }
    }

    fn compile_free(&self, addr: &Register, cb: &mut CodeBuilder) -> crate::Result<()> {
        let call = RuntimeCall::free(*addr);
        self.emit_runtime_call(cb, call, None)
    }

    fn compile_retain(&self, value: &Register, cb: &mut CodeBuilder) -> crate::Result<()> {
        let call = RuntimeCall::retain(*value);
        self.emit_runtime_call(cb, call, None)
    }

    fn compile_release(&self, value: &Register, cb: &mut CodeBuilder) -> crate::Result<()> {
        let call = RuntimeCall::release(*value);
        self.emit_runtime_call(cb, call, None)
    }

    fn compile_safepoint(&self, cb: &mut CodeBuilder) -> crate::Result<()> {
        let call = RuntimeCall::gc_safepoint();
        self.emit_runtime_call(cb, call, None)
    }
}

// ============================================================================
// 运行时函数调用
// ============================================================================
impl RiscvCompiler {
    fn emit_runtime_call(
        &self, cb: &mut CodeBuilder, call: RuntimeCall, result: Option<&Register>,
    ) -> crate::Result<()> {
        let return_rv = self.map_register(0); // a0 = x10

        // 保存 caller-saved 寄存器到虚拟栈
        let (saved_regs, stack_space) = self.save_call_clobbered_registers(cb);

        // RISC-V 函数调用参数寄存器: a0(x10)-a7(x17)
        // 对应 Karte 编号 0-7
        let arg_regs_rv: [u8; 8] = [10, 11, 12, 13, 14, 15, 16, 17];

        for (idx, arg) in call.args.iter().enumerate() {
            if idx >= arg_regs_rv.len() {
                return Err(format!("runtime call 参数超过 {}", arg_regs_rv.len()).into());
            }
            let target_rv = arg_regs_rv[idx];
            match arg {
                RuntimeArg::Immediate(value) => {
                    self.emit_load_imm64(cb, target_rv, *value);
                }
                RuntimeArg::Register(reg) => {
                    let src_rv = self.get_physical_register(reg)?;
                    if src_rv != target_rv {
                        self.emit_addi(cb, target_rv, src_rv, 0);
                    }
                }
            }
        }

        // 调用运行时函数：通过数据内联加载函数指针
        // AUIPC t0, 0; LD t0, t0, 12; JALR ra, t0, 0; .quad <func_ptr>
        let t0 = 5u8;
        self.emit_auipc(cb, t0, 0);
        self.emit_ld(cb, t0, t0, 12);
        self.emit_jalr(cb, 1, t0, 0); // JALR ra, t0, 0
        // 内联函数指针（8 字节）
        // 使用 emit_label_address 机制记录 patch 位置
        let runtime_label = format!("__runtime_{}", call.intrinsic.name());
        cb.emit_label_address(&runtime_label);

        // 恢复 caller-saved 寄存器
        self.restore_call_clobbered_registers(cb, &saved_regs, stack_space);

        // 结果从 a0 移动到目标寄存器
        if let (Some(dst), true) = (result, call.expects_result()) {
            let dst_rv = self.get_physical_register(dst)?;
            if dst_rv != return_rv {
                self.emit_addi(cb, dst_rv, return_rv, 0);
            }
        }

        Ok(())
    }

    /// 保存 caller-saved 寄存器到虚拟栈
    fn save_call_clobbered_registers(&self, cb: &mut CodeBuilder) -> (Vec<u8>, usize) {
        // RISC-V caller-saved 寄存器（Karte 编号）:
        // a0-a7 (#0-7), t0(#8), t1(#9), t3-t6(#23-26)
        // 注意：不保存 ra(#27) 和 effect_sp(#12)，由调用框架处理
        let caller_saved_lir: Vec<u8> = vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 23, 24, 25, 26];

        if caller_saved_lir.is_empty() {
            return (caller_saved_lir, 0);
        }

        let stack_space = align_to(caller_saved_lir.len() * 8, 16);
        let vm_sp = self.map_register(10); // x2

        // 在虚拟栈上分配空间: sub vm_sp, vm_sp, stack_space
        self.emit_load_imm64(cb, 5, -(stack_space as i64)); // t0 = -stack_space
        self.emit_add(cb, vm_sp, vm_sp, 5);

        // 保存每个寄存器
        for (idx, &lir_reg) in caller_saved_lir.iter().enumerate() {
            let rv_reg = self.map_register(lir_reg);
            let offset = (idx * 8) as i32;
            self.emit_sd(cb, rv_reg, vm_sp, offset);
        }

        (caller_saved_lir, stack_space)
    }

    /// 从虚拟栈恢复 caller-saved 寄存器
    fn restore_call_clobbered_registers(&self, cb: &mut CodeBuilder, regs: &[u8], stack_space: usize) {
        if regs.is_empty() {
            return;
        }

        let vm_sp = self.map_register(10); // x2

        for (idx, &lir_reg) in regs.iter().enumerate() {
            let rv_reg = self.map_register(lir_reg);
            let offset = (idx * 8) as i32;
            self.emit_ld(cb, rv_reg, vm_sp, offset);
        }

        // 恢复虚拟栈: add vm_sp, vm_sp, stack_space
        self.emit_load_imm64(cb, 5, stack_space as i64);
        self.emit_add(cb, vm_sp, vm_sp, 5);
    }
}

// ============================================================================
// 函数序言和尾声
// ============================================================================
impl RiscvCompiler {
    fn is_entry_function(&self, name: &str, program: &LirProgram) -> bool {
        program.main_function.as_deref() == Some(name)
    }

    /// 获取当前函数使用的 callee-saved 寄存器（映射后的 RISC-V 编号）
    fn get_callee_saved_registers(&self) -> Vec<u8> {
        // 使用 RISC-V 目标的调用约定（而不是编译主机的 CallingConvention::standard()）
        let cc = CallingConvention::standard_riscv64();
        self.current_function_used_regs
            .iter()
            .filter(|&&reg| cc.is_callee_saved(reg) && reg != cc.stack_pointer && reg != cc.frame_pointer)
            .map(|&reg| self.map_register(reg))
            .collect()
    }

    /// 生成主函数序言
    fn emit_main_function_prologue(&self, cb: &mut CodeBuilder) -> crate::Result<()> {
        // RISC-V 主函数序言
        // 参数：a0(x10) = 虚拟栈顶地址, a1(x11) = 虚拟栈底地址
        //
        // 关键设计：vm_sp = x2(sp)，覆盖硬件 SP。
        // 在设置 vm_sp 之前，必须保存硬件 SP。

        let vm_sp = self.map_register(10); // x2
        let vm_fp = self.map_register(11); // x8
        let a0 = 10u8; // a0 = 参数1
        let a1 = 11u8; // a1 = 参数2

        // 步骤 1: 保存 callee-saved 到系统栈（使用当前硬件 SP = x2）
        // 系统栈帧大小 = 14 个寄存器 * 8 = 112，对齐到 16
        let sys_frame_size: i32 = 112;
        self.emit_addi(cb, 2, 2, -sys_frame_size); // sp -= 112

        // 保存 callee-saved 寄存器
        self.emit_sd(cb, 1, 2, 0);   // ra (x1)
        self.emit_sd(cb, 8, 2, 8);   // s0 (x8) = vm_fp
        self.emit_sd(cb, 9, 2, 16);  // s1 (x9)
        self.emit_sd(cb, 18, 2, 24); // s2 (x18)
        self.emit_sd(cb, 19, 2, 32); // s3 (x19)
        self.emit_sd(cb, 20, 2, 40); // s4 (x20)
        self.emit_sd(cb, 21, 2, 48); // s5 (x21)
        self.emit_sd(cb, 22, 2, 56); // s6 (x22)
        self.emit_sd(cb, 23, 2, 64); // s7 (x23)
        self.emit_sd(cb, 24, 2, 72); // s8 (x24)
        self.emit_sd(cb, 25, 2, 80); // s9 (x25)
        self.emit_sd(cb, 26, 2, 88); // s10 (x26)
        self.emit_sd(cb, 27, 2, 96); // s11 (x27)

        // 步骤 2: 保存修改后的硬件 SP 到 s2（s2 原值已保存到系统栈）
        // 当前 x2 = 原 SP - sys_frame_size
        self.emit_addi(cb, 18, 2, 0); // s2 = 当前 SP

        // 步骤 3: 设置 vm_sp 和 vm_fp
        // a0 → x2(vm_sp), a1 → x8(vm_fp)
        self.emit_addi(cb, 2, a0, 0);  // vm_sp = a0
        self.emit_addi(cb, 8, a1, 0);  // vm_fp = a1

        // 步骤 4: 在虚拟栈上保存系统 SP (s2 中保存了)
        // sub vm_sp, vm_sp, 16
        self.emit_addi(cb, vm_sp, vm_sp, -16);
        // sd s2, 0(vm_sp) — 保存系统 SP
        self.emit_sd(cb, 18, vm_sp, 0);
        // sd x0, 8(vm_sp) — 保留对齐
        self.emit_sd(cb, 0, vm_sp, 8);

        // 步骤 5: 为返回值槽分配空间
        self.emit_addi(cb, vm_sp, vm_sp, -16);
        self.emit_sd(cb, 0, vm_sp, 0);
        self.emit_sd(cb, 0, vm_sp, 8);

        // 步骤 6: 设置帧指针
        if self.current_stack_frame_size > 0 {
            self.emit_load_imm64(cb, 5, -(self.current_stack_frame_size as i64));
            self.emit_add(cb, vm_sp, vm_sp, 5);
            self.emit_addi(cb, vm_fp, vm_sp, 0);
            self.emit_load_imm64(cb, 5, self.current_stack_frame_size as i64);
            self.emit_add(cb, vm_fp, vm_fp, 5);
        } else {
            self.emit_addi(cb, vm_fp, vm_sp, 0);
        }

        Ok(())
    }

    /// 生成内部函数序言
    fn emit_internal_function_prologue(&self, cb: &mut CodeBuilder) -> crate::Result<()> {
        let vm_sp = self.map_register(10); // x2
        let vm_fp = self.map_register(11); // x8
        // 使用 t0(x5) 作为临时寄存器，避免覆盖 a0（参数传递寄存器）
        let tmp = self.map_register(8);    // x5(t0) 临时

        // 保存旧 vm_sp
        self.emit_addi(cb, tmp, vm_sp, 0);

        // 分配空间保存 FP 和 SP
        self.emit_addi(cb, vm_sp, vm_sp, -16);
        self.emit_sd(cb, vm_fp, vm_sp, 8);  // 保存 vm_fp
        self.emit_sd(cb, tmp, vm_sp, 0);    // 保存旧 vm_sp

        // 保存 callee-saved 寄存器
        let callee_saved = self.get_callee_saved_registers();
        for &reg in &callee_saved {
            self.emit_addi(cb, vm_sp, vm_sp, -16);
            self.emit_sd(cb, reg, vm_sp, 0);
        }

        // 分配栈帧空间
        if self.current_stack_frame_size > 0 {
            self.emit_load_imm64(cb, tmp, -(self.current_stack_frame_size as i64));
            self.emit_add(cb, vm_sp, vm_sp, tmp);
        }

        // 设置帧指针
        if self.current_stack_frame_size > 0 {
            self.emit_addi(cb, vm_fp, vm_sp, 0);
            self.emit_load_imm64(cb, tmp, self.current_stack_frame_size as i64);
            self.emit_add(cb, vm_fp, vm_fp, tmp);
        } else {
            self.emit_addi(cb, vm_fp, vm_sp, 0);
        }

        Ok(())
    }

    /// 生成内部函数尾声
    fn emit_internal_function_epilogue(&self, cb: &mut CodeBuilder) -> crate::Result<()> {
        let vm_sp = self.map_register(10); // x2
        let tmp = self.map_register(8);    // t0 = x5

        // 恢复 callee-saved（逆序）
        let callee_saved = self.get_callee_saved_registers();
        for &reg in callee_saved.iter().rev() {
            self.emit_ld(cb, reg, vm_sp, 0);
            self.emit_addi(cb, vm_sp, vm_sp, 16);
        }

        // 恢复 FP 和 SP
        self.emit_ld(cb, tmp, vm_sp, 8);    // tmp = old_fp
        self.emit_ld(cb, vm_sp, vm_sp, 0);  // vm_sp = old_sp
        self.emit_addi(cb, self.map_register(11), tmp, 0); // vm_fp = tmp

        Ok(())
    }

    /// 生成主函数尾声
    fn emit_main_function_epilogue(&self, cb: &mut CodeBuilder) -> crate::Result<()> {
        // 返回值已在 a0(x10) 中

        // 跳过返回值槽
        let vm_sp = self.map_register(10); // x2
        self.emit_addi(cb, vm_sp, vm_sp, 16);

        // 从虚拟栈加载系统 SP（保存在 s2 中）
        // 但我们保存在虚拟栈 vm_sp+0 位置
        // 实际上在 prologue 中，系统 SP 保存到了虚拟栈的第一个位置
        // 现在跳过返回值槽后，vm_sp 指向保存系统 SP 的位置
        // 加载系统 SP 到 x2
        self.emit_ld(cb, 2, vm_sp, 0); // 从虚拟栈恢复系统 SP

        // 现在从系统栈恢复 callee-saved
        self.emit_ld(cb, 1, 2, 0);   // ra
        self.emit_ld(cb, 8, 2, 8);   // s0
        self.emit_ld(cb, 9, 2, 16);  // s1
        self.emit_ld(cb, 18, 2, 24); // s2
        self.emit_ld(cb, 19, 2, 32); // s3
        self.emit_ld(cb, 20, 2, 40); // s4
        self.emit_ld(cb, 21, 2, 48); // s5
        self.emit_ld(cb, 22, 2, 56); // s6
        self.emit_ld(cb, 23, 2, 64); // s7
        self.emit_ld(cb, 24, 2, 72); // s8
        self.emit_ld(cb, 25, 2, 80); // s9
        self.emit_ld(cb, 26, 2, 88); // s10
        self.emit_ld(cb, 27, 2, 96); // s11

        // 释放系统栈帧
        self.emit_addi(cb, 2, 2, 112);

        // 返回到调用者
        self.emit_jalr(cb, 0, 1, 0); // JALR x0, ra, 0
        Ok(())
    }
}

fn align_to(value: usize, alignment: usize) -> usize {
    ((value + alignment - 1) / alignment) * alignment
}

/// 计算函数需要的栈帧空间
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
    align_to(max_offset as usize, 16)
}

// ============================================================================
// 实现 JitCompiler trait
// ============================================================================
impl JitCompiler for RiscvCompiler {
    fn compile_function(
        &mut self,
        function: &LirFunction,
        program: &LirProgram,
    ) -> crate::Result<CompiledFunction> {
        if self.debug_mode {
            log::debug!("RISC-V: 开始编译函数 '{}'", function.name);
        }

        self.current_function_used_regs = function.get_used_regs().to_vec();

        // 使用 RISC-V 目标的调用约定（而不是编译主机的 CallingConvention::standard()）
        let cc = CallingConvention::standard_riscv64();
        let _computed_frame = compute_stack_frame_size(function, cc.frame_pointer);
        self.current_stack_frame_size = 0;
        self.stack_frame_size_for_epilogue = function.stack_frame_size as usize;

        let mut code_builder = CodeBuilder::with_target_arch(TargetArch::Riscv64);

        let function_label = format!("func_{}", function.name);
        code_builder.define_label(&function_label)?;

        let is_main_function = self.is_entry_function(&function.name, program);

        if is_main_function {
            self.emit_main_function_prologue(&mut code_builder)?;
        }

        let has_first_label = matches!(function.instructions.first(), Some(Instruction::Label { .. }));
        if has_first_label {
            if let Some(Instruction::Label { id, .. }) = function.instructions.first() {
                code_builder.define_label(&format!("label_{}", id.0))?;
            }
        }

        if !is_main_function {
            self.emit_internal_function_prologue(&mut code_builder)?;
        }

        let skip = if has_first_label { 1 } else { 0 };
        for instruction in function.instructions.iter().skip(skip) {
            self.compile_instruction(instruction, &mut code_builder, program, is_main_function)?;
        }

        let labels = code_builder.exported_labels().clone();
        let pending_jumps = code_builder.exported_pending_jumps().clone();
        let pending_label_addresses = code_builder.exported_pending_label_addresses().clone();
        let pending_adrs = code_builder.exported_pending_adrs().clone();

        let machine_code = code_builder.finalize()?;

        let mut compiled_function = CompiledFunction::new(function.name.clone(), machine_code, 0);
        compiled_function.labels = labels;
        compiled_function.pending_jumps = pending_jumps;
        compiled_function.pending_label_addresses = pending_label_addresses;
        compiled_function.pending_adrs = pending_adrs;

        log::info!(
            "RISC-V: 函数 '{}' 编译完成，机器码大小: {} 字节",
            function.name,
            compiled_function.code_size()
        );

        Ok(compiled_function)
    }

    fn target_architecture(&self) -> &'static str {
        "riscv64"
    }

    fn get_register_mapping(&self) -> &HashMap<Register, u8> {
        static EMPTY: std::sync::OnceLock<HashMap<Register, u8>> = std::sync::OnceLock::new();
        EMPTY.get_or_init(HashMap::new)
    }

    fn supports_debug_info(&self) -> bool {
        true
    }

    fn get_calling_convention(&self) -> CallingConventionInfo {
        CallingConventionInfo {
            parameter_registers: vec![0, 1, 2, 3, 4, 5, 6, 7],
            return_register: 0,
            stack_pointer: 10,
            frame_pointer: 11,
            caller_saved: vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 23, 24, 25, 26, 27],
            callee_saved: vec![11, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22],
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

        let mut compiler = RiscvCompiler::new(true).unwrap();
        let result = compiler.compile_function(&program.functions["test_basic"], &program);
        assert!(result.is_ok(), "编译失败: {:?}", result.err());

        let compiled = result.unwrap();
        assert!(compiled.code_size() > 0, "编译后的机器码为空");
    }

    #[test]
    fn test_register_mapping() {
        let compiler = RiscvCompiler::new(true).unwrap();
        assert_eq!(compiler.map_register(0), 10);  // a0
        assert_eq!(compiler.map_register(7), 17);  // a7
        assert_eq!(compiler.map_register(10), 2);  // sp (vm_sp)
        assert_eq!(compiler.map_register(11), 8);  // s0/fp (vm_fp)
        assert_eq!(compiler.map_register(27), 1);  // ra
    }

    #[test]
    fn test_r_type_encoding() {
        let compiler = RiscvCompiler::new(true).unwrap();
        let mut cb = CodeBuilder::with_target_arch(TargetArch::Riscv64);

        // ADD x5, x10, x11
        compiler.emit_add(&mut cb, 5, 10, 11);

        let code = cb.finalize().unwrap();
        let bytes = code.as_bytes();
        assert_eq!(bytes.len(), 4);

        let instr = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        // ADD: funct7=0, rs2=11(0xB), rs1=10(0xA), funct3=0, rd=5, opcode=0x33
        let expected = (0x0Bu32 << 20) | (0x0Au32 << 15) | (0x05 << 7) | 0x33;
        assert_eq!(instr, expected);
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
            addr: Register::Physical(10),
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

        let mut compiler = RiscvCompiler::new(true).unwrap();
        let result = compiler.compile_function(&program.functions["test_store_pair"], &program);
        assert!(result.is_ok(), "StorePair 编译失败: {:?}", result.err());
    }
}
