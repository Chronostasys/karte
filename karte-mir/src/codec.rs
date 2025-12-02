/// 使用新的 IR 编解码系统
///
/// 这个模块演示了如何使用 karte-ir-codec 和 karte-ir-derive
/// 自动生成 IR 类型的 Display 和 Parse 实现
use karte_ir_derive::IrCodec;

/// 基本块标识符 - 使用新的编解码系统
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, IrCodec)]
pub struct BasicBlockId(pub usize);

/// 临时变量标识符 - 使用新的编解码系统
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, IrCodec, Default)]
pub struct TempId(pub usize);

/// 二元运算符 - 使用新的编解码系统
#[derive(Debug, Clone, PartialEq, IrCodec)]
pub enum BinaryOperator {
    #[ir_codec(token = "+")]
    Add,
    Subtract,
    Multiply,
    Divide,
    Equal,
    NotEqual,
    LessThan,
    LessEqual,
    GreaterThan,
    GreaterEqual,
    And,
    Or,
}

/// 一元运算符 - 使用新的编解码系统
#[derive(Debug, Clone, PartialEq, IrCodec)]
pub enum UnaryOperator {
    Plus,
    Minus,
    Not,
}

/// MIR值 - 使用新的编解码系统
///
/// 注意：由于包含了 BTreeMap 等复杂类型，我们需要为这些类型也实现 IrCodec
/// 或者使用 #[ir_codec(skip)] 跳过某些字段
#[derive(Debug, Clone, PartialEq, IrCodec, Default)]
pub enum Value {
    /// 变量引用
    Variable { name: String },
    /// 数字常量
    Number { value: i64 },
    /// 布尔常量
    Boolean { value: bool },
    /// 单元值
    #[default]
    Unit,
    /// 临时变量
    Temp { id: TempId },
    /// 构造器值
    Constructor {
        name: String,
        arg: Option<Box<Value>>,
    },
    /// 限定构造器值
    QualifiedConstructor {
        type_name: String,
        constructor_name: String,
        arg: Option<Box<Value>>,
    },
    /// 函数值
    Function { name: String },
    /// 闭包值
    Closure {
        function_name: String,
        captured_values: Vec<Value>,
    },
    /// 引用值
    Reference { value: Box<Value> },
}

#[cfg(test)]
mod tests {
    use super::*;
    use karte_ir_codec::{IrDisplay, IrParse};

    #[test]
    fn test_basic_block_id() {
        let id = BasicBlockId(42);
        let formatted = id.to_ir_string();
        assert_eq!(formatted, "BasicBlockId(42)");

        // 测试解析
        let parsed = BasicBlockId::parse_ir(&formatted).unwrap();
        assert_eq!(parsed, id);
    }

    #[test]
    fn test_field_access_parsing() {
        use crate::Statement;
        use crate::TempId;
        use crate::Value;
        use karte_ir_codec::IrParse;

        let input = "%4 = %2.function_ptr";
        println!("Testing FieldAccess parsing for input: {}", input);

        match Statement::parse_nom(input) {
            Ok((rest, stmt)) => {
                assert_eq!(rest, "");
                let expected = Statement::FieldAccess {
                    target: Value::Temp { id: TempId(4), ty: None },
                    object: Value::Temp { id: TempId(2), ty: None },
                    field: "function_ptr".to_string(),
                    span: Default::default(),
                };
                assert_eq!(stmt, expected);
            }
            Err(e) => panic!("FieldAccess parse failed: {:?}", e),
        }
    }

    #[test]
    fn test_call_inline_parsing() {
        use crate::Statement;
        use crate::TempId;
        use crate::Value;
        use karte_ir_codec::IrParse;

        // The current codec emits call statements as:
        // `call function: %4, args: [%5, %3]` when there is no target.
        let input = "call function: %4, args: [%5, %3]";
        println!("Testing Call parsing for input: {}", input);

        match Statement::parse_nom(input) {
            Ok((rest, stmt)) => {
                assert_eq!(rest, "");
                let expected = Statement::Call {
                    target: None,
                    function: Value::Temp { id: TempId(4), ty: None },
                    args: vec![Value::Temp { id: TempId(5), ty: None }, Value::Temp { id: TempId(3), ty: None }],
                    span: Default::default(),
                };
                assert_eq!(stmt, expected, "Parsed Call did not match expected");
            }
            Err(e) => panic!("Call parse failed: {:?}", e),
        }
    }

    #[test]
    fn test_temp_id() {
        let id = TempId(10);
        let formatted = id.to_ir_string();
        assert_eq!(formatted, "TempId(10)");

        let parsed = TempId::parse_ir(&formatted).unwrap();
        assert_eq!(parsed, id);
    }

    #[test]
    fn test_binary_operator() {
        let op = BinaryOperator::Add;
        let formatted = op.to_ir_string();
        assert_eq!(formatted, "+");

        let parsed = BinaryOperator::parse_ir(&formatted).unwrap();
        assert_eq!(parsed, op);
    }

    #[test]
    fn test_value_variable() {
        let value = Value::Variable {
            name: "x".to_string(),
        };
        let formatted = value.to_ir_string();
        assert_eq!(formatted, "Variable(x)");

        let parsed = Value::parse_ir(&formatted).unwrap();
        assert_eq!(parsed, value);
    }

    #[test]
    fn test_value_number() {
        let value = Value::Number { value: 42 };
        let formatted = value.to_ir_string();
        assert_eq!(formatted, "Number(42)");

        let parsed = Value::parse_ir(&formatted).unwrap();
        assert_eq!(parsed, value);
    }

    #[test]
    fn test_value_constructor() {
        let value = Value::Constructor {
            name: "Some".to_string(),
            arg: Some(Box::new(Value::Number { value: 42 })),
        };
        let formatted = value.to_ir_string();
        // 格式可能是: Constructor { name = Some, arg = Number { value = 42 } }

        let parsed = Value::parse_ir(&formatted).unwrap();
        assert_eq!(parsed, value);
    }

    #[test]
    fn test_mir_function_simple() {
        use crate::ir::MirFunction;
        use karte_ir_codec::IrParse;

        let input = "MirFunction\nname: test\nparams: []\nblocks: {}";
        println!("Testing MirFunction parsing with input: {:?}", input);

        match MirFunction::parse_nom(input) {
            Ok((rest, func)) => {
                println!("✓ Parsed MirFunction: name={}", func.name);
                println!("  Remaining input: {:?}", rest);
            }
            Err(e) => {
                println!("✗ Failed to parse MirFunction: {:?}", e);
                panic!("MirFunction parsing failed");
            }
        }
    }

    #[test]
    fn test_mir_function_with_indentation() {
        use crate::ir::MirFunction;
        use karte_ir_codec::IrParse;

        // 模拟实际的格式：所有字段都有缩进
        let input = "MirFunction\n                name: \n                    test\n                params: \n                    []\n                blocks: \n                    {}";
        println!("Testing MirFunction parsing with indented input");
        println!("Input: {:?}", input);

        match MirFunction::parse_nom(input) {
            Ok((rest, func)) => {
                println!("✓ Parsed MirFunction: name={}", func.name);
                println!("  Remaining input: {:?}", rest);
            }
            Err(e) => {
                println!("✗ Failed to parse MirFunction: {:?}", e);
                panic!("MirFunction parsing with indentation failed");
            }
        }
    }

    #[test]
    fn test_basic_block_id_with_whitespace() {
        use crate::ir::BasicBlockId;
        use karte_ir_codec::IrParse;

        // 测试前导空白
        let inputs = vec![
            ("bb0", 0),
            ("  bb0", 0),
            ("\n                        bb0", 0), // 模拟实际格式
            ("bb42", 42),
        ];

        for (input, expected) in inputs {
            println!("Testing BasicBlockId with input: {:?}", input);
            match BasicBlockId::parse_nom(input) {
                Ok((rest, id)) => {
                    println!("  ✓ Parsed: bb{}, rest: {:?}", id.0, rest);
                    assert_eq!(id.0, expected);
                }
                Err(e) => {
                    println!("  ✗ Failed: {:?}", e);
                    panic!("BasicBlockId parsing failed for input: {:?}", input);
                }
            }
        }
    }

    #[test]
    fn test_terminator_direct() {
        use crate::ir::Terminator;
        use karte_ir_codec::IrParse;

        // 直接测试 Terminator 的解析
        println!("Testing Terminator::parse_nom with 'ret value: %0'");
        match Terminator::parse_nom("ret value: %0") {
            Ok((rest, value)) => {
                println!("  ✓ Parsed: {:?}, rest: {:?}", value, rest);
            }
            Err(e) => {
                println!("  ✗ Failed: {:?}", e);
            }
        }
    }

    #[test]
    fn test_keyword_function() {
        use karte_ir_codec::parse::keyword;

        // Test 1: matching keyword
        let input = "ret value: %0";
        println!("Test 1: keyword('ret') with input: '{}'", input);
        match keyword("ret")(input) {
            Ok((rest, matched)) => {
                println!("  ✓ keyword matched: '{}', rest: '{}'", matched, rest);
            }
            Err(e) => {
                println!("  ✗ keyword failed: {:?}", e);
            }
        }

        // Test 2: non-matching keyword
        let input2 = "ret value: %0";
        println!("Test 2: keyword('if') with input: '{}'", input2);
        match keyword("if")(input2) {
            Ok((rest, matched)) => {
                println!("  ✓ keyword matched: '{}', rest: '{}'", matched, rest);
            }
            Err(e) => {
                println!("  ✗ keyword failed: {:?}", e);
                println!(
                    "      Error type: {}",
                    match e {
                        nom::Err::Error(_) => "Error (alt will try next)",
                        nom::Err::Failure(_) => "Failure (alt will stop)",
                        nom::Err::Incomplete(_) => "Incomplete",
                    }
                );
            }
        }
    }

    #[test]
    fn test_option_terminator_direct() {
        use crate::ir::Terminator;
        use karte_ir_codec::IrParse;

        // 直接测试 Option<Terminator> 的解析
        let inputs = vec!["none", "ret value: %0"];

        for input in inputs {
            println!("Testing Option<Terminator> with input: {:?}", input);
            match <Option<Terminator>>::parse_nom(input) {
                Ok((rest, value)) => {
                    println!("  ✓ Parsed: {:?}, rest: {:?}", value, rest);
                }
                Err(e) => {
                    println!("  ✗ Failed: {:?}", e);
                }
            }
        }
    }

    #[test]
    fn test_separated_pair_basic() {
        use crate::ir::{BasicBlock, BasicBlockId};
        use karte_ir_codec::IrParse;
        use nom::character::complete::{char as nom_char, multispace0};
        use nom::sequence::delimited as nom_delimited;
        use nom::sequence::separated_pair;

        // 测试 separated_pair 是否能解析 "bb0: BasicBlock..."
        // updated format uses "ret value: %0" for terminator and labels like "id: bb0"
        let input = "bb0: BasicBlock\n                                id: bb0\n                                statements: []\n                                terminator: ret value: %0";

        println!(
            "Testing separated_pair with input: {:?}",
            &input.chars().take(50).collect::<String>()
        );

        let result = separated_pair(
            BasicBlockId::parse_nom,
            nom_delimited(multispace0, nom_char(':'), multispace0),
            BasicBlock::parse_nom,
        )(input);

        match result {
            Ok((rest, (key, _value))) => {
                println!("✓ Parsed pair: bb{} -> BasicBlock", key.0);
                println!(
                    "  Remaining: {:?}",
                    &rest.chars().take(50).collect::<String>()
                );
                assert_eq!(key.0, 0, "expected BasicBlockId 0");
            }
            Err(e) => panic!("separated_pair parsing failed: {:?}", e),
        }
    }

    #[test]
    fn test_btreemap_with_indentation() {
        use crate::ir::{BasicBlock, BasicBlockId};
        use karte_ir_codec::IrParse;
        use std::collections::BTreeMap;

        // 简化的 BTreeMap 测试，模拟实际格式
        // match current IR style (terminator uses "ret value: %0")
        let input = "{\n                        bb0: BasicBlock\n                                id: bb0\n                                statements: []\n                                terminator: ret value: %0\n                        }";

        println!("Testing BTreeMap parsing with indentation");
        println!("Input: {:?}", input);

        match <BTreeMap<BasicBlockId, BasicBlock>>::parse_nom(input) {
            Ok((rest, map)) => {
                println!("✓ Parsed BTreeMap with {} entries", map.len());
                println!("  Remaining input: {:?}", rest);
                assert!(map.len() >= 1, "expected at least one BasicBlock entry");
            }
            Err(e) => panic!("Failed to parse BTreeMap: {:?}", e),
        }
    }

    #[test]
    fn test_mir_function_real_format() {
        use crate::ir::MirFunction;
        use karte_ir_codec::IrParse;

        // Use a representative, current-format MirFunction fixture and assert parse succeeds
        let input = r#"MirFunction
                name: main
                params: []
                blocks: {
                        bb0: BasicBlock
                                id: bb0
                                statements: [
                                        %1 = num value: 2,
                                        %2 = %1,
                                        %3 = num value: 3,
                                        %0 = %2 * %3
                                    ]
                                terminator: ret value: %0
                        }"#;

        match MirFunction::parse_nom(input) {
            Ok((_rest, func)) => {
                println!("✓ Parsed MirFunction: name={}", func.name);
                println!("  BasicBlocks count: {}", func.basic_blocks.len());
                assert_eq!(func.name, "main");
                assert!(func.basic_blocks.len() >= 1);
            }
            Err(e) => panic!("MirFunction parsing failed: {:?}", e),
        }
    }

    #[test]
    fn test_basicblockid_parsing() {
        use crate::ir::BasicBlockId;
        use karte_ir_codec::IrParse;

        // Test BasicBlockId parsing
        let inputs = vec![
            "bb0",
            "bb1",
            "\n                        bb0", // With leading newline
            "bb0: something",                // With colon after
        ];

        for input in inputs {
            println!("Testing BasicBlockId with: {:?}", input);
            match BasicBlockId::parse_nom(input) {
                Ok((rest, id)) => {
                    println!("  ✓ Parsed: bb{}, rest: {:?}", id.0, rest);
                }
                Err(e) => {
                    println!("  ✗ Failed: {:?}", e);
                }
            }
        }
    }

    #[test]
    fn test_vec_statement_parsing() {
        use crate::ir::Statement;
        use karte_ir_codec::IrParse;

        println!("\n=== Test: Vec<Statement> ===");
        // numbers are emitted as `num value: N` in the current IR
        let input = "[\n                                        \n                                            %1 = num value: 2,\n                                        \n                                            %2 = %1,\n                                        \n                                            %3 = num value: 3,\n                                        \n                                            %0 = %2 * %3\n                                        ]";

        println!("Input length: {}", input.len());
        println!("Input: {:?}", &input.chars().take(150).collect::<String>());

        match <Vec<Statement>>::parse_nom(input) {
            Ok((rest, stmts)) => {
                println!("✓ Parsed {} statements", stmts.len());
                println!("  Remaining: {:?}", rest);
                assert_eq!(stmts.len(), 4, "expected 4 statements in the vector");
                assert_eq!(
                    rest, "",
                    "expected no remaining input after parsing statements"
                );
            }
            Err(e) => panic!("Vec<Statement> parsing failed: {:?}", e),
        }
    }

    #[test]
    fn test_basicblock_parsing() {
        use crate::ir::BasicBlock;
        use karte_ir_codec::IrParse;

        println!("\n=== Test 1: BasicBlock with empty statements ===");
        let input1 = "BasicBlock\n                                id: \n                                    bb0\n                                statements: \n                                    []\n                                terminator: \n                                    ret value: %0";

        println!("Input: {:?}", &input1.chars().take(100).collect::<String>());

        match BasicBlock::parse_nom(input1) {
            Ok((rest, block)) => {
                println!("✓ Parsed BasicBlock: id=bb{}", block.id.0);
                println!("  Statements: {}", block.statements.len());
                println!("  Remaining: {:?}", rest);
                assert_eq!(
                    block.statements.len(),
                    0,
                    "expected no statements in input1"
                );
            }
            Err(e) => panic!("Parsing BasicBlock failed: {:?}", e),
        }

        println!("\n=== Test 2: BasicBlock with statements ===");
        let input2 = "BasicBlock\n                                id: \n                                    bb0\n                                statements: \n                                    [\n                                        \n                                            %1 = num value: 2,\n                                        \n                                            %2 = %1,\n                                        \n                                            %3 = num value: 3,\n                                        \n                                            %0 = %2 * %3\n                                        ]\n                                terminator: \n                                    ret value: %0";

        println!("Input length: {}", input2.len());

        match BasicBlock::parse_nom(input2) {
            Ok((rest, block)) => {
                println!("✓ Parsed BasicBlock: id=bb{}", block.id.0);
                println!("  Statements: {}", block.statements.len());
                println!("  Remaining: {:?}", rest);
                assert_eq!(block.statements.len(), 4, "expected 4 statements in input2");
            }
            Err(e) => panic!("Parsing BasicBlock failed: {:?}", e),
        }
    }

    #[test]
    fn test_mir_function_direct() {
        use crate::ir::MirFunction;
        use karte_ir_codec::IrParse;

        // Test 1: Empty blocks
        println!("\n=== Test 1: MirFunction with empty blocks ===");
        let input1 = "MirFunction\n                name: \n                    main\n                params: \n                    []\n                blocks: \n                    {}";

        println!("Input: {:?}", &input1.chars().take(100).collect::<String>());

        match MirFunction::parse_nom(input1) {
            Ok((rest, func)) => {
                println!("✓ Parsed MirFunction: name={}", func.name);
                println!("  Remaining: {:?}", rest);
            }
            Err(e) => {
                println!("✗ Failed: {:?}", e);
            }
        }

        // Test 2: With BasicBlock
        println!("\n=== Test 2: MirFunction with BasicBlock ===");
        let input2 = "MirFunction\n                name: \n                    main\n                params: \n                    []\n                blocks: \n                    {\n                        bb0: \n                            BasicBlock\n                                id: \n                                    bb0\n                                statements: \n                                    [\n                                        \n                                            %1 = num value: 2,\n                                        \n                                            %2 = %1,\n                                        \n                                            %3 = num value: 3,\n                                        \n                                            %0 = %2 * %3\n                                        ]\n                                terminator: \n                                    ret value: %0\n                        }";

        println!("Input length: {}", input2.len());
        println!(
            "Input (first 100 chars): {:?}",
            &input2.chars().take(100).collect::<String>()
        );

        match MirFunction::parse_nom(input2) {
            Ok((rest, func)) => {
                println!("✓ Parsed MirFunction: name={}", func.name);
                println!("  BasicBlocks: {}", func.basic_blocks.len());
                println!(
                    "  Remaining: {:?}",
                    &rest.chars().take(50).collect::<String>()
                );
                assert!(
                    func.basic_blocks.len() >= 1,
                    "expected at least one basic block"
                );
            }
            Err(e) => panic!("MirFunction parsing failed: {:?}", e),
        }
    }

    #[test]
    fn test_hashmap_string_mirfunction() {
        use crate::ir::MirFunction;
        use karte_ir_codec::IrParse;
        use std::collections::HashMap;

        // Simple test first
        println!("\n=== Test 1: Simple HashMap ===");
        let simple_input = "{ main: MirFunction\nname: main\nparams: []\nblocks: {} }";
        println!("Input: {:?}", simple_input);
        match <HashMap<String, MirFunction>>::parse_nom(simple_input) {
            Ok((rest, map)) => println!("✓ Parsed {} entries, rest: {:?}", map.len(), rest),
            Err(e) => println!("✗ Failed: {:?}", e),
        }

        // Test with newlines matching the format
        println!("\n=== Test 2: HashMap with proper indentation ===");
        let input_with_indent = "{\n        main: \n            MirFunction\n                name: \n                    main\n                params: \n                    []\n                blocks: \n                    {}\n        }";
        println!("Input length: {}", input_with_indent.len());
        match <HashMap<String, MirFunction>>::parse_nom(input_with_indent) {
            Ok((_rest, map)) => {
                println!("✓ Parsed {} entries", map.len());
                for (k, v) in &map {
                    println!("  {}: {}", k, v.name);
                }
            }
            Err(e) => println!("✗ Failed: {:?}", e),
        }

        // Test with full BasicBlock
        println!("\n=== Test 3: HashMap with BasicBlock ===");
        let input = "{\n        main: \n            MirFunction\n                name: \n                    main\n                params: \n                    []\n                blocks: \n                    {\n                        bb0: \n                            BasicBlock\n                                id: \n                                    bb0\n                                statements: \n                                    [\n                                        \n                                            %1 = num value: 2,\n                                        \n                                            %2 = %1,\n                                        \n                                            %3 = num value: 3,\n                                        \n                                            %0 = %2 * %3\n                                        ]\n                                terminator: \n                                    ret value: %0\n                        }\n        }";

        println!("Input length: {}", input.len());

        match <HashMap<String, MirFunction>>::parse_nom(input) {
            Ok((rest, map)) => {
                println!("✓ Parsed HashMap with {} entries", map.len());
                for (k, v) in &map {
                    println!("  {}: {}", k, v.name);
                }
                println!(
                    "  Remaining: {:?}",
                    &rest.chars().take(100).collect::<String>()
                );
            }
            Err(e) => panic!("HashMap parsing failed: {:?}", e),
        }
    }

    #[test]
    fn test_expr_output_mir() {
        use crate::ir::{
            BasicBlockId, MirFunction, MirProgram, Statement, TempId, Terminator, Value,
        };
        use karte_diagnostics::Span;
        use karte_ir_codec::{IrDisplay, IrParse};

        let mut program = MirProgram::new();
        let mut main_fn = MirFunction::new("main".to_string(), vec![]);

        {
            let entry = BasicBlockId(0);
            let block = main_fn
                .basic_blocks
                .get_mut(&entry)
                .expect("entry block should exist");

            block.statements.push(Statement::Assign {
                target: Value::Temp { id: TempId(1), ty: None },
                source: Value::Number { value: 2, ty: None },
                span: Span::default(),
            });
            block.statements.push(Statement::Assign {
                target: Value::Temp { id: TempId(2), ty: None },
                source: Value::Temp { id: TempId(1), ty: None },
                span: Span::default(),
            });
            block.statements.push(Statement::Assign {
                target: Value::Temp { id: TempId(3), ty: None },
                source: Value::Number { value: 3, ty: None },
                span: Span::default(),
            });
            block.statements.push(Statement::BinaryOp {
                target: Value::Temp { id: TempId(0), ty: None },
                left: Value::Temp { id: TempId(2), ty: None },
                op: crate::ir::BinaryOperator::Multiply,
                right: Value::Temp { id: TempId(3), ty: None },
                span: Span::default(),
            });
            block.terminator = Some(Terminator::Return {
                value: Some(Value::Temp { id: TempId(0), ty: None }),
                span: Span::default(),
            });
        }

        program.add_function(main_fn);
        program.set_main("main".to_string());
        program.main_return_value = Some(Value::Temp { id: TempId(0), ty: None });

        let text = program.to_ir_string();
        println!("Rendered MirProgram:\n{}", text);

        let parsed = MirProgram::parse_ir(&text).expect("MirProgram text should parse");
        assert_eq!(parsed.functions.len(), 1);
        assert_eq!(parsed.main_function.as_deref(), Some("main"));
        assert_eq!(parsed.main_return_value, program.main_return_value);
        assert_eq!(parsed.temp_values.len(), 0);
        assert_eq!(parsed.struct_types.len(), 0);
        assert_eq!(parsed.to_ir_string(), text);
    }

    #[test]
    fn test_terminator_alt_debug() {
        use crate::ir::{Terminator, Value};
        use karte_ir_codec::parse::keyword;
        use karte_ir_codec::IrParse;
        use nom::character::complete::multispace1;
        use nom::combinator::map;
        use nom::sequence::{preceded, terminated};

        let input = "ret value: %0";
        println!("\nDEBUG: Testing Terminator parsing step by step");
        println!("Input: '{}'", input);

        // Test individual parsers that would be in the alt
        println!("\n1. Testing keyword('goto'):");
        match keyword("goto")(input) {
            Ok((rest, matched)) => println!("   ✓ matched: '{}', rest: '{}'", matched, rest),
            Err(e) => println!("   ✗ {:?}", e),
        }

        println!("\n2. Testing keyword('if'):");
        match keyword("if")(input) {
            Ok((rest, matched)) => println!("   ✓ matched: '{}', rest: '{}'", matched, rest),
            Err(e) => println!("   ✗ {:?}", e),
        }

        println!("\n3. Testing keyword('ret'):");
        match keyword("ret")(input) {
            Ok((rest, matched)) => println!("   ✓ matched: '{}', rest: '{}'", matched, rest),
            Err(e) => println!("   ✗ {:?}", e),
        }

        println!("\n4. Testing Return parser (keyword + multispace1 + Value):");
        let mut return_parser = map(
            preceded(
                terminated(keyword("ret"), multispace1),
                <Option<Value>>::parse_nom,
            ),
            |value| {
                println!("     Parsed value: {:?}", value);
                value
            },
        );
        match return_parser(input) {
            Ok((rest, _value)) => println!("   ✓ matched, rest: '{}'", rest),
            Err(e) => println!("   ✗ {:?}", e),
        }

        println!("\n5. Testing full Terminator::parse_nom:");
        match Terminator::parse_nom(input) {
            Ok((rest, result)) => println!("   ✓ Parsed: {:?}, rest: '{}'", result, rest),
            Err(e) => println!("   ✗ {:?}", e),
        }
    }
}

#[test]
fn test_single_binop_statement() {
    use crate::ir::Statement;
    use karte_ir_codec::IrParse;

    println!("\n=== Test: Single BinaryOp Statement ===");
    let input = "%0 = %2 * %3";

    println!("Input: {:?}", input);

    match Statement::parse_nom(input) {
        Ok((rest, stmt)) => {
            println!("✓ Parsed Statement: {:#?}", stmt);
            println!("  Remaining: {:?}", rest);
        }
        Err(e) => {
            println!("✗ Failed: {:?}", e);
        }
    }
}

#[test]
fn test_binop_operator_parsing() {
    use crate::ir::BinaryOperator;
    use karte_ir_codec::IrParse;

    println!("\n=== Test: BinaryOperator Parsing ===");
    let input = " * %3";

    println!("Input: {:?}", input);

    match BinaryOperator::parse_nom(input) {
        Ok((rest, op)) => {
            println!("✓ Parsed Operator: {:#?}", op);
            println!("  Remaining: {:?}", rest);
        }
        Err(e) => {
            println!("✗ Failed: {:?}", e);
        }
    }
}
