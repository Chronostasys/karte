//! AArch64 JIT编译器
//!
//! 将LIR指令编译为AArch64机器码

use super::code_buffer::{CodeBuilder, JumpType};
use super::compiler_trait::*;
use super::ffi::{RuntimeArg, RuntimeCall, RuntimeIntrinsic};
use super::jit_utils;
include!("dispatch_macro.rs");
use karte_common::calling_convention::{CallingConvention, PhysicalRegister, CC};
use karte_lir::{ComparisonCondition, Instruction, LirFunction, LirProgram, Operand, Register};
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
    /// 当前函数的栈帧大小（由 StackFrameLayoutPass 计算）
    current_stack_frame_size: usize,
    /// epilogue 需要跳过的帧大小
    stack_frame_size_for_epilogue: usize,
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
    pub fn new(debug_mode: bool) -> crate::Result<Self> {
        let mut compiler = Self {
            register_mapping: HashMap::new(),
            ffi_calling_convention: Self::create_calling_convention(),
            vm_calling_convention: CallingConvention::standard(),
            debug_mode: false, // 默认关闭调试模式，避免 I/O 瓶颈
            unique_label_counter: 0,
            current_function_use_regs: Vec::new(),
            current_function_name: String::new(),
            current_stack_frame_size: 0,
            stack_frame_size_for_epilogue: 0,
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
    fn get_physical_register(&self, reg: &Register) -> crate::Result<u8> {
        match reg {
            Register::Virtual(id) => {
                // 虚拟寄存器不应出现在 JIT 阶段，寄存器分配必须在 JIT 之前完成
                panic!("JIT 编译器遇到虚拟寄存器 Virtual({})，寄存器分配应在 JIT 之前完成", id);
            }
            _ => {
                self.register_mapping
                    .get(reg)
                    .copied()
                    .ok_or_else(|| format!("未映射的寄存器: {:?}", reg).into())
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
    ) -> crate::Result<()> {
        if self.debug_mode {
            log::debug!("编译AArch64指令: {}", instruction);
        }

        // AArch64 特有的 Nop、CallIndirect、IntCast、StorePair、LoadPair 处理
        match instruction {
            Instruction::Nop { .. } => {
                self.emit_nop(code_builder);
                return Ok(());
            }
            Instruction::CallIndirect { function_register, .. } => {
                return self.compile_call_indirect(function_register, code_builder);
            }
            Instruction::IntCast { dst, src, src_bits, dst_bits, signed, .. } => {
                return self.compile_intcast(dst, src, *src_bits, *dst_bits, *signed, code_builder);
            }
            // AArch64 原生 STP/LDP 指令（比宏中的 store64+store64 fallback 更高效）
            Instruction::StorePair { addr, offset, src1, src2, .. } => {
                return self.compile_store_pair(addr, *offset, src1, src2, code_builder);
            }
            Instruction::LoadPair { dst1, dst2, addr, offset, .. } => {
                return self.compile_load_pair(dst1, dst2, addr, *offset, code_builder);
            }
            _ => {}
        }

        // 构造 AArch64 的 runtime context（包含 instruction_metadata）
        let ctx = RuntimeCallContext {
            instruction_index,
            function,
        };

        // 共享的指令 dispatch（通过宏生成，避免跨平台重复）
        dispatch_compile_instruction!(self, instruction, code_builder, is_main_function, Some(ctx), "AArch64")
    }

    /// 编译移动指令
    fn compile_move(
        &mut self,
        dst: &Register,
        src: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
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
                return Err(format!("不支持的移动操作数类型: {:?}", src).into());
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
    ) -> crate::Result<()> {
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
            // 加法可交换：Immediate + Register = Register + Immediate
            (Operand::Immediate { value }, Operand::Register { id: src2_id }) => {
                let src2_reg = self.get_physical_register(src2_id)?;
                // ADD dst, src2, #imm
                self.emit_add_reg_reg_imm(code_builder, dst_reg, src2_reg, *value as i32);
            }
            // 两个立即数：先加载到临时寄存器
            (Operand::Immediate { value: v1 }, Operand::Immediate { value: v2 }) => {
                let result = v1 + v2;
                self.emit_mov_reg_imm64(code_builder, dst_reg, result);
            }
            _ => {
                return Err(format!("不支持的加法操作数组合: {:?}, {:?}", src1, src2).into());
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
    ) -> crate::Result<()> {
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
                return Err(format!("不支持的减法操作数组合: {:?}, {:?}", src1, src2).into());
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
    ) -> crate::Result<()> {
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
            // 乘法可交换：Immediate * Register = Register * Immediate
            (Operand::Immediate { value }, Operand::Register { id: src2_id }) => {
                let src2_reg = self.get_physical_register(src2_id)?;
                let temp_reg = AArch64Register::X16 as u8;
                self.emit_mov_reg_imm64(code_builder, temp_reg, *value);
                self.emit_mul_reg_reg_reg(code_builder, dst_reg, temp_reg, src2_reg);
            }
            _ => {
                return Err(format!("不支持的乘法操作数组合: {:?}, {:?}", src1, src2).into());
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
    ) -> crate::Result<()> {
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
            // 除法不可交换：dst = imm / reg，需要先加载 imm 到临时寄存器
            (Operand::Immediate { value }, Operand::Register { id: src2_id }) => {
                let temp_reg = AArch64Register::X16 as u8;
                self.emit_mov_reg_imm64(code_builder, temp_reg, *value);
                let src2_reg = self.get_physical_register(src2_id)?;
                self.emit_div_reg_reg_reg(code_builder, dst_reg, temp_reg, src2_reg);
            }
            _ => {
                return Err(format!("不支持的除法操作数组合: {:?}, {:?}", src1, src2).into());
            }
        }
        Ok(())
    }

    /// 编译取余指令
    /// AArch64 没有直接的取余指令，使用 remainder = dividend - (dividend / divisor) * divisor
    fn compile_mod(
        &mut self,
        dst: &Register,
        src1: &Operand,
        src2: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let dst_reg = self.get_physical_register(dst)?;
        // 使用 X16, X17 作为临时寄存器
        let temp_quot = AArch64Register::X16 as u8; // 存商
        let temp_divisor = AArch64Register::X17 as u8; // 存除数

        match (src1, src2) {
            (Operand::Register { id: src1_id }, Operand::Register { id: src2_id }) => {
                let src1_reg = self.get_physical_register(src1_id)?;
                let src2_reg = self.get_physical_register(src2_id)?;
                // 保存除数到临时寄存器（因为 src2 可能在后续操作中被覆盖）
                self.emit_mov_reg_reg(code_builder, temp_divisor, src2_reg);
                // SDIV temp_quot, src1, src2 (计算商)
                self.emit_div_reg_reg_reg(code_builder, temp_quot, src1_reg, src2_reg);
                // MSUB dst, temp_quot, temp_divisor, src1 (result = src1 - temp_quot * temp_divisor)
                self.emit_msub(code_builder, dst_reg, temp_quot, temp_divisor, src1_reg);
            }
            (Operand::Register { id: src1_id }, Operand::Immediate { value }) => {
                let src1_reg = self.get_physical_register(src1_id)?;
                self.emit_mov_reg_imm64(code_builder, temp_divisor, *value);
                // SDIV temp_quot, src1, temp_divisor
                self.emit_div_reg_reg_reg(code_builder, temp_quot, src1_reg, temp_divisor);
                // MSUB dst, temp_quot, temp_divisor, src1
                self.emit_msub(code_builder, dst_reg, temp_quot, temp_divisor, src1_reg);
            }
            // 取余不可交换：dst = imm % reg = imm - (imm/reg) * reg
            // MSUB Xd, Xn, Xm, Xa = Xa - Xn * Xm
            // 需要同时保留 quotient 和 imm，利用 dst_reg 保存 imm
            (Operand::Immediate { value }, Operand::Register { id: src2_id }) => {
                let src2_reg = self.get_physical_register(src2_id)?;
                // 先把 imm 加载到 dst_reg（作为 MSUB 的 Xa 参数）
                self.emit_mov_reg_imm64(code_builder, dst_reg, *value);
                // 保存除数到 temp_divisor (X17)
                self.emit_mov_reg_reg(code_builder, temp_divisor, src2_reg);
                // SDIV temp_quot, dst_reg, temp_divisor → temp_quot = imm / reg
                self.emit_div_reg_reg_reg(code_builder, temp_quot, dst_reg, temp_divisor);
                // MSUB dst, temp_quot, temp_divisor, dst_reg
                // = dst_reg - temp_quot * temp_divisor = imm - quotient * divisor ✓
                self.emit_msub(code_builder, dst_reg, temp_quot, temp_divisor, dst_reg);
            }
            _ => {
                return Err(format!("不支持的取余操作数组合: {:?}, {:?}", src1, src2).into());
            }
        }
        Ok(())
    }

    /// 编译按位与指令
    fn compile_bitand(
        &mut self,
        dst: &Register,
        src1: &Operand,
        src2: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let dst_reg = self.get_physical_register(dst)?;

        // 编译期常量折叠
        if let (Operand::Immediate { value: v1 }, Operand::Immediate { value: v2 }) = (src1, src2) {
            self.emit_mov_reg_imm64(code_builder, dst_reg, v1 & v2);
            return Ok(());
        }

        match (src1, src2) {
            (Operand::Register { id: src1_id }, Operand::Register { id: src2_id }) => {
                let src1_reg = self.get_physical_register(src1_id)?;
                let src2_reg = self.get_physical_register(src2_id)?;
                // AND Xd, Xn, Xm
                self.emit_and_reg_reg(code_builder, dst_reg, src1_reg, src2_reg);
            }
            (Operand::Register { id: src1_id }, Operand::Immediate { value }) => {
                let src1_reg = self.get_physical_register(src1_id)?;
                // AArch64 AND 没有直接支持任意立即数，先加载到临时寄存器
                let temp_reg = AArch64Register::X16 as u8;
                self.emit_mov_reg_imm64(code_builder, temp_reg, *value);
                self.emit_and_reg_reg(code_builder, dst_reg, src1_reg, temp_reg);
            }
            (Operand::Immediate { value }, Operand::Register { id: src2_id }) => {
                let src2_reg = self.get_physical_register(src2_id)?;
                let temp_reg = AArch64Register::X16 as u8;
                self.emit_mov_reg_imm64(code_builder, temp_reg, *value);
                self.emit_and_reg_reg(code_builder, dst_reg, temp_reg, src2_reg);
            }
            _ => {
                return Err(format!("不支持的按位与操作数组合: {:?}, {:?}", src1, src2).into());
            }
        }
        Ok(())
    }

    /// 编译按位或指令
    fn compile_bitor(
        &mut self,
        dst: &Register,
        src1: &Operand,
        src2: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let dst_reg = self.get_physical_register(dst)?;

        // 编译期常量折叠
        if let (Operand::Immediate { value: v1 }, Operand::Immediate { value: v2 }) = (src1, src2) {
            self.emit_mov_reg_imm64(code_builder, dst_reg, v1 | v2);
            return Ok(());
        }

        match (src1, src2) {
            (Operand::Register { id: src1_id }, Operand::Register { id: src2_id }) => {
                let src1_reg = self.get_physical_register(src1_id)?;
                let src2_reg = self.get_physical_register(src2_id)?;
                // ORR Xd, Xn, Xm
                self.emit_orr_reg_reg(code_builder, dst_reg, src1_reg, src2_reg);
            }
            (Operand::Register { id: src1_id }, Operand::Immediate { value }) => {
                let src1_reg = self.get_physical_register(src1_id)?;
                let temp_reg = AArch64Register::X16 as u8;
                self.emit_mov_reg_imm64(code_builder, temp_reg, *value);
                self.emit_orr_reg_reg(code_builder, dst_reg, src1_reg, temp_reg);
            }
            (Operand::Immediate { value }, Operand::Register { id: src2_id }) => {
                let src2_reg = self.get_physical_register(src2_id)?;
                let temp_reg = AArch64Register::X16 as u8;
                self.emit_mov_reg_imm64(code_builder, temp_reg, *value);
                self.emit_orr_reg_reg(code_builder, dst_reg, temp_reg, src2_reg);
            }
            _ => {
                return Err(format!("不支持的按位或操作数组合: {:?}, {:?}", src1, src2).into());
            }
        }
        Ok(())
    }

    /// 编译按位异或指令
    fn compile_bitxor(
        &mut self,
        dst: &Register,
        src1: &Operand,
        src2: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let dst_reg = self.get_physical_register(dst)?;

        // 编译期常量折叠
        if let (Operand::Immediate { value: v1 }, Operand::Immediate { value: v2 }) = (src1, src2) {
            self.emit_mov_reg_imm64(code_builder, dst_reg, v1 ^ v2);
            return Ok(());
        }

        match (src1, src2) {
            (Operand::Register { id: src1_id }, Operand::Register { id: src2_id }) => {
                let src1_reg = self.get_physical_register(src1_id)?;
                let src2_reg = self.get_physical_register(src2_id)?;
                // EOR Xd, Xn, Xm
                self.emit_eor_reg_reg(code_builder, dst_reg, src1_reg, src2_reg);
            }
            (Operand::Register { id: src1_id }, Operand::Immediate { value }) => {
                let src1_reg = self.get_physical_register(src1_id)?;
                let temp_reg = AArch64Register::X16 as u8;
                self.emit_mov_reg_imm64(code_builder, temp_reg, *value);
                self.emit_eor_reg_reg(code_builder, dst_reg, src1_reg, temp_reg);
            }
            (Operand::Immediate { value }, Operand::Register { id: src2_id }) => {
                let src2_reg = self.get_physical_register(src2_id)?;
                let temp_reg = AArch64Register::X16 as u8;
                self.emit_mov_reg_imm64(code_builder, temp_reg, *value);
                self.emit_eor_reg_reg(code_builder, dst_reg, temp_reg, src2_reg);
            }
            _ => {
                return Err(format!("不支持的按位异或操作数组合: {:?}, {:?}", src1, src2).into());
            }
        }
        Ok(())
    }

    /// 编译按位取反指令
    fn compile_bitnot(
        &mut self,
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
                // MVN Xd, Xm (等价于 ORR Xd, XZR, Xm)
                self.emit_mvn_reg_reg(code_builder, dst_reg, src_reg);
            }
            _ => {
                return Err(format!("不支持的按位取反操作数: {:?}", src).into());
            }
        }
        Ok(())
    }

    /// 编译左移指令
    fn compile_shift_left(
        &mut self,
        dst: &Register,
        src1: &Operand,
        src2: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let dst_reg = self.get_physical_register(dst)?;

        // 编译期常量折叠
        if let (Operand::Immediate { value: v1 }, Operand::Immediate { value: v2 }) = (src1, src2) {
            self.emit_mov_reg_imm64(code_builder, dst_reg, v1 << (v2 & 63));
            return Ok(());
        }

        match (src1, src2) {
            (Operand::Register { id: src1_id }, Operand::Register { id: src2_id }) => {
                let src1_reg = self.get_physical_register(src1_id)?;
                let src2_reg = self.get_physical_register(src2_id)?;
                // LSLV Xd, Xn, Xm
                self.emit_lslv_reg_reg(code_builder, dst_reg, src1_reg, src2_reg);
            }
            (Operand::Register { id: src1_id }, Operand::Immediate { value }) => {
                let src1_reg = self.get_physical_register(src1_id)?;
                let temp_reg = AArch64Register::X16 as u8;
                self.emit_mov_reg_imm64(code_builder, temp_reg, *value);
                self.emit_lslv_reg_reg(code_builder, dst_reg, src1_reg, temp_reg);
            }
            (Operand::Immediate { value }, Operand::Register { id: src2_id }) => {
                let src2_reg = self.get_physical_register(src2_id)?;
                let temp_reg = AArch64Register::X16 as u8;
                self.emit_mov_reg_imm64(code_builder, temp_reg, *value);
                self.emit_lslv_reg_reg(code_builder, dst_reg, temp_reg, src2_reg);
            }
            _ => {
                return Err(format!("不支持的左移操作数组合: {:?}, {:?}", src1, src2).into());
            }
        }
        Ok(())
    }

    /// 编译右移指令（算术右移）
    fn compile_shift_right(
        &mut self,
        dst: &Register,
        src1: &Operand,
        src2: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let dst_reg = self.get_physical_register(dst)?;

        // 编译期常量折叠
        if let (Operand::Immediate { value: v1 }, Operand::Immediate { value: v2 }) = (src1, src2) {
            // 算术右移
            self.emit_mov_reg_imm64(code_builder, dst_reg, v1 >> (v2 & 63));
            return Ok(());
        }

        match (src1, src2) {
            (Operand::Register { id: src1_id }, Operand::Register { id: src2_id }) => {
                let src1_reg = self.get_physical_register(src1_id)?;
                let src2_reg = self.get_physical_register(src2_id)?;
                // ASRV Xd, Xn, Xm
                self.emit_asrv_reg_reg(code_builder, dst_reg, src1_reg, src2_reg);
            }
            (Operand::Register { id: src1_id }, Operand::Immediate { value }) => {
                let src1_reg = self.get_physical_register(src1_id)?;
                let temp_reg = AArch64Register::X16 as u8;
                self.emit_mov_reg_imm64(code_builder, temp_reg, *value);
                self.emit_asrv_reg_reg(code_builder, dst_reg, src1_reg, temp_reg);
            }
            (Operand::Immediate { value }, Operand::Register { id: src2_id }) => {
                let src2_reg = self.get_physical_register(src2_id)?;
                let temp_reg = AArch64Register::X16 as u8;
                self.emit_mov_reg_imm64(code_builder, temp_reg, *value);
                self.emit_asrv_reg_reg(code_builder, dst_reg, temp_reg, src2_reg);
            }
            _ => {
                return Err(format!("不支持的右移操作数组合: {:?}, {:?}", src1, src2).into());
            }
        }
        Ok(())
    }

    /// 编译整数类型转换指令（截断/零扩展/符号扩展）
    fn compile_intcast(
        &mut self,
        dst: &Register,
        src: &Operand,
        src_bits: u8,
        dst_bits: u8,
        signed: bool,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let dst_reg = self.get_physical_register(dst)?;

        // 将源操作数加载到目标寄存器
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
            _ => return Err(format!("intcast不支持的src: {:?}", src).into()),
        }

        // 使用临时寄存器存放掩码
        let temp_reg = AArch64Register::X16 as u8;

        match (src_bits, dst_bits) {
            // 64 → 32：用 AND 掩码截断
            (64, 32) => {
                self.emit_mov_reg_imm64(code_builder, temp_reg, 0xFFFFFFFF);
                self.emit_and_reg_reg(code_builder, dst_reg, dst_reg, temp_reg);
            }
            // 64 → 16：用 AND 掩码截断
            (64, 16) => {
                self.emit_mov_reg_imm64(code_builder, temp_reg, 0xFFFF);
                self.emit_and_reg_reg(code_builder, dst_reg, dst_reg, temp_reg);
            }
            // 64 → 8：用 AND 掩码截断
            (64, 8) => {
                self.emit_mov_reg_imm64(code_builder, temp_reg, 0xFF);
                self.emit_and_reg_reg(code_builder, dst_reg, dst_reg, temp_reg);
            }
            // 32 → 64：零扩展或符号扩展
            (32, 64) => {
                if signed {
                    // 符号扩展：先将值截断到32位，再算术右移32位再左移32位
                    // SBFM Xd, Xn, #0, #31 — 等价于 SXTW
                    // SBFM encoding: 0x93000000 | (immr << 16) | (imms << 10) | (Rn << 5) | Rd
                    // SXTW: SBFM Xd, Xn, #0, #31 → immr=0, imms=31
                    let instruction =
                        0x93000000u32 | (0u32 << 16) | (31u32 << 10) | ((dst_reg as u32) << 5) | (dst_reg as u32);
                    code_builder.emit_bytes(&instruction.to_le_bytes());
                }
                // 无符号：32位值存储在64位寄存器中，需要先 AND 0xFFFFFFFF 清除高位
                // （AArch64 不会自动零扩展）
                self.emit_mov_reg_imm64(code_builder, temp_reg, 0xFFFFFFFF);
                self.emit_and_reg_reg(code_builder, dst_reg, dst_reg, temp_reg);
            }
            // 16 → 64：零扩展或符号扩展
            (16, 64) => {
                if signed {
                    // SBFM Xd, Xn, #0, #15 — 等价于 SXTH
                    let instruction =
                        0x93000000u32 | (0u32 << 16) | (15u32 << 10) | ((dst_reg as u32) << 5) | (dst_reg as u32);
                    code_builder.emit_bytes(&instruction.to_le_bytes());
                } else {
                    self.emit_mov_reg_imm64(code_builder, temp_reg, 0xFFFF);
                    self.emit_and_reg_reg(code_builder, dst_reg, dst_reg, temp_reg);
                }
            }
            // 8 → 64：零扩展或符号扩展
            (8, 64) => {
                if signed {
                    // SBFM Xd, Xn, #0, #7 — 等价于 SXTB
                    let instruction =
                        0x93000000u32 | (0u32 << 16) | (7u32 << 10) | ((dst_reg as u32) << 5) | (dst_reg as u32);
                    code_builder.emit_bytes(&instruction.to_le_bytes());
                } else {
                    self.emit_mov_reg_imm64(code_builder, temp_reg, 0xFF);
                    self.emit_and_reg_reg(code_builder, dst_reg, dst_reg, temp_reg);
                }
            }
            // 同位宽或不需要转换
            _ => {}
        }

        let _ = signed;
        Ok(())
    }

    /// 编译比较指令
    fn compile_compare(
        &mut self,
        src1: &Operand,
        src2: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
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
            // 比较不可交换：cmp imm, reg 需要先加载 imm 到临时寄存器
            (Operand::Immediate { value }, Operand::Register { id: src2_id }) => {
                let temp_reg = AArch64Register::X16 as u8;
                self.emit_mov_reg_imm64(code_builder, temp_reg, *value);
                let src2_reg = self.get_physical_register(src2_id)?;
                self.emit_cmp_reg_reg(code_builder, temp_reg, src2_reg);
            }
            _ => {
                return Err(format!("不支持的比较操作数组合: {:?}, {:?}", src1, src2).into());
            }
        }
        Ok(())
    }

    /// 编译 CompareSet 指令：cmp src1, src2; cset dst, condition
    /// 直接从比较条件产生 0/1 值到 dst 寄存器，不产生分支。
    fn compile_compare_set(
        &mut self,
        dst: &Register,
        condition: &ComparisonCondition,
        src1: &Operand,
        src2: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        // 先执行 CMP 设置 flags
        match (src1, src2) {
            (Operand::Register { id: src1_id }, Operand::Register { id: src2_id }) => {
                let src1_reg = self.get_physical_register(src1_id)?;
                let src2_reg = self.get_physical_register(src2_id)?;
                self.emit_cmp_reg_reg(code_builder, src1_reg, src2_reg);
            }
            (Operand::Register { id: src1_id }, Operand::Immediate { value }) => {
                let src1_reg = self.get_physical_register(src1_id)?;
                self.emit_cmp_reg_imm(code_builder, src1_reg, *value as i32);
            }
            // 比较不可交换：cmp imm, reg 需要先加载 imm 到临时寄存器
            (Operand::Immediate { value }, Operand::Register { id: src2_id }) => {
                let temp_reg = AArch64Register::X16 as u8;
                self.emit_mov_reg_imm64(code_builder, temp_reg, *value);
                let src2_reg = self.get_physical_register(src2_id)?;
                self.emit_cmp_reg_reg(code_builder, temp_reg, src2_reg);
            }
            _ => {
                return Err(format!("不支持的setcc操作数组合: {:?}, {:?}", src1, src2).into());
            }
        }

        let dst_reg = self.get_physical_register(dst)?;

        // CSET dst, condition
        // CSET 是 CSINC 的别名：CSET Xd, cond = CSINC Xd, XZR, XZR, invert(cond)
        // CSINC 编码: sf=1, 1101 0110, Rm, cond, Rn, Rd
        // base = 0x9A800000
        let aarch64_cond = match condition {
            ComparisonCondition::Equal => 0x0,        // EQ
            ComparisonCondition::NotEqual => 0x1,     // NE
            ComparisonCondition::LessThan => 0xB,     // LT
            ComparisonCondition::LessEqual => 0xD,    // LE
            ComparisonCondition::GreaterThan => 0xC,  // GT
            ComparisonCondition::GreaterEqual => 0xA, // GE
        };
        let inverted_cond = aarch64_cond ^ 1; // 反转条件

        let dst_enc = dst_reg as u32;
        let xzr = 31u32; // XZR 在 AArch64 中编码为 31

        // CSINC Xd, XZR, XZR, invert(cond)
        // CSINC 编码: sf=1, 00, 11010_100, Rm, cond, 01, Rn, Rd
        // 注意: bits[11:10] = 01 区分 CSINC 和 CSEL (bits[11:10] = 00)
        let instr: u32 = 0x9A800400u32      // CSINC base (sf=1, bits11-10=01)
            | (xzr << 16)                     // Rm = XZR (31)
            | (inverted_cond << 12)           // condition (inverted)
            | (xzr << 5)                      // Rn = XZR (31)
            | (dst_enc & 0x1F);              // Rd

        code_builder.emit_u32(instr);
        Ok(())
    }

    /// 编译无条件跳转指令
    fn compile_jump(
        &mut self,
        target: &karte_lir::LabelId,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
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
    ) -> crate::Result<()> {
        let label_name = format!("label_{}", target.0);
        code_builder.emit_jump(jump_type, &label_name);
        Ok(())
    }

    /// 编译函数调用指令
    fn compile_call(
        &mut self,
        target: &karte_lir::LabelId,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let label_name = format!("label_{}", target.0);

        // 🔧 优化：在连续内存架构中，优先使用相对跳转（BL指令）
        // BL指令支持±128MB的相对跳转范围，足够覆盖我们的代码段
        code_builder.emit_jump(JumpType::Call, &label_name);

        Ok(())
    }

    /// 编译间接函数调用指令
    ///
    /// 通过寄存器中的函数指针进行间接调用。
    /// 在 Karte 闭包调用中，function_register 指向闭包结构体地址，
    /// 需要先读取 function_ptr 字段（offset 0），然后通过 BLR 间接调用。
    fn compile_call_indirect(
        &mut self,
        function_register: &Register,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let func_reg = self.get_physical_register(function_register)?;

        // 闭包结构体 { function_ptr: i64, env_ptr: i64 }
        // function_ptr 在 offset 0
        // 从闭包结构体地址加载函数指针到 X16（间接跳转专用寄存器）
        let target_reg = AArch64Register::X16 as u8;

        // LDR X16, [func_reg, #0] — 读取 function_ptr
        self.emit_ldr_reg_mem(code_builder, target_reg, func_reg, 0);

        // BLR X16 — 间接调用（保存返回地址到 LR）
        // 编码: 0xD63F0000 | (Rn << 5)
        let instruction = 0xD63F0000u32 | ((target_reg as u32) << 5);
        code_builder.emit_u32(instruction);

        Ok(())
    }

    /// 编译 LoadGlobal 指令 - 从 runtime 全局数据区加载值
    fn compile_load_global(
        &mut self,
        dst: &Register,
        name: &str,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let dst_reg = self.get_physical_register(dst)?;
        let vm_sp_reg = self.vm_calling_convention.stack_pointer;

        // vm_sp 是动态值（SP 寄存器），直接读取
        if name == "vm_sp" {
            if dst_reg != vm_sp_reg {
                self.emit_mov_reg_reg(code_builder, dst_reg, vm_sp_reg);
            }
            return Ok(());
        }

        // stack_top = vstack_bottom + 65520
        if name == "stack_top" {
            let global_label = "__global_stack_bottom".to_string();
            let target_reg = AArch64Register::X16 as u8;
            // ADRP X16, global_label; LDR X16, [X16, :lo12:global_label]
            code_builder.emit_adrp(target_reg, &global_label);
            code_builder.emit_add_reg_label(target_reg, &global_label);
            // dst = vstack_bottom 地址，加载值
            self.emit_ldr_reg_mem(code_builder, dst_reg, target_reg, 0);
            // dst += 65520
            self.emit_add_reg_reg_imm(code_builder, dst_reg, dst_reg, 65520);
            return Ok(());
        }

        // 生成: ADRP + ADD 加载全局变量地址，然后 LDR 读取值
        let global_label = format!("__global_{}", name);
        let target_reg = AArch64Register::X16 as u8;
        code_builder.emit_adrp(target_reg, &global_label);
        code_builder.emit_add_reg_label(target_reg, &global_label);
        // dst = 全局变量的地址
        if dst_reg != target_reg {
            self.emit_mov_reg_reg(code_builder, dst_reg, target_reg);
        }
        // 从地址加载值
        self.emit_ldr_reg_mem(code_builder, dst_reg, dst_reg, 0);

        Ok(())
    }

    /// 编译 GC 寄存器保存/恢复指令
    /// gc_push_regs: 把所有 callee-saved 寄存器 dump 到虚拟栈
    /// gc_pop_regs: 从虚拟栈恢复所有 callee-saved 寄存器
    ///
    /// 保存的寄存器: X19-X28 (callee-saved, 10 个)
    /// 不保存: X0-X18 (caller-saved), X29 (FP), X30 (LR), SP
    fn compile_gc_reg_op(
        &mut self,
        is_push: bool,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        const REGS: [u8; 10] = [19, 20, 21, 22, 23, 24, 25, 26, 27, 28];
        const NUM_REGS: i64 = 10;
        const FRAME_SIZE: i64 = NUM_REGS * 8; // 80

        let vm_sp_reg = self.vm_calling_convention.stack_pointer;

        if is_push {
            // sub sp, sp, #80
            self.emit_sub_reg_reg_imm(code_builder, vm_sp_reg, vm_sp_reg, FRAME_SIZE as i32);
            // STR Xn, [sp, #offset] 逐个保存
            for (i, &reg) in REGS.iter().enumerate() {
                let offset = (i as i32) * 8;
                self.emit_str_reg_mem(code_builder, reg, vm_sp_reg, offset);
            }
        } else {
            // LDR Xn, [sp, #offset] 逐个恢复
            for (i, &reg) in REGS.iter().enumerate() {
                let offset = (i as i32) * 8;
                self.emit_ldr_reg_mem(code_builder, reg, vm_sp_reg, offset);
            }
            // add sp, sp, #80
            self.emit_add_reg_reg_imm(code_builder, vm_sp_reg, vm_sp_reg, FRAME_SIZE as i32);
        }

        Ok(())
    }

    /// 编译间接函数调用指令（带链接）：BLR Xn
    fn compile_jump_indirect(
        &mut self,
        function_register: &Register,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
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
    ) -> crate::Result<()> {
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
    ) -> crate::Result<()> {
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
            // Main函数返回：
            // LIR 已经在 Return 之前生成了 Add vm_sp, frame_size 恢复帧空间
            // 只需要弹出返回值槽，然后进入 epilogue

            // 1. 弹出返回值槽（16字节）
            self.emit_add_reg_reg_imm(code_builder, vm_sp_reg, vm_sp_reg, 16);

            // 2. epilogue 从虚拟栈读取系统SP/X30，恢复系统栈，恢复 callee-saved
            self.emit_function_epilogue(code_builder)?;
            self.emit_ret(code_builder);
        } else {
            // 内部函数逻辑：
            // 1. 恢复 callee-saved 寄存器和 vm_sp/vm_fp（epilogue 不弹出 old_sp/old_fp 空间）
            self.emit_internal_function_epilogue(code_builder)?;

            // 2. 读取返回地址（vm_sp 现在指向调用者保存返回地址的位置）
            self.emit_ldr_reg_mem(code_builder, return_addr_reg, vm_sp_reg, 0);

            // 3. 跳转到返回地址
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
    ) -> crate::Result<()> {
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
    ) -> crate::Result<()> {
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
                // store64 [addr + offset], label - 存储标签绝对地址
                // 方式：用 ADRP+ADD 计算 label 地址（已在 patch_inplace 中修补）
                let label_name = format!("label_{}", id.0);
                code_builder.emit_adrp(16, &label_name);
                code_builder.emit_add_reg_label(16, &label_name);
                self.emit_str_reg_mem(code_builder, 16, addr_reg, offset as i32);
            }
            _ => {
                return Err(format!("不支持的存储操作数类型: {:?}", src).into());
            }
        }
        Ok(())
    }

    /// 编译32位加载指令: LDR Wd, [Xn, #offset]
    fn compile_load32(
        &mut self,
        dst: &Register,
        addr: &Register,
        offset: i64,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let dst_reg = self.get_physical_register(dst)?;
        let addr_reg = self.get_physical_register(addr)?;
        // LDR Wd, [Xn, #offset]: 0xB9400000 | ((offset/4 & 0xFFF) << 10) | (Rn << 5) | Rt
        let scaled_offset = (offset / 4) as u32;
        let instruction = 0xB9400000u32 | (scaled_offset << 10) | ((addr_reg as u32) << 5) | (dst_reg as u32);
        code_builder.emit_u32(instruction);
        Ok(())
    }

    /// 编译32位存储指令: STR Wn, [Xn, #offset]
    fn compile_store32(
        &mut self,
        addr: &Register,
        offset: i64,
        src: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let addr_reg = self.get_physical_register(addr)?;
        match src {
            Operand::Register { id } => {
                let src_reg = self.get_physical_register(id)?;
                let scaled_offset = (offset / 4) as u32;
                let instruction = 0xB9000000u32 | (scaled_offset << 10) | ((addr_reg as u32) << 5) | (src_reg as u32);
                code_builder.emit_u32(instruction);
            }
            Operand::Immediate { value } => {
                let temp_reg = AArch64Register::X16 as u8;
                self.emit_mov_reg_imm64(code_builder, temp_reg, *value);
                let scaled_offset = (offset / 4) as u32;
                let instruction = 0xB9000000u32 | (scaled_offset << 10) | ((addr_reg as u32) << 5) | (temp_reg as u32);
                code_builder.emit_u32(instruction);
            }
            _ => return Err(format!("不支持的 Store32 操作数类型: {:?}", src).into()),
        }
        Ok(())
    }

    /// 编译8位加载指令: LDRB Wd, [Xn, #offset]
    fn compile_load8(
        &mut self,
        dst: &Register,
        addr: &Register,
        offset: i64,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let dst_reg = self.get_physical_register(dst)?;
        let addr_reg = self.get_physical_register(addr)?;
        // LDRB Wd, [Xn, #offset]: 0x39400000 | ((offset & 0xFFF) << 10) | (Rn << 5) | Rt
        let instruction = 0x39400000u32 | (((offset as u32) & 0xFFF) << 10) | ((addr_reg as u32) << 5) | (dst_reg as u32);
        code_builder.emit_u32(instruction);
        Ok(())
    }

    /// 编译8位存储指令: STRB Wn, [Xn, #offset]
    fn compile_store8(
        &mut self,
        addr: &Register,
        offset: i64,
        src: &Operand,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let addr_reg = self.get_physical_register(addr)?;
        match src {
            Operand::Register { id } => {
                let src_reg = self.get_physical_register(id)?;
                let instruction = 0x39000000u32 | (((offset as u32) & 0xFFF) << 10) | ((addr_reg as u32) << 5) | (src_reg as u32);
                code_builder.emit_u32(instruction);
            }
            Operand::Immediate { value } => {
                let temp_reg = AArch64Register::X16 as u8;
                self.emit_mov_reg_imm64(code_builder, temp_reg, *value);
                let instruction = 0x39000000u32 | (((offset as u32) & 0xFFF) << 10) | ((addr_reg as u32) << 5) | (temp_reg as u32);
                code_builder.emit_u32(instruction);
            }
            _ => return Err(format!("不支持的 Store8 操作数类型: {:?}", src).into()),
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
    ) -> crate::Result<()> {
        let base_reg = self.get_physical_register(addr)?;
        let reg1 = self.get_physical_register(src1)?;
        let reg2 = self.get_physical_register(src2)?;

        // STP <reg1>, <reg2>, [<base>, #<offset>]
        // offset必须是8的倍数，且在±256KB范围内
        if offset % 8 != 0 {
            return Err(format!(
                "StorePair offset must be 8-byte aligned, got: {}",
                offset
            ).into());
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
                ).into());
            }

            self.emit_stp_offset(code_builder, reg1, reg2, base_reg, aligned_offset as i32);
        } else {
            // 非SP寄存器，使用原始offset
            let scaled_offset = offset / 8;
            if scaled_offset < -4096 || scaled_offset > 4095 {
                return Err(format!(
                    "StorePair offset out of range (±32KB): {}",
                    scaled_offset
                ).into());
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
    ) -> crate::Result<()> {
        let base_reg = self.get_physical_register(addr)?;
        let reg1 = self.get_physical_register(dst1)?;
        let reg2 = self.get_physical_register(dst2)?;

        // LDP <reg1>, <reg2>, [<base>, #<offset>]
        // offset必须是8的倍数，且在±256KB范围内
        if offset % 8 != 0 {
            return Err(format!(
                "LoadPair offset must be 8-byte aligned, got: {}",
                offset
            ).into());
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
                ).into());
            }

            self.emit_ldp_offset(code_builder, reg1, reg2, base_reg, aligned_offset as i32);
        } else {
            // 非SP寄存器，使用原始offset
            let scaled_offset = offset / 8;
            if scaled_offset < -4096 || scaled_offset > 4095 {
                return Err(format!(
                    "LoadPair offset out of range (±32KB): {}",
                    scaled_offset
                ).into());
            }

            self.emit_ldp_offset(code_builder, reg1, reg2, base_reg, offset as i32);
        }
        Ok(())
    }

    // ========================================================================
    // Runtime 委托函数：全部使用 JitCompiler trait 的 default method 实现
    // （alloc/free/retain/release/safepoint/string_*/print_*/to_string）
    // emit_runtime_call 的实现在 impl JitCompiler for AArch64Compiler 块中
    // ========================================================================

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

        // 保守策略：和 x86 一样保存所有 caller-saved + 所有使用的 callee-saved
        // 不依赖 metadata（metadata 可能有遗漏导致 GC 无法追踪寄存器中的堆指针）
        let mut regs_to_virtual_stack: Vec<u8> = {
            let mut regs = Vec::new();
            
            // 1. 保存所有 caller-saved 寄存器（FFI 约定：被调用函数可破坏这些寄存器）
            for reg in &self.ffi_calling_convention.caller_saved {
                if !exclude.contains(reg) {
                    regs.push(*reg);
                }
            }
            
            // 2. 保存所有在当前函数中使用的 callee-saved 寄存器
            //    GC 需要扫描这些寄存器中的堆指针
            for reg in &self.vm_calling_convention.callee_saved {
                if self.current_function_use_regs.contains(reg) && !exclude.contains(reg) && !regs.contains(reg) {
                    regs.push(*reg);
                }
            }
            
            regs.sort();
            regs.dedup();
            regs
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

        // 步骤2：在系统栈保存 vm_sp(X10) 和 vm_fp(X11)
        // C 函数会破坏 X9-X15（AAPCS64 caller-saved），包括 vm_sp(X10)
        // 必须在系统栈保存，restore 时先恢复
        let vm_sp_reg = self.vm_calling_convention.stack_pointer;  // X10
        let vm_fp_reg = self.vm_calling_convention.frame_pointer;  // X11
        // STP X10, X11, [SP, #-16]!（X10/X11 不是 SP，合法的 STP）
        let stp_pre = 0xA9BF0000u32
            | ((vm_fp_reg as u32) << 10)
            | ((AArch64Register::SP as u32) << 5)
            | (vm_sp_reg as u32);
        code_builder.emit_u32(stp_pre);

        (regs_to_virtual_stack, virtual_stack_space)
    }
    fn restore_call_clobbered_registers(
        &self,
        code_builder: &mut CodeBuilder,
        regs: &[u8],
        stack_space: usize,
    ) {
        // 步骤1：先从系统栈恢复 vm_sp 和 vm_fp
        // 这必须在用 vm_sp 读取虚拟栈之前完成，因为 C 函数可能破坏了 X10
        let vm_sp_reg = self.vm_calling_convention.stack_pointer;
        let vm_fp_reg = self.vm_calling_convention.frame_pointer;
        // LDP vm_sp, vm_fp, [SP], #16 (post-index)
        let ldp_post = 0xA8C10000u32
            | ((vm_fp_reg as u32) << 10)
            | ((AArch64Register::SP as u32) << 5)
            | (vm_sp_reg as u32);
        code_builder.emit_u32(ldp_post);

        // 步骤2：从虚拟栈恢复寄存器
        if !regs.is_empty() {
            // 先用偏移加载所有寄存器（保持虚拟SP不变）
            for (idx, reg) in regs.iter().enumerate() {
                self.emit_ldr_reg_mem(code_builder, *reg, vm_sp_reg, (idx * 8) as i32);
            }

            // 然后一次性恢复虚拟栈指针
            self.emit_add_reg_reg_imm(
                code_builder,
                vm_sp_reg,
                vm_sp_reg,
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

    /// 生成ORR三寄存器指令: ORR <Xd>, <Xn>, <Xm>
    fn emit_orr_reg_reg(&self, code_builder: &mut CodeBuilder, dst: u8, src1: u8, src2: u8) {
        // ORR <Xd>, <Xn>, <Xm>
        // 31|30|29|28 27 26 25 24 23 22 21|20 16|15 10|9 5|4 0
        // 1 |0 |1 |0  0  0  1  0  0  0  0 |Xm   |0     |Xn |Xd
        let instruction =
            0xAA000000u32 | ((src2 as u32) << 16) | ((src1 as u32) << 5) | (dst as u32);
        code_builder.emit_bytes(&instruction.to_le_bytes());
    }

    /// 生成EOR三寄存器指令: EOR <Xd>, <Xn>, <Xm>
    fn emit_eor_reg_reg(&self, code_builder: &mut CodeBuilder, dst: u8, src1: u8, src2: u8) {
        // EOR <Xd>, <Xn>, <Xm>
        // 31|30|29|28 27 26 25 24 23 22 21|20 16|15 10|9 5|4 0
        // 1 |1 |0 |0  0  0  1  0  0  0  0 |Xm   |0     |Xn |Xd
        let instruction =
            0xCA000000u32 | ((src2 as u32) << 16) | ((src1 as u32) << 5) | (dst as u32);
        code_builder.emit_bytes(&instruction.to_le_bytes());
    }

    /// 生成MVN指令: MVN <Xd>, <Xm> (等价于 ORR <Xd>, XZR, <Xm>)
    fn emit_mvn_reg_reg(&self, code_builder: &mut CodeBuilder, dst: u8, src: u8) {
        // ORR <Xd>, XZR, <Xm> — 即 MVN <Xd>, <Xm>
        // 31|30|29|28 27 26 25 24 23 22 21|20 16|15 10|9 5|4 0
        // 1 |0 |1 |0  0  0  1  0  0  0  0 |Xm   |0     |Xn |Xd
        // XZR 编码为 31
        let xzr = 31u32;
        let instruction =
            0xAA000000u32 | ((src as u32) << 16) | (xzr << 5) | (dst as u32);
        code_builder.emit_bytes(&instruction.to_le_bytes());
    }

    /// 生成LSLV指令（逻辑左移）: LSLV <Xd>, <Xn>, <Xm>
    fn emit_lslv_reg_reg(&self, code_builder: &mut CodeBuilder, dst: u8, src1: u8, src2: u8) {
        // LSLV <Xd>, <Xn>, <Xm>
        // 31|30|29|28 27 26 25 24 23 22 21|20 16|15|14 10|9 5|4 0
        // 1 |0 |0 |1  1  0  1  0  0  0  0 |Xm   |0 |0 1 0 0|Xn |Xd
        // op2=0x08 (LSLV)
        let instruction =
            0x9AC02000u32 | ((src2 as u32) << 16) | ((src1 as u32) << 5) | (dst as u32);
        code_builder.emit_bytes(&instruction.to_le_bytes());
    }

    /// 生成ASRV指令（算术右移）: ASRV <Xd>, <Xn>, <Xm>
    fn emit_asrv_reg_reg(&self, code_builder: &mut CodeBuilder, dst: u8, src1: u8, src2: u8) {
        // ASRV <Xd>, <Xn>, <Xm>
        // 31|30|29|28 27 26 25 24 23 22 21|20 16|15|14 10|9 5|4 0
        // 1 |0 |0 |1  1  0  1  0  1  0  0 |Xm   |0 |0 1 0 0|Xn |Xd
        // op2=0x0A (ASRV)
        let instruction =
            0x9AC02800u32 | ((src2 as u32) << 16) | ((src1 as u32) << 5) | (dst as u32);
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

    /// 生成MSUB指令: Xd = Xa - Xn * Xm
    /// 用于 AArch64 取余运算: remainder = dividend - quotient * divisor
    fn emit_msub(&self, code_builder: &mut CodeBuilder, dst: u8, rn: u8, rm: u8, ra: u8) {
        // MSUB <Xd>, <Xn>, <Xm>, <Xa>
        // 31|30|29|28 27 26 25 24 23|22 21|20 16|15|14 10|9 5|4 0
        // 1 |0 |0 |1  1  0  1  1  0 |0  0 |Xm   |1 |Ra   |Xn |Xd
        // MADD: 0x9B000000, o0=0
        // MSUB: 0x9B008000, o0=1 (bit 15)
        let instruction =
            0x9B008000u32 | ((rm as u32) << 16) | ((ra as u32) << 10) | ((rn as u32) << 5) | (dst as u32);
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
            // 当 dst 或 base 是 X16 时，切换到 X17 避免 MOV X16 覆盖 dst/base
            let temp_reg = if dst == 16 || base == 16 {
                AArch64Register::X17 as u8
            } else {
                AArch64Register::X16 as u8
            };
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
            // 当 src 或 base 是 X16 时，切换到 X17 避免 MOV X16 覆盖 src/base
            let temp_reg = if src == 16 || base == 16 {
                AArch64Register::X17 as u8
            } else {
                AArch64Register::X16 as u8
            };
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

    /// 生成函数序言（main函数，从宿主环境调用）
    /// 对齐 x86_64 emit_main_function_prologue 的设计：
    /// - vm_sp=X10, vm_fp=X11, SP 始终指向系统栈
    /// - 在系统栈保存 callee-saved
    /// - 在虚拟栈保存系统 SP（用于返回时恢复）
    fn emit_function_prologue(&self, code_builder: &mut CodeBuilder) -> crate::Result<()> {
        // AAPCS64 入口参数：X0 = 虚拟栈顶, X1 = 虚拟栈底
        let x0 = AArch64Register::X0 as u8;
        let vm_sp = self.vm_calling_convention.stack_pointer;   // X10
        let vm_fp = self.vm_calling_convention.frame_pointer;   // X11

        // 1. AAPCS64 标准序言：保存 FP 和 LR
        // STP X29, X30, [SP, #-16]!
        let stp_x29_x30 = 0xA9BF7BFDu32;
        code_builder.emit_u32(stp_x29_x30);
        // MOV X29, SP
        self.emit_mov_reg_reg(code_builder, AArch64Register::X29 as u8, AArch64Register::SP as u8);

        // 2. 保存 callee-saved 寄存器到系统栈
        self.save_callee_saved_registers(code_builder)?;

        // 3. 设置虚拟栈指针（X10/X11 独立寄存器，不碰 SP）
        // MOV X10, X0（虚拟栈顶地址）
        // MOV X11, X1（虚拟栈底地址）
        self.emit_mov_reg_reg(code_builder, vm_sp, x0);
        self.emit_mov_reg_reg(code_builder, vm_fp, AArch64Register::X1 as u8);

        // 4. 在虚拟栈上保存系统 SP（返回时需要恢复）
        // SUB vm_sp, vm_sp, #16
        // MOV X16, SP; STR X16, [vm_sp, #0]  (系统SP)
        // STR X30, [vm_sp, #8]  (返回地址)
        self.emit_sub_reg_reg_imm(code_builder, vm_sp, vm_sp, 16);
        self.emit_mov_reg_reg(code_builder, 16, AArch64Register::SP as u8);
        self.emit_str_reg_mem(code_builder, 16, vm_sp, 0);
        self.emit_str_reg_mem(code_builder, AArch64Register::X30 as u8, vm_sp, 8);

        // 5. 为返回值槽分配空间（16字节）
        self.save_return_slot_pointer(code_builder);

        // 6. 设置帧指针（不分配帧空间——由 LIR Sub 指令管理）
        self.emit_mov_reg_reg(code_builder, vm_fp, vm_sp);

        Ok(())
    }

    /// 生成内部函数序言（用于虚拟机内部函数调用）
    fn emit_internal_function_prologue(
        &self,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let vm_sp_reg = self.vm_calling_convention.stack_pointer;
        let vm_fp_reg = self.vm_calling_convention.frame_pointer;
        let tmp_reg = AArch64Register::X16 as u8;

        // 1. 保存旧 vm_sp 到临时寄存器（在 SUB 之前）
        self.emit_mov_reg_reg(code_builder, tmp_reg, vm_sp_reg);

        // 2. 在虚拟栈上分配空间（保存 old_sp 和 old_fp）
        self.emit_sub_reg_reg_imm(code_builder, vm_sp_reg, vm_sp_reg, 16);

        // 3. 保存 old_fp 和 old_sp
        self.emit_str_reg_mem(code_builder, vm_fp_reg, vm_sp_reg, 8);    // [vm_sp+8] = old_fp
        self.emit_str_reg_mem(code_builder, tmp_reg, vm_sp_reg, 0);      // [vm_sp+0] = old_sp

        // 4. 保存 callee-saved 寄存器到虚拟栈
        let callee_saved = &self.get_vm_callee_saved_registers();
        if callee_saved.is_empty() {
            if self.debug_mode {
                log::debug!("生成内部函数序言：无需保存寄存器");
            }
            return Ok(());
        }

        for &reg in callee_saved {
            self.emit_sub_reg_reg_imm(code_builder, vm_sp_reg, vm_sp_reg, 16);
            self.emit_str_reg_mem(code_builder, reg, vm_sp_reg, 0);
        }

        // 5. 设置帧指针（不分配帧空间——由 LIR Sub 指令管理）
        self.emit_mov_reg_reg(code_builder, vm_fp_reg, vm_sp_reg);

        Ok(())
    }

    /// 生成内部函数尾声（用于虚拟机内部函数调用）
    fn emit_internal_function_epilogue(
        &self,
        code_builder: &mut CodeBuilder,
    ) -> crate::Result<()> {
        let vm_sp_reg = self.vm_calling_convention.stack_pointer;
        let vm_fp_reg = self.vm_calling_convention.frame_pointer;

        // 注意：帧空间由 LIR 的 Add/Sub 指令管理，这里不恢复帧空间

        // 按逆序恢复 callee-saved 寄存器
        let callee_saved = &self.get_vm_callee_saved_registers();
        for &reg in callee_saved.iter().rev() {
            self.emit_ldr_reg_mem(code_builder, reg, vm_sp_reg, 0);
            self.emit_add_reg_reg_imm(code_builder, vm_sp_reg, vm_sp_reg, 16);
        }

        // 恢复 vm_fp 和 vm_sp 从虚拟栈
        // 注意顺序：先读 vm_fp（在 vm_sp 被覆盖之前）
        self.emit_ldr_reg_mem(code_builder, vm_fp_reg, vm_sp_reg, 8);   // 先读 vm_fp
        self.emit_ldr_reg_mem(code_builder, vm_sp_reg, vm_sp_reg, 0);   // 再读 vm_sp
        // 不弹出 16 字节——vm_sp 现在指向调用者保存返回地址的位置

        Ok(())
    }

    /// 生成主函数尾声（用于与宿主环境交互的main函数）
    /// 对齐 x86_64 emit_main_function_epilogue 的设计：
    /// - vm_sp=X10, vm_fp=X11, SP 始终指向系统栈
    /// - 从虚拟栈读取系统SP并恢复
    /// - 从系统栈恢复 callee-saved
    fn emit_function_epilogue(&self, code_builder: &mut CodeBuilder) -> crate::Result<()> {
        let vm_sp_reg = self.vm_calling_convention.stack_pointer; // X10

        // 虚拟栈布局（从低到高）：
        //   [系统SP + X30, 16字节]  ← prologue 保存
        //   [返回值槽, 16字节]      ← prologue 分配（compile_return 已弹出）
        //   [栈帧空间, N字节]       ← LIR 管理
        //
        // compile_return 已弹出返回值槽 (+16) 和帧空间 (+frame_size)
        // 此时 vm_sp 指向 [系统SP + X30] 的位置

        // 1. 从虚拟栈读取系统 SP 和返回地址
        self.emit_ldr_reg_mem(code_builder, 16, vm_sp_reg, 0);           // X16 = 系统SP
        self.emit_ldr_reg_mem(code_builder, AArch64Register::X30 as u8, vm_sp_reg, 8); // X30 = 返回地址

        // 2. 恢复系统栈指针（X16 保存了序言中的系统 SP）
        self.emit_mov_reg_reg(code_builder, AArch64Register::SP as u8, 16);

        // 3. 从系统栈恢复 callee-saved 寄存器
        self.restore_callee_saved_registers(code_builder)?;

        // 4. LDP X29, X30, [SP], #16 — 恢复帧指针和链接寄存器
        let ldp_x29_x30 = 0xA8C17BFDu32;
        code_builder.emit_u32(ldp_x29_x30);

        Ok(())
    }

    /// 保存返回槽指针（caller通过X0传入）
    fn save_return_slot_pointer(&self, code_builder: &mut CodeBuilder) {
        let vm_sp = self.vm_calling_convention.stack_pointer;
        self.emit_sub_reg_reg_imm(code_builder, vm_sp, vm_sp, 16);
        self.emit_str_reg_mem(code_builder, AArch64Register::X0 as u8, vm_sp, 0);
    }

    /// 恢复返回槽指针并弹出栈空间
    fn load_and_pop_return_slot_pointer(&self, code_builder: &mut CodeBuilder, dst: u8) {
        let vm_sp = self.vm_calling_convention.stack_pointer;
        self.emit_ldr_reg_mem(code_builder, dst, vm_sp, 0);
        self.emit_add_reg_reg_imm(
            code_builder,
            vm_sp,
            vm_sp,
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
        jit_utils::is_entry_function(function_name, program)
    }

    /// 保存 callee-saved 寄存器
    fn save_callee_saved_registers(&self, code_builder: &mut CodeBuilder) -> crate::Result<()> {
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
    fn restore_callee_saved_registers(&self, code_builder: &mut CodeBuilder) -> crate::Result<()> {
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
    ) -> crate::Result<CompiledFunction> {
        if self.debug_mode {
            log::debug!("AArch64: 开始编译函数 '{}'", function.name);
        }

        // 🔧 修复：设置当前函数名，用于生成唯一label
        self.current_function_name = function.name.clone();
        self.unique_label_counter = 0; // 重置计数器

        // 缓存当前函数的 callee-saved 信息
        self.current_function_use_regs = function.get_used_regs().to_vec();

        // 计算栈帧大小（与 x86 编译器一致）
        // prologue 不分配帧空间——由 LIR 的 Sub vm_sp, N 指令分配
        self.current_stack_frame_size = 0;
        // epilogue 需要知道帧大小来跳过帧区域
        self.stack_frame_size_for_epilogue = function.stack_frame_size as usize;

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
            return Err(format!("函数 '{}' 的第一个指令必须是label", function.name).into());
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

        // debug 模式下输出 label 信息
        if self.debug_mode {
            let labels = code_builder.exported_labels();
            log::debug!("🔧 编译函数 '{}' 时收集到 {} 个label", function.name, labels.len());
            for (label, offset) in labels {
                log::debug!("🔧   label: {} -> 偏移: {}", label, offset);
            }
        }

        // 构建 CompiledFunction（通过共享宏统一 finalize 逻辑）
        let compiled_function = finalize_compiled_function!(code_builder, function.name, "AArch64");
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

    /// AArch64 平台的 runtime call 实现
    ///
    /// AArch64 特点：
    /// - 使用 `RuntimeCallContext` 获取 instruction_metadata 中的活跃寄存器信息
    /// - GC safepoint 会保存所有活跃寄存器 + 被使用的 callee-saved 寄存器
    /// - vm_sp(X10)/vm_fp(X11) 需要额外保存到系统栈（AAPCS64 caller-saved）
    /// - 返回值在 restore 之前移动（避免 restore 覆盖 X0）
    fn emit_runtime_call(
        &mut self,
        code_builder: &mut CodeBuilder,
        call: RuntimeCall,
        result: Option<&Register>,
        ctx: Option<super::compiler_trait::RuntimeCallContext<'_>>,
    ) -> crate::Result<()> {
        let return_reg = AArch64Register::X0 as u8;

        // 从 ctx 中提取平台特定的上下文信息
        let (instruction_index, function) = ctx.as_ref()
            .map(|c| (c.instruction_index, c.function))
            .unzip();
        let instruction_index = instruction_index.unwrap_or(0);

        // 使用统一的 exclude 计算
        let dst_phys = result.and_then(|r| self.get_physical_register(r).ok());
        let exclude: Vec<u8> = compute_exclude_dst_reg(&call, result, dst_phys);

        // 从 metadata 中获取调用位置活跃寄存器信息
        let live_register_info = function
            .and_then(|f| {
                f.instruction_metadata
                    .get(&instruction_index)
                    .and_then(|meta| meta.live_register_info.as_ref())
            });

        // 判断是否是 GC safepoint：AllocAligned, Free, GcSafepoint 会触发 GC
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

        self.emit_runtime_dispatch(code_builder, call.intrinsic.symbol_ptr() as u64);

        // 在 restore 之前把返回值从 X0 移到 dst_reg
        if let (Some(dst), true) = (result, call.expects_result()) {
            let dst_reg = self.get_physical_register(dst)?;
            if dst_reg != return_reg {
                self.emit_mov_reg_reg(code_builder, dst_reg, return_reg);
            }
        }

        self.restore_call_clobbered_registers(code_builder, &saved_regs, stack_space);

        Ok(())
    }
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
