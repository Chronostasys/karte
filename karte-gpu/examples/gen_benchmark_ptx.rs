use karte_gir::*;
use karte_gpu::PtxCompiler;

fn make_vec_add_kernel() -> GirFunction {
    let mut f = GirFunction::new("vec_add".to_string());
    f.params = vec![
        GirParam { name: "a".into(), dtype: GirDType::F32, is_ptr: true },
        GirParam { name: "b".into(), dtype: GirDType::F32, is_ptr: true },
        GirParam { name: "out".into(), dtype: GirDType::F32, is_ptr: true },
        GirParam { name: "n".into(), dtype: GirDType::I32, is_ptr: false },
    ];

    // tid = blockIdx.x * blockDim.x + threadIdx.x
    let bid = f.alloc_reg();
    let bdim = f.alloc_reg();
    let tlid = f.alloc_reg();
    let tid = f.alloc_reg();
    f.emit(GirInstruction::BlockId { dst: bid, dim: ThreadDim::X });
    f.emit(GirInstruction::BlockDim { dst: bdim, dim: ThreadDim::X });
    f.emit(GirInstruction::ThreadId { dst: tlid, dim: ThreadDim::X });
    f.emit(GirInstruction::Mul { dst: bid, src1: GirOperand::Reg(bid), src2: GirOperand::Reg(bdim), dtype: GirDType::I32 });
    f.emit(GirInstruction::Add { dst: tid, src1: GirOperand::Reg(bid), src2: GirOperand::Reg(tlid), dtype: GirDType::I32 });

    // if tid >= n: jump to end
    let cmp_reg = f.alloc_reg();
    let label_body = f.alloc_label();
    let label_end = f.alloc_label();
    f.emit(GirInstruction::Cmp { dst: cmp_reg, op: CmpOp::Lt, src1: GirOperand::Reg(tid), src2: GirOperand::Param(3), dtype: GirDType::I32 });
    f.emit(GirInstruction::BranchIf { cond: GirOperand::Reg(cmp_reg), then_label: label_body, else_label: label_end });

    // body:
    f.emit(GirInstruction::Label { id: label_body });

    // byte_offset = tid * 4 (s64 for pointer arithmetic)
    let byte_offset_64 = f.alloc_reg();
    let tid_64 = f.alloc_reg();
    // 用 I64 乘法直接算 byte offset，避免 s32→s64 转换
    f.emit(GirInstruction::Mul { dst: byte_offset_64, src1: GirOperand::Reg(tid), src2: GirOperand::Imm(4), dtype: GirDType::I64 });

    let addr_a = f.alloc_reg();
    f.emit(GirInstruction::Add { dst: addr_a, src1: GirOperand::Param(0), src2: GirOperand::Reg(byte_offset_64), dtype: GirDType::I64 });
    let addr_b = f.alloc_reg();
    f.emit(GirInstruction::Add { dst: addr_b, src1: GirOperand::Param(1), src2: GirOperand::Reg(byte_offset_64), dtype: GirDType::I64 });

    let val_a = f.alloc_reg();
    f.emit(GirInstruction::GlobalLoad { dst: val_a, addr: GirOperand::Reg(addr_a), dtype: GirDType::F32 });
    let val_b = f.alloc_reg();
    f.emit(GirInstruction::GlobalLoad { dst: val_b, addr: GirOperand::Reg(addr_b), dtype: GirDType::F32 });

    let result = f.alloc_reg();
    f.emit(GirInstruction::Add { dst: result, src1: GirOperand::Reg(val_a), src2: GirOperand::Reg(val_b), dtype: GirDType::F32 });

    let addr_out = f.alloc_reg();
    f.emit(GirInstruction::Add { dst: addr_out, src1: GirOperand::Param(2), src2: GirOperand::Reg(byte_offset_64), dtype: GirDType::I64 });
    f.emit(GirInstruction::GlobalStore { addr: GirOperand::Reg(addr_out), src: GirOperand::Reg(result), dtype: GirDType::F32 });

    f.emit(GirInstruction::Label { id: label_end });
    f.emit(GirInstruction::Return);
    f.block_dim = (256, 1, 1);
    f
}

fn make_dof_reward_kernel() -> GirFunction {
    let mut f = GirFunction::new("dof_reward".to_string());
    f.params = vec![
        GirParam { name: "ref_dof".into(), dtype: GirDType::F32, is_ptr: true },
        GirParam { name: "dof".into(), dtype: GirDType::F32, is_ptr: true },
        GirParam { name: "out".into(), dtype: GirDType::F32, is_ptr: true },
        // sigma 和 ndof 作为 i32 传递（避免 f32 param 加载问题）
        GirParam { name: "ndof".into(), dtype: GirDType::I32, is_ptr: false },
    ];

    // tid = blockIdx.x * blockDim.x + threadIdx.x
    let bid = f.alloc_reg();
    let bdim = f.alloc_reg();
    let tlid = f.alloc_reg();
    let tid = f.alloc_reg();
    f.emit(GirInstruction::BlockId { dst: bid, dim: ThreadDim::X });
    f.emit(GirInstruction::BlockDim { dst: bdim, dim: ThreadDim::X });
    f.emit(GirInstruction::ThreadId { dst: tlid, dim: ThreadDim::X });
    f.emit(GirInstruction::Mul { dst: bid, src1: GirOperand::Reg(bid), src2: GirOperand::Reg(bdim), dtype: GirDType::I32 });
    f.emit(GirInstruction::Add { dst: tid, src1: GirOperand::Reg(bid), src2: GirOperand::Reg(tlid), dtype: GirDType::I32 });

    // base_idx = tid * ndof
    let base_idx = f.alloc_reg();
    f.emit(GirInstruction::Mul { dst: base_idx, src1: GirOperand::Reg(tid), src2: GirOperand::Param(3), dtype: GirDType::I32 });

    // sum_sq = 0.0f (use F32 register)
    let sum_sq = f.alloc_reg();
    // GIR Move 总是输出 mov.s64，我们用 Add 0.0 代替初始化
    // 更好的方式：直接用 FMA 0*0+0 = 0
    let zero = f.alloc_reg();
    f.emit(GirInstruction::Mul { dst: zero, src1: GirOperand::Imm(0), src2: GirOperand::Imm(0), dtype: GirDType::F32 });
    f.emit(GirInstruction::Add { dst: sum_sq, src1: GirOperand::Reg(zero), src2: GirOperand::Imm(0), dtype: GirDType::F32 });

    // for i in 0..ndof
    let i = f.alloc_reg();
    f.emit(GirInstruction::Mul { dst: i, src1: GirOperand::Imm(0), src2: GirOperand::Imm(1), dtype: GirDType::I32 }); // i = 0

    let loop_body = f.alloc_label();
    let loop_exit = f.alloc_label();

    // 检查 i < ndof
    let cmp_i = f.alloc_reg();
    f.emit(GirInstruction::Cmp { dst: cmp_i, op: CmpOp::Lt, src1: GirOperand::Reg(i), src2: GirOperand::Param(3), dtype: GirDType::I32 });
    f.emit(GirInstruction::BranchIf { cond: GirOperand::Reg(cmp_i), then_label: loop_body, else_label: loop_exit });

    // loop_exit: store result and return
    f.emit(GirInstruction::Label { id: loop_exit });

    // byte_out = tid * 4
    let byte_out = f.alloc_reg();
    f.emit(GirInstruction::Mul { dst: byte_out, src1: GirOperand::Reg(tid), src2: GirOperand::Imm(4), dtype: GirDType::I64 });
    let addr_out = f.alloc_reg();
    f.emit(GirInstruction::Add { dst: addr_out, src1: GirOperand::Param(2), src2: GirOperand::Reg(byte_out), dtype: GirDType::I64 });
    f.emit(GirInstruction::GlobalStore { addr: GirOperand::Reg(addr_out), src: GirOperand::Reg(sum_sq), dtype: GirDType::F32 });
    f.emit(GirInstruction::Return);

    // loop_body:
    f.emit(GirInstruction::Label { id: loop_body });

    // idx = base_idx + i
    let idx = f.alloc_reg();
    f.emit(GirInstruction::Add { dst: idx, src1: GirOperand::Reg(base_idx), src2: GirOperand::Reg(i), dtype: GirDType::I32 });

    // byte_offset = idx * 4
    let byte_off = f.alloc_reg();
    f.emit(GirInstruction::Mul { dst: byte_off, src1: GirOperand::Reg(idx), src2: GirOperand::Imm(4), dtype: GirDType::I64 });

    // load ref[idx] and dof[idx]
    let addr_ref = f.alloc_reg();
    f.emit(GirInstruction::Add { dst: addr_ref, src1: GirOperand::Param(0), src2: GirOperand::Reg(byte_off), dtype: GirDType::I64 });
    let addr_dof = f.alloc_reg();
    f.emit(GirInstruction::Add { dst: addr_dof, src1: GirOperand::Param(1), src2: GirOperand::Reg(byte_off), dtype: GirDType::I64 });

    let val_ref = f.alloc_reg();
    f.emit(GirInstruction::GlobalLoad { dst: val_ref, addr: GirOperand::Reg(addr_ref), dtype: GirDType::F32 });
    let val_dof = f.alloc_reg();
    f.emit(GirInstruction::GlobalLoad { dst: val_dof, addr: GirOperand::Reg(addr_dof), dtype: GirDType::F32 });

    // diff = ref - dof
    let diff = f.alloc_reg();
    f.emit(GirInstruction::Sub { dst: diff, src1: GirOperand::Reg(val_ref), src2: GirOperand::Reg(val_dof), dtype: GirDType::F32 });

    // sum_sq += diff * diff  (FMA)
    f.emit(GirInstruction::Fma { dst: sum_sq, src1: GirOperand::Reg(diff), src2: GirOperand::Reg(diff), src3: GirOperand::Reg(sum_sq), dtype: GirDType::F32 });

    // i += 1
    f.emit(GirInstruction::Add { dst: i, src1: GirOperand::Reg(i), src2: GirOperand::Imm(1), dtype: GirDType::I32 });

    // loop back: check i < ndof
    let cmp_i2 = f.alloc_reg();
    f.emit(GirInstruction::Cmp { dst: cmp_i2, op: CmpOp::Lt, src1: GirOperand::Reg(i), src2: GirOperand::Param(3), dtype: GirDType::I32 });
    f.emit(GirInstruction::BranchIf { cond: GirOperand::Reg(cmp_i2), then_label: loop_body, else_label: loop_exit });

    // unreachable (loop always branches)
    f.emit(GirInstruction::Jump { target: loop_exit });

    // nothing after — loop_exit handles return
    f.block_dim = (256, 1, 1);
    f
}

fn main() {
    let mut gir = GirProgram::new();
    gir.add_kernel(make_vec_add_kernel());
    gir.add_kernel(make_dof_reward_kernel());

    // 对 dof_reward kernel 执行优化 pass
    for kernel in &mut gir.kernels {
        // 1. 向量化加载 pass
        karte_gir::VectorizePass::new().optimize(kernel);
        // 2. 循环展开 pass (factor=4)
        karte_gir::LoopUnroller::new(4).unroll(kernel);
        // 3. 软件流水线 pass
        karte_gir::SoftwarePipelinePass::new().optimize(kernel);
    }

    let mut ptx = PtxCompiler::new().target(12, 0);
    let output = ptx.compile(&gir);
    print!("{}", output);
    eprintln!("\nGenerated {} kernels, {} bytes PTX (with vectorize+unroll+pipeline)", gir.kernels.len(), output.len());
}
