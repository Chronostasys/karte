//! JIT内存管理器
//!
//! 基于连续内存分配策略的JIT内存管理器
//! 预分配大块连续虚拟内存，函数在其中分配子块，支持相对地址跳转

use log::{debug, error, info};
use std::collections::HashMap;

/// JIT代码段大小 (128MB，足够AArch64相对跳转范围)
const JIT_CODE_SECTION_SIZE: usize = 128 * 1024 * 1024;

/// 默认函数对齐 (16字节，适合AArch64指令对齐)
const FUNCTION_ALIGNMENT: usize = 16;

/// JIT内存管理器
///
/// 采用连续内存分配策略：
/// 1. 初始化时预分配大块连续虚拟内存
/// 2. 函数分配时从连续空间中分配子块
/// 3. 所有函数在同一地址空间，支持相对跳转
#[derive(Debug)]
pub struct JitMemoryManager {
    /// 预分配的大块内存基址
    code_section_base: *mut u8,
    /// 代码段总大小
    code_section_size: usize,
    /// 当前分配偏移（相对于base的偏移）
    current_offset: usize,
    /// 已分配的函数块信息
    allocated_functions: HashMap<String, FunctionBlock>,
    /// 调试模式
    debug_mode: bool,
    /// 是否已初始化
    initialized: bool,
}

/// 函数内存块信息
#[derive(Debug, Clone)]
struct FunctionBlock {
    /// 函数名
    name: String,
    /// 相对于代码段基址的偏移
    offset: usize,
    /// 函数大小
    size: usize,
    /// 函数入口点偏移（相对于函数开始）
    entry_offset: usize,
}

/// 可执行内存句柄（新版本）
#[derive(Debug)]
pub struct ExecutableMemory {
    /// 函数名
    function_name: String,
    /// 相对于代码段基址的偏移
    offset: usize,
    /// 内存大小
    size: usize,
    /// 入口点偏移
    entry_offset: usize,
    /// 代码段基址（用于计算绝对地址）
    base_address: *mut u8,
}

impl JitMemoryManager {
    /// 创建新的内存管理器
    pub fn new(debug_mode: bool) -> Self {
        Self {
            code_section_base: std::ptr::null_mut(),
            code_section_size: 0,
            current_offset: 0,
            allocated_functions: HashMap::new(),
            debug_mode,
            initialized: false,
        }
    }

    /// 初始化内存管理器（预分配大块连续内存）
    pub fn initialize(&mut self) -> crate::Result<()> {
        if self.initialized {
            return Ok(());
        }

        if self.debug_mode {
            info!(
                "初始化JIT内存管理器，预分配 {} MB 连续内存",
                JIT_CODE_SECTION_SIZE / (1024 * 1024)
            );
        }

        // 🔧 修复：确保分配的大小是页面对齐的
        let page_size = self.get_page_size();
        let aligned_size = self.align_size_to_page_boundary(JIT_CODE_SECTION_SIZE, page_size);

        if self.debug_mode {
            info!(
                "页面对齐: 原始大小={}MB, 对齐大小={}MB, 页面大小={}",
                JIT_CODE_SECTION_SIZE / (1024 * 1024),
                aligned_size / (1024 * 1024),
                page_size
            );
        }

        // 预分配大块连续虚拟内存
        self.code_section_base = self.allocate_virtual_memory(aligned_size)?;
        self.code_section_size = aligned_size;
        self.current_offset = 0;
        self.initialized = true;

        if self.debug_mode {
            info!(
                "JIT代码段分配成功: 基址={:p}, 大小={} MB, 页面对齐={}",
                self.code_section_base,
                self.code_section_size / (1024 * 1024),
                (self.code_section_base as usize) % page_size == 0
            );
        }

        Ok(())
    }

    /// 分配函数内存（从连续空间中分配子块）
    pub fn allocate_function_memory(
        &mut self,
        function_name: &str,
        code: &[u8],
    ) -> crate::Result<ExecutableMemory> {
        if !self.initialized {
            self.initialize()?;
        }

        let size = code.len();
        if size == 0 {
            return Err("无法分配零大小的函数内存".into());
        }

        // 计算对齐后的大小
        let aligned_size = self.align_size(size, FUNCTION_ALIGNMENT) * 2;

        // 检查是否有足够空间
        if self.current_offset + aligned_size > self.code_section_size {
            return Err(format!(
                "JIT代码段空间不足: 需要 {} 字节，剩余 {} 字节",
                aligned_size,
                self.code_section_size - self.current_offset
            ).into());
        }

        // 计算函数在代码段中的位置
        let function_offset = self.current_offset;
        let function_address = unsafe { self.code_section_base.add(function_offset) };

        if self.debug_mode {
            info!(
                "分配函数 '{}': 偏移=0x{:X}, 大小={}, 地址={:p}",
                function_name, function_offset, aligned_size, function_address
            );
        }

        // 🔧 修复：确保在页面边界上提交内存
        // 计算需要提交的页面范围（可能比函数大小大，但确保页面对齐）
        let page_size = self.get_page_size();
        let commit_start = self.align_to_page_boundary(function_address as usize, page_size);
        let commit_end = self.align_size_to_page_boundary(
            (function_address as usize - commit_start) + aligned_size,
            page_size,
        );

        if self.debug_mode {
            info!(
                "提交内存页面: 函数地址={:p}, 大小={}, 页面提交范围={:p}..{:p} ({}字节)",
                function_address,
                aligned_size,
                commit_start as *mut u8,
                (commit_start + commit_end) as *mut u8,
                commit_end
            );
        }

        // 提交这块内存的物理页面
        self.commit_memory_pages(commit_start as *mut u8, commit_end)?;

        // 复制机器码到分配的内存
        unsafe {
            std::ptr::copy_nonoverlapping(code.as_ptr(), function_address, size);

            // 清零剩余内存
            if aligned_size > size {
                std::ptr::write_bytes(function_address.add(size), 0, aligned_size - size);
            }
        }

        // 设置内存权限为可执行（使用与提交相同的页面范围）
        self.make_memory_executable(commit_start as *mut u8, commit_end)?;

        // 记录函数信息
        let function_block = FunctionBlock {
            name: function_name.to_string(),
            offset: function_offset,
            size: aligned_size,
            entry_offset: 0, // 函数入口就在开始
        };

        self.allocated_functions
            .insert(function_name.to_string(), function_block);

        // 更新当前偏移
        self.current_offset += aligned_size;

        if self.debug_mode {
            info!(
                "函数 '{}' 分配成功: 相对偏移=0x{:X}, 下一个偏移=0x{:X}",
                function_name, function_offset, self.current_offset
            );
        }

        Ok(ExecutableMemory {
            function_name: function_name.to_string(),
            offset: function_offset,
            size: aligned_size,
            entry_offset: 0,
            base_address: self.code_section_base,
        })
    }

    /// 获取函数地址（绝对地址）
    pub fn get_function_address(&self, function_name: &str) -> Option<*const u8> {
        self.allocated_functions
            .get(function_name)
            .map(|block| unsafe { self.code_section_base.add(block.offset) as *const u8 })
    }

    /// 获取函数相对偏移
    pub fn get_function_offset(&self, function_name: &str) -> Option<usize> {
        self.allocated_functions
            .get(function_name)
            .map(|block| block.offset)
    }

    /// 获取所有函数的地址映射表（用于跨函数引用解析）
    pub fn get_all_function_addresses(&self) -> HashMap<String, *const u8> {
        self.allocated_functions
            .iter()
            .map(|(name, block)| {
                let address = unsafe { self.code_section_base.add(block.offset) as *const u8 };
                (name.clone(), address)
            })
            .collect()
    }

    /// 计算两个函数之间的相对偏移（用于相对跳转）
    pub fn calculate_relative_offset(&self, from_function: &str, to_function: &str) -> Option<i64> {
        let from_offset = self.get_function_offset(from_function)?;
        let to_offset = self.get_function_offset(to_function)?;

        // 计算相对偏移
        let relative_offset = to_offset as i64 - from_offset as i64;

        if self.debug_mode {
            debug!(
                "相对跳转: {} (0x{:X}) -> {} (0x{:X}), 偏移=0x{:X}",
                from_function, from_offset, to_function, to_offset, relative_offset
            );
        }

        Some(relative_offset)
    }

    /// 清理所有内存
    pub fn cleanup(&mut self) -> crate::Result<()> {
        if self.initialized && !self.code_section_base.is_null() {
            if self.debug_mode {
                info!(
                    "清理JIT代码段: 基址={:p}, 大小={} MB",
                    self.code_section_base,
                    self.code_section_size / (1024 * 1024)
                );
            }

            self.free_virtual_memory(self.code_section_base, self.code_section_size)?;

            self.code_section_base = std::ptr::null_mut();
            self.code_section_size = 0;
            self.current_offset = 0;
            self.allocated_functions.clear();
            self.initialized = false;
        }

        Ok(())
    }

    /// 临时修改内存权限为可写（用于代码修补）
    pub fn temporarily_make_writable(&self, function_name: &str) -> crate::Result<()> {
        if let Some(block) = self.allocated_functions.get(function_name) {
            let address = unsafe { self.code_section_base.add(block.offset) };
            self.set_memory_writable(address, block.size)
        } else {
            Err(format!("未找到函数: {}", function_name).into())
        }
    }

    /// 恢复内存权限为可执行
    pub fn make_executable_again(&self, function_name: &str) -> crate::Result<()> {
        if let Some(block) = self.allocated_functions.get(function_name) {
            let address = unsafe { self.code_section_base.add(block.offset) };
            self.make_memory_executable(address, block.size)
        } else {
            Err(format!("未找到函数: {}", function_name).into())
        }
    }

    /// 获取代码段基址（用于调试和地址计算）
    pub fn get_code_section_base(&self) -> *const u8 {
        self.code_section_base as *const u8
    }

    /// 获取代码段使用统计
    pub fn get_usage_stats(&self) -> (usize, usize, usize) {
        let used = self.current_offset;
        let total = self.code_section_size;
        let functions = self.allocated_functions.len();
        (used, total, functions)
    }

    // ============= 平台相关的内存操作 =============

    /// 分配虚拟内存（不立即提交物理页面）
    fn allocate_virtual_memory(&self, size: usize) -> crate::Result<*mut u8> {
        #[cfg(target_os = "windows")]
        {
            use windows_sys::Win32::System::Memory::{VirtualAlloc, MEM_RESERVE, PAGE_NOACCESS};

            let addr = unsafe {
                VirtualAlloc(
                    std::ptr::null_mut(),
                    size,
                    MEM_RESERVE, // 只保留虚拟地址空间，不提交物理内存
                    PAGE_NOACCESS,
                )
            };

            if addr.is_null() {
                Err("VirtualAlloc(MEM_RESERVE)失败".into())
            } else {
                Ok(addr as *mut u8)
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            let addr = unsafe {
                libc::mmap(
                    std::ptr::null_mut(),
                    size,
                    libc::PROT_NONE, // 无权限，只保留地址空间
                    libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                    -1,
                    0,
                )
            };

            if addr == libc::MAP_FAILED {
                Err(format!(
                    "mmap(PROT_NONE)失败: {}",
                    std::io::Error::last_os_error()
                ).into())
            } else {
                if self.debug_mode {
                    debug!(
                        "虚拟内存分配成功: {:p}, 大小: {} MB",
                        addr,
                        size / (1024 * 1024)
                    );
                }
                Ok(addr as *mut u8)
            }
        }
    }

    /// 提交内存页面（为指定区域分配物理内存）
    fn commit_memory_pages(&self, address: *mut u8, size: usize) -> crate::Result<()> {
        #[cfg(target_os = "windows")]
        {
            use windows_sys::Win32::System::Memory::{VirtualAlloc, MEM_COMMIT, PAGE_READWRITE};

            let result = unsafe {
                VirtualAlloc(
                    address as *mut std::ffi::c_void,
                    size,
                    MEM_COMMIT, // 提交物理内存
                    PAGE_READWRITE,
                )
            };

            if result.is_null() {
                Err("VirtualAlloc(MEM_COMMIT)失败".into())
            } else {
                Ok(())
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            // 🔧 修复：确保地址和大小按页面边界对齐
            // 需要覆盖从 aligned_address 到 (address + size) 向上对齐的完整页面范围
            let page_size = self.get_page_size();
            let aligned_address = self.align_to_page_boundary(address as usize, page_size);
            let end_address = address as usize + size;
            let aligned_end = self.align_size_to_page_boundary(end_address, page_size);
            let aligned_size = aligned_end.saturating_sub(aligned_address).max(page_size);

            if self.debug_mode {
                debug!(
                    "mprotect对齐: 原始({:p}, {}), 对齐({:p}, {}), 页面大小={}",
                    address, size, aligned_address as *mut u8, aligned_size, page_size
                );
            }

            // 验证地址范围是否有效
            if aligned_address == 0 || aligned_size == 0 {
                return Err(format!("无效的内存地址或大小: {:p}, {}", address, size).into());
            }

            // 在Unix系统中，使用mprotect来提交页面并设置权限
            let result = unsafe {
                libc::mprotect(
                    aligned_address as *mut std::ffi::c_void,
                    aligned_size,
                    libc::PROT_READ | libc::PROT_WRITE,
                )
            };

            if result != 0 {
                let error = std::io::Error::last_os_error();
                Err(format!(
                    "mprotect(RW)失败: {} (地址={:p}, 大小={}, 页面大小={})",
                    error, aligned_address as *mut u8, aligned_size, page_size
                ).into())
            } else {
                if self.debug_mode {
                    debug!(
                        "内存页面提交成功: {:p}, 大小: {}",
                        aligned_address as *mut u8, aligned_size
                    );
                }
                Ok(())
            }
        }
    }

    /// 设置内存权限为可执行
    fn make_memory_executable(&self, address: *mut u8, size: usize) -> crate::Result<()> {
        #[cfg(target_os = "windows")]
        {
            use windows_sys::Win32::System::Memory::{VirtualProtect, PAGE_EXECUTE_READ};

            let mut old_protect = 0u32;
            let result = unsafe {
                VirtualProtect(
                    address as *mut std::ffi::c_void,
                    size,
                    PAGE_EXECUTE_READ,
                    &mut old_protect,
                )
            };

            if result == 0 {
                Err("VirtualProtect(EXECUTE_READ)失败".into())
            } else {
                Ok(())
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            // 🔧 修复：确保地址和大小按页面边界对齐
            // 需要覆盖从 aligned_address 到 (address + size) 向上对齐的完整页面范围
            let page_size = self.get_page_size();
            let aligned_address = self.align_to_page_boundary(address as usize, page_size);
            let end_address = address as usize + size;
            let aligned_end = self.align_size_to_page_boundary(end_address, page_size);
            let aligned_size = aligned_end.saturating_sub(aligned_address).max(page_size);

            if self.debug_mode {
                debug!(
                    "mprotect可执行对齐: 原始({:p}, {}), 对齐({:p}, {})",
                    address, size, aligned_address as *mut u8, aligned_size
                );
            }

            let result = unsafe {
                libc::mprotect(
                    aligned_address as *mut std::ffi::c_void,
                    aligned_size,
                    libc::PROT_READ | libc::PROT_EXEC,
                )
            };

            if result != 0 {
                let error = std::io::Error::last_os_error();
                Err(format!(
                    "mprotect(RX)失败: {} (地址={:p}, 大小={})",
                    error, aligned_address as *mut u8, aligned_size
                ).into())
            } else {
                // AArch64 特有：刷新指令缓存
                // I-Cache 和 D-Cache 在 AArch64 上不自动一致，
                // 写入代码内存后必须手动刷新，否则 CPU 执行旧指令
                #[cfg(target_arch = "aarch64")]
                {
                    self.flush_instruction_cache(aligned_address as *mut u8, aligned_size);
                }

                if self.debug_mode {
                    debug!(
                        "内存权限设置为可执行: {:p}, 大小: {}",
                        aligned_address as *mut u8, aligned_size
                    );
                }
                Ok(())
            }
        }
    }

    /// 刷新指令缓存（AArch64 特有）
    /// 在写入 JIT 代码后，必须刷新 I-Cache 以确保 CPU 执行最新代码
    #[cfg(target_arch = "aarch64")]
    fn flush_instruction_cache(&self, addr: *mut u8, size: usize) {
        // AArch64 cache line 大小通常是 64 字节
        const CACHE_LINE_SIZE: usize = 64;
        let start = addr as usize;
        let end = start + size;
        // 对齐到 cache line 边界
        let aligned_start = start & !(CACHE_LINE_SIZE - 1);
        let aligned_end = (end + CACHE_LINE_SIZE - 1) & !(CACHE_LINE_SIZE - 1);

        unsafe {
            // 遍历每个 cache line 执行 DC CVAU (Clean Data Cache by VA to PoU)
            let mut ptr = aligned_start;
            while ptr < aligned_end {
                // DC CVAU, X0: 清除数据缓存到 Point of Unification
                core::arch::asm!(
                    "dc cvau, {ptr}",
                    ptr = in(reg) ptr,
                );
                ptr += CACHE_LINE_SIZE;
            }
            // DSB ISH: 数据同步屏障（Inner Shareable）
            core::arch::asm!("dsb ish");

            // 遍历每个 cache line 执行 IC IVAU (Invalidate Instruction Cache by VA to PoU)
            let mut ptr = aligned_start;
            while ptr < aligned_end {
                // IC IVAU, X0: 使指令缓存无效
                core::arch::asm!(
                    "ic ivau, {ptr}",
                    ptr = in(reg) ptr,
                );
                ptr += CACHE_LINE_SIZE;
            }
            // DSB ISH: 数据同步屏障
            core::arch::asm!("dsb ish");
            // ISB: 指令同步屏障，刷新流水线
            core::arch::asm!("isb");
        }

        if self.debug_mode {
            debug!(
                "已刷新指令缓存: 范围 {:p} - {:p}, 大小 {}",
                aligned_start as *mut u8,
                aligned_end as *mut u8,
                aligned_end - aligned_start
            );
        }
    }

    /// 设置内存权限为可写
    fn set_memory_writable(&self, address: *mut u8, size: usize) -> crate::Result<()> {
        #[cfg(target_os = "windows")]
        {
            use windows_sys::Win32::System::Memory::{VirtualProtect, PAGE_READWRITE};

            let mut old_protect = 0u32;
            let result = unsafe {
                VirtualProtect(
                    address as *mut std::ffi::c_void,
                    size,
                    PAGE_READWRITE,
                    &mut old_protect,
                )
            };

            if result == 0 {
                Err("VirtualProtect(READWRITE)失败".into())
            } else {
                Ok(())
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            // 🔧 修复：确保地址和大小按页面边界对齐
            // 需要覆盖从 aligned_address 到 (address + size) 向上对齐的完整页面范围
            let page_size = self.get_page_size();
            let aligned_address = self.align_to_page_boundary(address as usize, page_size);
            let end_address = address as usize + size;
            let aligned_end = self.align_size_to_page_boundary(end_address, page_size);
            let aligned_size = aligned_end.saturating_sub(aligned_address).max(page_size);

            if self.debug_mode {
                debug!(
                    "mprotect可写对齐: 原始({:p}, {}), 对齐({:p}, {})",
                    address, size, aligned_address as *mut u8, aligned_size
                );
            }

            let result = unsafe {
                libc::mprotect(
                    aligned_address as *mut std::ffi::c_void,
                    aligned_size,
                    libc::PROT_READ | libc::PROT_WRITE,
                )
            };

            if result != 0 {
                let error = std::io::Error::last_os_error();
                Err(format!(
                    "mprotect(RW)失败: {} (地址={:p}, 大小={})",
                    error, aligned_address as *mut u8, aligned_size
                ).into())
            } else {
                if self.debug_mode {
                    debug!(
                        "内存权限设置为可写: {:p}, 大小: {}",
                        aligned_address as *mut u8, aligned_size
                    );
                }
                Ok(())
            }
        }
    }

    /// 释放虚拟内存
    fn free_virtual_memory(&self, address: *mut u8, size: usize) -> crate::Result<()> {
        #[cfg(target_os = "windows")]
        {
            use windows_sys::Win32::System::Memory::{VirtualFree, MEM_RELEASE};

            let result = unsafe { VirtualFree(address as *mut std::ffi::c_void, 0, MEM_RELEASE) };

            if result == 0 {
                Err("VirtualFree失败".into())
            } else {
                Ok(())
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            let result = unsafe { libc::munmap(address as *mut std::ffi::c_void, size) };

            if result != 0 {
                Err("munmap失败".into())
            } else {
                if self.debug_mode {
                    debug!(
                        "虚拟内存释放成功: {:p}, 大小: {} MB",
                        address,
                        size / (1024 * 1024)
                    );
                }
                Ok(())
            }
        }
    }

    /// 对齐大小到指定边界
    fn align_size(&self, size: usize, alignment: usize) -> usize {
        (size + alignment - 1) & !(alignment - 1)
    }

    /// 获取系统页面大小
    fn get_page_size(&self) -> usize {
        #[cfg(target_os = "windows")]
        {
            use windows_sys::Win32::System::SystemInformation::{GetSystemInfo, SYSTEM_INFO};
            let mut sys_info: SYSTEM_INFO = unsafe { std::mem::zeroed() };
            unsafe { GetSystemInfo(&mut sys_info) };
            sys_info.dwPageSize as usize
        }

        #[cfg(not(target_os = "windows"))]
        {
            unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize }
        }
    }

    /// 将地址对齐到页面边界（向下对齐）
    fn align_to_page_boundary(&self, address: usize, page_size: usize) -> usize {
        address & !(page_size - 1)
    }

    /// 将大小对齐到页面边界（向上对齐）
    fn align_size_to_page_boundary(&self, size: usize, page_size: usize) -> usize {
        (size + page_size - 1) & !(page_size - 1)
    }
}

impl ExecutableMemory {
    /// 获取函数的绝对地址
    pub fn address(&self) -> *mut u8 {
        unsafe { self.base_address.add(self.offset) }
    }

    /// 获取函数大小
    pub fn size(&self) -> usize {
        self.size
    }

    /// 获取函数在代码段中的偏移
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// 获取函数名
    pub fn function_name(&self) -> &str {
        &self.function_name
    }

    /// 获取函数指针（用于调用）
    pub unsafe fn as_function_ptr<F>(&self) -> crate::Result<F> {
        let function_addr = self.address().add(self.entry_offset);
        Ok(std::mem::transmute_copy(&function_addr))
    }
}

/// 实现Drop trait自动清理
impl Drop for JitMemoryManager {
    fn drop(&mut self) {
        if let Err(e) = self.cleanup() {
            error!("JIT内存管理器清理失败: {}", e);
        }
    }
}

/// 线程安全标记
unsafe impl Send for JitMemoryManager {}
unsafe impl Sync for JitMemoryManager {}

// ============= 向后兼容性方法 =============

impl JitMemoryManager {
    /// 向后兼容：分配可执行内存（旧接口）
    #[deprecated(note = "使用 allocate_function_memory 替代")]
    pub fn allocate_executable_memory(&mut self, code: &[u8]) -> crate::Result<ExecutableMemory> {
        let function_name = format!("anonymous_func_{}", self.allocated_functions.len());
        self.allocate_function_memory(&function_name, code)
    }

    /// 向后兼容：释放内存（旧接口）
    #[deprecated(note = "内存会在manager清理时自动释放")]
    pub fn deallocate_memory(&mut self, _memory: ExecutableMemory) -> crate::Result<()> {
        // 在新架构中，内存是从连续块中分配的，不需要单独释放
        // 只在整个管理器清理时一次性释放所有内存
        Ok(())
    }

    /// 向后兼容：注册函数地址（旧接口）
    #[deprecated(note = "函数地址会自动注册")]
    pub fn register_function(
        &mut self,
        _name: &str,
        _exec_mem_ptr: *const u8,
        _entry_offset: usize,
    ) {
        // 在新架构中，函数地址会在分配时自动注册
    }

    /// 向后兼容：获取函数地址（旧接口名称）
    pub fn get_function_address_compat(&self, name: &str) -> Option<*const u8> {
        self.get_function_address(name)
    }

    /// 向后兼容：全局函数地址映射表
    pub fn func_addr_map(&self) -> HashMap<String, *const u8> {
        self.get_all_function_addresses()
    }
}
