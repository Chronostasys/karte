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
}

impl X86Compiler {
    /// 创建新的x86编译器
    pub fn new(debug_mode: bool) -> Result<Self, String> {
        let mut compiler = Self {
            register_mapping: HashMap::new(),
            calling_convention: Self::create_calling_convention(),
            debug_mode: true, // 强制启用调试模式
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
        // 虚拟寄存器到物理寄存器的映射
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

        // 物理寄存器映射 - 直接映射到x86寄存器编号，但避免RSP和RBP
        for i in 0..16 {
            let physical_reg = Register::Physical(i);
            let x86_reg = match i {
                0 => X86Register::RAX as u8,
                1 => X86Register::RCX as u8,
                2 => X86Register::RDX as u8,
                3 => X86Register::RBX as u8,
                4 => X86Register::RSI as u8, // 避免使用RSP
                5 => X86Register::RDI as u8, // 避免使用RBP
                6 => X86Register::R10 as u8, // 虚拟机栈指针
                7 => X86Register::R11 as u8, // 虚拟机帧指针
                8 => X86Register::R8 as u8,
                9 => X86Register::R9 as u8,
                10 => X86Register::R10 as u8,
                11 => X86Register::R11 as u8,
                12 => X86Register::R12 as u8,
                13 => X86Register::R13 as u8,
                14 => X86Register::R14 as u8,
                15 => X86Register::R15 as u8,
                _ => i, // 超出范围的直接映射
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
            Instruction::JumpGreater { target, .. } => {
                self.compile_conditional_jump(JumpType::ConditionalGreater, target, code_builder)
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
        // 如果有返回值，将其移动到RAX
        if let Some(reg) = value {
            let src_reg = self.get_physical_register(reg)?;
            let rax = X86Register::RAX as u8;
            if src_reg != rax {
                self.emit_mov_reg_reg(code_builder, rax, src_reg);
            }
        }

        // 生成函数尾声（包括ret指令）
        self.emit_function_epilogue(code_builder)?;
        
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

        let mut code_builder = if self.debug_mode {
            CodeBuilder::with_debug_info()
        } else {
            CodeBuilder::new()
        };

        // 函数序言
        self.emit_function_prologue(&mut code_builder)?;

        // 编译函数体
        for (index, instruction) in function.instructions.iter().enumerate() {
            if self.debug_mode {
                code_builder.add_source_line(index);
                println!("编译指令 {}: {:?}", index, instruction);
            }

            self.compile_instruction(instruction, &mut code_builder, program)?;
        }

        // 🔧 修复：不在这里生成尾声，Return指令会自己生成尾声
        // 获取label信息（在finalize之前）
        let labels = code_builder.exported_labels().clone();
        let pending_jumps = code_builder.exported_pending_jumps().clone();
        let pending_label_addresses = code_builder.exported_pending_label_addresses().clone();
        let pending_adrs = code_builder.exported_pending_adrs().clone();

        // 完成代码生成
        let machine_code = code_builder.finalize()?;

        let mut compiled_function = CompiledFunction::new(
            function.name.clone(),
            machine_code,
            0, // 入口点在函数开始
        );

        // 保存label信息和待修补信息
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

    /// 编译单个函数（使用全局标签表）
    fn compile_function_with_global_labels(
        &mut self,
        function: &LirFunction,
        program: &LirProgram,
        global_labels: &std::collections::HashMap<String, *const u8>,
    ) -> Result<CompiledFunction, String> {
        if self.debug_mode {
            println!("开始编译函数: {} (使用全局标签表)", function.name);
            println!("LIR函数内容:");
            for (i, instruction) in function.instructions.iter().enumerate() {
                println!("  {}: {:?}", i, instruction);
            }
        }

        let mut code_builder = if self.debug_mode {
            CodeBuilder::with_debug_info()
        } else {
            CodeBuilder::new()
        };

        let global_labels_usize: std::collections::HashMap<String, usize> = global_labels
            .iter()
            .map(|(k, v)| (k.clone(), *v as usize))
            .collect();
        code_builder.set_global_labels(global_labels_usize);
        // 设置全局标签表
        // code_builder.set_global_labels(global_labels.clone());

        // 函数序言
        self.emit_function_prologue(&mut code_builder)?;

        // 编译函数体
        for (index, instruction) in function.instructions.iter().enumerate() {
            if self.debug_mode {
                code_builder.add_source_line(index);
                println!("编译指令 {}: {:?}", index, instruction);
            }

            self.compile_instruction(instruction, &mut code_builder, program)?;
        }

        // 🔧 修复：不在这里生成尾声，Return指令会自己生成尾声
        // 获取label信息（在finalize之前）
        let labels = code_builder.exported_labels().clone();

        // 从全局标签表中获取当前函数的可执行内存基址
        let func_label = format!("func_{}", function.name);
        let exec_base = if let Some(&func_addr) = global_labels.get(&func_label) {
            func_addr as usize
        } else {
            if self.debug_mode {
                println!("警告：未找到函数 '{}' 的地址，使用0作为exec_base", function.name);
            }
            0
        };

        // 完成代码生成，使用全局标签进行修补
        let machine_code = code_builder
            .finalize_with_global_addresses_and_exec_base(Some(global_labels), exec_base)?;

        let mut compiled_function = CompiledFunction::new(
            function.name.clone(),
            machine_code,
            0, // 入口点在函数开始
        );

        // 保存label信息
        compiled_function.labels = labels;

        if self.debug_mode {
            println!(
                "函数 '{}' 编译完成，生成机器码 {} 字节",
                function.name,
                compiled_function.code_size()
            );
        }

        Ok(compiled_function)
    }

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
    /// 生成函数序言
    fn emit_function_prologue(&self, code_builder: &mut CodeBuilder) -> Result<(), String> {
        // System V AMD64 ABI (Linux): rdi/rsi为前两个参数
        let rdi = X86Register::RDI as u8;
        let rsi = X86Register::RSI as u8;
        let vm_sp = X86Register::R10 as u8; // r6
        let vm_fp = X86Register::R11 as u8; // r7

        // 🔧 保存 callee-saved 寄存器 (RBX) - System V ABI requires preserving RBX
        code_builder.emit_byte(0x53); // push rbx
        
        // 🔧 Push R12 for 16-byte stack alignment (3 pushes above + this = 32 bytes)
        code_builder.emit_bytes(&[0x41, 0x54]); // push r12
        
        // 🔧 关键修复：保存VM_SP (R10) 和 VM_FP (R11) 到栈
        // 因为它们在 System V ABI 中是 caller-saved，但 VM 期望它们是 callee-saved
        // push r10
        code_builder.emit_bytes(&[0x41, 0x52]); // push r10
        // push r11
        code_builder.emit_bytes(&[0x41, 0x53]); // push r11
        
        // 🔧 将传入的虚拟栈(top/bottom)地址设置到 r10/r11（LIR使用r6/r7作为虚拟SP/FP基准）
        self.emit_mov_reg_reg(code_builder, vm_sp, rdi);
        self.emit_mov_reg_reg(code_builder, vm_fp, rsi);

        Ok(())
    }

    /// 生成函数尾声
    fn emit_function_epilogue(&self, code_builder: &mut CodeBuilder) -> Result<(), String> {
        // 🔧 恢复寄存器 (reverse order of prologue)
        // pop r11
        code_builder.emit_bytes(&[0x41, 0x5B]); // pop r11
        // pop r10
        code_builder.emit_bytes(&[0x41, 0x5A]); // pop r10
        // pop r12
        code_builder.emit_bytes(&[0x41, 0x5C]); // pop r12
        // pop rbx
        code_builder.emit_byte(0x5B); // pop rbx
        // ret
        code_builder.emit_byte(0xC3); // ret
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
