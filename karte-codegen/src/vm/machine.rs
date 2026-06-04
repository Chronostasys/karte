//! 虚拟机核心状态管理
//!
//! 定义了虚拟机的核心状态，包括寄存器文件、内存、标志位等

use super::{MEMORY_SIZE, NUM_REGISTERS, STACK_SIZE};
use karte_lir::Register;
use log::info;
use std::collections::HashMap;

/// 比较结果标志
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComparisonFlags {
    Equal,
    Greater,
    Less,
}

/// 虚拟机状态
#[derive(Debug, Clone)]
pub struct VirtualMachine {
    /// 通用寄存器文件 (r0-r31)
    pub registers: [i64; NUM_REGISTERS],
    /// 程序计数器
    pub pc: usize,
    /// 栈指针
    pub sp: usize,
    /// 比较标志
    pub flags: ComparisonFlags,
    /// 内存空间
    pub memory: Vec<i64>,
    /// 调用栈
    pub call_stack: Vec<usize>,
    /// 虚拟寄存器到物理寄存器的映射
    pub register_mapping: HashMap<Register, u8>,
}

impl VirtualMachine {
    /// 创建新的虚拟机实例
    pub fn new() -> Self {
        Self {
            registers: [0; NUM_REGISTERS],
            pc: 0,
            sp: STACK_SIZE - 1,
            flags: ComparisonFlags::Equal,
            memory: vec![0; MEMORY_SIZE],
            call_stack: Vec::new(),
            register_mapping: HashMap::new(),
        }
    }

    /// 获取物理寄存器的值
    pub fn get_physical_register(&self, reg_id: u8) -> crate::Result<i64> {
        if (reg_id as usize) < NUM_REGISTERS {
            Ok(self.registers[reg_id as usize])
        } else {
            Err(format!("Invalid physical register: r{}", reg_id).into())
        }
    }

    /// 设置物理寄存器的值
    pub fn set_physical_register(&mut self, reg_id: u8, value: i64) -> crate::Result<()> {
        if (reg_id as usize) < NUM_REGISTERS {
            self.registers[reg_id as usize] = value;
            Ok(())
        } else {
            Err(format!("Invalid physical register: r{}", reg_id).into())
        }
    }

    /// 获取虚拟寄存器的值（支持物理寄存器映射和溢出处理）
    pub fn get_virtual_register(&self, reg_id: &Register) -> crate::Result<i64> {
        if let Some(&physical_reg) = self.register_mapping.get(reg_id) {
            self.get_physical_register(physical_reg)
        } else {
            // 寄存器未映射 - 这应该在寄存器分配阶段被处理
            Err(format!(
                "Unmapped virtual register: {:?} - register allocation should handle spilling",
                reg_id
            ).into())
        }
    }

    /// 设置虚拟寄存器的值（支持物理寄存器映射和溢出处理）
    pub fn set_virtual_register(&mut self, reg_id: &Register, value: i64) -> crate::Result<()> {
        if let Some(&physical_reg) = self.register_mapping.get(reg_id) {
            self.set_physical_register(physical_reg, value)
        } else {
            // 寄存器未映射 - 这应该在寄存器分配阶段被处理
            Err(format!(
                "Unmapped virtual register: {:?} - register allocation should handle spilling",
                reg_id
            ).into())
        }
    }

    /// 推入调用栈
    pub fn push_call_stack(&mut self, return_address: usize) {
        self.call_stack.push(return_address);
    }

    /// 弹出调用栈
    pub fn pop_call_stack(&mut self) -> Option<usize> {
        self.call_stack.pop()
    }

    /// 比较两个值并设置标志
    pub fn compare(&mut self, v1: i64, v2: i64) {
        use std::cmp::Ordering;
        self.flags = match v1.cmp(&v2) {
            Ordering::Equal => ComparisonFlags::Equal,
            Ordering::Greater => ComparisonFlags::Greater,
            Ordering::Less => ComparisonFlags::Less,
        };
    }

    /// 检查条件是否满足
    pub fn check_condition(&self, condition: JumpCondition) -> bool {
        match condition {
            JumpCondition::Always => true,
            JumpCondition::Equal => self.flags == ComparisonFlags::Equal,
            JumpCondition::NotEqual => self.flags != ComparisonFlags::Equal,
            JumpCondition::Greater => self.flags == ComparisonFlags::Greater,
            JumpCondition::GreaterEqual => self.flags != ComparisonFlags::Less,
            JumpCondition::Less => self.flags == ComparisonFlags::Less,
            JumpCondition::LessEqual => self.flags != ComparisonFlags::Greater,
        }
    }

    /// 重置虚拟机状态
    pub fn reset(&mut self) {
        self.registers = [0; NUM_REGISTERS];
        self.pc = 0;
        self.sp = STACK_SIZE - 1;
        self.flags = ComparisonFlags::Equal;
        self.call_stack.clear();
        self.register_mapping.clear();
    }

    /// 获取可用的物理寄存器数量
    pub fn available_registers(&self) -> usize {
        NUM_REGISTERS - self.register_mapping.len()
    }

    /// 打印虚拟机状态（用于调试）
    pub fn print_state(&self) {
        info!("=== Virtual Machine State ===");
        info!("PC: {}", self.pc);
        info!("SP: {}", self.sp);
        info!("Flags: {:?}", self.flags);
        info!("Call Stack: {:?}", self.call_stack);

        // 只打印非零寄存器
        info!("Non-zero Registers:");
        for (i, &value) in self.registers.iter().enumerate() {
            if value != 0 {
                info!("  r{}: {}", i, value);
            }
        }

        // 打印寄存器映射
        if !self.register_mapping.is_empty() {
            info!("Register Mapping:");
            for (virtual_reg, &physical_reg) in &self.register_mapping {
                info!("  {:?} -> r{}", virtual_reg, physical_reg);
            }
        }

        // 溢出寄存器现在应该通过栈访问，不再单独存储

        // 打印虚拟机内存中的非零值（仅前100个位置）
        info!("Non-zero VM memory values (first 100):");
        for (i, &value) in self.memory.iter().enumerate().take(100) {
            if value != 0 {
                info!("  vm_memory[{}]: {}", i, value);
            }
        }

        // 打印高地址内存中的非零值（栈区域）
        info!("Non-zero VM memory values (stack area 1048400-1048576):");
        for (i, &value) in self.memory.iter().enumerate().skip(1048400) {
            if value != 0 {
                info!("  vm_memory[{}]: {}", i, value);
            }
        }
    }
}

/// 跳转条件
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JumpCondition {
    Always,
    Equal,
    NotEqual,
    Greater,
    GreaterEqual,
    Less,
    LessEqual,
}

impl Default for VirtualMachine {
    fn default() -> Self {
        Self::new()
    }
}
