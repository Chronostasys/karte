use syn::{Attribute, Field, Meta};

/// 字段格式化样式
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FieldStyle {
    /// 默认：内联显示
    Inline,
    /// 主体：换行缩进显示，用于大型字段（如 instructions, basic_blocks）
    Body,
    /// 紧凑：在一行内
    Compact,
    /// 每项换行：用于集合类型
    NewlineItems,
}

impl Default for FieldStyle {
    fn default() -> Self {
        FieldStyle::Inline
    }
}

/// 特殊格式化样式（用于variant级别）
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpecialFormatStyle {
    /// 无特殊格式
    None,
    /// 二元运算: target = left op right
    BinOp,
    /// 一元运算: target = op operand
    UnOp,
    /// 字段访问: target = object.field
    FieldAccess,
    /// 中缀表达式: left op right
    Infix,
}

/// 字段角色（用于特殊格式）
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FieldRole {
    None,
    Target,      // 目标值（赋值左侧）
    Left,        // 左操作数
    Right,       // 右操作数
    Op,          // 操作符
    Operand,     // 一元操作数
    Object,      // 对象
    FieldName,   // 字段名
}

/// 属性配置
#[derive(Debug, Clone)]
pub struct AttributeConfig {
    pub skip: bool,
    pub style: FieldStyle,
    pub label: Option<String>,
    pub token: Option<String>,
    pub is_arg: bool,
    pub is_extra: bool,   // 标记为 extra 字段，打印时放入 _extra_fields
    pub is_program: bool, // 标记为 program 类型，不输出外层结构
    pub special_format: SpecialFormatStyle, // variant级别的特殊格式
    pub field_role: FieldRole, // 字段在特殊格式中的角色
}

impl Default for AttributeConfig {
    fn default() -> Self {
        Self {
            skip: false,
            style: FieldStyle::Inline,
            label: None,
            token: None,
            is_arg: false,
            is_extra: false,
            is_program: false,
            special_format: SpecialFormatStyle::None,
            field_role: FieldRole::None,
        }
    }
}

/// 解析字段的 ir_codec 属性
pub fn parse_attributes(attrs: &[Attribute]) -> AttributeConfig {
    let mut config = AttributeConfig::default();

    for attr in attrs {
        if attr.path().is_ident("ir_codec") {
            if let Meta::List(meta_list) = &attr.meta {
                // 解析属性参数
                let tokens_str = meta_list.tokens.to_string();
                for part in tokens_str.split(',').map(|s| s.trim()) {
                    match part {
                        "skip" => config.skip = true,
                        "body" => config.style = FieldStyle::Body,
                        "compact" => config.style = FieldStyle::Compact,
                        "newline_items" => config.style = FieldStyle::NewlineItems,
                        "inline" => config.style = FieldStyle::Inline,
                        "args" | "arg" => config.is_arg = true,
                        "extra" => config.is_extra = true,
                        "program" => config.is_program = true,
                        // 特殊格式样式（variant级别）
                        "binop" => config.special_format = SpecialFormatStyle::BinOp,
                        "unop" => config.special_format = SpecialFormatStyle::UnOp,
                        "fieldaccess" => config.special_format = SpecialFormatStyle::FieldAccess,
                        "infix" => config.special_format = SpecialFormatStyle::Infix,
                        // 字段角色
                        "target" => config.field_role = FieldRole::Target,
                        "left" => config.field_role = FieldRole::Left,
                        "right" => config.field_role = FieldRole::Right,
                        "op" => config.field_role = FieldRole::Op,
                        "operand" => config.field_role = FieldRole::Operand,
                        "object" => config.field_role = FieldRole::Object,
                        "field_name" => config.field_role = FieldRole::FieldName,
                        s if s.starts_with("label") => {
                            // 解析 label = "xxx"
                            if let Some(idx) = s.find('=') {
                                let label = s[idx + 1..].trim().trim_matches('"').to_string();
                                config.label = Some(label);
                            }
                        }
                        s if s.starts_with("token") => {
                            // 解析 token = "xxx" 或 token("xxx")
                            if let Some(idx) = s.find('=') {
                                let token = s[idx + 1..].trim().trim_matches('"').to_string();
                                config.token = Some(token);
                            } else if let Some(start) = s.find('(') {
                                if let Some(end) = s.find(')') {
                                    let token =
                                        s[start + 1..end].trim().trim_matches('"').to_string();
                                    config.token = Some(token);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    config
}

/// 判断字段是否应该跳过
pub fn should_skip_field(field: &Field) -> bool {
    // 自动跳过 Span 类型
    if let syn::Type::Path(type_path) = &field.ty {
        if let Some(segment) = type_path.path.segments.last() {
            if segment.ident == "Span" {
                return true;
            }
        }
    }

    parse_attributes(&field.attrs).skip
}

/// 获取字段的格式化样式
pub fn get_field_style(field: &Field) -> FieldStyle {
    parse_attributes(&field.attrs).style
}

/// 获取字段的标签
pub fn get_field_label(field: &Field) -> Option<String> {
    parse_attributes(&field.attrs).label
}

/// 获取 variant 的 token
pub fn get_variant_token(variant: &syn::Variant) -> Option<String> {
    parse_attributes(&variant.attrs).token
}

/// 获取 variant 的 special format style
pub fn get_variant_format_style(variant: &syn::Variant) -> SpecialFormatStyle {
    parse_attributes(&variant.attrs).special_format
}

/// 获取字段的role（在特殊格式中的角色）
pub fn get_field_role(field: &Field) -> Option<FieldRole> {
    let role = parse_attributes(&field.attrs).field_role;
    match role {
        FieldRole::None => None,
        _ => Some(role),
    }
}

/// 检查类型是否是集合类型
#[allow(dead_code)]
pub fn is_collection_type(field: &Field) -> bool {
    if let syn::Type::Path(type_path) = &field.ty {
        if let Some(segment) = type_path.path.segments.last() {
            let ident = segment.ident.to_string();
            return ident == "Vec" || ident == "HashMap" || ident == "BTreeMap";
        }
    }
    false
}

/// 检查字段是否标记为 args
pub fn is_arg_field(field: &syn::Field) -> bool {
    parse_attributes(&field.attrs).is_arg
}

/// 检查字段是否标记为 extra（应该放入 _extra_fields）
pub fn is_extra_field(field: &syn::Field) -> bool {
    parse_attributes(&field.attrs).is_extra
}
