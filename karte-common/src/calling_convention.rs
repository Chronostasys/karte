//! Karte 虚拟机调用约定
//!
//! 定义了函数调用时的寄存器使用规范，遵循现代编译器的最佳实践。
//! 采用类似 System V ABI 的约定，适合 RISC 架构。

use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, karte_ir_derive::IrCodec)]
pub enum Register {
    #[ir_codec(token = "#v")]
    Virtual(#[ir_codec(args)] usize),

    #[ir_codec(token = "#p")]
    Physical(#[ir_codec(args)] u8),
}

impl Register {
    pub fn is_virtual(&self) -> bool {
        matches!(self, Register::Virtual(_))
    }
    pub fn is_physical(&self) -> bool {
        matches!(self, Register::Physical(_))
    }
    pub fn id(&self) -> usize {
        match self {
            Register::Virtual(id) => *id,
            Register::Physical(id) => *id as usize,
        }
    }

    pub fn as_physical(&self) -> Register {
        match self {
            Register::Virtual(id) => Register::Physical(*id as _),
            Register::Physical(_) => *self,
        }
    }
}

impl Default for Register {
    fn default() -> Self {
        Register::Virtual(0)
    }
}

/// 物理寄存器编号
pub type PhysicalRegister = u8;

// ARM64 AAPCS64 寄存器命名约定
pub const REG_X0: PhysicalRegister = 0; // 返回值/参数1
pub const REG_X1: PhysicalRegister = 1; // 参数2/返回值2
pub const REG_X2: PhysicalRegister = 2; // 参数3
pub const REG_X3: PhysicalRegister = 3; // 参数4
pub const REG_X4: PhysicalRegister = 4; // 参数5
pub const REG_X5: PhysicalRegister = 5; // 参数6
pub const REG_X6: PhysicalRegister = 6; // 参数7
pub const REG_X7: PhysicalRegister = 7; // 参数8
pub const REG_X8: PhysicalRegister = 8; // 间接结果位置寄存器
pub const REG_X9: PhysicalRegister = 9; // 临时寄存器
pub const REG_X10: PhysicalRegister = 10; // 临时寄存器
pub const REG_X11: PhysicalRegister = 11; // 临时寄存器
pub const REG_X12: PhysicalRegister = 12; // 临时寄存器
pub const REG_X13: PhysicalRegister = 13; // 临时寄存器
pub const REG_X14: PhysicalRegister = 14; // 临时寄存器
pub const REG_X15: PhysicalRegister = 15; // 临时寄存器
pub const REG_X16: PhysicalRegister = 16; // 过程内调用临时寄存器1
pub const REG_X17: PhysicalRegister = 17; // 过程内调用临时寄存器2
pub const REG_X18: PhysicalRegister = 18; // 平台寄存器
pub const REG_X19: PhysicalRegister = 19; // Callee-saved
pub const REG_X20: PhysicalRegister = 20; // Callee-saved
pub const REG_X21: PhysicalRegister = 21; // Callee-saved
pub const REG_X22: PhysicalRegister = 22; // Callee-saved
pub const REG_X23: PhysicalRegister = 23; // Callee-saved
pub const REG_X24: PhysicalRegister = 24; // Callee-saved
pub const REG_X25: PhysicalRegister = 25; // Callee-saved
pub const REG_X26: PhysicalRegister = 26; // Callee-saved
pub const REG_X27: PhysicalRegister = 27; // Callee-saved
pub const REG_X28: PhysicalRegister = 28; // Callee-saved
pub const REG_X29: PhysicalRegister = 29; // 帧指针 (FP)
pub const REG_X30: PhysicalRegister = 30; // 链接寄存器 (LR)
pub const REG_SP: PhysicalRegister = 31; // 栈指针 (SP) - 实际使用特殊编码

// x86-64 System V AMD64 ABI 寄存器命名约定
// 注意: PhysicalRegister 值直接等于 x86-64 硬件寄存器编号 (与 AArch64 一致)
pub const REG_RAX: PhysicalRegister = 0;  // 硬件 RAX = 0, 返回值寄存器
pub const REG_RCX: PhysicalRegister = 1;  // 硬件 RCX = 1, 参数4 (C调用约定)
pub const REG_RDX: PhysicalRegister = 2;  // 硬件 RDX = 2, 参数3 (C调用约定)
pub const REG_RBX: PhysicalRegister = 3;  // 硬件 RBX = 3, Callee-saved
pub const REG_RSP: PhysicalRegister = 4;  // 硬件 RSP = 4, 栈指针
pub const REG_RBP: PhysicalRegister = 5;  // 硬件 RBP = 5, 帧指针 (Callee-saved)
pub const REG_RSI: PhysicalRegister = 6;  // 硬件 RSI = 6, 参数2 (C调用约定)
pub const REG_RDI: PhysicalRegister = 7;  // 硬件 RDI = 7, 参数1 (C调用约定)
pub const REG_R8:  PhysicalRegister = 8;  // 硬件 R8 = 8, 参数5 (C调用约定)
pub const REG_R9:  PhysicalRegister = 9;  // 硬件 R9 = 9, 参数6 (C调用约定)
pub const REG_R10: PhysicalRegister = 10; // 硬件 R10 = 10, 临时寄存器
pub const REG_R11: PhysicalRegister = 11; // 硬件 R11 = 11, 临时寄存器
pub const REG_R12: PhysicalRegister = 12; // 硬件 R12 = 12, Callee-saved / Effect栈指针
pub const REG_R13: PhysicalRegister = 13; // 硬件 R13 = 13, Callee-saved
pub const REG_R14: PhysicalRegister = 14; // 硬件 R14 = 14, Callee-saved
pub const REG_R15: PhysicalRegister = 15; // 硬件 R15 = 15, Callee-saved

// 保持向后兼容的别名
pub const REG_RETURN: PhysicalRegister = REG_X0;
pub const REG_ARG0: PhysicalRegister = REG_X0;
pub const REG_ARG1: PhysicalRegister = REG_X1;
pub const REG_ARG2: PhysicalRegister = REG_X2;
pub const REG_ARG3: PhysicalRegister = REG_X3;
pub const REG_RETURN_ADDRESS: PhysicalRegister = REG_X30;
pub const REG_STACK_POINTER: PhysicalRegister = REG_SP;
pub const REG_FRAME_POINTER: PhysicalRegister = REG_X29;
pub const REG_EFFECT_STACK_POINTER: PhysicalRegister = REG_X12; // 使用临时寄存器
pub const REG_EFFECT_PAYLOAD: PhysicalRegister = REG_X0; // 使用返回值寄存器
pub const REG_EFFECT_TAG: PhysicalRegister = REG_X10; // 使用临时寄存器
pub const REG_EFFECT_RESUME_TMP: PhysicalRegister = REG_X15; // 使用临时寄存器

pub trait CC {
    fn is_caller_saved(&self, reg: PhysicalRegister) -> bool;
    fn is_callee_saved(&self, reg: PhysicalRegister) -> bool;
    /// 获取函数调用需要保存的寄存器列表
    fn get_caller_save_registers(
        &self,
        live_registers: &[PhysicalRegister],
    ) -> Vec<PhysicalRegister> {
        live_registers
            .iter()
            .filter(|&&reg| self.is_caller_saved(reg))
            .copied()
            .collect()
    }

    /// 获取函数需要保存的callee-saved寄存器
    fn get_callee_save_registers(
        &self,
        used_registers: &[PhysicalRegister],
    ) -> Vec<PhysicalRegister> {
        used_registers
            .iter()
            .filter(|&&reg| self.is_callee_saved(reg))
            .copied()
            .collect()
    }
}

/// 调用约定配置
#[derive(Debug, Clone)]
pub struct CallingConvention {
    /// 参数传递寄存器 (按顺序使用)
    pub argument_registers: Vec<PhysicalRegister>,
    /// 返回值寄存器
    pub return_register: PhysicalRegister,
    /// Caller-saved 寄存器 (调用者负责保存)
    pub caller_saved: HashSet<PhysicalRegister>,
    /// Callee-saved 寄存器 (被调用者负责保存)
    pub callee_saved: HashSet<PhysicalRegister>,
    /// 栈指针寄存器
    pub stack_pointer: PhysicalRegister,
    /// 帧指针寄存器
    pub frame_pointer: PhysicalRegister,
    /// 返回地址寄存器 (用于存储返回地址)
    pub return_address: PhysicalRegister,
    /// 代数效应栈指针寄存器
    pub effect_stack_pointer: PhysicalRegister,
    /// 代数效应 payload 寄存器（用于 resume/perform 传递值）
    pub effect_payload_register: PhysicalRegister,
    /// 代数效应 handler 匹配时保存 tag 的寄存器
    pub effect_tag_register: PhysicalRegister,
    /// 代数效应 resume 时保存跳转目标的寄存器
    pub effect_resume_temp: PhysicalRegister,
    /// 临时寄存器 (可以自由使用)
    pub temp_registers: Vec<PhysicalRegister>,
    /// 栈对齐要求（字节）
    pub stack_alignment: usize,
    /// 是否使用系统SP（需要硬件对齐）
    pub use_system_stack_pointer: bool,
}

impl CC for CallingConvention {
    /// 检查寄存器是否为 caller-saved
    fn is_caller_saved(&self, reg: PhysicalRegister) -> bool {
        self.caller_saved.contains(&reg)
    }

    /// 检查寄存器是否为 callee-saved
    fn is_callee_saved(&self, reg: PhysicalRegister) -> bool {
        self.callee_saved.contains(&reg)
    }
}

impl Default for CallingConvention {
    fn default() -> Self {
        Self::standard()
    }
}

impl CallingConvention {
    /// 创建标准调用约定（根据目标架构自动选择）
    ///
    /// 这个方法会根据编译目标架构返回对应的调用约定，确保 LIR passes 使用正确的架构配置：
    /// - x86-64: System V AMD64 ABI
    /// - AArch64: AAPCS64
    #[cfg(target_arch = "x86_64")]
    pub fn standard() -> Self {
        Self::x86_64()
    }
    
    #[cfg(target_arch = "aarch64")]
    pub fn standard() -> Self {
        Self::aarch64()
    }
    
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    pub fn standard() -> Self {
        compile_error!("不支持的目标架构");
    }

    /// 创建 x86-64 System V AMD64 ABI 调用约定
    ///
    /// 遵循 System V Application Binary Interface AMD64 Architecture Processor Supplement
    ///
    /// 寄存器分配策略 (C FFI调用约定):
    /// - RDI, RSI, RDX, RCX, R8, R9: 参数传递 (最多6个整数/指针参数)
    /// - RAX: 返回值寄存器
    /// - RBX, R12-R15, RBP: Callee-saved 寄存器
    /// - RAX, RCX, RDX, RSI, RDI, R8-R11: Caller-saved 寄存器
    /// - RSP: 栈指针
    ///
    /// VM内部调用约定 (Karte VM):
    /// - R9 (param1), R8 (param2), R2 (param3), R3 (param4), R4 (param5), R5 (param6): VM参数传递
    /// - RAX (R0): VM返回值
    /// - R10: VM栈指针
    /// - R11: VM帧指针
    /// - R12: Effect栈指针
    /// - R13-R15, RBX: VM Callee-saved
    ///
    /// 说明: 使用 R9/R8 作为前两个VM参数，以避免与C FFI的 RDI/RSI 冲突
    pub fn x86_64() -> Self {
        let mut caller_saved = HashSet::new();
        // C FFI caller-saved: RAX, RCX, RDX, RSI, RDI, R8-R11
        caller_saved.insert(REG_RAX); // 0
        caller_saved.insert(REG_RCX); // 1
        caller_saved.insert(REG_RDX); // 2
        caller_saved.insert(REG_RSI); // 6
        caller_saved.insert(REG_RDI); // 7
        caller_saved.insert(REG_R8);  // 8
        caller_saved.insert(REG_R9);  // 9
        caller_saved.insert(REG_R10); // 10
        caller_saved.insert(REG_R11); // 11

        let mut callee_saved = HashSet::new();
        // C FFI callee-saved: RBX, R12-R15, RBP, RSP
        callee_saved.insert(REG_RBX);  // 3
        callee_saved.insert(REG_RBP);  // 5
        callee_saved.insert(REG_R12);  // 12
        callee_saved.insert(REG_R13);  // 13
        callee_saved.insert(REG_R14);  // 14
        callee_saved.insert(REG_R15);  // 15
        callee_saved.insert(REG_RSP);  // 4 (特殊寄存器)

        Self {
            // VM内部调用约定: 使用 x86-64 自己的寄存器编号（0-15）
            // ⚠️ 注意：x86-64 只有 16 个通用寄存器，绝对不能使用 16 以上的编号！
            argument_registers: vec![
                REG_RAX, // 0 - 参数1 / 返回值
                REG_RCX, // 1 - 参数2
                REG_RDX, // 2 - 参数3
                REG_RBX, // 3 - 参数4
                REG_R8,  // 8 - 参数5
                REG_R9,  // 9 - 参数6
                REG_R10, // 10 - 参数7
                REG_R11, // 11 - 参数8
            ],
            return_register: REG_RAX,            // 0 - RAX
            caller_saved,
            callee_saved,
            stack_pointer: REG_RSP,              // 4 - RSP 作为VM栈指针
            frame_pointer: REG_RBP,              // 5 - RBP 作为VM帧指针
            return_address: REG_RAX,             // 0 - RAX 作为返回地址占位（x86用栈）
            effect_stack_pointer: REG_R12,       // 12 - R12
            effect_payload_register: REG_RAX,    // 0 - RAX
            effect_tag_register: REG_R10,        // 10 - R10
            effect_resume_temp: REG_R15,         // 15 - R15
            temp_registers: {
                // 临时寄存器包括所有 caller-saved 寄存器
                vec![
                    REG_RAX, REG_RCX, REG_RDX, REG_RSI, REG_RDI,
                    REG_R8, REG_R9, REG_R10, REG_R11,
                    // Callee-saved 也可用作临时寄存器（需要保存/恢复）
                    REG_RBX, REG_R12, REG_R13, REG_R14, REG_R15,
                ]
            },
            stack_alignment: 16,                 // x86-64 要求16字节对齐
            use_system_stack_pointer: true,      // 🔧 恢复：使用系统 RSP（与AArch64一致）
        }
    }

    /// 创建 ARM64 AAPCS64 调用约定
    ///
    /// 遵循 ARM Procedure Call Standard for the 64-bit Architecture (AAPCS64)
    ///
    /// 寄存器分配策略:
    /// - x0-x7: 参数传递 (最多8个参数) / 返回值 (x0-x1)
    /// - x8: 间接结果位置寄存器
    /// - x9-x15: 临时寄存器 (caller-saved)
    /// - x16-x17: 过程内调用临时寄存器 (caller-saved)
    /// - x18: 平台寄存器 (caller-saved)
    /// - x19-x28: Callee-saved 寄存器
    /// - x29 (FP): 帧指针 (callee-saved)
    /// - x30 (LR): 链接寄存器 (callee-saved)
    /// - sp (31): 栈指针 (特殊寄存器)
    ///
    /// Caller-saved: x0-x18 (易失寄存器，调用者负责保存)
    /// Callee-saved: x19-x30 (非易失寄存器，被调用者负责保存)
    pub fn aarch64() -> Self {
        let mut caller_saved = HashSet::new();
        // x0-x18 都是 caller-saved (易失寄存器)
        for reg in 0..=18 {
            caller_saved.insert(reg as PhysicalRegister);
        }

        let mut callee_saved = HashSet::new();
        // x19-x28 是 callee-saved (非易失寄存器)
        for reg in 19..=31 {
            callee_saved.insert(reg as PhysicalRegister);
        }
        // // x29 (FP) 和 x30 (LR) 也是 callee-saved
        // callee_saved.insert(REG_X29); // 帧指针
        // callee_saved.insert(REG_X30); // 链接寄存器

        Self {
            // 支持 8 个参数寄存器 (x0-x7)，符合 AAPCS64
            argument_registers: vec![
                REG_X0, REG_X1, REG_X2, REG_X3, REG_X4, REG_X5, REG_X6, REG_X7,
            ],
            return_register: REG_X0, // 返回值在 x0
            caller_saved,
            callee_saved,
            stack_pointer: REG_SP,           // 栈指针使用特殊寄存器
            frame_pointer: REG_X29,          // x29 作为帧指针
            return_address: REG_X30,         // x30 作为链接寄存器
            effect_stack_pointer: REG_X12,   // 使用临时寄存器 x12
            effect_payload_register: REG_X0, // 使用返回值寄存器 x0
            effect_tag_register: REG_X10,    // 使用临时寄存器 x10
            effect_resume_temp: REG_X15,     // 使用临时寄存器 x15
            temp_registers: {
                // 临时寄存器包括 caller-saved 寄存器 (除了特殊用途的)
                let mut temps = Vec::new();
                // x0-x7 (参数寄存器) 和 x8-x15 (临时寄存器)
                temps.extend_from_slice(&[
                    REG_X0, REG_X1, REG_X2, REG_X3, REG_X4, REG_X5, REG_X6, REG_X7,
                ]);
                temps.extend_from_slice(&[
                    REG_X8, REG_X9, REG_X10, REG_X11, REG_X12, REG_X13, REG_X14, REG_X15,
                ]);
                // x16-x18 也是临时寄存器
                temps.extend_from_slice(&[REG_X16, REG_X17, REG_X18]);
                // callee-saved 寄存器也可以用作临时寄存器（需要保存/恢复）
                for reg in 19..=28 {
                    temps.push(reg as PhysicalRegister);
                }
                temps
            },
            stack_alignment: 16,            // AArch64要求16字节对齐
            use_system_stack_pointer: true, // 使用系统SP
        }
    }

    /// 获取指定数量的参数寄存器
    pub fn get_argument_registers(&self, count: usize) -> Vec<PhysicalRegister> {
        self.argument_registers
            .iter()
            .take(count)
            .copied()
            .collect()
    }

    /// 检查是否是特殊寄存器（SP、FP、RA）
    pub fn is_special_register(&self, reg: Register) -> bool {
        matches!(reg, Register::Physical(_))
    }

    /// 获取可用于寄存器分配的通用寄存器
    pub fn get_allocatable_registers(&self) -> Vec<PhysicalRegister> {
        // 扩展寄存器池，包含 r8-r31 作为 callee-saved 寄存器使用
        (0..32u8)
            .filter(|&reg| {
                ![
                    self.return_register,
                    self.return_address,
                    self.stack_pointer,
                    self.frame_pointer,
                    self.effect_stack_pointer,
                    self.effect_payload_register,
                ]
                .contains(&reg)
            })
            .collect()
    }
}

/// 函数调用上下文
#[derive(Debug, Clone)]
pub struct CallContext {
    /// 调用约定
    pub convention: CallingConvention,
    /// 参数数量
    pub arg_count: usize,
    /// 是否有返回值
    pub has_return_value: bool,
    /// 调用前需要保存的寄存器
    pub registers_to_save: Vec<PhysicalRegister>,
}

impl CallContext {
    /// 创建函数调用上下文
    pub fn new(convention: CallingConvention, arg_count: usize, has_return_value: bool) -> Self {
        Self {
            convention,
            arg_count,
            has_return_value,
            registers_to_save: Vec::new(),
        }
    }

    /// 设置需要保存的寄存器
    pub fn set_registers_to_save(&mut self, live_registers: &[PhysicalRegister]) {
        self.registers_to_save = self.convention.get_caller_save_registers(live_registers);
    }

    /// 获取参数传递的寄存器分配
    pub fn get_argument_allocation(&self) -> Vec<PhysicalRegister> {
        self.convention.get_argument_registers(self.arg_count)
    }

    /// 获取返回值寄存器
    pub fn get_return_register(&self) -> Option<PhysicalRegister> {
        if self.has_return_value {
            Some(self.convention.return_register)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standard_calling_convention() {
        let cc = CallingConvention::standard();

        // 测试寄存器分类 (ARM64 AAPCS64)
        // x0-x18 是 caller-saved (易失寄存器)
        assert!(cc.is_caller_saved(REG_X0)); // 返回值/参数1
        assert!(cc.is_caller_saved(REG_X1)); // 参数2
        assert!(cc.is_caller_saved(REG_X9)); // 临时寄存器
        assert!(cc.is_caller_saved(REG_X18)); // 平台寄存器（最后一个 caller-saved）
                                              // x19-x31 是 callee-saved (非易失寄存器)
        assert!(cc.is_callee_saved(REG_X19)); // 第一个 callee-saved
        assert!(cc.is_callee_saved(REG_X29)); // FP
        assert!(cc.is_callee_saved(REG_X30)); // LR
        assert!(cc.is_callee_saved(REG_SP)); // SP

        // 测试参数寄存器分配 (x0, x1, x2)
        let args = cc.get_argument_registers(3);
        assert_eq!(args, vec![REG_X0, REG_X1, REG_X2]);

        // 测试可分配寄存器
        let allocatable = cc.get_allocatable_registers();
        // 验证保留寄存器不被分配
        assert!(!allocatable.contains(&REG_RETURN)); // 返回值寄存器保留 (x0)
        assert!(!allocatable.contains(&REG_STACK_POINTER)); // SP不应该被分配
        assert!(!allocatable.contains(&REG_FRAME_POINTER)); // FP不应该被分配
        assert!(!allocatable.contains(&REG_RETURN_ADDRESS)); // 返回地址保留 (x30)
        assert!(!allocatable.contains(&REG_EFFECT_STACK_POINTER)); // effect 栈顶不可分配 (x12)
        assert!(!allocatable.contains(&REG_EFFECT_PAYLOAD)); // effect payload 保留 (x0)
                                                             // 验证参数寄存器应该可以被分配 (除了 x0)
        assert!(allocatable.contains(&REG_ARG1)); // x1
        assert!(allocatable.contains(&REG_ARG2)); // x2
        assert!(allocatable.contains(&REG_ARG3)); // x3
                                                  // 验证allocatable_registers应该包含callee-saved寄存器
        assert!(
            allocatable.len() > 3,
            "应该包含更多可分配寄存器（包括callee-saved）"
        );
    }

    #[test]
    fn test_call_context() {
        let cc = CallingConvention::standard();
        let mut ctx = CallContext::new(cc, 2, true);

        // 测试参数分配 (ARM64: x0, x1)
        let arg_regs = ctx.get_argument_allocation();
        assert_eq!(arg_regs, vec![REG_X0, REG_X1]);

        // 测试返回值寄存器 (x0)
        assert_eq!(ctx.get_return_register(), Some(REG_X0));

        // 测试caller-save寄存器识别
        // x0, x1 是 caller-saved; x19, x20 是 callee-saved
        ctx.set_registers_to_save(&[REG_X0, REG_X1, REG_X19, REG_X20]);
        assert_eq!(ctx.registers_to_save, vec![REG_X0, REG_X1]);
    }
}
