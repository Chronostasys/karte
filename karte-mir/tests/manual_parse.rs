use karte_ir_codec::parse::IrParse;
use karte_mir::ir::MirFunction;

#[test]
fn parse_sample_mir_function() {
    let input = r#"MirFunction
                name: main
                params: []
                blocks: {
                        bb0: BasicBlock
                                id: bb0
                                statements: [
                                        %1 = num 2,
                                        %2 = %1,
                                        %3 = num 3,
                                        %0 = %2 * %3
                                        ]
                                terminator: ret %0
                        }
        }"#;

    let parsed = MirFunction::parse_ir(input);
    assert!(parsed.is_ok(), "Failed to parse: {:?}", parsed);
}

#[test]
fn parse_sample_basic_block_map() {
    use karte_mir::ir::{BasicBlock, BasicBlockId};
    use std::collections::BTreeMap;

    let input = r#"{
                bb0: BasicBlock
                                id: bb0
                                statements: [
                                        %1 = num 2,
                                        %2 = %1,
                                        %3 = num 3,
                                        %0 = %2 * %3
                                ]
                                terminator: ret %0
        }"#;

    let parsed = BTreeMap::<BasicBlockId, BasicBlock>::parse_ir(input);
    assert!(parsed.is_ok(), "Failed to parse blocks: {:?}", parsed);
}
