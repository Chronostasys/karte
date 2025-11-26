use crate::spec::TypeSpec;
use crate::utils::{get_field_label, get_field_style, is_arg_field, should_skip_field, FieldStyle};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Ident};

/// 生成 IrParse 实现
pub fn generate_parse_impl(input: &DeriveInput) -> TokenStream {
    let type_spec = TypeSpec::from_derive_input(input);
    let name = &type_spec.name;
    let is_program = type_spec.attrs.is_program;

    let parse_body = match &input.data {
        Data::Enum(data_enum) => generate_enum_parse(name, &data_enum.variants),
        Data::Struct(data_struct) => {
            generate_struct_parse(name, &data_struct.fields, is_program, &input.attrs)
        }
        Data::Union(_) => {
            panic!("Union types are not supported for IrCodec");
        }
    };

    quote! {
        impl karte_ir_codec::IrParse for #name {
            fn parse_ir(input: &str) -> karte_ir_codec::ParseResult<Self> {
                let (_, result) = Self::parse_nom(input)
                    .map_err(|e| karte_ir_codec::ParseError::from(e))?;
                Ok(result)
            }

            fn parse_nom(input: &str) -> nom::IResult<&str, Self> {
                #parse_body
            }
        }
    }
}

/// 生成 enum 的 Parse 实现
fn generate_enum_parse(
    enum_name: &Ident,
    variants: &syn::punctuated::Punctuated<syn::Variant, syn::token::Comma>,
) -> TokenStream {
    // 使用 (parser, priority) 元组，priority 越高越先尝试
    let mut variant_parsers: Vec<(TokenStream, usize)> = Vec::new();

    for variant in variants {
        let variant_name = &variant.ident;
        let variant_name_str = variant_name.to_string();
        let variant_token = crate::utils::get_variant_token(variant);
        let format_style = crate::utils::get_variant_format_style(variant);

        match &variant.fields {
            Fields::Named(fields) => {
                let parsed_field_names: Vec<_> = fields
                    .named
                    .iter()
                    .filter(|f| !should_skip_field(f))
                    .map(|f| f.ident.as_ref().unwrap())
                    .collect();

                let has_skipped_fields = fields.named.iter().any(|f| should_skip_field(f));

                // 检查特殊格式（binop, unop, fieldaccess, infix）
                match format_style {
                    crate::utils::SpecialFormatStyle::FieldAccess => {
                        // fieldaccess 格式: target = object.field
                        let target_field = fields.named.iter().find(|f| {
                            crate::utils::get_field_role(f) == Some(crate::utils::FieldRole::Target)
                        });
                        let object_field = fields.named.iter().find(|f| {
                            crate::utils::get_field_role(f) == Some(crate::utils::FieldRole::Object)
                        });
                        let field_name_field = fields.named.iter().find(|f| {
                            crate::utils::get_field_role(f)
                                == Some(crate::utils::FieldRole::FieldName)
                        });

                        if let (Some(target_f), Some(object_f), Some(field_f)) =
                            (target_field, object_field, field_name_field)
                        {
                            let target_name = target_f.ident.as_ref().unwrap();
                            let object_name = object_f.ident.as_ref().unwrap();
                            let field_name = field_f.ident.as_ref().unwrap();
                            let target_type = &target_f.ty;
                            let object_type = &object_f.ty;
                            let field_type = &field_f.ty;

                            let skipped_field_inits: Vec<_> = fields
                                .named
                                .iter()
                                .filter(|f| should_skip_field(f))
                                .map(|f| {
                                    let field_name = f.ident.as_ref().unwrap();
                                    quote! { #field_name: Default::default() }
                                })
                                .collect();

                            // 生成 fieldaccess parser with lookahead: target = object . field
                            variant_parsers.push((quote! {
                                {
                                    fn fieldaccess_parser(input: &str) -> nom::IResult<&str, #enum_name, nom::error::Error<&str>> {
                                        let original_input = input;

                                        // 解析 target = object
                                        let (input, target_val) = <#target_type>::parse_nom(input)?;
                                        let (input, _) = karte_ir_codec::parse::keyword("=")(input)?;
                                        let (input, object_val) = <#object_type>::parse_nom(input)?;

                                        // 解析 ".field"
                                        let (input, _) = karte_ir_codec::parse::ws(nom::character::complete::char('.'))(input)?;

                                        match <#field_type>::parse_nom(input) {
                                            Ok((input, field_val)) => {
                                                Ok((input, #enum_name::#variant_name {
                                                    #target_name: target_val,
                                                    #object_name: object_val,
                                                    #field_name: field_val,
                                                    #(#skipped_field_inits),*
                                                }))
                                            },
                                            Err(_) => {
                                                Err(nom::Err::Error(nom::error::Error::new(
                                                    original_input,
                                                    nom::error::ErrorKind::Tag
                                                )))
                                            }
                                        }
                                    }
                                    fieldaccess_parser
                                }
                            }, 15)); // 中高优先级
                            continue;
                        }
                    }
                    crate::utils::SpecialFormatStyle::BinOp => {
                        // binop格式: target = left op right
                        // 例如: %0 = %2 * %3

                        let target_field = fields.named.iter().find(|f| {
                            crate::utils::get_field_role(f) == Some(crate::utils::FieldRole::Target)
                        });
                        let left_field = fields.named.iter().find(|f| {
                            crate::utils::get_field_role(f) == Some(crate::utils::FieldRole::Left)
                        });
                        let op_field = fields.named.iter().find(|f| {
                            crate::utils::get_field_role(f) == Some(crate::utils::FieldRole::Op)
                        });
                        let right_field = fields.named.iter().find(|f| {
                            crate::utils::get_field_role(f) == Some(crate::utils::FieldRole::Right)
                        });

                        if let (Some(target_f), Some(left_f), Some(op_f), Some(right_f)) =
                            (target_field, left_field, op_field, right_field)
                        {
                            let target_name = target_f.ident.as_ref().unwrap();
                            let left_name = left_f.ident.as_ref().unwrap();
                            let op_name = op_f.ident.as_ref().unwrap();
                            let right_name = right_f.ident.as_ref().unwrap();
                            let target_type = &target_f.ty;
                            let left_type = &left_f.ty;
                            let op_type = &op_f.ty;
                            let right_type = &right_f.ty;

                            let skipped_field_inits: Vec<_> = fields
                                .named
                                .iter()
                                .filter(|f| should_skip_field(f))
                                .map(|f| {
                                    let field_name = f.ident.as_ref().unwrap();
                                    quote! { #field_name: Default::default() }
                                })
                                .collect();

                            // 生成binop parser with lookahead: target = left op right
                            // 使用lookahead确保后面真的有operator，避免与简单赋值冲突
                            variant_parsers.push((quote! {
                                {
                                    fn binop_parser(input: &str) -> nom::IResult<&str, #enum_name, nom::error::Error<&str>> {
                                        // 保存原始input用于回退
                                        let original_input = input;

                                        // 解析 target = left
                                        let (input, target_val) = <#target_type>::parse_nom(input)?;
                                        let (input, _) = karte_ir_codec::parse::keyword("=")(input)?;
                                        let (input, left_val) = <#left_type>::parse_nom(input)?;

                                        // 尝试解析operator - 如果失败说明不是BinOp
                                        match <#op_type>::parse_nom(input) {
                                            Ok((input, op_val)) => {
                                                // 确认是BinOp，继续解析right operand
                                                let (input, right_val) = <#right_type>::parse_nom(input)?;

                                                Ok((input, #enum_name::#variant_name {
                                                    #target_name: target_val,
                                                    #left_name: left_val,
                                                    #op_name: op_val,
                                                    #right_name: right_val,
                                                    #(#skipped_field_inits),*
                                                }))
                                            },
                                            Err(_) => {
                                                // 不是BinOp格式，返回错误并回退到原始输入位置
                                                // 让其他parser（如Assign）处理
                                                Err(nom::Err::Error(nom::error::Error::new(
                                                    original_input,
                                                    nom::error::ErrorKind::Tag
                                                )))
                                            }
                                        }
                                    }
                                    binop_parser
                                }
                            }, 20)); // 保持高优先级
                            continue;
                        }
                    }
                    crate::utils::SpecialFormatStyle::Infix => {
                        // infix格式: left token right
                        // 例如: %1 = num 2

                        let left_field = fields.named.iter().find(|f| {
                            crate::utils::get_field_role(f) == Some(crate::utils::FieldRole::Left)
                        });
                        let right_field = fields.named.iter().find(|f| {
                            crate::utils::get_field_role(f) == Some(crate::utils::FieldRole::Right)
                        });

                        if let (Some(left_f), Some(right_f), Some(token)) =
                            (left_field, right_field, variant_token.as_ref())
                        {
                            let left_name = left_f.ident.as_ref().unwrap();
                            let right_name = right_f.ident.as_ref().unwrap();
                            let left_type = &left_f.ty;
                            let right_type = &right_f.ty;

                            let skipped_field_inits: Vec<_> = fields
                                .named
                                .iter()
                                .filter(|f| should_skip_field(f))
                                .map(|f| {
                                    let field_name = f.ident.as_ref().unwrap();
                                    quote! { #field_name: Default::default() }
                                })
                                .collect();

                            // 生成infix parser: left token right
                            variant_parsers.push((
                                quote! {
                                    nom::combinator::map(
                                        nom::sequence::tuple((
                                            <#left_type>::parse_nom,
                                            karte_ir_codec::parse::keyword(#token),
                                            <#right_type>::parse_nom
                                        )),
                                        |(left_val, _, right_val)| #enum_name::#variant_name {
                                            #left_name: left_val,
                                            #right_name: right_val,
                                            #(#skipped_field_inits),*
                                        }
                                    )
                                },
                                10,
                            )); // 高优先级
                            continue;
                        }
                    }
                    _ => {} // 其他格式稍后处理
                }

                // 不对特定 token 做特殊处理（如 call）。所有带 token 且标记为 args 的变体
                // 使用统一的前缀解析格式（多参数以逗号分隔）： `token arg1, arg2, arg3`
                // 下面的通用处理会在后面应用。

                // No special-case handling for Call: let the generic token+args parser handle it.
                // Special-case: generate a focused parser for Statement::Call to avoid
                // ambiguity inside comma-separated statement lists. This keeps the
                // parser deterministic for the common `call function: %, args: [..]` form.
                if variant_name_str == "Call" {
                    // find the field idents and types for function and args by ident name
                    let function_field = fields
                        .named
                        .iter()
                        .find(|f| f.ident.as_ref().map(|id| id == "function").unwrap_or(false));
                    let args_field = fields
                        .named
                        .iter()
                        .find(|f| f.ident.as_ref().map(|id| id == "args").unwrap_or(false));

                    if let (Some(func_f), Some(args_f)) = (function_field, args_field) {
                        let func_ident = func_f.ident.as_ref().unwrap();
                        let func_ty = &func_f.ty;
                        let args_ident = args_f.ident.as_ref().unwrap();
                        let args_ty = &args_f.ty; // Vec<Inner>

                        // generate inits for skipped fields and for fields before the suffix (e.g. `target`)
                        let pre_and_skipped_inits: Vec<_> = fields
                            .named
                            .iter()
                            .filter(|f| should_skip_field(f))
                            .map(|f| {
                                let fname = f.ident.as_ref().unwrap();
                                quote! { #fname: Default::default() }
                            })
                            .collect();

                        // For non-skip fields that are not part of the suffix (e.g., `target`), initialize with default
                        let non_suffix_defaults: Vec<_> = fields
                            .named
                            .iter()
                            .filter(|f| {
                                !should_skip_field(f)
                                    && f.ident
                                        .as_ref()
                                        .map(|id| id != func_ident && id != args_ident && id != "target")
                                        .unwrap_or(false)
                            })
                            .map(|f| {
                                let fname = f.ident.as_ref().unwrap();
                                quote! { #fname: Default::default() }
                            })
                            .collect();

                        variant_parsers.push((quote! {
                            {
                                fn call_parser(input: &str) -> nom::IResult<&str, #enum_name, nom::error::Error<&str>> {
                                    let original = input;
                                    let (input, _) = karte_ir_codec::parse::keyword("call")(input)?;

                                    // optional leading `target: <Option<Value>>` (allows "target: none, function: ...").
                                    // We accept an optional trailing comma after the target so that
                                    // the subsequent `function` label can follow.
                                    let (input, opt_target) = nom::combinator::opt(
                                        nom::sequence::preceded(
                                            nom::sequence::preceded(
                                                karte_ir_codec::parse::ws(nom::bytes::complete::tag("target")),
                                                nom::sequence::delimited(
                                                    nom::character::complete::multispace0,
                                                    nom::branch::alt((nom::bytes::complete::tag(":"), nom::bytes::complete::tag("="))),
                                                    nom::character::complete::multispace0,
                                                ),
                                            ),
                                            nom::sequence::terminated(
                                                <Option<Value>>::parse_nom,
                                                nom::combinator::opt(karte_ir_codec::parse::ws(nom::character::complete::char(',')))
                                            )
                                        )
                                    )(input)?;

                                    // function: <value>
                                    let (input, _) = karte_ir_codec::parse::ws(nom::bytes::complete::tag("function"))(input)?;
                                    let (input, _) = nom::sequence::delimited(nom::character::complete::multispace0, nom::branch::alt((nom::bytes::complete::tag(":"), nom::bytes::complete::tag("="))), nom::character::complete::multispace0)(input)?;
                                    let (input, #func_ident) = <#func_ty>::parse_nom(input)?;

                                    // optional ", args: <Vec>" — parse using the Vec<T>::parse_nom which expects '['...']'
                                    let (input, opt_args) = nom::combinator::opt(
                                        nom::sequence::preceded(
                                            karte_ir_codec::parse::ws(nom::character::complete::char(',')),
                                            nom::sequence::preceded(
                                                nom::sequence::preceded(
                                                    karte_ir_codec::parse::ws(nom::bytes::complete::tag("args")),
                                                    nom::sequence::delimited(
                                                        nom::character::complete::multispace0,
                                                        nom::branch::alt((nom::bytes::complete::tag(":"), nom::bytes::complete::tag("="))),
                                                        nom::character::complete::multispace0
                                                    )
                                                ),
                                                <#args_ty>::parse_nom
                                            )
                                        )
                                    )(input)?;

                                    let #args_ident = opt_args.unwrap_or_default();

                                    Ok((input, #enum_name::#variant_name {
                                        #func_ident: #func_ident,
                                        #args_ident: #args_ident,
                                        #(#pre_and_skipped_inits,)*
                                        // parsed optional target (or default None)
                                        target: opt_target.unwrap_or_default(),
                                        #(#non_suffix_defaults),*
                                    }))
                                }
                                call_parser
                            }
                        }, 30));
                        continue;
                    }
                }
                // 如果存在 token 且所有需要解析的字段都被标记为 args，
                // 则将 token + args 作为前缀解析（除非显式指定了 special_format，上面已经处理）
                if let Some(token) = &variant_token {
                    // 收集被标记为 args 的字段（并排除被跳过的）
                    let arg_field_idents: Vec<_> = fields
                        .named
                        .iter()
                        .filter(|f| !should_skip_field(f) && is_arg_field(f))
                        .map(|f| f.ident.as_ref().unwrap())
                        .collect();

                    // 如果 args 非空且在 parsed_field_names 中形成一个连续的后缀（suffix），
                    // 则我们可以解析 token 后面的紧凑参数：
                    // - 先对后缀中的第一个字段进行常规解析
                    // - 后续的字段如果是 Vec<T> 则使用 many0(preceded(',', T::parse_nom))，否则使用 preceded(',', T::parse_nom)
                    // 对后缀之前的字段（如果有）使用默认值初始化（例如 Call.target -> None）
                    if !arg_field_idents.is_empty() {
                        // 找到 parsed_field_names 中第一个属于 args 的索引
                        let mut first_arg_index: Option<usize> = None;
                        for (i, name) in parsed_field_names.iter().enumerate() {
                            if arg_field_idents.contains(name) {
                                first_arg_index = Some(i);
                                break;
                            }
                        }

                        if let Some(start_idx) = first_arg_index {
                            // 检查从 start_idx 到结尾的所有字段是否都为 args
                            let mut is_suffix = true;
                            for name in parsed_field_names.iter().skip(start_idx) {
                                if !arg_field_idents.contains(name) {
                                    is_suffix = false;
                                    break;
                                }
                            }

                            if is_suffix {
                                // 为跳过的字段以及后缀之前的字段生成默认值初始化
                                let skipped_field_inits: Vec<_> = fields
                                    .named
                                    .iter()
                                    .filter(|f| {
                                        should_skip_field(f)
                                            || parsed_field_names
                                                .iter()
                                                .position(|n| *n == f.ident.as_ref().unwrap())
                                                .map_or(false, |idx| idx < start_idx)
                                    })
                                    .map(|f| {
                                        let field_name = f.ident.as_ref().unwrap();
                                        quote! { #field_name: Default::default() }
                                    })
                                    .collect();

                                // 为后缀字段生成解析器列表
                                let mut suffix_parsers: Vec<TokenStream> = Vec::new();
                                let mut suffix_assigns: Vec<TokenStream> = Vec::new();

                                for (i, name) in
                                    parsed_field_names.iter().enumerate().skip(start_idx)
                                {
                                    // 找到对应的 field spec
                                    let field_spec = fields
                                        .named
                                        .iter()
                                        .find(|f| {
                                            f.ident.as_ref().map(|id| id == *name).unwrap_or(false)
                                        })
                                        .unwrap();
                                    let field_ty = &field_spec.ty;
                                    let ident = name;

                                    // 判断是否为 Vec<T>
                                    let is_vec = match field_ty {
                                        syn::Type::Path(tp) => tp
                                            .path
                                            .segments
                                            .last()
                                            .map(|seg| seg.ident == "Vec")
                                            .unwrap_or(false),
                                        _ => false,
                                    };

                                    if i == start_idx {
                                        // 第一个后缀字段：允许有或没有前导逗号；也接受带标签的形式 label: value；如果是 Vec<T> 也接受方括号列表或 label: [ ... ]
                                        // 提取字段标签
                                        let field_label = crate::utils::get_field_label(field_spec)
                                            .map(|s| s.to_string())
                                            .unwrap_or_else(|| name.to_string());

                                        if is_vec {
                                            // 提取 Vec 的内层类型 T
                                            let inner_ty = match field_ty {
                                                syn::Type::Path(tp) => {
                                                    let last = tp.path.segments.last().unwrap();
                                                    if let syn::PathArguments::AngleBracketed(ab) =
                                                        &last.arguments
                                                    {
                                                        if let Some(syn::GenericArgument::Type(t)) =
                                                            ab.args.first()
                                                        {
                                                            t.clone()
                                                        } else {
                                                            panic!("Unsupported Vec inner type")
                                                        }
                                                    } else {
                                                        panic!("Unsupported Vec type args")
                                                    }
                                                }
                                                _ => {
                                                    panic!("Unsupported Vec inner type extraction")
                                                }
                                            };

                                            // 支持三类输入：带前导逗号的 label:[...], 直接 label:[...], 或 inline 列表 / 多个逗号分隔项
                                            // If there are multiple suffix fields, require labelled/bracketed form for the first field
                                            // to avoid partial consumption (e.g., parsing "function: %4" and leaving ", args: [...]" unconsumed).
                                            // Only accept labelled bracketed list for Vec suffix fields to avoid ambiguities
                                            suffix_parsers.push(quote! {
                                                nom::branch::alt((
                                                    // ", label: [ a, b ]"
                                                    nom::sequence::preceded(
                                                        karte_ir_codec::parse::ws(nom::character::complete::char(',')),
                                                        nom::sequence::preceded(
                                                            nom::sequence::preceded(
                                                                karte_ir_codec::parse::ws(nom::bytes::complete::tag(#field_label)),
                                                                nom::sequence::delimited(
                                                                    nom::character::complete::multispace0,
                                                                    nom::branch::alt((nom::bytes::complete::tag(":"), nom::bytes::complete::tag("="))),
                                                                    nom::character::complete::multispace0
                                                                )
                                                            ),
                                                            nom::sequence::delimited(
                                                                karte_ir_codec::parse::ws(nom::character::complete::char('[')),
                                                                nom::multi::separated_list0(
                                                                    nom::sequence::preceded(karte_ir_codec::parse::ws(nom::character::complete::char(',')), <#inner_ty>::parse_nom),
                                                                    <#inner_ty>::parse_nom
                                                                ),
                                                                karte_ir_codec::parse::ws(nom::character::complete::char(']'))
                                                            )
                                                        )
                                                    ),
                                                    // "label: [ a, b ]"
                                                    nom::sequence::preceded(
                                                        nom::sequence::preceded(
                                                            karte_ir_codec::parse::ws(nom::bytes::complete::tag(#field_label)),
                                                            nom::sequence::delimited(
                                                                nom::character::complete::multispace0,
                                                                nom::branch::alt((nom::bytes::complete::tag(":"), nom::bytes::complete::tag("="))),
                                                                nom::character::complete::multispace0
                                                            )
                                                        ),
                                                        nom::sequence::delimited(
                                                            karte_ir_codec::parse::ws(nom::character::complete::char('[')),
                                                            nom::multi::separated_list0(
                                                                nom::sequence::preceded(karte_ir_codec::parse::ws(nom::character::complete::char(',')), <#inner_ty>::parse_nom),
                                                                <#inner_ty>::parse_nom
                                                            ),
                                                            karte_ir_codec::parse::ws(nom::character::complete::char(']'))
                                                        )
                                                    )
                                                ))
                                            });
                                            suffix_assigns.push(quote! { #ident });
                                        } else {
                                            // 非 Vec：接受带标签的 "label: value" 或者 可能带前导逗号的 value
                                            // If there are multiple suffix fields, require a labelled form for the first suffix
                                            // to avoid partial consumption of subsequent labelled fields.
                                            // Only accept labelled forms (with ':' or '=') for suffix fields
                                            suffix_parsers.push(quote! {
                                                nom::branch::alt((
                                                    // ", label: value"
                                                    nom::sequence::preceded(
                                                        karte_ir_codec::parse::ws(nom::character::complete::char(',')),
                                                        nom::sequence::preceded(
                                                            nom::sequence::preceded(
                                                                karte_ir_codec::parse::ws(nom::bytes::complete::tag(#field_label)),
                                                                nom::sequence::delimited(
                                                                    nom::character::complete::multispace0,
                                                                    nom::branch::alt((nom::bytes::complete::tag(":"), nom::bytes::complete::tag("="))),
                                                                    nom::character::complete::multispace0
                                                                )
                                                            ),
                                                            <#field_ty>::parse_nom
                                                        )
                                                    ),
                                                    // "label: value" (without leading comma)
                                                    nom::sequence::preceded(
                                                        nom::sequence::preceded(
                                                            karte_ir_codec::parse::ws(nom::bytes::complete::tag(#field_label)),
                                                            nom::sequence::delimited(
                                                                nom::character::complete::multispace0,
                                                                nom::branch::alt((nom::bytes::complete::tag(":"), nom::bytes::complete::tag("="))),
                                                                nom::character::complete::multispace0
                                                            )
                                                        ),
                                                        <#field_ty>::parse_nom
                                                    )
                                                ))
                                            });
                                            suffix_assigns.push(quote! { #ident });
                                        }
                                    } else {
                                        // 后续后缀字段：通常以逗号分隔。也接受带标签形式 label: value 或 label: [ .. ]
                                        let field_label = crate::utils::get_field_label(field_spec)
                                            .map(|s| s.to_string())
                                            .unwrap_or_else(|| name.to_string());
                                        if is_vec {
                                            let inner_ty = match field_ty {
                                                syn::Type::Path(tp) => {
                                                    let last = tp.path.segments.last().unwrap();
                                                    if let syn::PathArguments::AngleBracketed(ab) =
                                                        &last.arguments
                                                    {
                                                        if let Some(syn::GenericArgument::Type(t)) =
                                                            ab.args.first()
                                                        {
                                                            t.clone()
                                                        } else {
                                                            panic!("Unsupported Vec inner type")
                                                        }
                                                    } else {
                                                        panic!("Unsupported Vec type args")
                                                    }
                                                }
                                                _ => {
                                                    panic!("Unsupported Vec inner type extraction")
                                                }
                                            };

                                            suffix_parsers.push(quote! {
                                                nom::branch::alt((
                                                    // ", label: [ ... ]"
                                                    nom::sequence::preceded(
                                                        karte_ir_codec::parse::ws(nom::character::complete::char(',')),
                                                        nom::sequence::preceded(
                                                            nom::sequence::preceded(
                                                                karte_ir_codec::parse::ws(nom::bytes::complete::tag(#field_label)),
                                                                nom::sequence::delimited(
                                                                    nom::character::complete::multispace0,
                                                                    nom::branch::alt((nom::bytes::complete::tag(":"), nom::bytes::complete::tag("="))),
                                                                    nom::character::complete::multispace0
                                                                )
                                                            ),
                                                            nom::sequence::delimited(
                                                                karte_ir_codec::parse::ws(nom::character::complete::char('[')),
                                                                nom::multi::separated_list0(
                                                                    nom::sequence::preceded(karte_ir_codec::parse::ws(nom::character::complete::char(',')), <#inner_ty>::parse_nom),
                                                                    <#inner_ty>::parse_nom
                                                                ),
                                                                karte_ir_codec::parse::ws(nom::character::complete::char(']'))
                                                            )
                                                        )
                                                    ),
                                                    // 逗号分隔项: ", value"
                                                    nom::multi::many0(
                                                        nom::sequence::preceded(
                                                            karte_ir_codec::parse::ws(nom::character::complete::char(',')),
                                                            <#inner_ty>::parse_nom
                                                        )
                                                    )
                                                ))
                                            });
                                            suffix_assigns.push(quote! { #ident });
                                        } else {
                                            suffix_parsers.push(quote! {
                                                nom::branch::alt((
                                                    nom::sequence::preceded(
                                                        karte_ir_codec::parse::ws(nom::character::complete::char(',')),
                                                        nom::branch::alt((
                                                            nom::sequence::preceded(
                                                                nom::sequence::preceded(
                                                                    karte_ir_codec::parse::ws(nom::bytes::complete::tag(#field_label)),
                                                                    nom::sequence::delimited(
                                                                        nom::character::complete::multispace0,
                                                                        nom::branch::alt((nom::bytes::complete::tag(":"), nom::bytes::complete::tag("="))),
                                                                        nom::character::complete::multispace0
                                                                    )
                                                                ),
                                                                <#field_ty>::parse_nom
                                                            ),
                                                            <#field_ty>::parse_nom
                                                        ))
                                                    ),
                                                    nom::sequence::preceded(
                                                        nom::sequence::preceded(
                                                            karte_ir_codec::parse::ws(nom::bytes::complete::tag(#field_label)),
                                                            nom::sequence::delimited(
                                                                nom::character::complete::multispace0,
                                                                nom::branch::alt((nom::bytes::complete::tag(":"), nom::bytes::complete::tag("="))),
                                                                nom::character::complete::multispace0
                                                            )
                                                        ),
                                                        <#field_ty>::parse_nom
                                                    )
                                                ))
                                            });
                                            suffix_assigns.push(quote! { #ident });
                                        }
                                    }
                                }

                                // 构造解析器：keyword(#token) 后跟 suffix_parsers 的 tuple（或单个parser）
                                if suffix_parsers.len() == 1 {
                                    let single = &suffix_parsers[0];
                                    let assign = &suffix_assigns[0];
                                    variant_parsers.push((quote! {
                                        nom::combinator::map(
                                            nom::sequence::preceded(
                                                karte_ir_codec::parse::keyword(#token),
                                                #single
                                            ),
                                            |#assign| #enum_name::#variant_name { #assign: #assign, #(#skipped_field_inits),* }
                                        )
                                    }, 5));
                                } else {
                                    variant_parsers.push((quote! {
                                        nom::combinator::map(
                                            nom::sequence::preceded(
                                                karte_ir_codec::parse::keyword(#token),
                                                nom::sequence::tuple((#(#suffix_parsers,)*))
                                            ),
                                            |(#(#suffix_assigns,)*)| #enum_name::#variant_name { #(#suffix_assigns,)* #(#skipped_field_inits),* }
                                        )
                                    }, 5));
                                }

                                continue;
                            }
                        }
                    }
                }

                if parsed_field_names.is_empty() && !has_skipped_fields {
                    // 无字段的命名变体
                    if let Some(token) = variant_token {
                        // 有token: 解析token
                        variant_parsers.push((
                            quote! {
                                nom::combinator::map(
                                    karte_ir_codec::parse::keyword(#token),
                                    |_| #enum_name::#variant_name {}
                                )
                            },
                            0,
                        ));
                    } else {
                        // 无token: 解析变体名
                        variant_parsers.push((
                            quote! {
                                nom::combinator::map(
                                    karte_ir_codec::parse::keyword(#variant_name_str),
                                    |_| #enum_name::#variant_name {}
                                )
                            },
                            0,
                        ));
                    }
                } else if parsed_field_names.len() == 1 {
                    // 单个需解析字段（可能有跳过字段）
                    let field = parsed_field_names[0];
                    let field_spec = fields
                        .named
                        .iter()
                        .find(|f| f.ident.as_ref() == Some(field))
                        .unwrap();
                    let field_type = &field_spec.ty;

                    // 检查是否为 args 字段
                    let is_arg = is_arg_field(field_spec);

                    // 为跳过的字段生成默认值初始化
                    let skipped_field_inits: Vec<_> = fields
                        .named
                        .iter()
                        .filter(|f| should_skip_field(f))
                        .map(|f| {
                            let field_name = f.ident.as_ref().unwrap();
                            quote! { #field_name: Default::default() }
                        })
                        .collect();

                    if variant_token.is_none() && is_arg {
                        // 无token + args字段: 直接解析字段值(如 Temp { id: TempId })
                        variant_parsers.push((
                            quote! {
                                nom::combinator::map(
                                    <#field_type>::parse_nom,
                                    |value| #enum_name::#variant_name {
                                        #field: value,
                                        #(#skipped_field_inits),*
                                    }
                                )
                            },
                            0,
                        ));
                    } else if let Some(token) = variant_token {
                        // 有token: 解析 "token value" 或带标签的 "token label: value" 格式(如 "ret %0" 或 "var name: x")
                        let field_label = crate::utils::get_field_label(field_spec)
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| field.to_string());
                        variant_parsers.push((quote! {
                            nom::combinator::map(
                                nom::sequence::preceded(
                                    karte_ir_codec::parse::keyword(#token),
                                    nom::branch::alt((
                                        nom::sequence::preceded(
                                            nom::sequence::preceded(
                                                karte_ir_codec::parse::ws(nom::bytes::complete::tag(#field_label)),
                                                nom::sequence::delimited(
                                                    nom::character::complete::multispace0,
                                                    nom::branch::alt((nom::bytes::complete::tag(":"), nom::bytes::complete::tag("="))),
                                                    nom::character::complete::multispace0
                                                )
                                            ),
                                            <#field_type>::parse_nom
                                        ),
                                        <#field_type>::parse_nom
                                    ))
                                ),
                                |value| #enum_name::#variant_name {
                                    #field: value,
                                    #(#skipped_field_inits),*
                                }
                            )
                        }, 0));
                    } else {
                        // 无token, 非args: VariantName(value) 或 VariantName
                        variant_parsers.push((quote! {
                            nom::combinator::map(
                                nom::sequence::preceded(
                                    karte_ir_codec::parse::keyword(#variant_name_str),
                                    nom::combinator::opt(karte_ir_codec::parse::parens(<#field_type>::parse_nom))
                                ),
                                |opt_value| #enum_name::#variant_name {
                                    #field: opt_value.unwrap_or_default(),
                                    #(#skipped_field_inits),*
                                }
                            )
                        }, 0));
                    }
                } else {
                    // 多字段或有跳过字段: VariantName { field1 = value1, field2 = value2 }
                    // 使用 token 或变体名
                    let token_or_name = variant_token.as_ref().unwrap_or(&variant_name_str);

                    let field_parsers: Vec<_> = fields.named.iter()
                        .filter(|f| !should_skip_field(f))
                        .enumerate()
                        .map(|(index, f)| {
                            let field_name = f.ident.as_ref().unwrap();
                            let field_name_str = field_name.to_string();
                            let field_type = &f.ty;

                            let prefix = if index == 0 {
                                quote! {
                                    nom::sequence::tuple((
                                        karte_ir_codec::parse::ws(nom::character::complete::multispace0),
                                        karte_ir_codec::parse::keyword(#field_name_str),
                                        karte_ir_codec::parse::ws(nom::bytes::complete::tag("=")),
                                    ))
                                }
                            } else {
                                quote! {
                                    nom::sequence::tuple((
                                        karte_ir_codec::parse::ws(nom::character::complete::char(',')),
                                        karte_ir_codec::parse::keyword(#field_name_str),
                                        karte_ir_codec::parse::ws(nom::bytes::complete::tag("=")),
                                    ))
                                }
                            };

                            quote! {
                                nom::sequence::preceded(
                                    #prefix,
                                    <#field_type>::parse_nom
                                )
                            }
                        })
                        .collect();

                    // 为所有字段（包括跳过的）生成初始化
                    let all_field_inits: Vec<_> = fields
                        .named
                        .iter()
                        .map(|f| {
                            let field_name = f.ident.as_ref().unwrap();
                            if should_skip_field(f) {
                                quote! { #field_name: Default::default() }
                            } else {
                                quote! { #field_name }
                            }
                        })
                        .collect();

                    variant_parsers.push((quote! {
                        nom::combinator::map(
                            nom::sequence::delimited(
                                nom::sequence::pair(
                                    karte_ir_codec::parse::keyword(#token_or_name),
                                    karte_ir_codec::parse::ws(nom::character::complete::char('{'))
                                ),
                                nom::sequence::tuple((#(#field_parsers,)*)),
                                karte_ir_codec::parse::ws(nom::character::complete::char('}'))
                            ),
                            |(#(#parsed_field_names,)*)| #enum_name::#variant_name { #(#all_field_inits,)* }
                        )
                    }, 0));
                }
            }
            Fields::Unnamed(fields) => {
                let field_count = fields.unnamed.len();

                if field_count == 0 {
                    // 使用 token 属性值（如果有）或变体名称
                    let token_or_name = variant_token.as_ref().unwrap_or(&variant_name_str);
                    variant_parsers.push((
                        quote! {
                            nom::combinator::map(
                                karte_ir_codec::parse::keyword(#token_or_name),
                                |_| #enum_name::#variant_name
                            )
                        },
                        0,
                    ));
                } else if field_count == 1 {
                    let field_type = &fields.unnamed.first().unwrap().ty;

                    variant_parsers.push((
                        quote! {
                            nom::combinator::map(
                                nom::sequence::preceded(
                                    karte_ir_codec::parse::keyword(#variant_name_str),
                                    karte_ir_codec::parse::parens(<#field_type>::parse_nom)
                                ),
                                |value| #enum_name::#variant_name(value)
                            )
                        },
                        0,
                    ));
                } else {
                    let field_types: Vec<_> = fields.unnamed.iter().map(|f| &f.ty).collect();
                    let field_names: Vec<Ident> = (0..field_count)
                        .map(|i| {
                            syn::Ident::new(&format!("__{}", i), proc_macro2::Span::call_site())
                        })
                        .collect();

                    variant_parsers.push((
                        quote! {
                            nom::combinator::map(
                                nom::sequence::preceded(
                                    karte_ir_codec::parse::keyword(#variant_name_str),
                                    karte_ir_codec::parse::parens(
                                        nom::sequence::tuple((
                                            #(<#field_types>::parse_nom,)*
                                        ))
                                    )
                                ),
                                |(#(#field_names,)*)| #enum_name::#variant_name(#(#field_names,)*)
                            )
                        },
                        0,
                    ));
                }
            }
            Fields::Unit => {
                // Unit 变体也需要检查 token 属性
                let token_or_name = variant_token.as_ref().unwrap_or(&variant_name_str);
                variant_parsers.push((
                    quote! {
                        nom::combinator::map(
                            karte_ir_codec::parse::keyword(#token_or_name),
                            |_| #enum_name::#variant_name
                        )
                    },
                    0,
                ));
            }
        }
    }

    // 按 priority 降序排序 (priority 越高越先尝试)
    variant_parsers.sort_by(|a, b| b.1.cmp(&a.1));

    // 提取出 parser (丢弃 priority)
    let sorted_parsers: Vec<_> = variant_parsers
        .into_iter()
        .map(|(parser, _)| parser)
        .collect();

    // nom::branch::alt 最多支持 21 个变体，需要分组处理大型 enum
    const MAX_ALT_SIZE: usize = 20; // 保守使用 20

    if sorted_parsers.len() <= MAX_ALT_SIZE {
        // 小型 enum，直接使用 alt
        quote! {
            nom::branch::alt((
                #(#sorted_parsers,)*
            ))(input)
        }
    } else {
        // 大型 enum，分组处理
        let chunks: Vec<Vec<_>> = sorted_parsers
            .chunks(MAX_ALT_SIZE)
            .map(|chunk| chunk.to_vec())
            .collect();

        let alt_groups: Vec<_> = chunks
            .iter()
            .map(|chunk| {
                quote! {
                    nom::branch::alt((
                        #(#chunk,)*
                    ))
                }
            })
            .collect();

        quote! {
            nom::branch::alt((
                #(#alt_groups,)*
            ))(input)
        }
    }
}

/// 生成 struct 的 Parse 实现
fn generate_struct_parse(
    struct_name: &Ident,
    fields: &Fields,
    is_program: bool,
    attrs: &[syn::Attribute],
) -> TokenStream {
    let struct_name_str = struct_name.to_string();

    // 获取 struct 级别的 token 属性
    let struct_token = crate::utils::parse_attributes(attrs).token;

    match fields {
        Fields::Named(fields) => {
            let field_parsers: Vec<_> = fields.named.iter()
                .filter(|f| !should_skip_field(f))
                .map(|f| {
                    let field_name = f.ident.as_ref().unwrap();
                    let field_name_str = field_name.to_string();
                    let field_type = &f.ty;
                    let field_style = get_field_style(f);
                    let field_label = get_field_label(f).unwrap_or_else(|| field_name_str.clone());

                    match field_style {
                        FieldStyle::Body | FieldStyle::NewlineItems => {
                            quote! {
                                karte_ir_codec::parse::body_field(#field_label, <#field_type>::parse_nom)
                            }
                        }
                        _ => {
                            quote! {
                                nom::sequence::preceded(
                                    nom::sequence::delimited(
                                        karte_ir_codec::parse::keyword(#field_label),
                                        nom::sequence::delimited(
                                            nom::character::complete::multispace0,
                                            nom::bytes::complete::tag(":"),
                                            nom::character::complete::multispace0
                                        ),
                                        nom::combinator::success(())
                                    ),
                                    <#field_type>::parse_nom
                                )
                            }
                        }
                    }
                })
                .collect();

            let parsed_field_names: Vec<_> = fields
                .named
                .iter()
                .filter(|f| !should_skip_field(f))
                .map(|f| f.ident.as_ref().unwrap())
                .collect();

            // 为所有字段（包括跳过的）生成初始化
            let all_field_inits: Vec<_> = fields
                .named
                .iter()
                .map(|f| {
                    let field_name = f.ident.as_ref().unwrap();
                    if should_skip_field(f) {
                        quote! { #field_name: Default::default() }
                    } else {
                        quote! { #field_name }
                    }
                })
                .collect();

            // If is_program is true, parse fields directly without expecting struct name
            if is_program {
                quote! {
                    nom::combinator::map(
                        nom::sequence::tuple((#(#field_parsers,)*)),
                        |(#(#parsed_field_names,)*)| #struct_name { #(#all_field_inits,)* }
                    )(input)
                }
            } else {
                // Normal struct parsing: expect struct name followed by fields in braces
                quote! {
                    nom::combinator::map(
                        nom::sequence::preceded(
                            karte_ir_codec::parse::ws(karte_ir_codec::parse::keyword(#struct_name_str)),
                            nom::sequence::tuple((#(#field_parsers,)*))
                        ),
                        |(#(#parsed_field_names,)*)| #struct_name { #(#all_field_inits,)* }
                    )(input)
                }
            }
        }
        Fields::Unnamed(fields) => {
            let field_types: Vec<_> = fields.unnamed.iter().map(|f| &f.ty).collect();
            let field_count = field_types.len();
            let field_names: Vec<Ident> = (0..field_count)
                .map(|i| syn::Ident::new(&format!("__{}", i), proc_macro2::Span::call_site()))
                .collect();

            // 检查是否所有字段都是 args
            let all_args = fields.unnamed.iter().all(|f| is_arg_field(f));

            // 如果只有一个字段且为 args，且有 token，生成简化格式解析器
            if field_count == 1 && all_args {
                let field_type = &field_types[0];
                if let Some(token) = &struct_token {
                    // 有 token: 解析 "token value" 格式 (如 "bb0" or "%0")
                    // 使用 token_prefix 而不是 keyword，因为token后面紧跟值，中间没有空格
                    quote! {
                        nom::combinator::map(
                            nom::sequence::preceded(
                                karte_ir_codec::parse::token_prefix(#token),
                                <#field_type>::parse_nom
                            ),
                            |value| #struct_name(value)
                        )(input)
                    }
                } else {
                    // 无 token: 直接解析字段值
                    quote! {
                        nom::combinator::map(
                            <#field_type>::parse_nom,
                            |value| #struct_name(value)
                        )(input)
                    }
                }
            } else {
                // 标准格式: StructName(...)
                quote! {
                    nom::combinator::map(
                        nom::sequence::preceded(
                            karte_ir_codec::parse::keyword(#struct_name_str),
                            karte_ir_codec::parse::parens(
                                nom::sequence::tuple((
                                    #(<#field_types>::parse_nom,)*
                                ))
                            )
                        ),
                        |(#(#field_names,)*)| #struct_name(#(#field_names,)*)
                    )(input)
                }
            }
        }
        Fields::Unit => {
            quote! {
                nom::combinator::map(
                    karte_ir_codec::parse::keyword("()"),
                    |_| #struct_name
                )(input)
            }
        }
    }
}
