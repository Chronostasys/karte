//! x86-64 JIT编译器
//!
//! 将LIR指令编译为x86-64机器码

use super::compiler_trait::*;
use super::code_buffer::{CodeBuilder, JumpType, VariableLocation};
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

        // 🔧 修复：物理寄存器映射，确保Physical(6)和Physical(7)映射到r10和r11
        for i in 0..16 {
            let physical_reg = Register::Physical(i);
            let x86_reg = match i {
                6 => X86Register::R10 as u8, // Physical(6) -> r10 (虚拟机栈指针)
                7 => X86Register::R11 as u8, // Physical(7) -> r11 (虚拟机帧指针)
                _ => i as u8, // 其他物理寄存器直接映射
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
            Instruction::Move { dst, src, .. } => {
                self.compile_move(dst, src, code_builder)
            }
            Instruction::Add { dst, src1, src2, .. } => {
                self.compile_add(dst, src1, src2, code_builder)
            }
            Instruction::Sub { dst, src1, src2, .. } => {
                self.compile_sub(dst, src1, src2, code_builder)
            }
            Instruction::Mul { dst, src1, src2, .. } => {
                self.compile_mul(dst, src1, src2, code_builder)
            }
            Instruction::Compare { src1, src2, .. } => {
                self.compile_compare(src1, src2, code_builder)
            }
            Instruction::Jump { target, .. } => {
                self.compile_jump(target, code_builder)
            }
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
            Instruction::Call { target, .. } => {
                self.compile_call(target, code_builder)
            }
            Instruction::Return { value, .. } => {
                self.compile_return(value.as_ref(), code_builder)
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
            Instruction::Nop { .. } => {
                // NOP指令
                code_builder.emit_byte(0x90);
                Ok(())
            }
            _ => {
                Err(format!("不支持的指令类型: {:?}", instruction))
            }
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
                return Err(format!("compare指令不支持的操作数组合: {:?}, {:?}", src1, src2));
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
            _ => {
                return Err(format!("store64指令不支持的src类型: {:?}", src));
            }
        }
        Ok(())
    }

    // x86-64指令编码实现
    /// 生成REX前缀
    fn emit_rex_prefix(&self, code_builder: &mut CodeBuilder, w: bool, r: u8, x: u8, b: u8) {
        let rex = 0x40 | 
                  (if w { 0x08 } else { 0x00 }) |
                  ((r & 0x08) >> 1) |
                  ((x & 0x08) >> 2) |
                  ((b & 0x08) >> 3);
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

    /// mov reg, imm64
    fn emit_mov_reg_imm64(&self, code_builder: &mut CodeBuilder, dst: u8, imm: i64) {
        // REX.W + B8+ rd: MOV r64, imm64
        self.emit_rex_prefix(code_builder, true, 0, 0, dst);
        code_builder.emit_byte(0xB8 + (dst & 0x07));
        code_builder.emit_i64(imm);
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
        
        if offset == 0 && (base & 0x07) != 5 { // RBP需要特殊处理
            // ModR/M: mod=00, reg=dst, r/m=base
            self.emit_modrm(code_builder, 0b00, dst, base);
        } else if offset >= -128 && offset <= 127 {
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
        
        if offset == 0 && (base & 0x07) != 5 { // RBP需要特殊处理
            self.emit_modrm(code_builder, 0b00, src, base);
        } else if offset >= -128 && offset <= 127 {
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
        } else if offset >= -128 && offset <= 127 {
            self.emit_modrm(code_builder, 0b01, 0, base);
            code_builder.emit_byte(offset as u8);
        } else {
            self.emit_modrm(code_builder, 0b10, 0, base);
            code_builder.emit_i32(offset);
        }
        code_builder.emit_i32(imm);
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

        // 🔧 修复：总是生成函数尾声，确保正确的寄存器恢复
        self.emit_function_epilogue(&mut code_builder)?;

        // 完成代码生成
        let machine_code = code_builder.finalize()?;
        
        let compiled_function = CompiledFunction::new(
            function.name.clone(),
            machine_code,
            0, // 入口点在函数开始
        );

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
        // Windows x64 ABI: rcx/rdx为前两个参数
        let rcx = X86Register::RCX as u8;
        let rdx = X86Register::RDX as u8;
        let vm_sp = X86Register::R10 as u8; // r6
        let vm_fp = X86Register::R11 as u8; // r7
        
        // 🔧 修复：将参数移动到虚拟机寄存器，但不干扰LIR的栈帧管理
        // 将虚拟栈指针参数移动到r10 (r6)
        self.emit_mov_reg_reg(code_builder, vm_sp, rcx);
        // 将虚拟帧指针参数移动到r11 (r7) 
        self.emit_mov_reg_reg(code_builder, vm_fp, rdx);
        
        // 🔧 新增：确保r6和r7的初始值正确，让LIR的栈帧管理指令能正常工作
        // 此时r6和r7已经包含了虚拟栈的地址，LIR的栈帧管理指令会基于这些值工作
        
        Ok(())
    }

    /// 生成函数尾声
    fn emit_function_epilogue(&self, code_builder: &mut CodeBuilder) -> Result<(), String> {
        // 只生成ret指令
        code_builder.emit_byte(0xC3);
        Ok(())
    }
} 