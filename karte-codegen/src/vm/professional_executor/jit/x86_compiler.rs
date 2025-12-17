//! x86-64 JIT编译器
//!
//! 将LIR指令编译为x86-64机器码

use super::code_buffer::{CodeBuilder, JumpType};
use super::compiler_trait::*;
use super::ffi::{RuntimeArg, RuntimeCall};
use karte_lir::{Instruction, LirFunction, LirProgram, Operand, Register};
use std::collections::HashMap;

/// x86-64编译器
#[derive(Debug)]
pub struct X86Compiler {
    /// 寄存器映射 (虚拟寄存器 -> 物理寄存器)
    register_mapping: HashMap<Register, u8>,
    /// 调用约定
    calling_convention: CallingConventionInfo,
    /// 调试模式
    debug_mode: bool,
    /// 唯一label计数器（用于在编译时生成跳转目标）
    unique_label_counter: usize,
    /// 当前函数使用的 callee-saved 寄存器列表
    current_function_use_regs: Vec<u8>,
    /// 当前编译的函数名（用于生成唯一label）
    current_function_name: String,
}

impl X86Compiler {
    /// 创建新的x86编译器
    pub fn new(debug_mode: bool) -> Result<Self, String> {
        let mut compiler = Self {
            register_mapping: HashMap::new(),
            calling_convention: Self::create_calling_convention(),
            debug_mode: true, // 强制启用调试模式
            unique_label_counter: 0,
            current_function_use_regs: Vec::new(),
            current_function_name: String::new(),
        };

        // 初始化寄存器映射
        compiler.initialize_register_mapping();

        Ok(compiler)
    }

    /// 创建x86-64调用约定
    fn create_calling_convention() -> CallingConventionInfo {
        CallingConventionInfo {
            parameter_registers: vec![
                X86Register::RDI as u8, // 第一个参数
                X86Register::RSI as u8, // 第二个参数
                X86Register::RDX as u8, // 第三个参数
                X86Register::RCX as u8, // 第四个参数
            ],
            return_register: X86Register::RAX as u8,
            stack_pointer: X86Register::RSP as u8,
            frame_pointer: X86Register::RBP as u8,
            caller_saved: vec![
                X86Register::RAX as u8,
                X86Register::RCX as u8,
                X86Register::RDX as u8,
                X86Register::RSI as u8,
                X86Register::RDI as u8,
                X86Register::R8 as u8,
                X86Register::R9 as u8,
                X86Register::R10 as u8,
                X86Register::R11 as u8,
            ],
            callee_saved: vec![
                X86Register::RBX as u8,
                X86Register::RBP as u8,
                X86Register::R12 as u8,
                X86Register::R13 as u8,
                X86Register::R14 as u8,
                X86Register::R15 as u8,
            ],
        }
    }

    /// 初始化寄存器映射
    fn initialize_register_mapping(&mut self) {
        // 🔧 修复：建立虚拟寄存器到物理寄存器的映射
        // 确保r0映射到RAX（返回值寄存器），避免与函数序言冲突
        for i in 0..8 {
            let virtual_reg = Register::Virtual(i);
            let physical_reg = match i {
                0 => X86Register::RAX as u8, // r0 -> rax (返回值寄存器)
                1 => X86Register::RBX as u8, // r1 -> rbx
                2 => X86Register::RCX as u8, // r2 -> rcx
                3 => X86Register::RDX as u8, // r3 -> rdx
                4 => X86Register::R8 as u8,  // r4 -> r8
                5 => X86Register::R9 as u8,  // r5 -> r9
                6 => X86Register::R10 as u8, // r6 -> r10 (虚拟机栈指针)
                7 => X86Register::R11 as u8, // r7 -> r11 (虚拟机帧指针)
                _ => X86Register::R12 as u8,
            };
            self.register_mapping.insert(virtual_reg, physical_reg);
        }

        // 🔧 修复：物理寄存器映射，支持0..32范围（对应AArch64寄存器编号）
        // x86_64只有16个通用寄存器，需要将32个物理寄存器映射到它们
        for i in 0..32 {
            let physical_reg = Register::Physical(i);
            let x86_reg = match i {
                // 直接映射的寄存器 (0-15)
                0 => X86Register::RAX as u8,  // r0 (返回值)
                1 => X86Register::RCX as u8,  // r1
                2 => X86Register::RDX as u8,  // r2
                3 => X86Register::RBX as u8,  // r3
                4 => X86Register::R8 as u8,   // r4
                5 => X86Register::R9 as u8,   // r5
                6 => X86Register::R10 as u8,  // r6 (虚拟机栈指针)
                7 => X86Register::R11 as u8,  // r7 (虚拟机帧指针)
                8 => X86Register::R12 as u8,  // r8
                9 => X86Register::R13 as u8,  // r9
                10 => X86Register::R14 as u8, // r10
                11 => X86Register::R15 as u8, // r11
                12 => X86Register::RSI as u8, // r12
                13 => X86Register::RDI as u8, // r13
                14 => X86Register::R12 as u8, // r14 (重用)
                15 => X86Register::R13 as u8, // r15 (重用)
                // 对于AArch64的高位寄存器(16-28)，映射到可用的x86寄存器
                16..=28 => X86Register::R14 as u8, // r16-r28 -> r14 (重用，实际不常用)
                29 => X86Register::RBP as u8,      // r29 -> rbp (帧指针)
                30 => X86Register::R15 as u8,      // r30 (链接寄存器) -> r15
                31 => X86Register::RSP as u8,      // r31 -> rsp (栈指针)
                _ => unreachable!(),
            };
            self.register_mapping.insert(physical_reg, x86_reg);
        }
    }

    /// 获取寄存器的物理编号
    fn get_physical_register(&self, reg: &Register) -> Result<u8, String> {
        self.register_mapping
            .get(reg)
            .copied()
            .ok_or_else(|| format!("未映射的寄存器: {:?}", reg))
    }

    /// 编译单个指令
    fn compile_instruction(
        &mut self,
        instruction: &Instruction,
        code_builder: &mut CodeBuilder,
        _program: &LirProgram,
    ) -> Result<(), String> {
        if self.debug_mode {
            println!("编译指令: {:?}", instruction);
        }

        match instruction {
            Instruction::Move { dst, src, .. } => self.compile_move(dst, src, code_builder),
            Instruction::Add {
                dst, src1, src2, ..
            } => self.compile_add(dst, src1, src2, code_builder),
            Instruction::Sub {
                dst, src1, src2, ..
            } => self.compile_sub(dst, src1, src2, code_builder),
            Instruction::Mul {
                dst, src1, src2, ..
            } => self.compile_mul(dst, src1, src2, code_builder),
            Instruction::Div {
                dst, src1, src2, ..
            } => self.compile_div(dst, src1, src2, code_builder),
            Instruction::Compare { src1, src2, .. } => {
                self.compile_compare(src1, src2, code_builder)
            }
            Instruction::Jump { target, .. } => self.compile_jump(target, code_builder),
            Instruction::JumpEqual { target, .. } => {
                self.compile_conditional_jump(JumpType::ConditionalEqual, target, code_builder)
            }
            Instruction::JumpNotEqual { target, .. } => {
                self.compile_conditional_jump(JumpType::ConditionalNotEqual, target, code_builder)
            }
            Instruction::JumpLess { target, .. } => {
                self.compile_conditional_jump(JumpType::ConditionalLess, target, code_builder)
            }
            Instruction::JumpLessEqual { target, .. } => {
                self.compile_conditional_jump(JumpType::ConditionalLessEqual, target, code_builder)
            }
            Instruction::JumpGreater { target, .. } => {
                self.compile_conditional_jump(JumpType::ConditionalGreater, target, code_builder)
            }
            Instruction::JumpGreaterEqual { target, .. } => {
                self.compile_conditional_jump(JumpType::ConditionalGreaterEqual, target, code_builder)
            }
            Instruction::Call { target, .. } => self.compile_call(target, code_builder),
            Instruction::JumpIndirect {
                function_register, ..
            } => self.compile_jump_indirect(function_register, code_builder),
            Instruction::JumpRegister {
                target_register, ..
            } => self.compile_jump_register(target_register, code_builder),
            Instruction::Return { value, .. } => self.compile_return(value.as_ref(), code_builder),
            Instruction::Label { id, .. } => {
                let label_name = format!("label_{}", id.0);
                code_builder.define_label(&label_name)?;
                Ok(())
            }
            Instruction::Load64 {
                dst, addr, offset, ..
            } => self.compile_load64(dst, addr, *offset, code_builder),
            Instruction::Store64 {
                addr, offset, src, ..
            } => self.compile_store64(addr, *offset, src, code_builder),
            Instruction::LoadPair {
                dst1,
                dst2,
                addr,
                offset,
                ..
            } => {
                // LoadPair是AArch64特有的，在x86上分解为两个Load64
                self.compile_load64(dst1, addr, *offset, code_builder)?;
                self.compile_load64(dst2, addr, *offset + 8, code_builder)
            }
            Instruction::StorePair {
                addr,
                offset,
                src1,
                src2,
                ..
            } => {
                // StorePair是AArch64特有的，在x86上分解为两个Store64
                let src1_operand = Operand::Register { id: src1.clone() };
                let src2_operand = Operand::Register { id: src2.clone() };
                self.compile_store64(addr, *offset, &src1_operand, code_builder)?;
                self.compile_store64(addr, *offset + 8, &src2_operand, code_builder)
            }
            Instruction::Alloc {
                dst,
                size,
                alignment,
                allocation_type,
                ..
            } => self.compile_alloc(dst, *size, *alignment, allocation_type, code_builder),
            Instruction::Free { addr, .. } => self.compile_free(addr, code_builder),
            Instruction::Retain { value, .. } => self.compile_retain(value, code_builder),
            Instruction::Release { value, .. } => self.compile_release(value, code_builder),
            Instruction::Safepoint { .. } => self.compile_safepoint(code_builder),
            Instruction::Nop { .. } => {
                // NOP指令
                code_builder.emit_byte(0x90);
                Ok(())
            }
            _ => Err(format!("不支持的指令类型: {:?}", instruction)),
        }
    }
}

/// x86-64寄存器枚举
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum X86Register {
    RAX = 0,
    RCX = 1,
    RDX = 2,
    RBX = 3,
    RSP = 4, // 栈指针
    RBP = 5, // 帧指针
    RSI = 6,
    RDI = 7,
    R8 = 8,
    R9 = 9,
    R10 = 10,
    R11 = 11,
    R12 = 12,
    R13 = 13,
    R14 = 14,
    R15 = 15,
}

impl X86Register {
    /// 检查是否需要REX前缀
    fn needs_rex_prefix(self) -> bool {
        (self as u8) >= 8
    }

    /// 获取ModR/M字段中的寄存器编码
    fn modrm_encoding(self) -> u8 {
        (self as u8) & 0x7
    }
}

// 继续X86Compiler的实现 - 指令编译方法
impl X86Compiler {
    /// 编译mov指令
    fn compile_move(
        &mut self,
        dst: &Register,
        src: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> Result<(), String> {
        let dst_reg = self.get_physical_register(dst)?;

        match src {
            Operand::Register { id } => {
                let src_reg = self.get_physical_register(id)?;
                // mov dst, src (64位)
                self.emit_mov_reg_reg(code_builder, dst_reg, src_reg);
            }
            Operand::Immediate { value } => {
                // mov dst, imm64
                self.emit_mov_reg_imm64(code_builder, dst_reg, *value);
            }
            Operand::Label { id } => {
                // mov dst, label - 加载标签地址到寄存器
                let label_name = format!("label_{}", id.0);

                // 使用CodeBuilder的标签地址功能
                code_builder.emit_label_address(&label_name);

                // 然后从内存加载地址到寄存器
                // 使用简单的内存加载：mov dst, [rip + 0]
                self.emit_mov_reg_rip_rel(code_builder, dst_reg, 0);
            }
            _ => {
                return Err(format!("mov指令不支持的操作数类型: {:?}", src));
            }
        }
        Ok(())
    }

    /// 编译add指令
    fn compile_add(
        &mut self,
        dst: &Register,
        src1: &Operand,
        src2: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> Result<(), String> {
        let dst_reg = self.get_physical_register(dst)?;

        // 先将src1移动到dst
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
                return Err(format!("add指令不支持的src1类型: {:?}", src1));
            }
        }

        // 然后将src2加到dst
        match src2 {
            Operand::Register { id } => {
                let src2_reg = self.get_physical_register(id)?;
                self.emit_add_reg_reg(code_builder, dst_reg, src2_reg);
            }
            Operand::Immediate { value } => {
                self.emit_add_reg_imm32(code_builder, dst_reg, *value as i32);
            }
            _ => {
                return Err(format!("add指令不支持的src2类型: {:?}", src2));
            }
        }
        Ok(())
    }

    /// 编译sub指令
    fn compile_sub(
        &mut self,
        dst: &Register,
        src1: &Operand,
        src2: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> Result<(), String> {
        let dst_reg = self.get_physical_register(dst)?;

        // 先将src1移动到dst
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
                return Err(format!("sub指令不支持的src1类型: {:?}", src1));
            }
        }

        // 然后从dst减去src2
        match src2 {
            Operand::Register { id } => {
                let src2_reg = self.get_physical_register(id)?;
                self.emit_sub_reg_reg(code_builder, dst_reg, src2_reg);
            }
            Operand::Immediate { value } => {
                self.emit_sub_reg_imm32(code_builder, dst_reg, *value as i32);
            }
            _ => {
                return Err(format!("sub指令不支持的src2类型: {:?}", src2));
            }
        }
        Ok(())
    }

    /// 编译mul指令
    fn compile_mul(
        &mut self,
        dst: &Register,
        src1: &Operand,
        src2: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> Result<(), String> {
        let dst_reg = self.get_physical_register(dst)?;

        // 将src1移动到dst
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
                return Err(format!("mul指令不支持的src1类型: {:?}", src1));
            }
        }

        // 使用imul指令乘以src2
        match src2 {
            Operand::Register { id } => {
                let src2_reg = self.get_physical_register(id)?;
                self.emit_imul_reg_reg(code_builder, dst_reg, src2_reg);
            }
            Operand::Immediate { value } => {
                self.emit_imul_reg_imm32(code_builder, dst_reg, *value as i32);
            }
            _ => {
                return Err(format!("mul指令不支持的src2类型: {:?}", src2));
            }
        }
        Ok(())
    }

    /// 编译div指令 (有符号除法)
    fn compile_div(
        &mut self,
        dst: &Register,
        src1: &Operand,
        src2: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> Result<(), String> {
        let dst_reg = self.get_physical_register(dst)?;
        let rax = X86Register::RAX as u8;
        let rdx = X86Register::RDX as u8;

        // x86-64除法使用IDIV指令，被除数在RDX:RAX中，除数在r/m64中
        // 商存储在RAX，余数存储在RDX
        
        // 1. 保存src2如果它在RAX或RDX中（因为CQO会覆盖RDX）
        let src2_in_tmp = match src2 {
            Operand::Register { id } => {
                let src2_reg = self.get_physical_register(id)?;
                if src2_reg == rax || src2_reg == rdx {
                    // src2在RAX或RDX中，需要先保存到临时寄存器
                    let tmp = X86Register::R8 as u8;
                    self.emit_mov_reg_reg(code_builder, tmp, src2_reg);
                    Some(tmp)
                } else {
                    None
                }
            }
            _ => None,
        };
        
        match src1 {
            Operand::Register { id } => {
                let src1_reg = self.get_physical_register(id)?;
                if src1_reg != rax {
                    self.emit_mov_reg_reg(code_builder, rax, src1_reg);
                }
            }
            Operand::Immediate { value } => {
                self.emit_mov_reg_imm64(code_builder, rax, *value);
            }
            _ => {
                return Err(format!("div指令不支持的src1类型: {:?}", src1));
            }
        }

        // 2. 符号扩展RAX到RDX:RAX (CQO指令)
        // REX.W + 99: CQO
        self.emit_rex_prefix(code_builder, true, 0, 0, 0);
        code_builder.emit_byte(0x99);

        // 3. 除以src2
        if let Some(tmp_reg) = src2_in_tmp {
            // src2已经保存在临时寄存器中
            self.emit_idiv_reg(code_builder, tmp_reg);
        } else {
            match src2 {
                Operand::Register { id } => {
                    let src2_reg = self.get_physical_register(id)?;
                    self.emit_idiv_reg(code_builder, src2_reg);
                }
                Operand::Immediate { value } => {
                    // 立即数需要先加载到寄存器
                    let tmp_reg = X86Register::R8 as u8;
                    self.emit_mov_reg_imm64(code_builder, tmp_reg, *value);
                    self.emit_idiv_reg(code_builder, tmp_reg);
                }
                _ => {
                    return Err(format!("div指令不支持的src2类型: {:?}", src2));
                }
            }
        }

        // 4. 将结果从RAX移动到dst
        if dst_reg != rax {
            self.emit_mov_reg_reg(code_builder, dst_reg, rax);
        }

        Ok(())
    }

    /// 编译比较指令
    fn compile_compare(
        &mut self,
        src1: &Operand,
        src2: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> Result<(), String> {
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
                ));
            }
        }
        Ok(())
    }

    /// 编译无条件跳转
    fn compile_jump(
        &mut self,
        target: &karte_lir::LabelId,
        code_builder: &mut CodeBuilder,
    ) -> Result<(), String> {
        let label_name = format!("label_{}", target.0);
        code_builder.emit_jump(JumpType::Unconditional, &label_name);
        Ok(())
    }

    /// 编译条件跳转
    fn compile_conditional_jump(
        &mut self,
        jump_type: JumpType,
        target: &karte_lir::LabelId,
        code_builder: &mut CodeBuilder,
    ) -> Result<(), String> {
        let label_name = format!("label_{}", target.0);
        code_builder.emit_jump(jump_type, &label_name);
        Ok(())
    }

    /// 编译函数调用
    fn compile_call(
        &mut self,
        target: &karte_lir::LabelId,
        code_builder: &mut CodeBuilder,
    ) -> Result<(), String> {
        let label_name = format!("label_{}", target.0);
        code_builder.emit_call(&label_name);
        Ok(())
    }

    /// 编译间接函数调用指令（call rax - 间接函数调用）
    fn compile_jump_indirect(
        &mut self,
        function_register: &Register,
        code_builder: &mut CodeBuilder,
    ) -> Result<(), String> {
        let function_reg = self.get_physical_register(function_register)?;

        // call rax - FF D0 (间接调用)
        // 对于x86-64，我们需要根据寄存器生成不同的指令
        match function_reg {
            0 => {
                // call rax - FF D0
                code_builder.emit_bytes(&[0xFF, 0xD0]);
            }
            1 => {
                // call rcx - FF D1
                code_builder.emit_bytes(&[0xFF, 0xD1]);
            }
            2 => {
                // call rdx - FF D2
                code_builder.emit_bytes(&[0xFF, 0xD2]);
            }
            3 => {
                // call rbx - FF D3
                code_builder.emit_bytes(&[0xFF, 0xD3]);
            }
            4 => {
                // call rsp - FF D4
                code_builder.emit_bytes(&[0xFF, 0xD4]);
            }
            5 => {
                // call rbp - FF D5
                code_builder.emit_bytes(&[0xFF, 0xD5]);
            }
            6 => {
                // call rsi - FF D6
                code_builder.emit_bytes(&[0xFF, 0xD6]);
            }
            7 => {
                // call rdi - FF D7
                code_builder.emit_bytes(&[0xFF, 0xD7]);
            }
            8 => {
                // call r8 - 41 FF D0
                code_builder.emit_bytes(&[0x41, 0xFF, 0xD0]);
            }
            9 => {
                // call r9 - 41 FF D1
                code_builder.emit_bytes(&[0x41, 0xFF, 0xD1]);
            }
            10 => {
                // call r10 - 41 FF D2
                code_builder.emit_bytes(&[0x41, 0xFF, 0xD2]);
            }
            11 => {
                // call r11 - 41 FF D3
                code_builder.emit_bytes(&[0x41, 0xFF, 0xD3]);
            }
            12 => {
                // call r12 - 41 FF D4
                code_builder.emit_bytes(&[0x41, 0xFF, 0xD4]);
            }
            13 => {
                // call r13 - 41 FF D5
                code_builder.emit_bytes(&[0x41, 0xFF, 0xD5]);
            }
            14 => {
                // call r14 - 41 FF D6
                code_builder.emit_bytes(&[0x41, 0xFF, 0xD6]);
            }
            15 => {
                // call r15 - 41 FF D7
                code_builder.emit_bytes(&[0x41, 0xFF, 0xD7]);
            }
            _ => {
                return Err(format!("不支持的寄存器: {}", function_reg));
            }
        }
        Ok(())
    }

    /// 编译寄存器跳转指令（jmp rax - 无链接跳转）
    fn compile_jump_register(
        &mut self,
        target_register: &Register,
        code_builder: &mut CodeBuilder,
    ) -> Result<(), String> {
        let reg = self.get_physical_register(target_register)?;

        // jmp rax - FF E0 (无链接跳转)
        // 对于x86-64，我们需要根据寄存器生成不同的指令
        match reg {
            0 => {
                // jmp rax - FF E0
                code_builder.emit_bytes(&[0xFF, 0xE0]);
            }
            1 => {
                // jmp rcx - FF E1
                code_builder.emit_bytes(&[0xFF, 0xE1]);
            }
            2 => {
                // jmp rdx - FF E2
                code_builder.emit_bytes(&[0xFF, 0xE2]);
            }
            3 => {
                // jmp rbx - FF E3
                code_builder.emit_bytes(&[0xFF, 0xE3]);
            }
            4 => {
                // jmp rsp - FF E4
                code_builder.emit_bytes(&[0xFF, 0xE4]);
            }
            5 => {
                // jmp rbp - FF E5
                code_builder.emit_bytes(&[0xFF, 0xE5]);
            }
            6 => {
                // jmp rsi - FF E6
                code_builder.emit_bytes(&[0xFF, 0xE6]);
            }
            7 => {
                // jmp rdi - FF E7
                code_builder.emit_bytes(&[0xFF, 0xE7]);
            }
            8 => {
                // jmp r8 - 41 FF E0
                code_builder.emit_bytes(&[0x41, 0xFF, 0xE0]);
            }
            9 => {
                // jmp r9 - 41 FF E1
                code_builder.emit_bytes(&[0x41, 0xFF, 0xE1]);
            }
            10 => {
                // jmp r10 - 41 FF E2
                code_builder.emit_bytes(&[0x41, 0xFF, 0xE2]);
            }
            11 => {
                // jmp r11 - 41 FF E3
                code_builder.emit_bytes(&[0x41, 0xFF, 0xE3]);
            }
            12 => {
                // jmp r12 - 41 FF E4
                code_builder.emit_bytes(&[0x41, 0xFF, 0xE4]);
            }
            13 => {
                // jmp r13 - 41 FF E5
                code_builder.emit_bytes(&[0x41, 0xFF, 0xE5]);
            }
            14 => {
                // jmp r14 - 41 FF E6
                code_builder.emit_bytes(&[0x41, 0xFF, 0xE6]);
            }
            15 => {
                // jmp r15 - 41 FF E7
                code_builder.emit_bytes(&[0x41, 0xFF, 0xE7]);
            }
            _ => {
                return Err(format!("不支持的寄存器: {}", reg));
            }
        }
        Ok(())
    }

    /// 编译返回指令
    fn compile_return(
        &mut self,
        value: Option<&Register>,
        code_builder: &mut CodeBuilder,
    ) -> Result<(), String> {
        // 🔧 修复：只移动返回值到RAX，不生成ret指令
        // 如果有返回值，将其移动到RAX
        if let Some(reg) = value {
            let src_reg = self.get_physical_register(reg)?;
            let rax = X86Register::RAX as u8;
            if src_reg != rax {
                self.emit_mov_reg_reg(code_builder, rax, src_reg);
            }
        }

        // 🔧 修复：不在这里生成ret指令，让函数尾声处理
        // ret指令会在函数尾声生成
        Ok(())
    }

    /// 编译64位加载指令
    fn compile_load64(
        &mut self,
        dst: &Register,
        addr: &Register,
        offset: i64,
        code_builder: &mut CodeBuilder,
    ) -> Result<(), String> {
        let dst_reg = self.get_physical_register(dst)?;
        let addr_reg = self.get_physical_register(addr)?;
        self.emit_mov_reg_mem(code_builder, dst_reg, addr_reg, offset as i32);
        Ok(())
    }

    /// 编译64位存储指令
    fn compile_store64(
        &mut self,
        addr: &Register,
        offset: i64,
        src: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> Result<(), String> {
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
                // store64 [addr + offset], label - 存储标签地址
                let label_name = format!("label_{}", id.0);

                // 使用CodeBuilder的标签地址功能
                code_builder.emit_store_label_address(addr_reg, offset, &label_name);
            }
            _ => {
                return Err(format!("store64指令不支持的src类型: {:?}", src));
            }
        }
        Ok(())
    }

    fn compile_alloc(
        &mut self,
        dst: &Register,
        size: usize,
        alignment: usize,
        allocation_type: &karte_lir::AllocationType,
        code_builder: &mut CodeBuilder,
    ) -> Result<(), String> {
        match allocation_type {
            karte_lir::AllocationType::Heap => {
                let call = RuntimeCall::alloc(size, alignment);
                self.emit_runtime_call(code_builder, call, Some(dst))
            }
            _ => {
                // 目前的JIT仅支持堆分配，其它类型暂未使用
                Err(format!(
                    "Alloc instruction with unsupported allocation type: {:?}",
                    allocation_type
                ))
            }
        }
    }

    fn compile_free(
        &mut self,
        addr: &Register,
        code_builder: &mut CodeBuilder,
    ) -> Result<(), String> {
        let call = RuntimeCall::free(*addr);
        self.emit_runtime_call(code_builder, call, None)
    }

    fn compile_retain(
        &mut self,
        value: &Register,
        code_builder: &mut CodeBuilder,
    ) -> Result<(), String> {
        let call = RuntimeCall::retain(*value);
        self.emit_runtime_call(code_builder, call, None)
    }

    fn compile_release(
        &mut self,
        value: &Register,
        code_builder: &mut CodeBuilder,
    ) -> Result<(), String> {
        let call = RuntimeCall::release(*value);
        self.emit_runtime_call(code_builder, call, None)
    }

    fn compile_safepoint(&mut self, code_builder: &mut CodeBuilder) -> Result<(), String> {
        // GC 安全点：调用运行时函数
        let call = RuntimeCall::gc_safepoint();
        self.emit_runtime_call(code_builder, call, None)
    }

    fn emit_runtime_call(
        &mut self,
        code_builder: &mut CodeBuilder,
        call: RuntimeCall,
        result: Option<&Register>,
    ) -> Result<(), String> {
        let return_reg = X86Register::RAX as u8;
        let exclude: Vec<u8> = if result.is_some() && call.expects_result() {
            vec![return_reg]
        } else {
            Vec::new()
        };
        let (saved_regs, stack_space) = self.save_call_clobbered_registers(code_builder, &exclude);

        let arg_regs = [
            X86Register::RDI as u8,
            X86Register::RSI as u8,
            X86Register::RDX as u8,
            X86Register::RCX as u8,
            X86Register::R8 as u8,
            X86Register::R9 as u8,
        ];

        for (idx, arg) in call.args.iter().enumerate() {
            if idx >= arg_regs.len() {
                return Err(format!(
                    "runtime call {} 超过支持的参数数量(最多 {})",
                    call.intrinsic.name(),
                    arg_regs.len()
                ));
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

    // x86-64指令编码实现
    /// 生成REX前缀
    fn emit_rex_prefix(&self, code_builder: &mut CodeBuilder, w: bool, r: u8, x: u8, b: u8) {
        let rex = 0x40
            | (if w { 0x08 } else { 0x00 })
            | ((r & 0x08) >> 1)
            | ((x & 0x08) >> 2)
            | ((b & 0x08) >> 3);
        code_builder.emit_byte(rex);
    }

    /// 生成ModR/M字节
    fn emit_modrm(&self, code_builder: &mut CodeBuilder, mode: u8, reg: u8, rm: u8) {
        let modrm = (mode << 6) | ((reg & 0x07) << 3) | (rm & 0x07);
        code_builder.emit_byte(modrm);
    }

    /// mov reg, reg (64位)
    fn emit_mov_reg_reg(&self, code_builder: &mut CodeBuilder, dst: u8, src: u8) {
        // REX.W + 89 /r: MOV r/m64, r64
        self.emit_rex_prefix(code_builder, true, src, 0, dst);
        code_builder.emit_byte(0x89);
        self.emit_modrm(code_builder, 0b11, src, dst);
    }

    fn emit_sub_rsp_imm(&self, code_builder: &mut CodeBuilder, imm: i32) {
        self.emit_rex_prefix(code_builder, true, 0, 0, X86Register::RSP as u8);
        code_builder.emit_byte(0x81);
        self.emit_modrm(code_builder, 0b11, 0b101, X86Register::RSP as u8);
        code_builder.emit_i32(imm);
    }

    fn emit_add_rsp_imm(&self, code_builder: &mut CodeBuilder, imm: i32) {
        self.emit_rex_prefix(code_builder, true, 0, 0, X86Register::RSP as u8);
        code_builder.emit_byte(0x81);
        self.emit_modrm(code_builder, 0b11, 0b000, X86Register::RSP as u8);
        code_builder.emit_i32(imm);
    }

    /// mov reg, imm64
    fn emit_mov_reg_imm64(&self, code_builder: &mut CodeBuilder, dst: u8, imm: i64) {
        // REX.W + B8+ rd: MOV r64, imm64
        self.emit_rex_prefix(code_builder, true, 0, 0, dst);
        code_builder.emit_byte(0xB8 + (dst & 0x07));
        code_builder.emit_i64(imm);
    }

    fn save_call_clobbered_registers(
        &mut self,
        code_builder: &mut CodeBuilder,
        exclude: &[u8],
    ) -> (Vec<u8>, usize) {
        let regs: Vec<u8> = self
            .calling_convention
            .caller_saved
            .iter()
            .copied()
            .filter(|reg| !exclude.contains(reg))
            .collect();

        if regs.is_empty() {
            return (regs, 0);
        }

        let stack_space = align_to(regs.len() * 8, 16);
        self.emit_sub_rsp_imm(code_builder, stack_space as i32);

        for (idx, reg) in regs.iter().enumerate() {
            self.emit_mov_mem_reg(code_builder, X86Register::RSP as u8, (idx * 8) as i32, *reg);
        }

        (regs, stack_space)
    }

    fn restore_call_clobbered_registers(
        &mut self,
        code_builder: &mut CodeBuilder,
        regs: &[u8],
        stack_space: usize,
    ) {
        if regs.is_empty() {
            return;
        }

        for (idx, reg) in regs.iter().enumerate() {
            self.emit_mov_reg_mem(code_builder, *reg, X86Register::RSP as u8, (idx * 8) as i32);
        }

        self.emit_add_rsp_imm(code_builder, stack_space as i32);
    }

    /// add reg, reg (64位)
    fn emit_add_reg_reg(&self, code_builder: &mut CodeBuilder, dst: u8, src: u8) {
        // REX.W + 01 /r: ADD r/m64, r64
        self.emit_rex_prefix(code_builder, true, src, 0, dst);
        code_builder.emit_byte(0x01);
        self.emit_modrm(code_builder, 0b11, src, dst);
    }

    /// add reg, imm32 (64位)
    fn emit_add_reg_imm32(&self, code_builder: &mut CodeBuilder, dst: u8, imm: i32) {
        // REX.W + 81 /0 id: ADD r/m64, imm32
        self.emit_rex_prefix(code_builder, true, 0, 0, dst);
        code_builder.emit_byte(0x81);
        self.emit_modrm(code_builder, 0b11, 0, dst);
        code_builder.emit_i32(imm);
    }

    /// sub reg, reg (64位)
    fn emit_sub_reg_reg(&self, code_builder: &mut CodeBuilder, dst: u8, src: u8) {
        // REX.W + 29 /r: SUB r/m64, r64
        self.emit_rex_prefix(code_builder, true, src, 0, dst);
        code_builder.emit_byte(0x29);
        self.emit_modrm(code_builder, 0b11, src, dst);
    }

    /// sub reg, imm32 (64位)
    fn emit_sub_reg_imm32(&self, code_builder: &mut CodeBuilder, dst: u8, imm: i32) {
        // REX.W + 81 /5 id: SUB r/m64, imm32
        self.emit_rex_prefix(code_builder, true, 0, 0, dst);
        code_builder.emit_byte(0x81);
        self.emit_modrm(code_builder, 0b11, 5, dst);
        code_builder.emit_i32(imm);
    }

    /// imul reg, reg (64位)
    fn emit_imul_reg_reg(&self, code_builder: &mut CodeBuilder, dst: u8, src: u8) {
        // REX.W + 0F AF /r: IMUL r64, r/m64
        self.emit_rex_prefix(code_builder, true, dst, 0, src);
        code_builder.emit_bytes(&[0x0F, 0xAF]);
        self.emit_modrm(code_builder, 0b11, dst, src);
    }

    /// imul reg, imm32 (64位)
    fn emit_imul_reg_imm32(&self, code_builder: &mut CodeBuilder, dst: u8, imm: i32) {
        // REX.W + 69 /r id: IMUL r64, r/m64, imm32
        self.emit_rex_prefix(code_builder, true, dst, 0, dst);
        code_builder.emit_byte(0x69);
        self.emit_modrm(code_builder, 0b11, dst, dst);
        code_builder.emit_i32(imm);
    }

    /// idiv reg (64位有符号除法)
    fn emit_idiv_reg(&self, code_builder: &mut CodeBuilder, src: u8) {
        // REX.W + F7 /7: IDIV r/m64
        self.emit_rex_prefix(code_builder, true, 0, 0, src);
        code_builder.emit_byte(0xF7);
        self.emit_modrm(code_builder, 0b11, 7, src);
    }

    /// cmp reg, reg (64位)
    fn emit_cmp_reg_reg(&self, code_builder: &mut CodeBuilder, reg1: u8, reg2: u8) {
        // REX.W + 39 /r: CMP r/m64, r64
        self.emit_rex_prefix(code_builder, true, reg2, 0, reg1);
        code_builder.emit_byte(0x39);
        self.emit_modrm(code_builder, 0b11, reg2, reg1);
    }

    /// cmp reg, imm32 (64位)
    fn emit_cmp_reg_imm32(&self, code_builder: &mut CodeBuilder, reg: u8, imm: i32) {
        // REX.W + 81 /7 id: CMP r/m64, imm32
        self.emit_rex_prefix(code_builder, true, 0, 0, reg);
        code_builder.emit_byte(0x81);
        self.emit_modrm(code_builder, 0b11, 7, reg);
        code_builder.emit_i32(imm);
    }

    /// mov reg, [reg + offset] (64位)
    fn emit_mov_reg_mem(&self, code_builder: &mut CodeBuilder, dst: u8, base: u8, offset: i32) {
        // REX.W + 8B /r: MOV r64, r/m64
        self.emit_rex_prefix(code_builder, true, dst, 0, base);
        code_builder.emit_byte(0x8B);

        if offset == 0 && (base & 0x07) != 5 {
            // RBP需要特殊处理
            // ModR/M: mod=00, reg=dst, r/m=base
            self.emit_modrm(code_builder, 0b00, dst, base);
        } else if (-128..=127).contains(&offset) {
            // ModR/M: mod=01, reg=dst, r/m=base + SIB + disp8
            self.emit_modrm(code_builder, 0b01, dst, base);
            code_builder.emit_byte(offset as u8);
        } else {
            // ModR/M: mod=10, reg=dst, r/m=base + SIB + disp32
            self.emit_modrm(code_builder, 0b10, dst, base);
            code_builder.emit_i32(offset);
        }
    }

    /// mov [reg + offset], reg (64位)
    fn emit_mov_mem_reg(&self, code_builder: &mut CodeBuilder, base: u8, offset: i32, src: u8) {
        // REX.W + 89 /r: MOV r/m64, r64
        self.emit_rex_prefix(code_builder, true, src, 0, base);
        code_builder.emit_byte(0x89);

        if offset == 0 && (base & 0x07) != 5 {
            // RBP需要特殊处理
            self.emit_modrm(code_builder, 0b00, src, base);
        } else if (-128..=127).contains(&offset) {
            self.emit_modrm(code_builder, 0b01, src, base);
            code_builder.emit_byte(offset as u8);
        } else {
            self.emit_modrm(code_builder, 0b10, src, base);
            code_builder.emit_i32(offset);
        }
    }

    /// mov [reg + offset], imm32
    fn emit_mov_mem_imm32(&self, code_builder: &mut CodeBuilder, base: u8, offset: i32, imm: i32) {
        // REX.W + C7 /0 id: MOV r/m64, imm32
        self.emit_rex_prefix(code_builder, true, 0, 0, base);
        code_builder.emit_byte(0xC7);

        if offset == 0 && (base & 0x07) != 5 {
            self.emit_modrm(code_builder, 0b00, 0, base);
        } else if (-128..=127).contains(&offset) {
            self.emit_modrm(code_builder, 0b01, 0, base);
            code_builder.emit_byte(offset as u8);
        } else {
            self.emit_modrm(code_builder, 0b10, 0, base);
            code_builder.emit_i32(offset);
        }
        code_builder.emit_i32(imm);
    }

    /// mov reg, [rip + offset] - RIP相对寻址
    fn emit_mov_reg_rip_rel(&self, code_builder: &mut CodeBuilder, dst: u8, offset: i32) {
        // REX.W + 8B /r: MOV r64, r/m64
        // 对于RIP相对寻址，ModR/M字段是 00 101 000 (mod=00, reg=dst, rm=101)
        self.emit_rex_prefix(code_builder, true, dst, 0, 0);
        code_builder.emit_byte(0x8B);

        // ModR/M: mod=00, reg=dst, rm=101 (RIP相对)
        let modrm = (dst << 3) | 0x05; // 00 101 000
        code_builder.emit_byte(modrm);

        // 32位偏移
        code_builder.emit_i32(offset);
    }

    /// 生成 call abs64 指令
    fn emit_call_absolute(&self, code_builder: &mut CodeBuilder, func: u64) {
        let tmp = X86Register::RAX as u8;
        self.emit_mov_reg_imm64(code_builder, tmp, func as i64);
        // CALL r/m64: FF /2
        code_builder.emit_byte(0xFF);
        self.emit_modrm(code_builder, 0b11, 0b010, tmp);
    }
}

// 实现JitCompiler trait
impl JitCompiler for X86Compiler {
    fn compile_function(
        &mut self,
        function: &LirFunction,
        program: &LirProgram,
    ) -> Result<CompiledFunction, String> {
        if self.debug_mode {
            println!("开始编译函数: {}", function.name);
            println!("LIR函数内容:");
            for (i, instruction) in function.instructions.iter().enumerate() {
                println!("  {}: {:?}", i, instruction);
            }
        }

        // 🔧 设置当前函数名，用于生成唯一label
        self.current_function_name = function.name.clone();
        self.unique_label_counter = 0; // 重置计数器

        // 🔧 缓存当前函数的 callee-saved 信息
        self.current_function_use_regs = function.get_used_regs().to_vec();

        let mut code_builder = if self.debug_mode {
            CodeBuilder::with_debug_info()
        } else {
            CodeBuilder::new()
        };

        // 🔧 生成函数标签（这是函数的入口点）
        let function_label = format!("func_{}", function.name);
        code_builder.define_label(&function_label)?;

        // 函数序言
        self.emit_function_prologue(&mut code_builder)?;

        // 🔧 检查第一个instruction是label，是则编译，不是则返回错误
        if let Some(Instruction::Label { id, .. }) = function.instructions.first() {
            code_builder.define_label(&format!("label_{}", id.0))?;
        } else {
            return Err(format!("函数 '{}' 的第一个指令必须是label", function.name));
        }

        // 编译函数体 (跳过第一个label指令，因为已经处理了)
        for (index, instruction) in function.instructions.iter().skip(1).enumerate() {
            if self.debug_mode {
                code_builder.add_source_line(index);
                println!("编译指令 {}: {:?}", index, instruction);
            }

            self.compile_instruction(instruction, &mut code_builder, program)?;
        }

        // 🔧 修复：总是生成函数尾声，确保正确的寄存器恢复
        self.emit_function_epilogue(&mut code_builder)?;

        // 完成代码生成
        // 🔧 导出待修补信息（用于后续的原地修补）
        let labels = code_builder.exported_labels().clone();
        let pending_jumps = code_builder.exported_pending_jumps().clone();
        let pending_label_addresses = code_builder.exported_pending_label_addresses().clone();
        let pending_adrs = code_builder.exported_pending_adrs().clone();

        // 第一轮编译：不修补跨函数标签引用，直接返回未修补的机器码
        let machine_code = code_builder.finalize()?;

        let mut compiled_function = CompiledFunction::new(
            function.name.clone(),
            machine_code,
            0, // 入口点在函数开始
        );

        // 🔧 保存label信息和待修补信息（用于patch_executable_memory）
        compiled_function.labels = labels;
        compiled_function.pending_jumps = pending_jumps;
        compiled_function.pending_label_addresses = pending_label_addresses;
        compiled_function.pending_adrs = pending_adrs;

        if self.debug_mode {
            println!(
                "函数 '{}' 编译完成，生成机器码 {} 字节",
                function.name,
                compiled_function.code_size()
            );
        }

        Ok(compiled_function)
    }

    // 🔧 优化：compile_function_with_global_labels 已被移除
    // 现在使用 compile_function + patch_executable_memory 进行单次编译+原地修补

    fn target_architecture(&self) -> &'static str {
        "x86-64"
    }

    fn get_register_mapping(&self) -> &HashMap<Register, u8> {
        &self.register_mapping
    }

    fn supports_debug_info(&self) -> bool {
        true
    }

    fn get_calling_convention(&self) -> CallingConventionInfo {
        self.calling_convention.clone()
    }
}

// 函数序言和尾声的实现
impl X86Compiler {
    /// 获取需要保存的callee-saved寄存器
    fn get_callee_saved_registers(&self) -> Vec<u8> {
        // 从calling convention获取callee-saved寄存器列表
        let callee_saved_regs = &self.calling_convention.callee_saved;
        
        // 过滤出实际被使用的寄存器
        self.current_function_use_regs
            .iter()
            .filter(|&&reg| callee_saved_regs.contains(&reg))
            .copied()
            .collect()
    }

    /// 生成函数序言
    fn emit_function_prologue(&self, code_builder: &mut CodeBuilder) -> Result<(), String> {
        // System V AMD64 ABI (Linux/Unix标准): rdi/rsi为前两个参数
        let rdi = X86Register::RDI as u8;
        let rsi = X86Register::RSI as u8;
        let vm_sp = X86Register::R10 as u8; // r6
        let vm_fp = X86Register::R11 as u8; // r7

        if self.debug_mode {
            println!("x86序言开始：生成符合 System V ABI 的函数序言");
        }

        // 1. 保存callee-saved寄存器
        self.save_callee_saved_registers(code_builder)?;

        // 2. 将参数移动到虚拟机寄存器
        // 将虚拟栈指针参数移动到r10 (r6)
        self.emit_mov_reg_reg(code_builder, vm_sp, rdi);
        // 将虚拟帧指针参数移动到r11 (r7)
        self.emit_mov_reg_reg(code_builder, vm_fp, rsi);

        if self.debug_mode {
            println!("x86序言完成");
        }

        Ok(())
    }

    /// 保存callee-saved寄存器
    fn save_callee_saved_registers(&self, code_builder: &mut CodeBuilder) -> Result<(), String> {
        let callee_saved = self.get_callee_saved_registers();
        if callee_saved.is_empty() {
            return Ok(());
        }

        if self.debug_mode {
            println!("保存 callee-saved 寄存器: {:?}", callee_saved);
        }

        // x86使用PUSH指令保存寄存器（自动递减RSP）
        for &reg in &callee_saved {
            // PUSH r64: REX.W + 50+rd (如果需要REX前缀)
            if reg >= 8 {
                // R8-R15需要REX前缀
                code_builder.emit_byte(0x41); // REX.B
            }
            code_builder.emit_byte(0x50 + (reg & 0x7));
        }

        // System V ABI要求RSP在CALL前必须16字节对齐
        // 函数入口时RSP = 16n + 8 (因为CALL压入了8字节返回地址)
        // 保存callee-saved寄存器后，如果保存了奇数个寄存器，需要额外对齐
        if callee_saved.len() % 2 == 1 {
            // 保存了奇数个寄存器，栈现在是16字节对齐的，需要减8使其错位
            // 这样CALL指令后栈又会16字节对齐（CALL会push 8字节返回地址）
            self.emit_sub_rsp_imm(code_builder, 8);
        }

        Ok(())
    }

    /// 恢复callee-saved寄存器
    fn restore_callee_saved_registers(&self, code_builder: &mut CodeBuilder) -> Result<(), String> {
        let callee_saved = self.get_callee_saved_registers();
        if callee_saved.is_empty() {
            return Ok(());
        }

        if self.debug_mode {
            println!("恢复 callee-saved 寄存器: {:?}", callee_saved);
        }

        // 如果保存时添加了对齐填充，恢复时需要先移除
        if callee_saved.len() % 2 == 1 {
            self.emit_add_rsp_imm(code_builder, 8);
        }

        // x86使用POP指令恢复寄存器（自动递增RSP）
        // 注意：恢复顺序必须与保存顺序相反
        for &reg in callee_saved.iter().rev() {
            // POP r64: REX.W + 58+rd (如果需要REX前缀)
            if reg >= 8 {
                // R8-R15需要REX前缀
                code_builder.emit_byte(0x41); // REX.B
            }
            code_builder.emit_byte(0x58 + (reg & 0x7));
        }

        Ok(())
    }

    /// 生成函数尾声
    fn emit_function_epilogue(&self, code_builder: &mut CodeBuilder) -> Result<(), String> {
        if self.debug_mode {
            println!("x86尾声开始");
        }

        // 1. 恢复callee-saved寄存器
        self.restore_callee_saved_registers(code_builder)?;

        // 2. 生成ret指令
        code_builder.emit_byte(0xC3);

        if self.debug_mode {
            println!("x86尾声完成");
        }

        Ok(())
    }
}

fn align_to(value: usize, alignment: usize) -> usize {
    ((value + alignment - 1) / alignment) * alignment
}

#[cfg(test)]
mod tests {
    use super::*;
    use karte_lir::{LirFunction, LirProgram, Register};

    #[test]
    fn test_jump_indirect_compilation() {
        // 创建一个简单的测试函数
        let mut function = LirFunction::new("test_jump_indirect".to_string());

        // 添加一个标签
        let target_label = function.new_label();
        function.add_instruction(Instruction::Label {
            id: target_label,
            span: karte_diagnostics::Span::dummy(),
        });

        // 添加一个mov指令将标签地址加载到寄存器
        function.add_instruction(Instruction::Move {
            dst: Register::Virtual(1),
            src: Operand::Label { id: target_label },
            span: karte_diagnostics::Span::dummy(),
        });

        // 添加JumpIndirect指令
        function.add_instruction(Instruction::JumpIndirect {
            function_register: Register::Virtual(1),
            span: karte_diagnostics::Span::dummy(),
        });

        // 创建程序
        let mut program = LirProgram::new();
        program.add_function(function);

        // 创建编译器
        let mut compiler = X86Compiler::new(true).unwrap();

        // 编译函数
        let result = compiler.compile_function(&program.functions["test_jump_indirect"], &program);

        // 验证编译成功
        assert!(
            result.is_ok(),
            "JumpIndirect指令编译失败: {:?}",
            result.err()
        );

        let compiled_function = result.unwrap();
        println!(
            "x86 JumpIndirect指令编译成功，机器码大小: {} 字节",
            compiled_function.code_size()
        );

        // 验证机器码不为空
        assert!(compiled_function.code_size() > 0, "编译后的机器码为空");
    }

    #[test]
    fn test_store_label_compilation() {
        // 创建一个简单的测试函数
        let mut function = LirFunction::new("test_store_label".to_string());

        // 添加一个标签
        let target_label = function.new_label();
        function.add_instruction(Instruction::Label {
            id: target_label,
            span: karte_diagnostics::Span::dummy(),
        });

        // 添加store64指令，将标签地址存储到内存
        function.add_instruction(Instruction::Store64 {
            addr: Register::Virtual(7), // r7
            offset: -16,
            src: Operand::Label { id: target_label },
            span: karte_diagnostics::Span::dummy(),
        });

        // 创建程序
        let mut program = LirProgram::new();
        program.add_function(function);

        // 创建编译器
        let mut compiler = X86Compiler::new(true).unwrap();

        // 编译函数
        let result = compiler.compile_function(&program.functions["test_store_label"], &program);

        // 验证编译成功
        assert!(
            result.is_ok(),
            "x86 Store指令标签参数编译失败: {:?}",
            result.err()
        );

        let compiled_function = result.unwrap();
        println!(
            "x86 Store指令标签参数编译成功，机器码大小: {} 字节",
            compiled_function.code_size()
        );

        // 验证机器码不为空
        assert!(compiled_function.code_size() > 0, "编译后的机器码为空");
    }
}
