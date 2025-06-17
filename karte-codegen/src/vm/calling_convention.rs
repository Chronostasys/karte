//! Karte 虚拟机调用约定
//! 
//! 定义了函数调用时的寄存器使用规范，遵循现代编译器的最佳实践。
//! 采用类似 System V ABI 的约定，适合 RISC 架构。

use karte_lir::RegisterId;
use std::collections::HashSet;

/// 物理寄存器编号
pub type PhysicalRegister = u8;

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
    /// Callee-saved: r5-r7 (返回地址、SP、FP)
    pub fn standard() -> Self {
        let mut caller_saved = HashSet::new();
        caller_saved.insert(0); // 返回值
        caller_saved.insert(1); // 参数1
        caller_saved.insert(2); // 参数2
        caller_saved.insert(3); // 参数3
        caller_saved.insert(4); // 参数4

        let mut callee_saved = HashSet::new();
        callee_saved.insert(5); // 返回地址
        callee_saved.insert(6); // 栈指针
        callee_saved.insert(7); // 帧指针

        Self {
            argument_registers: vec![1, 2, 3, 4],
            return_register: 0,
            caller_saved,
            callee_saved,
            stack_pointer: 6,
            frame_pointer: 7,
            return_address: 5,
            temp_registers: vec![0, 1, 2, 3, 4], // 临时寄存器可重用参数和返回值寄存器
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
    pub fn get_caller_save_registers(&self, live_registers: &[PhysicalRegister]) -> Vec<PhysicalRegister> {
        live_registers
            .iter()
            .filter(|&&reg| self.is_caller_saved(reg))
            .copied()
            .collect()
    }

    /// 获取函数需要保存的callee-saved寄存器
    pub fn get_callee_save_registers(&self, used_registers: &[PhysicalRegister]) -> Vec<PhysicalRegister> {
        used_registers
            .iter()
            .filter(|&&reg| self.is_callee_saved(reg))
            .copied()
            .collect()
    }

    /// 获取指定数量的参数寄存器
    pub fn get_argument_registers(&self, count: usize) -> Vec<PhysicalRegister> {
        self.argument_registers.iter().take(count).copied().collect()
    }

    /// 检查是否是特殊寄存器（SP、FP、RA）
    pub fn is_special_register(&self, reg: PhysicalRegister) -> bool {
        reg == self.stack_pointer || reg == self.frame_pointer || reg == self.return_address
    }

    /// 获取可用于寄存器分配的通用寄存器
    pub fn get_allocatable_registers(&self) -> Vec<PhysicalRegister> {
        // 排除特殊寄存器，只返回可分配的寄存器
        (0..8u8)
            .filter(|&reg| !self.is_special_register(reg))
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
        assert!(!allocatable.contains(&6)); // SP不应该被分配
        assert!(!allocatable.contains(&7)); // FP不应该被分配
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