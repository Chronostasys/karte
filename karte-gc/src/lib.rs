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

use std::cell::Cell;
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
    /// Trait 对象：包含单个堆指针（暂不使用）
    Trait = 1,
    /// 指针类型：单个指针或引用
    Pointer = 3,
    /// 保守类型：复杂对象，使用保守扫描（struct, closure, array, enum）
    Conservative = 4,
}

impl ObjectType {
    /// 从 u8 值创建 ObjectType
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(ObjectType::Atomic),
            1 => Some(ObjectType::Trait),
            3 => Some(ObjectType::Pointer),
            4 => Some(ObjectType::Conservative),
            _ => None,
        }
    }
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
            ObjectType::Trait => ImmixObjectType::Trait,
            ObjectType::Pointer => ImmixObjectType::Pointer,
            ObjectType::Conservative => ImmixObjectType::Conservative,
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
    let sp = immix::current_sp();
    update_virtual_stack_top(sp);
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
    let sp = immix::current_sp();
    update_virtual_stack_top(sp);
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
    let sp = immix::current_sp();
    update_virtual_stack_top(sp);
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
/// Thread-local：当前虚拟栈顶指针（对应r6寄存器的值）
///
/// 用于优化GC扫描：只扫描实际使用的栈区域，而不是整个64KB虚拟栈
thread_local! {
    static CURRENT_VIRTUAL_STACK_TOP: Cell<*const u8> = Cell::new(ptr::null());
}

/// 更新当前虚拟栈顶指针
///
/// 在FFI函数入口处调用，传入当前的r6寄存器值（虚拟SP）
///
/// # Safety
///
/// stack_top 必须是有效的虚拟栈指针
pub unsafe fn update_virtual_stack_top(stack_top: *const u8) {
    CURRENT_VIRTUAL_STACK_TOP.with(|top| {
        top.set(stack_top);
        log::trace!("Virtual stack top updated to: {:p}", stack_top);
    });
}

/// Karte 虚拟栈保守扫描器
///
/// 这个函数遍历虚拟栈区间中的所有 8 字节字，并使用启发式方法
/// 识别可能的堆指针。
///
/// 🔧 优化：使用thread-local的栈顶指针，只扫描实际使用的区域
///
/// # Safety
///
/// 这是一个 unsafe 函数，因为它直接访问原始指针。
unsafe fn karte_virtual_stack_scanner(
    stack_start: *const u8,
    stack_end: *const u8,
) -> Vec<*mut u8> {
    let mut roots = Vec::new();

    // 获取当前栈顶（r6的值）
    let current_top = CURRENT_VIRTUAL_STACK_TOP.with(|top| top.get());

    // 如果栈顶有效且在合理范围内，使用它；否则使用整个栈区间（后备方案）
    let scan_start =
        if !current_top.is_null() && current_top >= stack_start && current_top <= stack_end {
            log::debug!(
                "Using dynamic stack top: {:p} (saving {} bytes scan)",
                current_top,
                current_top as usize - stack_start as usize
            );
            current_top
        } else {
            log::debug!("Using full stack range (stack top not set or invalid)");
            stack_start
        };

    // 按 8 字节对齐遍历栈区间
    let mut current = scan_start as *const u64;
    let end = stack_end as *const u64;

    log::debug!(
        "=== Virtual Stack Scanner START: scanning {:p} - {:p} ({} bytes) ===",
        scan_start,
        stack_end,
        stack_end as usize - scan_start as usize
    );

    while current < end {
        let value = *current;

        // 启发式检查：可能是指针吗？
        if value != 0 && value % 8 == 0 {
            // 检查是否在用户空间地址范围内
            // 🔧 修复：使用 (value as isize) > 0 代替硬编码的 0x7FFFFFFFFFFF
            // x86_64 用户空间上限：0x00007FFFFFFFFFFF (128TB)
            // AArch64 用户空间上限：0x0000FFFFFFFFFFFF (256TB)
            // 内核地址特征：最高位为1（即作为 isize 时为负数）
            if value > 0x1000 && (value as isize) > 0 {
                // 🔧 修复：返回栈位置的地址（指向对象指针的指针），而不是对象指针本身
                // mark_ptr 期望接收指向对象指针的指针，它会解引用获取实际的对象指针
                log::debug!("  [ROOT] stack_loc={:p} -> heap_ptr=0x{:X}", current, value);
                roots.push(current as *mut u8);
            }
        }

        current = current.add(1);
    }

    log::debug!(
        "=== Virtual Stack Scanner END: found {} roots ===",
        roots.len()
    );

    roots
}

/// 注册虚拟栈区间作为 GC 根
///
/// Karte 使用堆上分配的虚拟栈 (Vec<i64>)，不是 C 系统栈。
/// 需要显式注册这个区间，以便 GC 能够扫描其中的根对象。
///
/// # Safety
///
/// - stack_start 和 stack_end 必须是有效的指针
/// - stack_end 必须大于 stack_start
/// - 调用者必须确保栈区间在 GC 期间保持有效
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
        "Registering Karte virtual stack as GC root: {:p} - {:p} ({} bytes)",
        stack_start,
        stack_end,
        stack_end as usize - stack_start as usize
    );

    // 调用 Immix 的自定义扫描器注册 API
    // 这会在每次 GC mark 阶段调用我们的虚拟栈扫描器
    immix::register_custom_stack_scanner(karte_virtual_stack_scanner, stack_start, stack_end);
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
        assert_eq!(u8::from(ObjectType::Trait), 1);
        assert_eq!(u8::from(ObjectType::Pointer), 3);
        assert_eq!(u8::from(ObjectType::Conservative), 4);
    }
}
