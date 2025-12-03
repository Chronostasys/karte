//! Karte GC 分配器
//!
//! 提供更高层次的内存分配接口，支持：
//! - 类型安全的分配
//! - 分配策略选择（栈 vs 堆）
//! - 分配统计和监控

use crate::{gc_alloc, gc_alloc_no_collect, ObjectType};
use std::alloc::Layout;
use std::sync::atomic::{AtomicUsize, Ordering};

/// 分配统计信息
#[derive(Debug, Default)]
pub struct AllocationStats {
    /// 总分配次数
    pub total_allocations: AtomicUsize,
    /// 总分配字节数
    pub total_bytes_allocated: AtomicUsize,
    /// 原子类型分配次数
    pub atomic_allocations: AtomicUsize,
    /// 指针类型分配次数
    pub pointer_allocations: AtomicUsize,
    /// 复杂类型分配次数
    pub complex_allocations: AtomicUsize,
}

impl AllocationStats {
    /// 创建新的统计实例
    pub fn new() -> Self {
        Self::default()
    }

    /// 记录一次分配
    pub fn record_allocation(&self, size: usize, obj_type: ObjectType) {
        self.total_allocations.fetch_add(1, Ordering::Relaxed);
        self.total_bytes_allocated
            .fetch_add(size, Ordering::Relaxed);

        match obj_type {
            ObjectType::Atomic => {
                self.atomic_allocations.fetch_add(1, Ordering::Relaxed);
            }
            ObjectType::Pointer => {
                self.pointer_allocations.fetch_add(1, Ordering::Relaxed);
            }
            ObjectType::Complex => {
                self.complex_allocations.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// 获取总分配次数
    pub fn total_allocations(&self) -> usize {
        self.total_allocations.load(Ordering::Relaxed)
    }

    /// 获取总分配字节数
    pub fn total_bytes(&self) -> usize {
        self.total_bytes_allocated.load(Ordering::Relaxed)
    }

    /// 生成统计报告
    pub fn report(&self) -> String {
        format!(
            "GC Allocation Stats:\n\
             - Total allocations: {}\n\
             - Total bytes: {} ({:.2} MB)\n\
             - Atomic: {}\n\
             - Pointer: {}\n\
             - Complex: {}",
            self.total_allocations(),
            self.total_bytes(),
            self.total_bytes() as f64 / 1024.0 / 1024.0,
            self.atomic_allocations.load(Ordering::Relaxed),
            self.pointer_allocations.load(Ordering::Relaxed),
            self.complex_allocations.load(Ordering::Relaxed)
        )
    }
}

/// GC 分配器
pub struct GcAllocator {
    /// 分配统计
    stats: AllocationStats,
}

impl GcAllocator {
    /// 创建新的 GC 分配器
    pub fn new() -> Self {
        Self {
            stats: AllocationStats::new(),
        }
    }

    /// 分配指定大小和类型的内存
    ///
    /// # Safety
    ///
    /// 返回的指针必须正确使用，遵守 GC 的生命周期规则
    pub unsafe fn allocate(&self, size: usize, obj_type: ObjectType) -> *mut u8 {
        let ptr = gc_alloc(size, obj_type);

        if !ptr.is_null() {
            self.stats.record_allocation(size, obj_type);
        }

        ptr
    }

    /// 分配内存但不触发 GC
    ///
    /// # Safety
    ///
    /// 如果内存不足可能返回 null
    pub unsafe fn allocate_no_collect(&self, size: usize, obj_type: ObjectType) -> *mut u8 {
        let ptr = gc_alloc_no_collect(size, obj_type);

        if !ptr.is_null() {
            self.stats.record_allocation(size, obj_type);
        }

        ptr
    }

    /// 分配指定 Layout 的内存
    ///
    /// # Safety
    ///
    /// 返回的指针必须满足 layout 的对齐要求
    pub unsafe fn allocate_layout(
        &self,
        layout: Layout,
        obj_type: ObjectType,
    ) -> Result<*mut u8, AllocationError> {
        // Immix 默认按 8 字节对齐，对于更大的对齐要求需要额外处理
        if layout.align() > 8 {
            return Err(AllocationError::UnsupportedAlignment(layout.align()));
        }

        let ptr = self.allocate(layout.size(), obj_type);

        if ptr.is_null() {
            Err(AllocationError::OutOfMemory)
        } else {
            Ok(ptr)
        }
    }

    /// 获取分配统计信息
    pub fn stats(&self) -> &AllocationStats {
        &self.stats
    }
}

impl Default for GcAllocator {
    fn default() -> Self {
        Self::new()
    }
}

/// 分配错误类型
#[derive(Debug, thiserror::Error)]
pub enum AllocationError {
    #[error("Out of memory")]
    OutOfMemory,

    #[error("Unsupported alignment: {0}")]
    UnsupportedAlignment(usize),

    #[error("Invalid size: {0}")]
    InvalidSize(usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allocation_stats() {
        let stats = AllocationStats::new();

        stats.record_allocation(100, ObjectType::Atomic);
        stats.record_allocation(200, ObjectType::Pointer);
        stats.record_allocation(300, ObjectType::Complex);

        assert_eq!(stats.total_allocations(), 3);
        assert_eq!(stats.total_bytes(), 600);
    }
}
