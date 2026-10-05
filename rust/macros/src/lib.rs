//! Derive macros for [rulekit](https://docs.rs/rulekit). Use them through
//! the `rulekit` crate (`#[derive(rulekit::Args)]`, `#[derive(rulekit::Input)]`), not directly.

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

/// Derive `rulekit::Input` for a struct of named fields.
///
/// The generated [`Input::get`](rulekit::Input::get) matches the first path
/// segment against field names (`#[rulekit(rename = "...")]` to change one,
/// `#[rulekit(skip)]` to omit one) and resolves the rest of the path on that
/// field only. An unknown field is absent. Lifetimes and type parameters are
/// kept; each field type must implement `rulekit::InputValue`.
#[proc_macro_derive(Input, attributes(rulekit))]
pub fn derive_input(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand_input(&input)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

struct InputField {
    ident: syn::Ident,
    name: String,
    ty: Type,
}

fn expand_input(input: &DeriveInput) -> Result<TokenStream2, Error> {
    let ident = &input.ident;
    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(named) => &named.named,
            _ => {
                return Err(Error::new(
                    ident.span(),
                    "`Input` needs a struct with named fields",
                ));
            }
        },
        _ => {
            return Err(Error::new(
                ident.span(),
                "`Input` can only be derived for a struct with named fields",
            ));
        }
    };

    let mut taken = std::collections::HashSet::new();
    let mut parsed = Vec::new();
    for field in fields {
        let ident = field.ident.clone().expect("named field");
        let mut name = ident.to_string();
        let mut skip = false;
        let mut renamed = false;
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
                    name = lit.value();
                    renamed = true;
                    Ok(())
                } else if meta.path.is_ident("skip") {
                    skip = true;
                    Ok(())
                } else {
                    Err(meta.error(
                        "unknown `rulekit` attribute; expected `rename = \"...\"` or `skip`",
                    ))
                }
            })?;
        }
        if skip && renamed {
            return Err(Error::new(
                field.span(),
                "`skip` and `rename` cannot be combined",
            ));
        }
        if skip {
            continue;
        }
        if !taken.insert(name.clone()) {
            return Err(Error::new(
                field.span(),
                format!("duplicate input field `{name}`"),
            ));
        }
        parsed.push(InputField {
            ident,
            name,
            ty: field.ty.clone(),
        });
    }

    let ctx = context_ty(input)?;
    let (impl_generics, ty_generics, _) = input.generics.split_for_impl();

    let arms = parsed.iter().map(|f| {
        let (name, ident, ty) = (&f.name, &f.ident, &f.ty);
        quote_spanned! {ty.span()=>
            #name => ::rulekit::__private::field::<#ctx, #ty>(&self.#ident, ctx, rest),
        }
    });
    let type_params: Vec<String> = input
        .generics
        .type_params()
        .map(|p| p.ident.to_string())
        .collect();
    // Only generic field types go in the where clause. A concrete type is
    // checked by the spanned `field` call, so an unsupported type fails here
    // with the error on the field.
    let bounds: Vec<TokenStream2> = parsed
        .iter()
        .filter(|f| mentions_type_param(&f.ty, &type_params))
        .map(|f| {
            let ty = &f.ty;
            quote_spanned!(ty.span() => #ty: ::rulekit::InputValue<#ctx>)
        })
        .collect();
    let orig_where = input.generics.where_clause.as_ref().map(|clause| {
        let preds = &clause.predicates;
        quote!(#preds,)
    });

    let bounds = &bounds;
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
                let Some((head, rest)) = path.split_first() else {
                    return ::core::result::Result::Ok(::core::option::Option::Some(
                        ::rulekit::value::Val::Ref(::rulekit::value::ValueRef::Object(
                            ::rulekit::value::ObjectRef::Opaque,
                        )),
                    ));
                };
                match head {
                    ::rulekit::ast::Segment::Key { key, .. } => match key.as_str() {
                        #(#arms)*
                        _ => ::core::result::Result::Ok(::core::option::Option::None),
                    },
                    ::rulekit::ast::Segment::Index(_) => {
                        ::core::result::Result::Ok(::core::option::Option::None)
                    }
                }
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

fn context_ty(input: &DeriveInput) -> Result<Type, Error> {
    let mut ctx = None;
    for attr in &input.attrs {
        if !attr.path().is_ident("rulekit") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("context") {
                ctx = Some(meta.value()?.parse()?);
                Ok(())
            } else {
                Err(meta
                    .error("unknown `rulekit` attribute; expected `context = ...` on the struct"))
            }
        })?;
    }
    Ok(ctx.unwrap_or_else(|| syn::parse_quote!(())))
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
