//! Procedural macros for the `serge` cross-platform UI framework.
//!
//! Provides:
//! - `#[component]` — ergonomic component function annotation
//! - `ui! { ... }` — QML/Slint + Dioxus-inspired reactive UI DSL
//! - `live_activity! { ... }` & `#[ios::live_activity]` — static UI snapshot DSL
//!   compiled to SwiftUI Dynamic Island / WidgetKit extensions

use proc_macro::{Delimiter, Group, Ident, Literal, Spacing, Span, TokenStream, TokenTree};
use std::iter::FromIterator;

/// Marks a UI component function, allowing PascalCase component naming without warnings.
#[proc_macro_attribute]
pub fn component(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let allow: TokenStream = "#[allow(non_snake_case, missing_docs, unused_mut)]"
        .parse()
        .expect("valid allow attribute");
    let mut out = TokenStream::new();
    out.extend(allow);
    out.extend(item);
    out
}

/// Marks an iOS Dynamic Island / Live Activity component function (`#[ios::live_activity]`).
///
/// Allows PascalCase naming and automatically binds the function name onto the returned
/// `serge::ios::LiveActivity` descriptor for SwiftUI `ActivityConfiguration` codegen.
#[proc_macro_attribute]
pub fn live_activity_attr(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let tokens: Vec<TokenTree> = item.into_iter().collect();
    let fn_name = find_fn_name(&tokens);
    let last_brace_idx = tokens
        .iter()
        .rposition(|tt| matches!(tt, TokenTree::Group(g) if g.delimiter() == Delimiter::Brace));

    let allow: TokenStream = "#[allow(non_snake_case, missing_docs, unused_mut)]"
        .parse()
        .expect("valid allow attribute");

    let mut out = TokenStream::new();
    out.extend(allow);

    match (fn_name, last_brace_idx) {
        (Some(name), Some(idx)) => {
            for (i, tt) in tokens.into_iter().enumerate() {
                if i == idx {
                    if let TokenTree::Group(g) = tt {
                        let body = g.stream();
                        let wrapped: TokenStream = format!(
                            "{{ ::serge::ios::LiveActivity::with_name({{ {} }}, \"{}\") }}",
                            body, name
                        )
                        .parse()
                        .unwrap_or_else(|_| TokenTree::Group(g).into());
                        out.extend(wrapped);
                    }
                } else {
                    out.extend(std::iter::once(tt));
                }
            }
        }
        _ => out.extend(tokens),
    }
    out
}

/// Declarative QML/Slint + `rsx!`-style cross-platform UI macro.
#[proc_macro]
pub fn ui(input: TokenStream) -> TokenStream {
    let tokens: Vec<TokenTree> = input.into_iter().collect();
    expand_ui_stream(&tokens, true)
}

/// Static SwiftUI Dynamic Island / WidgetKit Live Activity macro.
///
/// Expects named slots such as `compact_leading`, `compact_trailing`, and `expanded`,
/// each containing `ui! { ... }` blocks evaluated as static snapshots (without live signals).
#[proc_macro]
pub fn live_activity(input: TokenStream) -> TokenStream {
    let tokens: Vec<TokenTree> = input.into_iter().collect();
    let mut idx = 0;
    let mut stmts = TokenStream::new();

    while idx < tokens.len() {
        let Some(key_ident) = ident_at(&tokens, idx) else {
            return compile_error(
                tokens[idx].span(),
                "live_activity!: expected slot name like `compact_leading:`",
            );
        };
        if !is_punct(&tokens, idx + 1, ':') {
            return compile_error(
                key_ident.span(),
                "live_activity!: expected `:` after slot name",
            );
        }
        idx += 2;

        let val_tokens = take_until_top_comma(&tokens, &mut idx);
        let slot_expr = if val_tokens.len() >= 3
            && is_ident(&val_tokens, 0, "ui")
            && is_punct(&val_tokens, 1, '!')
        {
            if let TokenTree::Group(g) = &val_tokens[2] {
                let inner: Vec<TokenTree> = g.stream().into_iter().collect();
                expand_ui_stream(&inner, false)
            } else {
                transform_expr(&val_tokens)
            }
        } else {
            transform_expr(&val_tokens)
        };

        let key_str = key_ident.to_string();
        let method = match key_str.as_str() {
            "compact_leading" => "compact_leading",
            "compact_trailing" => "compact_trailing",
            "expanded" => "expanded",
            "minimal" => "minimal",
            _ => "named_slot",
        };

        if method == "named_slot" {
            let mut call_args = TokenStream::new();
            call_args.extend(std::iter::once(TokenTree::Literal(Literal::string(
                &key_str,
            ))));
            call_args.extend(",".parse::<TokenStream>().expect("comma"));
            call_args.extend(slot_expr);
            stmts.extend("__la = __la.slot".parse::<TokenStream>().expect("prefix"));
            stmts.extend(wrap_paren(call_args));
            stmts.extend(";".parse::<TokenStream>().expect("semi"));
        } else {
            stmts.extend(
                format!("__la = __la.{method}")
                    .parse::<TokenStream>()
                    .expect("method"),
            );
            stmts.extend(wrap_paren(slot_expr));
            stmts.extend(";".parse::<TokenStream>().expect("semi"));
        }
    }

    let mut out = TokenStream::new();
    out.extend(
        "let mut __la = ::serge::ios::LiveActivity::new();"
            .parse::<TokenStream>()
            .expect("init"),
    );
    out.extend(stmts);
    out.extend("__la".parse::<TokenStream>().expect("ret"));
    wrap_brace(out)
}

fn find_fn_name(tokens: &[TokenTree]) -> Option<String> {
    for (i, tt) in tokens.iter().enumerate() {
        if let TokenTree::Ident(id) = tt
            && id.to_string() == "fn"
            && let Some(TokenTree::Ident(name)) = tokens.get(i + 1)
        {
            return Some(name.to_string());
        }
    }
    None
}

fn expand_ui_stream(tokens: &[TokenTree], reactive: bool) -> TokenStream {
    if tokens.is_empty() {
        return "::serge::core::Element::empty()"
            .parse()
            .expect("empty element");
    }

    let mut idx = 0;
    let mut nodes: Vec<TokenStream> = Vec::new();
    while idx < tokens.len() {
        if is_punct(tokens, idx, ',') {
            idx += 1;
            continue;
        }
        nodes.push(parse_node_expr(tokens, &mut idx, reactive));
    }

    if nodes.len() == 1 {
        nodes.remove(0)
    } else {
        let mut items = TokenStream::new();
        for (i, n) in nodes.into_iter().enumerate() {
            if i > 0 {
                items.extend(",".parse::<TokenStream>().expect("comma"));
            }
            items.extend(n);
        }
        let mut out = TokenStream::new();
        out.extend(
            "::serge::core::Element::fragment"
                .parse::<TokenStream>()
                .expect("frag path"),
        );
        let mut vec_expr = "vec!".parse::<TokenStream>().expect("vec");
        vec_expr.extend(wrap_bracket(items));
        out.extend(wrap_paren(vec_expr));
        out
    }
}

fn parse_node_expr(tokens: &[TokenTree], idx: &mut usize, reactive: bool) -> TokenStream {
    if let Some(id) = ident_at(tokens, *idx) {
        let s = id.to_string();
        if s == "if" {
            return parse_if_node(tokens, idx, reactive);
        }
        if s == "for" {
            return parse_for_node(tokens, idx, reactive);
        }
        if let Some(TokenTree::Group(g)) = tokens.get(*idx + 1)
            && g.delimiter() == Delimiter::Brace
        {
            *idx += 2;
            let body: Vec<TokenTree> = g.stream().into_iter().collect();
            return build_element(&s, &body, reactive);
        }
    }
    compile_error(
        tokens
            .get(*idx)
            .map_or_else(Span::call_site, TokenTree::span),
        "ui!: expected UI element `Tag { ... }`, `if`, or `for`",
    )
}

fn parse_if_node(tokens: &[TokenTree], idx: &mut usize, reactive: bool) -> TokenStream {
    *idx += 1; // skip `if`
    let mut cond_tokens = Vec::new();
    while *idx < tokens.len() {
        if matches!(&tokens[*idx], TokenTree::Group(g) if g.delimiter() == Delimiter::Brace) {
            break;
        }
        cond_tokens.push(tokens[*idx].clone());
        *idx += 1;
    }
    let then_expr = match tokens.get(*idx) {
        Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Brace => {
            *idx += 1;
            let inner: Vec<TokenTree> = g.stream().into_iter().collect();
            expand_ui_stream(&inner, reactive)
        }
        other => {
            return compile_error(
                other.map_or_else(Span::call_site, TokenTree::span),
                "ui!: expected `{ ... }` after `if` condition",
            );
        }
    };

    let else_expr = if is_ident(tokens, *idx, "else") {
        *idx += 1;
        if is_ident(tokens, *idx, "if") {
            parse_if_node(tokens, idx, reactive)
        } else if let Some(TokenTree::Group(g)) = tokens.get(*idx)
            && g.delimiter() == Delimiter::Brace
        {
            *idx += 1;
            let inner: Vec<TokenTree> = g.stream().into_iter().collect();
            expand_ui_stream(&inner, reactive)
        } else {
            return compile_error(Span::call_site(), "ui!: expected `{ ... }` after `else`");
        }
    } else {
        "::serge::core::Element::empty()"
            .parse()
            .expect("empty element")
    };

    let cond = transform_expr(&cond_tokens);
    let mut out = TokenStream::new();
    if reactive {
        let mut args = "move || ".parse::<TokenStream>().expect("cond closure");
        args.extend(wrap_paren(cond));
        args.extend(", move || ".parse::<TokenStream>().expect("then"));
        args.extend(wrap_brace(then_expr));
        args.extend(", move || ".parse::<TokenStream>().expect("else"));
        args.extend(wrap_brace(else_expr));
        out.extend(
            "::serge::core::Element::conditional"
                .parse::<TokenStream>()
                .expect("cond path"),
        );
        out.extend(wrap_paren(args));
    } else {
        out.extend("if ".parse::<TokenStream>().expect("if"));
        out.extend(cond);
        out.extend(std::iter::once(TokenTree::Group(Group::new(
            Delimiter::Brace,
            then_expr,
        ))));
        out.extend(" else ".parse::<TokenStream>().expect("else"));
        out.extend(std::iter::once(TokenTree::Group(Group::new(
            Delimiter::Brace,
            else_expr,
        ))));
    }
    out
}

fn parse_for_node(tokens: &[TokenTree], idx: &mut usize, reactive: bool) -> TokenStream {
    *idx += 1; // skip `for`
    let mut pat_tokens = Vec::new();
    while *idx < tokens.len() {
        if is_ident(tokens, *idx, "in") {
            *idx += 1;
            break;
        }
        pat_tokens.push(tokens[*idx].clone());
        *idx += 1;
    }

    let mut iter_tokens = Vec::new();
    while *idx < tokens.len() {
        if matches!(&tokens[*idx], TokenTree::Group(g) if g.delimiter() == Delimiter::Brace) {
            break;
        }
        iter_tokens.push(tokens[*idx].clone());
        *idx += 1;
    }

    let body_expr = match tokens.get(*idx) {
        Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Brace => {
            *idx += 1;
            let inner: Vec<TokenTree> = g.stream().into_iter().collect();
            expand_ui_stream(&inner, reactive)
        }
        other => {
            return compile_error(
                other.map_or_else(Span::call_site, TokenTree::span),
                "ui!: expected `{ ... }` for `for` loop body",
            );
        }
    };

    let pat = TokenStream::from_iter(pat_tokens);
    let iter = transform_expr(&iter_tokens);
    let mut loop_body = TokenStream::new();
    loop_body.extend("__items.push".parse::<TokenStream>().expect("push"));
    loop_body.extend(wrap_paren(body_expr));
    loop_body.extend(";".parse::<TokenStream>().expect("semi"));

    let mut out = TokenStream::new();
    out.extend(
        "let mut __items = ::std::vec::Vec::new(); for "
            .parse::<TokenStream>()
            .expect("for init"),
    );
    out.extend(pat);
    out.extend(" in ".parse::<TokenStream>().expect("in"));
    out.extend(iter);
    out.extend(std::iter::once(TokenTree::Group(Group::new(
        Delimiter::Brace,
        loop_body,
    ))));
    out.extend(
        "::serge::core::Element::fragment(__items)"
            .parse::<TokenStream>()
            .expect("frag"),
    );
    wrap_brace(out)
}

fn build_element(tag: &str, body: &[TokenTree], reactive: bool) -> TokenStream {
    let mut stmts = TokenStream::new();
    stmts.extend(
        format!("let mut __el = ::serge::core::Element::new(\"{}\");", tag)
            .parse::<TokenStream>()
            .expect("new element"),
    );

    let mut idx = 0;
    while idx < body.len() {
        if is_punct(body, idx, ',') {
            idx += 1;
            continue;
        }

        // 1. Control flow child (`if` / `for`)
        if let Some(id) = ident_at(body, idx) {
            let s = id.to_string();
            if s == "if" || s == "for" {
                let child = parse_node_expr(body, &mut idx, reactive);
                stmts.extend("__el.push_child".parse::<TokenStream>().expect("push"));
                stmts.extend(wrap_paren(child));
                stmts.extend(";".parse::<TokenStream>().expect("end"));
                continue;
            }
        }

        // 2. Namespaced attribute (`web::class: expr` or `native::elevation: expr`)
        if let (Some(ns), true, Some(key), true) = (
            ident_at(body, idx),
            is_colon_colon(body, idx + 1),
            ident_at(body, idx + 3),
            is_punct(body, idx + 4, ':'),
        ) {
            idx += 5;
            let val_tokens = take_until_top_comma(body, &mut idx);
            let expr = transform_expr(&val_tokens);
            let mut attr_val = "::serge::core::AttrVal::from"
                .parse::<TokenStream>()
                .expect("attr from");
            attr_val.extend(wrap_paren(expr));
            let mut args = TokenStream::new();
            args.extend(std::iter::once(TokenTree::Literal(Literal::string(
                &ns.to_string(),
            ))));
            args.extend(",".parse::<TokenStream>().expect("comma"));
            args.extend(std::iter::once(TokenTree::Literal(Literal::string(
                &key.to_string(),
            ))));
            args.extend(",".parse::<TokenStream>().expect("comma"));
            args.extend(attr_val);
            stmts.extend(
                "__el.set_platform_attr"
                    .parse::<TokenStream>()
                    .expect("set_platform_attr"),
            );
            stmts.extend(wrap_paren(args));
            stmts.extend(";".parse::<TokenStream>().expect("semi"));
            continue;
        }

        // 3. Event handler without parens (`clicked => { ... }` or `clicked => async move { ... }`)
        if let Some(ev) = ident_at(body, idx)
            && is_fat_arrow(body, idx + 1)
        {
            idx += 3;
            emit_event_handler(
                &mut stmts,
                tag,
                &ev.to_string(),
                TokenStream::new(),
                body,
                &mut idx,
            );
            continue;
        }

        // 4. Event handler with parens (`changed(v) => { ... }`, `paint(bounds, ctx) => { ... }`)
        if let Some(ev) = ident_at(body, idx)
            && let Some(TokenTree::Group(args_g)) = body.get(idx + 1)
            && args_g.delimiter() == Delimiter::Parenthesis
            && is_fat_arrow(body, idx + 2)
        {
            let args = args_g.stream();
            idx += 4;
            emit_event_handler(&mut stmts, tag, &ev.to_string(), args, body, &mut idx);
            continue;
        }

        // 5. Property (`padding: 16px`, `text: ...`, `value: ...`, `range: -5.0..=5.0`)
        if let Some(prop) = ident_at(body, idx)
            && is_punct(body, idx + 1, ':')
        {
            let prop_name = prop.to_string();
            idx += 2;
            let val_tokens = take_until_top_comma(body, &mut idx);
            emit_property(&mut stmts, &prop_name, &val_tokens, reactive);
            continue;
        }

        // 6. Child UI node (`Text { ... }`, `Button { ... }`)
        if let Some(child_tag) = ident_at(body, idx)
            && let Some(TokenTree::Group(g)) = body.get(idx + 1)
            && g.delimiter() == Delimiter::Brace
        {
            idx += 2;
            let inner: Vec<TokenTree> = g.stream().into_iter().collect();
            let child = build_element(&child_tag.to_string(), &inner, reactive);
            stmts.extend("__el.push_child".parse::<TokenStream>().expect("push"));
            stmts.extend(wrap_paren(child));
            stmts.extend(";".parse::<TokenStream>().expect("end"));
            continue;
        }

        // 7. Positional content (`"Count: {count()}"` inside `Text`, `"cube"` inside `Icon`, etc.)
        let content_tokens = take_until_top_comma(body, &mut idx);
        emit_property(&mut stmts, "text", &content_tokens, reactive);
    }

    stmts.extend("__el".parse::<TokenStream>().expect("ret el"));
    wrap_brace(stmts)
}

fn emit_property(
    stmts: &mut TokenStream,
    prop_name: &str,
    val_tokens: &[TokenTree],
    reactive: bool,
) {
    // Check if the property is a single non-interpolated string literal.
    if prop_name == "text"
        && val_tokens.len() == 1
        && let TokenTree::Literal(lit) = &val_tokens[0]
        && let Some(expanded) = expand_string_literal(lit)
        && !expanded.interpolated
    {
        let lit_ts: TokenStream = std::iter::once(TokenTree::Literal(lit.clone())).collect();
        stmts.extend(
            "__el.set_text_static"
                .parse::<TokenStream>()
                .expect("static text"),
        );
        stmts.extend(wrap_paren(lit_ts));
        stmts.extend(";".parse::<TokenStream>().expect("end"));
        return;
    }

    let expr = transform_expr(val_tokens);
    let mut from_string = "::std::string::String::from"
        .parse::<TokenStream>()
        .expect("string from");
    if prop_name == "text" {
        from_string.extend(wrap_paren(expr));
        if reactive {
            let mut closure = "move || ".parse::<TokenStream>().expect("move");
            closure.extend(from_string);
            stmts.extend(
                "__el.set_text_reactive"
                    .parse::<TokenStream>()
                    .expect("text reactive"),
            );
            stmts.extend(wrap_paren(closure));
            stmts.extend(";".parse::<TokenStream>().expect("end"));
        } else {
            stmts.extend("__el.set_text".parse::<TokenStream>().expect("text"));
            stmts.extend(wrap_paren(from_string));
            stmts.extend(";".parse::<TokenStream>().expect("end"));
        }
        return;
    }

    let mut from_prop = "::serge::core::PropValue::from"
        .parse::<TokenStream>()
        .expect("prop from");
    from_prop.extend(wrap_paren(expr));

    if prop_name == "value" && reactive {
        let mut closure = "move || ".parse::<TokenStream>().expect("move");
        closure.extend(from_prop);
        stmts.extend(
            "__el.set_value_reactive"
                .parse::<TokenStream>()
                .expect("val reactive"),
        );
        stmts.extend(wrap_paren(closure));
        stmts.extend(";".parse::<TokenStream>().expect("end"));
        return;
    }

    let mut args = TokenStream::new();
    args.extend(std::iter::once(TokenTree::Literal(Literal::string(
        prop_name,
    ))));
    args.extend(",".parse::<TokenStream>().expect("comma"));
    args.extend(from_prop);
    stmts.extend("__el.set_prop".parse::<TokenStream>().expect("prop"));
    stmts.extend(wrap_paren(args));
    stmts.extend(";".parse::<TokenStream>().expect("end"));
}

fn emit_event_handler(
    stmts: &mut TokenStream,
    tag: &str,
    ev_name: &str,
    args: TokenStream,
    body: &[TokenTree],
    idx: &mut usize,
) {
    let handler_expr = take_handler_body(body, idx);

    let (method, params) = match ev_name {
        "clicked" => ("on_click", TokenStream::new()),
        "changed" | "input" => {
            let m = if matches!(tag, "Input" | "TextInput" | "TextField" | "TextArea") {
                "on_change_text"
            } else {
                "on_change_f32"
            };
            (m, args)
        }
        "paint" => ("set_paint", args),
        _ => ("on_custom_event", args),
    };

    let mut closure = TokenStream::new();
    closure.extend("move |".parse::<TokenStream>().expect("move pipe"));
    closure.extend(params);
    closure.extend("| ".parse::<TokenStream>().expect("pipe"));
    closure.extend(handler_expr);

    if method == "on_custom_event" {
        let mut call_args = TokenStream::new();
        call_args.extend(std::iter::once(TokenTree::Literal(Literal::string(
            ev_name,
        ))));
        call_args.extend(",".parse::<TokenStream>().expect("comma"));
        call_args.extend(closure);
        stmts.extend("__el.on_custom_event".parse::<TokenStream>().expect("ev"));
        stmts.extend(wrap_paren(call_args));
        stmts.extend(";".parse::<TokenStream>().expect("end"));
    } else {
        stmts.extend(
            format!("__el.{method}")
                .parse::<TokenStream>()
                .expect("method"),
        );
        stmts.extend(wrap_paren(closure));
        stmts.extend(";".parse::<TokenStream>().expect("end"));
    }
}

fn take_handler_body(tokens: &[TokenTree], idx: &mut usize) -> TokenStream {
    // Handle `async [move] { ... }`
    if is_ident(tokens, *idx, "async") {
        let mut async_prefix = TokenStream::new();
        async_prefix.extend(std::iter::once(tokens[*idx].clone()));
        *idx += 1;
        if is_ident(tokens, *idx, "move") {
            async_prefix.extend(std::iter::once(tokens[*idx].clone()));
            *idx += 1;
        }
        if let Some(TokenTree::Group(g)) = tokens.get(*idx)
            && g.delimiter() == Delimiter::Brace
        {
            let mut fut_tokens = async_prefix;
            fut_tokens.extend(std::iter::once(TokenTree::Group(Group::new(
                Delimiter::Brace,
                transform_stream(g.stream()),
            ))));
            *idx += 1;
            if is_punct(tokens, *idx, ',') {
                *idx += 1;
            }
            let mut block = TokenStream::new();
            block.extend(
                "::hyve::api::block_on"
                    .parse::<TokenStream>()
                    .expect("block_on"),
            );
            block.extend(wrap_paren(fut_tokens));
            block.extend(";".parse::<TokenStream>().expect("semi"));
            return wrap_brace(block);
        }
    }

    if let Some(TokenTree::Group(g)) = tokens.get(*idx)
        && g.delimiter() == Delimiter::Brace
    {
        *idx += 1;
        if is_punct(tokens, *idx, ',') {
            *idx += 1;
        }
        return TokenTree::Group(Group::new(Delimiter::Brace, transform_stream(g.stream()))).into();
    }

    let raw = take_until_top_comma(tokens, idx);
    transform_expr(&raw)
}

fn take_until_top_comma(tokens: &[TokenTree], idx: &mut usize) -> Vec<TokenTree> {
    let mut out = Vec::new();
    while *idx < tokens.len() {
        if is_punct(tokens, *idx, ',') {
            *idx += 1;
            break;
        }
        out.push(tokens[*idx].clone());
        *idx += 1;
    }
    out
}

fn transform_stream(stream: TokenStream) -> TokenStream {
    let vec: Vec<TokenTree> = stream.into_iter().collect();
    transform_expr(&vec)
}

fn transform_expr(tokens: &[TokenTree]) -> TokenStream {
    let mut out = TokenStream::new();
    for tt in tokens {
        match tt {
            TokenTree::Group(g) => {
                let inner = transform_stream(g.stream());
                let mut new_g = Group::new(g.delimiter(), inner);
                new_g.set_span(g.span());
                out.extend(std::iter::once(TokenTree::Group(new_g)));
            }
            TokenTree::Literal(lit) => {
                if let Some(px_expr) = try_parse_px_literal(lit) {
                    out.extend(px_expr);
                } else if let Some(str_exp) = expand_string_literal(lit)
                    && str_exp.interpolated
                {
                    out.extend(str_exp.tokens);
                } else {
                    out.extend(std::iter::once(TokenTree::Literal(lit.clone())));
                }
            }
            other => out.extend(std::iter::once(other.clone())),
        }
    }
    out
}

fn try_parse_px_literal(lit: &Literal) -> Option<TokenStream> {
    let s = lit.to_string();
    if s.starts_with('"') || s.starts_with('\'') || !s.ends_with("px") {
        return None;
    }
    let num_part = s.strip_suffix("px")?.replace('_', "");
    let val: f32 = num_part.parse().ok()?;
    Some(
        format!("::serge::style::Length::Px({}_f32)", val)
            .parse()
            .expect("valid Length::Px"),
    )
}

struct ExpandedString {
    interpolated: bool,
    tokens: TokenStream,
}

fn expand_string_literal(lit: &Literal) -> Option<ExpandedString> {
    let raw = lit.to_string();
    let content = unquote_rust_string(&raw)?;
    if !content.contains('{') {
        return Some(ExpandedString {
            interpolated: false,
            tokens: std::iter::once(TokenTree::Literal(lit.clone())).collect(),
        });
    }

    let chars: Vec<char> = content.chars().collect();
    let mut i = 0;
    let mut fmt_str = String::new();
    let mut args: Vec<String> = Vec::new();

    while i < chars.len() {
        if chars[i] == '{' {
            if chars.get(i + 1) == Some(&'{') {
                fmt_str.push_str("{{");
                i += 2;
                continue;
            }
            i += 1;
            let start = i;
            let mut brace_depth = 0usize;
            let mut paren_depth = 0usize;
            let mut bracket_depth = 0usize;
            let mut in_quote = false;
            let mut escaped = false;
            let mut split_colon: Option<usize> = None;

            while i < chars.len() {
                let c = chars[i];
                if in_quote {
                    if escaped {
                        escaped = false;
                    } else if c == '\\' {
                        escaped = true;
                    } else if c == '"' {
                        in_quote = false;
                    }
                    i += 1;
                    continue;
                }
                match c {
                    '"' => in_quote = true,
                    '(' => paren_depth += 1,
                    ')' => paren_depth = paren_depth.saturating_sub(1),
                    '[' => bracket_depth += 1,
                    ']' => bracket_depth = bracket_depth.saturating_sub(1),
                    '{' => brace_depth += 1,
                    '}' => {
                        if brace_depth == 0 {
                            break;
                        }
                        brace_depth -= 1;
                    }
                    ':' if brace_depth == 0
                        && paren_depth == 0
                        && bracket_depth == 0
                        && split_colon.is_none() =>
                    {
                        let prev_colon = i > start && chars[i - 1] == ':';
                        let next_colon = chars.get(i + 1) == Some(&':');
                        if !prev_colon && !next_colon {
                            split_colon = Some(i);
                        }
                    }
                    _ => {}
                }
                i += 1;
            }

            let end = i;
            if i < chars.len() && chars[i] == '}' {
                i += 1;
            }

            let (expr_str, spec) = if let Some(col) = split_colon {
                let e: String = chars[start..col].iter().collect();
                let sp: String = chars[col + 1..end].iter().collect();
                (e, Some(sp))
            } else {
                let e: String = chars[start..end].iter().collect();
                (e, None)
            };

            if let Some(sp) = spec {
                fmt_str.push_str("{:");
                fmt_str.push_str(&sp);
                fmt_str.push('}');
            } else {
                fmt_str.push_str("{}");
            }
            args.push(expr_str);
        } else if chars[i] == '}' && chars.get(i + 1) == Some(&'}') {
            fmt_str.push_str("}}");
            i += 2;
        } else {
            fmt_str.push(chars[i]);
            i += 1;
        }
    }

    if args.is_empty() {
        return Some(ExpandedString {
            interpolated: false,
            tokens: std::iter::once(TokenTree::Literal(lit.clone())).collect(),
        });
    }

    let mut fmt_args = TokenStream::new();
    fmt_args.extend(std::iter::once(TokenTree::Literal(Literal::string(
        &fmt_str,
    ))));
    for arg in args {
        fmt_args.extend(",".parse::<TokenStream>().expect("comma"));
        let parsed: TokenStream = arg
            .parse()
            .unwrap_or_else(|_| compile_error(lit.span(), "invalid expression inside `{...}`"));
        fmt_args.extend(transform_stream(parsed));
    }

    let mut out = "::std::format!"
        .parse::<TokenStream>()
        .expect("format macro");
    out.extend(wrap_paren(fmt_args));

    Some(ExpandedString {
        interpolated: true,
        tokens: out,
    })
}

fn unquote_rust_string(raw: &str) -> Option<String> {
    if let Some(inner) = raw.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next()? {
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    '\\' => out.push('\\'),
                    '"' => out.push('"'),
                    '\'' => out.push('\''),
                    '0' => out.push('\0'),
                    other => {
                        out.push('\\');
                        out.push(other);
                    }
                }
            } else {
                out.push(c);
            }
        }
        return Some(out);
    }

    if let Some(rest) = raw.strip_prefix('r') {
        let hashes = rest.bytes().take_while(|&b| b == b'#').count();
        let after_hashes = &rest[hashes..];
        let body = after_hashes.strip_prefix('"')?;
        let suffix_len = 1 + hashes;
        if body.len() >= suffix_len {
            return Some(body[..body.len() - suffix_len].to_owned());
        }
    }
    None
}

fn ident_at(tokens: &[TokenTree], idx: usize) -> Option<Ident> {
    match tokens.get(idx) {
        Some(TokenTree::Ident(id)) => Some(id.clone()),
        _ => None,
    }
}

fn is_ident(tokens: &[TokenTree], idx: usize, expected: &str) -> bool {
    ident_at(tokens, idx).is_some_and(|id| id.to_string() == expected)
}

fn is_punct(tokens: &[TokenTree], idx: usize, ch: char) -> bool {
    matches!(tokens.get(idx), Some(TokenTree::Punct(p)) if p.as_char() == ch)
}

fn is_colon_colon(tokens: &[TokenTree], idx: usize) -> bool {
    matches!(
        (tokens.get(idx), tokens.get(idx + 1)),
        (Some(TokenTree::Punct(a)), Some(TokenTree::Punct(b)))
            if a.as_char() == ':' && a.spacing() == Spacing::Joint && b.as_char() == ':'
    )
}

fn is_fat_arrow(tokens: &[TokenTree], idx: usize) -> bool {
    matches!(
        (tokens.get(idx), tokens.get(idx + 1)),
        (Some(TokenTree::Punct(a)), Some(TokenTree::Punct(b)))
            if a.as_char() == '=' && b.as_char() == '>'
    )
}

fn wrap_brace(stream: TokenStream) -> TokenStream {
    std::iter::once(TokenTree::Group(Group::new(Delimiter::Brace, stream))).collect()
}

fn wrap_paren(stream: TokenStream) -> TokenStream {
    std::iter::once(TokenTree::Group(Group::new(Delimiter::Parenthesis, stream))).collect()
}

fn wrap_bracket(stream: TokenStream) -> TokenStream {
    std::iter::once(TokenTree::Group(Group::new(Delimiter::Bracket, stream))).collect()
}

fn compile_error(span: Span, msg: &str) -> TokenStream {
    let mut ts: TokenStream = format!("::core::compile_error!({:?})", msg)
        .parse()
        .expect("compile_error macro");
    ts = ts
        .into_iter()
        .map(|mut tt| {
            tt.set_span(span);
            tt
        })
        .collect();
    ts
}
