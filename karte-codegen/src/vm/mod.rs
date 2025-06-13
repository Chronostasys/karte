//! Karte 虚拟机模块
//! 
//! 这个模块实现了一个专业的 LIR 虚拟机，包含：
//! - 固定数量的寄存器架构
//! - 寄存器分配算法
//! - 内存管理
//! - 指令执行引擎

pub mod machine;
pub mod register_allocator;
pub mod memory;
pub mod executor;

pub use machine::*;
pub use register_allocator::*;
pub use memory::*;
pub use executor::*;

/// 虚拟机配置常量
/// 通用寄存器数量 - 为了更好地测试寄存器分配算法，减少到8个
pub const NUM_REGISTERS: usize = 8;
/// 内存大小 (1MB)
pub const MEMORY_SIZE: usize = 1024 * 1024;
/// 栈大小
pub const STACK_SIZE: usize = 1024;

/// 寄存器类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterType {
    /// 通用寄存器 (r0-r31)
    General(u8),
    /// 程序计数器
    ProgramCounter,
    /// 栈指针
    StackPointer,
    /// 比较结果标志
    Flags,
}

impl RegisterType {
    pub fn is_general(&self) -> bool {
        matches!(self, RegisterType::General(_))
    }
    
    pub fn get_index(&self) -> Option<usize> {
        match self {
            RegisterType::General(idx) => Some(*idx as usize),
            _ => None,
        }
    }
} 