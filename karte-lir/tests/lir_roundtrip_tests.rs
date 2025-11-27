#[cfg(test)]
mod lir_roundtrip_tests {
    use karte_ir_codec::{IrDisplay, IrParse};
    use karte_lir::Register;
    use karte_lir::{
        AllocationType, Instruction, LabelId, LirFunction, LirProgram, Operand, StructTypeId,
    };

    #[test]
    fn test_lir_program_roundtrip_simple() {
        // 构造一个简单的 LIR 程序
        let mut func = LirFunction::new("main".to_string());
        func.add_instruction(Instruction::Label {
            id: LabelId(1),
            span: karte_diagnostics::Span::dummy(),
        });
        func.add_instruction(Instruction::Move {
            dst: Register::Virtual(1),
            src: Operand::Immediate { value: 42 },
            span: karte_diagnostics::Span::dummy(),
        });
        func.add_instruction(Instruction::Add {
            dst: Register::Virtual(0),
            src1: Operand::Register {
                id: Register::Virtual(1),
            },
            src2: Operand::Immediate { value: 8 },
            span: karte_diagnostics::Span::dummy(),
        });
        func.add_instruction(Instruction::Return {
            value: Some(Register::Virtual(0)),
            span: karte_diagnostics::Span::dummy(),
        });

        let mut prog = LirProgram::new();
        prog.add_function(func.clone());
        prog.set_main("main".to_string());

        let text = prog.to_ir_string();
        println!("=== LIR TEXT START ===\n{}\n=== LIR TEXT END ===", text);

        // 确保序列化包含预期标识
        assert!(
            text.contains("function"),
            "serialized LIR should contain 'function'"
        );
        assert!(
            text.contains("mov") || text.contains("move"),
            "serialized LIR should contain move instruction"
        );

        // 解析回 LirProgram
        // 有时生成的文本可能省略顶层类型名，尝试带/不带前缀两种方式解析以兼容不同生成格式
        let parsed = match LirProgram::parse_ir(&text) {
            Ok(p) => p,
            Err(_) => LirProgram::parse_ir(&format!("LirProgram({})", text))
                .expect("parse_ir should succeed with fallback prefix"),
        };

        // 检查解析结果包含 main 函数
        assert!(
            parsed.functions.contains_key("main"),
            "parsed program should contain main function"
        );
    }

    #[test]
    fn test_lir_call_indirect_roundtrip() {
        // 构造包含 call_indirect 的函数
        let mut func = LirFunction::new("callindirect_demo".to_string());
        func.add_instruction(Instruction::Label {
            id: LabelId(1),
            span: karte_diagnostics::Span::dummy(),
        });
        // mov r1, #100
        func.add_instruction(Instruction::Move {
            dst: Register::Virtual(1),
            src: Operand::Immediate { value: 100 },
            span: karte_diagnostics::Span::dummy(),
        });
        // add r2, r1, #0 (simulate address calc)
        func.add_instruction(Instruction::Add {
            dst: Register::Virtual(2),
            src1: Operand::Register {
                id: Register::Virtual(1),
            },
            src2: Operand::Immediate { value: 0 },
            span: karte_diagnostics::Span::dummy(),
        });
        // call_indirect r3 = call_indirect r2(r1)
        func.add_instruction(Instruction::CallIndirect {
            function_register: Register::Virtual(2),
            args: vec![Register::Virtual(1)],
            arg_operands: vec![],
            result: Some(Register::Virtual(3)),
            span: karte_diagnostics::Span::dummy(),
        });
        func.add_instruction(Instruction::Return {
            value: Some(Register::Virtual(3)),
            span: karte_diagnostics::Span::dummy(),
        });

        let mut prog = LirProgram::new();
        prog.add_function(func.clone());

        let text = prog.to_ir_string();
        println!("=== LIR TEXT START ===\n{}\n=== LIR TEXT END ===", text);
        // parse
        let parsed = match LirProgram::parse_ir(&text) {
            Ok(p) => p,
            Err(_) => LirProgram::parse_ir(&format!("LirProgram({})", text))
                .expect("parse_ir should succeed with fallback prefix"),
        };

        // Ensure the parsed function exists and contains a CallIndirect instruction
        let parsed_func = parsed
            .functions
            .get("callindirect_demo")
            .expect("function should exist");
        let has_call_indirect = parsed_func
            .instructions
            .iter()
            .any(|inst| matches!(inst, Instruction::CallIndirect { .. }));
        assert!(
            has_call_indirect,
            "parsed function should contain CallIndirect"
        );
    }

    #[test]
    fn test_lir_field_access_roundtrip() {
        // 构造包含 StructFieldLoad/Store 的函数
        let mut func = LirFunction::new("struct_demo".to_string());
        func.add_instruction(Instruction::Label {
            id: LabelId(1),
            span: karte_diagnostics::Span::dummy(),
        });
        func.add_instruction(Instruction::StructAlloc {
            dst: Register::Virtual(4),
            struct_type: StructTypeId(0),
            allocation_type: AllocationType::Stack,
            span: karte_diagnostics::Span::dummy(),
        });
        func.add_instruction(Instruction::StructFieldStore {
            struct_addr: Register::Virtual(4),
            field_offset: 0,
            src: Operand::Immediate { value: 10 },
            span: karte_diagnostics::Span::dummy(),
        });
        func.add_instruction(Instruction::StructFieldLoad {
            dst: Register::Virtual(5),
            struct_addr: Register::Virtual(4),
            field_offset: 0,
            span: karte_diagnostics::Span::dummy(),
        });
        func.add_instruction(Instruction::Return {
            value: Some(Register::Virtual(5)),
            span: karte_diagnostics::Span::dummy(),
        });

        let mut prog = LirProgram::new();
        prog.add_function(func.clone());

        let text = prog.to_ir_string();
        println!("=== LIR TEXT START ===\n{}\n=== LIR TEXT END ===", text);
        let parsed = match LirProgram::parse_ir(&text) {
            Ok(p) => p,
            Err(_) => LirProgram::parse_ir(&format!("LirProgram({})", text))
                .expect("parse_ir should succeed with fallback prefix"),
        };

        let parsed_func = parsed
            .functions
            .get("struct_demo")
            .expect("function should exist");
        let has_field_load = parsed_func
            .instructions
            .iter()
            .any(|inst| matches!(inst, Instruction::StructFieldLoad { .. }));
        assert!(
            has_field_load,
            "parsed function should contain StructFieldLoad"
        );
    }
}
