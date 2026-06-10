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
use std::collections::HashMap;
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
                type_definition_provider: Some(TypeDefinitionProviderCapability::Simple(true)),
                references_provider: Some(OneOf::Left(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
                    retrigger_characters: Some(vec![")".to_string()]),
                    ..Default::default()
                }),
                document_symbol_provider: Some(OneOf::Left(true)),
                workspace_symbol_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Left(true)),
                folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                inlay_hint_provider: Some(OneOf::Left(true)),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(SemanticTokensOptions {
                        work_done_progress_options: WorkDoneProgressOptions::default(),
                        legend: SemanticTokensLegend {
                            token_types: vec![
                                SemanticTokenType::NAMESPACE,    // 0 - module
                                SemanticTokenType::TYPE,         // 1 - type/enum/struct
                                SemanticTokenType::ENUM,         // 2 - enum
                                SemanticTokenType::STRUCT,       // 3 - struct
                                SemanticTokenType::ENUM_MEMBER,  // 4 - enum variant
                                SemanticTokenType::FUNCTION,     // 5 - function
                                SemanticTokenType::VARIABLE,     // 6 - variable
                                SemanticTokenType::PARAMETER,    // 7 - parameter
                                SemanticTokenType::NUMBER,       // 8 - number literal
                                SemanticTokenType::STRING,       // 9 - string literal
                                SemanticTokenType::KEYWORD,      // 10 - keyword
                                SemanticTokenType::OPERATOR,     // 11 - operator
                            ],
                            token_modifiers: vec![],
                        },
                        range: None,
                        full: Some(SemanticTokensFullOptions::Bool(true)),
                    }),
                ),
                document_highlight_provider: Some(OneOf::Left(true)),
                document_formatting_provider: Some(OneOf::Left(true)),
                document_range_formatting_provider: Some(OneOf::Left(true)),
                document_on_type_formatting_provider: Some(DocumentOnTypeFormattingOptions {
                    first_trigger_character: "}".to_string(),
                    more_trigger_character: Some(vec![";".to_string()]),
                }),
                code_lens_provider: Some(CodeLensOptions {
                    resolve_provider: Some(true),
                }),
                document_link_provider: Some(DocumentLinkOptions {
                    resolve_provider: Some(true),
                    work_done_progress_options: WorkDoneProgressOptions {
                        work_done_progress: None,
                    },
                }),
                selection_range_provider: Some(SelectionRangeProviderCapability::Simple(true)),
                call_hierarchy_provider: Some(CallHierarchyServerCapability::Simple(true)),
                inline_value_provider: Some(OneOf::Left(true)),
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

    async fn goto_type_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        // type definition 复用 goto_definition 的逻辑
        // 当用户请求跳转到类型定义时，跳转到变量类型的定义位置
        // 目前跳转到变量的定义处（与 goto_definition 一致）
        self.goto_definition(params).await
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

    async fn signature_help(
        &self,
        params: SignatureHelpParams,
    ) -> Result<Option<SignatureHelp>> {
        let position = params.text_document_position_params.position;

        let bridge = self.compiler_bridge.read().await;
        if let Some(sig_info) = bridge.get_signature_help(position) {
            return Ok(Some(sig_info));
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
                    let selection_range = if let Some(name_span) = sym.name_span {
                        crate::compiler_bridge::span_to_range(source, name_span)
                    } else {
                        range
                    };
                    DocumentSymbol {
                        name: sym.name,
                        kind,
                        detail: sym.type_signature,
                        deprecated: None,
                        range,
                        selection_range,
                        children: None,
                        tags: None,
                    }
                })
                .collect();

            return Ok(Some(DocumentSymbolResponse::Nested(items)));
        }

        Ok(None)
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        let bridge = self.compiler_bridge.read().await;
        let symbols = bridge.get_document_symbols();
        let query = params.query.to_lowercase();

        let results: Vec<SymbolInformation> = symbols
            .iter()
            .filter(|sym| {
                if query.is_empty() {
                    true
                } else {
                    sym.name.to_lowercase().contains(&query)
                }
            })
            .filter(|sym| !matches!(sym.kind, KarteSymbolKind::Parameter))
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
                SymbolInformation {
                    name: sym.name.clone(),
                    kind,
                    tags: None,
                    deprecated: None,
                    location: Location {
                        uri: Url::parse("file:///").unwrap(),
                        range: Range::default(),
                    },
                    container_name: None,
                }
            })
            .collect();

        if results.is_empty() {
            Ok(None)
        } else {
            Ok(Some(results))
        }
    }

    async fn rename(
        &self,
        params: RenameParams,
    ) -> Result<Option<WorkspaceEdit>> {
        let position = params.text_document_position.position;
        let new_name = params.new_name;
        let uri = params.text_document_position.text_document.uri.clone();

        let bridge = self.compiler_bridge.read().await;
        let spans = bridge.find_references(position);

        if spans.is_empty() {
            return Ok(None);
        }

        let store = self.document_store.read().await;
        let Some(document) = store.get(&uri) else {
            return Ok(None);
        };
        let source = &document.content;

        // 收集所有需要重命名的位置
        let mut changes = HashMap::new();
        let edits: Vec<TextEdit> = spans
            .iter()
            .map(|span| {
                let range = crate::compiler_bridge::span_to_range(source, *span);
                TextEdit {
                    range,
                    new_text: new_name.clone(),
                }
            })
            .collect();

        if edits.is_empty() {
            return Ok(None);
        }

        changes.insert(uri, edits);

        Ok(Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }))
    }

    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let store = self.document_store.read().await;
        let Some(document) = store.get(uri) else {
            return Ok(None);
        };
        let source = &document.content;

        let bridge = self.compiler_bridge.read().await;
        let references = bridge.find_references(position);

        if references.is_empty() {
            Ok(None)
        } else {
            let highlights: Vec<DocumentHighlight> = references
                .iter()
                .map(|span| {
                    let range = crate::compiler_bridge::span_to_range(source, *span);
                    DocumentHighlight {
                        range,
                        kind: Some(DocumentHighlightKind::TEXT),
                    }
                })
                .collect();
            Ok(Some(highlights))
        }
    }

    async fn prepare_call_hierarchy(
        &self,
        params: CallHierarchyPrepareParams,
    ) -> Result<Option<Vec<CallHierarchyItem>>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let store = self.document_store.read().await;
        let Some(document) = store.get(uri) else {
            return Ok(None);
        };
        let source = &document.content;

        let bridge = self.compiler_bridge.read().await;

        if let Some(result) = bridge.get_cached_result() {
            for sym in &result.symbols {
                if sym.kind != crate::compiler_bridge::KarteSymbolKind::Function {
                    continue;
                }
                let range = crate::compiler_bridge::span_to_range(source, sym.span);
                if position.line >= range.start.line && position.line <= range.end.line {
                    let name_range = if let Some(name_span) = sym.name_span {
                        crate::compiler_bridge::span_to_range(source, name_span)
                    } else {
                        range.clone()
                    };
                    return Ok(Some(vec![CallHierarchyItem {
                        name: sym.name.clone(),
                        kind: SymbolKind::FUNCTION,
                        tags: None,
                        detail: sym.type_signature.clone(),
                        uri: uri.clone(),
                        range,
                        selection_range: name_range,
                        data: None,
                    }]));
                }
            }
        }

        Ok(None)
    }

    async fn incoming_calls(
        &self,
        params: CallHierarchyIncomingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyIncomingCall>>> {
        let item = params.item;
        let uri = &item.uri;

        let store = self.document_store.read().await;
        let Some(document) = store.get(uri) else {
            return Ok(None);
        };

        let bridge = self.compiler_bridge.read().await;

        let mut incoming_calls = Vec::new();
        if let Some(result) = bridge.get_cached_result() {
            for sym in &result.symbols {
                if sym.kind == crate::compiler_bridge::KarteSymbolKind::Function
                    && sym.name != item.name {
                    let sym_range = crate::compiler_bridge::span_to_range(&document.content, sym.span);
                    let from_range = if let Some(name_span) = sym.name_span {
                        crate::compiler_bridge::span_to_range(&document.content, name_span)
                    } else {
                        sym_range.clone()
                    };
                    incoming_calls.push(CallHierarchyIncomingCall {
                        from: CallHierarchyItem {
                            name: sym.name.clone(),
                            kind: SymbolKind::FUNCTION,
                            tags: None,
                            detail: sym.type_signature.clone(),
                            uri: uri.clone(),
                            range: sym_range,
                            selection_range: from_range,
                            data: None,
                        },
                        from_ranges: vec![],
                    });
                }
            }
        }

        if incoming_calls.is_empty() {
            Ok(None)
        } else {
            Ok(Some(incoming_calls))
        }
    }

    async fn outgoing_calls(
        &self,
        params: CallHierarchyOutgoingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyOutgoingCall>>> {
        let item = params.item;
        let uri = &item.uri;

        let store = self.document_store.read().await;
        let Some(document) = store.get(uri) else {
            return Ok(None);
        };

        let bridge = self.compiler_bridge.read().await;

        let mut outgoing_calls = Vec::new();
        if let Some(result) = bridge.get_cached_result() {
            for sym in &result.symbols {
                if sym.kind == crate::compiler_bridge::KarteSymbolKind::Function
                    && sym.name != item.name {
                    let sym_range = crate::compiler_bridge::span_to_range(&document.content, sym.span);
                    let to_range = if let Some(name_span) = sym.name_span {
                        crate::compiler_bridge::span_to_range(&document.content, name_span)
                    } else {
                        sym_range.clone()
                    };
                    outgoing_calls.push(CallHierarchyOutgoingCall {
                        to: CallHierarchyItem {
                            name: sym.name.clone(),
                            kind: SymbolKind::FUNCTION,
                            tags: None,
                            detail: sym.type_signature.clone(),
                            uri: uri.clone(),
                            range: sym_range,
                            selection_range: to_range,
                            data: None,
                        },
                        from_ranges: vec![],
                    });
                }
            }
        }

        if outgoing_calls.is_empty() {
            Ok(None)
        } else {
            Ok(Some(outgoing_calls))
        }
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let uri = &params.text_document.uri;
        let store = self.document_store.read().await;
        let Some(document) = store.get(uri) else {
            return Ok(None);
        };
        let source = &document.content;

        let mut bridge = self.compiler_bridge.write().await;
        let _ = bridge.analyze(source);
        let analysis = bridge.get_cached_result();

        let mut tokens: Vec<SemanticToken> = Vec::new();
        let mut prev_line: u32 = 0;
        let mut prev_char: u32 = 0;

        // 收集所有 token 信息 (byte_pos, line, col, length, token_type)
        // 使用统一的收集方式避免重叠
        let mut raw_tokens: Vec<(usize, u32, u32, u32, u32)> = Vec::new();

        // 构建 byte position -> (line, col) 映射
        let mut byte_to_pos: std::collections::HashMap<usize, (u32, u32)> = std::collections::HashMap::new();
        let mut current_line: u32 = 0;
        let mut current_col: u32 = 0;
        for (i, ch) in source.char_indices() {
            byte_to_pos.insert(i, (current_line, current_col));
            if ch == '\n' {
                current_line += 1;
                current_col = 0;
            } else {
                current_col += 1;
            }
        }
        byte_to_pos.insert(source.len(), (current_line, current_col));

        // 第一层: 基于 lexer token 的高亮（关键字、数字、字符串、运算符）
        {
            let (lexer_tokens, _) = karte_lexer::tokenize(source);

            for token in &lexer_tokens {
                let token_type_opt: Option<u32> = match &token.token {
                    karte_lexer::Token::Identifier(ident) => {
                        match ident.as_str() {
                            "let" | "match" | "enum" | "struct" | "true" | "false"
                            | "if" | "else" | "while" | "fn" => Some(10), // keyword
                            _ => None, // 其他标识符由符号层处理
                        }
                    }
                    karte_lexer::Token::KwFor | karte_lexer::Token::KwIn
                    | karte_lexer::Token::KwReturn | karte_lexer::Token::KwContinue
                    | karte_lexer::Token::KwBreak | karte_lexer::Token::KwPerform
                    | karte_lexer::Token::KwResume | karte_lexer::Token::KwHandle => Some(10),
                    karte_lexer::Token::Number(_) => Some(8),   // number
                    karte_lexer::Token::StringLiteral(_) => Some(9),  // string
                    karte_lexer::Token::CharLiteral(_) => Some(8),    // char (as number)
                    karte_lexer::Token::Plus | karte_lexer::Token::Minus
                    | karte_lexer::Token::Multiply | karte_lexer::Token::Divide
                    | karte_lexer::Token::Percent | karte_lexer::Token::Equal
                    | karte_lexer::Token::EqualEqual | karte_lexer::Token::NotEqual
                    | karte_lexer::Token::Less | karte_lexer::Token::Greater
                    | karte_lexer::Token::LessEqual | karte_lexer::Token::GreaterEqual
                    | karte_lexer::Token::LogicalAnd | karte_lexer::Token::LogicalOr
                    | karte_lexer::Token::LogicalNot
                    | karte_lexer::Token::Ampersand | karte_lexer::Token::Pipe
                    | karte_lexer::Token::Caret | karte_lexer::Token::Arrow
                    | karte_lexer::Token::FatArrow | karte_lexer::Token::Dot
                    | karte_lexer::Token::DoubleDot | karte_lexer::Token::DoubleColon
                    | karte_lexer::Token::Tilde
                    | karte_lexer::Token::PlusEqual | karte_lexer::Token::MinusEqual
                    | karte_lexer::Token::StarEqual | karte_lexer::Token::SlashEqual
                    | karte_lexer::Token::PercentEqual
                    | karte_lexer::Token::ShiftLeftSym | karte_lexer::Token::ShiftRightSym
                    | karte_lexer::Token::ShiftLeftEqual | karte_lexer::Token::ShiftRightEqual
                    | karte_lexer::Token::PipeEqual | karte_lexer::Token::AmpersandEqual
                    | karte_lexer::Token::CaretEqual => Some(11), // operator
                    _ => None,
                };

                if let Some(token_type) = token_type_opt {
                    let start_pos = byte_to_pos.get(&token.span.start).copied().unwrap_or((0, 0));
                    let end_pos = byte_to_pos.get(&token.span.end).copied().unwrap_or((0, 0));

                    let length = if start_pos.0 == end_pos.0 {
                        end_pos.1.saturating_sub(start_pos.1).max(1)
                    } else {
                        source.lines().nth(start_pos.0 as usize)
                            .map(|line| line.len() as u32 - start_pos.1)
                            .unwrap_or(1)
                    };

                    raw_tokens.push((token.span.start, start_pos.0, start_pos.1, length.max(1), token_type));
                }
            }
        }

        // 第二层: 基于符号的高亮（函数、变量、类型等）——这些会覆盖 token 层
        if let Some(result) = analysis {
            for sym in &result.symbols {
                let range = crate::compiler_bridge::span_to_range(source, sym.span);
                let token_type: u32 = match sym.kind {
                    KarteSymbolKind::Function => 5,
                    KarteSymbolKind::Variable => 6,
                    KarteSymbolKind::Parameter => 7,
                    KarteSymbolKind::Type => 1,
                    KarteSymbolKind::Enum => 2,
                    KarteSymbolKind::Struct => 3,
                    KarteSymbolKind::EnumVariant => 4,
                    KarteSymbolKind::Module => 0,
                };
                let length = if range.end.line == range.start.line {
                    range.end.character.saturating_sub(range.start.character).max(1)
                } else {
                    source.lines().nth(range.start.line as usize)
                        .map(|line| line.len() as u32 - range.start.character)
                        .unwrap_or(1)
                };

                raw_tokens.push((sym.span.start, range.start.line, range.start.character, length, token_type));
            }
        }

        // 排序并去重（符号层覆盖 token 层，相同 byte_pos 保留后面的）
        raw_tokens.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.4.cmp(&b.4)));
        // 去重：相同起始位置只保留最后一个（符号层）
        let mut seen_positions: std::collections::HashSet<usize> = std::collections::HashSet::new();
        let mut deduped_tokens: Vec<(usize, u32, u32, u32, u32)> = Vec::new();
        // 从后往前遍历，保留高优先级的
        for tok in raw_tokens.iter().rev() {
            if !seen_positions.contains(&tok.0) {
                seen_positions.insert(tok.0);
                deduped_tokens.push(*tok);
            }
        }
        deduped_tokens.reverse();

        // 生成 delta 编码的 SemanticToken
        for (byte_pos, line, col, length, token_type) in &deduped_tokens {
            let delta_line = *line - prev_line;
            let delta_start = if delta_line == 0 {
                col.saturating_sub(prev_char)
            } else {
                *col
            };

            tokens.push(SemanticToken {
                delta_line,
                delta_start,
                length: *length,
                token_type: *token_type,
                token_modifiers_bitset: 0,
            });

            prev_line = *line;
            prev_char = *col;
        }

        let result = SemanticTokens {
            result_id: None,
            data: tokens,
        };
        Ok(Some(SemanticTokensResult::Tokens(result)))
    }

    async fn code_action(
        &self,
        params: CodeActionParams,
    ) -> Result<Option<CodeActionResponse>> {
        let uri = &params.text_document.uri;
        let mut actions = Vec::new();

        for diag in &params.context.diagnostics {
            // 检查是否是"未定义的变量"错误，提供拼写建议
            if diag.message.contains("未定义的变量") && diag.message.contains("你是否想输入") {
                // 提取建议 - 格式: "未定义的变量: xxx (你是否想输入 'yyy'?)"
                if let Some(start) = diag.message.rfind("'") {
                    // 找最后一个 ' 之前的建议
                    let before_last_quote = &diag.message[..start];
                    if let Some(second_quote) = before_last_quote.rfind("'") {
                        let suggestion = &diag.message[second_quote + 1..start];
                        if !suggestion.is_empty() {
                            let edit = TextEdit {
                                range: diag.range,
                                new_text: suggestion.to_string(),
                            };
                            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                                title: format!("替换为 '{}'", suggestion),
                                kind: Some(CodeActionKind::QUICKFIX),
                                diagnostics: Some(vec![diag.clone()]),
                                edit: Some(WorkspaceEdit {
                                    changes: Some(vec![(uri.clone(), vec![edit])].into_iter().collect()),
                                    document_changes: None,
                                    change_annotations: None,
                                }),
                                is_preferred: Some(true),
                                disabled: None,
                                data: None,
                                command: None,
                            }));
                        }
                    }
                }
            }

            // 检查是否是"未使用的变量"警告，提供添加下划线前缀的建议
            if diag.message.contains("未使用的变量") {
                if let Some(start) = diag.message.find('`') {
                    if let Some(end) = diag.message[start + 1..].find('`') {
                        let var_name = &diag.message[start + 1..start + 1 + end];
                        let edit = TextEdit {
                            range: diag.range,
                            new_text: format!("_{}", var_name),
                        };
                        actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                            title: format!("重命名为 '_{}'（标记为故意未使用）", var_name),
                            kind: Some(CodeActionKind::QUICKFIX),
                            diagnostics: Some(vec![diag.clone()]),
                            edit: Some(WorkspaceEdit {
                                changes: Some(vec![(uri.clone(), vec![edit])].into_iter().collect()),
                                document_changes: None,
                                change_annotations: None,
                            }),
                            is_preferred: Some(true),
                            disabled: None,
                            data: None,
                            command: None,
                        }));
                    }
                }
            }

            // 检查是否是"非穷尽 match"错误，提供添加通配符分支的建议
            if diag.message.contains("非穷尽") || diag.message.contains("穷尽") {
                actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title: "添加通配符分支 `_ => ()".to_string(),
                    kind: Some(CodeActionKind::QUICKFIX),
                    diagnostics: Some(vec![diag.clone()]),
                    edit: None,
                    is_preferred: Some(false),
                    disabled: None,
                    data: None,
                    command: None,
                }));
            }
        }

        if actions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(actions))
        }
    }

    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
        let uri = &params.text_document.uri;
        let store = self.document_store.read().await;
        let Some(document) = store.get(uri) else {
            return Ok(None);
        };
        let source = &document.content;

        let mut ranges = Vec::new();

        // 基于花括号的折叠
        {
            let mut brace_stack: Vec<(u32, u32)> = Vec::new(); // (line, col)
            let mut line: u32 = 0;
            let mut col: u32 = 0;
            for ch in source.chars() {
                if ch == '{' {
                    brace_stack.push((line, col));
                } else if ch == '}' {
                    if let Some((start_line, start_col)) = brace_stack.pop() {
                        if start_line < line {
                            ranges.push(FoldingRange {
                                start_line,
                                start_character: Some(start_col),
                                end_line: line,
                                end_character: Some(col),
                                kind: Some(FoldingRangeKind::Region),
                                collapsed_text: None,
                            });
                        }
                    }
                }
                if ch == '\n' {
                    line += 1;
                    col = 0;
                } else {
                    col += 1;
                }
            }
        }

        if ranges.is_empty() {
            Ok(None)
        } else {
            Ok(Some(ranges))
        }
    }

    async fn inlay_hint(
        &self,
        params: InlayHintParams,
    ) -> Result<Option<Vec<InlayHint>>> {
        let uri = &params.text_document.uri;
        let store = self.document_store.read().await;
        let Some(document) = store.get(uri) else {
            return Ok(None);
        };
        let source = &document.content;

        let mut bridge = self.compiler_bridge.write().await;
        let _ = bridge.analyze(source);
        let analysis = bridge.get_cached_result();

        let mut hints = Vec::new();

        if let Some(result) = analysis {
            // 构建 byte position -> (line, col) 映射
            let mut byte_to_pos: std::collections::HashMap<usize, (u32, u32)> = std::collections::HashMap::new();
            let mut current_line: u32 = 0;
            let mut current_col: u32 = 0;
            for (i, ch) in source.char_indices() {
                byte_to_pos.insert(i, (current_line, current_col));
                if ch == '\n' {
                    current_line += 1;
                    current_col = 0;
                } else {
                    current_col += 1;
                }
            }

            // 为每个有类型信息的标识符添加 inlay hint
            for (&(start, end), type_str) in &result.identifier_type_strings {
                // 只在视图范围内显示
                if let Some(&(line, col)) = byte_to_pos.get(&start) {
                    if line >= params.range.start.line && line <= params.range.end.line {
                        // 在标识符后面显示类型
                        if let Some(&(_end_line, end_col)) = byte_to_pos.get(&end) {
                            // 跳过 number 和 string 等明显的类型
                            if type_str != "number" && type_str != "string" && type_str != "bool" && type_str != "Unit" {
                                hints.push(InlayHint {
                                    position: Position::new(line, end_col),
                                    label: InlayHintLabel::String(format!(": {}", type_str)),
                                    kind: Some(InlayHintKind::TYPE),
                                    text_edits: None,
                                    tooltip: None,
                                    padding_left: Some(true),
                                    padding_right: None,
                                    data: None,
                                });
                            }
                        }
                    }
                }
            }
        }

        if hints.is_empty() {
            Ok(None)
        } else {
            Ok(Some(hints))
        }
    }

    async fn formatting(
        &self,
        params: DocumentFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        let uri = &params.text_document.uri;
        let store = self.document_store.read().await;
        let Some(document) = store.get(uri) else {
            return Ok(None);
        };
        let source = &document.content;

        let mut edits = Vec::new();
        let mut indent_level: u32 = 0;
        let indent_str = "    "; // 4 spaces

        for (line_num, line) in source.lines().enumerate() {
            let trimmed = line.trim();

            // Skip empty lines
            if trimmed.is_empty() {
                // Normalize empty lines: keep at most one consecutive empty line
                if line_num > 0 && source.lines().nth(line_num - 1).map(|l| l.trim().is_empty()).unwrap_or(false) {
                    // Skip consecutive empty lines
                    edits.push(TextEdit {
                        range: Range::new(
                            Position::new(line_num as u32, 0),
                            Position::new(line_num as u32 + 1, 0),
                        ),
                        new_text: String::new(),
                    });
                }
                continue;
            }

            // Decrease indent for closing braces
            let trimmed_start = trimmed_start_braces(trimmed);
            if trimmed_start {
                if indent_level > 0 {
                    indent_level -= 1;
                }
            }

            // Build formatted line
            let formatted = format!("{}{}", indent_str.repeat(indent_level as usize), trimmed);

            // Check if line needs formatting
            if formatted != line {
                edits.push(TextEdit {
                    range: Range::new(
                        Position::new(line_num as u32, 0),
                        Position::new(line_num as u32, line.len() as u32),
                    ),
                    new_text: formatted,
                });
            }

            // Increase indent for opening braces
            if trimmed_end_braces(trimmed) {
                indent_level += 1;
            }
        }

        if edits.is_empty() {
            Ok(None)
        } else {
            Ok(Some(edits))
        }
    }

    async fn linked_editing_range(
        &self,
        params: LinkedEditingRangeParams,
    ) -> Result<Option<LinkedEditingRanges>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let store = self.document_store.read().await;
        let Some(document) = store.get(uri) else {
            return Ok(None);
        };
        let source = &document.content;

        // 查找匹配的花括号范围
        let offset = crate::compiler_bridge::position_to_offset(source, position);

        // 检查光标是否在花括号附近
        let nearby = source.chars()
            .enumerate()
            .skip(if offset > 2 { offset - 2 } else { 0 })
            .take(5)
            .collect::<Vec<_>>();

        let is_near_brace = nearby.iter().any(|(_, ch)| *ch == '{' || *ch == '}');
        if !is_near_brace {
            return Ok(None);
        }

        // 查找匹配的花括号对
        let mut ranges = Vec::new();

        // 使用栈来匹配花括号
        let mut brace_stack: Vec<usize> = Vec::new();
        let mut current_offset = 0;
        for ch in source.chars() {
            if ch == '{' {
                brace_stack.push(current_offset);
            } else if ch == '}' {
                if let Some(open_pos) = brace_stack.pop() {
                    // 检查光标是否在这对花括号附近
                    if (open_pos >= offset.saturating_sub(2) && open_pos <= offset + 2)
                        || (current_offset >= offset.saturating_sub(2) && current_offset <= offset + 2) {
                        let open_range = crate::compiler_bridge::span_to_range(
                            source,
                            karte_diagnostics::Span::new(open_pos, open_pos + 1),
                        );
                        let close_range = crate::compiler_bridge::span_to_range(
                            source,
                            karte_diagnostics::Span::new(current_offset, current_offset + 1),
                        );
                        ranges.push(open_range);
                        ranges.push(close_range);
                    }
                }
            }
            current_offset += ch.len_utf8();
        }

        if ranges.is_empty() {
            Ok(None)
        } else {
            Ok(Some(LinkedEditingRanges {
                ranges,
                word_pattern: None,
            }))
        }
    }

    async fn on_type_formatting(
        &self,
        params: DocumentOnTypeFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        let trigger = &params.ch;

        // 只在 } 后触发格式化
        if trigger != "}" {
            return Ok(None);
        }

        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        let store = self.document_store.read().await;
        let Some(document) = store.get(uri) else {
            return Ok(None);
        };
        let source = &document.content;

        let offset = crate::compiler_bridge::position_to_offset(source, position);

        // 找到当前行
        let line_start = source[..offset].rfind('\n').map(|p| p + 1).unwrap_or(0);
        let line_end = source[line_start..].find('\n').map(|p| line_start + p).unwrap_or(source.len());
        let line = &source[line_start..line_end];
        let current_indent = line.chars().take_while(|c| *c == ' ').count();
        let trimmed = line.trim();

        if !trimmed.starts_with('}') {
            return Ok(None);
        }

        // 计算匹配 { 的缩进
        let mut brace_count = 1isize;
        let mut target_indent = 0usize;
        for prev_line in source[..line_start].lines().rev() {
            let prev_trimmed = prev_line.trim();
            if prev_trimmed.is_empty() {
                continue;
            }
            for ch in prev_trimmed.chars().rev() {
                if ch == '}' { brace_count += 1; }
                else if ch == '{' { brace_count -= 1; }
            }
            if brace_count <= 0 {
                target_indent = prev_line.chars().take_while(|c| *c == ' ').count();
                break;
            }
        }

        if current_indent != target_indent {
            return Ok(Some(vec![TextEdit {
                range: Range {
                    start: Position { line: position.line, character: 0 },
                    end: Position { line: position.line, character: current_indent as u32 },
                },
                new_text: " ".repeat(target_indent),
            }]));
        }

        Ok(None)
    }

    async fn prepare_type_hierarchy(
        &self,
        params: TypeHierarchyPrepareParams,
    ) -> Result<Option<Vec<TypeHierarchyItem>>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let store = self.document_store.read().await;
        let Some(document) = store.get(uri) else {
            return Ok(None);
        };
        let source = &document.content;

        let bridge = self.compiler_bridge.read().await;

        if let Some(result) = bridge.get_cached_result() {
            for sym in &result.symbols {
                match sym.kind {
                    crate::compiler_bridge::KarteSymbolKind::Struct
                    | crate::compiler_bridge::KarteSymbolKind::Enum => {
                        let range = crate::compiler_bridge::span_to_range(source, sym.span);
                        if position.line >= range.start.line && position.line <= range.end.line {
                            let name_range = if let Some(name_span) = sym.name_span {
                                crate::compiler_bridge::span_to_range(source, name_span)
                            } else {
                                range.clone()
                            };
                            return Ok(Some(vec![TypeHierarchyItem {
                                name: sym.name.clone(),
                                kind: if sym.kind == crate::compiler_bridge::KarteSymbolKind::Struct {
                                    SymbolKind::STRUCT
                                } else {
                                    SymbolKind::ENUM
                                },
                                tags: None,
                                detail: sym.type_signature.clone(),
                                uri: uri.clone(),
                                range,
                                selection_range: name_range,
                                data: None,
                            }]));
                        }
                    }
                    _ => {}
                }
            }
        }

        Ok(None)
    }

    async fn inline_value(
        &self,
        params: InlineValueParams,
    ) -> Result<Option<Vec<InlineValue>>> {
        let uri = &params.text_document.uri;
        let range = &params.range;

        let store = self.document_store.read().await;
        let Some(document) = store.get(uri) else {
            return Ok(None);
        };
        let source = &document.content;

        let bridge = self.compiler_bridge.read().await;
        let mut values = Vec::new();

        if let Some(result) = bridge.get_cached_result() {
            for sym in &result.symbols {
                // 只处理在可见范围内的变量和参数
                let sym_range = crate::compiler_bridge::span_to_range(source, sym.span);
                if sym_range.start.line < range.start.line || sym_range.start.line > range.end.line {
                    continue;
                }

                match sym.kind {
                    crate::compiler_bridge::KarteSymbolKind::Variable
                    | crate::compiler_bridge::KarteSymbolKind::Parameter => {
                        // 在变量定义行的末尾显示类型
                        if let Some(type_str) = &sym.type_signature {
                            let line = source.lines().nth(sym_range.start.line as usize);
                            if let Some(line_text) = line {
                                // 使用 VariableLookup 代替静态文本
                                values.push(InlineValue::Text(InlineValueText {
                                    range: Range {
                                        start: Position {
                                            line: sym_range.start.line,
                                            character: line_text.len() as u32,
                                        },
                                        end: Position {
                                            line: sym_range.start.line,
                                            character: line_text.len() as u32,
                                        },
                                    },
                                    text: format!(": {}", type_str),
                                }));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        if values.is_empty() {
            Ok(None)
        } else {
            Ok(Some(values))
        }
    }

    async fn moniker(
        &self,
        params: MonikerParams,
    ) -> Result<Option<Vec<Moniker>>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let store = self.document_store.read().await;
        let Some(document) = store.get(uri) else {
            return Ok(None);
        };
        let source = &document.content;

        let bridge = self.compiler_bridge.read().await;
        let mut monikers = Vec::new();

        if let Some(result) = bridge.get_cached_result() {
            for sym in &result.symbols {
                let range = crate::compiler_bridge::span_to_range(source, sym.span);
                if position.line >= range.start.line && position.line <= range.end.line {
                    monikers.push(Moniker {
                        scheme: "karte".to_string(),
                        identifier: sym.name.clone(),
                        unique: UniquenessLevel::Document,
                        kind: Some(MonikerKind::Export),
                    });
                }
            }
        }

        if monikers.is_empty() {
            Ok(None)
        } else {
            Ok(Some(monikers))
        }
    }
}

impl Backend {
    async fn code_lens(
        &self,
        params: CodeLensParams,
    ) -> Result<Option<Vec<CodeLens>>> {
        let uri = &params.text_document.uri;
        let store = self.document_store.read().await;
        let Some(document) = store.get(uri) else {
            return Ok(None);
        };
        let source = &document.content;

        let bridge = self.compiler_bridge.read().await;
        let symbols = bridge.get_document_symbols();

        let mut lenses = Vec::new();

        for sym in &symbols {
            // 为函数符号添加类型签名 code lens
            if sym.kind == KarteSymbolKind::Function {
                let range = crate::compiler_bridge::span_to_range(source, sym.span);
                let name = &sym.name;

                // 获取函数类型签名
                let type_str = bridge.get_cached_result()
                    .and_then(|r| r.identifier_type_strings.get(&(sym.span.start, sym.span.end)));
                if let Some(type_str) = type_str {
                    lenses.push(CodeLens {
                        range,
                        command: Some(Command {
                            title: format!("fn {}: {}", name, type_str),
                            command: String::new(),
                            arguments: None,
                        }),
                        data: None,
                    });
                }
            }
        }

        if lenses.is_empty() {
            Ok(None)
        } else {
            Ok(Some(lenses))
        }
    }

    async fn code_lens_resolve(
        &self,
        params: CodeLens,
    ) -> Result<CodeLens> {
        Ok(params)
    }

    async fn document_link(
        &self,
        params: DocumentLinkParams,
    ) -> Result<Option<Vec<DocumentLink>>> {
        let uri = &params.text_document.uri;
        let store = self.document_store.read().await;
        let Some(document) = store.get(uri) else {
            return Ok(None);
        };
        let source = &document.content;

        let mut links = Vec::new();

        // 查找 import 语句中的模块路径，生成 document link
        for (line_num, line) in source.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("import") {
                // 提取模块路径
                if let Some(path_start) = trimmed.find('"') {
                    if let Some(path_end) = trimmed[path_start + 1..].find('"') {
                        let module_path = &trimmed[path_start + 1..path_start + 1 + path_end];
                        let link_range = Range::new(
                            Position::new(line_num as u32, path_start as u32 + 1),
                            Position::new(line_num as u32, path_start as u32 + 1 + path_end as u32),
                        );

                        // 生成文件 URI
                        let file_path = if module_path.contains('.') {
                            module_path.replace('.', "/") + ".karte"
                        } else {
                            module_path.to_string() + ".karte"
                        };

                        let target_uri = Url::parse(&format!("file:///{}", file_path)).ok();

                        if let Some(target) = target_uri {
                            links.push(DocumentLink {
                                range: link_range,
                                target: Some(target),
                                tooltip: Some(format!("跳转到 {}", module_path)),
                                data: None,
                            });
                        }
                    }
                }
            }
        }

        if links.is_empty() {
            Ok(None)
        } else {
            Ok(Some(links))
        }
    }

    async fn document_link_resolve(
        &self,
        params: DocumentLink,
    ) -> Result<DocumentLink> {
        Ok(params)
    }
}

/// Check if line starts with a closing brace
fn trimmed_start_braces(line: &str) -> bool {
    line.starts_with('}') || line.starts_with(')')
}

/// Check if line ends with an opening brace
fn trimmed_end_braces(line: &str) -> bool {
    line.ends_with('{') || line.ends_with('(')
}

impl Backend {
    async fn range_formatting(
        &self,
        params: DocumentRangeFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        let uri = &params.text_document.uri;
        let store = self.document_store.read().await;
        let Some(document) = store.get(uri) else {
            return Ok(None);
        };
        let source = &document.content;
        let range = params.range;

        let mut edits = Vec::new();
        let indent_str = "    ";

        // 计算选中范围内的缩进级别
        let mut indent_level: u32 = 0;
        for (i, line) in source.lines().enumerate() {
            if i >= range.start.line as usize {
                break;
            }
            for ch in line.chars() {
                match ch {
                    '{' | '(' => indent_level += 1,
                    '}' | ')' => { if indent_level > 0 { indent_level -= 1; } }
                    _ => {}
                }
            }
        }

        for (line_num, line) in source.lines().enumerate() {
            let ln = line_num as u32;
            if ln < range.start.line || ln > range.end.line {
                continue;
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let trimmed_start = trimmed.starts_with('}') || trimmed.starts_with(')');
            if trimmed_start {
                if indent_level > 0 {
                    indent_level -= 1;
                }
            }

            let formatted = format!("{}{}", indent_str.repeat(indent_level as usize), trimmed);
            if formatted != line {
                edits.push(TextEdit {
                    range: Range::new(
                        Position::new(ln, 0),
                        Position::new(ln, line.len() as u32),
                    ),
                    new_text: formatted,
                });
            }

            if trimmed.ends_with('{') || trimmed.ends_with('(') {
                indent_level += 1;
            }
        }

        if edits.is_empty() {
            Ok(None)
        } else {
            Ok(Some(edits))
        }
    }
}
