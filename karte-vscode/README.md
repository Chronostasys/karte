# Karte Language Support for VS Code

这是 Karte 编程语言的 VS Code 扩展，提供语法高亮、代码补全、错误检查等功能。

## 功能特性

- ✨ 语法高亮
- 🔍 实时错误检查（语法错误、类型错误）
- 💡 智能代码补全
- 🎯 跳转到定义
- 📝 悬停提示

## 安装和使用

### 1. 构建 LSP 服务器

首先需要构建 Karte LSP 服务器：

```bash
cd /path/to/karte
cargo build --release -p karte-lsp
```

服务器二进制文件将生成在 `target/release/karte-lsp-server`。

### 2. 安装扩展依赖

```bash
cd karte-vscode
npm install
```

### 3. 编译扩展

```bash
npm run compile
```

### 4. 安装扩展

在 VS Code 中按 `F5` 启动调试模式，或者打包安装：

```bash
npm run package
code --install-extension karte-lang-0.1.0.vsix
```

### 5. 配置 LSP 服务器路径（可选）

如果 LSP 服务器不在默认路径，可以在 VS Code 设置中配置：

```json
{
  "karte.lsp.serverPath": "/path/to/karte-lsp-server"
}
```

## 开发

- 修改代码后运行 `npm run compile` 重新编译
- 按 `F5` 在 VS Code 扩展开发主机中测试
- 使用 `Karte: Restart Server` 命令重启 LSP 服务器

## 支持的文件扩展名

- `.karte` - Karte 源代码文件

## 语法示例

```karte
// 定义函数
fn factorial(n: number) -> number {
  if n <= 1 {
    1
  } else {
    n * factorial(n - 1)
  }
}

// 使用 let 绑定
let x = 10;
let result = factorial(x);

// 模式匹配
match result {
  0 => "zero",
  n => "non-zero"
}
```

## 问题反馈

如有问题或建议，请在项目仓库提交 issue。

## License

MIT
