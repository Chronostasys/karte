# Karte LSP 扩展快速开始指南

## ✅ 你已经完成了什么

根据诊断结果，你的环境已经配置好了！

## 🚀 现在开始测试

### 方法 1: 在调试模式中测试（推荐）

1. **打开扩展目录**
   ```bash
   cd /path/to/karte/karte-vscode
   code .
   ```

2. **启动扩展调试**
   - 按 `F5` 键（或点击 Run > Start Debugging）
   - 这会打开一个新的 VS Code 窗口（扩展开发主机）

3. **在新窗口中打开测试文件**
   - 打开项目中的测试文件：`../examples/lsp_test.karte`
   - 或者创建一个新的 `.karte` 文件

4. **查看输出日志**
   - 按 `Cmd+Shift+U` (macOS) 或 `Ctrl+Shift+U` (Windows/Linux)
   - 在下拉菜单中选择 "Karte Language Server"
   - 你应该看到类似这样的输出：
     ```
     Karte language support is activating...
     Found server at: /path/to/target/release/karte-lsp-server
     Using server: /path/to/target/release/karte-lsp-server
     Starting Karte LSP client...
     ✓ Karte LSP client started successfully
     ```

5. **测试功能**

   在 `.karte` 文件中输入：

   ```karte
   // 测试 1: 正常代码（无错误）
   fn add(a: number, b: number) -> number {
     a + b
   }

   let result = add(5, 3);

   // 测试 2: 错误诊断（应该显示红色波浪线）
   let x = undefined_variable;

   // 测试 3: 代码补全
   // 输入 'l' 应该提示 'let'
   // 输入 'f' 应该提示 'fn'
   ```

   **预期结果：**
   - ✅ `undefined_variable` 下方显示红色波浪线
   - ✅ 悬停在错误上显示错误信息
   - ✅ 输入关键字首字母时显示补全提示

### 方法 2: 打包并安装到 VS Code

如果你想在日常使用的 VS Code 中安装扩展：

```bash
cd karte-vscode

# 1. 打包扩展
npm run package

# 2. 安装扩展
code --install-extension karte-lang-0.1.0.vsix

# 3. 重启 VS Code
```

然后在任何地方打开 `.karte` 文件都会自动激活扩展。

## 🔍 验证功能

### 1. 语法高亮 ✅

打开任何 `.karte` 文件，应该看到：
- 关键字（`let`, `fn`, `if`, `match`）有颜色
- 数字和字符串有颜色
- 注释有不同的颜色

### 2. 实时错误诊断 ✅

创建测试文件 `test_error.karte`：

```karte
let x = some_undefined_variable;
```

应该看到 `some_undefined_variable` 下方有红色波浪线。

### 3. 代码补全 ✅

在 `.karte` 文件中：
- 输入 `l` → 应该提示 `let`
- 输入 `f` → 应该提示 `fn`
- 输入 `m` → 应该提示 `match`

### 4. 查看日志

**在扩展开发主机中：**
- 原窗口的调试控制台会显示详细日志

**在正式安装后：**
- `View` > `Output`
- 选择 "Karte Language Server"

## 🐛 如果遇到问题

### 问题：没有看到错误诊断

1. **检查输出日志**
   - 打开 Output 面板
   - 选择 "Karte Language Server"
   - 查找错误信息

2. **确认服务器正在运行**
   ```bash
   ps aux | grep karte-lsp-server
   ```

3. **重启 LSP 服务器**
   - 命令面板 (`Cmd+Shift+P`)
   - 运行 `Karte: Restart Karte LSP Server`

4. **查看详细故障排查指南**
   ```bash
   cat TROUBLESHOOTING.md
   ```

### 问题：服务器未找到

运行诊断脚本：
```bash
./diagnose.sh
```

如果服务器未构建：
```bash
cd ..
cargo build -p karte-lsp
```

## 📝 测试用例

使用项目中提供的测试文件：

```bash
# 打开测试文件
code ../examples/lsp_test.karte
```

这个文件包含各种 Karte 语言特性的示例。

## 🎯 下一步

### 配置服务器路径（可选）

如果扩展找不到服务器，可以手动配置：

1. 打开 VS Code 设置（`Cmd+,`）
2. 搜索 "karte"
3. 设置 "Karte: Lsp > Server Path"：
   ```
   /absolute/path/to/karte/target/debug/karte-lsp-server
   ```

### 启用详细日志（调试用）

在设置中：
```json
{
  "karte.lsp.trace.server": "verbose"
}
```

### 开发扩展

如果你想修改扩展代码：

1. **修改 TypeScript 代码**
   ```bash
   vim src/extension.ts
   npm run compile
   ```

2. **重新加载扩展**
   - 在扩展开发主机中按 `Cmd+R` 或 `Ctrl+R`

3. **修改 LSP 服务器代码**
   ```bash
   cd ..
   vim karte-lsp/src/backend.rs
   cargo build -p karte-lsp
   ```

4. **重启 LSP 服务器**
   - 命令面板 > `Karte: Restart Karte LSP Server`

## 📚 更多文档

- **安装指南**: `INSTALL.md`
- **故障排查**: `TROUBLESHOOTING.md`
- **实现文档**: `../LSP_IMPLEMENTATION.md`

## ✨ 当前功能状态

- ✅ 语法高亮 - **完全支持**
- ✅ 实时错误诊断 - **完全支持**
  - 词法错误
  - 语法错误
  - 类型错误
- ✅ 代码补全 - **基础支持**
  - 关键字补全
  - 代码片段
- ⚠️  跳转定义 - **框架已就绪，待实现**
- ⚠️  悬停信息 - **框架已就绪，待实现**

## 🎉 成功标志

当一切正常时，你应该看到：

1. ✅ 打开 `.karte` 文件后出现通知："Karte LSP server is running"
2. ✅ 输出面板显示："✓ Karte LSP client started successfully"
3. ✅ 错误的代码有红色波浪线
4. ✅ 输入关键字有代码补全
5. ✅ 关键字、类型、操作符有语法高亮

享受使用 Karte！🚀
