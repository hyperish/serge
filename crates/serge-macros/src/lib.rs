//! Procedural macros of serge: [`ui!`] and [`#[component]`](macro@component).
//! Use them through `serge::prelude`.

mod component;
mod ui;

use proc_macro::TokenStream;

/// The view markup (`docs/01-dx-and-dsl.md` §1):
///
/// ```ignore
/// ui! {
///     Card { padding: 16px, gap: 12px,
///         Text { "Count: {count()}" }
///         Button { text: "Increment", clicked => { count += 1; } }
///     }
/// }
/// ```
///
/// Widgets are Rust items in scope (built-ins and `#[component]`s);
/// properties are `name: value`, events `name(args) => { .. }` or
/// `name => async move { .. }`; `if` and `for item in items { key: k, .. }`
/// are reactive regions; `web::*` / `native::*` set target-only attributes;
/// `{expr}` embeds an element.
#[proc_macro]
pub fn ui(input: TokenStream) -> TokenStream {
    ui::expand(input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Makes a function returning `Element` usable as a widget in `ui!`.
#[proc_macro_attribute]
pub fn component(attr: TokenStream, item: TokenStream) -> TokenStream {
    component::expand(attr.into(), item.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
