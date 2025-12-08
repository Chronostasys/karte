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

    // 使用 Conservative 作为默认类型（保守但安全）
    // 对于不确定内部结构的分配，保守扫描可以保证不会漏掉任何潜在的指针
    let obj_type = ObjectType::Conservative;

    unsafe {
        let ptr = gc_alloc(size as usize, obj_type);
        if ptr.is_null() {
            log::warn!(
                "karte_jit_runtime_alloc_aligned: allocation failed for size={}",
                size
            );
            0
        } else {
            // 清零内存
            ptr.write_bytes(0, size as usize);
            log::info!(
                "karte_jit_runtime_alloc_aligned: allocated {} bytes at {:p}, type={:?}",
                size,
                ptr,
                obj_type
            );
            ptr as u64
        }
    }
}

/// JIT 调用的类型化分配函数
///
/// 允许 JIT 代码显式指定对象类型，以便 GC 能够更精确地扫描对象。
///
/// # 参数
///
/// * `size` - 要分配的字节数
/// * `obj_type` - 对象类型 (u8 表示): Atomic=0, Trait=1, Pointer=3, Conservative=4
///
/// # 返回值
///
/// 返回分配的内存指针（64位地址），如果分配失败返回 0
#[no_mangle]
pub extern "C" fn karte_jit_runtime_alloc_typed(size: u64, obj_type: u8) -> u64 {
    if size == 0 {
        return 0;
    }

    // 解析对象类型，如果解析失败则默认使用 Conservative
    // Conservative 类型使用保守扫描，能够安全处理所有对象
    let obj_type = ObjectType::from_u8(obj_type).unwrap_or(ObjectType::Conservative);

    unsafe {
        let ptr = gc_alloc(size as usize, obj_type);
        if ptr.is_null() {
            0
        } else {
            // 清零内存，确保对象初始状态干净
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

/// 更新虚拟栈顶指针（供JIT代码调用）
///
/// JIT代码在调用C FFI前调用此函数，传入当前的虚拟SP（x6寄存器的值）
///
/// # Safety
///
/// stack_top 必须是有效的虚拟栈指针
#[no_mangle]
pub extern "C" fn karte_jit_runtime_update_stack_top(stack_top: u64) {
    unsafe {
        karte_gc::update_virtual_stack_top(stack_top as *const u8);
    }
}
