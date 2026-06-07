//! AOT 编译器 — 编排编译流程
//!
//! 接收 LirProgram，复用现有 JIT 编译器后端生成机器码，
//! 与运行时代码合并，生成 ELF 可执行文件。

use crate::elf::{ElfArch, ElfWriter};
use crate::runtime_x86::X86Runtime;
use crate::runtime_riscv::RiscvRuntime;

use karte_codegen::vm::professional_executor::jit::{
    JitCompiler, X86Compiler,
};
use karte_lir::LirProgram;

use std::collections::HashMap;
use std::io::Write;

/// AOT 目标架构
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AotTarget {
    X86_64,
    AArch64,
    Riscv64,
}

impl Default for AotTarget {
    fn default() -> Self {
        #[cfg(target_arch = "x86_64")]
        { Self::X86_64 }
        #[cfg(target_arch = "aarch64")]
        { Self::AArch64 }
        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        { Self::X86_64 }
    }
}

/// AOT 编译器
pub struct AotCompiler {
    debug: bool,
    target: AotTarget,
}

impl AotCompiler {
    pub fn new(debug: bool) -> Self {
        Self { debug, target: AotTarget::default() }
    }

    pub fn with_target(mut self, target: AotTarget) -> Self {
        self.target = target;
        self
    }

    /// 编译 LirProgram 为字节
    pub fn compile_to_bytes(&self, program: &LirProgram) -> Result<Vec<u8>, String> {
        match self.target {
            AotTarget::X86_64 => {
                #[cfg(target_arch = "x86_64")]
                return self.compile_x86_64(program);
                #[cfg(not(target_arch = "x86_64"))]
                return Err("x86_64 AOT 编译需要 x86_64 主机".to_string());
            }
            AotTarget::AArch64 => {
                #[cfg(target_arch = "aarch64")]
                return self.compile_aarch64(program);
                #[cfg(not(target_arch = "aarch64"))]
                return Err("AArch64 AOT 编译尚未实现".to_string());
            }
            AotTarget::Riscv64 => {
                return self.compile_riscv64(program);
            }
        }
    }

    /// x86_64 AOT 编译
    fn compile_x86_64(&self, program: &LirProgram) -> Result<Vec<u8>, String> {
        use std::collections::{HashSet, VecDeque};
        use karte_codegen::vm::professional_executor::jit::ffi::RuntimeIntrinsic;
        use karte_lir::ir::Instruction;

        // ---- 0. Tree-shaking: 只保留从入口可达的 std 库函数 ----
        //
        // 背景：LIR 优化后，用户代码中的函数调用已被内联/展开为 Jump 指令，
        // 跳转目标可能指向优化后的生成代码块而非原始函数入口，导致静态分析
        // 无法追踪用户代码间的调用关系。
        //
        // 策略：
        // 1. 用户代码（main::xxx）全部保留（保守安全）
        // 2. std 库函数通过 LoadGlobal + Call/Jump 引用追踪（可静态分析）
        // 3. BFS 从入口函数出发，标记可达的 std 库函数
        //
        // 性能：O(F * I) 建图 + O(F + E) BFS，其中 F=函数数，I=指令数，E=调用边数

        let main_name = program.main_function.as_deref().unwrap_or("main");
        let entry_func_name = program.functions.keys()
            .find(|k: &&String| k.ends_with(&format!("::{}", main_name)) || k.as_str() == main_name)
            .cloned()
            .unwrap_or_else(|| main_name.to_string());

        // 步骤 1: 区分用户函数和 std 库函数
        let user_funcs: HashSet<&str> = program.functions.keys()
            .filter(|k| !k.starts_with("std."))
            .map(|s| s.as_str())
            .collect();
        let std_funcs: HashSet<&str> = program.functions.keys()
            .filter(|k| k.starts_with("std."))
            .map(|s| s.as_str())
            .collect();

        // 步骤 2: 建立 LabelId → std 函数名 映射（只追踪 std 库）
        let mut std_label_to_func: HashMap<usize, &str> = HashMap::new();
        for &name in &std_funcs {
            if let Some(func) = program.functions.get(name) {
                for instr in &func.instructions {
                    if let Instruction::Label { id, .. } = instr {
                        std_label_to_func.insert(id.0, name);
                    }
                }
            }
        }

        // 步骤 3: 短名 → canonical name 映射（用于 LoadGlobal）
        let std_short_to_canonical: HashMap<&str, &str> = std_funcs.iter()
            .filter_map(|&k| k.rsplit_once("::").map(|(_, short)| (short, k)))
            .collect();

        // 步骤 4: 扫描所有用户函数的指令，收集引用的 std 库函数
        let mut call_graph: HashMap<&str, HashSet<&str>> = HashMap::new();
        for (caller_name, func) in &program.functions {
            let mut callees = HashSet::new();
            for instr in &func.instructions {
                // 直接调用/跳转到 std 函数
                let target_id = match instr {
                    Instruction::Call { target, .. } => Some(target.0),
                    Instruction::Jump { target, .. } => Some(target.0),
                    Instruction::JumpEqual { target, .. } => Some(target.0),
                    Instruction::JumpNotEqual { target, .. } => Some(target.0),
                    Instruction::JumpGreater { target, .. } => Some(target.0),
                    Instruction::JumpGreaterEqual { target, .. } => Some(target.0),
                    Instruction::JumpLess { target, .. } => Some(target.0),
                    Instruction::JumpLessEqual { target, .. } => Some(target.0),
                    _ => None,
                };
                if let Some(id) = target_id {
                    if let Some(&callee) = std_label_to_func.get(&id) {
                        callees.insert(callee);
                    }
                }
                // LoadGlobal 加载 std 函数地址
                if let Instruction::LoadGlobal { name, .. } = instr {
                    if std_funcs.contains(name.as_str()) {
                        callees.insert(name.as_str());
                    } else if let Some(&callee) = std_short_to_canonical.get(name.as_str()) {
                        callees.insert(callee);
                    }
                }
            }
            call_graph.insert(caller_name.as_str(), callees);
        }

        // 步骤 5: BFS 从入口出发 + 所有用户函数作为根
        // 用户函数全部保留，只对 std 库做 tree-shaking
        let mut reachable: HashSet<String> = user_funcs.iter().map(|&s| s.to_string()).collect();
        let mut queue: VecDeque<String> = VecDeque::new();
        // 将所有用户函数作为 BFS 起点（它们可能引用 std 函数）
        for &name in &user_funcs {
            queue.push_back(name.to_string());
        }
        // 也要追踪 std 函数之间的引用
        while let Some(current) = queue.pop_front() {
            if let Some(callees) = call_graph.get(current.as_str()) {
                for &callee in callees {
                    if !reachable.contains(callee) {
                        reachable.insert(callee.to_string());
                        queue.push_back(callee.to_string());
                    }
                }
            }
        }

        let total = program.functions.len();
        let kept = reachable.len();
        if self.debug {
            eprintln!("AOT: main_function='{}', entry_func_name='{}'", main_name, entry_func_name);
        }
        if self.debug || kept < total {
            eprintln!("AOT: Tree-shaking: {}/{} 函数可达 (移除 {} 个死函数, 保留 {} 用户 + {} std)",
                kept, total, total - kept, user_funcs.len(), kept - user_funcs.len());
        }

        // ---- 1. 只编译可达的 Karte 函数 ----
        let mut compiler = X86Compiler::new(self.debug)
            .map_err(|e| format!("创建 x86 编译器失败: {}", e))?;

        let mut compiled_functions = Vec::new();
        let mut karte_code: Vec<u8> = Vec::new();
        let mut function_offsets: HashMap<String, usize> = HashMap::new();

        // 编译顺序: 入口函数放最后，只编译可达函数
        let mut func_names: Vec<String> = reachable.into_iter().collect();
        func_names.sort_by(|a, b| {
            if a == &entry_func_name { std::cmp::Ordering::Greater }
            else if b == &entry_func_name { std::cmp::Ordering::Less }
            else { a.cmp(b) }
        });

        for func_name in &func_names {
            let func = program.functions.get(func_name).unwrap();
            let compiled = compiler.compile_function(func, program)
                .map_err(|e| format!("编译函数 '{}' 失败: {}", func_name, e))?;

            // 对齐到 16 字节
            while karte_code.len() % 16 != 0 {
                karte_code.push(0x90);
            }

            let offset = karte_code.len();
            function_offsets.insert(func_name.clone(), offset);
            let code_size = compiled.machine_code().len();
            karte_code.extend_from_slice(compiled.machine_code());
            compiled_functions.push((func_name.clone(), compiled));

            if self.debug {
                eprintln!("AOT: 函数 '{}' @ offset={}, size={}",
                    func_name, offset, code_size);
            }
        }

        // ---- 2. 扫描 Karte 代码，收集需要的 runtime intrinsic ----
        // 扫描 movabs rax, <ptr>; call rax 模式，提取所有 JIT 函数指针
        let mut needed_jit_ptrs: HashSet<u64> = HashSet::new();
        let mut i = 0;
        while i + 12 <= karte_code.len() {
            if karte_code[i] == 0x48 && karte_code[i + 1] == 0xB8 &&
               karte_code[i + 10] == 0xFF && karte_code[i + 11] == 0xD0 {
                let addr = u64::from_le_bytes(karte_code[i + 2..i + 10].try_into().unwrap());
                if addr > 0x100000000 {
                    needed_jit_ptrs.insert(addr);
                }
            }
            i += 1;
        }

        // 将 JIT 指针映射到 intrinsic 名称
        let intrinsic_names: Vec<(&'static str, u64)> = [
            (RuntimeIntrinsic::AllocAligned, "karte_jit_runtime_alloc_aligned"),
            (RuntimeIntrinsic::Free, "karte_jit_runtime_free"),
            (RuntimeIntrinsic::Retain, "karte_jit_runtime_retain"),
            (RuntimeIntrinsic::Release, "karte_jit_runtime_release"),
            (RuntimeIntrinsic::GcSafepoint, "karte_jit_runtime_gc_safepoint"),
            (RuntimeIntrinsic::GcAlloc, "karte_jit_runtime_gc_alloc"),
            (RuntimeIntrinsic::RawSyscall6, "karte_jit_runtime_raw_syscall6"),
            (RuntimeIntrinsic::MemLoad64, "karte_jit_runtime_mem_load64"),
            (RuntimeIntrinsic::MemStore64, "karte_jit_runtime_mem_store64"),
            (RuntimeIntrinsic::StringEqual, "karte_jit_runtime_string_equal"),
            (RuntimeIntrinsic::StringCompare, "karte_jit_runtime_string_compare"),
            (RuntimeIntrinsic::StringConcat, "karte_jit_runtime_string_concat"),
            (RuntimeIntrinsic::StringCharAt, "karte_jit_runtime_string_char_at"),
            (RuntimeIntrinsic::CharToString, "karte_jit_runtime_char_to_string"),
            (RuntimeIntrinsic::StringSubstring, "karte_jit_runtime_string_substring"),
            (RuntimeIntrinsic::StringContains, "karte_jit_runtime_string_contains"),
            (RuntimeIntrinsic::SplitCount, "karte_jit_runtime_split_count"),
            (RuntimeIntrinsic::Trim, "karte_jit_runtime_trim"),
            (RuntimeIntrinsic::ToString, "karte_jit_runtime_to_string"),
            (RuntimeIntrinsic::PrintString, "karte_jit_runtime_print_string"),
            (RuntimeIntrinsic::PrintNumber, "karte_jit_runtime_print_number"),
            (RuntimeIntrinsic::PrintBool, "karte_jit_runtime_print_bool"),
            (RuntimeIntrinsic::Panic, "karte_jit_runtime_panic"),
        ].iter().map(|(intrinsic, name)| (*name, intrinsic.symbol_ptr() as u64)).collect();

        let mut needed_intrinsics: HashSet<String> = HashSet::new();
        for (name, ptr) in &intrinsic_names {
            if needed_jit_ptrs.contains(ptr) {
                needed_intrinsics.insert(name.to_string());
            }
        }

        if self.debug {
            eprintln!("AOT: 需要的 runtime intrinsic ({} 个):", needed_intrinsics.len());
            for name in &needed_intrinsics {
                eprintln!("  - {}", name);
            }
        }

        // ---- 3. 按需生成运行时代码 ----
        let mut runtime = X86Runtime::new();
        runtime.generate_with(&needed_intrinsics);
        let runtime_code = runtime.code.clone();
        let runtime_size = runtime_code.len();
        if self.debug {
            eprintln!("AOT: 运行时代码大小: {} 字节 (按需裁剪)", runtime_size);
            for f in &runtime.functions {
                if f.size > 0 {
                    eprintln!("  {} @ offset={}, size={}", f.name, f.offset, f.size);
                }
            }
        }

        // ---- 4. 构建全局标签表 ----
        let code_base: u64 = 0x400000;

        let mut global_labels: HashMap<String, u64> = HashMap::new();

        // 运行时函数地址（使用常量，保证与 runtime_x86.rs 中 fn_start 注册的名字一致）
        // 使用辅助宏简化注册代码
        macro_rules! register_runtime_label {
            ($name_const:expr, $jit_name:expr) => {
                if let Some(off) = runtime.find_offset($name_const) {
                    let addr = code_base + off as u64;
                    global_labels.insert($jit_name.to_string(), addr);
                    global_labels.insert(format!("__runtime_{}", $jit_name), addr);
                }
            };
        }

        register_runtime_label!(crate::runtime_x86::runtime_names::GC_ALLOC_ALIGNED, "karte_jit_runtime_alloc_aligned");
        register_runtime_label!(crate::runtime_x86::runtime_names::GC_ALLOC_ALIGNED, "karte_jit_runtime_alloc");
        register_runtime_label!(crate::runtime_x86::runtime_names::GC_ALLOC, "karte_jit_runtime_gc_alloc");
        register_runtime_label!(crate::runtime_x86::runtime_names::FREE, "karte_jit_runtime_free");
        register_runtime_label!(crate::runtime_x86::runtime_names::RAW_SYSCALL6, "karte_jit_runtime_raw_syscall6");
        register_runtime_label!(crate::runtime_x86::runtime_names::RETAIN, "karte_jit_runtime_retain");
        register_runtime_label!(crate::runtime_x86::runtime_names::RELEASE, "karte_jit_runtime_release");
        register_runtime_label!(crate::runtime_x86::runtime_names::GC_SAFEPOINT, "karte_jit_runtime_gc_safepoint");
        register_runtime_label!(crate::runtime_x86::runtime_names::GC_UPDATE_STACK_TOP, "karte_jit_runtime_update_stack_top");
        register_runtime_label!(crate::runtime_x86::runtime_names::STRING_EQUAL, "karte_jit_runtime_string_equal");
        register_runtime_label!(crate::runtime_x86::runtime_names::STRING_COMPARE, "karte_jit_runtime_string_compare");
        register_runtime_label!(crate::runtime_x86::runtime_names::STRING_CONCAT, "karte_jit_runtime_string_concat");
        register_runtime_label!(crate::runtime_x86::runtime_names::STRING_CHAR_AT, "karte_jit_runtime_string_char_at");
        register_runtime_label!(crate::runtime_x86::runtime_names::CHAR_TO_STRING, "karte_jit_runtime_char_to_string");
        register_runtime_label!(crate::runtime_x86::runtime_names::STRING_SUBSTRING, "karte_jit_runtime_string_substring");
        register_runtime_label!(crate::runtime_x86::runtime_names::STRING_CONTAINS, "karte_jit_runtime_string_contains");
        register_runtime_label!(crate::runtime_x86::runtime_names::SPLIT_COUNT, "karte_jit_runtime_split_count");
        register_runtime_label!(crate::runtime_x86::runtime_names::TRIM, "karte_jit_runtime_trim");
        register_runtime_label!(crate::runtime_x86::runtime_names::TO_STRING, "karte_jit_runtime_to_string");
        register_runtime_label!(crate::runtime_x86::runtime_names::MEM_LOAD64, "karte_jit_runtime_mem_load64");
        register_runtime_label!(crate::runtime_x86::runtime_names::MEM_STORE64, "karte_jit_runtime_mem_store64");
        register_runtime_label!(crate::runtime_x86::runtime_names::PRINT_STRING, "karte_jit_runtime_print_string");
        register_runtime_label!(crate::runtime_x86::runtime_names::PRINT_NUMBER, "karte_jit_runtime_print_number");
        register_runtime_label!(crate::runtime_x86::runtime_names::PRINT_BOOL, "karte_jit_runtime_print_bool");
        register_runtime_label!(crate::runtime_x86::runtime_names::PANIC, "karte_jit_runtime_panic");
        if let Some(off) = runtime.find_offset(crate::runtime_x86::runtime_names::PRINT_NUMBER) {
            let addr = code_base + off as u64;
            global_labels.insert("karte_jit_runtime_print_number".to_string(), addr);
            global_labels.insert("__runtime_karte_jit_runtime_print_number".to_string(), addr);
        }
        if let Some(off) = runtime.find_offset(crate::runtime_x86::runtime_names::PRINT_BOOL) {
            let addr = code_base + off as u64;
            global_labels.insert("karte_jit_runtime_print_bool".to_string(), addr);
            global_labels.insert("__runtime_karte_jit_runtime_print_bool".to_string(), addr);
        }
        if let Some(off) = runtime.find_offset(crate::runtime_x86::runtime_names::PANIC) {
            let addr = code_base + off as u64;
            global_labels.insert("karte_jit_runtime_panic".to_string(), addr);
            global_labels.insert("__runtime_karte_jit_runtime_panic".to_string(), addr);
        }

        // Karte 函数地址
        // 同时收集所有函数内部 labels → 绝对地址的映射
        for (func_name, compiled) in &compiled_functions {
            let func_offset = function_offsets.get(func_name).unwrap();
            let abs_func_addr = code_base + runtime_size as u64 + *func_offset as u64;

            // 注册函数名标签
            global_labels.insert(format!("func_{}", func_name), abs_func_addr);
            global_labels.insert(func_name.clone(), abs_func_addr);

            // 注册函数内部所有 labels
            for (label_name, label_offset) in &compiled.labels {
                let abs_label_addr = abs_func_addr + *label_offset as u64;
                global_labels.insert(label_name.clone(), abs_label_addr);
                if self.debug {
                    eprintln!("AOT: 标签 '{}' @ 0x{:X} (函数 '{}' offset={})",
                        label_name, abs_label_addr, func_name, label_offset);
                }
            }
        }

        // ---- 5. 修补运行时调用 (MOV RAX, imm64; CALL RAX 模式) ----
        // 使用之前构建的 intrinsic_names 列表统一构建 ptr_map
        let mut runtime_ptr_map: HashMap<u64, u64> = HashMap::new();
        {
            // update_stack_top 不在 RuntimeIntrinsic 中, 直接获取
            let update_stack_top_ptr = karte_rt::ffi::karte_jit_runtime_update_stack_top as u64;
            if let Some(&new) = global_labels.get("karte_jit_runtime_update_stack_top") {
                runtime_ptr_map.insert(update_stack_top_ptr, new);
            }

            // 从 intrinsic_names 列表构建映射
            for (jit_name, jit_ptr) in &intrinsic_names {
                if let Some(&new) = global_labels.get(*jit_name) {
                    runtime_ptr_map.insert(*jit_ptr, new);
                }
            }
        }

        // 扫描并修补 MOV RAX, imm64; CALL RAX 模式
        let mut patched_count = 0;
        let mut i = 0;
        while i + 12 <= karte_code.len() {
            if karte_code[i] == 0x48 && karte_code[i + 1] == 0xB8 &&
               karte_code[i + 10] == 0xFF && karte_code[i + 11] == 0xD0 {
                let addr = u64::from_le_bytes(karte_code[i + 2..i + 10].try_into().unwrap());
                if let Some(&new_addr) = runtime_ptr_map.get(&addr) {
                    karte_code[i + 2..i + 10].copy_from_slice(&new_addr.to_le_bytes());
                    patched_count += 1;
                    if self.debug {
                        eprintln!("AOT: 修补运行时调用 @ {}: 0x{:X} → 0x{:X}", i, addr, new_addr);
                    }
                }
            }
            i += 1;
        }

        // ---- 5. 修补 Karte 函数间的交叉引用 (pending jumps) ----
        for (func_name, compiled) in &compiled_functions {
            let func_offset = function_offsets.get(func_name).unwrap();

            for pending in &compiled.pending_jumps {
                let target_label = &pending.target_label;
                if let Some(&target_addr) = global_labels.get(target_label) {
                    let patch_pos_in_karte = func_offset + pending.patch_position;
                    // patch_position 指向需要写入 rel32 的位置 (E8 之后的位置)
                    let patch_abs_addr = code_base + runtime_size as u64 + patch_pos_in_karte as u64 + 4;

                    match pending.jump_type {
                        karte_codegen::vm::professional_executor::jit::code_buffer::JumpType::Call |
                        karte_codegen::vm::professional_executor::jit::code_buffer::JumpType::Unconditional => {
                            // emit_jump 生成的跳转: patch_position 指向操作码 (E9/E8)
                            // rel32 在操作码之后, 需要跳过操作码字节
                            let opcode_size = match pending.jump_type {
                                karte_codegen::vm::professional_executor::jit::code_buffer::JumpType::Call => 1, // E9 (1 byte)
                                _ => 1, // E9 (JMP, 1 byte)
                            };
                            let rel32_pos = patch_pos_in_karte + opcode_size;
                            let patch_abs_end = code_base + runtime_size as u64 + rel32_pos as u64 + 4;

                            let rel = target_addr as i64 - patch_abs_end as i64;
                            if self.debug {
                                eprintln!("AOT: 修补跳转 '{}' in '{}': patch_pos={}, rel32_pos={}, target=0x{:X}, end=0x{:X}, rel={}",
                                    target_label, func_name, patch_pos_in_karte, rel32_pos, target_addr, patch_abs_end, rel);
                            }
                            if rel < i32::MIN as i64 || rel > i32::MAX as i64 {
                                return Err(format!(
                                    "函数 '{}' 的跳转距离超出范围: {} → {}",
                                    func_name, patch_abs_end, target_addr
                                ));
                            }
                            let rel_bytes = (rel as i32).to_le_bytes();
                            karte_code[rel32_pos..rel32_pos + 4]
                                .copy_from_slice(&rel_bytes);
                        }
                        karte_codegen::vm::professional_executor::jit::code_buffer::JumpType::ConditionalEqual |
                        karte_codegen::vm::professional_executor::jit::code_buffer::JumpType::ConditionalNotEqual |
                        karte_codegen::vm::professional_executor::jit::code_buffer::JumpType::ConditionalLess |
                        karte_codegen::vm::professional_executor::jit::code_buffer::JumpType::ConditionalGreater |
                        karte_codegen::vm::professional_executor::jit::code_buffer::JumpType::ConditionalLessEqual |
                        karte_codegen::vm::professional_executor::jit::code_buffer::JumpType::ConditionalGreaterEqual => {
                            // 条件跳转: 0F 8x (2字节操作码), rel32 在 offset 2
                            let rel32_pos = patch_pos_in_karte + 2;
                            let patch_abs_end = code_base + runtime_size as u64 + rel32_pos as u64 + 4;
                            let rel = target_addr as i64 - patch_abs_end as i64;
                            if self.debug {
                                eprintln!("AOT: 修补条件跳转 '{}' in '{}': rel32_pos={}, target=0x{:X}, rel={}",
                                    target_label, func_name, rel32_pos, target_addr, rel);
                            }
                            if rel < i32::MIN as i64 || rel > i32::MAX as i64 {
                                return Err(format!(
                                    "函数 '{}' 的条件跳转距离超出范围", func_name
                                ));
                            }
                            let rel_bytes = (rel as i32).to_le_bytes();
                            karte_code[rel32_pos..rel32_pos + 4]
                                .copy_from_slice(&rel_bytes);
                        }
                    }
                } else {
                    if self.debug {
                        eprintln!("AOT: 警告: 未解析的跳转目标 '{}' 在函数 '{}'", target_label, func_name);
                    }
                }
            }

            // 修补 pending label addresses (64-bit absolute address loads)
            for pending in &compiled.pending_label_addresses {
                let target_label = &pending.target_label;
                
                // 处理 __global_* 标签 - 从 runtime 全局数据区获取地址
                if let Some(global_name) = target_label.strip_prefix("__global_") {
                    // 映射 Karte 全局名到 runtime 全局名
                    let runtime_global_name = match global_name {
                        "heap_base" => "__heap_start".to_string(),
                        "heap_limit" => "__heap_limit".to_string(),
                        "stack_bottom" => "__vstack_bottom".to_string(),
                        "stack_top" => "__vstack_top".to_string(),
                        _ => format!("__{}", global_name),
                    };
                    if let Some(global_offset) = runtime.find_offset(&runtime_global_name) {
                        let global_addr = code_base + global_offset as u64;
                        let patch_pos = func_offset + pending.patch_position;
                        if self.debug {
                            eprintln!("AOT: 修补全局变量 '{}' in '{}': patch_pos={}, addr=0x{:X}",
                                global_name, func_name, patch_pos, global_addr);
                        }
                        karte_code[patch_pos..patch_pos + 8].copy_from_slice(&global_addr.to_le_bytes());
                        continue;
                    }
                }
                
                if let Some(&target_addr) = global_labels.get(target_label) {
                    let patch_pos = func_offset + pending.patch_position;
                    if self.debug {
                        eprintln!("AOT: 修补标签地址 '{}' in '{}': patch_pos={}, target=0x{:X}",
                            target_label, func_name, patch_pos, target_addr);
                    }
                    karte_code[patch_pos..patch_pos + 8].copy_from_slice(&target_addr.to_le_bytes());
                }
            }
        }

        // ---- 6. 修补 _start 中的 CALL main ----
        let main_offset = function_offsets.get(&entry_func_name)
            .ok_or_else(|| format!("未找到主函数 '{}'", entry_func_name))?;
        let main_addr = code_base + runtime_size as u64 + *main_offset as u64;

        let mut runtime_code_mut = runtime.code;
        let call_rel_offset = runtime.call_main_rel32_offset;
        let call_end = code_base + call_rel_offset as u64 + 4;
        let rel = main_addr as i64 - call_end as i64;
        runtime_code_mut[call_rel_offset..call_rel_offset + 4]
            .copy_from_slice(&(rel as i32).to_le_bytes());

        if self.debug {
            eprintln!("AOT: main @ 0x{:X}, CALL rel32 = {}", main_addr, rel);
        }

        // ---- 7. 组装 ELF ----
        let mut elf = ElfWriter::new(ElfArch::X86_64, code_base, 0x800000);
        elf.set_entry_offset(0); // _start 在偏移 0

        elf.append_code(&runtime_code_mut);
        elf.append_code(&karte_code);

        let binary = elf.generate()
            .map_err(|e| format!("生成 ELF 失败: {}", e))?;

        if self.debug {
            eprintln!("AOT: 可执行文件大小: {} 字节", binary.len());
            eprintln!("AOT: entry: 0x{:X}", code_base);
        }

        Ok(binary)
    }

    /// AArch64 AOT 编译
    fn compile_aarch64(&self, program: &LirProgram) -> Result<Vec<u8>, String> {
        use crate::runtime_aarch64::AArch64Runtime;
        use karte_codegen::vm::professional_executor::jit::AArch64Compiler;

        // 1. 生成运行时
        let runtime = AArch64Runtime::new().generate();
        let runtime_code = runtime.code.clone();
        let runtime_size = runtime_code.len();
        if self.debug {
            eprintln!("AOT(AArch64): 运行时代码大小: {} 字节", runtime_size);
            for f in &runtime.functions {
                if f.size > 0 {
                    eprintln!("  {} @ offset={}, size={}", f.name, f.offset, f.size);
                }
            }
        }

        // 2. 编译 Karte 函数
        let mut compiler = AArch64Compiler::new(self.debug)
            .map_err(|e| format!("创建 AArch64 编译器失败: {}", e))?;

        let mut compiled_functions = Vec::new();
        let mut karte_code: Vec<u8> = Vec::new();
        let mut function_offsets: HashMap<String, usize> = HashMap::new();

        let main_name = program.main_function.as_deref().unwrap_or("main");
        let mut func_names: Vec<String> = program.functions.keys().cloned().collect();
        func_names.sort_by(|a, b| {
            if a == main_name { std::cmp::Ordering::Greater }
            else if b == main_name { std::cmp::Ordering::Less }
            else { a.cmp(b) }
        });

        for func_name in &func_names {
            let func = program.functions.get(func_name).unwrap();
            let compiled = compiler.compile_function(func, program)
                .map_err(|e| format!("编译函数 '{}' 失败: {}", func_name, e))?;
            // 4 字节对齐
            while karte_code.len() % 4 != 0 {
                karte_code.push(0x1F); // NOP padding
            }
            let offset = karte_code.len();
            function_offsets.insert(func_name.clone(), offset);
            karte_code.extend_from_slice(compiled.machine_code());
            compiled_functions.push((func_name.clone(), compiled));
        }

        // 3. 构建全局标签表
        let code_base: u64 = 0x400000;
        let mut global_labels: HashMap<String, u64> = HashMap::new();

        // 运行时函数地址（使用常量，保证与 runtime_x86.rs 中 fn_start 注册的名字一致）
        if let Some(off) = runtime.find_offset(crate::runtime_x86::runtime_names::GC_ALLOC_ALIGNED) {
            let addr = code_base + off as u64;
            global_labels.insert("karte_jit_runtime_alloc_aligned".to_string(), addr);
            global_labels.insert("karte_jit_runtime_alloc".to_string(), addr);
            global_labels.insert("__runtime_karte_jit_runtime_alloc_aligned".to_string(), addr);
            global_labels.insert("__runtime_karte_jit_runtime_alloc".to_string(), addr);
        }
        if let Some(off) = runtime.find_offset(crate::runtime_x86::runtime_names::FREE) {
            let addr = code_base + off as u64;
            global_labels.insert("karte_jit_runtime_free".to_string(), addr);
            global_labels.insert("__runtime_karte_jit_runtime_free".to_string(), addr);
        }
        if let Some(off) = runtime.find_offset(crate::runtime_x86::runtime_names::RETAIN) {
            let addr = code_base + off as u64;
            global_labels.insert("karte_jit_runtime_retain".to_string(), addr);
            global_labels.insert("__runtime_karte_jit_runtime_retain".to_string(), addr);
        }
        if let Some(off) = runtime.find_offset(crate::runtime_x86::runtime_names::RELEASE) {
            let addr = code_base + off as u64;
            global_labels.insert("karte_jit_runtime_release".to_string(), addr);
            global_labels.insert("__runtime_karte_jit_runtime_release".to_string(), addr);
        }
        if let Some(off) = runtime.find_offset(crate::runtime_x86::runtime_names::GC_SAFEPOINT) {
            let addr = code_base + off as u64;
            global_labels.insert("karte_jit_runtime_gc_safepoint".to_string(), addr);
            global_labels.insert("__runtime_karte_jit_runtime_gc_safepoint".to_string(), addr);
        }
        if let Some(off) = runtime.find_offset(crate::runtime_x86::runtime_names::GC_UPDATE_STACK_TOP) {
            let addr = code_base + off as u64;
            global_labels.insert("karte_jit_runtime_update_stack_top".to_string(), addr);
            global_labels.insert("__runtime_karte_jit_runtime_update_stack_top".to_string(), addr);
        }

        // Karte 函数地址
        for (func_name, compiled) in &compiled_functions {
            let func_offset = function_offsets.get(func_name).unwrap();
            let abs_addr = code_base + runtime_size as u64 + *func_offset as u64;
            global_labels.insert(format!("func_{}", func_name), abs_addr);
            global_labels.insert(func_name.clone(), abs_addr);
            for (label_name, label_offset) in &compiled.labels {
                global_labels.insert(label_name.clone(), abs_addr + *label_offset as u64);
            }
        }

        // 4. 修补运行时调用 (MOV X16, imm64; BLR X16 模式)
        // AArch64 JIT emit_runtime_dispatch 生成:
        //   MOVZ X16, #lo16      (4 bytes)
        //   MOVK X16, #mid16, LSL #16  (4 bytes)
        //   MOVK X16, #mid32, LSL #32  (4 bytes)
        //   MOVK X16, #hi16, LSL #48   (4 bytes)
        //   BLR X16               (4 bytes)
        // 共 20 字节
        use karte_codegen::vm::professional_executor::jit::ffi::RuntimeIntrinsic;

        let mut runtime_ptr_map: HashMap<u64, u64> = HashMap::new();
        let alloc_ptr = RuntimeIntrinsic::AllocAligned.symbol_ptr() as u64;
        let free_ptr = RuntimeIntrinsic::Free.symbol_ptr() as u64;
        let retain_ptr = RuntimeIntrinsic::Retain.symbol_ptr() as u64;
        let release_ptr = RuntimeIntrinsic::Release.symbol_ptr() as u64;
        let safepoint_ptr = RuntimeIntrinsic::GcSafepoint.symbol_ptr() as u64;
        let update_stack_top_ptr = karte_rt::ffi::karte_jit_runtime_update_stack_top as u64;

        if let Some(&new) = global_labels.get("karte_jit_runtime_alloc_aligned") {
            runtime_ptr_map.insert(alloc_ptr, new);
        }
        if let Some(&new) = global_labels.get("karte_jit_runtime_free") {
            runtime_ptr_map.insert(free_ptr, new);
        }
        if let Some(&new) = global_labels.get("karte_jit_runtime_retain") {
            runtime_ptr_map.insert(retain_ptr, new);
        }
        if let Some(&new) = global_labels.get("karte_jit_runtime_release") {
            runtime_ptr_map.insert(release_ptr, new);
        }
        if let Some(&new) = global_labels.get("karte_jit_runtime_gc_safepoint") {
            runtime_ptr_map.insert(safepoint_ptr, new);
        }
        if let Some(&new) = global_labels.get("karte_jit_runtime_update_stack_top") {
            runtime_ptr_map.insert(update_stack_top_ptr, new);
        }

        // 字符串相关运行时函数
        let string_intrinsics = [
            (RuntimeIntrinsic::StringEqual, "karte_jit_runtime_string_equal"),
            (RuntimeIntrinsic::StringConcat, "karte_jit_runtime_string_concat"),
            (RuntimeIntrinsic::StringCharAt, "karte_jit_runtime_string_char_at"),
            (RuntimeIntrinsic::StringContains, "karte_jit_runtime_string_contains"),
            (RuntimeIntrinsic::SplitCount, "karte_jit_runtime_split_count"),
            (RuntimeIntrinsic::Trim, "karte_jit_runtime_trim"),
            (RuntimeIntrinsic::ToString, "karte_jit_runtime_to_string"),
            (RuntimeIntrinsic::PrintString, "karte_jit_runtime_print_string"),
            (RuntimeIntrinsic::PrintNumber, "karte_jit_runtime_print_number"),
            (RuntimeIntrinsic::PrintBool, "karte_jit_runtime_print_bool"),
            (RuntimeIntrinsic::Panic, "karte_jit_runtime_panic"),
        ];        for (intrinsic, label) in &string_intrinsics {
            let ptr = intrinsic.symbol_ptr() as u64;
            if let Some(&new) = global_labels.get(*label) {
                runtime_ptr_map.insert(ptr, new);
            }
        }

        // 扫描并修补 MOV X16, imm64; BLR X16 模式 (20 bytes)
        // BLR X16 = 0xD63F0200
        let blr_x16: [u8; 4] = 0xD63F0200u32.to_le_bytes();
        let mut patched_count = 0;
        let mut i = 0;
        while i + 20 <= karte_code.len() {
            // 检查最后 4 字节是否是 BLR X16
            if karte_code[i + 16..i + 20] == blr_x16 {
                // 前 16 字节是 MOV X16, imm64 (4条MOVZ/MOVK)
                // 解析 imm64: 从4条指令中提取
                let w0 = u32::from_le_bytes(karte_code[i..i + 4].try_into().unwrap());
                let w1 = u32::from_le_bytes(karte_code[i + 4..i + 8].try_into().unwrap());
                let w2 = u32::from_le_bytes(karte_code[i + 8..i + 12].try_into().unwrap());
                let w3 = u32::from_le_bytes(karte_code[i + 12..i + 16].try_into().unwrap());

                // 验证是 MOVZ/MOVK X16 序列
                let rd0 = w0 & 0x1F;
                let rd1 = w1 & 0x1F;
                let rd2 = w2 & 0x1F;
                let rd3 = w3 & 0x1F;

                if rd0 == 16 && (rd1 == 16 || (w1 == 0 && rd1 == 0)) {
                    // 提取 imm64
                    let imm0 = (w0 >> 5) & 0xFFFF;
                    let hw0 = (w0 >> 21) & 0x3;
                    let mut val = imm0 << (hw0 * 16);

                    if rd1 == 16 {
                        let imm1 = (w1 >> 5) & 0xFFFF;
                        let hw1 = (w1 >> 21) & 0x3;
                        val |= imm1 << (hw1 * 16);
                    }
                    if rd2 == 16 {
                        let imm2 = (w2 >> 5) & 0xFFFF;
                        let hw2 = (w2 >> 21) & 0x3;
                        val |= imm2 << (hw2 * 16);
                    }
                    if rd3 == 16 {
                        let imm3 = (w3 >> 5) & 0xFFFF;
                        let hw3 = (w3 >> 21) & 0x3;
                        val |= imm3 << (hw3 * 16);
                    }

                    if let Some(&new_addr) = runtime_ptr_map.get(&(val as u64)) {
                        // 重写 MOV X16, new_addr
                        let v = new_addr as u64;
                        let hw0 = 0u32;
                        let new_w0 = (1u32 << 31) | (0b10 << 29) | (0b100101 << 23) | (hw0 << 21) | (((v & 0xFFFF) as u32) << 5) | 16;
                        karte_code[i..i + 4].copy_from_slice(&new_w0.to_le_bytes());

                        let hw1 = 1u32;
                        let new_w1 = (1u32 << 31) | (0b11 << 29) | (0b100101 << 23) | (hw1 << 21) | ((((v >> 16) & 0xFFFF) as u32) << 5) | 16;
                        karte_code[i + 4..i + 8].copy_from_slice(&new_w1.to_le_bytes());

                        let hw2 = 2u32;
                        let new_w2 = (1u32 << 31) | (0b11 << 29) | (0b100101 << 23) | (hw2 << 21) | ((((v >> 32) & 0xFFFF) as u32) << 5) | 16;
                        karte_code[i + 8..i + 12].copy_from_slice(&new_w2.to_le_bytes());

                        let hw3 = 3u32;
                        let new_w3 = (1u32 << 31) | (0b11 << 29) | (0b100101 << 23) | (hw3 << 21) | ((((v >> 48) & 0xFFFF) as u32) << 5) | 16;
                        karte_code[i + 12..i + 16].copy_from_slice(&new_w3.to_le_bytes());

                        patched_count += 1;
                    }
                }
            }
            i += 4; // AArch64 指令对齐到 4 字节
        }

        if self.debug {
            eprintln!("AOT(AArch64): 修补了 {} 个运行时调用", patched_count);
        }

        // 5. 修补 pending_label_addresses
        for (func_name, compiled) in &compiled_functions {
            let func_offset = function_offsets.get(func_name).unwrap();
            for pending in &compiled.pending_label_addresses {
                let target_label = &pending.target_label;
                // 全局变量标签
                if let Some(global_name) = target_label.strip_prefix("__global_") {
                    let runtime_global_name = match global_name {
                        "heap_base" => "__heap_start".to_string(),
                        "heap_limit" => "__heap_limit".to_string(),
                        "stack_bottom" => "__vstack_bottom".to_string(),
                        "stack_top" => "__vstack_top".to_string(),
                        _ => format!("__{}", global_name),
                    };
                    // AArch64 runtime 没有注册独立的全局标签函数
                    // 全局数据在 runtime 末尾的 globals_data_offset 处
                    if let Some(runtime_fn) = runtime.functions.iter().find(|f| f.name == runtime_global_name) {
                        let addr = code_base + runtime_fn.offset as u64;
                        let pos = func_offset + pending.patch_position;
                        karte_code[pos..pos + 8].copy_from_slice(&addr.to_le_bytes());
                        continue;
                    }
                }
                if let Some(&addr) = global_labels.get(target_label) {
                    let pos = func_offset + pending.patch_position;
                    karte_code[pos..pos + 8].copy_from_slice(&addr.to_le_bytes());
                }
            }

            // 5b. 修补 pending_adrs (ADR/ADRP 模式)
            for pending in &compiled.pending_adrs {
                let target_label = &pending.target_label;
                if let Some(&target_addr) = global_labels.get(target_label) {
                    let patch_pos = func_offset + pending.patch_position;
                    let patch_abs = code_base + runtime_size as u64 + patch_pos as u64;
                    use karte_codegen::vm::professional_executor::jit::code_buffer::AdrPatchType;
                    match &pending.patch_type {
                        AdrPatchType::Adr { dst_register: rd } => {
                            // ADR Rd, offset: 21-bit signed offset from PC
                            let off = target_addr as i64 - patch_abs as i64;
                            if off < -(1 << 20) || off > (1 << 20) - 1 {
                                return Err(format!("ADR 距离超出范围: offset={}", off));
                            }
                            let immlo = (off as u32) & 0x3;
                            let immhi = ((off as u32) >> 2) & 0x7FFFF;
                            let instr = (0b10000u32 << 24) | (immlo << 29) | (immhi << 5) | (*rd as u32);
                            karte_code[patch_pos..patch_pos + 4].copy_from_slice(&instr.to_le_bytes());
                        }
                        AdrPatchType::Adrp { dst_register: rd } => {
                            // ADRP Rd, page_offset: 页对齐的 33-bit 偏移
                            // ADRP 编码: bit[31]=1, bit[30:29]=immlo[1:0], bit[28:24]=10000
                            // 注意：之前错误地使用了 0b10000<<24 = 0x10000000（ADR），
                            // 正确应该是 bit[31]=1: (1u32<<31) | (immlo<<29) | (0b10000u32<<24) | ...
                            let pc_page = (patch_abs as usize) & !0xFFF;
                            let target_page = (target_addr as usize) & !0xFFF;
                            let page_off = target_page as i64 - pc_page as i64;
                            let immhi = ((page_off as u64) >> 12) as u32;
                            let immlo = ((page_off as u64) >> 2) as u32 & 0x3;
                            let instr = (1u32 << 31) | (immlo << 29) | (0b10000u32 << 24) | ((immhi & 0x7FFFF) << 5) | (*rd as u32);
                            karte_code[patch_pos..patch_pos + 4].copy_from_slice(&instr.to_le_bytes());
                        }
                        AdrPatchType::AddLabel { dst_register: _ } => {
                            // ADD Rd, Rn, #page_inner: 12-bit 页内偏移
                            let page_inner = (target_addr as u32) & 0xFFF;
                            let orig = u32::from_le_bytes(karte_code[patch_pos..patch_pos + 4].try_into().unwrap());
                            let rd = orig & 0x1F;
                            let rn = (orig >> 5) & 0x1F;
                            let instr = (1u32 << 31) | (0b100010 << 23) | ((page_inner & 0xFFF) << 10) | ((rn as u32) << 5) | (rd as u32);
                            karte_code[patch_pos..patch_pos + 4].copy_from_slice(&instr.to_le_bytes());
                        }
                        AdrPatchType::Store { base_register: _, offset: _ } => {
                            // Store: 直接将目标地址的某个偏移写入指令的 immediate 字段
                            // 暂时不处理
                        }
                    }
                } else {
                    // 标签未找到 - 可能在其他编译单元中
                }
            }
        }

        // 6. 修补 pending_jumps (AArch64 B/BL/B.cond)
        for (func_name, compiled) in &compiled_functions {
            let func_offset = function_offsets.get(func_name).unwrap();
            for pending in &compiled.pending_jumps {
                let target_label = &pending.target_label;
                if let Some(&target_addr) = global_labels.get(target_label) {
                    let patch_pos = func_offset + pending.patch_position;
                    let patch_abs = code_base + runtime_size as u64 + patch_pos as u64;
                    let offset = target_addr as i64 - patch_abs as i64;

                    // 读取原始指令，保留操作码位，只替换偏移量
                    let current_instr = u32::from_le_bytes(
                        karte_code[patch_pos..patch_pos + 4].try_into().unwrap()
                    );
                    // AArch64 跳转偏移以 4 字节为单位
                    let target_offset = (target_addr as i64 - patch_abs as i64) >> 2;

                    match pending.jump_type {
                        karte_codegen::vm::professional_executor::jit::code_buffer::JumpType::Call |
                        karte_codegen::vm::professional_executor::jit::code_buffer::JumpType::Unconditional => {
                            // B offset26 (0x14000000) / BL offset26 (0x94000000)
                            // imm26 在 bits [25:0]，操作码在 bits [31:26]
                            // ±128MB 范围检查
                            if target_offset < -(1i64 << 25) || target_offset > (1i64 << 25) - 1 {
                                return Err(format!("B/BL 距离超出范围: offset={}", target_offset << 2));
                            }
                            let new_instr = (current_instr & 0xFC000000) | ((target_offset as u32) & 0x03FFFFFF);
                            karte_code[patch_pos..patch_pos + 4].copy_from_slice(&new_instr.to_le_bytes());
                            if self.debug {
                                eprintln!("AOT(AArch64): 修补 B/BL '{}' in '{}': patch_pos=0x{:X}, offset={}",
                                    target_label, func_name, patch_pos, target_offset << 2);
                            }
                        }
                        _ => {
                            // B.cond offset19 (0x54000000 | (imm19 << 5) | cond)
                            // imm19 在 bits [23:5]，cond 在 bits [4:0]，操作码在 bits [31:24]
                            // ±1MB 范围检查
                            if target_offset < -(1i64 << 18) || target_offset > (1i64 << 18) - 1 {
                                return Err(format!("B.cond 距离超出范围: offset={}", target_offset << 2));
                            }
                            let new_instr = (current_instr & 0xFF00001F) | (((target_offset as u32) & 0x7FFFF) << 5);
                            karte_code[patch_pos..patch_pos + 4].copy_from_slice(&new_instr.to_le_bytes());
                            if self.debug {
                                eprintln!("AOT(AArch64): 修补 B.cond '{}' in '{}': patch_pos=0x{:X}, offset={}",
                                    target_label, func_name, patch_pos, target_offset << 2);
                            }
                        }
                    }
                }
            }
        }

        // 7. 修补 _start 的 BL main
        let main_offset = function_offsets.get(main_name)
            .ok_or_else(|| format!("未找到主函数 '{}'", main_name))?;
        let main_addr = code_base + runtime_size as u64 + *main_offset as u64;

        let mut runtime_code_mut = runtime.code;
        let call_bl_offset = runtime.call_main_offset;
        let bl_pc = code_base + call_bl_offset as u64;
        let bl_offset = main_addr as i64 - bl_pc as i64;

        let imm26 = ((bl_offset / 4) as u32) & 0x3FFFFFF;
        let bl_instr = (0b100101u32 << 26) | imm26;
        runtime_code_mut[call_bl_offset..call_bl_offset + 4]
            .copy_from_slice(&bl_instr.to_le_bytes());

        if self.debug {
            eprintln!("AOT(AArch64): main @ 0x{:X}, BL offset = {}", main_addr, bl_offset);
        }

        // 8. 生成 ELF
        let mut elf = ElfWriter::new(ElfArch::AArch64, code_base, 0x800000);
        elf.set_entry_offset(0);
        elf.append_code(&runtime_code_mut);
        elf.append_code(&karte_code);

        let binary = elf.generate()
            .map_err(|e| format!("生成 ELF 失败: {}", e))?;

        if self.debug {
            eprintln!("AOT(AArch64): 可执行文件大小: {} 字节", binary.len());
        }

        Ok(binary)
    }

    /// RISC-V 64 位 AOT 编译
    fn compile_riscv64(&self, program: &LirProgram) -> Result<Vec<u8>, String> {
        use karte_codegen::vm::professional_executor::jit::RiscvCompiler;

        // 1. 生成运行时
        let runtime = RiscvRuntime::new().generate();
        let runtime_code = runtime.code.clone();
        let runtime_size = runtime_code.len();

        // 2. 编译 Karte 函数
        let mut compiler = RiscvCompiler::new(self.debug)
            .map_err(|e| format!("创建 RISC-V 编译器失败: {}", e))?;

        let mut compiled_functions = Vec::new();
        let mut karte_code: Vec<u8> = Vec::new();
        let mut function_offsets: HashMap<String, usize> = HashMap::new();

        let main_name = program.main_function.as_deref().unwrap_or("main");
        let mut func_names: Vec<String> = program.functions.keys().cloned().collect();
        func_names.sort_by(|a, b| {
            if a == main_name { std::cmp::Ordering::Greater }
            else if b == main_name { std::cmp::Ordering::Less }
            else { a.cmp(b) } // 确定性排序：非main函数按名字排序
        });

        for func_name in &func_names {
            let func = program.functions.get(func_name).unwrap();
            let compiled = compiler.compile_function(func, program)
                .map_err(|e| format!("编译函数 '{}' 失败: {}", func_name, e))?;
            while karte_code.len() % 4 != 0 {
                karte_code.extend_from_slice(&0x00000013u32.to_le_bytes());
            }
            let offset = karte_code.len();
            function_offsets.insert(func_name.clone(), offset);
            karte_code.extend_from_slice(compiled.machine_code());
            compiled_functions.push((func_name.clone(), compiled));
        }

        // 3. 构建全局标签表
        let code_base: u64 = 0x400000;
        let mut global_labels: HashMap<String, u64> = HashMap::new();

        if let Some(off) = runtime.find_offset(crate::runtime_x86::runtime_names::GC_ALLOC_ALIGNED) {
            let addr = code_base + off as u64;
            global_labels.insert("karte_jit_runtime_alloc_aligned".to_string(), addr);
            global_labels.insert("karte_jit_runtime_alloc".to_string(), addr);
            // RISC-V 编译器 emit_runtime_call 生成的标签带 __runtime_ 前缀
            global_labels.insert("__runtime_karte_jit_runtime_alloc_aligned".to_string(), addr);
            global_labels.insert("__runtime_karte_jit_runtime_alloc".to_string(), addr);
        }
        if let Some(off) = runtime.find_offset(crate::runtime_x86::runtime_names::FREE) {
            let addr = code_base + off as u64;
            global_labels.insert("karte_jit_runtime_free".to_string(), addr);
            global_labels.insert("__runtime_karte_jit_runtime_free".to_string(), addr);
        }
        if let Some(off) = runtime.find_offset(crate::runtime_x86::runtime_names::RETAIN) {
            let addr = code_base + off as u64;
            global_labels.insert("karte_jit_runtime_retain".to_string(), addr);
            global_labels.insert("__runtime_karte_jit_runtime_retain".to_string(), addr);
        }
        if let Some(off) = runtime.find_offset(crate::runtime_x86::runtime_names::RELEASE) {
            let addr = code_base + off as u64;
            global_labels.insert("karte_jit_runtime_release".to_string(), addr);
            global_labels.insert("__runtime_karte_jit_runtime_release".to_string(), addr);
        }
        if let Some(off) = runtime.find_offset(crate::runtime_x86::runtime_names::GC_SAFEPOINT) {
            let addr = code_base + off as u64;
            global_labels.insert("karte_jit_runtime_gc_safepoint".to_string(), addr);
            global_labels.insert("__runtime_karte_jit_runtime_gc_safepoint".to_string(), addr);
        }
        if let Some(off) = runtime.find_offset(crate::runtime_x86::runtime_names::GC_UPDATE_STACK_TOP) {
            let addr = code_base + off as u64;
            global_labels.insert("karte_jit_runtime_update_stack_top".to_string(), addr);
            global_labels.insert("__runtime_karte_jit_runtime_update_stack_top".to_string(), addr);
        }

        // 字符串相关运行时函数
        if let Some(off) = runtime.find_offset(crate::runtime_x86::runtime_names::STRING_EQUAL) {
            let addr = code_base + off as u64;
            global_labels.insert("karte_jit_runtime_string_equal".to_string(), addr);
            global_labels.insert("__runtime_karte_jit_runtime_string_equal".to_string(), addr);
        }
        if let Some(off) = runtime.find_offset(crate::runtime_x86::runtime_names::STRING_CONCAT) {
            let addr = code_base + off as u64;
            global_labels.insert("karte_jit_runtime_string_concat".to_string(), addr);
            global_labels.insert("__runtime_karte_jit_runtime_string_concat".to_string(), addr);
        }
        if let Some(off) = runtime.find_offset(crate::runtime_x86::runtime_names::TO_STRING) {
            let addr = code_base + off as u64;
            global_labels.insert("karte_jit_runtime_to_string".to_string(), addr);
            global_labels.insert("__runtime_karte_jit_runtime_to_string".to_string(), addr);
        }
        if let Some(off) = runtime.find_offset(crate::runtime_x86::runtime_names::CHAR_TO_STRING) {
            let addr = code_base + off as u64;
            global_labels.insert("karte_jit_runtime_char_to_string".to_string(), addr);
            global_labels.insert("__runtime_karte_jit_runtime_char_to_string".to_string(), addr);
        }
        if let Some(off) = runtime.find_offset(crate::runtime_x86::runtime_names::PRINT_STRING) {
            let addr = code_base + off as u64;
            global_labels.insert("karte_jit_runtime_print_string".to_string(), addr);
            global_labels.insert("__runtime_karte_jit_runtime_print_string".to_string(), addr);
        }
        if let Some(off) = runtime.find_offset(crate::runtime_x86::runtime_names::PRINT_NUMBER) {
            let addr = code_base + off as u64;
            global_labels.insert("karte_jit_runtime_print_number".to_string(), addr);
            global_labels.insert("__runtime_karte_jit_runtime_print_number".to_string(), addr);
        }
        if let Some(off) = runtime.find_offset(crate::runtime_x86::runtime_names::PRINT_BOOL) {
            let addr = code_base + off as u64;
            global_labels.insert("karte_jit_runtime_print_bool".to_string(), addr);
            global_labels.insert("__runtime_karte_jit_runtime_print_bool".to_string(), addr);
        }
        if let Some(off) = runtime.find_offset(crate::runtime_x86::runtime_names::PANIC) {
            let addr = code_base + off as u64;
            global_labels.insert("karte_jit_runtime_panic".to_string(), addr);
            global_labels.insert("__runtime_karte_jit_runtime_panic".to_string(), addr);
        }

        for (func_name, compiled) in &compiled_functions {
            let func_offset = function_offsets.get(func_name).unwrap();
            let abs_addr = code_base + runtime_size as u64 + *func_offset as u64;
            global_labels.insert(format!("func_{}", func_name), abs_addr);
            global_labels.insert(func_name.clone(), abs_addr);
            for (label_name, label_offset) in &compiled.labels {
                global_labels.insert(label_name.clone(), abs_addr + *label_offset as u64);
            }
        }

        // 4. 修补 pending_label_addresses
        for (func_name, compiled) in &compiled_functions {
            let func_offset = function_offsets.get(func_name).unwrap();
            for pending in &compiled.pending_label_addresses {
                let target_label = &pending.target_label;
                if let Some(global_name) = target_label.strip_prefix("__global_") {
                    let runtime_global_name = match global_name {
                        "heap_base" => "__heap_start".to_string(),
                        "heap_limit" => "__heap_limit".to_string(),
                        "stack_bottom" => "__vstack_bottom".to_string(),
                        "stack_top" => "__vstack_top".to_string(),
                        _ => format!("__{}", global_name),
                    };
                    if let Some(global_offset) = runtime.find_offset(&runtime_global_name) {
                        let addr = code_base + global_offset as u64;
                        let pos = func_offset + pending.patch_position;
                        karte_code[pos..pos + 8].copy_from_slice(&addr.to_le_bytes());
                        continue;
                    }
                }
                if let Some(&addr) = global_labels.get(target_label) {
                    let pos = func_offset + pending.patch_position;
                    karte_code[pos..pos + 8].copy_from_slice(&addr.to_le_bytes());
                }
            }
        }

        // 5. 修补 pending_jumps (RISC-V 编码)
        for (func_name, compiled) in &compiled_functions {
            let func_offset = function_offsets.get(func_name).unwrap();
            for pending in &compiled.pending_jumps {
                let target_label = &pending.target_label;
                if let Some(&target_addr) = global_labels.get(target_label) {
                    let patch_pos = func_offset + pending.patch_position;
                    let patch_abs = code_base + runtime_size as u64 + patch_pos as u64;
                    let offset = target_addr as i64 - patch_abs as i64;

                    match pending.jump_type {
                        karte_codegen::vm::professional_executor::jit::code_buffer::JumpType::Call => {
                            // Call 通过 pending_label_addresses 修补，忽略
                        }
                        karte_codegen::vm::professional_executor::jit::code_buffer::JumpType::Unconditional => {
                            // JAL x0, offset (21-bit)
                            if offset < -(1 << 20) || offset > (1 << 20) - 1 {
                                return Err(format!("JAL 距离超出范围: offset={}", offset));
                            }
                            let v = offset as u32;
                            let instr = (((v >> 20) & 1) << 31) | (((v >> 1) & 0x3FF) << 21)
                                | (((v >> 11) & 1) << 20) | (((v >> 12) & 0xFF) << 12)
                                | (0u32 << 7) | 0x6F;
                            karte_code[patch_pos..patch_pos + 4].copy_from_slice(&instr.to_le_bytes());
                        }
                        _ => {
                            // B-type 条件分支 (13-bit)
                            if offset < -(1 << 12) || offset > (1 << 12) - 1 {
                                return Err(format!("条件分支距离超出范围: {}", offset));
                            }
                            let orig = u32::from_le_bytes(karte_code[patch_pos..patch_pos+4].try_into().unwrap());
                            let funct3 = ((orig >> 12) & 0x7) as u8;
                            let rs1 = ((orig >> 15) & 0x1F) as u8;
                            let rs2 = ((orig >> 20) & 0x1F) as u8;
                            let v = offset as u32;
                            let instr = (((v >> 12) & 1) << 31) | (((v >> 5) & 0x3F) << 25)
                                | ((rs2 as u32) << 20) | ((rs1 as u32) << 15)
                                | ((funct3 as u32) << 12) | (((v >> 1) & 0xF) << 8)
                                | (((v >> 11) & 1) << 7) | 0x63;
                            karte_code[patch_pos..patch_pos + 4].copy_from_slice(&instr.to_le_bytes());
                        }
                    }
                }
            }
        }

        // 6. 修补 _start 的 JAL main
        let main_offset = function_offsets.get(main_name)
            .ok_or_else(|| format!("未找到主函数 '{}'", main_name))?;
        let main_addr = code_base + runtime_size as u64 + *main_offset as u64;

        let mut runtime_code_mut = runtime.code;
        let call_jal_offset = runtime.call_main_offset;
        let jal_pc = code_base + call_jal_offset as u64;
        let jal_offset = main_addr as i64 - jal_pc as i64;
        if jal_offset < -(1 << 20) || jal_offset > (1 << 20) - 1 {
            return Err(format!("JAL main 距离超出范围: {}", jal_offset));
        }
        let v = jal_offset as u32;
        let jal_instr = (((v >> 20) & 1) << 31) | (((v >> 1) & 0x3FF) << 21)
            | (((v >> 11) & 1) << 20) | (((v >> 12) & 0xFF) << 12)
            | (1u32 << 7) | 0x6F;
        runtime_code_mut[call_jal_offset..call_jal_offset + 4]
            .copy_from_slice(&jal_instr.to_le_bytes());

        // 7. 生成 ELF
        let mut elf = ElfWriter::new(ElfArch::Riscv64, code_base, 0x800000);
        elf.set_entry_offset(0);
        elf.append_code(&runtime_code_mut);
        elf.append_code(&karte_code);

        let binary = elf.generate()
            .map_err(|e| format!("生成 ELF 失败: {}", e))?;

        if self.debug {
            eprintln!("AOT(RV64): 可执行文件大小: {} 字节", binary.len());
        }

        Ok(binary)
    }
}
