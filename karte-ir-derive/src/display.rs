use crate::{
    spec::{FieldSpec, StructFields, StructSpec, TypeKind, TypeSpec},
    utils::{
        get_field_label, get_field_style, get_variant_token, is_arg_field, is_extra_field,
        parse_attributes, should_skip_field, FieldStyle,
    },
};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DataEnum, DeriveInput, Fields, Ident, Index};

/// 生成 IrDisplay 实现
pub fn generate_display_impl(input: &DeriveInput) -> TokenStream {
    let type_spec = TypeSpec::from_derive_input(input);
    let name = &type_spec.name;

    let display_body = match (&type_spec.kind, &input.data) {
        (TypeKind::Enum(_), Data::Enum(data_enum)) => generate_enum_display(name, data_enum),
        (TypeKind::Struct(struct_spec), Data::Struct(_)) => {
            generate_struct_display_from_spec(struct_spec, name, &type_spec)
        }
        _ => panic!("Union types are not supported for IrCodec"),
    };

    let is_program = type_spec.attrs.is_program;

    if is_program {
        // Program 类型：直接输出内容，不输出外层结构名
        quote! {
            impl std::fmt::Display for #name {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    karte_ir_codec::IrDisplay::ir_fmt(self, f)
                }
            }

            impl karte_ir_codec::IrDisplay for #name {
                fn ir_fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    #display_body
                }
            }
        }
    } else {
        // 普通类型：输出结构名
        quote! {
            impl std::fmt::Display for #name {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    karte_ir_codec::IrDisplay::ir_fmt(self, f)
                }
            }

            impl karte_ir_codec::IrDisplay for #name {
                fn ir_fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    #display_body
                }
            }
        }
    }
}

/// 生成 enum 的 Display 实现
fn generate_enum_display(_enum_name: &Ident, data: &DataEnum) -> TokenStream {
    let mut match_arms = Vec::new();

    for variant in &data.variants {
        let variant_name = &variant.ident;
        let variant_name_str = variant_name.to_string();
        let variant_token = get_variant_token(variant);

        match &variant.fields {
            Fields::Named(fields) => {
                let field_names: Vec<_> = fields
                    .named
                    .iter()
                    .filter(|f| !should_skip_field(f))
                    .map(|f| f.ident.as_ref().unwrap())
                    .collect();

                // 收集标记为 args 的字段
                let arg_fields: Vec<_> = fields
                    .named
                    .iter()
                    .filter(|f| !should_skip_field(f) && is_arg_field(f))
                    .map(|f| f.ident.as_ref().unwrap())
                    .collect();

                let has_skipped = fields.named.iter().any(|f| should_skip_field(f));

                // 检查是否有特殊格式化样式
                let variant_attrs = parse_attributes(&variant.attrs);
                let special_format = variant_attrs.special_format;

                // 如果有特殊格式（binop/unop/fieldaccess/infix），优先使用它们
                use crate::utils::SpecialFormatStyle;

                if special_format == SpecialFormatStyle::BinOp {
                    use crate::utils::FieldRole;

                    // 根据字段角色查找对应字段
                    let mut target_field = None;
                    let mut left_field = None;
                    let mut op_field = None;
                    let mut right_field = None;

                    for field in &fields.named {
                        if should_skip_field(field) {
                            continue;
                        }
                        let field_attrs = parse_attributes(&field.attrs);
                        let field_name = field.ident.as_ref().unwrap();
                        match field_attrs.field_role {
                            FieldRole::Target => target_field = Some(field_name),
                            FieldRole::Left => left_field = Some(field_name),
                            FieldRole::Op => op_field = Some(field_name),
                            FieldRole::Right => right_field = Some(field_name),
                            _ => {}
                        }
                    }

                    let target = target_field.expect("binop needs target field");
                    let left = left_field.expect("binop needs left field");
                    let op = op_field.expect("binop needs op field");
                    let right = right_field.expect("binop needs right field");

                    if has_skipped {
                        match_arms.push(quote! {
                            Self::#variant_name { #target, #left, #op, #right, .. } => {
                                karte_ir_codec::IrDisplay::ir_fmt(#target, f)?;
                                write!(f, " = ")?;
                                karte_ir_codec::IrDisplay::ir_fmt(#left, f)?;
                                write!(f, " ")?;
                                karte_ir_codec::IrDisplay::ir_fmt(#op, f)?;
                                write!(f, " ")?;
                                karte_ir_codec::IrDisplay::ir_fmt(#right, f)
                            }
                        });
                    } else {
                        match_arms.push(quote! {
                            Self::#variant_name { #target, #left, #op, #right } => {
                                karte_ir_codec::IrDisplay::ir_fmt(#target, f)?;
                                write!(f, " = ")?;
                                karte_ir_codec::IrDisplay::ir_fmt(#left, f)?;
                                write!(f, " ")?;
                                karte_ir_codec::IrDisplay::ir_fmt(#op, f)?;
                                write!(f, " ")?;
                                karte_ir_codec::IrDisplay::ir_fmt(#right, f)
                            }
                        });
                    }
                    continue;
                }

                if special_format == SpecialFormatStyle::UnOp {
                    use crate::utils::FieldRole;

                    // 根据字段角色查找对应字段
                    let mut target_field = None;
                    let mut op_field = None;
                    let mut operand_field = None;

                    for field in &fields.named {
                        if should_skip_field(field) {
                            continue;
                        }
                        let field_attrs = parse_attributes(&field.attrs);
                        let field_name = field.ident.as_ref().unwrap();
                        match field_attrs.field_role {
                            FieldRole::Target => target_field = Some(field_name),
                            FieldRole::Op => op_field = Some(field_name),
                            FieldRole::Operand => operand_field = Some(field_name),
                            _ => {}
                        }
                    }

                    let target = target_field.expect("unop needs target field");
                    let op = op_field.expect("unop needs op field");
                    let operand = operand_field.expect("unop needs operand field");

                    if has_skipped {
                        match_arms.push(quote! {
                            Self::#variant_name { #target, #op, #operand, .. } => {
                                karte_ir_codec::IrDisplay::ir_fmt(#target, f)?;
                                write!(f, " = ")?;
                                karte_ir_codec::IrDisplay::ir_fmt(#op, f)?;
                                karte_ir_codec::IrDisplay::ir_fmt(#operand, f)
                            }
                        });
                    } else {
                        match_arms.push(quote! {
                            Self::#variant_name { #target, #op, #operand } => {
                                karte_ir_codec::IrDisplay::ir_fmt(#target, f)?;
                                write!(f, " = ")?;
                                karte_ir_codec::IrDisplay::ir_fmt(#op, f)?;
                                karte_ir_codec::IrDisplay::ir_fmt(#operand, f)
                            }
                        });
                    }
                    continue;
                }

                if special_format == SpecialFormatStyle::FieldAccess {
                    use crate::utils::FieldRole;

                    // 根据字段角色查找对应字段
                    let mut target_field = None;
                    let mut object_field = None;
                    let mut field_name_field = None;

                    for field in &fields.named {
                        if should_skip_field(field) {
                            continue;
                        }
                        let field_attrs = parse_attributes(&field.attrs);
                        let field_name = field.ident.as_ref().unwrap();
                        match field_attrs.field_role {
                            FieldRole::Target => target_field = Some(field_name),
                            FieldRole::Object => object_field = Some(field_name),
                            FieldRole::FieldName => field_name_field = Some(field_name),
                            _ => {}
                        }
                    }

                    let target = target_field.expect("fieldaccess needs target field");
                    let object = object_field.expect("fieldaccess needs object field");
                    let field = field_name_field.expect("fieldaccess needs field_name field");

                    if has_skipped {
                        match_arms.push(quote! {
                            Self::#variant_name { #target, #object, #field, .. } => {
                                karte_ir_codec::IrDisplay::ir_fmt(#target, f)?;
                                write!(f, " = ")?;
                                karte_ir_codec::IrDisplay::ir_fmt(#object, f)?;
                                write!(f, ".")?;
                                karte_ir_codec::IrDisplay::ir_fmt(#field, f)
                            }
                        });
                    } else {
                        match_arms.push(quote! {
                            Self::#variant_name { #target, #object, #field } => {
                                karte_ir_codec::IrDisplay::ir_fmt(#target, f)?;
                                write!(f, " = ")?;
                                karte_ir_codec::IrDisplay::ir_fmt(#object, f)?;
                                write!(f, ".")?;
                                karte_ir_codec::IrDisplay::ir_fmt(#field, f)
                            }
                        });
                    }
                    continue;
                }

                if special_format == SpecialFormatStyle::Infix {
                    use crate::utils::FieldRole;

                    // 根据字段角色查找对应字段
                    let mut left_field = None;
                    let mut right_field = None;

                    for field in &fields.named {
                        if should_skip_field(field) {
                            continue;
                        }
                        let field_attrs = parse_attributes(&field.attrs);
                        let field_name = field.ident.as_ref().unwrap();
                        match field_attrs.field_role {
                            FieldRole::Left => left_field = Some(field_name),
                            FieldRole::Right => right_field = Some(field_name),
                            _ => {}
                        }
                    }

                    let left = left_field.expect("infix needs left field");
                    let right = right_field.expect("infix needs right field");

                    let token = variant_token.as_ref().expect("infix needs token");
                    if has_skipped {
                        match_arms.push(quote! {
                            Self::#variant_name { #left, #right, .. } => {
                                karte_ir_codec::IrDisplay::ir_fmt(#left, f)?;
                                write!(f, " {} ", #token)?;
                                karte_ir_codec::IrDisplay::ir_fmt(#right, f)
                            }
                        });
                    } else {
                        match_arms.push(quote! {
                            Self::#variant_name { #left, #right } => {
                                karte_ir_codec::IrDisplay::ir_fmt(#left, f)?;
                                write!(f, " {} ", #token)?;
                                karte_ir_codec::IrDisplay::ir_fmt(#right, f)
                            }
                        });
                    }
                    continue;
                }

                // No special-case handling for Call: use the generic token+args printing below.

                // 如果存在 token 且所有需要展示的字段中有标记为 args 的字段，使用前缀样式并用逗号分隔参数：`token arg1, arg2`
                if let Some(token) = &variant_token {
                    if !arg_fields.is_empty() {
                        // 按字段名显示带标签的参数：`label: value`，多个字段用 ", " 分隔。
                        // 对于任意 Vec<T> 字段，统一以方括号形式显示元素列表，例如 `args: [a, b]`。
                        let arg_writes: Vec<_> = arg_fields
                            .iter()
                            .enumerate()
                            .map(|(i, arg_ident)| {
                                // 找到对应的 field 以获取标签和类型
                                let field = fields
                                    .named
                                    .iter()
                                    .find(|f| {
                                        f.ident.as_ref().map(|id| id == *arg_ident).unwrap_or(false)
                                    })
                                    .expect("arg field must exist");
                                let label = crate::utils::get_field_label(field)
                                    .unwrap_or_else(|| arg_ident.to_string());
                                let field_ty = &field.ty;

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

                                let write_field = if is_vec {
                                    // 显示为 label: [ elem1, elem2 ]
                                    quote! {
                                        write!(f, "{}: ", #label)?;
                                        write!(f, "[")?;
                                        {
                                            let mut __first_elem = true;
                                            for __elem in #arg_ident.iter() {
                                                if !__first_elem { write!(f, ", ")?; }
                                                karte_ir_codec::IrDisplay::ir_fmt(__elem, f)?;
                                                __first_elem = false;
                                            }
                                        }
                                        write!(f, "]")?;
                                    }
                                } else {
                                    // 普通字段：label: value
                                    quote! {
                                        write!(f, "{}: ", #label)?;
                                        karte_ir_codec::IrDisplay::ir_fmt(#arg_ident, f)?;
                                    }
                                };

                                if i == 0 {
                                    write_field
                                } else {
                                    quote! {
                                        write!(f, ", ")?;
                                        #write_field
                                    }
                                }
                            })
                            .collect();

                        if has_skipped {
                            match_arms.push(quote! {
                                Self::#variant_name { #(#arg_fields),*, .. } => {
                                    write!(f, "{}", #token)?;
                                    write!(f, " ")?;
                                    #(#arg_writes)*
                                    Ok(())
                                }
                            });
                        } else {
                            match_arms.push(quote! {
                                Self::#variant_name { #(#arg_fields),* } => {
                                    write!(f, "{}", #token)?;
                                    write!(f, " ")?;
                                    #(#arg_writes)*
                                    Ok(())
                                }
                            });
                        }
                        continue;
                    }
                }

                if field_names.is_empty() {
                    // 无字段或所有字段都被跳过
                    if has_skipped {
                        match_arms.push(quote! {
                            Self::#variant_name { .. } => write!(f, #variant_name_str)
                        });
                    } else {
                        match_arms.push(quote! {
                            Self::#variant_name {} => write!(f, #variant_name_str)
                        });
                    }
                } else if field_names.len() == 1 {
                    // 单字段，检查是否为 args（如果是，直接输出值，否则包装）
                    let field = field_names[0];
                    let is_arg = arg_fields.contains(&field);

                    if is_arg {
                        // 单个 args 字段，直接输出其值
                        if has_skipped {
                            match_arms.push(quote! {
                                Self::#variant_name { #field, .. } => {
                                    karte_ir_codec::IrDisplay::ir_fmt(#field, f)
                                }
                            });
                        } else {
                            match_arms.push(quote! {
                                Self::#variant_name { #field } => {
                                    karte_ir_codec::IrDisplay::ir_fmt(#field, f)
                                }
                            });
                        }
                    } else {
                        // 普通单字段，使用包装格式: VariantName(value)
                        if has_skipped {
                            match_arms.push(quote! {
                                Self::#variant_name { #field, .. } => {
                                    write!(f, "{}(", #variant_name_str)?;
                                    karte_ir_codec::IrDisplay::ir_fmt(#field, f)?;
                                    write!(f, ")")
                                }
                            });
                        } else {
                            match_arms.push(quote! {
                                Self::#variant_name { #field } => {
                                    write!(f, "{}(", #variant_name_str)?;
                                    karte_ir_codec::IrDisplay::ir_fmt(#field, f)?;
                                    write!(f, ")")
                                }
                            });
                        }
                    }
                } else {
                    // 多字段，生成美化的输出
                    let field_writes = generate_variant_fields_display(fields, field_names.clone());

                    if has_skipped {
                        match_arms.push(quote! {
                            Self::#variant_name { #(#field_names),*, .. } => {
                                write!(f, "{}", #variant_name_str)?;
                                #field_writes
                            }
                        });
                    } else {
                        match_arms.push(quote! {
                            Self::#variant_name { #(#field_names),* } => {
                                write!(f, "{}", #variant_name_str)?;
                                #field_writes
                            }
                        });
                    }
                }
            }
            Fields::Unnamed(fields) => {
                // 格式: VariantName(arg1, arg2, ...) 或使用 token
                let field_count = fields.unnamed.len();
                let field_names: Vec<Ident> = (0..field_count)
                    .map(|i| syn::Ident::new(&format!("__{}", i), proc_macro2::Span::call_site()))
                    .collect();

                // 检查是否所有字段都标记为 args
                let all_args = fields.unnamed.iter().all(|f| is_arg_field(f));

                if field_count == 0 {
                    match_arms.push(quote! {
                        Self::#variant_name => write!(f, #variant_name_str)
                    });
                } else if field_count == 1 && variant_token.is_some() && all_args {
                    // 单参数 + token: 格式为 "token arg" (无空格，紧凑格式)
                    let token = variant_token.as_ref().unwrap();
                    let field = &field_names[0];
                    match_arms.push(quote! {
                        Self::#variant_name(#field) => {
                            write!(f, "{}", #token)?;
                            karte_ir_codec::IrDisplay::ir_fmt(#field, f)
                        }
                    });
                } else if field_count == 1 {
                    // 单参数，无 token: 格式为 "VariantName(arg)"
                    let field = &field_names[0];
                    match_arms.push(quote! {
                        Self::#variant_name(#field) => {
                            write!(f, "{}(", #variant_name_str)?;
                            karte_ir_codec::IrDisplay::ir_fmt(#field, f)?;
                            write!(f, ")")
                        }
                    });
                } else {
                    let field_writes: Vec<_> = field_names
                        .iter()
                        .enumerate()
                        .map(|(i, field)| {
                            if i == 0 {
                                quote! {
                                    karte_ir_codec::IrDisplay::ir_fmt(#field, f)?;
                                }
                            } else {
                                quote! {
                                    write!(f, ", ")?;
                                    karte_ir_codec::IrDisplay::ir_fmt(#field, f)?;
                                }
                            }
                        })
                        .collect();

                    match_arms.push(quote! {
                        Self::#variant_name(#(#field_names),*) => {
                            write!(f, "{}(", #variant_name_str)?;
                            #(#field_writes)*
                            write!(f, ")")
                        }
                    });
                }
            }
            Fields::Unit => {
                // 格式: VariantName 或 token（如果有）
                if let Some(token) = &variant_token {
                    match_arms.push(quote! {
                        Self::#variant_name => write!(f, #token)
                    });
                } else {
                    match_arms.push(quote! {
                        Self::#variant_name => write!(f, #variant_name_str)
                    });
                }
            }
        }
    }

    quote! {
        match self {
            #(#match_arms,)*
        }
    }
}

/// 生成变体字段的美化显示
fn generate_variant_fields_display(
    fields: &syn::FieldsNamed,
    field_names: Vec<&Ident>,
) -> TokenStream {
    // 检查是否有 body 样式的字段
    let has_body_fields = fields
        .named
        .iter()
        .filter(|f| !should_skip_field(f))
        .any(|f| {
            matches!(
                get_field_style(f),
                FieldStyle::Body | FieldStyle::NewlineItems
            )
        });

    if has_body_fields {
        // 使用换行格式，分离 extra 字段
        let normal_field_writes: Vec<_> = fields
            .named
            .iter()
            .filter(|f| !should_skip_field(f) && !is_extra_field(f))
            .map(|f| {
                let field_name = f.ident.as_ref().unwrap();
                let label = get_field_label(f).unwrap_or_else(|| field_name.to_string());
                let style = get_field_style(f);

                match style {
                    FieldStyle::Body | FieldStyle::NewlineItems => {
                        quote! {
                            write!(f, "\n    {}: ", #label)?;
                            karte_ir_codec::write_multiline_suffix(f, #field_name, 8)?;
                        }
                    }
                    FieldStyle::Inline | FieldStyle::Compact => {
                        quote! {
                            write!(f, "\n    {}: ", #label)?;
                            karte_ir_codec::IrDisplay::ir_fmt(#field_name, f)?;
                        }
                    }
                }
            })
            .collect();

        // 收集 extra 字段
        let extra_field_writes: Vec<_> = fields
            .named
            .iter()
            .filter(|f| !should_skip_field(f) && is_extra_field(f))
            .map(|f| {
                let field_name = f.ident.as_ref().unwrap();
                let label = get_field_label(f).unwrap_or_else(|| field_name.to_string());
                quote! {
                    write!(f, " {} = ", #label)?;
                    karte_ir_codec::write_multiline_suffix(f, #field_name, 4)?;
                }
            })
            .collect();

        if !extra_field_writes.is_empty() {
            quote! {
                #(#normal_field_writes)*
                write!(f, "\n  _extra: {{")?;
                #(#extra_field_writes)*
                write!(f, " }}")?;
                Ok(())
            }
        } else {
            quote! {
                #(#normal_field_writes)*
                Ok(())
            }
        }
    } else {
        // 使用紧凑格式：VariantName(field1: value1, field2: value2)
        let field_writes: Vec<_> = field_names
            .iter()
            .enumerate()
            .map(|(i, field)| {
                let field_str = field.to_string();
                if i == 0 {
                    quote! {
                        write!(f, " {} = ", #field_str)?;
                        karte_ir_codec::write_multiline_suffix(f, #field, 4)?;
                    }
                } else {
                    quote! {
                        write!(f, ", {} = ", #field_str)?;
                        karte_ir_codec::write_multiline_suffix(f, #field, 4)?;
                    }
                }
            })
            .collect();

        quote! {
            write!(f, " {{")?;
            #(#field_writes)*
            write!(f, " }}")
        }
    }
}

/// 生成 struct 的 Display 实现（基于共享的 TypeSpec 元数据）
fn generate_struct_display_from_spec(
    struct_spec: &StructSpec,
    type_ident: &Ident,
    type_spec: &TypeSpec,
) -> TokenStream {
    let type_name_string = type_ident.to_string();
    let type_name = &type_name_string;
    match &struct_spec.fields {
        StructFields::Named(fields) => generate_named_struct_display(fields, type_spec, type_name),
        StructFields::Unnamed(fields) => {
            generate_unnamed_struct_display(fields, type_spec, type_name)
        }
        StructFields::Unit => quote! {
            write!(f, "{}", #type_name)
        },
    }
}

fn generate_named_struct_display(
    fields: &[FieldSpec],
    type_spec: &TypeSpec,
    type_name: &str,
) -> TokenStream {
    let printable: Vec<&FieldSpec> = fields.iter().filter(|f| !f.is_skipped()).collect();
    let has_body_fields = printable
        .iter()
        .any(|f| matches!(f.style(), FieldStyle::Body | FieldStyle::NewlineItems));

    if has_body_fields {
        let (line_prefix, _inline_indent) = if type_spec.attrs.is_program {
            ("", 4usize)
        } else {
            ("    ", 8usize)
        };

        let field_writes: Vec<_> = printable
            .into_iter()
            .map(|field| {
                let field_ident = field.ident().expect("named field should have identifier");
                let label = field
                    .label()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| field_ident.to_string());
                let style = field.style();

                match style {
                    FieldStyle::Body | FieldStyle::NewlineItems => {
                        if type_spec.attrs.is_program {
                            quote! {
                                write!(f, "\n{}: ", #label)?;
                                karte_ir_codec::write_multiline_suffix(f, &self.#field_ident, 4)?;
                            }
                        } else {
                            quote! {
                                write!(f, "\n    {}: ", #label)?;
                                karte_ir_codec::write_multiline_suffix(f, &self.#field_ident, 8)?;
                            }
                        }
                    }
                    FieldStyle::Inline | FieldStyle::Compact => {
                        quote! {
                            write!(f, "\n{}{}: ", #line_prefix, #label)?;
                            karte_ir_codec::IrDisplay::ir_fmt(&self.#field_ident, f)?;
                        }
                    }
                }
            })
            .collect();

        if type_spec.attrs.is_program {
            quote! {
                #(#field_writes)*
                Ok(())
            }
        } else {
            quote! {
                write!(f, "{}", #type_name)?;
                #(#field_writes)*
                Ok(())
            }
        }
    } else {
        let printable: Vec<(usize, &FieldSpec)> = fields
            .iter()
            .enumerate()
            .filter(|(_, field)| !field.is_skipped())
            .collect();

        let field_writes: Vec<_> = printable
            .iter()
            .enumerate()
            .map(|(display_idx, (_, field))| {
                let field_ident = field.ident().expect("named field should have identifier");
                let label = field
                    .label()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| field_ident.to_string());
                if display_idx == 0 {
                    quote! {
                        write!(f, "{}: ", #label)?;
                        karte_ir_codec::IrDisplay::ir_fmt(&self.#field_ident, f)?;
                    }
                } else {
                    quote! {
                        write!(f, ", {}: ", #label)?;
                        karte_ir_codec::IrDisplay::ir_fmt(&self.#field_ident, f)?;
                    }
                }
            })
            .collect();

        quote! {
            write!(f, "{}(", #type_name)?;
            #(#field_writes)*
            write!(f, ")")
        }
    }
}

fn generate_unnamed_struct_display(
    fields: &[FieldSpec],
    type_spec: &TypeSpec,
    type_name: &str,
) -> TokenStream {
    let printable: Vec<(usize, &FieldSpec)> = fields
        .iter()
        .enumerate()
        .filter(|(_, field)| !field.is_skipped())
        .collect();

    let all_args = printable.iter().all(|(_, field)| field.is_arg());

    if let Some(token) = type_spec.attrs.token.as_deref() {
        if all_args {
            if printable.is_empty() {
                return quote! {
                    write!(f, "{}", #token)
                };
            }

            if printable.len() == 1 {
                let idx = Index::from(printable[0].0);
                return quote! {
                    write!(f, "{}", #token)?;
                    karte_ir_codec::IrDisplay::ir_fmt(&self.#idx, f)
                };
            }

            let field_writes: Vec<_> = printable
                .iter()
                .map(|(original_idx, _)| {
                    let idx = Index::from(*original_idx);
                    quote! {
                        write!(f, " ")?;
                        karte_ir_codec::IrDisplay::ir_fmt(&self.#idx, f)?;
                    }
                })
                .collect();

            return quote! {
                write!(f, "{}", #token)?;
                #(#field_writes)*
                Ok(())
            };
        }
    }

    let field_writes: Vec<_> = printable
        .iter()
        .enumerate()
        .map(|(display_idx, (original_idx, _))| {
            let idx = Index::from(*original_idx);
            if display_idx == 0 {
                quote! {
                    karte_ir_codec::IrDisplay::ir_fmt(&self.#idx, f)?;
                }
            } else {
                quote! {
                    write!(f, ", ")?;
                    karte_ir_codec::IrDisplay::ir_fmt(&self.#idx, f)?;
                }
            }
        })
        .collect();

    quote! {
        write!(f, "{}(", #type_name)?;
        #(#field_writes)*
        write!(f, ")")
    }
}
