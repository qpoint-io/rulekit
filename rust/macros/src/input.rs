//! `#[derive(Input)]`. Paths follow `#[serde(...)]` attributes so a rule
//! reads a value by the path serde_json would write it at, like schemars'
//! `JsonSchema` derive.

use std::collections::HashSet;

use proc_macro2::{Span, TokenStream as TokenStream2, TokenTree};
use quote::{format_ident, quote, quote_spanned};
use syn::ext::IdentExt;
use syn::meta::ParseNestedMeta;
use syn::spanned::Spanned;
use syn::{
    Attribute, Data, DeriveInput, Error, ExprPath, Fields, LitStr, Member, Token, Type, Variant,
};

use crate::case::RenameRule;

pub fn expand(input: &DeriveInput) -> Result<TokenStream2, Error> {
    let attrs = ContainerAttrs::parse(&input.attrs)?;
    let ctx = attrs
        .context
        .clone()
        .unwrap_or_else(|| syn::parse_quote!(()));
    let mut cx = Cx {
        ctx,
        types: Vec::new(),
    };

    let body = match &input.data {
        Data::Struct(data) => {
            if attrs.untagged || attrs.content.is_some() || attrs.rename_all_fields.is_some() {
                return Err(Error::new(
                    input.ident.span(),
                    "`untagged`, `content`, and `rename_all_fields` apply to enums",
                ));
            }
            let shape = Shape::parse(&data.fields, attrs.rename_all, attrs.transparent)?;
            let tag = match (&attrs.tag, &shape) {
                (None, _) => None,
                (Some(tag), Shape::Struct(_)) => Some(Tag {
                    key: tag.clone(),
                    name: attrs
                        .rename
                        .clone()
                        .unwrap_or_else(|| input.ident.unraw().to_string()),
                }),
                (Some(_), _) => {
                    return Err(Error::new(
                        input.ident.span(),
                        "`#[serde(tag)]` on a struct needs named fields",
                    ));
                }
            };
            cx.lookup(&shape, &quote!(path), tag.as_ref(), &|f| {
                let member = &f.member;
                quote!(&self.#member)
            })?
        }
        Data::Enum(data) => expand_enum(&mut cx, &attrs, input.ident.span(), data.variants.iter())?,
        Data::Union(_) => {
            return Err(Error::new(
                input.ident.span(),
                "`Input` can only be derived for a struct or an enum",
            ));
        }
    };

    let ident = &input.ident;
    let ctx = &cx.ctx;
    let (impl_generics, ty_generics, _) = input.generics.split_for_impl();
    let type_params: Vec<String> = input
        .generics
        .type_params()
        .map(|p| p.ident.to_string())
        .collect();
    // Only generic field types go in the where clause. A concrete type is
    // checked by the spanned `field` call, so an unsupported type fails there
    // with the error on the field.
    let bounds: Vec<TokenStream2> = cx
        .types
        .iter()
        .filter(|(ty, _)| mentions_type_param(ty, &type_params))
        .map(|(ty, bytes)| {
            if *bytes {
                quote_spanned!(ty.span() => #ty: ::rulekit::ByteStr)
            } else {
                quote_spanned!(ty.span() => #ty: ::rulekit::InputValue<#ctx>)
            }
        })
        .collect();
    let orig_where = input.generics.where_clause.as_ref().map(|clause| {
        let preds = &clause.predicates;
        quote!(#preds,)
    });

    Ok(quote! {
        impl #impl_generics ::rulekit::Input<#ctx> for #ident #ty_generics
        where
            #orig_where
            #(#bounds,)*
        {
            fn get<'__rulekit>(
                &'__rulekit self,
                ctx: &'__rulekit #ctx,
                path: &[::rulekit::ast::Segment],
            ) -> ::core::result::Result<
                ::core::option::Option<::rulekit::value::Val<'__rulekit>>,
                ::rulekit::BoxError,
            > {
                #body
            }
        }

        impl #impl_generics ::rulekit::InputValue<#ctx> for #ident #ty_generics
        where
            #orig_where
            #(#bounds,)*
        {
            fn get<'__rulekit>(
                &'__rulekit self,
                ctx: &'__rulekit #ctx,
                path: &[::rulekit::ast::Segment],
            ) -> ::core::result::Result<
                ::core::option::Option<::rulekit::value::Val<'__rulekit>>,
                ::rulekit::BoxError,
            > {
                ::rulekit::Input::get(self, ctx, path)
            }
        }
    })
}

/// How an enum writes its variants (serde "enum representations").
enum Repr<'a> {
    External,
    Internal(&'a str),
    Adjacent(&'a str, &'a str),
    Untagged,
}

fn expand_enum<'v>(
    cx: &mut Cx,
    attrs: &ContainerAttrs,
    span: Span,
    variants: impl Iterator<Item = &'v Variant>,
) -> Result<TokenStream2, Error> {
    if attrs.transparent {
        return Err(Error::new(
            span,
            "`#[serde(transparent)]` applies to structs",
        ));
    }
    let repr = match (&attrs.tag, &attrs.content, attrs.untagged) {
        (None, None, false) => Repr::External,
        (Some(tag), None, false) => Repr::Internal(tag),
        (Some(tag), Some(content), false) => Repr::Adjacent(tag, content),
        (None, None, true) => Repr::Untagged,
        _ => {
            return Err(Error::new(
                span,
                "conflicting serde enum representation: use `tag`, `tag` + `content`, or `untagged`",
            ));
        }
    };

    let mut arms = Vec::new();
    for variant in variants {
        let vattrs = VariantAttrs::parse(&variant.attrs)?;
        let ident = &variant.ident;
        if vattrs.skip {
            // serde fails to serialize a skipped variant: nothing is on the wire.
            let pat = match &variant.fields {
                Fields::Named(_) => quote!(Self::#ident { .. }),
                Fields::Unnamed(_) => quote!(Self::#ident(..)),
                Fields::Unit => quote!(Self::#ident),
            };
            arms.push(quote!(#pat => ::core::result::Result::Ok(::core::option::Option::None),));
            continue;
        }
        let name = vattrs.rename.unwrap_or_else(|| {
            attrs
                .rename_all
                .apply_to_variant(&ident.unraw().to_string())
        });
        let rule = vattrs
            .rename_all
            .or(attrs.rename_all_fields)
            .unwrap_or_default();
        let shape = Shape::parse(&variant.fields, rule, false)?;

        let fields = shape.fields();
        let bind = |f: &FieldDef| format_ident!("__rulekit_{}", f.index);
        let pat = match &variant.fields {
            Fields::Named(_) => {
                let pairs = fields.iter().map(|f| {
                    let (member, binding) = (&f.member, bind(f));
                    quote!(#member: #binding)
                });
                quote!(Self::#ident { #(#pairs,)* .. })
            }
            Fields::Unnamed(unnamed) => {
                let slots = (0..unnamed.unnamed.len()).map(|i| {
                    if fields.iter().any(|f| f.index == i) {
                        let binding = format_ident!("__rulekit_{i}");
                        quote!(#binding)
                    } else {
                        quote!(_)
                    }
                });
                quote!(Self::#ident(#(#slots),*))
            }
            Fields::Unit => quote!(Self::#ident),
        };
        let access = |f: &FieldDef| {
            let binding = bind(f);
            quote!(#binding)
        };

        let repr = if vattrs.untagged {
            &Repr::Untagged
        } else {
            &repr
        };
        let body = match repr {
            Repr::Untagged => cx.lookup(&shape, &quote!(path), None, &access)?,
            Repr::External => match shape {
                Shape::Unit => quote!(::rulekit::__private::name(#name, path)),
                _ => {
                    let content = cx.lookup(&shape, &quote!(rest), None, &access)?;
                    quote! {
                        match path.split_first() {
                            ::core::option::Option::None => ::rulekit::__private::object(),
                            ::core::option::Option::Some((
                                ::rulekit::ast::Segment::Key { key, .. },
                                rest,
                            )) if key == #name => #content,
                            ::core::option::Option::Some(_) => {
                                ::core::result::Result::Ok(::core::option::Option::None)
                            }
                        }
                    }
                }
            },
            Repr::Internal(tag) => {
                let tag = Tag {
                    key: (*tag).to_owned(),
                    name: name.clone(),
                };
                match &shape {
                    Shape::Unit | Shape::Struct(_) => {
                        cx.lookup(&shape, &quote!(path), Some(&tag), &access)?
                    }
                    Shape::Newtype(_) => {
                        let (key, name) = (&tag.key, &tag.name);
                        let inner = cx.lookup(&shape, &quote!(path), None, &access)?;
                        quote! {
                            match path.split_first() {
                                ::core::option::Option::Some((
                                    ::rulekit::ast::Segment::Key { key, .. },
                                    rest,
                                )) if key == #key => ::rulekit::__private::name(#name, rest),
                                _ => #inner,
                            }
                        }
                    }
                    Shape::Tuple(_) => {
                        return Err(Error::new(
                            ident.span(),
                            "an internally tagged enum (`#[serde(tag)]`) cannot have tuple variants",
                        ));
                    }
                }
            }
            Repr::Adjacent(tag, content) => {
                let content_arm = match shape {
                    Shape::Unit => None,
                    _ => {
                        let inner = cx.lookup(&shape, &quote!(rest), None, &access)?;
                        Some(quote!(#content => #inner,))
                    }
                };
                quote! {
                    match path.split_first() {
                        ::core::option::Option::None => ::rulekit::__private::object(),
                        ::core::option::Option::Some((
                            ::rulekit::ast::Segment::Key { key, .. },
                            rest,
                        )) => match key.as_str() {
                            #tag => ::rulekit::__private::name(#name, rest),
                            #content_arm
                            _ => ::core::result::Result::Ok(::core::option::Option::None),
                        },
                        ::core::option::Option::Some(_) => {
                            ::core::result::Result::Ok(::core::option::Option::None)
                        }
                    }
                }
            }
        };
        arms.push(quote!(#pat => #body,));
    }
    if arms.is_empty() {
        return Ok(quote!(match *self {}));
    }
    Ok(quote! {
        match self {
            #(#arms)*
        }
    })
}

/// The tag field an internally tagged struct or variant writes first.
struct Tag {
    key: String,
    name: String,
}

/// Codegen state: the context type and every field type read, for bounds.
struct Cx {
    ctx: Type,
    types: Vec<(Type, bool)>,
}

impl Cx {
    /// An expression resolving the path in `path` against `shape`, as serde
    /// writes it. `access` yields a `&T` expression for a field.
    fn lookup(
        &mut self,
        shape: &Shape,
        path: &TokenStream2,
        tag: Option<&Tag>,
        access: &dyn Fn(&FieldDef) -> TokenStream2,
    ) -> Result<TokenStream2, Error> {
        Ok(match shape {
            Shape::Unit if tag.is_none() => quote!(::rulekit::__private::null(#path)),
            Shape::Unit => self.keyed(&[], path, tag, access)?,
            Shape::Newtype(f) => self.get(f, access, path),
            Shape::Tuple(fields) => {
                let arms: Vec<_> = fields
                    .iter()
                    .enumerate()
                    .map(|(i, f)| {
                        let get = self.get(f, access, &quote!(tail));
                        quote!(#i => #get,)
                    })
                    .collect();
                if arms.is_empty() {
                    quote! {
                        if #path.is_empty() {
                            ::rulekit::__private::object()
                        } else {
                            ::core::result::Result::Ok(::core::option::Option::None)
                        }
                    }
                } else {
                    quote! {{
                        let ::core::option::Option::Some((head, tail)) = #path.split_first() else {
                            return ::rulekit::__private::object();
                        };
                        match head {
                            ::rulekit::ast::Segment::Index(index) => match *index {
                                #(#arms)*
                                _ => ::core::result::Result::Ok(::core::option::Option::None),
                            },
                            ::rulekit::ast::Segment::Key { .. } => {
                                ::core::result::Result::Ok(::core::option::Option::None)
                            }
                        }
                    }}
                }
            }
            Shape::Struct(fields) => self.keyed(fields, path, tag, access)?,
        })
    }

    /// A struct: the tag, then named fields, then flattened fields in
    /// declaration order for any other key. The rest of the path below a key
    /// is `tail`, so `path` may name an outer `rest`.
    fn keyed(
        &mut self,
        fields: &[FieldDef],
        path: &TokenStream2,
        tag: Option<&Tag>,
        access: &dyn Fn(&FieldDef) -> TokenStream2,
    ) -> Result<TokenStream2, Error> {
        let mut taken = HashSet::new();
        let mut arms = Vec::new();
        if let Some(Tag { key, name }) = tag {
            taken.insert(key.clone());
            arms.push(quote!(#key => ::rulekit::__private::name(#name, tail),));
        }
        let mut flattened = Vec::new();
        for f in fields {
            if f.flatten {
                flattened.push(f);
                continue;
            }
            if !taken.insert(f.name.clone()) {
                return Err(Error::new(
                    f.span,
                    format!("duplicate input field `{}`", f.name),
                ));
            }
            let name = &f.name;
            let get = self.get(f, access, &quote!(tail));
            let guard = f.skip_if.as_ref().map(|pred| {
                let field = access(f);
                quote!(if !#pred(#field))
            });
            arms.push(quote!(#name #guard => #get,));
        }

        // Any other key: each flattened field in turn, as serde writes their
        // entries after the named fields.
        let mut fallback: Option<TokenStream2> = None;
        for f in flattened.iter().rev() {
            let mut get = self.get(f, access, path);
            if let Some(pred) = &f.skip_if {
                let field = access(f);
                get = quote! {
                    if #pred(#field) {
                        ::core::result::Result::Ok(::core::option::Option::None)
                    } else {
                        #get
                    }
                };
            }
            fallback = Some(match fallback {
                None => get,
                Some(next) => quote! {
                    match #get {
                        ::core::result::Result::Ok(::core::option::Option::None) => #next,
                        found => found,
                    }
                },
            });
        }
        let fallback = fallback
            .unwrap_or_else(|| quote!(::core::result::Result::Ok(::core::option::Option::None)));

        let (tail, key_arm) = if arms.is_empty() {
            (
                quote!(_),
                quote!(::rulekit::ast::Segment::Key { .. } => #fallback,),
            )
        } else {
            (
                quote!(tail),
                quote! {
                    ::rulekit::ast::Segment::Key { key, .. } => match key.as_str() {
                        #(#arms)*
                        _ => #fallback,
                    },
                },
            )
        };
        // An early return for the whole value, as the derive always emitted:
        // the arms then write their result straight into the return slot.
        Ok(quote! {{
            let ::core::option::Option::Some((head, #tail)) = #path.split_first() else {
                return ::rulekit::__private::object();
            };
            match head {
                #key_arm
                ::rulekit::ast::Segment::Index(_) => {
                    ::core::result::Result::Ok(::core::option::Option::None)
                }
            }
        }})
    }

    /// Resolve `path` on one field. The turbofish is spanned at the field
    /// type so an unsupported type fails there.
    fn get(
        &mut self,
        f: &FieldDef,
        access: &dyn Fn(&FieldDef) -> TokenStream2,
        path: &TokenStream2,
    ) -> TokenStream2 {
        let (ctx, ty, value) = (&self.ctx, &f.ty, access(f));
        self.types.push((ty.clone(), f.bytes));
        if f.bytes {
            quote_spanned! {ty.span()=>
                ::rulekit::__private::bytes_field::<#ctx, #ty>(#value, ctx, #path)
            }
        } else {
            quote_spanned! {ty.span()=>
                ::rulekit::__private::field::<#ctx, #ty>(#value, ctx, #path)
            }
        }
    }
}

/// A struct or variant body as serde writes it.
enum Shape {
    /// `null`, or only the tag when internally tagged.
    Unit,
    /// The inner value itself: a one-field tuple, or `transparent`.
    Newtype(Box<FieldDef>),
    /// An array of the fields serde writes.
    Tuple(Vec<FieldDef>),
    /// An object of the fields serde writes.
    Struct(Vec<FieldDef>),
}

impl Shape {
    fn parse(fields: &Fields, rule: RenameRule, transparent: bool) -> Result<Self, Error> {
        let mut defs = Vec::new();
        for (index, field) in fields.iter().enumerate() {
            if let Some(def) = FieldDef::parse(field, index, rule)? {
                defs.push(def);
            }
        }
        if transparent {
            if defs.len() != 1 || defs[0].flatten {
                return Err(Error::new(
                    fields.span(),
                    "`#[serde(transparent)]` needs exactly one field that is not skipped",
                ));
            }
            return Ok(Shape::Newtype(Box::new(defs.remove(0))));
        }
        for def in &defs {
            if !matches!(fields, Fields::Named(_)) && (def.flatten || def.skip_if.is_some()) {
                return Err(Error::new(
                    def.span,
                    "`flatten` and `skip_serializing_if` need a named field",
                ));
            }
        }
        Ok(match fields {
            Fields::Unit => Shape::Unit,
            Fields::Unnamed(unnamed) if unnamed.unnamed.len() == 1 => match defs.pop() {
                Some(def) => Shape::Newtype(Box::new(def)),
                None => {
                    return Err(Error::new(
                        unnamed.span(),
                        "the only field of a newtype cannot be skipped",
                    ));
                }
            },
            Fields::Unnamed(_) => Shape::Tuple(defs),
            Fields::Named(_) => Shape::Struct(defs),
        })
    }

    fn fields(&self) -> &[FieldDef] {
        match self {
            Shape::Unit => &[],
            Shape::Newtype(f) => std::slice::from_ref(f),
            Shape::Tuple(fields) | Shape::Struct(fields) => fields,
        }
    }
}

/// One field serde writes.
struct FieldDef {
    member: Member,
    /// Declaration position, for binding names.
    index: usize,
    ty: Type,
    /// The wire name of a named field.
    name: String,
    bytes: bool,
    skip_if: Option<ExprPath>,
    flatten: bool,
    span: Span,
}

impl FieldDef {
    /// `None` for a field serde does not write.
    fn parse(field: &syn::Field, index: usize, rule: RenameRule) -> Result<Option<Self>, Error> {
        let mut skip = false;
        let mut serde_name = None;
        let mut skip_if = None;
        let mut flatten = false;
        for attr in serde_attrs(&field.attrs) {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("rename") {
                    if let Some(lit) = serialize_name(&meta)? {
                        serde_name = Some(lit.value());
                    }
                } else if meta.path.is_ident("skip") || meta.path.is_ident("skip_serializing") {
                    skip = true;
                } else if meta.path.is_ident("skip_serializing_if") {
                    let lit: LitStr = meta.value()?.parse()?;
                    skip_if = Some(lit.parse::<ExprPath>()?);
                } else if meta.path.is_ident("flatten") {
                    flatten = true;
                } else if meta.path.is_ident("serialize_with") || meta.path.is_ident("with") {
                    return Err(meta.error(
                        "`#[derive(rulekit::Input)]` cannot follow a custom serializer: rules would \
                         not see the serialized form. Give the field a newtype that implements \
                         `Serialize` and `rulekit::InputValue` to match, and drop this attribute",
                    ));
                } else if meta.path.is_ident("getter") {
                    return Err(meta.error(
                        "`#[derive(rulekit::Input)]` cannot follow `getter`: derive it on a local type",
                    ));
                } else {
                    skip_value(&meta)?;
                }
                Ok(())
            })?;
        }

        let mut rk_name = None;
        let mut bytes = false;
        for attr in &field.attrs {
            if !attr.path().is_ident("rulekit") {
                continue;
            }
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("rename") {
                    let lit: LitStr = meta.value()?.parse()?;
                    if lit.value().is_empty() {
                        return Err(Error::new(lit.span(), "field name must not be empty"));
                    }
                    rk_name = Some(lit.value());
                    Ok(())
                } else if meta.path.is_ident("bytes") {
                    bytes = true;
                    Ok(())
                } else {
                    Err(meta.error(
                        "unknown `rulekit` attribute; expected `rename = \"...\"` or `bytes`",
                    ))
                }
            })?;
        }
        if skip {
            return Ok(None);
        }
        if flatten && (rk_name.is_some() || bytes) {
            return Err(Error::new(
                field.span(),
                "a flattened field has no name and is not a byte string",
            ));
        }

        let (member, name) = match &field.ident {
            Some(ident) => (
                Member::Named(ident.clone()),
                rule.apply_to_field(&ident.unraw().to_string()),
            ),
            None => (Member::Unnamed(index.into()), String::new()),
        };
        Ok(Some(FieldDef {
            member,
            index,
            ty: field.ty.clone(),
            // rulekit's own rename wins over serde's.
            name: rk_name.or(serde_name).unwrap_or(name),
            bytes,
            skip_if,
            flatten,
            span: field.span(),
        }))
    }
}

#[derive(Default)]
struct VariantAttrs {
    rename: Option<String>,
    rename_all: Option<RenameRule>,
    skip: bool,
    untagged: bool,
}

impl VariantAttrs {
    fn parse(attrs: &[Attribute]) -> Result<Self, Error> {
        let mut out = Self::default();
        for attr in attrs {
            if attr.path().is_ident("rulekit") {
                return Err(Error::new(
                    attr.span(),
                    "`rulekit` attributes go on fields; rename a variant with `#[serde(rename)]`",
                ));
            }
        }
        for attr in serde_attrs(attrs) {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("rename") {
                    if let Some(lit) = serialize_name(&meta)? {
                        out.rename = Some(lit.value());
                    }
                } else if meta.path.is_ident("rename_all") {
                    if let Some(lit) = serialize_name(&meta)? {
                        out.rename_all = Some(RenameRule::parse(&lit)?);
                    }
                } else if meta.path.is_ident("skip") || meta.path.is_ident("skip_serializing") {
                    out.skip = true;
                } else if meta.path.is_ident("untagged") {
                    out.untagged = true;
                } else if meta.path.is_ident("serialize_with") || meta.path.is_ident("with") {
                    return Err(meta.error(
                        "`#[derive(rulekit::Input)]` cannot follow a custom serializer: rules would \
                         not see the serialized form. Give the variant a newtype that implements \
                         `Serialize` and `rulekit::InputValue` to match, and drop this attribute",
                    ));
                } else {
                    skip_value(&meta)?;
                }
                Ok(())
            })?;
        }
        Ok(out)
    }
}

#[derive(Default)]
struct ContainerAttrs {
    rename: Option<String>,
    rename_all: RenameRule,
    rename_all_fields: Option<RenameRule>,
    tag: Option<String>,
    content: Option<String>,
    untagged: bool,
    transparent: bool,
    context: Option<Type>,
}

impl ContainerAttrs {
    fn parse(attrs: &[Attribute]) -> Result<Self, Error> {
        let mut out = Self::default();
        for attr in serde_attrs(attrs) {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("rename") {
                    if let Some(lit) = serialize_name(&meta)? {
                        out.rename = Some(lit.value());
                    }
                } else if meta.path.is_ident("rename_all") {
                    if let Some(lit) = serialize_name(&meta)? {
                        out.rename_all = RenameRule::parse(&lit)?;
                    }
                } else if meta.path.is_ident("rename_all_fields") {
                    if let Some(lit) = serialize_name(&meta)? {
                        out.rename_all_fields = Some(RenameRule::parse(&lit)?);
                    }
                } else if meta.path.is_ident("tag") {
                    out.tag = Some(meta.value()?.parse::<LitStr>()?.value());
                } else if meta.path.is_ident("content") {
                    out.content = Some(meta.value()?.parse::<LitStr>()?.value());
                } else if meta.path.is_ident("untagged") {
                    out.untagged = true;
                } else if meta.path.is_ident("transparent") {
                    out.transparent = true;
                } else if meta.path.is_ident("into") {
                    return Err(meta.error(
                        "`#[derive(rulekit::Input)]` cannot follow `into`: the wire form is the \
                         target type. Derive `rulekit::Input` on that type and evaluate rules on it",
                    ));
                } else if meta.path.is_ident("remote") {
                    return Err(meta.error(
                        "`#[derive(rulekit::Input)]` cannot follow `remote`: derive it on a local type",
                    ));
                } else {
                    skip_value(&meta)?;
                }
                Ok(())
            })?;
        }
        for attr in attrs {
            if !attr.path().is_ident("rulekit") {
                continue;
            }
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("context") {
                    out.context = Some(meta.value()?.parse()?);
                    Ok(())
                } else {
                    Err(meta.error("unknown `rulekit` attribute; expected `context = ...`"))
                }
            })?;
        }
        Ok(out)
    }
}

fn serde_attrs(attrs: &[Attribute]) -> impl Iterator<Item = &Attribute> {
    attrs.iter().filter(|attr| attr.path().is_ident("serde"))
}

/// The serialize name of `rename = "..."` or
/// `rename(serialize = "...", deserialize = "...")`.
fn serialize_name(meta: &ParseNestedMeta) -> Result<Option<LitStr>, Error> {
    if meta.input.peek(Token![=]) {
        return Ok(Some(meta.value()?.parse()?));
    }
    let mut out = None;
    meta.parse_nested_meta(|inner| {
        if inner.path.is_ident("serialize") {
            out = Some(inner.value()?.parse()?);
            Ok(())
        } else {
            skip_value(&inner)
        }
    })?;
    Ok(out)
}

/// Consume a serde attribute that does not change serialization:
/// `key`, `key = ...`, or `key(...)`.
fn skip_value(meta: &ParseNestedMeta) -> Result<(), Error> {
    if meta.input.peek(syn::token::Paren) {
        let content;
        syn::parenthesized!(content in meta.input);
        content.parse::<TokenStream2>()?;
    } else if meta.input.peek(Token![=]) {
        meta.input.parse::<Token![=]>()?;
        while !meta.input.is_empty() && !meta.input.peek(Token![,]) {
            meta.input.parse::<TokenTree>()?;
        }
    }
    Ok(())
}

fn mentions_type_param(ty: &Type, params: &[String]) -> bool {
    match ty {
        Type::Path(path) => path.path.segments.iter().any(|seg| {
            params.iter().any(|p| seg.ident == p)
                || match &seg.arguments {
                    syn::PathArguments::AngleBracketed(args) => {
                        args.args.iter().any(|arg| match arg {
                            syn::GenericArgument::Type(ty) => mentions_type_param(ty, params),
                            _ => false,
                        })
                    }
                    _ => false,
                }
        }),
        Type::Reference(ty) => mentions_type_param(&ty.elem, params),
        Type::Slice(ty) => mentions_type_param(&ty.elem, params),
        Type::Array(ty) => mentions_type_param(&ty.elem, params),
        Type::Tuple(ty) => ty.elems.iter().any(|ty| mentions_type_param(ty, params)),
        Type::Paren(ty) => mentions_type_param(&ty.elem, params),
        Type::Group(ty) => mentions_type_param(&ty.elem, params),
        Type::Ptr(ty) => mentions_type_param(&ty.elem, params),
        _ => false,
    }
}
