//! karte-syscall — 无 libc 依赖的原始系统调用封装
//!
//! 支持 Linux x86_64 和 AArch64 架构。
//! 所有系统调用通过内联汇编直接发起，不依赖 glibc/musl。

#![no_std]

#[cfg(target_arch = "x86_64")]
mod x86_64;
#[cfg(target_arch = "x86_64")]
pub use x86_64::*;

#[cfg(target_arch = "aarch64")]
mod aarch64;
#[cfg(target_arch = "aarch64")]
pub use aarch64::*;

/// 系统调用错误类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyscallError(pub i64);

impl SyscallError {
    pub fn code(&self) -> i64 {
        self.0
    }
}

/// 系统调用结果
pub type SyscallResult = Result<usize, SyscallError>;

// ---- Linux 系统调用号 (x86_64 / AArch64 通用常量) ----

pub const PROT_NONE: usize = 0;
pub const PROT_READ: usize = 1;
pub const PROT_WRITE: usize = 2;
pub const PROT_EXEC: usize = 4;

pub const MAP_PRIVATE: usize = 0x02;
pub const MAP_ANONYMOUS: usize = 0x20;
pub const MAP_FIXED: usize = 0x10;
pub const MAP_FAILED: usize = usize::MAX;

pub const STDOUT: usize = 1;
pub const STDERR: usize = 2;

pub const MADV_NORMAL: usize = 0;
pub const MADV_RANDOM: usize = 1;
pub const MADV_SEQUENTIAL: usize = 2;
pub const MADV_WILLNEED: usize = 3;
pub const MADV_DONTNEED: usize = 4;
pub const MADV_FREE: usize = 8;
