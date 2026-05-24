//! Karte 虚拟机调用约定
//!
//! 定义了函数调用时的寄存器使用规范，遵循现代编译器的最佳实践。
//! 支持多目标架构：AArch64 (AAPCS64) 和 x86_64 (System V ABI)。

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

// ============================================================================
// 架构相关的寄存器定义
// ============================================================================

#[cfg(target_arch = "aarch64")]
mod arch_regs {
    use super::PhysicalRegister;

    // ARM64 AAPCS64 寄存器命名约定
    pub const REG_X0: PhysicalRegister = 0;
    pub const REG_X1: PhysicalRegister = 1;
    pub const REG_X2: PhysicalRegister = 2;
    pub const REG_X3: PhysicalRegister = 3;
    pub const REG_X4: PhysicalRegister = 4;
    pub const REG_X5: PhysicalRegister = 5;
    pub const REG_X6: PhysicalRegister = 6;
    pub const REG_X7: PhysicalRegister = 7;
    pub const REG_X8: PhysicalRegister = 8;
    pub const REG_X9: PhysicalRegister = 9;
    pub const REG_X10: PhysicalRegister = 10;
    pub const REG_X11: PhysicalRegister = 11;
    pub const REG_X12: PhysicalRegister = 12;
    pub const REG_X13: PhysicalRegister = 13;
    pub const REG_X14: PhysicalRegister = 14;
    pub const REG_X15: PhysicalRegister = 15;
    pub const REG_X16: PhysicalRegister = 16;
    pub const REG_X17: PhysicalRegister = 17;
    pub const REG_X18: PhysicalRegister = 18;
    pub const REG_X19: PhysicalRegister = 19;
    pub const REG_X20: PhysicalRegister = 20;
    pub const REG_X21: PhysicalRegister = 21;
    pub const REG_X22: PhysicalRegister = 22;
    pub const REG_X23: PhysicalRegister = 23;
    pub const REG_X24: PhysicalRegister = 24;
    pub const REG_X25: PhysicalRegister = 25;
    pub const REG_X26: PhysicalRegister = 26;
    pub const REG_X27: PhysicalRegister = 27;
    pub const REG_X28: PhysicalRegister = 28;
    pub const REG_X29: PhysicalRegister = 29;
    pub const REG_X30: PhysicalRegister = 30;
    pub const REG_SP: PhysicalRegister = 31;

    pub const TOTAL_REGISTERS: usize = 32;

    pub const REG_RETURN: PhysicalRegister = REG_X0;
    pub const REG_ARG0: PhysicalRegister = REG_X0;
    pub const REG_ARG1: PhysicalRegister = REG_X1;
    pub const REG_ARG2: PhysicalRegister = REG_X2;
    pub const REG_ARG3: PhysicalRegister = REG_X3;
    pub const REG_RETURN_ADDRESS: PhysicalRegister = REG_X30;
    pub const REG_STACK_POINTER: PhysicalRegister = REG_SP;
    pub const REG_FRAME_POINTER: PhysicalRegister = REG_X29;
    pub const REG_EFFECT_STACK_POINTER: PhysicalRegister = REG_X12;
    pub const REG_EFFECT_PAYLOAD: PhysicalRegister = REG_X0;
    pub const REG_EFFECT_TAG: PhysicalRegister = REG_X10;
    pub const REG_EFFECT_RESUME_TMP: PhysicalRegister = REG_X15;
}

#[cfg(target_arch = "x86_64")]
mod arch_regs {
    use super::PhysicalRegister;

    // x86-64 System V ABI 寄存器编号（与 x86_compiler.rs 中的 X86Register 枚举一致）
    // 编号方式：RAX=0, RCX=1, RDX=2, RBX=3, RSP=4, RBP=5, RSI=6, RDI=7, R8-R15=8-15
    //
    // 但为了与 Karte 虚拟栈架构兼容，我们使用一种映射策略：
    // Karte 虚拟机使用自己的寄存器编号，x86_compiler 负责映射到实际硬件寄存器。
    //
    // 为了最小化改动并保持与 AArch64 的兼容性，我们使用与 AAPCS64 相同的编号体系，
    // 但限制可分配寄存器数量为 x86_64 实际可用的 16 个。
    //
    // 寄存器编号映射（Karte 虚拟编号 → x86_64 硬件寄存器）：
    // 0  → RAX  (返回值)
    // 1  → RCX  (参数4 / 临时)
    // 2  → RDX  (参数3 / 临时)
    // 3  → RBX  (callee-saved)
    // 4  → RSP  (栈指针)
    // 5  → RBP  (帧指针)
    // 6  → RSI  (参数2)
    // 7  → RDI  (参数1)
    // 8  → R8   (参数5)
    // 9  → R9   (参数6)
    // 10 → R10  (临时)
    // 11 → R11  (临时)
    // 12 → R12  (callee-saved) → effect 栈指针
    // 13 → R13  (callee-saved)
    // 14 → R14  (callee-saved)
    // 15 → R15  (callee-saved)
    //
    // 注意：x86_64 的参数传递使用 RDI, RSI, RDX, RCX, R8, R9 (System V ABI)
    // 而 Karte 的虚拟参数寄存器按顺序 0, 1, 2, 3, 4, 5 映射
    // 所以 argument_registers 使用 [7, 6, 2, 1, 8, 9] (RDI, RSI, RDX, RCX, R8, R9)

    pub const REG_RAX: PhysicalRegister = 0;
    pub const REG_RCX: PhysicalRegister = 1;
    pub const REG_RDX: PhysicalRegister = 2;
    pub const REG_RBX: PhysicalRegister = 3;
    pub const REG_RSP: PhysicalRegister = 4;
    pub const REG_RBP: PhysicalRegister = 5;
    pub const REG_RSI: PhysicalRegister = 6;
    pub const REG_RDI: PhysicalRegister = 7;
    pub const REG_R8: PhysicalRegister = 8;
    pub const REG_R9: PhysicalRegister = 9;
    pub const REG_R10: PhysicalRegister = 10;
    pub const REG_R11: PhysicalRegister = 11;
    pub const REG_R12: PhysicalRegister = 12;
    pub const REG_R13: PhysicalRegister = 13;
    pub const REG_R14: PhysicalRegister = 14;
    pub const REG_R15: PhysicalRegister = 15;

    pub const TOTAL_REGISTERS: usize = 16;

    // 语义化别名 — 与 AArch64 版本保持接口一致
    // 返回值寄存器
    pub const REG_RETURN: PhysicalRegister = REG_RAX;
    // 参数寄存器（按 System V ABI 顺序）
    pub const REG_ARG0: PhysicalRegister = REG_RDI; // 参数1
    pub const REG_ARG1: PhysicalRegister = REG_RSI; // 参数2
    pub const REG_ARG2: PhysicalRegister = REG_RDX; // 参数3
    pub const REG_ARG3: PhysicalRegister = REG_RCX; // 参数4
    // 返回地址：x86 用栈存，这里用一个 callee-saved 寄存器暂存
    pub const REG_RETURN_ADDRESS: PhysicalRegister = REG_R10; // 临时寄存器
    // 栈指针
    pub const REG_STACK_POINTER: PhysicalRegister = REG_RSP;
    // 帧指针
    pub const REG_FRAME_POINTER: PhysicalRegister = REG_RBP;
    // Effect 栈指针：使用 callee-saved 寄存器 R12
    pub const REG_EFFECT_STACK_POINTER: PhysicalRegister = REG_R12;
    // Effect payload：使用 RAX
    pub const REG_EFFECT_PAYLOAD: PhysicalRegister = REG_RAX;
    // Effect tag：使用 R10
    pub const REG_EFFECT_TAG: PhysicalRegister = REG_R10;
    // Effect resume 临时寄存器：使用 R11
    pub const REG_EFFECT_RESUME_TMP: PhysicalRegister = REG_R11;

    // 兼容别名（保持与 AArch64 代码的兼容性）
    pub const REG_X0: PhysicalRegister = REG_RAX;
    pub const REG_X1: PhysicalRegister = REG_RCX;
    pub const REG_X2: PhysicalRegister = REG_RDX;
    pub const REG_X3: PhysicalRegister = REG_RBX;
    pub const REG_X4: PhysicalRegister = REG_RSP;
    pub const REG_X5: PhysicalRegister = REG_RBP;
    pub const REG_X6: PhysicalRegister = REG_RSI;
    pub const REG_X7: PhysicalRegister = REG_RDI;
    pub const REG_X8: PhysicalRegister = REG_R8;
    pub const REG_X9: PhysicalRegister = REG_R9;
    pub const REG_X10: PhysicalRegister = REG_R10;
    pub const REG_X11: PhysicalRegister = REG_R11;
    pub const REG_X12: PhysicalRegister = REG_R12;
    pub const REG_X13: PhysicalRegister = REG_R13;
    pub const REG_X14: PhysicalRegister = REG_R14;
    pub const REG_X15: PhysicalRegister = REG_R15;
    pub const REG_X16: PhysicalRegister = 16; // 不存在，保留编号
    pub const REG_X17: PhysicalRegister = 17;
    pub const REG_X18: PhysicalRegister = 18;
    pub const REG_X19: PhysicalRegister = 19;
    pub const REG_X20: PhysicalRegister = 20;
    pub const REG_X21: PhysicalRegister = 21;
    pub const REG_X22: PhysicalRegister = 22;
    pub const REG_X23: PhysicalRegister = 23;
    pub const REG_X24: PhysicalRegister = 24;
    pub const REG_X25: PhysicalRegister = 25;
    pub const REG_X26: PhysicalRegister = 26;
    pub const REG_X27: PhysicalRegister = 27;
    pub const REG_X28: PhysicalRegister = 28;
    pub const REG_X29: PhysicalRegister = REG_RBP; // 帧指针 (兼容别名)
    pub const REG_X30: PhysicalRegister = 30; // LR 不存在，保留编号
    pub const REG_SP: PhysicalRegister = REG_RSP;
}

// 导出架构相关的常量
#[cfg(target_arch = "aarch64")]
pub use arch_regs::*;
#[cfg(target_arch = "x86_64")]
pub use arch_regs::*;

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
    fn is_caller_saved(&self, reg: PhysicalRegister) -> bool {
        self.caller_saved.contains(&reg)
    }

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
    /// 创建当前目标架构的标准调用约定
    #[cfg(target_arch = "aarch64")]
    pub fn standard() -> Self {
        let mut caller_saved = HashSet::new();
        // x0-x18 都是 caller-saved (易失寄存器)
        for reg in 0..=18u8 {
            caller_saved.insert(reg);
        }

        let mut callee_saved = HashSet::new();
        // x19-x31 是 callee-saved (非易失寄存器)
        for reg in 19..=31u8 {
            callee_saved.insert(reg);
        }

        Self {
            argument_registers: vec![
                REG_X0, REG_X1, REG_X2, REG_X3, REG_X4, REG_X5, REG_X6, REG_X7,
            ],
            return_register: REG_X0,
            caller_saved,
            callee_saved,
            stack_pointer: REG_SP,
            frame_pointer: REG_X29,
            return_address: REG_X30,
            effect_stack_pointer: REG_X12,
            effect_payload_register: REG_X0,
            effect_tag_register: REG_X10,
            effect_resume_temp: REG_X15,
            temp_registers: {
                let mut temps = Vec::new();
                temps.extend_from_slice(&[
                    REG_X0, REG_X1, REG_X2, REG_X3, REG_X4, REG_X5, REG_X6, REG_X7,
                ]);
                temps.extend_from_slice(&[
                    REG_X8, REG_X9, REG_X10, REG_X11, REG_X12, REG_X13, REG_X14, REG_X15,
                ]);
                temps.extend_from_slice(&[REG_X16, REG_X17, REG_X18]);
                for reg in 19..=28u8 {
                    temps.push(reg);
                }
                temps
            },
            stack_alignment: 16,
            use_system_stack_pointer: true,
        }
    }

    /// 创建当前目标架构的标准调用约定 (x86-64 System V ABI)
    #[cfg(target_arch = "x86_64")]
    pub fn standard() -> Self {
        // x86-64 System V ABI:
        // Caller-saved (易失): RAX, RCX, RDX, RSI, RDI, R8, R9, R10, R11
        // Callee-saved (非易失): RBX, RSP, RBP, R12, R13, R14, R15
        // 参数传递: RDI, RSI, RDX, RCX, R8, R9
        // 返回值: RAX

        let caller_saved: HashSet<PhysicalRegister> = [
            REG_RAX,  // 0 - 返回值
            REG_RCX,  // 1 - 参数4
            REG_RDX,  // 2 - 参数3
            REG_RSI,  // 6 - 参数2
            REG_RDI,  // 7 - 参数1
            REG_R8,   // 8 - 参数5
            REG_R9,   // 9 - 参数6
            REG_R10,  // 10 - 临时
            REG_R11,  // 11 - 临时
        ].into_iter().collect();

        let callee_saved: HashSet<PhysicalRegister> = [
            REG_RBX,  // 3 - callee-saved
            REG_RSP,  // 4 - 栈指针
            REG_RBP,  // 5 - 帧指针
            REG_R12,  // 12 - callee-saved (effect 栈指针)
            REG_R13,  // 13 - callee-saved
            REG_R14,  // 14 - callee-saved
            REG_R15,  // 15 - callee-saved
        ].into_iter().collect();

        Self {
            // System V ABI: RDI, RSI, RDX, RCX, R8, R9
            argument_registers: vec![
                REG_RDI, // 参数1 (编号7)
                REG_RSI, // 参数2 (编号6)
                REG_RDX, // 参数3 (编号2)
                REG_RCX, // 参数4 (编号1)
                REG_R8,  // 参数5 (编号8)
                REG_R9,  // 参数6 (编号9)
            ],
            return_register: REG_RAX,
            caller_saved,
            callee_saved,
            stack_pointer: REG_RSP,
            frame_pointer: REG_RBP,
            // x86 用栈存返回地址，但 Karte 虚拟机用寄存器存
            // 用 R10 作为虚拟返回地址寄存器
            return_address: REG_R10,
            effect_stack_pointer: REG_R12,
            effect_payload_register: REG_RAX,
            effect_tag_register: REG_R10,
            effect_resume_temp: REG_R11,
            temp_registers: {
                let mut temps = Vec::new();
                // 参数寄存器
                temps.extend_from_slice(&[
                    REG_RDI, REG_RSI, REG_RDX, REG_RCX, REG_R8, REG_R9,
                ]);
                // 临时寄存器
                temps.extend_from_slice(&[REG_R10, REG_R11]);
                // 返回值寄存器也可用作临时
                temps.push(REG_RAX);
                // callee-saved 也可临时使用（需要保存/恢复）
                temps.extend_from_slice(&[REG_RBX, REG_R13, REG_R14, REG_R15]);
                temps
            },
            stack_alignment: 16,
            use_system_stack_pointer: true,
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
        (0..TOTAL_REGISTERS as u8)
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

        // 验证特殊寄存器不被分配
        let allocatable = cc.get_allocatable_registers();
        assert!(!allocatable.contains(&cc.return_register), "返回值寄存器不应被分配");
        assert!(!allocatable.contains(&cc.stack_pointer), "栈指针不应被分配");
        assert!(!allocatable.contains(&cc.frame_pointer), "帧指针不应被分配");
        assert!(!allocatable.contains(&cc.return_address), "返回地址寄存器不应被分配");
        assert!(!allocatable.contains(&cc.effect_stack_pointer), "effect 栈指针不应被分配");
        assert!(!allocatable.contains(&cc.effect_payload_register), "effect payload 不应被分配");

        // 验证有足够的可分配寄存器
        assert!(allocatable.len() > 3, "应该包含更多可分配寄存器");
    }

    #[test]
    fn test_call_context() {
        let cc = CallingConvention::standard();
        let mut ctx = CallContext::new(cc.clone(), 2, true);

        // 验证参数分配
        let arg_regs = ctx.get_argument_allocation();
        assert_eq!(arg_regs.len(), 2);

        // 验证返回值寄存器
        assert!(ctx.get_return_register().is_some());
    }

    #[test]
    fn test_caller_callee_classification() {
        let cc = CallingConvention::standard();

        // 返回值寄存器应该是 caller-saved
        assert!(cc.is_caller_saved(cc.return_register));

        // 栈指针应该是 callee-saved
        assert!(cc.is_callee_saved(cc.stack_pointer));

        // 帧指针应该是 callee-saved
        assert!(cc.is_callee_saved(cc.frame_pointer));
    }
}
