#![warn(missing_docs)]
#![doc = include_str!("../README.md")]

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use std::collections::{HashMap, HashSet};
use syn::{
    Attribute, Data, DataEnum, DataStruct, DeriveInput, Expr, Field, Fields, FieldsNamed,
    FieldsUnnamed, GenericArgument, Lit, LitStr, Meta, PathArguments, Type, parse_macro_input,
    punctuated::Punctuated, spanned::Spanned,
};

#[proc_macro_derive(TierConfig, attributes(tier, serde))]
/// Derives `tier::TierMetadata` for nested configuration structs.
pub fn derive_tier_config(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand_tier_config(input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

#[proc_macro_derive(TierPatch, attributes(tier, serde))]
/// Derives `tier::TierPatch` for typed sparse override structs.
pub fn derive_tier_patch(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand_tier_patch(input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn expand_tier_config(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let tier_attrs = parse_tier_container_attrs(&input.attrs)?;
    let ident = input.ident;
    let generics = input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let container_attrs = parse_serde_container_attrs(&input.attrs)?;
    let field_tokens = match input.data {
        Data::Struct(data_struct) => expand_struct_metadata(data_struct, &container_attrs)?,
        Data::Enum(data_enum) => expand_enum_metadata(data_enum, &container_attrs)?,
        Data::Union(union) => {
            return Err(syn::Error::new_spanned(
                union.union_token,
                "TierConfig cannot be derived for unions",
            ));
        }
    };
    let check_tokens = container_check_tokens(&tier_attrs);

    Ok(quote! {
        impl #impl_generics ::tier::TierMetadata for #ident #ty_generics #where_clause {
            fn metadata() -> ::tier::ConfigMetadata {
                let mut metadata = ::tier::ConfigMetadata::new();
                #(#field_tokens)*
                #(#check_tokens)*
                metadata
            }
        }
    })
}

fn expand_tier_patch(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let ident = input.ident;
    let generics = input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let container_attrs = parse_serde_container_attrs(&input.attrs)?;
    ensure_struct_patch_container_attrs(&container_attrs)?;

    let field_tokens = match input.data {
        Data::Struct(data_struct) => match data_struct.fields {
            Fields::Named(fields) => expand_patch_fields_metadata(
                fields,
                SerdeFieldContext::for_struct(&container_attrs),
            )?,
            Fields::Unnamed(fields) => {
                return Err(syn::Error::new_spanned(
                    fields,
                    "TierPatch only supports structs with named fields",
                ));
            }
            Fields::Unit => Vec::new(),
        },
        Data::Enum(data_enum) => {
            return Err(syn::Error::new_spanned(
                data_enum.enum_token,
                "TierPatch cannot be derived for enums",
            ));
        }
        Data::Union(union) => {
            return Err(syn::Error::new_spanned(
                union.union_token,
                "TierPatch cannot be derived for unions",
            ));
        }
    };

    Ok(quote! {
        impl #impl_generics ::tier::TierPatch for #ident #ty_generics #where_clause {
            fn write_layer(
                &self,
                __tier_builder: &mut ::tier::patch::PatchLayerBuilder,
                __tier_prefix: &str,
            ) -> ::std::result::Result<(), ::tier::ConfigError> {
                #(#field_tokens)*
                Ok(())
            }
        }
    })
}

fn expand_struct_metadata(
    data_struct: DataStruct,
    container_attrs: &SerdeContainerAttrs,
) -> syn::Result<Vec<proc_macro2::TokenStream>> {
    ensure_struct_container_attrs(container_attrs)?;

    match data_struct.fields {
        Fields::Named(fields) => expand_named_fields_metadata(
            fields,
            SerdeFieldContext::for_struct(container_attrs),
            &format_ident!("metadata"),
            None,
        ),
        Fields::Unnamed(fields) => {
            expand_newtype_struct_metadata(fields, &format_ident!("metadata"))
        }
        Fields::Unit => Ok(Vec::new()),
    }
}

fn expand_enum_metadata(
    data_enum: DataEnum,
    container_attrs: &SerdeContainerAttrs,
) -> syn::Result<Vec<proc_macro2::TokenStream>> {
    let representation = enum_representation(container_attrs)?;
    let conflicts = non_external_variant_field_conflicts(&data_enum, container_attrs)?;
    let mut tokens = vec![quote! {
        metadata.push(
            ::tier::FieldMetadata::new("").merge_strategy(::tier::MergeStrategy::Replace)
        );
    }];
    if let Some(tag) = representation.tag_field() {
        let tag_lit = LitStr::new(tag, proc_macro2::Span::call_site());
        tokens.push(quote! {
            metadata.push(::tier::FieldMetadata::new(#tag_lit));
        });
    }

    for variant in data_enum.variants {
        let variant_ident = variant.ident.clone();
        let variant_attrs =
            parse_serde_variant_attrs(&variant.attrs, &variant_ident, container_attrs)?;
        if variant_attrs.skip_metadata {
            continue;
        }

        match variant.fields {
            Fields::Named(fields) => {
                let field_tokens = expand_named_fields_metadata(
                    fields,
                    SerdeFieldContext::for_enum_variant_fields(container_attrs),
                    &format_ident!("variant_metadata"),
                    Some(&conflicts),
                )?;
                push_variant_tokens(
                    &mut tokens,
                    field_tokens,
                    &variant_attrs,
                    &representation,
                    variant_ident.span(),
                );
            }
            Fields::Unnamed(fields) => {
                let field_tokens = expand_newtype_variant_metadata(
                    fields,
                    &representation,
                    variant_ident.span(),
                    &format_ident!("variant_metadata"),
                )?;
                push_variant_tokens(
                    &mut tokens,
                    field_tokens,
                    &variant_attrs,
                    &representation,
                    variant_ident.span(),
                );
            }
            Fields::Unit => {}
        }
    }

    Ok(tokens)
}

fn push_variant_tokens(
    tokens: &mut Vec<proc_macro2::TokenStream>,
    variant_tokens: Vec<proc_macro2::TokenStream>,
    variant_attrs: &SerdeVariantAttrs,
    representation: &EnumRepresentation,
    span: proc_macro2::Span,
) {
    let variant_name_lit = LitStr::new(&variant_attrs.canonical_name, span);
    let variant_alias_lits = variant_attrs
        .aliases
        .iter()
        .map(|alias| LitStr::new(alias, span))
        .collect::<Vec<_>>();

    match representation {
        EnumRepresentation::External => {
            tokens.push(quote! {
                {
                    let mut variant_metadata = ::tier::ConfigMetadata::new();
                    #(#variant_tokens)*
                    metadata.extend(::tier::metadata::prefixed_metadata(
                        #variant_name_lit,
                        ::std::vec![#(::std::string::String::from(#variant_alias_lits)),*],
                        variant_metadata,
                    ));
                }
            });
        }
        EnumRepresentation::Adjacent { content, .. } => {
            let content_lit = LitStr::new(content, span);
            tokens.push(quote! {
                {
                    let mut variant_metadata = ::tier::ConfigMetadata::new();
                    #(#variant_tokens)*
                    metadata.extend(::tier::metadata::prefixed_metadata(
                        #content_lit,
                        ::std::vec![],
                        variant_metadata,
                    ));
                }
            });
        }
        EnumRepresentation::Internal { .. } | EnumRepresentation::Untagged => {
            tokens.push(quote! {
                {
                    let mut variant_metadata = ::tier::ConfigMetadata::new();
                    #(#variant_tokens)*
                    metadata.extend(variant_metadata);
                }
            });
        }
    }
}

fn expand_named_fields_metadata(
    fields: FieldsNamed,
    context: SerdeFieldContext,
    accumulator: &proc_macro2::Ident,
    conflicts: Option<&NonExternalFieldConflicts>,
) -> syn::Result<Vec<proc_macro2::TokenStream>> {
    let mut field_tokens = Vec::new();

    for field in fields.named {
        field_tokens.extend(expand_named_field_metadata(
            field,
            context,
            accumulator,
            conflicts,
        )?);
    }

    Ok(field_tokens)
}

fn expand_named_field_metadata(
    field: Field,
    context: SerdeFieldContext,
    accumulator: &proc_macro2::Ident,
    conflicts: Option<&NonExternalFieldConflicts>,
) -> syn::Result<Vec<proc_macro2::TokenStream>> {
    let field_ident = field.ident.expect("named field");
    let mut serde_attrs = parse_serde_field_attrs(&field.attrs, &field_ident, context)?;
    let mut attrs = parse_tier_attrs(&field.attrs)?;
    if attrs.doc.is_none() {
        attrs.doc = doc_comment(&field.attrs);
    }

    if serde_attrs.skip_metadata {
        if attrs.has_any() {
            return Err(syn::Error::new_spanned(
                field_ident,
                "skipped fields cannot use tier metadata attributes",
            ));
        }
        return Ok(Vec::new());
    }

    if serde_attrs.flatten && attrs.has_any() {
        return Err(syn::Error::new_spanned(
            field_ident,
            "flattened fields cannot use tier metadata attributes",
        ));
    }

    if let Some(conflicts) = conflicts {
        if conflicts
            .skipped_fields
            .contains(&serde_attrs.canonical_name)
        {
            return Ok(Vec::new());
        }
        serde_attrs
            .aliases
            .retain(|alias| !conflicts.skipped_aliases.contains(alias));
        if attrs
            .env
            .as_ref()
            .is_some_and(|env| conflicts.skipped_envs.contains(env))
        {
            attrs.env = None;
        }
    }

    validate_merge_strategy(&attrs, &field.ty)?;
    validate_validation_attrs(&attrs, &field_ident)?;

    let field_type = field.ty;
    let metadata_ty = metadata_target_type(&field_type);
    let canonical_name_lit = LitStr::new(&serde_attrs.canonical_name, field_ident.span());
    let alias_lits = serde_attrs
        .aliases
        .iter()
        .map(|alias| LitStr::new(alias, field_ident.span()))
        .collect::<Vec<_>>();

    if serde_attrs.flatten {
        return Ok(vec![quote! {
            #accumulator.extend(<#metadata_ty as ::tier::TierMetadata>::metadata());
        }]);
    }

    Ok(vec![
        quote! {
            #accumulator.extend(::tier::metadata::prefixed_metadata(
                #canonical_name_lit,
                ::std::vec![#(::std::string::String::from(#alias_lits)),*],
                <#metadata_ty as ::tier::TierMetadata>::metadata(),
            ));
        },
        direct_field_metadata_tokens(
            accumulator,
            &canonical_name_lit,
            &alias_lits,
            &serde_attrs,
            &attrs,
            is_secret_type(metadata_ty),
        )?,
    ])
}

fn expand_newtype_struct_metadata(
    fields: FieldsUnnamed,
    accumulator: &proc_macro2::Ident,
) -> syn::Result<Vec<proc_macro2::TokenStream>> {
    if fields.unnamed.len() != 1 {
        return Err(syn::Error::new_spanned(
            fields,
            "TierConfig only supports tuple structs with exactly one field",
        ));
    }

    let field = fields.unnamed.into_iter().next().expect("single field");
    if parse_tier_attrs(&field.attrs)?.has_any() || has_field_naming_attrs(&field.attrs)? {
        return Err(syn::Error::new_spanned(
            field,
            "tuple struct wrappers cannot use field-level tier or serde naming attributes",
        ));
    }

    let metadata_ty = metadata_target_type(&field.ty);
    Ok(vec![quote! {
        #accumulator.extend(<#metadata_ty as ::tier::TierMetadata>::metadata());
    }])
}

fn expand_newtype_variant_metadata(
    fields: FieldsUnnamed,
    representation: &EnumRepresentation,
    span: proc_macro2::Span,
    accumulator: &proc_macro2::Ident,
) -> syn::Result<Vec<proc_macro2::TokenStream>> {
    if fields.unnamed.len() != 1 {
        return Err(syn::Error::new(
            span,
            "TierConfig only supports enum tuple variants with exactly one field",
        ));
    }

    if matches!(representation, EnumRepresentation::Internal { .. }) {
        return Err(syn::Error::new(
            span,
            "internally tagged enums with tuple variants are not supported by TierConfig metadata",
        ));
    }

    let field = fields.unnamed.into_iter().next().expect("single field");
    if parse_tier_attrs(&field.attrs)?.has_any() || has_field_naming_attrs(&field.attrs)? {
        return Err(syn::Error::new_spanned(
            field,
            "tuple enum variants cannot use field-level tier or serde naming attributes",
        ));
    }

    let metadata_ty = metadata_target_type(&field.ty);
    Ok(vec![quote! {
        #accumulator.extend(<#metadata_ty as ::tier::TierMetadata>::metadata());
    }])
}

fn ensure_struct_patch_container_attrs(container_attrs: &SerdeContainerAttrs) -> syn::Result<()> {
    if container_attrs.rename_all_fields_serialize.is_some()
        || container_attrs.rename_all_fields_deserialize.is_some()
        || container_attrs.tag.is_some()
        || container_attrs.content.is_some()
        || container_attrs.untagged
    {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "TierPatch only supports struct-style serde container attributes",
        ));
    }

    Ok(())
}

fn expand_patch_fields_metadata(
    fields: FieldsNamed,
    context: SerdeFieldContext,
) -> syn::Result<Vec<proc_macro2::TokenStream>> {
    let mut field_tokens = Vec::new();

    for field in fields.named {
        field_tokens.push(expand_patch_field_metadata(field, context)?);
    }

    Ok(field_tokens)
}

fn expand_patch_field_metadata(
    field: Field,
    context: SerdeFieldContext,
) -> syn::Result<proc_macro2::TokenStream> {
    let field_ident = field.ident.expect("named field");
    let serde_attrs = parse_serde_field_attrs(&field.attrs, &field_ident, context)?;
    let attrs = parse_patch_attrs(&field.attrs)?;

    if serde_attrs.skip_metadata {
        if attrs.path.is_some() || attrs.nested {
            return Err(syn::Error::new_spanned(
                field_ident,
                "skipped fields cannot use tier patch attributes",
            ));
        }
        return Ok(quote! {});
    }

    if serde_attrs.flatten && attrs.path.is_some() {
        return Err(syn::Error::new_spanned(
            field_ident,
            "flattened patch fields cannot override their tier path",
        ));
    }

    let default_path = attrs
        .path
        .clone()
        .unwrap_or_else(|| serde_attrs.canonical_name.clone());
    let path_lit = LitStr::new(&default_path, field_ident.span());
    let path_expr = if serde_attrs.flatten {
        quote! { ::std::string::String::from(__tier_prefix) }
    } else {
        quote! { ::tier::patch::join_patch_prefix(__tier_prefix, #path_lit) }
    };
    let field_access = quote! { &self.#field_ident };

    if serde_attrs.flatten || attrs.nested {
        return Ok(generate_nested_patch_tokens(
            &field.ty,
            field_access,
            path_expr,
        ));
    }

    Ok(generate_leaf_patch_tokens(
        &field.ty,
        field_access,
        path_expr,
    ))
}

fn generate_nested_patch_tokens(
    field_ty: &Type,
    field_access: proc_macro2::TokenStream,
    path_expr: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    if option_inner_type(field_ty).is_some() {
        quote! {
            if let ::std::option::Option::Some(value) = #field_access {
                let __tier_path = #path_expr;
                ::tier::TierPatch::write_layer(value, __tier_builder, &__tier_path)?;
            }
        }
    } else if patch_inner_type(field_ty).is_some() {
        quote! {
            if let ::std::option::Option::Some(value) = #field_access.as_ref() {
                let __tier_path = #path_expr;
                ::tier::TierPatch::write_layer(value, __tier_builder, &__tier_path)?;
            }
        }
    } else {
        quote! {
            {
                let __tier_path = #path_expr;
                ::tier::TierPatch::write_layer(#field_access, __tier_builder, &__tier_path)?;
            }
        }
    }
}

fn generate_leaf_patch_tokens(
    field_ty: &Type,
    field_access: proc_macro2::TokenStream,
    path_expr: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    if option_inner_type(field_ty).is_some() {
        quote! {
            if let ::std::option::Option::Some(value) = #field_access {
                let __tier_path = #path_expr;
                __tier_builder.insert_serialized(&__tier_path, value)?;
            }
        }
    } else if patch_inner_type(field_ty).is_some() {
        quote! {
            if let ::std::option::Option::Some(value) = #field_access.as_ref() {
                let __tier_path = #path_expr;
                __tier_builder.insert_serialized(&__tier_path, value)?;
            }
        }
    } else {
        quote! {
            {
                let __tier_path = #path_expr;
                __tier_builder.insert_serialized(&__tier_path, #field_access)?;
            }
        }
    }
}

#[derive(Debug, Default)]
struct TierAttrs {
    secret: bool,
    env: Option<String>,
    doc: Option<String>,
    example: Option<String>,
    deprecated: Option<String>,
    merge: Option<String>,
    non_empty: bool,
    min: Option<NumericLiteral>,
    max: Option<NumericLiteral>,
    min_length: Option<usize>,
    max_length: Option<usize>,
    one_of: Vec<Expr>,
    hostname: bool,
    ip_addr: bool,
    socket_addr: bool,
    absolute_path: bool,
    env_decode: Option<String>,
}

impl TierAttrs {
    fn has_any(&self) -> bool {
        self.secret
            || self.env.is_some()
            || self.doc.is_some()
            || self.example.is_some()
            || self.deprecated.is_some()
            || self.merge.is_some()
            || self.non_empty
            || self.min.is_some()
            || self.max.is_some()
            || self.min_length.is_some()
            || self.max_length.is_some()
            || !self.one_of.is_empty()
            || self.hostname
            || self.ip_addr
            || self.socket_addr
            || self.absolute_path
            || self.env_decode.is_some()
    }
}

#[derive(Debug, Default)]
struct PatchAttrs {
    path: Option<String>,
    nested: bool,
}

#[derive(Debug, Default)]
struct TierContainerAttrs {
    checks: Vec<ContainerValidationCheck>,
}

#[derive(Debug, Clone)]
struct NumericLiteral {
    tokens: proc_macro2::TokenStream,
    value: f64,
}

#[derive(Debug, Clone)]
enum ContainerValidationCheck {
    AtLeastOneOf(Vec<String>),
    ExactlyOneOf(Vec<String>),
    MutuallyExclusive(Vec<String>),
    RequiredWith {
        path: String,
        requires: Vec<String>,
    },
    RequiredIf {
        path: String,
        equals: Expr,
        requires: Vec<String>,
    },
}

#[derive(Debug, Default)]
struct SerdeContainerAttrs {
    rename_all_serialize: Option<RenameRule>,
    rename_all_deserialize: Option<RenameRule>,
    rename_all_fields_serialize: Option<RenameRule>,
    rename_all_fields_deserialize: Option<RenameRule>,
    default_fields: bool,
    tag: Option<String>,
    content: Option<String>,
    untagged: bool,
}

#[derive(Debug, Clone, Copy, Default)]
struct SerdeFieldContext {
    rename_serialize: Option<RenameRule>,
    rename_deserialize: Option<RenameRule>,
    default_fields: bool,
}

impl SerdeFieldContext {
    fn for_struct(container_attrs: &SerdeContainerAttrs) -> Self {
        Self {
            rename_serialize: container_attrs.rename_all_serialize,
            rename_deserialize: container_attrs.rename_all_deserialize,
            default_fields: container_attrs.default_fields,
        }
    }

    fn for_enum_variant_fields(container_attrs: &SerdeContainerAttrs) -> Self {
        Self {
            rename_serialize: container_attrs.rename_all_fields_serialize,
            rename_deserialize: container_attrs.rename_all_fields_deserialize,
            default_fields: false,
        }
    }
}

#[derive(Debug, Default)]
struct SerdeFieldAttrs {
    canonical_name: String,
    aliases: Vec<String>,
    flatten: bool,
    skip_metadata: bool,
    has_default: bool,
}

#[derive(Debug, Default)]
struct SerdeVariantAttrs {
    canonical_name: String,
    aliases: Vec<String>,
    skip_metadata: bool,
}

#[derive(Debug, Default)]
struct NonExternalFieldConflicts {
    skipped_fields: HashSet<String>,
    skipped_aliases: HashSet<String>,
    skipped_envs: HashSet<String>,
}

#[derive(Debug, Clone)]
enum EnumRepresentation {
    External,
    Internal { tag: String },
    Adjacent { tag: String, content: String },
    Untagged,
}

impl EnumRepresentation {
    fn tag_field(&self) -> Option<&str> {
        match self {
            Self::Internal { tag } => Some(tag.as_str()),
            Self::Adjacent { tag, .. } => Some(tag.as_str()),
            Self::External | Self::Untagged => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenameRule {
    Lower,
    Upper,
    Pascal,
    Camel,
    Snake,
    ScreamingSnake,
    Kebab,
    ScreamingKebab,
}

impl RenameRule {
    fn parse(value: &str, span: proc_macro2::Span) -> syn::Result<Self> {
        match value {
            "lowercase" => Ok(Self::Lower),
            "UPPERCASE" => Ok(Self::Upper),
            "PascalCase" => Ok(Self::Pascal),
            "camelCase" => Ok(Self::Camel),
            "snake_case" => Ok(Self::Snake),
            "SCREAMING_SNAKE_CASE" => Ok(Self::ScreamingSnake),
            "kebab-case" => Ok(Self::Kebab),
            "SCREAMING-KEBAB-CASE" => Ok(Self::ScreamingKebab),
            _ => Err(syn::Error::new(
                span,
                "unsupported serde rename rule for TierConfig",
            )),
        }
    }

    fn apply_to_field(self, value: &str) -> String {
        match self {
            Self::Lower | Self::Snake => value.to_owned(),
            Self::Upper | Self::ScreamingSnake => value.to_ascii_uppercase(),
            Self::Pascal => {
                let mut output = String::new();
                let mut capitalize = true;
                for ch in value.chars() {
                    if ch == '_' {
                        capitalize = true;
                    } else if capitalize {
                        output.push(ch.to_ascii_uppercase());
                        capitalize = false;
                    } else {
                        output.push(ch);
                    }
                }
                output
            }
            Self::Camel => {
                let pascal = Self::Pascal.apply_to_field(value);
                lowercase_first_char(&pascal)
            }
            Self::Kebab => value.replace('_', "-"),
            Self::ScreamingKebab => value.replace('_', "-").to_ascii_uppercase(),
        }
    }

    fn apply_to_variant(self, value: &str) -> String {
        match self {
            Self::Lower => value.to_ascii_lowercase(),
            Self::Upper => value.to_ascii_uppercase(),
            Self::Pascal => value.to_owned(),
            Self::Camel => lowercase_first_char(value),
            Self::Snake => {
                let mut output = String::new();
                for (index, ch) in value.char_indices() {
                    if index > 0 && ch.is_uppercase() {
                        output.push('_');
                    }
                    output.push(ch.to_ascii_lowercase());
                }
                output
            }
            Self::ScreamingSnake => Self::Snake.apply_to_variant(value).to_ascii_uppercase(),
            Self::Kebab => Self::Snake.apply_to_variant(value).replace('_', "-"),
            Self::ScreamingKebab => Self::Kebab.apply_to_variant(value).to_ascii_uppercase(),
        }
    }
}

fn lowercase_first_char(value: &str) -> String {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };

    let mut output = first.to_ascii_lowercase().to_string();
    output.push_str(chars.as_str());
    output
}

fn parse_tier_attrs(attributes: &[Attribute]) -> syn::Result<TierAttrs> {
    let mut attrs = TierAttrs::default();
    for attribute in attributes {
        if !attribute.path().is_ident("tier") {
            continue;
        }
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("secret") {
                attrs.secret = true;
                return Ok(());
            }
            if meta.path.is_ident("env") {
                attrs.env = Some(parse_string_value(meta)?);
                return Ok(());
            }
            if meta.path.is_ident("doc") {
                attrs.doc = Some(parse_string_value(meta)?);
                return Ok(());
            }
            if meta.path.is_ident("example") {
                attrs.example = Some(parse_string_value(meta)?);
                return Ok(());
            }
            if meta.path.is_ident("deprecated") {
                attrs.deprecated = Some(if meta.input.peek(syn::Token![=]) {
                    parse_string_value(meta)?
                } else {
                    "this field is deprecated".to_owned()
                });
                return Ok(());
            }
            if meta.path.is_ident("merge") {
                attrs.merge = Some(parse_string_value(meta)?);
                return Ok(());
            }
            if meta.path.is_ident("non_empty") {
                attrs.non_empty = true;
                consume_unused_meta(meta)?;
                return Ok(());
            }
            if meta.path.is_ident("min") {
                attrs.min = Some(parse_numeric_literal(meta)?);
                return Ok(());
            }
            if meta.path.is_ident("max") {
                attrs.max = Some(parse_numeric_literal(meta)?);
                return Ok(());
            }
            if meta.path.is_ident("min_length") {
                attrs.min_length = Some(parse_usize_value(meta)?);
                return Ok(());
            }
            if meta.path.is_ident("max_length") {
                attrs.max_length = Some(parse_usize_value(meta)?);
                return Ok(());
            }
            if meta.path.is_ident("one_of") {
                attrs.one_of = parse_literal_expr_list(meta)?;
                return Ok(());
            }
            if meta.path.is_ident("hostname") {
                attrs.hostname = true;
                consume_unused_meta(meta)?;
                return Ok(());
            }
            if meta.path.is_ident("ip_addr") {
                attrs.ip_addr = true;
                consume_unused_meta(meta)?;
                return Ok(());
            }
            if meta.path.is_ident("socket_addr") {
                attrs.socket_addr = true;
                consume_unused_meta(meta)?;
                return Ok(());
            }
            if meta.path.is_ident("absolute_path") {
                attrs.absolute_path = true;
                consume_unused_meta(meta)?;
                return Ok(());
            }
            if meta.path.is_ident("env_decode") {
                attrs.env_decode = Some(parse_string_value(meta)?);
                return Ok(());
            }
            Err(meta.error("unsupported tier attribute"))
        })?;
    }
    Ok(attrs)
}

fn parse_patch_attrs(attributes: &[Attribute]) -> syn::Result<PatchAttrs> {
    let mut attrs = PatchAttrs::default();
    for attribute in attributes {
        if !attribute.path().is_ident("tier") {
            continue;
        }
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("path") {
                attrs.path = Some(parse_string_value(meta)?);
                return Ok(());
            }
            if meta.path.is_ident("nested") {
                attrs.nested = true;
                consume_unused_meta(meta)?;
                return Ok(());
            }
            Err(meta.error("unsupported tier patch attribute"))
        })?;
    }
    Ok(attrs)
}

fn parse_tier_container_attrs(attributes: &[Attribute]) -> syn::Result<TierContainerAttrs> {
    let mut attrs = TierContainerAttrs::default();

    for attribute in attributes {
        if !attribute.path().is_ident("tier") {
            continue;
        }

        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("at_least_one_of") {
                attrs.checks.push(ContainerValidationCheck::AtLeastOneOf(
                    parse_string_list_call(meta)?,
                ));
                return Ok(());
            }
            if meta.path.is_ident("exactly_one_of") {
                attrs.checks.push(ContainerValidationCheck::ExactlyOneOf(
                    parse_string_list_call(meta)?,
                ));
                return Ok(());
            }
            if meta.path.is_ident("mutually_exclusive") {
                attrs
                    .checks
                    .push(ContainerValidationCheck::MutuallyExclusive(
                        parse_string_list_call(meta)?,
                    ));
                return Ok(());
            }
            if meta.path.is_ident("required_with") {
                attrs
                    .checks
                    .push(parse_required_with_container_check(meta)?);
                return Ok(());
            }
            if meta.path.is_ident("required_if") {
                attrs.checks.push(parse_required_if_container_check(meta)?);
                return Ok(());
            }
            Err(meta.error("unsupported tier container attribute"))
        })?;
    }

    Ok(attrs)
}

fn parse_serde_container_attrs(attributes: &[Attribute]) -> syn::Result<SerdeContainerAttrs> {
    let mut attrs = SerdeContainerAttrs::default();
    for attribute in attributes {
        if !attribute.path().is_ident("serde") {
            continue;
        }

        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename_all") {
                parse_rename_all_meta(
                    meta,
                    &mut attrs.rename_all_serialize,
                    &mut attrs.rename_all_deserialize,
                )?;
                return Ok(());
            }
            if meta.path.is_ident("rename_all_fields") {
                parse_rename_all_meta(
                    meta,
                    &mut attrs.rename_all_fields_serialize,
                    &mut attrs.rename_all_fields_deserialize,
                )?;
                return Ok(());
            }
            if meta.path.is_ident("default") {
                attrs.default_fields = true;
                consume_unused_meta(meta)?;
                return Ok(());
            }
            if meta.path.is_ident("tag") {
                attrs.tag = Some(parse_string_value(meta)?);
                return Ok(());
            }
            if meta.path.is_ident("content") {
                attrs.content = Some(parse_string_value(meta)?);
                return Ok(());
            }
            if meta.path.is_ident("untagged") {
                attrs.untagged = true;
                consume_unused_meta(meta)?;
                return Ok(());
            }
            consume_unused_meta(meta)?;
            Ok(())
        })?;
    }

    Ok(attrs)
}

fn parse_serde_field_attrs(
    attributes: &[Attribute],
    field_ident: &syn::Ident,
    context: SerdeFieldContext,
) -> syn::Result<SerdeFieldAttrs> {
    let base_name = unraw(field_ident);
    let mut rename_serialize = None;
    let mut rename_deserialize = None;
    let mut aliases = Vec::new();
    let mut flatten = false;
    let mut skip_metadata = false;
    let mut has_default = context.default_fields;

    for attribute in attributes {
        if !attribute.path().is_ident("serde") {
            continue;
        }

        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename") {
                parse_rename_meta(meta, &mut rename_serialize, &mut rename_deserialize)?;
                return Ok(());
            }
            if meta.path.is_ident("alias") {
                aliases.push(parse_string_value(meta)?);
                return Ok(());
            }
            if meta.path.is_ident("flatten") {
                flatten = true;
                return Ok(());
            }
            if meta.path.is_ident("default") {
                has_default = true;
                consume_unused_meta(meta)?;
                return Ok(());
            }
            if meta.path.is_ident("skip") || meta.path.is_ident("skip_deserializing") {
                skip_metadata = true;
                return Ok(());
            }
            consume_unused_meta(meta)?;
            Ok(())
        })?;
    }

    let has_explicit_rename = rename_serialize.is_some() || rename_deserialize.is_some();

    let canonical_name = rename_serialize
        .or_else(|| {
            context
                .rename_serialize
                .map(|rule| rule.apply_to_field(&base_name))
        })
        .unwrap_or_else(|| base_name.clone());
    let deserialize_name = rename_deserialize
        .or_else(|| {
            context
                .rename_deserialize
                .map(|rule| rule.apply_to_field(&base_name))
        })
        .unwrap_or_else(|| base_name.clone());

    if deserialize_name != canonical_name {
        aliases.push(deserialize_name);
    }

    if flatten && (!aliases.is_empty() || has_explicit_rename) {
        return Err(syn::Error::new_spanned(
            field_ident,
            "flattened fields cannot use serde rename or alias attributes",
        ));
    }

    aliases.retain(|alias| alias != &canonical_name);
    aliases.sort();
    aliases.dedup();

    Ok(SerdeFieldAttrs {
        canonical_name,
        aliases,
        flatten,
        skip_metadata,
        has_default,
    })
}

fn parse_serde_variant_attrs(
    attributes: &[Attribute],
    variant_ident: &syn::Ident,
    container_attrs: &SerdeContainerAttrs,
) -> syn::Result<SerdeVariantAttrs> {
    let base_name = unraw(variant_ident);
    let mut rename_serialize = None;
    let mut rename_deserialize = None;
    let mut aliases = Vec::new();
    let mut skip_metadata = false;

    for attribute in attributes {
        if !attribute.path().is_ident("serde") {
            continue;
        }

        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename") {
                parse_rename_meta(meta, &mut rename_serialize, &mut rename_deserialize)?;
                return Ok(());
            }
            if meta.path.is_ident("alias") {
                aliases.push(parse_string_value(meta)?);
                return Ok(());
            }
            if meta.path.is_ident("skip")
                || meta.path.is_ident("skip_deserializing")
                || meta.path.is_ident("other")
            {
                skip_metadata = true;
                consume_unused_meta(meta)?;
                return Ok(());
            }
            consume_unused_meta(meta)?;
            Ok(())
        })?;
    }

    let canonical_name = rename_serialize
        .or_else(|| {
            container_attrs
                .rename_all_serialize
                .map(|rule| rule.apply_to_variant(&base_name))
        })
        .unwrap_or_else(|| base_name.clone());
    let deserialize_name = rename_deserialize
        .or_else(|| {
            container_attrs
                .rename_all_deserialize
                .map(|rule| rule.apply_to_variant(&base_name))
        })
        .unwrap_or_else(|| base_name.clone());

    if deserialize_name != canonical_name {
        aliases.push(deserialize_name);
    }

    aliases.retain(|alias| alias != &canonical_name);
    aliases.sort();
    aliases.dedup();

    Ok(SerdeVariantAttrs {
        canonical_name,
        aliases,
        skip_metadata,
    })
}

fn ensure_struct_container_attrs(container_attrs: &SerdeContainerAttrs) -> syn::Result<()> {
    if container_attrs.rename_all_fields_serialize.is_some()
        || container_attrs.rename_all_fields_deserialize.is_some()
    {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "serde(rename_all_fields = ...) is only supported on enums",
        ));
    }
    if container_attrs.tag.is_some()
        || container_attrs.content.is_some()
        || container_attrs.untagged
    {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "serde enum tagging attributes are not supported on structs",
        ));
    }
    Ok(())
}

fn enum_representation(container_attrs: &SerdeContainerAttrs) -> syn::Result<EnumRepresentation> {
    if container_attrs.untagged && container_attrs.tag.is_some() {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "serde(untagged) cannot be combined with serde(tag = ...)",
        ));
    }
    if container_attrs.untagged && container_attrs.content.is_some() {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "serde(untagged) cannot be combined with serde(content = ...)",
        ));
    }
    if container_attrs.content.is_some() && container_attrs.tag.is_none() {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "serde(content = ...) requires serde(tag = ...)",
        ));
    }

    if container_attrs.untagged {
        return Ok(EnumRepresentation::Untagged);
    }

    match (&container_attrs.tag, &container_attrs.content) {
        (Some(tag), Some(content)) => Ok(EnumRepresentation::Adjacent {
            tag: tag.clone(),
            content: content.clone(),
        }),
        (Some(tag), None) => Ok(EnumRepresentation::Internal { tag: tag.clone() }),
        (None, None) => Ok(EnumRepresentation::External),
        (None, Some(_)) => unreachable!("validated above"),
    }
}

fn non_external_variant_field_conflicts(
    data_enum: &DataEnum,
    container_attrs: &SerdeContainerAttrs,
) -> syn::Result<NonExternalFieldConflicts> {
    let representation = enum_representation(container_attrs)?;
    if matches!(representation, EnumRepresentation::External) {
        return Ok(NonExternalFieldConflicts::default());
    }

    let context = SerdeFieldContext::for_enum_variant_fields(container_attrs);
    let mut counts = HashMap::<String, usize>::new();
    let mut canonical_names = HashSet::new();
    let mut alias_owners = HashMap::<String, HashSet<String>>::new();
    let mut env_owners = HashMap::<String, HashSet<String>>::new();

    for variant in &data_enum.variants {
        let variant_attrs =
            parse_serde_variant_attrs(&variant.attrs, &variant.ident, container_attrs)?;
        if variant_attrs.skip_metadata {
            continue;
        }

        let Fields::Named(fields) = &variant.fields else {
            continue;
        };

        let mut seen = HashSet::new();
        for field in &fields.named {
            let Some(field_ident) = &field.ident else {
                continue;
            };
            let serde_attrs = parse_serde_field_attrs(&field.attrs, field_ident, context)?;
            if serde_attrs.skip_metadata || serde_attrs.flatten {
                continue;
            }
            let tier_attrs = parse_tier_attrs(&field.attrs)?;
            let canonical_name = serde_attrs.canonical_name.clone();
            if seen.insert(canonical_name.clone()) {
                canonical_names.insert(canonical_name.clone());
                *counts.entry(canonical_name.clone()).or_default() += 1;
            }
            for alias in serde_attrs.aliases {
                alias_owners
                    .entry(alias)
                    .or_default()
                    .insert(canonical_name.clone());
            }
            if let Some(env) = tier_attrs.env {
                env_owners
                    .entry(env)
                    .or_default()
                    .insert(canonical_name.clone());
            }
        }
    }

    let skipped_fields = counts
        .into_iter()
        .filter_map(|(path, count)| (count > 1).then_some(path))
        .collect::<HashSet<_>>();

    let skipped_aliases = alias_owners
        .into_iter()
        .filter_map(|(alias, owners)| {
            (owners.len() > 1 || canonical_names.contains(&alias)).then_some(alias)
        })
        .collect::<HashSet<_>>();

    let skipped_envs = env_owners
        .into_iter()
        .filter_map(|(env, owners)| (owners.len() > 1).then_some(env))
        .collect::<HashSet<_>>();

    Ok(NonExternalFieldConflicts {
        skipped_fields,
        skipped_aliases,
        skipped_envs,
    })
}

fn has_field_naming_attrs(attributes: &[Attribute]) -> syn::Result<bool> {
    let mut has_naming = false;
    for attribute in attributes {
        if !attribute.path().is_ident("serde") {
            continue;
        }

        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename")
                || meta.path.is_ident("alias")
                || meta.path.is_ident("flatten")
                || meta.path.is_ident("default")
            {
                has_naming = true;
            }
            consume_unused_meta(meta)?;
            Ok(())
        })?;
    }

    Ok(has_naming)
}

fn validate_merge_strategy(attrs: &TierAttrs, ty: &Type) -> syn::Result<()> {
    if attrs.merge.as_deref() == Some("append") && !supports_append_strategy(ty) {
        return Err(syn::Error::new_spanned(
            ty,
            "tier(merge = \"append\") requires a Vec<T> or array-like field",
        ));
    }
    Ok(())
}

fn validate_validation_attrs(attrs: &TierAttrs, field_ident: &syn::Ident) -> syn::Result<()> {
    if let (Some(min), Some(max)) = (&attrs.min, &attrs.max)
        && min.value > max.value
    {
        return Err(syn::Error::new_spanned(
            field_ident,
            "tier(min = ...) cannot be greater than tier(max = ...)",
        ));
    }

    if let (Some(min_length), Some(max_length)) = (attrs.min_length, attrs.max_length)
        && min_length > max_length
    {
        return Err(syn::Error::new_spanned(
            field_ident,
            "tier(min_length = ...) cannot be greater than tier(max_length = ...)",
        ));
    }

    if attrs.one_of.is_empty()
        && (attrs.hostname || attrs.ip_addr || attrs.socket_addr || attrs.absolute_path)
    {
        return Ok(());
    }

    if !attrs.one_of.is_empty() && (attrs.min.is_some() || attrs.max.is_some()) {
        return Err(syn::Error::new_spanned(
            field_ident,
            "tier(one_of(...)) cannot be combined with tier(min = ...) or tier(max = ...)",
        ));
    }

    Ok(())
}

fn container_check_tokens(attrs: &TierContainerAttrs) -> Vec<proc_macro2::TokenStream> {
    attrs
        .checks
        .iter()
        .map(|check| match check {
            ContainerValidationCheck::AtLeastOneOf(paths) => {
                let path_lits = paths
                    .iter()
                    .map(|path| LitStr::new(path, proc_macro2::Span::call_site()))
                    .collect::<Vec<_>>();
                quote! {
                    metadata.push_check(::tier::ValidationCheck::AtLeastOneOf {
                        paths: ::std::vec![#(::std::string::String::from(#path_lits)),*],
                    });
                }
            }
            ContainerValidationCheck::ExactlyOneOf(paths) => {
                let path_lits = paths
                    .iter()
                    .map(|path| LitStr::new(path, proc_macro2::Span::call_site()))
                    .collect::<Vec<_>>();
                quote! {
                    metadata.push_check(::tier::ValidationCheck::ExactlyOneOf {
                        paths: ::std::vec![#(::std::string::String::from(#path_lits)),*],
                    });
                }
            }
            ContainerValidationCheck::MutuallyExclusive(paths) => {
                let path_lits = paths
                    .iter()
                    .map(|path| LitStr::new(path, proc_macro2::Span::call_site()))
                    .collect::<Vec<_>>();
                quote! {
                    metadata.push_check(::tier::ValidationCheck::MutuallyExclusive {
                        paths: ::std::vec![#(::std::string::String::from(#path_lits)),*],
                    });
                }
            }
            ContainerValidationCheck::RequiredWith { path, requires } => {
                let path = LitStr::new(path, proc_macro2::Span::call_site());
                let requires = requires
                    .iter()
                    .map(|item| LitStr::new(item, proc_macro2::Span::call_site()))
                    .collect::<Vec<_>>();
                quote! {
                    metadata.push_check(::tier::ValidationCheck::RequiredWith {
                        path: ::std::string::String::from(#path),
                        requires: ::std::vec![#(::std::string::String::from(#requires)),*],
                    });
                }
            }
            ContainerValidationCheck::RequiredIf {
                path,
                equals,
                requires,
            } => {
                let path = LitStr::new(path, proc_macro2::Span::call_site());
                let requires = requires
                    .iter()
                    .map(|item| LitStr::new(item, proc_macro2::Span::call_site()))
                    .collect::<Vec<_>>();
                quote! {
                    metadata.push_check(::tier::ValidationCheck::RequiredIf {
                        path: ::std::string::String::from(#path),
                        equals: ::tier::ValidationValue::from(#equals),
                        requires: ::std::vec![#(::std::string::String::from(#requires)),*],
                    });
                }
            }
        })
        .collect()
}

fn supports_append_strategy(ty: &Type) -> bool {
    let Some(inner) = metadata_inner_type(ty) else {
        return matches!(ty, Type::Array(_))
            || matches!(last_type_ident(ty).as_deref(), Some("Vec"));
    };
    supports_append_strategy(inner)
}

fn parse_rename_all_meta(
    meta: syn::meta::ParseNestedMeta<'_>,
    serialize: &mut Option<RenameRule>,
    deserialize: &mut Option<RenameRule>,
) -> syn::Result<()> {
    if meta.input.peek(syn::Token![=]) {
        let literal: LitStr = meta.value()?.parse()?;
        let rule = RenameRule::parse(&literal.value(), literal.span())?;
        *serialize = Some(rule);
        *deserialize = Some(rule);
        return Ok(());
    }

    meta.parse_nested_meta(|nested| {
        if nested.path.is_ident("serialize") {
            let literal: LitStr = nested.value()?.parse()?;
            *serialize = Some(RenameRule::parse(&literal.value(), literal.span())?);
            return Ok(());
        }
        if nested.path.is_ident("deserialize") {
            let literal: LitStr = nested.value()?.parse()?;
            *deserialize = Some(RenameRule::parse(&literal.value(), literal.span())?);
            return Ok(());
        }
        Err(nested.error("unsupported serde rename_all option"))
    })
}

fn parse_rename_meta(
    meta: syn::meta::ParseNestedMeta<'_>,
    serialize: &mut Option<String>,
    deserialize: &mut Option<String>,
) -> syn::Result<()> {
    if meta.input.peek(syn::Token![=]) {
        let value = parse_string_value(meta)?;
        *serialize = Some(value.clone());
        *deserialize = Some(value);
        return Ok(());
    }

    meta.parse_nested_meta(|nested| {
        if nested.path.is_ident("serialize") {
            *serialize = Some(parse_string_value(nested)?);
            return Ok(());
        }
        if nested.path.is_ident("deserialize") {
            *deserialize = Some(parse_string_value(nested)?);
            return Ok(());
        }
        Err(nested.error("unsupported serde rename option"))
    })
}

fn parse_string_value(meta: syn::meta::ParseNestedMeta<'_>) -> syn::Result<String> {
    let literal: LitStr = meta.value()?.parse()?;
    Ok(literal.value())
}

fn parse_usize_value(meta: syn::meta::ParseNestedMeta<'_>) -> syn::Result<usize> {
    let literal: syn::LitInt = meta.value()?.parse()?;
    literal.base10_parse()
}

fn parse_string_list_call(meta: syn::meta::ParseNestedMeta<'_>) -> syn::Result<Vec<String>> {
    let content;
    syn::parenthesized!(content in meta.input);
    let values = Punctuated::<LitStr, syn::Token![,]>::parse_terminated(&content)?;
    if values.is_empty() {
        return Err(meta.error("expected at least one string literal"));
    }
    Ok(values.into_iter().map(|value| value.value()).collect())
}

fn parse_literal_expr_list(meta: syn::meta::ParseNestedMeta<'_>) -> syn::Result<Vec<Expr>> {
    let content;
    syn::parenthesized!(content in meta.input);
    let values = Punctuated::<Expr, syn::Token![,]>::parse_terminated(&content)?;
    if values.is_empty() {
        return Err(meta.error("expected at least one literal value"));
    }
    let values = values.into_iter().collect::<Vec<_>>();
    for value in &values {
        validate_value_expr(value, value.span())?;
    }
    Ok(values)
}

fn parse_numeric_literal(meta: syn::meta::ParseNestedMeta<'_>) -> syn::Result<NumericLiteral> {
    let expr: Expr = meta.value()?.parse()?;
    parse_numeric_expr(expr, meta.path.span())
}

fn parse_numeric_expr(expr: Expr, span: proc_macro2::Span) -> syn::Result<NumericLiteral> {
    match expr {
        Expr::Lit(expr_lit) => match expr_lit.lit {
            Lit::Int(literal) => Ok(NumericLiteral {
                tokens: quote! { #literal },
                value: literal.base10_parse::<f64>()?,
            }),
            Lit::Float(literal) => Ok(NumericLiteral {
                tokens: quote! { #literal },
                value: literal.base10_parse::<f64>()?,
            }),
            _ => Err(syn::Error::new(
                span,
                "expected an integer or float literal",
            )),
        },
        Expr::Unary(expr_unary) if matches!(expr_unary.op, syn::UnOp::Neg(_)) => {
            match *expr_unary.expr {
                Expr::Lit(expr_lit) => match expr_lit.lit {
                    Lit::Int(literal) => Ok(NumericLiteral {
                        tokens: quote! { -#literal },
                        value: -literal.base10_parse::<f64>()?,
                    }),
                    Lit::Float(literal) => Ok(NumericLiteral {
                        tokens: quote! { -#literal },
                        value: -literal.base10_parse::<f64>()?,
                    }),
                    _ => Err(syn::Error::new(
                        span,
                        "expected an integer or float literal",
                    )),
                },
                _ => Err(syn::Error::new(
                    span,
                    "expected an integer or float literal",
                )),
            }
        }
        _ => Err(syn::Error::new(
            span,
            "expected an integer or float literal",
        )),
    }
}

fn parse_value_expr(meta: syn::meta::ParseNestedMeta<'_>) -> syn::Result<Expr> {
    let expr: Expr = meta.value()?.parse()?;
    validate_value_expr(&expr, meta.path.span())?;
    Ok(expr)
}

fn validate_value_expr(expr: &Expr, span: proc_macro2::Span) -> syn::Result<()> {
    match expr {
        Expr::Lit(expr_lit) => match &expr_lit.lit {
            Lit::Str(_) | Lit::Bool(_) | Lit::Int(_) | Lit::Float(_) => Ok(()),
            _ => Err(syn::Error::new(
                span,
                "expected a string, bool, integer, or float literal",
            )),
        },
        Expr::Unary(expr_unary) if matches!(expr_unary.op, syn::UnOp::Neg(_)) => match &*expr_unary
            .expr
        {
            Expr::Lit(expr_lit) if matches!(expr_lit.lit, Lit::Int(_) | Lit::Float(_)) => Ok(()),
            _ => Err(syn::Error::new(
                span,
                "expected a string, bool, integer, or float literal",
            )),
        },
        _ => Err(syn::Error::new(
            span,
            "expected a string, bool, integer, or float literal",
        )),
    }
}

fn parse_required_with_container_check(
    meta: syn::meta::ParseNestedMeta<'_>,
) -> syn::Result<ContainerValidationCheck> {
    let mut path = None;
    let mut requires = Vec::new();
    meta.parse_nested_meta(|nested| {
        if nested.path.is_ident("path") {
            path = Some(parse_string_value(nested)?);
            return Ok(());
        }
        if nested.path.is_ident("requires") {
            requires = parse_string_list_call(nested)?;
            return Ok(());
        }
        Err(nested.error("unsupported required_with option"))
    })?;

    let Some(path) = path else {
        return Err(meta.error("required_with requires `path = \"...\"`"));
    };
    if requires.is_empty() {
        return Err(meta.error("required_with requires `requires(\"...\")`"));
    }

    Ok(ContainerValidationCheck::RequiredWith { path, requires })
}

fn parse_required_if_container_check(
    meta: syn::meta::ParseNestedMeta<'_>,
) -> syn::Result<ContainerValidationCheck> {
    let mut path = None;
    let mut equals = None;
    let mut requires = Vec::new();
    meta.parse_nested_meta(|nested| {
        if nested.path.is_ident("path") {
            path = Some(parse_string_value(nested)?);
            return Ok(());
        }
        if nested.path.is_ident("equals") {
            equals = Some(parse_value_expr(nested)?);
            return Ok(());
        }
        if nested.path.is_ident("requires") {
            requires = parse_string_list_call(nested)?;
            return Ok(());
        }
        Err(nested.error("unsupported required_if option"))
    })?;

    let Some(path) = path else {
        return Err(meta.error("required_if requires `path = \"...\"`"));
    };
    let Some(equals) = equals else {
        return Err(meta.error("required_if requires `equals = ...`"));
    };
    if requires.is_empty() {
        return Err(meta.error("required_if requires `requires(\"...\")`"));
    }

    Ok(ContainerValidationCheck::RequiredIf {
        path,
        equals,
        requires,
    })
}

fn doc_comment(attributes: &[Attribute]) -> Option<String> {
    let mut lines = Vec::new();
    for attribute in attributes {
        if !attribute.path().is_ident("doc") {
            continue;
        }
        let Meta::NameValue(name_value) = &attribute.meta else {
            continue;
        };
        let Expr::Lit(expr_lit) = &name_value.value else {
            continue;
        };
        let Lit::Str(literal) = &expr_lit.lit else {
            continue;
        };
        let line = literal.value().trim().to_owned();
        if !line.is_empty() {
            lines.push(line);
        }
    }

    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn direct_field_metadata_tokens(
    accumulator: &proc_macro2::Ident,
    field_name: &LitStr,
    aliases: &[LitStr],
    serde_attrs: &SerdeFieldAttrs,
    attrs: &TierAttrs,
    secret_type: bool,
) -> syn::Result<proc_macro2::TokenStream> {
    let mut builder = quote! {
        ::tier::FieldMetadata::new(#field_name)
    };

    for alias in aliases {
        builder = quote! { #builder.alias(#alias) };
    }
    if attrs.secret || secret_type {
        builder = quote! { #builder.secret() };
    }
    if let Some(env) = &attrs.env {
        let env = LitStr::new(env, field_name.span());
        builder = quote! { #builder.env(#env) };
    }
    if let Some(doc) = &attrs.doc {
        let doc = LitStr::new(doc, field_name.span());
        builder = quote! { #builder.doc(#doc) };
    }
    if let Some(example) = &attrs.example {
        let example = LitStr::new(example, field_name.span());
        builder = quote! { #builder.example(#example) };
    }
    if let Some(deprecated) = &attrs.deprecated {
        let deprecated = LitStr::new(deprecated, field_name.span());
        builder = quote! { #builder.deprecated(#deprecated) };
    }
    if serde_attrs.has_default {
        builder = quote! { #builder.defaulted() };
    }
    if let Some(merge) = &attrs.merge {
        let merge_strategy = match merge.as_str() {
            "merge" => quote! { ::tier::MergeStrategy::Merge },
            "replace" => quote! { ::tier::MergeStrategy::Replace },
            "append" => quote! { ::tier::MergeStrategy::Append },
            _ => {
                return Err(syn::Error::new(
                    field_name.span(),
                    "unsupported tier merge strategy, expected merge|replace|append",
                ));
            }
        };
        builder = quote! { #builder.merge_strategy(#merge_strategy) };
    }
    if attrs.non_empty {
        builder = quote! { #builder.non_empty() };
    }
    if let Some(min) = &attrs.min {
        let min = &min.tokens;
        builder = quote! { #builder.min(#min) };
    }
    if let Some(max) = &attrs.max {
        let max = &max.tokens;
        builder = quote! { #builder.max(#max) };
    }
    if let Some(min_length) = attrs.min_length {
        builder = quote! { #builder.min_length(#min_length) };
    }
    if let Some(max_length) = attrs.max_length {
        builder = quote! { #builder.max_length(#max_length) };
    }
    if !attrs.one_of.is_empty() {
        let one_of = &attrs.one_of;
        builder = quote! { #builder.one_of([#(#one_of),*]) };
    }
    if attrs.hostname {
        builder = quote! { #builder.hostname() };
    }
    if attrs.ip_addr {
        builder = quote! { #builder.ip_addr() };
    }
    if attrs.socket_addr {
        builder = quote! { #builder.socket_addr() };
    }
    if attrs.absolute_path {
        builder = quote! { #builder.absolute_path() };
    }
    if let Some(env_decode) = &attrs.env_decode {
        let env_decode = match env_decode.as_str() {
            "csv" => quote! { ::tier::EnvDecoder::Csv },
            "path_list" => quote! { ::tier::EnvDecoder::PathList },
            "key_value_map" => quote! { ::tier::EnvDecoder::KeyValueMap },
            "whitespace" => quote! { ::tier::EnvDecoder::Whitespace },
            _ => {
                return Err(syn::Error::new(
                    field_name.span(),
                    "unsupported tier env decoder, expected csv|path_list|key_value_map|whitespace",
                ));
            }
        };
        builder = quote! { #builder.env_decoder(#env_decode) };
    }

    Ok(quote! {
        #accumulator.push(#builder);
    })
}

fn is_secret_type(ty: &Type) -> bool {
    matches!(last_type_ident(ty).as_deref(), Some("Secret"))
}

fn metadata_target_type(ty: &Type) -> &Type {
    let Some(inner) = metadata_inner_type(ty) else {
        return ty;
    };
    metadata_target_type(inner)
}

fn metadata_inner_type(ty: &Type) -> Option<&Type> {
    let Type::Path(type_path) = ty else {
        return None;
    };
    let segment = type_path.path.segments.last()?;
    match segment.ident.to_string().as_str() {
        "Option" | "Box" | "Arc" => match &segment.arguments {
            PathArguments::AngleBracketed(arguments) => {
                arguments.args.iter().find_map(|argument| {
                    if let GenericArgument::Type(ty) = argument {
                        Some(ty)
                    } else {
                        None
                    }
                })
            }
            _ => None,
        },
        _ => None,
    }
}

fn option_inner_type(ty: &Type) -> Option<&Type> {
    wrapper_inner_type(ty, "Option")
}

fn patch_inner_type(ty: &Type) -> Option<&Type> {
    wrapper_inner_type(ty, "Patch")
}

fn wrapper_inner_type<'a>(ty: &'a Type, wrapper: &str) -> Option<&'a Type> {
    let Type::Path(type_path) = ty else {
        return None;
    };
    let segment = type_path.path.segments.last()?;
    if segment.ident != wrapper {
        return None;
    }
    match &segment.arguments {
        PathArguments::AngleBracketed(arguments) => arguments.args.iter().find_map(|argument| {
            if let GenericArgument::Type(ty) = argument {
                Some(ty)
            } else {
                None
            }
        }),
        _ => None,
    }
}

fn last_type_ident(ty: &Type) -> Option<String> {
    let Type::Path(type_path) = ty else {
        return None;
    };
    type_path
        .path
        .segments
        .last()
        .map(|segment| segment.ident.to_string())
}

fn unraw(ident: &syn::Ident) -> String {
    ident.to_string().trim_start_matches("r#").to_owned()
}

fn consume_unused_meta(meta: syn::meta::ParseNestedMeta<'_>) -> syn::Result<()> {
    if meta.input.peek(syn::Token![=]) {
        let _: Expr = meta.value()?.parse()?;
        return Ok(());
    }

    if meta.input.peek(syn::token::Paren) {
        meta.parse_nested_meta(|nested| {
            consume_unused_meta(nested)?;
            Ok(())
        })?;
    }

    Ok(())
}
