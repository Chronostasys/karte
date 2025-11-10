pub mod collections;
/// IR 编解码器 - 提供 IR 的文本序列化和反序列化支持
///
/// 这个 crate 提供了核心 trait 定义和运行时支持，
/// 配合 karte-ir-derive 可以自动为 IR 类型生成序列化/反序列化代码。
pub mod display;
pub mod error;
pub mod parse;

// 重新导出主要类型
pub use display::IrDisplay;
pub use error::{ParseError, ParseResult};
pub use parse::{IrParse, ParseContext};

// 重新导出用于生成代码的辅助函数
pub use display::*;
pub use parse::*;
