use crate::AnalysisPass;

use super::*;
use karte_diagnostics::Span;

#[test]
fn test_index_based_transformer() {
    info!("🔧 测试 index-based 变换系统");

    let mut transformer = IndexInstructionTransformer::new();

    // 创建一个测试函数
    let mut function = LirFunction::new("test_function".to_string());

    // 添加一些测试指令
    function.instructions.push(Instruction::Move {
        dst: Register::Virtual(1),
        src: Operand::Immediate { value: 42 },
        span: Span { start: 0, end: 0 },
    });

    function.instructions.push(Instruction::Store64 {
        addr: Register::Virtual(2),
        offset: 0,
        src: Operand::Immediate { value: 100 },
        span: Span { start: 0, end: 0 },
    });

    function.instructions.push(Instruction::Load64 {
        dst: Register::Virtual(3),
        addr: Register::Virtual(2),
        offset: 0,
        span: Span { start: 0, end: 0 },
    });

    info!("🔧 原始函数有 {} 条指令", function.instructions.len());

    // 添加变换操作：删除第1个指令，替换第2个指令
    transformer.remove(1);
    transformer.replace(
        2,
        Instruction::Move {
            dst: Register::Virtual(3),
            src: Operand::Immediate { value: 200 },
            span: Span { start: 0, end: 0 },
        },
    );

    // 应用变换
    let (changed, _, _, _) = transformer.apply_to_function(&mut function);

    info!(
        "🔧 应用变换后，函数有 {} 条指令",
        function.instructions.len()
    );
    info!("🔧 变换是否成功: {}", changed);

    // 验证结果
    assert_eq!(function.instructions.len(), 2); // 删除了1个，保留了2个

    // 第一个指令应该是原始的Move
    if let Instruction::Move { dst, src, .. } = &function.instructions[0] {
        assert_eq!(*dst, Register::Virtual(1));
        assert_eq!(*src, Operand::Immediate { value: 42 });
        info!("✅ 第一个指令正确保留");
    } else {
        panic!("第一个指令应该是Move");
    }

    // 第二个指令应该是替换后的Move
    if let Instruction::Move { dst, src, .. } = &function.instructions[1] {
        assert_eq!(*dst, Register::Virtual(3));
        assert_eq!(*src, Operand::Immediate { value: 200 });
        info!("✅ 第二个指令正确替换");
    } else {
        panic!("第二个指令应该是Move");
    }

    info!("🔧 index-based 变换系统测试完成 ✅");
}

#[test]
fn test_history_based_transformer() {
    info!("🧠 测试基于历史的变换系统");

    let mut transformer = HistoryBasedTransformer::new();

    // 创建一个测试函数
    let mut function = LirFunction::new("test_function".to_string());

    // 添加一些测试指令
    for i in 0..5 {
        function.instructions.push(Instruction::Move {
            dst: Register::Virtual(i),
            src: Operand::Immediate { value: i as i64 },
            span: Span { start: 0, end: 0 },
        });
    }

    info!("🧠 原始函数有 {} 条指令", function.instructions.len());

    // 添加变换操作：删除第1个，替换第3个，插入到第2个位置
    transformer.remove_at(1);
    transformer.replace_at(
        3,
        Instruction::Move {
            dst: Register::Virtual(99),
            src: Operand::Immediate { value: 999 },
            span: Span { start: 0, end: 0 },
        },
    );
    transformer.insert_at(
        2,
        Instruction::Move {
            dst: Register::Virtual(88),
            src: Operand::Immediate { value: 888 },
            span: Span { start: 0, end: 0 },
        },
    );

    // 应用变换
    let (changed, _, _, _) = transformer.apply_to_function(&mut function);

    info!(
        "🧠 应用变换后，函数有 {} 条指令",
        function.instructions.len()
    );
    info!("🧠 变换是否成功: {}", changed);

    // 验证结果
    assert_eq!(function.instructions.len(), 5); // 删除了1个，插入了1个，总共5个

    // 验证指令顺序
    if let Instruction::Move { dst, src, .. } = &function.instructions[0] {
        assert_eq!(*dst, Register::Virtual(0));
        assert_eq!(*src, Operand::Immediate { value: 0 });
    }

    // 2 被替换了
    if let Instruction::Move { dst, src, .. } = &function.instructions[1] {
        assert_eq!(*dst, Register::Virtual(88));
        assert_eq!(*src, Operand::Immediate { value: 888 });
    }

    if let Instruction::Move { dst, src, .. } = &function.instructions[2] {
        assert_eq!(*dst, Register::Virtual(2));
        assert_eq!(*src, Operand::Immediate { value: 2 });
    }

    if let Instruction::Move { dst, src, .. } = &function.instructions[3] {
        assert_eq!(*dst, Register::Virtual(99));
        assert_eq!(*src, Operand::Immediate { value: 999 });
    }

    if let Instruction::Move { dst, src, .. } = &function.instructions[4] {
        assert_eq!(*dst, Register::Virtual(4));
        assert_eq!(*src, Operand::Immediate { value: 4 });
    }

    info!("🧠 基于历史的变换系统测试完成 ✅");
}

#[test]
fn test_phi_insertion_algorithm() {
    info!("🎯 测试phi节点插入算法");

    // 创建测试函数
    let mut function = LirFunction::new("test_phi_function".to_string());

    // 添加指令：模拟用户提供的示例
    function.instructions.push(Instruction::Label {
        id: LabelId(1),
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Alloc {
        dst: Register::Physical(0),
        size: 8,
        alignment: 8,
        allocation_type: AllocationType::Stack,
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Alloc {
        dst: Register::Virtual(1),
        size: 8,
        alignment: 8,
        allocation_type: AllocationType::Stack,
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Alloc {
        dst: Register::Virtual(2),
        size: 8,
        alignment: 8,
        allocation_type: AllocationType::Stack,
        span: Span::dummy(),
    });

    function.instructions.push(Instruction::Label {
        id: LabelId(8),
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Store64 {
        addr: Register::Virtual(1),
        offset: 0,
        src: Operand::Immediate { value: 1 },
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Load64 {
        dst: Register::Virtual(3),
        addr: Register::Virtual(1),
        offset: 0,
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Compare {
        src1: Operand::Register {
            id: Register::Virtual(3),
        },
        src2: Operand::Immediate { value: 0 },
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::JumpNotEqual {
        target: LabelId(6),
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Jump {
        target: LabelId(7),
        span: Span::dummy(),
    });

    function.instructions.push(Instruction::Label {
        id: LabelId(6),
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Store64 {
        addr: Register::Virtual(2),
        offset: 0,
        src: Operand::Immediate { value: 0 },
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Load64 {
        dst: Register::Virtual(4),
        addr: Register::Virtual(2),
        offset: 0,
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Compare {
        src1: Operand::Register {
            id: Register::Virtual(4),
        },
        src2: Operand::Immediate { value: 0 },
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::JumpNotEqual {
        target: LabelId(3),
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Jump {
        target: LabelId(5),
        span: Span::dummy(),
    });

    function.instructions.push(Instruction::Label {
        id: LabelId(7),
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Jump {
        target: LabelId(4),
        span: Span::dummy(),
    });

    function.instructions.push(Instruction::Label {
        id: LabelId(4),
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Load64 {
        dst: Register::Virtual(5),
        addr: Register::Physical(0),
        offset: 0,
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Return {
        value: Some(Register::Virtual(5)),
        span: Span::dummy(),
    });

    function.instructions.push(Instruction::Label {
        id: LabelId(3),
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Store64 {
        addr: Register::Physical(0),
        offset: 0,
        src: Operand::Immediate { value: 1 },
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Jump {
        target: LabelId(2),
        span: Span::dummy(),
    });

    function.instructions.push(Instruction::Label {
        id: LabelId(5),
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Store64 {
        addr: Register::Physical(0),
        offset: 0,
        src: Operand::Immediate { value: 2 },
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Jump {
        target: LabelId(2),
        span: Span::dummy(),
    });

    function.instructions.push(Instruction::Label {
        id: LabelId(2),
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Jump {
        target: LabelId(4),
        span: Span::dummy(),
    });

    info!(
        "🎯 创建测试函数，共 {} 条指令：",
        function.instructions.len()
    );
    info!("function test_phi_function (stack_frame: 0):");
    for (i, instruction) in function.instructions.iter().enumerate() {
        info!("  [{}] {}", i, instruction);
    }

    // 创建分析结果
    let mut analysis = Memory2RegAnalysis {
        stack_slots: HashMap::new(),
        promotable_slots: vec![Register::Physical(0)],
        basic_blocks: HashMap::new(),
        phi_insertions: Vec::new(),
        dominance_info: None,
    };

    // 模拟CFG分析结果
    analysis.basic_blocks.insert(
        0,
        BasicBlock {
            id: 0,
            label: Some(LabelId(1)),
            start: 0,
            end: 4,
            predecessors: vec![],
            successors: vec![1],
        },
    );
    analysis.basic_blocks.insert(
        1,
        BasicBlock {
            id: 1,
            label: Some(LabelId(8)),
            start: 4,
            end: 9,
            predecessors: vec![0],
            successors: vec![2, 3],
        },
    );
    analysis.basic_blocks.insert(
        2,
        BasicBlock {
            id: 2,
            label: Some(LabelId(6)),
            start: 9,
            end: 14,
            predecessors: vec![1],
            successors: vec![4, 5],
        },
    );
    analysis.basic_blocks.insert(
        3,
        BasicBlock {
            id: 3,
            label: Some(LabelId(7)),
            start: 14,
            end: 15,
            predecessors: vec![1],
            successors: vec![4],
        },
    );
    analysis.basic_blocks.insert(
        4,
        BasicBlock {
            id: 4,
            label: Some(LabelId(4)),
            start: 15,
            end: 20,
            predecessors: vec![2, 7],
            successors: vec![],
        },
    );
    analysis.basic_blocks.insert(
        5,
        BasicBlock {
            id: 5,
            label: Some(LabelId(3)),
            start: 20,
            end: 22,
            predecessors: vec![2],
            successors: vec![7],
        },
    );
    analysis.basic_blocks.insert(
        6,
        BasicBlock {
            id: 6,
            label: Some(LabelId(5)),
            start: 22,
            end: 24,
            predecessors: vec![2],
            successors: vec![7],
        },
    );
    analysis.basic_blocks.insert(
        7,
        BasicBlock {
            id: 7,
            label: Some(LabelId(2)),
            start: 27,
            end: 29,
            predecessors: vec![5, 6],
            successors: vec![4],
        },
    );

    // 自动收集store/load索引和块映射
    let mut slot = StackSlot {
        alloc_instruction: 1,
        address_register: Register::Physical(0),
        size: 8,
        promotable: true,
        loads: Vec::new(),
        stores: Vec::new(),
        store_to_block: HashMap::new(),
        load_to_block: HashMap::new(),
    };
    // 反向映射：指令索引 -> 块id
    let mut inst_to_block = HashMap::new();
    for (block_id, block) in &analysis.basic_blocks {
        for i in block.start..block.end {
            inst_to_block.insert(i, *block_id);
        }
    }
    for (i, instr) in function.instructions.iter().enumerate() {
        match instr {
            Instruction::Store64 { addr, .. } if *addr == Register::Physical(0) => {
                slot.stores.push(i);
                if let Some(&block_id) = inst_to_block.get(&i) {
                    slot.store_to_block.insert(i, block_id);
                }
            }
            Instruction::Load64 { addr, .. } if *addr == Register::Physical(0) => {
                slot.loads.push(i);
                if let Some(&block_id) = inst_to_block.get(&i) {
                    slot.load_to_block.insert(i, block_id);
                }
            }
            _ => {}
        }
    }
    analysis.stack_slots.insert(Register::Physical(0), slot);

    let dominance_info = DominanceInfo::default();

    // 测试phi节点插入算法
    let pass = Memory2RegPass::new();

    // 🔧 调试：打印每个块的指令内容
    info!("🔍 调试：打印每个块的指令内容");
    for (block_id, block) in &analysis.basic_blocks {
        info!("🔍 块{}: 范围[{}, {})", block_id, block.start, block.end);
        for i in block.start..block.end {
            if i < function.instructions.len() {
                info!("🔍   指令{}: {:?}", i, function.instructions[i]);
            }
        }
    }

    let phi_insertions = pass.compute_phi_insertions(&mut function, &analysis, &dominance_info);

    info!("🎯 计算出的phi节点数量: {}", phi_insertions.len());
    for phi in &phi_insertions {
        info!("🎯 phi节点: {:?}", phi);
    }

    // 🔧 验证：检查phi节点是否正确
    // 应该只在L4插入phi节点，因为只有L4有load且多前驱
    assert_eq!(phi_insertions.len(), 2, "应该插入2个phi节点");

    let phi = &phi_insertions[0];
    assert_eq!(phi.block_id, 4, "phi节点应该在块4（L4）");

    // 验证incoming值：应该来自L2和L7
    assert_eq!(phi.incoming.len(), 2, "phi节点应该有2个incoming值");

    // 检查来自L2的值（应该是默认值0，因为L2没有store）
    let l2_incoming = phi.incoming.iter().find(|(label, _)| label.0 == 6);
    assert!(l2_incoming.is_some(), "应该有来自L2的incoming值");
    if let Some((_, value)) = l2_incoming {
        match value {
            Operand::Immediate { value: 0 } => info!("✅ L2的incoming值正确：默认值0"),
            _ => panic!("L2的incoming值应该是默认值0，但得到{:?}", value),
        }
    }

    // 检查来自L7的值（应该是L7的phi结果寄存器）
    let l7_incoming = phi.incoming.iter().find(|(label, _)| label.0 == 2); // L7的标签是LabelId(2)
    assert!(l7_incoming.is_some(), "应该有来自L7的incoming值");
    if let Some((_, value)) = l7_incoming {
        match value {
            Operand::Register { .. } => info!("✅ L7的incoming值正确：phi结果寄存器"),
            _ => panic!("L7的incoming值应该是phi结果寄存器，但得到{:?}", value),
        }
    }

    info!("✅ phi节点插入算法测试通过");
}

#[test]
fn test_phi_insertion_returns_1_bug() {
    info!("🎯 测试phi节点插入导致返回值错误的问题");

    // 创建测试函数：模拟用户提供的返回1的代码
    let mut function = LirFunction::new("main".to_string());

    // 添加指令：模拟2.log中的代码
    function.instructions.push(Instruction::Label {
        id: LabelId(1),
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Alloc {
        dst: Register::Physical(0),
        size: 8,
        alignment: 8,
        allocation_type: AllocationType::Stack,
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Alloc {
        dst: Register::Virtual(1),
        size: 8,
        alignment: 8,
        allocation_type: AllocationType::Stack,
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Alloc {
        dst: Register::Virtual(2),
        size: 8,
        alignment: 8,
        allocation_type: AllocationType::Stack,
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Alloc {
        dst: Register::Virtual(3),
        size: 8,
        alignment: 8,
        allocation_type: AllocationType::Stack,
        span: Span::dummy(),
    });

    function.instructions.push(Instruction::Label {
        id: LabelId(5),
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Store64 {
        addr: Register::Virtual(3),
        offset: 0,
        src: Operand::Immediate { value: 1 },
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Load64 {
        dst: Register::Virtual(5),
        addr: Register::Virtual(3),
        offset: 0,
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Move {
        dst: Register::Virtual(8),
        src: Operand::Immediate { value: 1 },
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Sub {
        dst: Register::Virtual(4),
        src1: Operand::Register {
            id: Register::Virtual(8),
        },
        src2: Operand::Register {
            id: Register::Virtual(5),
        },
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Store64 {
        addr: Register::Virtual(2),
        offset: 0,
        src: Operand::Register {
            id: Register::Virtual(4),
        },
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Load64 {
        dst: Register::Virtual(10),
        addr: Register::Virtual(2),
        offset: 0,
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Move {
        dst: Register::Virtual(11),
        src: Operand::Immediate { value: 1 },
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Sub {
        dst: Register::Virtual(9),
        src1: Operand::Register {
            id: Register::Virtual(11),
        },
        src2: Operand::Register {
            id: Register::Virtual(10),
        },
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Store64 {
        addr: Register::Virtual(1),
        offset: 0,
        src: Operand::Register {
            id: Register::Virtual(9),
        },
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Load64 {
        dst: Register::Virtual(12),
        addr: Register::Virtual(1),
        offset: 0,
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Compare {
        src1: Operand::Register {
            id: Register::Virtual(12),
        },
        src2: Operand::Immediate { value: 1 },
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::JumpEqual {
        target: LabelId(2),
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Compare {
        src1: Operand::Register {
            id: Register::Virtual(12),
        },
        src2: Operand::Immediate { value: 0 },
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::JumpEqual {
        target: LabelId(3),
        span: Span::dummy(),
    });

    function.instructions.push(Instruction::Label {
        id: LabelId(2),
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Store64 {
        addr: Register::Physical(0),
        offset: 0,
        src: Operand::Immediate { value: 1 },
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Jump {
        target: LabelId(4),
        span: Span::dummy(),
    });

    function.instructions.push(Instruction::Label {
        id: LabelId(3),
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Store64 {
        addr: Register::Physical(0),
        offset: 0,
        src: Operand::Immediate { value: 0 },
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Jump {
        target: LabelId(4),
        span: Span::dummy(),
    });

    function.instructions.push(Instruction::Label {
        id: LabelId(4),
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Load64 {
        dst: Register::Virtual(13),
        addr: Register::Physical(0),
        offset: 0,
        span: Span::dummy(),
    });
    function.instructions.push(Instruction::Return {
        value: Some(Register::Virtual(13)),
        span: Span::dummy(),
    });

    info!(
        "🎯 创建返回1的测试函数，共 {} 条指令：",
        function.instructions.len()
    );

    // 🔧 使用CFG pass生成正确的basic_blocks
    let mut cfg_analysis = crate::pass::analysis::ControlFlowAnalysis::new();
    let analysis_manager = crate::pass::AnalysisManager::new();
    let cfg_result = cfg_analysis
        .analyze_function(&function, &analysis_manager)
        .unwrap();
    let cfg = cfg_result
        .as_any()
        .downcast_ref::<crate::pass::analysis::ControlFlowGraph>()
        .unwrap();

    info!("🔧 CFG分析结果:");
    for node in &cfg.nodes {
        info!(
            "  块{}: 范围[{}, {}), 标签={:?}, 前驱={:?}, 后继={:?}",
            node.block_id,
            node.instruction_range.0,
            node.instruction_range.1,
            node.label,
            node.predecessors,
            node.successors
        );
    }

    // 将CFG结果转换为Memory2RegAnalysis需要的格式
    let mut analysis = Memory2RegAnalysis {
        stack_slots: HashMap::new(),
        promotable_slots: vec![Register::Physical(0)],
        basic_blocks: HashMap::new(),
        phi_insertions: Vec::new(),
        dominance_info: None,
    };

    // 转换CFG节点为BasicBlock
    for node in &cfg.nodes {
        let basic_block = BasicBlock {
            id: node.block_id,
            label: node.label,
            start: node.instruction_range.0,
            end: node.instruction_range.1,
            predecessors: node.predecessors.clone(),
            successors: node.successors.clone(),
        };
        analysis.basic_blocks.insert(node.block_id, basic_block);
    }

    // 自动收集store/load索引和块映射
    let mut slot = StackSlot {
        alloc_instruction: 1,
        address_register: Register::Physical(0),
        size: 8,
        promotable: true,
        loads: Vec::new(),
        stores: Vec::new(),
        store_to_block: HashMap::new(),
        load_to_block: HashMap::new(),
    };

    // 反向映射：指令索引 -> 块id
    let mut inst_to_block = HashMap::new();
    for (block_id, block) in &analysis.basic_blocks {
        for i in block.start..block.end {
            inst_to_block.insert(i, *block_id);
        }
    }

    for (i, instr) in function.instructions.iter().enumerate() {
        match instr {
            Instruction::Store64 { addr, .. } if *addr == Register::Physical(0) => {
                slot.stores.push(i);
                if let Some(&block_id) = inst_to_block.get(&i) {
                    slot.store_to_block.insert(i, block_id);
                }
            }
            Instruction::Load64 { addr, .. } if *addr == Register::Physical(0) => {
                slot.loads.push(i);
                if let Some(&block_id) = inst_to_block.get(&i) {
                    slot.load_to_block.insert(i, block_id);
                }
            }
            _ => {}
        }
    }
    analysis.stack_slots.insert(Register::Physical(0), slot);

    let dominance_info = DominanceInfo::default();

    // 测试phi节点插入算法
    let pass = Memory2RegPass::new();

    // 🔧 调试：打印每个块的指令内容
    info!("🔍 调试：打印每个块的指令内容");
    for (block_id, block) in &analysis.basic_blocks {
        info!("🔍 块{}: 范围[{}, {})", block_id, block.start, block.end);
        for i in block.start..block.end {
            if i < function.instructions.len() {
                info!("🔍   指令{}: {:?}", i, function.instructions[i]);
            }
        }
    }

    let phi_insertions = pass.compute_phi_insertions(&mut function, &analysis, &dominance_info);

    info!("🎯 计算出的phi节点数量: {}", phi_insertions.len());
    for phi in &phi_insertions {
        info!("🎯 phi节点: {:?}", phi);
    }

    // 🔧 验证：检查phi节点是否正确
    // 只在L4插入phi节点
    assert_eq!(phi_insertions.len(), 1, "应该只在L4插入phi节点");
    let phi = &phi_insertions[0];
    assert_eq!(phi.block_id, 5, "phi节点应该在块5（L4）");
    assert_eq!(phi.incoming.len(), 2, "phi节点应该有2个incoming值");
    let l2_incoming = phi.incoming.iter().find(|(label, _)| label.0 == 2);
    assert!(l2_incoming.is_some(), "应该找到来自L2的incoming值");
    if let Some((_, value)) = l2_incoming {
        match value {
            Operand::Immediate { value: 1 } => info!("✅ L2的incoming值正确: 1"),
            _ => panic!("L2的incoming值应该是1，实际是: {:?}", value),
        }
    }
    let l3_incoming = phi.incoming.iter().find(|(label, _)| label.0 == 3);
    assert!(l3_incoming.is_some(), "应该找到来自L3的incoming值");
    if let Some((_, value)) = l3_incoming {
        match value {
            Operand::Immediate { value: 0 } => info!("✅ L3的incoming值正确: 0"),
            _ => panic!("L3的incoming值应该是0，实际是: {:?}", value),
        }
    }
}
