# Karte VS Code 扩展故障排查指南

## 快速检查清单

### 1. 检查扩展是否激活

打开一个 `.karte` 文件后，检查：

1. **查看输出通道**
   - 按 `Cmd+Shift+U` (macOS) 或 `Ctrl+Shift+U` (Windows/Linux) 打开输出面板
   - 在右上角下拉菜单中选择 "Karte Language Server"
   - 应该看到类似这样的日志：
     ```
     Karte language support is activating...
     Found server at: /path/to/target/debug/karte-lsp-server
     Using server: /path/to/target/debug/karte-lsp-server
     Starting Karte LSP client...
     ✓ Karte LSP client started successfully
     ```

2. **检查状态栏**
   - 应该看到一个通知："Karte LSP server is running"

3. **检查错误诊断**
   - 在 `.karte` 文件中输入错误代码，例如：
     ```karte
     let x = undefined_variable;
     ```
   - 应该看到红色波浪线

### 2. 常见问题

#### 问题：看不到输出日志

**解决方法：**

1. 确保已经打开了输出面板（`View` > `Output`）
2. 在下拉菜单中选择 "Karte Language Server"
3. 如果没有这个选项，说明扩展没有激活：
   - 检查文件扩展名是否为 `.karte`
   - 尝试重新加载窗口（`Cmd+Shift+P` > `Reload Window`）

#### 问题：服务器未找到

**错误信息：**
```
Could not find karte-lsp-server. Please build it or configure the path.
```

**解决方法：**

1. **构建 LSP 服务器**
   ```bash
   cd /path/to/karte
   cargo build -p karte-lsp
   ```

2. **手动配置路径**

   在 VS Code 设置中（`Cmd+,`），搜索 "karte"，设置：
   ```json
   {
     "karte.lsp.serverPath": "/absolute/path/to/karte/target/debug/karte-lsp-server"
   }
   ```

3. **验证服务器可执行**
   ```bash
   ls -lh /path/to/target/debug/karte-lsp-server
   # 应该看到可执行文件

   # 测试运行
   /path/to/target/debug/karte-lsp-server
   # 应该启动并等待输入（按 Ctrl+C 退出）
   ```

#### 问题：服务器启动失败

**错误信息示例：**
```
✗ Failed to start LSP client: spawn ENOENT
```

**可能原因和解决方法：**

1. **服务器二进制不存在**
   - 检查路径是否正确
   - 重新构建服务器

2. **权限问题**
   ```bash
   chmod +x /path/to/karte-lsp-server
   ```

3. **依赖缺失**（macOS）
   - 确保 Rust 工具链正确安装
   - 重新构建服务器

#### 问题：有语法高亮，但没有错误检查

**可能原因：**

语法高亮和 LSP 是两个独立的功能。如果只有语法高亮，说明 LSP 服务器没有成功连接。

**排查步骤：**

1. **检查输出日志**
   - 打开 "Karte Language Server" 输出通道
   - 查找错误信息

2. **验证服务器是否运行**
   ```bash
   ps aux | grep karte-lsp-server
   # 应该看到服务器进程
   ```

3. **重启 LSP 服务器**
   - 命令面板（`Cmd+Shift+P`）
   - 运行 `Karte: Restart Karte LSP Server`

4. **查看详细日志**

   在设置中启用详细日志：
   ```json
   {
     "karte.lsp.trace.server": "verbose"
   }
   ```

   然后重启服务器，查看详细的 LSP 通信日志。

#### 问题：TypeScript 编译错误

**错误示例：**
```
Cannot find module 'vscode' or its corresponding type declarations
```

**解决方法：**

```bash
cd karte-vscode
rm -rf node_modules package-lock.json
npm install
npm run compile
```

### 3. 诊断步骤

#### 步骤 1: 验证环境

```bash
# 检查 Node.js 版本
node --version  # 应该 >= 18.0.0

# 检查 npm
npm --version

# 检查 Rust
cargo --version

# 检查 VS Code
code --version
```

#### 步骤 2: 重新构建所有内容

```bash
# 1. 重新构建 LSP 服务器
cd /path/to/karte
cargo clean -p karte-lsp
cargo build -p karte-lsp

# 2. 重新编译扩展
cd karte-vscode
rm -rf node_modules out
npm install
npm run compile

# 3. 验证文件
ls -lh ../target/debug/karte-lsp-server
ls -lh out/extension.js
```

#### 步骤 3: 测试服务器手动运行

```bash
# 运行服务器
/path/to/target/debug/karte-lsp-server

# 应该看到服务器等待输入
# 不会有错误信息
# 按 Ctrl+C 退出
```

#### 步骤 4: 清理 VS Code 缓存

```bash
# 关闭所有 VS Code 窗口

# 清理缓存（macOS）
rm -rf ~/Library/Application\ Support/Code/Cache/*
rm -rf ~/Library/Application\ Support/Code/CachedData/*

# 重新打开 VS Code
code /path/to/karte
```

### 4. 收集调试信息

如果问题仍然存在，收集以下信息以便诊断：

1. **VS Code 版本**
   ```bash
   code --version
   ```

2. **扩展输出日志**
   - 复制 "Karte Language Server" 输出通道的全部内容

3. **服务器手动运行输出**
   ```bash
   RUST_LOG=debug /path/to/karte-lsp-server 2>&1 | tee server.log
   # 按 Ctrl+C 退出
   # 检查 server.log
   ```

4. **扩展配置**
   ```bash
   # 显示当前配置
   code --list-extensions --show-versions | grep karte
   ```

5. **工作区结构**
   ```bash
   # 显示目录结构
   tree -L 3 /path/to/karte
   ```

### 5. 调试模式

#### 启用扩展调试

1. 在 VS Code 中打开 `karte-vscode` 目录
2. 按 `F5` 启动扩展开发主机
3. 在新窗口中打开 `.karte` 文件
4. 原窗口的调试控制台会显示详细日志

#### 启用服务器详细日志

在项目的 `.vscode/settings.json` 中添加：

```json
{
  "karte.lsp.trace.server": "verbose"
}
```

### 6. 手动测试 LSP 功能

创建测试文件 `test.karte`：

```karte
// 1. 正常代码 - 应该没有错误
fn add(a: number, b: number) -> number {
  a + b
}

let result = add(5, 3);

// 2. 错误代码 - 应该显示红色波浪线
let undefined_test = some_undefined_variable;

// 3. 类型错误 - 应该显示类型错误
// let wrong_type = add("not", "number");
```

**预期结果：**

- ✅ 第一部分：无错误
- ✅ 第二部分：`some_undefined_variable` 下有红色波浪线
- ✅ 悬停在 `add` 上应该显示信息（如果实现了 hover）
- ✅ 输入 `l` 应该提示 `let` 补全

### 7. 已知限制

当前版本的已知限制：

- ✅ 实时错误诊断 - **已实现**
- ✅ 基础代码补全 - **已实现**（仅关键字）
- ⚠️  跳转定义 - **未完全实现**
- ⚠️  悬停信息 - **未完全实现**
- ❌ 代码格式化 - **未实现**
- ❌ 重命名重构 - **未实现**

### 8. 获取帮助

如果以上方法都无法解决问题：

1. 检查 GitHub Issues 是否有类似问题
2. 创建新 Issue 并附上：
   - 操作系统和版本
   - VS Code 版本
   - 扩展输出日志
   - 重现步骤
   - 测试文件内容

## 成功运行的标志

当一切正常工作时，你应该看到：

1. ✅ 输出通道显示 "✓ Karte LSP client started successfully"
2. ✅ 打开 `.karte` 文件有语法高亮
3. ✅ 错误的代码下方有红色波浪线
4. ✅ 输入关键字时有代码补全提示
5. ✅ 状态栏没有错误图标

## 快速修复命令

```bash
# 一键重新构建和测试
cd /path/to/karte
cargo build -p karte-lsp && \
cd karte-vscode && \
npm run compile && \
code .
# 然后按 F5 测试
```
