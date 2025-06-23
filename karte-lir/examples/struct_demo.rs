//! LIR结构体支持演示
//! 
//! 这个文件演示了改进后的LIR系统对结构体的专业支持

use karte_lir::{
    LirProgram, LirFunction, Instruction, Operand, Register, 
    StructLayoutManager, StructLayout, StructField, StructTypeId, AllocationType
};
use karte_diagnostics::Span;

fn main() {
    println!("=== LIR结构体支持演示 ===\n");
    
    // 演示结构体布局管理
    demo_struct_layout_manager();
    
    // 演示LIR结构体指令
    demo_lir_struct_instructions();
    
    // 演示完整的结构体程序
    demo_complete_struct_program();
}

/// 演示结构体布局管理器
fn demo_struct_layout_manager() {
    println!("1. 结构体布局管理器演示");
    println!("========================");
    
    let mut layout_manager = StructLayoutManager::new();
    
    // 创建Point结构体的模拟字段（在实际应用中这些来自HIR）
    use karte_hir::types::{Type, StructField as HirStructField};
    
    let point_fields = vec![
        HirStructField {
            name: "x".to_string(),
            field_type: Type::Number,
        },
        HirStructField {
            name: "y".to_string(),
            field_type: Type::Number,
        },
    ];
    
    let rectangle_fields = vec![
        HirStructField {
            name: "top_left".to_string(),
            field_type: Type::Struct {
                name: "Point".to_string(),
                fields: point_fields.clone(),
            },
        },
        HirStructField {
            name: "width".to_string(),
            field_type: Type::Number,
        },
        HirStructField {
            name: "height".to_string(),
            field_type: Type::Number,
        },
    ];
    
    // 计算布局
    let point_layout = layout_manager.compute_layout("Point", &point_fields).unwrap();
    let rectangle_layout = layout_manager.compute_layout("Rectangle", &rectangle_fields).unwrap();
    
    println!("Point结构体布局:");
    println!("{}", point_layout);
    
    println!("Rectangle结构体布局:");
    println!("{}", rectangle_layout);
    
    // 布局分析
    let point_analysis = layout_manager.analyze_layout(&point_layout);
    println!("Point结构体分析:");
    println!("  总大小: {} 字节", point_analysis.total_size);
    println!("  字段数量: {}", point_analysis.field_count);
    println!("  填充开销: {} 字节 ({:.1}%)", 
             point_analysis.padding_overhead, 
             point_analysis.padding_ratio * 100.0);
    
    for suggestion in &point_analysis.suggestions {
        println!("  建议: {}", suggestion);
    }
    
    println!();
}

/// 演示LIR结构体指令
fn demo_lir_struct_instructions() {
    println!("2. LIR结构体指令演示");
    println!("===================");
    
    let mut function = LirFunction::new("demo_struct_ops".to_string());
    
    // 添加Point结构体类型
    let point_layout = StructLayout {
        name: "Point".to_string(),
        fields: vec![
            StructField {
                name: "x".to_string(),
                offset: 0,
                size: 8,
                alignment: 8,
            },
            StructField {
                name: "y".to_string(),
                offset: 8,
                size: 8,
                alignment: 8,
            },
        ],
        total_size: 16,
        alignment: 8,
    };
    
    let struct_type_id = function.add_struct_type(point_layout);
    
    // 生成结构体操作指令
    let point_addr = function.new_register();
    let x_value = function.new_register();
    let y_value = function.new_register();
    let loaded_x = function.new_register();
    
    // 分配结构体
    function.add_instruction(Instruction::StructAlloc {
        dst: point_addr,
        struct_type: struct_type_id,
        allocation_type: AllocationType::Stack,
        span: Span::dummy(),
    });
    
    // 设置x和y值
    function.add_instruction(Instruction::Move {
        dst: x_value,
        src: Operand::Immediate { value: 10 },
        span: Span::dummy(),
    });
    
    function.add_instruction(Instruction::Move {
        dst: y_value,
        src: Operand::Immediate { value: 20 },
        span: Span::dummy(),
    });
    
    // 存储字段
    function.add_instruction(Instruction::StructFieldStore {
        struct_addr: point_addr,
        field_offset: 0, // x字段
        src: Operand::Register { id: x_value },
        span: Span::dummy(),
    });
    
    function.add_instruction(Instruction::StructFieldStore {
        struct_addr: point_addr,
        field_offset: 8, // y字段
        src: Operand::Register { id: y_value },
        span: Span::dummy(),
    });
    
    // 读取字段
    function.add_instruction(Instruction::StructFieldLoad {
        dst: loaded_x,
        struct_addr: point_addr,
        field_offset: 0, // x字段
        span: Span::dummy(),
    });
    
    // 返回
    function.add_instruction(Instruction::Return {
        value: Some(loaded_x),
        span: Span::dummy(),
    });
    
    println!("生成的LIR函数:");
    println!("{}", function);
}

/// 演示完整的结构体程序
fn demo_complete_struct_program() {
    println!("3. 完整结构体程序演示");
    println!("====================");
    
    let mut program = LirProgram::new();
    
    // 添加全局结构体类型
    let point_layout = StructLayout {
        name: "Point".to_string(),
        fields: vec![
            StructField {
                name: "x".to_string(),
                offset: 0,
                size: 8,
                alignment: 8,
            },
            StructField {
                name: "y".to_string(),
                offset: 8,
                size: 8,
                alignment: 8,
            },
        ],
        total_size: 16,
        alignment: 8,
    };
    
    program.add_global_struct_type("Point".to_string(), point_layout);
    
    // 创建主函数
    let mut main_function = LirFunction::new("main".to_string());
    
    // 分配两个Point结构体
    let point1 = main_function.new_register();
    let point2 = main_function.new_register();
    let temp = main_function.new_register();
    let result = main_function.new_register();
    
    // 分配内存
    main_function.add_instruction(Instruction::Alloc {
        dst: point1,
        size: 16,
        alignment: 8,
        allocation_type: AllocationType::Stack,
        span: Span::dummy(),
    });
    
    main_function.add_instruction(Instruction::Alloc {
        dst: point2,
        size: 16,
        alignment: 8,
        allocation_type: AllocationType::Stack,
        span: Span::dummy(),
    });
    
    // 初始化point1 (10, 20)
    main_function.add_instruction(Instruction::Store64 {
        addr: point1,
        offset: 0,
        src: Operand::Immediate { value: 10 },
        span: Span::dummy(),
    });
    
    main_function.add_instruction(Instruction::Store64 {
        addr: point1,
        offset: 8,
        src: Operand::Immediate { value: 20 },
        span: Span::dummy(),
    });
    
    // 初始化point2 (30, 40)
    main_function.add_instruction(Instruction::Store64 {
        addr: point2,
        offset: 0,
        src: Operand::Immediate { value: 30 },
        span: Span::dummy(),
    });
    
    main_function.add_instruction(Instruction::Store64 {
        addr: point2,
        offset: 8,
        src: Operand::Immediate { value: 40 },
        span: Span::dummy(),
    });
    
    // 计算距离平方: (x2-x1)^2 + (y2-y1)^2
    let x1 = main_function.new_register();
    let y1 = main_function.new_register();
    let x2 = main_function.new_register();
    let y2 = main_function.new_register();
    let dx = main_function.new_register();
    let dy = main_function.new_register();
    let dx2 = main_function.new_register();
    let dy2 = main_function.new_register();
    
    // 加载坐标
    main_function.add_instruction(Instruction::Load64 {
        dst: x1,
        addr: point1,
        offset: 0,
        span: Span::dummy(),
    });
    
    main_function.add_instruction(Instruction::Load64 {
        dst: y1,
        addr: point1,
        offset: 8,
        span: Span::dummy(),
    });
    
    main_function.add_instruction(Instruction::Load64 {
        dst: x2,
        addr: point2,
        offset: 0,
        span: Span::dummy(),
    });
    
    main_function.add_instruction(Instruction::Load64 {
        dst: y2,
        addr: point2,
        offset: 8,
        span: Span::dummy(),
    });
    
    // 计算差值
    main_function.add_instruction(Instruction::Sub {
        dst: dx,
        src1: Operand::Register { id: x2 },
        src2: Operand::Register { id: x1 },
        span: Span::dummy(),
    });
    
    main_function.add_instruction(Instruction::Sub {
        dst: dy,
        src1: Operand::Register { id: y2 },
        src2: Operand::Register { id: y1 },
        span: Span::dummy(),
    });
    
    // 计算平方
    main_function.add_instruction(Instruction::Mul {
        dst: dx2,
        src1: Operand::Register { id: dx },
        src2: Operand::Register { id: dx },
        span: Span::dummy(),
    });
    
    main_function.add_instruction(Instruction::Mul {
        dst: dy2,
        src1: Operand::Register { id: dy },
        src2: Operand::Register { id: dy },
        span: Span::dummy(),
    });
    
    // 计算总和
    main_function.add_instruction(Instruction::Add {
        dst: result,
        src1: Operand::Register { id: dx2 },
        src2: Operand::Register { id: dy2 },
        span: Span::dummy(),
    });
    
    // 返回结果
    main_function.add_instruction(Instruction::Return {
        value: Some(result),
        span: Span::dummy(),
    });
    
    program.add_function(main_function);
    program.set_main("main".to_string());
    
    println!("完整的LIR程序:");
    println!("{}", program);
    
    println!("\n=== 演示总结 ===");
    println!("改进后的LIR系统提供了以下功能:");
    println!("✓ 专业的结构体内存布局管理");
    println!("✓ 结构体专用的LIR指令集");
    println!("✓ 自动内存对齐和布局优化");
    println!("✓ 栈和堆分配支持");
    println!("✓ 结构化的内存访问");
    println!("✓ 类型安全的字段操作");
    println!("✓ 详细的调试信息显示");
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_struct_layout_calculation() {
        let mut manager = StructLayoutManager::new();
        
        use karte_hir::types::{Type, StructField as HirStructField};
        let fields = vec![
            HirStructField {
                name: "x".to_string(),
                field_type: Type::Number,
            },
            HirStructField {
                name: "y".to_string(),
                field_type: Type::Number,
            },
        ];
        
        let layout = manager.compute_layout("Point", &fields).unwrap();
        
        assert_eq!(layout.total_size, 16);
        assert_eq!(layout.alignment, 8);
        assert_eq!(layout.fields.len(), 2);
        assert_eq!(layout.fields[0].offset, 0);
        assert_eq!(layout.fields[1].offset, 8);
    }
    
    #[test]
    fn test_struct_instruction_generation() {
        let mut function = LirFunction::new("test".to_string());
        
        let layout = StructLayout {
            name: "Point".to_string(),
            fields: vec![
                StructField {
                    name: "x".to_string(),
                    offset: 0,
                    size: 8,
                    alignment: 8,
                },
            ],
            total_size: 8,
            alignment: 8,
        };
        
        let struct_type_id = function.add_struct_type(layout);
        let addr = function.new_register();
        
        function.add_instruction(Instruction::StructAlloc {
            dst: addr,
            struct_type: struct_type_id,
            allocation_type: AllocationType::Stack,
            span: Span::dummy(),
        });
        
        assert_eq!(function.instructions.len(), 1);
        assert!(matches!(function.instructions[0], Instruction::StructAlloc { .. }));
    }
} 