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

// ==================== 通用内存读写原语 ====================
// 提供 load/store 操作，让标准库能用纯 Karte 实现数组、哈希表等数据结构。
// 这是 "Runtime 只提供 syscall 级抽象" 设计哲学的体现。

/// 从指定地址读取 64 位值
/// addr 是字节偏移地址（number 类型）
#[no_mangle]
pub extern "C" fn karte_jit_runtime_mem_load64(addr: u64) -> u64 {
    if addr == 0 {
        return 0;
    }
    unsafe { *(addr as *const u64) }
}

/// 向指定地址写入 64 位值
/// addr 是字节偏移地址（number 类型）
#[no_mangle]
pub extern "C" fn karte_jit_runtime_mem_store64(addr: u64, value: u64) {
    if addr == 0 {
        return;
    }
    unsafe {
        *(addr as *mut u64) = value;
    }
}

/// 分配 GC 管理的内存，使用 Conservative 类型（保守扫描）
/// 用于在标准库中实现数组等数据结构
#[no_mangle]
pub extern "C" fn karte_jit_runtime_gc_alloc(size: u64) -> u64 {
    if size == 0 {
        return 0;
    }
    unsafe {
        let ptr = gc_alloc(size as usize, ObjectType::Conservative);
        if ptr.is_null() {
            return 0;
        }
        // 清零分配的内存
        std::ptr::write_bytes(ptr, 0u8, size as usize);
        ptr as u64
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

/// 字符串连接：将两个字符串连接成新字符串
/// 字符串格式：[length: i64][bytes...]
#[no_mangle]
pub extern "C" fn karte_jit_runtime_string_concat(left_ptr: u64, right_ptr: u64) -> u64 {
    unsafe {
        if left_ptr == 0 || right_ptr == 0 {
            return 0;
        }
        let left_len = *(left_ptr as *const i64) as usize;
        let right_len = *(right_ptr as *const i64) as usize;
        let total_len = left_len + right_len;
        // 8 字节头部 + 数据对齐到 8 字节
        let total_size = 8 + ((total_len + 7) & !7);

        // 先拷贝源数据到栈上的缓冲区，避免 gc_alloc 触发 GC 后指针失效
        // 栈上分配足够大的缓冲区
        let mut left_buf: Vec<u8> = Vec::with_capacity(left_len);
        let mut right_buf: Vec<u8> = Vec::with_capacity(right_len);

        let left_data = (left_ptr as *const u8).add(8);
        std::ptr::copy_nonoverlapping(left_data, left_buf.as_mut_ptr(), left_len);
        left_buf.set_len(left_len);

        let right_data = (right_ptr as *const u8).add(8);
        std::ptr::copy_nonoverlapping(right_data, right_buf.as_mut_ptr(), right_len);
        right_buf.set_len(right_len);

        // 现在安全地分配新内存（GC 可能移动 left/right 对象，但我们已经复制了数据）
        let new_ptr = gc_alloc(total_size, ObjectType::Atomic);
        if new_ptr.is_null() {
            return 0;
        }

        // 写入总长度
        *(new_ptr as *mut i64) = total_len as i64;

        // 拷贝左字符串数据
        let dest = (new_ptr as *mut u8).add(8);
        std::ptr::copy_nonoverlapping(left_buf.as_ptr(), dest, left_len);

        // 拷贝右字符串数据
        std::ptr::copy_nonoverlapping(right_buf.as_ptr(), dest.add(left_len), right_len);

        new_ptr as u64
    }
}

/// 字符取值：返回字符串中第 index 字节位置的单字节字符串
/// 字符串格式：[length: i64][bytes...]
/// GC 安全：先从源字符串读取目标字节，再分配新内存
#[no_mangle]
pub extern "C" fn karte_jit_runtime_string_char_at(str_ptr: u64, index: u64) -> u64 {
    unsafe {
        if str_ptr == 0 {
            return 0;
        }
        // 先从源字符串读取目标字节（在 gc_alloc 之前）
        let data_ptr = (str_ptr as *const u8).add(8); // 跳过 8 字节 header
        let byte_val = *data_ptr.add(index as usize);

        // 分配新字符串对象：16 字节（8字节 header + 8字节对齐数据区）
        let new_ptr = gc_alloc(16, ObjectType::Atomic);
        if new_ptr.is_null() {
            return 0;
        }

        // 写入新字符串：[length: i64 = 1][byte_val][padding zeros]
        let len_ptr = new_ptr as *mut i64;
        *len_ptr = 1;
        let data_start = new_ptr.add(8);
        *data_start = byte_val;
        // 清零剩余字节（对齐到 8 字节）
        let remaining = data_start.add(1);
        for i in 0..7 {
            *remaining.add(i) = 0;
        }

        new_ptr as u64
    }
}

/// 子字符串截取：从字符串中截取从 start 开始、长度为 length 的子串
/// 字符串格式：[length: i64][bytes...]
/// GC 安全：使用 Vec<u8> 栈缓冲区在 gc_alloc 前复制源数据
#[no_mangle]
pub extern "C" fn karte_jit_runtime_string_substring(str_ptr: u64, start: u64, length: u64) -> u64 {
    unsafe {
        if str_ptr == 0 {
            return 0;
        }
        let src_len = *(str_ptr as *const i64) as usize;
        let start = start as usize;
        let length = length as usize;

        // 边界检查
        let actual_start = if start >= src_len {
            // start 超出源字符串长度，返回空字符串
            let new_ptr = gc_alloc(16, ObjectType::Atomic);
            if new_ptr.is_null() {
                return 0;
            }
            *(new_ptr as *mut i64) = 0;
            return new_ptr as u64;
        } else {
            start
        };

        let available = src_len - actual_start;
        let actual_length = if length > available {
            available
        } else {
            length
        };

        // 先从源字符串复制数据到栈缓冲区（GC 安全）
        let src_data = (str_ptr as *const u8).add(8 + actual_start);
        let mut buf: Vec<u8> = Vec::with_capacity(actual_length);
        std::ptr::copy_nonoverlapping(src_data, buf.as_mut_ptr(), actual_length);
        buf.set_len(actual_length);

        // 分配新字符串：8 字节 header + 数据对齐到 8 字节
        let total_size = 8 + ((actual_length + 7) & !7);
        let new_ptr = gc_alloc(total_size, ObjectType::Atomic);
        if new_ptr.is_null() {
            return 0;
        }

        // 写入长度
        *(new_ptr as *mut i64) = actual_length as i64;

        // 拷贝数据
        let dest = (new_ptr as *mut u8).add(8);
        std::ptr::copy_nonoverlapping(buf.as_ptr(), dest, actual_length);

        // 清零剩余字节（对齐到 8 字节）
        let remaining = actual_length % 8;
        if remaining != 0 {
            let padding_start = dest.add(actual_length);
            for i in 0..(8 - remaining) {
                *padding_start.add(i) = 0;
            }
        }

        new_ptr as u64
    }
}

/// 字符串包含检测：检测字符串中是否包含指定 ASCII 字节值
/// 字符串格式：[length: i64][bytes...]
/// 不触发 GC（无内存分配，纯逐字节扫描）
#[no_mangle]
pub extern "C" fn karte_jit_runtime_string_contains(str_ptr: u64, char_code: u64) -> u64 {
    unsafe {
        if str_ptr == 0 {
            return 0;
        }
        let len = *(str_ptr as *const i64) as usize;
        let data_ptr = (str_ptr as *const u8).add(8);
        let target = char_code as u8;
        for i in 0..len {
            if *data_ptr.add(i) == target {
                return 1;
            }
        }
        0
    }
}

/// 字符串分割计数：统计按分隔符字节值分割后的字段数量
/// 字符串格式：[length: i64][bytes...]
/// 不触发 GC（无内存分配，纯扫描计数）
#[no_mangle]
pub extern "C" fn karte_jit_runtime_split_count(str_ptr: u64, sep_code: u64) -> u64 {
    unsafe {
        if str_ptr == 0 {
            return 1; // 空指针视为空串，1个字段
        }
        let len = *(str_ptr as *const i64) as usize;
        if len == 0 {
            return 1; // 空串是1个字段
        }
        let data_ptr = (str_ptr as *const u8).add(8);
        let sep = sep_code as u8;
        let mut count = 1; // 至少1个字段
        for i in 0..len {
            if *data_ptr.add(i) == sep {
                count += 1;
            }
        }
        count as u64
    }
}

/// 去除字符串首尾空格
/// 字符串格式：[length: i64][bytes...]
/// GC 安全：使用 Vec<u8> 栈缓冲区在 gc_alloc 前复制源数据
#[no_mangle]
pub extern "C" fn karte_jit_runtime_trim(str_ptr: u64) -> u64 {
    unsafe {
        if str_ptr == 0 {
            return 0;
        }
        let len = *(str_ptr as *const i64) as usize;
        if len == 0 {
            return str_ptr; // 空串直接返回
        }
        let data_ptr = (str_ptr as *const u8).add(8);

        // 找到第一个非空格位置
        let mut start = 0;
        while start < len && *data_ptr.add(start) == 32 {
            start += 1;
        }

        // 找到最后一个非空格位置
        let mut end = len;
        while end > start && *data_ptr.add(end - 1) == 32 {
            end -= 1;
        }

        let trimmed_len = end - start;
        if trimmed_len == 0 {
            // 全是空格，返回空字符串
            let new_ptr = gc_alloc(16, ObjectType::Atomic);
            if new_ptr.is_null() {
                return 0;
            }
            *(new_ptr as *mut i64) = 0;
            return new_ptr as u64;
        }

        // GC 安全：先复制源数据到栈缓冲区
        let mut buf: Vec<u8> = Vec::with_capacity(trimmed_len);
        std::ptr::copy_nonoverlapping(data_ptr.add(start), buf.as_mut_ptr(), trimmed_len);
        buf.set_len(trimmed_len);

        // 分配新字符串
        let total_size = 8 + ((trimmed_len + 7) & !7);
        let new_ptr = gc_alloc(total_size, ObjectType::Atomic);
        if new_ptr.is_null() {
            return 0;
        }

        // 写入长度
        *(new_ptr as *mut i64) = trimmed_len as i64;

        // 拷贝数据
        let dest = (new_ptr as *mut u8).add(8);
        std::ptr::copy_nonoverlapping(buf.as_ptr(), dest, trimmed_len);

        // 清零剩余字节
        let remaining = trimmed_len % 8;
        if remaining != 0 {
            let padding_start = dest.add(trimmed_len);
            for i in 0..(8 - remaining) {
                *padding_start.add(i) = 0;
            }
        }

        new_ptr as u64
    }
}

/// 数字转字符串：将 i64 值转换为字符串
/// 字符串格式：[length: i64][bytes...]
/// GC 安全：先 format 再 gc_alloc，format 不会触发 GC
#[no_mangle]
pub extern "C" fn karte_jit_runtime_to_string(value: i64) -> u64 {
    let s = format!("{}", value);
    let byte_len = s.len();
    let total_size = ((byte_len + 7) / 8) * 8 + 8;
    unsafe {
        let ptr = gc_alloc(total_size, ObjectType::Atomic);
        if ptr.is_null() {
            return 0;
        }
        *(ptr as *mut i64) = byte_len as i64;
        let data = ptr.add(8) as *mut u8;
        std::ptr::copy_nonoverlapping(s.as_ptr(), data, byte_len);
        let padded = ((byte_len + 7) / 8) * 8;
        std::ptr::write_bytes(data.add(byte_len), 0, padded - byte_len);
        ptr as u64
    }
}

/// ASCII 码转单字符字符串：将 ASCII 码转换为单字符字符串
/// 字符串格式：[length: i64][bytes...]
/// 固定 16 字节：8字节 header(length=1) + 8字节数据(1字节ASCII + 7字节零填充)
#[no_mangle]
pub extern "C" fn karte_jit_runtime_char_to_string(ascii_code: i64) -> u64 {
    unsafe {
        // 分配新字符串对象：16 字节（8字节 header + 8字节对齐数据区）
        let new_ptr = gc_alloc(16, ObjectType::Atomic);
        if new_ptr.is_null() {
            return 0;
        }

        // 写入新字符串：[length: i64 = 1][ascii_byte][padding zeros]
        let len_ptr = new_ptr as *mut i64;
        *len_ptr = 1;
        let data_start = new_ptr.add(8);
        *data_start = ascii_code as u8;
        // 清零剩余 7 字节
        let remaining = data_start.add(1);
        for i in 0..7 {
            *remaining.add(i) = 0;
        }

        new_ptr as u64
    }
}

/// 字符串内容比较：逐字节比较两个字符串的内容
/// 字符串格式：[length: i64][bytes...]
/// 返回 1（相等）或 0（不等）
#[no_mangle]
pub extern "C" fn karte_jit_runtime_string_equal(left_ptr: u64, right_ptr: u64) -> u64 {
    unsafe {
        // 同一指针，内容必然相同
        if left_ptr == right_ptr {
            return 1;
        }
        if left_ptr == 0 || right_ptr == 0 {
            return 0;
        }
        let left_len = *(left_ptr as *const i64) as usize;
        let right_len = *(right_ptr as *const i64) as usize;
        // 长度不同，内容必然不同
        if left_len != right_len {
            return 0;
        }
        // 逐字节比较内容
        let left_bytes = (left_ptr as *const u8).add(8);
        let right_bytes = (right_ptr as *const u8).add(8);
        for i in 0..left_len {
            if *left_bytes.add(i) != *right_bytes.add(i) {
                return 0;
            }
        }
        1
    }
}

/// 打印字符串到 stdout
/// 字符串格式：[length: i64][bytes...]
/// 返回 0（Unit）

/// 字符串比较（字典序）
/// 字符串格式：[length: i64][bytes...]
/// 返回 -1 (left < right), 0 (left == right), 1 (left > right)
#[no_mangle]
pub extern "C" fn karte_jit_runtime_string_compare(left_ptr: u64, right_ptr: u64) -> i64 {
    unsafe {
        if left_ptr == right_ptr {
            return 0;
        }
        if left_ptr == 0 {
            return -1;
        }
        if right_ptr == 0 {
            return 1;
        }
        let left_len = *(left_ptr as *const i64) as usize;
        let right_len = *(right_ptr as *const i64) as usize;
        let min_len = left_len.min(right_len);
        let left_bytes = (left_ptr as *const u8).add(8);
        let right_bytes = (right_ptr as *const u8).add(8);
        for i in 0..min_len {
            let lb = *left_bytes.add(i);
            let rb = *right_bytes.add(i);
            if lb < rb {
                return -1;
            }
            if lb > rb {
                return 1;
            }
        }
        // 公共前缀相同，比较长度
        if left_len < right_len {
            -1
        } else if left_len > right_len {
            1
        } else {
            0
        }
    }
}

/// 打印字符串到 stdout
/// 字符串格式：[length: i64][bytes...]
#[no_mangle]
pub extern "C" fn karte_jit_runtime_print_string(str_ptr: u64) -> u64 {
    unsafe {
        if str_ptr == 0 {
            return 0;
        }
        let len = *(str_ptr as *const i64) as usize;
        let data = (str_ptr as *const u8).add(8);

        if len > 0 {
            libc::write(1, data as *const libc::c_void, len);
        }
        0
    }
}

/// 打印数字到 stdout
/// 将 i64 值转换为十进制字符串并输出
/// 返回 0（Unit）
#[no_mangle]
pub extern "C" fn karte_jit_runtime_print_number(value: i64) -> u64 {
    let s = format!("{}\n", value);
    unsafe {
        libc::write(1, s.as_ptr() as *const libc::c_void, s.len());
    }
    0
}

/// 打印布尔值到 stdout
/// value != 0 输出 "true\n"，value == 0 输出 "false\n"
/// 返回 0（Unit）
#[no_mangle]
pub extern "C" fn karte_jit_runtime_print_bool(value: i64) -> u64 {
    let s = if value != 0 { "true\n" } else { "false\n" };
    unsafe {
        libc::write(1, s.as_ptr() as *const libc::c_void, s.len());
    }
    0
}

#[no_mangle]
pub extern "C" fn karte_jit_runtime_panic() {
    eprintln!("runtime error: division by zero");
    std::process::exit(134);
}

/// raw_syscall6(sysno, a1, a2, a3, a4, a5, a6) -> result
/// 通用 syscall 入口，Go 风格。所有参数和返回值都是 i64/u64。
/// std 库中用 Karte 代码封装 open/read/write/close 等。
#[no_mangle]
pub extern "C" fn karte_jit_runtime_raw_syscall6(
    sysno: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, a6: u64,
) -> u64 {
    #[cfg(target_arch = "x86_64")]
    {
        let ret: u64;
        unsafe {
            core::arch::asm!(
                "syscall",
                inlateout("rax") sysno => ret,
                in("rdi") a1,
                in("rsi") a2,
                in("rdx") a3,
                in("r10") a4,
                in("r8") a5,
                in("r9") a6,
                out("rcx") _,
                out("r11") _,
                options(nostack)
            );
        }
        ret
    }
    #[cfg(target_arch = "aarch64")]
    {
        let ret: u64;
        unsafe {
            core::arch::asm!(
                "svc #0",
                inlateout("x8") sysno => ret,
                in("x0") a1,
                in("x1") a2,
                in("x2") a3,
                in("x3") a4,
                in("x4") a5,
                in("x5") a6,
                options(nostack)
            );
        }
        ret
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        let _ = (sysno, a1, a2, a3, a4, a5, a6);
        0
    }
}
