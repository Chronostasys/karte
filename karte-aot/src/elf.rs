//! ELF64 可执行文件生成器
//!
//! 生成最小化的 ELF64 可执行文件，支持 Linux x86_64 和 AArch64。
//! 不依赖任何外部链接器，直接输出可执行二进制。

use std::io::{self, Write};

// ELF 常量
const ELFMAG: [u8; 4] = [0x7f, b'E', b'L', b'F'];
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1; // Little endian
const EV_CURRENT: u8 = 1;
const ELFOSABI_NONE: u8 = 0;

const ET_EXEC: u16 = 2;
const EM_X86_64: u16 = 62;
const EM_AARCH64: u16 = 183;
const EM_RISCV: u16 = 243;

const PT_LOAD: u32 = 1;
const PF_X: u32 = 1;
const PF_W: u32 = 2;
const PF_R: u32 = 4;

/// ELF64 文件头 (64 bytes)
#[repr(C, packed)]
struct Elf64Ehdr {
    e_ident: [u8; 16],
    e_type: u16,
    e_machine: u16,
    e_version: u32,
    e_entry: u64,
    e_phoff: u64,
    e_shoff: u64,
    e_flags: u32,
    e_ehsize: u16,
    e_phentsize: u16,
    e_phnum: u16,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
}

/// ELF64 程序头 (56 bytes)
#[repr(C, packed)]
struct Elf64Phdr {
    p_type: u32,
    p_flags: u32,
    p_offset: u64,
    p_vaddr: u64,
    p_paddr: u64,
    p_filesz: u64,
    p_memsz: u64,
    p_align: u64,
}

/// 目标架构
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElfArch {
    X86_64,
    AArch64,
    Riscv64,
}

/// ELF 生成器
pub struct ElfWriter {
    arch: ElfArch,
    /// 代码段 (可读+可执行)
    code_segment: Vec<u8>,
    /// 数据段 (可读+可写)
    data_segment: Vec<u8>,
    /// 代码段中 _start 的偏移
    entry_offset: usize,
    /// 代码段基地址 (虚拟地址)
    code_base_addr: u64,
    /// 数据段基地址 (虚拟地址)
    data_base_addr: u64,
}

impl ElfWriter {
    /// 创建新的 ELF 生成器
    ///
    /// - `arch`: 目标架构
    /// - `code_base_addr`: 代码段虚拟地址 (建议 0x400000)
    /// - `data_base_addr`: 数据段虚拟地址 (建议 0x800000)
    pub fn new(arch: ElfArch, code_base_addr: u64, data_base_addr: u64) -> Self {
        Self {
            arch,
            code_segment: Vec::new(),
            data_segment: Vec::new(),
            entry_offset: 0,
            code_base_addr,
            data_base_addr,
        }
    }

    /// 设置入口点偏移 (在代码段中的偏移)
    pub fn set_entry_offset(&mut self, offset: usize) {
        self.entry_offset = offset;
    }

    /// 追加代码到代码段，返回偏移量
    pub fn append_code(&mut self, code: &[u8]) -> usize {
        let offset = self.code_segment.len();
        self.code_segment.extend_from_slice(code);
        offset
    }

    /// 追加数据到数据段，返回偏移量
    pub fn append_data(&mut self, d: &[u8]) -> usize {
        let offset = self.data_segment.len();
        self.data_segment.extend_from_slice(d);
        offset
    }

    /// 获取代码段当前大小
    pub fn code_len(&self) -> usize {
        self.code_segment.len()
    }

    /// 获取数据段当前大小
    pub fn data_len(&self) -> usize {
        self.data_segment.len()
    }

    /// 获取代码段可变引用 (用于 patching)
    pub fn code_mut(&mut self) -> &mut Vec<u8> {
        &mut self.code_segment
    }

    /// 获取代码段基地址
    pub fn code_base(&self) -> u64 {
        self.code_base_addr
    }

    /// 在代码段指定偏移写入 u64 (小端序)
    pub fn patch_u64_at(&mut self, offset: usize, value: u64) {
        let bytes = value.to_le_bytes();
        self.code_segment[offset..offset + 8].copy_from_slice(&bytes);
    }

    /// 对齐代码段到指定对齐边界，返回对齐后的偏移
    pub fn align_code(&mut self, alignment: usize) -> usize {
        let current = self.code_segment.len();
        let padding = (alignment - (current % alignment)) % alignment;
        self.code_segment.extend(std::iter::repeat(0).take(padding));
        self.code_segment.len()
    }

    /// 生成完整的 ELF 可执行文件
    pub fn generate(&self) -> io::Result<Vec<u8>> {
        let ehdr_size = 64;
        let phdr_size = 56;
        let num_phdrs = if self.data_segment.is_empty() { 1 } else { 2 };

        // 段头紧跟文件头
        let phdr_offset = ehdr_size;
        let headers_total = ehdr_size + phdr_size * num_phdrs;

        // 代码段在文件中的偏移 (页对齐)
        let page_size: u64 = 0x1000;
        let code_file_offset = align_up(headers_total as u64, page_size);

        // 数据段紧跟代码段 (页对齐)
        let data_file_offset = code_file_offset + align_up(self.code_segment.len() as u64, page_size);

        // 虚拟地址
        let code_vaddr = self.code_base_addr;
        let data_vaddr = self.data_base_addr;
        let entry_vaddr = code_vaddr + self.entry_offset as u64;

        let e_machine = match self.arch {
            ElfArch::X86_64 => EM_X86_64,
            ElfArch::AArch64 => EM_AARCH64,
            ElfArch::Riscv64 => EM_RISCV,
        };

        // RISC-V 特殊 flags: 0x0 = RV64I, 0x5 = RVC (压缩指令)
        let e_flags = match self.arch {
            ElfArch::Riscv64 => 0x0,
            _ => 0,
        };

        // 构建 ELF header
        let mut ehdr = Elf64Ehdr {
            e_ident: [0; 16],
            e_type: ET_EXEC,
            e_machine,
            e_version: EV_CURRENT as u32,
            e_entry: entry_vaddr,
            e_phoff: phdr_offset as u64,
            e_shoff: 0, // 无段头
            e_flags,
            e_ehsize: ehdr_size as u16,
            e_phentsize: phdr_size as u16,
            e_phnum: num_phdrs as u16,
            e_shentsize: 0,
            e_shnum: 0,
            e_shstrndx: 0,
        };
        ehdr.e_ident[0..4].copy_from_slice(&ELFMAG);
        ehdr.e_ident[4] = ELFCLASS64;
        ehdr.e_ident[5] = ELFDATA2LSB;
        ehdr.e_ident[6] = EV_CURRENT;
        ehdr.e_ident[7] = ELFOSABI_NONE;

        // 构建 PT_LOAD for code
        let code_phdr = Elf64Phdr {
            p_type: PT_LOAD,
            p_flags: PF_R | PF_W | PF_X,
            p_offset: code_file_offset,
            p_vaddr: code_vaddr,
            p_paddr: code_vaddr,
            p_filesz: self.code_segment.len() as u64,
            p_memsz: self.code_segment.len() as u64,
            p_align: page_size,
        };

        // 构建 PT_LOAD for data
        let data_phdr = Elf64Phdr {
            p_type: PT_LOAD,
            p_flags: PF_R | PF_W,
            p_offset: data_file_offset,
            p_vaddr: data_vaddr,
            p_paddr: data_vaddr,
            p_filesz: self.data_segment.len() as u64,
            p_memsz: self.data_segment.len() as u64, // TODO: 可扩展
            p_align: page_size,
        };

        // 组装输出
        let total_size = data_file_offset as usize
            + if self.data_segment.is_empty() { 0 } else { self.data_segment.len() };
        let mut output = Vec::with_capacity(total_size);

        // 写入 ELF header
        output.extend_from_slice(unsafe {
            std::slice::from_raw_parts(
                &ehdr as *const Elf64Ehdr as *const u8,
                std::mem::size_of::<Elf64Ehdr>(),
            )
        });

        // 写入 program headers
        output.extend_from_slice(unsafe {
            std::slice::from_raw_parts(
                &code_phdr as *const Elf64Phdr as *const u8,
                std::mem::size_of::<Elf64Phdr>(),
            )
        });

        if num_phdrs > 1 {
            output.extend_from_slice(unsafe {
                std::slice::from_raw_parts(
                    &data_phdr as *const Elf64Phdr as *const u8,
                    std::mem::size_of::<Elf64Phdr>(),
                )
            });
        }

        // 填充到代码段偏移
        while output.len() < code_file_offset as usize {
            output.push(0);
        }

        // 写入代码段
        output.extend_from_slice(&self.code_segment);

        // 填充到数据段偏移
        if !self.data_segment.is_empty() {
            while output.len() < data_file_offset as usize {
                output.push(0);
            }
            output.extend_from_slice(&self.data_segment);
        }

        Ok(output)
    }
}

fn align_up(value: u64, alignment: u64) -> u64 {
    (value + alignment - 1) & !(alignment - 1)
}
