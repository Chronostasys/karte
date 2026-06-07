#!/bin/bash
# test_cc 回归测试套件
# 用法: bash test_cc/run_tests.sh
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PASS=0
FAIL=0

run_test() {
    local name="$1"
    local file="$2"
    local expected="$3"
    local out="/tmp/test_cc_reg_$name"
    
    if bash "$SCRIPT_DIR/test_cc.sh" "$SCRIPT_DIR/tests/$file" -o "$out" 2>/dev/null; then
        local result=$("$out"; echo $?)
        if [ "$result" = "$expected" ]; then
            echo "  PASS: $name (exit=$result)"
            PASS=$((PASS + 1))
        else
            echo "  FAIL: $name (expected=$expected, got=$result)"
            FAIL=$((FAIL + 1))
        fi
    else
        echo "  FAIL: $name (compilation error)"
        FAIL=$((FAIL + 1))
    fi
    rm -f "$out"
}

echo "=== test_cc regression tests ==="

run_test "add"          "add.c"          "7"
run_test "fib"          "fib.c"          "55"
run_test "fib_nobrace"  "fib_nobrace.c"  "55"
run_test "var"          "var.c"          "53"
run_test "if_else"      "if_else.c"      "7"
run_test "while"        "while.c"        "55"
run_test "sum_recur"    "sum_recur.c"    "15"
run_test "for_loop"     "for_loop.c"     "55"
run_test "for_complex"  "for_complex.c"  "120"
run_test "for_nested"   "for_nested.c"   "9"
run_test "nested_call"  "nested_call.c"  "42"
run_test "multi_recur"  "multi_recur.c"  "9"

echo ""
echo "=== Results: $PASS passed, $FAIL failed ==="

if [ "$FAIL" -gt 0 ]; then
    exit 1
fi
