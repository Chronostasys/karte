//! Karte 运行时支持库
//!
//! 提供可替换的分配器接口（P1 目标）以及供 JIT 代码复用的 FFI 入口。

pub mod allocator;
pub mod ffi;
pub mod stats;

pub use allocator::{
    set_allocator, AllocError, Allocator, AllocatorHandle, LayoutRequest, SystemAllocator,
};
pub use stats::{AllocationEvent, HeapStats};

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn alloc_and_free_through_ffi() {
        let ptr = ffi::karte_jit_runtime_alloc_aligned(32, 16);
        assert_ne!(ptr, 0);
        ffi::karte_jit_runtime_free(ptr);
    }

    #[test]
    #[serial]
    fn heap_stats_updates() {
        let mut stats = HeapStats::default();
        ffi::karte_jit_runtime_heap_stats(&mut stats as *mut _);
        let before_total = stats.total_allocations;

        let ptr = ffi::karte_jit_runtime_alloc(64);
        assert_ne!(ptr, 0);
        ffi::karte_jit_runtime_free(ptr);

        ffi::karte_jit_runtime_heap_stats(&mut stats as *mut _);
        assert!(stats.total_allocations >= before_total + 1);
    }

    #[test]
    #[serial]
    fn retain_and_release_drive_counts() {
        // Reset stats or capture baseline?
        // Since we are serial, we expect clean state if other tests clean up.
        // But let's capture baseline to be safe against previous tests.
        let mut initial_stats = HeapStats::default();
        ffi::karte_jit_runtime_heap_stats(&mut initial_stats as *mut _);

        let ptr = ffi::karte_jit_runtime_alloc(8);
        assert_ne!(ptr, 0);

        // 第一次 retain：计数从 1 -> 2
        ffi::karte_jit_runtime_retain(ptr);
        let mut stats = HeapStats::default();
        ffi::karte_jit_runtime_heap_stats(&mut stats as *mut _);

        // Check relative changes
        assert_eq!(stats.total_retain_ops, initial_stats.total_retain_ops + 1);
        assert_eq!(
            stats.rc_tracked_objects,
            initial_stats.rc_tracked_objects + 1
        );

        // 第一次 release：计数回到 1，不会释放
        ffi::karte_jit_runtime_release(ptr);
        ffi::karte_jit_runtime_heap_stats(&mut stats as *mut _);
        assert_eq!(stats.total_release_ops, initial_stats.total_release_ops + 1);
        assert_eq!(
            stats.active_allocations,
            initial_stats.active_allocations + 1
        );
        assert_eq!(stats.rc_zero_releases, initial_stats.rc_zero_releases);

        // 第二次 release：计数降为 0，自动释放
        ffi::karte_jit_runtime_release(ptr);
        ffi::karte_jit_runtime_heap_stats(&mut stats as *mut _);
        assert_eq!(stats.active_allocations, initial_stats.active_allocations);
        assert_eq!(stats.rc_zero_releases, initial_stats.rc_zero_releases + 1);
        assert_eq!(stats.rc_tracked_objects, initial_stats.rc_tracked_objects);
    }
}
