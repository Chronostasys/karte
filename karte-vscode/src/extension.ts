// Karte VS Code 扩展入口点

import * as path from 'path';
import * as fs from 'fs';
import * as vscode from 'vscode';
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  Executable,
} from 'vscode-languageclient/node';

let client: LanguageClient | undefined;
let outputChannel: vscode.OutputChannel;

function findServerPath(): string | null {
  const config = vscode.workspace.getConfiguration('karte');
  let serverPath = config.get<string>('lsp.serverPath', '');

  // 1. 使用用户配置的路径
  if (serverPath && fs.existsSync(serverPath)) {
    outputChannel.appendLine(`Found server at configured path: ${serverPath}`);
    return serverPath;
  }

  // 2. 尝试从工作区根目录查找
  const workspaceRoot = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
  if (workspaceRoot) {
    // 尝试相对路径（假设在 karte 项目中）
    const candidates = [
      path.join(workspaceRoot, 'target/release/karte-lsp-server'),
      path.join(workspaceRoot, 'target/debug/karte-lsp-server'),
      path.join(workspaceRoot, '../target/release/karte-lsp-server'),
      path.join(workspaceRoot, '../target/debug/karte-lsp-server'),
      path.join(workspaceRoot, '../../target/release/karte-lsp-server'),
      path.join(workspaceRoot, '../../target/debug/karte-lsp-server'),
    ];

    for (const candidate of candidates) {
      if (fs.existsSync(candidate)) {
        outputChannel.appendLine(`Found server at: ${candidate}`);
        return candidate;
      }
    }
  }

  // 3. 尝试 PATH 中的命令
  outputChannel.appendLine('Server not found in workspace, will try PATH');
  return 'karte-lsp-server';
}

export function activate(context: vscode.ExtensionContext) {
  // 创建输出通道
  outputChannel = vscode.window.createOutputChannel('Karte Language Server');
  outputChannel.appendLine('Karte language support is activating...');
  outputChannel.show(true);

  // 查找服务器路径
  const serverPath = findServerPath();
  if (!serverPath) {
    const message = 'Could not find karte-lsp-server. Please build it or configure the path.';
    outputChannel.appendLine(`ERROR: ${message}`);
    vscode.window.showErrorMessage(message);
    return;
  }

  outputChannel.appendLine(`Using server: ${serverPath}`);

  // 配置服务器可执行文件
  const executable: Executable = {
    command: serverPath,
    args: [],
    options: {
      env: {
        ...process.env,
        RUST_LOG: 'debug', // 启用 Rust 日志
      },
    },
  };

  const serverOptions: ServerOptions = executable;

  // 配置客户端选项
  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: 'file', language: 'karte' }],
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher('**/*.karte'),
    },
    outputChannel: outputChannel,
  };

  // 创建 Language Client
  client = new LanguageClient(
    'karteLanguageServer',
    'Karte Language Server',
    serverOptions,
    clientOptions
  );

  // 启动客户端
  outputChannel.appendLine('Starting Karte LSP client...');
  client.start().then(
    () => {
      outputChannel.appendLine('✓ Karte LSP client started successfully');
      vscode.window.showInformationMessage('Karte LSP server is running');
    },
    (error) => {
      outputChannel.appendLine(`✗ Failed to start LSP client: ${error}`);
      vscode.window.showErrorMessage(`Failed to start Karte LSP: ${error.message}`);
    }
  );

  // 注册命令：重启 LSP 服务器
  const restartCommand = vscode.commands.registerCommand(
    'karte.restartServer',
    async () => {
      if (client) {
        outputChannel.appendLine('Restarting LSP server...');
        await client.stop();
        await client.start();
        outputChannel.appendLine('✓ LSP server restarted');
        vscode.window.showInformationMessage('Karte LSP server restarted');
      }
    }
  );

  context.subscriptions.push(restartCommand);
  context.subscriptions.push(outputChannel);
}

export function deactivate(): Thenable<void> | undefined {
  if (!client) {
    return undefined;
  }
  return client.stop();
}
