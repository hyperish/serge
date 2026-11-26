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

fn widget(
    platform: serge_widget_codegen::Platform,
    attrs: TokenStream,
    item: TokenStream,
) -> TokenStream {
    let generate = || -> syn::Result<proc_macro2::TokenStream> {
        if !attrs.is_empty() {
            return Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                "platform widget attributes take no options",
            ));
        }
        let func = syn::parse::<syn::ItemFn>(item)?;
        let widget = serge_widget_codegen::Widget::parse(platform, &func)?;
        Ok(widget.rust_function(func))
    };
    generate()
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
/// Compiles a static WidgetKit live activity and a public state projection.
#[proc_macro_attribute]
pub fn live_activity(attrs: TokenStream, item: TokenStream) -> TokenStream {
    widget(serge_widget_codegen::Platform::LiveActivity, attrs, item)
}
/// Compiles a static Jetpack Glance widget and a public state projection.
#[proc_macro_attribute]
pub fn glance_widget(attrs: TokenStream, item: TokenStream) -> TokenStream {
    widget(serge_widget_codegen::Platform::Glance, attrs, item)
}

/// Generates a WidgetKit timeline widget and its App Intent.
#[proc_macro_attribute]
pub fn home_widget(attrs: TokenStream, item: TokenStream) -> TokenStream {
    widget(serge_widget_codegen::Platform::WidgetKit, attrs, item)
}
/// Generates an Android system notification from a static presentation.
#[proc_macro_attribute]
pub fn ongoing_notification(attrs: TokenStream, item: TokenStream) -> TokenStream {
    widget(serge_widget_codegen::Platform::Ongoing, attrs, item)
}

/// Marks a capture-free `fn() -> u32` render calculation. The body may only
/// contain unsigned literals and arithmetic or bitwise operators. In a normal
/// build this is an ordinary function.
#[proc_macro_attribute]
pub fn render_value(
    args: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let mut f = syn::parse_macro_input!(item as syn::ItemFn);
    if !args.is_empty() {
        return syn::Error::new(f.sig.ident.span(), "render_value takes no arguments")
            .into_compile_error()
            .into();
    }
    if let Err(e) = serge_widget_codegen::render_value::check(&f) {
        return e.into_compile_error().into();
    }
    let body = f.block;
    f.block = Box::new(syn::parse_quote!({ ::serge::__render_value(|| #body) }));
    quote::quote!(#f).into()
}
