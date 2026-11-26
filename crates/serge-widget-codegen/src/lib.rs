//! A static subset of the Serge view language shared by the attribute
//! macros and the build stage. Both use the same parsed bindings and layout.
mod emit;
mod parse;

pub mod render_value;

use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use std::collections::BTreeMap;
use syn::{FnArg, Ident, Item, ItemFn, Pat, Result, Stmt};

/// Native host for the generated source.
#[derive(Clone, Copy, Eq, PartialEq)]
pub enum Platform {
    /// ActivityKit with compact and expanded Dynamic Island presentations.
    LiveActivity,
    /// A Jetpack Glance home-screen widget.
    Glance,
    /// A WidgetKit home or lock-screen widget and its Shortcuts action.
    WidgetKit,
    /// An Android ongoing progress notification.
    Ongoing,
}
impl Platform {
    fn is_apple(self) -> bool {
        matches!(self, Self::LiveActivity | Self::WidgetKit)
    }
}
/// One compiled, typed widget.
pub struct Widget {
    platform: Platform,
    name: Ident,
    argument: Ident,
    fields: BTreeMap<String, Field>,
    sections: BTreeMap<String, Node>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Text,
    Number,
}
enum Part {
    Literal(String),
    Field(String),
    Percent(String),
}
enum Node {
    Text(Vec<Part>),
    Icon(String),
    Progress(String),
    Stack {
        horizontal: bool,
        gap: u32,
        children: Vec<Node>,
    },
}

impl Widget {
    /// Parses a declaration and rejects runtime UI and unsupported field expressions.
    pub fn parse(platform: Platform, f: &ItemFn) -> Result<Self> {
        let arg = match f.sig.inputs.first() {
            Some(FnArg::Typed(a)) if f.sig.inputs.len() == 1 => match &*a.pat {
                Pat::Ident(p) if p.by_ref.is_none() && p.subpat.is_none() => p.ident.clone(),
                _ => return Err(syn::Error::new_spanned(a, "use a named state argument")),
            },
            _ => {
                return Err(syn::Error::new_spanned(
                    &f.sig,
                    "a widget takes one state model",
                ));
            }
        };
        if f.sig.asyncness.is_some()
            || f.sig.unsafety.is_some()
            || !f.sig.generics.params.is_empty()
        {
            return Err(syn::Error::new_spanned(
                &f.sig,
                "widgets must be safe, synchronous, concrete functions",
            ));
        }
        let m = match f.block.stmts.as_slice() {
            [Stmt::Expr(syn::Expr::Macro(m), _)] => &m.mac,
            [Stmt::Macro(m)] => &m.mac,
            _ => {
                return Err(syn::Error::new_spanned(
                    &f.block,
                    "expected one static live_activity! or ui! expression",
                ));
            }
        };
        let mut widget = Self {
            platform,
            name: f.sig.ident.clone(),
            argument: arg,
            fields: BTreeMap::new(),
            sections: BTreeMap::new(),
        };
        widget.parse_sections(m)?;
        Ok(widget)
    }
    /// Produces the Rust function that validates field types and serializes the model for the host.
    pub fn rust_function(&self, mut f: ItemFn) -> TokenStream {
        let var = &self.argument;
        let fields = self.fields.iter().map(|(name, kind)| {
            let ident = Ident::new(name, self.argument.span());
            let value = match kind {
                Field::Text => quote! { ::core::convert::AsRef::<str>::as_ref(&#var.#ident) },
                Field::Number => quote! { ::core::convert::Into::<f64>::into(#var.#ident) },
            };
            quote! { (#name, ::serge::__private::WidgetValue::from(#value)) }
        });
        let name = self.name.to_string();
        f.block = syn::parse_quote!({
            let _ = &#var;
            ::serge::PlatformWidget::from_fields(#name, [#(#fields),*])
        });
        f.into_token_stream()
    }
    /// The Rust declaration name, also used as the platform type name.
    pub fn name(&self) -> String {
        self.name.to_string()
    }
    /// The target of the generated source.
    pub fn platform(&self) -> Platform {
        self.platform
    }
    /// Emits a complete Swift widget or Kotlin widget and receiver.
    pub fn source(&self, android_package: &str) -> Result<String> {
        emit::source(self, android_package)
    }
    /// Android manifest and provider resources for the compiled receiver.
    pub fn android_resources(&self, android_package: &str) -> Result<Vec<(String, String)>> {
        emit::resources(self, android_package)
    }
}

/// Finds widget declarations in a package's configured Rust source file, also
/// visiting inline modules. External modules are listed as their own inputs.
pub fn discover(source: &str) -> Result<Vec<Widget>> {
    fn items(list: Vec<Item>, out: &mut Vec<Widget>) -> Result<()> {
        for item in list {
            match item {
                Item::Fn(f) => {
                    for attr in &f.attrs {
                        let segments: Vec<_> = attr
                            .path()
                            .segments
                            .iter()
                            .map(|s| s.ident.to_string())
                            .collect();
                        let platform = match segments.as_slice() {
                            [.., a, b] if a == "ios" && b == "live_activity" => {
                                Platform::LiveActivity
                            }
                            [.., a, b] if a == "ios" && b == "widget" => Platform::WidgetKit,
                            [.., a, b] if a == "android" && b == "widget" => Platform::Glance,
                            [.., a, b] if a == "android" && b == "ongoing_notification" => {
                                Platform::Ongoing
                            }
                            _ => continue,
                        };
                        out.push(Widget::parse(platform, &f)?);
                    }
                }
                Item::Mod(m) => {
                    if let Some((_, list)) = m.content {
                        items(list, out)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    let mut widgets = Vec::new();
    items(syn::parse_file(source)?.items, &mut widgets)?;
    Ok(widgets)
}
