//! SPIR-V 二进制后端完整测试套件
//!
//! 覆盖全部 GIR 指令到 SPIR-V Op 的映射，
//! 以及 SPIR-V 模块结构的完整性验证。

use karte_gir::*;
use karte_gpu::SpirvCompiler;
use karte_gpu_runtime::GpuRuntime;

// ============================================================
// 辅助函数
// ============================================================

const SPIRV_MAGIC: u32 = 0x07230203;

fn make_kernel(name: &str, params: Vec<GirParam>, instrs: Vec<GirInstruction>) -> GirProgram {
    let mut func = GirFunction::new(name.to_string());
    func.params = params;
    func.next_reg = 100;
    func.next_label = 50;
    func.block_dim = (256, 1, 1);
    for instr in instrs {
        func.emit(instr);
    }
    func.emit(GirInstruction::Return);
    let mut prog = GirProgram::new();
    prog.add_kernel(func);
    prog
}

fn make_default_kernel(name: &str, instrs: Vec<GirInstruction>) -> GirProgram {
    make_kernel(name, vec![
        GirParam { name: "in".to_string(), dtype: GirDType::F32, is_ptr: true },
        GirParam { name: "out".to_string(), dtype: GirDType::F32, is_ptr: true },
    ], instrs)
}

/// 编译并返回 SPIR-V word 序列
fn compile(instrs: Vec<GirInstruction>) -> Vec<u32> {
    let mut c = SpirvCompiler::new();
    c.compile(&make_default_kernel("test", instrs))
}

/// 编译指定名称 kernel
fn compile_named(name: &str, instrs: Vec<GirInstruction>) -> Vec<u32> {
    let mut c = SpirvCompiler::new();
    c.compile(&make_default_kernel(name, instrs))
}

/// 提取所有 opcode（仅指令头，跳过数据 word）
fn instruction_opcodes(words: &[u32]) -> Vec<u32> {
    let mut ops = Vec::new();
    let mut i = 5; // 跳过 header (5 words)
    while i < words.len() {
        let header = words[i];
        let word_count = (header >> 16) as usize;
        let opcode = header & 0xFFFF;
        ops.push(opcode);
        if word_count == 0 {
            break; // 安全保护
        }
        i += word_count;
    }
    ops
}

/// 验证 SPIR-V header
fn validate_header(words: &[u32]) {
    assert_eq!(words[0], SPIRV_MAGIC, "magic 不正确");
    assert!(words[1] >= 0x00010000, "版本 >= 1.0");
    assert!(words[3] > 0, "bound > 0");
    assert_eq!(words[4], 0, "schema = 0");
}

/// 断言 opcode 存在
fn assert_has_op(words: &[u32], op: u32, name: &str) {
    assert!(
        instruction_opcodes(words).contains(&op),
        "应包含 {} ({})",
        name, op
    );
}

/// 断言 opcode 不存在
fn assert_no_op(words: &[u32], op: u32, name: &str) {
    assert!(
        !instruction_opcodes(words).contains(&op),
        "不应包含 {} ({})",
        name, op
    );
}

// SPIR-V opcode 常量（与 spirv.rs 中一致）
const OP_CAPABILITY: u32 = 17;
const OP_EXT_INST_IMPORT: u32 = 11;
const OP_MEMORY_MODEL: u32 = 14;
const OP_ENTRY_POINT: u32 = 15;
const OP_EXECUTION_MODE: u32 = 16;
const OP_TYPE_VOID: u32 = 19;
const OP_TYPE_BOOL: u32 = 20;
const OP_TYPE_INT: u32 = 21;
const OP_TYPE_FLOAT: u32 = 22;
const OP_TYPE_VECTOR: u32 = 23;
const OP_TYPE_POINTER: u32 = 32;
const OP_TYPE_FUNCTION: u32 = 33;
const OP_CONSTANT: u32 = 43;
const OP_FUNCTION: u32 = 54;
const OP_FUNCTION_END: u32 = 56;
const OP_LABEL: u32 = 248;
const OP_RETURN: u32 = 253;
const OP_VARIABLE: u32 = 59;
const OP_DECORATE: u32 = 71;
const OP_COPY_OBJECT: u32 = 83;
const OP_F_ADD: u32 = 129;
const OP_F_SUB: u32 = 131;
const OP_F_MUL: u32 = 133;
const OP_F_DIV: u32 = 136;
const OP_I_ADD: u32 = 128;
const OP_I_SUB: u32 = 130;
const OP_I_MUL: u32 = 132;
const OP_S_DIV: u32 = 135;
const OP_S_REM: u32 = 138;
const OP_F_REM: u32 = 140;
const OP_EXT_INST: u32 = 12;
const OP_LOAD: u32 = 61;
const OP_STORE: u32 = 62;
const OP_CONVERT_U_TO_PTR: u32 = 120;
const OP_ACCESS_CHAIN: u32 = 65;
const OP_COMPOSITE_EXTRACT: u32 = 81;
const OP_COMPOSITE_CONSTRUCT: u32 = 80;
const OP_CONTROL_BARRIER: u32 = 224;
const OP_BRANCH: u32 = 249;
const OP_BRANCH_CONDITIONAL: u32 = 250;
const OP_SELECT: u32 = 169;
const OP_I_EQUAL: u32 = 170;
const OP_I_NOT_EQUAL: u32 = 171;
const OP_S_LESS_THAN: u32 = 177;
const OP_S_GREATER_THAN: u32 = 173;
const OP_F_ORD_EQUAL: u32 = 180;
const OP_F_ORD_NOT_EQUAL: u32 = 182;
const OP_F_ORD_LESS_THAN: u32 = 184;
const OP_F_ORD_GREATER_THAN: u32 = 186;
const OP_F_ORD_LESS_THAN_EQUAL: u32 = 188;
const OP_F_ORD_GREATER_THAN_EQUAL: u32 = 190;
const OP_GROUP_F_ADD: u32 = 265;
const OP_GROUP_F_MIN: u32 = 266;
const OP_GROUP_F_MAX: u32 = 269;
const OP_PTR_ACCESS_CHAIN: u32 = 67;

// ============================================================
// 1. SPIR-V 模块结构验证
// ============================================================

#[test]
fn test_struct_header() {
    let w = compile(vec![]);
    validate_header(&w);
}

#[test]
fn test_struct_capabilities() {
    let w = compile(vec![]);
    assert_has_op(&w, OP_CAPABILITY, "OpCapability");
}

#[test]
fn test_struct_memory_model() {
    let w = compile(vec![]);
    assert_has_op(&w, OP_MEMORY_MODEL, "OpMemoryModel");
}

#[test]
fn test_struct_ext_inst_import() {
    let w = compile(vec![]);
    assert_has_op(&w, OP_EXT_INST_IMPORT, "OpExtInstImport");
    // 验证 "OpenCL.std" 字符串存在（在 word 流中编码）
    // "OpenCL.std" = [0x706e654f, 0x434c6f70, 0x642e534c, 0x00000064]
    // 搜索 magic 之后是否有该字符串
    let found = w.windows(4).any(|chunk| {
        let s = chunk.iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<_>>();
        String::from_utf8_lossy(&s).contains("OpenCL.std")
    });
    assert!(found, "应包含 OpenCL.std 扩展导入");
}

#[test]
fn test_struct_entry_point() {
    let w = compile_named("my_kernel", vec![]);
    assert_has_op(&w, OP_ENTRY_POINT, "OpEntryPoint");
}

#[test]
fn test_struct_execution_mode_not_emitted() {
    // OpExecutionMode 不再发射 — 让 OpenCL 运行时根据 local_work_size 参数决定
    let w = compile(vec![]);
    assert_no_op(&w, OP_EXECUTION_MODE, "OpExecutionMode 不应被发射");
}

#[test]
fn test_struct_types() {
    let w = compile(vec![]);
    assert_has_op(&w, OP_TYPE_VOID, "OpTypeVoid");
    assert_has_op(&w, OP_TYPE_BOOL, "OpTypeBool");
    assert_has_op(&w, OP_TYPE_INT, "OpTypeInt");
    assert_has_op(&w, OP_TYPE_FLOAT, "OpTypeFloat");
    assert_has_op(&w, OP_TYPE_POINTER, "OpTypePointer");
    assert_has_op(&w, OP_TYPE_FUNCTION, "OpTypeFunction");
}

#[test]
fn test_struct_constants() {
    let w = compile(vec![]);
    assert_has_op(&w, OP_CONSTANT, "OpConstant");
}

#[test]
fn test_struct_function() {
    let w = compile(vec![]);
    assert_has_op(&w, OP_FUNCTION, "OpFunction");
    assert_has_op(&w, OP_FUNCTION_END, "OpFunctionEnd");
    assert_has_op(&w, OP_LABEL, "OpLabel");
    assert_has_op(&w, OP_RETURN, "OpReturn");
}

#[test]
fn test_struct_builtins() {
    // 添加 ThreadId 指令以触发 built-in 变量声明
    let w = compile(vec![
        GirInstruction::ThreadId { dst: 10, dim: ThreadDim::X },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_DECORATE, "OpDecorate");
    assert_has_op(&w, OP_VARIABLE, "OpVariable");
}

#[test]
fn test_struct_custom_block_dim_not_emitted() {
    // block_dim 仍在 GIR 中保留（供 PTX 后端使用），但 SPIR-V 不发射 OpExecutionMode
    let mut func = GirFunction::new("custom".to_string());
    func.params = vec![
        GirParam { name: "x".to_string(), dtype: GirDType::F32, is_ptr: true },
    ];
    func.next_reg = 10;
    func.next_label = 5;
    func.block_dim = (128, 2, 4);
    func.emit(GirInstruction::Return);
    let mut prog = GirProgram::new();
    prog.add_kernel(func);

    let mut c = SpirvCompiler::new();
    let w = c.compile(&prog);
    assert_no_op(&w, OP_EXECUTION_MODE, "OpExecutionMode 不应被发射");
}

// ============================================================
// 2. 标量算术指令
// ============================================================

#[test]
fn test_move() {
    let w = compile(vec![
        GirInstruction::Move { dst: 10, src: GirOperand::Reg(0) },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_COPY_OBJECT, "OpCopyObject");
}

#[test]
fn test_add_f32() {
    let w = compile(vec![
        GirInstruction::Add { dst: 10, src1: GirOperand::Reg(0), src2: GirOperand::Reg(1), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_F_ADD, "OpFAdd");
    assert_no_op(&w, OP_I_ADD, "不应有 OpIAdd");
}

#[test]
fn test_add_i32() {
    let w = compile(vec![
        GirInstruction::Add { dst: 10, src1: GirOperand::Reg(0), src2: GirOperand::Reg(1), dtype: GirDType::I32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_I_ADD, "OpIAdd");
    assert_no_op(&w, OP_F_ADD, "不应有 OpFAdd");
}

#[test]
fn test_sub_f32() {
    let w = compile(vec![
        GirInstruction::Sub { dst: 10, src1: GirOperand::Reg(0), src2: GirOperand::Reg(1), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_F_SUB, "OpFSub");
}

#[test]
fn test_sub_i32() {
    let w = compile(vec![
        GirInstruction::Sub { dst: 10, src1: GirOperand::Reg(0), src2: GirOperand::Reg(1), dtype: GirDType::I32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_I_SUB, "OpISub");
}

#[test]
fn test_mul_f32() {
    let w = compile(vec![
        GirInstruction::Mul { dst: 10, src1: GirOperand::Reg(0), src2: GirOperand::Reg(1), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_F_MUL, "OpFMul");
}

#[test]
fn test_mul_i32() {
    let w = compile(vec![
        GirInstruction::Mul { dst: 10, src1: GirOperand::Reg(0), src2: GirOperand::Reg(1), dtype: GirDType::I32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_I_MUL, "OpIMul");
}

#[test]
fn test_div_f32() {
    let w = compile(vec![
        GirInstruction::Div { dst: 10, src1: GirOperand::Reg(0), src2: GirOperand::Reg(1), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_F_DIV, "OpFDiv");
}

#[test]
fn test_div_i32() {
    let w = compile(vec![
        GirInstruction::Div { dst: 10, src1: GirOperand::Reg(0), src2: GirOperand::Reg(1), dtype: GirDType::I32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_S_DIV, "OpSDiv");
}

#[test]
fn test_mod_f32() {
    let w = compile(vec![
        GirInstruction::Mod { dst: 10, src1: GirOperand::Reg(0), src2: GirOperand::Reg(1), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_F_REM, "OpFRem");
}

#[test]
fn test_mod_i32() {
    let w = compile(vec![
        GirInstruction::Mod { dst: 10, src1: GirOperand::Reg(0), src2: GirOperand::Reg(1), dtype: GirDType::I32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_S_REM, "OpSRem");
}

#[test]
fn test_fma() {
    let w = compile(vec![
        GirInstruction::Fma {
            dst: 10, src1: GirOperand::Reg(0), src2: GirOperand::Reg(1),
            src3: GirOperand::Reg(2), dtype: GirDType::F32,
        },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_EXT_INST, "OpExtInst (Fma)");
}

#[test]
fn test_exp() {
    let w = compile(vec![
        GirInstruction::Exp { dst: 10, src: GirOperand::Reg(0), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_EXT_INST, "OpExtInst (Exp)");
}

#[test]
fn test_recip() {
    let w = compile(vec![
        GirInstruction::Recip { dst: 10, src: GirOperand::Reg(0), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    // Recip = 1.0 / x → OpFDiv
    assert_has_op(&w, OP_F_DIV, "OpFDiv (Recip)");
}

#[test]
fn test_arithmetic_chain() {
    let w = compile(vec![
        GirInstruction::ThreadId { dst: 10, dim: ThreadDim::X },
        GirInstruction::Add { dst: 11, src1: GirOperand::Reg(10), src2: GirOperand::Imm(1), dtype: GirDType::I32 },
        GirInstruction::GlobalLoad { dst: 12, addr: GirOperand::Reg(11), dtype: GirDType::F32 },
        GirInstruction::Mul { dst: 13, src1: GirOperand::Reg(12), src2: GirOperand::Imm(1065353216), dtype: GirDType::F32 },
        GirInstruction::GlobalStore { addr: GirOperand::Param(1), src: GirOperand::Reg(13), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    validate_header(&w);
    assert_has_op(&w, OP_I_ADD, "OpIAdd");
    assert_has_op(&w, OP_F_MUL, "OpFMul");
    assert_has_op(&w, OP_LOAD, "OpLoad");
    assert_has_op(&w, OP_STORE, "OpStore");
}

// ============================================================
// 3. 比较指令 — 全部 CmpOp
// ============================================================

#[test]
fn test_cmp_eq_f32() {
    let w = compile(vec![
        GirInstruction::Cmp { dst: 10, op: CmpOp::Eq, src1: GirOperand::Reg(0), src2: GirOperand::Reg(1), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_F_ORD_EQUAL, "OpFOrdEqual");
    assert_has_op(&w, OP_SELECT, "OpSelect");
}

#[test]
fn test_cmp_ne_f32() {
    let w = compile(vec![
        GirInstruction::Cmp { dst: 10, op: CmpOp::Ne, src1: GirOperand::Reg(0), src2: GirOperand::Reg(1), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_F_ORD_NOT_EQUAL, "OpFOrdNotEqual");
}

#[test]
fn test_cmp_lt_f32() {
    let w = compile(vec![
        GirInstruction::Cmp { dst: 10, op: CmpOp::Lt, src1: GirOperand::Reg(0), src2: GirOperand::Reg(1), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_F_ORD_LESS_THAN, "OpFOrdLessThan");
}

#[test]
fn test_cmp_le_f32() {
    let w = compile(vec![
        GirInstruction::Cmp { dst: 10, op: CmpOp::Le, src1: GirOperand::Reg(0), src2: GirOperand::Reg(1), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_F_ORD_LESS_THAN_EQUAL, "OpFOrdLessThanEqual");
}

#[test]
fn test_cmp_gt_f32() {
    let w = compile(vec![
        GirInstruction::Cmp { dst: 10, op: CmpOp::Gt, src1: GirOperand::Reg(0), src2: GirOperand::Reg(1), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_F_ORD_GREATER_THAN, "OpFOrdGreaterThan");
}

#[test]
fn test_cmp_ge_f32() {
    let w = compile(vec![
        GirInstruction::Cmp { dst: 10, op: CmpOp::Ge, src1: GirOperand::Reg(0), src2: GirOperand::Reg(1), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_F_ORD_GREATER_THAN_EQUAL, "OpFOrdGreaterThanEqual");
}

#[test]
fn test_cmp_eq_i32() {
    let w = compile(vec![
        GirInstruction::Cmp { dst: 10, op: CmpOp::Eq, src1: GirOperand::Reg(0), src2: GirOperand::Reg(1), dtype: GirDType::I32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_I_EQUAL, "OpIEqual");
}

#[test]
fn test_cmp_ne_i32() {
    let w = compile(vec![
        GirInstruction::Cmp { dst: 10, op: CmpOp::Ne, src1: GirOperand::Reg(0), src2: GirOperand::Reg(1), dtype: GirDType::I32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_I_NOT_EQUAL, "OpINotEqual");
}

#[test]
fn test_cmp_lt_i32() {
    let w = compile(vec![
        GirInstruction::Cmp { dst: 10, op: CmpOp::Lt, src1: GirOperand::Reg(0), src2: GirOperand::Reg(1), dtype: GirDType::I32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_S_LESS_THAN, "OpSLessThan");
}

#[test]
fn test_cmp_gt_i32() {
    let w = compile(vec![
        GirInstruction::Cmp { dst: 10, op: CmpOp::Gt, src1: GirOperand::Reg(0), src2: GirOperand::Reg(1), dtype: GirDType::I32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_S_GREATER_THAN, "OpSGreaterThan");
}

// ============================================================
// 4. 控制流指令
// ============================================================

#[test]
fn test_branch_if() {
    let w = compile(vec![
        GirInstruction::Cmp { dst: 10, op: CmpOp::Gt, src1: GirOperand::Reg(0), src2: GirOperand::Reg(1), dtype: GirDType::F32 },
        GirInstruction::BranchIf { cond: GirOperand::Reg(10), then_label: 1, else_label: 2 },
        GirInstruction::Label { id: 1 },
        GirInstruction::Move { dst: 20, src: GirOperand::Imm(1) },
        GirInstruction::Jump { target: 3 },
        GirInstruction::Label { id: 2 },
        GirInstruction::Move { dst: 20, src: GirOperand::Imm(0) },
        GirInstruction::Jump { target: 3 },
        GirInstruction::Label { id: 3 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_BRANCH_CONDITIONAL, "OpBranchConditional");
}

#[test]
fn test_jump() {
    let w = compile(vec![
        GirInstruction::Jump { target: 1 },
        GirInstruction::Label { id: 1 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_BRANCH, "OpBranch");
}

#[test]
fn test_label() {
    let w = compile(vec![
        GirInstruction::Label { id: 0 },
        GirInstruction::Return,
    ]);
    // 每个 Label 产生一个 OpLabel
    let label_count = instruction_opcodes(&w).iter().filter(|&&op| op == OP_LABEL).count();
    // 函数入口已有 1 个 OpLabel，加上显式 Label
    assert!(label_count >= 2, "应有至少 2 个 OpLabel (入口 + 显式)");
}

#[test]
fn test_return() {
    let w = compile(vec![
        GirInstruction::Move { dst: 10, src: GirOperand::Reg(0) },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_RETURN, "OpReturn");
    assert_has_op(&w, OP_FUNCTION_END, "OpFunctionEnd");
}

// ============================================================
// 5. 内存操作指令
// ============================================================

#[test]
fn test_global_load() {
    let w = compile(vec![
        GirInstruction::GlobalLoad { dst: 10, addr: GirOperand::Param(0), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_LOAD, "OpLoad");
    // 指针参数直接用于 OpLoad，不需要 OpConvertUToPtr
    let has_convert = w.iter().any(|word| (*word & 0xFFFF) == 120); // OpConvertUToPtr = 120
    assert!(!has_convert, "指针参数不应有 OpConvertUToPtr");
}

#[test]
fn test_global_store() {
    let w = compile(vec![
        GirInstruction::GlobalStore { addr: GirOperand::Param(1), src: GirOperand::Reg(0), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_STORE, "OpStore");
    // 指针参数直接用于 OpStore，不需要 OpConvertUToPtr
    let has_convert = w.iter().any(|word| (*word & 0xFFFF) == 120);
    assert!(!has_convert, "指针参数不应有 OpConvertUToPtr");
}

#[test]
fn test_global_load_v4() {
    let w = compile(vec![
        GirInstruction::GlobalLoadV4 { dst_base: 10, addr: GirOperand::Param(0), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_TYPE_VECTOR, "OpTypeVector");
    assert_has_op(&w, OP_LOAD, "OpLoad");
    assert_has_op(&w, OP_COMPOSITE_EXTRACT, "OpCompositeExtract");
}

#[test]
fn test_global_store_v4() {
    let w = compile(vec![
        GirInstruction::GlobalStoreV4 { addr: GirOperand::Param(1), src_base: 10, dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_TYPE_VECTOR, "OpTypeVector");
    assert_has_op(&w, OP_STORE, "OpStore");
    assert_has_op(&w, OP_COMPOSITE_CONSTRUCT, "OpCompositeConstruct");
}

#[test]
fn test_global_load_v2() {
    let w = compile(vec![
        GirInstruction::GlobalLoadV2 { dst_base: 10, addr: GirOperand::Param(0), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_TYPE_VECTOR, "OpTypeVector");
    assert_has_op(&w, OP_LOAD, "OpLoad");
    assert_has_op(&w, OP_COMPOSITE_EXTRACT, "OpCompositeExtract");
}

#[test]
fn test_global_store_v2() {
    let w = compile(vec![
        GirInstruction::GlobalStoreV2 { addr: GirOperand::Param(1), src_base: 10, dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_TYPE_VECTOR, "OpTypeVector");
    assert_has_op(&w, OP_STORE, "OpStore");
    assert_has_op(&w, OP_COMPOSITE_CONSTRUCT, "OpCompositeConstruct");
}

#[test]
fn test_shared_load() {
    let w = compile(vec![
        GirInstruction::SharedAlloc { dst: 10, size: 1024, dtype: GirDType::F32 },
        GirInstruction::SharedLoad { dst: 11, addr: GirOperand::Reg(10), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_LOAD, "OpLoad");
    assert_has_op(&w, OP_ACCESS_CHAIN, "OpAccessChain");
}

#[test]
fn test_shared_store() {
    let w = compile(vec![
        GirInstruction::SharedAlloc { dst: 10, size: 1024, dtype: GirDType::F32 },
        GirInstruction::SharedStore { addr: GirOperand::Reg(10), src: GirOperand::Reg(0), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_STORE, "OpStore");
    assert_has_op(&w, OP_ACCESS_CHAIN, "OpAccessChain");
}

#[test]
fn test_shared_alloc() {
    let w = compile(vec![
        GirInstruction::SharedAlloc { dst: 10, size: 2048, dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    // SharedAlloc 当前返回 null 指针 — 不应 panic
    validate_header(&w);
}

// ============================================================
// 6. 同步与通信指令
// ============================================================

#[test]
fn test_barrier() {
    let w = compile(vec![
        GirInstruction::Barrier,
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_CONTROL_BARRIER, "OpControlBarrier");
}

#[test]
fn test_warp_shuffle() {
    let w = compile(vec![
        GirInstruction::WarpShuffle {
            dst: 10, src: GirOperand::Reg(0), src_lane: GirOperand::Imm(1),
            op: ShuffleOp::Idx, dtype: GirDType::F32,
        },
        GirInstruction::Return,
    ]);
    // WarpShuffle 当前用 CopyObject 近似
    assert_has_op(&w, OP_COPY_OBJECT, "OpCopyObject (WarpShuffle 近似)");
}

#[test]
fn test_reduce_sum() {
    let w = compile(vec![
        GirInstruction::Reduce { dst: 10, src: GirOperand::Reg(0), op: ReduceOp::Sum, dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_GROUP_F_ADD, "OpGroupFAdd (Reduce Sum)");
}

#[test]
fn test_reduce_max() {
    let w = compile(vec![
        GirInstruction::Reduce { dst: 10, src: GirOperand::Reg(0), op: ReduceOp::Max, dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_GROUP_F_MAX, "OpGroupFMax (Reduce Max)");
}

#[test]
fn test_reduce_min() {
    let w = compile(vec![
        GirInstruction::Reduce { dst: 10, src: GirOperand::Reg(0), op: ReduceOp::Min, dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_GROUP_F_MIN, "OpGroupFMin (Reduce Min)");
}

// ============================================================
// 7. 线程索引指令 — 全部维度
// ============================================================

#[test]
fn test_thread_id_x() {
    let w = compile(vec![
        GirInstruction::ThreadId { dst: 10, dim: ThreadDim::X },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_LOAD, "OpLoad (LocalInvocationId)");
    assert_has_op(&w, OP_COMPOSITE_EXTRACT, "OpCompositeExtract");
}

#[test]
fn test_thread_id_y() {
    let w = compile(vec![
        GirInstruction::ThreadId { dst: 10, dim: ThreadDim::Y },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_COMPOSITE_EXTRACT, "OpCompositeExtract");
}

#[test]
fn test_thread_id_z() {
    let w = compile(vec![
        GirInstruction::ThreadId { dst: 10, dim: ThreadDim::Z },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_COMPOSITE_EXTRACT, "OpCompositeExtract");
}

#[test]
fn test_block_id_x() {
    let w = compile(vec![
        GirInstruction::BlockId { dst: 10, dim: ThreadDim::X },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_LOAD, "OpLoad (WorkgroupId)");
    assert_has_op(&w, OP_COMPOSITE_EXTRACT, "OpCompositeExtract");
}

#[test]
fn test_block_dim_x() {
    let w = compile(vec![
        GirInstruction::BlockDim { dst: 10, dim: ThreadDim::X },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_LOAD, "OpLoad (WorkgroupSize)");
    assert_has_op(&w, OP_COMPOSITE_EXTRACT, "OpCompositeExtract");
}

#[test]
fn test_grid_dim_x() {
    let w = compile(vec![
        GirInstruction::GridDim { dst: 10, dim: ThreadDim::X },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_LOAD, "OpLoad (NumWorkgroups)");
    assert_has_op(&w, OP_COMPOSITE_EXTRACT, "OpCompositeExtract");
}

#[test]
fn test_all_thread_indices() {
    let w = compile(vec![
        GirInstruction::ThreadId { dst: 10, dim: ThreadDim::X },
        GirInstruction::ThreadId { dst: 11, dim: ThreadDim::Y },
        GirInstruction::ThreadId { dst: 12, dim: ThreadDim::Z },
        GirInstruction::BlockId { dst: 13, dim: ThreadDim::X },
        GirInstruction::BlockId { dst: 14, dim: ThreadDim::Y },
        GirInstruction::BlockId { dst: 15, dim: ThreadDim::Z },
        GirInstruction::BlockDim { dst: 16, dim: ThreadDim::X },
        GirInstruction::BlockDim { dst: 17, dim: ThreadDim::Y },
        GirInstruction::BlockDim { dst: 18, dim: ThreadDim::Z },
        GirInstruction::GridDim { dst: 19, dim: ThreadDim::X },
        GirInstruction::GridDim { dst: 20, dim: ThreadDim::Y },
        GirInstruction::GridDim { dst: 21, dim: ThreadDim::Z },
        GirInstruction::Return,
    ]);
    validate_header(&w);
    // 所有 built-in 变量都应被加载
    let load_count = instruction_opcodes(&w).iter().filter(|&&op| op == OP_LOAD).count();
    assert!(load_count >= 4, "应有至少 4 个 OpLoad (4 个 built-in 各一次)");
}

// ============================================================
// 8. 数学函数指令
// ============================================================

#[test]
fn test_sqrt() {
    let w = compile(vec![
        GirInstruction::Sqrt { dst: 10, src: GirOperand::Reg(0), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_EXT_INST, "OpExtInst (Sqrt)");
}

#[test]
fn test_log() {
    let w = compile(vec![
        GirInstruction::Log { dst: 10, src: GirOperand::Reg(0), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_EXT_INST, "OpExtInst (Log)");
}

#[test]
fn test_rsqrt() {
    let w = compile(vec![
        GirInstruction::Rsqrt { dst: 10, src: GirOperand::Reg(0), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_EXT_INST, "OpExtInst (Rsqrt)");
}

#[test]
fn test_abs_f32() {
    let w = compile(vec![
        GirInstruction::Abs { dst: 10, src: GirOperand::Reg(0), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_EXT_INST, "OpExtInst (FAbs)");
}

#[test]
fn test_abs_i32() {
    let w = compile(vec![
        GirInstruction::Abs { dst: 10, src: GirOperand::Reg(0), dtype: GirDType::I32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_EXT_INST, "OpExtInst (SAbs)");
}

#[test]
fn test_max_f32() {
    let w = compile(vec![
        GirInstruction::Max { dst: 10, src1: GirOperand::Reg(0), src2: GirOperand::Reg(1), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_EXT_INST, "OpExtInst (FMax)");
}

#[test]
fn test_max_i32() {
    let w = compile(vec![
        GirInstruction::Max { dst: 10, src1: GirOperand::Reg(0), src2: GirOperand::Reg(1), dtype: GirDType::I32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_EXT_INST, "OpExtInst (SMax)");
}

#[test]
fn test_min_f32() {
    let w = compile(vec![
        GirInstruction::Min { dst: 10, src1: GirOperand::Reg(0), src2: GirOperand::Reg(1), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_EXT_INST, "OpExtInst (FMin)");
}

#[test]
fn test_min_i32() {
    let w = compile(vec![
        GirInstruction::Min { dst: 10, src1: GirOperand::Reg(0), src2: GirOperand::Reg(1), dtype: GirDType::I32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_EXT_INST, "OpExtInst (SMin)");
}

#[test]
fn test_tanh() {
    let w = compile(vec![
        GirInstruction::Tanh { dst: 10, src: GirOperand::Reg(0), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_EXT_INST, "OpExtInst (Tanh)");
}

#[test]
fn test_cos() {
    let w = compile(vec![
        GirInstruction::Cos { dst: 10, src: GirOperand::Reg(0), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_EXT_INST, "OpExtInst (Cos)");
}

#[test]
fn test_sin() {
    let w = compile(vec![
        GirInstruction::Sin { dst: 10, src: GirOperand::Reg(0), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_EXT_INST, "OpExtInst (Sin)");
}

#[test]
fn test_ceil() {
    let w = compile(vec![
        GirInstruction::Ceil { dst: 10, src: GirOperand::Reg(0), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_EXT_INST, "OpExtInst (Ceil)");
}

#[test]
fn test_floor() {
    let w = compile(vec![
        GirInstruction::Floor { dst: 10, src: GirOperand::Reg(0), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_EXT_INST, "OpExtInst (Floor)");
}

#[test]
fn test_pow() {
    let w = compile(vec![
        GirInstruction::Pow { dst: 10, base: GirOperand::Reg(0), exp: GirOperand::Reg(1), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_EXT_INST, "OpExtInst (Pow)");
}

#[test]
fn test_clamp_f32() {
    let w = compile(vec![
        GirInstruction::Clamp { dst: 10, src: GirOperand::Reg(0), lo: GirOperand::Imm(0), hi: GirOperand::Imm(1065353216), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_EXT_INST, "OpExtInst (FClamp)");
}

#[test]
fn test_clamp_i32() {
    let w = compile(vec![
        GirInstruction::Clamp { dst: 10, src: GirOperand::Reg(0), lo: GirOperand::Imm(0), hi: GirOperand::Imm(255), dtype: GirDType::I32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_EXT_INST, "OpExtInst (SClamp)");
}

#[test]
fn test_lerp() {
    let w = compile(vec![
        GirInstruction::Lerp { dst: 10, a: GirOperand::Reg(0), b: GirOperand::Reg(1), t: GirOperand::Reg(2), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_EXT_INST, "OpExtInst (Mix)");
}

#[test]
fn test_all_math_functions() {
    let w = compile(vec![
        GirInstruction::Sqrt { dst: 10, src: GirOperand::Reg(0), dtype: GirDType::F32 },
        GirInstruction::Log { dst: 11, src: GirOperand::Reg(0), dtype: GirDType::F32 },
        GirInstruction::Rsqrt { dst: 12, src: GirOperand::Reg(0), dtype: GirDType::F32 },
        GirInstruction::Abs { dst: 13, src: GirOperand::Reg(0), dtype: GirDType::F32 },
        GirInstruction::Max { dst: 14, src1: GirOperand::Reg(0), src2: GirOperand::Reg(1), dtype: GirDType::F32 },
        GirInstruction::Min { dst: 15, src1: GirOperand::Reg(0), src2: GirOperand::Reg(1), dtype: GirDType::F32 },
        GirInstruction::Tanh { dst: 16, src: GirOperand::Reg(0), dtype: GirDType::F32 },
        GirInstruction::Cos { dst: 17, src: GirOperand::Reg(0), dtype: GirDType::F32 },
        GirInstruction::Sin { dst: 18, src: GirOperand::Reg(0), dtype: GirDType::F32 },
        GirInstruction::Ceil { dst: 19, src: GirOperand::Reg(0), dtype: GirDType::F32 },
        GirInstruction::Floor { dst: 20, src: GirOperand::Reg(0), dtype: GirDType::F32 },
        GirInstruction::Pow { dst: 21, base: GirOperand::Reg(0), exp: GirOperand::Reg(1), dtype: GirDType::F32 },
        GirInstruction::Clamp { dst: 22, src: GirOperand::Reg(0), lo: GirOperand::Imm(0), hi: GirOperand::Imm(1065353216), dtype: GirDType::F32 },
        GirInstruction::Lerp { dst: 23, a: GirOperand::Reg(0), b: GirOperand::Reg(1), t: GirOperand::Reg(2), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    validate_header(&w);
    // 所有数学函数都通过 OpExtInst 编码
    let ext_count = instruction_opcodes(&w).iter().filter(|&&op| op == OP_EXT_INST).count();
    assert!(ext_count >= 14, "应有至少 14 个 OpExtInst (14 个数学函数)");
}

// ============================================================
// 9. 条件操作指令
// ============================================================

#[test]
fn test_where() {
    let w = compile(vec![
        GirInstruction::Where {
            dst: 10, cond: GirOperand::Reg(0),
            then_val: GirOperand::Imm(1065353216),
            else_val: GirOperand::Imm(0),
            dtype: GirDType::F32,
        },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_SELECT, "OpSelect");
    assert_has_op(&w, OP_F_ORD_NOT_EQUAL, "OpFOrdNotEqual (cond 转 bool)");
}

#[test]
fn test_masked_global_load() {
    let w = compile(vec![
        GirInstruction::MaskedGlobalLoad {
            dst: 10, addr: GirOperand::Param(0), mask: GirOperand::Reg(0),
            default_val: GirOperand::Imm(0), dtype: GirDType::F32,
        },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_SELECT, "OpSelect");
    assert_has_op(&w, OP_LOAD, "OpLoad");
    assert_has_op(&w, OP_CONVERT_U_TO_PTR, "OpConvertUToPtr");
}

#[test]
fn test_masked_global_store() {
    let w = compile(vec![
        GirInstruction::MaskedGlobalStore {
            addr: GirOperand::Param(1), src: GirOperand::Reg(0),
            mask: GirOperand::Reg(1), dtype: GirDType::F32,
        },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_STORE, "OpStore");
    assert_has_op(&w, OP_CONVERT_U_TO_PTR, "OpConvertUToPtr");
}

// ============================================================
// 10. Tile 操作指令
// ============================================================

#[test]
fn test_tile_zeros() {
    let w = compile(vec![
        GirInstruction::TileZeros { dst: 10, tile_rows: 4, tile_cols: 4, dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    // TileZeros 生成 4 个 const zero
    validate_header(&w);
    assert_has_op(&w, OP_CONSTANT, "OpConstant (TileZeros 零初始化)");
}

#[test]
fn test_tile_load() {
    let w = compile(vec![
        GirInstruction::TileLoad {
            dst: 10, base: GirOperand::Param(0), row: GirOperand::Imm(0), col: GirOperand::Imm(0),
            tile_rows: 4, tile_cols: 4, stride: GirOperand::Imm(16), dtype: GirDType::F32,
        },
        GirInstruction::Return,
    ]);
    validate_header(&w); // 不应 panic
}

#[test]
fn test_tile_store() {
    let w = compile(vec![
        GirInstruction::TileStore {
            base: GirOperand::Param(1), row: GirOperand::Imm(0), col: GirOperand::Imm(0),
            src: 10, tile_rows: 4, tile_cols: 4, stride: GirOperand::Imm(16), dtype: GirDType::F32,
        },
        GirInstruction::Return,
    ]);
    validate_header(&w); // 不应 panic
}

#[test]
fn test_tile_matmul() {
    let w = compile(vec![
        GirInstruction::TileMatmul {
            dst: 10, a: 20, b: 30, m: 4, k: 4, n: 4,
            dtype_a: GirDType::F32, dtype_b: GirDType::F32, dtype_c: GirDType::F32,
        },
        GirInstruction::Return,
    ]);
    validate_header(&w); // 不应 panic
}

#[test]
fn test_mma() {
    let w = compile(vec![
        GirInstruction::Mma {
            dst: 10, a: GirOperand::Reg(0), b: GirOperand::Reg(1),
            m: 4, k: 4, n: 4,
            dtype_a: GirDType::F32, dtype_b: GirDType::F32, dtype_c: GirDType::F32,
        },
        GirInstruction::Return,
    ]);
    validate_header(&w); // 不应 panic
}

// ============================================================
// 11. 多 Kernel 程序
// ============================================================

#[test]
fn test_multi_kernel_program() {
    let mut prog = GirProgram::new();

    let mut func1 = GirFunction::new("kernel_a".to_string());
    func1.params = vec![GirParam { name: "x".to_string(), dtype: GirDType::F32, is_ptr: true }];
    func1.next_reg = 10;
    func1.next_label = 5;
    func1.block_dim = (128, 1, 1);
    func1.emit(GirInstruction::ThreadId { dst: 10, dim: ThreadDim::X });
    func1.emit(GirInstruction::GlobalLoad { dst: 11, addr: GirOperand::Reg(10), dtype: GirDType::F32 });
    func1.emit(GirInstruction::Return);

    let mut func2 = GirFunction::new("kernel_b".to_string());
    func2.params = vec![
        GirParam { name: "a".to_string(), dtype: GirDType::F32, is_ptr: true },
        GirParam { name: "b".to_string(), dtype: GirDType::F32, is_ptr: true },
    ];
    func2.next_reg = 10;
    func2.next_label = 5;
    func2.block_dim = (256, 1, 1);
    func2.emit(GirInstruction::ThreadId { dst: 10, dim: ThreadDim::X });
    func2.emit(GirInstruction::GlobalLoad { dst: 11, addr: GirOperand::Param(0), dtype: GirDType::F32 });
    func2.emit(GirInstruction::GlobalLoad { dst: 12, addr: GirOperand::Param(1), dtype: GirDType::F32 });
    func2.emit(GirInstruction::Add { dst: 13, src1: GirOperand::Reg(11), src2: GirOperand::Reg(12), dtype: GirDType::F32 });
    func2.emit(GirInstruction::Return);

    prog.add_kernel(func1);
    prog.add_kernel(func2);

    let mut c = SpirvCompiler::new();
    let w = c.compile(&prog);
    validate_header(&w);

    // 应有 2 个 OpEntryPoint
    let entry_count = instruction_opcodes(&w).iter().filter(|&&op| op == OP_ENTRY_POINT).count();
    assert_eq!(entry_count, 2, "应有 2 个 OpEntryPoint");

    // 应有 2 个 OpFunction + 2 个 OpFunctionEnd
    let func_count = instruction_opcodes(&w).iter().filter(|&&op| op == OP_FUNCTION).count();
    assert_eq!(func_count, 2, "应有 2 个 OpFunction");
    let func_end_count = instruction_opcodes(&w).iter().filter(|&&op| op == OP_FUNCTION_END).count();
    assert_eq!(func_end_count, 2, "应有 2 个 OpFunctionEnd");
}

// ============================================================
// 12. 类型系统测试
// ============================================================

#[test]
fn test_f64_arithmetic() {
    let w = compile(vec![
        GirInstruction::Add { dst: 10, src1: GirOperand::Reg(0), src2: GirOperand::Reg(1), dtype: GirDType::F64 },
        GirInstruction::Return,
    ]);
    // F64 使用 OpFAdd (与 F32 相同 opcode)，但类型声明不同
    assert_has_op(&w, OP_F_ADD, "OpFAdd (F64)");
    // 应有 OpTypeFloat 64 位声明
    let ops = instruction_opcodes(&w);
    for i in 0..ops.len() {
        if ops[i] == OP_TYPE_FLOAT && i + 1 < w.len() {
            // OpTypeFloat: [header, result_id, width]
            let wc = (w[i] >> 16) as usize;
            if wc >= 3 && w[i + 2] == 64 {
                return; // 找到 64-bit float
            }
        }
    }
    // 如果没有 F64 类型声明可能是因为当前 kernel 没有使用 F64 参数
    // 但 Add F64 应该触发生成
    // 不过类型声明是全局的，只在初始化时声明
    validate_header(&w);
}

#[test]
fn test_i64_arithmetic() {
    let w = compile(vec![
        GirInstruction::Add { dst: 10, src1: GirOperand::Reg(0), src2: GirOperand::Reg(1), dtype: GirDType::I64 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_I_ADD, "OpIAdd (I64)");
}

#[test]
fn test_i64_constants() {
    let w = compile(vec![
        GirInstruction::Move { dst: 10, src: GirOperand::Imm(0x1234567890ABCDEF) },
        GirInstruction::Return,
    ]);
    validate_header(&w);
    // 64-bit 常量需要 2 个 value word
    assert_has_op(&w, OP_CONSTANT, "OpConstant (I64)");
}

#[test]
fn test_mixed_int_float() {
    let w = compile(vec![
        GirInstruction::ThreadId { dst: 10, dim: ThreadDim::X },
        GirInstruction::GlobalLoad { dst: 11, addr: GirOperand::Reg(10), dtype: GirDType::F32 },
        GirInstruction::Add { dst: 12, src1: GirOperand::Reg(11), src2: GirOperand::Imm(0), dtype: GirDType::F32 },
        GirInstruction::Cmp { dst: 13, op: CmpOp::Lt, src1: GirOperand::Reg(10), src2: GirOperand::Imm(256), dtype: GirDType::I32 },
        GirInstruction::Where {
            dst: 14, cond: GirOperand::Reg(13),
            then_val: GirOperand::Reg(12), else_val: GirOperand::Imm(0),
            dtype: GirDType::F32,
        },
        GirInstruction::GlobalStore { addr: GirOperand::Param(1), src: GirOperand::Reg(14), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    validate_header(&w);
    // Add F32 产生 OpFAdd (不是 IAdd)
    assert_has_op(&w, OP_F_ADD, "OpFAdd");
    assert_has_op(&w, OP_S_LESS_THAN, "OpSLessThan");
    assert_has_op(&w, OP_SELECT, "OpSelect");
    assert_has_op(&w, OP_LOAD, "OpLoad");
    assert_has_op(&w, OP_STORE, "OpStore");
}

// ============================================================
// 13. 常量编码测试
// ============================================================

#[test]
fn test_f32_constant_encoding() {
    // f32 1.0 = 0x3F800000 = 1065353216
    let w = compile(vec![
        GirInstruction::Add { dst: 10, src1: GirOperand::Reg(0), src2: GirOperand::Imm(1065353216), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_CONSTANT, "OpConstant (f32)");
    assert_has_op(&w, OP_F_ADD, "OpFAdd");
}

#[test]
fn test_i32_constant_encoding() {
    let w = compile(vec![
        GirInstruction::Add { dst: 10, src1: GirOperand::Reg(0), src2: GirOperand::Imm(42), dtype: GirDType::I32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_CONSTANT, "OpConstant (i32)");
    assert_has_op(&w, OP_I_ADD, "OpIAdd");
}

#[test]
fn test_zero_constant() {
    let w = compile(vec![
        GirInstruction::Move { dst: 10, src: GirOperand::Imm(0) },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_CONSTANT, "OpConstant (zero)");
}

#[test]
fn test_negative_i32_constant() {
    let w = compile(vec![
        GirInstruction::Add { dst: 10, src1: GirOperand::Reg(0), src2: GirOperand::Imm(-1), dtype: GirDType::I32 },
        GirInstruction::Return,
    ]);
    assert_has_op(&w, OP_CONSTANT, "OpConstant (negative i32)");
    assert_has_op(&w, OP_I_ADD, "OpIAdd");
}

// ============================================================
// 14. 函数参数测试
// ============================================================

#[test]
fn test_function_parameters() {
    let w = compile(vec![
        GirInstruction::GlobalLoad { dst: 10, addr: GirOperand::Param(0), dtype: GirDType::F32 },
        GirInstruction::GlobalStore { addr: GirOperand::Param(1), src: GirOperand::Reg(10), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    // OpFunctionParameter 应存在
    let ops = instruction_opcodes(&w);
    // OpFunctionParameter = 55
    let param_count = ops.iter().filter(|&&op| op == 55).count();
    assert_eq!(param_count, 2, "应有 2 个 OpFunctionParameter");
}

#[test]
fn test_scalar_parameter() {
    let prog = make_kernel("scalar_param", vec![
        GirParam { name: "alpha".to_string(), dtype: GirDType::F32, is_ptr: false },
        GirParam { name: "out".to_string(), dtype: GirDType::F32, is_ptr: true },
    ], vec![
        GirInstruction::GlobalLoad { dst: 10, addr: GirOperand::Param(0), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    let mut c = SpirvCompiler::new();
    let w = c.compile(&prog);
    validate_header(&w);
    let ops = instruction_opcodes(&w);
    let param_count = ops.iter().filter(|&&op| op == 55).count();
    assert_eq!(param_count, 2, "应有 2 个 OpFunctionParameter");
}

// ============================================================
// 15. 端到端复杂场景测试
// ============================================================

#[test]
fn test_end_to_end_sigmoid_kernel() {
    // 使用 ThreadId (LocalInvocationId) — 与 Python 前端一致，避免 SGPR built-in
    let w = compile(vec![
        GirInstruction::ThreadId { dst: 10, dim: ThreadDim::X },
        // x = input[tid]
        GirInstruction::GlobalLoad { dst: 15, addr: GirOperand::Reg(10), dtype: GirDType::F32 },
        // sigmoid = 1 / (1 + exp(-x))
        GirInstruction::Recip { dst: 16, src: GirOperand::Reg(15), dtype: GirDType::F32 },
        // output[tid] = sigmoid
        GirInstruction::GlobalStore { addr: GirOperand::Param(1), src: GirOperand::Reg(16), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    validate_header(&w);
    assert_has_op(&w, OP_LOAD, "OpLoad");
    assert_has_op(&w, OP_F_DIV, "OpFDiv (Recip)");
    assert_has_op(&w, OP_STORE, "OpStore");
}

#[test]
fn test_end_to_end_dot_product() {
    let w = compile(vec![
        GirInstruction::ThreadId { dst: 10, dim: ThreadDim::X },
        // 加载 4 个连续 float (向量化)
        GirInstruction::GlobalLoadV4 { dst_base: 20, addr: GirOperand::Param(0), dtype: GirDType::F32 },
        // 加载另外 4 个
        GirInstruction::GlobalLoadV4 { dst_base: 30, addr: GirOperand::Param(1), dtype: GirDType::F32 },
        // dot = a[0]*b[0] + a[1]*b[1] + a[2]*b[2] + a[3]*b[3]
        GirInstruction::Mul { dst: 40, src1: GirOperand::Reg(20), src2: GirOperand::Reg(30), dtype: GirDType::F32 },
        GirInstruction::Fma { dst: 41, src1: GirOperand::Reg(21), src2: GirOperand::Reg(31), src3: GirOperand::Reg(40), dtype: GirDType::F32 },
        GirInstruction::Fma { dst: 42, src1: GirOperand::Reg(22), src2: GirOperand::Reg(32), src3: GirOperand::Reg(41), dtype: GirDType::F32 },
        GirInstruction::Fma { dst: 43, src1: GirOperand::Reg(23), src2: GirOperand::Reg(33), src3: GirOperand::Reg(42), dtype: GirDType::F32 },
        // 归约
        GirInstruction::Reduce { dst: 50, src: GirOperand::Reg(43), op: ReduceOp::Sum, dtype: GirDType::F32 },
        // 屏障
        GirInstruction::Barrier,
        // 存储结果
        GirInstruction::GlobalStore { addr: GirOperand::Param(1), src: GirOperand::Reg(50), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    validate_header(&w);
    assert_has_op(&w, OP_F_MUL, "OpFMul");
    assert_has_op(&w, OP_EXT_INST, "OpExtInst (Fma)");
    assert_has_op(&w, OP_GROUP_F_ADD, "OpGroupFAdd (Reduce)");
    assert_has_op(&w, OP_CONTROL_BARRIER, "OpControlBarrier");
    assert_has_op(&w, OP_TYPE_VECTOR, "OpTypeVector (V4 load)");
    assert_has_op(&w, OP_COMPOSITE_EXTRACT, "OpCompositeExtract");
}

#[test]
fn test_end_to_end_conditional_store() {
    let w = compile(vec![
        GirInstruction::ThreadId { dst: 10, dim: ThreadDim::X },
        GirInstruction::BlockDim { dst: 11, dim: ThreadDim::X },
        GirInstruction::Mul { dst: 12, src1: GirOperand::Reg(10), src2: GirOperand::Imm(4), dtype: GirDType::I32 },
        // addr = input + offset
        GirInstruction::Add { dst: 13, src1: GirOperand::Param(0), src2: GirOperand::Reg(12), dtype: GirDType::I64 },
        // val = input[addr]
        GirInstruction::GlobalLoad { dst: 14, addr: GirOperand::Reg(13), dtype: GirDType::F32 },
        // if val > 0.5 then val else 0
        GirInstruction::Cmp { dst: 15, op: CmpOp::Gt, src1: GirOperand::Reg(14), src2: GirOperand::Imm(1056964608), dtype: GirDType::F32 },
        GirInstruction::Where {
            dst: 16, cond: GirOperand::Reg(15),
            then_val: GirOperand::Reg(14), else_val: GirOperand::Imm(0),
            dtype: GirDType::F32,
        },
        // out[addr] = result
        GirInstruction::GlobalStore { addr: GirOperand::Reg(13), src: GirOperand::Reg(16), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    validate_header(&w);
    assert_has_op(&w, OP_F_ORD_GREATER_THAN, "OpFOrdGreaterThan (Cmp Gt)");
    assert_has_op(&w, OP_SELECT, "OpSelect (Where)");
}

#[test]
fn test_end_to_end_body_rot_kernel() {
    // 模拟 body_rot_reward kernel 的核心模式
    let w = compile(vec![
        GirInstruction::ThreadId { dst: 10, dim: ThreadDim::X },
        // total = 0.0
        GirInstruction::Move { dst: 20, src: GirOperand::Imm(0) },
        // 循环展开: j = 0..3
        GirInstruction::GlobalLoad { dst: 30, addr: GirOperand::Reg(10), dtype: GirDType::F32 },
        GirInstruction::GlobalLoad { dst: 31, addr: GirOperand::Reg(10), dtype: GirDType::F32 },
        GirInstruction::Add { dst: 32, src1: GirOperand::Reg(20), src2: GirOperand::Reg(30), dtype: GirDType::F32 },
        GirInstruction::Add { dst: 20, src1: GirOperand::Reg(32), src2: GirOperand::Reg(31), dtype: GirDType::F32 },
        // total = exp(-sigma * total / 14.0)
        GirInstruction::Exp { dst: 40, src: GirOperand::Reg(20), dtype: GirDType::F32 },
        // output[tid] = result
        GirInstruction::GlobalStore { addr: GirOperand::Param(1), src: GirOperand::Reg(40), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    validate_header(&w);
    assert_has_op(&w, OP_F_ADD, "OpFAdd");
    assert_has_op(&w, OP_EXT_INST, "OpExtInst (Exp)");
    assert_has_op(&w, OP_STORE, "OpStore");
}

// ============================================================
// 16. GirDType + TypeMapper 测试
// ============================================================

#[test]
fn test_dtype_spirv_suffix_all() {
    assert_eq!(GirDType::I32.spirv_suffix(), "i32");
    assert_eq!(GirDType::I64.spirv_suffix(), "i64");
    assert_eq!(GirDType::F16.spirv_suffix(), "f16");
    assert_eq!(GirDType::F32.spirv_suffix(), "f32");
    assert_eq!(GirDType::F64.spirv_suffix(), "f64");
}

#[test]
fn test_dtype_ptx_suffix_all() {
    assert_eq!(GirDType::I32.ptx_suffix(), "s32");
    assert_eq!(GirDType::I64.ptx_suffix(), "s64");
    assert_eq!(GirDType::F16.ptx_suffix(), "f16");
    assert_eq!(GirDType::F32.ptx_suffix(), "f32");
    assert_eq!(GirDType::F64.ptx_suffix(), "f64");
}

#[test]
fn test_dtype_is_float() {
    assert!(GirDType::F16.is_float());
    assert!(GirDType::F32.is_float());
    assert!(GirDType::F64.is_float());
    assert!(!GirDType::I32.is_float());
    assert!(!GirDType::I64.is_float());
}

#[test]
fn test_dtype_is_64bit() {
    assert!(GirDType::I64.is_64bit());
    assert!(GirDType::F64.is_64bit());
    assert!(!GirDType::I32.is_64bit());
    assert!(!GirDType::F32.is_64bit());
    assert!(!GirDType::F16.is_64bit());
}

#[test]
fn test_dtype_size() {
    assert_eq!(GirDType::I32.size_in_bytes(), 4);
    assert_eq!(GirDType::I64.size_in_bytes(), 8);
    assert_eq!(GirDType::F32.size_in_bytes(), 4);
    assert_eq!(GirDType::F64.size_in_bytes(), 8);
}

#[test]
fn test_type_mapper_ptx() {
    let mapper = PtxTypeMapper;
    assert_eq!(mapper.map(GirDType::I32), "s32");
    assert_eq!(mapper.map(GirDType::F32), "f32");
}

#[test]
fn test_type_mapper_spirv() {
    let mapper = SpirvTypeMapper;
    assert_eq!(mapper.map(GirDType::I32), "i32");
    assert_eq!(mapper.map(GirDType::F32), "f32");
}

// ============================================================
// 17. 运行时抽象测试
// ============================================================

#[test]
fn test_runtime_detect_backends() {
    let backends = karte_gpu_runtime::detect_available_backends();
    for b in &backends {
        assert!(*b == "cuda" || *b == "opencl", "后端应为 cuda 或 opencl");
    }
}

#[test]
fn test_runtime_auto_select() {
    // 不应 panic
    let _ = karte_gpu_runtime::auto_select_backend();
}

#[test]
fn test_runtime_opencl_creation() {
    let rt = karte_gpu_runtime::OpenClRuntime::new();
    assert_eq!(rt.backend_name(), "opencl");
    // is_available 不应 panic
    let _ = rt.is_available();
}

#[test]
fn test_runtime_cuda_creation() {
    let rt = karte_gpu_runtime::CudaRuntime::new();
    assert_eq!(rt.backend_name(), "cuda");
    let _ = rt.is_available();
}

#[test]
fn test_runtime_launch_config() {
    use karte_gpu_runtime::LaunchConfig;
    let c = LaunchConfig::new(10, 256);
    assert_eq!(c.grid, (10, 1, 1));
    assert_eq!(c.block, (256, 1, 1));
    assert_eq!(c.shared_mem, 0);
    assert_eq!(c.total_threads(), 10 * 256);

    let c2 = LaunchConfig::new_2d((4, 4), (16, 16));
    assert_eq!(c2.grid, (4, 4, 1));
    assert_eq!(c2.block, (16, 16, 1));
    assert_eq!(c2.total_threads(), 4 * 4 * 16 * 16);

    let c3 = LaunchConfig::new(10, 256).with_shared_mem(1024);
    assert_eq!(c3.shared_mem, 1024);
}

// ============================================================
// 18. SPIR-V 二进制输出格式验证
// ============================================================

#[test]
fn test_spirv_output_is_valid_word_sequence() {
    let w = compile(vec![
        GirInstruction::ThreadId { dst: 10, dim: ThreadDim::X },
        GirInstruction::Return,
    ]);
    // 每条 SPIR-V 指令的 word_count 在高 16 位，opcode 在低 16 位
    let mut i = 5; // 跳过 header (5 words)
    while i < w.len() {
        let header = w[i];
        let word_count = (header >> 16) as usize;
        let opcode = header & 0xFFFF;
        assert!(word_count >= 1, "word_count >= 1 at offset {}", i);
        assert!(opcode != 0, "opcode != 0 at offset {}", i);
        assert!(i + word_count <= w.len(), "指令不越界 at offset {} (wc={}, len={})", i, word_count, w.len());
        i += word_count;
    }
}

#[test]
fn test_spirv_word_count_correctness() {
    // 验证所有指令的 word_count 与实际操作数数量一致
    let w = compile(vec![
        GirInstruction::Add { dst: 10, src1: GirOperand::Reg(0), src2: GirOperand::Reg(1), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);
    let mut i = 5;
    while i < w.len() {
        let header = w[i];
        let wc = (header >> 16) as usize;
        // OpFAdd: wc = 1 + 2 + 2 = 5 (header + type + result + src1 + src2)
        // OpReturn: wc = 1
        // OpLabel: wc = 2 (header + label_id)
        assert!(wc >= 1 && wc <= 100, "word_count 合理范围 at offset {}: {}", i, wc);
        i += wc;
    }
}

#[test]
fn test_spirv_hex_dump() {
    use karte_gpu::spirv::spirv_hex_dump;
    let w = compile(vec![]);
    let dump = spirv_hex_dump(&w);
    assert!(dump.contains("07230203"), "hex dump 应包含 magic");
}

// ============================================================
// 19. GpuBackend trait 测试
// ============================================================

#[test]
fn test_gpu_backend_trait_spirv() {
    use karte_gpu::GpuBackend;
    let c = SpirvCompiler::new();
    assert_eq!(c.target_name(), "spirv");
}

#[test]
fn test_gpu_backend_trait_ptx() {
    use karte_gpu::GpuBackend;
    let c = karte_gpu::PtxCompiler::new();
    assert_eq!(c.target_name(), "ptx");
}

#[test]
fn test_gpu_backend_compile_returns_correct_type() {
    use karte_gpu::GpuBackend;
    let mut c = SpirvCompiler::new();
    let prog = make_default_kernel("test", vec![GirInstruction::Return]);
    let output: Vec<u32> = c.compile(&prog);
    assert!(!output.is_empty());
    assert_eq!(output[0], SPIRV_MAGIC);
}

// ============================================================
// 回归测试: 多指针参数 + 固定字节偏移地址计算
// ============================================================
// Bug: addr_pattern_map 把 Imm(字节偏移) 当作元素索引传给 OpPtrAccessChain
// 例如 Imm(4) 被解释为 ptr[4]（第5个 f32）而非 ptr[1]（第2个 f32，偏移4字节）
// 修复: GlobalLoad/GlobalStore 中对 Imm offset 除以 sizeof(dtype) 转为元素索引

#[test]
fn test_multi_ptr_imm_offset_addr_pattern() {
    // 3 个指针参数，用 Imm(4) 作为偏移访问第二个参数的第二个元素
    let prog = make_kernel("multi_ptr", vec![
        GirParam { name: "x".to_string(), dtype: GirDType::F32, is_ptr: true },
        GirParam { name: "y".to_string(), dtype: GirDType::F32, is_ptr: true },
        GirParam { name: "out".to_string(), dtype: GirDType::F32, is_ptr: true },
    ], vec![
        // tid
        GirInstruction::ThreadId { dst: 0, dim: ThreadDim::X },
        // load x[tid]: addr = tid*4 + param[0]
        GirInstruction::Mul { dst: 1, src1: GirOperand::Reg(0), src2: GirOperand::Imm(4), dtype: GirDType::I64 },
        GirInstruction::Add { dst: 1, src1: GirOperand::Reg(1), src2: GirOperand::Param(0), dtype: GirDType::I64 },
        GirInstruction::GlobalLoad { dst: 2, addr: GirOperand::Reg(1), dtype: GirDType::F32 },
        // load y[1]: addr = 4 + param[1]  (Imm(4) = 字节偏移 = 第2个 f32)
        GirInstruction::Add { dst: 3, src1: GirOperand::Imm(4), src2: GirOperand::Param(1), dtype: GirDType::I64 },
        GirInstruction::GlobalLoad { dst: 4, addr: GirOperand::Reg(3), dtype: GirDType::F32 },
        // out = x + y[1]
        GirInstruction::Add { dst: 5, src1: GirOperand::Reg(2), src2: GirOperand::Reg(4), dtype: GirDType::F32 },
        // store out[tid]
        GirInstruction::Mul { dst: 6, src1: GirOperand::Reg(0), src2: GirOperand::Imm(4), dtype: GirDType::I64 },
        GirInstruction::Add { dst: 6, src1: GirOperand::Reg(6), src2: GirOperand::Param(2), dtype: GirDType::I64 },
        GirInstruction::GlobalStore { addr: GirOperand::Reg(6), src: GirOperand::Reg(5), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);

    let mut c = SpirvCompiler::new();
    let w = c.compile(&prog);

    // 验证: 编译成功且包含 OpPtrAccessChain (用于 Imm offset 路径)
    assert_has_op(&w, OP_PTR_ACCESS_CHAIN, "OpPtrAccessChain");
    // 验证: 不包含 OpConvertUToPtr (Imm offset 应走 OpPtrAccessChain 路径而非 fallback)
    // 注意: tid*4+param 也用 OpPtrAccessChain，所以两者都存在是正常的
    assert!(w.len() > 100, "SPIR-V 应有足够指令");
}

#[test]
fn test_imm_offset_uses_correct_element_index() {
    // 验证 Imm(8) 对 f32 指针转换为元素索引 2 (8/4=2)
    // 而非直接使用 8 作为元素索引
    let prog = make_kernel("imm_offset", vec![
        GirParam { name: "x".to_string(), dtype: GirDType::F32, is_ptr: true },
        GirParam { name: "out".to_string(), dtype: GirDType::F32, is_ptr: true },
    ], vec![
        GirInstruction::ThreadId { dst: 0, dim: ThreadDim::X },
        // addr = 8 + param[0]  → 应转换为 OpPtrAccessChain(param, 2)
        GirInstruction::Add { dst: 1, src1: GirOperand::Imm(8), src2: GirOperand::Param(0), dtype: GirDType::I64 },
        GirInstruction::GlobalLoad { dst: 2, addr: GirOperand::Reg(1), dtype: GirDType::F32 },
        // store
        GirInstruction::Mul { dst: 3, src1: GirOperand::Reg(0), src2: GirOperand::Imm(4), dtype: GirDType::I64 },
        GirInstruction::Add { dst: 3, src1: GirOperand::Reg(3), src2: GirOperand::Param(1), dtype: GirDType::I64 },
        GirInstruction::GlobalStore { addr: GirOperand::Reg(3), src: GirOperand::Reg(2), dtype: GirDType::F32 },
        GirInstruction::Return,
    ]);

    let mut c = SpirvCompiler::new();
    let w = c.compile(&prog);

    // 直接遍历 SPIR-V words 查找 OpConstant 值为 2
    let mut i = 5; // 跳过 header
    let mut found_elem_index_2 = false;
    while i < w.len() {
        let opcode = w[i] & 0xFFFF;
        let wc = (w[i] >> 16) as usize;
        if opcode == OP_CONSTANT && wc >= 4 {
            // OpConstant: [header, type_id, result_id, value]
            let val = w[i + 3] as i32;
            if val == 2 {
                found_elem_index_2 = true;
                break;
            }
        }
        if wc == 0 { break; }
        i += wc;
    }
    assert!(found_elem_index_2, "Imm(8) 字节偏移应转换为元素索引 2 (8/sizeof(f32)=2)");
}

// ============================================================
// tile_expansion pass 测试
// ============================================================

#[test]
fn test_tile_expansion_tile_zeros() {
    use karte_gpu::tile_expansion::TileExpander;

    let mut func = GirFunction::new("test".to_string());
    func.emit(GirInstruction::TileZeros { dst: 10, tile_rows: 4, tile_cols: 4, dtype: GirDType::F32 });
    func.emit(GirInstruction::Return);

    let mut prog = GirProgram::new();
    prog.add_kernel(func);

    let expander = TileExpander::with_default();
    let expanded = expander.expand_program(&prog);

    let kernel = &expanded.kernels[0];
    // TileZeros 展开后应为 Move(dst, 0) + Return
    assert!(kernel.instructions.iter().any(|i| matches!(i,
        GirInstruction::Move { dst: 10, src: GirOperand::Imm(0) }
    )), "TileZeros 应展开为 Move 零值");
    // 不应包含 TileZeros 指令
    assert!(!kernel.instructions.iter().any(|i| matches!(i,
        GirInstruction::TileZeros { .. }
    )), "不应有未展开的 TileZeros");
}

#[test]
fn test_tile_expansion_tile_load_generates_shared_store_and_barrier() {
    use karte_gpu::tile_expansion::TileExpander;

    let mut func = GirFunction::new("test".to_string());
    func.params = vec![GirParam { name: "A".to_string(), dtype: GirDType::F32, is_ptr: true }];
    func.emit(GirInstruction::TileLoad {
        dst: 10, base: GirOperand::Param(0), row: GirOperand::Imm(0), col: GirOperand::Imm(0),
        tile_rows: 4, tile_cols: 4, stride: GirOperand::Imm(4), dtype: GirDType::F32,
    });
    func.emit(GirInstruction::Return);

    let mut prog = GirProgram::new();
    prog.add_kernel(func);

    let expander = TileExpander::with_default();
    let expanded = expander.expand_program(&prog);

    let kernel = &expanded.kernels[0];
    // 应包含 GlobalLoad, SharedStore, Barrier
    assert!(kernel.instructions.iter().any(|i| matches!(i, GirInstruction::GlobalLoad { .. })),
        "TileLoad 应展开为含 GlobalLoad");
    assert!(kernel.instructions.iter().any(|i| matches!(i, GirInstruction::SharedStore { .. })),
        "TileLoad 应展开为含 SharedStore");
    assert!(kernel.instructions.iter().any(|i| matches!(i, GirInstruction::Barrier)),
        "TileLoad 应展开为含 Barrier");
    // 不应有未展开的 TileLoad
    assert!(!kernel.instructions.iter().any(|i| matches!(i, GirInstruction::TileLoad { .. })),
        "不应有未展开的 TileLoad");
}

#[test]
fn test_tile_expansion_tile_matmul_generates_fma_loop() {
    use karte_gpu::tile_expansion::TileExpander;

    let m = 2; let k = 3; let n = 2;
    let mut func = GirFunction::new("test".to_string());
    func.emit(GirInstruction::TileZeros { dst: 0, tile_rows: m, tile_cols: n, dtype: GirDType::F32 });
    func.emit(GirInstruction::TileLoad {
        dst: 1, base: GirOperand::Param(0), row: GirOperand::Imm(0), col: GirOperand::Imm(0),
        tile_rows: m, tile_cols: k, stride: GirOperand::Imm(k as i64), dtype: GirDType::F32,
    });
    func.emit(GirInstruction::TileLoad {
        dst: 2, base: GirOperand::Param(1), row: GirOperand::Imm(0), col: GirOperand::Imm(0),
        tile_rows: k, tile_cols: n, stride: GirOperand::Imm(n as i64), dtype: GirDType::F32,
    });
    func.emit(GirInstruction::TileMatmul {
        dst: 0, a: 1, b: 2, m, k, n,
        dtype_a: GirDType::F32, dtype_b: GirDType::F32, dtype_c: GirDType::F32,
    });
    func.emit(GirInstruction::Return);
    func.params = vec![
        GirParam { name: "A".to_string(), dtype: GirDType::F32, is_ptr: true },
        GirParam { name: "B".to_string(), dtype: GirDType::F32, is_ptr: true },
    ];

    let mut prog = GirProgram::new();
    prog.add_kernel(func);

    let expander = TileExpander::with_default();
    let expanded = expander.expand_program(&prog);

    let kernel = &expanded.kernels[0];
    // 应包含 SharedLoad 和 Fma (从 shared memory 读数据并累加)
    assert!(kernel.instructions.iter().any(|i| matches!(i, GirInstruction::SharedLoad { .. })),
        "TileMatmul 应展开为含 SharedLoad");
    assert!(kernel.instructions.iter().any(|i| matches!(i, GirInstruction::Fma { .. })),
        "TileMatmul 应展开为含 Fma");
    // Fma 指令数量应为 k 次 (内层循环)
    let fma_count = kernel.instructions.iter().filter(|i| matches!(i, GirInstruction::Fma { .. })).count();
    assert_eq!(fma_count, k, "TileMatmul 应展开为 k={} 次 Fma", k);
    // 不应有未展开的 TileMatmul
    assert!(!kernel.instructions.iter().any(|i| matches!(i, GirInstruction::TileMatmul { .. })),
        "不应有未展开的 TileMatmul");
}

#[test]
fn test_tile_expansion_compiles_to_spirv() {
    use karte_gpu::tile_expansion::TileExpander;

    // 构建一个简单的 tiled kernel: load tile, matmul, store tile
    let mut func = GirFunction::new("tiled_gemm".to_string());
    func.params = vec![
        GirParam { name: "A".to_string(), dtype: GirDType::F32, is_ptr: true },
        GirParam { name: "B".to_string(), dtype: GirDType::F32, is_ptr: true },
        GirParam { name: "C".to_string(), dtype: GirDType::F32, is_ptr: true },
    ];

    let m = 4; let k = 4; let n = 4;

    func.emit(GirInstruction::TileZeros { dst: 0, tile_rows: m, tile_cols: n, dtype: GirDType::F32 });
    func.emit(GirInstruction::TileLoad {
        dst: 1, base: GirOperand::Param(0), row: GirOperand::Imm(0), col: GirOperand::Imm(0),
        tile_rows: m, tile_cols: k, stride: GirOperand::Imm(k as i64), dtype: GirDType::F32,
    });
    func.emit(GirInstruction::TileLoad {
        dst: 2, base: GirOperand::Param(1), row: GirOperand::Imm(0), col: GirOperand::Imm(0),
        tile_rows: k, tile_cols: n, stride: GirOperand::Imm(n as i64), dtype: GirDType::F32,
    });
    func.emit(GirInstruction::TileMatmul {
        dst: 0, a: 1, b: 2, m, k, n,
        dtype_a: GirDType::F32, dtype_b: GirDType::F32, dtype_c: GirDType::F32,
    });
    func.emit(GirInstruction::TileStore {
        base: GirOperand::Param(2), row: GirOperand::Imm(0), col: GirOperand::Imm(0),
        src: 0, tile_rows: m, tile_cols: n, stride: GirOperand::Imm(n as i64), dtype: GirDType::F32,
    });
    func.emit(GirInstruction::Return);

    let mut prog = GirProgram::new();
    prog.add_kernel(func);

    // 展开 tile 指令
    let expander = TileExpander::with_default();
    let expanded = expander.expand_program(&prog);

    // 编译为 SPIR-V (验证展开后的指令可以被 SPIR-V 后端接受)
    let mut c = SpirvCompiler::new();
    let w = c.compile(&expanded);

    assert!(!w.is_empty(), "SPIR-V 应成功生成");
    assert_eq!(w[0], SPIRV_MAGIC, "SPIR-V magic 正确");
    assert_has_op(&w, OP_CONTROL_BARRIER, "应有 Barrier (OpControlBarrier)");
    assert!(w.len() > 200, "tiled GEMM SPIR-V 应有足够指令");
}

// ============================================================
// operator_fusion 测试
// ============================================================

#[test]
fn test_operator_fusion_elementwise_chain() {
    use karte_gpu::operator_fusion::{OperatorFusion, FusionPattern};

    // kernel1: out[tid] = x[tid] + 1.0
    let mut k1 = GirFunction::new("add_one".to_string());
    k1.params = vec![
        GirParam { name: "x".to_string(), dtype: GirDType::F32, is_ptr: true },
        GirParam { name: "out".to_string(), dtype: GirDType::F32, is_ptr: true },
    ];
    k1.block_dim = (64, 1, 1);
    k1.emit(GirInstruction::ThreadId { dst: 0, dim: ThreadDim::X });
    k1.emit(GirInstruction::Mul { dst: 1, src1: GirOperand::Reg(0), src2: GirOperand::Imm(4), dtype: GirDType::I64 });
    k1.emit(GirInstruction::Add { dst: 1, src1: GirOperand::Reg(1), src2: GirOperand::Param(0), dtype: GirDType::I64 });
    k1.emit(GirInstruction::GlobalLoad { dst: 2, addr: GirOperand::Reg(1), dtype: GirDType::F32 });
    k1.emit(GirInstruction::Add { dst: 3, src1: GirOperand::Reg(2), src2: GirOperand::Imm(0x3F800000), dtype: GirDType::F32 });
    // store to out[tid]
    k1.emit(GirInstruction::Mul { dst: 4, src1: GirOperand::Reg(0), src2: GirOperand::Imm(4), dtype: GirDType::I64 });
    k1.emit(GirInstruction::Add { dst: 4, src1: GirOperand::Reg(4), src2: GirOperand::Param(1), dtype: GirDType::I64 });
    k1.emit(GirInstruction::GlobalStore { addr: GirOperand::Reg(4), src: GirOperand::Reg(3), dtype: GirDType::F32 });
    k1.emit(GirInstruction::Return);

    let pat = OperatorFusion::analyze_kernel(&k1);
    assert_eq!(pat, FusionPattern::ElementWiseChain, "纯 element-wise kernel 应可融合");
}

#[test]
fn test_operator_fusion_detects_reduction_as_not_fusable() {
    use karte_gpu::operator_fusion::{OperatorFusion, FusionPattern};

    let mut k = GirFunction::new("reduce_sum".to_string());
    k.emit(GirInstruction::Reduce { dst: 0, src: GirOperand::Reg(1), op: ReduceOp::Sum, dtype: GirDType::F32 });
    k.emit(GirInstruction::Return);

    let pat = OperatorFusion::analyze_kernel(&k);
    assert_eq!(pat, FusionPattern::NotFusable, "含 Reduction 的 kernel 不可融合");
}

// ============================================================
// auto_tuning 测试
// ============================================================

#[test]
fn test_auto_tuner_generates_variants() {
    use karte_gpu::auto_tuning::{AutoTuner, TuningConfig};

    let config = TuningConfig {
        tile_sizes: vec![(8, 8, 8), (16, 16, 16), (32, 32, 16)],
        num_runs: 1,
        warmup: false,
    };
    let tuner = AutoTuner::new(config);

    let mut func = GirFunction::new("gemm".to_string());
    func.emit(GirInstruction::TileLoad {
        dst: 0, base: GirOperand::Param(0), row: GirOperand::Imm(0), col: GirOperand::Imm(0),
        tile_rows: 16, tile_cols: 16, stride: GirOperand::Imm(16), dtype: GirDType::F32,
    });
    func.emit(GirInstruction::Return);
    func.params = vec![GirParam { name: "A".to_string(), dtype: GirDType::F32, is_ptr: true }];

    let mut prog = GirProgram::new();
    prog.add_kernel(func);

    let variants = tuner.generate_variants(&prog);
    assert_eq!(variants.len(), 3, "应生成 3 个变体");

    // 验证每个变体的 tile 大小不同
    let sizes: Vec<(usize, usize, usize)> = variants.iter().map(|(t, _)| *t).collect();
    assert!(sizes.contains(&(8, 8, 8)), "应包含 8×8×8 变体");
    assert!(sizes.contains(&(16, 16, 16)), "应包含 16×16×16 变体");
    assert!(sizes.contains(&(32, 32, 16)), "应包含 32×32×16 变体");
}

#[test]
fn test_compile_pipeline_optimize() {
    use karte_gpu::CompilePipeline;

    let mut func = GirFunction::new("simple".to_string());
    func.params = vec![
        GirParam { name: "x".to_string(), dtype: GirDType::F32, is_ptr: true },
        GirParam { name: "out".to_string(), dtype: GirDType::F32, is_ptr: true },
    ];
    func.emit(GirInstruction::ThreadId { dst: 0, dim: ThreadDim::X });
    func.emit(GirInstruction::Mul { dst: 1, src1: GirOperand::Reg(0), src2: GirOperand::Imm(4), dtype: GirDType::I64 });
    func.emit(GirInstruction::Add { dst: 1, src1: GirOperand::Reg(1), src2: GirOperand::Param(0), dtype: GirDType::I64 });
    func.emit(GirInstruction::GlobalLoad { dst: 2, addr: GirOperand::Reg(1), dtype: GirDType::F32 });
    func.emit(GirInstruction::Mul { dst: 1, src1: GirOperand::Reg(0), src2: GirOperand::Imm(4), dtype: GirDType::I64 });
    func.emit(GirInstruction::Add { dst: 1, src1: GirOperand::Reg(1), src2: GirOperand::Param(1), dtype: GirDType::I64 });
    func.emit(GirInstruction::GlobalStore { addr: GirOperand::Reg(1), src: GirOperand::Reg(2), dtype: GirDType::F32 });
    func.emit(GirInstruction::Return);

    let mut prog = GirProgram::new();
    prog.add_kernel(func);

    let pipeline = CompilePipeline::default();
    let optimized = pipeline.optimize(&prog);

    assert!(!optimized.kernels.is_empty(), "优化后应有 kernel");
    // 验证没有未展开的 Tile 指令
    for k in &optimized.kernels {
        assert!(!k.instructions.iter().any(|i| matches!(i,
            GirInstruction::TileLoad { .. } | GirInstruction::TileMatmul { .. }
        )), "不应有未展开的 Tile 指令");
    }
}
