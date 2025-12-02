// Karte LSP 服务器入口点
//
// 该二进制文件启动 Karte LSP 服务器，通过 stdio 与客户端通信

use karte_lsp::Backend;
use tower_lsp::{LspService, Server};

#[tokio::main]
async fn main() {
    // 初始化日志
    env_logger::init();

    // 创建 LSP 服务
    let (service, socket) = LspService::new(Backend::new);

    // 启动服务器，使用 stdio 通信
    Server::new(tokio::io::stdin(), tokio::io::stdout(), socket)
        .serve(service)
        .await;
}
