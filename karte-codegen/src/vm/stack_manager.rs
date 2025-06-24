//! Karte 虚拟机栈管理
//!
//! 实现专业的栈帧管理，包括：
//! - 栈帧的创建和销毁
//! - 局部变量分配
//! - 参数传递
//! - 返回地址管理

use super::calling_convention::{CallingConvention, PhysicalRegister};
use karte_lir::Register;
use std::collections::HashMap;

/// 栈帧布局
#[derive(Debug, Clone)]
pub struct StackFrame {
    /// 栈帧基址偏移 (相对于帧指针)
    pub base_offset: i64,
    /// 局部变量区大小
    pub locals_size: usize,
    /// 保存的寄存器区大小
    pub saved_registers_size: usize,
    /// 参数区大小 (用于传递给其他函数的参数)
    pub outgoing_args_size: usize,
    /// 总栈帧大小
    pub total_size: usize,
    /// 保存的寄存器映射 (寄存器 -> 栈偏移)
    pub saved_registers: HashMap<PhysicalRegister, i64>,
    /// 局部变量映射 (变量ID -> 栈偏移)
    pub local_variables: HashMap<Register, i64>,
}

impl Default for StackFrame {
    fn default() -> Self {
        Self::new()
    }
}

impl StackFrame {
    /// 创建新的栈帧
    pub fn new() -> Self {
        Self {
            base_offset: 0,
            locals_size: 0,
            saved_registers_size: 0,
            outgoing_args_size: 0,
            total_size: 0,
            saved_registers: HashMap::new(),
            local_variables: HashMap::new(),
        }
    }

    /// 分配局部变量存储空间
    pub fn allocate_local(&mut self, var_id: Register, size: usize, alignment: usize) -> i64 {
        // 对齐当前偏移量
        let aligned_offset = align_up(self.locals_size, alignment);
        let offset = -(aligned_offset as i64 + size as i64);

        // 记录变量位置
        self.local_variables.insert(var_id, offset);

        // 更新局部变量区大小
        self.locals_size = aligned_offset + size;
        self.recalculate_total_size();

        offset
    }

    /// 分配保存寄存器的存储空间
    pub fn allocate_saved_register(&mut self, reg: PhysicalRegister) -> i64 {
        let offset = -(self.locals_size as i64 + self.saved_registers_size as i64 + 8);
        self.saved_registers.insert(reg, offset);
        self.saved_registers_size += 8;
        self.recalculate_total_size();
        offset
    }

    /// 设置传出参数区大小
    pub fn set_outgoing_args_size(&mut self, size: usize) {
        self.outgoing_args_size = size;
        self.recalculate_total_size();
    }

    /// 获取局部变量的栈偏移
    pub fn get_local_offset(&self, var_id: &Register) -> Option<i64> {
        self.local_variables.get(var_id).copied()
    }

    /// 获取保存寄存器的栈偏移
    pub fn get_saved_register_offset(&self, reg: PhysicalRegister) -> Option<i64> {
        self.saved_registers.get(&reg).copied()
    }

    /// 重新计算总栈帧大小
    fn recalculate_total_size(&mut self) {
        self.total_size = self.locals_size + self.saved_registers_size + self.outgoing_args_size;
        // 确保栈帧大小是16字节对齐的（现代架构的要求）
        self.total_size = align_up(self.total_size, 16);
    }
}

/// 栈管理器
#[derive(Debug)]
pub struct StackManager {
    /// 当前栈指针值
    pub stack_pointer: i64,
    /// 当前帧指针值
    pub frame_pointer: i64,
    /// 栈帧栈（用于函数调用嵌套）
    pub frame_stack: Vec<StackFrame>,
    /// 调用约定
    pub calling_convention: CallingConvention,
}

impl StackManager {
    /// 创建新的栈管理器
    pub fn new(calling_convention: CallingConvention, stack_base: i64) -> Self {
        Self {
            stack_pointer: stack_base,
            frame_pointer: stack_base,
            frame_stack: Vec::new(),
            calling_convention,
        }
    }

    /// 创建新的栈帧（函数入口）
    pub fn enter_function(
        &mut self,
        saved_registers: &[PhysicalRegister],
        local_var_size: usize,
        max_outgoing_args: usize,
    ) -> StackFrame {
        let mut frame = StackFrame::new();

        // 设置传出参数区大小
        frame.set_outgoing_args_size(max_outgoing_args * 8); // 假设每个参数8字节

        // 分配保存寄存器的空间
        for &reg in saved_registers {
            frame.allocate_saved_register(reg);
        }

        // 预留局部变量空间
        if local_var_size > 0 {
            frame.locals_size = local_var_size;
            frame.recalculate_total_size();
        }

        // 更新栈指针
        self.stack_pointer -= frame.total_size as i64;
        frame.base_offset = self.frame_pointer;

        // 保存当前栈帧
        self.frame_stack.push(frame.clone());

        // 更新帧指针
        self.frame_pointer = self.stack_pointer + frame.total_size as i64;

        frame
    }

    /// 离开当前栈帧（函数出口）
    pub fn leave_function(&mut self) -> Option<StackFrame> {
        if let Some(frame) = self.frame_stack.pop() {
            // 恢复栈指针
            self.stack_pointer += frame.total_size as i64;

            // 恢复帧指针
            if let Some(parent_frame) = self.frame_stack.last() {
                self.frame_pointer = parent_frame.base_offset;
            } else {
                // 回到栈基址
                self.frame_pointer = self.stack_pointer;
            }

            Some(frame)
        } else {
            None
        }
    }

    /// 获取当前栈帧
    pub fn current_frame(&self) -> Option<&StackFrame> {
        self.frame_stack.last()
    }

    /// 获取当前栈帧（可变引用）
    pub fn current_frame_mut(&mut self) -> Option<&mut StackFrame> {
        self.frame_stack.last_mut()
    }

    /// 计算相对于帧指针的地址
    pub fn get_frame_relative_address(&self, offset: i64) -> i64 {
        self.frame_pointer + offset
    }

    /// 计算相对于栈指针的地址
    pub fn get_stack_relative_address(&self, offset: i64) -> i64 {
        self.stack_pointer + offset
    }

    /// 推入值到栈
    pub fn push_value(&mut self, size: usize) -> i64 {
        self.stack_pointer -= size as i64;
        self.stack_pointer
    }

    /// 从栈弹出值
    pub fn pop_value(&mut self, size: usize) -> i64 {
        let old_sp = self.stack_pointer;
        self.stack_pointer += size as i64;
        old_sp
    }

    /// 检查栈是否为空
    pub fn is_empty(&self) -> bool {
        self.frame_stack.is_empty()
    }

    /// 获取栈深度
    pub fn depth(&self) -> usize {
        self.frame_stack.len()
    }

    /// 打印栈管理器状态
    pub fn print_state(&self) {
        println!("=== Stack Manager State ===");
        println!("Stack Pointer: {}", self.stack_pointer);
        println!("Frame Pointer: {}", self.frame_pointer);
        println!("Stack Depth: {}", self.depth());
        if let Some(current_frame) = self.current_frame() {
            println!("Current Frame:");
            println!("  Locals Size: {}", current_frame.locals_size);
            println!("  Saved Registers: {}", current_frame.saved_registers.len());
            println!("  Outgoing Args Size: {}", current_frame.outgoing_args_size);
        }
        println!();
    }
}

/// 栈操作指令
#[derive(Debug, Clone, PartialEq)]
pub enum StackOperation {
    /// 推入寄存器值到栈
    Push { register: PhysicalRegister },
    /// 从栈弹出值到寄存器
    Pop { register: PhysicalRegister },
    /// 分配栈空间
    AllocateSpace { size: usize },
    /// 释放栈空间
    DeallocateSpace { size: usize },
    /// 加载栈相对地址的值
    LoadFromStack { dst: PhysicalRegister, offset: i64 },
    /// 存储值到栈相对地址
    StoreToStack { src: PhysicalRegister, offset: i64 },
    /// 加载帧相对地址的值
    LoadFromFrame { dst: PhysicalRegister, offset: i64 },
    /// 存储值到帧相对地址
    StoreToFrame { src: PhysicalRegister, offset: i64 },
}

/// 对齐函数
fn align_up(value: usize, alignment: usize) -> usize {
    (value + alignment - 1) & !(alignment - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stack_frame_allocation() {
        let mut frame = StackFrame::new();

        // 分配局部变量
        let var1 = Register::Virtual(1);
        let offset1 = frame.allocate_local(var1, 8, 8);
        assert_eq!(offset1, -8);

        let var2 = Register::Virtual(2);
        let offset2 = frame.allocate_local(var2, 4, 4);
        assert_eq!(offset2, -12);

        // 分配保存寄存器
        let reg_offset = frame.allocate_saved_register(5);
        assert_eq!(reg_offset, -20);

        // 检查总大小
        assert_eq!(frame.total_size, 32); // 对齐到16字节边界
    }

    #[test]
    fn test_stack_manager() {
        let cc = CallingConvention::standard();
        let mut stack_mgr = StackManager::new(cc, 1000);

        // 进入函数
        let saved_regs = vec![5, 7];
        let frame = stack_mgr.enter_function(&saved_regs, 16, 2);

        assert_eq!(stack_mgr.depth(), 1);
        assert!(frame.total_size > 0);

        // 离开函数
        let popped_frame = stack_mgr.leave_function();
        assert!(popped_frame.is_some());
        assert_eq!(stack_mgr.depth(), 0);
    }

    #[test]
    fn test_alignment() {
        assert_eq!(align_up(1, 8), 8);
        assert_eq!(align_up(8, 8), 8);
        assert_eq!(align_up(9, 8), 16);
        assert_eq!(align_up(15, 16), 16);
        assert_eq!(align_up(17, 16), 32);
    }
}
