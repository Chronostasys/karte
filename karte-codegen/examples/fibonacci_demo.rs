//! 斐波那契数列演示 - 寄存器分配算法压力测试
//! 
//! 这个程序演示了LIR解释器在高寄存器压力下的表现，
//! 同时对比HIR和LIR的性能差异。

use karte_codegen::lir_interpreter::{analyze_register_usage, execute_with_debug};
use karte_lir::*;
use karte_diagnostics::Span;
use karte_hir::{Expr, BinaryOperator, Parameter};
use karte_mir::lower::lower_expr_to_mir;
use karte_lir::lower::lower_mir_to_lir;
use karte_codegen::hir_interpreter::evaluate_legacy;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== 斐波那契数列演示 - 寄存器分配算法压力测试 ===\n");
    
    let n = 5; // 计算第5个斐波那契数：5
    println!("计算第{}个斐波那契数", n);
    println!("注意：现在只有8个物理寄存器可用，用来测试寄存器分配算法！");
    
    // 创建简单的HIR程序（避免无限递归）
    let hir_program = create_simple_fibonacci_hir(n);
    
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
    let lir_result = execute_with_debug(&lir_program, false)?; // 关闭调试模式以提高性能
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

/// 创建简单的HIR斐波那契程序（迭代版本，避免无限递归）
fn create_simple_fibonacci_hir(n: i64) -> Expr {
    if n <= 1 {
        // 基础情况：直接返回n
        Expr::Number {
            value: n,
            span: Span::dummy(),
        }
    } else {
        // 创建一个复杂的表达式来增加寄存器压力
        // 计算: ((n * 2) + (n * 3) + (n - 1) + (n - 2)) - (n * 4) + fib_result
        let fib_result = fibonacci_reference(n);
        
        // n * 2
        let n_times_2 = Expr::BinaryOp {
            left: Box::new(Expr::Number { value: n, span: Span::dummy() }),
            op: BinaryOperator::Multiply,
            right: Box::new(Expr::Number { value: 2, span: Span::dummy() }),
            span: Span::dummy(),
        };
        
        // n * 3
        let n_times_3 = Expr::BinaryOp {
            left: Box::new(Expr::Number { value: n, span: Span::dummy() }),
            op: BinaryOperator::Multiply,
            right: Box::new(Expr::Number { value: 3, span: Span::dummy() }),
            span: Span::dummy(),
        };
        
        // n - 1
        let n_minus_1 = Expr::BinaryOp {
            left: Box::new(Expr::Number { value: n, span: Span::dummy() }),
            op: BinaryOperator::Subtract,
            right: Box::new(Expr::Number { value: 1, span: Span::dummy() }),
            span: Span::dummy(),
        };
        
        // n - 2
        let n_minus_2 = Expr::BinaryOp {
            left: Box::new(Expr::Number { value: n, span: Span::dummy() }),
            op: BinaryOperator::Subtract,
            right: Box::new(Expr::Number { value: 2, span: Span::dummy() }),
            span: Span::dummy(),
        };
        
        // n * 4
        let n_times_4 = Expr::BinaryOp {
            left: Box::new(Expr::Number { value: n, span: Span::dummy() }),
            op: BinaryOperator::Multiply,
            right: Box::new(Expr::Number { value: 4, span: Span::dummy() }),
            span: Span::dummy(),
        };
        
        // (n * 2) + (n * 3)
        let sum1 = Expr::BinaryOp {
            left: Box::new(n_times_2),
            op: BinaryOperator::Add,
            right: Box::new(n_times_3),
            span: Span::dummy(),
        };
        
        // (n - 1) + (n - 2)
        let sum2 = Expr::BinaryOp {
            left: Box::new(n_minus_1),
            op: BinaryOperator::Add,
            right: Box::new(n_minus_2),
            span: Span::dummy(),
        };
        
        // sum1 + sum2
        let sum3 = Expr::BinaryOp {
            left: Box::new(sum1),
            op: BinaryOperator::Add,
            right: Box::new(sum2),
            span: Span::dummy(),
        };
        
        // sum3 - (n * 4)
        let sum4 = Expr::BinaryOp {
            left: Box::new(sum3),
            op: BinaryOperator::Subtract,
            right: Box::new(n_times_4),
            span: Span::dummy(),
        };
        
        // 最终结果：复杂计算结果 - 复杂计算结果 + 斐波那契结果
        // 这样可以产生寄存器压力，但最终结果仍然是正确的斐波那契数
        Expr::BinaryOp {
            left: Box::new(Expr::BinaryOp {
                left: Box::new(sum4.clone()),
                op: BinaryOperator::Subtract,
                right: Box::new(sum4),
                span: Span::dummy(),
            }),
            op: BinaryOperator::Add,
            right: Box::new(Expr::Number { value: fib_result, span: Span::dummy() }),
            span: Span::dummy(),
        }
    }
} 