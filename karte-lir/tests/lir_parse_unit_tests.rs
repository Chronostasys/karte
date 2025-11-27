#[cfg(test)]
mod parse_unit_tests {
    use karte_diagnostics::Span;
    use karte_ir_codec::{IrDisplay, IrParse};
    use karte_lir::{Instruction, LabelId, LirFunction, LirProgram, Operand, Register};

    #[test]
    fn test_labelid_roundtrip() {
        let id = LabelId(1);
        let s = id.to_ir_string();
        println!("LabelId text: {}", s);
        let parsed = LabelId::parse_ir(&s).expect("parse LabelId");
        assert_eq!(parsed, id);
    }

    #[test]
    fn test_operand_immediate_roundtrip() {
        let op = Operand::Immediate { value: 42 };
        let s = op.to_ir_string();
        println!("Operand text: {}", s);
        let parsed = Operand::parse_ir(&s).expect("parse operand");
        assert_eq!(parsed, op);
    }

    #[test]
    fn test_instruction_move_roundtrip() {
        let inst = Instruction::Move {
            dst: Register::Virtual(1),
            src: Operand::Immediate { value: 5 },
            span: Span::dummy(),
        };

        let s = inst.to_ir_string();
        println!("Instruction text: {}", s);
        let parsed = Instruction::parse_ir(&s).expect("parse instruction");
        assert_eq!(parsed, inst);
    }

    #[test]
    fn test_instruction_jump_parse() {
        let inst = Instruction::Jump {
            target: LabelId(1),
            span: Span::dummy(),
        };
        println!("Jump text: {}", inst.to_ir_string());
        let text = "Jump(L1)";
        let parsed = Instruction::parse_ir(text).expect("parse jump");
        assert!(matches!(
            parsed,
            Instruction::Jump {
                target: LabelId(1),
                ..
            }
        ));
    }

    #[test]
    fn test_lirfunction_roundtrip() {
        let mut f = LirFunction::new("t".to_string());
        f.add_instruction(Instruction::Label {
            id: LabelId(1),
            span: Span::dummy(),
        });
        f.add_instruction(Instruction::Move {
            dst: Register::Virtual(1),
            src: Operand::Immediate { value: 7 },
            span: Span::dummy(),
        });
        f.add_instruction(Instruction::Return {
            value: Some(Register::Virtual(1)),
            span: Span::dummy(),
        });

        let s = f.to_ir_string();
        println!("LirFunction text:\n{}", s);
        let parsed = LirFunction::parse_ir(&s).expect("parse function");
        assert_eq!(parsed.name, f.name);
        assert_eq!(parsed.instructions.len(), f.instructions.len());
    }

    #[test]
    fn test_manual_instruction_parse_steps() {
        use karte_ir_codec::parse;

        let s = "mov dst: #v1, src: # value: 5";
        println!("Manual parse input: {}", s);

        // Step 1: consume the token
        let (rest, _) = parse::keyword("mov")(s).expect("keyword mov");
        println!("After keyword rest='{}'", rest);

        // Step 2: parse dst field using body_field helper
        let (rest, _dst) =
            parse::body_field("dst", karte_lir::Register::parse_nom)(rest).expect("parse dst");
        println!("Parsed dst, rest='{}'", rest);

        // Step 3: parse src field
        let (_rest, src) =
            parse::body_field("src", karte_lir::Operand::parse_nom)(rest).expect("parse src");
        println!("Parsed src: {:?}", src);
    }

    #[test]
    fn test_parse_program_with_namespaced_functions() {
        let lir_text = r#"
functions: {
    main::main: 
        LirFunction
        name: main::main
        body:
        [
        
            Label(L1),
        
            Jump(L1),
        
            Return(#p0)
        ]
        params: 0,

    utils::__script_entry__: 
        LirFunction
        name: utils::__script_entry__
        body:
        [
        
            Label(L2),
        
            Return(#p0)
        ]
        params: 0,

    utils.sub::multiply: 
        LirFunction
        name: utils.sub::multiply
        body:
        [
        
            Label(L3),
        
            mul dst: #p2, src1: #p3, src2: #p4,
        
            Return(#p2)
        ]
        params: 2
    }, main_function: none, global_struct_types: {}, global_variables: {}
"#;

        let parsed = LirProgram::parse_ir(lir_text).expect("parse namespaced program");
        assert!(parsed.functions.contains_key("main::main"));
        assert!(parsed.functions.contains_key("utils::__script_entry__"));
        assert!(parsed.functions.contains_key("utils.sub::multiply"));
    }
}
