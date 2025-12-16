//! Karte 虚拟机模块
//!
//! 这个模块实现了一个专业的 LIR 虚拟机，包含：
//! - 固定数量的寄存器架构
//! - 专业的寄存器分配算法
//! - 调用约定管理
//! - 栈帧管理
//! - 内存管理
//! - 指令执行引擎

pub mod machine;
pub mod memory;

// 专业模块
pub mod calling_convention;
pub mod professional_executor;
pub mod stack_manager;

pub use calling_convention::*;
pub use machine::*;
pub use memory::*;
pub use professional_executor::*;
pub use stack_manager::*;

/// 创建标准配置的专业虚拟机
pub fn create_professional_vm() -> Result<VirtualMachine, String> {
    let mut vm = VirtualMachine::new();
    let calling_convention = CallingConvention::standard();

    // 初始化特殊寄存器
    vm.set_physical_register(
        calling_convention.stack_pointer,
        (MEMORY_SIZE - STACK_SIZE) as i64,
    )?;
    vm.set_physical_register(
        calling_convention.frame_pointer,
        (MEMORY_SIZE - STACK_SIZE) as i64,
    )?;
    vm.set_physical_register(calling_convention.return_address, 0)?;

    Ok(vm)
}

/// 专业虚拟机管理器
#[derive(Debug)]
pub struct ProfessionalVMManager {
    /// 专业执行器
    pub executor: ProfessionalExecutor,
}

/// 兼容性虚拟机管理器（保留原有接口）
#[derive(Debug)]
pub struct CompatibilityVMManager {
    /// 虚拟机实例
    pub vm: VirtualMachine,
    /// 调用约定
    pub calling_convention: CallingConvention,
    /// 栈管理器
    pub stack_manager: StackManager,
}

impl ProfessionalVMManager {
    /// 创建新的专业虚拟机管理器
    pub fn new(debug_mode: bool) -> Result<Self, String> {
        Ok(Self {
            executor: ProfessionalExecutor::new(debug_mode)?,
        })
    }

    /// 执行 LIR 程序
    pub fn execute_program(&mut self, program: &karte_lir::LirProgram) -> Result<i64, String> {
        self.executor.execute(program)
    }

    /// 获取虚拟机状态
    pub fn get_vm(&self) -> &VirtualMachine {
        self.executor.get_vm()
    }

    /// 获取栈管理器
    pub fn get_stack_manager(&self) -> &StackManager {
        self.executor.get_stack_manager()
    }

    /// 获取内存管理器
    pub fn get_memory(&self) -> &MemoryManager {
        self.executor.get_memory()
    }
}

impl CompatibilityVMManager {
    /// 创建新的兼容性虚拟机管理器
    pub fn new() -> Result<Self, String> {
        let calling_convention = CallingConvention::standard();
        let stack_base = (MEMORY_SIZE - STACK_SIZE) as i64;

        Ok(Self {
            vm: create_professional_vm()?,
            calling_convention: calling_convention.clone(),
            stack_manager: StackManager::new(calling_convention.clone(), stack_base),
        })
    }

    /// 重置虚拟机状态
    pub fn reset(&mut self) -> Result<(), String> {
        self.vm.reset();
        let stack_base = (MEMORY_SIZE - STACK_SIZE) as i64;
        self.stack_manager = StackManager::new(self.calling_convention.clone(), stack_base);

        // 重新初始化特殊寄存器
        self.vm
            .set_physical_register(self.calling_convention.stack_pointer, stack_base)?;
        self.vm
            .set_physical_register(self.calling_convention.frame_pointer, stack_base)?;
        self.vm
            .set_physical_register(self.calling_convention.return_address, 0)?;

        Ok(())
    }
}

impl Default for ProfessionalVMManager {
    fn default() -> Self {
        Self::new(false).expect("Failed to create professional VM manager")
    }
}

impl Default for CompatibilityVMManager {
    fn default() -> Self {
        Self::new().expect("Failed to create compatibility VM manager")
    }
}

/// 虚拟机配置常量
/// 通用寄存器数量 - ARM64 ABI 使用 32 个通用寄存器 (x0-x30 + sp)
pub const NUM_REGISTERS: usize = 32;
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
