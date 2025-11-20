use karte_ir_codec::IrParse;
use karte_mir::Statement;

#[test]
fn test_parse_call_labelled() {
    let s = "call function: %4, args: [%5, %3]";
    let res = Statement::parse_ir(s);
    assert!(res.is_ok(), "call labelled parse failed: {:?}", res);
}
