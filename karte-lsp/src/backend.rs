// LSP 后端实现
//
// 实现 tower-lsp 的 LanguageServer trait，处理所有 LSP 请求

use crate::compiler_bridge::{CompilerBridge, DiagnosticSeverity, LspDiagnostic};
use crate::document_store::DocumentStore;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

/// LSP 后端
pub struct Backend {
    /// LSP 客户端
    client: Client,
    /// 文档存储
    document_store: Arc<RwLock<DocumentStore>>,
    /// 编译器桥接
    compiler_bridge: Arc<CompilerBridge>,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            document_store: Arc::new(RwLock::new(DocumentStore::new())),
            compiler_bridge: Arc::new(CompilerBridge::new()),
        }
    }

    /// 发布诊断信息到客户端
    async fn publish_diagnostics(&self, uri: Url, diagnostics: Vec<LspDiagnostic>) {
        let lsp_diagnostics = diagnostics
            .into_iter()
            .map(|diag| Diagnostic {
                range: diag.range,
                severity: Some(match diag.severity {
                    DiagnosticSeverity::Error => tower_lsp::lsp_types::DiagnosticSeverity::ERROR,
                    DiagnosticSeverity::Warning => {
                        tower_lsp::lsp_types::DiagnosticSeverity::WARNING
                    }
                    DiagnosticSeverity::Information => {
                        tower_lsp::lsp_types::DiagnosticSeverity::INFORMATION
                    }
                    DiagnosticSeverity::Hint => tower_lsp::lsp_types::DiagnosticSeverity::HINT,
                }),
                message: diag.message,
                source: Some("karte".to_string()),
                ..Default::default()
            })
            .collect();

        self.client
            .publish_diagnostics(uri, lsp_diagnostics, None)
            .await;
    }

    /// 分析文档并发布诊断
    async fn analyze_document(&self, uri: &Url) {
        let store = self.document_store.read().await;
        if let Some(document) = store.get(uri) {
            let diagnostics = self.compiler_bridge.analyze(&document.content);
            drop(store); // 释放锁
            self.publish_diagnostics(uri.clone(), diagnostics).await;
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::FULL),
                        will_save: None,
                        will_save_wait_until: None,
                        save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
                            include_text: Some(false),
                        })),
                    },
                )),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec![".".to_string(), ":".to_string()]),
                    ..Default::default()
                }),
                definition_provider: Some(OneOf::Left(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "karte-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Karte LSP server initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let content = params.text_document.text;
        let version = params.text_document.version;

        // 存储文档
        {
            let mut store = self.document_store.write().await;
            store.open(uri.clone(), content, version);
        }

        // 分析并发布诊断
        self.analyze_document(&uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = params.text_document.version;

        if let Some(change) = params.content_changes.into_iter().next() {
            // 更新文档内容
            {
                let mut store = self.document_store.write().await;
                store.update(&uri, change.text, version);
            }

            // 分析并发布诊断
            self.analyze_document(&uri).await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri;
        // 保存时重新分析
        self.analyze_document(&uri).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        let mut store = self.document_store.write().await;
        store.close(&uri);

        // 清除诊断
        self.client.publish_diagnostics(uri, vec![], None).await;
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let _uri = params.text_document_position.text_document.uri;

        // TODO: 实现智能代码补全
        // 目前返回一些基本的关键字补全
        let items = vec![
            CompletionItem {
                label: "let".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some("Variable binding".to_string()),
                insert_text: Some("let $1 = $0;".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            },
            CompletionItem {
                label: "fn".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some("Function definition".to_string()),
                insert_text: Some("fn $1($2) -> $3 {\n    $0\n}".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            },
            CompletionItem {
                label: "if".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some("Conditional expression".to_string()),
                insert_text: Some("if $1 {\n    $0\n}".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            },
            CompletionItem {
                label: "while".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some("While loop".to_string()),
                insert_text: Some("while $1 {\n    $0\n}".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            },
            CompletionItem {
                label: "match".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some("Pattern matching".to_string()),
                insert_text: Some("match $1 {\n    $2 => $0\n}".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            },
        ];

        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn goto_definition(
        &self,
        _params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        // TODO: 实现跳转定义功能
        // 需要建立符号表，跟踪变量和函数定义位置
        Ok(None)
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let _position = params.text_document_position_params.position;

        let store = self.document_store.read().await;
        if let Some(_document) = store.get(&uri) {
            // TODO: 实现悬停信息功能
            // 返回当前位置的类型信息、文档等
        }

        Ok(None)
    }
}
