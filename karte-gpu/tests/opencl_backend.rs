//! OpenCL C 后端单元测试
//!
//! 测试 GIR → OpenCL C 源码生成的正确性

use karte_gir::*;
use karte_gpu::OpenClCompiler;
use karte_gpu_runtime::GpuRuntime;

fn make_simple_kernel(name: &str, instrs: Vec<GirInstruction>) -> GirProgram {
    let mut func = GirFunction::new(name.to_string());
    func.params = vec![
        GirParam { name: "in".to_string(), dtype: GirDType::F32, is_ptr: true },
        GirParam { name: "out".to_string(), dtype: GirDType::F32, is_ptr: true },
    ];
    func.next_reg = 10;
    func.next_label = 5;
    func.block_dim = (256, 1, 1);
    for instr in instrs {
        func.emit(instr);
    }
    func.emit(GirInstruction::Return);

    let mut prog = GirProgram::new();
    prog.add_kernel(func);
    prog
}

#[test]
fn test_opencl_compiler_basic() {
    let mut compiler = OpenClCompiler::new();
    let prog = make_simple_kernel("test_kernel", vec![]);
    let source = compiler.compile(&prog);

    assert!(source.contains("__kernel void test_kernel"));
    assert!(source.contains("__global float* in"));
    assert!(source.contains("__global float* out"));
    assert!(source.contains("#pragma OPENCL EXTENSION cl_khr_fp64"));
    assert!(source.contains("#pragma OPENCL EXTENSION cl_khr_subgroups"));
}

#[test]
fn test_opencl_arithmetic() {
    let mut compiler = OpenClCompiler::new();
    let prog = make_simple_kernel("arith_kernel", vec![
        GirInstruction::Add { dst: 10, src1: GirOperand::Reg(0), src2: GirOperand::Reg(1), dtype: GirDType::F32 },
        GirInstruction::Sub { dst: 11, src1: GirOperand::Reg(10), src2: GirOperand::Imm(0), dtype: GirDType::F32 },
        GirInstruction::Mul { dst: 12, src1: GirOperand::Reg(11), src2: GirOperand::Imm(1065353216), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    let source = compiler.compile(&prog);

    assert!(source.contains("r10 = r0 + r1"));
    assert!(source.contains("r11 = r10 - as_float(0u)"));
    assert!(source.contains("r12 = r11 * as_float(1065353216u)"));
}

#[test]
fn test_opencl_math_functions() {
    let mut compiler = OpenClCompiler::new();
    let prog = make_simple_kernel("math_kernel", vec![
        GirInstruction::Sqrt { dst: 10, src: GirOperand::Reg(0), dtype: GirDType::F32 },
        GirInstruction::Exp { dst: 11, src: GirOperand::Reg(10), dtype: GirDType::F32 },
        GirInstruction::Log { dst: 12, src: GirOperand::Reg(11), dtype: GirDType::F32 },
        GirInstruction::Sin { dst: 13, src: GirOperand::Reg(0), dtype: GirDType::F32 },
        GirInstruction::Cos { dst: 14, src: GirOperand::Reg(0), dtype: GirDType::F32 },
        GirInstruction::Tanh { dst: 15, src: GirOperand::Reg(0), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    let source = compiler.compile(&prog);

    assert!(source.contains("sqrt("));
    assert!(source.contains("exp("));
    assert!(source.contains("log("));
    assert!(source.contains("sin("));
    assert!(source.contains("cos("));
    assert!(source.contains("tanh("));
}

#[test]
fn test_opencl_thread_index() {
    let mut compiler = OpenClCompiler::new();
    let prog = make_simple_kernel("tid_kernel", vec![
        GirInstruction::ThreadId { dst: 10, dim: ThreadDim::X },
        GirInstruction::BlockId { dst: 11, dim: ThreadDim::Y },
        GirInstruction::BlockDim { dst: 12, dim: ThreadDim::X },
        GirInstruction::GridDim { dst: 13, dim: ThreadDim::Z },
        GirInstruction::Return,
    ]);
    let source = compiler.compile(&prog);

    assert!(source.contains("get_local_id(0)"));
    assert!(source.contains("get_group_id(1)"));
    assert!(source.contains("get_local_size(0)"));
    assert!(source.contains("get_num_groups(2)"));
}

#[test]
fn test_opencl_memory_ops() {
    let mut compiler = OpenClCompiler::new();
    let prog = make_simple_kernel("mem_kernel", vec![
        GirInstruction::GlobalLoad { dst: 10, addr: GirOperand::Param(0), dtype: GirDType::F32 },
        GirInstruction::GlobalStore { addr: GirOperand::Param(1), src: GirOperand::Reg(10), dtype: GirDType::F32 },
        GirInstruction::Barrier,
        GirInstruction::Return,
    ]);
    let source = compiler.compile(&prog);

    assert!(source.contains("barrier(CLK_LOCAL_MEM_FENCE"));
    assert!(source.contains("__global float*)"));
}

#[test]
fn test_opencl_backend_trait() {
    use karte_gpu::GpuBackend;
    let compiler = OpenClCompiler::new();
    assert_eq!(compiler.target_name(), "opencl");
}

#[test]
fn test_opencl_vecl4() {
    let mut compiler = OpenClCompiler::new();
    let prog = make_simple_kernel("v4_kernel", vec![
        GirInstruction::GlobalLoadV4 { dst_base: 10, addr: GirOperand::Param(0), dtype: GirDType::F32 },
        GirInstruction::GlobalStoreV4 { addr: GirOperand::Param(1), src_base: 10, dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    let source = compiler.compile(&prog);

    assert!(source.contains("float4"));
    assert!(source.contains(".s0"));
    assert!(source.contains(".s3"));
}

#[test]
fn test_opencl_where_and_clamp() {
    let mut compiler = OpenClCompiler::new();
    let prog = make_simple_kernel("cond_kernel", vec![
        GirInstruction::Where {
            dst: 10, cond: GirOperand::Reg(0),
            then_val: GirOperand::Imm(1065353216),  // 1.0f
            else_val: GirOperand::Imm(0),           // 0.0f
            dtype: GirDType::F32,
        },
        GirInstruction::Clamp {
            dst: 11, src: GirOperand::Reg(10),
            lo: GirOperand::Imm(0), hi: GirOperand::Imm(1065353216),
            dtype: GirDType::F32,
        },
        GirInstruction::Return,
    ]);
    let source = compiler.compile(&prog);

    assert!(source.contains("?") && source.contains(":"));
    assert!(source.contains("clamp("));
}

#[test]
fn test_gir_dtype_spirv_suffix() {
    assert_eq!(GirDType::I32.spirv_suffix(), "i32");
    assert_eq!(GirDType::I64.spirv_suffix(), "i64");
    assert_eq!(GirDType::F32.spirv_suffix(), "f32");
    assert_eq!(GirDType::F64.spirv_suffix(), "f64");
    assert_eq!(GirDType::F16.spirv_suffix(), "f16");
}

#[test]
fn test_gir_dtype_helpers() {
    assert!(GirDType::F32.is_float());
    assert!(!GirDType::I32.is_float());
    assert!(GirDType::I64.is_64bit());
    assert!(!GirDType::F32.is_64bit());
}

#[test]
fn test_runtime_backend_detection() {
    // 检测函数不应 panic（可能返回空列表）
    let backends = karte_gpu_runtime::detect_available_backends();
    // 在无 GPU 的 CI 环境中可能为空
    for b in &backends {
        assert!(*b == "cuda" || *b == "opencl");
    }
}

#[test]
fn test_opencl_runtime_creation() {
    let rt = karte_gpu_runtime::OpenClRuntime::new();
    assert_eq!(rt.backend_name(), "opencl");
    // is_available 在无 OpenCL 的环境中返回 false，不 panic
    let _ = rt.is_available();
}
