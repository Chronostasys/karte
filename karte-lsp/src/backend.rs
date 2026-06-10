// LSP 后端实现
//
// 实现 tower-lsp 的 LanguageServer trait
// 增强版：支持悬停信息、跳转定义、智能补全、文档符号、查找引用

use crate::compiler_bridge::{
    CompilerBridge, KarteCompletionItem, KarteCompletionKind, KarteDiagnosticSeverity,
    KarteSymbolKind, LspDiagnostic,
};
use crate::document_store::DocumentStore;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

pub struct Backend {
    client: Client,
    document_store: Arc<RwLock<DocumentStore>>,
    compiler_bridge: Arc<RwLock<CompilerBridge>>,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            document_store: Arc::new(RwLock::new(DocumentStore::new())),
            compiler_bridge: Arc::new(RwLock::new(CompilerBridge::new())),
        }
    }

    async fn publish_diagnostics(&self, uri: Url, diagnostics: Vec<LspDiagnostic>) {
        let lsp_diagnostics: Vec<Diagnostic> = diagnostics
            .into_iter()
            .map(|diag| Diagnostic {
                range: diag.range,
                severity: Some(match diag.severity {
                    KarteDiagnosticSeverity::Error => DiagnosticSeverity::ERROR,
                    KarteDiagnosticSeverity::Warning => DiagnosticSeverity::WARNING,
                    KarteDiagnosticSeverity::Information => DiagnosticSeverity::INFORMATION,
                    KarteDiagnosticSeverity::Hint => DiagnosticSeverity::HINT,
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

    async fn analyze_document(&self, uri: &Url) {
        let store = self.document_store.read().await;
        if let Some(document) = store.get(uri) {
            let mut bridge = self.compiler_bridge.write().await;
            let diagnostics = bridge.analyze(&document.content);
            drop(store);
            drop(bridge);
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
                    trigger_characters: Some(vec![
                        ".".to_string(),
                        ":".to_string(),
                        " ".to_string(),
                    ]),
                    ..Default::default()
                }),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
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

        {
            let mut store = self.document_store.write().await;
            store.open(uri.clone(), content, version);
        }

        self.analyze_document(&uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = params.text_document.version;

        if let Some(change) = params.content_changes.into_iter().next() {
            {
                let mut store = self.document_store.write().await;
                store.update(&uri, change.text, version);
            }
            self.analyze_document(&uri).await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        self.analyze_document(&params.text_document.uri).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.document_store.write().await.close(&uri);
        self.client.publish_diagnostics(uri, vec![], None).await;
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let position = params.text_document_position.position;
        let bridge = self.compiler_bridge.read().await;
        let completions = bridge.get_completions(position);

        let items: Vec<CompletionItem> = completions
            .into_iter()
            .map(|item| CompletionItem {
                label: item.label,
                kind: Some(match item.kind {
                    KarteCompletionKind::Keyword => CompletionItemKind::KEYWORD,
                    KarteCompletionKind::Function => CompletionItemKind::FUNCTION,
                    KarteCompletionKind::Variable => CompletionItemKind::VARIABLE,
                    KarteCompletionKind::Class => CompletionItemKind::CLASS,
                    KarteCompletionKind::Enum => CompletionItemKind::ENUM,
                    KarteCompletionKind::Struct => CompletionItemKind::STRUCT,
                    KarteCompletionKind::EnumVariant => CompletionItemKind::ENUM_MEMBER,
                    KarteCompletionKind::Module => CompletionItemKind::MODULE,
                    KarteCompletionKind::Field => CompletionItemKind::FIELD,
                    KarteCompletionKind::Snippet => CompletionItemKind::SNIPPET,
                }),
                detail: item.detail,
                insert_text: item.insert_text,
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            })
            .collect();

        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let bridge = self.compiler_bridge.read().await;
        if let Some(span) = bridge.get_definition(position) {
            let store = self.document_store.read().await;
            if let Some(document) = store.get(&uri) {
                let range =
                    crate::compiler_bridge::span_to_range(&document.content, span);
                return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                    uri,
                    range,
                })));
            }
        }

        Ok(None)
    }

    async fn references(
        &self,
        params: ReferenceParams,
    ) -> Result<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        let bridge = self.compiler_bridge.read().await;
        let spans = bridge.find_references(position);

        if spans.is_empty() {
            return Ok(None);
        }

        let store = self.document_store.read().await;
        if let Some(document) = store.get(&uri) {
            let locations: Vec<Location> = spans
                .into_iter()
                .map(|span| Location {
                    uri: uri.clone(),
                    range: crate::compiler_bridge::span_to_range(&document.content, span),
                })
                .collect();
            return Ok(Some(locations));
        }

        Ok(None)
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let position = params.text_document_position_params.position;

        let bridge = self.compiler_bridge.read().await;
        if let Some((info, range)) = bridge.get_hover_info(position) {
            return Ok(Some(Hover {
                contents: HoverContents::Scalar(MarkedString::String(format!(
                    "```karte\n{}\n```",
                    info
                ))),
                range: Some(range),
            }));
        }

        Ok(None)
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = &params.text_document.uri;
        let bridge = self.compiler_bridge.read().await;
        let symbols = bridge.get_document_symbols();

        let store = self.document_store.read().await;
        if let Some(document) = store.get(uri) {
            let source = &document.content;
            let items: Vec<DocumentSymbol> = symbols
                .into_iter()
                .filter(|sym| !matches!(sym.kind, KarteSymbolKind::Parameter | KarteSymbolKind::EnumVariant))
                .map(|sym| {
                    let kind = match sym.kind {
                        KarteSymbolKind::Function => SymbolKind::FUNCTION,
                        KarteSymbolKind::Variable => SymbolKind::VARIABLE,
                        KarteSymbolKind::Parameter => SymbolKind::VARIABLE,
                        KarteSymbolKind::Type => SymbolKind::CLASS,
                        KarteSymbolKind::Enum => SymbolKind::ENUM,
                        KarteSymbolKind::Struct => SymbolKind::STRUCT,
                        KarteSymbolKind::EnumVariant => SymbolKind::ENUM_MEMBER,
                        KarteSymbolKind::Module => SymbolKind::MODULE,
                    };
                    let range = crate::compiler_bridge::span_to_range(source, sym.span);
                    DocumentSymbol {
                        name: sym.name,
                        kind,
                        detail: sym.type_signature,
                        deprecated: None,
                        range,
                        selection_range: range,
                        children: None,
                        tags: None,
                    }
                })
                .collect();

            return Ok(Some(DocumentSymbolResponse::Nested(items)));
        }

        Ok(None)
    }
}
