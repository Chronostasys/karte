//! AArch64 JIT编译器
//!
//! 将LIR指令编译为AArch64机器码

use super::code_buffer::{CodeBuilder, JumpType};
use super::compiler_trait::*;
use super::ffi::{RuntimeArg, RuntimeCall};
use karte_common::calling_convention::{REG_RETURN, REG_RETURN_ADDRESS, REG_STACK_POINTER};
use karte_lir::{Instruction, LirFunction, LirProgram, Operand, Register};
use std::collections::HashMap;

/// AArch64编译器
#[derive(Debug)]
pub struct AArch64Compiler {
    /// 寄存器映射 (虚拟寄存器 -> 物理寄存器)
    register_mapping: HashMap<Register, u8>,
    /// 调用约定
    calling_convention: CallingConventionInfo,
    /// 调试模式
    debug_mode: bool,
    /// 唯一label计数器（用于在编译时生成跳转目标）
    unique_label_counter: usize,
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
            calling_convention: Self::create_calling_convention(),
            debug_mode: true, // 强制启用调试模式以便观察编译过程
            unique_label_counter: 0,
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
        // 虚拟寄存器到物理寄存器的映射
        // 简化映射，避免复杂的栈指针管理
        for i in 0..8 {
            let virtual_reg = Register::Virtual(i);
            let physical_reg = match i {
                0 => AArch64Register::X0 as u8, // r0 -> x0 (返回值寄存器)
                1 => AArch64Register::X1 as u8, // r1 -> x1
                2 => AArch64Register::X2 as u8, // r2 -> x2
                3 => AArch64Register::X3 as u8, // r3 -> x3
                4 => AArch64Register::X4 as u8, // r4 -> x4
                5 => AArch64Register::X5 as u8, // r5 -> x5
                6 => AArch64Register::X6 as u8, // r6 -> x6 (简化：不再用作虚拟栈指针)
                7 => AArch64Register::X7 as u8, // r7 -> x7 (简化：不再用作虚拟帧指针)
                _ => AArch64Register::X9 as u8, // 其他使用临时寄存器
            };
            self.register_mapping.insert(virtual_reg, physical_reg);
        }

        // 物理寄存器直接映射
        for i in 0..32 {
            let physical_reg = Register::Physical(i);
            // 简化映射，直接对应AArch64寄存器
            let aarch64_reg = match i {
                31 => AArch64Register::SP as u8, // Physical(31) -> SP
                _ => i,                          // 其他物理寄存器直接映射
            };
            self.register_mapping.insert(physical_reg, aarch64_reg);
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
        // 如果有返回值，先将其移动到返回值寄存器X0
        if let Some(return_reg) = value {
            let src_reg = self.get_physical_register(return_reg)?;
            if src_reg != (AArch64Register::X0 as u8) {
                self.emit_mov_reg_reg(code_builder, AArch64Register::X0 as u8, src_reg);
            }
        } else {
            // 无返回值的函数默认返回0
            self.emit_mov_reg_imm64(code_builder, AArch64Register::X0 as u8, 0);
        }

        // 只有主函数会与宿主环境交换返回槽指针
        if is_main_function {
            let slot_reg = AArch64Register::X16 as u8;
            self.load_and_pop_return_slot_pointer(code_builder, slot_reg);
        }

        // VM调用约定：返回时需要弹出虚拟返回地址并跳转
        let vm_sp_reg = self.get_physical_register(&Register::Physical(REG_STACK_POINTER))?;
        let return_addr_reg =
            self.get_physical_register(&Register::Physical(REG_RETURN_ADDRESS))?;

        // 加载返回地址到专用寄存器
        self.emit_ldr_reg_mem(code_builder, return_addr_reg, vm_sp_reg, 0);
        // 弹出返回地址槽位
        self.emit_add_reg_reg_imm(code_builder, vm_sp_reg, vm_sp_reg, 8);

        if is_main_function {
            // main 函数负责从VM世界回退到宿主环境
            let host_return_label = self.next_label("jit_return_host");
            self.emit_cmp_reg_imm(code_builder, return_addr_reg, 0);
            code_builder.emit_jump(JumpType::ConditionalEqual, &host_return_label);

            // 非零返回地址：继续在JIT世界中执行
            // 🔧 修复：如果是JIT内部调用，不需要写回返回槽（因为调用者没传槽指针）
            // 直接返回X0中的值即可
            let ret_reg = Register::Physical(REG_RETURN_ADDRESS);
            self.compile_jump_register(&ret_reg, code_builder)?;

            // 零返回地址：回到宿主
            code_builder.define_label(&host_return_label)?;

            // 🔧 修复：不需要写回返回槽，直接返回X0中的值
            // if let Some(slot_reg) = return_slot_reg {
            //     self.emit_store_return_value_to_slot(code_builder, slot_reg);
            // }

            self.emit_function_epilogue(code_builder)?;
            self.emit_ret(code_builder);
        } else {
            // 普通函数：直接跳向被调用者设置的继续执行位置
            let ret_reg = Register::Physical(REG_RETURN_ADDRESS);
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
        let return_reg = AArch64Register::X0 as u8;
        let exclude: Vec<u8> = if result.is_some() && call.expects_result() {
            vec![return_reg]
        } else {
            Vec::new()
        };
        let (saved_regs, stack_space) = self.save_call_clobbered_registers(code_builder, &exclude);

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
        self.emit_sub_reg_reg_imm(
            code_builder,
            AArch64Register::SP as u8,
            AArch64Register::SP as u8,
            stack_space as i32,
        );

        for (idx, reg) in regs.iter().enumerate() {
            self.emit_str_reg_mem(
                code_builder,
                *reg,
                AArch64Register::SP as u8,
                (idx * 8) as i32,
            );
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

        for (idx, reg) in regs.iter().enumerate() {
            self.emit_ldr_reg_mem(
                code_builder,
                *reg,
                AArch64Register::SP as u8,
                (idx * 8) as i32,
            );
        }

        self.emit_add_reg_reg_imm(
            code_builder,
            AArch64Register::SP as u8,
            AArch64Register::SP as u8,
            stack_space as i32,
        );
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
        // 🔧 修复：AArch64 LDR指令只支持无符号偏移！范围是0到32760（8字节对齐）
        // 负偏移必须使用间接寻址或pre/post-indexed寻址模式

        if offset >= 0 && offset % 8 == 0 && offset <= 32760 {
            // 使用立即数偏移寻址模式（8字节对齐，仅正偏移）
            // 31|30|29|28 27 26|25 24|23 22|21   12|11 10|9 5|4 0
            // 1 |1 |1 |1  1  0 |0  1 |0  1 |imm12  |0  1 |Rn |Rt

            // 对于8字节对齐的偏移，实际编码的是 offset/8
            let scaled_offset = (offset / 8) as u32;
            // 12位无符号偏移：范围0到4095

            let instruction =
                0xF9400000u32 | (scaled_offset << 10) | ((base as u32) << 5) | (dst as u32);
            code_builder.emit_bytes(&instruction.to_le_bytes());
        } else {
            // 负偏移或超出范围，使用间接寻址
            let temp_reg = AArch64Register::X17 as u8;

            // 先将偏移加载到临时寄存器
            self.emit_mov_reg_imm64(code_builder, temp_reg, offset as i64);

            // 计算有效地址：temp_reg = base + offset
            self.emit_add_reg_reg_reg(code_builder, temp_reg, base, temp_reg);

            // LDR dst, [temp_reg] (基础寻址模式)
            let instruction = 0xF9400000u32 | ((temp_reg as u32) << 5) | (dst as u32);
            code_builder.emit_bytes(&instruction.to_le_bytes());
        }
    }

    /// 生成STR内存存储指令
    fn emit_str_reg_mem(&self, code_builder: &mut CodeBuilder, src: u8, base: u8, offset: i32) {
        // STR <Xt>, [<Xn|SP>, #offset]
        // 🔧 修复：AArch64 STR指令只支持无符号偏移！范围是0到32760（8字节对齐）
        // 负偏移必须使用间接寻址或pre/post-indexed寻址模式

        if offset >= 0 && offset % 8 == 0 && offset <= 32760 {
            // 使用立即数偏移寻址模式（8字节对齐，仅正偏移）
            // 31|30|29|28 27 26|25 24|23 22|21   12|11 10|9 5|4 0
            // 1 |1 |1 |1  1  0 |0  1 |0  0 |imm12  |0  1 |Rn |Rt

            // 对于8字节对齐的偏移，实际编码的是 offset/8
            let scaled_offset = (offset / 8) as u32;
            // 12位无符号偏移：范围0到4095

            let instruction =
                0xF9000000u32 | (scaled_offset << 10) | ((base as u32) << 5) | (src as u32);
            code_builder.emit_bytes(&instruction.to_le_bytes());
        } else {
            // 负偏移或超出范围，使用间接寻址
            let temp_reg = AArch64Register::X17 as u8;

            // 先将偏移加载到临时寄存器
            self.emit_mov_reg_imm64(code_builder, temp_reg, offset as i64);

            // 计算有效地址：temp_reg = base + offset
            self.emit_add_reg_reg_reg(code_builder, temp_reg, base, temp_reg);

            // STR src, [temp_reg] (基础寻址模式)
            let instruction = 0xF9000000u32 | ((temp_reg as u32) << 5) | (src as u32);
            code_builder.emit_bytes(&instruction.to_le_bytes());
        }
    }

    /// 生成函数序言
    fn emit_function_prologue(&self, code_builder: &mut CodeBuilder) -> Result<(), String> {
        // AArch64 AAPCS64调用约定：X0和X1为前两个参数
        // 参考x86实现，将参数移动到虚拟机寄存器
        let x0 = AArch64Register::X0 as u8; // 第一个参数：虚拟栈顶地址
        let x1 = AArch64Register::X1 as u8; // 第二个参数：虚拟栈底地址
        let vm_sp = self
            .get_physical_register(&karte_lir::Register::Physical(6))
            .unwrap_or(6);
        let vm_fp = self
            .get_physical_register(&karte_lir::Register::Physical(7))
            .unwrap_or(7);

        // 🔧 修复：保存 VM_SP (X6) 和 VM_FP (X7) 到栈
        // 因为它们在 AAPCS64 中是 caller-saved，但 VM 期望它们是 callee-saved
        // STP X6, X7, [SP, #-16]!
        // 0xA9BF1FE6
        let stp_x6_x7 = 0xA9BF1FE6u32;
        code_builder.emit_bytes(&stp_x6_x7.to_le_bytes());

        // 标准函数序言：保存帧指针和链接寄存器
        // STP X29, X30, [SP, #-16]!
        let instruction = 0xA9BF7BFDu32;
        code_builder.emit_bytes(&instruction.to_le_bytes());

        // MOV X29, SP (设置帧指针)
        self.emit_mov_reg_reg(
            code_builder,
            AArch64Register::X29 as u8,
            AArch64Register::SP as u8,
        );

        // 🔧 关键修复：将传入的虚拟栈(top/bottom)地址设置到 r6/r7（LIR使用r6/r7作为虚拟SP/FP基准）
        self.emit_mov_reg_reg(code_builder, vm_sp, x0);
        self.emit_mov_reg_reg(code_builder, vm_fp, x1);

        // 保存返回值槽指针，供Return阶段写回
        self.save_return_slot_pointer(code_builder);

        // // 🔧 新增：保存参数寄存器到栈，防止被后续指令覆盖
        // // STP X0, X1, [SP, #-16]! (保存参数寄存器)
        // let stp_x0_x1 = 0xA9BF03E0u32;
        // code_builder.emit_bytes(&stp_x0_x1.to_le_bytes());

        // 此时r6和r7已经包含了虚拟栈的地址，LIR的栈帧管理指令会基于这些值工作

        Ok(())
    }

    /// 生成内部函数序言（简化版本，用于虚拟机内部函数调用）
    fn emit_internal_function_prologue(
        &self,
        _code_builder: &mut CodeBuilder,
    ) -> Result<(), String> {
        // 内部函数完全依赖VM栈和寄存器，不需要与宿主交换返回槽
        Ok(())
    }

    /// 生成函数尾声
    fn emit_function_epilogue(&self, code_builder: &mut CodeBuilder) -> Result<(), String> {
        // // 🔧 新增：恢复参数寄存器
        // // LDP X0, X1, [SP], #16 (恢复参数寄存器)
        // let ldp_x0_x1 = 0xA8C103E0u32;
        // code_builder.emit_bytes(&ldp_x0_x1.to_le_bytes());

        // LDP X29, X30, [SP], #16 (post-index load pair)
        // 恢复帧指针和链接寄存器，并增加栈指针16字节
        let instruction = 0xA8C17BFDu32; // ldp x29, x30, [sp], #16
        code_builder.emit_bytes(&instruction.to_le_bytes());

        // 🔧 修复：恢复 VM_SP (X6) 和 VM_FP (X7)
        // LDP X6, X7, [SP], #16
        // 0xA8C11FE6
        let ldp_x6_x7 = 0xA8C11FE6u32;
        code_builder.emit_bytes(&ldp_x6_x7.to_le_bytes());

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
    fn next_label(&mut self, prefix: &str) -> String {
        let label = format!("{}_{}", prefix, self.unique_label_counter);
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

        // 创建代码构建器
        let mut code_builder = CodeBuilder::new();

        // 生成函数标签
        let function_label = format!("func_{}", function.name);
        code_builder.define_label(&function_label)?;

        // 检查第一个instruction是label，是则编译，不是则返回错误
        if let Some(Instruction::Label { id, .. }) = function.instructions.first() {
            code_builder.define_label(&format!("label_{}", id.0))?;
        } else {
            return Err(format!("函数 '{}' 的第一个指令必须是label", function.name));
        }

        let is_main_function = self.is_entry_function(&function.name, program);
        // 生成函数序言
        if is_main_function {
            self.emit_function_prologue(&mut code_builder)?;
        } else {
            // 简化序言：用于内部函数调用
            self.emit_internal_function_prologue(&mut code_builder)?;
        }
        // 编译所有指令
        for instruction in function.instructions.iter().skip(1) {
            self.compile_instruction(instruction, &mut code_builder, is_main_function)?;
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

    /// 编译单个函数（使用全局标签表）
    fn compile_function_with_global_labels(
        &mut self,
        function: &LirFunction,
        program: &LirProgram,
        global_labels: &std::collections::HashMap<String, *const u8>,
    ) -> Result<CompiledFunction, String> {
        if self.debug_mode {
            log::debug!("AArch64: 开始编译函数 '{}' (使用全局标签表)", function.name);
        }

        let mut code_builder = if self.debug_mode {
            CodeBuilder::with_debug_info()
        } else {
            CodeBuilder::new()
        };

        // 设置全局标签表
        let global_labels_usize: std::collections::HashMap<String, usize> = global_labels
            .iter()
            .map(|(k, v)| (k.clone(), *v as usize))
            .collect();
        code_builder.set_global_labels(global_labels_usize);

        // 生成函数标签
        let function_label = format!("func_{}", function.name);
        code_builder.define_label(&function_label)?;

        // 🔧 修复：只有main函数才需要C FFI序言尾声，其他函数使用简化版本
        let is_main_function = self.is_entry_function(&function.name, program);
        // 检查第一个instruction是label，是则编译，不是则返回错误
        if let Some(Instruction::Label { id, .. }) = function.instructions.first() {
            code_builder.define_label(&format!("label_{}", id.0))?;
        } else {
            return Err(format!("函数 '{}' 的第一个指令必须是label", function.name));
        }

        if is_main_function {
            // C FFI序言：用于main函数的外部调用
            self.emit_function_prologue(&mut code_builder)?;
        } else {
            // 简化序言：用于内部函数调用
            self.emit_internal_function_prologue(&mut code_builder)?;
        }

        // 编译函数体
        for (index, instruction) in function.instructions.iter().skip(1).enumerate() {
            if self.debug_mode {
                code_builder.add_source_line(index);
            }

            self.compile_instruction(instruction, &mut code_builder, is_main_function)?;
        }

        // 获取label信息（在finalize之前）
        let labels = code_builder.exported_labels().clone();

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

        // 获取编译后的机器码
        // 从全局标签表中获取当前函数的可执行内存基址
        let exec_base =
            if let Some(&func_addr) = global_labels.get(&format!("func_{}", function.name)) {
                // 函数地址就是基址
                if self.debug_mode {
                    log::debug!("🔧 找到函数的地址: 0x{:016X}", func_addr as usize);
                }
                func_addr as usize
            } else {
                // 如果没有找到函数地址，使用0作为默认值
                if self.debug_mode {
                    log::debug!(
                        "🔧 警告：未找到函数 '{}' 的地址，使用0作为exec_base",
                        function.name
                    );
                }
                0
            };

        let machine_code = code_builder
            .finalize_with_global_addresses_and_exec_base(Some(global_labels), exec_base)?;

        // 创建编译后的函数
        let mut compiled_function = CompiledFunction::new(
            function.name.clone(),
            machine_code,
            0, // 入口点就是函数开始
        );

        // 保存label信息
        compiled_function.labels = labels;

        println!(
            "AArch64: 函数 '{}' 编译完成，机器码大小: {} 字节\n{}",
            function.name,
            compiled_function.code_size(),
            compiled_function
        );

        Ok(compiled_function)
    }

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
        self.calling_convention.clone()
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

        // 指令降级
        log::debug!("\n6. 指令降级");
        if let Err(lowering_error) = karte_lir::lower_program_instructions(&mut lir_program) {
            log::debug!("指令降级失败: {}", lowering_error);
            return Err(format!("指令降级失败: {}", lowering_error).into());
        }
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

        // 强制启用JIT
        env::set_var("KARTE_JIT", "1");

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

        // 强制启用JIT
        env::set_var("KARTE_JIT", "1");

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

        // 强制启用JIT
        env::set_var("KARTE_JIT", "1");

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
