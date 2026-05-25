//! Effect指令降级Pass
//!
//! 负责降级代数效应相关的指令，包括：
//! - EffectPushHandler
//! - EffectPopHandler
//! - EffectPerform
//! - EffectResume

use crate::pass::{AnalysisManager, FunctionPass, PassResult};
use crate::{Instruction, LirFunction, Operand, Register};
use karte_common::calling_convention::CallingConvention;
use karte_diagnostics::Span;
use std::collections::HashMap;

/// Effect指令降级Pass
#[derive(Debug)]
pub struct EffectLoweringPass {
    /// 调用约定
    calling_convention: CallingConvention,
    /// 最近一次 EffectPerform 的结果寄存器（用于在 pop 后生成正确的返回路径）
    last_effect_perform_result: Option<Register>,
    /// 是否已插入提前返回（避免再次生成 Return 序言/尾声）
    inserted_early_return: bool,
    /// 下一个可用的虚拟寄存器ID
    next_register: usize,
}

impl EffectLoweringPass {
    pub fn new() -> Self {
        Self {
            calling_convention: CallingConvention::standard(),
            last_effect_perform_result: None,
            inserted_early_return: false,
            next_register: 40,
        }
    }

    /// 获取effect栈指针寄存器
    fn effect_stack_register(&self) -> Register {
        Register::Physical(self.calling_convention.effect_stack_pointer)
    }

    /// 获取effect payload寄存器
    fn effect_payload_register(&self) -> Register {
        Register::Physical(self.calling_convention.effect_payload_register)
    }

    /// 获取effect tag寄存器
    fn effect_tag_register(&self) -> Register {
        Register::Physical(self.calling_convention.effect_tag_register)
    }

    /// 获取effect resume临时寄存器
    fn effect_resume_temp_register(&self) -> Register {
        Register::Physical(self.calling_convention.effect_resume_temp)
    }

    /// 获取栈指针寄存器
    fn stack_pointer_register(&self) -> Register {
        Register::Physical(self.calling_convention.stack_pointer)
    }

    /// 获取帧指针寄存器
    fn frame_pointer_register(&self) -> Register {
        Register::Physical(self.calling_convention.frame_pointer)
    }

    /// 创建新的虚拟寄存器
    fn new_register(&mut self) -> Register {
        self.next_register += 1;
        Register::Virtual(self.next_register)
    }

    /// 降级EffectPushHandler指令
    fn lower_effect_push_handler(
        &mut self,
        tag: &Operand,
        handler_label: crate::LabelId,
        span: &Span,
        new_instructions: &mut Vec<Instruction>,
        function: &mut LirFunction,
    ) -> crate::Result<()> {
        let eff = self.effect_stack_register();
        let sp = self.stack_pointer_register();

        // 分配栈帧
        new_instructions.push(Instruction::Sub {
            dst: sp,
            src1: Operand::Register { id: sp },
            src2: Operand::Immediate { value: 32 },
            span: *span,
        });

        // 保存旧的effect栈指针
        new_instructions.push(Instruction::Store64 {
            addr: sp,
            offset: 0,
            src: Operand::Register { id: eff },
            span: *span,
        });

        // 更新effect栈指针
        new_instructions.push(Instruction::Move {
            dst: eff,
            src: Operand::Register { id: sp },
            span: *span,
        });

        // 存储tag
        match tag {
            Operand::Immediate { .. } => {
                new_instructions.push(Instruction::Store64 {
                    addr: eff,
                    offset: 8,
                    src: tag.clone(),
                    span: *span,
                });
            }
            Operand::Register { .. } => {
                new_instructions.push(Instruction::Store64 {
                    addr: eff,
                    offset: 8,
                    src: tag.clone(),
                    span: *span,
                });
            }
            _ => {
                let tmp = self.new_register();
                new_instructions.push(Instruction::Move {
                    dst: tmp,
                    src: tag.clone(),
                    span: *span,
                });
                new_instructions.push(Instruction::Store64 {
                    addr: eff,
                    offset: 8,
                    src: Operand::Register { id: tmp },
                    span: *span,
                });
            }
        }

        // 存储handler标签
        new_instructions.push(Instruction::Store64 {
            addr: eff,
            offset: 16,
            src: Operand::Label { id: handler_label },
            span: *span,
        });

        // 初始化resume地址为0
        new_instructions.push(Instruction::Store64 {
            addr: eff,
            offset: 24,
            src: Operand::Immediate { value: 0 },
            span: *span,
        });

        Ok(())
    }

    /// 降级EffectPopHandler指令
    fn lower_effect_pop_handler(
        &mut self,
        span: &Span,
        new_instructions: &mut Vec<Instruction>,
    ) -> crate::Result<()> {
        let sp = self.stack_pointer_register();
        let eff = self.effect_stack_register();
        let tmp = self.new_register();

        // 恢复上一层effect栈指针
        new_instructions.push(Instruction::Load64 {
            dst: tmp,
            addr: eff,
            offset: 0,
            span: *span,
        });
        new_instructions.push(Instruction::Move {
            dst: eff,
            src: Operand::Register { id: tmp },
            span: *span,
        });

        // 回收帧
        new_instructions.push(Instruction::Add {
            dst: sp,
            src1: Operand::Register { id: sp },
            src2: Operand::Immediate { value: 32 },
            span: *span,
        });

        Ok(())
    }

    /// 降级EffectPerform指令
    fn lower_effect_perform(
        &mut self,
        tag: &Operand,
        payload: &Operand,
        result: &Option<Register>,
        span: &Span,
        new_instructions: &mut Vec<Instruction>,
        function: &mut LirFunction,
    ) -> crate::Result<()> {
        let sp = self.stack_pointer_register();
        let eff = self.effect_stack_register();

        // 保存 effect栈指针到栈上
        new_instructions.push(Instruction::Sub {
            dst: sp,
            src1: Operand::Register { id: sp },
            src2: Operand::Immediate { value: 16 },
            span: *span,
        });
        new_instructions.push(Instruction::Store64 {
            addr: sp,
            offset: 0,
            src: Operand::Register { id: eff },
            span: *span,
        });

        // 为tag分配独立临时寄存器
        let tag_reg = self.effect_tag_register();
        match tag {
            Operand::Immediate { .. } => {
                // 直接在比较中使用立即数
            }
            Operand::Register { id } => {
                new_instructions.push(Instruction::Move {
                    dst: tag_reg,
                    src: Operand::Register { id: *id },
                    span: *span,
                });
            }
            _ => {
                new_instructions.push(Instruction::Move {
                    dst: tag_reg,
                    src: tag.clone(),
                    span: *span,
                });
            }
        }

        // 创建标签
        let loop_label = function.new_label();
        let found_label = function.new_label();
        let not_found_label = function.new_label();
        let resume_label = function.new_label();
        let cont_label = function.new_label();

        // 搜索匹配的处理器
        new_instructions.push(Instruction::Label {
            id: loop_label,
            span: *span,
        });

        // 检查是否到达栈底
        new_instructions.push(Instruction::Compare {
            src1: Operand::Register { id: eff },
            src2: Operand::Immediate { value: 0 },
            span: *span,
        });
        new_instructions.push(Instruction::JumpEqual {
            target: not_found_label,
            span: *span,
        });

        // 加载并比较tag
        let tmp = self.new_register();
        new_instructions.push(Instruction::Load64 {
            dst: tmp,
            addr: eff,
            offset: 8,
            span: *span,
        });

        match tag {
            Operand::Immediate { value } => {
                new_instructions.push(Instruction::Compare {
                    src1: Operand::Register { id: tmp },
                    src2: Operand::Immediate { value: *value },
                    span: *span,
                });
            }
            _ => {
                new_instructions.push(Instruction::Compare {
                    src1: Operand::Register { id: tmp },
                    src2: Operand::Register { id: tag_reg },
                    span: *span,
                });
            }
        }

        new_instructions.push(Instruction::JumpEqual {
            target: found_label,
            span: *span,
        });

        // 移动到下一个handler
        new_instructions.push(Instruction::Load64 {
            dst: eff,
            addr: eff,
            offset: 0,
            span: *span,
        });
        new_instructions.push(Instruction::Jump {
            target: loop_label,
            span: *span,
        });

        // 找到匹配的handler
        new_instructions.push(Instruction::Label {
            id: found_label,
            span: *span,
        });

        // 设置resume地址
        new_instructions.push(Instruction::Store64 {
            addr: eff,
            offset: 24,
            src: Operand::Label { id: resume_label },
            span: *span,
        });

        // 设置payload
        new_instructions.push(Instruction::Move {
            dst: self.effect_payload_register(),
            src: payload.clone(),
            span: *span,
        });

        // 跳转到handler
        let tmp2 = self.new_register();
        new_instructions.push(Instruction::Load64 {
            dst: tmp2,
            addr: eff,
            offset: 16,
            span: *span,
        });
        new_instructions.push(Instruction::JumpRegister {
            target_register: tmp2,
            span: *span,
        });

        // Resume点
        new_instructions.push(Instruction::Label {
            id: resume_label,
            span: *span,
        });

        if let Some(res_reg) = result {
            new_instructions.push(Instruction::Move {
                dst: *res_reg,
                src: Operand::Register {
                    id: self.effect_payload_register(),
                },
                span: *span,
            });
        }

        new_instructions.push(Instruction::Jump {
            target: cont_label,
            span: *span,
        });

        // 未找到handler
        new_instructions.push(Instruction::Label {
            id: not_found_label,
            span: *span,
        });

        // 错误处理
        new_instructions.push(Instruction::Move {
            dst: eff,
            src: Operand::Immediate { value: 0 },
            span: *span,
        });
        new_instructions.push(Instruction::Move {
            dst: Register::Virtual(0),
            src: Operand::Immediate { value: -777 },
            span: *span,
        });

        // 恢复栈帧并返回
        if function.stack_frame_size > 0 {
            new_instructions.push(Instruction::Add {
                dst: sp,
                src1: Operand::Register { id: sp },
                src2: Operand::Immediate {
                    value: function.stack_frame_size as i64,
                },
                span: Span::dummy(),
            });
        }

        new_instructions.push(Instruction::Move {
            dst: sp,
            src: Operand::Register {
                id: self.frame_pointer_register(),
            },
            span: Span::dummy(),
        });

        new_instructions.push(Instruction::Load64 {
            dst: self.frame_pointer_register(),
            addr: sp,
            offset: 0,
            span: Span::dummy(),
        });

        new_instructions.push(Instruction::Add {
            dst: sp,
            src1: Operand::Register { id: sp },
            src2: Operand::Immediate { value: 16 },
            span: Span::dummy(),
        });

        new_instructions.push(Instruction::Return {
            value: Some(Register::Virtual(0)),
            span: *span,
        });

        // 继续执行
        new_instructions.push(Instruction::Label {
            id: cont_label,
            span: *span,
        });

        // 恢复 effect栈指针
        new_instructions.push(Instruction::Load64 {
            dst: eff,
            addr: sp,
            offset: 0,
            span: *span,
        });
        new_instructions.push(Instruction::Add {
            dst: sp,
            src1: Operand::Register { id: sp },
            src2: Operand::Immediate { value: 16 },
            span: *span,
        });

        Ok(())
    }

    /// 降级EffectResume指令
    fn lower_effect_resume(
        &mut self,
        value: &Operand,
        span: &Span,
        new_instructions: &mut Vec<Instruction>,
    ) -> crate::Result<()> {
        let eff = self.effect_stack_register();
        let tmp = self.effect_resume_temp_register();

        // 设置返回值
        new_instructions.push(Instruction::Move {
            dst: self.effect_payload_register(),
            src: value.clone(),
            span: *span,
        });

        // 加载resume地址
        new_instructions.push(Instruction::Load64 {
            dst: tmp,
            addr: eff,
            offset: 24,
            span: *span,
        });

        // 跳转到resume点
        new_instructions.push(Instruction::JumpRegister {
            target_register: tmp,
            span: *span,
        });

        Ok(())
    }
}

impl Default for EffectLoweringPass {
    fn default() -> Self {
        Self::new()
    }
}

impl FunctionPass for EffectLoweringPass {
    fn name(&self) -> &str {
        "effect-lowering"
    }

    fn description(&self) -> &str {
        "Effect指令降级 - 将高级Effect指令降级为基础指令"
    }

    fn required_analyses(&self) -> Vec<&'static str> {
        // 🔧 2025-12: Effect指令降级必须在CFG分析之前运行
        // 因为CFG不理解effect指令，所以这个pass不能依赖任何CFG相关的分析
        vec![]
    }

    fn invalidated_analyses(&self) -> Vec<&'static str> {
        // Effect指令降级会改变指令序列，使所有分析失效
        // 🔧 2025-12: 修复分析名称不匹配的bug - 使用正确的分析名称
        vec!["lifetime-analysis", "cfg", "def-use", "liveness"]
    }

    fn run_on_function(
        &mut self,
        function: &mut LirFunction,
        _analyses: &mut AnalysisManager,
    ) -> PassResult {
        let mut new_instructions = Vec::new();
        let mut changed = false;

        // 重置状态
        self.last_effect_perform_result = None;
        self.inserted_early_return = false;

        // 克隆指令列表以避免借用检查问题
        let instructions_to_process = function.instructions.clone();

        for instruction in &instructions_to_process {
            match instruction {
                Instruction::EffectPushHandler {
                    tag,
                    handler_label,
                    span,
                } => {
                    if let Err(e) = self.lower_effect_push_handler(
                        tag,
                        *handler_label,
                        span,
                        &mut new_instructions,
                        function,
                    ) {
                        return PassResult::Failed(format!(
                            "Failed to lower EffectPushHandler: {}",
                            e
                        ));
                    }
                    changed = true;
                }

                Instruction::EffectPopHandler { span } => {
                    if let Err(e) = self.lower_effect_pop_handler(span, &mut new_instructions) {
                        return PassResult::Failed(format!(
                            "Failed to lower EffectPopHandler: {}",
                            e
                        ));
                    }
                    changed = true;
                }

                Instruction::EffectPerform {
                    tag,
                    payload,
                    result,
                    span,
                } => {
                    if let Err(e) = self.lower_effect_perform(
                        tag,
                        payload,
                        result,
                        span,
                        &mut new_instructions,
                        function,
                    ) {
                        return PassResult::Failed(format!("Failed to lower EffectPerform: {}", e));
                    }
                    changed = true;
                }

                Instruction::EffectResume { value, span } => {
                    if let Err(e) = self.lower_effect_resume(value, span, &mut new_instructions) {
                        return PassResult::Failed(format!("Failed to lower EffectResume: {}", e));
                    }
                    changed = true;
                }

                _ => {
                    new_instructions.push(instruction.clone());
                }
            }
        }

        if changed {
            function.instructions = new_instructions;
            PassResult::Changed
        } else {
            PassResult::Unchanged
        }
    }
}
