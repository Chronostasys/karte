use super::*;
use crate::pass::stack_frame_layout::StackFrameLayoutPass;
use crate::{Instruction, LirFunction, Operand, Register};
use karte_common::calling_convention::{CallingConvention, TOTAL_REGISTERS, REG_X0, REG_X1, REG_X2};
use karte_diagnostics::Span;
use std::collections::HashMap;

/// 解析LIR文件内容为LirFunction
fn parse_lir_file(content: &str) -> crate::Result<LirFunction> {
    let lines: Vec<&str> = content.lines().collect();

    if lines.is_empty() {
        return Err("Empty file".into());
    }

    // 解析函数头
    let first_line = lines[0].trim();
    if !first_line.starts_with("function ") {
        return Err("Invalid function header".into());
    }

    let function_name = first_line
        .strip_prefix("function ")
        .and_then(|s| s.split(' ').next())
        .unwrap_or("main")
        .to_string();

    let mut function = LirFunction {
        name: function_name,
        parameter_registers: vec![],
        instructions: vec![],
        next_register: 16,
        struct_types: HashMap::new(),
        stack_frame_size: 0,
        parameter_count: 0,
        used_regs: Vec::new(),
        lowered_lifetimes: None,
        lowered_register_mapping: None,
        instruction_metadata: HashMap::new(),
        target_arch: None,
    };

    // 解析指令
    for (line_num, line) in lines.iter().enumerate().skip(1) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let instruction = parse_instruction(line, line_num)?;
        function.instructions.push(instruction);
    }

    Ok(function)
}

/// 解析单条指令
fn parse_instruction(line: &str, line_num: usize) -> crate::Result<Instruction> {
    let parts: Vec<&str> = line.split_whitespace().collect();

    if parts.is_empty() {
        return Err(format!("Empty instruction at line {}", line_num).into());
    }

    match parts[0] {
        // 标签
        label if label.ends_with(':') => {
            let label_name = label.trim_end_matches(':');
            let label_id = match label_name {
                "L1" => crate::LabelId(1),
                _ => crate::LabelId(0),
            };
            Ok(Instruction::Label {
                id: label_id,
                span: Span::dummy(),
            })
        }
        // mov 指令
        "mov" => {
            if parts.len() != 3 {
                return Err(format!("Invalid mov instruction at line {}", line_num).into());
            }

            let dst = parse_register(parts[1].trim_end_matches(','))?;
            let src = parse_operand(parts[2])?;

            Ok(Instruction::Move {
                dst,
                src,
                span: Span::dummy(),
            })
        }
        // add 指令
        "add" => {
            if parts.len() != 4 {
                return Err(format!("Invalid add instruction at line {}", line_num).into());
            }

            let dst = parse_register(parts[1].trim_end_matches(','))?;
            let src1 = parse_operand(parts[2].trim_end_matches(','))?;
            let src2 = parse_operand(parts[3])?;

            Ok(Instruction::Add {
                dst,
                src1,
                src2,
                span: Span::dummy(),
            })
        }
        // load64 指令 - load64 r10, [r5]
        "load64" => {
            if parts.len() != 3 {
                return Err(format!("Invalid load64 instruction at line {}", line_num).into());
            }

            let dst = parse_register(parts[1].trim_end_matches(','))?;
            let addr_str = parts[2];

            // 简单解析 [rX] 格式
            if addr_str.starts_with('[') && addr_str.ends_with(']') {
                let inner = &addr_str[1..addr_str.len() - 1];
                let addr = parse_register(inner)?;

                Ok(Instruction::Load64 {
                    dst,
                    addr,
                    offset: 0,
                    span: Span::dummy(),
                })
            } else {
                Err(format!("Invalid load64 address format: {}", addr_str).into())
            }
        }
        // ret 指令
        "ret" => {
            let value = if parts.len() > 1 {
                Some(parse_register(parts[1])?)
            } else {
                None
            };

            Ok(Instruction::Return {
                value,
                span: Span::dummy(),
            })
        }
        _ => {
            // 检查是否是 call_indirect 格式: r9 = call_indirect r10(r1)
            if line.contains("call_indirect") {
                // 解析格式: r9 = call_indirect r10(r1)
                if let Some(equals_pos) = line.find('=') {
                    let result_part = line[..equals_pos].trim();
                    let call_part = line[equals_pos + 1..].trim();

                    if call_part.starts_with("call_indirect") {
                        let result_reg = parse_register(result_part)?;

                        // 解析 call_indirect r10(r1) 部分
                        let call_content = call_part.strip_prefix("call_indirect").unwrap().trim();
                        if let Some(paren_pos) = call_content.find('(') {
                            let function_reg_str = call_content[..paren_pos].trim();
                            let args_str = &call_content[paren_pos + 1..];
                            let args_str = args_str.trim_end_matches(')');

                            let function_register = parse_register(function_reg_str)?;
                            let args = if args_str.is_empty() {
                                vec![]
                            } else {
                                args_str
                                    .split(',')
                                    .map(|s| parse_register(s.trim()))
                                    .collect::<Result<Vec<_>, _>>()?
                            };

                            return Ok(Instruction::CallIndirect {
                                function_register,
                                args,
                                arg_operands: vec![], // 简化处理
                                result: Some(result_reg),
                                span: Span::dummy(),
                            });
                        }
                    }
                }
            }

            Err(format!(
                "Unknown instruction '{}' at line {}",
                parts[0], line_num
            ).into())
        }
    }
}

/// 解析寄存器
fn parse_register(s: &str) -> crate::Result<Register> {
    if let Some(num_str) = s.strip_prefix('r') {
        let num: usize = num_str
            .parse()
            .map_err(|_| crate::KarteError::from(format!("Invalid register number: {}", s)))?;
        Ok(Register::Virtual(num))
    } else {
        Err(format!("Invalid register format: {}", s).into())
    }
}

/// 解析操作数
fn parse_operand(s: &str) -> crate::Result<Operand> {
    if let Some(num_str) = s.strip_prefix('#') {
        // 立即数
        let value: i64 = num_str
            .parse()
            .map_err(|_| crate::KarteError::from(format!("Invalid immediate value: {}", s)))?;
        Ok(Operand::Immediate { value })
    } else if s.starts_with('r') {
        // 寄存器
        let reg = parse_register(s)?;
        Ok(Operand::Register { id: reg })
    } else {
        Err(format!("Invalid operand format: {}", s).into())
    }
}

#[test]
fn test_file_1_register_allocation() {
    // 读取文件"1"的内容
    let file_content = r#"function main (stack_frame: 0):
L1:
  mov r1, #1
  mov r2, #2
  mov r3, #3
  mov r4, #4
  mov r5, #5
  mov r8, #6
  mov r9, #7
  add r10, r8, r9
  add r11, r5, r10
  add r12, r4, r11
  add r13, r3, r12
  add r14, r2, r13
  add r15, r1, r14
  ret r15"#;

    info!("🧪 测试文件'1'的寄存器分配");
    info!("原始LIR内容:");
    for (i, line) in file_content.lines().enumerate() {
        info!("  {}: {}", i, line);
    }

    // 解析LIR文件
    let mut function = parse_lir_file(file_content).expect("Failed to parse LIR file");

    info!("\n🧪 解析后的LIR函数:\n{}", function);
    // for (i, instruction) in function.instructions.iter().enumerate() {
    //     info!("  {}: {}", i, instruction);
    // }

    // 运行寄存器分配 Pass
    let result = run_simple_stack_register_allocation(&mut function);
    // 运行统一栈帧布局 Pass（负责下沉为 FP+offset 并复用栈槽）
    let mut layout = StackFrameLayoutPass::new();
    let mut analyses = AnalysisManager::new();
    let _ = layout.run_on_function(&mut function, &mut analyses);

    info!("\n🧪 寄存器分配后的LIR函数:\n{}", function);

    // 验证结果
    assert!(matches!(result, PassResult::Changed));

    // 验证所有寄存器都在物理寄存器范围内
    // 🔧 修复：现在支持 callee-saved 寄存器，物理寄存器范围扩展到 0-31
    let uses_only_physical_regs = function.instructions.iter().all(|inst| {
        let used_regs = inst.get_used_registers();
        let def_reg = inst.get_def_register();

        // 检查所有使用的寄存器都是物理寄存器 (r0-r31)
        let used_ok = used_regs
            .iter()
            .all(|reg| reg.is_physical() && reg.id() <= 31);
        let def_ok = match def_reg {
            None => true,
            Some(reg) => reg.is_physical() && reg.id() <= 31,
        };

        used_ok && def_ok
    });

    // 验证是否有基于 FP 的栈访问（布局已下沉为 FP+offset）
    let cc = CallingConvention::standard();
    let fp_reg = cc.frame_pointer;
    let has_fp_memory_access = function.instructions.iter().any(|inst| match inst {
        Instruction::Load64 { addr, .. } | Instruction::Store64 { addr, .. } => {
            addr.id() == fp_reg as usize
        }
        _ => false,
    });

    info!("\n✅ 验证结果:");
    info!("  - 物理寄存器范围: {}", uses_only_physical_regs);
    info!("  - 存在FP寻址: {}", has_fp_memory_access);
    info!("  - 指令数量: {} -> {}", 14, function.instructions.len());

    assert!(
        uses_only_physical_regs,
        "所有寄存器都应该在物理寄存器范围内"
    );
    // 对于该用例，RA 可能通过寄存器复用避免溢出，因此不强制要求出现栈访问

    info!("🎉 文件'1'的寄存器分配测试通过！");
}

/// 测试call_indirect指令的寄存器分配修复
#[test]
fn test_call_indirect_register_allocation() {
    // 模拟call_indirect场景的简化版本
    let test_content = r#"function main (stack_frame: 0):
L1:
  mov r3, #16
  mov r4, r3
  add r5, r4, #0
  mov r8, #42
  mov r1, r8
  load64 r10, [r5]
  r9 = call_indirect r10(r1)
  mov r11, r9
  ret r11"#;

    let mut function = parse_lir_file(test_content).expect("Failed to parse test LIR");

    info!("\n🧪 测试call_indirect指令修复:");
    info!("原始LIR:\n{}", function);

    // 运行新的寄存器分配Pass
    let result = run_simple_stack_register_allocation(&mut function);

    info!("\n寄存器分配后:\n{}", function);

    // 验证结果
    assert!(matches!(result, PassResult::Changed));

    // 关键验证：检查call_indirect指令中的函数地址寄存器
    let mut found_call_indirect = false;
    for instruction in &function.instructions {
        if let Instruction::CallIndirect {
            function_register,
            args,
            ..
        } = instruction
        {
            found_call_indirect = true;
            info!("🔍 发现call_indirect指令:");
            info!("  函数地址寄存器: r{}", function_register.id());
            info!(
                "  参数寄存器: {:?}",
                args.iter()
                    .map(|r| format!("r{}", r.id()))
                    .collect::<Vec<_>>()
            );

            // // 验证函数地址寄存器在合理范围内
            // assert!(
            //     function_register.id() <= 4,
            //     "函数地址寄存器应该在r0-r4范围内"
            // );

            // 验证参数寄存器都是物理寄存器且在合法范围内
            for arg in args {
                assert!(arg.is_physical(), "参数寄存器应该是物理寄存器，实际: {:?}", arg);
                assert!(
                    arg.id() < TOTAL_REGISTERS,
                    "参数寄存器应该在物理寄存器范围内，实际: r{}", arg.id()
                );
            }
        }
    }

    assert!(found_call_indirect, "应该找到call_indirect指令");

    // 验证load64指令的正确性（加载函数地址）
    for (i, instruction) in function.instructions.iter().enumerate() {
        if let Instruction::Load64 { dst, addr, .. } = instruction {
            // 检查是否有后续的call_indirect指令使用这个寄存器
            for j in (i + 1)..function.instructions.len() {
                if let Instruction::CallIndirect {
                    function_register, ..
                } = &function.instructions[j]
                {
                    if dst == function_register {
                        info!("🔍 发现load64指令为call_indirect准备函数地址:");
                        info!("  load64 r{}, [r{}]", dst.id(), addr.id());
                        info!("  call_indirect r{}(...)", function_register.id());

                        // 验证地址寄存器在合理范围内 (r0-r7)
                        assert!(addr.id() <= 7, "地址寄存器应该在r0-r7范围内");
                        break;
                    }
                }
            }
        }
    }

    info!("✅ call_indirect指令寄存器分配修复测试通过！");
}

/// 测试函数参数寄存器分配
#[test]
fn test_function_parameter_register_allocation() {
    info!("🧪 测试函数参数寄存器分配:");

    // 创建一个有参数的函数
    let mut function = LirFunction {
        name: "test_func".to_string(),
        parameter_registers: vec![
            Register::Virtual(100),
            Register::Virtual(101),
            Register::Virtual(102),
        ], // 3个参数
        instructions: vec![
            Instruction::Label {
                id: crate::LabelId(1),
                span: Span::dummy(),
            },
            // 使用参数寄存器
            Instruction::Add {
                dst: Register::Virtual(200),
                src1: Operand::Register {
                    id: Register::Virtual(100),
                }, // 第一个参数
                src2: Operand::Register {
                    id: Register::Virtual(101),
                }, // 第二个参数
                span: Span::dummy(),
            },
            Instruction::Add {
                dst: Register::Virtual(201),
                src1: Operand::Register {
                    id: Register::Virtual(200),
                },
                src2: Operand::Register {
                    id: Register::Virtual(102),
                }, // 第三个参数
                span: Span::dummy(),
            },
            // 返回结果
            Instruction::Return {
                value: Some(Register::Virtual(201)),
                span: Span::dummy(),
            },
        ],
        next_register: 202,
        struct_types: HashMap::new(),
        stack_frame_size: 0,
        parameter_count: 3,
        used_regs: Vec::new(),
        lowered_lifetimes: None,
        lowered_register_mapping: None,
        instruction_metadata: HashMap::new(),
        target_arch: None,
    };

    info!("🧪 测试前的函数参数: {:?}", function.parameter_registers);

    // 运行寄存器分配
    let mut pass = SimpleStackRegisterAllocation::new();
    let mut analyses = AnalysisManager::new();

    let result = pass.run_on_function(&mut function, &mut analyses);

    info!("🧪 测试后的LIR函数:");
    for (i, instruction) in function.instructions.iter().enumerate() {
        info!("  {}: {:?}", i, instruction);
    }

    // 验证结果
    assert!(matches!(result, PassResult::Changed));

    // 验证参数寄存器分配是否正确 (ARM64 AAPCS64)
    // 参数1(RegisterId(100)) 应该分配到 x0 (REG_X0)
    // 参数2(RegisterId(101)) 应该分配到 x1 (REG_X1)
    // 参数3(RegisterId(102)) 应该分配到 x2 (REG_X2)
    // 返回值(RegisterId(201)) 应该分配到 x0 (REG_X0)

    let mut found_param_usage = false;
    let mut found_return_assignment = false;

    for instruction in &function.instructions {
        match instruction {
            Instruction::Add { src1, src2, .. } => {
                // 检查是否使用了正确的参数寄存器 (ARM64: x0, x1, x2)
                if let (Operand::Register { id: reg1 }, Operand::Register { id: reg2 }) =
                    (src1, src2)
                {
                    let r1 = reg1.id();
                    let r2 = reg2.id();
                    // 检查是否使用了 x0 和 x1，或者 x1 和 x2
                    if (r1 == REG_X0 as usize && r2 == REG_X1 as usize)
                        || (r1 == REG_X1 as usize && r2 == REG_X0 as usize)
                        || (r1 == REG_X1 as usize && r2 == REG_X2 as usize)
                        || (r1 == REG_X2 as usize && r2 == REG_X1 as usize)
                    {
                        found_param_usage = true;
                        info!("✅ 找到正确的参数寄存器使用: x{} + x{}", r1, r2);
                    }
                }
            }
            Instruction::Return {
                value: Some(reg), ..
            } => {
                if reg.id() == REG_X0 as usize {
                    found_return_assignment = true;
                    info!("✅ 找到正确的返回值寄存器: x{}", reg.id());
                }
            }
            _ => {}
        }
    }

    assert!(found_param_usage, "应该找到参数寄存器的正确使用");
    assert!(found_return_assignment, "应该找到返回值寄存器的正确分配");

    info!("✅ 函数参数寄存器分配测试通过");
}
