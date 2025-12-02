# Karte VS Code 扩展安装指南

## 前置要求

- Node.js (推荐 v18 或更高版本)
- npm 或 yarn
- Rust 工具链（用于构建 LSP 服务器）
- VS Code

## 安装步骤

### 1. 构建 LSP 服务器

```bash
# 进入 Karte 项目根目录
cd /path/to/karte

# 构建 LSP 服务器（debug 版本，快速编译）
cargo build -p karte-lsp

# 或者构建 release 版本（编译慢但运行快）
cargo build --release -p karte-lsp
```

编译完成后，服务器二进制文件位于：
- Debug 版本: `target/debug/karte-lsp-server`
- Release 版本: `target/release/karte-lsp-server`

### 2. 安装扩展依赖

```bash
# 进入 VS Code 扩展目录
cd karte-vscode

# 安装 npm 依赖
npm install
```

### 3. 编译 TypeScript 扩展代码

```bash
npm run compile
```

### 4. 开发模式测试

在 VS Code 中打开 `karte-vscode` 目录：

```bash
code .
```

然后按 `F5` 启动扩展开发主机。这会打开一个新的 VS Code 窗口，其中已加载 Karte 扩展。

### 5. 打包并安装扩展（可选）

如果想要将扩展安装到日常使用的 VS Code 中：

```bash
# 打包扩展
npm run package

# 安装扩展（会生成 .vsix 文件）
code --install-extension karte-lang-0.1.0.vsix
```

## 配置

### 自动检测 LSP 服务器路径

扩展会自动尝试以下路径查找 LSP 服务器：
1. 用户配置的路径（见下方）
2. `../target/release/karte-lsp-server`（相对于工作区根目录）
3. `../target/debug/karte-lsp-server`（相对于工作区根目录）
4. 系统 PATH 中的 `karte-lsp-server`

### 手动配置 LSP 服务器路径

如果自动检测失败，可以在 VS Code 设置中配置：

打开设置（`Cmd+,` 或 `Ctrl+,`），搜索 "karte"，然后设置：

```json
{
  "karte.lsp.serverPath": "/absolute/path/to/karte-lsp-server"
}
```

或者在项目的 `.vscode/settings.json` 中添加：

```json
{
  "karte.lsp.serverPath": "${workspaceFolder}/target/debug/karte-lsp-server"
}
```

### 启用 LSP 调试日志

如果需要查看 LSP 通信日志：

```json
{
  "karte.lsp.trace.server": "verbose"
}
```

## 测试扩展

### 1. 创建测试文件

创建一个 `.karte` 文件，例如 `test.karte`：

```karte
// 简单的函数定义
fn add(a: number, b: number) -> number {
  a + b
}

// 测试函数调用
let result = add(5, 3);

// 带错误的代码（用于测试错误诊断）
let x = undefined_variable;
```

### 2. 验证功能

- **语法高亮**: 关键字应该有颜色高亮
- **错误诊断**: `undefined_variable` 应该显示红色波浪线
- **代码补全**: 输入 `l` 时应该看到 `let` 的补全建议
- **格式化**: 代码应该自动缩进

### 3. 查看 LSP 日志

如果遇到问题，可以查看 LSP 输出：
1. 打开输出面板（`View` > `Output`）
2. 在下拉菜单中选择 "Karte Language Server"

## 常见问题

### 扩展未启动

1. 检查 LSP 服务器是否存在：
   ```bash
   ls -lh target/debug/karte-lsp-server
   ```

2. 检查 VS Code 输出面板的错误信息

3. 尝试重启 LSP 服务器：
   - 打开命令面板（`Cmd+Shift+P`）
   - 运行 `Karte: Restart Server`

### 找不到 LSP 服务器

确保已经正确构建了服务器，并且路径配置正确。可以手动运行服务器测试：

```bash
/path/to/karte-lsp-server
```

服务器应该启动并等待 LSP 协议输入。按 `Ctrl+C` 退出。

### TypeScript 编译错误

确保已经安装了所有依赖：

```bash
rm -rf node_modules
npm install
npm run compile
```

## 开发工作流

在开发扩展时：

1. 修改 Rust 代码后，重新构建 LSP 服务器：
   ```bash
   cargo build -p karte-lsp
   ```

2. 修改 TypeScript 代码后，重新编译：
   ```bash
   npm run compile
   ```

3. 在扩展开发主机中重启 LSP 服务器（命令面板 > `Karte: Restart Server`）

4. 或者按 `Cmd+R`/`Ctrl+R` 重新加载扩展开发主机

## 卸载

```bash
code --uninstall-extension karte-lang.karte-lang
```

## 支持

如有问题，请在 GitHub 项目中提交 issue。
