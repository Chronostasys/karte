//! x86_64 AOT 运行时代码生成（带三色标记 GC）
//!
//! 生成最小化运行时，使用原始系统调用，不依赖 libc。
//! 包含三色标记-清除-压缩 GC。

/// 运行时函数名常量
///
/// 所有 runtime 函数名在此统一定义，compiler.rs 和 runtime_x86.rs 共用。
/// 保证 find_offset 查找的名字和 fn_start 注册的名字永远一致。
/// 新增 runtime 函数时必须在此添加常量，否则编译失败。
pub mod runtime_names {
    pub const START: &str = "_start";
    pub const GC_ALLOC_ALIGNED: &str = "__karte_gc_alloc_aligned";
    pub const GC_COLLECT: &str = "__karte_gc_collect";
    pub const GC_SAFEPOINT: &str = "__karte_gc_safepoint";
    pub const GC_UPDATE_STACK_TOP: &str = "__karte_gc_update_stack_top";
    pub const FREE: &str = "__karte_free";
    pub const RETAIN: &str = "__karte_retain";
    pub const RELEASE: &str = "__karte_release";
    pub const STRING_EQUAL: &str = "__karte_string_equal";
    pub const STRING_CONCAT: &str = "__karte_string_concat";
    pub const STRING_CHAR_AT: &str = "__karte_string_char_at";
    pub const TO_STRING: &str = "__karte_to_string";
    pub const TRIM: &str = "__karte_string_trim";
    pub const PRINT_STRING: &str = "__karte_print_string";
    pub const PRINT_NUMBER: &str = "__karte_print_number";
    pub const PRINT_BOOL: &str = "__karte_print_bool";
    pub const PANIC: &str = "__karte_panic";
}

/// 运行时函数描述
#[derive(Debug, Clone)]
pub struct RuntimeFunction {
    pub name: String,
    pub offset: usize,
    pub size: usize,
}

/// 三色标记颜色常量
const COLOR_WHITE: u8 = 0;
const COLOR_GRAY: u8 = 1;
const COLOR_BLACK: u8 = 2;

/// 对象头大小 (8 字节)
const GC_HEADER_SIZE: u64 = 8;

/// GC 触发阈值（每次分配后检查，超过此数触发 GC）
const GC_THRESHOLD: u64 = 256;

/// x86_64 运行时
#[derive(Debug, Clone)]
pub struct X86Runtime {
    pub code: Vec<u8>,
    pub functions: Vec<RuntimeFunction>,
    /// _start 中 call main 的 rel32 修补偏移
    pub call_main_rel32_offset: usize,
}

impl X86Runtime {
    pub fn new() -> Self {
        Self {
            code: Vec::new(),
            functions: Vec::new(),
            call_main_rel32_offset: 0,
        }
    }

    pub fn find_offset(&self, name: &str) -> Option<usize> {
        self.functions.iter().find(|f| f.name == name).map(|f| f.offset)
    }

    pub fn generate(mut self) -> Self {
        self.emit_start();
        self.emit_gc_alloc();
        self.emit_gc_collect();
        self.emit_gc_safepoint();
        self.emit_gc_update_stack_top();
        self.emit_free();
        self.emit_retain();
        self.emit_release();
        self.emit_string_equal();
        self.emit_string_concat();
        self.emit_string_char_at();
        self.emit_trim();
        self.emit_to_string();
        self.emit_print_string();
        self.emit_print_number();
        self.emit_print_bool();
        self.emit_panic();
        self
    }

    fn fn_start(&mut self, name: &str) {
        let offset = self.code.len();
        self.functions.push(RuntimeFunction { name: name.to_string(), offset, size: 0 });
    }
    fn fn_end(&mut self) {
        if let Some(f) = self.functions.last_mut() {
            f.size = self.code.len() - f.offset;
        }
    }

    // ================ 指令编码辅助 ================

    fn b(&mut self, x: u8) { self.code.push(x); }
    fn bs(&mut self, xs: &[u8]) { self.code.extend_from_slice(xs); }
    fn u32(&mut self, v: u32) { self.bs(&v.to_le_bytes()); }
    fn u64(&mut self, v: u64) { self.bs(&v.to_le_bytes()); }

    fn reg_ext(reg: u8) -> bool { reg >= 8 }

    fn rex(&mut self, r: bool, x: bool, b: bool) {
        let mut p: u8 = 0x48;
        if r { p |= 0x04; }
        if x { p |= 0x02; }
        if b { p |= 0x01; }
        self.b(p);
    }

    /// MOV r64, imm64
    fn mov_ri(&mut self, rd: u8, imm: u64) {
        self.rex(false, false, Self::reg_ext(rd));
        self.b(0xB8 | (rd & 7));
        self.u64(imm);
    }

    /// MOV r64, r64
    fn mov_rr(&mut self, dst: u8, src: u8) {
        self.rex(Self::reg_ext(src), false, Self::reg_ext(dst));
        self.b(0x89);
        self.b(0xC0 | ((src & 7) << 3) | (dst & 7));
    }

    /// MOV r64, [rip + disp32]
    fn mov_rip_load(&mut self, rd: u8, disp: i32) {
        self.rex(Self::reg_ext(rd), false, false);
        self.b(0x8B);
        self.b(0x05 | ((rd & 7) << 3));
        self.u32(disp as u32);
    }

    /// MOV [rip + disp32], r64
    fn mov_rip_store(&mut self, rs: u8, disp: i32) {
        self.rex(Self::reg_ext(rs), false, false);
        self.b(0x89);
        self.b(0x05 | ((rs & 7) << 3));
        self.u32(disp as u32);
    }

    /// MOV r64, [r64 + disp32]
    fn mov_mem_load(&mut self, rd: u8, base: u8, disp: i32) {
        self.rex(Self::reg_ext(rd), false, Self::reg_ext(base));
        self.b(0x8B);
        self.emit_modrm_offset(rd, base, disp);
    }

    /// MOV [r64 + disp32], r64
    fn mov_mem_store(&mut self, base: u8, disp: i32, rs: u8) {
        self.rex(Self::reg_ext(rs), false, Self::reg_ext(base));
        self.b(0x89);
        self.emit_modrm_offset(rs, base, disp);
    }

    /// Emit ModR/M with displacement
    /// 处理 x86_64 特殊情况:
    /// - base_field == 4 (RSP/R12) 需要 SIB 字节
    /// - base_field == 5 (RBP/R13) + disp == 0 必须用 disp8=0 模式
    fn emit_modrm_offset(&mut self, reg: u8, base: u8, disp: i32) {
        let reg_field = (reg & 7) << 3;
        let base_field = base & 7;
        // RSP/R12 (base_field == 4) 需要 SIB 字节: scale=0, index=RSP(none), base=RSP/R12
        let needs_sib = base_field == 4;
        if disp == 0 && base_field != 5 && !needs_sib {
            self.b(0x00 | reg_field | base_field);
        } else if needs_sib && disp == 0 {
            // mod=00, reg, r/m=100(SIB), SIB=0x24 (no index, base=RSP/R12)
            self.b(0x00 | reg_field | 0x04);
            self.b(0x24); // SIB: scale=0, index=100(none), base=100(RSP/R12)
        } else if needs_sib && disp >= -128 && disp <= 127 {
            // mod=01, reg, r/m=100(SIB), SIB=0x24, disp8
            self.b(0x40 | reg_field | 0x04);
            self.b(0x24);
            self.b(disp as u8);
        } else if needs_sib {
            // mod=10, reg, r/m=100(SIB), SIB=0x24, disp32
            self.b(0x80 | reg_field | 0x04);
            self.b(0x24);
            self.u32(disp as u32);
        } else if disp >= -128 && disp <= 127 {
            self.b(0x40 | reg_field | base_field);
            self.b(disp as u8);
        } else {
            self.b(0x80 | reg_field | base_field);
            self.u32(disp as u32);
        }
    }

    /// ADD r64, r64
    fn add_rr(&mut self, dst: u8, src: u8) {
        self.rex(Self::reg_ext(src), false, Self::reg_ext(dst));
        self.b(0x01);
        self.b(0xC0 | ((src & 7) << 3) | (dst & 7));
    }

    /// SUB r64, r64
    fn sub_rr(&mut self, dst: u8, src: u8) {
        self.rex(Self::reg_ext(src), false, Self::reg_ext(dst));
        self.b(0x29);
        self.b(0xC0 | ((src & 7) << 3) | (dst & 7));
    }

    /// CMP r64, r64
    fn cmp_rr(&mut self, r1: u8, r2: u8) {
        self.rex(Self::reg_ext(r2), false, Self::reg_ext(r1));
        self.b(0x39);
        self.b(0xC0 | ((r2 & 7) << 3) | (r1 & 7));
    }

    /// CMP [r64+disp], imm8
    fn cmp_mem_imm8(&mut self, base: u8, disp: i32, imm: u8) {
        self.rex(false, false, Self::reg_ext(base));
        self.b(0x83);
        self.emit_modrm_offset(7, base, disp); // reg=7 for CMP
        self.b(imm);
    }

    /// TEST r64, r64 (AND without storing, sets flags)
    fn test_rr(&mut self, r1: u8, r2: u8) {
        self.rex(Self::reg_ext(r2), false, Self::reg_ext(r1));
        self.b(0x85);
        self.b(0xC0 | ((r2 & 7) << 3) | (r1 & 7));
    }

    /// AND r64, imm32 (sign-extended)
    fn and_ri32(&mut self, rd: u8, imm: u32) {
        self.rex(false, false, Self::reg_ext(rd));
        self.b(0x81);
        self.b(0xE0 | (rd & 7));
        self.u32(imm);
    }

    /// ADD r64, imm8
    fn add_ri8(&mut self, rd: u8, imm: u8) {
        self.rex(false, false, Self::reg_ext(rd));
        self.b(0x83);
        self.b(0xC0 | (rd & 7));
        self.b(imm);
    }

    /// SUB r64, imm8
    fn sub_ri8(&mut self, rd: u8, imm: u8) {
        self.rex(false, false, Self::reg_ext(rd));
        self.b(0x83);
        self.b(0xE8 | (rd & 7));
        self.b(imm);
    }

    /// XOR r64, r64 (zero register)
    fn xor_rr(&mut self, dst: u8, src: u8) {
        self.rex(Self::reg_ext(src), false, Self::reg_ext(dst));
        self.b(0x31);
        self.b(0xC0 | ((src & 7) << 3) | (dst & 7));
    }

    /// SHL r64, imm8
    fn shl_ri8(&mut self, rd: u8, imm: u8) {
        self.rex(false, false, Self::reg_ext(rd));
        self.bs(&[0xC1, 0xE0 | (rd & 7), imm]);
    }

    /// MOV byte [r64+disp], imm8
    fn mov_byte_mem_imm(&mut self, base: u8, disp: i32, imm: u8) {
        // No REX.W for byte operation
        if Self::reg_ext(base) { self.b(0x41); }
        self.b(0xC6);
        self.emit_modrm_offset_rm(0, base, disp);
        self.b(imm);
    }

    /// Emit ModR/M for r/m only (reg=0)
    /// 处理 x86_64 特殊情况: base_field == 4 (RSP/R12) 需要 SIB 字节
    fn emit_modrm_offset_rm(&mut self, _reg: u8, base: u8, disp: i32) {
        let base_field = base & 7;
        let needs_sib = base_field == 4;
        if disp == 0 && base_field != 5 && !needs_sib {
            self.b(0x00 | base_field);
        } else if needs_sib && disp == 0 {
            self.b(0x00 | 0x04); // mod=00, r/m=100(SIB)
            self.b(0x24); // SIB
        } else if needs_sib && disp >= -128 && disp <= 127 {
            self.b(0x40 | 0x04);
            self.b(0x24);
            self.b(disp as u8);
        } else if needs_sib {
            self.b(0x80 | 0x04);
            self.b(0x24);
            self.u32(disp as u32);
        } else if disp >= -128 && disp <= 127 {
            self.b(0x40 | base_field);
            self.b(disp as u8);
        } else {
            self.b(0x80 | base_field);
            self.u32(disp as u32);
        }
    }

    /// MOV dword [r64+disp], imm32
    fn mov_dword_mem_imm(&mut self, base: u8, disp: i32, imm: u32) {
        if Self::reg_ext(base) { self.b(0x41); }
        self.b(0xC7);
        self.emit_modrm_offset_rm(0, base, disp);
        self.u32(imm);
    }

    /// MOVZX r64, byte [r64+disp]
    fn movzx_byte(&mut self, rd: u8, base: u8, disp: i32) {
        self.rex(Self::reg_ext(rd), false, Self::reg_ext(base));
        self.bs(&[0x0F, 0xB6]);
        self.emit_modrm_offset(rd, base, disp);
    }

    /// JE rel32
    fn je_rel32(&mut self, rel: i32) {
        self.bs(&[0x0F, 0x84]);
        self.u32(rel as u32);
    }

    /// JZ rel32 (same as JE)
    fn jz_rel32(&mut self, rel: i32) {
        self.je_rel32(rel);
    }

    /// JNE rel32
    fn jne_rel32(&mut self, rel: i32) {
        self.bs(&[0x0F, 0x85]);
        self.u32(rel as u32);
    }

    /// JA rel32
    fn ja_rel32(&mut self, rel: i32) {
        self.bs(&[0x0F, 0x87]);
        self.u32(rel as u32);
    }

    /// JAE rel32
    fn jae_rel32(&mut self, rel: i32) {
        self.bs(&[0x0F, 0x83]);
        self.u32(rel as u32);
    }

    /// JGE rel32 (有符号大于等于, SF=OF)
    fn jge_rel32(&mut self, rel: i32) {
        self.bs(&[0x0F, 0x8D]);
        self.u32(rel as u32);
    }

    /// JB rel32
    fn jb_rel32(&mut self, rel: i32) {
        self.bs(&[0x0F, 0x82]);
        self.u32(rel as u32);
    }

    /// JBE rel32
    fn jbe_rel32(&mut self, rel: i32) {
        self.bs(&[0x0F, 0x86]);
        self.u32(rel as u32);
    }

    /// JL rel32 (有符号小于, SF!=OF)
    fn jl_rel32(&mut self, rel: i32) {
        self.bs(&[0x0F, 0x8C]);
        self.u32(rel as u32);
    }

    /// JMP rel32
    fn jmp_rel32(&mut self, rel: i32) {
        self.b(0xE9);
        self.u32(rel as u32);
    }

    /// CALL rel32
    fn call_rel32(&mut self, rel: i32) {
        self.b(0xE8);
        self.u32(rel as u32);
    }

    /// LEA r64, [rip + disp32]
    fn lea_rip(&mut self, rd: u8, disp: i32) {
        self.rex(Self::reg_ext(rd), false, false);
        self.b(0x8D);
        self.b(0x05 | ((rd & 7) << 3));
        self.u32(disp as u32);
    }

    fn push(&mut self, r: u8) {
        if Self::reg_ext(r) { self.b(0x41); }
        self.b(0x50 | (r & 7));
    }
    fn pop(&mut self, r: u8) {
        if Self::reg_ext(r) { self.b(0x41); }
        self.b(0x58 | (r & 7));
    }
    fn ret(&mut self) { self.b(0xC3); }
    fn syscall(&mut self) { self.bs(&[0x0F, 0x05]); }
    fn nop(&mut self) { self.b(0x90); }

    /// MOV [base], r64 (indirect store, handles RSP/RBP/R13 special cases)
    fn mov_indirect_store(&mut self, base: u8, src: u8) {
        self.rex(Self::reg_ext(src), false, Self::reg_ext(base));
        self.b(0x89);
        if (base & 7) == 5 {
            // RBP/R13: mod=01 + disp8=0（mod=00+r/m=5 会被解释为 RIP-relative）
            self.b(0x40 | ((src & 7) << 3) | (base & 7));
            self.b(0x00);
        } else if (base & 7) == 4 {
            // RSP/R12: 需要 SIB 字节
            self.b(0x00 | ((src & 7) << 3) | (base & 7));
            self.b(0x24);
        } else {
            self.b(0x00 | ((src & 7) << 3) | (base & 7));
        }
    }

    /// MOV r64, [base] (indirect load, handles RSP/RBP/R13 special cases)
    fn mov_indirect_load(&mut self, dst: u8, base: u8) {
        self.rex(Self::reg_ext(dst), false, Self::reg_ext(base));
        self.b(0x8B);
        if (base & 7) == 5 {
            // RBP/R13: mod=01 + disp8=0
            self.b(0x40 | ((dst & 7) << 3) | (base & 7));
            self.b(0x00);
        } else if (base & 7) == 4 {
            // RSP/R12: 需要 SIB 字节
            self.b(0x00 | ((dst & 7) << 3) | (base & 7));
            self.b(0x24);
        } else {
            self.b(0x00 | ((dst & 7) << 3) | (base & 7));
        }
    }

    // ================ 全局数据偏移管理 ================
    //
    // 全局数据在 _start 末尾，布局:
    //   Offset 0:  bump_ptr       (u64) 当前分配位置
    //   Offset 8:  heap_start     (u64) 堆起始地址
    //   Offset 16: heap_limit     (u64) 堆上限
    //   Offset 24: vstack_bottom  (u64) 虚拟栈底
    //   Offset 32: alloc_count    (u64) 分配计数
    //   Offset 40: gc_threshold   (u64) GC 触发阈值
    //
    // 对象头格式 (8 字节):
    //   Byte 0: color (WHITE=0, GRAY=1, BLACK=2)
    //   Byte 1: obj_type
    //   Byte 2-3: padding
    //   Byte 4-7: total_size (含头部，16 字节对齐)

    fn globals_offset(&self) -> Option<usize> {
        self.functions.iter().find(|f| f.name == "__bump_ptr").map(|f| f.offset)
    }
    fn heap_limit_offset(&self) -> Option<usize> {
        self.functions.iter().find(|f| f.name == "__heap_limit").map(|f| f.offset)
    }

    // ================ 运行时函数生成 ================

    /// _start — 程序入口
    fn emit_start(&mut self) {
        self.fn_start(runtime_names::START);
        self.push(5); self.push(3); self.push(12); self.push(13); self.push(14); self.push(15);

        // ---- mmap 虚拟栈 64KB ----
        self.mov_ri(0, 9); self.mov_ri(7, 0); self.mov_ri(6, 65536);
        self.mov_ri(2, 3); self.mov_ri(10, 0x22); self.mov_ri(8, !0u64); self.mov_ri(9, 0);
        self.syscall();
        self.mov_rr(12, 0); // R12 = vstack_base

        // R10 = vm_sp = vstack_base + 65520
        self.mov_rr(10, 0);
        self.mov_ri(0, 65520);
        self.add_rr(10, 0);
        // sentinel
        self.mov_ri(0, 0);
        self.mov_indirect_store(10, 0);
        // R11 = vm_fp = R10
        self.mov_rr(11, 10);
        // 保存 vm_sp/vm_fp
        self.push(10); self.push(11);

        // ---- mmap 堆 4MB ----
        self.mov_ri(0, 9); self.mov_ri(7, 0); self.mov_ri(6, 4*1024*1024);
        self.mov_ri(2, 3); self.mov_ri(10, 0x22); self.mov_ri(8, !0u64); self.mov_ri(9, 0);
        self.syscall();
        // RAX = heap_base
        self.pop(11); self.pop(10); // 恢复 vm_sp/vm_fp

        // 存储全局变量 (占位, 修补 RIP-relative)
        // bump_ptr = heap_base (mmap 返回值)
        let bump_store = self.code.len();
        self.mov_rip_store(0, 0);
        // heap_start = heap_base (在 RAX 被覆盖之前先存)
        let heap_start_store = self.code.len();
        self.mov_rip_store(0, 0);
        // heap_limit = heap_base + 4MB
        self.mov_rr(1, 0); self.mov_ri(0, 4*1024*1024); self.add_rr(1, 0);
        let limit_store = self.code.len();
        self.mov_rip_store(1, 0);

        // vstack_bottom = R12
        self.mov_rr(0, 12);
        let vstack_bottom_store = self.code.len();
        self.mov_rip_store(0, 0);

        // vstack_top = R12 + 65520 (虚拟栈顶部，用于 GC 扫描)
        self.mov_rr(0, 12);
        self.mov_ri(1, 65520);
        self.add_rr(0, 1);
        let vstack_top_store = self.code.len();
        self.mov_rip_store(0, 0);

        // alloc_count = 0
        self.xor_rr(0, 0);
        let alloc_count_store = self.code.len();
        self.mov_rip_store(0, 0);

        // gc_threshold = GC_THRESHOLD
        self.mov_ri(0, GC_THRESHOLD);
        let threshold_store = self.code.len();
        self.mov_rip_store(0, 0);

        // ---- 初始化 heap inline header (gc_init 的功能) ----
        // heap header: [bump_ptr(8)][alloc_count(8)][threshold(8)]
        // gc_alloc/gc_collect 通过 unsafe_load/unsafe_store 操作这些字段
        // RAX = heap_base (需要重新加载)
        let heap_base_load = self.code.len();
        self.mov_rip_load(0, 0); // RAX = __heap_start, 占位
        // [heap+0] = bump_ptr = heap_base + 24 (跳过 header 自身)
        self.mov_rr(1, 0); // RCX = heap_base
        self.add_ri8(1, 24); // RCX = heap_base + 24
        // MOV [RAX], RCX
        self.bs(&[0x48, 0x89, 0x08]); // MOV [RAX], RCX
        // [heap+8] = alloc_count = 0 (mmap 保证清零，不需要写)
        // [heap+16] = threshold = 256
        self.mov_ri(1, 256);
        // MOV [RAX+16], RCX
        self.bs(&[0x48, 0x89, 0x48, 0x10]); // MOV [RAX+16], RCX

        // ---- 调用 main ----
        self.mov_rr(7, 10); // RDI = vm_sp
        self.mov_rr(6, 12); // RSI = vstack_bottom
        self.call_main_rel32_offset = self.code.len() + 1;
        self.call_rel32(0); // 占位

        // ---- 将 main() 返回值 (RAX) 通过 sys_write 输出到 stdout ----
        //
        // Linux exit_group 只取低 8 位 (0-255)，结果 >= 256 会截断。
        // 改为: 将 i64 转为 ASCII 十进制字符串 → sys_write(stdout) → exit_group(0)。
        //
        // 寄存器分配:
        //   RAX = 当前数值 / syscall 号 / syscall 返回值
        //   RBX = 原始返回值备份
        //   RCX = 临时 (除法/比较)
        //   RDX = 符号标志 (0=正, 1=负) / 除法余数
        //   R8  = 字符计数
        //   R9  = 常量 10 (除数)
        //   RSP = 缓冲区 (从高地址往低地址增长)
        //
        // 算法:
        //   1. 处理负数 (记录符号, 取绝对值)
        //   2. 循环: RAX % 10 得到末位数字, RAX /= 10, 将 ASCII 数字推入栈
        //   3. 如果是负数, 推入 '-'
        //   4. 推入 '\n'
        //   5. sys_write(1, RSP, R8)

        // 保存 callee-saved 和工作寄存器
        self.push(3);  // 保存 RBX
        self.push(8);  // 保存 R8
        self.push(9);  // 保存 R9

        self.mov_rr(3, 0); // RBX = 原始返回值

        // 处理负数
        self.mov_rr(0, 3); // RAX = 返回值
        self.xor_rr(2, 2); // RDX = 0 (符号: 0=正)
        self.mov_ri(1, 0);
        self.cmp_rr(0, 1);
        let neg_skip = self.code.len();
        self.jge_rel32(0); // >= 0 则跳过（有符号比较）

        // 负数: 标记符号, 取绝对值
        self.mov_ri(2, 1); // RDX = 1 (负数)
        // NEG RAX: 用 0 - RAX
        self.mov_rr(1, 0); // RCX = RAX
        self.xor_rr(0, 0); // RAX = 0
        self.sub_rr(0, 1); // RAX = -RCX

        // neg_skip:
        let neg_skip_label = self.code.len();

        // 初始化计数器和常量
        self.xor_rr(8, 8); // R8 = 字符计数 = 0
        self.mov_ri(9, 10); // R9 = 10

        // 先推入 '\n' (位于高地址, 数字写在低地址, sys_write 从 RSP 往上读: 数字...'\n')
        self.sub_ri8(4, 1);
        self.bs(&[0xC6, 0x04, 0x24, b'\n' as u8]); // MOV byte [RSP], '\n'
        self.add_ri8(8, 1); // 字符计数 = 1

        // 处理 RAX == 0 的特殊情况
        self.test_rr(0, 0);
        let nonzero = self.code.len();
        self.jne_rel32(0);

        // RAX == 0: 写入 '0'
        self.sub_ri8(4, 1); // RSP -= 1
        self.bs(&[0xC6, 0x04, 0x24, b'0' as u8]); // MOV byte [RSP], '0'
        self.add_ri8(8, 1); // 字符计数 = 2 (0 + \n)
        let zero_done = self.code.len();
        self.jmp_rel32(0); // → sys_write

        // nonzero / digit_loop:
        let nonzero_label = self.code.len();
        let digit_loop = self.code.len();

        self.test_rr(0, 0);
        let digit_done = self.code.len();
        self.jz_rel32(0); // RAX == 0, 退出循环

        // IDIV R9: RAX = 商, RDX = 余数
        // 需要保存符号标志 (RDX) → 存到 R11 (此时 vm_fp 不再需要)
        self.mov_rr(11, 2);   // R11 = 符号标志
        self.bs(&[0x48, 0x99]); // CQO: RDX:RAX = sign-extend(RAX)
        // IDIV R9 = REX.WB(0x49) + F7 + ModRM(11_111_001 = 0xF9)
        self.bs(&[0x49, 0xF7, 0xF9]); // IDIV R9
        // RDX = 余数, RAX = 商
        self.add_ri8(2, b'0' as u8); // RDX = ASCII 数字
        self.sub_ri8(4, 1); // RSP -= 1
        self.bs(&[0x88, 0x14, 0x24]); // MOV byte [RSP], DL
        self.add_ri8(8, 1); // 字符计数++
        // 恢复符号标志
        self.mov_rr(2, 11); // RDX = 符号标志

        // 跳回循环
        {
            let rel = (digit_loop as i64 - (self.code.len() as i64 + 5)) as i32;
            self.jmp_rel32(rel);
        }

        // digit_done:
        let digit_done_label = self.code.len();

        // 如果是负数, 推入 '-'
        self.test_rr(2, 2);
        let no_neg_sign = self.code.len();
        self.jz_rel32(0);

        self.sub_ri8(4, 1);
        self.bs(&[0xC6, 0x04, 0x24, b'-' as u8]);
        self.add_ri8(8, 1);

        // no_neg_sign:
        let no_neg_sign_label = self.code.len();

        // ---- sys_write(1, RSP, R8) ----
        self.mov_ri(0, 1);  // syscall 号: sys_write
        self.mov_ri(7, 1);  // fd = stdout
        self.mov_rr(6, 4);  // buf = RSP
        self.mov_rr(2, 8);  // count = R8
        self.syscall();

        // 恢复 RSP: 移除推入的数字字符（R8 = 字符计数，syscall 后 R8 保留）
        // 使用 R9 作为临时: R9 还没恢复，先保存 R9 → 用 R9 保存 R8 → add rsp, r9 → 恢复
        // 更简单: 直接 add rsp, r8（因为 R8 在 syscall 后保留）
        // 但是 R8 在后面 pop 时被覆盖，所以要先处理
        // 方案: 先用 RSP 加回字符数，再 pop
        self.add_rr(4, 8);  // RSP += R8 (恢复推入的字符空间)

        // 恢复保存的寄存器
        self.pop(9);  // 恢复 R9
        self.pop(8);  // 恢复 R8
        self.pop(3);  // 恢复 RBX

        self.pop(15); self.pop(14); self.pop(13); self.pop(12); self.pop(3); self.pop(5);
        // exit_group(0)
        self.xor_rr(7, 7); self.mov_ri(0, 231); self.syscall();

        // ---- 全局数据 ----
        while self.code.len() % 8 != 0 { self.nop(); }
        let g = self.code.len();
        self.u64(0);                    // bump_ptr
        self.u64(0);                    // heap_start
        self.u64(0);                    // heap_limit
        self.u64(0);                    // vstack_bottom
        self.u64(0);                    // vstack_top
        self.u64(0);                    // alloc_count
        self.u64(0);                    // gc_threshold

        self.functions.push(RuntimeFunction { name: "__bump_ptr".into(), offset: g, size: 8 });
        self.functions.push(RuntimeFunction { name: "__heap_start".into(), offset: g+8, size: 8 });
        self.functions.push(RuntimeFunction { name: "__heap_limit".into(), offset: g+16, size: 8 });
        self.functions.push(RuntimeFunction { name: "__vstack_bottom".into(), offset: g+24, size: 8 });
        self.functions.push(RuntimeFunction { name: "__vstack_top".into(), offset: g+32, size: 8 });
        self.functions.push(RuntimeFunction { name: "__alloc_count".into(), offset: g+40, size: 8 });
        self.functions.push(RuntimeFunction { name: "__gc_threshold".into(), offset: g+48, size: 8 });

        // 修补所有 RIP-relative store
        fn patch_rip_store(code: &mut Vec<u8>, store_pos: usize, target: usize) {
            let rip_after = (store_pos + 7) as i32;
            let disp = target as i32 - rip_after;
            code[store_pos+3..store_pos+7].copy_from_slice(&disp.to_le_bytes());
        }
        patch_rip_store(&mut self.code, bump_store, g);
        patch_rip_store(&mut self.code, heap_start_store, g+8);
        patch_rip_store(&mut self.code, limit_store, g+16);
        patch_rip_store(&mut self.code, vstack_bottom_store, g+24);
        patch_rip_store(&mut self.code, vstack_top_store, g+32);
        patch_rip_store(&mut self.code, alloc_count_store, g+40);
        patch_rip_store(&mut self.code, threshold_store, g+48);

        // 修补 heap inline header 初始化中的 heap_base load
        fn patch_rip_load(code: &mut Vec<u8>, load_pos: usize, target: usize) {
            let rip_after = (load_pos + 7) as i32;
            let disp = target as i32 - rip_after;
            code[load_pos+3..load_pos+7].copy_from_slice(&disp.to_le_bytes());
        }
        let g_heap_start = g + 8;
        patch_rip_load(&mut self.code, heap_base_load, g_heap_start);

        // ---- 修补 i64→ASCII 输出的跳转标签 ----
        fn patch_jmp(code: &mut Vec<u8>, jmp_pos: usize, target: usize) {
            // rel32 跳转: jmp_pos 是 Jcc/JMP 的位置, +6 (2字节opcode + 4字节rel32)
            let jmp_end = (jmp_pos + 6) as i32;
            let rel = target as i32 - jmp_end;
            code[jmp_pos+2..jmp_pos+6].copy_from_slice(&rel.to_le_bytes());
        }
        fn patch_jmp_5(code: &mut Vec<u8>, jmp_pos: usize, target: usize) {
            // JMP rel32 (E9 + 4字节): 5 字节长
            let jmp_end = (jmp_pos + 5) as i32;
            let rel = target as i32 - jmp_end;
            code[jmp_pos+1..jmp_pos+5].copy_from_slice(&rel.to_le_bytes());
        }

        // neg_skip (JAE) → neg_skip_label
        patch_jmp(&mut self.code, neg_skip, neg_skip_label);
        // nonzero (JNE) → nonzero_label (digit_loop)
        patch_jmp(&mut self.code, nonzero, nonzero_label);
        // zero_done (JMP) → no_neg_sign_label (跳过数字循环, 直接到输出前的负号检查)
        patch_jmp_5(&mut self.code, zero_done, no_neg_sign_label);
        // digit_done (JZ) → digit_done_label
        patch_jmp(&mut self.code, digit_done, digit_done_label);
        // no_neg_sign (JZ) → no_neg_sign_label
        patch_jmp(&mut self.code, no_neg_sign, no_neg_sign_label);

        self.fn_end();
    }

    /// __karte_gc_alloc(size, align) → 带 GC 头的分配
    ///
    /// RDI = size, RSI = align
    /// 返回 RAX = 数据指针 (跳过 8 字节 header)
    ///
    /// 对象布局: [color:u8][obj_type:u8][pad:u16][total_size:u32] [data...]
    fn emit_gc_alloc(&mut self) {
        self.fn_start(runtime_names::GC_ALLOC_ALIGNED);
        self.push(5); self.push(3);

        // RBX = size
        self.mov_rr(3, 7); // RBX = size

        // total_size = align_up(8 + size, 16)
        // RAX = 8 + size
        self.mov_ri(0, GC_HEADER_SIZE);
        self.add_rr(0, 3);
        // RAX = (RAX + 15) & ~15
        self.add_ri8(0, 15);
        self.and_ri32(0, 0xFFFFFFF0);
        // RCX = total_size (保存)
        self.mov_rr(1, 0);

        // 检查是否需要 GC (alloc_count >= threshold)
        let bump_load = self.code.len();
        self.mov_rip_load(0, 0); // RAX = bump_ptr, 占位
        // 保存旧 bump_ptr 作为返回值候选
        let limit_load = self.code.len();
        self.mov_rip_load(2, 0); // RDX = heap_limit, 占位

        // RAX = bump_ptr + total_size
        self.add_rr(0, 1); // RAX = new_bump_ptr

        // 检查是否超出堆
        self.cmp_rr(0, 2);
        let ja_pos = self.code.len();
        self.ja_rel32(0); // 占位 → 跳到 overflow

        // 更新 bump_ptr
        let bump_update = self.code.len();
        self.mov_rip_store(0, 0); // 存储 new bump_ptr, 占位

        // 写入 GC 头部 (旧 bump_ptr 位置)
        // 头部: [color=WHITE(0)][obj_type=0][pad=0][total_size]
        // 我们已经知道旧 bump_ptr = new_bump_ptr - total_size = RAX - RCX
        self.sub_rr(0, 1); // RAX = old_bump_ptr (对象起始)
        // color = WHITE (0) — bump 分配的内存已经清零 (mmap), 不需要再写
        // total_size at offset 4:
        // MOV dword [RAX+4], ECX (total_size 的低 32 位)
        // 89 48 04
        self.bs(&[0x89, 0x48, 0x04]); // MOV [RAX+4], ECX

        // 增加分配计数
        let alloc_count_load = self.code.len();
        self.mov_rip_load(2, 0); // RDX = alloc_count, 占位
        self.add_ri8(2, 1); // alloc_count++
        let alloc_count_store = self.code.len();
        self.mov_rip_store(2, 0); // 存储, 占位

        // 返回 data 指针 = 对象起始 + GC_HEADER_SIZE
        self.add_ri8(0, GC_HEADER_SIZE as u8); // RAX += 8
        self.pop(3); self.pop(5);
        self.ret();

        // ---- overflow: 触发 GC 然后重试 ----
        let overflow_pos = self.code.len();
        // 调用 gc_collect(vm_sp)
        // vm_sp 在 R10, 但我们不在运行时函数里直接知道 vm_sp
        // 用 R11 (vm_fp) 作为近似 — 或者用一个全局变量存储当前 vm_sp
        // 简化方案: gc_collect 不需要参数, 它扫描从 vstack_bottom 到 bump_ptr 之前的区域
        // 实际上它需要 vm_sp... 让我们先传 0 (GC 会读全局 vstack_bottom)
        self.mov_ri(7, 0); // RDI = 0 (gc_collect 会用全局 vstack_bottom)
        // CALL gc_collect (需要知道 gc_collect 的偏移, 稍后修补)
        // 用一个占位 call, 记录位置
        let gc_call_pos = self.code.len();
        self.call_rel32(0); // 占位

        // GC 后重试分配
        // 重新加载 bump_ptr, 再次检查
        let retry_load = self.code.len();
        self.mov_rip_load(0, 0); // RAX = bump_ptr (GC 后可能变小)
        self.add_rr(0, 1); // RAX = bump_ptr + total_size
        let limit_load2 = self.code.len();
        self.mov_rip_load(2, 0); // RDX = heap_limit
        self.cmp_rr(0, 2);
        // 仍然溢出 → 返回 0 (真的 OOM)
        let ja2_pos = self.code.len();
        self.ja_rel32(0); // 占位

        // 更新 bump_ptr
        let bump_update2 = self.code.len();
        self.mov_rip_store(0, 0);
        self.sub_rr(0, 1);
        self.bs(&[0x89, 0x48, 0x04]); // total_size
        self.add_ri8(0, GC_HEADER_SIZE as u8);
        self.pop(3); self.pop(5);
        self.ret();

        // 真的 OOM
        let oom_pos = self.code.len();
        self.xor_rr(0, 0); // RAX = 0
        self.pop(3); self.pop(5);
        self.ret();

        // ---- 修补 ----
        let g = self.globals_offset().unwrap();
        let hl = self.heap_limit_offset().unwrap();
        // 找 alloc_count 和 gc_threshold 的偏移
        let ac = self.functions.iter().find(|f| f.name == "__alloc_count").unwrap().offset;
        let gt_off = self.functions.iter().find(|f| f.name == "__gc_threshold").unwrap().offset;

        // bump_ptr load (7 bytes)
        let rip = (bump_load + 7) as i32;
        self.code[bump_load+3..bump_load+7].copy_from_slice(&(g as i32 - rip).to_le_bytes());
        // heap_limit load
        let rip = (limit_load + 7) as i32;
        self.code[limit_load+3..limit_load+7].copy_from_slice(&(hl as i32 - rip).to_le_bytes());
        // bump update
        let rip = (bump_update + 7) as i32;
        self.code[bump_update+3..bump_update+7].copy_from_slice(&(g as i32 - rip).to_le_bytes());
        // alloc_count load
        let rip = (alloc_count_load + 7) as i32;
        self.code[alloc_count_load+3..alloc_count_load+7].copy_from_slice(&(ac as i32 - rip).to_le_bytes());
        // alloc_count store
        let rip = (alloc_count_store + 7) as i32;
        self.code[alloc_count_store+3..alloc_count_store+7].copy_from_slice(&(ac as i32 - rip).to_le_bytes());

        // overflow JA
        let ja_rel = (overflow_pos - (ja_pos + 6)) as i32;
        self.code[ja_pos+2..ja_pos+6].copy_from_slice(&ja_rel.to_le_bytes());

        // gc_collect call (修补为 gc_collect 函数的偏移 — 暂时先留占位, 在 generate 最后修补)
        // 记录 gc_call_pos 以便稍后修补
        // 这里我们先记录, 等所有函数生成完后再修补
        self.functions.push(RuntimeFunction {
            name: "__gc_alloc_call_collect".into(),
            offset: gc_call_pos,
            size: 5,
        });

        // retry bump_ptr load
        let rip = (retry_load + 7) as i32;
        self.code[retry_load+3..retry_load+7].copy_from_slice(&(g as i32 - rip).to_le_bytes());
        // retry limit load
        let rip = (limit_load2 + 7) as i32;
        self.code[limit_load2+3..limit_load2+7].copy_from_slice(&(hl as i32 - rip).to_le_bytes());
        // ja2
        let ja2_rel = (oom_pos - (ja2_pos + 6)) as i32;
        self.code[ja2_pos+2..ja2_pos+6].copy_from_slice(&ja2_rel.to_le_bytes());
        // bump update2
        let rip = (bump_update2 + 7) as i32;
        self.code[bump_update2+3..bump_update2+7].copy_from_slice(&(g as i32 - rip).to_le_bytes());

        self.fn_end();
    }

    /// __karte_gc_collect(vm_sp) — 三色标记-清除-压缩
    ///
    /// RDI = vm_sp (虚拟栈顶, 0 表示用全局 vstack_bottom)
    ///
    /// 算法:
    /// 1. 标记阶段: 保守扫描虚拟栈 → 标记可达对象
    /// 2. 压缩阶段: 将存活对象移到堆前端
    ///
    /// 为了简化实现, 这里只做标记-清除 (不清除, 只重置 bump_ptr)
    /// 实际压缩太复杂了, 先做最简版本
    fn emit_gc_collect(&mut self) {
        self.fn_start(runtime_names::GC_COLLECT);
        self.push(5); self.push(3); self.push(12); self.push(13); self.push(14); self.push(15);

        let g = self.globals_offset().unwrap();
        let g_heap_start = self.functions.iter().find(|f| f.name == "__heap_start").unwrap().offset;
        let g_heap_limit = self.functions.iter().find(|f| f.name == "__heap_limit").unwrap().offset;
        let g_vstack_bottom = self.functions.iter().find(|f| f.name == "__vstack_bottom").unwrap().offset;
        let g_alloc_count = self.functions.iter().find(|f| f.name == "__alloc_count").unwrap().offset;

        // ---- 1. 标记阶段: 扫描虚拟栈, 保守标记 ----
        //
        // R12 = heap_start (用于范围检查)
        // R13 = heap_limit (用于范围检查)
        // R14 = bump_ptr (用于范围检查和堆遍历)
        // R15 = vstack_bottom (扫描起始)

        // 加载 heap_start → R12
        self.lea_rip(12, 0); // 占位
        let lea_hs = self.code.len() - 7;
        self.mov_indirect_load(12, 12); // R12 = [R12]

        // 加载 heap_limit → R13
        self.lea_rip(13, 0);
        let lea_hl = self.code.len() - 7;
        self.mov_indirect_load(13, 13);

        // 加载 bump_ptr → R14
        self.lea_rip(14, 0);
        let lea_bp = self.code.len() - 7;
        self.mov_indirect_load(14, 14);

        // 加载 vstack_bottom → R15
        self.lea_rip(15, 0);
        let lea_vb = self.code.len() - 7;
        self.mov_indirect_load(15, 15);

        // 修补 LEA 指令
        fn patch_lea(code: &mut Vec<u8>, lea_pos: usize, target: usize) {
            let rip_after = (lea_pos + 7) as i32;
            let disp = target as i32 - rip_after;
            code[lea_pos+3..lea_pos+7].copy_from_slice(&disp.to_le_bytes());
        }
        patch_lea(&mut self.code, lea_hs, g_heap_start);
        patch_lea(&mut self.code, lea_hl, g_heap_limit);
        patch_lea(&mut self.code, lea_bp, g);
        patch_lea(&mut self.code, lea_vb, g_vstack_bottom);

        // 如果 vm_sp 参数 (RDI) 非 0, 使用它作为栈顶; 否则跳过扫描
        // 简化: 我们使用 vstack_bottom 作为扫描范围, vm_sp 参数暂时忽略
        //        (safepoint 会传正确的 vm_sp)

        // ---- 扫描虚拟栈 ----
        // RSI = vstack_bottom (扫描起始 = R15)
        // RDI = vm_sp (扫描结束, 使用参数 RDI)
        // 如果 RDI == 0, 不扫描
        self.mov_rr(6, 15); // RSI = vstack_bottom
        // 检查 RDI (vm_sp) 是否为 0
        self.test_rr(7, 7);
        let skip_scan = self.code.len();
        self.jz_rel32(0); // 如果 vm_sp == 0, 跳过扫描

        // 扫描循环: for each word in [vstack_bottom, vm_sp)
        // RAX = 当前扫描位置
        self.mov_rr(0, 15); // RAX = vstack_bottom
        // scan_loop:
        let scan_loop = self.code.len();
        // 检查 RAX < RDI (vm_sp)
        self.cmp_rr(0, 7);
        let scan_done = self.code.len();
        self.jae_rel32(0); // 如果 RAX >= vm_sp, 跳出

        // 读取当前 word: RCX = [RAX]
        self.mov_mem_load(1, 0, 0); // RCX = [RAX]

        // 检查 RCX 是否在堆范围内: heap_start <= RCX < heap_limit
        // 但实际对象数据在 [heap_start + 8, bump_ptr) (跳过 header)
        // 简化: 检查 heap_start <= RCX < bump_ptr
        self.cmp_rr(1, 12); // CMP RCX, R12 (heap_start)
        let scan_next = self.code.len();
        self.jb_rel32(0); // 如果 < heap_start, 跳过

        self.cmp_rr(1, 14); // CMP RCX, R14 (bump_ptr)
        let scan_next2 = self.code.len();
        self.jae_rel32(0); // 如果 >= bump_ptr, 跳过

        // RCX 可能是一个堆指针, 找到对象头并标记
        // 对象头 = RCX - (RCX - heap_start) % total_size 对齐...
        // 简化: 线性搜索堆, 找到包含 RCX 的对象
        // 这太慢了. 改用简单方法: 标记 RCX-8 处的对象头 (假设 RCX 指向 data)
        // 但 RCX 可能指向 data 中间...

        // 更好的方法: 从 heap_start 开始线性遍历, 找到第一个
        // header <= RCX < header + total_size 的对象
        // RDX = 当前搜索位置
        self.mov_rr(2, 12); // RDX = heap_start
        // find_loop:
        let find_loop = self.code.len();
        // 检查 RDX < bump_ptr
        self.cmp_rr(2, 14);
        let find_done = self.code.len();
        self.jae_rel32(0); // 如果 >= bump_ptr, 没找到

        // 读取对象 total_size: R8 = [RDX + 4] (u32)
        self.mov_mem_load(8, 2, 4); // R8 = total_size
        // 检查 RCX >= RDX + 8 (data start)
        self.mov_rr(9, 2); // R9 = header_pos
        self.add_ri8(9, GC_HEADER_SIZE as u8); // R9 = data_start
        self.cmp_rr(1, 9); // CMP RCX, data_start
        let find_next = self.code.len();
        self.jb_rel32(0); // RCX < data_start, 对象不包含此指针

        // 检查 RCX < RDX + total_size
        self.mov_rr(9, 2);
        self.add_rr(9, 8); // R9 = header + total_size
        self.cmp_rr(1, 9);
        let found_obj = self.code.len();
        self.jb_rel32(0); // RCX < end → 找到了!

        // find_next: RDX += total_size
        let find_next_label = self.code.len();
        self.add_rr(2, 8); // RDX += total_size
        {
            let rel = (find_loop as i64 - (self.code.len() as i64 + 5)) as i32;
            self.jmp_rel32(rel);
        }
        // find_done:
        let find_done_label = self.code.len();
        // 没找到 → 跳到扫描下一个 word
        self.jmp_rel32(0); // 占位 → scan_next_label

        // found_obj: 标记对象
        let found_label = self.code.len();
        // 设置 color = BLACK: MOV byte [RDX], BLACK(2)
        self.mov_byte_mem_imm(2, 0, COLOR_BLACK);

        // 继续扫描下一个 word
        // scan_next_label:
        let scan_next_label = self.code.len();
        self.add_ri8(0, 8); // RAX += 8 (下一个 word)
        {
            let rel = (scan_loop as i64 - (self.code.len() as i64 + 5)) as i32;
            self.jmp_rel32(rel);
        }

        // scan_done_label:
        let scan_done_label = self.code.len();

        // ---- 2. 压缩阶段: 将存活对象复制到堆前端 ----
        // RSI = 写入位置 (dest), 从 heap_start 开始
        self.mov_rr(6, 12); // RSI = dest = heap_start
        // RDI = 读取位置 (src), 从 heap_start 开始
        self.mov_rr(7, 12); // RDI = src = heap_start

        // compact_loop:
        let compact_loop = self.code.len();
        // 检查 RDI < bump_ptr
        self.cmp_rr(7, 14);
        let compact_done = self.code.len();
        self.jae_rel32(0); // done

        // 读取 total_size: R8 = [RDI + 4]
        self.mov_mem_load(8, 7, 4);
        // 读取 color: R9b = [RDI]
        self.movzx_byte(9, 7, 0);

        // 检查 color == BLACK
        self.cmp_rr(9, 2); // R9 < 2? (实际上应该和立即数比)
        // 用 test + jnz 代替
        // 先检查是否是 WHITE 或 BLACK
        // 如果 BLACK: 复制对象到 dest 位置
        let is_black = self.code.len();
        // 如果 color != BLACK, 跳过 (释放此对象)
        self.cmp_rr(9, 2);
        let skip_obj = self.code.len();
        self.jne_rel32(0); // 不是 BLACK → 跳过

        // 复制存活对象: 从 RDI 复制 total_size 字节到 RSI
        // 用简单的 rep movsb 或者逐字节复制
        // 使用 RCX 作为计数器, rep movsb
        self.mov_rr(1, 8); // RCX = total_size
        // 保存 RSI/RDI (rep movsb 会修改它们)
        self.push(7); self.push(6);
        // RSI = src (RDI), RDI = dst (RSI) — 注意 rep movsb 是 [RSI] → [RDI]
        // 所以我们需要: RSI = src, RDI = dst
        self.mov_rr(6, 7); // RSI = src (original RDI)
        // 从栈上恢复 dest 到 RDI: push(6) 保存的 RSI(dest) 现在在 [RSP]
        self.mov_mem_load(7, 4, 0); // RDI = [RSP] = dest
        self.push(8); // 保存 total_size
        // rep movsb
        self.bs(&[0xF3, 0xA4]); // REP MOVSB
        self.pop(8); // 恢复 total_size
        self.pop(6); self.pop(7); // 恢复原始 RSI/RDI

        // 更新 dest: RSI += total_size
        self.add_rr(6, 8);

        // 更新新位置的 color 为 WHITE (为下次 GC 准备)
        // 新位置是 RSI - total_size
        self.sub_rr(6, 8); // RSI = new_obj_start
        self.mov_byte_mem_imm(6, 0, COLOR_WHITE);
        self.add_rr(6, 8); // RSI = next dest

        // advance_src:
        let advance_src = self.code.len();
        // src += total_size
        self.add_rr(7, 8);
        {
            let rel = (compact_loop as i64 - (self.code.len() as i64 + 5)) as i32;
            self.jmp_rel32(rel);
        }

        // skip_obj: 跳过非存活对象
        let skip_obj_label = self.code.len();
        self.add_rr(7, 8); // src += total_size
        {
            let rel = (compact_loop as i64 - (self.code.len() as i64 + 5)) as i32;
            self.jmp_rel32(rel);
        }

        // compact_done:
        let compact_done_label = self.code.len();

        // 更新 bump_ptr = RSI (新的分配位置)
        // MOV [rip + bump_ptr_offset], RSI
        let bump_update_final = self.code.len();
        self.mov_rip_store(6, 0); // 占位

        // 重置 alloc_count = 0
        self.xor_rr(0, 0);
        let ac_reset = self.code.len();
        self.mov_rip_store(0, 0); // 占位

        self.pop(15); self.pop(14); self.pop(13); self.pop(12); self.pop(3); self.pop(5);
        self.ret();

        // ---- 修补所有跳转 ----
        // skip_scan (jz)
        let skip_scan_rel = (compact_done_label - (skip_scan + 6)) as i32;
        self.code[skip_scan+2..skip_scan+6].copy_from_slice(&skip_scan_rel.to_le_bytes());

        // scan_done (jae)
        let scan_done_rel = (scan_done_label - (scan_done + 6)) as i32;
        self.code[scan_done+2..scan_done+6].copy_from_slice(&scan_done_rel.to_le_bytes());

        // scan_next (jb - pointer < heap_start)
        let sn_rel = (scan_next_label - (scan_next + 6)) as i32;
        self.code[scan_next+2..scan_next+6].copy_from_slice(&sn_rel.to_le_bytes());

        // scan_next2 (jae - pointer >= bump_ptr)
        let sn2_rel = (scan_next_label - (scan_next2 + 6)) as i32;
        self.code[scan_next2+2..scan_next2+6].copy_from_slice(&sn2_rel.to_le_bytes());

        // find_done (jae)
        let fd_rel = (find_done_label - (find_done + 6)) as i32;
        self.code[find_done+2..find_done+6].copy_from_slice(&fd_rel.to_le_bytes());

        // find_next (jb - pointer < data_start)
        let fn_rel = (find_next_label - (find_next + 6)) as i32;
        self.code[find_next+2..find_next+6].copy_from_slice(&fn_rel.to_le_bytes());

        // found_obj (jb - pointer < end)
        let fo_rel = (found_label - (found_obj + 6)) as i32;
        self.code[found_obj+2..found_obj+6].copy_from_slice(&fo_rel.to_le_bytes());

        // find_done → scan_next
        let fd2sn_rel = (scan_next_label - (find_done_label + 5)) as i32;
        self.code[find_done_label+1..find_done_label+5].copy_from_slice(&fd2sn_rel.to_le_bytes());

        // compact_done (jae)
        let cd_rel = (compact_done_label - (compact_done + 6)) as i32;
        self.code[compact_done+2..compact_done+6].copy_from_slice(&cd_rel.to_le_bytes());

        // skip_obj (jne)
        let so_rel = (skip_obj_label - (skip_obj + 6)) as i32;
        self.code[skip_obj+2..skip_obj+6].copy_from_slice(&so_rel.to_le_bytes());

        // bump_ptr update
        let rip = (bump_update_final + 7) as i32;
        self.code[bump_update_final+3..bump_update_final+7].copy_from_slice(&(g as i32 - rip).to_le_bytes());

        // alloc_count reset
        let rip = (ac_reset + 7) as i32;
        self.code[ac_reset+3..ac_reset+7].copy_from_slice(&(g_alloc_count as i32 - rip).to_le_bytes());

        self.fn_end();
    }

    /// __karte_gc_safepoint(vm_sp) — 检查是否需要 GC
    fn emit_gc_safepoint(&mut self) {
        self.fn_start(runtime_names::GC_SAFEPOINT);
        self.push(5); self.push(3);

        let g = self.globals_offset().unwrap();
        let ac = self.functions.iter().find(|f| f.name == "__alloc_count").unwrap().offset;
        let gt = self.functions.iter().find(|f| f.name == "__gc_threshold").unwrap().offset;

        // 加载 alloc_count
        let ac_load = self.code.len();
        self.mov_rip_load(0, 0); // RAX = alloc_count, 占位
        // 加载 gc_threshold
        let gt_load = self.code.len();
        self.mov_rip_load(1, 0); // RCX = gc_threshold, 占位

        // 比较: alloc_count >= threshold?
        self.cmp_rr(0, 1);
        let no_gc = self.code.len();
        self.jb_rel32(0); // 如果 < threshold, 跳过 GC

        // 触发 GC
        // RDI = vm_sp (已经在 RDI 中)
        // 调用 gc_collect(vm_sp)
        let gc_call = self.code.len();
        self.call_rel32(0); // 占位

        // 重置 alloc_count = 0
        self.xor_rr(0, 0);
        let ac_reset = self.code.len();
        self.mov_rip_store(0, 0); // 占位

        // no_gc:
        let no_gc_label = self.code.len();
        self.pop(3); self.pop(5);
        self.ret();

        // ---- 修补 ----
        let rip = (ac_load + 7) as i32;
        self.code[ac_load+3..ac_load+7].copy_from_slice(&(ac as i32 - rip).to_le_bytes());
        let rip = (gt_load + 7) as i32;
        self.code[gt_load+3..gt_load+7].copy_from_slice(&(gt as i32 - rip).to_le_bytes());
        let no_gc_rel = (no_gc_label - (no_gc + 6)) as i32;
        self.code[no_gc+2..no_gc+6].copy_from_slice(&no_gc_rel.to_le_bytes());
        let rip = (ac_reset + 7) as i32;
        self.code[ac_reset+3..ac_reset+7].copy_from_slice(&(ac as i32 - rip).to_le_bytes());

        // gc_collect call — 记录以便后续修补
        self.functions.push(RuntimeFunction {
            name: "__safepoint_call_collect".into(),
            offset: gc_call,
            size: 5,
        });

        self.fn_end();
    }

    /// __karte_gc_update_stack_top(vm_sp) — 更新虚拟栈顶信息
    fn emit_gc_update_stack_top(&mut self) {
        self.fn_start(runtime_names::GC_UPDATE_STACK_TOP);
        // 目前 no-op, GC 直接用 vstack_bottom 和 bump_ptr 推断
        self.ret();
        self.fn_end();
    }

    fn emit_free(&mut self) {
        self.fn_start(runtime_names::FREE);
        self.ret();
        self.fn_end();
    }
    fn emit_retain(&mut self) {
        self.fn_start(runtime_names::RETAIN);
        self.ret();
        self.fn_end();
    }
    fn emit_release(&mut self) {
        self.fn_start(runtime_names::RELEASE);
        self.ret();
        self.fn_end();
    }

    /// __karte_string_equal(left_ptr, right_ptr) → 1 或 0
    ///
    /// RDI = left_ptr, RSI = right_ptr
    /// 返回 RAX = 1 (相等) 或 0 (不等)
    ///
    /// 字符串格式：[length: i64][bytes...]
    fn emit_string_equal(&mut self) {
        self.fn_start(runtime_names::STRING_EQUAL);
        // 保存 callee-saved 寄存器
        self.push(5);  // RBP
        self.push(3);  // RBX
        self.push(12); // R12

        // RDI(7) = left_ptr, RSI(6) = right_ptr

        // 同一指针比较
        self.cmp_rr(7, 6);  // CMP RDI, RSI
        self.je_rel32(0); // JE .equal (placeholder)
        let equal_patch_1 = self.code.len() - 4;

        // 空指针检查: test rdi, rdi
        self.test_rr(7, 7);  // TEST RDI, RDI
        self.jz_rel32(0);  // JZ .not_equal
        let not_equal_patch_1 = self.code.len() - 4;

        self.test_rr(6, 6);  // TEST RSI, RSI
        self.jz_rel32(0);  // JZ .not_equal
        let not_equal_patch_2 = self.code.len() - 4;

        // 比较长度: mov rax, [rdi]; mov rcx, [rsi]; cmp rax, rcx
        self.mov_mem_load(0, 7, 0);  // MOV RAX, [RDI+0]
        self.mov_mem_load(1, 6, 0);  // MOV RCX, [RSI+0]
        self.cmp_rr(0, 1);           // CMP RAX, RCX
        self.jne_rel32(0);           // JNE .not_equal
        let not_equal_patch_3 = self.code.len() - 4;

        // 如果长度为 0 → 相等
        self.test_rr(0, 0);          // TEST RAX, RAX
        self.jz_rel32(0);            // JZ .equal
        let equal_patch_2 = self.code.len() - 4;

        // 逐字节比较循环
        // RBX = index = 0
        self.xor_rr(3, 3);  // XOR RBX, RBX

        // .loop:
        let loop_start = self.code.len();

        // 使用 R12 保存 offset = index + 8 (跳过 length header)
        self.mov_rr(12, 3);    // R12 = index
        self.add_ri8(12, 8);   // R12 += 8

        // 计算左地址并加载字节
        self.push(7);           // 保存 RDI
        self.add_rr(7, 12);     // RDI = RDI + offset
        self.movzx_byte(2, 7, 0); // MOVZX RDX, byte [RDI]
        self.pop(7);            // 恢复 RDI

        // 计算右地址并加载字节
        self.push(6);           // 保存 RSI
        self.add_rr(6, 12);     // RSI = RSI + offset
        self.movzx_byte(1, 6, 0); // MOVZX RCX, byte [RSI]
        self.pop(6);            // 恢复 RSI

        // CMP RDX, RCX
        self.cmp_rr(2, 1);
        self.jne_rel32(0);     // JNE .not_equal
        let not_equal_patch_4 = self.code.len() - 4;

        // index++
        self.add_ri8(3, 1);     // INC RBX
        // CMP RBX, RAX (length)
        self.cmp_rr(3, 0);
        self.jl_rel32(0);       // JL .loop
        let loop_patch = self.code.len() - 4;

        // .equal:
        let equal_label = self.code.len();
        self.mov_ri(0, 1);      // MOV RAX, 1
        self.jmp_rel32(0);      // JMP .done
        let done_patch_1 = self.code.len() - 4;

        // .not_equal:
        let not_equal_label = self.code.len();
        self.xor_rr(0, 0);      // XOR RAX, RAX → 0

        // .done:
        let done_label = self.code.len();
        self.pop(12); // R12
        self.pop(3);  // RBX
        self.pop(5);  // RBP
        self.ret();
        self.fn_end();

        // 修补所有跳转
        // equal 跳转
        let rel = equal_label as i32 - (equal_patch_1 as i32 + 4);
        self.code[equal_patch_1..equal_patch_1 + 4].copy_from_slice(&rel.to_le_bytes());
        let rel = equal_label as i32 - (equal_patch_2 as i32 + 4);
        self.code[equal_patch_2..equal_patch_2 + 4].copy_from_slice(&rel.to_le_bytes());

        // not_equal 跳转
        for patch in [not_equal_patch_1, not_equal_patch_2, not_equal_patch_3, not_equal_patch_4] {
            let rel = not_equal_label as i32 - (patch as i32 + 4);
            self.code[patch..patch + 4].copy_from_slice(&rel.to_le_bytes());
        }

        // loop 跳转 (向前跳转，rel 为负数)
        let rel = loop_start as i32 - (loop_patch as i32 + 4);
        self.code[loop_patch..loop_patch + 4].copy_from_slice(&rel.to_le_bytes());

        // done 跳转
        let rel = done_label as i32 - (done_patch_1 as i32 + 4);
        self.code[done_patch_1..done_patch_1 + 4].copy_from_slice(&rel.to_le_bytes());
    }

    /// __karte_string_concat(left_ptr, right_ptr) → 新字符串指针
    ///
    /// RDI(7) = left_ptr, RSI(6) = right_ptr
    /// 返回 RAX = 新字符串指针
    ///
    /// 字符串布局: [length: i64][bytes...]
    /// 算法: 先将 left/right 数据复制到系统栈缓冲区，再调用 gc_alloc 分配，
    /// 最后从栈缓冲区复制到 gc 结果中。避免 gc_alloc 后原始指针失效。
    fn emit_string_concat(&mut self) {
        self.fn_start(runtime_names::STRING_CONCAT);

        // 保存 callee-saved 寄存器
        self.push(5);  // RBP
        self.push(3);  // RBX
        self.push(12); // R12
        self.push(13); // R13

        // 空指针检查: left_ptr == 0 || right_ptr == 0 → 返回 0
        self.test_rr(7, 7); // test RDI, RDI
        self.jz_rel32(0);
        let left_null_patch = self.code.len() - 4;

        self.test_rr(6, 6); // test RSI, RSI
        self.jz_rel32(0);
        let right_null_patch = self.code.len() - 4;

        // R13 = 原始 RSP（push 之后，sub 之前）
        self.mov_rr(13, 4); // R13 = RSP

        // 读长度
        // RBX = left_len = [left_ptr + 0]
        self.mov_mem_load(3, 7, 0); // RBX = [RDI+0]
        // RBP = right_len = [right_ptr + 0]
        self.mov_mem_load(5, 6, 0); // RBP = [RSI+0]
        // R12 = right_ptr（保存，rep movsb 会破坏 RSI）
        self.mov_rr(12, 6); // R12 = RSI

        // ---- 在系统栈上分配临时缓冲区 ----
        // stack_buf_size = (left_len + right_len + 7) & ~7
        self.mov_rr(0, 3);  // RAX = RBX (left_len)
        self.add_rr(0, 5);  // RAX += RBP (right_len)
        self.add_ri8(0, 7); // RAX += 7
        self.and_ri32(0, 0xFFFFFFF8); // RAX &= ~7 (对齐到 8)
        // SUB RSP, RAX
        self.sub_rr(4, 0); // RSP -= RAX

        // ---- 复制 left data 到 [RSP] ----
        // rep movsb: RSI=src, RDI=dst, RCX=count
        self.mov_rr(6, 7);  // RSI = RDI (left_ptr)
        self.add_ri8(6, 8); // RSI = left_ptr + 8 (数据起始)
        self.mov_rr(7, 4);  // RDI = RSP (目标)
        self.mov_rr(1, 3);  // RCX = RBX (left_len)
        self.bs(&[0xF3, 0xA4]); // REP MOVSB

        // ---- 复制 right data 到 [RSP + left_len] ----
        self.mov_rr(6, 12); // RSI = R12 (right_ptr)
        self.add_ri8(6, 8); // RSI = right_ptr + 8
        self.mov_rr(7, 4);  // RDI = RSP
        self.add_rr(7, 3);  // RDI += RBX (left_len)
        self.mov_rr(1, 5);  // RCX = RBP (right_len)
        self.bs(&[0xF3, 0xA4]); // REP MOVSB

        // ---- 计算 total_size = 8 + ((left_len + right_len + 7) & ~7) ----
        self.add_rr(3, 5);  // RBX += RBP → RBX = total_copy_len
        self.mov_rr(0, 3);  // RAX = RBX
        self.add_ri8(0, 7); // RAX += 7
        self.and_ri32(0, 0xFFFFFFF8); // RAX &= ~7
        self.add_ri8(0, 8); // RAX += 8 → total_size

        // ---- 调用 gc_alloc(total_size, 16) ----
        // RDI = total_size, RSI = 16 (align)
        self.mov_rr(7, 0);  // RDI = RAX (total_size)
        self.mov_ri(6, 16); // RSI = 16
        let call_pos = self.code.len();
        self.call_rel32(0); // 占位 CALL，稍后修补到 gc_alloc
        self.functions.push(RuntimeFunction {
            name: "__string_concat_call_alloc".to_string(),
            offset: call_pos,
            size: 5,
        });

        // ---- 检查 gc_alloc 返回值 ----
        self.test_rr(0, 0); // RAX == 0?
        self.jz_rel32(0);   // → OOM
        let oom_patch = self.code.len() - 4;

        // ---- 写 total_len 到 [RAX + 0] ----
        // MOV [RAX+0], RBX (total_copy_len)
        self.mov_mem_store(0, 0, 3);

        // ---- 从栈缓冲区复制到 [RAX + 8] ----
        self.mov_rr(7, 0);  // RDI = RAX (result ptr)
        self.add_ri8(7, 8); // RDI = result + 8 (数据区)
        self.mov_rr(6, 4);  // RSI = RSP (栈缓冲区)
        self.mov_rr(1, 3);  // RCX = RBX (total_copy_len)
        self.bs(&[0xF3, 0xA4]); // REP MOVSB

        // ---- done: 恢复 RSP，返回 RAX ----
        self.mov_rr(4, 13); // RSP = R13 (恢复原始 RSP)
        self.pop(13);
        self.pop(12);
        self.pop(3);
        self.pop(5);
        self.ret();

        // ---- early_null: 空指针，返回 0 ----
        let early_null_label = self.code.len();
        self.xor_rr(0, 0); // RAX = 0
        self.pop(13);
        self.pop(12);
        self.pop(3);
        self.pop(5);
        self.ret();

        // ---- oom: gc_alloc 返回 0，恢复 RSP 并返回 0 ----
        let oom_label = self.code.len();
        self.mov_rr(4, 13); // RSP = R13 (恢复原始 RSP)
        self.xor_rr(0, 0);  // RAX = 0
        self.pop(13);
        self.pop(12);
        self.pop(3);
        self.pop(5);
        self.ret();

        self.fn_end();

        // ---- 修补跳转 ----
        // left_null_patch → early_null_label
        let rel = early_null_label as i32 - (left_null_patch as i32 + 4);
        self.code[left_null_patch..left_null_patch + 4].copy_from_slice(&rel.to_le_bytes());
        // right_null_patch → early_null_label
        let rel = early_null_label as i32 - (right_null_patch as i32 + 4);
        self.code[right_null_patch..right_null_patch + 4].copy_from_slice(&rel.to_le_bytes());
        // oom_patch → oom_label
        let rel = oom_label as i32 - (oom_patch as i32 + 4);
        self.code[oom_patch..oom_patch + 4].copy_from_slice(&rel.to_le_bytes());
    }

    /// __karte_string_char_at(str_ptr, index) → 新字符串指针
    ///
    /// RDI(7) = str_ptr, RSI(6) = index
    /// 返回 RAX = 新字符串指针（单字节字符串）
    ///
    /// 字符串布局: [length: i64][bytes...]
    /// 算法: 先从源字符串读取目标字节到寄存器，再调用 gc_alloc 分配，
    /// 最后写入 length=1 和 byte_val。GC 安全。
    fn emit_string_char_at(&mut self) {
        self.fn_start(runtime_names::STRING_CHAR_AT);

        // 保存 callee-saved 寄存器
        self.push(5);  // RBP
        self.push(3);  // RBX

        // RDI(7) = str_ptr, RSI(6) = index

        // 空指针检查: str_ptr == 0 → 返回 0
        self.test_rr(7, 7); // test RDI, RDI
        self.jz_rel32(0);
        let null_patch = self.code.len() - 4;

        // GC 安全：先从源字符串读取目标字节到 RBX
        // RBX = index + 8 (跳过 length header)
        self.mov_rr(3, 6);   // RBX = RSI (index)
        self.add_ri8(3, 8);  // RBX += 8
        // RBX = byte_val = [str_ptr + RBX]
        self.push(7);         // 保存 RDI
        self.add_rr(7, 3);    // RDI = RDI + offset
        self.movzx_byte(3, 7, 0); // MOVZX RBX, byte [RDI]
        self.pop(7);          // 恢复 RDI

        // 现在安全地分配新内存（GC 可能移动源字符串，但我们已经读取了字节值）
        // RDI = 16 (size), RSI = 8 (align)
        self.mov_ri(7, 16); // RDI = 16
        self.mov_ri(6, 8);  // RSI = 8
        let call_pos = self.code.len();
        self.call_rel32(0); // 占位 CALL，稍后修补到 gc_alloc
        self.functions.push(RuntimeFunction {
            name: "__string_char_at_call_alloc".to_string(),
            offset: call_pos,
            size: 5,
        });

        // 检查 gc_alloc 返回值: RAX == 0?
        self.test_rr(0, 0); // test RAX, RAX
        self.jz_rel32(0);   // → OOM
        let oom_patch = self.code.len() - 4;

        // 写入 length = 1 到 [RAX + 0]
        // 使用 RBP 临时存储 1
        self.mov_ri(5, 1);  // RBP = 1
        self.mov_mem_store(0, 0, 5); // MOV [RAX+0], RBP (length = 1)

        // 写入 byte_val 到 [RAX + 8]
        // RAX 已经是 result ptr，需要计算 RAX+8 并存储 RBX (byte_val)
        self.push(0);       // 保存 RAX
        self.add_ri8(0, 8); // RAX += 8
        // MOV byte [RAX], BL (RBX 低 8 位)
        // 使用 MOV [RAX], BL = 88 1B (store byte from BL)
        // RBX = register 3, BL = 0x1B modrm for [RAX]
        self.bs(&[0x88, 0x18]); // MOV [RAX], BL

        // 清零剩余 7 字节: [RAX+1]..[RAX+7] = 0
        // 使用 XOR ECX, ECX 然后 store byte 7 次
        self.xor_rr(1, 1);  // XOR RCX, RCX = 0
        for i in 1..8u8 {
            // MOV byte [RAX+i], CL = 88 48+i (modrm: [RAX+disp8])
            self.bs(&[0x88, 0x48, i]);
        }

        self.pop(0);        // 恢复 RAX (result ptr)

        // done: 返回 RAX
        self.pop(3);  // RBX
        self.pop(5);  // RBP
        self.ret();

        // null: 空指针，返回 0
        let null_label = self.code.len();
        self.xor_rr(0, 0); // RAX = 0
        self.pop(3);  // RBX
        self.pop(5);  // RBP
        self.ret();

        // oom: gc_alloc 返回 0，返回 0
        let oom_label = self.code.len();
        self.xor_rr(0, 0); // RAX = 0
        self.pop(3);  // RBX
        self.pop(5);  // RBP
        self.ret();

        self.fn_end();

        // 修补跳转
        // null_patch → null_label
        let rel = null_label as i32 - (null_patch as i32 + 4);
        self.code[null_patch..null_patch + 4].copy_from_slice(&rel.to_le_bytes());
        // oom_patch → oom_label
        let rel = oom_label as i32 - (oom_patch as i32 + 4);
        self.code[oom_patch..oom_patch + 4].copy_from_slice(&rel.to_le_bytes());
    }

    /// __karte_string_trim(str_ptr) → 新字符串指针
    ///
    /// RDI(7) = str_ptr
    /// 返回 RAX = 新字符串指针（已 trim）
    ///
    /// 字符串布局: [length: i64][bytes...]
    /// 算法:
    ///   1. 空指针检查
    ///   2. 读 len，如果 len == 0 返回原指针
    ///   3. GC 安全：用 REP MOVSB 将源字符串数据复制到系统栈缓冲区
    ///   4. 扫描 start：从前往后找到第一个非空格位置
    ///   5. 扫描 end：从后往前找到第一个非空格位置
    ///   6. 如果 trimmed_len == 0：分配 16 字节空字符串
    ///   7. 否则：分配 total_size = 8 + ((trimmed_len + 7) & !7)，复制数据
    ///   8. 清零尾部对齐字节
    fn emit_trim(&mut self) {
        self.fn_start(runtime_names::TRIM);

        // 保存 callee-saved 寄存器
        self.push(5);  // RBP
        self.push(3);  // RBX
        self.push(12); // R12
        self.push(13); // R13

        // RDI(7) = str_ptr

        // ---- 空指针检查: str_ptr == 0 → 返回 0 ----
        self.test_rr(7, 7); // test RDI, RDI
        self.jz_rel32(0);
        let null_patch = self.code.len() - 4;

        // ---- 读 len = [str_ptr + 0] ----
        // RBX = len
        self.mov_mem_load(3, 7, 0); // RBX = [RDI+0]

        // ---- 如果 len == 0 → 返回原指针 ----
        self.test_rr(3, 3); // test RBX, RBX
        self.jz_rel32(0);
        let zero_len_patch = self.code.len() - 4;

        // ---- R12 = str_ptr（保存，rep movsb 会破坏 RSI） ----
        self.mov_rr(12, 7); // R12 = RDI (str_ptr)

        // ---- R13 = 原始 RSP（push 之后，sub 之前） ----
        self.mov_rr(13, 4); // R13 = RSP

        // ---- 在系统栈上分配缓冲区，大小 = (len + 7) & ~7 ----
        // RAX = (RBX + 7) & ~7
        self.mov_rr(0, 3);  // RAX = RBX (len)
        self.add_ri8(0, 7); // RAX += 7
        self.and_ri32(0, 0xFFFFFFF8); // RAX &= ~7
        // SUB RSP, RAX
        self.sub_rr(4, 0); // RSP -= RAX

        // ---- 用 REP MOVSB 将源字符串数据复制到栈缓冲区 ----
        // RSI = str_ptr + 8 (数据起始)
        self.mov_rr(6, 12); // RSI = R12 (str_ptr)
        self.add_ri8(6, 8); // RSI += 8
        // RDI = RSP (目标)
        self.mov_rr(7, 4); // RDI = RSP
        // RCX = RBX (len)
        self.mov_rr(1, 3); // RCX = RBX
        self.bs(&[0xF3, 0xA4]); // REP MOVSB

        // ---- 现在 RSP 指向栈缓冲区，包含完整的字符串数据 ----
        // ---- 扫描 start：从前往后找到第一个非空格位置 ----
        // RBP = start = 0
        self.xor_rr(5, 5); // RBP = 0

        // scan_start_loop: while start < len && buf[start] == 0x20
        let scan_start_loop = self.code.len();
        // CMP RBP, RBX (start >= len?)
        self.cmp_rr(5, 3); // CMP RBP, RBX
        self.jae_rel32(0); // 如果 start >= len，跳到 scan_start_done
        let scan_start_exit_patch = self.code.len() - 4;

        // 检查 buf[start] == 0x20
        // RAX 不需要保存 — 每次循环都重新计算
        self.mov_rr(0, 4); // RAX = RSP
        self.add_rr(0, 5); // RAX += RBP (start)
        self.movzx_byte(11, 0, 0); // R11 = byte [RAX + 0] = buf[start]

        // CMP R11, 0x20 (空格)
        self.bs(&[0x49, 0x83, 0xFB, 0x20]); // CMP R11, 0x20
        self.jne_rel32(0); // 如果不等于空格，跳到 scan_start_done
        let scan_start_found_patch = self.code.len() - 4;

        // start++
        self.add_ri8(5, 1); // RBP += 1

        // JMP scan_start_loop
        self.jmp_rel32(0);
        let jmp_scan_start_patch = self.code.len() - 4;
        // 修补: JMP 回到 scan_start_loop
        let rel = scan_start_loop as i32 - (jmp_scan_start_patch as i32 + 4);
        self.code[jmp_scan_start_patch..jmp_scan_start_patch + 4].copy_from_slice(&rel.to_le_bytes());

        // scan_start_done:
        let scan_start_done = self.code.len();
        // 修补 scan_start 退出跳转
        let rel = scan_start_done as i32 - (scan_start_exit_patch as i32 + 4);
        self.code[scan_start_exit_patch..scan_start_exit_patch + 4].copy_from_slice(&rel.to_le_bytes());
        let rel = scan_start_done as i32 - (scan_start_found_patch as i32 + 4);
        self.code[scan_start_found_patch..scan_start_found_patch + 4].copy_from_slice(&rel.to_le_bytes());

        // ---- 扫描 end：从后往前找到第一个非空格位置 ----
        // R11 = end = len (RBX)
        self.mov_rr(11, 3); // R11 = RBX (len)

        // scan_end_loop: while end > start && buf[end-1] == 0x20
        let scan_end_loop = self.code.len();
        // CMP R11, RBP (end <= start?)
        self.cmp_rr(11, 5); // CMP R11, RBP
        self.jbe_rel32(0); // 如果 end <= start，跳到 scan_end_done
        let scan_end_exit_patch = self.code.len() - 4;

        // 检查 buf[end-1] == 0x20
        // RAX 不需要保存 — 每次循环都重新计算
        self.mov_rr(0, 4); // RAX = RSP
        self.add_rr(0, 11); // RAX += end
        self.sub_ri8(0, 1); // RAX -= 1 → RAX = RSP + end - 1
        self.movzx_byte(10, 0, 0); // R10 = byte [RAX + 0]

        // CMP R10, 0x20
        self.bs(&[0x49, 0x83, 0xFA, 0x20]); // CMP R10, 0x20
        self.jne_rel32(0); // 如果不等于空格，跳到 scan_end_done
        let scan_end_found_patch = self.code.len() - 4;

        // end--
        // SUB R11, 1: R11 是 reg 11, reg_ext(11) = true
        self.bs(&[0x49, 0x83, 0xEB, 0x01]); // SUB R11, 1

        // JMP scan_end_loop
        self.jmp_rel32(0);
        let jmp_scan_end_patch = self.code.len() - 4;
        let rel = scan_end_loop as i32 - (jmp_scan_end_patch as i32 + 4);
        self.code[jmp_scan_end_patch..jmp_scan_end_patch + 4].copy_from_slice(&rel.to_le_bytes());

        // scan_end_done:
        let scan_end_done = self.code.len();
        let rel = scan_end_done as i32 - (scan_end_exit_patch as i32 + 4);
        self.code[scan_end_exit_patch..scan_end_exit_patch + 4].copy_from_slice(&rel.to_le_bytes());
        let rel = scan_end_done as i32 - (scan_end_found_patch as i32 + 4);
        self.code[scan_end_found_patch..scan_end_found_patch + 4].copy_from_slice(&rel.to_le_bytes());

        // ---- trimmed_len = end - start = R11 - RBP ----
        self.mov_rr(0, 11); // RAX = R11 (end)
        self.sub_rr(0, 5);  // RAX -= RBP (start) → RAX = trimmed_len

        // ---- 如果 trimmed_len == 0：分配空字符串 ----
        self.test_rr(0, 0);
        self.jz_rel32(0);
        let trimmed_zero_patch = self.code.len() - 4;

        // ---- 计算 total_size = 8 + ((trimmed_len + 7) & !7) ----
        // RAX = trimmed_len
        // R10 = trimmed_len (保存，后面复制用)
        self.mov_rr(10, 0);  // R10 = trimmed_len
        self.add_ri8(0, 7);  // RAX += 7
        self.and_ri32(0, 0xFFFFFFF8); // RAX &= ~7
        self.add_ri8(0, 8);  // RAX += 8 → total_size

        // ---- 调用 gc_alloc(total_size, 16) ----
        self.mov_rr(7, 0);  // RDI = total_size
        self.mov_ri(6, 16); // RSI = 16 (align)
        let call_pos = self.code.len();
        self.call_rel32(0); // 占位 CALL，稍后修补到 gc_alloc
        self.functions.push(RuntimeFunction {
            name: "__trim_call_alloc".to_string(),
            offset: call_pos,
            size: 5,
        });

        // ---- 检查 gc_alloc 返回值 ----
        self.test_rr(0, 0); // RAX == 0?
        self.jz_rel32(0);   // → OOM
        let oom_patch = self.code.len() - 4;

        // ---- 写 trimmed_len 到 [RAX + 0] ----
        self.mov_mem_store(0, 0, 10); // MOV [RAX+0], R10 (trimmed_len)

        // ---- 从栈缓冲区 [start..end] 复制到 [RAX + 8] ----
        // RSI = RSP + start (RBP)
        self.mov_rr(6, 4); // RSI = RSP
        self.add_rr(6, 5); // RSI += RBP (start)
        // RDI = RAX + 8
        self.push(0); // 保存 RAX (result ptr)
        self.mov_rr(7, 0); // RDI = RAX (result ptr)
        self.add_ri8(7, 8); // RDI = RAX + 8
        // RCX = trimmed_len (R10)
        self.mov_rr(1, 10); // RCX = R10
        self.bs(&[0xF3, 0xA4]); // REP MOVSB

        // ---- 清零尾部对齐字节 ----
        // remaining = trimmed_len % 8
        // 如果 remaining == 0，不需要清零
        // padding_start = RDI (当前已经指向数据末尾)
        // padding_count = (8 - remaining) % 8
        self.pop(0); // 恢复 RAX (result ptr)
        // 计算 remaining = trimmed_len & 7
        self.mov_rr(1, 10); // RCX = trimmed_len
        self.and_ri32(1, 7); // RCX &= 7 → remaining
        self.jz_rel32(0);    // 如果 remaining == 0，跳过清零
        let no_padding_patch = self.code.len() - 4;
        // padding_count = 8 - remaining
        self.mov_ri(2, 8); // RDX = 8
        self.sub_rr(2, 1); // RDX -= RCX → padding_count
        // 计算 padding_start = RAX + 8 + trimmed_len
        self.push(0); // 保存 RAX
        self.add_ri8(0, 8); // RAX += 8
        self.add_rr(0, 10); // RAX += trimmed_len → padding_start
        // 清零循环：XOR CL(此处用 R11), MOV byte [RAX+i], CL
        // 实际上用 R11 存 0，然后循环写
        self.xor_rr(11, 11); // R11 = 0
        // padding_count 最大为 7，直接逐字节写
        // 用 RDX 作为循环计数器，RAX 作为地址
        let pad_loop = self.code.len();
        self.test_rr(2, 2); // TEST RDX, RDX
        self.jz_rel32(0);   // 如果 RDX == 0，跳到 pad_done
        let pad_done_patch = self.code.len() - 4;
        // MOV byte [RAX], R11L
        // REX.R=1 (R11 扩展), REX.B=0 (RAX): 0x44
        // ModRM: reg=011(R11L低3位), r/m=000(RAX低3位), mod=00 → 0x18
        self.bs(&[0x44, 0x88, 0x18]); // MOV byte [RAX], R11L
        self.add_ri8(0, 1); // RAX++ (地址递增)
        self.sub_ri8(2, 1); // RDX-- (计数递减)
        self.jmp_rel32(0);
        let jmp_pad_patch = self.code.len() - 4;
        let rel = pad_loop as i32 - (jmp_pad_patch as i32 + 4);
        self.code[jmp_pad_patch..jmp_pad_patch + 4].copy_from_slice(&rel.to_le_bytes());

        let pad_done = self.code.len();
        let rel = pad_done as i32 - (pad_done_patch as i32 + 4);
        self.code[pad_done_patch..pad_done_patch + 4].copy_from_slice(&rel.to_le_bytes());
        self.pop(0); // 恢复 RAX

        // ---- done: 恢复 RSP，返回 RAX ----
        let no_padding_label = self.code.len();
        self.mov_rr(4, 13); // RSP = R13 (恢复原始 RSP)
        self.pop(13);
        self.pop(12);
        self.pop(3);
        self.pop(5);
        self.ret();

        // ---- null: 空指针，返回 0 ----
        let null_label = self.code.len();
        self.xor_rr(0, 0); // RAX = 0
        self.pop(13);
        self.pop(12);
        self.pop(3);
        self.pop(5);
        self.ret();

        // ---- zero_len: len == 0，返回原指针 ----
        // 此时 RDI(7) 仍然是原指针
        let zero_len_label = self.code.len();
        self.mov_rr(0, 7); // RAX = RDI (原指针)
        self.pop(13);
        self.pop(12);
        self.pop(3);
        self.pop(5);
        self.ret();

        // ---- trimmed_zero: trimmed_len == 0，分配 16 字节空字符串 ----
        let trimmed_zero_label = self.code.len();
        self.mov_ri(7, 16); // RDI = 16 (size)
        self.mov_ri(6, 16); // RSI = 16 (align)
        let call_pos2 = self.code.len();
        self.call_rel32(0); // 占位 CALL gc_alloc
        self.functions.push(RuntimeFunction {
            name: "__trim_call_alloc".to_string(),
            offset: call_pos2,
            size: 5,
        });
        // 检查返回值
        self.test_rr(0, 0);
        self.jz_rel32(0);
        let trimmed_zero_oom_patch = self.code.len() - 4;
        // 写入 length = 0
        self.push(0);
        self.xor_rr(2, 2); // RDX = 0
        self.mov_mem_store(0, 0, 2); // [RAX+0] = 0
        self.pop(0);
        // 恢复 RSP 并返回
        self.mov_rr(4, 13); // RSP = R13
        self.pop(13);
        self.pop(12);
        self.pop(3);
        self.pop(5);
        self.ret();

        // ---- oom: gc_alloc 返回 0，恢复 RSP 并返回 0 ----
        let oom_label = self.code.len();
        self.mov_rr(4, 13); // RSP = R13
        self.xor_rr(0, 0);  // RAX = 0
        self.pop(13);
        self.pop(12);
        self.pop(3);
        self.pop(5);
        self.ret();

        // ---- trimmed_zero_om: trimmed_zero 中 gc_alloc 返回 0 ----
        let trimmed_zero_oom_label = self.code.len();
        self.mov_rr(4, 13); // RSP = R13
        self.xor_rr(0, 0);
        self.pop(13);
        self.pop(12);
        self.pop(3);
        self.pop(5);
        self.ret();

        self.fn_end();

        // ---- 修补所有跳转 ----
        // null_patch → null_label
        let rel = null_label as i32 - (null_patch as i32 + 4);
        self.code[null_patch..null_patch + 4].copy_from_slice(&rel.to_le_bytes());
        // zero_len_patch → zero_len_label
        let rel = zero_len_label as i32 - (zero_len_patch as i32 + 4);
        self.code[zero_len_patch..zero_len_patch + 4].copy_from_slice(&rel.to_le_bytes());
        // trimmed_zero_patch → trimmed_zero_label
        let rel = trimmed_zero_label as i32 - (trimmed_zero_patch as i32 + 4);
        self.code[trimmed_zero_patch..trimmed_zero_patch + 4].copy_from_slice(&rel.to_le_bytes());
        // oom_patch → oom_label
        let rel = oom_label as i32 - (oom_patch as i32 + 4);
        self.code[oom_patch..oom_patch + 4].copy_from_slice(&rel.to_le_bytes());
        // no_padding_patch → no_padding_label
        let rel = no_padding_label as i32 - (no_padding_patch as i32 + 4);
        self.code[no_padding_patch..no_padding_patch + 4].copy_from_slice(&rel.to_le_bytes());
        // trimmed_zero_oom_patch → trimmed_zero_oom_label
        let rel = trimmed_zero_oom_label as i32 - (trimmed_zero_oom_patch as i32 + 4);
        self.code[trimmed_zero_oom_patch..trimmed_zero_oom_patch + 4].copy_from_slice(&rel.to_le_bytes());
    }

    /// __karte_to_string(value) → 新字符串指针
    ///
    /// RDI(7) = value (i64)
    /// 返回 RAX = 新字符串指针
    ///
    /// 字符串布局: [length: i64][bytes...]
    /// 算法: 在栈上构建数字字符串（最多 20 字节: -9223372036854775808），
    /// 然后调用 gc_alloc 分配，拷贝到 GC 对象。
    fn emit_to_string(&mut self) {
        self.fn_start(runtime_names::TO_STRING);

        // 保存 callee-saved 寄存器
        self.push(5);  // RBP
        self.push(3);  // RBX
        self.push(12); // R12
        self.push(13); // R13
        self.push(14); // R14
        self.push(15); // R15

        // 分配 32 字节栈空间作为临时 buffer（对齐到 16 字节）
        // 使用 RSP 下面的空间：sub rsp, 32
        self.bs(&[0x48, 0x83, 0xEC, 0x20]); // SUB RSP, 32

        // RDI(7) = value
        // R15 = value (保存参数)
        self.mov_rr(15, 7); // R15 = value

        // R14 = buffer start = RSP (栈上 buffer 的起始地址)
        self.mov_rr(14, 4); // R14 = RSP (buffer start)

        // R12 = buffer end (写入位置，从末尾往前写)
        // R12 = RSP + 20 (数字最多 20 字节)
        self.mov_rr(12, 4); // R12 = RSP
        self.add_ri8(12, 20); // R12 += 20

        // 处理负数
        // R13 = is_negative flag
        self.xor_rr(13, 13); // R13 = 0 (false)
        // 检查 value < 0
        self.bs(&[0x48, 0x85, 0xFF]); // TEST RDI, RDI
        self.bs(&[0x0F, 0x89, 0x00, 0x00, 0x00, 0x00]); // JNS skip_neg (6 bytes)
        let neg_patch = self.code.len() - 4;

        // 负数: R13 = 1, value = -value
        self.mov_ri(13, 1); // R13 = 1 (is_negative)
        // NEG R15
        self.bs(&[0x49, 0xF7, 0xDF]); // NEG R15

        // skip_neg:
        let skip_neg = self.code.len();
        let rel = skip_neg as i32 - (neg_patch as i32 + 4);
        self.code[neg_patch..neg_patch + 4].copy_from_slice(&rel.to_le_bytes());

        // 特殊情况: value == 0
        // 使用 RBX 保存 digit count
        self.mov_ri(3, 0); // RBX = 0 (digit count)
        // 如果 R15 != 0, 跳到循环
        self.test_rr(15, 15);
        self.bs(&[0x0F, 0x85, 0x00, 0x00, 0x00, 0x00]); // JNZ digit_loop
        let zero_patch = self.code.len() - 4;

        // value == 0: 写入 '0' 到 buffer end - 1
        self.bs(&[0x49, 0x83, 0xEC, 0x01]); // SUB R12, 1
        // MOV byte [R12], '0' (0x30)
        // MOV [R12], 0x30 → 使用 RAX 作为临时
        self.push(0); // 保存 RAX
        self.mov_ri(0, 0x30); // RAX = '0'
        // MOV [R12], AL = 88 04 24 → 但 R12 是 R12
        // 88 04 24 = MOV [RSP], AL
        // R12 是寄存器 12, MODRM for [R12] needs REX.B
        self.bs(&[0x41, 0x88, 0x04, 0x24]); // MOV [R12], AL
        self.pop(0); // 恢复 RAX
        self.mov_ri(3, 1); // RBX = 1 (1 digit)
        self.bs(&[0xE9, 0x00, 0x00, 0x00, 0x00]); // JMP after_loop
        let after_zero_patch = self.code.len() - 4;

        // digit_loop: 逐位转换 (从后往前)
        let digit_loop = self.code.len();
        let rel = digit_loop as i32 - (zero_patch as i32 + 4);
        self.code[zero_patch..zero_patch + 4].copy_from_slice(&rel.to_le_bytes());

        // 循环: while R15 != 0
        // digit = R15 % 10
        // R15 = R15 / 10
        // 这里需要除以 10, 使用乘法逆元避免 DIV 指令
        // 或者用简单的循环减法

        // 使用 XOR DX,DX + DIV 方式
        // MOV RAX, R15
        self.mov_rr(0, 15); // RAX = R15
        // XOR EDX, EDX (清零 RDX)
        self.xor_rr(2, 2);
        // MOV RCX, 10
        self.mov_ri(1, 10); // RCX = 10
        // DIV RCX → RAX = R15/10, RDX = R15%10
        self.bs(&[0x48, 0xF7, 0xF1]); // DIV RCX

        // R15 = RAX (商)
        self.mov_rr(15, 0); // R15 = quotient
        // digit = RDX (余数) + '0'
        self.add_ri8(2, 0x30); // RDX += '0'

        // buffer[--R12] = digit
        self.bs(&[0x49, 0x83, 0xEC, 0x01]); // SUB R12, 1
        // MOV [R12], DL → DL 是 RDX 的低 8 位
        // REX.B + MOV [R12], DL = 41 88 14 24
        self.bs(&[0x41, 0x88, 0x14, 0x24]); // MOV [R12], DL

        // RBX += 1 (digit count)
        self.add_ri8(3, 1); // RBX += 1

        // if R15 != 0, continue loop
        self.test_rr(15, 15);
        // JNZ short jump backward to digit_loop
        // 占位 2 字节: 75 XX
        self.bs(&[0x75, 0x00]); // 占位 JNZ
        let jnz_pos = self.code.len() - 1;
        let loop_back_rel = digit_loop as i32 - (jnz_pos as i32 + 1);
        self.code[jnz_pos] = loop_back_rel as u8;

        // after_loop:
        let after_loop = self.code.len();
        let rel = after_loop as i32 - (after_zero_patch as i32 + 4);
        self.code[after_zero_patch..after_zero_patch + 4].copy_from_slice(&rel.to_le_bytes());

        // 如果 is_negative (R13 == 1), 在前面加 '-'
        self.test_rr(13, 13);
        self.bs(&[0x74, 0x18]); // JZ skip_sign (+24 bytes)
        // buffer[--R12] = '-'
        self.bs(&[0x49, 0x83, 0xEC, 0x01]); // SUB R12, 1
        self.push(0); // save RAX
        self.mov_ri(0, 0x2D); // RAX = '-'
        self.bs(&[0x41, 0x88, 0x04, 0x24]); // MOV [R12], AL
        self.pop(0);
        self.add_ri8(3, 1); // RBX += 1 (负号算一个字符)

        // skip_sign:
        // 现在 R12 = 字符串起始, RBX = 字符串长度
        // 计算分配大小: ((RBX + 7) / 8) * 8 + 8
        // 简化: RBX + 16 足够（对齐到 8 + 8 字节 header）
        // 使用 RAX = RBX, 向上对齐到 8, 再 +8
        self.mov_rr(0, 3); // RAX = RBX (length)
        self.add_ri8(0, 7); // RAX += 7
        self.bs(&[0x48, 0x25, 0xF8, 0xFF, 0xFF, 0xFF]); // AND RAX, ~7 (clear low 3 bits)
        self.add_ri8(0, 8); // RAX += 8 (header)

        // 调用 gc_alloc(RAX, 8)
        // RDI = size, RSI = alignment
        self.mov_rr(7, 0); // RDI = size
        self.mov_ri(6, 8);  // RSI = 8 (alignment)
        let call_pos = self.code.len();
        self.call_rel32(0); // CALL gc_alloc (占位)
        self.functions.push(RuntimeFunction {
            name: "__to_string_call_alloc".to_string(),
            offset: call_pos,
            size: 5,
        });

        // 检查 gc_alloc 返回值
        self.test_rr(0, 0);
        self.bs(&[0x0F, 0x84, 0x00, 0x00, 0x00, 0x00]); // JZ oom
        let oom_patch = self.code.len() - 4;

        // 写入 length header: [RAX] = RBX
        // MOV [RAX], RBX
        self.bs(&[0x48, 0x89, 0x18]); // MOV [RAX], RBX

        // 拷贝字符串数据: [RAX+8] = [R12] for RBX bytes
        // 使用 RSI 作为 src, RDI 作为 dst, RCX 作为 count
        // 保存需要的寄存器
        self.push(0); // 保存 RAX (result ptr)
        // RSI = R12 (src)
        self.mov_rr(6, 12); // RSI = R12 (src)
        // RDI = RAX + 8 (dst)
        self.mov_rr(7, 0); // RDI = RAX (alloc result)
        self.add_ri8(7, 8); // RDI = RAX + 8
        // RCX = RBX (count)
        self.mov_rr(1, 3); // RCX = RBX

        // 逐字节拷贝循环
        // 记录循环起点
        let copy_loop_start = self.code.len();
        // copy_loop: if RCX == 0, done
        self.bs(&[0x48, 0x85, 0xC9]); // TEST RCX, RCX
        self.bs(&[0x74, 0x00]); // JZ copy_done (占位, +0)
        let jz_copy_done_patch = self.code.len() - 1;

        // MOV AL, [RSI]
        self.bs(&[0x8A, 0x06]); // MOV AL, [RSI]
        // MOV [RDI], AL
        self.bs(&[0x88, 0x07]); // MOV [RDI], AL
        // INC RSI, INC RDI, DEC RCX
        self.bs(&[0x48, 0xFF, 0xC6]); // INC RSI
        self.bs(&[0x48, 0xFF, 0xC7]); // INC RDI
        self.bs(&[0x48, 0xFF, 0xC9]); // DEC RCX
        // JMP copy_loop
        self.bs(&[0xEB, 0x00]); // 占位 JMP
        let jmp_back_patch = self.code.len() - 1;

        // 修补: JMP back to copy_loop_start
        let jmp_back_rel = copy_loop_start as i32 - (jmp_back_patch as i32 + 1);
        self.code[jmp_back_patch] = jmp_back_rel as u8;

        // copy_done label:
        let copy_done_label = self.code.len();
        // 修补: JZ copy_done
        let jz_rel = copy_done_label as i32 - (jz_copy_done_patch as i32 + 1);
        self.code[jz_copy_done_patch] = jz_rel as u8;
        // copy_test 的位置是 push(1) 之后的位置
        // 让我直接使用 copy_test_label

        // copy_done:
        // 清零填充字节 (已经由 gc_alloc 的 mmap 初始化为 0)
        // 恢复 RAX
        self.pop(0); // RAX = result ptr

        // done: 返回 RAX
        // 恢复栈
        self.bs(&[0x48, 0x83, 0xC4, 0x20]); // ADD RSP, 32
        self.pop(15); // R15
        self.pop(14); // R14
        self.pop(13); // R13
        self.pop(12); // R12
        self.pop(3);  // RBX
        self.pop(5);  // RBP
        self.ret();

        // oom: 返回 0
        let oom_label = self.code.len();
        self.bs(&[0x48, 0x83, 0xC4, 0x20]); // ADD RSP, 32
        self.pop(15);
        self.pop(14);
        self.pop(13);
        self.pop(12);
        self.pop(3);
        self.pop(5);
        self.xor_rr(0, 0);
        self.ret();

        self.fn_end();

        // 修补 oom 跳转
        let rel = oom_label as i32 - (oom_patch as i32 + 4);
        self.code[oom_patch..oom_patch + 4].copy_from_slice(&rel.to_le_bytes());
    }

    /// __karte_print_string(str_ptr) → 0
    ///
    /// RDI = str_ptr
    /// 使用 sys_write(1, data, len) 输出字符串内容
    fn emit_print_string(&mut self) {
        self.fn_start(runtime_names::PRINT_STRING);
        self.push(5);  // RBP
        self.push(3);  // RBX
        self.push(12); // R12

        // RDI(7) = str_ptr
        // 空指针检查
        self.test_rr(7, 7);
        self.jz_rel32(0);
        let null_patch = self.code.len() - 4;

        // R12 = str_ptr (保存)
        self.mov_rr(12, 7);

        // 读取长度: RBX = [str_ptr]
        self.mov_mem_load(3, 12, 0); // RBX = length

        // 检查 len > 0
        self.test_rr(3, 3);
        self.jz_rel32(0);
        let zero_len_patch = self.code.len() - 4;

        // sys_write(1, data_ptr, len)
        // data_ptr = str_ptr + 8
        self.mov_ri(0, 1);           // RAX = syscall 1 (write)
        self.mov_ri(7, 1);           // RDI = fd 1 (stdout)
        self.mov_rr(6, 12);          // RSI = str_ptr
        self.add_ri8(6, 8);          // RSI = str_ptr + 8 (data)
        self.mov_rr(2, 3);           // RDX = length
        self.syscall();

        // 输出换行符: sys_write(1, &"\n", 1)
        self.sub_ri8(4, 1);          // RSP -= 1
        self.bs(&[0xC6, 0x04, 0x24, 0x0A]); // MOV byte [RSP], '\n'
        self.mov_ri(0, 1);           // syscall 1 (write)
        self.mov_ri(7, 1);           // fd 1 (stdout)
        self.mov_rr(6, 4);           // RSI = RSP
        self.mov_ri(2, 1);           // RDX = 1
        self.syscall();
        self.add_ri8(4, 1);          // RSP += 1 (恢复)

        // done:
        let done_label = self.code.len();
        self.xor_rr(0, 0);           // RAX = 0 (Unit)
        self.pop(12);
        self.pop(3);
        self.pop(5);
        self.ret();
        self.fn_end();

        // 修补跳转
        let rel = done_label as i32 - (null_patch as i32 + 4);
        self.code[null_patch..null_patch + 4].copy_from_slice(&rel.to_le_bytes());
        let rel = done_label as i32 - (zero_len_patch as i32 + 4);
        self.code[zero_len_patch..zero_len_patch + 4].copy_from_slice(&rel.to_le_bytes());
    }

    /// __karte_print_number(value) → 0
    ///
    /// RDI = i64 value
    /// 将数值转换为十进制 ASCII 并通过 sys_write 输出
    fn emit_print_number(&mut self) {
        self.fn_start(runtime_names::PRINT_NUMBER);
        self.push(5);  // RBP
        self.push(3);  // RBX
        self.push(8);  // R8
        self.push(9);  // R9

        // RDI(7) = value
        // RBX = 原始值备份
        self.mov_rr(3, 7);

        // 处理负数
        self.mov_rr(0, 3);    // RAX = value
        self.xor_rr(2, 2);    // RDX = 0 (符号: 0=正)
        self.mov_ri(1, 0);
        self.cmp_rr(0, 1);
        self.jge_rel32(0);
        let neg_skip = self.code.len() - 4;

        // 负数: 标记符号, 取绝对值
        self.mov_ri(2, 1);    // RDX = 1 (负数)
        self.mov_rr(1, 0);    // RCX = RAX
        self.xor_rr(0, 0);    // RAX = 0
        self.sub_rr(0, 1);    // RAX = -RCX

        // neg_skip:
        let neg_skip_label = self.code.len();

        // 初始化计数器和常量
        self.xor_rr(8, 8);    // R8 = 字符计数 = 0
        self.mov_ri(9, 10);   // R9 = 10

        // 先推入 '\n'
        self.sub_ri8(4, 1);   // RSP -= 1
        self.bs(&[0xC6, 0x04, 0x24, b'\n']);
        self.add_ri8(8, 1);   // 计数 = 1

        // 处理 RAX == 0
        self.test_rr(0, 0);
        self.jne_rel32(0);
        let nonzero = self.code.len() - 4;

        // RAX == 0: 写 '0'
        self.sub_ri8(4, 1);
        self.bs(&[0xC6, 0x04, 0x24, b'0']);
        self.add_ri8(8, 1);
        let zero_done = self.code.len();
        self.jmp_rel32(0); // → sys_write

        // digit_loop:
        let digit_loop = self.code.len();

        self.test_rr(0, 0);
        self.jz_rel32(0);
        let digit_done = self.code.len() - 4;

        // IDIV R9: RAX = 商, RDX = 余数
        self.mov_rr(11, 2);   // R11 = 符号标志
        self.bs(&[0x48, 0x99]); // CQO
        self.bs(&[0x49, 0xF7, 0xF9]); // IDIV R9
        self.add_ri8(2, b'0' as u8); // RDX = ASCII 数字
        self.sub_ri8(4, 1);
        self.bs(&[0x88, 0x14, 0x24]); // MOV byte [RSP], DL
        self.add_ri8(8, 1);
        self.mov_rr(2, 11);   // 恢复符号标志

        // 跳回循环
        {
            let rel = (digit_loop as i64 - (self.code.len() as i64 + 5)) as i32;
            self.jmp_rel32(rel);
        }

        // digit_done_label:
        let digit_done_label = self.code.len();

        // 负数推入 '-'
        self.test_rr(2, 2);
        self.jz_rel32(0);
        let no_neg_sign = self.code.len() - 4;

        self.sub_ri8(4, 1);
        self.bs(&[0xC6, 0x04, 0x24, b'-']);
        self.add_ri8(8, 1);

        // no_neg_sign_label:
        let no_neg_sign_label = self.code.len();

        // sys_write(1, RSP, R8)
        self.mov_ri(0, 1);   // syscall: write
        self.mov_ri(7, 1);   // fd: stdout
        self.mov_rr(6, 4);   // buf: RSP
        self.mov_rr(2, 8);   // count: R8
        self.syscall();

        // 恢复 RSP: 移除推入的数字字符（R8 = 字符计数，syscall 后 R8 保留）
        self.add_rr(4, 8);  // RSP += R8

        // 恢复寄存器
        self.xor_rr(0, 0);   // RAX = 0 (Unit)
        self.pop(9);
        self.pop(8);
        self.pop(3);
        self.pop(5);
        self.ret();
        self.fn_end();

        // 修补跳转
        // neg_skip (JGE) → neg_skip_label
        let rel = neg_skip_label as i32 - (neg_skip as i32 + 4);
        self.code[neg_skip..neg_skip + 4].copy_from_slice(&rel.to_le_bytes());
        // nonzero (JNE) → digit_loop
        let rel = digit_loop as i32 - (nonzero as i32 + 4);
        self.code[nonzero..nonzero + 4].copy_from_slice(&rel.to_le_bytes());
        // zero_done (JMP) → no_neg_sign_label
        let rel = no_neg_sign_label as i32 - (zero_done as i32 + 4);
        self.code[zero_done..zero_done + 4].copy_from_slice(&rel.to_le_bytes());
        // digit_done (JZ) → digit_done_label
        let rel = digit_done_label as i32 - (digit_done as i32 + 4);
        self.code[digit_done..digit_done + 4].copy_from_slice(&rel.to_le_bytes());
        // no_neg_sign (JZ) → no_neg_sign_label
        let rel = no_neg_sign_label as i32 - (no_neg_sign as i32 + 4);
        self.code[no_neg_sign..no_neg_sign + 4].copy_from_slice(&rel.to_le_bytes());
    }

    /// __karte_print_bool(value) → 0
    ///
    /// RDI(7) = i64 value
    /// value != 0 输出 "true\n"，value == 0 输出 "false\n"
    fn emit_print_bool(&mut self) {
        self.fn_start(runtime_names::PRINT_BOOL);

        // RDI(7) = value
        // 判断 value == 0
        self.test_rr(7, 7);
        self.jne_rel32(0); // → true_label
        let false_skip = self.code.len() - 4;

        // false 分支: 输出 "false\n" (6 bytes)
        self.push(0);  // 在栈上分配 8 字节空间
        self.mov_byte_mem_imm(4, 0, 0x66); // [RSP+0] = 'f'
        self.mov_byte_mem_imm(4, 1, 0x61); // [RSP+1] = 'a'
        self.mov_byte_mem_imm(4, 2, 0x6C); // [RSP+2] = 'l'
        self.mov_byte_mem_imm(4, 3, 0x73); // [RSP+3] = 's'
        self.mov_byte_mem_imm(4, 4, 0x65); // [RSP+4] = 'e'
        self.mov_byte_mem_imm(4, 5, 0x0A); // [RSP+5] = '\n'
        // sys_write(1, RSP, 6)
        self.mov_ri(0, 1);  // syscall: write
        self.mov_ri(7, 1);  // fd: stdout
        self.mov_rr(6, 4);  // buf: RSP
        self.mov_ri(2, 6);  // count: 6
        self.syscall();
        self.pop(0);  // 恢复 RSP
        // → done
        let false_done = self.code.len();
        self.jmp_rel32(0);
        let done_patch = self.code.len() - 4;

        // true_label:
        let true_label = self.code.len();
        // 输出 "true\n" (5 bytes)
        self.push(0);  // 在栈上分配 8 字节空间
        self.mov_byte_mem_imm(4, 0, 0x74); // [RSP+0] = 't'
        self.mov_byte_mem_imm(4, 1, 0x72); // [RSP+1] = 'r'
        self.mov_byte_mem_imm(4, 2, 0x75); // [RSP+2] = 'u'
        self.mov_byte_mem_imm(4, 3, 0x65); // [RSP+3] = 'e'
        self.mov_byte_mem_imm(4, 4, 0x0A); // [RSP+4] = '\n'
        // sys_write(1, RSP, 5)
        self.mov_ri(0, 1);
        self.mov_ri(7, 1);
        self.mov_rr(6, 4);
        self.mov_ri(2, 5);
        self.syscall();
        self.pop(0);  // 恢复 RSP

        // done:
        let done_label = self.code.len();
        self.xor_rr(0, 0);  // RAX = 0 (Unit)
        self.ret();
        self.fn_end();

        // 修补跳转
        // false_skip (JNE) → true_label
        let rel = true_label as i32 - (false_skip as i32 + 4);
        self.code[false_skip..false_skip + 4].copy_from_slice(&rel.to_le_bytes());
        // done_patch (JMP) → done_label
        let rel = done_label as i32 - (done_patch as i32 + 4);
        self.code[done_patch..done_patch + 4].copy_from_slice(&rel.to_le_bytes());
    }

    /// __karte_panic() → !
    ///
    /// 输出 "runtime error: division by zero\n" 到 stderr，然后以 exit code 134 (SIGABRT) 退出。
    fn emit_panic(&mut self) {
        self.fn_start(runtime_names::PANIC);
        self.push(5);  // RBP

        // 将错误信息写入栈（34 字节 + 6 字节对齐 = 40 字节）
        let msg = b"runtime error: division by zero\n";
        let msg_len = msg.len();
        let aligned_len = ((msg_len + 7) & !7); // 40
        self.sub_ri8(4, aligned_len as u8);  // RSP -= 40

        // 逐字节写入栈
        for (i, &byte) in msg.iter().enumerate() {
            self.mov_byte_mem_imm(4, i as i32, byte); // MOV byte [RSP+i], byte
        }

        // sys_write(2, RSP, msg_len)
        self.mov_ri(0, 1);           // RAX = syscall 1 (write)
        self.mov_ri(7, 2);           // RDI = fd 2 (stderr)
        self.mov_rr(6, 4);           // RSI = RSP
        self.mov_ri(2, msg_len as u64); // RDX = length
        self.syscall();

        // 恢复栈（虽然不会到达这里，但保持完整性）
        self.add_ri8(4, aligned_len as u8);

        // sys_exit_group(134)
        self.mov_ri(0, 231); // RAX = syscall 231 (exit_group)
        self.mov_ri(7, 134);  // RDI = exit code 134 (SIGABRT)
        self.syscall();

        self.pop(5);
        self.ret();
        self.fn_end();
    }

    /// 修补内部函数调用 (gc_alloc → gc_collect, safepoint → gc_collect, string_concat → gc_alloc)
    pub fn patch_internal_calls(&mut self) {
        let gc_collect = self.find_offset(runtime_names::GC_COLLECT).unwrap();
        let gc_alloc = self.find_offset(runtime_names::GC_ALLOC_ALIGNED).unwrap();
        let patches: Vec<(usize, usize, i64)> = self.functions.iter()
            .filter(|f| f.name == "__gc_alloc_call_collect" || f.name == "__safepoint_call_collect")
            .map(|f| (f.offset, f.size, gc_collect as i64))
            .collect();
        let alloc_patches: Vec<(usize, usize, i64)> = self.functions.iter()
            .filter(|f| f.name == "__string_concat_call_alloc" || f.name == "__string_char_at_call_alloc" || f.name == "__to_string_call_alloc" || f.name == "__trim_call_alloc")
            .map(|f| (f.offset, f.size, gc_alloc as i64))
            .collect();

        let all_patches: Vec<_> = patches.into_iter().chain(alloc_patches.into_iter()).collect();

        for (call_pos, _size, target) in all_patches {
            // call_pos 指向 E8 字节, rel32 在 call_pos+1
            let rel32_pos = call_pos + 1;
            // CALL rel32: target = RIP_after + rel32
            // RIP_after = call_pos + 5 (从运行时起始)
            // rel32 = target - RIP_after
            let rip_after = (call_pos + 5) as i64;
            let rel = (target - rip_after) as i32;
            self.code[rel32_pos..rel32_pos+4].copy_from_slice(&rel.to_le_bytes());
        }
    }
}
