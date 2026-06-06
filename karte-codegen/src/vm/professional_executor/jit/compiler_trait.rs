//! JIT编译器trait定义
//!
//! 定义了所有JIT编译器必须实现的接口，支持不同目标架构
//!
//! ## 统一抽象架构
//!
//! `JitCompiler` trait 同时包含：
//! - **平台特定方法**（必须实现）：寄存器映射、指令编码等
//! - **共享 default method**：runtime 调用委托、编译流程框架等
//!
//! 所有 runtime 委托函数（alloc/free/retain/release/safepoint/string_*）的
//! 逻辑完全相同——构造 `RuntimeCall` 然后调用 `emit_runtime_call`——因此
//! 作为 default method 实现，每个平台只需实现底层的 emit 方法。

use karte_common::calling_convention::CC;
use karte_lir::{LirFunction, LirProgram, Register};
use std::collections::HashMap;

use super::code_buffer::CodeBuilder;
use super::ffi::{RuntimeCall, RuntimeArg};

/// 运行时调用的上下文信息
///
/// 封装了平台在 runtime call 中可能需要的额外信息。
/// 对于不需要这些信息的平台（如 x86_64），可以传入 None。
#[derive(Clone)]
pub struct RuntimeCallContext<'a> {
    /// 当前指令在函数中的索引（用于查找 instruction_metadata）
    pub instruction_index: usize,
    /// 当前正在编译的函数引用
    pub function: &'a LirFunction,
}

/// JIT编译器trait
///
/// 所有目标架构的编译器都必须实现此trait
///
/// # 平台特定方法（必须实现）
///
/// - `target_architecture()`: 返回架构名称
/// - `get_register_mapping()`: 返回虚拟→物理寄存器映射
/// - `get_calling_convention()`: 返回调用约定信息
/// - `return_register()`: 返回值寄存器编号
/// - `ffi_arg_registers()`: FFI 参数寄存器列表
/// - `emit_mov_reg_reg()`: 寄存器间移动
/// - `emit_mov_reg_imm64()`: 加载立即数到寄存器
/// - `emit_call_to_ptr()`: 调用绝对地址的函数
/// - `save_call_clobbered_registers()`: 保存 caller-saved 寄存器
/// - `restore_call_clobbered_registers()`: 恢复 caller-saved 寄存器
/// - `emit_runtime_call()`: 完整的 runtime call（包含平台特定的 save/restore/调用逻辑）
///
/// # 共享 default method（无需重写）
///
/// - `compile_alloc()`: 通过 `emit_runtime_call` 调用 `alloc`
/// - `compile_free()`: 通过 `emit_runtime_call` 调用 `free`
/// - `compile_retain()` / `compile_release()`: RC 操作
/// - `compile_safepoint()`: GC 安全点
/// - `compile_string_*()`: 字符串操作
/// - `compile_print_*()`: 打印操作
/// - `compile_to_string()`: 类型转换
pub trait JitCompiler: std::fmt::Debug {
    // ==================== 必须实现的方法 ====================

    /// 编译单个函数
    fn compile_function(
        &mut self,
        function: &LirFunction,
        program: &LirProgram,
    ) -> crate::Result<CompiledFunction>;

    /// 获取目标架构名称
    fn target_architecture(&self) -> &'static str;

    /// 获取寄存器映射信息
    fn get_register_mapping(&self) -> &HashMap<Register, u8>;

    /// 是否支持调试符号
    fn supports_debug_info(&self) -> bool {
        false
    }

    /// 获取调用约定信息
    fn get_calling_convention(&self) -> CallingConventionInfo;

    // ==================== 平台特定的 runtime call ====================

    /// 平台特定的 runtime call 实现
    ///
    /// 每个平台必须实现此方法，包含：
    /// 1. 计算 exclude 列表（返回值寄存器）
    /// 2. save caller-saved 寄存器
    /// 3. 传递参数到 FFI 参数寄存器
    /// 4. 调用 runtime 函数
    /// 5. 移动返回值到目标寄存器
    /// 6. restore caller-saved 寄存器
    ///
    /// `ctx` 参数包含平台可能需要的额外信息（如 AArch64 的 instruction_metadata），
    /// 不需要的平台可以忽略。
    fn emit_runtime_call(
        &mut self,
        code_builder: &mut CodeBuilder,
        call: RuntimeCall,
        result: Option<&Register>,
        ctx: Option<RuntimeCallContext<'_>>,
    ) -> crate::Result<()>;

    // ==================== 共享的 runtime 委托函数（default 实现） ====================
    //
    // 以下方法的所有逻辑在所有平台完全相同：
    // 构造 RuntimeCall → 调用 emit_runtime_call
    // 每个平台只需实现 emit_runtime_call 即可

    /// 编译内存分配指令
    fn compile_alloc(
        &mut self,
        dst: &Register,
        size: usize,
        alignment: usize,
        allocation_type: &karte_lir::AllocationType,
        code_builder: &mut CodeBuilder,
        ctx: Option<RuntimeCallContext<'_>>,
    ) -> crate::Result<()> {
        match allocation_type {
            karte_lir::AllocationType::Heap => {
                let call = RuntimeCall::alloc(size, alignment);
                self.emit_runtime_call(code_builder, call, Some(dst), ctx)
            }
            _ => Err(format!(
                "Alloc instruction with unsupported allocation type: {:?}",
                allocation_type
            ).into()),
        }
    }

    /// 编译内存释放指令
    fn compile_free(
        &mut self,
        addr: &Register,
        code_builder: &mut CodeBuilder,
        ctx: Option<RuntimeCallContext<'_>>,
    ) -> crate::Result<()> {
        let call = RuntimeCall::free(*addr);
        self.emit_runtime_call(code_builder, call, None, ctx)
    }

    /// 编译引用计数 retain 指令
    fn compile_retain(
        &mut self,
        value: &Register,
        code_builder: &mut CodeBuilder,
        ctx: Option<RuntimeCallContext<'_>>,
    ) -> crate::Result<()> {
        let call = RuntimeCall::retain(*value);
        self.emit_runtime_call(code_builder, call, None, ctx)
    }

    /// 编译引用计数 release 指令
    fn compile_release(
        &mut self,
        value: &Register,
        code_builder: &mut CodeBuilder,
        ctx: Option<RuntimeCallContext<'_>>,
    ) -> crate::Result<()> {
        let call = RuntimeCall::release(*value);
        self.emit_runtime_call(code_builder, call, None, ctx)
    }

    /// 编译 GC 安全点指令
    fn compile_safepoint(
        &mut self,
        code_builder: &mut CodeBuilder,
        ctx: Option<RuntimeCallContext<'_>>,
    ) -> crate::Result<()> {
        let call = RuntimeCall::gc_safepoint();
        self.emit_runtime_call(code_builder, call, None, ctx)
    }

    /// 编译字符串拼接指令
    fn compile_string_concat(
        &mut self,
        dst: &Register,
        left: &Register,
        right: &Register,
        code_builder: &mut CodeBuilder,
        ctx: Option<RuntimeCallContext<'_>>,
    ) -> crate::Result<()> {
        let call = RuntimeCall::string_concat(*left, *right);
        self.emit_runtime_call(code_builder, call, Some(dst), ctx)
    }

    /// 编译字符串相等比较指令
    fn compile_string_equal(
        &mut self,
        dst: &Register,
        left: &Register,
        right: &Register,
        code_builder: &mut CodeBuilder,
        ctx: Option<RuntimeCallContext<'_>>,
    ) -> crate::Result<()> {
        let call = RuntimeCall::string_equal(*left, *right);
        self.emit_runtime_call(code_builder, call, Some(dst), ctx)
    }

    /// 编译字符串字典序比较指令
    fn compile_string_compare(
        &mut self,
        dst: &Register,
        left: &Register,
        right: &Register,
        code_builder: &mut CodeBuilder,
        ctx: Option<RuntimeCallContext<'_>>,
    ) -> crate::Result<()> {
        let call = RuntimeCall::string_compare(*left, *right);
        self.emit_runtime_call(code_builder, call, Some(dst), ctx)
    }

    /// 编译字符串字符访问指令
    fn compile_string_char_at(
        &mut self,
        dst: &Register,
        str_ptr: &Register,
        index: &Register,
        code_builder: &mut CodeBuilder,
        ctx: Option<RuntimeCallContext<'_>>,
    ) -> crate::Result<()> {
        let call = RuntimeCall::string_char_at(*str_ptr, *index);
        self.emit_runtime_call(code_builder, call, Some(dst), ctx)
    }

    /// 编译字符串子串指令
    fn compile_string_substring(
        &mut self,
        dst: &Register,
        str_ptr: &Register,
        start: &Register,
        length: &Register,
        code_builder: &mut CodeBuilder,
        ctx: Option<RuntimeCallContext<'_>>,
    ) -> crate::Result<()> {
        let call = RuntimeCall::string_substring(*str_ptr, *start, *length);
        self.emit_runtime_call(code_builder, call, Some(dst), ctx)
    }

    /// 编译字符串包含指令
    fn compile_string_contains(
        &mut self,
        dst: &Register,
        str_ptr: &Register,
        char_code: &Register,
        code_builder: &mut CodeBuilder,
        ctx: Option<RuntimeCallContext<'_>>,
    ) -> crate::Result<()> {
        let call = RuntimeCall::string_contains(*str_ptr, *char_code);
        self.emit_runtime_call(code_builder, call, Some(dst), ctx)
    }

    /// 编译字符串分割计数指令
    fn compile_split_count(
        &mut self,
        dst: &Register,
        str_ptr: &Register,
        separator: &Register,
        code_builder: &mut CodeBuilder,
        ctx: Option<RuntimeCallContext<'_>>,
    ) -> crate::Result<()> {
        let call = RuntimeCall::split_count(*str_ptr, *separator);
        self.emit_runtime_call(code_builder, call, Some(dst), ctx)
    }

    /// 编译字符串 trim 指令
    fn compile_trim(
        &mut self,
        dst: &Register,
        str_ptr: &Register,
        code_builder: &mut CodeBuilder,
        ctx: Option<RuntimeCallContext<'_>>,
    ) -> crate::Result<()> {
        let call = RuntimeCall::trim(*str_ptr);
        self.emit_runtime_call(code_builder, call, Some(dst), ctx)
    }

    /// 编译 ASCII 码转字符串指令
    fn compile_char_to_string(
        &mut self,
        dst: &Register,
        value: &Register,
        code_builder: &mut CodeBuilder,
        ctx: Option<RuntimeCallContext<'_>>,
    ) -> crate::Result<()> {
        let call = RuntimeCall::char_to_string(*value);
        self.emit_runtime_call(code_builder, call, Some(dst), ctx)
    }

    /// 编译值转字符串指令
    fn compile_to_string(
        &mut self,
        dst: &Register,
        value: &Register,
        code_builder: &mut CodeBuilder,
        ctx: Option<RuntimeCallContext<'_>>,
    ) -> crate::Result<()> {
        let call = RuntimeCall::to_string(*value);
        self.emit_runtime_call(code_builder, call, Some(dst), ctx)
    }

    /// 编译打印字符串指令
    fn compile_print_string(
        &mut self,
        ptr: &Register,
        code_builder: &mut CodeBuilder,
        ctx: Option<RuntimeCallContext<'_>>,
    ) -> crate::Result<()> {
        let call = RuntimeCall::print_string(*ptr);
        self.emit_runtime_call(code_builder, call, None, ctx)
    }

    /// 编译打印数字指令
    fn compile_print_number(
        &mut self,
        value: &Register,
        code_builder: &mut CodeBuilder,
        ctx: Option<RuntimeCallContext<'_>>,
    ) -> crate::Result<()> {
        let call = RuntimeCall::print_number(*value);
        self.emit_runtime_call(code_builder, call, None, ctx)
    }

    /// 编译打印布尔值指令
    fn compile_print_bool(
        &mut self,
        value: &Register,
        code_builder: &mut CodeBuilder,
        ctx: Option<RuntimeCallContext<'_>>,
    ) -> crate::Result<()> {
        let call = RuntimeCall::print_bool(*value);
        self.emit_runtime_call(code_builder, call, None, ctx)
    }

    /// 编译 panic 指令
    fn compile_panic(
        &mut self,
        code_builder: &mut CodeBuilder,
        ctx: Option<RuntimeCallContext<'_>>,
    ) -> crate::Result<()> {
        let call = RuntimeCall::panic();
        self.emit_runtime_call(code_builder, call, None, ctx)
    }

    // ==================== 共享工具方法 ====================

}

/// 计算运行时调用的 exclude 列表（用于 x86/RISC-V 风格：restore 之后移动返回值）
///
/// 对于在 restore **之后**移动返回值的平台（x86_64、RISC-V），
/// 需要排除返回值寄存器（RAX/a0），否则 restore 会覆盖返回值。
///
/// 对于在 restore **之前**移动返回值的平台（AArch64），
/// 应排除结果目标寄存器，由平台自行计算。
pub fn compute_exclude_return_reg(
    call: &RuntimeCall,
    result: Option<&Register>,
    return_reg: u8,
) -> Vec<u8> {
    if result.is_some() && call.expects_result() {
        vec![return_reg]
    } else {
        vec![]
    }
}

/// 计算运行时调用的 exclude 列表（用于 AArch64 风格：restore 之前移动返回值）
///
/// 对于在 restore **之前**移动返回值的平台（AArch64），
/// 需要排除结果目标物理寄存器，否则 restore 会覆盖已经移动好的返回值。
pub fn compute_exclude_dst_reg(
    call: &RuntimeCall,
    result: Option<&Register>,
    dst_phys_reg: Option<u8>,
) -> Vec<u8> {
    if result.is_some() && call.expects_result() {
        dst_phys_reg.map(|r| vec![r]).unwrap_or_default()
    } else {
        vec![]
    }
}

/// 编译后的函数
#[derive(Debug, Clone)]
pub struct CompiledFunction {
    /// 函数名称
    pub name: String,
    /// 机器码缓冲区（仅用于调试/分配前）
    pub code: MachineCodeBuffer,
    /// 可执行内存基址
    pub exec_mem_ptr: *const u8,
    /// 可执行内存大小
    pub exec_mem_size: usize,
    /// 入口点偏移（相对于可执行内存基址）
    pub entry_offset: usize,
    /// 调试信息（可选）
    pub debug_info: Option<DebugInfo>,
    /// 外部函数引用
    pub external_refs: Vec<ExternalReference>,
    /// 函数内的所有label及其偏移
    pub labels: std::collections::HashMap<String, usize>,
    /// 待修补的跳转
    pub pending_jumps: Vec<crate::vm::professional_executor::jit::code_buffer::PendingJump>,
    /// 待修补的标签地址
    pub pending_label_addresses:
        Vec<crate::vm::professional_executor::jit::code_buffer::PendingLabelAddress>,
    /// 待修补的ADR指令
    pub pending_adrs: Vec<crate::vm::professional_executor::jit::code_buffer::PendingAdr>,
}

impl std::fmt::Display for CompiledFunction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "CompiledFunction {{ name: {}, entry_offset: {:X} }}",
            self.name, self.entry_offset
        )?;
        writeln!(f, "pending_jumps: {:?}", self.pending_jumps)?;
        writeln!(
            f,
            "pending_label_addresses: {:?}",
            self.pending_label_addresses
        )?;
        writeln!(f, "pending_adrs: {:?}", self.pending_adrs)?;
        writeln!(f, "labels: {:?}", self.labels)?;

        writeln!(f, "exec_mem_ptr: {:p}", self.exec_mem_ptr)?;
        writeln!(f, "exec_mem_size: {}", self.exec_mem_size)?;
        // 打印机器码
        writeln!(f, "机器码内容:")?;
        let code = self.machine_code();
        for (i, chunk) in code.chunks(16).enumerate() {
            let hex_part: String = chunk
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect::<Vec<_>>()
                .join(" ");
            let ascii_part: String = chunk
                .iter()
                .map(|&b| if b.is_ascii_graphic() { b as char } else { '.' })
                .collect();
            writeln!(f, "  {:04x}: {:<48} |{}|", i * 16, hex_part, ascii_part)?;
        }
        Ok(())
    }
}

impl CompiledFunction {
    /// 创建新的编译函数
    pub fn new(name: String, code: MachineCodeBuffer, entry_point: usize) -> Self {
        Self {
            name,
            code,
            exec_mem_ptr: std::ptr::null(),
            exec_mem_size: 0,
            entry_offset: 0,
            debug_info: None,
            external_refs: Vec::new(),
            labels: HashMap::new(),
            pending_jumps: Vec::new(),
            pending_label_addresses: Vec::new(),
            pending_adrs: Vec::new(),
        }
    }

    /// 获取代码大小
    pub fn code_size(&self) -> usize {
        self.code.len()
    }

    /// 获取可执行内存中的入口地址
    pub fn get_entry_address(&self) -> *const u8 {
        unsafe { self.exec_mem_ptr.add(self.entry_offset) }
    }

    /// 获取可执行内存slice
    pub fn machine_code(&self) -> &[u8] {
        if !self.exec_mem_ptr.is_null() && self.exec_mem_size > 0 {
            unsafe { std::slice::from_raw_parts(self.exec_mem_ptr, self.exec_mem_size) }
        } else {
            self.code.as_bytes()
        }
    }

    /// 添加外部引用
    pub fn add_external_ref(&mut self, external_ref: ExternalReference) {
        self.external_refs.push(external_ref);
    }

    /// 设置调试信息
    pub fn set_debug_info(&mut self, debug_info: DebugInfo) {
        self.debug_info = Some(debug_info);
    }
}

/// 机器码缓冲区
#[derive(Debug, Clone)]
pub struct MachineCodeBuffer {
    /// 机器码字节
    code: Vec<u8>,
    /// 是否可执行
    executable: bool,
}

impl MachineCodeBuffer {
    /// 创建新的代码缓冲区
    pub fn new() -> Self {
        Self {
            code: Vec::new(),
            executable: false,
        }
    }

    /// 创建带初始容量的代码缓冲区
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            code: Vec::with_capacity(capacity),
            executable: false,
        }
    }

    /// 添加字节到缓冲区
    pub fn push_byte(&mut self, byte: u8) {
        self.code.push(byte);
    }

    /// 添加多个字节
    pub fn push_bytes(&mut self, bytes: &[u8]) {
        self.code.extend_from_slice(bytes);
    }

    /// 获取代码长度
    pub fn len(&self) -> usize {
        self.code.len()
    }

    /// 检查是否为空
    pub fn is_empty(&self) -> bool {
        self.code.is_empty()
    }

    /// 获取代码字节引用
    pub fn as_bytes(&self) -> &[u8] {
        &self.code
    }

    /// 设置为可执行
    pub fn make_executable(&mut self) -> crate::Result<()> {
        if self.executable {
            return Ok(());
        }

        // 在实际实现中，这里会调用系统API（如mprotect）
        // 使内存页可执行。现在简单标记为可执行
        self.executable = true;
        Ok(())
    }

    /// 检查是否可执行
    pub fn is_executable(&self) -> bool {
        self.executable
    }

    /// 获取当前位置
    pub fn position(&self) -> usize {
        self.code.len()
    }

    /// 在指定位置写入字节
    pub fn write_at(&mut self, position: usize, byte: u8) -> crate::Result<()> {
        if position < self.code.len() {
            self.code[position] = byte;
            Ok(())
        } else {
            Err(format!(
                "写入位置 {} 超出缓冲区范围 {}",
                position,
                self.code.len()
            ).into())
        }
    }

    /// 在指定位置写入多个字节
    pub fn write_bytes_at(&mut self, position: usize, bytes: &[u8]) -> crate::Result<()> {
        if position + bytes.len() <= self.code.len() {
            self.code[position..position + bytes.len()].copy_from_slice(bytes);
            Ok(())
        } else {
            Err(format!(
                "写入位置 {}+{} 超出缓冲区范围 {}",
                position,
                bytes.len(),
                self.code.len()
            ).into())
        }
    }
}

impl Default for MachineCodeBuffer {
    fn default() -> Self {
        Self::new()
    }
}

/// 调用约定信息
#[derive(Debug, Clone)]
pub struct CallingConventionInfo {
    /// 参数寄存器
    pub parameter_registers: Vec<u8>,
    /// 返回值寄存器
    pub return_register: u8,
    /// 栈指针寄存器
    pub stack_pointer: u8,
    /// 帧指针寄存器
    pub frame_pointer: u8,
    /// 调用者保存寄存器
    pub caller_saved: Vec<u8>,
    /// 被调用者保存寄存器
    pub callee_saved: Vec<u8>,
}

impl CC for CallingConventionInfo {
    fn is_caller_saved(&self, reg: u8) -> bool {
        self.caller_saved.contains(&reg)
    }

    fn is_callee_saved(&self, reg: u8) -> bool {
        self.callee_saved.contains(&reg)
    }
}

/// 调试信息
#[derive(Debug, Clone)]
pub struct DebugInfo {
    /// 源代码行号映射
    pub line_map: HashMap<usize, usize>, // 机器码偏移 -> 源代码行号
    /// 变量信息
    pub variables: Vec<VariableInfo>,
}

/// 变量信息
#[derive(Debug, Clone)]
pub struct VariableInfo {
    /// 变量名
    pub name: String,
    /// 寄存器或栈偏移
    pub location: VariableLocation,
    /// 生命周期（机器码偏移范围）
    pub scope: (usize, usize),
}

/// 变量位置
#[derive(Debug, Clone)]
pub enum VariableLocation {
    /// 在寄存器中
    Register(u8),
    /// 在栈上（相对于帧指针的偏移）
    Stack(i32),
}

/// 外部函数引用
#[derive(Debug, Clone)]
pub struct ExternalReference {
    /// 函数名称
    pub name: String,
    /// 需要修补的代码位置
    pub patch_offset: usize,
    /// 引用类型
    pub reference_type: ReferenceType,
}

/// 引用类型
#[derive(Debug, Clone)]
pub enum ReferenceType {
    /// 直接调用（需要32位相对地址）
    DirectCall,
    /// 间接调用（需要64位绝对地址）
    IndirectCall,
    /// 数据引用
    DataReference,
}
