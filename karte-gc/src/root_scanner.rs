//! Karte 专用的根扫描器
//!
//! 负责扫描 Karte 运行时的栈帧和寄存器，找到所有 GC 根对象。
//!
//! # 扫描策略
//!
//! - **保守扫描**: 将栈上看起来像指针的值都视为潜在的根
//! - **虚拟栈集成**: 与 Karte 的虚拟栈模型配合
//! - **寄存器扫描**: 扫描所有通用寄存器

use std::ptr;

/// 栈帧信息
#[derive(Debug, Clone)]
pub struct StackFrame {
    /// 帧指针 (FP)
    pub frame_pointer: *mut u8,
    /// 栈指针 (SP)
    pub stack_pointer: *mut u8,
    /// 返回地址
    pub return_address: *mut u8,
    /// 帧大小（字节）
    pub frame_size: usize,
}

impl StackFrame {
    /// 创建新的栈帧
    pub fn new(fp: *mut u8, sp: *mut u8) -> Self {
        Self {
            frame_pointer: fp,
            stack_pointer: sp,
            return_address: ptr::null_mut(),
            frame_size: 0,
        }
    }

    /// 获取栈帧范围
    pub fn range(&self) -> (*mut u8, *mut u8) {
        (self.stack_pointer, self.frame_pointer)
    }

    /// 检查地址是否在栈帧内
    pub fn contains(&self, addr: *mut u8) -> bool {
        let (start, end) = self.range();
        addr >= start && addr < end
    }
}

/// 根扫描器
pub struct RootScanner {
    /// 全局根对象
    global_roots: Vec<*mut u8>,
    /// 是否启用详细日志
    verbose: bool,
}

impl RootScanner {
    /// 创建新的根扫描器
    pub fn new() -> Self {
        Self {
            global_roots: Vec::new(),
            verbose: false,
        }
    }

    /// 启用详细日志
    pub fn with_verbose(mut self) -> Self {
        self.verbose = true;
        self
    }

    /// 注册全局根
    pub fn register_global(&mut self, ptr: *mut u8) {
        if self.verbose {
            log::debug!("Registering global root: {:p}", ptr);
        }
        self.global_roots.push(ptr);
    }

    /// 扫描虚拟栈区间
    ///
    /// Karte 使用自己分配的虚拟栈，这个方法扫描给定的栈区间。
    ///
    /// # Arguments
    ///
    /// * `stack_start` - 虚拟栈的起始地址
    /// * `stack_end` - 虚拟栈的结束地址
    ///
    /// # Safety
    ///
    /// 调用者必须确保 stack_start 和 stack_end 指向有效的内存区间
    pub unsafe fn scan_virtual_stack(&self, stack_start: *const u8, stack_end: *const u8) -> Vec<*mut u8> {
        let mut roots = Vec::new();

        if self.verbose {
            log::debug!(
                "Scanning virtual stack: {:p} - {:p} ({} bytes)",
                stack_start,
                stack_end,
                stack_end as usize - stack_start as usize
            );
        }

        // 保守扫描：遍历栈区间中的所有字（word，8字节）
        let mut current = stack_start as *const *mut u8;
        let end = stack_end as *const *mut u8;

        while current < end {
            let potential_ptr = *current;

            // 检查是否像一个有效的堆指针
            if self.looks_like_heap_pointer(potential_ptr) {
                roots.push(potential_ptr as *mut u8);
                if self.verbose {
                    log::trace!("Found potential root at {:p} -> {:p}", current, potential_ptr);
                }
            }

            current = current.add(1);
        }

        if self.verbose {
            log::debug!("Found {} potential roots on virtual stack", roots.len());
        }

        roots
    }

    /// 扫描当前 C 系统栈（不推荐用于 Karte）
    ///
    /// **警告**：Karte 使用虚拟栈，不应该使用这个方法。
    /// 请使用 `scan_virtual_stack()` 代替。
    ///
    /// # Safety
    ///
    /// 需要正确的栈指针和帧指针
    #[deprecated(note = "Karte uses virtual stack, use scan_virtual_stack() instead")]
    pub unsafe fn scan_current_stack(&self) -> Vec<*mut u8> {
        log::warn!("scan_current_stack() is deprecated for Karte, which uses virtual stack");
        Vec::new()
    }

    /// 扫描寄存器
    ///
    /// # Safety
    ///
    /// 需要在 GC 安全点调用
    pub unsafe fn scan_registers(&self) -> Vec<*mut u8> {
        let roots = Vec::new();

        // 在保守扫描模式下，Immix 会自动扫描寄存器
        // 这里提供一个占位符实现

        if self.verbose {
            log::debug!("Register scanning delegated to Immix conservative scanner");
        }

        roots
    }

    /// 获取所有全局根
    pub fn global_roots(&self) -> &[*mut u8] {
        &self.global_roots
    }

    /// 检查一个值是否看起来像堆指针
    ///
    /// 启发式规则：
    /// - 非空
    /// - 对齐到 8 字节
    /// - 在合理的地址范围内
    fn looks_like_heap_pointer(&self, ptr: *mut u8) -> bool {
        if ptr.is_null() {
            return false;
        }

        // 检查对齐
        if (ptr as usize) % 8 != 0 {
            return false;
        }

        // 检查是否在用户空间地址范围
        let addr = ptr as usize;

        #[cfg(target_pointer_width = "64")]
        {
            // 64位系统：用户空间通常在低地址
            // 避免内核空间地址 (通常 > 0x0000_7fff_ffff_ffff)
            if addr > 0x0000_7fff_ffff_ffff {
                return false;
            }
        }

        // 避免明显的无效地址
        if addr < 0x1000 {
            return false;
        }

        true
    }
}

impl Default for RootScanner {
    fn default() -> Self {
        Self::new()
    }
}

/// 栈帧迭代器
///
/// 用于遍历调用栈中的所有栈帧
pub struct StackFrameIterator {
    current_fp: *mut *mut u8,
    stack_bottom: *mut u8,
}

impl StackFrameIterator {
    /// 创建新的栈帧迭代器
    ///
    /// # Safety
    ///
    /// 需要有效的帧指针和栈底地址
    pub unsafe fn new(fp: *mut *mut u8, stack_bottom: *mut u8) -> Self {
        Self {
            current_fp: fp,
            stack_bottom,
        }
    }

    /// 获取下一个栈帧
    ///
    /// # Safety
    ///
    /// 需要确保栈帧链未被破坏
    pub unsafe fn next_frame(&mut self) -> Option<StackFrame> {
        if self.current_fp.is_null() || self.current_fp as *mut u8 >= self.stack_bottom {
            return None;
        }

        // 栈帧布局（AArch64/x86_64 标准）：
        // [fp] -> 前一个帧的 fp
        // [fp+8] -> 返回地址
        let prev_fp = *self.current_fp;
        let return_addr = *(self.current_fp.add(1));

        let frame = StackFrame {
            frame_pointer: self.current_fp as *mut u8,
            stack_pointer: self.current_fp as *mut u8, // 简化：使用 fp 作为 sp
            return_address: return_addr as *mut u8,
            frame_size: 0,
        };

        self.current_fp = prev_fp as *mut *mut u8;

        Some(frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stack_frame_creation() {
        let sp = 0x1000 as *mut u8;
        let fp = 0x2000 as *mut u8;

        let frame = StackFrame::new(fp, sp);

        assert_eq!(frame.stack_pointer, sp);
        assert_eq!(frame.frame_pointer, fp);
    }

    #[test]
    fn test_looks_like_heap_pointer() {
        let scanner = RootScanner::new();

        // 空指针
        assert!(!scanner.looks_like_heap_pointer(ptr::null_mut()));

        // 未对齐的指针
        assert!(!scanner.looks_like_heap_pointer(0x1001 as *mut u8));

        // 有效的指针
        assert!(scanner.looks_like_heap_pointer(0x10000 as *mut u8));
    }
}
