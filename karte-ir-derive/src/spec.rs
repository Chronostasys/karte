use crate::utils::{parse_attributes, AttributeConfig, FieldStyle};
use syn::{Data, DeriveInput, Field, Fields, Ident, Type};

#[derive(Clone)]
pub struct TypeSpec {
    pub name: Ident,
    pub attrs: AttributeConfig,
    pub kind: TypeKind,
}

#[derive(Clone)]
pub enum TypeKind {
    Struct(StructSpec),
    Enum(EnumSpec),
}

#[derive(Clone)]
pub struct StructSpec {
    pub fields: StructFields,
}

#[derive(Clone)]
pub enum StructFields {
    Named(Vec<FieldSpec>),
    Unnamed(Vec<FieldSpec>),
    Unit,
}

#[derive(Clone)]
pub struct EnumSpec {
    pub variants: Vec<VariantSpec>,
}

#[derive(Clone)]
pub struct VariantSpec {
    pub ident: Ident,
    pub attrs: AttributeConfig,
    pub fields: VariantFields,
}

impl VariantSpec {
    pub fn token(&self) -> Option<&str> {
        self.attrs.token.as_deref()
    }
}

#[derive(Clone)]
pub enum VariantFields {
    Named(Vec<FieldSpec>),
    Unnamed(Vec<FieldSpec>),
    Unit,
}

#[derive(Clone)]
pub enum FieldKind {
    Named(Ident),
    Unnamed(usize),
}

#[derive(Clone)]
pub struct FieldSpec {
    pub kind: FieldKind,
    pub ty: Type,
    pub attrs: AttributeConfig,
    pub skip: bool,
}

impl FieldSpec {
    pub fn is_skipped(&self) -> bool {
        self.skip
    }

    pub fn is_arg(&self) -> bool {
        self.attrs.is_arg
    }

    pub fn is_extra(&self) -> bool {
        self.attrs.is_extra
    }

    pub fn style(&self) -> FieldStyle {
        self.attrs.style
    }

    pub fn label(&self) -> Option<&str> {
        self.attrs.label.as_deref()
    }

    pub fn ident(&self) -> Option<&Ident> {
        match &self.kind {
            FieldKind::Named(ident) => Some(ident),
            _ => None,
        }
    }

    pub fn index(&self) -> Option<usize> {
        match &self.kind {
            FieldKind::Unnamed(index) => Some(*index),
            _ => None,
        }
    }
}

impl TypeSpec {
    pub fn from_derive_input(input: &DeriveInput) -> Self {
        let name = input.ident.clone();
        let type_config = parse_attributes(&input.attrs);

        let kind = match &input.data {
            Data::Struct(data_struct) => TypeKind::Struct(StructSpec {
                fields: build_struct_fields(&data_struct.fields),
            }),
            Data::Enum(data_enum) => TypeKind::Enum(EnumSpec {
                variants: data_enum
                    .variants
                    .iter()
                    .map(|variant| VariantSpec {
                        ident: variant.ident.clone(),
                        attrs: parse_attributes(&variant.attrs),
                        fields: build_variant_fields(&variant.fields),
                    })
                    .collect(),
            }),
            Data::Union(_) => panic!("Union types are not supported for IrCodec"),
        };

        TypeSpec {
            name,
            attrs: type_config,
            kind,
        }
    }
}

fn build_struct_fields(fields: &Fields) -> StructFields {
    match fields {
        Fields::Named(fields_named) => StructFields::Named(
            fields_named
                .named
                .iter()
                .map(|field| FieldSpec::from_named(field))
                .collect(),
        ),
        Fields::Unnamed(fields_unnamed) => StructFields::Unnamed(
            fields_unnamed
                .unnamed
                .iter()
                .enumerate()
                .map(|(index, field)| FieldSpec::from_unnamed(field, index))
                .collect(),
        ),
        Fields::Unit => StructFields::Unit,
    }
}

fn build_variant_fields(fields: &Fields) -> VariantFields {
    match fields {
        Fields::Named(fields_named) => VariantFields::Named(
            fields_named
                .named
                .iter()
                .map(|field| FieldSpec::from_named(field))
                .collect(),
        ),
        Fields::Unnamed(fields_unnamed) => VariantFields::Unnamed(
            fields_unnamed
                .unnamed
                .iter()
                .enumerate()
                .map(|(index, field)| FieldSpec::from_unnamed(field, index))
                .collect(),
        ),
        Fields::Unit => VariantFields::Unit,
    }
}

impl FieldSpec {
    fn from_named(field: &Field) -> Self {
        let ident = field
            .ident
            .clone()
            .expect("named field should have an identifier");
        Self::from_field(field, FieldKind::Named(ident))
    }

    fn from_unnamed(field: &Field, index: usize) -> Self {
        Self::from_field(field, FieldKind::Unnamed(index))
    }

    fn from_field(field: &Field, kind: FieldKind) -> Self {
        let mut attrs = parse_attributes(&field.attrs);
        let skip = attrs.skip || is_span_type(&field.ty);
        attrs.skip = skip;

        FieldSpec {
            kind,
            ty: field.ty.clone(),
            attrs,
            skip,
        }
    }
}

fn is_span_type(ty: &Type) -> bool {
    if let Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            return segment.ident == "Span";
        }
    }
    false
}
