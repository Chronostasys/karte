//! 供 JIT/FFI 直接调用的运行时 API

use crate::allocator::{global_allocator, LayoutRequest};
use crate::stats::{
    current_stats, register_allocation, release_allocation, retain_allocation,
    unregister_allocation, HeapStats, ReleaseOutcome,
};
use log::trace;
use std::ptr;

#[no_mangle]
pub extern "C" fn karte_jit_runtime_alloc(size: u64) -> u64 {
    karte_jit_runtime_alloc_aligned(size, 8)
}

#[no_mangle]
pub extern "C" fn karte_jit_runtime_alloc_aligned(size: u64, alignment: u64) -> u64 {
    let request = LayoutRequest {
        size: size as usize,
        align: alignment as usize,
    };
    let layout = match request.to_layout() {
        Ok(layout) => layout,
        Err(_) => return 0,
    };

    global_allocator().with(|alloc| match alloc.alloc_zeroed(layout) {
        Ok(ptr) => {
            register_allocation(ptr, layout);
            ptr as u64
        }
        Err(_) => 0,
    })
}

#[no_mangle]
pub extern "C" fn karte_jit_runtime_free(ptr: u64) {
    if ptr == 0 {
        return;
    }

    if let Some(layout) = unregister_allocation(ptr) {
        trace!("free {:p} (size={})", ptr as *const u8, layout.size());
        global_allocator().with(|alloc| {
            alloc.dealloc(ptr as *mut u8, layout);
        });
    } else {
        trace!("free {:p} ignored (not tracked)", ptr as *const u8);
    }
}

/// ARC 钩子：增加引用计数（当前实现仅做调试级引用计数）
#[no_mangle]
pub extern "C" fn karte_jit_runtime_retain(ptr: u64) {
    if let Some(count) = retain_allocation(ptr) {
        trace!("retain {:p} -> {}", ptr as *const u8, count);
    }
}

/// ARC 钩子：减少引用计数，降为 0 时自动释放
#[no_mangle]
pub extern "C" fn karte_jit_runtime_release(ptr: u64) {
    match release_allocation(ptr) {
        Some(ReleaseOutcome::StillAlive(count)) => {
            trace!("release {:p} -> {}", ptr as *const u8, count);
        }
        Some(ReleaseOutcome::ShouldFree(layout)) => {
            trace!("release {:p} -> drop", ptr as *const u8);
            global_allocator().with(|alloc| {
                alloc.dealloc(ptr as *mut u8, layout);
            });
        }
        None => {
            if ptr != 0 {
                trace!("release {:p} ignored (not tracked)", ptr as *const u8);
            }
        }
    }
}

/// 将当前堆统计信息写入 `out`（可为空）
#[no_mangle]
pub extern "C" fn karte_jit_runtime_heap_stats(out: *mut HeapStats) {
    if out.is_null() {
        return;
    }
    unsafe {
        ptr::write(out, current_stats());
    }
}
