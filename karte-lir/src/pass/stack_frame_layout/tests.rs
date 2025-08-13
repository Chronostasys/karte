use crate::pass::{stack_frame_layout::StackFrameLayoutPass, AnalysisManager, FunctionPass};
use crate::{AllocationType, Instruction, LirFunction, Operand};
use karte_common::calling_convention::Register;
use karte_diagnostics::Span;

fn build_non_overlapping_two_slots_function() -> LirFunction {
    let mut f = LirFunction::new("test_func".to_string());

    // 两个虚拟地址寄存器作为栈槽地址
    let addr_a = f.new_register();
    let addr_b = f.new_register();
    // 临时值寄存器
    let tmp1 = f.new_register();
    let tmp2 = f.new_register();

    // Alloc 两个 8 字节槽
    f.add_instruction(Instruction::Alloc {
        dst: addr_a,
        size: 8,
        alignment: 8,
        allocation_type: AllocationType::Stack,
        span: Span::dummy(),
    });
    f.add_instruction(Instruction::Alloc {
        dst: addr_b,
        size: 8,
        alignment: 8,
        allocation_type: AllocationType::Stack,
        span: Span::dummy(),
    });

    // 使用 A 槽（前半段）
    f.add_instruction(Instruction::Store64 {
        addr: addr_a,
        offset: 0,
        src: Operand::Immediate { value: 1 },
        span: Span::dummy(),
    });
    f.add_instruction(Instruction::Load64 {
        dst: tmp1,
        addr: addr_a,
        offset: 0,
        span: Span::dummy(),
    });

    // 中间插入一些非相关指令
    f.add_instruction(Instruction::Add {
        dst: tmp1,
        src1: Operand::Register { id: tmp1 },
        src2: Operand::Immediate { value: 10 },
        span: Span::dummy(),
    });

    // 使用 B 槽（后半段），与 A 不重叠
    f.add_instruction(Instruction::Store64 {
        addr: addr_b,
        offset: 0,
        src: Operand::Immediate { value: 2 },
        span: Span::dummy(),
    });
    f.add_instruction(Instruction::Load64 {
        dst: tmp2,
        addr: addr_b,
        offset: 0,
        span: Span::dummy(),
    });

    f
}

#[test]
fn stack_slots_are_reused_when_non_overlapping() {
    let mut f = build_non_overlapping_two_slots_function();

    let mut pass = StackFrameLayoutPass::new();
    let mut analyses = AnalysisManager::new();
    let _ = pass.run_on_function(&mut f, &mut analyses);

    // 所有内存访问都应已经下沉为 FP+offset
    let mut offsets = vec![];
    for instr in &f.instructions {
        match instr {
            Instruction::Load64 { addr, offset, .. } => {
                assert_eq!(*addr, Register::Physical(7));
                offsets.push(*offset);
            }
            Instruction::Store64 { addr, offset, .. } => {
                assert_eq!(*addr, Register::Physical(7));
                offsets.push(*offset);
            }
            _ => {}
        }
    }

    // 预期：两个槽被复用，因此四次访问应具有完全相同的基偏移
    assert!(offsets.len() >= 4);
    let first = offsets[0];
    assert!(offsets.iter().all(|&o| o == first));

    // 栈帧大小应为 8（按 8 对齐）
    assert_eq!(f.stack_frame_size, 8);
}


