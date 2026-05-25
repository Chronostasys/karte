//! AArch64 JIT编译器
//!
//! 将LIR指令编译为AArch64机器码

use super::code_buffer::{CodeBuilder, JumpType};
use super::compiler_trait::*;
use super::ffi::{RuntimeArg, RuntimeCall, RuntimeIntrinsic};
use karte_common::calling_convention::{CallingConvention, PhysicalRegister, CC};
use karte_lir::{Instruction, LirFunction, LirProgram, Operand, Register};
use std::collections::HashMap;

/// AArch64编译器
#[derive(Debug)]
pub struct AArch64Compiler {
    /// 寄存器映射 (虚拟寄存器 -> 物理寄存器)
    register_mapping: HashMap<Register, u8>,
    /// C FFI调用约定 (AAPCS64)
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

/// AArch64寄存器枚举
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AArch64Register {
    // 通用寄存器 X0-X30
    X0 = 0,   // 参数/返回值寄存器
    X1 = 1,   // 参数寄存器
    X2 = 2,   // 参数寄存器
    X3 = 3,   // 参数寄存器
    X4 = 4,   // 参数寄存器
    X5 = 5,   // 参数寄存器
    X6 = 6,   // 参数寄存器
    X7 = 7,   // 参数寄存器
    X8 = 8,   // 间接结果位置寄存器
    X9 = 9,   // 临时寄存器
    X10 = 10, // 临时寄存器
    X11 = 11, // 临时寄存器
    X12 = 12, // 临时寄存器
    X13 = 13, // 临时寄存器
    X14 = 14, // 临时寄存器
    X15 = 15, // 临时寄存器
    X16 = 16, // 过程内调用临时寄存器
    X17 = 17, // 过程内调用临时寄存器
    X18 = 18, // 平台寄存器（保留）
    X19 = 19, // 被调用者保存寄存器
    X20 = 20, // 被调用者保存寄存器
    X21 = 21, // 被调用者保存寄存器
    X22 = 22, // 被调用者保存寄存器
    X23 = 23, // 被调用者保存寄存器
    X24 = 24, // 被调用者保存寄存器
    X25 = 25, // 被调用者保存寄存器
    X26 = 26, // 被调用者保存寄存器
    X27 = 27, // 被调用者保存寄存器
    X28 = 28, // 被调用者保存寄存器
    X29 = 29, // 帧指针 (FP)
    X30 = 30, // 链接寄存器 (LR)
    // 栈指针（SP）作为特殊寄存器处理
    SP = 31, // 栈指针
}

impl AArch64Compiler {
    /// 创建新的AArch64编译器
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

    /// 创建AArch64调用约定 (AAPCS64)
    fn create_calling_convention() -> CallingConventionInfo {
        CallingConventionInfo {
            parameter_registers: vec![
                AArch64Register::X0 as u8, // 第一个参数/返回值
                AArch64Register::X1 as u8, // 第二个参数
                AArch64Register::X2 as u8, // 第三个参数
                AArch64Register::X3 as u8, // 第四个参数
                AArch64Register::X4 as u8, // 第五个参数
                AArch64Register::X5 as u8, // 第六个参数
                AArch64Register::X6 as u8, // 第七个参数
                AArch64Register::X7 as u8, // 第八个参数
            ],
            return_register: AArch64Register::X0 as u8,
            stack_pointer: AArch64Register::SP as u8,
            frame_pointer: AArch64Register::X29 as u8,
            caller_saved: vec![
                // 参数/结果寄存器
                AArch64Register::X0 as u8,
                AArch64Register::X1 as u8,
                AArch64Register::X2 as u8,
                AArch64Register::X3 as u8,
                AArch64Register::X4 as u8,
                AArch64Register::X5 as u8,
                AArch64Register::X6 as u8,
                AArch64Register::X7 as u8,
                // 间接结果位置
                AArch64Register::X8 as u8,
                // 临时寄存器
                AArch64Register::X9 as u8,
                AArch64Register::X10 as u8,
                AArch64Register::X11 as u8,
                AArch64Register::X12 as u8,
                AArch64Register::X13 as u8,
                AArch64Register::X14 as u8,
                AArch64Register::X15 as u8,
                // 过程内调用临时寄存器
                AArch64Register::X16 as u8,
                AArch64Register::X17 as u8,
            ],
            callee_saved: vec![
                // 被调用者保存寄存器
                AArch64Register::X19 as u8,
                AArch64Register::X20 as u8,
                AArch64Register::X21 as u8,
                AArch64Register::X22 as u8,
                AArch64Register::X23 as u8,
                AArch64Register::X24 as u8,
                AArch64Register::X25 as u8,
                AArch64Register::X26 as u8,
                AArch64Register::X27 as u8,
                AArch64Register::X28 as u8,
                // 帧指针和链接寄存器
                AArch64Register::X29 as u8,
                AArch64Register::X30 as u8,
            ],
        }
    }

    /// 初始化寄存器映射
    fn initialize_register_mapping(&mut self) {
        // 物理寄存器直接映射
        for i in 0..32 {
            let physical_reg = Register::Physical(i);
            // 映射到对应的 AArch64 寄存器
            let aarch64_reg = match i {
                0 => AArch64Register::X0 as u8,   // r0 (返回值)
                1 => AArch64Register::X1 as u8,   // r1 (参数1)
                2 => AArch64Register::X2 as u8,   // r2 (参数2)
                3 => AArch64Register::X3 as u8,   // r3 (参数3)
                4 => AArch64Register::X4 as u8,   // r4 (参数4)
                5 => AArch64Register::X5 as u8,   // r5 (返回地址)
                6 => AArch64Register::X6 as u8,   // r6 (栈指针)
                7 => AArch64Register::X7 as u8,   // r7 (帧指针)
                8 => AArch64Register::X8 as u8,   // r8
                9 => AArch64Register::X9 as u8,   // r9
                10 => AArch64Register::X10 as u8, // r10
                11 => AArch64Register::X11 as u8, // r11
                12 => AArch64Register::X12 as u8, // r12 (effect栈指针)
                13 => AArch64Register::X13 as u8, // r13
                14 => AArch64Register::X14 as u8, // r14
                15 => AArch64Register::X15 as u8, // r15
                16 => AArch64Register::X16 as u8, // r16
                17 => AArch64Register::X17 as u8, // r17
                18 => AArch64Register::X18 as u8, // r18
                19 => AArch64Register::X19 as u8, // r19
                20 => AArch64Register::X20 as u8, // r20
                21 => AArch64Register::X21 as u8, // r21
                22 => AArch64Register::X22 as u8, // r22
                23 => AArch64Register::X23 as u8, // r23
                24 => AArch64Register::X24 as u8, // r24
                25 => AArch64Register::X25 as u8, // r25
                26 => AArch64Register::X26 as u8, // r26
                27 => AArch64Register::X27 as u8, // r27
                28 => AArch64Register::X28 as u8, // r28
                29 => AArch64Register::X29 as u8, // r29 (帧指针)
                30 => AArch64Register::X30 as u8, // r30 (链接寄存器)
                31 => AArch64Register::SP as u8,  // 栈指针
                _ => unreachable!(),              // 由于循环范围是 0..32，这里不会执行
            };
            self.register_mapping.insert(physical_reg, aarch64_reg);
        }
    }

    /// 获取寄存器的物理编号
    fn get_physical_register(&self, reg: &Register) -> Result<u8, String> {
        match reg {
            Register::Virtual(id) => {
                // 虚拟寄存器不应出现在 JIT 阶段，寄存器分配必须在 JIT 之前完成
                panic!("JIT 编译器遇到虚拟寄存器 Virtual({})，寄存器分配应在 JIT 之前完成", id);
            }
            _ => {
                self.register_mapping
                    .get(reg)
                    .copied()
                    .ok_or_else(|| format!("未映射的寄存器: {:?}", reg))
            }
        }
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
            log::debug!("编译AArch64指令: {}", instruction);
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
                // AArch64 NOP指令 (0xD503201F)
                self.emit_nop(code_builder);
                Ok(())
            }
            _ => Err(format!("不支持的AArch64指令类型: {:?}", instruction)),
        }
    }

    /// 编译移动指令
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
                // MOV dst, src (实际使用 ORR dst, XZR, src)
                self.emit_mov_reg_reg(code_builder, dst_reg, src_reg);
            }
            Operand::Immediate { value } => {
                // MOV dst, #imm
                self.emit_mov_reg_imm64(code_builder, dst_reg, *value);
            }
            Operand::Label { id } => {
                // 加载标签地址到寄存器
                let label_name = format!("label_{}", id.0);

                // 使用ADRP+ADD指令序列加载标签地址
                // 1. ADRP dst, label (加载页地址)
                code_builder.emit_adrp(dst_reg, &label_name);

                // 2. ADD dst, dst, :lo12:label (加载页内偏移)
                code_builder.emit_add_reg_label(dst_reg, &label_name);
            }
            _ => {
                return Err(format!("不支持的移动操作数类型: {:?}", src));
            }
        }
        Ok(())
    }

    /// 编译加法指令
    fn compile_add(
        &mut self,
        dst: &Register,
        src1: &Operand,
        src2: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> Result<(), String> {
        let dst_reg = self.get_physical_register(dst)?;

        match (src1, src2) {
            (Operand::Register { id: src1_id }, Operand::Register { id: src2_id }) => {
                let src1_reg = self.get_physical_register(src1_id)?;
                let src2_reg = self.get_physical_register(src2_id)?;
                // ADD dst, src1, src2
                self.emit_add_reg_reg_reg(code_builder, dst_reg, src1_reg, src2_reg);
            }
            (Operand::Register { id: src1_id }, Operand::Immediate { value }) => {
                let src1_reg = self.get_physical_register(src1_id)?;
                // ADD dst, src1, #imm
                self.emit_add_reg_reg_imm(code_builder, dst_reg, src1_reg, *value as i32);
            }
            _ => {
                return Err(format!("不支持的加法操作数组合: {:?}, {:?}", src1, src2));
            }
        }
        Ok(())
    }

    /// 编译减法指令
    fn compile_sub(
        &mut self,
        dst: &Register,
        src1: &Operand,
        src2: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> Result<(), String> {
        let dst_reg = self.get_physical_register(dst)?;

        match (src1, src2) {
            (Operand::Register { id: src1_id }, Operand::Register { id: src2_id }) => {
                let src1_reg = self.get_physical_register(src1_id)?;
                let src2_reg = self.get_physical_register(src2_id)?;
                // SUB dst, src1, src2
                self.emit_sub_reg_reg_reg(code_builder, dst_reg, src1_reg, src2_reg);
            }
            (Operand::Register { id: src1_id }, Operand::Immediate { value }) => {
                let src1_reg = self.get_physical_register(src1_id)?;
                // SUB dst, src1, #imm
                self.emit_sub_reg_reg_imm(code_builder, dst_reg, src1_reg, *value as i32);
            }
            (Operand::Immediate { value }, Operand::Register { id: src2_id }) => {
                // dst = value - reg
                // 1. mov temp, value
                // 2. sub dst, temp, reg
                let temp_reg = AArch64Register::X16 as u8;
                self.emit_mov_reg_imm64(code_builder, temp_reg, *value);
                let src2_reg = self.get_physical_register(src2_id)?;
                self.emit_sub_reg_reg_reg(code_builder, dst_reg, temp_reg, src2_reg);
            }
            (Operand::Immediate { value: v1 }, Operand::Immediate { value: v2 }) => {
                // dst = v1 - v2
                let result = v1.wrapping_sub(*v2);
                self.emit_mov_reg_imm64(code_builder, dst_reg, result);
            }
            _ => {
                return Err(format!("不支持的减法操作数组合: {:?}, {:?}", src1, src2));
            }
        }
        Ok(())
    }

    /// 编译乘法指令
    fn compile_mul(
        &mut self,
        dst: &Register,
        src1: &Operand,
        src2: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> Result<(), String> {
        let dst_reg = self.get_physical_register(dst)?;

        match (src1, src2) {
            (Operand::Register { id: src1_id }, Operand::Register { id: src2_id }) => {
                let src1_reg = self.get_physical_register(src1_id)?;
                let src2_reg = self.get_physical_register(src2_id)?;
                // MUL dst, src1, src2
                self.emit_mul_reg_reg_reg(code_builder, dst_reg, src1_reg, src2_reg);
            }
            // AArch64没有直接的立即数乘法，需要先加载立即数到寄存器
            (Operand::Register { id: src1_id }, Operand::Immediate { value }) => {
                let src1_reg = self.get_physical_register(src1_id)?;
                // 使用X16临时寄存器
                let temp_reg = AArch64Register::X16 as u8;
                self.emit_mov_reg_imm64(code_builder, temp_reg, *value);
                self.emit_mul_reg_reg_reg(code_builder, dst_reg, src1_reg, temp_reg);
            }
            _ => {
                return Err(format!("不支持的乘法操作数组合: {:?}, {:?}", src1, src2));
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
        let dst_reg = self.get_physical_register(dst)?;

        match (src1, src2) {
            (Operand::Register { id: src1_id }, Operand::Register { id: src2_id }) => {
                let src1_reg = self.get_physical_register(src1_id)?;
                let src2_reg = self.get_physical_register(src2_id)?;
                // DIV dst, src1, src2
                self.emit_div_reg_reg_reg(code_builder, dst_reg, src1_reg, src2_reg);
            }
            // AArch64没有直接的立即数除法，需要先加载立即数到寄存器
            (Operand::Register { id: src1_id }, Operand::Immediate { value }) => {
                let src1_reg = self.get_physical_register(src1_id)?;
                // 使用X16临时寄存器
                let temp_reg = AArch64Register::X16 as u8;
                self.emit_mov_reg_imm64(code_builder, temp_reg, *value);
                self.emit_div_reg_reg_reg(code_builder, dst_reg, src1_reg, temp_reg);
            }
            _ => {
                return Err(format!("不支持的除法操作数组合: {:?}, {:?}", src1, src2));
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
            (Operand::Register { id: src1_id }, Operand::Register { id: src2_id }) => {
                let src1_reg = self.get_physical_register(src1_id)?;
                let src2_reg = self.get_physical_register(src2_id)?;
                // CMP src1, src2
                self.emit_cmp_reg_reg(code_builder, src1_reg, src2_reg);
            }
            (Operand::Register { id: src1_id }, Operand::Immediate { value }) => {
                let src1_reg = self.get_physical_register(src1_id)?;
                // CMP src1, #imm
                self.emit_cmp_reg_imm(code_builder, src1_reg, *value as i32);
            }
            _ => {
                return Err(format!("不支持的比较操作数组合: {:?}, {:?}", src1, src2));
            }
        }
        Ok(())
    }

    /// 编译无条件跳转指令
    fn compile_jump(
        &mut self,
        target: &karte_lir::LabelId,
        code_builder: &mut CodeBuilder,
    ) -> Result<(), String> {
        let label_name = format!("label_{}", target.0);
        code_builder.emit_jump(JumpType::Unconditional, &label_name);
        Ok(())
    }

    /// 编译条件跳转指令
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

    /// 编译函数调用指令
    fn compile_call(
        &mut self,
        target: &karte_lir::LabelId,
        code_builder: &mut CodeBuilder,
    ) -> Result<(), String> {
        let label_name = format!("label_{}", target.0);

        // 🔧 优化：在连续内存架构中，优先使用相对跳转（BL指令）
        // BL指令支持±128MB的相对跳转范围，足够覆盖我们的代码段
        code_builder.emit_jump(JumpType::Call, &label_name);

        Ok(())
    }

    /// 编译间接函数调用指令（带链接）：BLR Xn
    fn compile_jump_indirect(
        &mut self,
        function_register: &Register,
        code_builder: &mut CodeBuilder,
    ) -> Result<(), String> {
        let function_reg = self.get_physical_register(function_register)?;

        // 🔧 修复：使用 BR 而不是 BLR
        // 因为我们在 LIR 降级阶段已经手动将返回地址压入栈
        // BLR 会将返回地址保存到 LR (X30)，导致返回地址重复
        // 使用 BR 进行纯跳转，不保存返回地址

        // 使用X16作为跳转目标寄存器
        let target_reg = AArch64Register::X16 as u8;

        // 如果函数地址不在X16中，先移动到X16
        if function_reg != target_reg {
            self.emit_mov_reg_reg(code_builder, target_reg, function_reg);
        }

        // BR X16 - 间接跳转（不保存返回地址）
        // 31|30|29|28 27 26 25 24 23 22 21|20 16|15 10|9 5|4 0
        // 1 |1 |0 |1  0  1  1  0  0  0  0 |1    |1     |Rn |0
        let instruction = 0xD61F0000u32 | ((target_reg as u32) << 5);
        code_builder.emit_u32(instruction);

        Ok(())
    }

    /// 编译寄存器跳转指令（无链接跳转）：BR Xn
    fn compile_jump_register(
        &mut self,
        target_register: &Register,
        code_builder: &mut CodeBuilder,
    ) -> Result<(), String> {
        let reg = self.get_physical_register(target_register)? as u32;
        // BR Xn: 1101 0110 0001 1111 0000 0000 0000 0000 | Rn(5)
        let instr = 0xD61F0000u32 | (reg << 5);
        code_builder.emit_u32(instr);
        Ok(())
    }

    /// 编译return指令
    fn compile_return(
        &mut self,
        value: Option<&Register>,
        code_builder: &mut CodeBuilder,
        is_main_function: bool,
    ) -> Result<(), String> {
        // 1. 将返回值移动到X0寄存器
        if let Some(return_reg) = value {
            let src_reg = self.get_physical_register(return_reg)?;
            if src_reg != (AArch64Register::X0 as u8) {
                self.emit_mov_reg_reg(code_builder, AArch64Register::X0 as u8, src_reg);
            }
        } else {
            // 无返回值的函数默认返回0
            self.emit_mov_reg_imm64(code_builder, AArch64Register::X0 as u8, 0);
        }
        // VM调用约定：返回时需要弹出虚拟返回地址并跳转
        let vm_sp_reg = self.vm_calling_convention.stack_pointer;
        let return_addr_reg = self.vm_calling_convention.return_address;
        if is_main_function {
            // Main函数逻辑：
            // 虚拟栈布局（从低地址到高地址）：
            // [SP+0]: 返回值槽指针（X0参数，由序言保存）
            // [SP+16]: 系统SP（由序言保存）
            //
            // 2. 弹出返回值槽（16字节）
            self.emit_add_reg_reg_imm(code_builder, vm_sp_reg, vm_sp_reg, 16);

            // 3. 调用epilogue恢复系统栈并返回
            // epilogue会：
            //   - 从虚拟栈读取系统SP并切换回系统栈
            //   - 恢复callee-saved寄存器
            //   - 恢复X29/X30
            //   - RET（使用系统栈上的X30）
            self.emit_function_epilogue(code_builder)?;
            self.emit_ret(code_builder);
        } else {
            // 内部函数逻辑：
            // 2. 恢复callee-saved寄存器（从虚拟栈）
            self.emit_internal_function_epilogue(code_builder)?;
            // 加载返回地址到专用寄存器
            self.emit_ldr_reg_mem(code_builder, return_addr_reg, vm_sp_reg, 0);

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

        // LDR dst, [addr, #offset]
        self.emit_ldr_reg_mem(code_builder, dst_reg, addr_reg, offset as i32);
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
                // STR src, [addr, #offset]
                self.emit_str_reg_mem(code_builder, src_reg, addr_reg, offset as i32);
            }
            Operand::Immediate { value } => {
                // AArch64没有直接的立即数存储，需要先加载到临时寄存器
                let temp_reg = AArch64Register::X16 as u8;
                self.emit_mov_reg_imm64(code_builder, temp_reg, *value);
                self.emit_str_reg_mem(code_builder, temp_reg, addr_reg, offset as i32);
            }
            Operand::Label { id } => {
                // store64 [addr + offset], label - 存储标签地址
                let label_name = format!("label_{}", id.0);
                // 1. ADRP+ADD加载label地址到x16
                code_builder.emit_adrp(16, &label_name);
                code_builder.emit_add_reg_label(16, &label_name);
                // 2. 存储x16到目标内存
                self.emit_str_reg_mem(code_builder, 16, addr_reg, offset as i32);
            }
            _ => {
                return Err(format!("不支持的存储操作数类型: {:?}", src));
            }
        }
        Ok(())
    }

    /// 编译StorePair指令 (STP)
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

        // STP <reg1>, <reg2>, [<base>, #<offset>]
        // offset必须是8的倍数，且在±256KB范围内
        if offset % 8 != 0 {
            return Err(format!(
                "StorePair offset must be 8-byte aligned, got: {}",
                offset
            ));
        }

        // 🔧 修复：如果操作SP，确保使用16字节对齐
        if base_reg == AArch64Register::SP as u8 {
            // 确保offset是16字节的倍数，以保持SP对齐
            let aligned_offset = if offset % 16 != 0 {
                // 向下对齐到16字节边界
                offset - (offset % 16)
            } else {
                offset
            };

            let scaled_offset = aligned_offset / 8;
            if scaled_offset < -4096 || scaled_offset > 4095 {
                return Err(format!(
                    "StorePair offset out of range (±32KB): {}",
                    scaled_offset
                ));
            }

            self.emit_stp_offset(code_builder, reg1, reg2, base_reg, aligned_offset as i32);
        } else {
            // 非SP寄存器，使用原始offset
            let scaled_offset = offset / 8;
            if scaled_offset < -4096 || scaled_offset > 4095 {
                return Err(format!(
                    "StorePair offset out of range (±32KB): {}",
                    scaled_offset
                ));
            }

            self.emit_stp_offset(code_builder, reg1, reg2, base_reg, offset as i32);
        }
        Ok(())
    }

    /// 编译LoadPair指令 (LDP)
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

        // LDP <reg1>, <reg2>, [<base>, #<offset>]
        // offset必须是8的倍数，且在±256KB范围内
        if offset % 8 != 0 {
            return Err(format!(
                "LoadPair offset must be 8-byte aligned, got: {}",
                offset
            ));
        }

        // 🔧 修复：如果操作SP，确保使用16字节对齐
        if base_reg == AArch64Register::SP as u8 {
            // 确保offset是16字节的倍数，以保持SP对齐
            let aligned_offset = if offset % 16 != 0 {
                // 向下对齐到16字节边界
                offset - (offset % 16)
            } else {
                offset
            };

            let scaled_offset = aligned_offset / 8;
            if scaled_offset < -4096 || scaled_offset > 4095 {
                return Err(format!(
                    "LoadPair offset out of range (±32KB): {}",
                    scaled_offset
                ));
            }

            self.emit_ldp_offset(code_builder, reg1, reg2, base_reg, aligned_offset as i32);
        } else {
            // 非SP寄存器，使用原始offset
            let scaled_offset = offset / 8;
            if scaled_offset < -4096 || scaled_offset > 4095 {
                return Err(format!(
                    "LoadPair offset out of range (±32KB): {}",
                    scaled_offset
                ));
            }

            self.emit_ldp_offset(code_builder, reg1, reg2, base_reg, offset as i32);
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
        let return_reg = AArch64Register::X0 as u8;

        // 🔧 关键修复：排除当前指令定义的目标寄存器
        // 因为在调用之前，目标寄存器还不存在，不应该被保存
        let mut exclude: Vec<u8> = vec![];

        // 排除返回值寄存器（x0）
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
            AArch64Register::X0 as u8,
            AArch64Register::X1 as u8,
            AArch64Register::X2 as u8,
            AArch64Register::X3 as u8,
            AArch64Register::X4 as u8,
            AArch64Register::X5 as u8,
            AArch64Register::X6 as u8,
            AArch64Register::X7 as u8,
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

        self.emit_runtime_dispatch(code_builder, call.intrinsic.symbol_ptr() as u64);
        self.restore_call_clobbered_registers(code_builder, &saved_regs, stack_space);

        if let (Some(dst), true) = (result, call.expects_result()) {
            let dst_reg = self.get_physical_register(dst)?;
            if dst_reg != return_reg {
                self.emit_mov_reg_reg(code_builder, dst_reg, return_reg);
            }
        }

        Ok(())
    }

    // ============= AArch64机器码生成方法 =============

    /// 生成NOP指令
    fn emit_nop(&self, code_builder: &mut CodeBuilder) {
        // AArch64 NOP = 0xD503201F
        code_builder.emit_bytes(&[0x1F, 0x20, 0x03, 0xD5]);
    }

    /// 生成RET指令
    fn emit_ret(&self, code_builder: &mut CodeBuilder) {
        // RET = 0xD65F03C0
        code_builder.emit_bytes(&[0xC0, 0x03, 0x5F, 0xD6]);
    }

    fn emit_runtime_dispatch(&self, code_builder: &mut CodeBuilder, func: u64) {
        let tmp = AArch64Register::X16 as u8;
        self.emit_mov_reg_imm64(code_builder, tmp, func as i64);
        self.emit_blr(code_builder, tmp);
    }

    fn emit_blr(&self, code_builder: &mut CodeBuilder, reg: u8) {
        // BLR Xt = 1101 0110 0011 1111 0000 0000 000r rrrr
        let instruction = 0xD63F0000u32 | ((reg as u32) << 5);
        code_builder.emit_u32(instruction);
    }

    /// 生成MOV寄存器到寄存器指令
    fn emit_mov_reg_reg(&self, code_builder: &mut CodeBuilder, dst: u8, src: u8) {
        // 对于SP寄存器，需要使用ADD指令实现MOV
        if src == 31 || dst == 31 {
            // SP寄存器
            // ADD dst, src, #0 (实现MOV dst, src)
            // 31|30|29|28 27 26 25 24 23 22|21 10|9 5|4 0
            // 1 |0 |0 |0  1  0  0  0  1  0 |imm12|Rn |Rd
            let instruction = 0x91000000u32 | ((src as u32) << 5) | (dst as u32);
            code_builder.emit_bytes(&instruction.to_le_bytes());
        } else {
            // ORR dst, XZR, src (实现MOV dst, src)
            // 指令格式: ORR <Xd>, <Xn>, <Xm>
            // 31|30|29|28 27 26 25 24 23 22 21|20 16|15 10|9 5|4 0
            // 1 |0 |1 |0  1  0  1  0  0  0  0 |Xm   |0     |Xn |Xd
            let instruction = 0xAA000000u32 | ((src as u32) << 16) | (31u32 << 5) | (dst as u32);
            code_builder.emit_bytes(&instruction.to_le_bytes());
        }
    }

    /// 生成MOV立即数到寄存器指令
    fn emit_mov_reg_imm64(&self, code_builder: &mut CodeBuilder, dst: u8, imm: i64) {
        // 对于64位立即数，需要分多个16位块进行编码
        let uimm = imm as u64;

        // MOVZ指令用于第一个非零的16位块
        // MOVK指令用于后续的16位块

        let chunks = [
            (uimm & 0xFFFF) as u16,         // bits [15:0]
            ((uimm >> 16) & 0xFFFF) as u16, // bits [31:16]
            ((uimm >> 32) & 0xFFFF) as u16, // bits [47:32]
            ((uimm >> 48) & 0xFFFF) as u16, // bits [63:48]
        ];

        let mut first = true;
        for (shift, chunk) in chunks.iter().enumerate() {
            if *chunk != 0 || (first && uimm == 0) {
                if first {
                    // MOVZ Xd, #imm16, LSL #shift*16
                    // 31|30|29|28 27 26 25 24 23|22 21|20 5|4 0
                    // 1 |1 |0 |1  0  0  1  0  1 |hw   |imm16|Rd
                    let instruction = 0xD2800000u32
                        | ((shift as u32) << 21)
                        | ((*chunk as u32) << 5)
                        | (dst as u32);
                    code_builder.emit_bytes(&instruction.to_le_bytes());
                    first = false;
                } else {
                    // MOVK Xd, #imm16, LSL #shift*16
                    // 31|30|29|28 27 26 25 24 23|22 21|20 5|4 0
                    // 1 |1 |1 |1  0  0  1  0  1 |hw   |imm16|Rd
                    let instruction = 0xF2800000u32
                        | ((shift as u32) << 21)
                        | ((*chunk as u32) << 5)
                        | (dst as u32);
                    code_builder.emit_bytes(&instruction.to_le_bytes());
                }
            }
        }

        // 如果立即数为0且没有生成任何指令，生成MOVZ x, #0
        if first && uimm == 0 {
            let instruction = 0xD2800000u32 | (dst as u32);
            code_builder.emit_bytes(&instruction.to_le_bytes());
        }
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
                (0..=31).collect::<Vec<u8>>()
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

        // 🔧 修复：确保虚拟栈空间是 16 字节对齐的
        let raw_stack_space = regs_to_virtual_stack.len() * 8;
        let virtual_stack_space = ((raw_stack_space + 15) / 16) * 16;

        // 步骤1：保存 r0-r5 到虚拟栈
        if !regs_to_virtual_stack.is_empty() {
            // 一次性调整虚拟栈指针（向下增长）
            self.emit_sub_reg_reg_imm(
                code_builder,
                karte_virtual_sp_reg,
                karte_virtual_sp_reg,
                virtual_stack_space as i32 + 32,
            );

            // 保存所有寄存器到调整后的虚拟栈上
            for (idx, reg) in regs_to_virtual_stack.iter().enumerate() {
                self.emit_str_reg_mem(code_builder, *reg, karte_virtual_sp_reg, (idx * 8) as i32);
            }
        }

        // 步骤2：只在系统栈保存 VM 帧指针，SP 依靠栈平衡自动恢复
        let vm_fp_reg = self.vm_calling_convention.frame_pointer;
        if self.debug_mode {
            log::debug!("保存 VM 帧寄存器到系统栈: fp=p{}", vm_fp_reg);
        }
        // 依旧分配 16 字节，保持与原 STP 相同的栈平衡
        self.emit_add_reg_reg_imm(
            code_builder,
            AArch64Register::SP as u8,
            AArch64Register::SP as u8,
            -16,
        );
        // 与之前 STP 的 second slot 对齐，写入 [SP, #8]
        self.emit_str_reg_mem(code_builder, vm_fp_reg, AArch64Register::SP as u8, 8);

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
        // 2. 再从虚拟栈恢复 r0-r5

        // 步骤1：恢复 VM 帧指针，并保持与保存步骤相同的栈调整
        let vm_fp_reg = self.vm_calling_convention.frame_pointer;
        self.emit_ldr_reg_mem(code_builder, vm_fp_reg, AArch64Register::SP as u8, 8);
        self.emit_add_reg_reg_imm(
            code_builder,
            AArch64Register::SP as u8,
            AArch64Register::SP as u8,
            16,
        );

        // 步骤2：恢复 r0-r5 从虚拟栈
        if !regs.is_empty() {
            let karte_virtual_sp_reg = self.vm_calling_convention.stack_pointer;

            // 先用偏移加载所有寄存器（保持虚拟SP不变）
            for (idx, reg) in regs.iter().enumerate() {
                self.emit_ldr_reg_mem(code_builder, *reg, karte_virtual_sp_reg, (idx * 8) as i32);
            }

            // 然后一次性恢复虚拟栈指针（向上增长）
            self.emit_add_reg_reg_imm(
                code_builder,
                karte_virtual_sp_reg,
                karte_virtual_sp_reg,
                stack_space as i32 + 32,
            );
        }
    }

    /// 生成ADD三寄存器指令
    fn emit_add_reg_reg_reg(&self, code_builder: &mut CodeBuilder, dst: u8, src1: u8, src2: u8) {
        // ADD <Xd>, <Xn>, <Xm>
        // 31|30|29|28 27 26 25 24 23 22 21|20 16|15 10|9 5|4 0
        // 1 |0 |0 |0  1  0  1  1  0  0  0 |Xm   |0     |Xn |Xd
        let instruction =
            0x8B000000u32 | ((src2 as u32) << 16) | ((src1 as u32) << 5) | (dst as u32);
        code_builder.emit_bytes(&instruction.to_le_bytes());
    }

    /// 生成ADD寄存器和立即数指令
    fn emit_add_reg_reg_imm(&self, code_builder: &mut CodeBuilder, dst: u8, src: u8, imm: i32) {
        // 🔧 修复：处理负立即数的情况
        if imm < 0 {
            // 负立即数：ADD dst, src, #-imm => SUB dst, src, #imm
            let abs_imm = { -imm };
            if abs_imm <= 4095 {
                // 使用SUB指令 with positive immediate
                let instruction =
                    0xD1000000u32 | ((abs_imm as u32) << 10) | ((src as u32) << 5) | (dst as u32);
                code_builder.emit_bytes(&instruction.to_le_bytes());
                return;
            } else {
                // 立即数太大，使用临时寄存器
                let temp_reg = AArch64Register::X16 as u8;
                self.emit_mov_reg_imm64(code_builder, temp_reg, imm as i64);
                self.emit_add_reg_reg_reg(code_builder, dst, src, temp_reg);
                return;
            }
        }

        // 正立即数
        if imm > 4095 {
            // 立即数超出范围，先加载到临时寄存器
            let temp_reg = AArch64Register::X16 as u8;
            self.emit_mov_reg_imm64(code_builder, temp_reg, imm as i64);
            self.emit_add_reg_reg_reg(code_builder, dst, src, temp_reg);
            return;
        }

        // ADD <Xd>, <Xn>, #imm12
        // 31|30|29|28 27 26 25 24 23 22|21 10|9 5|4 0
        // 1 |0 |0 |0  1  0  0  0  1  0 |imm12|Xn |Xd
        let instruction = 0x91000000u32 | ((imm as u32) << 10) | ((src as u32) << 5) | (dst as u32);
        code_builder.emit_bytes(&instruction.to_le_bytes());
    }

    /// 生成SUB三寄存器指令
    fn emit_sub_reg_reg_reg(&self, code_builder: &mut CodeBuilder, dst: u8, src1: u8, src2: u8) {
        // SUB <Xd>, <Xn>, <Xm>
        // 31|30|29|28 27 26 25 24 23 22 21|20 16|15 10|9 5|4 0
        // 1 |1 |0 |0  1  0  1  1  0  0  0 |Xm   |0     |Xn |Xd
        let instruction =
            0xCB000000u32 | ((src2 as u32) << 16) | ((src1 as u32) << 5) | (dst as u32);
        code_builder.emit_bytes(&instruction.to_le_bytes());
    }

    /// 生成SUB寄存器和立即数指令
    fn emit_sub_reg_reg_imm(&self, code_builder: &mut CodeBuilder, dst: u8, src: u8, imm: i32) {
        // SUB <Xd>, <Xn>, #imm12
        if !(0..=4095).contains(&imm) {
            let temp_reg = AArch64Register::X16 as u8;
            self.emit_mov_reg_imm64(code_builder, temp_reg, imm as i64);
            self.emit_sub_reg_reg_reg(code_builder, dst, src, temp_reg);
            return;
        }

        // 31|30|29|28 27 26 25 24 23 22|21 10|9 5|4 0
        // 1 |1 |0 |0  1  0  0  0  1  0 |imm12|Xn |Xd
        let instruction = 0xD1000000u32 | ((imm as u32) << 10) | ((src as u32) << 5) | (dst as u32);
        code_builder.emit_bytes(&instruction.to_le_bytes());
    }

    /// 生成AND三寄存器指令
    fn emit_and_reg_reg(&self, code_builder: &mut CodeBuilder, dst: u8, src1: u8, src2: u8) {
        // AND <Xd>, <Xn>, <Xm>
        // 31|30|29|28 27 26 25 24 23 22 21|20 16|15 10|9 5|4 0
        // 1 |0 |0 |0  0  0  1  0  0  0  0 |Xm   |0     |Xn |Xd
        let instruction =
            0x8A000000u32 | ((src2 as u32) << 16) | ((src1 as u32) << 5) | (dst as u32);
        code_builder.emit_bytes(&instruction.to_le_bytes());
    }

    /// 生成MUL三寄存器指令
    fn emit_mul_reg_reg_reg(&self, code_builder: &mut CodeBuilder, dst: u8, src1: u8, src2: u8) {
        // MUL <Xd>, <Xn>, <Xm>
        // 31|30|29|28 27 26 25 24 23|22 21|20 16|15|14 10|9 5|4 0
        // 1 |0 |0 |1  1  0  1  1  0 |0  0 |Xm   |0 |1 1 1 1|Xn |Xd
        let instruction =
            0x9B007C00u32 | ((src2 as u32) << 16) | ((src1 as u32) << 5) | (dst as u32);
        code_builder.emit_bytes(&instruction.to_le_bytes());
    }

    /// 生成DIV三寄存器指令
    fn emit_div_reg_reg_reg(&self, code_builder: &mut CodeBuilder, dst: u8, src1: u8, src2: u8) {
        // SDIV <Xd>, <Xn>, <Xm> (有符号除法)
        // 31|30|29|28 27 26 25 24 23|22 21|20 16|15|14 10|9 5|4 0
        // 1 |0 |0 |1  1  0  1  1  0 |0  0 |Xm   |0 |0 0 0 1|Xn |Xd
        let instruction =
            0x9AC00800u32 | ((src2 as u32) << 16) | ((src1 as u32) << 5) | (dst as u32);
        code_builder.emit_bytes(&instruction.to_le_bytes());
    }

    /// 生成CMP寄存器指令
    fn emit_cmp_reg_reg(&self, code_builder: &mut CodeBuilder, src1: u8, src2: u8) {
        // CMP <Xn>, <Xm> (等价于 SUBS XZR, <Xn>, <Xm>)
        // 31|30|29|28 27 26 25 24 23 22 21|20 16|15 10|9 5|4 0
        // 1 |1 |1 |0  1  0  1  1  0  0  0 |Xm   |0     |Xn |11111
        let instruction = 0xEB00001Fu32 | ((src2 as u32) << 16) | ((src1 as u32) << 5);
        code_builder.emit_bytes(&instruction.to_le_bytes());
    }

    /// 生成CMP立即数指令
    fn emit_cmp_reg_imm(&self, code_builder: &mut CodeBuilder, src: u8, imm: i32) {
        // CMP <Xn>, #imm12 (等价于 SUBS XZR, <Xn>, #imm12)
        if !(0..=4095).contains(&imm) {
            let temp_reg = AArch64Register::X16 as u8;
            self.emit_mov_reg_imm64(code_builder, temp_reg, imm as i64);
            self.emit_cmp_reg_reg(code_builder, src, temp_reg);
            return;
        }

        // 31|30|29|28 27 26 25 24 23 22|21 10|9 5|4 0
        // 1 |1 |1 |0  1  0  0  0  1  0 |imm12|Xn |11111
        let instruction = 0xF100001Fu32 | ((imm as u32) << 10) | ((src as u32) << 5);
        code_builder.emit_bytes(&instruction.to_le_bytes());
    }

    /// 生成LDR内存加载指令
    fn emit_ldr_reg_mem(&self, code_builder: &mut CodeBuilder, dst: u8, base: u8, offset: i32) {
        // LDR <Xt>, [<Xn|SP>, #offset]

        if offset >= 0 && offset % 8 == 0 && offset <= 32760 {
            // 正偏移，8字节对齐：使用 LDR (unsigned offset)
            // 指令编码: 1111 1001 01 imm12 Rn Rt
            // F9 40 0000: LDR Xt, [Xn/SP, #imm12*8]
            let scaled_offset = (offset / 8) as u32;
            let instruction =
                0xF9400000u32 | (scaled_offset << 10) | ((base as u32) << 5) | (dst as u32);
            code_builder.emit_bytes(&instruction.to_le_bytes());
        } else if offset >= -256 && offset <= 255 {
            // 小范围偏移（含负偏移、非对齐）：使用 LDUR (unscaled)
            // 指令编码: 1111 1000 01 0 imm9 00 Rn Rt
            // F8 40 0000: LDUR Xt, [Xn/SP, #simm9]
            // imm9 是 9 位有符号数，范围 -256 到 +255
            let imm9 = (offset & 0x1FF) as u32; // 取低 9 位作为有符号数
            let instruction = 0xF8400000u32 | (imm9 << 12) | ((base as u32) << 5) | (dst as u32);
            code_builder.emit_bytes(&instruction.to_le_bytes());
        } else {
            // 大偏移：使用临时寄存器 + 寄存器偏移模式
            // 1. MOV X16, #offset
            // 2. LDR Xt, [Xn, X16]
            let temp_reg = AArch64Register::X16 as u8;
            self.emit_mov_reg_imm64(code_builder, temp_reg, offset as i64);

            // LDR Xt, [Xn, Xm] - 寄存器偏移模式
            // 指令编码: 1111 1000 011 Rm 011 S 10 Rn Rt
            // F8 60 68 00: LDR Xt, [Xn, Xm, LSL #0]
            let instruction =
                0xF8606800u32 | ((temp_reg as u32) << 16) | ((base as u32) << 5) | (dst as u32);
            code_builder.emit_bytes(&instruction.to_le_bytes());
        }
    }

    /// 生成STR内存存储指令
    fn emit_str_reg_mem(&self, code_builder: &mut CodeBuilder, src: u8, base: u8, offset: i32) {
        // STR <Xt>, [<Xn|SP>, #offset]

        if offset >= 0 && offset % 8 == 0 && offset <= 32760 {
            // 正偏移，8字节对齐：使用 STR (unsigned offset)
            // 指令编码: 1111 1001 00 imm12 Rn Rt
            // F9 00 0000: STR Xt, [Xn/SP, #imm12*8]
            let scaled_offset = (offset / 8) as u32;
            let instruction =
                0xF9000000u32 | (scaled_offset << 10) | ((base as u32) << 5) | (src as u32);
            code_builder.emit_bytes(&instruction.to_le_bytes());
        } else if offset >= -256 && offset <= 255 {
            // 小范围偏移（含负偏移、非对齐）：使用 STUR (unscaled)
            // 指令编码: 1111 1000 00 0 imm9 00 Rn Rt
            // F8 00 0000: STUR Xt, [Xn/SP, #simm9]
            // imm9 是 9 位有符号数，范围 -256 到 +255
            let imm9 = (offset & 0x1FF) as u32; // 取低 9 位作为有符号数
            let instruction = 0xF8000000u32 | (imm9 << 12) | ((base as u32) << 5) | (src as u32);
            code_builder.emit_bytes(&instruction.to_le_bytes());
        } else {
            // 大偏移：使用临时寄存器 + 寄存器偏移模式
            // 1. MOV X16, #offset
            // 2. STR Xt, [Xn, X16]
            let temp_reg = AArch64Register::X16 as u8;
            self.emit_mov_reg_imm64(code_builder, temp_reg, offset as i64);

            // STR Xt, [Xn, Xm] - 寄存器偏移模式
            // 指令编码: 1111 1000 001 Rm 011 S 10 Rn Rt
            // F8 20 68 00: STR Xt, [Xn, Xm, LSL #0]
            let instruction =
                0xF8206800u32 | ((temp_reg as u32) << 16) | ((base as u32) << 5) | (src as u32);
            code_builder.emit_bytes(&instruction.to_le_bytes());
        }
    }

    /// 生成 STP (pre-index) 指令，通常用于将寄存器对压入系统栈
    fn emit_stp_pre_index(
        &self,
        code_builder: &mut CodeBuilder,
        first: u8,
        second: u8,
        base: u8,
        offset: i32,
    ) {
        debug_assert!(
            offset % 8 == 0,
            "STP offset must be 8-byte aligned: {}",
            offset
        );
        let scaled = offset / 8;
        debug_assert!(
            (-64..=63).contains(&scaled),
            "STP offset out of encodable range: {}",
            offset
        );

        let imm7 = ((scaled & 0x7F) as u32) << 15;
        let instruction = 0xA9800000u32
            | imm7
            | (((second as u32) & 0x1F) << 10)
            | (((base as u32) & 0x1F) << 5)
            | ((first as u32) & 0x1F);

        code_builder.emit_bytes(&instruction.to_le_bytes());
    }

    /// 生成 STP (offset) 指令，使用偏移寻址但不修改基址寄存器
    fn emit_stp_offset(
        &self,
        code_builder: &mut CodeBuilder,
        first: u8,
        second: u8,
        base: u8,
        offset: i32,
    ) {
        debug_assert!(
            offset % 8 == 0,
            "STP offset must be 8-byte aligned: {}",
            offset
        );
        let scaled = offset / 8;
        debug_assert!(
            (-64..=63).contains(&scaled),
            "STP offset out of encodable range: {}",
            offset
        );

        let imm7 = ((scaled & 0x7F) as u32) << 15;
        // 0xA9000000 = STP with signed offset (no writeback)
        let instruction = 0xA9000000u32
            | imm7
            | (((second as u32) & 0x1F) << 10)
            | (((base as u32) & 0x1F) << 5)
            | ((first as u32) & 0x1F);

        code_builder.emit_bytes(&instruction.to_le_bytes());
    }

    /// 生成 LDP (post-index) 指令，通常用于从系统栈恢复寄存器对
    fn emit_ldp_post_index(
        &self,
        code_builder: &mut CodeBuilder,
        first: u8,
        second: u8,
        base: u8,
        offset: i32,
    ) {
        debug_assert!(
            offset % 8 == 0,
            "LDP offset must be 8-byte aligned: {}",
            offset
        );
        let scaled = offset / 8;
        debug_assert!(
            (-64..=63).contains(&scaled),
            "LDP offset out of encodable range: {}",
            offset
        );

        let imm7 = ((scaled & 0x7F) as u32) << 15;
        let instruction = 0xA8C00000u32
            | imm7
            | (((second as u32) & 0x1F) << 10)
            | (((base as u32) & 0x1F) << 5)
            | ((first as u32) & 0x1F);

        code_builder.emit_bytes(&instruction.to_le_bytes());
    }

    /// 生成 LDP (offset) 指令，使用偏移寻址但不修改基址寄存器
    fn emit_ldp_offset(
        &self,
        code_builder: &mut CodeBuilder,
        first: u8,
        second: u8,
        base: u8,
        offset: i32,
    ) {
        debug_assert!(
            offset % 8 == 0,
            "LDP offset must be 8-byte aligned: {}",
            offset
        );
        let scaled = offset / 8;
        debug_assert!(
            (-64..=63).contains(&scaled),
            "LDP offset out of encodable range: {}",
            offset
        );

        let imm7 = ((scaled & 0x7F) as u32) << 15;
        // 0xA9400000 = LDP with signed offset (no writeback)
        let instruction = 0xA9400000u32
            | imm7
            | (((second as u32) & 0x1F) << 10)
            | (((base as u32) & 0x1F) << 5)
            | ((first as u32) & 0x1F);

        code_builder.emit_bytes(&instruction.to_le_bytes());
    }

    fn get_c_ffi_callee_saved_registers(&self) -> Vec<u8> {
        self.ffi_calling_convention
            .get_callee_save_registers(&self.current_function_use_regs)
    }

    fn get_vm_callee_saved_registers(&self) -> Vec<u8> {
        self.vm_calling_convention
            .get_callee_save_registers(&self.current_function_use_regs)
    }

    /// 生成函数序言
    fn emit_function_prologue(&self, code_builder: &mut CodeBuilder) -> Result<(), String> {
        // AArch64 AAPCS64调用约定：X0和X1为前两个参数
        // 参考x86实现，将参数移动到虚拟机寄存器
        let x0 = AArch64Register::X0 as u8; // 第一个参数：虚拟栈顶地址
        let x1 = AArch64Register::X1 as u8; // 第二个参数：虚拟栈底地址
        let vm_sp = self.vm_calling_convention.stack_pointer;
        let vm_fp = self.vm_calling_convention.frame_pointer;

        if self.debug_mode {
            log::debug!("序言开始：生成符合 AAPCS64 的函数序言");
        }

        // AAPCS64 标准序言：
        // 1. 为系统栈分配帧空间（32字节）
        // SUB SP, SP, #32
        self.emit_add_reg_reg_imm(
            code_builder,
            AArch64Register::SP as u8,
            AArch64Register::SP as u8,
            -32,
        );

        // 2. 保存 x29 到系统栈（X30稍后保存到虚拟栈）
        // STR X29, [SP, #0]
        self.emit_str_reg_mem(
            code_builder,
            AArch64Register::X29 as u8,
            AArch64Register::SP as u8,
            0,
        );

        // 3. 设置新帧指针
        // MOV X29, SP
        self.emit_mov_reg_reg(
            code_builder,
            AArch64Register::X29 as u8,
            AArch64Register::SP as u8,
        );

        // 4. 保存其他 callee-saved 寄存器（如果有的话）
        self.save_callee_saved_registers(code_builder)?;

        // 5. 保存系统SP到X16
        // MOV X16, SP
        self.emit_mov_reg_reg(code_builder, 16, AArch64Register::SP as u8);

        // 6. 切换到虚拟栈
        // MOV SP, X0 (x0 = 虚拟栈顶地址)
        // MOV X29, X1 (x1 = 虚拟栈底地址)
        self.emit_mov_reg_reg(code_builder, vm_sp, x0);
        self.emit_mov_reg_reg(code_builder, vm_fp, x1);

        // 7. 在虚拟栈保存系统SP和X30
        // SUB SP, SP, #16
        // STR X16, [SP, #0]  (系统SP)
        // STR X30, [SP, #8]  (返回地址)
        self.emit_add_reg_reg_imm(
            code_builder,
            AArch64Register::SP as u8,
            AArch64Register::SP as u8,
            -16,
        );
        self.emit_str_reg_mem(code_builder, 16, AArch64Register::SP as u8, 0);
        self.emit_str_reg_mem(
            code_builder,
            AArch64Register::X30 as u8,
            AArch64Register::SP as u8,
            8,
        );

        // 8. 为返回值槽分配空间（16字节对齐）
        self.save_return_slot_pointer(code_builder);

        // AAPCS64 要求：栈必须在函数入口处16字节对齐
        // 检查当前栈使用情况：
        // - 每个 STP 指令分配 16 字节
        // - callee-saved 寄存器数量决定栈使用量
        let stack_usage = 16 * (2 + (self.get_c_ffi_callee_saved_registers().len() + 1) / 2); // X29/X30 + X6/X7 + callee-saved
        if self.debug_mode {
            log::debug!("序言：栈使用量 = {} 字节", stack_usage);
            log::debug!(
                "序言：{} 个 callee-saved 寄存器",
                self.get_c_ffi_callee_saved_registers().len()
            );
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
        self.emit_sub_reg_reg_imm(code_builder, vm_sp_reg, vm_sp_reg, 16);
        self.emit_str_reg_mem(code_builder, vm_fp_reg, vm_sp_reg, 8);
        self.emit_str_reg_mem(code_builder, vm_sp_reg, vm_sp_reg, 0);

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
            self.emit_sub_reg_reg_imm(code_builder, vm_sp_reg, vm_sp_reg, 16);

            // 存储寄存器值到虚拟栈
            self.emit_str_reg_mem(code_builder, reg, vm_sp_reg, 0);
        }

        if self.debug_mode {
            eprintln!(
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
            self.emit_ldr_reg_mem(code_builder, reg, vm_sp_reg, 0);

            // 弹出虚拟栈（16字节对齐）
            self.emit_add_reg_reg_imm(code_builder, vm_sp_reg, vm_sp_reg, 16);
        }

        if self.debug_mode {
            log::debug!(
                "恢复了 {} 个 callee-saved 寄存器从虚拟栈",
                callee_saved.len()
            );
        }

        // 恢复fp sp从虚拟栈
        self.emit_ldr_reg_mem(code_builder, vm_sp_reg, vm_sp_reg, 0);
        self.emit_ldr_reg_mem(code_builder, vm_fp_reg, vm_sp_reg, 8);
        self.emit_add_reg_reg_imm(code_builder, vm_sp_reg, vm_sp_reg, 16);

        Ok(())
    }

    /// 生成主函数尾声（用于与宿主环境交互的main函数）
    fn emit_function_epilogue(&self, code_builder: &mut CodeBuilder) -> Result<(), String> {
        // AAPCS64 标准尾声：按照序言的逆序恢复寄存器
        // 注意：进入尾声时，SP指向虚拟栈，X29可能也指向虚拟栈

        // 1. 不处理返回值槽（由 compile_return 负责）

        // 2. 从系统栈帧恢复系统SP（需要知道系统栈帧的X29值）
        // 问题：X29现在指向虚拟栈，无法直接访问系统栈帧
        // 解决方案：系统栈帧的X29保存在系统栈 [系统SP, #0] 位置
        // 但我们需要先知道系统SP...这是个循环依赖

        // 新方案：利用虚拟栈底（X1参数）来定位保存的系统栈指针
        // 实际上，我们应该在序言中将系统栈信息保存到一个固定可访问的位置

        // 临时方案：使用X19作为系统栈帧指针寄存器
        // 在序言中保存系统X29到X19，这里从X19恢复

        // 但这不可行，因为X19可能被使用...

        // 正确方案：恢复系统栈的流程应该是：
        // 1. 从某个已知位置读取保存的系统SP
        // 2. MOV SP, 系统SP
        // 3. 从系统栈恢复 callee-saved 寄存器
        // 4. 从系统栈恢复 X29, X30
        // 5. 释放系统栈帧

        // 关键问题：如何从虚拟栈状态访问系统栈保存的值？
        // 答案：在序言中，我们将系统SP保存到了 [X29(系统帧), #16]
        // 但现在X29指向虚拟栈，我们无法访问系统帧的X29

        // 解决方案：在序言中，除了将系统SP保存到系统栈，也保存到虚拟栈的固定位置
        // 或者：使用一个callee-saved寄存器（如X19）来保存系统帧指针

        // 让我重新设计：使用X19保存系统栈帧指针
        // 序言：MOV X19, X29（系统帧）
        // 尾声：LDR X16, [X19, #16]（从系统帧读取保存的系统SP）

        // 但这要求X19不被使用，或者需要额外保存X19...

        // 最简单的方案：将系统SP保存到虚拟栈底（通过X1参数）的固定偏移位置
        // 但这会污染虚拟栈

        // 实际上，让我重新思考整个设计：
        // 目标：支持从虚拟栈恢复到系统栈
        // 约束：切换到虚拟栈后，无法直接访问系统栈帧
        // 解决方案选项：
        // 1. 使用 callee-saved 寄存器保存系统栈信息（但需要额外保存该寄存器）
        // 2. 将系统栈信息保存到虚拟栈（简单但占用虚拟栈空间）
        // 3. 使用全局变量保存系统栈信息（线程不安全）

        // 选择方案2：将系统SP保存到虚拟栈顶部固定位置

        // 修改后的设计：
        // 序言：
        // 1. 在系统栈分配帧并保存X29/X30
        // 2. 保存callee-saved寄存器到系统栈
        // 3. 切换到虚拟栈
        // 4. 在虚拟栈分配空间并保存系统SP
        // 尾声：
        // 1. 从虚拟栈读取系统SP
        // 2. 切换回系统栈
        // 3. 恢复callee-saved寄存器
        // 4. 恢复X29/X30并释放帧

        // 实现：假设序言在虚拟栈 [SP, #8] 保存了系统SP

        // 虚拟栈不需要恢复，直接切换到系统栈即可

        // 关键修复：序言中保存系统SP到系统栈帧 [X29, #16]
        // 这里需要先找到系统栈帧的X29

        // 重新审视问题：序言保存系统SP到 [系统X29, #16]
        // 但切换到虚拟栈后，X29被覆盖为虚拟X29
        // 所以我们需要在切换前，将系统X29保存到某处

        // 新方案：在序言中，将系统X29保存到虚拟栈的固定位置
        // 尾声中，从虚拟栈读取系统X29，然后从系统栈读取系统SP

        // 等等，我想复杂了。让我重新看看序言代码...

        // 看序言代码：
        // 5. MOV X16, SP（此时SP是系统SP）
        // 6. STR X16, [X29, #16]（X29是系统帧指针）
        // 7. 切换到虚拟栈

        // 所以系统SP确实保存在系统栈帧的 [系统X29, #16]
        // 但我们切换到虚拟栈后，X29变成了虚拟X29

        // 关键insight：系统X29保存在系统栈 [系统SP, #0]
        // 而系统SP保存在系统栈 [系统X29, #16]
        // 这是循环依赖！

        // 解决方案：在切换到虚拟栈前，计算好系统栈帧的基址，并保存到虚拟栈
        // 或者：序言中，在切换到虚拟栈后，将系统栈信息保存到虚拟栈

        // 最简洁的方案：
        // 序言：SUB SP(系统), #32 → 保存X29/X30 → MOV X29(系统), SP →
        //      保存callee-saved → MOV X16, SP(系统当前值包含callee-saved) →
        //      切换到虚拟栈 → SUB SP(虚拟), #16 → STR X16, [SP(虚拟), #8] → ...
        // 尾声：... → LDR X16, [SP(虚拟)+偏移, #8] → 切换回系统栈 → ...

        // 让我直接实现，假设序言将系统SP保存到了 [系统X29, #16]，
        // 同时也保存到虚拟栈的某个位置

        // 实际上，查看序言最后的save_return_slot_pointer，它会：
        // SUB SP, #16
        // STR X0, [SP, #0]
        // 所以虚拟栈布局是：
        // [SP+0]: 返回值槽指针(X0)
        // [SP+8]: 未使用
        // [SP+16]: 虚拟栈上可能还有其他数据

        // 我的修改后序言会是：
        // 系统栈：分配32字节，保存X29/X30/callee-saved/系统SP到系统栈
        // 虚拟栈：只保存返回值槽指针

        // 问题：我修改后的序言不再将系统SP保存到虚拟栈！
        // 所以尾声无法从虚拟栈读取系统SP

        // 我需要修改序言，在虚拟栈也保存系统SP，或者在尾声中想办法访问系统栈

        // 实际上，可以利用这个事实：callee-saved寄存器中可能有某个寄存器没被使用
        // 或者，使用一个临时寄存器（如X17）来传递系统帧信息

        // 更简单的方案：既然系统X29保存在系统栈 [系统SP, #0]，
        // 而系统SP保存在 [系统X29, #16]，
        // 我们可以在序言中，除了保存到系统栈，也保存一份到虚拟栈

        // 或者，最最简单的方案：使用一个全局变量/寄存器来保存系统栈帧指针
        // 但这需要额外的机制

        // 让我采用最直接的方案：在虚拟栈固定位置保存系统SP

        // 修改序言为：
        // 1-5. 在系统栈setup帧并保存系统SP到[X29, #16]
        // 6. 切换到虚拟栈
        // 7. SUB SP(虚拟), #16
        // 8. STR X16(系统SP), [SP(虚拟), #8]
        // 9. 调用save_return_slot_pointer（会再分配16字节）

        // 这样虚拟栈布局是：
        // [SP+0]: 返回值槽指针
        // [SP+16]: 系统SP保存位置 [SP+16+8]

        // 尾声：
        // 1. 跳过返回值槽：ADD SP, #16
        // 2. 读取系统SP：LDR X16, [SP, #8]
        // 3. 回收保存系统SP的空间：ADD SP, #16
        // 4. 切换回系统栈：MOV SP, X16
        // 5. 恢复callee-saved
        // 6. 恢复X29/X30

        // 这个方案可行！让我实现它

        // 但我刚才的序言修改没有在虚拟栈保存系统SP！我需要补上

        // 对了，我可以直接在这里写尾声，然后回头修改序言

        // 尾声实现（假设虚拟栈布局如上所述）：

        // compile_return已经弹出了返回值槽（+16字节）
        // 虚拟栈布局（compile_return后）：
        // [SP+0]: 系统SP
        // [SP+8]: X30

        // 1. 从虚拟栈读取X30和系统SP
        self.emit_ldr_reg_mem(
            code_builder,
            AArch64Register::X30 as u8,
            AArch64Register::SP as u8,
            8,
        );
        self.emit_ldr_reg_mem(code_builder, 16, AArch64Register::SP as u8, 0);

        // 2. 切换回系统栈
        self.emit_mov_reg_reg(code_builder, AArch64Register::SP as u8, 16);

        // 3. 恢复 callee-saved 寄存器（从系统栈）
        self.restore_callee_saved_registers(code_builder)?;

        // 4. 恢复 X29
        // LDR X29, [SP, #0]
        self.emit_ldr_reg_mem(
            code_builder,
            AArch64Register::X29 as u8,
            AArch64Register::SP as u8,
            0,
        );

        // 5. 释放系统栈帧（32字节）
        self.emit_add_reg_reg_imm(
            code_builder,
            AArch64Register::SP as u8,
            AArch64Register::SP as u8,
            32,
        );

        Ok(())
    }

    /// 保存返回槽指针（caller通过X0传入）
    fn save_return_slot_pointer(&self, code_builder: &mut CodeBuilder) {
        self.emit_add_reg_reg_imm(
            code_builder,
            AArch64Register::SP as u8,
            AArch64Register::SP as u8,
            -16,
        );
        self.emit_str_reg_mem(
            code_builder,
            AArch64Register::X0 as u8,
            AArch64Register::SP as u8,
            0,
        );
    }

    /// 恢复返回槽指针并弹出栈空间
    fn load_and_pop_return_slot_pointer(&self, code_builder: &mut CodeBuilder, dst: u8) {
        self.emit_ldr_reg_mem(code_builder, dst, AArch64Register::SP as u8, 0);
        self.emit_add_reg_reg_imm(
            code_builder,
            AArch64Register::SP as u8,
            AArch64Register::SP as u8,
            16,
        );
    }

    /// 将当前X0返回值写入返回槽地址
    fn emit_store_return_value_to_slot(&self, code_builder: &mut CodeBuilder, slot_reg: u8) {
        self.emit_str_reg_mem(code_builder, AArch64Register::X0 as u8, slot_reg, 0);
        // 🔧 修复：不要修改X0，保持返回值在X0中
        // self.emit_mov_reg_reg(code_builder, AArch64Register::X0 as u8, slot_reg);
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

    /// 保存 callee-saved 寄存器
    fn save_callee_saved_registers(&self, code_builder: &mut CodeBuilder) -> Result<(), String> {
        let callee_saved = &self.get_c_ffi_callee_saved_registers();
        if callee_saved.is_empty() {
            return Ok(());
        }

        if self.debug_mode {
            log::debug!("保存 callee-saved 寄存器: {:?}", callee_saved);
        }

        // 使用 STP 成对保存（确保 16 字节对齐）
        let mut regs = callee_saved.clone();
        // 🔧 修复：X29/X30 在标准序言中单独保存，这里排除
        regs.retain(|&r| r != 29 && r != 30);

        if regs.is_empty() {
            return Ok(());
        }

        // 排序以确保确定性的输出
        regs.sort_unstable();

        if self.debug_mode {
            log::debug!("准备保存的 callee-saved 寄存器（排序后）: {:?}", regs);
        }

        // AAPCS64 要求栈16字节对齐，STP指令总是操作16字节
        // 成对保存寄存器，如果是奇数个需要额外填充
        let pairs = regs.chunks_exact(2);
        let remainder = pairs.remainder();

        // 处理完整的寄存器对
        for chunk in pairs {
            let reg1 = chunk[0];
            let reg2 = chunk[1];

            // 将 Karte 物理寄存器映射到 AArch64 寄存器
            let aarch64_reg1 = self.get_physical_register(&Register::Physical(reg1))?;
            let aarch64_reg2 = self.get_physical_register(&Register::Physical(reg2))?;

            // STP Xreg1, Xreg2, [SP, #-16]!
            // 🔧 修复：正确的 STP 指令编码
            // - 基址 0xA9BF03E0 包含 Rn = 31 (SP)
            // - Rt  (reg1) 在 bits [4:0]，shift = 0
            // - Rt2 (reg2) 在 bits [14:10]，shift = 10
            let instruction = 0xA9BF03E0u32
                | ((aarch64_reg1 as u32 & 0x1F) << 0)
                | ((aarch64_reg2 as u32 & 0x1F) << 10);

            if self.debug_mode {
                log::debug!(
                    "生成 STP 指令: p{} -> X{}, p{} -> X{}",
                    reg1,
                    aarch64_reg1,
                    reg2,
                    aarch64_reg2
                );
            }

            code_builder.emit_bytes(&instruction.to_le_bytes());
        }

        // 如果有奇数个寄存器，需要特殊处理
        if !remainder.is_empty() {
            let reg = remainder[0];
            let aarch64_reg = self.get_physical_register(&Register::Physical(reg))?;

            if self.debug_mode {
                log::debug!(
                    "奇数个 callee-saved 寄存器，使用 STP 配合零寄存器保存 p{} -> X{}",
                    reg,
                    aarch64_reg
                );
            }

            // 使用 XZR (零寄存器) 作为配对
            // STP Xreg, XZR, [SP, #-16]!
            // 🔧 修复：正确的 STP 指令编码
            // - 基址 0xA9BF7FE0 包含 Rn = 31 (SP) 和 Rt2 = 31 (XZR)
            // - Rt (reg) 在 bits [4:0]
            let instruction = 0xA9BF7FE0u32 | ((aarch64_reg as u32 & 0x1F) << 0);
            code_builder.emit_bytes(&instruction.to_le_bytes());
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

        // 准备恢复列表（需要逆序）
        let mut regs = callee_saved.clone();
        // 排除 X29/X30（在标准尾声中单独恢复）
        regs.retain(|&r| r != 29 && r != 30);

        if regs.is_empty() {
            return Ok(());
        }

        // 排序以确保确定性的输出（保存时排序了，恢复时也要排序以便逆序）
        regs.sort_unstable();

        if self.debug_mode {
            log::debug!("准备恢复的 callee-saved 寄存器（排序后）: {:?}", regs);
        }

        // 成对和奇数处理
        let pairs = regs.chunks_exact(2);
        let remainder = pairs.remainder();

        // 收集所有需要恢复的指令
        // 🔧 修复：恢复顺序必须与保存顺序完全相反
        // 保存时：pairs正序 + remainder = [19/20, 21/22, 23/24, 25/26] + [27/XZR]
        // 栈布局（从栈顶到栈底）：[27/XZR, 25/26, 23/24, 21/22, 19/20]
        // 恢复时：应该 [27/XZR, 25/26, 23/24, 21/22, 19/20]
        let mut restore_instructions = Vec::new();

        // 1. 先处理奇数寄存器（在栈顶，需要先恢复）
        if !remainder.is_empty() {
            let reg = remainder[0];
            let aarch64_reg = self.get_physical_register(&Register::Physical(reg))?;

            if self.debug_mode {
                log::debug!(
                    "恢复奇数个 callee-saved 寄存器: p{} -> X{}",
                    reg,
                    aarch64_reg
                );
            }

            // LDP Xreg, XZR, [SP], #16
            let instruction = 0xA8C17FE0u32 | ((aarch64_reg as u32 & 0x1F) << 0);
            restore_instructions.push(instruction);
        }

        // 2. 然后逆序处理完整的寄存器对
        for chunk in pairs.rev() {
            let reg1 = chunk[0];
            let reg2 = chunk[1];

            let aarch64_reg1 = self.get_physical_register(&Register::Physical(reg1))?;
            let aarch64_reg2 = self.get_physical_register(&Register::Physical(reg2))?;

            // LDP Xreg1, Xreg2, [SP], #16
            let instruction = 0xA8C103E0u32
                | ((aarch64_reg1 as u32 & 0x1F) << 0)
                | ((aarch64_reg2 as u32 & 0x1F) << 10);

            restore_instructions.push(instruction);
        }

        // 3. 正序生成恢复指令（已经按照正确的恢复顺序收集了）
        for instruction in restore_instructions {
            code_builder.emit_bytes(&instruction.to_le_bytes());
        }

        Ok(())
    }
}

/// 实现JitCompiler trait
impl JitCompiler for AArch64Compiler {
    /// 编译函数
    fn compile_function(
        &mut self,
        function: &LirFunction,
        program: &LirProgram,
    ) -> Result<CompiledFunction, String> {
        if self.debug_mode {
            log::debug!("AArch64: 开始编译函数 '{}'", function.name);
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
            "AArch64: 函数 '{}' 编译完成，机器码大小: {} 字节\n{}",
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
        "aarch64"
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

fn align_to(value: usize, alignment: usize) -> usize {
    ((value + alignment - 1) / alignment) * alignment
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::professional_executor::ProfessionalExecutor;
    use karte_lexer::tokenize;
    use karte_lir::lower::lower_mir_to_lir;
    use karte_lir::optimization_pipeline::{OptimizationLevel, OptimizationPipeline};
    use karte_mir::lower::lower_expr_to_mir;
    use karte_parser::parse_with_type_check;
    use log::LevelFilter;
    use std::env;

    /// 完整的编译到LIR的流程
    fn compile_to_lir(input: &str) -> Result<LirProgram, Box<dyn std::error::Error>> {
        log::debug!("=== 开始编译源代码到LIR ===");
        log::debug!("源代码: {}", input);

        // 词法分析
        log::debug!("\n1. 词法分析");
        let (tokens, lex_diagnostics) = tokenize(input);
        if !lex_diagnostics.is_empty() {
            log::debug!("词法分析诊断信息:");
            lex_diagnostics.print_fancy(input, "test").unwrap();
            if lex_diagnostics.has_errors() {
                return Err("词法分析失败".into());
            }
        }
        log::debug!("词法分析成功，生成 {} 个词法单元", tokens.len());

        // 语法分析和类型检查
        log::debug!("\n2. 语法分析和类型检查");
        let (result, parse_diagnostics) =
            parse_with_type_check(&tokens, karte_parser::ParserMode::Script, None);
        if !parse_diagnostics.is_empty() {
            log::debug!("语法分析诊断信息:");
            parse_diagnostics.print_fancy(input, "test").unwrap();
            if parse_diagnostics.has_errors() {
                return Err("语法分析或类型检查失败".into());
            }
        }

        let result = result.ok_or("表达式解析或类型检查失败")?;
        log::debug!("语法分析成功，AST: {}", result.expr());
        log::debug!("类型: {}", result.result_type);

        // Lowering to MIR
        log::debug!("\n3. 降级到MIR");
        let mir_program = match lower_expr_to_mir(result.expr()) {
            Ok(prog) => {
                log::debug!("MIR生成成功");
                log::debug!("{}", prog);
                prog
            }
            Err(errors) => {
                log::debug!("MIR生成失败:");
                for err in &errors {
                    log::debug!("  - {}", err);
                }
                return Err(format!("MIR降级失败: {:?}", errors).into());
            }
        };

        // Lowering to LIR
        log::debug!("\n4. 降级到LIR");
        let mut lir_program = match lower_mir_to_lir(&mir_program) {
            Ok(prog) => {
                log::debug!("LIR生成成功");
                log::debug!("{}", prog);
                prog
            }
            Err(errors) => {
                log::debug!("LIR生成失败:");
                for err in &errors {
                    log::debug!("  - {}", err);
                }
                return Err(format!("LIR降级失败: {:?}", errors).into());
            }
        };

        // LIR 优化
        log::debug!("\n5. LIR优化");
        let mut pipeline = OptimizationPipeline::new(OptimizationLevel::Performance);
        match pipeline.optimize(&mut lir_program) {
            Ok(stats) => {
                log::debug!("优化完成:");
                log::debug!("  - 总耗时: {}ms", stats.total_time_ms);
                log::debug!("  - 执行pass数: {}", stats.passes_executed);
                println!(
                    "  - 指令数变化: {} -> {}",
                    stats.instructions_before, stats.instructions_after
                );
            }
            Err(errors) => {
                log::debug!("优化失败:");
                for err in &errors {
                    log::debug!("  - {}", err);
                }
                return Err(format!("LIR优化失败: {:?}", errors).into());
            }
        }

        // 注意：指令降级现在已在优化管线中自动执行
        log::debug!("\n6. 指令降级（已在优化管线中完成）");
        log::debug!("指令降级成功");

        log::debug!("\n=== 最终生成的LIR程序 ===");
        log::debug!("{}", lir_program);

        Ok(lir_program)
    }

    #[test]
    #[ignore]
    fn test_direct_machine_code() {
        log::debug!("=== 直接机器码测试 ===");

        // 测试简化的AArch64机器码
        let test_code = vec![
            // 简单的函数：输入参数在X0，返回X0*2
            0xFD, 0x7B, 0xBF, 0xA9, // stp x29, x30, [sp, #-16]!
            0xFD, 0x03, 0x00, 0x91, // mov x29, sp
            0x00, 0x04, 0x00, 0x11, // add w0, w0, w0 (x0 = x0 * 2)
            0xFD, 0x7B, 0xC1, 0xA8, // ldp x29, x30, [sp], #16
            0xC0, 0x03, 0x5F, 0xD6, // ret
        ];

        log::debug!("测试机器码长度: {} 字节", test_code.len());
        log::debug!("机器码内容:");
        for (i, byte) in test_code.iter().enumerate() {
            if i % 16 == 0 {
                print!("  {:04X}: ", i);
            }
            print!("{:02X} ", byte);
            if i % 16 == 15 {
                println!();
            }
        }
        if test_code.len() % 16 != 0 {
            println!();
        }

        // 创建机器码缓冲区并执行测试
        use crate::vm::professional_executor::jit::memory_manager::JitMemoryManager;

        let mut memory_manager = JitMemoryManager::new(true);

        // 分配并写入测试代码
        match memory_manager.allocate_executable_memory(&test_code) {
            Ok(executable_memory) => {
                log::debug!(
                    "分配内存成功: 地址=0x{:X}",
                    executable_memory.address() as usize
                );

                log::debug!("机器码写入完成");

                // 注意：不直接执行测试代码，只验证分配和写入过程
                log::debug!("测试完成：机器码分配和写入成功");

                // 清理
                if let Err(e) = memory_manager.deallocate_memory(executable_memory) {
                    log::debug!("内存释放失败: {}", e);
                }
            }
            Err(err) => {
                log::debug!("内存分配失败: {}", err);
            }
        }
    }

    #[test]
    #[ignore]
    fn test_jit_lambda() {
        use std::io::Write;
        // 初始化日志系统
        env_logger::builder()
            .filter_level(LevelFilter::Debug)
            .format(|buf, record| writeln!(buf, "{}: {}", record.level(), record.args()))
            .init();

        log::debug!("\n=== 测试JIT编译和执行 ===");
        log::debug!("测试用例: 简单lambda函数");

        // 测试源代码：let f = |x| x * 2; f(5)
        let source_code = "let a = 1;let b = 2;let c = 3; let d = |e,f,g|e+f+g;d(a,b,c)";

        // 编译到LIR
        let lir_program = match compile_to_lir(source_code) {
            Ok(program) => program,
            Err(err) => {
                panic!("编译失败: {}", err);
            }
        };

        log::debug!("\n=== 创建JIT执行器 ===");
        // 创建JIT执行器
        let mut executor = match ProfessionalExecutor::new_with_jit(true) {
            Ok(exec) => {
                log::debug!("JIT执行器创建成功");
                exec
            }
            Err(err) => {
                panic!("创建JIT执行器失败: {}", err);
            }
        };

        log::debug!("\n=== 执行JIT编译的程序 ===");
        // 执行程序
        match executor.execute_with_jit(&lir_program) {
            Ok(result) => {
                // 预期结果应该是6 (1 + 2 + 3)
                assert_eq!(result, 6, "JIT执行结果错误：期望6，实际得到{}", result);
                log::debug!("JIT执行成功，结果: {} (预期值: 6)", result);
            }
            Err(err) => {
                panic!("JIT执行失败: {}", err);
            }
        }
    }

    #[test]
    #[ignore]
    fn test_continuous_memory_architecture() {
        use std::io::Write;
        // 初始化日志系统
        env_logger::builder()
            .filter_level(LevelFilter::Debug)
            .format(|buf, record| writeln!(buf, "{}: {}", record.level(), record.args()))
            .init();

        log::debug!("\n=== 测试连续内存架构 ===");
        log::debug!("测试用例: 多函数程序的连续内存分配和相对跳转");

        // 测试源代码：定义辅助函数和主函数
        let source_code = "let double = |x| x * 2; let add_one = |x| x + 1; double(add_one(5))";

        // 编译到LIR
        let _lir_program = match compile_to_lir(source_code) {
            Ok(program) => {
                log::debug!("编译成功，函数数量: {}", program.functions.len());
                for (name, func) in &program.functions {
                    log::debug!("函数 '{}': {} 条指令", name, func.instructions.len());
                }
                program
            }
            Err(err) => {
                panic!("编译失败: {}", err);
            }
        };

        log::debug!("\n=== 创建连续内存JIT执行器 ===");
        // 使用新的内存管理器测试连续内存分配
        let mut memory_manager =
            crate::vm::professional_executor::jit::memory_manager::JitMemoryManager::new(true);
        memory_manager.initialize().expect("内存管理器初始化失败");

        // 模拟编译多个函数到连续内存
        let test_functions = vec![
            ("func_double", vec![0x1F, 0x20, 0x03, 0xD5]), // nop指令作为示例
            ("func_add_one", vec![0x1F, 0x20, 0x03, 0xD5]),
            ("main", vec![0x1F, 0x20, 0x03, 0xD5]),
        ];

        for (func_name, machine_code) in test_functions {
            match memory_manager.allocate_function_memory(func_name, &machine_code) {
                Ok(exec_mem) => {
                    log::debug!(
                        "函数 '{}' 分配成功: 偏移=0x{:X}, 地址={:p}",
                        func_name,
                        exec_mem.offset(),
                        exec_mem.address()
                    );
                }
                Err(err) => {
                    panic!("函数 '{}' 分配失败: {}", func_name, err);
                }
            }
        }

        // 测试相对偏移计算
        if let Some(relative_offset) =
            memory_manager.calculate_relative_offset("main", "func_double")
        {
            log::debug!("main -> func_double 相对偏移: 0x{:X}", relative_offset);
            assert!(
                relative_offset.abs() < 1024 * 1024,
                "相对偏移应该在合理范围内"
            );
        }

        // 测试内存使用统计
        let (used, total, functions) = memory_manager.get_usage_stats();
        log::debug!(
            "内存使用统计: 已用={}字节, 总计={}MB, 函数数={}",
            used,
            total / (1024 * 1024),
            functions
        );

        assert_eq!(functions, 3, "应该有3个函数");
        assert!(used > 0, "应该有内存使用");
        assert!(used < total, "使用量应该小于总量");

        log::debug!("连续内存架构测试通过！");
    }

    #[test]
    #[ignore]
    fn test_relative_jump_range() {
        log::debug!("\n=== 测试相对跳转范围 ===");

        // AArch64 BL指令的跳转范围是±128MB
        const MAX_RELATIVE_JUMP: i64 = 128 * 1024 * 1024;

        let mut memory_manager =
            crate::vm::professional_executor::jit::memory_manager::JitMemoryManager::new(true);
        memory_manager.initialize().expect("内存管理器初始化失败");

        // 分配一个函数在开始位置
        let func1_code = vec![0x1F, 0x20, 0x03, 0xD5]; // nop
        memory_manager
            .allocate_function_memory("func_start", &func1_code)
            .unwrap();

        // 分配多个小函数，逐渐增加偏移
        for i in 0..10 {
            let func_name = format!("func_{}", i);
            // 创建1KB的nop指令 (每个nop是4字节，所以256个nop = 1024字节)
            let nop_instruction = [0x1F, 0x20, 0x03, 0xD5]; // 单个nop指令
            let dummy_code = nop_instruction.repeat(256); // 重复256次得到1KB
            memory_manager
                .allocate_function_memory(&func_name, &dummy_code)
                .unwrap();
        }

        // 计算最远的相对偏移
        if let Some(max_offset) = memory_manager.calculate_relative_offset("func_start", "func_9") {
            log::debug!(
                "最大相对偏移: 0x{:X} ({} KB)",
                max_offset,
                max_offset / 1024
            );
            assert!(
                max_offset.abs() < MAX_RELATIVE_JUMP,
                "相对偏移 {} 应该在AArch64 BL指令范围 ±{} 内",
                max_offset,
                MAX_RELATIVE_JUMP
            );
        }

        log::debug!("相对跳转范围测试通过！");
    }

    #[test]
    #[ignore]
    fn test_all_comparison_instructions() {
        use std::io::Write;
        // 初始化日志系统
        env_logger::builder()
            .filter_level(LevelFilter::Debug)
            .format(|buf, record| writeln!(buf, "{}: {}", record.level(), record.args()))
            .init();

        log::debug!("\n=== 测试所有比较指令 ===");
        log::debug!("测试用例: 验证所有6种比较指令都能正确编译和执行");

        // 测试所有比较操作的源代码
        let test_cases = vec![
            (
                "相等比较",
                "let x = 5; let y = 5; if x == y { 1 } else { 0 }",
                1,
            ),
            (
                "不相等比较",
                "let x = 5; let y = 3; if x != y { 1 } else { 0 }",
                1,
            ),
            (
                "小于比较",
                "let x = 3; let y = 5; if x < y { 1 } else { 0 }",
                1,
            ),
            (
                "小于等于比较",
                "let x = 5; let y = 5; if x <= y { 1 } else { 0 }",
                1,
            ),
            (
                "大于比较",
                "let x = 7; let y = 5; if x > y { 1 } else { 0 }",
                1,
            ),
            (
                "大于等于比较",
                "let x = 5; let y = 5; if x >= y { 1 } else { 0 }",
                1,
            ),
            // 反向测试：条件为假的情况
            (
                "相等比较(假)",
                "let x = 5; let y = 3; if x == y { 1 } else { 0 }",
                0,
            ),
            (
                "不相等比较(假)",
                "let x = 5; let y = 5; if x != y { 1 } else { 0 }",
                0,
            ),
            (
                "小于比较(假)",
                "let x = 7; let y = 5; if x < y { 1 } else { 0 }",
                0,
            ),
            (
                "小于等于比较(假)",
                "let x = 7; let y = 5; if x <= y { 1 } else { 0 }",
                0,
            ),
            (
                "大于比较(假)",
                "let x = 3; let y = 5; if x > y { 1 } else { 0 }",
                0,
            ),
            (
                "大于等于比较(假)",
                "let x = 3; let y = 5; if x >= y { 1 } else { 0 }",
                0,
            ),
        ];

        for (test_name, source_code, expected_result) in test_cases {
            log::debug!("\n--- 测试: {} ---", test_name);
            log::debug!("源代码: {}", source_code);
            log::debug!("期望结果: {}", expected_result);

            // 编译到LIR
            let lir_program = match compile_to_lir(source_code) {
                Ok(program) => {
                    log::debug!("编译成功");
                    program
                }
                Err(err) => {
                    panic!("编译失败: {}", err);
                }
            };

            // 创建JIT执行器
            let mut executor = match ProfessionalExecutor::new_with_jit(true) {
                Ok(exec) => {
                    log::debug!("JIT执行器创建成功");
                    exec
                }
                Err(err) => {
                    panic!("创建JIT执行器失败: {}", err);
                }
            };

            // 执行程序
            match executor.execute_with_jit(&lir_program) {
                Ok(result) => {
                    assert_eq!(
                        result, expected_result,
                        "测试 '{}' 失败：期望{}，实际得到{}",
                        test_name, expected_result, result
                    );
                    log::debug!("测试 '{}' 通过，结果: {}", test_name, result);
                }
                Err(err) => {
                    panic!("测试 '{}' 执行失败: {}", test_name, err);
                }
            }
        }

        log::debug!("\n=== 所有比较指令测试通过！ ===");
    }
}
