//! 堆分配器
//!
//! 提供专业的堆内存分配功能，支持：
//! - 基本的堆内存分配
//! - 对象类型管理
//! - 内存布局优化
//! - 为lambda闭包环境提供分配支持

use std::collections::HashMap;

/// 堆对象类型
#[derive(Debug, Clone, PartialEq)]
pub enum HeapObjectType {
    /// 闭包环境对象
    ClosureEnv {
        /// 环境大小（字节数）
        size: usize,
        /// 字段数量
        field_count: usize,
    },
    /// 结构体对象
    Struct {
        name: String,
        size: usize,
        field_count: usize,
    },
    /// 数组对象
    Array { element_size: usize, length: usize },
    /// 原始数据块
    Raw { size: usize },
}

/// 堆对象元数据
#[derive(Debug, Clone)]
pub struct HeapObject {
    /// 对象类型
    pub object_type: HeapObjectType,
    /// 分配地址
    pub address: usize,
    /// 对象大小
    pub size: usize,
    /// 是否仍在使用
    pub is_alive: bool,
    /// 分配时间戳（用于调试）
    pub allocation_id: u64,
}

/// 内存块信息
#[derive(Debug, Clone)]
struct MemoryBlock {
    /// 起始地址
    start_addr: usize,
    /// 块大小
    size: usize,
    /// 是否可用
    is_free: bool,
}

/// 专业堆分配器
///
/// 使用简单的first-fit分配策略，专注于正确性而非性能
#[derive(Debug)]
pub struct HeapAllocator {
    /// 堆内存起始地址
    heap_start: usize,
    /// 堆内存总大小
    heap_size: usize,
    /// 当前分配指针
    current_ptr: usize,
    /// 已分配对象的元数据
    allocated_objects: HashMap<usize, HeapObject>,
    /// 空闲内存块列表
    free_blocks: Vec<MemoryBlock>,
    /// 分配计数器（用于生成分配ID）
    allocation_counter: u64,
    /// 总分配字节数
    total_allocated: usize,
    /// 峰值内存使用
    peak_usage: usize,
}

impl HeapAllocator {
    /// 创建新的堆分配器
    ///
    /// # 参数
    /// - `heap_start`: 堆内存起始地址
    /// - `heap_size`: 堆内存总大小
    pub fn new(heap_start: usize, heap_size: usize) -> Self {
        let mut free_blocks = Vec::new();
        free_blocks.push(MemoryBlock {
            start_addr: heap_start,
            size: heap_size,
            is_free: true,
        });

        Self {
            heap_start,
            heap_size,
            current_ptr: heap_start,
            allocated_objects: HashMap::new(),
            free_blocks,
            allocation_counter: 0,
            total_allocated: 0,
            peak_usage: 0,
        }
    }

    /// 分配闭包环境对象
    ///
    /// # 参数
    /// - `field_count`: 环境中字段的数量
    ///
    /// # 返回值
    /// 返回分配的地址，失败时返回错误
    pub fn allocate_closure_env(&mut self, field_count: usize) -> crate::Result<usize> {
        // 每个字段8字节，加上对象头部的8字节
        let size = 8 + field_count * 8;
        let aligned_size = align_up(size, 8);

        let obj_type = HeapObjectType::ClosureEnv {
            size: aligned_size,
            field_count,
        };

        self.allocate_object(obj_type, aligned_size)
    }

    /// 分配结构体对象
    pub fn allocate_struct(
        &mut self,
        name: String,
        field_count: usize,
        size: usize,
    ) -> crate::Result<usize> {
        let aligned_size = align_up(size, 8);

        let obj_type = HeapObjectType::Struct {
            name,
            size: aligned_size,
            field_count,
        };

        self.allocate_object(obj_type, aligned_size)
    }

    /// 分配原始内存块
    pub fn allocate_raw(&mut self, size: usize) -> crate::Result<usize> {
        let aligned_size = align_up(size, 8);

        let obj_type = HeapObjectType::Raw { size: aligned_size };

        self.allocate_object(obj_type, aligned_size)
    }

    /// 通用对象分配函数
    fn allocate_object(&mut self, obj_type: HeapObjectType, size: usize) -> crate::Result<usize> {
        // 查找合适的空闲块
        let block_index = self
            .find_suitable_block(size)
            .ok_or_else(|| crate::KarteError::from(format!("堆内存不足：需要 {} 字节", size)))?;

        let alloc_addr = self.free_blocks[block_index].start_addr;
        let old_block_size = self.free_blocks[block_index].size;

        // 分割块（如果剩余空间足够）
        if old_block_size > size + 16 {
            // 16字节的最小块大小
            let remaining_block = MemoryBlock {
                start_addr: alloc_addr + size,
                size: old_block_size - size,
                is_free: true,
            };
            self.free_blocks.push(remaining_block);
        }

        // 标记当前块为已使用
        self.free_blocks[block_index].size = size;
        self.free_blocks[block_index].is_free = false;

        // 创建对象元数据
        self.allocation_counter += 1;
        let heap_object = HeapObject {
            object_type: obj_type,
            address: alloc_addr,
            size,
            is_alive: true,
            allocation_id: self.allocation_counter,
        };

        self.allocated_objects.insert(alloc_addr, heap_object);

        // 更新统计信息
        self.total_allocated += size;
        self.peak_usage = self.peak_usage.max(self.total_allocated);

        Ok(alloc_addr)
    }

    /// 查找合适的空闲块
    fn find_suitable_block(&self, size: usize) -> Option<usize> {
        self.free_blocks
            .iter()
            .enumerate()
            .find(|(_, block)| block.is_free && block.size >= size)
            .map(|(index, _)| index)
    }

    /// 获取对象元数据
    pub fn get_object(&self, address: usize) -> Option<&HeapObject> {
        self.allocated_objects.get(&address)
    }

    /// 检查地址是否有效
    pub fn is_valid_address(&self, address: usize) -> bool {
        address >= self.heap_start
            && address < self.heap_start + self.heap_size
            && self.allocated_objects.contains_key(&address)
    }

    /// 获取分配统计信息
    pub fn get_allocation_stats(&self) -> AllocationStats {
        AllocationStats {
            total_allocated: self.total_allocated,
            peak_usage: self.peak_usage,
            active_objects: self.allocated_objects.len(),
            free_blocks: self.free_blocks.iter().filter(|b| b.is_free).count(),
            heap_size: self.heap_size,
            heap_utilization: (self.total_allocated as f64 / self.heap_size as f64) * 100.0,
        }
    }

    /// 重置分配器（清除所有分配）
    pub fn reset(&mut self) {
        self.current_ptr = self.heap_start;
        self.allocated_objects.clear();
        self.free_blocks.clear();
        self.allocation_counter = 0;
        self.total_allocated = 0;
        self.peak_usage = 0;

        // 重新添加整个堆作为一个空闲块
        self.free_blocks.push(MemoryBlock {
            start_addr: self.heap_start,
            size: self.heap_size,
            is_free: true,
        });
    }

    /// 打印分配状态（调试用）
    pub fn print_allocation_state(&self) {
        println!("=== 堆分配器状态 ===");
        println!("堆起始地址: 0x{:x}", self.heap_start);
        println!("堆大小: {} 字节", self.heap_size);
        println!("已分配对象数: {}", self.allocated_objects.len());
        println!("总分配字节: {}", self.total_allocated);
        println!("峰值使用: {} 字节", self.peak_usage);

        println!("已分配对象:");
        for (addr, obj) in &self.allocated_objects {
            println!("  0x{:x}: {:?}", addr, obj.object_type);
        }

        println!("空闲块:");
        for (i, block) in self.free_blocks.iter().enumerate() {
            if block.is_free {
                println!(
                    "  块{}: 0x{:x} - 0x{:x} ({} 字节)",
                    i,
                    block.start_addr,
                    block.start_addr + block.size,
                    block.size
                );
            }
        }
    }
}

/// 分配统计信息
#[derive(Debug, Clone)]
pub struct AllocationStats {
    pub total_allocated: usize,
    pub peak_usage: usize,
    pub active_objects: usize,
    pub free_blocks: usize,
    pub heap_size: usize,
    pub heap_utilization: f64,
}

/// 向上对齐到指定边界
fn align_up(value: usize, alignment: usize) -> usize {
    (value + alignment - 1) & !(alignment - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heap_allocator_creation() {
        let allocator = HeapAllocator::new(0x10000, 1024);
        assert_eq!(allocator.heap_start, 0x10000);
        assert_eq!(allocator.heap_size, 1024);
        assert_eq!(allocator.total_allocated, 0);
    }

    #[test]
    fn test_closure_env_allocation() {
        let mut allocator = HeapAllocator::new(0x10000, 1024);

        // 分配一个有3个字段的闭包环境
        let addr = allocator.allocate_closure_env(3).unwrap();
        assert_eq!(addr, 0x10000);

        let obj = allocator.get_object(addr).unwrap();
        match &obj.object_type {
            HeapObjectType::ClosureEnv { field_count, .. } => {
                assert_eq!(*field_count, 3);
            }
            _ => panic!("期望闭包环境对象"),
        }
    }

    #[test]
    fn test_multiple_allocations() {
        let mut allocator = HeapAllocator::new(0x10000, 1024);

        let addr1 = allocator.allocate_closure_env(2).unwrap();
        let addr2 = allocator.allocate_closure_env(1).unwrap();

        assert_ne!(addr1, addr2);
        assert!(allocator.is_valid_address(addr1));
        assert!(allocator.is_valid_address(addr2));
    }

    #[test]
    fn test_allocation_stats() {
        let mut allocator = HeapAllocator::new(0x10000, 1024);

        allocator.allocate_closure_env(2).unwrap();
        allocator.allocate_raw(64).unwrap();

        let stats = allocator.get_allocation_stats();
        assert!(stats.total_allocated > 0);
        assert_eq!(stats.active_objects, 2);
        assert!(stats.heap_utilization > 0.0);
    }

    #[test]
    fn test_alignment() {
        assert_eq!(align_up(1, 8), 8);
        assert_eq!(align_up(8, 8), 8);
        assert_eq!(align_up(9, 8), 16);
        assert_eq!(align_up(15, 8), 16);
        assert_eq!(align_up(16, 8), 16);
    }
}
