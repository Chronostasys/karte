//! JIT 编译器共享上下文
//!
//! 提供所有架构共享的编译器状态和方法骨架，
//! 具体架构（x86_64, AArch64）通过实现 trait 来提供架构特定的指令编码。

use karte_lir::{LirFunction, LirProgram};

/// 编译器共享上下文
///
/// 包含所有架构的 JIT 编译器共享的状态
#[derive(Debug, Clone)]
pub struct CompilerContext {
    /// 当前函数使用的寄存器列表
    pub current_function_use_regs: Vec<u8>,
    /// 当前函数的栈帧大小（由 StackFrameLayoutPass 计算）
    pub current_stack_frame_size: usize,
    /// epilogue 需要跳过的帧大小
    pub stack_frame_size_for_epilogue: usize,
    /// 当前编译的函数名
    pub current_function_name: String,
    /// 调试模式
    pub debug_mode: bool,
}

impl CompilerContext {
    pub fn new(debug_mode: bool) -> Self {
        Self {
            current_function_use_regs: Vec::new(),
            current_stack_frame_size: 0,
            stack_frame_size_for_epilogue: 0,
            current_function_name: String::new(),
            debug_mode,
        }
    }

    /// 重置为编译新函数的状态
    pub fn reset_for_function(&mut self) {
        self.current_function_use_regs.clear();
        self.current_stack_frame_size = 0;
        self.stack_frame_size_for_epilogue = 0;
        self.current_function_name.clear();
    }

    /// 设置编译新函数的状态
    ///
    /// 从 LirFunction 提取函数名、使用寄存器、栈帧大小等信息。
    /// 在 compile_function 开始时调用。
    pub fn setup_for_function(&mut self, function: &LirFunction) {
        self.current_function_name = function.name.clone();
        self.current_function_use_regs = function.get_used_regs().to_vec();
        // prologue 不分配帧空间——由 LIR 的 Sub vm_sp, N 指令分配
        self.current_stack_frame_size = 0;
        // epilogue 需要知道帧大小来跳过帧区域
        self.stack_frame_size_for_epilogue = function.stack_frame_size as usize;
    }

    /// 判断是否为入口函数（main 或脚本入口点）
    pub fn is_entry_function(&self, function_name: &str, program: &LirProgram) -> bool {
        if let Some(main) = &program.main_function {
            if main == function_name {
                return true;
            }
        }
        function_name == "main" || function_name == karte_mir::lower::SCRIPT_ENTRY_POINT
    }

    /// 生成函数入口标签名
    pub fn func_label(&self) -> String {
        format!("func_{}", self.current_function_name)
    }

    /// 生成 LIR 标签名
    pub fn label_name(&self, label_id: u64) -> String {
        format!("label_{}", label_id)
    }
}
