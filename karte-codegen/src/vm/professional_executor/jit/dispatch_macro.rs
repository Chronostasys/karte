// JIT 编译器共享的 dispatch 宏
//
// 消除三个平台编译器（x86_64 / AArch64 / RISC-V）之间
// `compile_instruction` match arm 的重复代码。
//
// 使用方式：在各编译器文件中 include!("dispatch_macro.rs");

/// 生成 `compile_instruction` 的完整 match 表达式
///
/// - `$self`: 编译器实例
/// - `$instr`: 指令引用
/// - `$cb`: CodeBuilder 可变引用
/// - `$is_main`: 是否为 main 函数
/// - `$ctx_expr`: RuntimeCallContext 构造表达式（`None` 或 `Some(ctx)`）
/// - `$arch_name`: 架构名称（用于 wildcard 错误消息）
#[allow(unused_macro_rules)]
macro_rules! dispatch_compile_instruction {
    ($self:expr, $instr:expr, $cb:expr, $is_main:expr, $ctx_expr:expr, $arch_name:expr) => {
        match $instr {
            // ==================== 算术/逻辑指令 ====================
            Instruction::Move { dst, src, .. } => $self.compile_move(dst, src, $cb),
            Instruction::Add { dst, src1, src2, .. } => {
                $self.compile_add(dst, src1, src2, $cb)
            }
            Instruction::Sub { dst, src1, src2, .. } => {
                $self.compile_sub(dst, src1, src2, $cb)
            }
            Instruction::Mul { dst, src1, src2, .. } => {
                $self.compile_mul(dst, src1, src2, $cb)
            }
            Instruction::Div { dst, src1, src2, .. } => {
                $self.compile_div(dst, src1, src2, $cb)
            }
            Instruction::Mod { dst, src1, src2, .. } => {
                $self.compile_mod(dst, src1, src2, $cb)
            }
            Instruction::BitAnd { dst, src1, src2, .. } => {
                $self.compile_bitand(dst, src1, src2, $cb)
            }
            Instruction::BitOr { dst, src1, src2, .. } => {
                $self.compile_bitor(dst, src1, src2, $cb)
            }
            Instruction::BitXor { dst, src1, src2, .. } => {
                $self.compile_bitxor(dst, src1, src2, $cb)
            }
            Instruction::ShiftLeft { dst, src1, src2, .. } => {
                $self.compile_shift_left(dst, src1, src2, $cb)
            }
            Instruction::ShiftRight { dst, src1, src2, .. } => {
                $self.compile_shift_right(dst, src1, src2, $cb)
            }
            Instruction::BitNot { dst, src, .. } => {
                $self.compile_bitnot(dst, src, $cb)
            }

            // ==================== 比较/分支指令 ====================
            Instruction::Compare { src1, src2, .. } => {
                $self.compile_compare(src1, src2, $cb)
            }
            Instruction::CompareSet { dst, condition, src1, src2, .. } => {
                $self.compile_compare_set(dst, condition, src1, src2, $cb)
            }
            Instruction::Jump { target, .. } => $self.compile_jump(target, $cb),
            Instruction::JumpEqual { target, .. } => {
                $self.compile_conditional_jump(JumpType::ConditionalEqual, target, $cb)
            }
            Instruction::JumpNotEqual { target, .. } => {
                $self.compile_conditional_jump(JumpType::ConditionalNotEqual, target, $cb)
            }
            Instruction::JumpLess { target, .. } => {
                $self.compile_conditional_jump(JumpType::ConditionalLess, target, $cb)
            }
            Instruction::JumpLessEqual { target, .. } => {
                $self.compile_conditional_jump(JumpType::ConditionalLessEqual, target, $cb)
            }
            Instruction::JumpGreater { target, .. } => {
                $self.compile_conditional_jump(JumpType::ConditionalGreater, target, $cb)
            }
            Instruction::JumpGreaterEqual { target, .. } => {
                $self.compile_conditional_jump(JumpType::ConditionalGreaterEqual, target, $cb)
            }

            // ==================== 函数调用/返回 ====================
            Instruction::Call { target, .. } => $self.compile_call(target, $cb),
            Instruction::JumpIndirect { function_register, .. } => {
                $self.compile_jump_indirect(function_register, $cb)
            }
            Instruction::JumpRegister { target_register, .. } => {
                $self.compile_jump_register(target_register, $cb)
            }
            Instruction::Return { value, .. } => {
                $self.compile_return(value.as_ref(), $cb, $is_main)
            }

            // ==================== 标签 ====================
            Instruction::Label { id, .. } => {
                let label_name = format!("label_{}", id.0);
                $cb.define_label(&label_name)?;
                Ok(())
            }

            // ==================== 内存指令 ====================
            Instruction::Load64 { dst, addr, offset, .. } => {
                $self.compile_load64(dst, addr, *offset, $cb)
            }
            Instruction::Store64 { addr, offset, src, .. } => {
                $self.compile_store64(addr, *offset, src, $cb)
            }
            Instruction::Load32 { dst, addr, offset, .. } => {
                $self.compile_load32(dst, addr, *offset, $cb)
            }
            Instruction::Store32 { addr, offset, src, .. } => {
                $self.compile_store32(addr, *offset, src, $cb)
            }
            Instruction::Load8 { dst, addr, offset, .. } => {
                $self.compile_load8(dst, addr, *offset, $cb)
            }
            Instruction::Store8 { addr, offset, src, .. } => {
                $self.compile_store8(addr, *offset, src, $cb)
            }
            Instruction::StorePair { addr, offset, src1, src2, .. } => {
                $self.compile_store64(addr, *offset, &Operand::Register { id: *src1 }, $cb)?;
                $self.compile_store64(addr, *offset + 8, &Operand::Register { id: *src2 }, $cb)
            }
            Instruction::LoadPair { dst1, dst2, addr, offset, .. } => {
                $self.compile_load64(dst1, addr, *offset, $cb)?;
                $self.compile_load64(dst2, addr, *offset + 8, $cb)
            }

            // ==================== Runtime 委托（使用 trait default method） ====================
            Instruction::Alloc { dst, size, alignment, allocation_type, .. } => {
                $self.compile_alloc(dst, *size, *alignment, allocation_type, $cb, $ctx_expr)
            }
            Instruction::Free { addr, .. } => {
                $self.compile_free(addr, $cb, $ctx_expr)
            }
            Instruction::Retain { value, .. } => {
                $self.compile_retain(value, $cb, $ctx_expr)
            }
            Instruction::Release { value, .. } => {
                $self.compile_release(value, $cb, $ctx_expr)
            }
            Instruction::Safepoint { .. } => {
                $self.compile_safepoint($cb, $ctx_expr)
            }

            // ==================== 字符串操作 ====================
            Instruction::StringConcat { dst, left, right, .. } => {
                $self.compile_string_concat(dst, left, right, $cb, $ctx_expr)
            }
            Instruction::StringEqual { dst, left, right, .. } => {
                $self.compile_string_equal(dst, left, right, $cb, $ctx_expr)
            }
            Instruction::StringCharAt { dst, str_ptr, index, .. } => {
                $self.compile_string_char_at(dst, str_ptr, index, $cb, $ctx_expr)
            }
            Instruction::StringSubstring { dst, str_ptr, start, length, .. } => {
                $self.compile_string_substring(dst, str_ptr, start, length, $cb, $ctx_expr)
            }
            Instruction::StringContains { dst, str_ptr, char_code, .. } => {
                $self.compile_string_contains(dst, str_ptr, char_code, $cb, $ctx_expr)
            }
            Instruction::SplitCount { dst, str_ptr, separator, .. } => {
                $self.compile_split_count(dst, str_ptr, separator, $cb, $ctx_expr)
            }
            Instruction::Trim { dst, str_ptr, .. } => {
                $self.compile_trim(dst, str_ptr, $cb, $ctx_expr)
            }
            Instruction::ToString { dst, value, .. } => {
                $self.compile_to_string(dst, value, $cb, $ctx_expr)
            }

            // ==================== 打印操作 ====================
            Instruction::PrintString { ptr, .. } => {
                $self.compile_print_string(ptr, $cb, $ctx_expr)
            }
            Instruction::PrintNumber { value, .. } => {
                $self.compile_print_number(value, $cb, $ctx_expr)
            }
            Instruction::PrintBool { value, .. } => {
                $self.compile_print_bool(value, $cb, $ctx_expr)
            }

            Instruction::Panic { .. } => {
                $self.compile_panic($cb, $ctx_expr)
            }

            // ==================== 其他指令 ====================
            Instruction::StructAlloc { .. } => {
                Err("StructAlloc 应该已经被降级为 Alloc".into())
            }
            Instruction::StructFieldLoad { .. }
            | Instruction::StructFieldStore { .. }
            | Instruction::StructFieldAddr { .. } => {
                Err(format!("Struct 操作应该已经被降级: {:?}", $instr).into())
            }
            Instruction::MemCopy { .. } => {
                Err("MemCopy 应该已经被降级为多条 Load64/Store64".into())
            }
            Instruction::LoadGlobal { dst, name, .. } => {
                $self.compile_load_global(dst, name, $cb)
            }
            Instruction::GcRegOp { is_push, .. } => {
                $self.compile_gc_reg_op(*is_push, $cb)
            }
            Instruction::Phi { .. } => {
                log::warn!("Phi 指令出现在 JIT 编译阶段，这表明 SSA 降级不完整");
                Ok(())
            }

            // ==================== 平台特定指令 ====================
            _ => Err(format!(
                "不支持的{}指令类型: {:?}",
                $arch_name, $instr
            ).into()),
        }
    };
}

/// 从 CodeBuilder 构建 CompiledFunction 的共享逻辑
///
/// 统一三个编译器中 `compile_function` 后半部分完全相同的步骤：
/// 1. 导出 labels、pending_jumps、pending_label_addresses、pending_adrs
/// 2. finalize 机器码
/// 3. 构建 CompiledFunction 并填充元数据
///
/// # 参数
///
/// - `$cb`: CodeBuilder 可变引用
/// - `$func_name`: 函数名字符串
/// - `$arch_name`: 架构名称（用于日志）
macro_rules! finalize_compiled_function {
    ($cb:expr, $func_name:expr, $arch_name:expr) => {{
        let labels = $cb.exported_labels().clone();
        let pending_jumps = $cb.exported_pending_jumps().clone();
        let pending_label_addresses = $cb.exported_pending_label_addresses().clone();
        let pending_adrs = $cb.exported_pending_adrs().clone();

        let machine_code = $cb.finalize()?;

        let mut compiled_function = $crate::vm::professional_executor::jit::compiler_trait::CompiledFunction::new(
            $func_name.to_string(),
            machine_code,
            0,
        );

        compiled_function.labels = labels;
        compiled_function.pending_jumps = pending_jumps;
        compiled_function.pending_label_addresses = pending_label_addresses;
        compiled_function.pending_adrs = pending_adrs;

        log::info!(
            "{}: 函数 '{}' 编译完成，机器码大小: {} 字节",
            $arch_name,
            $func_name,
            compiled_function.code_size()
        );

        compiled_function
    }};
}
