//! JIT编译器trait定义
//!
//! 定义了所有JIT编译器必须实现的接口，支持不同目标架构

use karte_common::calling_convention::CC;
use karte_lir::{LirFunction, LirProgram, Register};
use std::collections::HashMap;

/// JIT编译器trait
///
/// 所有目标架构的编译器都必须实现此trait
pub trait JitCompiler: std::fmt::Debug {
    /// 编译单个函数
    ///
    /// 🔧 优化：现在只需调用此方法一次，然后使用 patch_executable_memory 进行原地修补
    /// compile_function_with_global_labels 已被移除以消除二次编译开销
    fn compile_function(
        &mut self,
        function: &LirFunction,
        program: &LirProgram,
    ) -> Result<CompiledFunction, String>;

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
    pub fn make_executable(&mut self) -> Result<(), String> {
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
    pub fn write_at(&mut self, position: usize, byte: u8) -> Result<(), String> {
        if position < self.code.len() {
            self.code[position] = byte;
            Ok(())
        } else {
            Err(format!(
                "写入位置 {} 超出缓冲区范围 {}",
                position,
                self.code.len()
            ))
        }
    }

    /// 在指定位置写入多个字节
    pub fn write_bytes_at(&mut self, position: usize, bytes: &[u8]) -> Result<(), String> {
        if position + bytes.len() <= self.code.len() {
            self.code[position..position + bytes.len()].copy_from_slice(bytes);
            Ok(())
        } else {
            Err(format!(
                "写入位置 {}+{} 超出缓冲区范围 {}",
                position,
                bytes.len(),
                self.code.len()
            ))
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
