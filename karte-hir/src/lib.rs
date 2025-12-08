//! `karte` 编译器的的高级中间表示 (HIR)。
//!
//! HIR (High-level Intermediate Representation) 是紧随在解析（Parsing）阶段之后的一种抽象语法树（AST）的"丰富化"表示。
//! 它的主要职责是进行类型检查和语义分析，将一个纯粹的语法结构（AST）转化为一个带有类型信息和语义含义的结构。
//!
//! HIR 的核心流程是 `type_checker` 模块，它会遍历 AST，解析类型注解，执行类型推断，
//! 并最终为每一个表达式和模式赋予一个确定的类型。这个过程解决了变量绑定、函数重载（如果支持的话）等问题，
//! 并确保程序在类型上是健全的。
//!
//! 在 HIR 阶段完成后，我们就得到了一个经过完整静态分析、类型标注的程序表示。
//! 这种表示是后续优化的基础，并将在 `mir` 阶段被降低（lower）为一个更线性的、基于控制流图的表示。

pub mod ast;
pub mod errors;
pub mod type_checker;
pub mod types;

pub use ast::Parameter;

// 重新导出常用类型，保持API兼容性
pub use ast::*;
pub use type_checker::{
    type_check, type_check_with_context, type_check_with_context_and_maps, ModuleContext,
    TypeChecker,
};
pub use types::{Type, TypeVar};
