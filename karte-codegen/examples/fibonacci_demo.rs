//! 斐波那契数列演示程序
//! 
//! 对比HIR和LIR解释器计算斐波那契数列的性能

use karte_codegen::lir_interpreter::{execute, analyze_register_usage, execute_with_debug};
use karte_codegen::hir_interpreter::{evaluate_legacy};
use karte_lir::*;
use karte_hir::*;
use karte_mir::lower::{lower_expr_to_mir};
use karte_lir::lower::{lower_mir_to_lir};
use karte_diagnostics::Span;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== 斐波那契数列演示 - 寄存器分配算法压力测试 ===\n");
    
    let n = 5; // 计算第5个斐波那契数：5
    println!("计算第{}个斐波那契数", n);
    println!("注意：现在只有8个物理寄存器可用，用来测试寄存器分配算法！");
    
    // 创建HIR程序（作为公平比较的基础）
    let hir_program = create_fibonacci_hir(n);
    
    // 从HIR lowering到LIR（公平比较）
    println!("\n1. HIR -> MIR -> LIR 转换：");
    let mir_program = lower_expr_to_mir(&hir_program).map_err(|e| format!("MIR lowering failed: {:?}", e))?;
    let lir_program = lower_mir_to_lir(&mir_program).map_err(|e| format!("LIR lowering failed: {:?}", e))?;
    println!("   ✅ 成功从HIR转换到LIR");
    
    // 寄存器使用分析
    println!("\n2. LIR寄存器使用分析：");
    let analysis = analyze_register_usage(&lir_program)?;
    analysis.print_analysis();
    
    // 检查是否有寄存器压力
    if analysis.max_register_pressure > 8 {
        println!("🔥 寄存器压力测试：需要{}个寄存器，但只有8个可用！", analysis.max_register_pressure);
        println!("这将测试寄存器溢出(spilling)功能！");
    } else {
        println!("⚠️  寄存器压力不足：只需要{}个寄存器", analysis.max_register_pressure);
    }
    
    // 性能测试 - HIR解释器（基准测试）
    println!("\n3. HIR解释器执行（基准测试）：");
    let start = Instant::now();
    let hir_result = evaluate_legacy(&hir_program)?;
    let hir_duration = start.elapsed();
    println!("   HIR解释器: 结果 = {}, 耗时 = {:?}", hir_result, hir_duration);
    
    // 性能测试 - LIR解释器（从HIR转换而来）
    println!("\n4. LIR解释器执行（从HIR转换而来）：");
    let start = Instant::now();
    let lir_result = execute_with_debug(&lir_program,true)?;
    let lir_duration = start.elapsed();
    println!("   LIR解释器: 结果 = {}, 耗时 = {:?}", lir_result, lir_duration);
    
    // 验证结果正确性
    println!("\n5. 结果验证：");
    let expected_fib = fibonacci_reference(n);
    println!("   参考实现: 结果 = {}", expected_fib);
    
    let mut all_correct = true;
    
    if hir_result == expected_fib {
        println!("   ✅ HIR计算结果正确！");
    } else {
        println!("   ❌ HIR结果不匹配！HIR: {}, 期望: {}", hir_result, expected_fib);
        all_correct = false;
    }
    
    if lir_result == expected_fib {
        println!("   ✅ LIR计算结果正确！");
    } else {
        println!("   ❌ LIR结果不匹配！LIR: {}, 期望: {}", lir_result, expected_fib);
        all_correct = false;
    }
    
    if !all_correct {
        return Err("计算结果错误".into());
    }
    
    // 如果寄存器压力够大，运行调试模式看看寄存器分配（放在中间）
    if analysis.max_register_pressure >= 6 {
        println!("\n6. 寄存器分配调试模式：");
        println!("   让我们看看寄存器分配算法是如何处理寄存器压力的...");
        println!("   ---");
        
        let debug_result = execute_with_debug(&lir_program, true)?;
        println!("   调试模式结果: {}", debug_result);
        
        println!("   --- 调试结束 ---");
    }
    
    // === 最终性能对比结果（放在最下面，避免被调试信息淹没）===
    let separator = "=".repeat(60);
    println!("\n{}", separator);
    println!("🎯 最终性能对比结果");
    println!("{}", separator);
    
    if hir_duration.as_nanos() > 0 && lir_duration.as_nanos() > 0 {
        let speedup = hir_duration.as_nanos() as f64 / lir_duration.as_nanos() as f64;
        if speedup > 1.0 {
            println!("🚀 LIR解释器比HIR解释器快 {:.2}x", speedup);
        } else {
            println!("🐌 HIR解释器比LIR解释器快 {:.2}x", 1.0 / speedup);
        }
        println!("📊 详细时间:");
        println!("   HIR耗时: {:?}", hir_duration);
        println!("   LIR耗时: {:?}", lir_duration);
        
        // 计算时间差
        if hir_duration > lir_duration {
            let diff = hir_duration - lir_duration;
            println!("   时间差: {:?} (LIR更快)", diff);
        } else {
            let diff = lir_duration - hir_duration;
            println!("   时间差: {:?} (HIR更快)", diff);
        }
    } else {
        println!("⚠️  时间太短，无法准确比较");
    }
    
    println!("\n{}", separator);
    println!("✅ 演示完成");
    println!("✅ 斐波那契数列计算正确");
    println!("✅ 寄存器分配算法压力测试完成");
    println!("✅ HIR vs LIR 性能对比完成（公平转换）");
    println!("{}", separator);
    
    Ok(())
}

/// 创建更复杂的LIR斐波那契程序，增加寄存器使用
fn create_complex_fibonacci(n: i64) -> LirProgram {
    let mut program = LirProgram::new();
    let mut main_fn = LirFunction::new("main".to_string());
    
    if n <= 1 {
        // 基础情况
        let result_reg = main_fn.new_register();
        let entry_label = main_fn.new_label();
        
        main_fn.add_instruction(Instruction::Label { 
            id: entry_label, 
            span: Span::dummy() 
        });
        
        main_fn.add_instruction(Instruction::Move { 
            dst: result_reg, 
            src: Operand::Immediate { value: n }, 
            span: Span::dummy() 
        });
        
        main_fn.add_instruction(Instruction::Return { 
            value: Some(result_reg), 
            span: Span::dummy() 
        });
        
        program.add_function(main_fn);
        program.set_main("main".to_string());
        return program;
    }
    
    // 创建大量寄存器来强制产生寄存器压力，但保证生命周期正确
    let n_reg = main_fn.new_register();           // 寄存器 0: 存储n的值（整个程序都需要）
    let a_reg = main_fn.new_register();           // 寄存器 1: fib(i-2)
    let b_reg = main_fn.new_register();           // 寄存器 2: fib(i-1)
    let i_reg = main_fn.new_register();           // 寄存器 3: 循环计数器
    let const_1_reg = main_fn.new_register();     // 寄存器 4: 存储常数1（整个程序都需要）
    let temp_add_reg = main_fn.new_register();    // 寄存器 5: 临时加法结果
    let helper1_reg = main_fn.new_register();     // 寄存器 6: 辅助寄存器1
    let helper2_reg = main_fn.new_register();     // 寄存器 7: 辅助寄存器2
    let helper3_reg = main_fn.new_register();     // 寄存器 8: 辅助寄存器3
    let helper4_reg = main_fn.new_register();     // 寄存器 9: 辅助寄存器4
    let helper5_reg = main_fn.new_register();     // 寄存器 10: 辅助寄存器5
    let helper6_reg = main_fn.new_register();     // 寄存器 11: 辅助寄存器6
    let helper7_reg = main_fn.new_register();     // 寄存器 12: 辅助寄存器7
    let sum_reg = main_fn.new_register();         // 寄存器 13: 总和累计寄存器
    let debug_reg = main_fn.new_register();       // 寄存器 14: 调试寄存器
    
    // 创建标签
    let entry_label = main_fn.new_label();
    let loop_start = main_fn.new_label();
    let loop_end = main_fn.new_label();
    
    // 程序开始
    main_fn.add_instruction(Instruction::Label { 
        id: entry_label, 
        span: Span::dummy() 
    });
    
    // 初始化核心寄存器
    main_fn.add_instruction(Instruction::Move { 
        dst: n_reg, 
        src: Operand::Immediate { value: n }, 
        span: Span::dummy() 
    });
    
    main_fn.add_instruction(Instruction::Move { 
        dst: const_1_reg, 
        src: Operand::Immediate { value: 1 }, 
        span: Span::dummy() 
    });
    
    main_fn.add_instruction(Instruction::Move { 
        dst: a_reg, 
        src: Operand::Immediate { value: 0 }, 
        span: Span::dummy() 
    });
    
    main_fn.add_instruction(Instruction::Move { 
        dst: b_reg, 
        src: Operand::Immediate { value: 1 }, 
        span: Span::dummy() 
    });
    
    main_fn.add_instruction(Instruction::Move { 
        dst: i_reg, 
        src: Operand::Immediate { value: 2 }, 
        span: Span::dummy() 
    });
    
    // 初始化所有辅助寄存器，并保持它们在整个程序中活跃
    main_fn.add_instruction(Instruction::Move { 
        dst: helper1_reg, 
        src: Operand::Immediate { value: 10 }, 
        span: Span::dummy() 
    });
    
    main_fn.add_instruction(Instruction::Move { 
        dst: helper2_reg, 
        src: Operand::Immediate { value: 20 }, 
        span: Span::dummy() 
    });
    
    main_fn.add_instruction(Instruction::Move { 
        dst: helper3_reg, 
        src: Operand::Immediate { value: 30 }, 
        span: Span::dummy() 
    });
    
    main_fn.add_instruction(Instruction::Move { 
        dst: helper4_reg, 
        src: Operand::Immediate { value: 40 }, 
        span: Span::dummy() 
    });
    
    main_fn.add_instruction(Instruction::Move { 
        dst: helper5_reg, 
        src: Operand::Immediate { value: 50 }, 
        span: Span::dummy() 
    });
    
    main_fn.add_instruction(Instruction::Move { 
        dst: helper6_reg, 
        src: Operand::Immediate { value: 60 }, 
        span: Span::dummy() 
    });
    
    main_fn.add_instruction(Instruction::Move { 
        dst: helper7_reg, 
        src: Operand::Immediate { value: 70 }, 
        span: Span::dummy() 
    });
    
    main_fn.add_instruction(Instruction::Move { 
        dst: sum_reg, 
        src: Operand::Immediate { value: 0 }, 
        span: Span::dummy() 
    });
    
    main_fn.add_instruction(Instruction::Move { 
        dst: debug_reg, 
        src: Operand::Immediate { value: 999 }, 
        span: Span::dummy() 
    });
    
    // 循环开始：while i <= n
    main_fn.add_instruction(Instruction::Label { 
        id: loop_start, 
        span: Span::dummy() 
    });
    
    // 循环条件：i > n 则跳出
    main_fn.add_instruction(Instruction::Compare { 
        src1: Operand::Register { id: i_reg }, 
        src2: Operand::Register { id: n_reg }, 
        span: Span::dummy() 
    });
    
    main_fn.add_instruction(Instruction::JumpGreater { 
        target: loop_end, 
        span: Span::dummy() 
    });
    
    // 复杂的斐波那契计算，同时使用大量寄存器保持压力
    // temp_add = a + b
    main_fn.add_instruction(Instruction::Add { 
        dst: temp_add_reg, 
        src1: Operand::Register { id: a_reg }, 
        src2: Operand::Register { id: b_reg }, 
        span: Span::dummy() 
    });
    
    // 使用所有辅助寄存器进行无意义但复杂的计算，保持它们活跃
    // helper1 = helper1 + const_1
    main_fn.add_instruction(Instruction::Add { 
        dst: helper1_reg, 
        src1: Operand::Register { id: helper1_reg }, 
        src2: Operand::Register { id: const_1_reg }, 
        span: Span::dummy() 
    });
    
    // helper2 = helper2 + helper1
    main_fn.add_instruction(Instruction::Add { 
        dst: helper2_reg, 
        src1: Operand::Register { id: helper2_reg }, 
        src2: Operand::Register { id: helper1_reg }, 
        span: Span::dummy() 
    });
    
    // helper3 = helper3 + const_1
    main_fn.add_instruction(Instruction::Add { 
        dst: helper3_reg, 
        src1: Operand::Register { id: helper3_reg }, 
        src2: Operand::Register { id: const_1_reg }, 
        span: Span::dummy() 
    });
    
    // helper4 = helper4 + helper3  
    main_fn.add_instruction(Instruction::Add { 
        dst: helper4_reg, 
        src1: Operand::Register { id: helper4_reg }, 
        src2: Operand::Register { id: helper3_reg }, 
        span: Span::dummy() 
    });
    
    // helper5 = helper5 + const_1
    main_fn.add_instruction(Instruction::Add { 
        dst: helper5_reg, 
        src1: Operand::Register { id: helper5_reg }, 
        src2: Operand::Register { id: const_1_reg }, 
        span: Span::dummy() 
    });
    
    // helper6 = helper6 + helper5
    main_fn.add_instruction(Instruction::Add { 
        dst: helper6_reg, 
        src1: Operand::Register { id: helper6_reg }, 
        src2: Operand::Register { id: helper5_reg }, 
        span: Span::dummy() 
    });
    
    // helper7 = helper7 + const_1
    main_fn.add_instruction(Instruction::Add { 
        dst: helper7_reg, 
        src1: Operand::Register { id: helper7_reg }, 
        src2: Operand::Register { id: const_1_reg }, 
        span: Span::dummy() 
    });
    
    // debug_reg = debug_reg + const_1 (保持活跃)
    main_fn.add_instruction(Instruction::Add { 
        dst: debug_reg, 
        src1: Operand::Register { id: debug_reg }, 
        src2: Operand::Register { id: const_1_reg }, 
        span: Span::dummy() 
    });
    
    // sum_reg += temp_add (累加斐波那契数)
    main_fn.add_instruction(Instruction::Add { 
        dst: sum_reg, 
        src1: Operand::Register { id: sum_reg }, 
        src2: Operand::Register { id: temp_add_reg }, 
        span: Span::dummy() 
    });
    
    // 更新斐波那契序列：a = b, b = temp_add
    main_fn.add_instruction(Instruction::Move { 
        dst: a_reg, 
        src: Operand::Register { id: b_reg }, 
        span: Span::dummy() 
    });
    
    main_fn.add_instruction(Instruction::Move { 
        dst: b_reg, 
        src: Operand::Register { id: temp_add_reg }, 
        span: Span::dummy() 
    });
    
    // i = i + 1
    main_fn.add_instruction(Instruction::Add { 
        dst: i_reg, 
        src1: Operand::Register { id: i_reg }, 
        src2: Operand::Register { id: const_1_reg }, 
        span: Span::dummy() 
    });
    
    // 跳回循环开始
    main_fn.add_instruction(Instruction::Jump { 
        target: loop_start, 
        span: Span::dummy() 
    });
    
    // 循环结束
    main_fn.add_instruction(Instruction::Label { 
        id: loop_end, 
        span: Span::dummy() 
    });
    
    // 在返回前再次使用所有辅助寄存器，确保它们的生命周期延续到最后
    // 这些操作不影响结果，但保持寄存器压力
    main_fn.add_instruction(Instruction::Add { 
        dst: helper1_reg, 
        src1: Operand::Register { id: helper1_reg }, 
        src2: Operand::Register { id: helper2_reg }, 
        span: Span::dummy() 
    });
    
    main_fn.add_instruction(Instruction::Add { 
        dst: helper3_reg, 
        src1: Operand::Register { id: helper3_reg }, 
        src2: Operand::Register { id: helper4_reg }, 
        span: Span::dummy() 
    });
    
    main_fn.add_instruction(Instruction::Add { 
        dst: helper5_reg, 
        src1: Operand::Register { id: helper5_reg }, 
        src2: Operand::Register { id: helper6_reg }, 
        span: Span::dummy() 
    });
    
    main_fn.add_instruction(Instruction::Add { 
        dst: debug_reg, 
        src1: Operand::Register { id: debug_reg }, 
        src2: Operand::Register { id: helper7_reg }, 
        span: Span::dummy() 
    });
    
    // 返回斐波那契结果
    main_fn.add_instruction(Instruction::Return { 
        value: Some(b_reg), 
        span: Span::dummy() 
    });
    
    program.add_function(main_fn);
    program.set_main("main".to_string());
    program 
}

/// 参考斐波那契实现（用于验证）
fn fibonacci_reference(n: i64) -> i64 {
    if n <= 1 {
        n
    } else {
        let mut a = 0;
        let mut b = 1;
        for _ in 2..=n {
            let temp = a + b;
            a = b;
            b = temp;
        }
        b
    }
}

/// 创建HIR斐波那契程序（Y组合子版本）
fn create_fibonacci_hir(n: i64) -> Expr {
    if n <= 1 {
        // 基础情况：直接返回n
        Expr::Number {
            value: n,
            span: Span::dummy(),
        }
    } else {
        // 使用Y组合子创建真正的递归斐波那契
        create_fibonacci_y_combinator(n)
    }
}

/// 创建使用Y组合子的斐波那契表达式
fn create_fibonacci_y_combinator(n: i64) -> Expr {
    // 创建Y组合子形式的斐波那契：
    // (|fib, n| if n <= 1 then n else fib(fib, n-1) + fib(fib, n-2))(lambda_itself, target_n)
    
    // 创建斐波那契lambda：|fib, n| if n <= 1 then n else fib(fib, n-1) + fib(fib, n-2)
    let fib_lambda = Expr::Lambda {
        params: vec![
            Parameter::simple("fib".to_string()),
            Parameter::simple("n".to_string()),
        ],
        body: Box::new(Expr::If {
            condition: Box::new(Expr::BinaryOp {
                left: Box::new(Expr::Identifier { 
                    name: "n".to_string(), 
                    span: Span::dummy() 
                }),
                op: BinaryOperator::LessEqual,
                right: Box::new(Expr::Number { value: 1, span: Span::dummy() }),
                span: Span::dummy(),
            }),
            then_branch: Box::new(Expr::Identifier { 
                name: "n".to_string(), 
                span: Span::dummy() 
            }),
            else_branch: Some(Box::new(Expr::BinaryOp {
                left: Box::new(Expr::FunctionCall {
                    function: Box::new(Expr::Identifier { 
                        name: "fib".to_string(), 
                        span: Span::dummy() 
                    }),
                    args: vec![
                        Expr::Identifier { name: "fib".to_string(), span: Span::dummy() },
                        Expr::BinaryOp {
                            left: Box::new(Expr::Identifier { 
                                name: "n".to_string(), 
                                span: Span::dummy() 
                            }),
                            op: BinaryOperator::Subtract,
                            right: Box::new(Expr::Number { value: 1, span: Span::dummy() }),
                            span: Span::dummy(),
                        }
                    ],
                    span: Span::dummy(),
                }),
                op: BinaryOperator::Add,
                right: Box::new(Expr::FunctionCall {
                    function: Box::new(Expr::Identifier { 
                        name: "fib".to_string(), 
                        span: Span::dummy() 
                    }),
                    args: vec![
                        Expr::Identifier { name: "fib".to_string(), span: Span::dummy() },
                        Expr::BinaryOp {
                            left: Box::new(Expr::Identifier { 
                                name: "n".to_string(), 
                                span: Span::dummy() 
                            }),
                            op: BinaryOperator::Subtract,
                            right: Box::new(Expr::Number { value: 2, span: Span::dummy() }),
                            span: Span::dummy(),
                        }
                    ],
                    span: Span::dummy(),
                }),
                span: Span::dummy(),
            })),
            span: Span::dummy(),
        }),
        span: Span::dummy(),
    };
    
    // 现在调用这个lambda：(|fib, n| ...)(lambda_itself, target_n)
    Expr::FunctionCall {
        function: Box::new(fib_lambda.clone()),
        args: vec![
            fib_lambda, // 传递自己作为第一个参数（自应用）
            Expr::Number { value: n, span: Span::dummy() }
        ],
        span: Span::dummy(),
    }
} 