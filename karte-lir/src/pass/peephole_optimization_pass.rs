//! 窥孔优化Pass
//!
//! 负责优化局部指令序列，包括：
//! - 合并连续的push/pop操作为StorePair/LoadPair
//! - 优化栈对齐
//! - 移除冗余的NOP指令

use crate::pass::{AnalysisManager, FunctionPass, PassResult};
use crate::{Instruction, LirFunction, Operand, Register};
use karte_common::calling_convention::CallingConvention;
use karte_diagnostics::Span;

/// 窥孔优化Pass
#[derive(Debug)]
pub struct PeepholeOptimizationPass {
    /// 栈指针寄存器
    stack_pointer_reg: Register,
    /// 栈对齐要求
    stack_alignment: usize,
}

impl PeepholeOptimizationPass {
    pub fn new(stack_alignment: usize) -> Self {
        let calling_convention = CallingConvention::standard();
        Self {
            stack_pointer_reg: Register::Physical(calling_convention.stack_pointer),
            stack_alignment,
        }
    }

    fn optimize_instructions(&mut self, instructions: &mut Vec<Instruction>) -> Result<(), String> {
        let mut i = 0;

        while i < instructions.len() {
            // 查找连续的push操作
            if let Some(push_len) = self.detect_push_sequence(instructions, i) {
                if push_len >= 4 {
                    // 至少两个push操作才优化
                    self.optimize_push_sequence(instructions, i, push_len);
                    // 继续处理当前位置，不增加i
                    continue;
                }
            }

            // 查找连续的pop操作
            if let Some(pop_len) = self.detect_pop_sequence(instructions, i) {
                if pop_len >= 4 {
                    // 至少两个pop操作才优化
                    self.optimize_pop_sequence(instructions, i, pop_len);
                    continue;
                }
            }

            i += 1;
        }

        Ok(())
    }

    /// 检测连续的push操作 (Sub+Store64序列)
    fn detect_push_sequence(&self, instructions: &[Instruction], start: usize) -> Option<usize> {
        if start + 1 >= instructions.len() {
            return None;
        }

        let mut count = 0;
        let mut i = start;

        while i + 1 < instructions.len() {
            match (&instructions[i], &instructions[i + 1]) {
                (
                    Instruction::Sub {
                        dst,
                        src1,
                        src2: Operand::Immediate { value: 8 },
                        ..
                    },
                    Instruction::Store64 {
                        addr, offset: 0, ..
                    },
                ) if *dst == *addr && {
                    if let Operand::Register { id } = src1 {
                        dst == id
                    } else {
                        false
                    }
                } =>
                {
                    count += 2;
                    i += 2;
                }
                _ => break,
            }
        }

        if count >= 4 {
            Some(count)
        } else {
            None
        }
    }

    /// 检测连续的pop操作 (Load64+Add序列)
    fn detect_pop_sequence(&self, instructions: &[Instruction], start: usize) -> Option<usize> {
        if start + 1 >= instructions.len() {
            return None;
        }

        let mut count = 0;
        let mut i = start;

        while i + 1 < instructions.len() {
            match (&instructions[i], &instructions[i + 1]) {
                (
                    Instruction::Load64 {
                        addr, offset: 0, ..
                    },
                    Instruction::Add {
                        dst,
                        src1,
                        src2: Operand::Immediate { value: 8 },
                        ..
                    },
                ) if {
                    if let Operand::Register { id } = src1 {
                        addr == id && dst == addr
                    } else {
                        false
                    }
                } =>
                {
                    count += 2;
                    i += 2;
                }
                _ => break,
            }
        }

        if count >= 4 {
            Some(count)
        } else {
            None
        }
    }

    /// 优化push序列为StorePair
    fn optimize_push_sequence(
        &self,
        instructions: &mut Vec<Instruction>,
        start: usize,
        len: usize,
    ) {
        let num_pushes = len / 2;
        let mut registers = Vec::new();

        // 提取所有被保存的寄存器
        for i in 0..num_pushes {
            if let Instruction::Store64 { src, .. } = &instructions[start + i * 2 + 1] {
                if let Operand::Register { id } = src {
                    registers.push(*id);
                }
            }
        }

        // 删除原始指令
        instructions.drain(start..start + len);

        // 生成优化的指令
        let mut insert_pos = start;

        // 先调整栈指针对齐到16字节边界
        let total_size = registers.len() * 8;
        let aligned_size = (total_size + self.stack_alignment - 1) & !(self.stack_alignment - 1);
        let padding = aligned_size - total_size;

        if padding > 0 {
            instructions.insert(
                insert_pos,
                Instruction::Sub {
                    dst: self.stack_pointer_reg,
                    src1: Operand::Register {
                        id: self.stack_pointer_reg,
                    },
                    src2: Operand::Immediate {
                        value: padding as i64,
                    },
                    span: Span::default(),
                },
            );
            insert_pos += 1;
        }

        // 使用StorePair优化连续的寄存器对
        let mut i = 0;
        while i + 1 < registers.len() {
            // StorePair
            instructions.insert(
                insert_pos,
                Instruction::StorePair {
                    addr: self.stack_pointer_reg,
                    offset: 0,
                    src1: registers[i],
                    src2: registers[i + 1],
                    span: Span::default(),
                },
            );

            // Sub 16
            instructions.insert(
                insert_pos + 1,
                Instruction::Sub {
                    dst: self.stack_pointer_reg,
                    src1: Operand::Register {
                        id: self.stack_pointer_reg,
                    },
                    src2: Operand::Immediate { value: 16 },
                    span: Span::default(),
                },
            );

            insert_pos += 2;
            i += 2;
        }

        // 处理奇数个寄存器的情况
        if registers.len() % 2 == 1 {
            let last_reg = registers[registers.len() - 1];
            instructions.insert(
                insert_pos,
                Instruction::Sub {
                    dst: self.stack_pointer_reg,
                    src1: Operand::Register {
                        id: self.stack_pointer_reg,
                    },
                    src2: Operand::Immediate { value: 8 },
                    span: Span::default(),
                },
            );
            instructions.insert(
                insert_pos + 1,
                Instruction::Store64 {
                    addr: self.stack_pointer_reg,
                    offset: 0,
                    src: Operand::Register { id: last_reg },
                    span: Span::default(),
                },
            );
        }
    }

    /// 优化pop序列为LoadPair
    fn optimize_pop_sequence(&self, instructions: &mut Vec<Instruction>, start: usize, len: usize) {
        let num_pops = len / 2;
        let mut registers = Vec::new();

        // 提取所有被加载的寄存器
        for i in 0..num_pops {
            if let Instruction::Load64 { dst, .. } = &instructions[start + i * 2] {
                registers.push(*dst);
            }
        }

        // 删除原始指令
        instructions.drain(start..start + len);

        // 生成优化的指令（逆序，因为是pop）
        let mut insert_pos = start;
        let registers_len = registers.len();
        let mut i = registers_len;

        while i >= 2 {
            i -= 2;

            // LoadPair
            instructions.insert(
                insert_pos,
                Instruction::LoadPair {
                    dst1: registers[i],
                    dst2: registers[i + 1],
                    addr: self.stack_pointer_reg,
                    offset: 0,
                    span: Span::default(),
                },
            );

            // Add 16
            instructions.insert(
                insert_pos + 1,
                Instruction::Add {
                    dst: self.stack_pointer_reg,
                    src1: Operand::Register {
                        id: self.stack_pointer_reg,
                    },
                    src2: Operand::Immediate { value: 16 },
                    span: Span::default(),
                },
            );
        }

        // 处理奇数个寄存器的情况
        if registers_len % 2 == 1 {
            instructions.insert(
                insert_pos,
                Instruction::Load64 {
                    dst: registers[0],
                    addr: self.stack_pointer_reg,
                    offset: 0,
                    span: Span::default(),
                },
            );
            instructions.insert(
                insert_pos + 1,
                Instruction::Add {
                    dst: self.stack_pointer_reg,
                    src1: Operand::Register {
                        id: self.stack_pointer_reg,
                    },
                    src2: Operand::Immediate { value: 8 },
                    span: Span::default(),
                },
            );
        }

        // 恢复栈指针对齐
        let total_size = registers_len * 8;
        let aligned_size = (total_size + self.stack_alignment - 1) & !(self.stack_alignment - 1);
        let padding = aligned_size - total_size;

        if padding > 0 {
            let last_pos = instructions.len();
            instructions.insert(
                last_pos,
                Instruction::Add {
                    dst: self.stack_pointer_reg,
                    src1: Operand::Register {
                        id: self.stack_pointer_reg,
                    },
                    src2: Operand::Immediate {
                        value: padding as i64,
                    },
                    span: Span::default(),
                },
            );
        }
    }
}

impl FunctionPass for PeepholeOptimizationPass {
    fn name(&self) -> &str {
        "peephole_optimization"
    }

    fn description(&self) -> &str {
        "窥孔优化：优化局部指令序列，合并push/pop为StorePair/LoadPair"
    }

    fn run_on_function(
        &mut self,
        function: &mut LirFunction,
        _analyses: &mut AnalysisManager,
    ) -> PassResult {
        // 优化指令序列
        if let Err(e) = self.optimize_instructions(&mut function.instructions) {
            return PassResult::Failed(e);
        }

        PassResult::Changed
    }
}
