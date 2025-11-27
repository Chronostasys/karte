//! `karte` 编译器的代码生成与执行后端。



pub mod lir_codegen;
pub mod vm;
pub use lir_codegen::*;
