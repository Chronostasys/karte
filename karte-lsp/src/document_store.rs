// 文档存储管理
//
// 负责管理 LSP 客户端打开的所有文档，跟踪文档内容和版本

use std::collections::HashMap;
use tower_lsp::lsp_types::Url;

/// 文档信息
#[derive(Debug, Clone)]
pub struct Document {
    /// 文档内容
    pub content: String,
    /// 文档版本（每次修改递增）
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

    /// 打开一个新文档
    pub fn open(&mut self, uri: Url, content: String, version: i32) {
        self.documents.insert(uri, Document::new(content, version));
    }

    /// 更新文档内容
    pub fn update(&mut self, uri: &Url, content: String, version: i32) {
        if let Some(doc) = self.documents.get_mut(uri) {
            doc.content = content;
            doc.version = version;
        }
    }

    /// 关闭文档
    pub fn close(&mut self, uri: &Url) {
        self.documents.remove(uri);
    }

    /// 获取文档内容
    pub fn get(&self, uri: &Url) -> Option<&Document> {
        self.documents.get(uri)
    }

    /// 获取文档内容（可变引用）
    pub fn get_mut(&mut self, uri: &Url) -> Option<&mut Document> {
        self.documents.get_mut(uri)
    }

    /// 检查文档是否存在
    pub fn contains(&self, uri: &Url) -> bool {
        self.documents.contains_key(uri)
    }
}
