//! x86_64 Linux 原始系统调用
//!
//! 使用 `syscall` 指令直接发起系统调用。
//! 寄存器约定: rax=syscall_nr, rdi=a0, rsi=a1, rdx=a2, r10=a3, r8=a4, r9=a5
//! 返回值: rax (成功) 或 -errno (失败)

use crate::{SyscallError, SyscallResult};

// x86_64 系统调用号
const SYS_READ: u64 = 0;
const SYS_WRITE: u64 = 1;
const SYS_EXIT: u64 = 60;
const SYS_MMAP: u64 = 9;
const SYS_MUNMAP: u64 = 11;
const SYS_BRK: u64 = 12;
const SYS_EXIT_GROUP: u64 = 231;
const SYS_ARCH_PRCTL: u64 = 158;
const SYS_WRITEV: u64 = 20;

const ARCH_SET_FS: u64 = 0x1002;

/// 原始系统调用 (6 参数)
#[inline(always)]
unsafe fn syscall6(n: u64, a0: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64) -> u64 {
    let ret: u64;
    core::arch::asm!(
        "syscall",
        inlateout("rax") n => ret,
        in("rdi") a0,
        in("rsi") a1,
        in("rdx") a2,
        in("r10") a3,
        in("r8") a4,
        in("r9") a5,
        out("rcx") _,
        out("r11") _,
        options(nostack)
    );
    ret
}

/// 原始系统调用 (3 参数)
#[inline(always)]
unsafe fn syscall3(n: u64, a0: u64, a1: u64, a2: u64) -> u64 {
    syscall6(n, a0, a1, a2, 0, 0, 0)
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

/// arch_prctl(code, addr) — 架构特定操作
pub fn sys_arch_prctl(code: u64, addr: u64) -> SyscallResult {
    unsafe { check(syscall2(SYS_ARCH_PRCTL, code, addr)) }
}

/// 设置 FS 段基址 (用于线程本地存储)
pub fn sys_set_fs(addr: usize) -> SyscallResult {
    sys_arch_prctl(ARCH_SET_FS, addr as u64)
}

/// writev(fd, iov, iovcnt) — 分散写入
pub fn sys_writev(fd: usize, iov: *const u8, iovcnt: usize) -> SyscallResult {
    unsafe { check(syscall3(SYS_WRITEV, fd as u64, iov as u64, iovcnt as u64)) }
}
