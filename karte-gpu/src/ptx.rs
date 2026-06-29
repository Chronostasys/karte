//! PTX 后端 — 将 GIR 编译为 NVIDIA PTX 汇编文本

use karte_gir::*;

/// PTX 编译器 — 将 GIR 程序编译为 PTX 文本
pub struct PtxCompiler {
    /// 目标 SM 版本 (如 (8, 0) = SM 8.0)
    sm_version: (u32, u32),
    /// 输出缓冲
    output: String,
    /// 寄存器类型追踪: reg_id → GirDType
    /// 用于自动插入 I32→I64 的 cvt 指令
    reg_types: std::collections::HashMap<usize, GirDType>,
}

impl PtxCompiler {
    /// 创建默认编译器（SM 8.0 = A100）
    pub fn new() -> Self {
        Self {
            sm_version: (8, 0),
            output: String::new(),
            reg_types: std::collections::HashMap::new(),
        }
    }

    /// 设置目标 SM 版本
    pub fn target(mut self, major: u32, minor: u32) -> Self {
        self.sm_version = (major, minor);
        self
    }

    /// 编译 GIR 程序为 PTX 文本
    pub fn compile(&mut self, gir: &GirProgram) -> String {
        self.emit_header();
        self.output.push('\n');

        for kernel in &gir.kernels {
            self.compile_kernel(kernel);
            self.output.push('\n');
        }

        self.output.clone()
    }

    /// 输出 PTX 头部
    fn emit_header(&mut self) {
        // PTX ISA 版本固定为 8.0（广泛兼容），SM 版本单独设置
        self.output.push_str(".version 8.0\n");
        let (major, minor) = self.sm_version;
        self.output.push_str(&format!(".target sm_{}_{}\n", major, minor));
        self.output.push_str(".address_size 64\n");
    }

    /// 编译单个 kernel
    fn compile_kernel(&mut self, func: &GirFunction) {
        self.reg_types.clear();
        // .entry kernel_name(.param ...) {
        self.output.push_str(&format!(".entry {}(", func.name));
        for (i, param) in func.params.iter().enumerate() {
            if i > 0 {
                self.output.push_str(", ");
            }
            let ptx_type = if param.is_ptr {
                "u64".to_string()
            } else {
                param.dtype.ptx_suffix().to_string()
            };
            self.output.push_str(&format!(".param .{} %param_{}", ptx_type, i));
        }
        self.output.push_str(") {\n");

        // 寄存器声明 — 包含 cvt 临时寄存器空间（i64_operand 使用 id+8000）
        let num_regs = func.next_reg.max(1);
        let total_regs = num_regs + 8100; // 预留空间给 cvt 临时寄存器
        self.output.push_str(&format!("    .reg .s32 %r<{}>;\n", total_regs));
        self.output.push_str(&format!("    .reg .s64 %rd<{}>;\n", total_regs));
        self.output.push_str(&format!("    .reg .f32 %f<{}>;\n", total_regs));
        self.output.push_str("    .reg .pred %p<16>;\n");

        // 共享内存声明（如果 kernel 使用了共享内存）
        if func.shared_mem_size > 0 {
            self.output.push_str(&format!("    .shared .align 16 .b8 smem[{}];\n", func.shared_mem_size));
        }

        // 加载参数到寄存器 — 根据参数类型选择正确的寄存器前缀
        for (i, param) in func.params.iter().enumerate() {
            if param.is_ptr {
                self.output.push_str(&format!("    ld.param.u64 %rd{}, [%param_{}];\n", i, i));
                self.track_reg(i, GirDType::I64);
            } else if param.dtype == GirDType::F32 {
                self.output.push_str(&format!("    ld.param.f32 %f{}, [%param_{}];\n", i, i));
                self.track_reg(i, GirDType::F32);
            } else {
                self.output.push_str(&format!("    ld.param.s32 %r{}, [%param_{}];\n", i, i));
                self.track_reg(i, GirDType::I32);
            }
        }

        // 编译指令（跳过最后一条如果是 Return，避免重复 ret）
        let instrs = &func.instructions;
        let last_is_ret = instrs.last().map(|i| matches!(i, GirInstruction::Return)).unwrap_or(false);
        let end = if last_is_ret { instrs.len() - 1 } else { instrs.len() };
        let mut label_counter = 0;
        for instr in &instrs[..end] {
            self.compile_instruction(instr, &mut label_counter);
        }

        self.output.push_str("    ret;\n");
        self.output.push_str("}\n");
    }

    /// 编译单条 GIR 指令为 PTX
    fn compile_instruction(&mut self, instr: &GirInstruction, _label_counter: &mut usize) {
        match instr {
            // —— 标量算术 ——
            GirInstruction::Move { dst, src } => {
                let (dst_s, src_s) = (reg64(*dst), operand_to_str(src, true));
                self.emit(&format!("    mov.s64 {}, {};", dst_s, src_s));
                self.track_reg(*dst, GirDType::I64);
            }
            GirInstruction::Add { dst, src1, src2, dtype } => {
                match dtype {
                    GirDType::F32 => {
                        self.emit(&format!("    add.f32 %f{}, {}, {};", *dst, operand_to_str_f32(src1), operand_to_str_f32(src2)));
                        self.track_reg(*dst, GirDType::F32);
                    }
                    GirDType::I64 => {
                        let s1 = self.i64_operand(src1);
                        let s2 = self.i64_operand(src2);
                        self.emit(&format!("    add.s64 %rd{}, {}, {};", *dst, s1, s2));
                        self.track_reg(*dst, GirDType::I64);
                    }
                    _ => {
                        self.emit(&format!("    add.s32 %r{}, {}, {};", *dst, operand_to_str(src1, false), operand_to_str(src2, false)));
                        self.track_reg(*dst, GirDType::I32);
                    }
                }
            }
            GirInstruction::Sub { dst, src1, src2, dtype } => {
                match dtype {
                    GirDType::F32 => {
                        self.emit(&format!("    sub.f32 %f{}, {}, {};", *dst, operand_to_str_f32(src1), operand_to_str_f32(src2)));
                        self.track_reg(*dst, GirDType::F32);
                    }
                    GirDType::I64 => {
                        let s1 = self.i64_operand(src1);
                        let s2 = self.i64_operand(src2);
                        self.emit(&format!("    sub.s64 %rd{}, {}, {};", *dst, s1, s2));
                        self.track_reg(*dst, GirDType::I64);
                    }
                    _ => {
                        self.emit(&format!("    sub.s32 %r{}, {}, {};", *dst, operand_to_str(src1, false), operand_to_str(src2, false)));
                        self.track_reg(*dst, GirDType::I32);
                    }
                }
            }
            GirInstruction::Mul { dst, src1, src2, dtype } => {
                match dtype {
                    GirDType::F32 => {
                        self.emit(&format!("    mul.f32 %f{}, {}, {};", *dst, operand_to_str_f32(src1), operand_to_str_f32(src2)));
                        self.track_reg(*dst, GirDType::F32);
                    }
                    GirDType::I64 => {
                        let s1 = self.i64_operand(src1);
                        let s2 = self.i64_operand(src2);
                        self.emit(&format!("    mul.lo.s64 %rd{}, {}, {};", *dst, s1, s2));
                        self.track_reg(*dst, GirDType::I64);
                    }
                    _ => {
                        self.emit(&format!("    mul.lo.s32 %r{}, {}, {};", *dst, operand_to_str(src1, false), operand_to_str(src2, false)));
                        self.track_reg(*dst, GirDType::I32);
                    }
                }
            }
            GirInstruction::Div { dst, src1, src2, dtype } => {
                let is_f32 = *dtype == GirDType::F32;
                if is_f32 {
                    // f32 除法: 如果 src2 是立即数，用 rcp + mul（更高效且兼容 sm_12.0）
                    // 否则用 div.rn.f32
                    match src2 {
                        GirOperand::Imm(val) => {
                            // rcp.approx.f32 %tmp, imm; mul.f32 %dst, %src1, %tmp
                            let rcp_reg = *dst + 5000;
                            self.emit(&format!("    rcp.approx.f32 %f{}, 0f{:08X};", rcp_reg, *val as u32));
                            self.emit(&format!("    mul.f32 %f{}, {}, %f{};", *dst, operand_to_str_f32(src1), rcp_reg));
                        }
                        _ => {
                            self.emit(&format!("    div.rn.f32 %f{}, {}, {};", *dst, operand_to_str_f32(src1), operand_to_str_f32(src2)));
                        }
                    }
                    self.track_reg(*dst, GirDType::F32);
                } else {
                    let d = dtype.ptx_suffix();
                    let is_64 = *dtype == GirDType::I64 || *dtype == GirDType::F64;
                    let op = if d.starts_with('f') { "div.rn" } else { "div.full" };
                    self.emit(&format!("    {}.{} {}, {}, {};", op, d, reg(*dst, is_64), operand_to_str(src1, is_64), operand_to_str(src2, is_64)));
                    self.track_reg(*dst, *dtype);
                }
            }
            GirInstruction::Mod { dst, src1, src2, dtype } => {
                let d = dtype.ptx_suffix();
                let is_64 = *dtype == GirDType::I64 || *dtype == GirDType::F64;
                self.emit(&format!("    rem.{} {}, {}, {};", d, reg(*dst, is_64), operand_to_str(src1, is_64), operand_to_str(src2, is_64)));
            }
            GirInstruction::Fma { dst, src1, src2, src3, dtype } => {
                let is_f32 = *dtype == GirDType::F32;
                if is_f32 {
                    // sm_12.0 不支持 fma.rn.f32，展开为 mul + add
                    self.emit(&format!("    mul.f32 %f{}, {}, {};", *dst, operand_to_str_f32(src1), operand_to_str_f32(src2)));
                    self.emit(&format!("    add.f32 %f{}, %f{}, {};", *dst, *dst, operand_to_str_f32(src3)));
                } else {
                    let is_64 = *dtype == GirDType::I64 || *dtype == GirDType::F64;
                    self.emit(&format!("    mul.{} {}, {}, {};", dtype.ptx_suffix(), reg(*dst, is_64), operand_to_str(src1, is_64), operand_to_str(src2, is_64)));
                    self.emit(&format!("    add.{} {}, {}, {};", dtype.ptx_suffix(), reg(*dst, is_64), reg(*dst, is_64), operand_to_str(src3, is_64)));
                }
            }
            // Exp(x) = 2^(x * log2(e)) — 展开为 mul + ex2.approx
            GirInstruction::Exp { dst, src, dtype } => {
                let _ = dtype;
                // log2(e) = 1.442695041 = 0x3FB8AA3B
                self.emit(&format!("    mul.f32 %f{}, {}, 0f3FB8AA3B;", *dst, operand_to_str_f32(src)));
                self.emit(&format!("    ex2.approx.f32 %f{}, %f{};", *dst, *dst));
            }
            // Recip: dst = 1.0 / src
            GirInstruction::Recip { dst, src, dtype } => {
                let _ = dtype;
                self.emit(&format!("    rcp.approx.f32 %f{}, {};", *dst, operand_to_str_f32(src)));
            }

            // —— 比较 ——
            GirInstruction::Cmp { dst, op, src1, src2, dtype } => {
                let d = dtype.ptx_suffix();
                let is_64 = *dtype == GirDType::I64 || *dtype == GirDType::F64;
                let ptx_op = match op {
                    CmpOp::Eq => "eq",
                    CmpOp::Ne => "ne",
                    CmpOp::Lt => "lt",
                    CmpOp::Le => "le",
                    CmpOp::Gt => "gt",
                    CmpOp::Ge => "ge",
                };
                self.emit(&format!("    setp.{}.{} %p{}, {}, {};", ptx_op, d, *dst % 16, operand_to_str(src1, is_64), operand_to_str(src2, is_64)));
            }

            // —— 分支 ——
            GirInstruction::BranchIf { cond, then_label, else_label } => {
                let cond_str = match cond {
                    GirOperand::Reg(id) => format!("%p{}", id % 16),
                    _ => "%p0".to_string(),
                };
                self.emit(&format!("    @{} bra LABEL_{};", cond_str, then_label));
                self.emit(&format!("    bra LABEL_{};", else_label));
            }
            GirInstruction::Jump { target } => {
                self.emit(&format!("    bra LABEL_{};", target));
            }

            // —— GPU 内存 ——
            GirInstruction::GlobalLoad { dst, addr, dtype } => {
                let d = dtype.ptx_suffix();
                let is_f32 = *dtype == GirDType::F32;
                let dst_str = if is_f32 { format!("%f{}", *dst) } else { reg(*dst, *dtype == GirDType::I64 || *dtype == GirDType::F64) };
                self.emit(&format!("    ld.global.{} {}, [{}];", d, dst_str, operand_to_str(addr, true)));
            }
            GirInstruction::GlobalStore { addr, src, dtype } => {
                let d = dtype.ptx_suffix();
                let is_f32 = *dtype == GirDType::F32;
                let addr_s = self.i64_operand(addr);
                let src_str = if is_f32 { operand_to_str_f32(src) } else { operand_to_str(src, *dtype == GirDType::I64 || *dtype == GirDType::F64) };
                self.emit(&format!("    st.global.{} [{}], {};", d, addr_s, src_str));
            }
            // 向量化加载 4×f32 — PTX: ld.global.v4.f32 {%f0,%f1,%f2,%f3}, [addr];
            GirInstruction::GlobalLoadV4 { dst_base, addr, dtype } => {
                let _ = dtype;
                let addr_s = self.i64_operand(addr);
                self.emit(&format!(
                    "    ld.global.v4.f32 {{%f{}, %f{}, %f{}, %f{}}}, [{}];",
                    *dst_base, *dst_base+1, *dst_base+2, *dst_base+3,
                    addr_s,
                ));
                self.track_reg(*dst_base, GirDType::F32);
            }
            // 向量化存储 4×f32 — PTX: st.global.v4.f32 [addr], {%f0,%f1,%f2,%f3};
            GirInstruction::GlobalStoreV4 { addr, src_base, dtype } => {
                let _ = dtype;
                self.emit(&format!(
                    "    st.global.v4.f32 [{}], {{%f{}, %f{}, %f{}, %f{}}};",
                    operand_to_str(addr, true),
                    *src_base, *src_base+1, *src_base+2, *src_base+3,
                ));
            }
            // 向量化加载 2×f32
            GirInstruction::GlobalLoadV2 { dst_base, addr, dtype } => {
                let _ = dtype;
                self.emit(&format!(
                    "    ld.global.v2.f32 {{%f{}, %f{}}}, [{}];",
                    *dst_base, *dst_base+1,
                    operand_to_str(addr, true),
                ));
            }
            // 向量化存储 2×f32
            GirInstruction::GlobalStoreV2 { addr, src_base, dtype } => {
                let _ = dtype;
                self.emit(&format!(
                    "    st.global.v2.f32 [{}], {{%f{}, %f{}}};",
                    operand_to_str(addr, true),
                    *src_base, *src_base+1,
                ));
            }
            GirInstruction::SharedLoad { dst, addr, dtype } => {
                let d = dtype.ptx_suffix();
                let is_64 = *dtype == GirDType::I64 || *dtype == GirDType::F64;
                self.emit(&format!("    ld.shared.{} {}, [{}];", d, reg(*dst, is_64), operand_to_str(addr, true)));
            }
            GirInstruction::SharedStore { addr, src, dtype } => {
                let d = dtype.ptx_suffix();
                let is_64 = *dtype == GirDType::I64 || *dtype == GirDType::F64;
                self.emit(&format!("    st.shared.{} [{}], {};", d, operand_to_str(addr, true), operand_to_str(src, is_64)));
            }

            // —— 同步 ——
            GirInstruction::Barrier => {
                self.emit("    bar.sync 0;");
            }
            GirInstruction::WarpShuffle { dst, src, src_lane, op, dtype } => {
                let d = dtype.ptx_suffix();
                let ptx_op = match op {
                    ShuffleOp::Idx => "idx",
                    ShuffleOp::Bfly => "bfly",
                    ShuffleOp::Up => "up",
                    ShuffleOp::Down => "down",
                    ShuffleOp::Xor => "xor",
                };
                self.emit(&format!("    shfl.sync.{}.b32 %r{}, {}, {}, 0x1f;", ptx_op, *dst % 1000, operand_to_str(src, false), operand_to_str(src_lane, false)));
            }

            // —— Tensor Core MMA ——
            GirInstruction::Mma { dst, a, b, m, k, n, dtype_c, .. } => {
                let d = dtype_c.ptx_suffix();
                self.emit(&format!("    // MMA m={}=n={}=k={} dtype={}", m, n, k, d));
                self.emit(&format!("    // mma.m{}n{}k{}.row.col.{} {{...}}, {}, {}, {{...}};", m, n, k, d, operand_to_str(a, false), operand_to_str(b, false)));
            }

            // —— 线程索引 ——
            GirInstruction::ThreadId { dst, dim } => {
                let d = match dim { ThreadDim::X => "x", ThreadDim::Y => "y", ThreadDim::Z => "z" };
                self.emit(&format!("    mov.u32 %r{}, %tid.{};", *dst, d));
                self.track_reg(*dst, GirDType::I32);
            }
            GirInstruction::BlockId { dst, dim } => {
                let d = match dim { ThreadDim::X => "x", ThreadDim::Y => "y", ThreadDim::Z => "z" };
                self.emit(&format!("    mov.u32 %r{}, %ctaid.{};", *dst, d));
                self.track_reg(*dst, GirDType::I32);
            }
            GirInstruction::BlockDim { dst, dim } => {
                let d = match dim { ThreadDim::X => "x", ThreadDim::Y => "y", ThreadDim::Z => "z" };
                self.emit(&format!("    mov.u32 %r{}, %ntid.{};", *dst, d));
                self.track_reg(*dst, GirDType::I32);
            }
            GirInstruction::GridDim { dst, dim } => {
                let d = match dim { ThreadDim::X => "x", ThreadDim::Y => "y", ThreadDim::Z => "z" };
                self.emit(&format!("    mov.u32 %r{}, %nctaid.{};", *dst, d));
                self.track_reg(*dst, GirDType::I32);
            }

            // —— 标签与返回 ——
            GirInstruction::Label { id } => {
                self.output.push_str(&format!("LABEL_{}:\n", id));
            }
            GirInstruction::Return => {
                self.emit("    ret;");
            }

            // —— 高级 Tile 操作（正常应在 tile_expansion pass 中被展开）——
            GirInstruction::SharedAlloc { dst, size, dtype } => {
                self.emit(&format!("    // SharedAlloc: reg={} size={}B dtype={}", dst, size, dtype));
                // 共享内存基地址 = 0（smem 数组起始）
                self.emit(&format!("    mov.s64 {}, 0;", reg64(*dst)));
            }
            GirInstruction::TileLoad { dst, base, row, col, tile_rows, tile_cols, stride, dtype } => {
                self.emit(&format!("    // TileLoad: {}×{} dtype={} (should be expanded)", tile_rows, tile_cols, dtype));
            }
            GirInstruction::TileStore { base, row, col, src, tile_rows, tile_cols, stride, dtype } => {
                self.emit(&format!("    // TileStore: {}×{} dtype={} (should be expanded)", tile_rows, tile_cols, dtype));
            }
            GirInstruction::TileZeros { dst, tile_rows, tile_cols, dtype } => {
                self.emit(&format!("    // TileZeros: {}×{} dtype={} (should be expanded)", tile_rows, tile_cols, dtype));
            }
            GirInstruction::TileMatmul { dst, a, b, m, k, n, .. } => {
                self.emit(&format!("    // TileMatmul: {}×{}×{} (should be expanded)", m, k, n));
            }

            // —— 高级数学与条件操作 ——
            GirInstruction::MaskedGlobalLoad { dst, addr, mask, default_val, dtype } => {
                let is_f32 = *dtype == GirDType::F32;
                let d = dtype.ptx_suffix();
                let addr_s = self.i64_operand(addr);
                if is_f32 {
                    self.emit(&format!("    ld.global.f32 %f{}, [{}];", *dst, addr_s));
                } else {
                    self.emit(&format!("    ld.global.{} %r{}, [{}];", d, *dst, addr_s));
                }
                let mask_str = match mask {
                    GirOperand::Reg(id) => format!("%f{}", id),
                    GirOperand::Imm(v) => format!("0f{:08X}", *v as u32),
                    _ => "0f00000000".to_string(),
                };
                let pred = *dst % 16;
                self.emit(&format!("    setp.ne.f32 %p{}, {}, 0f00000000;", pred, mask_str));
                let default_str = match default_val {
                    GirOperand::Reg(id) => format!("%f{}", id),
                    GirOperand::Imm(v) => format!("0f{:08X}", *v as u32),
                    _ => "0f00000000".to_string(),
                };
                if is_f32 {
                    self.emit(&format!("    selp.f32 %f{}, %f{}, {}, %p{};", *dst, *dst, default_str, pred));
                } else {
                    self.emit(&format!("    selp.{} %r{}, %r{}, {}, %p{};", d, *dst, *dst, default_str, pred));
                }
                self.track_reg(*dst, *dtype);
            }
            GirInstruction::MaskedGlobalStore { addr, src, mask, dtype } => {
                let mask_str = match mask {
                    GirOperand::Reg(id) => format!("%f{}", id),
                    GirOperand::Imm(v) => format!("0f{:08X}", *v as u32),
                    _ => "0f00000000".to_string(),
                };
                let pred = 15;
                self.emit(&format!("    setp.ne.f32 %p{}, {}, 0f00000000;", pred, mask_str));
                let addr_s = self.i64_operand(addr);
                let d = dtype.ptx_suffix();
                let src_str = operand_to_str_f32(src);
                self.emit(&format!("    @%p{} st.global.{} [{}], {};", pred, d, addr_s, src_str));
            }
            GirInstruction::Reduce { dst, src, op, dtype } => {
                let _ = dtype;
                let src_str = operand_to_str_f32(src);
                let mut prev_reg = *dst;
                self.emit(&format!("    mov.f32 %f{}, {};", *dst, src_str));
                for offset in [16u32, 8, 4, 2, 1] {
                    let tmp = *dst + 100 + offset as usize;
                    let ptx_op = match op {
                        ReduceOp::Sum => "add.f32",
                        ReduceOp::Max => "max.f32",
                        ReduceOp::Min => "min.f32",
                    };
                    self.emit(&format!("    shfl.sync.down.b32 %f{}, %f{}, {}, 0x1f, 0x1f;", tmp, prev_reg, offset));
                    self.emit(&format!("    {} %f{}, %f{}, %f{};", ptx_op, tmp, prev_reg, tmp));
                    prev_reg = tmp;
                }
                self.emit(&format!("    shfl.sync.idx.b32 %f{}, %f{}, 0, 0x1f, 0x1f;", *dst, prev_reg));
            }
            GirInstruction::Where { dst, cond, then_val, else_val, dtype } => {
                let _ = dtype;
                let cond_str = operand_to_str_f32(cond);
                let pred = *dst % 16;
                self.emit(&format!("    setp.ne.f32 %p{}, {}, 0f00000000;", pred, cond_str));
                let then_str = operand_to_str_f32(then_val);
                let else_str = operand_to_str_f32(else_val);
                self.emit(&format!("    selp.f32 %f{}, {}, {}, %p{};", *dst, then_str, else_str, pred));
                self.track_reg(*dst, GirDType::F32);
            }
            GirInstruction::Sqrt { dst, src, dtype } => {
                let _ = dtype;
                self.emit(&format!("    sqrt.approx.f32 %f{}, {};", *dst, operand_to_str_f32(src)));
                self.track_reg(*dst, GirDType::F32);
            }
            GirInstruction::Log { dst, src, dtype } => {
                let _ = dtype;
                self.emit(&format!("    lg2.approx.f32 %f{}, {};", *dst, operand_to_str_f32(src)));
                self.emit(&format!("    mul.f32 %f{}, %f{}, 0f3F317218;", *dst, *dst));
                self.track_reg(*dst, GirDType::F32);
            }
            GirInstruction::Rsqrt { dst, src, dtype } => {
                let _ = dtype;
                self.emit(&format!("    rsqrt.approx.f32 %f{}, {};", *dst, operand_to_str_f32(src)));
                self.track_reg(*dst, GirDType::F32);
            }
            GirInstruction::Abs { dst, src, dtype } => {
                let _ = dtype;
                self.emit(&format!("    abs.f32 %f{}, {};", *dst, operand_to_str_f32(src)));
                self.track_reg(*dst, GirDType::F32);
            }
            GirInstruction::Max { dst, src1, src2, dtype } => {
                let _ = dtype;
                self.emit(&format!("    max.f32 %f{}, {}, {};", *dst, operand_to_str_f32(src1), operand_to_str_f32(src2)));
                self.track_reg(*dst, GirDType::F32);
            }
            GirInstruction::Min { dst, src1, src2, dtype } => {
                let _ = dtype;
                self.emit(&format!("    min.f32 %f{}, {}, {};", *dst, operand_to_str_f32(src1), operand_to_str_f32(src2)));
                self.track_reg(*dst, GirDType::F32);
            }
        }
    }

    /// 输出一行 PTX
    fn emit(&mut self, line: &str) {
        self.output.push_str(line);
        self.output.push('\n');
    }

    /// 获取 I64 上下文中的操作数字符串
    /// 如果寄存器之前定义为 I32，自动插入 cvt.u64.u32 转换
    fn i64_operand(&mut self, op: &GirOperand) -> String {
        match op {
            GirOperand::Reg(id) => {
                match self.reg_types.get(id) {
                    Some(t) if *t == GirDType::I32 => {
                        let cvt_reg = id + 8000;
                        self.emit(&format!("    cvt.s64.s32 %rd{}, %r{};", cvt_reg, id));
                        self.reg_types.insert(cvt_reg, GirDType::I64);
                        format!("%rd{}", cvt_reg)
                    }
                    _ => format!("%rd{}", id),
                }
            }
            GirOperand::Imm(v) => format!("{}", v),
            GirOperand::Param(id) => format!("%rd{}", id),
            _ => "0".to_string(),
        }
    }

    /// 记录寄存器定义类型
    fn track_reg(&mut self, id: usize, dtype: GirDType) {
        self.reg_types.insert(id, dtype);
    }
}

impl Default for PtxCompiler {
    fn default() -> Self {
        Self::new()
    }
}

/// 生成 64 位寄存器名
fn reg64(id: usize) -> String {
    format!("%rd{}", id)
}

/// 生成 32 位寄存器名
fn reg32(id: usize) -> String {
    format!("%r{}", id)
}

/// 根据位宽生成寄存器名
fn reg(id: usize, is_64: bool) -> String {
    if is_64 { reg64(id) } else { reg32(id) }
}

/// 将 GIR Operand 转换为 PTX 字符串
fn operand_to_str(op: &GirOperand, is_64: bool) -> String {
    match op {
        GirOperand::Reg(id) => reg(*id, is_64),
        GirOperand::Imm(val) => {
            if is_64 {
                format!("{}", val)
            } else {
                format!("{}", *val as i32)
            }
        }
        GirOperand::Label(id) => format!("LABEL_{}", id),
        GirOperand::Param(id) => {
            if is_64 { format!("%rd{}", id) } else { format!("%r{}", id) }
        }
    }
}

/// 将 GIR Operand 转换为 f32 PTX 字符串
fn operand_to_str_f32(op: &GirOperand) -> String {
    match op {
        GirOperand::Reg(id) => format!("%f{}", id),
        GirOperand::Imm(val) => {
            // val 存储的是 f32 的 bit pattern (u32) 扩展为 i64
            // 直接取低 32 位作为 f32 位模式
            format!("0f{:08X}", *val as u32)
        }
        GirOperand::Label(id) => format!("LABEL_{}", id),
        GirOperand::Param(id) => format!("%f{}", id),  // f32 参数在 %f 寄存器中
    }
}

/// GpuBackend trait 实现
impl super::GpuBackend for PtxCompiler {
    type Output = String;

    fn compile(&mut self, gir: &GirProgram) -> Self::Output {
        let mut compiler = PtxCompiler::new();
        compiler.compile(gir)
    }

    fn target_name(&self) -> &str {
        "ptx"
    }
}
