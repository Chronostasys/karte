// 文档存储管理
//
// 负责管理 LSP 客户端打开的所有文档，跟踪文档内容和版本

use std::collections::HashMap;
use tower_lsp::lsp_types::Url;

/// 文档信息
#[derive(Debug, Clone)]
pub struct Document {
    pub content: String,
    pub version: i32,
}

impl Document {
    pub fn new(content: String, version: i32) -> Self {
        Self { content, version }
    }
}

/// 文档存储
#[derive(Debug, Default)]
pub struct DocumentStore {
    documents: HashMap<Url, Document>,
}

impl DocumentStore {
    pub fn new() -> Self {
        Self {
            documents: HashMap::new(),
        }
    }

    pub fn open(&mut self, uri: Url, content: String, version: i32) {
        self.documents.insert(uri, Document::new(content, version));
    }

    pub fn update(&mut self, uri: &Url, content: String, version: i32) {
        if let Some(doc) = self.documents.get_mut(uri) {
            doc.content = content;
            doc.version = version;
        }
    }

    pub fn close(&mut self, uri: &Url) {
        self.documents.remove(uri);
    }

    pub fn get(&self, uri: &Url) -> Option<&Document> {
        self.documents.get(uri)
    }

    pub fn get_mut(&mut self, uri: &Url) -> Option<&mut Document> {
        self.documents.get_mut(uri)
    }

    pub fn contains(&self, uri: &Url) -> bool {
        self.documents.contains_key(uri)
    }

    /// 获取第一个文档的源码（用于 document_symbol 等需要源码的操作）
    pub fn first_source(&self) -> Option<&str> {
        self.documents.values().next().map(|d| d.content.as_str())
    }
}
