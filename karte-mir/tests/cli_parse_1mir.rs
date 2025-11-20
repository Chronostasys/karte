use karte_ir_codec::IrParse;
use karte_mir::MirProgram;

#[test]
fn test_parse_1_mir_cli_output() {
    // Snapshot of current 1.mir (produced by CLI). Ensure the parser accepts the labelled display form.
    let input = r#"
functions: 
    {
        main: 
            MirFunction
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
                        },
    
        lambda$0: 
            MirFunction
                name: 
                    lambda$0
                params: 
                    [
                        
                            __env,
                        
                            x
                        ]
                blocks: 
                    {
                        bb0: 
                            BasicBlock
                                id: 
                                    bb0
                                statements: 
                                    [
                                        
                                            %1 = var name: x,
                                        
                                            %2 = num value: 2,
                                        
                                            %0 = %1 * %2
                                        ]
                                terminator: 
                                    ret value: %0
                        }
        }
main_function: 
    main
main_return_value: 
    %0
temp_values: 
    {}
struct_types: 
    {}
"#;

    let res = MirProgram::parse_ir(input);
    assert!(res.is_ok(), "Parse failed: {:?}", res);
}
