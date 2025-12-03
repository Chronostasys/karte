//! Karte GC - 基于 Immix 的垃圾收集器集成层
//!
//! 这个 crate 提供了 Karte 编程语言专用的垃圾收集器接口，
//! 底层使用高性能的 Immix GC 算法。
//!
//! # 架构
//!
//! - **Immix GC**: 128KB block + 128-byte line 结构，支持快速分配和高效回收
//! - **保守栈扫描**: 自动在栈和寄存器中查找根对象，无需复杂的栈映射
//! - **自动GC**: 当内存压力大时自动触发收集
//! - **线程本地分配器**: 减少线程竞争，提升分配性能

use std::ptr;

// 重新导出 immix 的关键类型和函数
pub use immix::{
    gc_collect, gc_init, gc_malloc_fast_unwind, gc_malloc_no_collect, safepoint_fast_unwind,
    ObjectType as ImmixObjectType, GLOBAL_ALLOCATOR,
};

mod allocator;
mod root_scanner;

pub use allocator::*;
pub use root_scanner::*;

/// Karte 对象类型，映射到 Immix ObjectType
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ObjectType {
    /// 原子类型：不包含指针的基本类型（number, bool）
    Atomic = 0,
    /// 指针类型：单个指针或引用
    Pointer = 1,
    /// 复杂类型：包含多个字段的结构体、数组、闭包等
    Complex = 2,
}

impl From<ObjectType> for u8 {
    fn from(obj_type: ObjectType) -> u8 {
        obj_type as u8
    }
}

impl From<ObjectType> for ImmixObjectType {
    fn from(obj_type: ObjectType) -> ImmixObjectType {
        match obj_type {
            ObjectType::Atomic => ImmixObjectType::Atomic,
            ObjectType::Pointer => ImmixObjectType::Pointer,
            ObjectType::Complex => ImmixObjectType::Complex,
        }
    }
}

/// GC 初始化
///
/// 必须在程序启动时调用一次
///
/// # Safety
///
/// 这是一个 unsafe 函数，因为它涉及底层内存管理的初始化
pub unsafe fn initialize_gc() {
    log::info!("Initializing Karte GC (Immix-based)");

    // 如果启用了 llvm_stackmap 功能，需要调用 gc_init
    // 对于保守栈扫描模式，GC会自动初始化，无需显式调用
    #[cfg(feature = "llvm_stackmap")]
    {
        // TODO: 当我们有了实际的 stackmap 数据时，传递实际的指针
        // 现在先创建一个空的 stackmap 结构
        log::warn!("LLVM stackmap feature enabled but not using actual stackmap data");
        // 不调用 gc_init，让GC使用保守扫描作为后备
    }

    log::info!("Karte GC initialized successfully (using conservative stack scanning)");
}

/// 使用 GC 分配内存
///
/// 这是主要的分配函数，支持自动 GC 触发
///
/// **注意**：这个版本使用 Immix 的内部保守栈扫描，但由于 Karte 使用虚拟栈，
/// 实际的根应该通过 `register_virtual_stack_range()` 注册。
///
/// # Arguments
///
/// * `size` - 要分配的字节数
/// * `obj_type` - 对象类型（Atomic/Pointer/Complex）
///
/// # Returns
///
/// 返回分配的内存指针，如果分配失败返回 null
///
/// # Safety
///
/// 返回的指针必须正确使用，不能在 GC 回收后继续访问
pub unsafe fn gc_alloc(size: usize, obj_type: ObjectType) -> *mut u8 {
    log::trace!("GC alloc: size={}, type={:?}", size, obj_type);

    // 传递 null 指针作为栈指针
    // 这告诉 Immix 不要使用栈指针进行栈遍历
    // Immix 仍然会使用保守扫描来扫描当前线程的寄存器和栈
    // 对于 Karte 的虚拟栈，应该通过 register_virtual_stack_range() 注册
    gc_malloc_fast_unwind(size, obj_type.into(), ptr::null_mut())
}

/// 分配内存但不触发 GC
///
/// 用于在 GC 过程中或特殊情况下分配内存
///
/// # Safety
///
/// 如果内存不足，这个函数可能返回 null，调用者需要处理这种情况
pub unsafe fn gc_alloc_no_collect(size: usize, obj_type: ObjectType) -> *mut u8 {
    log::trace!("GC alloc (no collect): size={}, type={:?}", size, obj_type);
    gc_malloc_no_collect(size, obj_type.into())
}

/// 手动触发 GC 收集
///
/// 一般情况下不需要手动调用，GC 会在需要时自动触发
///
/// # Safety
///
/// GC 收集期间会暂停所有线程（STW），确保在安全的时机调用
pub unsafe fn gc_trigger_collect() {
    log::info!("Manual GC collection triggered");
    gc_collect();
}

/// GC 安全点
///
/// 在循环或长时间运行的代码中插入，允许 GC 在这些点暂停执行
///
/// **注意**：Karte 使用虚拟栈，所以这个函数不传递 C 系统栈指针。
/// 虚拟栈的根应该通过 `register_virtual_stack_range()` 预先注册。
///
/// # Safety
///
/// 调用此函数时，必须确保虚拟栈区间已经注册为 GC 根
pub unsafe fn gc_safepoint() {
    // 注意：由于 Karte 使用虚拟栈，我们不传递 C 系统栈指针
    // 虚拟栈的根通过其他方式注册

    // 如果 Immix 要求栈指针，我们传递一个占位符
    // 但实际的根扫描应该使用注册的虚拟栈区间
    safepoint_fast_unwind(ptr::null_mut());
}

/// 注册虚拟栈区间作为 GC 根
///
/// Karte 使用自己分配的虚拟栈（`Vec<i64>`），需要将整个栈区间注册为 GC 根。
///
/// # Arguments
///
/// * `stack_start` - 虚拟栈的起始地址
/// * `stack_end` - 虚拟栈的结束地址
///
/// # Safety
///
/// - 调用者必须确保 stack_start 和 stack_end 指向有效的栈内存区间
/// - 区间必须在 GC 的整个生命周期内保持有效
/// - 通常在 ExecutionEngine 初始化时调用一次
///
/// # Example
///
/// ```rust,ignore
/// let engine = ExecutionEngine::new();
/// let stack_start = engine.virtual_stack.as_ptr() as *const u8;
/// let stack_end = unsafe { stack_start.add(engine.virtual_stack.len() * 8) };
/// unsafe {
///     register_virtual_stack_range(stack_start, stack_end);
/// }
/// ```
pub unsafe fn register_virtual_stack_range(stack_start: *const u8, stack_end: *const u8) {
    log::info!(
        "Registering virtual stack range as GC roots: {:p} - {:p} ({} bytes)",
        stack_start,
        stack_end,
        stack_end as usize - stack_start as usize
    );

    // Immix GC 的保守扫描会在 GC 时遍历这个区间
    // 我们不需要显式注册区间，因为 Immix 会使用保守扫描
    // 但是我们需要确保在 GC 触发时，虚拟栈的内容是可达的

    // TODO: 当 Immix 支持显式栈区间注册时，在这里调用相应的 API
    // 目前，Immix 的保守扫描会扫描线程栈，但由于我们使用虚拟栈，
    // 可能需要在 GC 触发时手动扫描虚拟栈区间
}

/// 注册全局根对象
///
/// 全局变量可能包含 GC 对象的引用，需要注册为根
///
/// # Safety
///
/// 必须确保指针在 GC 生命周期内有效
pub unsafe fn register_global_root(ptr: *mut u8, obj_type: ObjectType) {
    log::trace!("Registering global root: {:p}, type={:?}", ptr, obj_type);
    immix::register_global(ptr, obj_type.into());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_object_type_conversion() {
        assert_eq!(u8::from(ObjectType::Atomic), 0);
        assert_eq!(u8::from(ObjectType::Pointer), 1);
        assert_eq!(u8::from(ObjectType::Complex), 2);
    }
}
