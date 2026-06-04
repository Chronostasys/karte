//! JIT 编译器共享的工具函数
//!
//! 包含所有平台无关的工具函数，供 x86_64 / AArch64 / RISC-V 编译器共同使用。

use karte_lir::{Instruction, LirFunction, LirProgram, Operand, Register};

/// 对齐到指定边界
///
/// 将 value 向上对齐到 alignment 的整数倍。
/// 例如: align_to(13, 16) = 16, align_to(32, 16) = 32
pub fn align_to(value: usize, alignment: usize) -> usize {
    ((value + alignment - 1) / alignment) * alignment
}

/// 计算函数需要的栈帧空间
///
/// 从 StackFrameLayoutPass 生成的 `Add FP, offset` 指令推断最大偏移量，
/// 然后对齐到 16 字节。prologue 不使用此值分配帧空间——由 LIR 的 `Sub vm_sp, N` 指令分配。
/// 此函数仅用于 epilogue 中恢复 vm_sp 时需要跳过的帧区域大小。
pub fn compute_stack_frame_size(function: &LirFunction, frame_pointer_reg: u8) -> usize {
    let mut max_offset = 0i64;
    for inst in &function.instructions {
        if let Instruction::Add { src1, src2, .. } = inst {
            if let Operand::Register { id: Register::Physical(fp) } = src1 {
                if *fp == frame_pointer_reg {
                    if let Operand::Immediate { value } = src2 {
                        if *value < 0 {
                            max_offset = max_offset.max(-*value);
                        }
                    }
                }
            }
        }
    }
    align_to(max_offset as usize, 16)
}

/// 判断给定函数名是否为程序的入口函数
///
/// 入口函数判断逻辑：
/// 1. 如果 `program.main_function` 有值，与之比较
/// 2. 否则 fallback 到字面 "main" 或 `SCRIPT_ENTRY_POINT`
pub fn is_entry_function(name: &str, program: &LirProgram) -> bool {
    if let Some(main) = &program.main_function {
        if main == name {
            return true;
        }
    }

    name == "main" || name == karte_mir::lower::SCRIPT_ENTRY_POINT
}
