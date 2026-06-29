//! body_rot_reward kernel — 纯 GIR 构造，Karte 自动优化，PtxCompiler 自动生成 PTX
//!
//! 计算等价于:
//!   for j in 0..14:
//!     body_quat = vload4(body_ptr + tid*224 + j*16)
//!     ref_quat  = vload4(ref_ptr + tid*224 + j*16)
//!     dw = dot(body_quat, ref_quat)   // 四元数 chordal distance 的 w 分量
//!     error += 8*(1-dw)
//!   reward = exp(-sigma * error / 14)

use karte_gir::*;
use karte_gpu::PtxCompiler;

const N_BODIES: usize = 14;
const N_ENVS: usize = 4096;
const BLOCK_SIZE: usize = 256;

fn make_body_rot_reward() -> GirFunction {
    let mut f = GirFunction::new("body_rot_reward".to_string());
    f.params = vec![
        GirParam { name: "body_ptr".into(), dtype: GirDType::F32, is_ptr: true },
        GirParam { name: "ref_ptr".into(), dtype: GirDType::F32, is_ptr: true },
        GirParam { name: "out_ptr".into(), dtype: GirDType::F32, is_ptr: true },
        GirParam { name: "sigma".into(), dtype: GirDType::F32, is_ptr: false },
    ];
    f.block_dim = (BLOCK_SIZE, 1, 1);

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

    // base = tid * (N_BODIES * 4 * sizeof(f32)) = tid * 224
    let env_base = f.alloc_reg();
    f.emit(GirInstruction::Mul { dst: env_base, src1: GirOperand::Reg(tid), src2: GirOperand::Imm((N_BODIES * 16) as i64), dtype: GirDType::I64 });

    // 将 body_ptr/ref_ptr/out_ptr 加上 env_base 得到各 env 的起始地址
    let body_base = f.alloc_reg();
    f.emit(GirInstruction::Add { dst: body_base, src1: GirOperand::Param(0), src2: GirOperand::Reg(env_base), dtype: GirDType::I64 });
    let ref_base = f.alloc_reg();
    f.emit(GirInstruction::Add { dst: ref_base, src1: GirOperand::Param(1), src2: GirOperand::Reg(env_base), dtype: GirDType::I64 });

    // total_dot = 0.0f (累加 dw 值，最后统一计算 error = 8*(14 - total_dot))
    let total_dot = f.alloc_reg();
    f.emit(GirInstruction::Mul { dst: total_dot, src1: GirOperand::Imm(0), src2: GirOperand::Imm(0), dtype: GirDType::F32 });

    // —— 编译期展开 14 个 body（等价于 Triton 的 tl.static_range(14)）——
    for j in 0..N_BODIES {
        let byte_off = (j * 16) as i64; // j * 4 * sizeof(f32)

        // 地址 = base + byte_off
        let body_addr = f.alloc_reg();
        f.emit(GirInstruction::Add { dst: body_addr, src1: GirOperand::Reg(body_base), src2: GirOperand::Imm(byte_off), dtype: GirDType::I64 });
        let ref_addr = f.alloc_reg();
        f.emit(GirInstruction::Add { dst: ref_addr, src1: GirOperand::Reg(ref_base), src2: GirOperand::Imm(byte_off), dtype: GirDType::I64 });

        // 向量化加载四元数：ld.global.v4.f32 {%b0,%b1,%b2,%b3}, [addr]
        // 必须用 alloc_regs(4) 分配 4 个连续寄存器，避免与后续分配重叠
        let body_quat_base = f.alloc_regs(4);
        f.emit(GirInstruction::GlobalLoadV4 { dst_base: body_quat_base, addr: GirOperand::Reg(body_addr), dtype: GirDType::F32 });

        let ref_quat_base = f.alloc_regs(4);
        f.emit(GirInstruction::GlobalLoadV4 { dst_base: ref_quat_base, addr: GirOperand::Reg(ref_addr), dtype: GirDType::F32 });

        // dw = bx*rx + by*ry + bz*rz + bw*rw
        // body_quat = {body_quat_base, +1, +2, +3}
        // ref_quat  = {ref_quat_base, +1, +2, +3}
        let p0 = f.alloc_reg();
        f.emit(GirInstruction::Mul { dst: p0, src1: GirOperand::Reg(body_quat_base), src2: GirOperand::Reg(ref_quat_base), dtype: GirDType::F32 });

        let p1 = f.alloc_reg();
        f.emit(GirInstruction::Mul { dst: p1, src1: GirOperand::Reg(body_quat_base + 1), src2: GirOperand::Reg(ref_quat_base + 1), dtype: GirDType::F32 });

        let p2 = f.alloc_reg();
        f.emit(GirInstruction::Mul { dst: p2, src1: GirOperand::Reg(body_quat_base + 2), src2: GirOperand::Reg(ref_quat_base + 2), dtype: GirDType::F32 });

        let p3 = f.alloc_reg();
        f.emit(GirInstruction::Mul { dst: p3, src1: GirOperand::Reg(body_quat_base + 3), src2: GirOperand::Reg(ref_quat_base + 3), dtype: GirDType::F32 });

        // dw = p0 + p1 + p2 + p3
        let dw = f.alloc_reg();
        f.emit(GirInstruction::Add { dst: dw, src1: GirOperand::Reg(p0), src2: GirOperand::Reg(p1), dtype: GirDType::F32 });
        f.emit(GirInstruction::Add { dst: dw, src1: GirOperand::Reg(dw), src2: GirOperand::Reg(p2), dtype: GirDType::F32 });
        f.emit(GirInstruction::Add { dst: dw, src1: GirOperand::Reg(dw), src2: GirOperand::Reg(p3), dtype: GirDType::F32 });

        // total_dot += dw
        f.emit(GirInstruction::Add { dst: total_dot, src1: GirOperand::Reg(total_dot), src2: GirOperand::Reg(dw), dtype: GirDType::F32 });
    }

    // error = 8 * (N_BODIES - total_dot) / N_BODIES
    //       = (8*N_BODIES - 8*total_dot) / N_BODIES
    // 先算 8*total_dot
    let eight_dot = f.alloc_reg();
    f.emit(GirInstruction::Mul { dst: eight_dot, src1: GirOperand::Reg(total_dot), src2: GirOperand::Imm(f32_to_bits(8.0)), dtype: GirDType::F32 });
    // 8*N_BODIES - 8*total_dot
    let error = f.alloc_reg();
    f.emit(GirInstruction::Sub { dst: error, src1: GirOperand::Imm(f32_to_bits((8 * N_BODIES) as f32)), src2: GirOperand::Reg(eight_dot), dtype: GirDType::F32 });

    // scaled = -sigma * error / N_BODIES
    // 融合为一次乘法: -sigma/N_BODIES * error
    // -0.25 / 14 = -0.017857... → 但 sigma 是参数，不能硬编码
    // 用: tmp = sigma * error; tmp = tmp * (1/N_BODIES); tmp = -tmp
    let tmp = f.alloc_reg();
    f.emit(GirInstruction::Mul { dst: tmp, src1: GirOperand::Param(3), src2: GirOperand::Reg(error), dtype: GirDType::F32 });
    // 除以 N_BODIES 用乘以倒数
    let recip_n = f.alloc_reg();
    f.emit(GirInstruction::Recip { dst: recip_n, src: GirOperand::Imm(f32_to_bits(N_BODIES as f32)), dtype: GirDType::F32 });
    f.emit(GirInstruction::Mul { dst: tmp, src1: GirOperand::Reg(tmp), src2: GirOperand::Reg(recip_n), dtype: GirDType::F32 });

    // reward = exp(-tmp) — Exp 内部展开为 ex2.approx
    // 注意: exp(-x) = 2^(-x * log2(e))，我们在 Exp 指令中乘以 log2(e)
    // 所以传 -tmp 给 Exp
    // 但 GIR Exp 展开是: dst = 2^(src * log2(e))
    // 我们要: reward = 2^(-tmp * log2(e))
    // 所以传 -tmp（但 GIR 没有单独的 Neg，用 0 - tmp）
    let neg_tmp = f.alloc_reg();
    f.emit(GirInstruction::Sub { dst: neg_tmp, src1: GirOperand::Imm(0), src2: GirOperand::Reg(tmp), dtype: GirDType::F32 });
    let reward = f.alloc_reg();
    f.emit(GirInstruction::Exp { dst: reward, src: GirOperand::Reg(neg_tmp), dtype: GirDType::F32 });

    // out[tid] = reward
    let out_byte = f.alloc_reg();
    f.emit(GirInstruction::Mul { dst: out_byte, src1: GirOperand::Reg(tid), src2: GirOperand::Imm(4), dtype: GirDType::I64 });
    let out_addr = f.alloc_reg();
    f.emit(GirInstruction::Add { dst: out_addr, src1: GirOperand::Param(2), src2: GirOperand::Reg(out_byte), dtype: GirDType::I64 });
    f.emit(GirInstruction::GlobalStore { addr: GirOperand::Reg(out_addr), src: GirOperand::Reg(reward), dtype: GirDType::F32 });

    f.emit(GirInstruction::Return);
    f
}

/// f32 → IEEE 754 bits → i64
fn f32_to_bits(f: f32) -> i64 {
    f.to_bits() as i64
}

fn main() {
    let mut gir = GirProgram::new();
    let mut kernel = make_body_rot_reward();

    // Karte 优化 pass 1: 向量化加载（检测连续标量 load → 合并为 v4）
    // 注意：此 kernel 在 GIR 构造时已直接使用 GlobalLoadV4
    // VectorizePass 仍会扫描是否有遗漏的标量 load 可合并
    VectorizePass::new().optimize(&mut kernel);

    // Karte 优化 pass 2: 软件流水线（重排 load/compute 顺序，隐藏延迟）
    // 14 个 body 已在编译期展开，PipelinePass 可以交错不同 body 的 load 和 compute
    SoftwarePipelinePass::new().optimize(&mut kernel);

    gir.add_kernel(kernel);

    // PtxCompiler 生成 PTX
    let mut ptx = PtxCompiler::new().target(12, 0); // SM 12.0 = RTX 5080
    let output = ptx.compile(&gir);

    print!("{}", output);
    eprintln!("\nKarte pipeline: GIR → VectorizePass → SoftwarePipelinePass → PtxCompiler");
    eprintln!("Generated {} bytes PTX for sm_12_0", output.len());
}
