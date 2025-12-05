//! GC 分配器 - 基于 Immix GC 的内存分配器
//!
//! 这个模块实现了 Allocator trait，使用 Karte GC 进行内存管理。

use crate::allocator::{AllocError, Allocator, LayoutRequest};
use karte_gc::{gc_alloc, ObjectType};
use std::alloc::Layout;

/// GC 分配器
///
/// 使用 Karte GC (Immix) 进行内存分配和管理。
/// 不需要手动释放内存，GC 会自动回收不可达的对象。
#[derive(Debug, Default)]
pub struct GcAllocator;

impl GcAllocator {
    /// 创建新的 GC 分配器
    pub fn new() -> Self {
        Self
    }

    /// 根据布局大小判断对象类型
    fn object_type_from_layout(layout: &Layout) -> ObjectType {
        // 简单的启发式规则：
        // - 小对象（<= 8 字节）：可能是基本类型（number, bool），标记为 Conservative
        // - 大对象（> 8 字节）：可能包含指针或复杂结构，使用 Conservative 保守扫描
        //
        // 注意：由于无法在分配时精确知道对象内部结构，我们默认使用 Conservative
        // 类型。Conservative 类型会保守扫描对象的每个字段，确保不会漏掉任何指针。
        let size = layout.size();

        if size <= 8 {
            // 单个字段，可能是基本类型，但为了安全仍使用 Conservative
            ObjectType::Conservative
        } else {
            // 较大的对象，很可能包含多个字段或指针
            ObjectType::Conservative
        }
    }
}

impl Allocator for GcAllocator {
    fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
        if layout.size() == 0 {
            return Err(AllocError::InvalidLayout {
                request: LayoutRequest {
                    size: layout.size(),
                    align: layout.align(),
                },
            });
        }

        let obj_type = Self::object_type_from_layout(&layout);

        unsafe {
            let ptr = gc_alloc(layout.size(), obj_type);

            if ptr.is_null() {
                Err(AllocError::OutOfMemory { layout })
            } else {
                Ok(ptr)
            }
        }
    }

    fn alloc_zeroed(&self, layout: Layout) -> Result<*mut u8, AllocError> {
        // GC 分配器默认会将内存清零
        // Immix 的实现通常会返回已清零的内存
        let ptr = self.alloc(layout)?;

        // 为了确保，我们显式清零
        unsafe {
            ptr.write_bytes(0, layout.size());
        }

        Ok(ptr)
    }

    fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // GC 管理的内存不需要手动释放
        // 这是一个 no-op
        // GC 会自动回收不可达的对象
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gc_allocator_basic() {
        let allocator = GcAllocator::new();

        // 测试小对象分配
        let layout = Layout::from_size_align(64, 8).unwrap();
        let ptr = allocator.alloc(layout).expect("分配失败");
        assert!(!ptr.is_null());

        // GC 管理的内存不需要手动释放
        // allocator.dealloc(ptr, layout); // no-op
    }

    #[test]
    fn test_gc_allocator_zeroed() {
        let allocator = GcAllocator::new();

        let layout = Layout::from_size_align(128, 8).unwrap();
        let ptr = allocator.alloc_zeroed(layout).expect("分配失败");
        assert!(!ptr.is_null());

        // 验证内存已清零
        unsafe {
            for i in 0..128 {
                assert_eq!(*ptr.add(i), 0, "字节 {} 应该为0", i);
            }
        }
    }

    #[test]
    fn test_gc_allocator_multiple_allocs() {
        let allocator = GcAllocator::new();

        // 多次分配
        for i in 0..10 {
            let size = 64 + i * 8;
            let layout = Layout::from_size_align(size, 8).unwrap();
            let ptr = allocator.alloc(layout).expect(&format!("分配 {} 失败", i));
            assert!(!ptr.is_null());
        }
    }

    #[test]
    fn test_object_type_selection() {
        // 测试对象类型选择逻辑
        // 所有大小的对象都使用 Conservative 类型进行保守扫描
        let small_layout = Layout::from_size_align(8, 8).unwrap();
        assert_eq!(
            GcAllocator::object_type_from_layout(&small_layout),
            ObjectType::Conservative
        );

        let medium_layout = Layout::from_size_align(64, 8).unwrap();
        assert_eq!(
            GcAllocator::object_type_from_layout(&medium_layout),
            ObjectType::Conservative
        );

        let large_layout = Layout::from_size_align(512, 8).unwrap();
        assert_eq!(
            GcAllocator::object_type_from_layout(&large_layout),
            ObjectType::Conservative
        );
    }
}
