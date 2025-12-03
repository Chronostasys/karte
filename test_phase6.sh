#!/bin/bash

echo "=== Phase 6: MIR 集成测试 ==="
echo ""
echo "测试 1: 简单函数测试"
echo "--------------------"
KARTE_ENABLE_ESCAPE_ANALYSIS=1 ./target/release/karte run --mode script test_escape_simple.karte
echo ""

echo "测试 2: 栈分配测试"
echo "--------------------"
KARTE_ENABLE_ESCAPE_ANALYSIS=1 ./target/release/karte run --mode script test_escape_stack.karte
echo ""

echo "测试 3: 带 verbose 输出的详细分析"
echo "--------------------"
KARTE_ENABLE_ESCAPE_ANALYSIS=1 ./target/release/karte --verbose run --mode script test_escape_stack.karte 2>&1 | grep -A 10 "逃逸分析结果"
echo ""

echo "=== 所有测试完成 ==="
