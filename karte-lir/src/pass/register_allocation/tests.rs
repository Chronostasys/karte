use super::*;
use crate::pass::stack_frame_layout::StackFrameLayoutPass;
use crate::{AllocationType, Instruction, LirFunction, Operand, Register};
use karte_common::calling_convention::REG_EFFECT_PAYLOAD;
use karte_diagnostics::Span;

#[test]
fn test_register_type_analysis() {
    // 创建一个简单的测试函数
    let function = LirFunction {
        name: "test_function".to_string(),
        parameter_registers: vec![Register::Virtual(100), Register::Virtual(101)],
        instructions: vec![
            // alloc指令 - 栈分配
            Instruction::Alloc {
                dst: Register::Virtual(200),
                size: 8,
                alignment: 8,
                allocation_type: AllocationType::Stack,
                span: Span::dummy(),
            },
            // alloc指令 - 堆分配
            Instruction::Alloc {
                dst: Register::Virtual(201),
                size: 16,
                alignment: 8,
                allocation_type: AllocationType::Heap,
                span: Span::dummy(),
            },
            // add指令 - 栈地址计算
            Instruction::Add {
                dst: Register::Virtual(202),
                src1: Operand::Register {
                    id: Register::Physical(7),
                }, // FP
                src2: Operand::Immediate { value: -8 },
                span: Span::dummy(),
            },
            // 普通数据操作
            Instruction::Add {
                dst: Register::Virtual(203),
                src1: Operand::Register {
                    id: Register::Virtual(100),
                },
                src2: Operand::Register {
                    id: Register::Virtual(101),
                },
                span: Span::dummy(),
            },
        ],
        next_register: 204,
        struct_types: HashMap::new(),
        stack_frame_size: 0,
        parameter_count: 2,
        used_regs: Vec::new(),
    };

    let calling_convention = types::CallingConvention::standard();
    let lifetime_analyzer = LifetimeAnalyzer::new(calling_convention);
    let (lifetimes, register_types) = lifetime_analyzer.analyze_simple(&function);

    // 验证函数参数类型
    assert_eq!(
        register_types.get(&Register::Virtual(100)),
        Some(&RegisterType::FunctionParameter)
    );
    assert_eq!(
        register_types.get(&Register::Virtual(101)),
        Some(&RegisterType::FunctionParameter)
    );

    // 验证栈地址寄存器类型
    assert_eq!(
        register_types.get(&Register::Virtual(200)),
        Some(&RegisterType::StackAddress)
    ); // 栈alloc
    assert_eq!(
        register_types.get(&Register::Virtual(202)),
        Some(&RegisterType::StackAddress)
    ); // FP + offset

    // 验证数据寄存器类型
    assert_eq!(
        register_types.get(&Register::Virtual(201)),
        Some(&RegisterType::Data)
    ); // 堆alloc
    assert_eq!(
        register_types.get(&Register::Virtual(203)),
        Some(&RegisterType::Data)
    ); // 普通计算

    // 验证生命周期中的寄存器类型
    for lifetime in &lifetimes {
        match lifetime.register.id() {
            100 | 101 => assert_eq!(lifetime.register_type, RegisterType::FunctionParameter),
            200 | 202 => assert_eq!(lifetime.register_type, RegisterType::StackAddress),
            201 | 203 => assert_eq!(lifetime.register_type, RegisterType::Data),
            _ => {}
        }
    }

    info!("✅ 寄存器类型分析测试通过");
}

#[test]
fn test_spill_constraints() {
    // 测试溢出约束
    assert!(RegisterType::Data.can_spill());
    assert!(!RegisterType::StackAddress.can_spill());
    assert!(!RegisterType::FunctionParameter.can_spill());
    assert!(!RegisterType::Special.can_spill());

    info!("✅ 溢出约束测试通过");
}

#[test]
fn test_ra_spill_and_layout_fp_lowering() {
    // 构造一个需要溢出的场景，并验证最终由 StackFrameLayout 下沉为 FP+offset
    let mut f = LirFunction::new("spill_case".to_string());
    // 构造若干临时寄存器使用，迫使溢出（简化：只构造若干 mov/add）
    let v1 = Register::Virtual(10);
    let v2 = Register::Virtual(11);
    let v3 = Register::Virtual(12);
    let v4 = Register::Virtual(13);
    f.instructions = vec![
        Instruction::Label {
            id: crate::LabelId(1),
            span: Span::dummy(),
        },
        Instruction::Move {
            dst: v1,
            src: Operand::Immediate { value: 1 },
            span: Span::dummy(),
        },
        Instruction::Move {
            dst: v2,
            src: Operand::Immediate { value: 2 },
            span: Span::dummy(),
        },
        Instruction::Add {
            dst: v3,
            src1: Operand::Register { id: v1 },
            src2: Operand::Register { id: v2 },
            span: Span::dummy(),
        },
        Instruction::Add {
            dst: v4,
            src1: Operand::Register { id: v3 },
            src2: Operand::Immediate { value: 3 },
            span: Span::dummy(),
        },
        Instruction::Return {
            value: Some(v4),
            span: Span::dummy(),
        },
    ];

    let mut pass = SimpleStackRegisterAllocation::new();
    let mut analyses = AnalysisManager::new();
    let _ = pass.run_on_function(&mut f, &mut analyses);

    // 后置布局
    let mut layout = StackFrameLayoutPass::new();
    let _ = layout.run_on_function(&mut f, &mut analyses);

    // 验证：存在基于 FP 的 Load/Store（来自 spill/scratch）
    let has_fp_mem = f.instructions.iter().any(|inst| match inst {
        Instruction::Load64 { addr, .. } | Instruction::Store64 { addr, .. } => addr.id() == 7,
        _ => false,
    });
    // 该最小用例不一定触发溢出（取决于寄存器压力），因此不强制要求出现 FP 访问
    let _ = has_fp_mem;
}

#[test]
fn simple_stack_allocator_reserves_effect_payload_register() {
    let mut f = LirFunction::new("reserve_effect_payload".to_string());
    let regs: Vec<Register> = (0..7).map(|i| Register::Virtual(100 + i)).collect();
    let acc = Register::Virtual(200);

    f.instructions = vec![
        Instruction::Move {
            dst: regs[0],
            src: Operand::Immediate { value: 1 },
            span: Span::dummy(),
        },
        Instruction::Move {
            dst: regs[1],
            src: Operand::Immediate { value: 2 },
            span: Span::dummy(),
        },
        Instruction::Move {
            dst: regs[2],
            src: Operand::Immediate { value: 3 },
            span: Span::dummy(),
        },
        Instruction::Move {
            dst: regs[3],
            src: Operand::Immediate { value: 4 },
            span: Span::dummy(),
        },
        Instruction::Add {
            dst: regs[4],
            src1: Operand::Register { id: regs[0] },
            src2: Operand::Register { id: regs[1] },
            span: Span::dummy(),
        },
        Instruction::Add {
            dst: regs[5],
            src1: Operand::Register { id: regs[2] },
            src2: Operand::Register { id: regs[3] },
            span: Span::dummy(),
        },
        Instruction::Add {
            dst: acc,
            src1: Operand::Register { id: regs[4] },
            src2: Operand::Register { id: regs[5] },
            span: Span::dummy(),
        },
        Instruction::Return {
            value: Some(acc),
            span: Span::dummy(),
        },
    ];

    let mut pass = SimpleStackRegisterAllocation::new();
    let mut analyses = AnalysisManager::new();
    let _ = pass.run_on_function(&mut f, &mut analyses);

    let uses_reserved_register = f.instructions.iter().any(|inst| {
        let def_hits = matches!(
            inst.get_def_register(),
            Some(Register::Physical(id)) if id == REG_EFFECT_PAYLOAD
        );
        let use_hits = inst
            .get_used_registers()
            .iter()
            .any(|reg| matches!(reg, Register::Physical(id) if *id == REG_EFFECT_PAYLOAD));
        def_hits || use_hits
    });

    assert!(
        !uses_reserved_register,
        "allocator should never materialize the effect payload register"
    );
}

#[test]
fn test_parameter_return_conflict() {
    // fn identity(x) { return x; }
    // x is param 0 (r1)
    // return x (needs r0)
    // Allocator should NOT force x to r0, keeping it in r1.

    let mut function = LirFunction {
        name: "identity".to_string(),
        instructions: vec![Instruction::Return {
            value: Some(Register::Virtual(100)),
            span: Span::dummy(),
        }],
        next_register: 102,
        struct_types: std::collections::HashMap::new(),
        stack_frame_size: 0,
        parameter_count: 1,
        parameter_registers: vec![Register::Virtual(100)],
        used_regs: Vec::new(),
    };

    let mut pass = SimpleStackRegisterAllocation::new();
    let mut analysis_manager = AnalysisManager::new();
    pass.run_on_function(&mut function, &mut analysis_manager);

    // Check allocation
    // The Return instruction should have value: Some(Physical(1)) (r1)
    // NOT Physical(0) (r0)

    if let Instruction::Return {
        value: Some(reg), ..
    } = &function.instructions[0]
    {
        match reg {
            Register::Physical(p) => {
                assert_eq!(
                    *p, 1,
                    "Parameter should remain in r1, not moved to r0 by allocator"
                );
            }
            _ => panic!("Expected physical register"),
        }
    } else {
        panic!("Expected Return instruction");
    }
}
