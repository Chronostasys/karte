//! RISC-V 64 位 AOT 运行时代码生成
//!
//! 生成最小化运行时，使用原始系统调用，不依赖 libc。
//! 包含 bump allocator，GC 将用 Karte 语言在阶段五实现。
//!
//! 全局数据访问模式：
//!   AUIPC + LD/SD，偏移量在 patch_globals() 中统一修补。

use super::runtime_x86::RuntimeFunction;
pub use super::runtime_x86::runtime_names;

// RISC-V 寄存器
const ZERO: u8 = 0; const RA: u8 = 1; const SP: u8 = 2; const GP: u8 = 3;
const TP: u8 = 4; const T0: u8 = 5; const T1: u8 = 6; const T2: u8 = 7;
const S0_FP: u8 = 8; const S1: u8 = 9;
const A0: u8 = 10; const A1: u8 = 11; const A2: u8 = 12; const A3: u8 = 13;
const A4: u8 = 14; const A5: u8 = 15; const A6: u8 = 16; const A7: u8 = 17;
const S2: u8 = 18; const S3: u8 = 19; const S4: u8 = 20; const S5: u8 = 21;
const S6: u8 = 22; const S7: u8 = 23; const S8: u8 = 24; const S9: u8 = 25;
const S10: u8 = 26; const S11: u8 = 27;
const T3: u8 = 28; const T4: u8 = 29; const T5: u8 = 30; const T6: u8 = 31;

/// 全局变量偏移
const G_BUMP_PTR: usize = 0;
const G_HEAP_START: usize = 8;
const G_HEAP_LIMIT: usize = 16;
const G_VSTACK_BOTTOM: usize = 24;
const G_ALLOC_COUNT: usize = 32;
const G_GC_THRESHOLD: usize = 40;
const GLOBALS_SIZE: usize = 48;

/// 需要修补的全局变量访问
#[derive(Debug, Clone)]
struct GlobalPatch {
    auipc_pos: usize,
    mem_pos: usize,
    global_offset: usize,
}

/// RISC-V 64 位运行时
#[derive(Debug, Clone)]
pub struct RiscvRuntime {
    pub code: Vec<u8>,
    pub functions: Vec<RuntimeFunction>,
    pub call_main_offset: usize,
    global_patches: Vec<GlobalPatch>,
    globals_data_offset: usize,
}

impl RiscvRuntime {
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

    /// I-type
    fn i_type(&mut self, imm: i32, rs1: u8, f3: u8, rd: u8, op: u8) {
        self.w((((imm as u32) & 0xFFF) << 20) | ((rs1 as u32 & 0x1F) << 15)
            | ((f3 as u32 & 0x7) << 12) | ((rd as u32 & 0x1F) << 7) | (op as u32 & 0x7F));
    }
    /// R-type
    fn r_type(&mut self, f7: u8, rs2: u8, rs1: u8, f3: u8, rd: u8, op: u8) {
        self.w(((f7 as u32) << 25) | ((rs2 as u32 & 0x1F) << 20)
            | ((rs1 as u32 & 0x1F) << 15) | ((f3 as u32 & 0x7) << 12)
            | ((rd as u32 & 0x1F) << 7) | (op as u32 & 0x7F));
    }
    /// S-type
    fn s_type(&mut self, imm: i32, rs2: u8, rs1: u8, f3: u8, op: u8) {
        let v = imm as u32;
        self.w((((v >> 5) & 0x7F) << 25) | ((rs2 as u32 & 0x1F) << 20)
            | ((rs1 as u32 & 0x1F) << 15) | ((f3 as u32 & 0x7) << 12)
            | ((v & 0x1F) << 7) | (op as u32 & 0x7F));
    }
    /// B-type
    fn b_type(&mut self, imm: i32, rs2: u8, rs1: u8, f3: u8, op: u8) {
        let v = imm as u32;
        self.w((((v >> 12) & 1) << 31) | (((v >> 5) & 0x3F) << 25)
            | ((rs2 as u32 & 0x1F) << 20) | ((rs1 as u32 & 0x1F) << 15)
            | ((f3 as u32 & 0x7) << 12) | (((v >> 1) & 0xF) << 8)
            | (((v >> 11) & 1) << 7) | (op as u32 & 0x7F));
    }
    /// U-type
    fn u_type(&mut self, imm: u32, rd: u8, op: u8) {
        self.w((imm & 0xFFFFF000) | ((rd as u32 & 0x1F) << 7) | (op as u32 & 0x7F));
    }
    /// J-type
    fn j_type(&mut self, imm: i32, rd: u8, op: u8) {
        let v = imm as u32;
        self.w((((v >> 20) & 1) << 31) | (((v >> 1) & 0x3FF) << 21)
            | (((v >> 11) & 1) << 20) | (((v >> 12) & 0xFF) << 12)
            | ((rd as u32 & 0x1F) << 7) | (op as u32 & 0x7F));
    }

    // 便捷指令
    fn add(&mut self, rd: u8, rs1: u8, rs2: u8) { self.r_type(0x00, rs2, rs1, 0x0, rd, 0x33); }
    fn sub(&mut self, rd: u8, rs1: u8, rs2: u8) { self.r_type(0x20, rs2, rs1, 0x0, rd, 0x33); }
    fn addi(&mut self, rd: u8, rs1: u8, imm: i32) { self.i_type(imm, rs1, 0x0, rd, 0x13); }
    fn andi(&mut self, rd: u8, rs1: u8, imm: i32) { self.i_type(imm, rs1, 0x7, rd, 0x13); }
    fn xori(&mut self, rd: u8, rs1: u8, imm: i32) { self.i_type(imm, rs1, 0x4, rd, 0x13); }
    fn ori(&mut self, rd: u8, rs1: u8, imm: i32) { self.i_type(imm, rs1, 0x6, rd, 0x13); }
    fn ld(&mut self, rd: u8, rs1: u8, off: i32) { self.i_type(off, rs1, 0x3, rd, 0x03); }
    fn sd(&mut self, rs2: u8, rs1: u8, off: i32) { self.s_type(off, rs2, rs1, 0x3, 0x23); }
    fn sw(&mut self, rs2: u8, rs1: u8, off: i32) { self.s_type(off, rs2, rs1, 0x2, 0x23); }
    fn lui(&mut self, rd: u8, upper20: u32) { self.u_type(upper20 << 12, rd, 0x37); }
    fn auipc(&mut self, rd: u8, upper20: u32) { self.u_type(upper20 << 12, rd, 0x17); }
    fn jal(&mut self, rd: u8, off: i32) { self.j_type(off, rd, 0x6F); }
    fn jalr(&mut self, rd: u8, rs1: u8, off: i32) { self.i_type(off, rs1, 0x0, rd, 0x67); }
    fn beq(&mut self, rs1: u8, rs2: u8, off: i32) { self.b_type(off, rs2, rs1, 0x0, 0x63); }
    fn bne(&mut self, rs1: u8, rs2: u8, off: i32) { self.b_type(off, rs2, rs1, 0x1, 0x63); }
    fn blt(&mut self, rs1: u8, rs2: u8, off: i32) { self.b_type(off, rs2, rs1, 0x4, 0x63); }
    fn bge(&mut self, rs1: u8, rs2: u8, off: i32) { self.b_type(off, rs2, rs1, 0x5, 0x63); }
    fn bltu(&mut self, rs1: u8, rs2: u8, off: i32) { self.b_type(off, rs2, rs1, 0x6, 0x63); }
    fn bgeu(&mut self, rs1: u8, rs2: u8, off: i32) { self.b_type(off, rs2, rs1, 0x7, 0x63); }
    fn ecall(&mut self) { self.w(0x00000073); }
    fn mv(&mut self, rd: u8, rs: u8) { self.addi(rd, rs, 0); }
    fn ret(&mut self) { self.jalr(ZERO, RA, 0); }

    /// 加载立即数 (-2048 到 ±2^31 范围)
    fn li(&mut self, rd: u8, imm: i64) {
        if imm >= -2048 && imm <= 2047 {
            self.addi(rd, ZERO, imm as i32);
            return;
        }
        // LUI + ADDIW (处理低 32 位)
        let val = if imm < 0 { imm as u64 } else { imm as u64 };
        let lo = val as u32;
        let lo12 = (lo & 0xFFF) as i32;
        let lo12_ext = if lo12 >= 0x800 { lo12 - 0x1000 } else { lo12 };
        let upper = ((imm as i64).wrapping_sub(lo12_ext as i64) as u32) >> 12;
        if upper != 0 { self.lui(rd, upper); }
        self.addi(rd, if upper != 0 { rd } else { ZERO }, lo12_ext);
        // 高 32 位（如果需要）
        let hi = (val >> 32) as u32;
        if hi != 0 || (imm < 0 && hi == 0 && upper == 0 && lo12_ext >= 0) {
            // SLLI rd, rd, 32
            self.w((32u32 << 20) | ((rd as u32 & 0x1F) << 15) | (0x1 << 12) | ((rd as u32 & 0x1F) << 7) | 0x13);
            // 加载高 32 位到 T0
            let hi_lo12 = (hi & 0xFFF) as i32;
            let hi_lo12_ext = if hi_lo12 >= 0x800 { hi_lo12 - 0x1000 } else { hi_lo12 };
            let hi_upper = ((hi as i64).wrapping_sub(hi_lo12_ext as i64) as u32) >> 12;
            if hi_upper != 0 { self.lui(T0, hi_upper); }
            self.addi(T0, if hi_upper != 0 { T0 } else { ZERO }, hi_lo12_ext);
            // OR rd, rd, T0
            self.r_type(0x00, T0, rd, 0x6, rd, 0x33);
        }
    }

    // ================ 全局变量访问 ================

    /// 加载全局变量值到寄存器（AUIPC + LD，后续修补偏移）
    fn load_global(&mut self, rd: u8, g_offset: usize) {
        let auipc_pos = self.code.len();
        self.auipc(rd, 0);     // 占位
        let mem_pos = self.code.len();
        self.ld(rd, rd, 0);    // 占位
        self.global_patches.push(GlobalPatch { auipc_pos, mem_pos, global_offset: g_offset });
    }

    /// 存储寄存器值到全局变量（AUIPC + SD，后续修补偏移）
    fn store_global(&mut self, rs: u8, g_offset: usize) {
        let auipc_pos = self.code.len();
        self.auipc(T6, 0);     // T6 作为临时地址
        let mem_pos = self.code.len();
        self.sd(rs, T6, 0);
        self.global_patches.push(GlobalPatch { auipc_pos, mem_pos, global_offset: g_offset });
    }

    /// 修补所有全局变量 AUIPC+LD/SD
    fn patch_globals(&mut self) {
        eprintln!("patch_globals: {} patches, globals_data_offset={}",
            self.global_patches.len(), self.globals_data_offset);
        let globals_start = self.globals_data_offset;
        for patch in &self.global_patches {
            let target = globals_start + patch.global_offset;
            let auipc_pos = patch.auipc_pos;
            let total_off = target as i32 - auipc_pos as i32;
            // AUIPC rd, upper: rd = PC + (upper << 12)
            // LD/SD rd, lower(rd)
            // lower + (upper << 12) = total_off
            let lower = total_off << 20 >> 20; // sign-extend bits[11:0]
            let upper = (total_off.wrapping_sub(lower)) >> 12;
            eprintln!("    → upper={}, lower={}", upper, lower);
            // 修补 AUIPC 的 imm 字段 (bits[31:12])
            // U-type: imm 字段是完整的 32 位，取 bits[31:12]
            let auipc_word = u32::from_le_bytes(self.code[auipc_pos..auipc_pos+4].try_into().unwrap());
            let rd_bits = auipc_word & (0x1F << 7);
            let new_auipc = ((upper as u32 & 0xFFFFF) << 12) | rd_bits | 0x17;
            self.code[auipc_pos..auipc_pos+4].copy_from_slice(&new_auipc.to_le_bytes());
            // 修补 LD/SD 的 offset 字段
            // 注意：LD 是 I-type (imm 在 bits[31:20])
            //      SD 是 S-type (imm[4:0] 在 bits[11:7], imm[11:5] 在 bits[31:25])
            let mem_pos = patch.mem_pos;
            let mem_word = u32::from_le_bytes(self.code[mem_pos..mem_pos+4].try_into().unwrap());
            let opcode = mem_word & 0x7F;

            let new_mem = if opcode == 0x03 {
                // I-type (LD): imm 在 bits[31:20]
                (mem_word & !(0xFFF << 20)) | (((lower as u32) & 0xFFF) << 20)
            } else {
                // S-type (SD/SW/SB): imm[4:0] 在 bits[11:7], imm[11:5] 在 bits[31:25]
                let lower_u = lower as u32;
                let imm_4_0 = (lower_u & 0x1F) << 7;
                let imm_11_5 = ((lower_u >> 5) & 0x7F) << 25;
                (mem_word & !((0x1F << 7) | (0x7F << 25))) | imm_4_0 | imm_11_5
            };
            self.code[mem_pos..mem_pos+4].copy_from_slice(&new_mem.to_le_bytes());
        }
    }

    // ================ 函数生成 ================

    /// no-op 函数（直接返回）
    fn emit_nop(&mut self, name: &str) {
        self.fn_start(name);
        self.ret();
        self.fn_end();
    }

    /// _start — 程序入口
    fn emit_start(&mut self) {
        self.fn_start(runtime_names::START);

        // 保存 callee-saved 到系统栈：ra + s0-s11 = 13 个 × 8 = 104, 对齐到 128
        self.addi(SP, SP, -128);
        let mut o = 0i32;
        for &reg in &[RA, S0_FP, S1, S2, S3, S4, S5, S6, S7, S8, S9, S10, S11] {
            self.sd(reg, SP, o); o += 8;
        }
        // o=104: main 返回值保存位置

        // 保存 callee-saved 到系统栈
        self.mv(S3, SP);           // S3 = system_sp (callee-saved 之前)

        // ---- mmap 虚拟栈 64KB ----
        self.li(A0, 0);
        self.li(A1, 65536);
        self.li(A2, 3);            // PROT_READ | PROT_WRITE
        self.li(A3, 0x22);         // MAP_PRIVATE | MAP_ANON
        self.li(A4, -1i64 as u64 as i64);
        self.li(A5, 0);
        self.li(A7, 222);
        self.ecall();
        // a0 = vstack_base
        self.mv(S2, A0);           // S2 = vstack_base

        // 初始化虚拟栈顶（不修改 SP，保持 SP 为系统栈）
        self.li(T0, 65520);
        self.add(A0, S2, T0);      // A0 = vstack_base + 65520 = vm_sp
        self.sd(ZERO, A0, 0);      // sentinel
        self.mv(S0_FP, A0);        // vm_fp = vm_sp

        // ---- mmap 堆 4MB ----
        self.li(A0, 0);
        self.li(A1, 4 * 1024 * 1024);
        self.li(A2, 3);
        self.li(A3, 0x22);
        self.li(A4, -1i64 as u64 as i64);
        self.li(A5, 0);
        self.li(A7, 222);
        self.ecall();
        // a0 = heap_base
        self.mv(S4, A0);           // S4 = heap_base
        self.li(T0, 4 * 1024 * 1024);
        self.add(S5, S4, T0);      // S5 = heap_limit

        // 写入全局变量初始值
        self.store_global(S4, G_BUMP_PTR);        // bump_ptr = heap_base
        self.store_global(S4, G_HEAP_START);       // heap_start
        self.store_global(S5, G_HEAP_LIMIT);       // heap_limit
        self.store_global(S2, G_VSTACK_BOTTOM);    // vstack_bottom
        self.li(A0, 0);
        self.store_global(A0, G_ALLOC_COUNT);      // alloc_count = 0
        self.li(A0, 256);
        self.store_global(A0, G_GC_THRESHOLD);     // gc_threshold = 256

        // ---- 调用 main ----
        // a0 = vm_sp (虚拟栈顶), a1 = vstack_bottom
        // 注意：SP(x2) 保持为系统栈，main 函数的 prologue 负责保存 callee-saved 到系统栈
        self.li(T0, 65520);
        self.add(A0, S2, T0);      // a0 = vm_sp = vstack_base + 65520
        self.mv(A1, S2);           // a1 = vstack_bottom
        self.call_main_offset = self.code.len();
        self.jal(RA, 0);           // 占位

        // main 返回值在 a0
        // 先恢复系统栈，然后把返回值保存到系统栈
        self.mv(SP, S3);           // 恢复系统 SP
        self.sd(A0, SP, 104);      // 在系统栈上保存返回值（slot 13，ra 之后）

        // 恢复 callee-saved
        o = 0;
        for &reg in &[RA, S0_FP, S1, S2, S3, S4, S5, S6, S7, S8, S9, S10, S11] {
            self.ld(reg, SP, o); o += 8;
        }
        // 读取 main 返回值
        self.ld(A0, SP, 104);
        self.addi(SP, SP, 128);

        // exit_group(a0)
        self.li(A7, 94);
        self.ecall();

        // ---- 全局数据区 (48 bytes) ----
        while self.code.len() % 8 != 0 { self.addi(ZERO, ZERO, 0); } // NOP 对齐
        self.globals_data_offset = self.code.len();
        for _ in 0..6 {
            self.code.extend_from_slice(&0u64.to_le_bytes());
        }
        // 注册全局数据区
        self.functions.push(RuntimeFunction { name: "__bump_ptr".into(), offset: self.globals_data_offset + G_BUMP_PTR, size: 8 });
        self.functions.push(RuntimeFunction { name: "__heap_start".into(), offset: self.globals_data_offset + G_HEAP_START, size: 8 });
        self.functions.push(RuntimeFunction { name: "__heap_limit".into(), offset: self.globals_data_offset + G_HEAP_LIMIT, size: 8 });
        self.functions.push(RuntimeFunction { name: "__vstack_bottom".into(), offset: self.globals_data_offset + G_VSTACK_BOTTOM, size: 8 });
        self.functions.push(RuntimeFunction { name: "__alloc_count".into(), offset: self.globals_data_offset + G_ALLOC_COUNT, size: 8 });
        self.functions.push(RuntimeFunction { name: "__gc_threshold".into(), offset: self.globals_data_offset + G_GC_THRESHOLD, size: 8 });

        self.fn_end();
    }

    /// __karte_gc_alloc_aligned(size) — bump 分配器 + GC
    fn emit_gc_alloc(&mut self) {
        self.fn_start(runtime_names::GC_ALLOC_ALIGNED);

        // 保存
        self.addi(SP, SP, -32);
        self.sd(RA, SP, 0);
        self.sd(S1, SP, 8);
        self.sd(S2, SP, 16);
        self.sd(S3, SP, 24);

        self.mv(S1, A0);          // S1 = size

        // total_size = align_up(8 + size, 16)
        self.addi(T0, ZERO, 8);
        self.add(T0, T0, S1);     // T0 = 8 + size
        self.addi(T0, T0, 15);
        self.andi(T0, T0, -16);   // T0 &= ~15
        self.mv(S2, T0);          // S2 = total_size

        // ---- alloc_start: 尝试分配 ----
        let alloc_start = self.code.len();

        // 加载 bump_ptr 和 heap_limit
        self.load_global(T0, G_BUMP_PTR);    // T0 = bump_ptr
        self.load_global(T1, G_HEAP_LIMIT);  // T1 = heap_limit

        // new_bump = bump_ptr + total_size
        self.add(T2, T0, S2);

        // 检查 new_bump <= heap_limit
        // if heap_limit < new_bump → overflow (触发 GC)
        self.bgeu(T2, T1, 0); // 占位 → overflow
        let overflow_jmp = self.code.len() - 4;

        // 正常：更新 bump_ptr
        self.store_global(T2, G_BUMP_PTR);

        // 写 GC 头
        self.sub(T3, T2, S2);     // T3 = old_bump
        self.sw(S2, T3, 4);       // [old_bump+4] = total_size

        // alloc_count++
        self.load_global(T4, G_ALLOC_COUNT);
        self.addi(T4, T4, 1);
        self.store_global(T4, G_ALLOC_COUNT);

        // 返回 data_ptr = old_bump + 8
        self.addi(A0, T3, 8);
        self.ld(RA, SP, 0);
        self.ld(S1, SP, 8);
        self.ld(S2, SP, 16);
        self.ld(S3, SP, 24);
        self.addi(SP, SP, 32);
        self.ret();

        // overflow: 触发 GC 然后重试
        let overflow_label = self.code.len();
        // 调用 gc_collect(0) — 传入 vm_sp=0 (GC 使用全局 vstack_bottom)
        self.addi(A0, ZERO, 0);
        // 使用 AUIPC + LD + JALR 调用 gc_collect
        self.auipc(T1, 0);
        self.ld(T1, T1, 12);
        self.jal(ZERO, 12);
        let gc_collect_addr_pos = self.code.len();
        self.u64(0); // 稍后修补
        self.jalr(RA, T1, 0);

        // GC 后重试分配
        self.jal(ZERO, alloc_start as i32 - self.code.len() as i32);

        // 修补 overflow 跳转
        fn patch_btype(code: &mut Vec<u8>, jmp_pos: usize, target: usize) {
            let offset = target as i32 - jmp_pos as i32;
            let orig = u32::from_le_bytes(code[jmp_pos..jmp_pos+4].try_into().unwrap());
            let funct3 = ((orig >> 12) & 0x7) as u8;
            let rs1 = ((orig >> 15) & 0x1F) as u8;
            let rs2 = ((orig >> 20) & 0x1F) as u8;
            let opcode = (orig & 0x7F) as u8;
            let v = offset as u32;
            let instr = (((v >> 12) & 1) << 31)
                | (((v >> 5) & 0x3F) << 25)
                | ((rs2 as u32 & 0x1F) << 20)
                | ((rs1 as u32 & 0x1F) << 15)
                | ((funct3 as u32 & 0x7) << 12)
                | (((v >> 1) & 0xF) << 8)
                | (((v >> 11) & 1) << 7)
                | (opcode as u32 & 0x7F);
            code[jmp_pos..jmp_pos+4].copy_from_slice(&instr.to_le_bytes());
        }
        patch_btype(&mut self.code, overflow_jmp, overflow_label);

        // 记录 gc_collect 调用位置用于修补
        self.functions.push(RuntimeFunction {
            name: "__gc_alloc_call_collect".into(),
            offset: gc_collect_addr_pos,
            size: 8,
        });

        self.fn_end();
    }

    /// __karte_gc_collect(vm_sp) — 简化 mark-sweep GC
    ///
    /// 参数 A0 = vm_sp (虚拟栈顶)
    /// 算法：
    ///   1. 标记：扫描虚拟栈 [vstack_bottom, vm_sp)，对堆指针做标记
    ///   2. 清除：遍历堆，将 bump_ptr 回退到第一个未标记对象之后
    ///
    /// 对象布局：[8 bytes header] [data...]
    ///   header[0] bit 0: mark bit
    ///   header[4..8]: total_size (u32)
    ///
    /// 寄存器分配：
    ///   S1 = heap_start
    ///   S2 = bump_ptr (当前)
    ///   S3 = vstack_bottom (扫描起点)
    ///   S4 = vm_sp (扫描终点 = 参数 A0)
    fn emit_gc_collect(&mut self) {
        self.fn_start(runtime_names::GC_COLLECT);

        // 保存 callee-saved
        self.addi(SP, SP, -64);
        self.sd(RA, SP, 0);
        self.sd(S1, SP, 8);
        self.sd(S2, SP, 16);
        self.sd(S3, SP, 24);
        self.sd(S4, SP, 32);
        self.sd(S5, SP, 40);
        self.sd(S6, SP, 48);
        self.sd(S7, SP, 56);

        // 加载全局变量
        self.load_global(S1, G_HEAP_START);     // S1 = heap_start
        self.load_global(S2, G_BUMP_PTR);        // S2 = bump_ptr
        self.load_global(S3, G_VSTACK_BOTTOM);   // S3 = vstack_bottom
        self.mv(S4, A0);                          // S4 = vm_sp

        // ---- 1. 标记阶段：扫描虚拟栈 ----
        // 如果 vm_sp == 0，跳过扫描
        self.beq(S4, ZERO, 0); // 占位 → skip_scan
        let skip_scan_jmp = self.code.len() - 4;

        // T0 = vstack_bottom (扫描指针)
        self.mv(T0, S3); // T0 = vstack_bottom

        // scan_loop:
        let scan_loop = self.code.len();
        // if T0 >= S4 (vm_sp), 跳出
        self.bgeu(T0, S4, 0); // 占位 → scan_done
        let scan_done_jmp = self.code.len() - 4;

        // T1 = [T0] (读取一个 word)
        self.ld(T1, T0, 0);

        // 检查 T1 是否在堆范围 [heap_start, bump_ptr)
        self.bltu(T1, S1, 0); // 占位 → scan_next (< heap_start)
        let scan_skip1_jmp = self.code.len() - 4;
        self.bgeu(T1, S2, 0); // 占位 → scan_next (>= bump_ptr)
        let scan_skip2_jmp = self.code.len() - 4;

        // T1 是堆内指针，需要找到对应的对象头并标记
        // 简化：假设 T1 指向 data 区域 (header+8 之后)
        // 对象头 = T1 - 8 (假设 T1 恰好指向 data 开头)
        // 但 T1 可能指向 data 中间，所以需要线性搜索

        // 线性搜索：从 heap_start 遍历，找到包含 T1 的对象
        self.mv(T2, S1); // T2 = 当前搜索位置

        // find_loop:
        let find_loop = self.code.len();
        // if T2 >= bump_ptr, 没找到 → scan_next
        self.bgeu(T2, S2, 0); // 占位 → scan_next
        let find_done_jmp = self.code.len() - 4;

        // T3 = total_size = [T2+4] (u32，零扩展加载)
        // 用 LW 加载然后零扩展
        self.i_type(4, T2, 0x2, T3, 0x03); // LW T3, 4(T2) → 32-bit 加载

        // 检查 T1 >= T2 + 8 (data_start)
        self.addi(T4, T2, 8); // T4 = T2 + 8 = data_start
        self.bltu(T1, T4, 0); // 占位 → find_next (< data_start)
        let find_skip_jmp = self.code.len() - 4;

        // 检查 T1 < T2 + total_size
        self.add(T4, T2, T3); // T4 = T2 + total_size
        self.bgeu(T1, T4, 0); // 占位 → find_next (>= end)
        let find_skip2_jmp = self.code.len() - 4;

        // 找到包含 T1 的对象，标记它
        // LB T4, 0(T2) → 读取 mark byte
        self.i_type(0, T2, 0x0, T4, 0x03); // LB T4, 0(T2)
        // 检查是否已标记 (bit 0)
        self.andi(T4, T4, 1);
        self.bne(T4, ZERO, 0); // 占位 → already_marked
        let already_marked_jmp = self.code.len() - 4;

        // 标记：设置 header[0] bit 0
        self.i_type(0, T2, 0x0, T4, 0x03); // LB T4, 0(T2)
        self.ori(T4, T4, 1);                // T4 |= 1
        self.s_type(0, T4, T2, 0x0, 0x23);  // SB T4, 0(T2)

        // 标记后，扫描对象 data 区域中的潜在指针（递归标记）
        // 为简化，暂不实现递归标记，只标记直接引用

        // already_marked / find_next:
        let already_marked_label = self.code.len();
        // T2 += total_size，继续搜索
        self.add(T2, T2, T3);
        self.jal(ZERO, find_loop as i32 - self.code.len() as i32);

        // ---- 修补 find 循环的跳转 ----
        // find_done → scan_next
        let scan_next_label_placeholder = 0; // 稍后修补
        // find_skip → find_next (= already_marked_label，继续搜索)
        // find_skip2 → find_next

        // scan_next:
        let scan_next_label = self.code.len();
        // T0 += 8 (下一个 word)
        self.addi(T0, T0, 8);
        self.jal(ZERO, scan_loop as i32 - self.code.len() as i32);

        // scan_done:
        let scan_done_label = self.code.len();

        // ---- 2. 清除阶段：遍历堆 ----
        // 从 heap_start 到 bump_ptr，清除未标记的对象
        // 策略：将 bump_ptr 回退，跳过标记的对象（紧凑化）
        // 简化：遍历堆，对未标记的对象不做处理（只清除标记位）
        //        然后 reset alloc_count

        self.mv(T0, S1); // T0 = 当前堆位置

        // sweep_loop:
        let sweep_loop = self.code.len();
        // if T0 >= bump_ptr, 结束
        self.bgeu(T0, S2, 0); // 占位 → sweep_done
        let sweep_done_jmp = self.code.len() - 4;

        // T1 = total_size = [T0+4]
        self.i_type(4, T0, 0x2, T1, 0x03); // LW T1, 4(T0)

        // 检查 mark bit
        self.i_type(0, T0, 0x0, T2, 0x03); // LB T2, 0(T0)
        self.andi(T2, T2, 1);

        // 清除标记位 (为下次 GC 准备)
        self.i_type(0, T0, 0x0, T3, 0x03); // LB T3, 0(T0)
        self.andi(T3, T3, -2 as i32);       // T3 &= ~1
        self.s_type(0, T3, T0, 0x0, 0x23);  // SB T3, 0(T0)

        // 如果 total_size == 0，跳过（安全检查）
        self.beq(T1, ZERO, 0); // 占位 → sweep_next
        let sweep_safety_jmp = self.code.len() - 4;

        // T0 += total_size
        self.add(T0, T0, T1);
        self.jal(ZERO, sweep_loop as i32 - self.code.len() as i32);

        // sweep_next (safety):
        self.addi(T0, T0, 16); // 最小步进
        self.jal(ZERO, sweep_loop as i32 - self.code.len() as i32);

        // sweep_done:
        let sweep_done_label = self.code.len();

        // 重置 alloc_count
        self.li(T0, 0);
        self.store_global(T0, G_ALLOC_COUNT);

        // skip_scan:
        let skip_scan_label = self.code.len();

        // 恢复 callee-saved
        self.ld(RA, SP, 0);
        self.ld(S1, SP, 8);
        self.ld(S2, SP, 16);
        self.ld(S3, SP, 24);
        self.ld(S4, SP, 32);
        self.ld(S5, SP, 40);
        self.ld(S6, SP, 48);
        self.ld(S7, SP, 56);
        self.addi(SP, SP, 64);
        self.ret();

        // ---- 修补所有跳转 ----
        fn patch_btype(code: &mut Vec<u8>, jmp_pos: usize, target: usize) {
            let offset = target as i32 - jmp_pos as i32;
            // 重写 B-type 的 offset
            let orig = u32::from_le_bytes(code[jmp_pos..jmp_pos+4].try_into().unwrap());
            let funct3 = ((orig >> 12) & 0x7) as u8;
            let rs1 = ((orig >> 15) & 0x1F) as u8;
            let rs2 = ((orig >> 20) & 0x1F) as u8;
            let opcode = (orig & 0x7F) as u8;
            // 重新编码 B-type
            let v = offset as u32;
            let instr = (((v >> 12) & 1) << 31)
                | (((v >> 5) & 0x3F) << 25)
                | ((rs2 as u32 & 0x1F) << 20)
                | ((rs1 as u32 & 0x1F) << 15)
                | ((funct3 as u32 & 0x7) << 12)
                | (((v >> 1) & 0xF) << 8)
                | (((v >> 11) & 1) << 7)
                | (opcode as u32 & 0x7F);
            code[jmp_pos..jmp_pos+4].copy_from_slice(&instr.to_le_bytes());
        }
        fn patch_jtype(code: &mut Vec<u8>, jmp_pos: usize, target: usize) {
            let offset = target as i32 - (jmp_pos as i32);
            let v = offset as u32;
            let instr = (((v >> 20) & 1) << 31)
                | (((v >> 1) & 0x3FF) << 21)
                | (((v >> 11) & 1) << 20)
                | (((v >> 12) & 0xFF) << 12)
                | (0u32 << 7) | 0x6F;
            code[jmp_pos..jmp_pos+4].copy_from_slice(&instr.to_le_bytes());
        }

        // skip_scan: vm_sp == 0 时跳到 skip_scan_label
        patch_btype(&mut self.code, skip_scan_jmp, skip_scan_label);

        // scan_done: T0 >= vm_sp
        patch_btype(&mut self.code, scan_done_jmp, scan_done_label);

        // scan_skip1: T1 < heap_start → scan_next
        patch_btype(&mut self.code, scan_skip1_jmp, scan_next_label);
        // scan_skip2: T1 >= bump_ptr → scan_next
        patch_btype(&mut self.code, scan_skip2_jmp, scan_next_label);

        // find_done: T2 >= bump_ptr → scan_next
        patch_btype(&mut self.code, find_done_jmp, scan_next_label);
        // find_skip: T1 < data_start → already_marked (继续搜索)
        patch_btype(&mut self.code, find_skip_jmp, already_marked_label);
        // find_skip2: T1 >= end → already_marked (继续搜索)
        patch_btype(&mut self.code, find_skip2_jmp, already_marked_label);
        // already_marked: 已标记 → already_marked_label
        patch_btype(&mut self.code, already_marked_jmp, already_marked_label);

        // sweep_done: T0 >= bump_ptr
        patch_btype(&mut self.code, sweep_done_jmp, sweep_done_label);
        // sweep_safety: total_size == 0
        let sweep_next_label = sweep_loop; // sweep_next 就是继续循环
        // 修补 sweep_loop 中的 JAL 指令（向回跳）
        // 它们已经在 emit 时计算了正确的 offset

        self.fn_end();
    }

    /// __karte_gc_safepoint(vm_sp) — 检查是否需要 GC，需要则触发
    fn emit_gc_safepoint(&mut self) {
        self.fn_start(runtime_names::GC_SAFEPOINT);

        // 保存 RA
        self.addi(SP, SP, -16);
        self.sd(RA, SP, 0);

        // 加载 alloc_count
        self.load_global(T0, G_ALLOC_COUNT);

        // 加载 gc_threshold
        self.load_global(T1, G_GC_THRESHOLD);

        // if alloc_count < threshold, 跳过
        self.blt(T0, T1, 0); // 占位 → no_gc
        let no_gc_jmp = self.code.len() - 4;

        // 触发 GC: 调用 gc_collect(vm_sp)
        // A0 已经包含 vm_sp (参数)
        // 需要生成 CALL gc_collect 的代码
        // 使用 AUIPC + LD + JALR 模式 (patch_globals 修补)
        self.auipc(T1, 0);
        self.ld(T1, T1, 12);
        self.jal(ZERO, 12);
        // 8 bytes 占位 (gc_collect 的地址)
        let gc_collect_addr_pos = self.code.len();
        self.u64(0); // 稍后修补
        self.jalr(RA, T1, 0);

        // 记录需要修补的位置
        // 这里我们需要把 gc_collect 函数的地址写到这里
        // 但 patch_globals 不会处理这种自定义 patch
        // 改用直接 CALL (通过记录偏移量，在 generate 最后修补)
        // 但 RISC-V 没有直接的 CALL，需要 AUIPC+JALR
        // 简化：把 gc_collect 的 patch 信息存起来

        // no_gc:
        let no_gc_label = self.code.len();
        self.ld(RA, SP, 0);
        self.addi(SP, SP, 16);
        self.ret();

        // 修补 no_gc 跳转
        fn patch_btype(code: &mut Vec<u8>, jmp_pos: usize, target: usize) {
            let offset = target as i32 - jmp_pos as i32;
            let orig = u32::from_le_bytes(code[jmp_pos..jmp_pos+4].try_into().unwrap());
            let funct3 = ((orig >> 12) & 0x7) as u8;
            let rs1 = ((orig >> 15) & 0x1F) as u8;
            let rs2 = ((orig >> 20) & 0x1F) as u8;
            let opcode = (orig & 0x7F) as u8;
            let v = offset as u32;
            let instr = (((v >> 12) & 1) << 31)
                | (((v >> 5) & 0x3F) << 25)
                | ((rs2 as u32 & 0x1F) << 20)
                | ((rs1 as u32 & 0x1F) << 15)
                | ((funct3 as u32 & 0x7) << 12)
                | (((v >> 1) & 0xF) << 8)
                | (((v >> 11) & 1) << 7)
                | (opcode as u32 & 0x7F);
            code[jmp_pos..jmp_pos+4].copy_from_slice(&instr.to_le_bytes());
        }
        patch_btype(&mut self.code, no_gc_jmp, no_gc_label);

        // 记录 safepoint 内部调用 gc_collect 的位置，用于后续修补
        self.functions.push(RuntimeFunction {
            name: "__safepoint_call_collect".into(),
            offset: gc_collect_addr_pos,
            size: 8,
        });

        self.fn_end();
    }

    /// 修补 safepoint 中对 gc_collect 的调用
    pub fn patch_internal_calls(&mut self) {
        let gc_collect_offset = self.find_offset(runtime_names::GC_COLLECT).unwrap();
        let code_base = 0; // runtime 代码从 0 开始

        for f in &self.functions.clone() {
            if f.name == "__safepoint_call_collect" || f.name == "__gc_alloc_call_collect" {
                let addr_pos = f.offset;
                let abs_addr = code_base + gc_collect_offset;
                self.code[addr_pos..addr_pos+8].copy_from_slice(&abs_addr.to_le_bytes());
            }
        }
    }
}
