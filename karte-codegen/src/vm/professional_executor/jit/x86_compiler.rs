//! x86-64 JIT编译器
//!
//! 将LIR指令编译为x86-64机器码

use super::code_buffer::{CodeBuilder, JumpType};
use super::compiler_trait::*;
use super::ffi::{RuntimeArg, RuntimeCall, RuntimeIntrinsic};
use karte_common::calling_convention::{CallingConvention, PhysicalRegister, CC};
use karte_lir::{Instruction, LirFunction, LirProgram, Operand, Register};
use std::collections::HashMap;

/// x86-64编译器
#[derive(Debug)]
pub struct X86Compiler {
    /// 寄存器映射 (虚拟寄存器 -> 物理寄存器)
    register_mapping: HashMap<Register, u8>,
    /// C FFI调用约定 (System V AMD64 ABI on Linux/macOS, Microsoft x64 on Windows)
    ffi_calling_convention: CallingConventionInfo,
    /// Karte VM调用约定
    vm_calling_convention: CallingConvention,
    /// 调试模式
    debug_mode: bool,
    /// 唯一label计数器（用于在编译时生成跳转目标）
    unique_label_counter: usize,
    /// 当前函数使用的 callee-saved 寄存器列表
    current_function_use_regs: Vec<PhysicalRegister>,
    /// 当前编译的函数名（用于生成唯一label）
    current_function_name: String,
}

impl X86Compiler {
    /// 创建新的x86编译器
    pub fn new(debug_mode: bool) -> Result<Self, String> {
        let mut compiler = Self {
            register_mapping: HashMap::new(),
            ffi_calling_convention: Self::create_calling_convention(),
            vm_calling_convention: CallingConvention::standard(),
            debug_mode: true, // 强制启用调试模式以便观察编译过程
            unique_label_counter: 0,
            current_function_use_regs: Vec::new(),
            current_function_name: String::new(),
        };

        // 初始化寄存器映射
        compiler.initialize_register_mapping();

        Ok(compiler)
    }

    /// 创建x86-64调用约定 (System V AMD64 ABI)
    fn create_calling_convention() -> CallingConventionInfo {
        CallingConventionInfo {
            parameter_registers: vec![
                X86Register::RDI as u8, // 第一个参数/返回值
                X86Register::RSI as u8, // 第二个参数
                X86Register::RDX as u8, // 第三个参数
                X86Register::RCX as u8, // 第四个参数
                X86Register::R8 as u8,  // 第五个参数
                X86Register::R9 as u8,  // 第六个参数
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
        // 虚拟寄存器到物理寄存器的映射
        // 简化映射，避免复杂的栈指针管理
        for i in 0..8 {
            let virtual_reg = Register::Virtual(i);
            let physical_reg = match i {
                0 => X86Register::RAX as u8, // r0 -> rax (返回值寄存器)
                1 => X86Register::RBX as u8, // r1 -> rbx
                2 => X86Register::RCX as u8, // r2 -> rcx
                3 => X86Register::RDX as u8, // r3 -> rdx
                4 => X86Register::R8 as u8,  // r4 -> r8
                5 => X86Register::R9 as u8,  // r5 -> r9
                6 => X86Register::R10 as u8, // r6 -> r10 (简化：不再用作虚拟栈指针)
                7 => X86Register::R11 as u8, // r7 -> r11 (简化：不再用作虚拟帧指针)
                _ => X86Register::R12 as u8, // 其他使用临时寄存器
            };
            self.register_mapping.insert(virtual_reg, physical_reg);
        }

        // 物理寄存器直接映射 - x86只有16个通用寄存器,但需要支持32个物理寄存器
        // 寄存器16-31将复用x86寄存器,但要避免冲突
        for i in 0..32 {
            let physical_reg = Register::Physical(i);
            // 映射到对应的 x86-64 寄存器
            let x86_reg = match i {
                0 => X86Register::RAX as u8,  // r0 (返回值)
                1 => X86Register::RBX as u8,  // r1 (参数1)
                2 => X86Register::RCX as u8,  // r2 (参数2)
                3 => X86Register::RDX as u8,  // r3 (参数3)
                4 => X86Register::R8 as u8,   // r4 (参数4)
                5 => X86Register::R9 as u8,   // r5 (返回地址)
                6 => X86Register::R10 as u8,  // r6 (栈指针)
                7 => X86Register::R11 as u8,  // r7 (帧指针)
                8 => X86Register::RSI as u8,  // r8
                9 => X86Register::RDI as u8,  // r9
                10 => X86Register::R12 as u8, // r10
                11 => X86Register::R13 as u8, // r11
                12 => X86Register::R14 as u8, // r12 (effect栈指针)
                13 => X86Register::R15 as u8, // r13
                14 => X86Register::RBP as u8, // r14
                15 => X86Register::RSP as u8, // r15
                // 16-31: 复用寄存器（x86-64只有16个通用寄存器）
                // 这些寄存器应该在寄存器分配时避免使用,或者spill到栈上
                16 => X86Register::RAX as u8,
                17 => X86Register::RBX as u8,
                18 => X86Register::RCX as u8,
                19 => X86Register::RDX as u8,
                20 => X86Register::RSI as u8,
                21 => X86Register::RDI as u8,
                22 => X86Register::R8 as u8,
                23 => X86Register::R9 as u8,
                24 => X86Register::R10 as u8,
                25 => X86Register::R11 as u8,
                26 => X86Register::R12 as u8,
                27 => X86Register::R13 as u8,
                28 => X86Register::R14 as u8,
                29 => X86Register::R15 as u8,
                30 => X86Register::RBP as u8,
                31 => X86Register::RSP as u8,
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
        is_main_function: bool,
        instruction_index: usize,
        function: &LirFunction,
    ) -> Result<(), String> {
        if self.debug_mode {
            log::debug!("编译x86-64指令: {}", instruction);
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
            Instruction::JumpGreaterEqual { target, .. } => self.compile_conditional_jump(
                JumpType::ConditionalGreaterEqual,
                target,
                code_builder,
            ),
            Instruction::Call { target, .. } => self.compile_call(target, code_builder),
            Instruction::JumpIndirect {
                function_register, ..
            } => self.compile_jump_indirect(function_register, code_builder),
            Instruction::JumpRegister {
                target_register, ..
            } => self.compile_jump_register(target_register, code_builder),
            Instruction::Return { value, .. } => {
                self.compile_return(value.as_ref(), code_builder, is_main_function)
            }
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
            Instruction::StorePair {
                addr,
                offset,
                src1,
                src2,
                ..
            } => self.compile_store_pair(addr, *offset, src1, src2, code_builder),
            Instruction::LoadPair {
                dst1,
                dst2,
                addr,
                offset,
                ..
            } => self.compile_load_pair(dst1, dst2, addr, *offset, code_builder),
            Instruction::Alloc {
                dst,
                size,
                alignment,
                allocation_type,
                ..
            } => self.compile_alloc(
                dst,
                *size,
                *alignment,
                allocation_type,
                code_builder,
                instruction_index,
                function,
            ),
            Instruction::Free { addr, .. } => {
                self.compile_free(addr, code_builder, instruction_index, function)
            }
            Instruction::Retain { value, .. } => {
                self.compile_retain(value, code_builder, instruction_index, function)
            }
            Instruction::Release { value, .. } => {
                self.compile_release(value, code_builder, instruction_index, function)
            }
            Instruction::Safepoint { .. } => {
                self.compile_safepoint(code_builder, instruction_index, function)
            }
            Instruction::Nop { .. } => {
                // x86-64 NOP指令 (0x90)
                self.emit_nop(code_builder);
                Ok(())
            }
            _ => Err(format!("不支持的x86-64指令类型: {:?}", instruction)),
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

    /// 编译除法指令
    fn compile_div(
        &mut self,
        dst: &Register,
        src1: &Operand,
        src2: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> Result<(), String> {
        // x86-64的除法指令比较特殊：
        // - 被除数必须在RAX中
        // - 除数可以是任何寄存器或内存
        // - 商存储在RAX中，余数存储在RDX中
        // - 需要先用CQO指令将RAX符号扩展到RDX:RAX
        
        let dst_reg = self.get_physical_register(dst)?;
        let rax = X86Register::RAX as u8;
        let rdx = X86Register::RDX as u8;

        // 保存RAX和RDX（如果它们不是dst）
        let need_save_rax = dst_reg != rax;
        let need_save_rdx = dst_reg != rdx;
        
        if need_save_rax {
            // push rax
            code_builder.emit_bytes(&[0x50]);
        }
        if need_save_rdx {
            // push rdx
            code_builder.emit_bytes(&[0x52]);
        }

        // 将src1移动到RAX
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

        // CQO: 符号扩展RAX到RDX:RAX
        code_builder.emit_bytes(&[0x48, 0x99]);

        // IDIV src2
        match src2 {
            Operand::Register { id } => {
                let src2_reg = self.get_physical_register(id)?;
                // REX.W + F7 /7: IDIV r/m64
                self.emit_rex_prefix(code_builder, true, 0, 0, src2_reg);
                code_builder.emit_byte(0xF7);
                self.emit_modrm(code_builder, 0b11, 7, src2_reg);
            }
            Operand::Immediate { value } => {
                // 除数是立即数，需要先加载到寄存器
                // 使用R8作为临时寄存器
                let temp_reg = X86Register::R8 as u8;
                self.emit_mov_reg_imm64(code_builder, temp_reg, *value);
                self.emit_rex_prefix(code_builder, true, 0, 0, temp_reg);
                code_builder.emit_byte(0xF7);
                self.emit_modrm(code_builder, 0b11, 7, temp_reg);
            }
            _ => {
                return Err(format!("div指令不支持的src2类型: {:?}", src2));
            }
        }

        // 将商从RAX移动到dst
        if dst_reg != rax {
            self.emit_mov_reg_reg(code_builder, dst_reg, rax);
        }

        // 恢复RAX和RDX
        if need_save_rdx {
            // pop rdx
            code_builder.emit_bytes(&[0x5A]);
        }
        if need_save_rax && dst_reg != rax {
            // pop rax
            code_builder.emit_bytes(&[0x58]);
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
        is_main_function: bool,
    ) -> Result<(), String> {
        // 1. 将返回值移动到RAX寄存器
        if let Some(return_reg) = value {
            let src_reg = self.get_physical_register(return_reg)?;
            if src_reg != (X86Register::RAX as u8) {
                self.emit_mov_reg_reg(code_builder, X86Register::RAX as u8, src_reg);
            }
        } else {
            // 无返回值的函数默认返回0
            self.emit_mov_reg_imm64(code_builder, X86Register::RAX as u8, 0);
        }

        // VM调用约定：返回时需要弹出虚拟返回地址并跳转
        let vm_sp_reg = self.vm_calling_convention.stack_pointer;
        let return_addr_reg = self.vm_calling_convention.return_address;
        
        if is_main_function {
            // Main函数逻辑：
            // 虚拟栈布局（从低地址到高地址）：
            // [SP+0]: 返回值槽指针（RDI参数，由序言保存）
            // [SP+16]: 系统SP（由序言保存）
            //
            // 2. 弹出返回值槽（16字节）
            self.emit_add_reg_imm32(code_builder, vm_sp_reg, 16);

            // 3. 调用epilogue恢复系统栈并返回
            // epilogue会：
            //   - 从虚拟栈读取系统SP并切换回系统栈
            //   - 恢复callee-saved寄存器
            //   - 恢复RBP
            //   - RET（使用系统栈上的返回地址）
            self.emit_function_epilogue(code_builder)?;
        } else {
            // 内部函数逻辑：
            // 2. 恢复callee-saved寄存器（从虚拟栈）
            self.emit_internal_function_epilogue(code_builder)?;
            
            // 3. 加载返回地址到专用寄存器
            self.emit_mov_reg_mem(code_builder, return_addr_reg, vm_sp_reg, 0);

            // 4. 跳转到返回地址
            let ret_reg = Register::Physical(return_addr_reg);
            self.compile_jump_register(&ret_reg, code_builder)?;
        }

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
                // 1. 使用R8加载label地址
                // LEA R8, [RIP + label]
                self.emit_lea_reg_rip_rel(code_builder, 8, &label_name);
                // 2. 存储R8到目标内存
                self.emit_mov_mem_reg(code_builder, addr_reg, offset as i32, 8);
            }
            _ => {
                return Err(format!("store64指令不支持的src类型: {:?}", src));
            }
        }
        Ok(())
    }

    /// 编译StorePair指令 (模拟ARM的STP)
    /// x86没有原生的pair store指令，使用两条mov指令
    fn compile_store_pair(
        &mut self,
        addr: &Register,
        offset: i64,
        src1: &Register,
        src2: &Register,
        code_builder: &mut CodeBuilder,
    ) -> Result<(), String> {
        let base_reg = self.get_physical_register(addr)?;
        let reg1 = self.get_physical_register(src1)?;
        let reg2 = self.get_physical_register(src2)?;

        // 存储第一个寄存器到 [base + offset]
        self.emit_mov_mem_reg(code_builder, base_reg, offset as i32, reg1);
        // 存储第二个寄存器到 [base + offset + 8]
        self.emit_mov_mem_reg(code_builder, base_reg, (offset + 8) as i32, reg2);

        Ok(())
    }

    /// 编译LoadPair指令 (模拟ARM的LDP)
    /// x86没有原生的pair load指令，使用两条mov指令
    fn compile_load_pair(
        &mut self,
        dst1: &Register,
        dst2: &Register,
        addr: &Register,
        offset: i64,
        code_builder: &mut CodeBuilder,
    ) -> Result<(), String> {
        let base_reg = self.get_physical_register(addr)?;
        let reg1 = self.get_physical_register(dst1)?;
        let reg2 = self.get_physical_register(dst2)?;

        // 加载第一个寄存器从 [base + offset]
        self.emit_mov_reg_mem(code_builder, reg1, base_reg, offset as i32);
        // 加载第二个寄存器从 [base + offset + 8]
        self.emit_mov_reg_mem(code_builder, reg2, base_reg, (offset + 8) as i32);

        Ok(())
    }

    fn compile_alloc(
        &mut self,
        dst: &Register,
        size: usize,
        alignment: usize,
        allocation_type: &karte_lir::AllocationType,
        code_builder: &mut CodeBuilder,
        instruction_index: usize,
        function: &LirFunction,
    ) -> Result<(), String> {
        match allocation_type {
            karte_lir::AllocationType::Heap => {
                let call = RuntimeCall::alloc(size, alignment);
                self.emit_runtime_call(code_builder, call, Some(dst), instruction_index, function)
            }
            _ => Err(format!(
                "Alloc instruction with unsupported allocation type: {:?}",
                allocation_type
            )),
        }
    }

    fn compile_free(
        &mut self,
        addr: &Register,
        code_builder: &mut CodeBuilder,
        instruction_index: usize,
        function: &LirFunction,
    ) -> Result<(), String> {
        let call = RuntimeCall::free(*addr);
        self.emit_runtime_call(code_builder, call, None, instruction_index, function)
    }

    fn compile_retain(
        &mut self,
        value: &Register,
        code_builder: &mut CodeBuilder,
        instruction_index: usize,
        function: &LirFunction,
    ) -> Result<(), String> {
        let call = RuntimeCall::retain(*value);
        self.emit_runtime_call(code_builder, call, None, instruction_index, function)
    }

    fn compile_release(
        &mut self,
        value: &Register,
        code_builder: &mut CodeBuilder,
        instruction_index: usize,
        function: &LirFunction,
    ) -> Result<(), String> {
        let call = RuntimeCall::release(*value);
        self.emit_runtime_call(code_builder, call, None, instruction_index, function)
    }

    fn compile_safepoint(
        &mut self,
        code_builder: &mut CodeBuilder,
        instruction_index: usize,
        function: &LirFunction,
    ) -> Result<(), String> {
        // GC 安全点：调用运行时函数
        let call = RuntimeCall::gc_safepoint();
        self.emit_runtime_call(code_builder, call, None, instruction_index, function)
    }

    fn emit_runtime_call(
        &mut self,
        code_builder: &mut CodeBuilder,
        call: RuntimeCall,
        result: Option<&Register>,
        instruction_index: usize,
        function: &LirFunction,
    ) -> Result<(), String> {
        let return_reg = X86Register::RAX as u8;

        // 🔧 关键修复：排除当前指令定义的目标寄存器
        // 因为在调用之前，目标寄存器还不存在，不应该被保存
        let mut exclude: Vec<u8> = vec![];

        // 排除返回值寄存器（rax）
        if result.is_some() && call.expects_result() {
            exclude.push(return_reg);
        }

        // 🔧 排除目标寄存器本身（当前指令正在定义的寄存器）
        // 例如：Alloc { dst = #p1 } 在调用 GC 分配之前，#p1 还不存在
        if let Some(dst) = result {
            if let Ok(dst_reg) = self.get_physical_register(dst) {
                if !exclude.contains(&dst_reg) {
                    exclude.push(dst_reg);
                }
            }
        }

        // 🔧 从 metadata 中获取调用位置活跃寄存器信息
        // metadata 包含预先计算的活跃寄存器列表
        let live_register_info = function
            .instruction_metadata
            .get(&instruction_index)
            .and_then(|meta| meta.live_register_info.as_ref());

        // 🔧 判断是否是 GC safepoint：AllocAligned, Free, GcSafepoint 会触发 GC
        let is_gc_safepoint = matches!(
            call.intrinsic,
            RuntimeIntrinsic::AllocAligned | RuntimeIntrinsic::Free | RuntimeIntrinsic::GcSafepoint
        );

        let (saved_regs, stack_space) = self.save_call_clobbered_registers(
            code_builder,
            &exclude,
            live_register_info,
            is_gc_safepoint,
        );

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
                    "runtime call {} 超出支持的参数数量(最多 {})",
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
        &self,
        code_builder: &mut CodeBuilder,
        exclude: &[u8],
        live_register_info: Option<&karte_lir::LiveRegisterInfo>,
        is_gc_safepoint: bool,
    ) -> (Vec<u8>, usize) {
        // 🔧 基于调用位置活跃寄存器信息的优化寄存器保存
        //
        // 关键区别：
        // 1. GC safepoint相关调用（alloc, free, gc_safepoint）：
        //    需要保存**所有活跃寄存器**
        //    和在函数开头未被保存的vm callee-saved寄存器（保证GC root都在stack中）
        //
        // 2. 普通runtime call（retain, release等）：
        //    只需要保存**活跃的ffi caller-saved寄存器**

        let karte_virtual_sp_reg = self.vm_calling_convention.stack_pointer;

        // 从 metadata 读取活跃寄存器列表
        let mut regs_to_virtual_stack: Vec<u8> = if let Some(live_info) = live_register_info {
            // 从 metadata 中获取活跃寄存器
            let mut live_regs = Vec::new();

            for reg in &live_info.live_registers {
                if let Register::Physical(phys_reg) = reg {
                    if is_gc_safepoint {
                        // GC safepoint：保存所有活跃寄存器
                        if !exclude.contains(phys_reg) {
                            live_regs.push(*phys_reg);
                        }
                    } else {
                        // 普通runtime call：只保存活跃的caller-saved寄存器
                        if self.ffi_calling_convention.is_caller_saved(*phys_reg) {
                            if !exclude.contains(phys_reg) {
                                live_regs.push(*phys_reg);
                            }
                        }
                    }
                }
            }

            // 如果是 GC safepoint，添加未使用的 VM callee-saved 寄存器
            if is_gc_safepoint {
                for i in self
                    .vm_calling_convention
                    .callee_saved
                    .iter()
                    .filter(|e| !self.current_function_use_regs.contains(*e))
                {
                    if !live_regs.contains(i) && !exclude.contains(i) {
                        live_regs.push(*i);
                    }
                }
            }

            // 去重并排序
            live_regs.sort();
            live_regs.dedup();

            if self.debug_mode {
                if is_gc_safepoint {
                    log::debug!(
                        "✅ GC Safepoint：需保存 {} 个活跃寄存器: {:?}",
                        live_regs.len(),
                        live_regs
                    );
                } else {
                    log::debug!(
                        "✅ 普通调用：只需保存 {} 个caller-saved寄存器: {:?}",
                        live_regs.len(),
                        live_regs
                    );
                }
            }

            live_regs
        } else {
            // 保守回退：如果没有活跃寄存器信息，保守地保存所有caller-saved寄存器
            let all_caller_saved: Vec<u8> = if is_gc_safepoint {
                (0..=15).collect::<Vec<u8>>()
            } else {
                self.ffi_calling_convention.caller_saved.clone()
            };

            if self.debug_mode {
                log::warn!(
                    "未找到活跃寄存器信息，保守保存所有caller-saved寄存器: {:?}",
                    all_caller_saved
                );
            }
            all_caller_saved
        };
        regs_to_virtual_stack.retain(|reg| !exclude.contains(reg));

        if regs_to_virtual_stack.is_empty() {
            return (regs_to_virtual_stack, 0);
        }

        // 🔧 修复：确保虚拟栈空间是 16 字节对齐的
        let raw_stack_space = regs_to_virtual_stack.len() * 8;
        let virtual_stack_space = ((raw_stack_space + 15) / 16) * 16;

        // 步骤1：保存寄存器到虚拟栈
        // 一次性调整虚拟栈指针（向下增长）
        self.emit_sub_reg_imm32(code_builder, karte_virtual_sp_reg, virtual_stack_space as i32 + 32);

        // 保存所有寄存器到调整后的虚拟栈上
        for (idx, reg) in regs_to_virtual_stack.iter().enumerate() {
            self.emit_mov_mem_reg(code_builder, karte_virtual_sp_reg, (idx * 8) as i32, *reg);
        }

        // 步骤2：只在系统栈保存 VM 帧指针，SP 依靠栈平衡自动恢复
        let vm_fp_reg = self.vm_calling_convention.frame_pointer;
        if self.debug_mode {
            log::debug!("保存 VM 帧寄存器到系统栈: fp=p{}", vm_fp_reg);
        }
        // 分配 16 字节，保持与原来相同的栈平衡
        self.emit_sub_rsp_imm(code_builder, 16);
        // 写入 [RSP, #8]
        self.emit_mov_mem_reg(code_builder, X86Register::RSP as u8, 8, vm_fp_reg);

        (regs_to_virtual_stack, virtual_stack_space)
    }

    fn restore_call_clobbered_registers(
        &self,
        code_builder: &mut CodeBuilder,
        regs: &[u8],
        stack_space: usize,
    ) {
        // 🔧 关键修复：恢复顺序与保存顺序相反
        // 1. 先从系统栈恢复 VM 栈/帧指针
        // 2. 再从虚拟栈恢复寄存器

        // 步骤1：恢复 VM 帧指针，并保持与保存步骤相同的栈调整
        let vm_fp_reg = self.vm_calling_convention.frame_pointer;
        self.emit_mov_reg_mem(code_builder, vm_fp_reg, X86Register::RSP as u8, 8);
        self.emit_add_rsp_imm(code_builder, 16);

        // 步骤2：恢复寄存器从虚拟栈
        if !regs.is_empty() {
            let karte_virtual_sp_reg = self.vm_calling_convention.stack_pointer;

            // 先用偏移加载所有寄存器（保持虚拟SP不变）
            for (idx, reg) in regs.iter().enumerate() {
                self.emit_mov_reg_mem(code_builder, *reg, karte_virtual_sp_reg, (idx * 8) as i32);
            }

            // 然后一次性恢复虚拟栈指针（向上增长）
            self.emit_add_reg_imm32(code_builder, karte_virtual_sp_reg, stack_space as i32 + 32);
        }
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

    /// 生成NOP指令
    fn emit_nop(&self, code_builder: &mut CodeBuilder) {
        // x86-64 NOP = 0x90
        code_builder.emit_byte(0x90);
    }

    /// 生成RET指令
    fn emit_ret(&self, code_builder: &mut CodeBuilder) {
        // RET = 0xC3
        code_builder.emit_byte(0xC3);
    }

    /// LEA reg, [RIP + label] - 加载标签地址
    fn emit_lea_reg_rip_rel(&self, code_builder: &mut CodeBuilder, dst: u8, label: &str) {
        // REX.W + 8D /r: LEA r64, m
        // 对于RIP相对寻址，ModR/M字段是 00 dst 101
        self.emit_rex_prefix(code_builder, true, dst, 0, 0);
        code_builder.emit_byte(0x8D);
        
        // ModR/M: mod=00, reg=dst, rm=101 (RIP相对)
        let modrm = (dst << 3) | 0x05;
        code_builder.emit_byte(modrm);
        
        // 记录需要修补的位置
        // 这里需要CodeBuilder支持标签引用
        // 暂时先emit一个占位符
        code_builder.emit_i32(0);
    }

    /// 生成唯一label名称
    /// 🔧 修复：包含函数名以确保跨函数唯一性
    fn next_label(&mut self, prefix: &str) -> String {
        let label = format!(
            "{}_{}_{}",
            self.current_function_name, prefix, self.unique_label_counter
        );
        self.unique_label_counter += 1;
        label
    }

    /// 判断当前函数是否是程序入口（main）
    fn is_entry_function(&self, function_name: &str, program: &LirProgram) -> bool {
        if let Some(main) = &program.main_function {
            if main == function_name {
                return true;
            }
        }

        function_name == "main" || function_name == karte_mir::lower::SCRIPT_ENTRY_POINT
    }

    fn get_c_ffi_callee_saved_registers(&self) -> Vec<u8> {
        self.ffi_calling_convention
            .get_callee_save_registers(&self.current_function_use_regs)
    }

    fn get_vm_callee_saved_registers(&self) -> Vec<u8> {
        self.vm_calling_convention
            .get_callee_save_registers(&self.current_function_use_regs)
    }
}

// 实现JitCompiler trait
impl JitCompiler for X86Compiler {
    /// 编译函数
    fn compile_function(
        &mut self,
        function: &LirFunction,
        program: &LirProgram,
    ) -> Result<CompiledFunction, String> {
        if self.debug_mode {
            log::debug!("x86-64: 开始编译函数 '{}'", function.name);
        }

        // 🔧 修复：设置当前函数名，用于生成唯一label
        self.current_function_name = function.name.clone();
        self.unique_label_counter = 0; // 重置计数器

        // 缓存当前函数的 callee-saved 信息
        self.current_function_use_regs = function.get_used_regs().to_vec();

        // 🔧 活跃寄存器信息已由 CallsiteLiveRegisterPass 预先计算并存储在 instruction_metadata 中
        // 无需在 JIT 编译时重新运行生命周期分析

        // 创建代码构建器
        let mut code_builder = CodeBuilder::new();

        // 生成函数标签（这是函数的入口点）
        let function_label = format!("func_{}", function.name);
        code_builder.define_label(&function_label)?;

        let is_main_function = self.is_entry_function(&function.name, program);
        // 生成函数序言（在函数标签之后，但在实际代码之前）
        // 注意：只有main函数在这里插入prologue，因为main是被外部C代码调用的
        if is_main_function {
            self.emit_function_prologue(&mut code_builder)?;
        }

        // 检查第一个instruction是label，是则编译，不是则返回错误
        if let Some(Instruction::Label { id, .. }) = function.instructions.first() {
            code_builder.define_label(&format!("label_{}", id.0))?;
        } else {
            return Err(format!("函数 '{}' 的第一个指令必须是label", function.name));
        }
        if !is_main_function {
            // 简化序言：用于内部函数调用
            self.emit_internal_function_prologue(&mut code_builder)?;
        }
        // 编译所有指令
        for (index, instruction) in function.instructions.iter().skip(1).enumerate() {
            // skip(1)后enumerate从0开始，所以实际指令索引是index+1
            self.compile_instruction(
                instruction,
                &mut code_builder,
                is_main_function,
                index + 1,
                function,
            )?;
        }

        // 获取label信息（在finalize之前）
        let labels = code_builder.exported_labels().clone();
        let pending_jumps = code_builder.exported_pending_jumps().clone();
        let pending_label_addresses = code_builder.exported_pending_label_addresses().clone();
        let pending_adrs = code_builder.exported_pending_adrs().clone();

        if self.debug_mode {
            log::debug!(
                "🔧 编译函数 '{}' 时收集到 {} 个label",
                function.name,
                labels.len()
            );
            for (label, offset) in &labels {
                log::debug!("🔧   label: {} -> 偏移: {}", label, offset);
            }
        }

        // 第一轮编译：不修补跨函数标签引用，直接返回未修补的机器码
        let machine_code = code_builder.finalize()?;

        // 创建编译后的函数
        let mut compiled_function = CompiledFunction::new(
            function.name.clone(),
            machine_code,
            0, // 入口点就是函数开始
        );

        // 保存label信息和待修补信息
        compiled_function.labels = labels;
        compiled_function.pending_jumps = pending_jumps;
        compiled_function.pending_label_addresses = pending_label_addresses;
        compiled_function.pending_adrs = pending_adrs;

        log::info!(
            "x86-64: 函数 '{}' 编译完成，机器码大小: {} 字节\n{}",
            function.name,
            compiled_function.code_size(),
            compiled_function
        );

        Ok(compiled_function)
    }

    // 🔧 优化：compile_function_with_global_labels 已被移除
    // 现在使用 compile_function + patch_executable_memory 进行单次编译+原地修补

    /// 获取目标架构名称
    fn target_architecture(&self) -> &'static str {
        "x86-64"
    }

    /// 获取寄存器映射
    fn get_register_mapping(&self) -> &HashMap<Register, u8> {
        &self.register_mapping
    }

    /// 是否支持调试信息
    fn supports_debug_info(&self) -> bool {
        true
    }

    /// 获取调用约定信息
    fn get_calling_convention(&self) -> CallingConventionInfo {
        self.ffi_calling_convention.clone()
    }
}

// 函数序言和尾声的实现
impl X86Compiler {
    /// 生成函数序言 (主函数)
    fn emit_function_prologue(&self, code_builder: &mut CodeBuilder) -> Result<(), String> {
        // System V AMD64 ABI: rdi/rsi为前两个参数
        let rdi = X86Register::RDI as u8; // 第一个参数：虚拟栈顶地址
        let rsi = X86Register::RSI as u8; // 第二个参数：虚拟栈底地址
        let vm_sp = self.vm_calling_convention.stack_pointer;
        let vm_fp = self.vm_calling_convention.frame_pointer;

        if self.debug_mode {
            log::debug!("序言开始：生成符合 System V AMD64 ABI 的函数序言");
        }

        // System V AMD64 ABI 标准序言：
        // 1. 为系统栈分配帧空间（32字节）
        // push rbp
        code_builder.emit_byte(0x55);
        // mov rbp, rsp
        self.emit_mov_reg_reg(code_builder, X86Register::RBP as u8, X86Register::RSP as u8);
        // sub rsp, 32
        self.emit_sub_rsp_imm(code_builder, 32);

        // 2. 保存其他 callee-saved 寄存器（如果有的话）
        self.save_callee_saved_registers(code_builder)?;

        // 3. 保存系统SP到R8
        // mov r8, rsp
        self.emit_mov_reg_reg(code_builder, 8, X86Register::RSP as u8);

        // 4. 切换到虚拟栈
        // mov r10, rdi (rdi = 虚拟栈顶地址)
        // mov r11, rsi (rsi = 虚拟栈底地址)
        self.emit_mov_reg_reg(code_builder, vm_sp, rdi);
        self.emit_mov_reg_reg(code_builder, vm_fp, rsi);

        // 5. 在虚拟栈保存系统SP和返回地址（x86没有专门的返回地址寄存器，使用栈）
        // sub r10, 16
        self.emit_sub_reg_imm32(code_builder, vm_sp, 16);
        // mov [r10], r8  (系统SP)
        self.emit_mov_mem_reg(code_builder, vm_sp, 0, 8);
        // 注意：x86的返回地址由call指令自动压入系统栈，不需要手动保存

        // 6. 为返回值槽分配空间（16字节对齐）
        self.save_return_slot_pointer(code_builder);

        if self.debug_mode {
            log::debug!("序言：栈使用量 = {} 字节", 32 + (self.get_c_ffi_callee_saved_registers().len() * 8));
            log::debug!("序言：{} 个 callee-saved 寄存器", self.get_c_ffi_callee_saved_registers().len());
        }

        Ok(())
    }

    /// 生成内部函数序言（用于虚拟机内部函数调用）
    fn emit_internal_function_prologue(
        &self,
        code_builder: &mut CodeBuilder,
    ) -> Result<(), String> {
        // 首先存fp sp，然后保存callee-saved寄存器
        // 获取虚拟栈指针寄存器
        let vm_sp_reg = self.vm_calling_convention.stack_pointer;
        let vm_fp_reg = self.vm_calling_convention.frame_pointer;
        
        // 保存fp sp到虚拟栈
        self.emit_sub_reg_imm32(code_builder, vm_sp_reg, 16);
        self.emit_mov_mem_reg(code_builder, vm_sp_reg, 8, vm_fp_reg);
        self.emit_mov_mem_reg(code_builder, vm_sp_reg, 0, vm_sp_reg);

        // 使用LIR寄存器分配器计算的实际使用的callee-saved寄存器
        let callee_saved = &self.get_vm_callee_saved_registers();

        // 早期返回：如果没有需要保存的寄存器
        if callee_saved.is_empty() {
            if self.debug_mode {
                log::debug!("生成内部函数序言：无需保存寄存器");
            }
            return Ok(());
        }

        if self.debug_mode {
            log::debug!("生成内部函数序言：保存 {} 个寄存器", callee_saved.len());
        }

        // 保存每个 callee-saved 寄存器到虚拟栈
        // 每次分配16字节以确保SP保持16字节对齐
        for &reg in callee_saved {
            // 先压入虚拟栈（16字节对齐）
            self.emit_sub_reg_imm32(code_builder, vm_sp_reg, 16);
            
            // 存储寄存器值到虚拟栈
            self.emit_mov_mem_reg(code_builder, vm_sp_reg, 0, reg);
        }

        if self.debug_mode {
            log::debug!(
                "保存了 {} 个 callee-saved 寄存器到虚拟栈",
                callee_saved.len()
            );
        }

        Ok(())
    }

    /// 生成内部函数尾声（用于虚拟机内部函数调用）
    fn emit_internal_function_epilogue(
        &self,
        code_builder: &mut CodeBuilder,
    ) -> Result<(), String> {
        let vm_sp_reg = self.vm_calling_convention.stack_pointer;
        let vm_fp_reg = self.vm_calling_convention.frame_pointer;

        // 使用LIR寄存器分配器计算的实际使用的callee-saved寄存器
        let callee_saved = &self.get_vm_callee_saved_registers();

        // 按逆序恢复寄存器（后进先出）
        for &reg in callee_saved.iter().rev() {
            // 从虚拟栈加载寄存器值
            self.emit_mov_reg_mem(code_builder, reg, vm_sp_reg, 0);
            
            // 弹出虚拟栈（16字节对齐）
            self.emit_add_reg_imm32(code_builder, vm_sp_reg, 16);
        }

        if self.debug_mode {
            log::debug!(
                "恢复了 {} 个 callee-saved 寄存器从虚拟栈",
                callee_saved.len()
            );
        }

        // 恢复fp sp从虚拟栈
        self.emit_mov_reg_mem(code_builder, vm_sp_reg, vm_sp_reg, 0);
        self.emit_mov_reg_mem(code_builder, vm_fp_reg, vm_sp_reg, 8);
        self.emit_add_reg_imm32(code_builder, vm_sp_reg, 16);

        Ok(())
    }

    /// 生成主函数尾声（用于与宿主环境交互的main函数）
    fn emit_function_epilogue(&self, code_builder: &mut CodeBuilder) -> Result<(), String> {
        // System V AMD64 ABI 标准尾声：按照序言的逆序恢复寄存器

        // compile_return已经弹出了返回值槽（+16字节）
        // 虚拟栈布局（compile_return后）：
        // [SP+0]: 系统SP

        // 1. 从虚拟栈读取系统SP
        self.emit_mov_reg_mem(code_builder, 8, self.vm_calling_convention.stack_pointer, 0);

        // 2. 切换回系统栈
        self.emit_mov_reg_reg(code_builder, X86Register::RSP as u8, 8);

        // 3. 恢复 callee-saved 寄存器（从系统栈）
        self.restore_callee_saved_registers(code_builder)?;

        // 4. 恢复系统栈帧
        // mov rsp, rbp
        self.emit_mov_reg_reg(code_builder, X86Register::RSP as u8, X86Register::RBP as u8);
        // pop rbp
        code_builder.emit_byte(0x5D);

        // 5. ret
        self.emit_ret(code_builder);

        Ok(())
    }

    /// 保存返回槽指针（caller通过RDI传入）
    fn save_return_slot_pointer(&self, code_builder: &mut CodeBuilder) {
        let vm_sp = self.vm_calling_convention.stack_pointer;
        self.emit_sub_reg_imm32(code_builder, vm_sp, 16);
        self.emit_mov_mem_reg(code_builder, vm_sp, 0, X86Register::RDI as u8);
    }

    /// 恢复返回槽指针并弹出栈空间
    fn load_and_pop_return_slot_pointer(&self, code_builder: &mut CodeBuilder, dst: u8) {
        let vm_sp = self.vm_calling_convention.stack_pointer;
        self.emit_mov_reg_mem(code_builder, dst, vm_sp, 0);
        self.emit_add_reg_imm32(code_builder, vm_sp, 16);
    }

    /// 将当前RAX返回值写入返回槽地址
    fn emit_store_return_value_to_slot(&self, code_builder: &mut CodeBuilder, slot_reg: u8) {
        self.emit_mov_mem_reg(code_builder, slot_reg, 0, X86Register::RAX as u8);
    }

    /// 保存 callee-saved 寄存器
    fn save_callee_saved_registers(&self, code_builder: &mut CodeBuilder) -> Result<(), String> {
        let callee_saved = &self.get_c_ffi_callee_saved_registers();
        if callee_saved.is_empty() {
            return Ok(());
        }

        if self.debug_mode {
            log::debug!("保存 callee-saved 寄存器: {:?}", callee_saved);
        }

        // 排除 RBP（已经在标准序言中保存）
        let mut regs = callee_saved.clone();
        regs.retain(|&r| r != X86Register::RBP as u8);

        for &reg in &regs {
            // push reg
            if reg >= 8 {
                // R8-R15 需要REX前缀
                code_builder.emit_byte(0x41);
            }
            code_builder.emit_byte(0x50 + (reg & 0x7));
        }

        Ok(())
    }

    /// 恢复 callee-saved 寄存器
    fn restore_callee_saved_registers(&self, code_builder: &mut CodeBuilder) -> Result<(), String> {
        let callee_saved = &self.get_c_ffi_callee_saved_registers();
        if callee_saved.is_empty() {
            return Ok(());
        }

        if self.debug_mode {
            log::debug!("恢复 callee-saved 寄存器: {:?}", callee_saved);
        }

        // 排除 RBP（在标准尾声中恢复）
        let mut regs = callee_saved.clone();
        regs.retain(|&r| r != X86Register::RBP as u8);

        // 逆序恢复
        for &reg in regs.iter().rev() {
            // pop reg
            if reg >= 8 {
                // R8-R15 需要REX前缀
                code_builder.emit_byte(0x41);
            }
            code_builder.emit_byte(0x58 + (reg & 0x7));
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
