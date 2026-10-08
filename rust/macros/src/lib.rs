//! Derive macros for [rulekit](https://docs.rs/rulekit). Use them through
//! the `rulekit` crate (`#[derive(rulekit::Args)]`, `#[derive(rulekit::Input)]`), not directly.

mod case;
mod input;

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{quote, quote_spanned};
use syn::spanned::Spanned;
use syn::visit_mut::VisitMut;
use syn::{
    Data, DeriveInput, Error, Fields, GenericParam, Lifetime, LitStr, Type, parse_macro_input,
};

/// Derive `rulekit::Args` for a struct of function arguments.
///
/// Each named field is one positional argument, in declaration order; the
/// argument name is the field name unless `#[rulekit(rename = "...")]` says
/// otherwise. Field types must implement `rulekit::FromArg`. An optional last
/// field of type `rulekit::Rest<'a>` takes the remaining arguments. The
/// struct may have one lifetime parameter for borrowed arguments.
#[proc_macro_derive(Args, attributes(rulekit))]
pub fn derive_args(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand(&input)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

struct Field {
    ident: syn::Ident,
    name: String,
    ty: Type,
    rest: bool,
}

fn expand(input: &DeriveInput) -> Result<TokenStream2, Error> {
    let ident = &input.ident;
    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(named) => &named.named,
            _ => {
                return Err(Error::new(
                    ident.span(),
                    "`Args` needs a struct with named fields: each field is one argument",
                ));
            }
        },
        _ => {
            return Err(Error::new(
                ident.span(),
                "`Args` can only be derived for a struct with named fields",
            ));
        }
    };

    let mut lifetime: Option<&Lifetime> = None;
    for param in &input.generics.params {
        match param {
            GenericParam::Lifetime(def) if lifetime.is_none() => lifetime = Some(&def.lifetime),
            GenericParam::Lifetime(def) => {
                return Err(Error::new(
                    def.span(),
                    "`Args` structs may have at most one lifetime parameter",
                ));
            }
            other => {
                return Err(Error::new(
                    other.span(),
                    "`Args` structs may not have type or const parameters",
                ));
            }
        }
    }
    if let Some(clause) = &input.generics.where_clause {
        return Err(Error::new(
            clause.span(),
            "`Args` structs may not have a where clause",
        ));
    }

    let mut parsed = Vec::with_capacity(fields.len());
    for (i, field) in fields.iter().enumerate() {
        let ident = field.ident.clone().expect("named field");
        let mut name = ident.to_string();
        for attr in &field.attrs {
            if !attr.path().is_ident("rulekit") {
                continue;
            }
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("rename") {
                    let lit: LitStr = meta.value()?.parse()?;
                    if lit.value().is_empty() {
                        return Err(Error::new(lit.span(), "argument name must not be empty"));
                    }
                    name = lit.value();
                    Ok(())
                } else {
                    Err(meta.error("unknown `rulekit` attribute; expected `rename = \"...\"`"))
                }
            })?;
        }
        let rest = is_rest(&field.ty);
        if rest && i + 1 != fields.len() {
            return Err(Error::new(
                field.ty.span(),
                "a `Rest` field must be the last field",
            ));
        }
        parsed.push(Field {
            ident,
            name,
            ty: field.ty.clone(),
            rest,
        });
    }

    // The lifetime the generated code uses for borrowed arguments: the
    // struct's own lifetime, or a fresh one.
    let lt = lifetime
        .cloned()
        .unwrap_or_else(|| Lifetime::new("'__rulekit", Span::call_site()));
    let of_ty = if lifetime.is_some() {
        quote!(#ident<#lt>)
    } else {
        quote!(#ident)
    };
    let self_ty = if lifetime.is_some() {
        quote!(#ident<'static>)
    } else {
        quote!(#ident)
    };

    let params = parsed.iter().map(|f| {
        let name = &f.name;
        if f.rest {
            return quote!(::rulekit::Param::rest(#name));
        }
        let mut static_ty = f.ty.clone();
        if let Some(lifetime) = lifetime {
            StaticLifetime(lifetime).visit_type_mut(&mut static_ty);
        }
        quote_spanned!(f.ty.span()=> ::rulekit::Param::new(#name, <#static_ty as ::rulekit::FromArg<'static>>::TYPE))
    });
    let inits = parsed.iter().enumerate().map(|(i, f)| {
        let (ident, name, ty) = (&f.ident, &f.name, &f.ty);
        if f.rest {
            quote!(#ident: ::rulekit::Rest::__new(&vals[#i..]))
        } else {
            quote_spanned!(ty.span()=> #ident: ::rulekit::__private::arg::<#lt, #ty>(vals, #i, #name)?)
        }
    });

    Ok(quote! {
        impl ::rulekit::Args for #self_ty {
            type Of<#lt> = #of_ty;
            const PARAMS: &'static [::rulekit::Param] = &[#(#params),*];
            fn parse<#lt>(
                vals: &#lt [::rulekit::value::Val<#lt>],
            ) -> ::core::result::Result<#of_ty, ::rulekit::Error> {
                ::core::result::Result::Ok(#ident { #(#inits),* })
            }
        }
    })
}

/// Whether a field type is `Rest<...>` (by its last path segment).
fn is_rest(ty: &Type) -> bool {
    match ty {
        Type::Path(path) => {
            path.qself.is_none() && path.path.segments.last().is_some_and(|s| s.ident == "Rest")
        }
        _ => false,
    }
}

/// Replaces the struct's lifetime with `'static`, to name field types in
/// constants.
struct StaticLifetime<'l>(&'l Lifetime);

impl VisitMut for StaticLifetime<'_> {
    fn visit_lifetime_mut(&mut self, lifetime: &mut Lifetime) {
        if lifetime.ident == self.0.ident {
            *lifetime = Lifetime::new("'static", lifetime.span());
        }
    }
}

/// Derive `rulekit::Input` and `rulekit::InputValue` for a struct or enum.
///
/// A rule path is the path serde_json would write the value at: the derive
/// follows `#[serde(...)]` attributes the way schemars' `JsonSchema` derive
/// does. Names follow `rename`, `rename_all`, and `rename_all_fields`; enums
/// follow their representation (externally tagged, `tag`, `tag` + `content`,
/// `untagged`, per-variant `untagged`); `flatten` fields answer keys the
/// container's own fields do not; `transparent` and newtypes are their inner
/// value. `skip` and `skip_serializing` fields are missing, and a field whose
/// `skip_serializing_if` predicate holds is missing (the predicate runs when
/// the field is read). A field serde writes whose value is absent (`None`)
/// reads as `null`. A unit variant written as a string reads as that string.
/// A struct, map-like variant, or tuple read whole is an opaque object.
/// Attributes that only affect deserialization are ignored. `serialize_with`,
/// `with`, `getter`, `into`, and `remote` are rejected: the rule could not
/// see the serialized form.
///
/// Only the path segments a rule reads are resolved. On fields,
/// `#[rulekit(rename = "...")]` sets the name (over any serde rename), and
/// `#[rulekit(bytes)]` reads a `Vec<u8>`, `&[u8]`, `[u8; N]`, `Box<[u8]>`, or
/// `Cow<[u8]>` as a byte string. `#[rulekit(context = Ctx)]` on the type sets
/// the context type. Lifetimes and type parameters are kept; each field type
/// must implement `rulekit::InputValue`, or `rulekit::ByteStr` when marked
/// `bytes`.
#[proc_macro_derive(Input, attributes(rulekit, serde))]
pub fn derive_input(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    input::expand(&input)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}
