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

pub const REG_RETURN: PhysicalRegister = 0;
pub const REG_ARG0: PhysicalRegister = 1;
pub const REG_ARG1: PhysicalRegister = 2;
pub const REG_ARG2: PhysicalRegister = 3;
pub const REG_ARG3: PhysicalRegister = 4;
pub const REG_RETURN_ADDRESS: PhysicalRegister = 5;
pub const REG_STACK_POINTER: PhysicalRegister = 6;
pub const REG_FRAME_POINTER: PhysicalRegister = 7;
pub const REG_EFFECT_STACK_POINTER: PhysicalRegister = 12;
pub const REG_EFFECT_PAYLOAD: PhysicalRegister = REG_ARG0;
pub const REG_EFFECT_TAG: PhysicalRegister = 10;
pub const REG_EFFECT_RESUME_TMP: PhysicalRegister = 15;

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
}

impl CallingConvention {
    /// 创建标准的调用约定
    ///
    /// 寄存器分配策略:
    /// - r0: 返回值
    /// - r1-r4: 参数传递 (最多4个参数)
    /// - r5: 返回地址
    /// - r6: 栈指针 (SP)
    /// - r7: 帧指针 (FP)
    ///
    /// Caller-saved: r0-r4 (返回值和参数寄存器)
    /// Callee-saved: r5-r7 (返回地址、SP、FP), r12 (effect栈指针)
    pub fn standard() -> Self {
        let mut caller_saved = HashSet::new();
        caller_saved.insert(REG_RETURN); // 返回值
        caller_saved.insert(REG_ARG0); // 参数1
        caller_saved.insert(REG_ARG1); // 参数2
        caller_saved.insert(REG_ARG2); // 参数3
        caller_saved.insert(REG_ARG3); // 参数4

        let mut callee_saved = HashSet::new();
        callee_saved.insert(REG_RETURN_ADDRESS); // 返回地址 r5
        callee_saved.insert(REG_STACK_POINTER); // 栈指针 r6
        callee_saved.insert(REG_FRAME_POINTER); // 帧指针 r7
        callee_saved.insert(REG_EFFECT_STACK_POINTER); // effect栈指针 r12

        // 添加 r8-r31 作为 callee-saved 寄存器
        // 注意：r11 (X11) 在 AAPCS64 中是临时寄存器，但我们在这里也将其作为 callee-saved
        for reg in 8..32 {
            callee_saved.insert(reg);
        }

        Self {
            argument_registers: vec![REG_ARG0, REG_ARG1, REG_ARG2, REG_ARG3],
            return_register: REG_RETURN,
            caller_saved,
            callee_saved,
            stack_pointer: REG_STACK_POINTER,
            frame_pointer: REG_FRAME_POINTER,
            return_address: REG_RETURN_ADDRESS,
            effect_stack_pointer: REG_EFFECT_STACK_POINTER,
            effect_payload_register: REG_EFFECT_PAYLOAD,
            effect_tag_register: REG_EFFECT_TAG,
            effect_resume_temp: REG_EFFECT_RESUME_TMP,
            temp_registers: {
                // 临时寄存器：除了特殊寄存器外的所有寄存器
                let mut temps = Vec::new();
                // 添加参数寄存器作为临时寄存器
                temps.extend_from_slice(&[REG_RETURN, REG_ARG1, REG_ARG2, REG_ARG3]);
                // 添加 callee-saved 寄存器 r8-r31 作为临时寄存器
                for reg in 8..32 {
                    if reg != REG_EFFECT_STACK_POINTER {
                        temps.push(reg);
                    }
                }
                temps
            }, // 临时寄存器包括参数寄存器和callee-saved寄存器
        }
    }

    /// 检查寄存器是否是 caller-saved
    pub fn is_caller_saved(&self, reg: PhysicalRegister) -> bool {
        self.caller_saved.contains(&reg)
    }

    /// 检查寄存器是否是 callee-saved
    pub fn is_callee_saved(&self, reg: PhysicalRegister) -> bool {
        self.callee_saved.contains(&reg)
    }

    /// 获取函数调用需要保存的寄存器列表
    pub fn get_caller_save_registers(
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
    pub fn get_callee_save_registers(
        &self,
        used_registers: &[PhysicalRegister],
    ) -> Vec<PhysicalRegister> {
        used_registers
            .iter()
            .filter(|&&reg| self.is_callee_saved(reg))
            .copied()
            .collect()
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

    /// 获取 AArch64 callee-saved 寄存器列表
    /// 根据 AAPCS64 标准，X19-X28、X29(FP)、X30(LR) 是 callee-saved
    pub fn get_aarch64_callee_saved(&self) -> Vec<PhysicalRegister> {
        vec![19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30]
    }

    /// 检查物理寄存器是否是 AArch64 callee-saved
    pub fn is_aarch64_callee_saved(&self, reg: PhysicalRegister) -> bool {
        matches!(reg, 19..=30)
    }

    /// 获取 AArch64 caller-saved 寄存器列表
    /// 根据 AAPCS64 标准，X0-X18 是 caller-saved
    pub fn get_aarch64_caller_saved(&self) -> Vec<PhysicalRegister> {
        vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18]
    }

    /// 检查物理寄存器是否是 AArch64 caller-saved
    pub fn is_aarch64_caller_saved(&self, reg: PhysicalRegister) -> bool {
        matches!(reg, 0..=18)
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

        // 测试寄存器分类
        assert!(cc.is_caller_saved(0)); // 返回值
        assert!(cc.is_caller_saved(1)); // 参数1
        assert!(cc.is_callee_saved(6)); // SP
        assert!(cc.is_callee_saved(7)); // FP

        // 测试参数寄存器分配
        let args = cc.get_argument_registers(3);
        assert_eq!(args, vec![1, 2, 3]);

        // 测试可分配寄存器
        let allocatable = cc.get_allocatable_registers();
        // 验证保留寄存器不被分配
        assert!(!allocatable.contains(&REG_RETURN)); // 返回值寄存器保留
        assert!(!allocatable.contains(&REG_STACK_POINTER)); // SP不应该被分配
        assert!(!allocatable.contains(&REG_FRAME_POINTER)); // FP不应该被分配
        assert!(!allocatable.contains(&REG_RETURN_ADDRESS)); // 返回地址保留
        assert!(!allocatable.contains(&REG_EFFECT_STACK_POINTER)); // effect 栈顶不可分配
        assert!(!allocatable.contains(&REG_EFFECT_PAYLOAD)); // effect payload 保留
        // 验证参数寄存器应该可以被分配
        assert!(allocatable.contains(&REG_ARG1));
        assert!(allocatable.contains(&REG_ARG2));
        assert!(allocatable.contains(&REG_ARG3));
        // 验证allocatable_registers应该包含callee-saved寄存器
        assert!(allocatable.len() > 3, "应该包含更多可分配寄存器（包括callee-saved）");
    }

    #[test]
    fn test_call_context() {
        let cc = CallingConvention::standard();
        let mut ctx = CallContext::new(cc, 2, true);

        // 测试参数分配
        let arg_regs = ctx.get_argument_allocation();
        assert_eq!(arg_regs, vec![1, 2]);

        // 测试返回值寄存器
        assert_eq!(ctx.get_return_register(), Some(0));

        // 测试caller-save寄存器识别
        ctx.set_registers_to_save(&[0, 1, 6, 7]);
        assert_eq!(ctx.registers_to_save, vec![0, 1]);
    }
}
