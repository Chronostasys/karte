use karte_mir::MirFunction;
use karte_ir_codec::IrParse;

#[test]
fn debug_parse_main_mir_function() {
    let input = r#"MirFunction
                name: 
                    main
                params: 
                    []
                blocks: 
                    {
                        bb0: 
                            BasicBlock
                                id: 
                                    bb0
                                statements: 
                                    [
                                        
                                            %1 = Struct { name = 
                                                Closure, fields = 
                                                {
                                                    env_ptr: 
                                                        num value: 0,
                                                    function_ptr: 
                                                        fn name: lambda$0
                                                    } },
                                        
                                            %2 = %1,
                                        
                                            %3 = num value: 5,
                                        
                                            %4 = %2.function_ptr,
                                        
                                            %5 = %2.env_ptr,
                                        
                                            call function: %4, args: [%5, %3]
                                        ]
                                terminator: 
                                    ret value: %0
                        }
        }"#;

    let res = MirFunction::parse_ir(input);
    assert!(res.is_ok(), "MirFunction parse failed: {:?}", res);
}
