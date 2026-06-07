//! AArch64 Linux 原始系统调用
//!
//! 使用 `svc #0` 指令直接发起系统调用。
//! 寄存器约定: x8=syscall_nr, x0-x5=参数
//! 返回值: x0 (成功) 或 -errno (失败)

use crate::{SyscallError, SyscallResult};

// AArch64 系统调用号 (与 x86_64 不同!)
const SYS_READ: u64 = 63;
const SYS_WRITE: u64 = 64;
const SYS_OPENAT: u64 = 56;
const SYS_CLOSE: u64 = 57;
const SYS_EXIT: u64 = 93;
const SYS_MMAP: u64 = 222;
const SYS_MUNMAP: u64 = 215;
const SYS_BRK: u64 = 214;
const SYS_EXIT_GROUP: u64 = 94;

/// 原始系统调用 (6 参数)
#[inline(always)]
unsafe fn syscall6(n: u64, a0: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64) -> u64 {
    let ret: u64;
    core::arch::asm!(
        "svc #0",
        in("x8") n,
        inlateout("x0") a0 => ret,
        in("x1") a1,
        in("x2") a2,
        in("x3") a3,
        in("x4") a4,
        in("x5") a5,
        options(nostack)
    );
    ret
}

/// 原始系统调用 (3 参数)
#[inline(always)]
unsafe fn syscall3(n: u64, a0: u64, a1: u64, a2: u64) -> u64 {
    syscall6(n, a0, a1, a2, 0, 0, 0)
}

/// 原始系统调用 (4 参数)
#[inline(always)]
unsafe fn syscall4(n: u64, a0: u64, a1: u64, a2: u64, a3: u64) -> u64 {
    syscall6(n, a0, a1, a2, a3, 0, 0)
}

/// 原始系统调用 (2 参数)
#[inline(always)]
unsafe fn syscall2(n: u64, a0: u64, a1: u64) -> u64 {
    syscall6(n, a0, a1, 0, 0, 0, 0)
}

/// 原始系统调用 (1 参数)
#[inline(always)]
unsafe fn syscall1(n: u64, a0: u64) -> u64 {
    syscall6(n, a0, 0, 0, 0, 0, 0)
}

/// 检查系统调用返回值
fn check(ret: u64) -> SyscallResult {
    let signed = ret as i64;
    if signed < 0 && signed >= -4096 {
        Err(SyscallError(-signed))
    } else {
        Ok(ret as usize)
    }
}

/// write(fd, buf, count) — 写入数据到文件描述符
pub fn sys_write(fd: usize, buf: *const u8, count: usize) -> SyscallResult {
    unsafe { check(syscall3(SYS_WRITE, fd as u64, buf as u64, count as u64)) }
}

/// read(fd, buf, count) — 从文件描述符读取数据
pub fn sys_read(fd: usize, buf: *mut u8, count: usize) -> SyscallResult {
    unsafe { check(syscall3(SYS_READ, fd as u64, buf as u64, count as u64)) }
}

/// openat(dirfd, path, flags, mode) — 打开文件 (AArch64 使用 openat)
pub fn sys_open(path: *const u8, flags: i32, mode: u32) -> SyscallResult {
    unsafe { check(syscall4(SYS_OPENAT, 0xFFFFFFFFFFFFFFFFu64, path as u64, flags as u64, mode as u64)) }
}

/// close(fd) — 关闭文件描述符
pub fn sys_close(fd: i32) -> SyscallResult {
    unsafe { check(syscall1(SYS_CLOSE, fd as u64)) }
}

/// exit(code) — 退出进程
pub fn sys_exit(code: i32) -> ! {
    unsafe { syscall1(SYS_EXIT, code as u64); }
    unreachable!()
}

/// exit_group(code) — 退出所有线程
pub fn sys_exit_group(code: i32) -> ! {
    unsafe { syscall1(SYS_EXIT_GROUP, code as u64); }
    unreachable!()
}

/// mmap(addr, len, prot, flags, fd, offset) — 内存映射
pub fn sys_mmap(addr: usize, len: usize, prot: usize, flags: usize, fd: i32, offset: usize) -> SyscallResult {
    unsafe {
        check(syscall6(
            SYS_MMAP,
            addr as u64,
            len as u64,
            prot as u64,
            flags as u64,
            fd as u64,
            offset as u64,
        ))
    }
}

/// munmap(addr, len) — 取消内存映射
pub fn sys_munmap(addr: usize, len: usize) -> SyscallResult {
    unsafe { check(syscall2(SYS_MUNMAP, addr as u64, len as u64)) }
}

/// brk(addr) — 设置程序断点
pub fn sys_brk(addr: usize) -> SyscallResult {
    unsafe { check(syscall1(SYS_BRK, addr as u64)) }
}
