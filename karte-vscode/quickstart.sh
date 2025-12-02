#!/bin/bash
# Karte VS Code 扩展快速启动脚本

set -e

echo "🚀 Karte VS Code Extension Quick Start"
echo "======================================="

# 1. 检查是否在正确的目录
if [ ! -f "package.json" ]; then
  echo "❌ Error: Please run this script from the karte-vscode directory"
  exit 1
fi

# 2. 构建 LSP 服务器
echo ""
echo "📦 Step 1: Building LSP server..."
cd ..
if cargo build -p karte-lsp; then
  echo "✅ LSP server built successfully"
else
  echo "❌ Failed to build LSP server"
  exit 1
fi

# 3. 返回扩展目录
cd karte-vscode

# 4. 安装 npm 依赖
echo ""
echo "📦 Step 2: Installing npm dependencies..."
if npm install; then
  echo "✅ Dependencies installed"
else
  echo "❌ Failed to install dependencies"
  exit 1
fi

# 5. 编译 TypeScript
echo ""
echo "🔨 Step 3: Compiling TypeScript..."
if npm run compile; then
  echo "✅ Extension compiled"
else
  echo "❌ Failed to compile extension"
  exit 1
fi

# 6. 提示用户
echo ""
echo "✅ Setup complete!"
echo ""
echo "Next steps:"
echo "1. Open this directory in VS Code: code ."
echo "2. Press F5 to start the extension in debug mode"
echo "3. Open a .karte file to test the extension"
echo ""
echo "Or package and install:"
echo "  npm run package"
echo "  code --install-extension karte-lang-0.1.0.vsix"
echo ""
