//! SPIR-V 二进制后端单元测试
//!
//! 测试 GIR → SPIR-V 二进制生成的正确性

use karte_gir::*;
use karte_gpu::SpirvCompiler;

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

/// 验证 SPIR-V header 正确性
fn validate_header(words: &[u32]) {
    assert_eq!(words[0], 0x07230203, "SPIR-V magic 不正确");
    assert!(words[1] >= 0x00010000, "SPIR-V 版本 >= 1.0");
    // words[3] = bound (应 > 0)
    assert!(words[3] > 0, "Bound 应 > 0");
    assert_eq!(words[4], 0, "Schema 应为 0");
}

#[test]
fn test_spirv_compiler_basic() {
    let mut compiler = SpirvCompiler::new();
    let prog = make_simple_kernel("test_kernel", vec![]);
    let words = compiler.compile(&prog);

    validate_header(&words);
    // 至少有 header (5) + capabilities + memory model + types + kernel
    assert!(words.len() > 50, "SPIR-V 应有足够指令");
}

#[test]
fn test_spirv_backend_trait() {
    use karte_gpu::GpuBackend;
    let compiler = SpirvCompiler::new();
    assert_eq!(compiler.target_name(), "spirv");
}

#[test]
fn test_spirv_arithmetic() {
    let mut compiler = SpirvCompiler::new();
    let prog = make_simple_kernel("arith_kernel", vec![
        GirInstruction::Add { dst: 10, src1: GirOperand::Reg(0), src2: GirOperand::Reg(1), dtype: GirDType::F32 },
        GirInstruction::Sub { dst: 11, src1: GirOperand::Reg(10), src2: GirOperand::Imm(0), dtype: GirDType::F32 },
        GirInstruction::Mul { dst: 12, src1: GirOperand::Reg(11), src2: GirOperand::Imm(1065353216), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    let words = compiler.compile(&prog);

    validate_header(&words);
    // 验证包含 FAdd (opcode 129), FSub (131), FMul (133)
    let opcodes: Vec<u32> = words.iter().map(|w| w & 0xFFFF).collect();
    assert!(opcodes.contains(&129), "应包含 OpFAdd");
    assert!(opcodes.contains(&131), "应包含 OpFSub");
    assert!(opcodes.contains(&133), "应包含 OpFMul");
}

#[test]
fn test_spirv_math_functions() {
    let mut compiler = SpirvCompiler::new();
    let prog = make_simple_kernel("math_kernel", vec![
        GirInstruction::Sqrt { dst: 10, src: GirOperand::Reg(0), dtype: GirDType::F32 },
        GirInstruction::Exp { dst: 11, src: GirOperand::Reg(10), dtype: GirDType::F32 },
        GirInstruction::Log { dst: 12, src: GirOperand::Reg(11), dtype: GirDType::F32 },
        GirInstruction::Sin { dst: 13, src: GirOperand::Reg(0), dtype: GirDType::F32 },
        GirInstruction::Cos { dst: 14, src: GirOperand::Reg(0), dtype: GirDType::F32 },
        GirInstruction::Tanh { dst: 15, src: GirOperand::Reg(0), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    let words = compiler.compile(&prog);

    validate_header(&words);
    // 验证包含 OpExtInst (opcode 12) 用于数学函数
    let opcodes: Vec<u32> = words.iter().map(|w| w & 0xFFFF).collect();
    assert!(opcodes.contains(&12), "应包含 OpExtInst (数学函数)");
}

#[test]
fn test_spirv_thread_index() {
    let mut compiler = SpirvCompiler::new();
    let prog = make_simple_kernel("tid_kernel", vec![
        GirInstruction::ThreadId { dst: 10, dim: ThreadDim::X },
        GirInstruction::BlockId { dst: 11, dim: ThreadDim::Y },
        GirInstruction::BlockDim { dst: 12, dim: ThreadDim::X },
        GirInstruction::GridDim { dst: 13, dim: ThreadDim::Z },
        GirInstruction::Return,
    ]);
    let words = compiler.compile(&prog);

    validate_header(&words);
    // 验证包含 OpLoad (61) 和 OpCompositeExtract (81)
    let opcodes: Vec<u32> = words.iter().map(|w| w & 0xFFFF).collect();
    assert!(opcodes.contains(&61), "应包含 OpLoad (读取 built-in)");
    assert!(opcodes.contains(&81), "应包含 OpCompositeExtract (提取分量)");
}

#[test]
fn test_spirv_memory_ops() {
    let mut compiler = SpirvCompiler::new();
    let prog = make_simple_kernel("mem_kernel", vec![
        GirInstruction::GlobalLoad { dst: 10, addr: GirOperand::Param(0), dtype: GirDType::F32 },
        GirInstruction::GlobalStore { addr: GirOperand::Param(1), src: GirOperand::Reg(10), dtype: GirDType::F32 },
        GirInstruction::Barrier,
        GirInstruction::Return,
    ]);
    let words = compiler.compile(&prog);

    validate_header(&words);
    // 验证 OpLoad (61), OpStore (62), OpControlBarrier (224), OpConvertUToPtr (120)
    let opcodes: Vec<u32> = words.iter().map(|w| w & 0xFFFF).collect();
    assert!(opcodes.contains(&61), "应包含 OpLoad");
    assert!(opcodes.contains(&62), "应包含 OpStore");
    assert!(opcodes.contains(&224), "应包含 OpControlBarrier");
    assert!(opcodes.contains(&120), "应包含 OpConvertUToPtr");
}

#[test]
fn test_spirv_barrier() {
    let mut compiler = SpirvCompiler::new();
    let prog = make_simple_kernel("barrier_kernel", vec![
        GirInstruction::Barrier,
        GirInstruction::Return,
    ]);
    let words = compiler.compile(&prog);

    validate_header(&words);
    let opcodes: Vec<u32> = words.iter().map(|w| w & 0xFFFF).collect();
    assert!(opcodes.contains(&224), "应包含 OpControlBarrier (224)");
}

#[test]
fn test_spirv_vecl4() {
    let mut compiler = SpirvCompiler::new();
    let prog = make_simple_kernel("v4_kernel", vec![
        GirInstruction::GlobalLoadV4 { dst_base: 10, addr: GirOperand::Param(0), dtype: GirDType::F32 },
        GirInstruction::GlobalStoreV4 { addr: GirOperand::Param(1), src_base: 10, dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    let words = compiler.compile(&prog);

    validate_header(&words);
    // 验证有 OpTypeVector (23), OpCompositeExtract (81), OpCompositeConstruct (80)
    let opcodes: Vec<u32> = words.iter().map(|w| w & 0xFFFF).collect();
    assert!(opcodes.contains(&23), "应包含 OpTypeVector");
    assert!(opcodes.contains(&81), "应包含 OpCompositeExtract (解包)");
    assert!(opcodes.contains(&80), "应包含 OpCompositeConstruct (打包)");
}

#[test]
fn test_spirv_where_and_clamp() {
    let mut compiler = SpirvCompiler::new();
    let prog = make_simple_kernel("cond_kernel", vec![
        GirInstruction::Where {
            dst: 10, cond: GirOperand::Reg(0),
            then_val: GirOperand::Imm(1065353216),
            else_val: GirOperand::Imm(0),
            dtype: GirDType::F32,
        },
        GirInstruction::Clamp {
            dst: 11, src: GirOperand::Reg(10),
            lo: GirOperand::Imm(0), hi: GirOperand::Imm(1065353216),
            dtype: GirDType::F32,
        },
        GirInstruction::Return,
    ]);
    let words = compiler.compile(&prog);

    validate_header(&words);
    // OpSelect = 169, OpExtInst = 12 (for FClamp)
    let opcodes: Vec<u32> = words.iter().map(|w| w & 0xFFFF).collect();
    assert!(opcodes.contains(&169), "应包含 OpSelect (where → select)");
    assert!(opcodes.contains(&12), "应包含 OpExtInst (clamp → OpenCL.std FClamp)");
}

#[test]
fn test_spirv_int_arithmetic() {
    let mut compiler = SpirvCompiler::new();
    let prog = make_simple_kernel("int_kernel", vec![
        GirInstruction::Add { dst: 10, src1: GirOperand::Reg(0), src2: GirOperand::Reg(1), dtype: GirDType::I32 },
        GirInstruction::Div { dst: 11, src1: GirOperand::Reg(10), src2: GirOperand::Imm(2), dtype: GirDType::I32 },
        GirInstruction::Return,
    ]);
    let words = compiler.compile(&prog);

    validate_header(&words);
    let opcodes: Vec<u32> = words.iter().map(|w| w & 0xFFFF).collect();
    assert!(opcodes.contains(&128), "应包含 OpIAdd (128)");
    assert!(opcodes.contains(&135), "应包含 OpSDiv (135)");
}

#[test]
fn test_spirv_entry_point() {
    let mut compiler = SpirvCompiler::new();
    let prog = make_simple_kernel("entry_test", vec![]);
    let words = compiler.compile(&prog);

    validate_header(&words);
    // OpEntryPoint = 15, OpExecutionMode = 16
    let opcodes: Vec<u32> = words.iter().map(|w| w & 0xFFFF).collect();
    assert!(opcodes.contains(&15), "应包含 OpEntryPoint");
    assert!(opcodes.contains(&16), "应包含 OpExecutionMode");
}

#[test]
fn test_spirv_capabilities() {
    let mut compiler = SpirvCompiler::new();
    let prog = make_simple_kernel("cap_test", vec![]);
    let words = compiler.compile(&prog);

    validate_header(&words);
    // OpCapability = 17, OpMemoryModel = 14
    let opcodes: Vec<u32> = words.iter().map(|w| w & 0xFFFF).collect();
    assert!(opcodes.contains(&17), "应包含 OpCapability");
    assert!(opcodes.contains(&14), "应包含 OpMemoryModel");
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
    use karte_gpu_runtime::GpuRuntime;
    let backends = karte_gpu_runtime::detect_available_backends();
    for b in &backends {
        assert!(*b == "cuda" || *b == "opencl");
    }

    let ocl = karte_gpu_runtime::OpenClRuntime::new();
    assert_eq!(ocl.backend_name(), "opencl");
    let _ = ocl.is_available();

    let cuda = karte_gpu_runtime::CudaRuntime::new();
    assert_eq!(cuda.backend_name(), "cuda");
    let _ = cuda.is_available();
}
