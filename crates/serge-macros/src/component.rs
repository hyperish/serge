//! `#[component]` (`docs/01-dx-and-dsl.md` §1, 컴포넌트 인자 규칙): keeps the
//! function and adds a builder type of the same name (type namespace), so
//! `Name { prop: value, .. }` in `ui!` works like a built-in widget.
//!
//! Arguments come from three sources: `Ctx<T>` (injected), `Path<T>` /
//! `Query<T>` (route parameters), or properties from the parent. A
//! `children: Element` argument takes the child nodes, a `Callback<A>`
//! argument is an event (`name(a) => { .. }`), an `Option<T>` property may be
//! left out, and a missing required property is a compile error naming it.

use proc_macro2::TokenStream;
use quote::{format_ident, quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{FnArg, GenericArgument, ItemFn, Pat, PathArguments, Type};

enum Arg {
    /// Injected or route parameter: built by `FromView`.
    View,
    /// `children: Element`.
    Children,
    /// A required property.
    Prop,
    /// An `Option<T>` property (inner type).
    OptProp(Type),
    /// A required `Callback<A>` event.
    Event(Option<Type>),
    /// An `Option<Callback<A>>` event.
    OptEvent(Option<Type>),
}

fn last_segment(ty: &Type) -> Option<(&syn::Ident, Option<&Type>)> {
    let Type::Path(p) = ty else { return None };
    let seg = p.path.segments.last()?;
    let inner = match &seg.arguments {
        PathArguments::AngleBracketed(a) => a.args.iter().find_map(|g| match g {
            GenericArgument::Type(t) => Some(t),
            _ => None,
        }),
        _ => None,
    };
    Some((&seg.ident, inner))
}

fn callback_arg(ty: &Type) -> Option<Option<Type>> {
    let (name, inner) = last_segment(ty)?;
    if name != "Callback" {
        return None;
    }
    Some(match inner {
        Some(Type::Tuple(t)) if t.elems.is_empty() => None,
        Some(t) => Some(t.clone()),
        None => None,
    })
}

fn classify(name: &syn::Ident, ty: &Type) -> Arg {
    if let Some((seg, inner)) = last_segment(ty) {
        if seg == "Ctx" || seg == "Path" || seg == "Query" {
            return Arg::View;
        }
        if name == "children" && seg == "Element" {
            return Arg::Children;
        }
        if let Some(a) = callback_arg(ty) {
            return Arg::Event(a);
        }
        if seg == "Option"
            && let Some(inner) = inner
        {
            if let Some(a) = callback_arg(inner) {
                return Arg::OptEvent(a);
            }
            return Arg::OptProp(inner.clone());
        }
    }
    Arg::Prop
}

pub fn expand(attr: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    if !attr.is_empty() {
        return Err(syn::Error::new(
            attr.span(),
            "`#[component]` takes no arguments",
        ));
    }
    let f: ItemFn = syn::parse2(item)?;
    if !f.sig.generics.params.is_empty() {
        return Err(syn::Error::new(
            f.sig.generics.span(),
            "generic components are not supported yet; use concrete types",
        ));
    }
    if f.sig.asyncness.is_some() {
        return Err(syn::Error::new(
            f.sig.asyncness.span(),
            "components are not async; start async work in a handler (`=> async move { .. }`) or with `spawn`",
        ));
    }
    let vis = &f.vis;
    let name = &f.sig.ident;
    let mut args = Vec::new();
    for input in &f.sig.inputs {
        let FnArg::Typed(pt) = input else {
            return Err(syn::Error::new(
                input.span(),
                "components cannot take `self`",
            ));
        };
        let Pat::Ident(pi) = &*pt.pat else {
            return Err(syn::Error::new(
                pt.pat.span(),
                "component arguments must be plain names (`store: Ctx<Store>`)",
            ));
        };
        let kind = classify(&pi.ident, &pt.ty);
        args.push((pi.ident.clone(), (*pt.ty).clone(), kind));
    }

    // Typestate parameters, one per required property or event.
    let required: Vec<usize> = args
        .iter()
        .enumerate()
        .filter(|(_, a)| matches!(a.2, Arg::Prop | Arg::Event(_)))
        .map(|(i, _)| i)
        .collect();
    let params: Vec<syn::Ident> = (0..required.len())
        .map(|i| format_ident!("__P{}", i))
        .collect();
    let unset = quote! { ::serge::__private::Unset };
    let set = quote! { ::serge::__private::Set };

    let mut fields = Vec::new();
    let mut inits = Vec::new();
    let mut moves = Vec::new();
    for (ident, ty, kind) in &args {
        let field = format_ident!("{}", ident);
        let fty = match kind {
            Arg::View => continue,
            Arg::Children => quote! { ::std::vec::Vec<::serge::__private::Element> },
            Arg::Prop | Arg::Event(_) => quote! { ::core::option::Option<#ty> },
            Arg::OptProp(_) | Arg::OptEvent(_) => quote! { #ty },
        };
        fields.push(quote! { #field: #fty });
        inits.push(match kind {
            Arg::Children => quote! { #field: ::std::vec::Vec::new() },
            _ => quote! { #field: ::core::option::Option::None },
        });
        moves.push(field);
    }

    let struct_params = params.iter().map(|p| quote! { #p = #unset });
    let gen_params = quote! { #(#params),* };

    let mut methods = Vec::new();
    for (i, (ident, ty, kind)) in args.iter().enumerate() {
        let field = format_ident!("{}", ident);
        let others: Vec<_> = moves.iter().filter(|m| **m != field).collect();
        let req = required.iter().position(|r| *r == i);
        let out_params = |set_here: bool| {
            let ps = params.iter().enumerate().map(|(j, p)| {
                if set_here && Some(j) == req {
                    set.clone()
                } else {
                    quote! { #p }
                }
            });
            quote! { #name<#(#ps),*> }
        };
        let span = ident.span();
        match kind {
            Arg::View => {}
            Arg::Children => methods.push(quote_spanned! {span=>
                #[doc(hidden)]
                pub fn __child(mut self, child: ::serge::__private::Element) -> Self {
                    self.children.push(child);
                    self
                }
            }),
            Arg::Prop => {
                let out = out_params(true);
                methods.push(quote_spanned! {span=>
                    #[allow(missing_docs)]
                    pub fn #field(self, v: impl ::serge::__private::IntoArg<#ty>) -> #out {
                        #name {
                            #field: ::core::option::Option::Some(v.into_arg()),
                            #(#others: self.#others,)*
                            __s: ::core::marker::PhantomData,
                        }
                    }
                });
            }
            Arg::OptProp(inner) => methods.push(quote_spanned! {span=>
                #[allow(missing_docs)]
                pub fn #field(mut self, v: impl ::serge::__private::IntoArg<#inner>) -> Self {
                    self.#field = ::core::option::Option::Some(v.into_arg());
                    self
                }
            }),
            Arg::Event(a) | Arg::OptEvent(a) => {
                let method = format_ident!("on_{}", ident, span = span);
                let (bound, cb) = match a {
                    None => (
                        quote! { FnOnce() + ::core::clone::Clone + 'static },
                        quote! { ::serge::__private::Callback::new(move |()| (f.clone())()) },
                    ),
                    Some(a) => (
                        quote! { FnOnce(#a) + ::core::clone::Clone + 'static },
                        quote! { ::serge::__private::Callback::new(move |a| (f.clone())(a)) },
                    ),
                };
                let out = if matches!(kind, Arg::Event(_)) {
                    out_params(true)
                } else {
                    quote! { Self }
                };
                methods.push(quote_spanned! {span=>
                    #[allow(missing_docs)]
                    pub fn #method<__F: #bound>(self, f: __F) -> #out {
                        #name {
                            #field: ::core::option::Option::Some(#cb),
                            #(#others: self.#others,)*
                            __s: ::core::marker::PhantomData,
                        }
                    }
                });
            }
        }
    }

    // `Build` only when every required property is set; each missing one
    // reports its own name.
    let mut needs = Vec::new();
    let mut bounds = Vec::new();
    for (j, i) in required.iter().enumerate() {
        let (ident, _, kind) = &args[*i];
        let tr = format_ident!("__{}_needs_{}", name, ident);
        let what = if matches!(kind, Arg::Event(_)) {
            format!("`{name}` needs the event `{ident}` (`{ident} => {{ .. }}`)")
        } else {
            format!("`{name}` needs the property `{ident}`")
        };
        let label = format!("add `{ident}` to this `{name} {{ .. }}`");
        needs.push(quote! {
            #[diagnostic::on_unimplemented(message = #what, label = #label)]
            #[doc(hidden)]
            #[allow(non_camel_case_types)]
            #vis trait #tr {}
            impl #tr for #set {}
        });
        let p = &params[j];
        bounds.push(quote! { #p: #tr });
    }

    let mut lets = Vec::new();
    let mut call_args = Vec::new();
    for (ident, ty, kind) in &args {
        let field = format_ident!("{}", ident);
        lets.push(match kind {
            Arg::View => quote_spanned! {ty.span()=>
                let #field = <#ty as ::serge::__private::FromView>::from_view()
                    .unwrap_or_else(|e| ::core::panic!("serge: `{}`: {}", ::core::stringify!(#name), e));
            },
            Arg::Children => quote! { let #field = ::serge::__private::Element::fragment(self.children); },
            Arg::Prop | Arg::Event(_) => {
                quote! { let #field = self.#field.expect("set (typestate)"); }
            }
            Arg::OptProp(_) | Arg::OptEvent(_) => quote! { let #field = self.#field; },
        });
        call_args.push(field);
    }

    let doc = format!("Builder for the `{name}` component in `ui!`.");
    Ok(quote! {
        #[allow(non_snake_case)]
        #f

        #[doc = #doc]
        #[allow(non_camel_case_types)]
        #vis struct #name<#(#struct_params),*> {
            #(#fields,)*
            __s: ::core::marker::PhantomData<(#(#params,)*)>,
        }

        impl #name {
            #[doc(hidden)]
            pub fn __new() -> Self {
                #name { #(#inits,)* __s: ::core::marker::PhantomData }
            }
        }

        impl<#gen_params> #name<#gen_params> {
            #(#methods)*
        }

        #(#needs)*

        impl<#(#bounds),*> ::serge::__private::Build for #name<#gen_params> {
            const REGION: bool = true;
            fn build(self) -> ::serge::__private::Element {
                #(#lets)*
                ::serge::__private::untrack(move || #name(#(#call_args),*))
            }
        }
    })
}
