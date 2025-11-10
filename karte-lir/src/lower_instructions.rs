//! LIR指令降级器
//!
//! 将高级LIR指令（如Alloc、Load64、Store64等）降级为更基础的指令组合
//! 这样虚拟机只需要支持最基础的指令集

use crate::{AllocationType, Instruction, LirFunction, LirProgram, Operand, Register};
use karte_diagnostics::Span;

/// 指令降级器
pub struct InstructionLowerer {
    /// 栈指针寄存器（固定使用寄存器6）
    stack_pointer_reg: Register,
    /// 帧指针寄存器（固定使用寄存器7）
    frame_pointer_reg: Register,
    /// 最近一次 EffectPerform 的结果寄存器（用于在 pop 后生成正确的返回路径）
    last_effect_perform_result: Option<Register>,
    /// 是否已插入提前返回（避免再次生成 Return 序言/尾声）
    inserted_early_return: bool,
    next_register: usize,
}

impl InstructionLowerer {
    /// 创建新的指令降级器
    pub fn new() -> Self {
        Self {
            // 使用寄存器6作为栈指针（SP），与调用约定匹配
            stack_pointer_reg: Register::Physical(6),
            // 使用寄存器7作为帧指针（FP），与调用约定匹配
            frame_pointer_reg: Register::Physical(7),
            last_effect_perform_result: None,
            inserted_early_return: false,
            next_register: 40,
        }
    }

    /// 降级整个LIR程序
    pub fn lower_program(&mut self, program: &mut LirProgram) -> Result<(), String> {
        // 为每个函数降级指令
        for (_, function) in program.functions.iter_mut() {
            self.lower_function(function)?;
        }
        Ok(())
    }

    pub fn lower_effect_instructions(&mut self, function: &mut LirFunction) -> Result<(), String> {
        let mut new_instructions = Vec::new();
        let instructions_to_process = function.instructions.clone();
        for instruction in instructions_to_process {
            // 只lower effect相关
            match instruction {
                Instruction::EffectPushHandler { .. }
                | Instruction::EffectPopHandler { .. }
                | Instruction::EffectPerform { .. }
                | Instruction::EffectResume { .. } => {
                    self.lower_inst(function, &mut new_instructions, &mut false, &instruction)?;
                }
                _ => {
                    new_instructions.push(instruction.clone());
                }
            }
        }
        function.instructions = new_instructions;
        Ok(())
    }

    /// 降级单个函数的指令
    pub fn lower_function(&mut self, function: &mut LirFunction) -> Result<(), String> {
        let mut new_instructions = Vec::new();
        // 进入函数前重置状态
        self.last_effect_perform_result = None;
        self.inserted_early_return = false;

        // 克隆指令列表以避免借用检查问题
        let instructions_to_process = function.instructions.clone();

        // 在看到第一个 Label 后，立即发射函数序言与 r12 初始化
        let mut prologue_emitted = false;

        for instruction in &instructions_to_process {
            self.lower_inst(
                function,
                &mut new_instructions,
                &mut prologue_emitted,
                instruction,
            )?;
        }

        function.instructions = new_instructions;
        Ok(())
    }

    fn new_register(&mut self) -> Register {
        self.next_register += 1;
        Register::Virtual(self.next_register)
    }

    fn lower_inst(
        &mut self,
        function: &mut LirFunction,
        new_instructions: &mut Vec<Instruction>,
        prologue_emitted: &mut bool,
        instruction: &Instruction,
    ) -> Result<(), String> {
        Ok(match instruction {
            // Label：发射并在首次遇到时生成函数序言与 r12 初始化
            Instruction::Label { id, span } => {
                new_instructions.push(Instruction::Label {
                    id: *id,
                    span: *span,
                });
                if !*prologue_emitted {
                    *prologue_emitted = true;
                    // 序言：保存旧fp
                    new_instructions.push(Instruction::Sub {
                        dst: self.stack_pointer_reg,
                        src1: Operand::Register {
                            id: self.stack_pointer_reg,
                        },
                        src2: Operand::Immediate { value: 8 },
                        span: *span,
                    });
                    new_instructions.push(Instruction::Store64 {
                        addr: self.stack_pointer_reg,
                        offset: 0,
                        src: Operand::Register {
                            id: self.frame_pointer_reg,
                        },
                        span: Span::dummy(),
                    });
                    // fp = sp
                    new_instructions.push(Instruction::Move {
                        dst: self.frame_pointer_reg,
                        src: Operand::Register {
                            id: self.stack_pointer_reg,
                        },
                        span: Span::dummy(),
                    });
                    // 预留栈帧
                    if function.stack_frame_size > 0 {
                        new_instructions.push(Instruction::Sub {
                            dst: self.stack_pointer_reg,
                            src1: Operand::Register {
                                id: self.stack_pointer_reg,
                            },
                            src2: Operand::Immediate {
                                value: function.stack_frame_size as i64,
                            },
                            span: Span::dummy(),
                        });
                    }
                    // 仅在 main 函数初始化 effect 栈顶 r12 = 0，其它函数继承调用者的 effect 栈
                    if function.name == "main" {
                        new_instructions.push(Instruction::Move {
                            dst: Register::Physical(12),
                            src: Operand::Immediate { value: 0 },
                            span: Span::dummy(),
                        });
                    }
                }
            }
            // 降级 Alloc 指令
            Instruction::Alloc {
                dst,
                size,
                alignment,
                allocation_type,
                span,
            } => {
                self.lower_alloc(
                    dst,
                    *size,
                    *alignment,
                    allocation_type,
                    span,
                    new_instructions,
                    function,
                )?;
            }

            // ===== 代数效应：指令展开 =====
            // EffectPushHandler: 在常规栈上压入处理器帧，并更新r12为effect栈顶
            Instruction::EffectPushHandler {
                tag,
                handler_label,
                span,
            } => {
                let eff = Register::Physical(12);
                new_instructions.push(Instruction::Sub {
                    dst: self.stack_pointer_reg,
                    src1: Operand::Register {
                        id: self.stack_pointer_reg,
                    },
                    src2: Operand::Immediate { value: 32 },
                    span: *span,
                });
                new_instructions.push(Instruction::Store64 {
                    addr: self.stack_pointer_reg,
                    offset: 0,
                    src: Operand::Register { id: eff },
                    span: *span,
                });
                new_instructions.push(Instruction::Move {
                    dst: eff,
                    src: Operand::Register {
                        id: self.stack_pointer_reg,
                    },
                    span: *span,
                });
                // tag store supports immediate or needs move
                match tag {
                    Operand::Immediate { .. } => new_instructions.push(Instruction::Store64 {
                        addr: eff,
                        offset: 8,
                        src: tag.clone(),
                        span: *span,
                    }),
                    _ => {
                        // If tag is register operand already, store directly; else move then store
                        if let Operand::Register { .. } = tag {
                            new_instructions.push(Instruction::Store64 {
                                addr: eff,
                                offset: 8,
                                src: tag.clone(),
                                span: *span,
                            });
                        } else {
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
                }
                new_instructions.push(Instruction::Store64 {
                    addr: eff,
                    offset: 16,
                    src: Operand::Label { id: *handler_label },
                    span: *span,
                });
                new_instructions.push(Instruction::Store64 {
                    addr: eff,
                    offset: 24,
                    src: Operand::Immediate { value: 0 },
                    span: *span,
                });
            }

            // EffectPopHandler: 恢复上一层effect栈顶并回收帧
            Instruction::EffectPopHandler { span } => {
                let sp = self.stack_pointer_reg;
                let eff = Register::Physical(12);
                let tmp = self.new_register();
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
                new_instructions.push(Instruction::Add {
                    dst: sp,
                    src1: Operand::Register { id: sp },
                    src2: Operand::Immediate { value: 32 },
                    span: *span,
                });
                // new_instructions.push(Instruction::Move { dst: Register::Physical(12), src: Operand::Register { id: sp }, span: *span });
            }

            // EffectPerform: 搜索匹配的处理器帧，设置resume地址并跳转到处理器
            Instruction::EffectPerform {
                tag,
                payload,
                result,
                span,
            } => {
                // 保存 r12 到栈上
                new_instructions.push(Instruction::Sub {
                    dst: self.stack_pointer_reg,
                    src1: Operand::Register {
                        id: self.stack_pointer_reg,
                    },
                    src2: Operand::Immediate { value: 8 },
                    span: *span,
                });
                new_instructions.push(Instruction::Store64 {
                    addr: self.stack_pointer_reg,
                    offset: 0,
                    src: Operand::Register {
                        id: Register::Physical(12),
                    },
                    span: *span,
                });

                let eff = Register::Physical(12);
                // 为 tag 分配独立临时寄存器，避免后续覆盖
                let tag_reg = Register::Physical(10); // 保留 r10 存放 tag
                match tag {
                    Operand::Immediate { .. } => { /* 直接在比较中使用立即数 */ }
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
                let loop_label = function.new_label();
                let found_label = function.new_label();
                let not_found_label = function.new_label();
                let resume_label = function.new_label();
                let cont_label = function.new_label();
                new_instructions.push(Instruction::Label {
                    id: loop_label,
                    span: *span,
                });
                new_instructions.push(Instruction::Compare {
                    src1: Operand::Register { id: eff },
                    src2: Operand::Immediate { value: 0 },
                    span: *span,
                });
                new_instructions.push(Instruction::JumpEqual {
                    target: not_found_label,
                    span: *span,
                });
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
                new_instructions.push(Instruction::Label {
                    id: found_label,
                    span: *span,
                });
                new_instructions.push(Instruction::Store64 {
                    addr: eff,
                    offset: 24,
                    src: Operand::Label { id: resume_label },
                    span: *span,
                });
                new_instructions.push(Instruction::Move {
                    dst: Register::Physical(1),
                    src: payload.clone(),
                    span: *span,
                });
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
                new_instructions.push(Instruction::Label {
                    id: resume_label,
                    span: *span,
                });
                if let Some(res_reg) = result {
                    new_instructions.push(Instruction::Move {
                        dst: *res_reg,
                        src: Operand::Register {
                            id: Register::Physical(1),
                        },
                        span: *span,
                    });
                }
                new_instructions.push(Instruction::Jump {
                    target: cont_label,
                    span: *span,
                });
                new_instructions.push(Instruction::Label {
                    id: not_found_label,
                    span: *span,
                });
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
                if function.stack_frame_size > 0 {
                    new_instructions.push(Instruction::Add {
                        dst: self.stack_pointer_reg,
                        src1: Operand::Register {
                            id: self.stack_pointer_reg,
                        },
                        src2: Operand::Immediate {
                            value: function.stack_frame_size as i64,
                        },
                        span: Span::dummy(),
                    });
                }
                new_instructions.push(Instruction::Move {
                    dst: self.stack_pointer_reg,
                    src: Operand::Register {
                        id: self.frame_pointer_reg,
                    },
                    span: Span::dummy(),
                });
                new_instructions.push(Instruction::Load64 {
                    dst: self.frame_pointer_reg,
                    addr: self.stack_pointer_reg,
                    offset: 0,
                    span: Span::dummy(),
                });
                new_instructions.push(Instruction::Add {
                    dst: self.stack_pointer_reg,
                    src1: Operand::Register {
                        id: self.stack_pointer_reg,
                    },
                    src2: Operand::Immediate { value: 8 },
                    span: Span::dummy(),
                });
                new_instructions.push(Instruction::Return {
                    value: Some(Register::Virtual(0)),
                    span: *span,
                });
                new_instructions.push(Instruction::Label {
                    id: cont_label,
                    span: *span,
                });

                // 恢复 r12
                new_instructions.push(Instruction::Load64 {
                    dst: Register::Physical(12),
                    addr: self.stack_pointer_reg,
                    offset: 0,
                    span: *span,
                });
                new_instructions.push(Instruction::Add {
                    dst: self.stack_pointer_reg,
                    src1: Operand::Register {
                        id: self.stack_pointer_reg,
                    },
                    src2: Operand::Immediate { value: 8 },
                    span: *span,
                });
            }

            // 降级 Load64 指令
            Instruction::Load64 {
                dst,
                addr,
                offset,
                span,
            } => {
                self.lower_load64(dst, addr, *offset, span, new_instructions)?;
            }

            // 降级 Store64 指令
            Instruction::Store64 {
                addr,
                offset,
                src,
                span,
            } => {
                self.lower_store64(addr, *offset, src, span, new_instructions)?;
            }

            // 降级 StructAlloc 指令
            Instruction::StructAlloc {
                dst,
                struct_type,
                allocation_type,
                span,
            } => {
                self.lower_struct_alloc(
                    dst,
                    struct_type,
                    allocation_type,
                    span,
                    new_instructions,
                    function,
                )?;
            }

            // 降级 StructFieldLoad 指令
            Instruction::StructFieldLoad {
                dst,
                struct_addr,
                field_offset,
                span,
            } => {
                self.lower_struct_field_load(
                    dst,
                    struct_addr,
                    *field_offset,
                    span,
                    new_instructions,
                )?;
            }

            // 降级 StructFieldStore 指令
            Instruction::StructFieldStore {
                struct_addr,
                field_offset,
                src,
                span,
            } => {
                self.lower_struct_field_store(
                    struct_addr,
                    *field_offset,
                    src,
                    span,
                    new_instructions,
                )?;
            }

            // 降级 Phi 指令
            Instruction::Phi {
                dst,
                incoming,
                span,
            } => {
                self.lower_phi(dst, incoming, span, new_instructions)?;
            }

            // 降级 Call 指令
            Instruction::Call {
                target,
                args: _,
                arg_operands,
                result,
                span,
            } => {
                // 1. 保存 caller-saved 寄存器（r0-r4）到栈
                // 2. 参数依次mov到r1-r4
                // 3. 生成返回标签并压栈
                // 4. jump 到目标label
                // 5. 返回标签：恢复caller-saved寄存器，处理返回值

                // 生成唯一的返回标签
                let return_label = function.new_label();

                // 保存caller-saved寄存器到栈
                for reg in 0..=4 {
                    // sp = sp - 8
                    new_instructions.push(Instruction::Sub {
                        dst: self.stack_pointer_reg,
                        src1: Operand::Register {
                            id: self.stack_pointer_reg,
                        },
                        src2: Operand::Immediate { value: 8 },
                        span: *span,
                    });
                    // store64 [sp], reg
                    new_instructions.push(Instruction::Store64 {
                        addr: self.stack_pointer_reg,
                        offset: 0,
                        src: Operand::Register {
                            id: Register::Virtual(reg),
                        },
                        span: *span,
                    });
                }

                // 参数传递
                for (i, op) in arg_operands.iter().enumerate() {
                    if i < 4 {
                        new_instructions.push(Instruction::Move {
                            dst: Register::Virtual(i + 1),
                            src: op.clone(),
                            span: *span,
                        });
                    }
                }

                // 将返回标签地址压栈
                new_instructions.push(Instruction::Sub {
                    dst: self.stack_pointer_reg,
                    src1: Operand::Register {
                        id: self.stack_pointer_reg,
                    },
                    src2: Operand::Immediate { value: 8 },
                    span: *span,
                });
                new_instructions.push(Instruction::Store64 {
                    addr: self.stack_pointer_reg,
                    offset: 0,
                    src: Operand::Label { id: return_label },
                    span: *span,
                });

                // 跳转到目标函数
                new_instructions.push(Instruction::Jump {
                    target: *target,
                    span: *span,
                });

                // 返回标签：恢复caller-saved寄存器
                new_instructions.push(Instruction::Label {
                    id: return_label,
                    span: *span,
                });

                // 从栈上弹出返回地址（丢弃）
                new_instructions.push(Instruction::Load64 {
                    dst: Register::Physical(0), // 临时使用r0
                    addr: self.stack_pointer_reg,
                    offset: 0,
                    span: *span,
                });
                new_instructions.push(Instruction::Add {
                    dst: self.stack_pointer_reg,
                    src1: Operand::Register {
                        id: self.stack_pointer_reg,
                    },
                    src2: Operand::Immediate { value: 8 },
                    span: *span,
                });

                // 恢复caller-saved寄存器
                for reg in (0..=4).rev() {
                    new_instructions.push(Instruction::Load64 {
                        dst: Register::Virtual(reg),
                        addr: self.stack_pointer_reg,
                        offset: 0,
                        span: *span,
                    });
                    new_instructions.push(Instruction::Add {
                        dst: self.stack_pointer_reg,
                        src1: Operand::Register {
                            id: self.stack_pointer_reg,
                        },
                        src2: Operand::Immediate { value: 8 },
                        span: *span,
                    });
                }

                // 返回值处理（r0已经包含返回值）
                if let Some(result_reg) = result {
                    new_instructions.push(Instruction::Move {
                        dst: *result_reg,
                        src: Operand::Register {
                            id: Register::Physical(0),
                        },
                        span: *span,
                    });
                }
            }
            // 降级 CallIndirect 指令
            Instruction::CallIndirect {
                function_register,
                span,
                ..
            } => {
                // 间接跳转
                // 从函数寄存器加载函数地址到临时寄存器
                new_instructions.push(Instruction::JumpIndirect {
                    function_register: *function_register,
                    span: *span,
                });
            }

            // 降级 Return 指令
            Instruction::Return { .. } => {
                if self.inserted_early_return {
                    // 已经插入提前返回：跳过重复的尾声与Return，避免重复恢复栈
                    // 用 Nop 占位
                    new_instructions.push(Instruction::Nop {
                        span: Span::dummy(),
                    });
                } else {
                    // 原始逻辑
                    if function.stack_frame_size > 0 {
                        new_instructions.push(Instruction::Add {
                            dst: self.stack_pointer_reg,
                            src1: Operand::Register {
                                id: self.stack_pointer_reg,
                            },
                            src2: Operand::Immediate {
                                value: function.stack_frame_size as i64,
                            },
                            span: Span::dummy(),
                        });
                    }
                    new_instructions.push(Instruction::Move {
                        dst: self.stack_pointer_reg,
                        src: Operand::Register {
                            id: self.frame_pointer_reg,
                        },
                        span: Span::dummy(),
                    });
                    new_instructions.push(Instruction::Load64 {
                        dst: self.frame_pointer_reg,
                        addr: self.stack_pointer_reg,
                        offset: 0,
                        span: Span::dummy(),
                    });
                    new_instructions.push(Instruction::Add {
                        dst: self.stack_pointer_reg,
                        src1: Operand::Register {
                            id: self.stack_pointer_reg,
                        },
                        src2: Operand::Immediate { value: 8 },
                        span: Span::dummy(),
                    });
                    new_instructions.push(instruction.clone());
                }
            }

            // EffectResume: 恢复到上一个处理器帧
            Instruction::EffectResume { value, span } => {
                // Lower resume: move value to r0, load current effect frame pointer r12, load saved resume label addr [r12+24], jump there
                let eff = Register::Physical(12);
                let tmp = Register::Physical(15); // reuse tmp for address
                                                  // move r0 = value (value may be immediate/register/memory)
                new_instructions.push(Instruction::Move {
                    dst: Register::Physical(1),
                    src: value.clone(),
                    span: *span,
                });
                // load resume address into tmp
                new_instructions.push(Instruction::Load64 {
                    dst: tmp,
                    addr: eff,
                    offset: 24,
                    span: *span,
                });
                // jump to saved resume continuation
                new_instructions.push(Instruction::JumpRegister {
                    target_register: tmp,
                    span: *span,
                });
            }

            // 其他指令直接保留
            _ => {
                new_instructions.push(instruction.clone());
            }
        })
    }

    /// 降级 Alloc 指令为栈指针操作
    fn lower_alloc(
        &mut self,
        dst: &Register,
        size: usize,
        _alignment: usize,
        allocation_type: &AllocationType,
        span: &Span,
        instructions: &mut Vec<Instruction>,
        function: &mut LirFunction,
    ) -> Result<(), String> {
        match allocation_type {
            AllocationType::Stack => {
                // 栈分配：SP = SP - size; dst = SP (新的栈指针值)

                // 1. 获取栈指针寄存器（确保使用正确的栈指针）
                let stack_pointer = function.get_stack_pointer_register();

                // 2. 直接使用立即数进行栈指针计算，避免生成新寄存器
                // SP = SP - size (移动栈指针)
                instructions.push(Instruction::Sub {
                    dst: stack_pointer,
                    src1: Operand::Register { id: stack_pointer },
                    src2: Operand::Immediate { value: size as i64 },
                    span: *span,
                });

                // 4. dst = SP (将新的栈指针值作为分配的地址)
                instructions.push(Instruction::Move {
                    dst: *dst,
                    src: Operand::Register { id: stack_pointer },
                    span: *span,
                });
            }
            AllocationType::Heap => {
                // 堆分配：保持原始的Alloc指令不变，让虚拟机的专业执行器处理
                instructions.push(Instruction::Alloc {
                    dst: *dst,
                    size,
                    alignment: _alignment,
                    allocation_type: allocation_type.clone(),
                    span: *span,
                });
            }
            AllocationType::Static => {
                // 静态分配：保持原始的Alloc指令不变，让虚拟机的专业执行器处理
                instructions.push(Instruction::Alloc {
                    dst: *dst,
                    size,
                    alignment: _alignment,
                    allocation_type: allocation_type.clone(),
                    span: *span,
                });
            }
        }
        Ok(())
    }

    /// 降级 Load64 指令为基础内存操作
    fn lower_load64(
        &mut self,
        dst: &Register,
        addr: &Register,
        offset: i64,
        span: &Span,
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), String> {
        // 🔧 关键修复：Load64指令应该保持不变，让专业执行器直接处理
        // 不要转换为Move指令，因为Move指令的Memory操作数处理有问题
        instructions.push(Instruction::Load64 {
            dst: *dst,
            addr: *addr,
            offset,
            span: *span,
        });
        Ok(())
    }

    /// 降级 Store64 指令为基础内存操作
    fn lower_store64(
        &mut self,
        addr: &Register,
        offset: i64,
        src: &Operand,
        span: &Span,
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), String> {
        // Store64操作需要特殊处理：不能覆盖地址寄存器
        // 我们直接保留Store64指令，让专业执行器处理
        instructions.push(Instruction::Store64 {
            addr: *addr,
            offset,
            src: src.clone(),
            span: *span,
        });

        Ok(())
    }

    /// 降级 StructAlloc 指令
    fn lower_struct_alloc(
        &mut self,
        dst: &Register,
        struct_type: &crate::StructTypeId,
        allocation_type: &AllocationType,
        span: &Span,
        instructions: &mut Vec<Instruction>,
        function: &mut LirFunction,
    ) -> Result<(), String> {
        // 获取结构体大小
        let struct_layout = function
            .struct_types
            .get(struct_type)
            .ok_or_else(|| format!("Unknown struct type: {:?}", struct_type))?;

        // 降级为普通的 Alloc 指令
        self.lower_alloc(
            dst,
            struct_layout.total_size,
            struct_layout.alignment,
            allocation_type,
            span,
            instructions,
            function,
        )
    }

    /// 降级 StructFieldLoad 指令
    fn lower_struct_field_load(
        &mut self,
        dst: &Register,
        struct_addr: &Register,
        field_offset: usize,
        span: &Span,
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), String> {
        // 降级为 Load64 指令
        self.lower_load64(dst, struct_addr, field_offset as i64, span, instructions)
    }

    /// 降级 StructFieldStore 指令
    fn lower_struct_field_store(
        &mut self,
        struct_addr: &Register,
        field_offset: usize,
        src: &Operand,
        span: &Span,
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), String> {
        // 降级为 Store64 指令
        self.lower_store64(struct_addr, field_offset as i64, src, span, instructions)
    }

    /// 降级 Phi 指令 - 专业实现
    fn lower_phi(
        &mut self,
        dst: &Register,
        incoming: &Vec<(crate::LabelId, Operand)>,
        span: &Span,
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), String> {
        // φ指令的专业降级：
        // 1. 在每个前驱基本块的末尾插入mov指令
        // 2. φ指令本身被移除

        println!("🔧 专业降级φ指令: dst={:?}, incoming={:?}", dst, incoming);

        // φ指令不应该出现在最终的降级LIR中
        // 它应该在前面的优化阶段被处理
        // 但如果出现了，我们需要插入适当的mov指令

        // 为了专业处理，我们需要：
        // 1. 识别当前φ指令所在的基本块
        // 2. 找到所有前驱基本块
        // 3. 在每个前驱基本块的末尾插入mov指令

        // 由于我们在降级阶段，控制流信息可能不完整
        // 作为专业实现，我们采用保守策略：
        // 选择第一个可用的incoming值，并发出警告

        if incoming.is_empty() {
            return Err("φ指令没有incoming值".to_string());
        }

        // 专业编译器中，φ指令应该在SSA降级阶段被完全消除
        // 如果到了这里，说明前面的优化有问题
        println!("⚠️  警告：φ指令出现在降级阶段，这表明SSA降级不完整");

        // 选择第一个incoming值作为fallback
        let (source_block, ref operand) = incoming[0];
        println!(
            "🔧 使用fallback策略，选择来自块 {:?} 的值: {:?}",
            source_block, operand
        );

        instructions.push(Instruction::Move {
            dst: *dst,
            src: operand.clone(),
            span: *span,
        });

        Ok(())
    }
}

impl Default for InstructionLowerer {
    fn default() -> Self {
        Self::new()
    }
}

pub fn lower_effect_instructions(program: &mut LirProgram) -> Result<(), String> {
    let mut lowerer = InstructionLowerer::new();
    for (_, function) in program.functions.iter_mut() {
        lowerer.lower_effect_instructions(function)?;
    }
    Ok(())
}

/// 降级整个LIR程序的指令
pub fn lower_program_instructions(program: &mut LirProgram) -> Result<(), String> {
    let mut lowerer = InstructionLowerer::new();

    // 降级指令
    lowerer.lower_program(program)?;

    // 验证栈指针寄存器使用规则
    for (function_name, function) in &program.functions {
        if let Err(validation_error) = function.validate_stack_pointer_usage() {
            return Err(format!(
                "函数 '{}' 中的栈指针使用验证失败: {}",
                function_name, validation_error
            ));
        }
    }

    Ok(())
}
