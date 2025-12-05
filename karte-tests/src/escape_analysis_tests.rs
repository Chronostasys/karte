//! 逃逸分析集成测试
//!
//! 这些测试验证逃逸分析功能的正确性，包括：
//! - 堆分配插入
//! - 引用安全性
//! - 多次闭包调用
//! - 不同变量生命周期场景

use karte_lexer::Lexer;
use karte_lir::OptimizationLevel;
use karte_mir::lower::{lower_expr_to_mir_with_options, LoweringOptions};
use karte_module_system::{lower_mir_to_final_lir, optimize_mir_with_escape_analysis};
use karte_parser::{parse_with_type_check, ParserMode};
use std::collections::HashSet;

#[test]
fn test_escape_simple_address_of() {
    let source = r#"
fn main() -> number {
    let a = || {
        let d = 1;
        &d
    };
    let ptr = a();
    *ptr
}
"#;

    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();

    let (parse_result, _diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
    let parse_result = parse_result.expect("No parse result");
    let ast = parse_result.expr();

    let options = LoweringOptions {
        known_functions: HashSet::new(),
        module_context: None,
        expr_types: parse_result.expr_types.clone(),
    };

    let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");

    // 应用逃逸分析转换
    optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");

    // 验证 lambda$0 包含 HeapAlloc 指令
    let lambda0 = mir.functions.get("lambda$0").expect("lambda$0 not found");
    let bb0 = lambda0.basic_blocks.get(&lambda0.entry_block).expect("bb0 not found");

    let has_heap_alloc = bb0.statements.iter().any(|stmt| {
        matches!(
            stmt,
            karte_mir::Statement::HeapAlloc {
                object_type,
                ..
            } if object_type == "escaped_value"
        )
    });

    assert!(
        has_heap_alloc,
        "lambda$0 should contain HeapAlloc instruction"
    );

    // 验证包含 Store 指令
    let has_store = bb0.statements.iter().any(|stmt| {
        matches!(stmt, karte_mir::Statement::Store { .. })
    });

    assert!(has_store, "lambda$0 should contain Store instruction");
}

#[test]
fn test_escape_two_closures_with_calls() {
    let source = r#"
fn main() -> number {
    let a = || {
        let d = 1;
        &d
    };
    let b = || {
        999
    };
    let ptr = a();
    let tmp = b();
    *ptr
}
"#;

    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();

    let (parse_result, _diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
    let parse_result = parse_result.expect("No parse result");
    let ast = parse_result.expr();

    let options = LoweringOptions {
        known_functions: HashSet::new(),
        module_context: None,
        expr_types: parse_result.expr_types.clone(),
    };

    let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");

    // 应用逃逸分析
    optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");

    // lambda$0 应该有堆分配（因为 &d 逃逸）
    let lambda0 = mir.functions.get("lambda$0").expect("lambda$0 not found");
    let bb0 = lambda0.basic_blocks.get(&lambda0.entry_block).expect("bb0 not found");

    let heap_alloc_count = bb0
        .statements
        .iter()
        .filter(|stmt| matches!(stmt, karte_mir::Statement::HeapAlloc { .. }))
        .count();

    assert_eq!(
        heap_alloc_count, 1,
        "lambda$0 should have exactly 1 HeapAlloc"
    );

    // lambda$1 不应该有堆分配（没有逃逸）
    let lambda1 = mir.functions.get("lambda$1").expect("lambda$1 not found");
    let bb0 = lambda1.basic_blocks.get(&lambda1.entry_block).expect("bb0 not found");

    let heap_alloc_count = bb0
        .statements
        .iter()
        .filter(|stmt| matches!(stmt, karte_mir::Statement::HeapAlloc { .. }))
        .count();

    assert_eq!(
        heap_alloc_count, 0,
        "lambda$1 should have no HeapAlloc (no escaping)"
    );
}

#[test]
fn test_escape_no_deref() {
    let source = r#"
fn main() -> number {
    let a = || {
        let d = 1;
        &d
    };
    let b = || {
        999
    };
    let ptr = a();
    let tmp = b();
    tmp
}
"#;

    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();

    let (parse_result, _diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
    let parse_result = parse_result.expect("No parse result");
    let ast = parse_result.expr();

    let options = LoweringOptions {
        known_functions: HashSet::new(),
        module_context: None,
        expr_types: parse_result.expr_types.clone(),
    };

    let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");

    // 应用逃逸分析
    optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");

    // 即使不解引用，lambda$0 中的 &d 仍然逃逸
    let lambda0 = mir.functions.get("lambda$0").expect("lambda$0 not found");
    let bb0 = lambda0.basic_blocks.get(&lambda0.entry_block).expect("bb0 not found");

    let has_heap_alloc = bb0
        .statements
        .iter()
        .any(|stmt| matches!(stmt, karte_mir::Statement::HeapAlloc { .. }));

    assert!(
        has_heap_alloc,
        "lambda$0 should still heap-allocate even without deref"
    );
}

#[test]
fn test_escape_analysis_temp_id_range() {
    let source = r#"
fn main() -> number {
    let a = || {
        let d = 1;
        &d
    };
    let ptr = a();
    *ptr
}
"#;

    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();

    let (parse_result, _diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
    let parse_result = parse_result.expect("No parse result");
    let ast = parse_result.expr();

    let options = LoweringOptions {
        known_functions: HashSet::new(),
        module_context: None,
        expr_types: parse_result.expr_types.clone(),
    };

    let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");

    // 应用逃逸分析
    optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");

    let lambda0 = mir.functions.get("lambda$0").expect("lambda$0 not found");
    let bb0 = lambda0.basic_blocks.get(&lambda0.entry_block).expect("bb0 not found");

    // 查找 HeapAlloc 的 target
    let heap_alloc_target = bb0.statements.iter().find_map(|stmt| {
        if let karte_mir::Statement::HeapAlloc { target, .. } = stmt {
            Some(target)
        } else {
            None
        }
    });

    assert!(
        heap_alloc_target.is_some(),
        "Should have HeapAlloc instruction"
    );

    // 验证 target 是 Temp 且 ID >= 10000
    if let Some(karte_mir::Value::Temp { id, .. }) = heap_alloc_target {
        assert!(
            id.0 >= 10000,
            "Heap-allocated temp should have ID >= 10000, got {}",
            id.0
        );
    } else {
        panic!("HeapAlloc target should be a Temp variable");
    }
}

#[test]
fn test_escape_analysis_lir_integration() {
    let source = r#"
fn main() -> number {
    let a = || {
        let d = 1;
        &d
    };
    let ptr = a();
    *ptr
}
"#;

    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();

    let (parse_result, _diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
    let parse_result = parse_result.expect("No parse result");
    let ast = parse_result.expr();

    let options = LoweringOptions {
        known_functions: HashSet::new(),
        module_context: None,
        expr_types: parse_result.expr_types.clone(),
    };

    let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");

    // 应用逃逸分析
    optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");

    // Lower to LIR
    let lir = lower_mir_to_final_lir(&mir, OptimizationLevel::Balanced, false).expect("LIR lowering failed");

    // 验证 lambda$0 函数存在
    let lambda0 = lir
        .functions
        .get("lambda$0")
        .expect("lambda$0 not found in LIR");

    // 验证包含 Alloc 指令（HeapAlloc 在 LIR 中变成 Alloc）
    let has_alloc = lambda0.instructions.iter().any(|instr| {
        matches!(
            instr,
            karte_lir::Instruction::Alloc {
                allocation_type: karte_lir::AllocationType::Heap,
                ..
            }
        )
    });

    assert!(
        has_alloc,
        "LIR lambda$0 should contain Heap Alloc instruction"
    );

    // 验证包含 Store64 指令
    let has_store = lambda0
        .instructions
        .iter()
        .any(|instr| matches!(instr, karte_lir::Instruction::Store64 { .. }));

    assert!(
        has_store,
        "LIR lambda$0 should contain Store64 instruction"
    );
}

#[test]
fn test_escape_multiple_address_of() {
    let source = r#"
fn main() -> number {
    let a = || {
        let d = 1;
        let e = 2;
        let ptr1 = &d;
        let ptr2 = &e;
        *ptr1 + *ptr2
    };
    a()
}
"#;

    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();

    let (parse_result, _diagnostics) = parse_with_type_check(&tokens, ParserMode::Project, None);
    let parse_result = parse_result.expect("No parse result");
    let ast = parse_result.expr();

    let options = LoweringOptions {
        known_functions: HashSet::new(),
        module_context: None,
        expr_types: parse_result.expr_types.clone(),
    };

    let mut mir = lower_expr_to_mir_with_options(&ast, options).expect("MIR lowering failed");

    // 应用逃逸分析
    optimize_mir_with_escape_analysis(&mut mir, false).expect("Escape analysis failed");

    let lambda0 = mir.functions.get("lambda$0").expect("lambda$0 not found");
    let bb0 = lambda0.basic_blocks.get(&lambda0.entry_block).expect("bb0 not found");

    // ✅ Phase 1优化：ptr1和ptr2只在本地使用，没有逃逸
    // 因此 d 和 e 不需要堆分配，应该是 0 个 HeapAlloc
    let heap_alloc_count = bb0
        .statements
        .iter()
        .filter(|stmt| matches!(stmt, karte_mir::Statement::HeapAlloc { .. }))
        .count();

    assert_eq!(
        heap_alloc_count, 0,
        "Should have 0 HeapAlloc instructions because ptr1 and ptr2 don't escape"
    );

    // 相应地，应该没有 Store 指令
    let store_count = bb0
        .statements
        .iter()
        .filter(|stmt| matches!(stmt, karte_mir::Statement::Store { .. }))
        .count();

    assert_eq!(
        store_count, 0,
        "Should have 0 Store instructions because d and e are stack-allocated"
    );
}
