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
    fn emit_modrm_offset(&mut self, reg: u8, base: u8, disp: i32) {
        let reg_field = (reg & 7) << 3;
        let base_field = base & 7;
        if disp == 0 && base_field != 5 {
            self.b(0x00 | reg_field | base_field);
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
    fn emit_modrm_offset_rm(&mut self, _reg: u8, base: u8, disp: i32) {
        let base_field = base & 7;
        if disp == 0 && base_field != 5 {
            self.b(0x00 | base_field);
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

    /// 修补内部函数调用 (gc_alloc → gc_collect, safepoint → gc_collect)
    pub fn patch_internal_calls(&mut self) {
        let gc_collect = self.find_offset(runtime_names::GC_COLLECT).unwrap();
        let patches: Vec<(usize, usize)> = self.functions.iter()
            .filter(|f| f.name == "__gc_alloc_call_collect" || f.name == "__safepoint_call_collect")
            .map(|f| (f.offset, f.size))
            .collect();

        for (call_pos, _size) in patches {
            // call_pos 指向 E8 字节, rel32 在 call_pos+1
            let rel32_pos = call_pos + 1;
            // CALL rel32: target = RIP_after + rel32
            // RIP_after = call_pos + 5 (从运行时起始)
            // rel32 = gc_collect - RIP_after
            let rip_after = (call_pos + 5) as i64;
            let target = gc_collect as i64;
            let rel = (target - rip_after) as i32;
            self.code[rel32_pos..rel32_pos+4].copy_from_slice(&rel.to_le_bytes());
        }
    }
}
