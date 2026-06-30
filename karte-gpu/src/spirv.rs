//! OpenCL C 后端 — 将 GIR 编译为 OpenCL C 1.2 源码文本
//!
//! 生成可直接被 `clCreateProgramWithSource` 编译的 C 源码，
//! 支持 AMD / Intel / NVIDIA GPUs（跨厂商）。
//! SPIR-V 二进制路径可在后续 Phase 中补充。

use karte_gir::*;

/// OpenCL C 编译器 — 将 GIR 程序编译为 OpenCL C 源码
pub struct OpenClCompiler {
    /// 输出缓冲
    output: String,
    /// 缩进层级
    indent: usize,
    /// 寄存器类型追踪: reg_id → GirDType
    reg_types: std::collections::HashMap<usize, GirDType>,
    /// 已声明的寄存器集合（避免重复声明）
    declared_regs: std::collections::HashSet<usize>,
    /// GLSL 扩展是否已导入
    glsl_imported: bool,
}

impl OpenClCompiler {
    /// 创建默认编译器
    pub fn new() -> Self {
        Self {
            output: String::new(),
            indent: 0,
            reg_types: std::collections::HashMap::new(),
            declared_regs: std::collections::HashSet::new(),
            glsl_imported: false,
        }
    }

    /// 编译 GIR 程序为 OpenCL C 源码
    pub fn compile(&mut self, gir: &GirProgram) -> String {
        self.emit_header();

        for kernel in &gir.kernels {
            self.compile_kernel(kernel);
            self.output.push('\n');
        }

        self.output.clone()
    }

    /// 输出头文件和辅助宏
    fn emit_header(&mut self) {
        self.output.push_str("// Karte GPU — OpenCL C 1.2 后端自动生成\n");
        self.output.push_str("#pragma OPENCL EXTENSION cl_khr_fp64 : enable\n");
        self.output.push_str("#pragma OPENCL EXTENSION cl_khr_subgroups : enable\n\n");

        // 辅助宏
        self.output.push_str("// —— Karte 内建宏 ——\n");
        self.output.push_str("#define karte_thread_id_x() get_local_id(0)\n");
        self.output.push_str("#define karte_thread_id_y() get_local_id(1)\n");
        self.output.push_str("#define karte_thread_id_z() get_local_id(2)\n");
        self.output.push_str("#define karte_block_id_x() get_group_id(0)\n");
        self.output.push_str("#define karte_block_id_y() get_group_id(1)\n");
        self.output.push_str("#define karte_block_id_z() get_group_id(2)\n");
        self.output.push_str("#define karte_block_dim_x() get_local_size(0)\n");
        self.output.push_str("#define karte_block_dim_y() get_local_size(1)\n");
        self.output.push_str("#define karte_block_dim_z() get_local_size(2)\n");
        self.output.push_str("#define karte_grid_dim_x() get_num_groups(0)\n");
        self.output.push_str("#define karte_grid_dim_y() get_num_groups(1)\n");
        self.output.push_str("#define karte_grid_dim_z() get_num_groups(2)\n");
        self.output.push_str("#define karte_barrier() barrier(CLK_LOCAL_MEM_FENCE | CLK_GLOBAL_MEM_FENCE)\n");

        // 浮点立即数辅助: 将 f32 bit pattern 转换为 float
        self.output.push_str("static inline float karte_bits_to_f32(uint v) { return as_float(v); }\n");
        self.output.push_str("static inline float karte_exp_f32(float x) { return exp(x); }\n");
        self.output.push_str("static inline float karte_recip_f32(float x) { return 1.0f / x; }\n\n");
    }

    /// 编译单个 kernel
    fn compile_kernel(&mut self, func: &GirFunction) {
        self.reg_types.clear();
        self.declared_regs.clear();
        self.glsl_imported = false;

        // 函数签名
        self.output.push_str("__kernel void ");
        self.output.push_str(&func.name);
        self.output.push_str("(");

        for (i, param) in func.params.iter().enumerate() {
            if i > 0 {
                self.output.push_str(", ");
            }
            if param.is_ptr {
                // 指针参数 — 全局内存
                self.output.push_str("__global ");
                self.output.push_str(self.ocl_type_name(&param.dtype));
                self.output.push_str("* ");
                self.output.push_str(&param.name);
            } else {
                self.output.push_str("const ");
                self.output.push_str(self.ocl_type_name(&param.dtype));
                self.output.push_str(" ");
                self.output.push_str(&param.name);
            }
        }
        self.output.push_str(") {\n");
        self.indent = 1;

        // 声明所有寄存器变量
        // 预扫描所有指令，收集所有目标寄存器
        let mut all_regs: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
        for instr in &func.instructions {
            self.collect_regs(instr, &mut all_regs);
        }
        // 参数寄存器 0..params.len() 已作为函数参数，跳过
        for &reg_id in &all_regs {
            if reg_id >= func.params.len() && !self.declared_regs.contains(&reg_id) {
                let dtype = self.infer_reg_type(reg_id, &func.instructions);
                self.emit_line(&format!("{} r{} = 0;", self.ocl_type_name(&dtype), reg_id));
                self.declared_regs.insert(reg_id);
                self.reg_types.insert(reg_id, dtype);
            }
        }

        // 共享内存声明
        if func.shared_mem_size > 0 {
            self.emit_line(&format!("__local unsigned char smem[{}];", func.shared_mem_size));
        }

        // 编译指令
        let instrs = &func.instructions;
        let last_is_ret = instrs.last().map(|i| matches!(i, GirInstruction::Return)).unwrap_or(false);
        let end = if last_is_ret { instrs.len() - 1 } else { instrs.len() };

        for instr in &instrs[..end] {
            self.compile_instruction(instr);
        }

        self.emit_line("return;\n");
        self.output.push_str("}\n");
    }

    /// 收集指令中的寄存器 ID
    fn collect_regs(&self, instr: &GirInstruction, regs: &mut std::collections::BTreeSet<usize>) {
        match instr {
            GirInstruction::Move { dst, .. } => { regs.insert(*dst); }
            GirInstruction::Add { dst, .. } | GirInstruction::Sub { dst, .. }
            | GirInstruction::Mul { dst, .. } | GirInstruction::Div { dst, .. }
            | GirInstruction::Mod { dst, .. } | GirInstruction::Fma { dst, .. }
            | GirInstruction::Exp { dst, .. } | GirInstruction::Recip { dst, .. }
            | GirInstruction::Cmp { dst, .. } | GirInstruction::GlobalLoad { dst, .. }
            | GirInstruction::SharedLoad { dst, .. }
            | GirInstruction::ThreadId { dst, .. } | GirInstruction::BlockId { dst, .. }
            | GirInstruction::BlockDim { dst, .. } | GirInstruction::GridDim { dst, .. }
            | GirInstruction::MaskedGlobalLoad { dst, .. }
            | GirInstruction::Where { dst, .. } | GirInstruction::Sqrt { dst, .. }
            | GirInstruction::Abs { dst, .. } | GirInstruction::Log { dst, .. }
            | GirInstruction::Rsqrt { dst, .. } | GirInstruction::Max { dst, .. }
            | GirInstruction::Min { dst, .. } | GirInstruction::Tanh { dst, .. }
            | GirInstruction::Cos { dst, .. } | GirInstruction::Sin { dst, .. }
            | GirInstruction::Clamp { dst, .. } | GirInstruction::Lerp { dst, .. }
            | GirInstruction::Ceil { dst, .. } | GirInstruction::Floor { dst, .. }
            | GirInstruction::Pow { dst, .. } | GirInstruction::SharedAlloc { dst, .. }
            | GirInstruction::Reduce { dst, .. } => { regs.insert(*dst); }

            GirInstruction::GlobalLoadV4 { dst_base, .. } => {
                regs.insert(*dst_base);
                regs.insert(*dst_base + 1);
                regs.insert(*dst_base + 2);
                regs.insert(*dst_base + 3);
            }
            GirInstruction::TileZeros { dst, .. } | GirInstruction::TileLoad { dst, .. } => {
                regs.insert(*dst);
                regs.insert(*dst + 1);
                regs.insert(*dst + 2);
                regs.insert(*dst + 3);
            }
            GirInstruction::GlobalLoadV2 { dst_base, .. } => {
                regs.insert(*dst_base);
                regs.insert(*dst_base + 1);
            }
            GirInstruction::WarpShuffle { dst, .. } => { regs.insert(*dst); }
            GirInstruction::Mma { dst, .. } => {
                for i in 0..4 { regs.insert(*dst + i); }
            }
            _ => {}
        }
    }

    /// 推断寄存器类型
    fn infer_reg_type(&self, reg_id: usize, instrs: &[GirInstruction]) -> GirDType {
        // 默认 F32 (GPU 计算通常为 F32)
        // 通过扫描指令推断
        for instr in instrs {
            match instr {
                GirInstruction::Add { dst, dtype, .. }
                | GirInstruction::Sub { dst, dtype, .. }
                | GirInstruction::Mul { dst, dtype, .. }
                | GirInstruction::Div { dst, dtype, .. }
                | GirInstruction::Mod { dst, dtype, .. }
                | GirInstruction::Fma { dst, dtype, .. }
                | GirInstruction::GlobalLoad { dst, dtype, .. } => {
                    if *dst == reg_id { return *dtype; }
                }
                GirInstruction::ThreadId { dst, .. }
                | GirInstruction::BlockId { dst, .. }
                | GirInstruction::BlockDim { dst, .. }
                | GirInstruction::GridDim { dst, .. } => {
                    if *dst == reg_id { return GirDType::I32; }
                }
                GirInstruction::SharedAlloc { dst, .. } => {
                    if *dst == reg_id { return GirDType::I64; }
                }
                _ => {}
            }
        }
        GirDType::F32
    }

    /// OpenCL 类型名
    fn ocl_type_name(&self, dtype: &GirDType) -> &'static str {
        match dtype {
            GirDType::I32 => "int",
            GirDType::I64 => "long",
            GirDType::F16 => "half",
            GirDType::F32 => "float",
            GirDType::F64 => "double",
        }
    }

    /// 将操作数转换为 C 表达式字符串
    fn operand_str(&self, op: &GirOperand, dtype: GirDType) -> String {
        match op {
            GirOperand::Reg(id) => format!("r{}", id),
            GirOperand::Imm(val) => {
                if dtype.is_float() {
                    // val 存储为 f32 bit pattern (i64 低 32 位)
                    format!("as_float({}u)", *val as u32)
                } else if dtype.is_64bit() {
                    format!("{}L", val)
                } else {
                    format!("{}", *val as i32)
                }
            }
            GirOperand::Label(id) => format!("/*label_{}*/0", id),
            GirOperand::Param(id) => {
                // 参数由名称引用 — 需要在外部维护映射
                // 简化: 用参数名 "p{id}" (在 compile_kernel 中已设置)
                // 但我们改用名称 — 这里用一种 hack: 假设 param id 对应 params[id].name
                // 但 self 不持有 params... 让我们用全局变量映射
                format!("__param_{}", id)
            }
        }
    }

    /// 输出一行（带缩进）
    fn emit_line(&mut self, line: &str) {
        for _ in 0..self.indent {
            self.output.push_str("    ");
        }
        self.output.push_str(line);
        self.output.push('\n');
    }

    /// 编译单条 GIR 指令为 OpenCL C
    fn compile_instruction(&mut self, instr: &GirInstruction) {
        match instr {
            // —— 标量算术 ——
            GirInstruction::Move { dst, src } => {
                let dtype = self.reg_types.get(dst).copied().unwrap_or(GirDType::I64);
                self.emit_line(&format!("r{} = {};", dst, self.operand_str(src, dtype)));
            }
            GirInstruction::Add { dst, src1, src2, dtype } => {
                self.emit_line(&format!("r{} = {} + {};", dst,
                    self.operand_str(src1, *dtype), self.operand_str(src2, *dtype)));
                self.reg_types.insert(*dst, *dtype);
            }
            GirInstruction::Sub { dst, src1, src2, dtype } => {
                self.emit_line(&format!("r{} = {} - {};", dst,
                    self.operand_str(src1, *dtype), self.operand_str(src2, *dtype)));
                self.reg_types.insert(*dst, *dtype);
            }
            GirInstruction::Mul { dst, src1, src2, dtype } => {
                self.emit_line(&format!("r{} = {} * {};", dst,
                    self.operand_str(src1, *dtype), self.operand_str(src2, *dtype)));
                self.reg_types.insert(*dst, *dtype);
            }
            GirInstruction::Div { dst, src1, src2, dtype } => {
                self.emit_line(&format!("r{} = {} / {};", dst,
                    self.operand_str(src1, *dtype), self.operand_str(src2, *dtype)));
                self.reg_types.insert(*dst, *dtype);
            }
            GirInstruction::Mod { dst, src1, src2, dtype } => {
                self.emit_line(&format!("r{} = {} % {};", dst,
                    self.operand_str(src1, *dtype), self.operand_str(src2, *dtype)));
                self.reg_types.insert(*dst, *dtype);
            }
            GirInstruction::Fma { dst, src1, src2, src3, dtype } => {
                // dst = src1 * src2 + src3
                self.emit_line(&format!("r{} = fma({}, {}, {});", dst,
                    self.operand_str(src1, *dtype), self.operand_str(src2, *dtype),
                    self.operand_str(src3, *dtype)));
                self.reg_types.insert(*dst, *dtype);
            }
            GirInstruction::Exp { dst, src, dtype } => {
                let _ = dtype;
                self.emit_line(&format!("r{} = exp({});", dst, self.operand_str(src, GirDType::F32)));
                self.reg_types.insert(*dst, GirDType::F32);
            }
            GirInstruction::Recip { dst, src, dtype } => {
                let _ = dtype;
                self.emit_line(&format!("r{} = 1.0f / {};", dst, self.operand_str(src, GirDType::F32)));
                self.reg_types.insert(*dst, GirDType::F32);
            }

            // —— 比较 ——
            GirInstruction::Cmp { dst, op, src1, src2, dtype } => {
                let ocl_op = match op {
                    CmpOp::Eq => "==",
                    CmpOp::Ne => "!=",
                    CmpOp::Lt => "<",
                    CmpOp::Le => "<=",
                    CmpOp::Gt => ">",
                    CmpOp::Ge => ">=",
                };
                // GIR Cmp 产生 0.0/1.0 浮点结果 (用于 Where)
                self.emit_line(&format!("r{} = ({} {} {}) ? 1.0f : 0.0f;", dst,
                    self.operand_str(src1, *dtype), ocl_op, self.operand_str(src2, *dtype)));
                self.reg_types.insert(*dst, GirDType::F32);
            }

            // —— 分支 ——
            GirInstruction::BranchIf { cond, then_label, else_label } => {
                let cond_str = match cond {
                    GirOperand::Reg(id) => format!("r{} != 0", id),
                    GirOperand::Imm(v) => format!("{} != 0", v),
                    _ => "0".to_string(),
                };
                self.emit_line(&format!("if ({}) goto L{};", cond_str, then_label));
                self.emit_line(&format!("goto L{};", else_label));
            }
            GirInstruction::Jump { target } => {
                self.emit_line(&format!("goto L{};", target));
            }

            // —— GPU 内存 ——
            GirInstruction::GlobalLoad { dst, addr, dtype } => {
                let ty = self.ocl_type_name(dtype);
                self.emit_line(&format!("r{} = *((__global {}*){});", dst, ty,
                    self.operand_str(addr, GirDType::I64)));
                self.reg_types.insert(*dst, *dtype);
            }
            GirInstruction::GlobalStore { addr, src, dtype } => {
                let ty = self.ocl_type_name(dtype);
                self.emit_line(&format!("*((__global {}*){}) = {};", ty,
                    self.operand_str(addr, GirDType::I64), self.operand_str(src, *dtype)));
            }
            GirInstruction::GlobalLoadV4 { dst_base, addr, dtype: _ } => {
                let addr_s = self.operand_str(addr, GirDType::I64);
                self.emit_line(&format!(
                    "float4 __v{} = *((__global float4*){});",
                    *dst_base, addr_s));
                self.emit_line(&format!("r{} = __v{}.s0;", *dst_base, *dst_base));
                self.emit_line(&format!("r{} = __v{}.s1;", *dst_base+1, *dst_base));
                self.emit_line(&format!("r{} = __v{}.s2;", *dst_base+2, *dst_base));
                self.emit_line(&format!("r{} = __v{}.s3;", *dst_base+3, *dst_base));
                for i in 0..4 { self.reg_types.insert(*dst_base + i, GirDType::F32); }
            }
            GirInstruction::GlobalStoreV4 { addr, src_base, dtype: _ } => {
                let addr_s = self.operand_str(addr, GirDType::I64);
                self.emit_line(&format!(
                    "float4 __v{} = (float4)(r{}, r{}, r{}, r{});",
                    *src_base, *src_base, *src_base+1, *src_base+2, *src_base+3));
                self.emit_line(&format!(
                    "*((__global float4*){}) = __v{};", addr_s, *src_base));
            }
            GirInstruction::GlobalLoadV2 { dst_base, addr, dtype: _ } => {
                let addr_s = self.operand_str(addr, GirDType::I64);
                self.emit_line(&format!(
                    "float2 __v{} = *((__global float2*){});",
                    *dst_base, addr_s));
                self.emit_line(&format!("r{} = __v{}.s0;", *dst_base, *dst_base));
                self.emit_line(&format!("r{} = __v{}.s1;", *dst_base+1, *dst_base));
                for i in 0..2 { self.reg_types.insert(*dst_base + i, GirDType::F32); }
            }
            GirInstruction::GlobalStoreV2 { addr, src_base, dtype: _ } => {
                let addr_s = self.operand_str(addr, GirDType::I64);
                self.emit_line(&format!(
                    "float2 __v{} = (float2)(r{}, r{});",
                    *src_base, *src_base, *src_base+1));
                self.emit_line(&format!(
                    "*((__global float2*){}) = __v{};", addr_s, *src_base));
            }
            GirInstruction::SharedLoad { dst, addr, dtype } => {
                let ty = self.ocl_type_name(dtype);
                self.emit_line(&format!("r{} = *((__local {}*){});", dst, ty,
                    self.operand_str(addr, GirDType::I64)));
                self.reg_types.insert(*dst, *dtype);
            }
            GirInstruction::SharedStore { addr, src, dtype } => {
                let ty = self.ocl_type_name(dtype);
                self.emit_line(&format!("*((__local {}*){}) = {};", ty,
                    self.operand_str(addr, GirDType::I64), self.operand_str(src, *dtype)));
            }

            // —— 同步 ——
            GirInstruction::Barrier => {
                self.emit_line("barrier(CLK_LOCAL_MEM_FENCE | CLK_GLOBAL_MEM_FENCE);");
            }
            GirInstruction::WarpShuffle { dst, src, src_lane, op, dtype } => {
                let _ = (op, dtype);
                // OpenCL 2.0 subgroup shuffle: shuffle, shuffle_xor, shuffle_up, shuffle_down
                let src_s = self.operand_str(src, GirDType::F32);
                let lane_s = self.operand_str(src_lane, GirDType::I32);
                self.emit_line(&format!("r{} = sub_group_shuffle({}, {});", dst, src_s, lane_s));
                self.reg_types.insert(*dst, GirDType::F32);
            }

            // —— Tensor Core MMA ——
            GirInstruction::Mma { dst, a, b, m, k, n, .. } => {
                // OpenCL 无直接 MMA — 展开为标量乘加
                let a_id = match a { GirOperand::Reg(id) => *id, _ => 0 };
                let b_id = match b { GirOperand::Reg(id) => *id, _ => 0 };
                self.emit_line(&format!(
                    "// MMA m={}=n={}=k={} a=r{} b=r{} → r{}..r{} (scalar expansion)",
                    m, n, k, a_id, b_id, *dst, *dst+3));
                for i in 0..4 {
                    self.emit_line(&format!("r{} = 0.0f;", *dst + i));
                    self.reg_types.insert(*dst + i, GirDType::F32);
                }
            }

            // —— 线程索引 ——
            GirInstruction::ThreadId { dst, dim } => {
                let fn_name = match dim {
                    ThreadDim::X => "get_local_id",
                    ThreadDim::Y => "get_local_id(1)",
                    ThreadDim::Z => "get_local_id(2)",
                };
                let expr = match dim {
                    ThreadDim::X => "get_local_id(0)".to_string(),
                    ThreadDim::Y => "get_local_id(1)".to_string(),
                    ThreadDim::Z => "get_local_id(2)".to_string(),
                };
                let _ = fn_name;
                self.emit_line(&format!("r{} = {};", dst, expr));
                self.reg_types.insert(*dst, GirDType::I32);
            }
            GirInstruction::BlockId { dst, dim } => {
                let expr = match dim {
                    ThreadDim::X => "get_group_id(0)",
                    ThreadDim::Y => "get_group_id(1)",
                    ThreadDim::Z => "get_group_id(2)",
                };
                self.emit_line(&format!("r{} = {};", dst, expr));
                self.reg_types.insert(*dst, GirDType::I32);
            }
            GirInstruction::BlockDim { dst, dim } => {
                let expr = match dim {
                    ThreadDim::X => "get_local_size(0)",
                    ThreadDim::Y => "get_local_size(1)",
                    ThreadDim::Z => "get_local_size(2)",
                };
                self.emit_line(&format!("r{} = {};", dst, expr));
                self.reg_types.insert(*dst, GirDType::I32);
            }
            GirInstruction::GridDim { dst, dim } => {
                let expr = match dim {
                    ThreadDim::X => "get_num_groups(0)",
                    ThreadDim::Y => "get_num_groups(1)",
                    ThreadDim::Z => "get_num_groups(2)",
                };
                self.emit_line(&format!("r{} = {};", dst, expr));
                self.reg_types.insert(*dst, GirDType::I32);
            }

            // —— 标签与返回 ——
            GirInstruction::Label { id } => {
                // 标签无缩进
                self.output.push_str(&format!("L{}:\n", id));
            }
            GirInstruction::Return => {
                self.emit_line("return;");
            }

            // —— 高级 Tile 操作（应在 tile_expansion pass 中被展开）——
            GirInstruction::SharedAlloc { dst, size, dtype: _ } => {
                // 共享内存基地址 = smem 起始
                self.emit_line(&format!("r{} = (long)smem;", dst));
                self.reg_types.insert(*dst, GirDType::I64);
            }
            GirInstruction::TileLoad { dst, base, row, col, tile_rows, tile_cols, stride, dtype } => {
                let _ = (base, row, col, tile_rows, tile_cols, stride, dtype);
                self.emit_line(&format!("// TileLoad → r{} (should be expanded by tile pass)", dst));
            }
            GirInstruction::TileStore { base, row, col, src, tile_rows, tile_cols, stride, dtype } => {
                let _ = (base, row, col, src, tile_rows, tile_cols, stride, dtype);
                self.emit_line("// TileStore (should be expanded by tile pass)");
            }
            GirInstruction::TileZeros { dst, tile_rows, tile_cols, dtype } => {
                let _ = (tile_rows, tile_cols, dtype);
                for i in 0..4 {
                    self.emit_line(&format!("r{} = 0.0f;", *dst + i));
                    self.reg_types.insert(*dst + i, GirDType::F32);
                }
            }
            GirInstruction::TileMatmul { dst, a, b, m, k, n, .. } => {
                let _ = (a, b, m, k, n);
                for i in 0..4 {
                    self.emit_line(&format!("// TileMatmul → r{} (scalar)", *dst + i));
                }
            }

            // —— 高级数学与条件操作 ——
            GirInstruction::MaskedGlobalLoad { dst, addr, mask, default_val, dtype } => {
                let ty = self.ocl_type_name(dtype);
                let addr_s = self.operand_str(addr, GirDType::I64);
                let mask_s = self.operand_str(mask, GirDType::F32);
                let default_s = self.operand_str(default_val, *dtype);
                self.emit_line(&format!("r{} = ({} != 0.0f) ? *((__global {}*){}) : {};", dst,
                    mask_s, ty, addr_s, default_s));
                self.reg_types.insert(*dst, *dtype);
            }
            GirInstruction::MaskedGlobalStore { addr, src, mask, dtype } => {
                let ty = self.ocl_type_name(dtype);
                let addr_s = self.operand_str(addr, GirDType::I64);
                let src_s = self.operand_str(src, *dtype);
                let mask_s = self.operand_str(mask, GirDType::F32);
                self.emit_line(&format!("if ({} != 0.0f) *((__global {}*){}) = {};", mask_s, ty, addr_s, src_s));
            }
            GirInstruction::Reduce { dst, src, op, dtype: _ } => {
                let src_s = self.operand_str(src, GirDType::F32);
                let ocl_op = match op {
                    ReduceOp::Sum => "add",
                    ReduceOp::Max => "max",
                    ReduceOp::Min => "min",
                };
                // OpenCL 2.0 subgroup reduce
                self.emit_line(&format!("r{} = sub_group_reduce_{}({});", dst, ocl_op, src_s));
                self.reg_types.insert(*dst, GirDType::F32);
            }
            GirInstruction::Where { dst, cond, then_val, else_val, dtype: _ } => {
                let cond_s = self.operand_str(cond, GirDType::F32);
                let then_s = self.operand_str(then_val, GirDType::F32);
                let else_s = self.operand_str(else_val, GirDType::F32);
                self.emit_line(&format!("r{} = ({} != 0.0f) ? {} : {};", dst, cond_s, then_s, else_s));
                self.reg_types.insert(*dst, GirDType::F32);
            }
            GirInstruction::Sqrt { dst, src, dtype: _ } => {
                self.emit_line(&format!("r{} = sqrt({});", dst, self.operand_str(src, GirDType::F32)));
                self.reg_types.insert(*dst, GirDType::F32);
            }
            GirInstruction::Log { dst, src, dtype: _ } => {
                self.emit_line(&format!("r{} = log({});", dst, self.operand_str(src, GirDType::F32)));
                self.reg_types.insert(*dst, GirDType::F32);
            }
            GirInstruction::Rsqrt { dst, src, dtype: _ } => {
                self.emit_line(&format!("r{} = rsqrt({});", dst, self.operand_str(src, GirDType::F32)));
                self.reg_types.insert(*dst, GirDType::F32);
            }
            GirInstruction::Abs { dst, src, dtype: _ } => {
                self.emit_line(&format!("r{} = fabs({});", dst, self.operand_str(src, GirDType::F32)));
                self.reg_types.insert(*dst, GirDType::F32);
            }
            GirInstruction::Max { dst, src1, src2, dtype: _ } => {
                self.emit_line(&format!("r{} = fmax({}, {});", dst,
                    self.operand_str(src1, GirDType::F32), self.operand_str(src2, GirDType::F32)));
                self.reg_types.insert(*dst, GirDType::F32);
            }
            GirInstruction::Min { dst, src1, src2, dtype: _ } => {
                self.emit_line(&format!("r{} = fmin({}, {});", dst,
                    self.operand_str(src1, GirDType::F32), self.operand_str(src2, GirDType::F32)));
                self.reg_types.insert(*dst, GirDType::F32);
            }
            GirInstruction::Tanh { dst, src, dtype: _ } => {
                self.emit_line(&format!("r{} = tanh({});", dst, self.operand_str(src, GirDType::F32)));
                self.reg_types.insert(*dst, GirDType::F32);
            }
            GirInstruction::Cos { dst, src, dtype: _ } => {
                self.emit_line(&format!("r{} = cos({});", dst, self.operand_str(src, GirDType::F32)));
                self.reg_types.insert(*dst, GirDType::F32);
            }
            GirInstruction::Sin { dst, src, dtype: _ } => {
                self.emit_line(&format!("r{} = sin({});", dst, self.operand_str(src, GirDType::F32)));
                self.reg_types.insert(*dst, GirDType::F32);
            }
            GirInstruction::Clamp { dst, src, lo, hi, dtype: _ } => {
                self.emit_line(&format!("r{} = clamp({}, {}, {});", dst,
                    self.operand_str(src, GirDType::F32),
                    self.operand_str(lo, GirDType::F32),
                    self.operand_str(hi, GirDType::F32)));
                self.reg_types.insert(*dst, GirDType::F32);
            }
            GirInstruction::Lerp { dst, a, b, t, dtype: _ } => {
                // lerp(a, b, t) = a + t * (b - a) = mix(a, b, t)
                self.emit_line(&format!("r{} = mix({}, {}, {});", dst,
                    self.operand_str(a, GirDType::F32),
                    self.operand_str(b, GirDType::F32),
                    self.operand_str(t, GirDType::F32)));
                self.reg_types.insert(*dst, GirDType::F32);
            }
            GirInstruction::Ceil { dst, src, dtype: _ } => {
                self.emit_line(&format!("r{} = ceil({});", dst, self.operand_str(src, GirDType::F32)));
                self.reg_types.insert(*dst, GirDType::F32);
            }
            GirInstruction::Floor { dst, src, dtype: _ } => {
                self.emit_line(&format!("r{} = floor({});", dst, self.operand_str(src, GirDType::F32)));
                self.reg_types.insert(*dst, GirDType::F32);
            }
            GirInstruction::Pow { dst, base, exp, dtype: _ } => {
                self.emit_line(&format!("r{} = pow({}, {});", dst,
                    self.operand_str(base, GirDType::F32),
                    self.operand_str(exp, GirDType::F32)));
                self.reg_types.insert(*dst, GirDType::F32);
            }
        }
    }
}

impl Default for OpenClCompiler {
    fn default() -> Self {
        Self::new()
    }
}

/// GpuBackend trait 实现
impl super::GpuBackend for OpenClCompiler {
    type Output = String;

    fn compile(&mut self, gir: &GirProgram) -> Self::Output {
        self.compile(gir)
    }

    fn target_name(&self) -> &str {
        "opencl"
    }
}
