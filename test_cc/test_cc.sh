#!/bin/bash
# test_cc — 用 Karte 编写的 C 子集编译器
# 使用方式: ./test_cc.sh <input.c> [-o <output>]
#
# 内部流程:
#   1. AOT 编译 test_cc 为原生二进制
#   2. 运行 AOT 二进制，将 C 源码编译为 x86_64 汇编
#   3. 用系统 as + ld 汇编链接为可执行文件

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
KARTE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
KARTE_BIN="$KARTE_ROOT/target/debug/karte"
TEST_CC_SRC="$KARTE_ROOT/test_cc/src/main.karte"
AOT_BIN="/tmp/.karte_test_cc_aot"

# 解析参数
INPUT=""
OUTPUT="a.out"
while [[ $# -gt 0 ]]; do
    case $1 in
        -o)
            OUTPUT="$2"
            shift 2
            ;;
        -h|--help)
            echo "Usage: $0 <input.c> [-o <output>]"
            echo "A C subset compiler written in Karte language."
            echo ""
            echo "Supported C features:"
            echo "  - Functions (recursive), int return type"
            echo "  - Local variables with int type"
            echo "  - if/else if/else, while, for loops"
            echo "  - Arithmetic: + - * / %"
            echo "  - Comparison: == != < > <= >="
            echo "  - Logical: && || !"
            echo "  - Compound assignment: += -= *= /= %="
            echo "  - return statements"
            echo "  - Single-line (//) and multi-line (/* */) comments"
            exit 0
            ;;
        *)
            INPUT="$1"
            shift
            ;;
    esac
done

if [[ -z "$INPUT" ]]; then
    echo "Error: no input file" >&2
    echo "Usage: $0 <input.c> [-o <output>]" >&2
    exit 1
fi

if [[ ! -f "$INPUT" ]]; then
    echo "Error: file not found: $INPUT" >&2
    exit 1
fi

# 确保 karte 已编译
if [[ ! -x "$KARTE_BIN" ]] || [[ "$TEST_CC_SRC" -nt "$AOT_BIN" ]] || [[ "$KARTE_BIN" -nt "$AOT_BIN" ]]; then
    echo "Building test_cc (AOT)..." >&2
    cd "$KARTE_ROOT"
    cargo build 2>/dev/null
    "$KARTE_BIN" aot --mode project "$TEST_CC_SRC" -o "$AOT_BIN" 2>/dev/null
fi

# 编译 C → 汇编
ASM_FILE=$(mktemp /tmp/test_cc_XXXXXX.s)
trap "rm -f $ASM_FILE" EXIT

"$AOT_BIN" < "$INPUT" > "$ASM_FILE" 2>/dev/null

# 清理末尾可能的多余数字输出
sed -i '/^[0-9]\+$/d' "$ASM_FILE"

# 汇编 + 链接
OBJ_FILE=$(mktemp /tmp/test_cc_XXXXXX.o)
trap "rm -f $ASM_FILE $OBJ_FILE" EXIT

as --64 "$ASM_FILE" -o "$OBJ_FILE" 2>&1 | head -5
ld "$OBJ_FILE" -o "$OUTPUT" 2>&1 | head -5

chmod +x "$OUTPUT"
echo "Compiled: $OUTPUT" >&2
