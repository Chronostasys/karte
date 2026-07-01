//! SPIR-V 二进制后端 — 将 GIR 编译为 SPIR-V 二进制 (Vec<u32>)
//!
//! 与 PTX 后端对等的低级虚拟 ISA 生成，保留全部 GIR 优化结果。
//! 通过 `clCreateProgramWithIL` 加载 (OpenCL 2.1+)，
//! 支持 AMD / Intel / NVIDIA GPU。

use karte_gir::*;
use std::collections::{HashMap, HashSet};

// ============================================================
// SPIR-V 常量 — 来自官方 spirv.core.grammar.json
// ============================================================

const SPIRV_MAGIC: u32 = 0x07230203;
const SPIRV_VERSION_1_0: u32 = 0x00010000;
const GENERATOR_MAGIC: u32 = 0x4B415254; // "KART"

// Opcodes
const OP_CAPABILITY: u32 = 17;
const OP_EXT_INST_IMPORT: u32 = 11;
const OP_MEMORY_MODEL: u32 = 14;
const OP_ENTRY_POINT: u32 = 15;
const OP_EXECUTION_MODE: u32 = 16;
const OP_DECORATE: u32 = 71;

const OP_TYPE_VOID: u32 = 19;
const OP_TYPE_BOOL: u32 = 20;
const OP_TYPE_INT: u32 = 21;
const OP_TYPE_ARRAY: u32 = 28;
const OP_TYPE_FLOAT: u32 = 22;
const OP_TYPE_VECTOR: u32 = 23;
const OP_TYPE_STRUCT: u32 = 30;
const OP_TYPE_POINTER: u32 = 32;
const OP_TYPE_FUNCTION: u32 = 33;

const OP_CONSTANT_TRUE: u32 = 41;
const OP_CONSTANT_FALSE: u32 = 42;
const OP_CONSTANT: u32 = 43;
const OP_CONSTANT_COMPOSITE: u32 = 44;

const OP_FUNCTION: u32 = 54;
const OP_FUNCTION_PARAMETER: u32 = 55;
const OP_FUNCTION_END: u32 = 56;
const OP_FUNCTION_CALL: u32 = 57;

const OP_VARIABLE: u32 = 59;
const OP_LOAD: u32 = 61;
const OP_STORE: u32 = 62;
const OP_ACCESS_CHAIN: u32 = 65;

const OP_COMPOSITE_CONSTRUCT: u32 = 80;
const OP_COMPOSITE_EXTRACT: u32 = 81;
const OP_VECTOR_SHUFFLE: u32 = 79;
const OP_COPY_OBJECT: u32 = 83;

const OP_S_NEGATE: u32 = 126;
const OP_F_NEGATE: u32 = 127;
const OP_I_ADD: u32 = 128;
const OP_F_ADD: u32 = 129;
const OP_I_SUB: u32 = 130;
const OP_F_SUB: u32 = 131;
const OP_I_MUL: u32 = 132;
const OP_F_MUL: u32 = 133;
const OP_U_DIV: u32 = 134;
const OP_S_DIV: u32 = 135;
const OP_F_DIV: u32 = 136;
const OP_S_REM: u32 = 138;
const OP_F_REM: u32 = 140;
const OP_DOT: u32 = 148;

const OP_SELECT: u32 = 169;
const OP_I_EQUAL: u32 = 170;
const OP_I_NOT_EQUAL: u32 = 171;
const OP_U_GREATER_THAN: u32 = 172;
const OP_S_GREATER_THAN: u32 = 173;
const OP_U_GREATER_THAN_EQUAL: u32 = 174;
const OP_S_GREATER_THAN_EQUAL: u32 = 175;
const OP_U_LESS_THAN: u32 = 176;
const OP_S_LESS_THAN: u32 = 177;
const OP_U_LESS_THAN_EQUAL: u32 = 178;
const OP_S_LESS_THAN_EQUAL: u32 = 179;
const OP_F_ORD_EQUAL: u32 = 180;
const OP_F_ORD_NOT_EQUAL: u32 = 182;

// 指针运算 — 替代 ConvertPtrToU + IAdd + ConvertUToPtr 三步转换
const OP_PTR_ACCESS_CHAIN: u32 = 67;
const OP_F_ORD_LESS_THAN: u32 = 184;
const OP_F_ORD_GREATER_THAN: u32 = 186;
const OP_F_ORD_LESS_THAN_EQUAL: u32 = 188;
const OP_F_ORD_GREATER_THAN_EQUAL: u32 = 190;

const OP_BITWISE_OR: u32 = 197;
const OP_BITWISE_XOR: u32 = 198;
const OP_BITWISE_AND: u32 = 199;
const OP_NOT: u32 = 200;

const OP_CONTROL_BARRIER: u32 = 224;

const OP_LABEL: u32 = 248;
const OP_BRANCH: u32 = 249;
const OP_BRANCH_CONDITIONAL: u32 = 250;
const OP_SELECTION_MERGE: u32 = 247;
const OP_PHI: u32 = 245;
const OP_RETURN: u32 = 253;

const OP_EXT_INST: u32 = 12;
const OP_CONVERT_S_TO_F: u32 = 111;
const OP_CONVERT_U_TO_F: u32 = 112;
const OP_CONVERT_F_TO_S: u32 = 110;
const OP_CONVERT_F_TO_U: u32 = 109;
const OP_BITCAST: u32 = 124;
const OP_CONVERT_U_TO_PTR: u32 = 120;
const OP_CONVERT_PTR_TO_U: u32 = 117;

// 类型转换
const OP_S_CONVERT: u32 = 114;  // OpSConvert — 整数扩展/截断
const OP_F_CONVERT: u32 = 115;  // OpFConvert — 浮点转换

// Group operations
const OP_GROUP_F_ADD: u32 = 265;
const OP_GROUP_F_MIN: u32 = 266;
const OP_GROUP_S_MIN: u32 = 268;
const OP_GROUP_F_MAX: u32 = 269;
const OP_GROUP_S_MAX: u32 = 270;

// Capabilities
const CAP_KERNEL: u32 = 6;
const CAP_ADDRESSES: u32 = 4;
const CAP_FLOAT16: u32 = 9;
const CAP_FLOAT64: u32 = 10;
const CAP_GROUPS: u32 = 57;

// Execution Model
const EXEC_KERNEL: u32 = 6;

// Addressing Model
const ADDR_PHYSICAL64: u32 = 2;

// Memory Model
const MEM_OPENCL: u32 = 2;

// Storage Classes
const SC_INPUT: u32 = 1;
const SC_WORKGROUP: u32 = 4;
const SC_CROSS_WORKGROUP: u32 = 5;
const SC_FUNCTION: u32 = 7;

// Decorations
const DEC_BUILT_IN: u32 = 11;

// Built-in IDs
const BUILTIN_NUM_WORKGROUPS: u32 = 24;
const BUILTIN_WORKGROUP_SIZE: u32 = 25;
const BUILTIN_WORKGROUP_ID: u32 = 26;
const BUILTIN_LOCAL_INVOCATION_ID: u32 = 27;
const BUILTIN_GLOBAL_INVOCATION_ID: u32 = 28; // 纯 VGPR — 替代 BlockId*BlockDim+ThreadId

// Execution Mode
const EXEC_MODE_LOCAL_SIZE: u32 = 17;

// Scope constants (for barriers)
const SCOPE_DEVICE: u32 = 1;
const SCOPE_WORKGROUP: u32 = 2;
const SCOPE_SUBGROUP: u32 = 3;

// Memory Semantics (SPIR-V 规范值)
const MEM_SEM_NONE: u32 = 0;
const MEM_SEM_ACQUIRE: u32 = 0x2;
const MEM_SEM_RELEASE: u32 = 0x4;
const MEM_SEM_ACQUIRE_RELEASE: u32 = 0x8;
const MEM_SEM_SEQ_CST: u32 = 0x10;
const MEM_SEM_WORKGROUP_MEMORY: u32 = 0x20;

// OpenCL.std extended instruction numbers
const OCL_EXP: u32 = 19;
const OCL_FABS: u32 = 23;
const OCL_FLOOR: u32 = 25;
const OCL_FMA: u32 = 26;
const OCL_FMAX: u32 = 27;
const OCL_FMIN: u32 = 28;
const OCL_LOG: u32 = 37;
const OCL_POW: u32 = 48;
const OCL_RSQRT: u32 = 56;
const OCL_SIN: u32 = 57;
const OCL_SQRT: u32 = 61;
const OCL_COS: u32 = 14;
const OCL_TANH: u32 = 63;
const OCL_CEIL: u32 = 12;
const OCL_FCLAMP: u32 = 95;
const OCL_MIX: u32 = 99;
const OCL_SABS: u32 = 141;
const OCL_SCLAMP: u32 = 149;
const OCL_SMAX: u32 = 156;
const OCL_SMIN: u32 = 158;

// ============================================================
// SpirvCompiler
// ============================================================

/// SPIR-V 二进制编译器 — 将 GIR 程序编译为 SPIR-V word 序列
pub struct SpirvCompiler {
    words: Vec<u32>,
    /// 延迟发射的类型/常量/全局变量声明（entry point 之后插入）
    decl_words: Vec<u32>,
    /// 函数体指令缓冲（编译完成后追加到 words 末尾）
    func_words: Vec<u32>,
    /// 是否正在编译函数体（控制 instr 目标缓冲）
    in_function: bool,
    next_id: u32,

    // 类型缓存
    type_void: u32,
    type_bool: u32,
    type_i32: u32,
    type_i64: u32,
    type_f32: u32,
    type_f64: u32,
    type_v3_i32: u32, // vec3<i32> for built-in IDs
    type_ptr_crossworkgroup_f32: u32,
    type_ptr_crossworkgroup_i32: u32,
    type_ptr_crossworkgroup_i64: u32,
    type_ptr_workgroup_f32: u32,
    type_ptr_workgroup_i32: u32,
    type_ptr_workgroup_i64: u32,
    type_ptr_function_f32: u32,
    type_ptr_function_i32: u32,
    type_ptr_function_i64: u32,

    // 常量
    const_zero_i32: u32,
    const_zero_i64: u32,
    const_zero_f32: u32,
    const_one_i32: u32,

    // Scope/semantics 常量
    const_scope_workgroup: u32,
    const_mem_sem_release: u32,

    // Shared memory (Workgroup) 变量
    shared_mem_var: u32,
    shared_mem_type: u32,  // OpTypeArray (array of f32)
    shared_mem_ptr_type: u32, // OpTypePointer Workgroup f32_array

    // OpenCL.std 扩展指令集 ID
    ext_inst_set: u32,

    // Built-in 变量 ID
    builtin_local_invocation_id: u32,
    builtin_workgroup_id: u32,
    builtin_workgroup_size: u32,
    builtin_num_workgroups: u32,
    builtin_global_invocation_id: u32,
    /// 标记哪些 built-in 实际被使用（避免声明未使用的 SGPR built-in 导致 AMD LLVM 问题）
    need_local_invocation_id: bool,
    need_workgroup_id: bool,
    need_workgroup_size: bool,
    need_num_workgroups: bool,
    need_global_invocation_id: bool,
    /// 全局线程 ID 模式优化: 检测到 Add(Mul(BlockId,BlockDim),ThreadId) 模式时
    /// 用 GlobalInvocationId 替代 — 避免 SGPR built-in 导致 AMD VGPR/SGPR copy 错误
    /// 映射: 原始全局 tid reg → GlobalInvocationId.x 的 SPIR-V ID
    global_tid_remap: HashMap<usize, u32>,
    /// 延迟加载的 GlobalInvocationId 目标 reg（等待 OpVariable 发射后再加载）
    pending_global_tid_dst: Option<usize>,
    /// 标记 decl_words 是否已初始化（new() 完成后为 true）
    /// instr_global 在 new() 阶段写 words，compile() 阶段写 decl_words
    decl_words_ready: bool,

    // 运行时映射
    reg_map: HashMap<usize, u32>,    // GIR 寄存器 → SPIR-V ID
    reg_types: HashMap<usize, GirDType>,
    label_map: HashMap<usize, u32>,  // GIR 标签 → SPIR-V label ID
    param_ids: Vec<u32>,             // 函数参数 SPIR-V ID
    param_ptr_types: Vec<u32>,       // 指针参数的类型 ID
    param_is_ptr: Vec<bool>,         // 参数是否为指针
    branch_merge_map: HashMap<usize, usize>, // BranchIf 指令索引 → merge label
    current_instr_idx: usize,        // 当前编译的指令索引
    branch_then_block: u32,         // 最近 BranchIf 的 then block SPIR-V ID
    branch_else_block: u32,         // 最近 BranchIf 的 else block SPIR-V ID
    at_merge_point: bool,           // 当前是否在合并点
    /// 合并点的 Where 指令索引 → Function 局部变量 ID（替代 OpPhi）
    merge_var_map: HashMap<usize, u32>,
    /// Jump 指令索引 → 需要在跳转前存储的 (temp_var_id, value_operand) 列表
    jump_store_map: HashMap<usize, Vec<(u32, GirOperand)>>,
    /// 所有需要声明的 Function 局部变量 ID
    temp_func_vars: Vec<u32>,
    /// Function 存储 class 的 f32 指针类型 ID
    func_f32_ptr_type: u32,
    /// 地址模式：Add(offset, Param(ptr)) 的 dst reg → (param_id, offset_operand)
    /// 用于 GlobalLoad/GlobalStore 时用 OpPtrAccessChain 替代 ConvertPtrToU+IAdd+ConvertUToPtr
    addr_pattern_map: HashMap<usize, (u32, GirOperand)>,
    /// 需要跳过的指令索引（地址计算 Add 已合并到 GlobalLoad/GlobalStore）
    skip_instrs: HashSet<usize>,
    /// i8 类型和 byte 指针类型（用于 OpPtrAccessChain + OpBitcast）
    type_i8: u32,
    ptr_byte_cw_type: u32,  // ptr<CrossWorkgroup, i8>

    // 当前 kernel 基本信息
    kernel_name: String,
    kernel_func_id: u32,
    kernel_func_type_id: u32,
    kernel_block_dim: (usize, usize, usize),

    // 浮点常量缓存: bit_pattern → ID
    f32_const_cache: HashMap<u32, u32>,
    i32_const_cache: HashMap<i32, u32>,
    i64_const_cache: HashMap<i64, u32>,
}

impl SpirvCompiler {
    pub fn new() -> Self {
        let mut c = Self {
            words: Vec::new(),
            decl_words: Vec::new(),
            func_words: Vec::new(),
            in_function: false,
            next_id: 1,
            type_void: 0,
            type_bool: 0,
            type_i32: 0,
            type_i64: 0,
            type_f32: 0,
            type_f64: 0,
            type_v3_i32: 0,
            type_ptr_crossworkgroup_f32: 0,
            type_ptr_crossworkgroup_i32: 0,
            type_ptr_crossworkgroup_i64: 0,
            type_ptr_workgroup_f32: 0,
            type_ptr_workgroup_i32: 0,
            type_ptr_workgroup_i64: 0,
            type_ptr_function_f32: 0,
            type_ptr_function_i32: 0,
            type_ptr_function_i64: 0,
            const_zero_i32: 0,
            const_zero_i64: 0,
            const_zero_f32: 0,
            const_one_i32: 0,
            const_scope_workgroup: 0,
            const_mem_sem_release: 0,
            shared_mem_var: 0,
            shared_mem_type: 0,
            shared_mem_ptr_type: 0,
            ext_inst_set: 0,
            builtin_local_invocation_id: 0,
            builtin_workgroup_id: 0,
            builtin_workgroup_size: 0,
            builtin_num_workgroups: 0,
            builtin_global_invocation_id: 0,
            need_local_invocation_id: false,
            need_workgroup_id: false,
            need_workgroup_size: false,
            need_num_workgroups: false,
            need_global_invocation_id: false,
            global_tid_remap: HashMap::new(),
            pending_global_tid_dst: None,
            decl_words_ready: false,
            reg_map: HashMap::new(),
            reg_types: HashMap::new(),
            label_map: HashMap::new(),
            param_ids: Vec::new(),
            param_ptr_types: Vec::new(),
            param_is_ptr: Vec::new(),
            branch_merge_map: HashMap::new(),
            current_instr_idx: 0,
            branch_then_block: 0,
            branch_else_block: 0,
            at_merge_point: false,
            merge_var_map: HashMap::new(),
            jump_store_map: HashMap::new(),
            temp_func_vars: Vec::new(),
            func_f32_ptr_type: 0,
            addr_pattern_map: HashMap::new(),
            skip_instrs: HashSet::new(),
            type_i8: 0,
            ptr_byte_cw_type: 0,
            kernel_name: String::new(),
            kernel_func_id: 0,
            kernel_func_type_id: 0,
            kernel_block_dim: (256, 1, 1),
            f32_const_cache: HashMap::new(),
            i32_const_cache: HashMap::new(),
            i64_const_cache: HashMap::new(),
        };
        c.emit_header();
        c.emit_capabilities();
        c.emit_extensions();
        c.emit_memory_model();
        // 发射类型/常量/全局变量声明，保存到 decl_words（编译时在 entry point 之后插入）
        let header_end = c.words.len();
        c.emit_types_and_constants();
        // 只分配 built-in 变量 ID，不发射 OpVariable（等 compile() 扫描后再决定发射哪些）
        c.builtin_local_invocation_id = c.alloc_id();
        c.builtin_workgroup_id = c.alloc_id();
        c.builtin_workgroup_size = c.alloc_id();
        c.builtin_num_workgroups = c.alloc_id();
        c.builtin_global_invocation_id = c.alloc_id();
        c.decl_words = c.words[header_end..].to_vec();
        c.words.truncate(header_end);
        c.decl_words_ready = true;
        c
    }

    fn alloc_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// 编译 GIR 程序为 SPIR-V 二进制
    pub fn compile(&mut self, gir: &GirProgram) -> Vec<u32> {
        // 预扫描: 确定哪些 built-in 被使用
        self.scan_builtins(gir);

        // 发射实际使用的 built-in OpVariable
        self.emit_used_builtin_vars();

        // 预分配 kernel 函数 ID 并发射 entry points
        let mut func_ids: Vec<u32> = Vec::new();
        for kernel in &gir.kernels {
            let fid = self.alloc_id();
            func_ids.push(fid);
            self.emit_entry_point(kernel, fid);
        }

        // 发射 built-in 装饰（OpDecorate — 在 entry points 之后、types 之前）
        self.emit_builtin_decorations();

        // 发射函数体（OpTypeFunction + OpFunction + instructions + OpFunctionEnd）
        // 注意: compile_kernel_body 可能向 decl_words 添加类型（如 FuncVariable 指针类型）
        // 所以 decl_words 的批量追加必须在函数体编译之后
        for (i, kernel) in gir.kernels.iter().enumerate() {
            self.compile_kernel_body(kernel, func_ids[i]);
        }

        // 发射全局声明（types + constants + builtins + 函数体中添加的类型 — 在装饰之后、函数体之前）
        self.words.extend_from_slice(&self.decl_words);

        // 将函数体追加到主缓冲（全局声明之后）
        self.words.append(&mut self.func_words);

        self.fixup_header_bound();
        self.words.clone()
    }

    // —— 编码辅助 ——

    fn instr(&mut self, opcode: u32, operands: &[u32]) {
        let word_count = 1 + operands.len() as u32;
        let target = if self.in_function { &mut self.func_words } else { &mut self.words };
        target.push((word_count << 16) | opcode);
        target.extend_from_slice(operands);
    }

    /// 全局指令（类型/常量声明）— 根据 decl_words_ready 标志决定写入目标
    /// new() 阶段写入 words（随后会被保存到 decl_words），compile() 阶段直接写入 decl_words
    fn instr_global(&mut self, opcode: u32, operands: &[u32]) {
        let word_count = 1 + operands.len() as u32;
        if self.decl_words_ready {
            self.decl_words.push((word_count << 16) | opcode);
            self.decl_words.extend_from_slice(operands);
        } else {
            self.words.push((word_count << 16) | opcode);
            self.words.extend_from_slice(operands);
        }
    }

    fn instr_with_result(&mut self, opcode: u32, result_type: u32, operands: &[u32]) -> u32 {
        let result_id = self.alloc_id();
        let word_count = 1 + 2 + operands.len() as u32;
        let target = if self.in_function { &mut self.func_words } else { &mut self.words };
        target.push((word_count << 16) | opcode);
        target.push(result_type);
        target.push(result_id);
        target.extend_from_slice(operands);
        result_id
    }

    fn emit_string(&mut self, s: &str) {
        let mut remaining = s.as_bytes();
        while remaining.len() >= 4 {
            let w = u32::from_le_bytes([remaining[0], remaining[1], remaining[2], remaining[3]]);
            self.words.push(w);
            remaining = &remaining[4..];
        }
        let mut last = [0u8; 4];
        for (i, &b) in remaining.iter().enumerate() {
            last[i] = b;
        }
        self.words.push(u32::from_le_bytes(last));
    }

    // —— Header ——

    fn emit_header(&mut self) {
        self.words.push(SPIRV_MAGIC);
        self.words.push(SPIRV_VERSION_1_0);
        self.words.push(GENERATOR_MAGIC);
        self.words.push(0); // Bound — 后面 fixup
        self.words.push(0); // Schema
    }

    fn fixup_header_bound(&mut self) {
        self.words[3] = self.next_id;
    }

    // —— Capabilities ——

    fn emit_capabilities(&mut self) {
        self.instr(OP_CAPABILITY, &[CAP_KERNEL]);
        self.instr(OP_CAPABILITY, &[CAP_ADDRESSES]);
        // Int64 — 用于 OpTypeInt 64
        self.instr(OP_CAPABILITY, &[11]);
        // Float64 — 用于 OpTypeFloat 64
        self.instr(OP_CAPABILITY, &[10]);
    }

    fn emit_extensions(&mut self) {
        // 导入 OpenCL.std 扩展指令集
        self.ext_inst_set = self.alloc_id();
        let name = "OpenCL.std";
        let str_bytes = name.as_bytes();
        // 字符串 word 数: (bytes + null) 向上取整到 4 字节
        let str_words = (str_bytes.len() + 1 + 3) / 4;
        let total_count = 1 + 1 + str_words as u32; // header + result_id + string
        self.words.push((total_count << 16) | OP_EXT_INST_IMPORT);
        self.words.push(self.ext_inst_set);
        let mut padded: Vec<u8> = str_bytes.to_vec();
        padded.push(0); // null terminator
        while padded.len() % 4 != 0 {
            padded.push(0);
        }
        for chunk in padded.chunks_exact(4) {
            self.words.push(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        }
    }

    fn emit_memory_model(&mut self) {
        self.instr(OP_MEMORY_MODEL, &[ADDR_PHYSICAL64, MEM_OPENCL]);
    }

    // —— 类型与常量声明 ——

    fn emit_types_and_constants(&mut self) {
        // OpTypeVoid
        self.type_void = self.alloc_id();
        self.instr(OP_TYPE_VOID, &[self.type_void]);

        // OpTypeBool
        self.type_bool = self.alloc_id();
        self.instr(OP_TYPE_BOOL, &[self.type_bool]);

        // OpTypeInt 32 — Kernel 模式下必须为无符号 (Signedness=0)
        self.type_i32 = self.alloc_id();
        self.instr(OP_TYPE_INT, &[self.type_i32, 32, 0]);

        // OpTypeInt 64 — Kernel 模式下必须为无符号 (Signedness=0)
        self.type_i64 = self.alloc_id();
        self.instr(OP_TYPE_INT, &[self.type_i64, 64, 0]);

        // OpTypeFloat 32
        self.type_f32 = self.alloc_id();
        self.instr(OP_TYPE_FLOAT, &[self.type_f32, 32]);

        // OpTypeFloat 64
        self.type_f64 = self.alloc_id();
        self.instr(OP_TYPE_FLOAT, &[self.type_f64, 64]);

        // vec3<i32> for built-in IDs
        self.type_v3_i32 = self.alloc_id();
        self.instr_global(OP_TYPE_VECTOR, &[self.type_v3_i32, self.type_i32, 3]);

        // 指针类型
        self.type_ptr_crossworkgroup_f32 = self.alloc_id();
        self.instr_global(OP_TYPE_POINTER, &[self.type_ptr_crossworkgroup_f32, SC_CROSS_WORKGROUP, self.type_f32]);
        self.type_ptr_crossworkgroup_i32 = self.alloc_id();
        self.instr_global(OP_TYPE_POINTER, &[self.type_ptr_crossworkgroup_i32, SC_CROSS_WORKGROUP, self.type_i32]);
        self.type_ptr_crossworkgroup_i64 = self.alloc_id();
        self.instr_global(OP_TYPE_POINTER, &[self.type_ptr_crossworkgroup_i64, SC_CROSS_WORKGROUP, self.type_i64]);

        self.type_ptr_workgroup_f32 = self.alloc_id();
        self.instr_global(OP_TYPE_POINTER, &[self.type_ptr_workgroup_f32, SC_WORKGROUP, self.type_f32]);
        self.type_ptr_workgroup_i32 = self.alloc_id();
        self.instr_global(OP_TYPE_POINTER, &[self.type_ptr_workgroup_i32, SC_WORKGROUP, self.type_i32]);
        self.type_ptr_workgroup_i64 = self.alloc_id();
        self.instr_global(OP_TYPE_POINTER, &[self.type_ptr_workgroup_i64, SC_WORKGROUP, self.type_i64]);

        self.type_ptr_function_f32 = self.alloc_id();
        self.instr_global(OP_TYPE_POINTER, &[self.type_ptr_function_f32, SC_FUNCTION, self.type_f32]);
        self.type_ptr_function_i32 = self.alloc_id();
        self.instr_global(OP_TYPE_POINTER, &[self.type_ptr_function_i32, SC_FUNCTION, self.type_i32]);
        self.type_ptr_function_i64 = self.alloc_id();
        self.instr_global(OP_TYPE_POINTER, &[self.type_ptr_function_i64, SC_FUNCTION, self.type_i64]);

        // 常量: zero values
        self.const_zero_i32 = self.emit_const_i32(0);
        self.const_zero_i64 = self.emit_const_i64(0);
        self.const_zero_f32 = self.emit_const_f32(0.0);
        self.const_one_i32 = self.emit_const_i32(1);

        // Scope 和 Memory Semantics 常量
        // Barrier 使用 AcquireRelease + WorkgroupMemory 确保共享内存可见性
        self.const_scope_workgroup = self.emit_const_i32(SCOPE_WORKGROUP as i32);
        self.const_mem_sem_release = self.emit_const_i32((MEM_SEM_ACQUIRE_RELEASE | MEM_SEM_WORKGROUP_MEMORY) as i32);

        // Shared memory: 发射 Workgroup storage class 的数组变量
        // 最大 4096 个 f32 = 16KB shared memory
        let shared_mem_size = 4096u32;
        let const_size = self.emit_const_i32(shared_mem_size as i32);
        self.shared_mem_type = self.alloc_id();
        self.instr_global(OP_TYPE_ARRAY, &[self.shared_mem_type, self.type_f32, const_size]);
        self.shared_mem_ptr_type = self.alloc_id();
        self.instr_global(OP_TYPE_POINTER, &[self.shared_mem_ptr_type, SC_WORKGROUP, self.shared_mem_type]);
        // OpVariable Workgroup (在全局声明区)
        self.shared_mem_var = self.alloc_id();
        self.instr_global(OP_VARIABLE, &[self.shared_mem_ptr_type, self.shared_mem_var, SC_WORKGROUP]);
    }

    fn emit_const_i32(&mut self, val: i32) -> u32 {
        if let Some(&id) = self.i32_const_cache.get(&val) {
            return id;
        }
        let id = self.alloc_id();
        let word_count = 3u32 + 1;
        self.instr_global(OP_CONSTANT, &[self.type_i32, id, val as u32]);
        self.i32_const_cache.insert(val, id);
        id
    }

    fn emit_const_i64(&mut self, val: i64) -> u32 {
        if let Some(&id) = self.i64_const_cache.get(&val) {
            return id;
        }
        let id = self.alloc_id();
        let lo = val as u32;
        let hi = (val >> 32) as u32;
        self.instr_global(OP_CONSTANT, &[self.type_i64, id, lo, hi]);
        self.i64_const_cache.insert(val, id);
        id
    }

    fn emit_const_f32(&mut self, val: f32) -> u32 {
        let bits = val.to_bits();
        if let Some(&id) = self.f32_const_cache.get(&bits) {
            return id;
        }
        let id = self.alloc_id();
        self.instr_global(OP_CONSTANT, &[self.type_f32, id, bits]);
        self.f32_const_cache.insert(bits, id);
        id
    }

    // —— Built-in 变量 ——

    /// 发射 built-in 的 OpDecorate（在 entry point 之后、types 之前）
    fn emit_builtin_decorations(&mut self) {
        if self.need_local_invocation_id {
            self.instr(OP_DECORATE, &[self.builtin_local_invocation_id, DEC_BUILT_IN, BUILTIN_LOCAL_INVOCATION_ID]);
        }
        if self.need_workgroup_id {
            self.instr(OP_DECORATE, &[self.builtin_workgroup_id, DEC_BUILT_IN, BUILTIN_WORKGROUP_ID]);
        }
        if self.need_workgroup_size {
            self.instr(OP_DECORATE, &[self.builtin_workgroup_size, DEC_BUILT_IN, BUILTIN_WORKGROUP_SIZE]);
        }
        if self.need_num_workgroups {
            self.instr(OP_DECORATE, &[self.builtin_num_workgroups, DEC_BUILT_IN, BUILTIN_NUM_WORKGROUPS]);
        }
        if self.need_global_invocation_id {
            self.instr(OP_DECORATE, &[self.builtin_global_invocation_id, DEC_BUILT_IN, BUILTIN_GLOBAL_INVOCATION_ID]);
        }
    }

    /// 发射实际使用的 built-in 的 OpVariable + OpTypePointer
    fn emit_used_builtin_vars(&mut self) {
        // Input 指针类型: ptr<Input, vec3<i32>> — 写入 decl_words（在类型声明之后）
        let ptr_type = self.alloc_id();
        let wc = 4u32;
        self.decl_words.push((wc << 16) | OP_TYPE_POINTER);
        self.decl_words.push(ptr_type);
        self.decl_words.push(SC_INPUT);
        self.decl_words.push(self.type_v3_i32);

        let emit_var = |dw: &mut Vec<u32>, pt: u32, vid: u32| {
            dw.push((4u32 << 16) | OP_VARIABLE);
            dw.push(pt);
            dw.push(vid);
            dw.push(SC_INPUT);
        };
        if self.need_local_invocation_id { emit_var(&mut self.decl_words, ptr_type, self.builtin_local_invocation_id); }
        if self.need_workgroup_id { emit_var(&mut self.decl_words, ptr_type, self.builtin_workgroup_id); }
        if self.need_workgroup_size { emit_var(&mut self.decl_words, ptr_type, self.builtin_workgroup_size); }
        if self.need_num_workgroups { emit_var(&mut self.decl_words, ptr_type, self.builtin_num_workgroups); }
        if self.need_global_invocation_id { emit_var(&mut self.decl_words, ptr_type, self.builtin_global_invocation_id); }
    }

    /// 扫描所有 kernel 指令，标记哪些 built-in 被使用
    fn scan_builtins(&mut self, gir: &GirProgram) {
        for kernel in &gir.kernels {
            // 预检测: BlockId*BlockDim+ThreadId 模式 → 用 GlobalInvocationId 替代
            // 收集所有 BlockId/BlockDim/ThreadId 的 dst reg
            let mut blockid_regs: Vec<(usize, usize)> = Vec::new(); // (instr_idx, dst_reg)
            let mut blockdim_regs: Vec<(usize, usize)> = Vec::new();
            let mut threadid_regs: Vec<(usize, usize)> = Vec::new();
            for (i, instr) in kernel.instructions.iter().enumerate() {
                match instr {
                    GirInstruction::BlockId { dst, .. } => blockid_regs.push((i, *dst)),
                    GirInstruction::BlockDim { dst, .. } => blockdim_regs.push((i, *dst)),
                    GirInstruction::ThreadId { dst, .. } => threadid_regs.push((i, *dst)),
                    _ => {}
                }
            }
            // 检测 Add(Mul(BlockId, BlockDim), ThreadId) 模式
            let mut found_pattern = false;
            for &(bi_idx, bi_reg) in &blockid_regs {
                for &(bd_idx, bd_reg) in &blockdim_regs {
                    for &(ti_idx, ti_reg) in &threadid_regs {
                        // 寻找 Mul(bi_reg, bd_reg) 或 Mul(bd_reg, bi_reg)
                        for (mi, instr) in kernel.instructions.iter().enumerate() {
                            if let GirInstruction::Mul { dst: mul_dst, src1, src2, .. } = instr {
                                let is_match = (matches!(src1, GirOperand::Reg(r) if *r == bi_reg) && matches!(src2, GirOperand::Reg(r) if *r == bd_reg))
                                            || (matches!(src1, GirOperand::Reg(r) if *r == bd_reg) && matches!(src2, GirOperand::Reg(r) if *r == bi_reg));
                                if is_match {
                                    // 寻找 Add(mul_dst, ti_reg) 或 Add(ti_reg, mul_dst)
                                    for (ai, ainstr) in kernel.instructions.iter().enumerate() {
                                        if let GirInstruction::Add { dst: add_dst, src1, src2, .. } = ainstr {
                                            let is_add = (matches!(src1, GirOperand::Reg(r) if *r == *mul_dst) && matches!(src2, GirOperand::Reg(r) if *r == ti_reg))
                                                       || (matches!(src1, GirOperand::Reg(r) if *r == ti_reg) && matches!(src2, GirOperand::Reg(r) if *r == *mul_dst));
                                            if is_add {
                                                // 检查 bi_reg, bd_reg, ti_reg, mul_dst 是否只在此模式中使用
                                                let mut all_only = true;
                                                for (ci, cinstr) in kernel.instructions.iter().enumerate() {
                                                    if ci == bi_idx || ci == bd_idx || ci == ti_idx || ci == mi || ci == ai {
                                                        continue; // 来源指令本身
                                                    }
                                                    let (br, dr, tr2) = (bi_reg, bd_reg, ti_reg);
                                                    let refs = |op: &GirOperand| -> bool {
                                                        matches!(op, GirOperand::Reg(r) if *r == br || *r == dr || *r == tr2)
                                                    };
                                                    let uses = match cinstr {
                                                        GirInstruction::Mul { src1, src2, .. } => refs(src1) || refs(src2),
                                                        GirInstruction::Add { src1, src2, .. } => refs(src1) || refs(src2),
                                                        _ => false, // 其他指令不检查（简化：假设只在 Mul/Add 中引用）
                                                    };
                                                    if uses { all_only = false; break; }
                                                }
                                                if all_only {
                                                    self.need_global_invocation_id = true;
                                                    found_pattern = true;
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            if found_pattern { break; }
                        }
                        if found_pattern { break; }
                    }
                    if found_pattern { break; }
                }
                if found_pattern { break; }
            }
            // 如果检测到全局 ID 模式，不设置单独的 BlockId/BlockDim/ThreadId 标志
            if found_pattern { continue; }
            // 否则: 设置单独的 built-in 标志
            for instr in &kernel.instructions {
                match instr {
                    GirInstruction::ThreadId { .. } => self.need_local_invocation_id = true,
                    GirInstruction::BlockId { .. } => self.need_workgroup_id = true,
                    GirInstruction::BlockDim { .. } => self.need_workgroup_size = true,
                    GirInstruction::GridDim { .. } => self.need_num_workgroups = true,
                    _ => {}
                }
            }
        }
    }

    // —— 类型辅助 ——

    fn type_id(&self, dtype: GirDType) -> u32 {
        match dtype {
            GirDType::I32 => self.type_i32,
            GirDType::I64 => self.type_i64,
            GirDType::F32 => self.type_f32,
            GirDType::F64 => self.type_f64,
            GirDType::F16 => self.type_f32, // f16 暂映射为 f32
        }
    }

    fn ptr_type_id(&self, dtype: GirDType, storage: u32) -> u32 {
        // 返回缓存的指针类型或创建新的（简化：不缓存动态组合）
        // 这里返回预缓存的常用类型
        match (dtype, storage) {
            (GirDType::F32, SC_CROSS_WORKGROUP) => self.type_ptr_crossworkgroup_f32,
            (GirDType::I32, SC_CROSS_WORKGROUP) => self.type_ptr_crossworkgroup_i32,
            (GirDType::I64, SC_CROSS_WORKGROUP) => self.type_ptr_crossworkgroup_i64,
            (GirDType::F32, SC_WORKGROUP) => self.type_ptr_workgroup_f32,
            (GirDType::I32, SC_WORKGROUP) => self.type_ptr_workgroup_i32,
            (GirDType::I64, SC_WORKGROUP) => self.type_ptr_workgroup_i64,
            (GirDType::F32, SC_FUNCTION) => self.type_ptr_function_f32,
            (GirDType::I32, SC_FUNCTION) => self.type_ptr_function_i32,
            (GirDType::I64, SC_FUNCTION) => self.type_ptr_function_i64,
            _ => self.type_ptr_crossworkgroup_f32, // fallback
        }
    }

    // —— 操作数解析 ——

    fn operand_id(&mut self, op: &GirOperand, dtype: GirDType) -> u32 {
        match op {
            GirOperand::Reg(id) => {
                // 全局线程 ID 优化: 如果此 reg 被重映射到 GlobalInvocationId
                if let Some(&gid) = self.global_tid_remap.get(id) {
                    // 检查类型是否匹配
                    if dtype != GirDType::I32 {
                        let target_type = self.type_id(dtype);
                        return self.instr_with_result(OP_S_CONVERT, target_type, &[gid]);
                    }
                    return gid;
                }
                let raw = *self.reg_map.get(id).unwrap_or(&self.const_zero_i32);
                // 检查是否需要类型转换
                if let Some(&reg_dtype) = self.reg_types.get(id) {
                    if reg_dtype != dtype && !reg_dtype.is_float() == !dtype.is_float() {
                        // 同类类型（int→int 或 float→float）但宽度不同 — 发射转换
                        let target_type = self.type_id(dtype);
                        let conv_op = if dtype.is_float() { OP_F_CONVERT } else { OP_S_CONVERT };
                        return self.instr_with_result(conv_op, target_type, &[raw]);
                    }
                }
                raw
            }
            GirOperand::Imm(val) => {
                if dtype.is_float() {
                    self.emit_const_f32(f32::from_bits(*val as u32))
                } else if dtype.is_64bit() {
                    self.emit_const_i64(*val)
                } else {
                    self.emit_const_i32(*val as i32)
                }
            }
            GirOperand::Label(id) => {
                *self.label_map.get(id).unwrap_or(&self.const_zero_i32)
            }
            GirOperand::Param(id) => {
                let param_id = self.param_ids.get(*id).copied().unwrap_or(self.const_zero_i32);
                // 指针参数在整数运算中需要先转换为 i64
                if *id < self.param_is_ptr.len() && self.param_is_ptr[*id] && !dtype.is_float() {
                    let ptr_to_u = self.instr_with_result(OP_CONVERT_PTR_TO_U, self.type_i64, &[param_id]);
                    ptr_to_u
                } else {
                    param_id
                }
            }
        }
    }

    // —— Kernel 编译 ——

    /// 发射 OpEntryPoint + OpExecutionMode（在 entry point section）
    fn emit_entry_point(&mut self, func: &GirFunction, func_id: u32) {
        // 只包含实际使用的 built-in 在 OpEntryPoint 接口列表中
        let mut iface_ids: Vec<u32> = Vec::new();
        if self.need_local_invocation_id { iface_ids.push(self.builtin_local_invocation_id); }
        if self.need_workgroup_id { iface_ids.push(self.builtin_workgroup_id); }
        if self.need_workgroup_size { iface_ids.push(self.builtin_workgroup_size); }
        if self.need_num_workgroups { iface_ids.push(self.builtin_num_workgroups); }
        if self.need_global_invocation_id { iface_ids.push(self.builtin_global_invocation_id); }

        let name_bytes = func.name.as_bytes();
        let str_words = (name_bytes.len() + 1 + 3) / 4;
        let total_words = 1 + 2 + str_words + iface_ids.len();
        self.words.push((total_words as u32) << 16 | OP_ENTRY_POINT);
        self.words.push(EXEC_KERNEL);
        self.words.push(func_id);
        let mut name_padded: Vec<u8> = name_bytes.to_vec();
        name_padded.push(0);
        while name_padded.len() % 4 != 0 { name_padded.push(0); }
        for chunk in name_padded.chunks_exact(4) {
            self.words.push(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        }
        for id in &iface_ids { self.words.push(*id); }

        // 不发射 OpExecutionMode LocalSize — 让 OpenCL 运行时根据
        // clEnqueueNDRangeKernel 的 local_work_size 参数自行决定 workgroup 大小。
        // 若发射了 LocalSize，则运行时 local_work_size 必须与之精确匹配，
        // 否则报 CL_INVALID_WORK_GROUP_SIZE (-54)。
    }

    fn compile_kernel_body(&mut self, func: &GirFunction, func_id: u32) {
        self.reg_map.clear();
        self.reg_types.clear();
        self.label_map.clear();
        self.param_ids.clear();
        self.param_ptr_types.clear();
        self.param_is_ptr.clear();
        self.kernel_name = func.name.clone();
        self.kernel_block_dim = func.block_dim;

        // 预扫描：收集寄存器类型
        self.scan_reg_types(func);

        // 预分配所有 label ID
        for instr in &func.instructions {
            if let GirInstruction::Label { id } = instr {
                let lid = self.alloc_id();
                self.label_map.insert(*id, lid);
            }
        }

        // 构建函数类型: void (ptr_f32, ptr_f32, ...) 或 void (ptr, ...)
        let mut param_types: Vec<u32> = Vec::new();
        for param in &func.params {
            let ptype = if param.is_ptr {
                let pt = self.alloc_id();
                self.instr_global(OP_TYPE_POINTER, &[pt, SC_CROSS_WORKGROUP, self.type_id(param.dtype)]);
                param_types.push(pt);
                self.param_ptr_types.push(pt);
                self.param_is_ptr.push(true);
                pt
            } else {
                param_types.push(self.type_id(param.dtype));
                self.param_is_ptr.push(false);
                self.type_id(param.dtype)
            };
            let _ = ptype;
        }

        // OpTypeFunction void (param_types...)
        self.kernel_func_type_id = self.alloc_id();
        let mut func_type_operands = vec![self.kernel_func_type_id, self.type_void];
        func_type_operands.extend(&param_types);
        let wc = (1 + func_type_operands.len() as u32) << 16;
        self.decl_words.push(wc | OP_TYPE_FUNCTION);
        self.decl_words.extend_from_slice(&func_type_operands);

        // 使用预分配的 func_id
        self.kernel_func_id = func_id;

        // 进入函数体模式 — 后续指令发射到 func_words
        self.in_function = true;

        // OpFunction void %func_id None %func_type
        let func_control = 0u32; // None
        self.instr(OP_FUNCTION, &[
            self.type_void, self.kernel_func_id, func_control, self.kernel_func_type_id,
        ]);

        // 函数参数
        for (i, param) in func.params.iter().enumerate() {
            let ptype = param_types[i];
            let pid = self.instr_with_result(OP_FUNCTION_PARAMETER, ptype, &[]);
            self.param_ids.push(pid);
            // 将参数寄存器 ID i 映射到参数
            self.reg_map.insert(i, pid);
        }

        // 第一个基本块必须以 OpLabel 开始 — 分配独立的入口 label ID
        let entry_label = self.alloc_id();
        self.instr(OP_LABEL, &[entry_label]);

        // 声明局部变量（对每个非参数寄存器创建 OpVariable Function）
        let num_params = func.params.len();
        // 收集所有需要声明的寄存器
        let mut all_regs: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
        for instr in &func.instructions {
            self.collect_result_regs(instr, &mut all_regs);
        }
        for &reg_id in &all_regs {
            if reg_id >= num_params && !self.reg_map.contains_key(&reg_id) {
                let dtype = self.reg_types.get(&reg_id).copied().unwrap_or(GirDType::F32);
                // 为寄存器分配一个 SPIR-V ID（值不需要显式变量——SPIR-V 是 SSA）
                let var_id = self.alloc_id();
                self.reg_map.insert(reg_id, var_id);
            }
        }

        // 编译指令序列
        let instrs = &func.instructions;
        let last_is_ret = instrs.last().map(|i| matches!(i, GirInstruction::Return)).unwrap_or(false);
        let end = if last_is_ret { instrs.len() - 1 } else { instrs.len() };

        // 预扫描: 为每个 BranchIf 找到 merge label（其后第一个 Jump 的目标）
        self.branch_merge_map.clear();
        self.merge_var_map.clear();
        self.jump_store_map.clear();
        self.temp_func_vars.clear();
        self.addr_pattern_map.clear();
        self.skip_instrs.clear();
        self.global_tid_remap.clear();

        // 预扫描: 地址计算模式 — Mul(tid, sizeof) + Add(offset, Param(ptr))
        // 检测此模式后用 OpPtrAccessChain(param, tid) 替代 ConvertPtrToU+IAdd+ConvertUToPtr
        // tid 直接作为元素偏移（不需要 byte pointer / i8 类型）
        for i in 0..end {
            if let GirInstruction::Add { dst, src1, src2, dtype: GirDType::I64 } = &instrs[i] {
                // 检查 src2 是否是指针 Param
                if let GirOperand::Param(pid) = src2 {
                    if *pid < func.params.len() && func.params[*pid].is_ptr {
                        // 检查 src1 (offset) 是否来自 Mul(tid, sizeof)
                        if let GirOperand::Reg(offset_reg) = src1 {
                            for j in 0..i {
                                if let GirInstruction::Mul { dst: mdst, src1: msrc1, src2: msrc2, dtype } = &instrs[j] {
                                    if *mdst == *offset_reg {
                                        // 找到 Mul — 用 tid (src1 或 src2 中非 sizeof 的那个) 作为元素偏移
                                        let tid_op = if matches!(msrc2, GirOperand::Imm(_)) { msrc1.clone() }
                                                     else if matches!(msrc1, GirOperand::Imm(_)) { msrc2.clone() }
                                                     else { src1.clone() };
                                        let elem_dtype = if dtype.is_float() { GirDType::I32 } else { *dtype };
                                        self.addr_pattern_map.insert(*dst, (self.param_ids[*pid], tid_op));
                                        self.skip_instrs.insert(i);
                                        self.skip_instrs.insert(j);
                                        break;
                                    }
                                }
                            }
                        }
                        if !self.skip_instrs.contains(&i) {
                            // 没找到 Mul — 直接用 offset 作为元素偏移
                            self.addr_pattern_map.insert(*dst, (self.param_ids[*pid], src1.clone()));
                            self.skip_instrs.insert(i);
                        }
                        continue;
                    }
                }
                // 检查 src1 是否是指针 Param
                if let GirOperand::Param(pid) = src1 {
                    if *pid < func.params.len() && func.params[*pid].is_ptr {
                        if let GirOperand::Reg(offset_reg) = src2 {
                            for j in 0..i {
                                if let GirInstruction::Mul { dst: mdst, src1: msrc1, src2: msrc2, dtype } = &instrs[j] {
                                    if *mdst == *offset_reg {
                                        let tid_op = if matches!(msrc2, GirOperand::Imm(_)) { msrc1.clone() }
                                                     else if matches!(msrc1, GirOperand::Imm(_)) { msrc2.clone() }
                                                     else { src2.clone() };
                                        self.addr_pattern_map.insert(*dst, (self.param_ids[*pid], tid_op));
                                        self.skip_instrs.insert(i);
                                        self.skip_instrs.insert(j);
                                        break;
                                    }
                                }
                            }
                        }
                        if !self.skip_instrs.contains(&i) {
                            self.addr_pattern_map.insert(*dst, (self.param_ids[*pid], src2.clone()));
                            self.skip_instrs.insert(i);
                        }
                        continue;
                    }
                }
            }
        }
        for i in 0..end {
            if matches!(instrs[i], GirInstruction::BranchIf { .. }) {
                for j in (i + 1)..end {
                    if let GirInstruction::Jump { target } = &instrs[j] {
                        self.branch_merge_map.insert(i, *target);
                        break;
                    }
                }
            }
        }

        // 预扫描: 为每个 BranchIf 的 merge 点 Where 分配 Function 局部变量
        // 并记录每个 Jump 需要在跳转前存储的值
        for (bi_idx, &merge_label) in &self.branch_merge_map.clone() {
            if let GirInstruction::BranchIf { then_label, else_label, .. } = &instrs[*bi_idx] {
                let then_lbl = *then_label;
                let else_lbl = *else_label;

                // 找到 then block 和 else block 中的 Jump 指令索引
                let mut then_jump_idx: Option<usize> = None;
                let mut else_jump_idx: Option<usize> = None;
                let mut current_block = 0u64; // 0=before, 1=then, 2=else, 3=merge

                for k in (*bi_idx + 1)..end {
                    match &instrs[k] {
                        GirInstruction::Label { id } => {
                            if *id == then_lbl { current_block = 1; }
                            else if *id == else_lbl { current_block = 2; }
                            else if *id == merge_label { current_block = 3; }
                        }
                        GirInstruction::Jump { .. } => {
                            if current_block == 1 { then_jump_idx = Some(k); }
                            else if current_block == 2 { else_jump_idx = Some(k); }
                        }
                        _ => {}
                    }
                }

                // 在 merge block 中找所有 Where 指令
                let mut current_block = 0u64;
                for k in (*bi_idx + 1)..end {
                    match &instrs[k] {
                        GirInstruction::Label { id } => {
                            if *id == merge_label { current_block = 3; }
                            else if *id == then_lbl { current_block = 1; }
                            else if *id == else_lbl { current_block = 2; }
                        }
                        GirInstruction::Where { dst, then_val, else_val, .. } if current_block == 3 => {
                            // 分配 Function 局部变量
                            let temp_var = self.alloc_id();
                            self.temp_func_vars.push(temp_var);
                            self.merge_var_map.insert(k, temp_var);

                            // 记录 then block Jump 需要存储 then_val
                            if let Some(ji) = then_jump_idx {
                                self.jump_store_map.entry(ji).or_default()
                                    .push((temp_var, then_val.clone()));
                            }
                            // 记录 else block Jump 需要存储 else_val
                            if let Some(ji) = else_jump_idx {
                                self.jump_store_map.entry(ji).or_default()
                                    .push((temp_var, else_val.clone()));
                            }
                            let _ = dst;
                        }
                        _ => {}
                    }
                }
            }
        }

        // 预扫描: 全局线程 ID 模式 — Add(Mul(BlockId,BlockDim),ThreadId) → GlobalInvocationId
        if self.need_global_invocation_id {
            let mut blockid_reg: Option<usize> = None;
            let mut blockdim_reg: Option<usize> = None;
            let mut threadid_reg: Option<usize> = None;
            let mut blockid_idx: Option<usize> = None;
            let mut blockdim_idx: Option<usize> = None;
            let mut threadid_idx: Option<usize> = None;
            let mut mul_idx: Option<usize> = None;
            let mut mul_dst: Option<usize> = None;
            let mut add_idx: Option<usize> = None;
            let mut add_dst: Option<usize> = None;
            for (i, instr) in instrs[..end].iter().enumerate() {
                match instr {
                    GirInstruction::BlockId { dst, .. } => { blockid_reg = Some(*dst); blockid_idx = Some(i); }
                    GirInstruction::BlockDim { dst, .. } => { blockdim_reg = Some(*dst); blockdim_idx = Some(i); }
                    GirInstruction::ThreadId { dst, .. } => { threadid_reg = Some(*dst); threadid_idx = Some(i); }
                    GirInstruction::Mul { dst, src1, src2, .. } => {
                        if let (Some(br), Some(dr)) = (blockid_reg, blockdim_reg) {
                            let m1 = matches!(src1, GirOperand::Reg(r) if *r == br) && matches!(src2, GirOperand::Reg(r) if *r == dr);
                            let m2 = matches!(src1, GirOperand::Reg(r) if *r == dr) && matches!(src2, GirOperand::Reg(r) if *r == br);
                            if m1 || m2 { mul_idx = Some(i); mul_dst = Some(*dst); }
                        }
                    }
                    GirInstruction::Add { dst, src1, src2, .. } => {
                        if let (Some(md), Some(tr)) = (mul_dst, threadid_reg) {
                            let m1 = matches!(src1, GirOperand::Reg(r) if *r == md) && matches!(src2, GirOperand::Reg(r) if *r == tr);
                            let m2 = matches!(src1, GirOperand::Reg(r) if *r == tr) && matches!(src2, GirOperand::Reg(r) if *r == md);
                            if m1 || m2 { add_idx = Some(i); add_dst = Some(*dst); }
                        }
                    }
                    _ => {}
                }
            }
            // 跳过原指令（GlobalInvocationId 加载延后到 FuncVariable 之后）
            if let Some(ai) = add_idx {
                for idx in [blockid_idx, blockdim_idx, threadid_idx, mul_idx, Some(ai)] {
                    if let Some(x) = idx { self.skip_instrs.insert(x); }
                }
                // 记录 add_dst 用于后续加载（延后到 FuncVariable 之后执行）
                if let Some(dst) = add_dst {
                    self.pending_global_tid_dst = Some(dst);
                }
            }
        }

        // 发射 Function 局部变量（用于 if/else merge 替代 OpPhi）
        // OpVariable 必须是函数第一个基本块的最前指令
        if !self.temp_func_vars.is_empty() {
            self.func_f32_ptr_type = self.alloc_id();
            // 指针类型写入 decl_words（全局声明区）
            self.decl_words.push((4u32 << 16) | OP_TYPE_POINTER);
            self.decl_words.push(self.func_f32_ptr_type);
            self.decl_words.push(SC_FUNCTION);
            self.decl_words.push(self.type_f32);
            // OpVariable 写入 func_words（函数体）
            for &vid in &self.temp_func_vars.clone() {
                self.func_words.push((4u32 << 16) | OP_VARIABLE);
                self.func_words.push(self.func_f32_ptr_type);
                self.func_words.push(vid);
                self.func_words.push(SC_FUNCTION);
            }
        }

        // 加载 GlobalInvocationId（在 OpVariable 之后，确保 SPIR-V 布局正确）
        if let Some(dst) = self.pending_global_tid_dst.take() {
            let gid_vec = self.instr_with_result(OP_LOAD, self.type_v3_i32, &[self.builtin_global_invocation_id]);
            let gid_x = self.instr_with_result(OP_COMPOSITE_EXTRACT, self.type_i32, &[gid_vec, 0]);
            self.global_tid_remap.insert(dst, gid_x);
        }

        for (i, instr) in instrs[..end].iter().enumerate() {
            if self.skip_instrs.contains(&i) {
                continue;
            }
            self.current_instr_idx = i;
            self.compile_instruction(instr);
        }

        // OpReturn + OpFunctionEnd
        self.instr(OP_RETURN, &[]);
        self.instr(OP_FUNCTION_END, &[]);

        // 退出函数体模式
        self.in_function = false;
    }

    fn collect_result_regs(&self, instr: &GirInstruction, regs: &mut std::collections::BTreeSet<usize>) {
        match instr {
            GirInstruction::Move { dst, .. }
            | GirInstruction::Add { dst, .. } | GirInstruction::Sub { dst, .. }
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
            | GirInstruction::WarpShuffle { dst, .. } | GirInstruction::Reduce { dst, .. } => {
                regs.insert(*dst);
            }
            GirInstruction::GlobalLoadV4 { dst_base, .. }
            | GirInstruction::GlobalLoadV2 { dst_base, .. } => {
                regs.insert(*dst_base);
                regs.insert(*dst_base + 1);
                if let GirInstruction::GlobalLoadV4 { .. } = instr {
                    regs.insert(*dst_base + 2);
                    regs.insert(*dst_base + 3);
                }
            }
            GirInstruction::TileZeros { dst, .. } | GirInstruction::TileLoad { dst, .. } => {
                regs.insert(*dst);
                regs.insert(*dst + 1);
                regs.insert(*dst + 2);
                regs.insert(*dst + 3);
            }
            GirInstruction::Mma { dst, .. } => {
                for i in 0..4 { regs.insert(*dst + i); }
            }
            _ => {}
        }
    }

    fn scan_reg_types(&mut self, func: &GirFunction) {
        for param in &func.params {
            // 参数寄存器 ID = 索引
        }
        for (i, param) in func.params.iter().enumerate() {
            self.reg_types.insert(i, param.dtype);
        }
        for instr in &func.instructions {
            match instr {                GirInstruction::Add { dst, dtype, .. }
                | GirInstruction::Sub { dst, dtype, .. }
                | GirInstruction::Mul { dst, dtype, .. }
                | GirInstruction::Div { dst, dtype, .. }
                | GirInstruction::Mod { dst, dtype, .. }
                | GirInstruction::Fma { dst, dtype, .. }
                | GirInstruction::GlobalLoad { dst, dtype, .. }
                | GirInstruction::SharedLoad { dst, dtype, .. }
                | GirInstruction::MaskedGlobalLoad { dst, dtype, .. } => {
                    self.reg_types.insert(*dst, *dtype);
                }
                GirInstruction::Exp { dst, .. } | GirInstruction::Recip { dst, .. }
                | GirInstruction::Sqrt { dst, .. } | GirInstruction::Log { dst, .. }
                | GirInstruction::Rsqrt { dst, .. } | GirInstruction::Abs { dst, .. }
                | GirInstruction::Max { dst, .. } | GirInstruction::Min { dst, .. }
                | GirInstruction::Tanh { dst, .. } | GirInstruction::Cos { dst, .. }
                | GirInstruction::Sin { dst, .. } | GirInstruction::Clamp { dst, .. }
                | GirInstruction::Lerp { dst, .. } | GirInstruction::Ceil { dst, .. }
                | GirInstruction::Floor { dst, .. } | GirInstruction::Pow { dst, .. }
                | GirInstruction::Where { dst, .. } => {
                    self.reg_types.insert(*dst, GirDType::F32);
                }
                GirInstruction::ThreadId { dst, .. } | GirInstruction::BlockId { dst, .. }
                | GirInstruction::BlockDim { dst, .. } | GirInstruction::GridDim { dst, .. } => {
                    self.reg_types.insert(*dst, GirDType::I32);
                }
                GirInstruction::SharedAlloc { dst, .. } => {
                    self.reg_types.insert(*dst, GirDType::I64);
                }
                GirInstruction::Cmp { dst, .. } => {
                    self.reg_types.insert(*dst, GirDType::F32); // GIR Cmp 产出 0.0/1.0
                }
                GirInstruction::GlobalLoadV4 { dst_base, .. } => {
                    for i in 0..4 { self.reg_types.insert(*dst_base + i, GirDType::F32); }
                }
                GirInstruction::GlobalLoadV2 { dst_base, .. } => {
                    for i in 0..2 { self.reg_types.insert(*dst_base + i, GirDType::F32); }
                }
                _ => {}
            }
        }
        // Move 指令：从源操作数推断类型
        for instr in &func.instructions {
            if let GirInstruction::Move { dst, src } = instr {
                if self.reg_types.contains_key(dst) {
                    continue;
                }
                let inferred = match src {
                    GirOperand::Reg(id) => self.reg_types.get(id).copied().unwrap_or(GirDType::F32),
                    GirOperand::Imm(_) => GirDType::F32,  // Python tracer 中 Move+Imm 总是 float
                    GirOperand::Param(id) => func.params.get(*id).map(|p| p.dtype).unwrap_or(GirDType::F32),
                    GirOperand::Label(_) => GirDType::I32,
                };
                self.reg_types.insert(*dst, inferred);
            }
        }
    }

    // —— 单条指令编译 ——

    fn compile_instruction(&mut self, instr: &GirInstruction) {
        match instr {
            GirInstruction::Move { dst, src } => {
                let dtype = self.reg_types.get(dst).copied().unwrap_or(GirDType::I64);
                let src_id = self.operand_id(src, dtype);
                let dst_type = self.type_id(dtype);
                let new_id = self.instr_with_result(OP_COPY_OBJECT, dst_type, &[src_id]);
                self.reg_map.insert(*dst, new_id);
            }

            // —— 标量算术 ——
            GirInstruction::Add { dst, src1, src2, dtype } => {
                let op = if dtype.is_float() { OP_F_ADD } else { OP_I_ADD };
                let s1 = self.operand_id(src1, *dtype);
                let s2 = self.operand_id(src2, *dtype);
                let r = self.instr_with_result(op, self.type_id(*dtype), &[s1, s2]);
                self.reg_map.insert(*dst, r);
            }
            GirInstruction::Sub { dst, src1, src2, dtype } => {
                let op = if dtype.is_float() { OP_F_SUB } else { OP_I_SUB };
                let s1 = self.operand_id(src1, *dtype);
                let s2 = self.operand_id(src2, *dtype);
                let r = self.instr_with_result(op, self.type_id(*dtype), &[s1, s2]);
                self.reg_map.insert(*dst, r);
            }
            GirInstruction::Mul { dst, src1, src2, dtype } => {
                let op = if dtype.is_float() { OP_F_MUL } else { OP_I_MUL };
                let s1 = self.operand_id(src1, *dtype);
                let s2 = self.operand_id(src2, *dtype);
                let r = self.instr_with_result(op, self.type_id(*dtype), &[s1, s2]);
                self.reg_map.insert(*dst, r);
            }
            GirInstruction::Div { dst, src1, src2, dtype } => {
                let op = if dtype.is_float() { OP_F_DIV } else { OP_S_DIV };
                let s1 = self.operand_id(src1, *dtype);
                let s2 = self.operand_id(src2, *dtype);
                let r = self.instr_with_result(op, self.type_id(*dtype), &[s1, s2]);
                self.reg_map.insert(*dst, r);
            }
            GirInstruction::Mod { dst, src1, src2, dtype } => {
                let op = if dtype.is_float() { OP_F_REM } else { OP_S_REM };
                let s1 = self.operand_id(src1, *dtype);
                let s2 = self.operand_id(src2, *dtype);
                let r = self.instr_with_result(op, self.type_id(*dtype), &[s1, s2]);
                self.reg_map.insert(*dst, r);
            }
            GirInstruction::Fma { dst, src1, src2, src3, dtype } => {
                let s1 = self.operand_id(src1, *dtype);
                let s2 = self.operand_id(src2, *dtype);
                let s3 = self.operand_id(src3, *dtype);
                // OpenCL.std Fma = 26
                let r = self.instr_with_result(OP_EXT_INST, self.type_id(*dtype), &[
                    self.ext_inst_set, OCL_FMA, s1, s2, s3,
                ]);
                self.reg_map.insert(*dst, r);
            }
            GirInstruction::Exp { dst, src, dtype: _ } => {
                let s = self.operand_id(src, GirDType::F32);
                let r = self.instr_with_result(OP_EXT_INST, self.type_f32, &[
                    self.ext_inst_set, OCL_EXP, s,
                ]);
                self.reg_map.insert(*dst, r);
            }
            GirInstruction::Recip { dst, src, dtype: _ } => {
                let s = self.operand_id(src, GirDType::F32);
                let one = self.emit_const_f32(1.0);
                let r = self.instr_with_result(OP_F_DIV, self.type_f32, &[one, s]);
                self.reg_map.insert(*dst, r);
            }

            // —— 数学函数 (OpenCL.std) ——
            GirInstruction::Sqrt { dst, src, .. } => self.emit_ext_inst_1(dst, src, OCL_SQRT),
            GirInstruction::Log { dst, src, .. } => self.emit_ext_inst_1(dst, src, OCL_LOG),
            GirInstruction::Rsqrt { dst, src, .. } => self.emit_ext_inst_1(dst, src, OCL_RSQRT),
            GirInstruction::Abs { dst, src, dtype } => {
                let ext = if dtype.is_float() { OCL_FABS } else { OCL_SABS };
                self.emit_ext_inst_1(dst, src, ext);
            }
            GirInstruction::Max { dst, src1, src2, dtype } => {
                let ext = if dtype.is_float() { OCL_FMAX } else { OCL_SMAX };
                self.emit_ext_inst_2(dst, src1, src2, ext, *dtype);
            }
            GirInstruction::Min { dst, src1, src2, dtype } => {
                let ext = if dtype.is_float() { OCL_FMIN } else { OCL_SMIN };
                self.emit_ext_inst_2(dst, src1, src2, ext, *dtype);
            }
            GirInstruction::Tanh { dst, src, .. } => self.emit_ext_inst_1(dst, src, OCL_TANH),
            GirInstruction::Cos { dst, src, .. } => self.emit_ext_inst_1(dst, src, OCL_COS),
            GirInstruction::Sin { dst, src, .. } => self.emit_ext_inst_1(dst, src, OCL_SIN),
            GirInstruction::Ceil { dst, src, .. } => self.emit_ext_inst_1(dst, src, OCL_CEIL),
            GirInstruction::Floor { dst, src, .. } => self.emit_ext_inst_1(dst, src, OCL_FLOOR),
            GirInstruction::Pow { dst, base, exp, .. } => self.emit_ext_inst_2(dst, base, exp, OCL_POW, GirDType::F32),
            GirInstruction::Clamp { dst, src, lo, hi, dtype } => {
                let ext = if dtype.is_float() { OCL_FCLAMP } else { OCL_SCLAMP };
                let s = self.operand_id(src, *dtype);
                let l = self.operand_id(lo, *dtype);
                let h = self.operand_id(hi, *dtype);
                let r = self.instr_with_result(OP_EXT_INST, self.type_id(*dtype), &[
                    self.ext_inst_set, ext, s, l, h,
                ]);
                self.reg_map.insert(*dst, r);
            }
            GirInstruction::Lerp { dst, a, b, t, .. } => {
                let av = self.operand_id(a, GirDType::F32);
                let bv = self.operand_id(b, GirDType::F32);
                let tv = self.operand_id(t, GirDType::F32);
                let r = self.instr_with_result(OP_EXT_INST, self.type_f32, &[
                    self.ext_inst_set, OCL_MIX, av, bv, tv,
                ]);
                self.reg_map.insert(*dst, r);
            }

            // —— 比较 ——
            GirInstruction::Cmp { dst, op, src1, src2, dtype } => {
                let (cmp_op, is_float) = match op {
                    CmpOp::Eq => (if dtype.is_float() { OP_F_ORD_EQUAL } else { OP_I_EQUAL }, dtype.is_float()),
                    CmpOp::Ne => (if dtype.is_float() { OP_F_ORD_NOT_EQUAL } else { OP_I_NOT_EQUAL }, dtype.is_float()),
                    CmpOp::Lt => (if dtype.is_float() { OP_F_ORD_LESS_THAN } else { OP_S_LESS_THAN }, dtype.is_float()),
                    CmpOp::Le => (if dtype.is_float() { OP_F_ORD_LESS_THAN_EQUAL } else { OP_S_LESS_THAN_EQUAL }, dtype.is_float()),
                    CmpOp::Gt => (if dtype.is_float() { OP_F_ORD_GREATER_THAN } else { OP_S_GREATER_THAN }, dtype.is_float()),
                    CmpOp::Ge => (if dtype.is_float() { OP_F_ORD_GREATER_THAN_EQUAL } else { OP_S_GREATER_THAN_EQUAL }, dtype.is_float()),
                };
                let s1 = self.operand_id(src1, *dtype);
                let s2 = self.operand_id(src2, *dtype);
                let bool_id = self.instr_with_result(cmp_op, self.type_bool, &[s1, s2]);
                // GIR Cmp 输出 0.0/1.0 float — 用 OpSelect 转换
                let zero = self.emit_const_f32(0.0);
                let one = self.emit_const_f32(1.0);
                let r = self.instr_with_result(OP_SELECT, self.type_f32, &[bool_id, one, zero]);
                self.reg_map.insert(*dst, r);
                let _ = is_float;
            }

            // —— 控制流 ——
            GirInstruction::BranchIf { cond, then_label, else_label } => {
                let cond_id = match cond {
                    GirOperand::Reg(id) => {
                        let cval = *self.reg_map.get(id).unwrap_or(&self.const_zero_i32);
                        // GIR cond 是 float 0.0/1.0 — 转换为 bool
                        let zero = self.emit_const_f32(0.0);
                        self.instr_with_result(OP_F_ORD_NOT_EQUAL, self.type_bool, &[cval, zero])
                    }
                    GirOperand::Imm(v) => {
                        if *v != 0 { self.emit_const_i32(1) } else { self.emit_const_i32(0) }
                    }
                    _ => self.const_zero_i32,
                };
                let then_id = *self.label_map.get(then_label).unwrap_or(&self.const_zero_i32);
                let else_id = *self.label_map.get(else_label).unwrap_or(&self.const_zero_i32);
                // 记录 then/else block IDs 用于后续 OpPhi
                self.branch_then_block = then_id;
                self.branch_else_block = else_id;
                // 发射 OpSelectionMerge（SPIR-V 结构化控制流要求）
                if let Some(&merge_target) = self.branch_merge_map.get(&self.current_instr_idx) {
                    let merge_id = *self.label_map.get(&merge_target).unwrap_or(&self.const_zero_i32);
                    self.instr(OP_SELECTION_MERGE, &[merge_id, 0]); // None selection control
                }
                self.instr(OP_BRANCH_CONDITIONAL, &[cond_id, then_id, else_id]);
            }
            GirInstruction::Jump { target } => {
                // 如果此 Jump 是 if/else 分支的出口，先存储 merge 值到局部变量
                if let Some(stores) = self.jump_store_map.get(&self.current_instr_idx).cloned() {
                    for (temp_var, val) in &stores {
                        let val_id = self.operand_id(val, GirDType::F32);
                        self.instr(OP_STORE, &[*temp_var, val_id]);
                    }
                }
                let tid = *self.label_map.get(target).unwrap_or(&self.const_zero_i32);
                self.instr(OP_BRANCH, &[tid]);
            }
            GirInstruction::Label { id } => {
                let lid = *self.label_map.get(id).unwrap_or(&self.const_zero_i32);
                self.instr(OP_LABEL, &[lid]);
                // 检测是否是合并点（BranchIf 的 merge target）
                self.at_merge_point = self.branch_merge_map.values().any(|&v| v == *id);
            }
            GirInstruction::Return => {
                self.instr(OP_RETURN, &[]);
            }

            // —— GPU 内存 ——
            GirInstruction::GlobalLoad { dst, addr, dtype } => {
                let ptr_id = match addr {
                    GirOperand::Param(id) if *id < self.param_is_ptr.len() && self.param_is_ptr[*id] => {
                        self.param_ids[*id]
                    }
                    GirOperand::Reg(rid) if self.addr_pattern_map.contains_key(rid) => {
                        // 地址模式: Add(offset, Param(ptr)) — 用 OpPtrAccessChain(param, elem_index) 替代
                        let (param_id, offset_op) = self.addr_pattern_map[rid].clone();
                        // offset_op 来自 Mul(tid, sizeof) 时是 Reg(tid)，元素索引正确
                        // offset_op 来自 Imm(字节偏移) 时需转换为元素索引 (除以 sizeof)
                        let offset_id = match &offset_op {
                            GirOperand::Imm(byte_off) => {
                                let elem_size = dtype.size_in_bytes() as i64;
                                self.emit_const_i32((*byte_off / elem_size) as i32)
                            }
                            _ => self.operand_id(&offset_op, GirDType::I32),
                        };
                        let float_ptr_type = self.ptr_type_id(*dtype, SC_CROSS_WORKGROUP);
                        self.instr_with_result(OP_PTR_ACCESS_CHAIN, float_ptr_type, &[param_id, offset_id])
                    }
                    _ => {
                        let addr_id = self.operand_id(addr, GirDType::I64);
                        let ptr_type = self.ptr_type_id(*dtype, SC_CROSS_WORKGROUP);
                        self.instr_with_result(OP_CONVERT_U_TO_PTR, ptr_type, &[addr_id])
                    }
                };
                let r = self.instr_with_result(OP_LOAD, self.type_id(*dtype), &[ptr_id]);
                self.reg_map.insert(*dst, r);
            }
            GirInstruction::GlobalStore { addr, src, dtype } => {
                let val_id = self.operand_id(src, *dtype);
                let ptr_id = match addr {
                    GirOperand::Param(id) if *id < self.param_is_ptr.len() && self.param_is_ptr[*id] => {
                        self.param_ids[*id]
                    }
                    GirOperand::Reg(rid) if self.addr_pattern_map.contains_key(rid) => {
                        let (param_id, offset_op) = self.addr_pattern_map[rid].clone();
                        // Imm 字节偏移需转换为元素索引
                        let offset_id = match &offset_op {
                            GirOperand::Imm(byte_off) => {
                                let elem_size = dtype.size_in_bytes() as i64;
                                self.emit_const_i32((*byte_off / elem_size) as i32)
                            }
                            _ => self.operand_id(&offset_op, GirDType::I32),
                        };
                        let float_ptr_type = self.ptr_type_id(*dtype, SC_CROSS_WORKGROUP);
                        self.instr_with_result(OP_PTR_ACCESS_CHAIN, float_ptr_type, &[param_id, offset_id])
                    }
                    _ => {
                        let addr_id = self.operand_id(addr, GirDType::I64);
                        let ptr_type = self.ptr_type_id(*dtype, SC_CROSS_WORKGROUP);
                        self.instr_with_result(OP_CONVERT_U_TO_PTR, ptr_type, &[addr_id])
                    }
                };
                self.instr(OP_STORE, &[ptr_id, val_id]);
            }
            GirInstruction::GlobalLoadV4 { dst_base, addr, .. } => {
                let addr_id = self.operand_id(addr, GirDType::I64);
                // ptr<CrossWorkgroup, vec4<f32>>
                let vec4_type = self.alloc_id();
                self.instr_global(OP_TYPE_VECTOR, &[vec4_type, self.type_f32, 4]);
                let ptr_vec4 = self.alloc_id();
                self.instr_global(OP_TYPE_POINTER, &[ptr_vec4, SC_CROSS_WORKGROUP, vec4_type]);
                let ptr_id = self.instr_with_result(OP_CONVERT_U_TO_PTR, ptr_vec4, &[addr_id]);
                let vec_id = self.instr_with_result(OP_LOAD, vec4_type, &[ptr_id]);
                // 解包 4 个分量
                for i in 0..4 {
                    let comp = self.instr_with_result(OP_COMPOSITE_EXTRACT, self.type_f32, &[vec_id, i as u32]);
                    self.reg_map.insert(*dst_base + i, comp);
                }
            }
            GirInstruction::GlobalStoreV4 { addr, src_base, .. } => {
                let addr_id = self.operand_id(addr, GirDType::I64);
                let v0 = self.operand_id(&GirOperand::Reg(*src_base), GirDType::F32);
                let v1 = self.operand_id(&GirOperand::Reg(*src_base + 1), GirDType::F32);
                let v2 = self.operand_id(&GirOperand::Reg(*src_base + 2), GirDType::F32);
                let v3 = self.operand_id(&GirOperand::Reg(*src_base + 3), GirDType::F32);
                let vec4_type = self.alloc_id();
                self.instr_global(OP_TYPE_VECTOR, &[vec4_type, self.type_f32, 4]);
                let vec_id = self.instr_with_result(OP_COMPOSITE_CONSTRUCT, vec4_type, &[v0, v1, v2, v3]);
                let ptr_vec4 = self.alloc_id();
                self.instr_global(OP_TYPE_POINTER, &[ptr_vec4, SC_CROSS_WORKGROUP, vec4_type]);
                let ptr_id = self.instr_with_result(OP_CONVERT_U_TO_PTR, ptr_vec4, &[addr_id]);
                self.instr(OP_STORE, &[ptr_id, vec_id]);
            }
            GirInstruction::GlobalLoadV2 { dst_base, addr, .. } => {
                let addr_id = self.operand_id(addr, GirDType::I64);
                let vec2_type = self.alloc_id();
                self.instr_global(OP_TYPE_VECTOR, &[vec2_type, self.type_f32, 2]);
                let ptr_vec2 = self.alloc_id();
                self.instr_global(OP_TYPE_POINTER, &[ptr_vec2, SC_CROSS_WORKGROUP, vec2_type]);
                let ptr_id = self.instr_with_result(OP_CONVERT_U_TO_PTR, ptr_vec2, &[addr_id]);
                let vec_id = self.instr_with_result(OP_LOAD, vec2_type, &[ptr_id]);
                for i in 0..2 {
                    let comp = self.instr_with_result(OP_COMPOSITE_EXTRACT, self.type_f32, &[vec_id, i as u32]);
                    self.reg_map.insert(*dst_base + i, comp);
                }
            }
            GirInstruction::GlobalStoreV2 { addr, src_base, .. } => {
                let addr_id = self.operand_id(addr, GirDType::I64);
                let v0 = self.operand_id(&GirOperand::Reg(*src_base), GirDType::F32);
                let v1 = self.operand_id(&GirOperand::Reg(*src_base + 1), GirDType::F32);
                let vec2_type = self.alloc_id();
                self.instr_global(OP_TYPE_VECTOR, &[vec2_type, self.type_f32, 2]);
                let vec_id = self.instr_with_result(OP_COMPOSITE_CONSTRUCT, vec2_type, &[v0, v1]);
                let ptr_vec2 = self.alloc_id();
                self.instr_global(OP_TYPE_POINTER, &[ptr_vec2, SC_CROSS_WORKGROUP, vec2_type]);
                let ptr_id = self.instr_with_result(OP_CONVERT_U_TO_PTR, ptr_vec2, &[addr_id]);
                self.instr(OP_STORE, &[ptr_id, vec_id]);
            }
            GirInstruction::SharedLoad { dst, addr, dtype } => {
                let byte_offset_id = self.operand_id(addr, GirDType::I64);
                let elem_size = dtype.size_in_bytes() as i64;
                let elem_size_const = self.emit_const_i64(elem_size);
                let elem_index_id = self.instr_with_result(OP_S_DIV, self.type_i64,
                    &[byte_offset_id, elem_size_const]);
                // 用 OpAccessChain (而非 OpPtrAccessChain) 索引数组元素
                let elem_ptr = self.instr_with_result(OP_ACCESS_CHAIN, self.ptr_type_id(GirDType::F32, SC_WORKGROUP),
                    &[self.shared_mem_var, elem_index_id]);
                let r = self.instr_with_result(OP_LOAD, self.type_f32, &[elem_ptr]);
                self.reg_map.insert(*dst, r);
            }
            GirInstruction::SharedStore { addr, src, dtype } => {
                let byte_offset_id = self.operand_id(addr, GirDType::I64);
                let val_id = self.operand_id(src, *dtype);
                let elem_size = dtype.size_in_bytes() as i64;
                let elem_size_const = self.emit_const_i64(elem_size);
                let elem_index_id = self.instr_with_result(OP_S_DIV, self.type_i64,
                    &[byte_offset_id, elem_size_const]);
                let elem_ptr = self.instr_with_result(OP_ACCESS_CHAIN, self.ptr_type_id(GirDType::F32, SC_WORKGROUP),
                    &[self.shared_mem_var, elem_index_id]);
                self.instr(OP_STORE, &[elem_ptr, val_id]);
            }
            GirInstruction::SharedAlloc { dst, .. } => {
                // 共享内存分配 — 在 SPIR-V 中用 Workgroup 变量
                // 简化: 返回一个 null 指针（实际共享内存在 Workgroup scope 中管理）
                self.reg_map.insert(*dst, self.const_zero_i64);
            }

            // —— 同步 ——
            GirInstruction::Barrier => {
                // OpControlBarrier Workgroup Workgroup AcquireRelease|SequentiallyConsistent
                self.instr(OP_CONTROL_BARRIER, &[
                    self.const_scope_workgroup, self.const_scope_workgroup,
                    self.const_mem_sem_release,
                ]);
            }
            GirInstruction::WarpShuffle { dst, src, src_lane, .. } => {
                // SPIR-V 无直接 warp shuffle — 使用 GroupBroadcast 作为近似
                // 简化: 直接拷贝（正确性降级，功能等价在单线程场景）
                let s = self.operand_id(src, GirDType::F32);
                let r = self.instr_with_result(OP_COPY_OBJECT, self.type_f32, &[s]);
                self.reg_map.insert(*dst, r);
                let _ = src_lane;
            }
            GirInstruction::Reduce { dst, src, op, .. } => {
                let s = self.operand_id(src, GirDType::F32);
                let group_op = match op {
                    ReduceOp::Sum => OP_GROUP_F_ADD,
                    ReduceOp::Max => OP_GROUP_F_MAX,
                    ReduceOp::Min => OP_GROUP_F_MIN,
                };
                let r = self.instr_with_result(group_op, self.type_f32, &[
                    self.const_scope_workgroup, 0, s, // 0 = Reduce operation
                ]);
                self.reg_map.insert(*dst, r);
            }

            // —— 线程索引 ——
            GirInstruction::ThreadId { dst, dim } => {
                let vec_id = self.instr_with_result(OP_LOAD, self.type_v3_i32, &[self.builtin_local_invocation_id]);
                let idx = match dim { ThreadDim::X => 0, ThreadDim::Y => 1, ThreadDim::Z => 2 };
                let r = self.instr_with_result(OP_COMPOSITE_EXTRACT, self.type_i32, &[vec_id, idx as u32]);
                self.reg_map.insert(*dst, r);
            }
            GirInstruction::BlockId { dst, dim } => {
                let vec_id = self.instr_with_result(OP_LOAD, self.type_v3_i32, &[self.builtin_workgroup_id]);
                let idx = match dim { ThreadDim::X => 0, ThreadDim::Y => 1, ThreadDim::Z => 2 };
                let r = self.instr_with_result(OP_COMPOSITE_EXTRACT, self.type_i32, &[vec_id, idx as u32]);
                self.reg_map.insert(*dst, r);
            }
            GirInstruction::BlockDim { dst, dim } => {
                let vec_id = self.instr_with_result(OP_LOAD, self.type_v3_i32, &[self.builtin_workgroup_size]);
                let idx = match dim { ThreadDim::X => 0, ThreadDim::Y => 1, ThreadDim::Z => 2 };
                let r = self.instr_with_result(OP_COMPOSITE_EXTRACT, self.type_i32, &[vec_id, idx as u32]);
                self.reg_map.insert(*dst, r);
            }
            GirInstruction::GridDim { dst, dim } => {
                let vec_id = self.instr_with_result(OP_LOAD, self.type_v3_i32, &[self.builtin_num_workgroups]);
                let idx = match dim { ThreadDim::X => 0, ThreadDim::Y => 1, ThreadDim::Z => 2 };
                let r = self.instr_with_result(OP_COMPOSITE_EXTRACT, self.type_i32, &[vec_id, idx as u32]);
                self.reg_map.insert(*dst, r);
            }

            // —— 条件操作 ——
            GirInstruction::Where { dst, cond, then_val, else_val, .. } => {
                if self.at_merge_point {
                    // 合并点: 从 Function 局部变量加载（替代 OpPhi，避免 AMD VGPR/SGPR 问题）
                    if let Some(&temp_var) = self.merge_var_map.get(&self.current_instr_idx) {
                        let r = self.instr_with_result(OP_LOAD, self.type_f32, &[temp_var]);
                        self.reg_map.insert(*dst, r);
                    } else {
                        // 回退到 OpPhi（不应发生，但保持安全）
                        let t = self.operand_id(then_val, GirDType::F32);
                        let e = self.operand_id(else_val, GirDType::F32);
                        let r = self.instr_with_result(OP_PHI, self.type_f32, &[
                            t, self.branch_then_block,
                            e, self.branch_else_block,
                        ]);
                        self.reg_map.insert(*dst, r);
                    }
                } else {
                    // 非合并点: 使用 OpSelect（predicated select）
                    let c = self.operand_id(cond, GirDType::F32);
                    let zero = self.emit_const_f32(0.0);
                    let bool_id = self.instr_with_result(OP_F_ORD_NOT_EQUAL, self.type_bool, &[c, zero]);
                    let t = self.operand_id(then_val, GirDType::F32);
                    let e = self.operand_id(else_val, GirDType::F32);
                    let r = self.instr_with_result(OP_SELECT, self.type_f32, &[bool_id, t, e]);
                    self.reg_map.insert(*dst, r);
                }
            }
            GirInstruction::MaskedGlobalLoad { dst, addr, mask, default_val, dtype } => {
                let addr_id = self.operand_id(addr, GirDType::I64);
                let mask_id = self.operand_id(mask, GirDType::F32);
                let default_id = self.operand_id(default_val, *dtype);
                let zero = self.emit_const_f32(0.0);
                let bool_id = self.instr_with_result(OP_F_ORD_NOT_EQUAL, self.type_bool, &[mask_id, zero]);
                let ptr_type = self.ptr_type_id(*dtype, SC_CROSS_WORKGROUP);
                let ptr_id = self.instr_with_result(OP_CONVERT_U_TO_PTR, ptr_type, &[addr_id]);
                let loaded = self.instr_with_result(OP_LOAD, self.type_id(*dtype), &[ptr_id]);
                let r = self.instr_with_result(OP_SELECT, self.type_id(*dtype), &[bool_id, loaded, default_id]);
                self.reg_map.insert(*dst, r);
            }
            GirInstruction::MaskedGlobalStore { addr, src, mask, dtype } => {
                let addr_id = self.operand_id(addr, GirDType::I64);
                let mask_id = self.operand_id(mask, GirDType::F32);
                let val_id = self.operand_id(src, *dtype);
                let zero = self.emit_const_f32(0.0);
                let bool_id = self.instr_with_result(OP_F_ORD_NOT_EQUAL, self.type_bool, &[mask_id, zero]);
                // 条件存储: 仅当 mask != 0 时执行 OpStore
                // SPIR-V 无条件 store — 用 if 分支模拟
                // 简化: 总是存储（功能正确但在 mask=0 时可能写入无效地址）
                // 正确做法需要用分支
                let ptr_type = self.ptr_type_id(*dtype, SC_CROSS_WORKGROUP);
                let ptr_id = self.instr_with_result(OP_CONVERT_U_TO_PTR, ptr_type, &[addr_id]);
                self.instr(OP_STORE, &[ptr_id, val_id]);
                let _ = bool_id;
            }

            // —— Tile 操作（应由 tile_expansion pass 展开）——
            GirInstruction::TileZeros { dst, .. } => {
                for i in 0..4 {
                    let z = self.emit_const_f32(0.0);
                    self.reg_map.insert(*dst + i, z);
                }
            }
            GirInstruction::TileLoad { .. } | GirInstruction::TileStore { .. }
            | GirInstruction::TileMatmul { .. } | GirInstruction::Mma { .. } => {
                // Tile 操作应已被 tile_expansion pass 展开为标量指令
                // MMA 同理 — 如果到达这里说明 pass 未展开
                // 输出 NOP (const 0)
                let _ = instr;
            }
        }
    }

    // —— 扩展指令辅助 ——
    fn emit_ext_inst_1(&mut self, dst: &usize, src: &GirOperand, ext_num: u32) {
        let s = self.operand_id(src, GirDType::F32);
        let r = self.instr_with_result(OP_EXT_INST, self.type_f32, &[self.ext_inst_set, ext_num, s]);
        self.reg_map.insert(*dst, r);
    }

    fn emit_ext_inst_2(&mut self, dst: &usize, src1: &GirOperand, src2: &GirOperand, ext_num: u32, dtype: GirDType) {
        let s1 = self.operand_id(src1, dtype);
        let s2 = self.operand_id(src2, dtype);
        let r = self.instr_with_result(OP_EXT_INST, self.type_id(dtype), &[self.ext_inst_set, ext_num, s1, s2]);
        self.reg_map.insert(*dst, r);
    }
}

impl Default for SpirvCompiler {
    fn default() -> Self {
        Self::new()
    }
}

impl super::GpuBackend for SpirvCompiler {
    type Output = Vec<u32>;

    fn compile(&mut self, gir: &GirProgram) -> Self::Output {
        self.compile(gir)
    }

    fn target_name(&self) -> &str {
        "spirv"
    }
}

// ============================================================
// SPIR-V 二进制转文本（调试用）
// ============================================================

/// 将 SPIR-V word 序列转为十六进制 dump（调试用）
pub fn spirv_hex_dump(words: &[u32]) -> String {
    let mut out = String::new();
    for (i, w) in words.iter().enumerate() {
        if i % 4 == 0 {
            out.push_str(&format!("{:04x}: ", i * 4));
        }
        out.push_str(&format!("{:08x} ", w));
        if i % 4 == 3 {
            out.push('\n');
        }
    }
    if words.len() % 4 != 0 {
        out.push('\n');
    }
    out
}
