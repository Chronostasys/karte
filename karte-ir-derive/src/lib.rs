/// Karte IR 自动编解码器 - Proc Macro
///
/// 这个 crate 提供了自动生成 IR Display 和 Parse 实现的 proc macro。
/// 使用方式类似 serde：
///
/// ```rust
/// use karte_ir_derive::IrCodec;
/// #[derive(IrCodec)]
/// enum Value {
///     Number { value: i64 },
///     Variable { name: String },
/// }
/// ```
use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

mod display;
mod parse;
mod spec;
mod utils;

use display::generate_display_impl;
use parse::generate_parse_impl;

/// 自动实现 IrDisplay 和 IrParse trait
#[proc_macro_derive(IrCodec, attributes(ir_codec))]
pub fn derive_ir_codec(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    // Generate Display implementation
    let display_impl = generate_display_impl(&input);

    // Generate Parse implementation
    let parse_impl = generate_parse_impl(&input);

    // Combine both implementations
    let expanded = quote! {
        #display_impl
        #parse_impl
    };

    TokenStream::from(expanded)
}

/// 只实现 IrDisplay trait
#[proc_macro_derive(IrDisplay, attributes(ir_codec))]
pub fn derive_ir_display(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let display_impl = generate_display_impl(&input);
    TokenStream::from(display_impl)
}

/// 只实现 IrParse trait
#[proc_macro_derive(IrParse, attributes(ir_codec))]
pub fn derive_ir_parse(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let parse_impl = generate_parse_impl(&input);
    TokenStream::from(parse_impl)
}
