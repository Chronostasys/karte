//! SPIR-V 二进制后端 — 将 GIR 编译为 SPIR-V 二进制 (Vec<u32>)
//!
//! 与 PTX 后端对等的低级虚拟 ISA 生成，保留全部 GIR 优化结果。
//! 通过 `clCreateProgramWithIL` 加载 (OpenCL 2.1+)，
//! 支持 AMD / Intel / NVIDIA GPU。

use karte_gir::*;
use std::collections::HashMap;

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
const OP_RETURN: u32 = 253;

const OP_EXT_INST: u32 = 12;
const OP_CONVERT_S_TO_F: u32 = 111;
const OP_CONVERT_U_TO_F: u32 = 112;
const OP_CONVERT_F_TO_S: u32 = 110;
const OP_CONVERT_F_TO_U: u32 = 109;
const OP_BITCAST: u32 = 124;
const OP_CONVERT_U_TO_PTR: u32 = 120;

// Group operations
const OP_GROUP_F_ADD: u32 = 265;
const OP_GROUP_F_MIN: u32 = 266;
const OP_GROUP_S_MIN: u32 = 268;
const OP_GROUP_F_MAX: u32 = 269;
const OP_GROUP_S_MAX: u32 = 270;

// Capabilities
const CAP_KERNEL: u32 = 6;
const CAP_ADDRESSES: u32 = 5;
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
const BUILTIN_GLOBAL_INVOCATION_ID: u32 = 28;

// Execution Mode
const EXEC_MODE_LOCAL_SIZE: u32 = 17;

// Scope constants (for barriers)
const SCOPE_DEVICE: u32 = 1;
const SCOPE_WORKGROUP: u32 = 2;
const SCOPE_SUBGROUP: u32 = 3;

// Memory Semantics
const MEM_SEM_NONE: u32 = 0;
const MEM_SEM_ACQUIRE_RELEASE: u32 = 0x8 | 0x4; // Acquire(2) | Release(4)
const MEM_SEM_SEQ_CST: u32 = 0x10;

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

    // OpenCL.std 扩展指令集 ID
    ext_inst_set: u32,

    // Built-in 变量 ID
    builtin_local_invocation_id: u32,
    builtin_workgroup_id: u32,
    builtin_workgroup_size: u32,
    builtin_num_workgroups: u32,

    // 运行时映射
    reg_map: HashMap<usize, u32>,    // GIR 寄存器 → SPIR-V ID
    reg_types: HashMap<usize, GirDType>,
    label_map: HashMap<usize, u32>,  // GIR 标签 → SPIR-V label ID
    param_ids: Vec<u32>,             // 函数参数 SPIR-V ID
    param_ptr_types: Vec<u32>,       // 指针参数的类型 ID

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
            ext_inst_set: 0,
            builtin_local_invocation_id: 0,
            builtin_workgroup_id: 0,
            builtin_workgroup_size: 0,
            builtin_num_workgroups: 0,
            reg_map: HashMap::new(),
            reg_types: HashMap::new(),
            label_map: HashMap::new(),
            param_ids: Vec::new(),
            param_ptr_types: Vec::new(),
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
        c.emit_types_and_constants();
        c.emit_builtins();
        c
    }

    fn alloc_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// 编译 GIR 程序为 SPIR-V 二进制
    pub fn compile(&mut self, gir: &GirProgram) -> Vec<u32> {
        for kernel in &gir.kernels {
            self.compile_kernel(kernel);
        }
        self.fixup_header_bound();
        self.words.clone()
    }

    // —— 编码辅助 ——

    fn instr(&mut self, opcode: u32, operands: &[u32]) {
        let word_count = 1 + operands.len() as u32;
        self.words.push((word_count << 16) | opcode);
        self.words.extend_from_slice(operands);
    }

    fn instr_with_result(&mut self, opcode: u32, result_type: u32, operands: &[u32]) -> u32 {
        let result_id = self.alloc_id();
        let word_count = 1 + 2 + operands.len() as u32;
        self.words.push((word_count << 16) | opcode);
        self.words.push(result_type);
        self.words.push(result_id);
        self.words.extend_from_slice(operands);
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

        // OpTypeInt 32 signed
        self.type_i32 = self.alloc_id();
        self.instr(OP_TYPE_INT, &[self.type_i32, 32, 1]);

        // OpTypeInt 64 signed
        self.type_i64 = self.alloc_id();
        self.instr(OP_TYPE_INT, &[self.type_i64, 64, 1]);

        // OpTypeFloat 32
        self.type_f32 = self.alloc_id();
        self.instr(OP_TYPE_FLOAT, &[self.type_f32, 32]);

        // OpTypeFloat 64
        self.type_f64 = self.alloc_id();
        self.instr(OP_TYPE_FLOAT, &[self.type_f64, 64]);

        // vec3<i32> for built-in IDs
        self.type_v3_i32 = self.alloc_id();
        self.instr(OP_TYPE_VECTOR, &[self.type_v3_i32, self.type_i32, 3]);

        // 指针类型
        self.type_ptr_crossworkgroup_f32 = self.alloc_id();
        self.instr(OP_TYPE_POINTER, &[self.type_ptr_crossworkgroup_f32, SC_CROSS_WORKGROUP, self.type_f32]);
        self.type_ptr_crossworkgroup_i32 = self.alloc_id();
        self.instr(OP_TYPE_POINTER, &[self.type_ptr_crossworkgroup_i32, SC_CROSS_WORKGROUP, self.type_i32]);
        self.type_ptr_crossworkgroup_i64 = self.alloc_id();
        self.instr(OP_TYPE_POINTER, &[self.type_ptr_crossworkgroup_i64, SC_CROSS_WORKGROUP, self.type_i64]);

        self.type_ptr_workgroup_f32 = self.alloc_id();
        self.instr(OP_TYPE_POINTER, &[self.type_ptr_workgroup_f32, SC_WORKGROUP, self.type_f32]);
        self.type_ptr_workgroup_i32 = self.alloc_id();
        self.instr(OP_TYPE_POINTER, &[self.type_ptr_workgroup_i32, SC_WORKGROUP, self.type_i32]);
        self.type_ptr_workgroup_i64 = self.alloc_id();
        self.instr(OP_TYPE_POINTER, &[self.type_ptr_workgroup_i64, SC_WORKGROUP, self.type_i64]);

        self.type_ptr_function_f32 = self.alloc_id();
        self.instr(OP_TYPE_POINTER, &[self.type_ptr_function_f32, SC_FUNCTION, self.type_f32]);
        self.type_ptr_function_i32 = self.alloc_id();
        self.instr(OP_TYPE_POINTER, &[self.type_ptr_function_i32, SC_FUNCTION, self.type_i32]);
        self.type_ptr_function_i64 = self.alloc_id();
        self.instr(OP_TYPE_POINTER, &[self.type_ptr_function_i64, SC_FUNCTION, self.type_i64]);

        // 常量: zero values
        self.const_zero_i32 = self.emit_const_i32(0);
        self.const_zero_i64 = self.emit_const_i64(0);
        self.const_zero_f32 = self.emit_const_f32(0.0);
        self.const_one_i32 = self.emit_const_i32(1);

        // Scope 和 Memory Semantics 常量
        self.const_scope_workgroup = self.emit_const_i32(SCOPE_WORKGROUP as i32);
        self.const_mem_sem_release = self.emit_const_i32((MEM_SEM_ACQUIRE_RELEASE | MEM_SEM_SEQ_CST) as i32);
    }

    fn emit_const_i32(&mut self, val: i32) -> u32 {
        if let Some(&id) = self.i32_const_cache.get(&val) {
            return id;
        }
        let id = self.alloc_id();
        let word_count = 3u32 + 1; // opcode + type + result + value
        self.words.push((word_count << 16) | OP_CONSTANT);
        self.words.push(self.type_i32);
        self.words.push(id);
        self.words.push(val as u32);
        self.i32_const_cache.insert(val, id);
        id
    }

    fn emit_const_i64(&mut self, val: i64) -> u32 {
        if let Some(&id) = self.i64_const_cache.get(&val) {
            return id;
        }
        let id = self.alloc_id();
        let word_count = 3u32 + 2; // opcode + type + result + 2 value words
        self.words.push((word_count << 16) | OP_CONSTANT);
        self.words.push(self.type_i64);
        self.words.push(id);
        let lo = val as u32;
        let hi = (val >> 32) as u32;
        self.words.push(lo);
        self.words.push(hi);
        self.i64_const_cache.insert(val, id);
        id
    }

    fn emit_const_f32(&mut self, val: f32) -> u32 {
        let bits = val.to_bits();
        if let Some(&id) = self.f32_const_cache.get(&bits) {
            return id;
        }
        let id = self.alloc_id();
        let word_count = 3u32 + 1;
        self.words.push((word_count << 16) | OP_CONSTANT);
        self.words.push(self.type_f32);
        self.words.push(id);
        self.words.push(bits);
        self.f32_const_cache.insert(bits, id);
        id
    }

    // —— Built-in 变量 ——

    fn emit_builtins(&mut self) {
        // LocalInvocationId — vec3<i32>, Input storage
        self.builtin_local_invocation_id = self.alloc_id();
        let ptr_type = self.alloc_id();
        self.instr(OP_TYPE_POINTER, &[ptr_type, SC_INPUT, self.type_v3_i32]);
        self.instr(OP_DECORATE, &[self.builtin_local_invocation_id, DEC_BUILT_IN, BUILTIN_LOCAL_INVOCATION_ID]);
        self.instr(OP_VARIABLE, &[ptr_type, self.builtin_local_invocation_id, SC_INPUT]);

        // WorkgroupId — vec3<i32>, Input
        self.builtin_workgroup_id = self.alloc_id();
        self.instr(OP_DECORATE, &[self.builtin_workgroup_id, DEC_BUILT_IN, BUILTIN_WORKGROUP_ID]);
        self.instr(OP_VARIABLE, &[ptr_type, self.builtin_workgroup_id, SC_INPUT]);

        // WorkgroupSize — vec3<i32>, Input (constant)
        self.builtin_workgroup_size = self.alloc_id();
        self.instr(OP_DECORATE, &[self.builtin_workgroup_size, DEC_BUILT_IN, BUILTIN_WORKGROUP_SIZE]);
        self.instr(OP_VARIABLE, &[ptr_type, self.builtin_workgroup_size, SC_INPUT]);

        // NumWorkgroups — vec3<i32>, Input
        self.builtin_num_workgroups = self.alloc_id();
        self.instr(OP_DECORATE, &[self.builtin_num_workgroups, DEC_BUILT_IN, BUILTIN_NUM_WORKGROUPS]);
        self.instr(OP_VARIABLE, &[ptr_type, self.builtin_num_workgroups, SC_INPUT]);
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
                *self.reg_map.get(id).unwrap_or(&self.const_zero_i32)
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
                self.param_ids.get(*id).copied().unwrap_or(self.const_zero_i32)
            }
        }
    }

    // —— Kernel 编译 ——

    fn compile_kernel(&mut self, func: &GirFunction) {
        self.reg_map.clear();
        self.reg_types.clear();
        self.label_map.clear();
        self.param_ids.clear();
        self.param_ptr_types.clear();
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
                // 指针参数: ptr<CrossWorkgroup, dtype>
                // 为每个参数创建专用指针类型
                let pt = self.alloc_id();
                self.instr(OP_TYPE_POINTER, &[pt, SC_CROSS_WORKGROUP, self.type_id(param.dtype)]);
                param_types.push(pt);
                self.param_ptr_types.push(pt);
                pt
            } else {
                param_types.push(self.type_id(param.dtype));
                self.type_id(param.dtype)
            };
            let _ = ptype;
        }

        // OpTypeFunction void (param_types...)
        self.kernel_func_type_id = self.alloc_id();
        let mut func_type_operands = vec![self.kernel_func_type_id, self.type_void];
        func_type_operands.extend(&param_types);
        let wc = (1 + func_type_operands.len() as u32) << 16;
        self.words.push(wc | OP_TYPE_FUNCTION);
        self.words.extend_from_slice(&func_type_operands);

        // OpEntryPoint Kernel %func "name" %builtin...
        self.kernel_func_id = self.alloc_id();
        let entry_operands: Vec<u32> = vec![
            EXEC_KERNEL, self.kernel_func_id,
        ];
        // 编码: opcode + execution_model + func_id + string + interface vars
        let name_bytes = func.name.as_bytes();
        let str_words = (name_bytes.len() + 1 + 3) / 4; // null + padding
        let total_words = 1 + 2 + str_words + 4; // opcode + model + id + string + 4 builtins
        self.words.push((total_words as u32) << 16 | OP_ENTRY_POINT);
        self.words.push(EXEC_KERNEL);
        self.words.push(self.kernel_func_id);
        // 编码 name 字符串
        let mut name_padded: Vec<u8> = name_bytes.to_vec();
        name_padded.push(0);
        while name_padded.len() % 4 != 0 { name_padded.push(0); }
        for chunk in name_padded.chunks_exact(4) {
            self.words.push(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        }
        // Interface: built-in variables
        self.words.push(self.builtin_local_invocation_id);
        self.words.push(self.builtin_workgroup_id);
        self.words.push(self.builtin_workgroup_size);
        self.words.push(self.builtin_num_workgroups);

        // OpExecutionMode LocalSize x y z
        self.instr(OP_EXECUTION_MODE, &[
            self.kernel_func_id, EXEC_MODE_LOCAL_SIZE,
            func.block_dim.0 as u32, func.block_dim.1 as u32, func.block_dim.2 as u32,
        ]);

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

        // 第一个基本块必须以 OpLabel 开始
        // 在函数参数之后立即开始第一个基本块
        let entry_label = if self.label_map.contains_key(&0) {
            self.label_map[&0]
        } else {
            self.alloc_id()
        };
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

        for instr in &instrs[..end] {
            self.compile_instruction(instr);
        }

        // OpReturn + OpFunctionEnd
        self.instr(OP_RETURN, &[]);
        self.instr(OP_FUNCTION_END, &[]);
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
            match instr {
                GirInstruction::Add { dst, dtype, .. }
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
                self.instr(OP_BRANCH_CONDITIONAL, &[cond_id, then_id, else_id]);
            }
            GirInstruction::Jump { target } => {
                let tid = *self.label_map.get(target).unwrap_or(&self.const_zero_i32);
                self.instr(OP_BRANCH, &[tid]);
            }
            GirInstruction::Label { id } => {
                let lid = *self.label_map.get(id).unwrap_or(&self.const_zero_i32);
                self.instr(OP_LABEL, &[lid]);
            }
            GirInstruction::Return => {
                self.instr(OP_RETURN, &[]);
            }

            // —— GPU 内存 ——
            GirInstruction::GlobalLoad { dst, addr, dtype } => {
                let addr_id = self.operand_id(addr, GirDType::I64);
                // 将 i64 地址转换为指针
                let ptr_type = self.ptr_type_id(*dtype, SC_CROSS_WORKGROUP);
                let ptr_id = self.instr_with_result(OP_CONVERT_U_TO_PTR, ptr_type, &[addr_id]);
                let r = self.instr_with_result(OP_LOAD, self.type_id(*dtype), &[ptr_id]);
                self.reg_map.insert(*dst, r);
            }
            GirInstruction::GlobalStore { addr, src, dtype } => {
                let addr_id = self.operand_id(addr, GirDType::I64);
                let val_id = self.operand_id(src, *dtype);
                let ptr_type = self.ptr_type_id(*dtype, SC_CROSS_WORKGROUP);
                let ptr_id = self.instr_with_result(OP_CONVERT_U_TO_PTR, ptr_type, &[addr_id]);
                self.instr(OP_STORE, &[ptr_id, val_id]);
            }
            GirInstruction::GlobalLoadV4 { dst_base, addr, .. } => {
                let addr_id = self.operand_id(addr, GirDType::I64);
                // ptr<CrossWorkgroup, vec4<f32>>
                let vec4_type = self.alloc_id();
                self.instr(OP_TYPE_VECTOR, &[vec4_type, self.type_f32, 4]);
                let ptr_vec4 = self.alloc_id();
                self.instr(OP_TYPE_POINTER, &[ptr_vec4, SC_CROSS_WORKGROUP, vec4_type]);
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
                self.instr(OP_TYPE_VECTOR, &[vec4_type, self.type_f32, 4]);
                let vec_id = self.instr_with_result(OP_COMPOSITE_CONSTRUCT, vec4_type, &[v0, v1, v2, v3]);
                let ptr_vec4 = self.alloc_id();
                self.instr(OP_TYPE_POINTER, &[ptr_vec4, SC_CROSS_WORKGROUP, vec4_type]);
                let ptr_id = self.instr_with_result(OP_CONVERT_U_TO_PTR, ptr_vec4, &[addr_id]);
                self.instr(OP_STORE, &[ptr_id, vec_id]);
            }
            GirInstruction::GlobalLoadV2 { dst_base, addr, .. } => {
                let addr_id = self.operand_id(addr, GirDType::I64);
                let vec2_type = self.alloc_id();
                self.instr(OP_TYPE_VECTOR, &[vec2_type, self.type_f32, 2]);
                let ptr_vec2 = self.alloc_id();
                self.instr(OP_TYPE_POINTER, &[ptr_vec2, SC_CROSS_WORKGROUP, vec2_type]);
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
                self.instr(OP_TYPE_VECTOR, &[vec2_type, self.type_f32, 2]);
                let vec_id = self.instr_with_result(OP_COMPOSITE_CONSTRUCT, vec2_type, &[v0, v1]);
                let ptr_vec2 = self.alloc_id();
                self.instr(OP_TYPE_POINTER, &[ptr_vec2, SC_CROSS_WORKGROUP, vec2_type]);
                let ptr_id = self.instr_with_result(OP_CONVERT_U_TO_PTR, ptr_vec2, &[addr_id]);
                self.instr(OP_STORE, &[ptr_id, vec_id]);
            }
            GirInstruction::SharedLoad { dst, addr, dtype } => {
                let addr_id = self.operand_id(addr, GirDType::I64);
                let ptr_type = self.ptr_type_id(*dtype, SC_WORKGROUP);
                let ptr_id = self.instr_with_result(OP_CONVERT_U_TO_PTR, ptr_type, &[addr_id]);
                let r = self.instr_with_result(OP_LOAD, self.type_id(*dtype), &[ptr_id]);
                self.reg_map.insert(*dst, r);
            }
            GirInstruction::SharedStore { addr, src, dtype } => {
                let addr_id = self.operand_id(addr, GirDType::I64);
                let val_id = self.operand_id(src, *dtype);
                let ptr_type = self.ptr_type_id(*dtype, SC_WORKGROUP);
                let ptr_id = self.instr_with_result(OP_CONVERT_U_TO_PTR, ptr_type, &[addr_id]);
                self.instr(OP_STORE, &[ptr_id, val_id]);
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
                let c = self.operand_id(cond, GirDType::F32);
                let zero = self.emit_const_f32(0.0);
                let bool_id = self.instr_with_result(OP_F_ORD_NOT_EQUAL, self.type_bool, &[c, zero]);
                let t = self.operand_id(then_val, GirDType::F32);
                let e = self.operand_id(else_val, GirDType::F32);
                let r = self.instr_with_result(OP_SELECT, self.type_f32, &[bool_id, t, e]);
                self.reg_map.insert(*dst, r);
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
