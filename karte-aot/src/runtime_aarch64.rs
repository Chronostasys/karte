//! AArch64 AOT 运行时代码生成
//!
//! 生成最小化运行时，使用原始系统调用，不依赖 libc。
//! 包含 bump allocator，GC 将用 Karte 语言实现。
//!
//! 全局数据访问模式：
//!   ADRP + LDR/STR，偏移量在 patch_globals() 中统一修补。
//!
//! AArch64 寄存器约定（来自 JIT 编译器）：
//!   X10 = vm_sp (虚拟栈指针)
//!   X11 = vm_fp (虚拟帧指针)
//!   X0-X7 = 函数参数/返回值 (AAPCS64)
//!   X19-X28 = callee-saved
//!   X29 = FP, X30 = LR, SP = X31

use super::runtime_x86::RuntimeFunction;
pub use super::runtime_x86::runtime_names;
use std::collections::HashMap;

// AArch64 寄存器
const X0: u8 = 0;  const X1: u8 = 1;  const X2: u8 = 2;  const X3: u8 = 3;
const X4: u8 = 4;  const X5: u8 = 5;  const X6: u8 = 6;  const X7: u8 = 7;
const X8: u8 = 8;  const X9: u8 = 9;  const X10: u8 = 10; const X11: u8 = 11;
const X12: u8 = 12; const X13: u8 = 13; const X14: u8 = 14; const X15: u8 = 15;
const X16: u8 = 16; const X17: u8 = 17;
const X19: u8 = 19; const X20: u8 = 20; const X21: u8 = 21; const X22: u8 = 22;
const X23: u8 = 23; const X24: u8 = 24; const X25: u8 = 25; const X26: u8 = 26;
const X27: u8 = 27; const X28: u8 = 28;
const FP: u8 = 29; const LR: u8 = 30;
const SP_R: u8 = 31;
const XZR: u8 = 31;

// 全局变量偏移
const G_BUMP_PTR: usize = 0;
const G_HEAP_START: usize = 8;
const G_HEAP_LIMIT: usize = 16;
const G_VSTACK_BOTTOM: usize = 24;
const G_VSTACK_TOP: usize = 32;
const G_ALLOC_COUNT: usize = 40;
const G_GC_THRESHOLD: usize = 48;
const GLOBALS_SIZE: usize = 56;

// 条件码
const COND_EQ: u8 = 0;
const COND_NE: u8 = 1;
const COND_CS: u8 = 2;
const COND_CC: u8 = 3;
const COND_GE: u8 = 10;
const COND_LT: u8 = 11;
const COND_GT: u8 = 12;
const COND_LE: u8 = 13;
const COND_AL: u8 = 14; // 无条件 (Always)

#[derive(Debug, Clone)]
struct GlobalPatch {
    adrp_pos: usize,
    mem_pos: usize,
    global_offset: usize,
}

/// AArch64 运行时
#[derive(Debug, Clone)]
pub struct AArch64Runtime {
    pub code: Vec<u8>,
    pub functions: Vec<RuntimeFunction>,
    pub call_main_offset: usize,
    global_patches: Vec<GlobalPatch>,
    globals_data_offset: usize,
}

impl AArch64Runtime {
    pub fn new() -> Self {
        Self { code: Vec::new(), functions: Vec::new(), call_main_offset: 0, global_patches: Vec::new(), globals_data_offset: 0 }
    }

    pub fn find_offset(&self, name: &str) -> Option<usize> {
        self.functions.iter().find(|f| f.name == name).map(|f| f.offset)
    }

    pub fn generate(mut self) -> Self {
        self.emit_start();
        self.emit_gc_alloc();
        self.emit_gc_collect();
        self.emit_gc_safepoint();
        self.emit_nop(runtime_names::GC_UPDATE_STACK_TOP);
        self.emit_nop(runtime_names::FREE);
        self.emit_nop(runtime_names::RETAIN);
        self.emit_nop(runtime_names::RELEASE);
        self.patch_globals();
        self.patch_internal_calls();
        self
    }

    fn fn_start(&mut self, name: &str) {
        let offset = self.code.len();
        self.functions.push(RuntimeFunction { name: name.to_string(), offset, size: 0 });
    }
    fn fn_end(&mut self) {
        if let Some(f) = self.functions.last_mut() { f.size = self.code.len() - f.offset; }
    }

    // ================ 指令编码 ================

    fn w(&mut self, instr: u32) { self.code.extend_from_slice(&instr.to_le_bytes()); }
    fn u64(&mut self, val: u64) { self.code.extend_from_slice(&val.to_le_bytes()); }

    // 加载 64 位立即数
    fn mov_imm64(&mut self, rd: u8, val: i64) {
        let v = val as u64;
        self.emit_movz(rd, (v & 0xFFFF) as u16, 0);
        if (v >> 16) & 0xFFFF != 0 || (v >> 32) != 0 || (v >> 48) != 0 {
            self.emit_movk(rd, ((v >> 16) & 0xFFFF) as u16, 16);
        }
        if (v >> 32) & 0xFFFF != 0 || (v >> 48) != 0 {
            self.emit_movk(rd, ((v >> 32) & 0xFFFF) as u16, 32);
        }
        if (v >> 48) != 0 {
            self.emit_movk(rd, ((v >> 48) & 0xFFFF) as u16, 48);
        }
    }

    fn emit_movz(&mut self, rd: u8, imm16: u16, shift: u16) {
        let hw = (shift / 16) & 0x3;
        self.w((1u32 << 31) | (0b10u32 << 29) | (0b100101u32 << 23) | ((hw as u32) << 21) | ((imm16 as u32) << 5) | (rd as u32));
    }

    fn emit_movk(&mut self, rd: u8, imm16: u16, shift: u16) {
        let hw = (shift / 16) & 0x3;
        self.w((1u32 << 31) | (0b11u32 << 29) | (0b100101u32 << 23) | ((hw as u32) << 21) | ((imm16 as u32) << 5) | (rd as u32));
    }

    // 算术
    fn add(&mut self, rd: u8, rn: u8, rm: u8) {
        self.w((1u32 << 31) | (0b01011 << 24) | ((rm as u32) << 16) | ((rn as u32) << 5) | (rd as u32));
    }
    fn sub(&mut self, rd: u8, rn: u8, rm: u8) {
        self.w((1u32 << 31) | (1u32 << 30) | (0b01011 << 24) | ((rm as u32) << 16) | ((rn as u32) << 5) | (rd as u32));
    }
    fn add_imm(&mut self, rd: u8, rn: u8, imm: u32) {
        self.w((1u32 << 31) | (0b100010 << 23) | ((imm & 0xFFF) << 10) | ((rn as u32) << 5) | (rd as u32));
    }
    fn sub_imm(&mut self, rd: u8, rn: u8, imm: u32) {
        self.w((1u32 << 31) | (1u32 << 30) | (0b100010 << 23) | ((imm & 0xFFF) << 10) | ((rn as u32) << 5) | (rd as u32));
    }

    // 位操作
    fn and_reg(&mut self, rd: u8, rn: u8, rm: u8) {
        self.w((1u32 << 31) | (0b00u32 << 29) | (0b01010u32 << 24) | (0u32 << 21) | ((rm as u32) << 16) | ((rn as u32) << 5) | (rd as u32));
    }

    // 移动
    fn mov_reg(&mut self, rd: u8, rn: u8) {
        if rd == SP_R || rn == SP_R {
            self.add_imm(rd, rn, 0);
        } else {
            // ORR Xd, XZR, Xn
            self.w((1u32 << 31) | (0b01u32 << 29) | (0b01010u32 << 24) | (0u32 << 21) | ((rn as u32) << 16) | ((XZR as u32) << 5) | (rd as u32));
        }
    }

    // 内存
    fn ldr_offset(&mut self, rt: u8, rn: u8, offset: i32) {
        let imm12 = (offset as u32 / 8) & 0xFFF;
        // LDR (unsigned offset): size=11, 111, V=0, 01, opc=01, imm12, Rn, Rt
        self.w((0b11u32 << 30) | (0b111u32 << 27) | (0b01u32 << 24) | (0b01u32 << 22) | (imm12 << 10) | ((rn as u32) << 5) | (rt as u32));
    }
    /// LDRB Wt, [Xn, #imm] — 无符号字节加载 (imm 必须是对齐到 1 的非负数)
    fn ldrb_imm(&mut self, rt: u8, rn: u8, offset: u32) {
        let imm12 = offset & 0xFFF;
        // LDRB (unsigned offset): size=00, 111, V=0, 01, opc=01, imm12, Rn, Rt
        self.w((0b00u32 << 30) | (0b111u32 << 27) | (0b01u32 << 24) | (0b01u32 << 22) | (imm12 << 10) | ((rn as u32) << 5) | (rt as u32));
    }
    fn str_offset(&mut self, rt: u8, rn: u8, offset: i32) {
        let imm12 = (offset as u32 / 8) & 0xFFF;
        // STR (unsigned offset): size=11, 111, V=0, 01, opc=00, imm12, Rn, Rt
        self.w((0b11u32 << 30) | (0b111u32 << 27) | (0b01u32 << 24) | (0b00u32 << 22) | (imm12 << 10) | ((rn as u32) << 5) | (rt as u32));
    }

    // 比较
    fn cmp_reg(&mut self, rn: u8, rm: u8) {
        // SUBS XZR, Xn, Xm
        self.w((1u32 << 31) | (1u32 << 30) | (1u32 << 29) | (0b01011 << 24) | ((rm as u32) << 16) | ((rn as u32) << 5) | (XZR as u32));
    }
    fn cmp_imm(&mut self, rn: u8, imm: i64) {
        // SUBS XZR, Xn, #imm
        let v = imm as u64;
        self.emit_movz(28, (v & 0xFFFF) as u16, 0); // X28 = imm
        if (v >> 16) & 0xFFFF != 0 { self.emit_movk(28, ((v >> 16) & 0xFFFF) as u16, 16); }
        if (v >> 32) & 0xFFFF != 0 { self.emit_movk(28, ((v >> 32) & 0xFFFF) as u16, 32); }
        if (v >> 48) != 0 { self.emit_movk(28, ((v >> 48) & 0xFFFF) as u16, 48); }
        self.cmp_reg(rn, 28);
    }

    // ANDS (用于 TEST Xn, Xm → 设置 NZCV 标志)
    fn ands_reg(&mut self, rd: u8, rn: u8, rm: u8) {
        // ANDS Xd, Xn, Xm (shifted register, LSL #0, N=0)
        // 编码: sf=1 opc=11(ANDS) 0101 sh=00 N=0 Rm imm6=0 Rn Rd
        // = 0xEA000000 | (Rm << 16) | (Rn << 5) | Rd
        self.w(0xEA000000u32 | ((rm as u32) << 16) | ((rn as u32) << 5) | (rd as u32));
    }

    // 除法 (用乘法+移位模拟, 避免 UDIV 兼容性问题)
    fn udiv(&mut self, rd: u8, rn: u8, rm: u8) {
        // UDIV Xd, Xn, Xm
        self.w((1u32 << 31) | (0b10u32 << 29) | (0b11010110u32 << 21) | ((rm as u32) << 16) | ((rn as u32) << 5) | (rd as u32));
    }
    // MSUB (乘减: Rd = Rn - Ra*Rm)
    fn msub(&mut self, rd: u8, rn: u8, ra: u8, rm: u8) {
        // MSUB Xd, Xn, Xa, Xm
        self.w((1u32 << 31) | (0b00u32 << 29) | (0b11011u32 << 24) | (1u32 << 21) | ((rm as u32) << 16) | ((ra as u32) << 10) | ((rn as u32) << 5) | (rd as u32));
    }

    // 用乘法+移位实现 Xd = Xn / 10 (无余数)
    fn div_by_10(&mut self, rd: u8, rn: u8, rem_rd: u8) {
        // Xd = Xn / 10
        // 余数 = Xn - Xd * 10 → 存入 rem_rd
        //
        // 方法: 用 ARM 的 magic number 乘法
        // 对于无符号除以 10:
        //   magic = 0xCCCCCCCCCCCCCCCD
        //   high = UMULH(Xn, magic)
        //   result = high >> 2
        //
        // 但 UMULH 需要特殊指令。简化方案：用 LSR + UDIV

        // 保存 Xn 到临时寄存器 (X3 或 X28)
        self.mov_reg(X28, rn); // X28 = Xn (保存原始值)

        // UDIV rd, rn, X9 (X9 = 10, 已在调用者设置)
        self.udiv(rd, rn, 9); // rd = Xn / 10

        // 余数: rem_rd = X28 - rd * 10
        // MADD rem_rd, rd, X9, XZR → rem_rd = rd * 10
        // 但 MADD 没有直接编码为 rd*10... 用 MSUB: rem_rd = X28 - rd*X9
        self.msub(rem_rd, X28, rd, 9); // rem_rd = Xn - (Xn/10)*10 = Xn%10
    }

    // SUB (寄存器, 3操作数)
    fn sub_reg(&mut self, rd: u8, rn: u8, rm: u8) {
        self.sub(rd, rn, rm);
    }

    // ADD (寄存器, 3操作数) — 使用 add 方法
    fn add_reg(&mut self, rd: u8, rn: u8, rm: u8) {
        self.add(rd, rn, rm);
    }

    // STRB (store byte, unsigned offset)
    fn strb_imm(&mut self, rn: u8, offset: u32, val: u8) {
        // 先写入值到 W28，再存储
        self.w((0x52800000u32) | ((val as u32) << 5) | (28u32)); // MOVZ W28, #val
        // STRB W28, [Xn, #offset]: 0x39000000 | (imm12 << 10) | (Rn << 5) | Rt
        let imm12 = (offset & 0xFFF) << 10;
        self.w(0x39000000u32 | imm12 | ((rn as u32) << 5) | (28u32));
    }

    // STRB Wt, [Xn, #offset] - 存储寄存器低位字节 (unsigned offset)
    fn strb_reg(&mut self, rt: u8, rn: u8, offset: u32) {
        // STRB Wt, [Xn, #imm12]: 11 111 0 01 00 imm12 Rn Rt
        let imm12 = (offset & 0xFFF) << 10;
        self.w(0x39000000u32 | imm12 | ((rn as u32) << 5) | (rt as u32));
    }

    // 分支
    fn b(&mut self, offset: i32) {
        let imm26 = (offset / 4) as u32 & 0x3FFFFFF;
        self.w((0b000101u32 << 26) | imm26);
    }
    fn bl(&mut self, offset: i32) {
        let imm26 = (offset / 4) as u32 & 0x3FFFFFF;
        self.w((0b100101u32 << 26) | imm26);
    }
    fn ret(&mut self) { self.w(0xD65F03C0); }
    fn b_cond(&mut self, cond: u8, offset: i32) {
        let imm19 = ((offset / 4) as u32) & 0x7FFFF;
        self.w((0b0101010u32 << 25) | (0u32 << 24) | (imm19 << 5) | (cond as u32 & 0xF));
    }
    fn svc0(&mut self) { self.w(0xD4000001); }
    fn nop(&mut self) { self.w(0xD503201F); }

    // ================ 全局变量 ================

    fn load_global(&mut self, rd: u8, g_offset: usize) {
        let adrp_pos = self.code.len();
        self.w((0b10000u32 << 24) | (rd as u32)); // ADRP 占位
        let mem_pos = self.code.len();
        self.ldr_offset(rd, rd, 0); // LDR 占位
        self.global_patches.push(GlobalPatch { adrp_pos, mem_pos, global_offset: g_offset });
    }

    fn store_global(&mut self, rs: u8, g_offset: usize) {
        let adrp_pos = self.code.len();
        self.w((0b10000u32 << 24) | (X17 as u32)); // ADRP X17 占位
        let mem_pos = self.code.len();
        self.str_offset(rs, X17, 0); // STR 占位
        self.global_patches.push(GlobalPatch { adrp_pos, mem_pos, global_offset: g_offset });
    }

    fn patch_globals(&mut self) {
        let globals_start = self.globals_data_offset;
        for patch in &self.global_patches {
            let target = globals_start + patch.global_offset;
            let adrp_pos = patch.adrp_pos;

            let pc_page = adrp_pos & !0xFFF;
            let target_page = target & !0xFFF;
            let page_offset = target_page as i64 - pc_page as i64;
            let immhi = ((page_offset as u64) >> 12) as u32;
            let immlo = ((page_offset as u64) >> 2) as u32 & 0x3;

            let adrp_word = u32::from_le_bytes(self.code[adrp_pos..adrp_pos + 4].try_into().unwrap());
            let rd = adrp_word & 0x1F;
            // ADRP 编码: 1 | 10000 | immlo(2) | 000000 | immhi(19) | Rd(5)
            // bit 31 = 1 表示 ADRP（页地址），0 表示 ADR（PC相对地址）
            let new_adrp = (1u32 << 31) | (0b10000u32 << 24) | (immlo << 29) | ((immhi & 0x7FFFF) << 5) | rd;
            self.code[adrp_pos..adrp_pos + 4].copy_from_slice(&new_adrp.to_le_bytes());

            let mem_pos = patch.mem_pos;
            let page_inner = (target & 0xFFF) as u32;
            let mem_word = u32::from_le_bytes(self.code[mem_pos..mem_pos + 4].try_into().unwrap());
            let new_mem = (mem_word & !(0xFFFu32 << 10)) | (((page_inner / 8) & 0xFFF) << 10);
            self.code[mem_pos..mem_pos + 4].copy_from_slice(&new_mem.to_le_bytes());
        }
    }

    fn patch_internal_calls(&mut self) {
        let func_offsets: HashMap<String, usize> = self.functions.iter()
            .map(|f| (f.name.clone(), f.offset)).collect();
        // 扫描 BL 指令并修补（runtime 内部目前没有互相调用）
        let _ = func_offsets;
    }

    // ================ 函数生成 ================

    fn emit_nop(&mut self, name: &str) {
        self.fn_start(name);
        self.ret();
        self.fn_end();
    }

    /// _start — 程序入口
    fn emit_start(&mut self) {
        self.fn_start(runtime_names::START);

        // 保存 callee-saved: X19-X28 + FP + LR = 12 × 8 = 96, 对齐到 128
        self.sub_imm(SP_R, SP_R, 128);
        let mut o: i32 = 0;
        for &reg in &[X19, X20, X21, X22, X23, X24, X25, X26, X27, X28, FP, LR] {
            self.str_offset(reg, SP_R, o); o += 8;
        }

        self.mov_reg(X23, SP_R); // X23 = system_sp

        // mmap 虚拟栈 512KB
        self.mov_imm64(X0, 0);
        self.mov_imm64(X1, 524288);
        self.mov_imm64(X2, 3);
        self.mov_imm64(X3, 0x22);
        self.mov_imm64(X4, -1i64 as u64 as i64);
        self.mov_imm64(X5, 0);
        self.mov_imm64(X8, 222);
        self.svc0();
        self.mov_reg(X20, X0); // X20 = vstack_base

        // 初始化 vm_sp
        self.mov_imm64(X9, 524272);
        self.add(X10, X20, X9); // X10 = vm_sp
        self.str_offset(XZR, X10, 0);
        self.mov_reg(X11, X10); // X11 = vm_fp

        // mmap 堆 4MB
        self.mov_imm64(X0, 0);
        self.mov_imm64(X1, 4 * 1024 * 1024);
        self.mov_imm64(X2, 3);
        self.mov_imm64(X3, 0x22);
        self.mov_imm64(X4, -1i64 as u64 as i64);
        self.mov_imm64(X5, 0);
        self.mov_imm64(X8, 222);
        self.svc0();
        self.mov_reg(X24, X0); // X24 = heap_base
        self.mov_imm64(X9, 4 * 1024 * 1024);
        self.add(X25, X24, X9); // X25 = heap_limit

        // 全局变量初始化
        self.store_global(X24, G_BUMP_PTR);
        self.store_global(X24, G_HEAP_START);
        self.store_global(X25, G_HEAP_LIMIT);
        self.store_global(X20, G_VSTACK_BOTTOM);
        self.mov_imm64(X9, 524272);
        self.add(X9, X20, X9);
        self.store_global(X9, G_VSTACK_TOP);
        self.mov_imm64(X0, 0);
        self.store_global(X0, G_ALLOC_COUNT);
        self.mov_imm64(X0, 256);
        self.store_global(X0, G_GC_THRESHOLD);

        // heap inline header
        self.mov_imm64(X9, 24);
        self.add(X9, X24, X9);
        self.str_offset(X9, X24, 0);
        self.mov_imm64(X9, 256);
        self.str_offset(X9, X24, 16);

        // 调用 main
        self.mov_imm64(X9, 524272);
        self.add(X0, X20, X9);
        self.mov_reg(X1, X20);
        self.call_main_offset = self.code.len();
        self.bl(0);

        // ---- 将 main() 返回值 (X0) 通过 sys_write 输出到 stdout ----
        //
        // AArch64 Linux sys_write(fd, buf, count):
        //   X8 = 64 (syscall number)
        //   X0 = fd (1 = stdout)
        //   X1 = buf pointer
        //   X2 = count
        //   SVC #0
        //
        // 寄存器分配:
        //   X0 = 当前数值 / fd
        //   X1 = 缓冲区指针 (SP)
        //   X2 = 字符计数
        //   X9 = 常量 10 (除数)
        //   X28 = 原始返回值备份
        //   X3 = 符号标志
        //   X4 = 临时 (MSUB 余数)
        //
        // 使用 32 字节栈缓冲区 (对齐到 16 字节), 从后往前填充

        // 恢复系统 SP（用 X23 保存的值）
        self.mov_reg(SP_R, X23);

        // 分配 32 字节缓冲区 (对齐)
        self.sub_imm(SP_R, SP_R, 32);
        // X1 = SP (缓冲区基址)
        self.mov_reg(X1, SP_R);

        // X28 = 原始返回值
        self.mov_reg(X28, X0);

        // 处理负数
        // X3 = 符号标志 (0=正, 1=负)
        self.mov_imm64(X3, 0);
        self.cmp_imm(X0, 0);
        let neg_skip_pos = self.code.len();
        self.b_cond(COND_GE, 0); // >= 0 则跳过

        // 负数: 标记符号, 先写入 '-' 符号到缓冲区, 然后取绝对值
        self.mov_imm64(X3, 1);
        // 写入 '-' 到缓冲区 (在数字之前)
        self.strb_imm(X1, 0, b'-' as u8);
        self.add_imm(X1, X1, 1); // buf 指针后移
        self.add_imm(X2, X2, 1); // count++ (包含符号)

        // 取绝对值 (NEG X0)
        self.sub_reg(X0, XZR, X0); // X0 = -X0

        // neg_skip:
        let neg_skip_label = self.code.len();
        {
            let offset = neg_skip_label as i32 - neg_skip_pos as i32;
            let instr = u32::from_le_bytes(self.code[neg_skip_pos..neg_skip_pos+4].try_into().unwrap());
            let imm19 = ((offset / 4) as u32) & 0x7FFFF;
            let new_instr = (instr & !(0x7FFFFu32 << 5)) | (imm19 << 5);
            self.code[neg_skip_pos..neg_skip_pos+4].copy_from_slice(&new_instr.to_le_bytes());
        }

        // 初始化
        self.mov_imm64(X2, 0);   // X2 = 字符计数 = 0
        self.mov_imm64(X9, 10);  // X9 = 10

        // digit_loop:
        let digit_loop = self.code.len();

        // 检查 X0 == 0
        self.ands_reg(XZR, X0, X0);
        let digit_done_pos = self.code.len();
        self.b_cond(COND_EQ, 0); // X0 == 0 退出循环

        // X0 % 10 通过连续减 10 实现
        // 商存入 X4, 余数在 X0
        self.mov_reg(X4, X0); // X4 = 当前值
        self.mov_imm64(X6, 0); // X6 = 商 = 0

        // div_loop:
        let div_loop = self.code.len();
        // 检查 X4 >= 10
        self.cmp_imm(X4, 10);
        let div_done_pos = self.code.len();
        self.b_cond(COND_LT, 0); // X4 < 10 退出循环

        self.sub_imm(X4, X4, 10); // X4 -= 10
        self.add_imm(X6, X6, 1);   // X6++ (商++)
        // 跳回 div_loop
        {
            let loop_back_pos = self.code.len();
            let offset = div_loop as i32 - loop_back_pos as i32;
            let instr = (0b000101u32 << 26) | (((offset / 4) as u32) & 0x3FFFFFF);
            self.code.extend_from_slice(&instr.to_le_bytes());
        }

        // div_done:
        let div_done_label = self.code.len();
        {
            let offset = div_done_label as i32 - div_done_pos as i32;
            let instr = u32::from_le_bytes(self.code[div_done_pos..div_done_pos+4].try_into().unwrap());
            let imm19 = ((offset / 4) as u32) & 0x7FFFF;
            let new_instr = (instr & !(0x7FFFFu32 << 5)) | (imm19 << 5);
            self.code[div_done_pos..div_done_pos+4].copy_from_slice(&new_instr.to_le_bytes());
        }

        // X0 = 余数 = X4 (最后剩余的值, 0-9)
        self.mov_reg(X0, X4);
        // X4 = 商 = X6 (用于下次迭代)
        self.mov_reg(X4, X6);
        // X0 = 余数 = 最低位数字
        // ASCII = 余数 + '0'
        self.add_imm(X0, X0, b'0' as u32);
        // 存入缓冲区: ADD X5 = buf+count, STRB W0, [X5, #0]
        self.add_reg(X5, X1, X2);
        self.strb_reg(X0, X5, 0);
        self.add_imm(X2, X2, 1); // count++
        // X0 = 商 (X4)
        self.mov_reg(X0, X4);

        // 跳回循环
        {
            let loop_back_pos = self.code.len();
            let offset = digit_loop as i32 - loop_back_pos as i32;
            let instr = (0b000101u32 << 26) | (((offset / 4) as u32) & 0x3FFFFFF);
            self.code.extend_from_slice(&instr.to_le_bytes());
        }

        // digit_done:
        let digit_done_label = self.code.len();
        {
            let offset = digit_done_label as i32 - digit_done_pos as i32;
            let instr = u32::from_le_bytes(self.code[digit_done_pos..digit_done_pos+4].try_into().unwrap());
            let imm19 = ((offset / 4) as u32) & 0x7FFFF;
            let new_instr = (instr & !(0x7FFFFu32 << 5)) | (imm19 << 5);
            self.code[digit_done_pos..digit_done_pos+4].copy_from_slice(&new_instr.to_le_bytes());
        }

        // 处理 count == 0 (原始值为 0): 如果 count == 0, 写入 '0'
        self.cmp_imm(X2, 0);
        let nonzero_count_pos = self.code.len();
        self.b_cond(COND_NE, 0); // count != 0 跳过

        self.strb_imm(X1, 0, b'0' as u8); // buf[0] = '0'
        self.add_imm(X2, X2, 1);

        let nonzero_count_label = self.code.len();
        {
            let offset = nonzero_count_label as i32 - nonzero_count_pos as i32;
            let instr = u32::from_le_bytes(self.code[nonzero_count_pos..nonzero_count_pos+4].try_into().unwrap());
            let imm19 = ((offset / 4) as u32) & 0x7FFFF;
            let new_instr = (instr & !(0x7FFFFu32 << 5)) | (imm19 << 5);
            self.code[nonzero_count_pos..nonzero_count_pos+4].copy_from_slice(&new_instr.to_le_bytes());
        }

        // ---- 反转缓冲区中的数字字符 ----
        // 数字按最低位在前存储（如 19 → ['9','1']），需要反转为 ['1','9']
        // 使用双指针法：X5 = left = X1, X6 = right = X1 + X2 - 1
        // 循环条件：X5 < X6
        // X7 = temp byte
        self.sub_imm(X6, X2, 1);    // X6 = count - 1
        self.add_reg(X6, X1, X6);   // X6 = buf + count - 1 = right pointer
        self.mov_reg(X5, X1);       // X5 = left pointer

        // reverse_loop:
        let reverse_loop = self.code.len();
        // 检查 X5 < X6
        self.cmp_reg(X5, X6);
        let reverse_done_pos = self.code.len();
        self.b_cond(COND_GE, 0); // X5 >= X6 退出

        // 交换 X5 和 X6 指向的字节
        self.ldrb_imm(X7, X5, 0);   // X7 = *left
        self.ldrb_imm(X10, X6, 0);  // X10 = *right
        self.strb_reg(X7, X6, 0);   // *right = X7
        self.strb_reg(X10, X5, 0);  // *left = X10

        // left++, right--
        self.add_imm(X5, X5, 1);
        self.sub_imm(X6, X6, 1);

        // 跳回 reverse_loop
        {
            let loop_back_pos = self.code.len();
            let offset = reverse_loop as i32 - loop_back_pos as i32;
            let instr = (0b000101u32 << 26) | (((offset / 4) as u32) & 0x3FFFFFF);
            self.code.extend_from_slice(&instr.to_le_bytes());
        }

        // reverse_done:
        let reverse_done_label = self.code.len();
        {
            let offset = reverse_done_label as i32 - reverse_done_pos as i32;
            let instr = u32::from_le_bytes(self.code[reverse_done_pos..reverse_done_pos+4].try_into().unwrap());
            let imm19 = ((offset / 4) as u32) & 0x7FFFF;
            let new_instr = (instr & !(0x7FFFFu32 << 5)) | (imm19 << 5);
            self.code[reverse_done_pos..reverse_done_pos+4].copy_from_slice(&new_instr.to_le_bytes());
        }

        // ---- sys_write(1, X1, X2) ----
        self.mov_imm64(X8, 64);  // syscall 号: sys_write
        self.mov_imm64(X0, 1);   // fd = stdout
        // X1 = buf (已在上面设置)
        // X2 = count (已在上面设置)
        self.svc0();

        // 恢复 SP (移除缓冲区)
        self.add_imm(SP_R, SP_R, 32);

        // 恢复 callee-saved
        let mut o: i32 = 0;
        for &reg in &[X19, X20, X21, X22, X23, X24, X25, X26, X27, X28, FP, LR] {
            self.ldr_offset(reg, SP_R, o); o += 8;
        }
        self.add_imm(SP_R, SP_R, 128);

        // exit_group(0)
        self.mov_imm64(X0, 0);
        self.mov_imm64(X8, 94);
        self.svc0();

        // 全局数据区 (7 × 8 = 56 bytes)
        while self.code.len() % 8 != 0 { self.nop(); }
        self.globals_data_offset = self.code.len();
        for _ in 0..7 { self.u64(0); }

        self.fn_end();
    }

    /// Bump allocator
    fn emit_gc_alloc(&mut self) {
        self.fn_start(runtime_names::GC_ALLOC_ALIGNED);

        // 保存 LR
        self.sub_imm(SP_R, SP_R, 16);
        self.str_offset(LR, SP_R, 0);

        // 加载 bump_ptr
        self.load_global(X2, G_BUMP_PTR);

        // 对齐到 16: aligned = (bump_ptr + 15) & ~15
        self.mov_imm64(X3, 15);
        self.add(X3, X2, X3);
        self.mov_imm64(X4, -16i64 as u64 as i64);
        self.and_reg(X3, X3, X4);

        // new_bump = aligned + size (X0)
        self.add(X5, X3, X0);

        // 检查溢出
        self.load_global(X6, G_HEAP_LIMIT);
        self.cmp_reg(X5, X6);
        let bge_pos = self.code.len();
        self.b_cond(COND_GE, 0); // 占位

        // 更新 bump_ptr
        self.store_global(X5, G_BUMP_PTR);
        self.mov_reg(X0, X3);

        self.ldr_offset(LR, SP_R, 0);
        self.add_imm(SP_R, SP_R, 16);
        self.ret();

        // overflow: 返回 0
        let overflow_pos = self.code.len();
        let bge_off = (overflow_pos as i32) - (bge_pos as i32);
        let bge_word = u32::from_le_bytes(self.code[bge_pos..bge_pos + 4].try_into().unwrap());
        let cond = bge_word & 0xF;
        let imm19 = ((bge_off / 4) as u32) & 0x7FFFF;
        let new_bge = (0b0101010u32 << 25) | (0u32 << 24) | (imm19 << 5) | cond;
        self.code[bge_pos..bge_pos + 4].copy_from_slice(&new_bge.to_le_bytes());

        self.mov_imm64(X0, 0);
        self.ldr_offset(LR, SP_R, 0);
        self.add_imm(SP_R, SP_R, 16);
        self.ret();

        self.fn_end();
    }

    fn emit_gc_collect(&mut self) {
        self.fn_start(runtime_names::GC_COLLECT);
        self.ret();
        self.fn_end();
    }

    fn emit_gc_safepoint(&mut self) {
        self.fn_start(runtime_names::GC_SAFEPOINT);
        self.ret();
        self.fn_end();
    }
}
