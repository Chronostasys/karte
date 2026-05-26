//! AOT 编译器 — 编排编译流程
//!
//! 接收 LirProgram，复用现有 JIT 编译器后端生成机器码，
//! 与运行时代码合并，生成 ELF 可执行文件。

use crate::elf::{ElfArch, ElfWriter};
use crate::runtime_x86::X86Runtime;

use karte_codegen::vm::professional_executor::jit::{
    JitCompiler, X86Compiler,
};
use karte_lir::LirProgram;

use std::collections::HashMap;
use std::io::Write;

/// AOT 编译器
pub struct AotCompiler {
    debug: bool,
}

impl AotCompiler {
    pub fn new(debug: bool) -> Self {
        Self { debug }
    }

    /// 编译 LirProgram 为字节
    pub fn compile_to_bytes(&self, program: &LirProgram) -> Result<Vec<u8>, String> {
        #[cfg(target_arch = "x86_64")]
        return self.compile_x86_64(program);

        #[cfg(target_arch = "aarch64")]
        return self.compile_aarch64(program);
    }

    /// x86_64 AOT 编译
    fn compile_x86_64(&self, program: &LirProgram) -> Result<Vec<u8>, String> {
        // ---- 1. 生成运行时代码 ----
        let mut runtime = X86Runtime::new().generate();
        runtime.patch_internal_calls();
        let runtime_code = runtime.code.clone();
        let runtime_size = runtime_code.len();

        if self.debug {
            eprintln!("AOT: 运行时代码大小: {} 字节", runtime_size);
            for f in &runtime.functions {
                if f.size > 0 {
                    eprintln!("  {} @ offset={}, size={}", f.name, f.offset, f.size);
                }
            }
        }

        // ---- 2. 编译 Karte 函数 ----
        let mut compiler = X86Compiler::new(self.debug)
            .map_err(|e| format!("创建 x86 编译器失败: {}", e))?;

        let mut compiled_functions = Vec::new();
        let mut karte_code: Vec<u8> = Vec::new();
        let mut function_offsets: HashMap<String, usize> = HashMap::new();

        // 编译顺序: main 放最后
        let main_name = program.main_function.as_deref().unwrap_or("main");
        let mut func_names: Vec<String> = program.functions.keys().cloned().collect();
        func_names.sort_by(|a, b| {
            if a == main_name { std::cmp::Ordering::Greater }
            else if b == main_name { std::cmp::Ordering::Less }
            else { std::cmp::Ordering::Equal }
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

        // ---- 3. 构建全局标签表 ----
        let code_base: u64 = 0x400000;

        let mut global_labels: HashMap<String, u64> = HashMap::new();

        // 运行时函数地址（使用常量，保证与 runtime_x86.rs 中 fn_start 注册的名字一致）
        if let Some(off) = runtime.find_offset(crate::runtime_x86::runtime_names::GC_ALLOC_ALIGNED) {
            global_labels.insert("karte_jit_runtime_alloc_aligned".to_string(), code_base + off as u64);
            global_labels.insert("karte_jit_runtime_alloc".to_string(), code_base + off as u64);
        }
        if let Some(off) = runtime.find_offset(crate::runtime_x86::runtime_names::FREE) {
            global_labels.insert("karte_jit_runtime_free".to_string(), code_base + off as u64);
        }
        if let Some(off) = runtime.find_offset(crate::runtime_x86::runtime_names::RETAIN) {
            global_labels.insert("karte_jit_runtime_retain".to_string(), code_base + off as u64);
        }
        if let Some(off) = runtime.find_offset(crate::runtime_x86::runtime_names::RELEASE) {
            global_labels.insert("karte_jit_runtime_release".to_string(), code_base + off as u64);
        }
        if let Some(off) = runtime.find_offset(crate::runtime_x86::runtime_names::GC_SAFEPOINT) {
            global_labels.insert("karte_jit_runtime_gc_safepoint".to_string(), code_base + off as u64);
        }
        if let Some(off) = runtime.find_offset(crate::runtime_x86::runtime_names::GC_UPDATE_STACK_TOP) {
            global_labels.insert("karte_jit_runtime_update_stack_top".to_string(), code_base + off as u64);
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

        // ---- 4. 修补运行时调用 (MOV RAX, imm64; CALL RAX 模式) ----
        // JIT 生成的代码中运行时调用使用绝对地址, 需要替换为 AOT 运行时地址
        let mut runtime_ptr_map: HashMap<u64, u64> = HashMap::new();
        {
            use karte_codegen::vm::professional_executor::jit::ffi::RuntimeIntrinsic;

            let alloc_ptr = RuntimeIntrinsic::AllocAligned.symbol_ptr() as u64;
            let free_ptr = RuntimeIntrinsic::Free.symbol_ptr() as u64;
            let retain_ptr = RuntimeIntrinsic::Retain.symbol_ptr() as u64;
            let release_ptr = RuntimeIntrinsic::Release.symbol_ptr() as u64;
            let safepoint_ptr = RuntimeIntrinsic::GcSafepoint.symbol_ptr() as u64;

            // update_stack_top 不在 RuntimeIntrinsic 中, 直接获取
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
        let main_offset = function_offsets.get(main_name)
            .ok_or_else(|| format!("未找到主函数 '{}'", main_name))?;
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

    /// AArch64 AOT 编译 (TODO)
    fn compile_aarch64(&self, _program: &LirProgram) -> Result<Vec<u8>, String> {
        Err("AArch64 AOT 编译尚未实现".to_string())
    }
}
