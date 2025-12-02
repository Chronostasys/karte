// Karte Language Server Protocol 实现
//
// 该模块提供 Karte 语言的 LSP 服务器实现，支持：
// - 语法诊断
// - 类型检查
// - 代码补全
// - 跳转定义
// - 悬停信息

pub mod backend;
pub mod compiler_bridge;
pub mod document_store;

pub use backend::Backend;
pub use compiler_bridge::CompilerBridge;
pub use document_store::DocumentStore;
