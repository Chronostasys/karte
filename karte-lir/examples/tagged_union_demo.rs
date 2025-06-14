//! Tagged Union 演示程序
//! 
//! 演示新的安全加法类型实现，使用结构体作为tagged union，
//! 避免与用户数据的编码冲突。

use karte_lir::{
    LirProgram, LirFunction, Instruction, Operand, RegisterId, 
    StructLayoutManager, AllocationType, tagged_union::TaggedUnionManager
};
use karte_diagnostics::Span;

fn main() {
    println!("=== Tagged Union 安全加法类型演示 ===\n");

    // 1. Tagged Union管理器演示
    demonstrate_tagged_union_manager();

    // 2. LIR指令生成演示
    demonstrate_lir_generation();

    // 3. 完整程序演示
    demonstrate_complete_program();

    println!("\n=== 演示总结 ===");
    println!("新的Tagged Union实现提供了以下优势:");
    println!("✓ 用户可以安全使用所有i64数值，无编码冲突");
    println!("✓ 类型安全的构造器和模式匹配");
    println!("✓ 清晰的内存布局和访问模式");
    println!("✓ 支持嵌套和复杂的加法类型");
    println!("✓ 高效的运行时性能");
}

fn demonstrate_tagged_union_manager() {
    println!("1. Tagged Union管理器演示");
    println!("========================");

    let mut manager = TaggedUnionManager::new();

    // 显示预注册的内置标签
    println!("预注册的内置标签:");
    manager.debug_print_tags();

    // 测试构造器ID生成
    println!("\n构造器ID生成测试:");
    let true_id = manager.get_constructor_id("True");
    let false_id = manager.get_constructor_id("False");
    let some_id = manager.get_constructor_id("Some");
    let none_id = manager.get_constructor_id("None");

    println!("  True -> ID {}", true_id);
    println!("  False -> ID {}", false_id);
    println!("  Some -> ID {}", some_id);
    println!("  None -> ID {}", none_id);

    // 测试用户自定义构造器
    println!("\n用户自定义构造器:");
    let color_red = manager.get_qualified_constructor_id("Color", "Red");
    let color_green = manager.get_qualified_constructor_id("Color", "Green");
    let status_red = manager.get_qualified_constructor_id("Status", "Red");

    println!("  Color::Red -> ID {}", color_red);
    println!("  Color::Green -> ID {}", color_green);
    println!("  Status::Red -> ID {}", status_red);
    println!("  注意: Color::Red 和 Status::Red 有不同的ID ({} vs {})", color_red, status_red);

    // 显示布局信息
    let layout = manager.get_layout();
    println!("\nTagged Union内存布局:");
    println!("  标签偏移: {} 字节", layout.tag_offset);
    println!("  数据偏移: {} 字节", layout.data_offset);
    println!("  总大小: {} 字节", layout.total_size);
    println!("  对齐要求: {} 字节", layout.alignment);
}

fn demonstrate_lir_generation() {
    println!("\n2. LIR指令生成演示");
    println!("===================");

    let mut manager = TaggedUnionManager::new();
    let span = Span::dummy();

    // 演示boolean值的指令生成
    println!("Boolean True值的LIR指令:");
    let dst_reg = RegisterId(1);
    let true_tag_id = manager.get_constructor_id("True");
    let instructions = manager.generate_allocation_instructions(
        dst_reg,
        true_tag_id,
        None,
        span,
    );

    for (i, instruction) in instructions.iter().enumerate() {
        println!("  {}. {}", i + 1, instruction);
    }

    // 演示带数据的构造器指令生成
    println!("\nSome(42)值的LIR指令:");
    let dst_reg = RegisterId(2);
    let some_tag_id = manager.get_constructor_id("Some");
    let data_operand = Some(Operand::Immediate { value: 42 });
    let instructions = manager.generate_allocation_instructions(
        dst_reg,
        some_tag_id,
        data_operand,
        span,
    );

    for (i, instruction) in instructions.iter().enumerate() {
        println!("  {}. {}", i + 1, instruction);
    }

    // 演示标签检查指令
    println!("\n标签检查指令 (检查是否为True):");
    let union_addr = RegisterId(1);
    let temp_reg = RegisterId(10);
    let expected_tag_id = manager.get_constructor_id("True");
    let check_instructions = manager.generate_tag_check_instructions(
        union_addr,
        expected_tag_id,
        temp_reg,
        span,
    );

    for (i, instruction) in check_instructions.iter().enumerate() {
        println!("  {}. {}", i + 1, instruction);
    }

    // 演示数据提取指令
    println!("\n数据提取指令 (从Some中提取值):");
    let union_addr = RegisterId(2);
    let dst_reg = RegisterId(11);
    let extract_instructions = manager.generate_data_extraction_instructions(
        union_addr,
        dst_reg,
        span,
    );

    for (i, instruction) in extract_instructions.iter().enumerate() {
        println!("  {}. {}", i + 1, instruction);
    }
}

fn demonstrate_complete_program() {
    println!("\n3. 完整程序演示");
    println!("===============");

    println!("生成演示程序: Option匹配和数值安全");

    let mut main_function = LirFunction::new("main".to_string());
    let mut manager = TaggedUnionManager::new();
    let span = Span::dummy();

    println!("\n生成的LIR函数:");
    
    // 创建 Some(999999999999) - 使用极大数值测试安全性
    let some_addr = main_function.new_register();
    let some_tag_id = manager.get_constructor_id("Some");
    let large_number = 999999999999i64; // 用户数据
    
    let some_instructions = manager.generate_allocation_instructions(
        some_addr,
        some_tag_id,
        Some(Operand::Immediate { value: large_number }),
        span,
    );
    
    for instruction in some_instructions {
        main_function.add_instruction(instruction);
    }

    // 创建 None
    let none_addr = main_function.new_register();
    let none_tag_id = manager.get_constructor_id("None");
    
    let none_instructions = manager.generate_allocation_instructions(
        none_addr,
        none_tag_id,
        None,
        span,
    );
    
    for instruction in none_instructions {
        main_function.add_instruction(instruction);
    }

    // 模式匹配演示: 检查some_addr是否为Some
    let temp_reg = main_function.new_register();
    let result_reg = main_function.new_register();
    
    // 检查标签
    let tag_check_instructions = manager.generate_tag_check_instructions(
        some_addr,
        some_tag_id,
        temp_reg,
        span,
    );
    
    for instruction in tag_check_instructions {
        main_function.add_instruction(instruction);
    }

    // 如果匹配，提取数据
    main_function.add_instruction(Instruction::JumpNotEqual {
        target: karte_lir::LabelId(100), // none_case
        span,
    });

    // Some分支: 提取数据
    let extract_instructions = manager.generate_data_extraction_instructions(
        some_addr,
        result_reg,
        span,
    );
    
    for instruction in extract_instructions {
        main_function.add_instruction(instruction);
    }

    main_function.add_instruction(Instruction::Jump {
        target: karte_lir::LabelId(200), // end
        span,
    });

    // None分支
    main_function.add_instruction(Instruction::Label {
        id: karte_lir::LabelId(100),
        span,
    });
    
    main_function.add_instruction(Instruction::Move {
        dst: result_reg,
        src: Operand::Immediate { value: 0 },
        span,
    });

    // 结束
    main_function.add_instruction(Instruction::Label {
        id: karte_lir::LabelId(200),
        span,
    });

    main_function.add_instruction(Instruction::Return {
        value: Some(result_reg),
        span,
    });

    // 显示生成的函数
    println!("function main:");
    for (i, instruction) in main_function.instructions.iter().enumerate() {
        println!("  {:2}. {}", i, instruction);
    }

    println!("\n程序说明:");
    println!("  1. 创建 Some({}) - 使用极大数值测试", large_number);
    println!("  2. 创建 None");
    println!("  3. 检查第一个值是否为Some");
    println!("  4. 如果是Some，提取其中的数据返回");
    println!("  5. 如果是None，返回0");
    println!("  6. 用户数值 {} 完全安全，不会与编译器内部编码冲突", large_number);

    // 创建完整程序
    let mut program = LirProgram::new();
    program.add_function(main_function);
    program.set_main("main".to_string());

    println!("\n程序统计:");
    println!("  函数数量: {}", program.functions.len());
    println!("  主函数: {:?}", program.main_function);
    println!("  指令总数: {}", program.functions.get("main").map_or(0, |f| f.instructions.len()));
} 