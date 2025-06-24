//! 虚拟机内存管理
//!
//! 提供内存分配、访问和管理功能

use super::{MEMORY_SIZE, STACK_SIZE};

/// 内存管理器
#[derive(Debug, Clone)]
pub struct MemoryManager {
    /// 主内存区域
    pub memory: Vec<i64>,
    /// 栈内存
    pub stack: Vec<i64>,
    /// 堆指针
    pub heap_pointer: usize,
    /// 栈指针
    pub stack_pointer: usize,
}

impl MemoryManager {
    /// 创建新的内存管理器
    pub fn new() -> Self {
        Self {
            memory: vec![0; MEMORY_SIZE],
            stack: vec![0; STACK_SIZE],
            heap_pointer: 0,
            stack_pointer: STACK_SIZE - 1,
        }
    }

    /// 从内存读取值
    pub fn read_memory(&self, address: usize) -> Result<i64, String> {
        if address < self.memory.len() {
            Ok(self.memory[address])
        } else {
            Err(format!("Memory read out of bounds: address {}", address))
        }
    }

    /// 向内存写入值
    pub fn write_memory(&mut self, address: usize, value: i64) -> Result<(), String> {
        if address < self.memory.len() {
            self.memory[address] = value;
            Ok(())
        } else {
            Err(format!("Memory write out of bounds: address {}", address))
        }
    }

    /// 从栈中读取值
    pub fn read_stack(&self, offset: usize) -> Result<i64, String> {
        let address = self.stack_pointer.wrapping_add(offset);
        if address < self.stack.len() {
            Ok(self.stack[address])
        } else {
            Err(format!("Stack read out of bounds: offset {}", offset))
        }
    }

    /// 向栈中写入值
    pub fn write_stack(&mut self, offset: usize, value: i64) -> Result<(), String> {
        let address = self.stack_pointer.wrapping_add(offset);
        if address < self.stack.len() {
            self.stack[address] = value;
            Ok(())
        } else {
            Err(format!("Stack write out of bounds: offset {}", offset))
        }
    }

    /// 推入栈
    pub fn push_stack(&mut self, value: i64) -> Result<(), String> {
        if self.stack_pointer > 0 {
            self.stack_pointer -= 1;
            self.stack[self.stack_pointer] = value;
            Ok(())
        } else {
            Err("Stack overflow".to_string())
        }
    }

    /// 弹出栈
    pub fn pop_stack(&mut self) -> Result<i64, String> {
        if self.stack_pointer < STACK_SIZE {
            let value = self.stack[self.stack_pointer];
            self.stack[self.stack_pointer] = 0; // 清零
            self.stack_pointer += 1;
            Ok(value)
        } else {
            Err("Stack underflow".to_string())
        }
    }

    /// 分配堆内存
    pub fn allocate_heap(&mut self, size: usize) -> Result<usize, String> {
        if self.heap_pointer + size <= self.memory.len() {
            let address = self.heap_pointer;
            self.heap_pointer += size;
            Ok(address)
        } else {
            Err("Heap allocation failed: not enough memory".to_string())
        }
    }

    /// 重置内存状态
    pub fn reset(&mut self) {
        self.memory.fill(0);
        self.stack.fill(0);
        self.heap_pointer = 0;
        self.stack_pointer = STACK_SIZE - 1;
    }

    /// 获取内存使用统计
    pub fn get_memory_stats(&self) -> MemoryStats {
        MemoryStats {
            total_memory: self.memory.len(),
            used_heap: self.heap_pointer,
            available_heap: self.memory.len() - self.heap_pointer,
            stack_size: STACK_SIZE,
            used_stack: STACK_SIZE - self.stack_pointer,
            available_stack: self.stack_pointer,
        }
    }

    /// 打印内存状态（用于调试）
    pub fn print_memory_state(&self) {
        println!("=== Memory State ===");
        println!("Heap pointer: {}", self.heap_pointer);
        println!("Stack pointer: {}", self.stack_pointer);

        let stats = self.get_memory_stats();
        println!("Memory statistics: {:?}", stats);

        // 打印栈的非零内容
        println!("Non-zero stack values:");
        for (i, &value) in self.stack.iter().enumerate() {
            if value != 0 {
                println!("  stack[{}]: {}", i, value);
            }
        }

        // 打印堆的非零内容（仅前100个位置）
        println!("Non-zero heap values (first 100):");
        for (i, &value) in self.memory.iter().enumerate().take(100) {
            if value != 0 {
                println!("  memory[{}]: {}", i, value);
            }
        }
    }
}

/// 内存使用统计
#[derive(Debug, Clone)]
pub struct MemoryStats {
    pub total_memory: usize,
    pub used_heap: usize,
    pub available_heap: usize,
    pub stack_size: usize,
    pub used_stack: usize,
    pub available_stack: usize,
}

impl Default for MemoryManager {
    fn default() -> Self {
        Self::new()
    }
}
