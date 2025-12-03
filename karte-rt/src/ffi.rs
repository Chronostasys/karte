//! 供 JIT/FFI 直接调用的运行时 API

use crate::stats::{current_stats, HeapStats};
use log::trace;
use std::ptr;

// GC 相关导入
use karte_gc::{gc_alloc, gc_alloc_no_collect, gc_safepoint, ObjectType};

#[no_mangle]
pub extern "C" fn karte_jit_runtime_alloc(size: u64) -> u64 {
    karte_jit_runtime_alloc_aligned(size, 8)
}

#[no_mangle]
pub extern "C" fn karte_jit_runtime_alloc_aligned(size: u64, alignment: u64) -> u64 {
    // 忽略 alignment 参数，GC 会自动对齐
    // GC 分配的对象至少 8 字节对齐
    let _ = alignment;

    if size == 0 {
        return 0;
    }

    // 根据大小选择对象类型
    let obj_type = if size <= 8 {
        ObjectType::Atomic
    } else if size <= 64 {
        ObjectType::Complex
    } else {
        ObjectType::Complex
    };

    unsafe {
        let ptr = gc_alloc(size as usize, obj_type);
        if ptr.is_null() {
            0
        } else {
            // 清零内存
            ptr.write_bytes(0, size as usize);
            ptr as u64
        }
    }
}

#[no_mangle]
pub extern "C" fn karte_jit_runtime_free(ptr: u64) {
    // GC 管理的内存不需要手动释放
    // 这是一个 no-op 函数，保留以兼容现有代码
    if ptr != 0 {
        trace!("free {:p} (no-op, GC managed)", ptr as *const u8);
    }
}

/// ARC 钩子：增加引用计数
///
/// **注意**：在 GC 模式下，这是一个 no-op 函数。
/// GC 会自动管理对象的生命周期，不需要手动引用计数。
#[no_mangle]
pub extern "C" fn karte_jit_runtime_retain(ptr: u64) {
    if ptr != 0 {
        trace!("retain {:p} (no-op, GC managed)", ptr as *const u8);
    }
}

/// ARC 钩子：减少引用计数
///
/// **注意**：在 GC 模式下，这是一个 no-op 函数。
/// GC 会自动管理对象的生命周期，不需要手动引用计数。
#[no_mangle]
pub extern "C" fn karte_jit_runtime_release(ptr: u64) {
    if ptr != 0 {
        trace!("release {:p} (no-op, GC managed)", ptr as *const u8);
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

/// GC 安全点
///
/// 在循环回边、长时间运行的代码等位置调用，允许 GC 在这些点暂停执行。
/// 这是一个无操作函数（no-op），主要依赖 Immix GC 的保守扫描机制。
#[no_mangle]
pub extern "C" fn karte_jit_runtime_gc_safepoint() {
    unsafe {
        gc_safepoint();
    }
    trace!("GC safepoint reached");
}
