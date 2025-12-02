#!/bin/bash
# Karte VS Code 扩展诊断脚本

set -e

echo "🔍 Karte LSP Extension Diagnostics"
echo "===================================="
echo ""

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# 检查函数
check_pass() {
  echo -e "${GREEN}✓${NC} $1"
}

check_fail() {
  echo -e "${RED}✗${NC} $1"
}

check_warn() {
  echo -e "${YELLOW}⚠${NC} $1"
}

# 1. 检查 Node.js
echo "1. Checking Node.js..."
if command -v node &> /dev/null; then
  NODE_VERSION=$(node --version)
  check_pass "Node.js installed: $NODE_VERSION"
else
  check_fail "Node.js not found"
  exit 1
fi

# 2. 检查 npm
echo ""
echo "2. Checking npm..."
if command -v npm &> /dev/null; then
  NPM_VERSION=$(npm --version)
  check_pass "npm installed: $NPM_VERSION"
else
  check_fail "npm not found"
  exit 1
fi

# 3. 检查 Rust/Cargo
echo ""
echo "3. Checking Rust..."
if command -v cargo &> /dev/null; then
  CARGO_VERSION=$(cargo --version)
  check_pass "Cargo installed: $CARGO_VERSION"
else
  check_warn "Cargo not found (needed to build LSP server)"
fi

# 4. 检查扩展依赖
echo ""
echo "4. Checking extension dependencies..."
if [ -d "node_modules" ]; then
  check_pass "node_modules directory exists"
else
  check_fail "node_modules not found. Run: npm install"
  exit 1
fi

# 5. 检查编译输出
echo ""
echo "5. Checking compiled extension..."
if [ -f "out/extension.js" ]; then
  check_pass "Extension compiled: out/extension.js"
else
  check_fail "Extension not compiled. Run: npm run compile"
  exit 1
fi

# 6. 查找 LSP 服务器
echo ""
echo "6. Looking for LSP server..."
FOUND=0

# 候选路径
CANDIDATES=(
  "../target/release/karte-lsp-server"
  "../target/debug/karte-lsp-server"
  "../../target/release/karte-lsp-server"
  "../../target/debug/karte-lsp-server"
)

for candidate in "${CANDIDATES[@]}"; do
  if [ -f "$candidate" ]; then
    check_pass "Found server: $candidate"
    SERVER_PATH="$candidate"
    FOUND=1

    # 检查是否可执行
    if [ -x "$candidate" ]; then
      check_pass "Server is executable"
    else
      check_warn "Server is not executable. Run: chmod +x $candidate"
    fi

    # 检查文件大小
    SIZE=$(ls -lh "$candidate" | awk '{print $5}')
    echo "   Server size: $SIZE"
    break
  fi
done

if [ $FOUND -eq 0 ]; then
  check_fail "LSP server not found in any candidate location"
  echo ""
  echo "   Searched in:"
  for candidate in "${CANDIDATES[@]}"; do
    echo "   - $candidate"
  done
  echo ""
  echo "   To build the server, run:"
  echo "   cd .. && cargo build -p karte-lsp"
  exit 1
fi

# 7. 测试运行服务器
echo ""
echo "7. Testing LSP server..."
if [ -n "$SERVER_PATH" ]; then
  # 使用 timeout 避免无限等待
  if command -v timeout &> /dev/null; then
    TIMEOUT_CMD="timeout 2s"
  elif command -v gtimeout &> /dev/null; then
    TIMEOUT_CMD="gtimeout 2s"
  else
    TIMEOUT_CMD=""
    check_warn "timeout command not found, skipping server test"
  fi

  if [ -n "$TIMEOUT_CMD" ]; then
    if $TIMEOUT_CMD "$SERVER_PATH" 2>&1 | head -1 &> /dev/null; then
      check_pass "Server can be started"
    else
      # Timeout 是预期的（服务器会等待输入）
      check_pass "Server can be started (timeout is expected)"
    fi
  fi
fi

# 8. 检查语法文件
echo ""
echo "8. Checking syntax files..."
if [ -f "syntaxes/karte.tmLanguage.json" ]; then
  check_pass "Syntax definition found"
else
  check_fail "Syntax definition not found"
fi

if [ -f "language-configuration.json" ]; then
  check_pass "Language configuration found"
else
  check_fail "Language configuration not found"
fi

# 9. 检查 VS Code
echo ""
echo "9. Checking VS Code..."
if command -v code &> /dev/null; then
  CODE_VERSION=$(code --version | head -1)
  check_pass "VS Code installed: $CODE_VERSION"
else
  check_warn "VS Code 'code' command not found in PATH"
  echo "   You may need to install it from VS Code: Cmd+Shift+P > 'Shell Command: Install code command in PATH'"
fi

# 总结
echo ""
echo "===================================="
echo "Diagnosis complete!"
echo ""
echo "Next steps:"
echo "1. Open this directory in VS Code: code ."
echo "2. Press F5 to launch extension in debug mode"
echo "3. In the new window, open a .karte file"
echo "4. Check 'View > Output' and select 'Karte Language Server'"
echo ""
echo "If you see issues, check TROUBLESHOOTING.md"
echo ""
