//! Karte 运行时支持库
//!
//! 提供可替换的分配器接口（P1 目标）以及供 JIT 代码复用的 FFI 入口。

pub mod allocator;
pub mod ffi;
pub mod gc_allocator;
pub mod stats;

pub use allocator::{
    set_allocator, AllocError, Allocator, AllocatorHandle, LayoutRequest, SystemAllocator,
};
pub use gc_allocator::GcAllocator;
pub use stats::{AllocationEvent, HeapStats};

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn alloc_and_free_through_ffi() {
        // 初始化 GC
        unsafe {
            karte_gc::initialize_gc();
        }

        let ptr = ffi::karte_jit_runtime_alloc_aligned(32, 16);
        assert_ne!(ptr, 0);

        // free 在 GC 模式下是 no-op，但不应崩溃
        ffi::karte_jit_runtime_free(ptr);

        // 验证指针仍然有效（GC 还未回收）
        unsafe {
            let byte_ptr = ptr as *mut u8;
            assert_eq!(*byte_ptr, 0); // 应该是已清零的内存
        }
    }

    #[test]
    #[serial]
    fn gc_alloc_basic() {
        unsafe {
            karte_gc::initialize_gc();
        }

        // 测试多次分配都成功
        for i in 0..10 {
            let size = 64 + i * 8;
            let ptr = ffi::karte_jit_runtime_alloc(size);
            assert_ne!(ptr, 0, "分配 {} 字节失败", size);

            // 验证内存可写
            unsafe {
                let byte_ptr = ptr as *mut u8;
                *byte_ptr = (i as u8);
                assert_eq!(*byte_ptr, i as u8);
            }
        }
    }

    #[test]
    #[serial]
    fn gc_alloc_zeroed() {
        unsafe {
            karte_gc::initialize_gc();
        }

        let ptr = ffi::karte_jit_runtime_alloc(128);
        assert_ne!(ptr, 0);

        // 验证内存已清零
        unsafe {
            let byte_ptr = ptr as *mut u8;
            for i in 0..128 {
                assert_eq!(*byte_ptr.add(i), 0, "字节 {} 应该为 0", i);
            }
        }
    }

    #[test]
    #[serial]
    fn retain_and_release_are_noop() {
        unsafe {
            karte_gc::initialize_gc();
        }

        let ptr = ffi::karte_jit_runtime_alloc(8);
        assert_ne!(ptr, 0);

        // retain 和 release 在 GC 模式下是 no-op
        // 它们不应崩溃
        ffi::karte_jit_runtime_retain(ptr);
        ffi::karte_jit_runtime_release(ptr);
        ffi::karte_jit_runtime_release(ptr);

        // 指针仍然有效（GC 管理）
        unsafe {
            let byte_ptr = ptr as *mut u8;
            *byte_ptr = 42;
            assert_eq!(*byte_ptr, 42);
        }
    }
}
