use karte_diagnostics::Span;
use karte_ir_codec::{parse::IrParse, IrDisplay};
use karte_mir::ir::{
    BasicBlock, BasicBlockId, BinaryOperator, MirFunction, Statement, TempId, Terminator, Value,
};
use std::collections::BTreeMap;

#[test]
fn parse_sample_mir_function() {
    let func = build_sample_function();
    let input = func.to_ir_string();

    let parsed = MirFunction::parse_ir(&input);
    assert!(parsed.is_ok(), "Failed to parse MirFunction: {:?}", parsed);
}

#[test]
fn parse_sample_basic_block_map() {
    let func = build_sample_function();
    let input = func.basic_blocks.to_ir_string();

    let parsed = BTreeMap::<BasicBlockId, BasicBlock>::parse_ir(&input);
    assert!(parsed.is_ok(), "Failed to parse blocks: {:?}", parsed);
}

fn build_sample_function() -> MirFunction {
    let mut func = MirFunction::new("main".to_string(), vec![]);
    let entry = BasicBlockId(0);
    let block = func
        .basic_blocks
        .get_mut(&entry)
        .expect("entry block should exist");

    block.statements.push(Statement::Assign {
        target: Value::Temp {
            id: TempId(1),
            ty: None,
        },
        source: Value::Number { value: 2, ty: None },
        span: Span::default(),
    });
    block.statements.push(Statement::Assign {
        target: Value::Temp {
            id: TempId(2),
            ty: None,
        },
        source: Value::Temp {
            id: TempId(1),
            ty: None,
        },
        span: Span::default(),
    });
    block.statements.push(Statement::Assign {
        target: Value::Temp {
            id: TempId(3),
            ty: None,
        },
        source: Value::Number { value: 3, ty: None },
        span: Span::default(),
    });
    block.statements.push(Statement::BinaryOp {
        target: Value::Temp {
            id: TempId(0),
            ty: None,
        },
        left: Value::Temp {
            id: TempId(2),
            ty: None,
        },
        op: BinaryOperator::Multiply,
        right: Value::Temp {
            id: TempId(3),
            ty: None,
        },
        span: Span::default(),
    });
    block.terminator = Some(Terminator::Return {
        value: Some(Value::Temp {
            id: TempId(0),
            ty: None,
        }),
        span: Span::default(),
    });

    func
}
