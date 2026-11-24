//! `ui!` (`docs/01-dx-and-dsl.md` §1): parsed over raw token trees so the
//! grammar's own punctuation (`name:` vs `web::name:`, `=>`) is exact and
//! every error path advances, ending in a spanned diagnostic.

use proc_macro2::{Delimiter, Group, Ident, Literal, Punct, Spacing, Span, TokenStream, TokenTree};
use quote::{ToTokens, quote, quote_spanned};
use syn::{Expr, LitStr};

type Result<T> = std::result::Result<T, syn::Error>;

/// One `ui!` node.
enum Node {
    Widget {
        path: TokenStream,
        span: Span,
        items: Vec<Item>,
    },
    Text(LitStr),
    If(IfNode),
    For {
        pat: TokenStream,
        iter: TokenStream,
        key: TokenStream,
        body: Vec<Node>,
        span: Span,
    },
    Embed(TokenStream, Span),
}

struct IfNode {
    cond: TokenStream,
    then: Vec<Node>,
    els: Option<Box<Else>>,
}

enum Else {
    If(IfNode),
    Block(Vec<Node>),
}

/// One item inside a widget's braces.
enum Item {
    Prop {
        ns: Option<Ident>,
        name: Ident,
        value: TokenStream,
        value_span: Span,
    },
    Event {
        name: Ident,
        args: Vec<TokenStream>,
        body: TokenStream,
        is_async: bool,
    },
    Child(Node),
}

struct Cursor {
    toks: Vec<TokenTree>,
    i: usize,
    end_span: Span,
}

impl Cursor {
    fn new(ts: TokenStream, end_span: Span) -> Cursor {
        Cursor {
            toks: ts.into_iter().collect(),
            i: 0,
            end_span,
        }
    }

    fn peek(&self, n: usize) -> Option<&TokenTree> {
        self.toks.get(self.i + n)
    }

    fn done(&self) -> bool {
        self.i >= self.toks.len()
    }

    fn next(&mut self) -> Option<TokenTree> {
        let t = self.toks.get(self.i).cloned();
        self.i += 1;
        t
    }

    fn span(&self) -> Span {
        self.peek(0).map_or(self.end_span, TokenTree::span)
    }

    fn is_ident(&self, n: usize, name: &str) -> bool {
        matches!(self.peek(n), Some(TokenTree::Ident(i)) if i == name)
    }

    fn is_punct(&self, n: usize, c: char) -> bool {
        matches!(self.peek(n), Some(TokenTree::Punct(p)) if p.as_char() == c)
    }

    fn is_punct_alone(&self, n: usize, c: char) -> bool {
        matches!(self.peek(n), Some(TokenTree::Punct(p)) if p.as_char() == c && p.spacing() == Spacing::Alone)
    }

    fn is_group(&self, n: usize, d: Delimiter) -> bool {
        matches!(self.peek(n), Some(TokenTree::Group(g)) if g.delimiter() == d)
    }

    /// `=>` at `n`.
    fn is_fat_arrow(&self, n: usize) -> bool {
        matches!(self.peek(n), Some(TokenTree::Punct(p)) if p.as_char() == '=' && p.spacing() == Spacing::Joint)
            && self.is_punct(n + 1, '>')
    }

    fn skip_comma(&mut self) {
        if self.is_punct(0, ',') {
            self.i += 1;
        }
    }

    /// Tokens up to (not including) the next top-level `,`.
    fn until_comma(&mut self) -> TokenStream {
        let mut out = TokenStream::new();
        while let Some(t) = self.peek(0) {
            if matches!(t, TokenTree::Punct(p) if p.as_char() == ',') {
                break;
            }
            out.extend([self.next().expect("peeked")]);
        }
        out
    }

    /// Tokens up to (not including) the next brace group.
    fn until_brace(&mut self) -> TokenStream {
        let mut out = TokenStream::new();
        while !self.done() && !self.is_group(0, Delimiter::Brace) {
            out.extend([self.next().expect("not done")]);
        }
        out
    }

    fn brace(&mut self, what: &str) -> Result<Group> {
        match self.next() {
            Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Brace => Ok(g),
            Some(t) => Err(syn::Error::new(t.span(), format!("expected `{{` {what}"))),
            None => Err(syn::Error::new(
                self.end_span,
                format!("expected `{{` {what}"),
            )),
        }
    }
}

fn is_str_lit(t: &TokenTree) -> bool {
    matches!(t, TokenTree::Literal(l) if { let s = l.to_string(); s.starts_with('"') || s.starts_with("r\"") || s.starts_with("r#") })
}

/// Parses the nodes of `ui! { .. }` or of an `if`/`for` body.
fn parse_nodes(ts: TokenStream, span: Span) -> Result<Vec<Node>> {
    let mut c = Cursor::new(ts, span);
    let mut out = Vec::new();
    while !c.done() {
        if c.is_punct(0, ',') {
            c.i += 1;
            continue;
        }
        let at = c.span();
        if let Some(TokenTree::Ident(i)) = c.peek(0)
            && (c.is_punct_alone(1, ':')
                || c.is_fat_arrow(1)
                || c.is_group(1, Delimiter::Parenthesis) && c.is_fat_arrow(2))
        {
            return Err(syn::Error::new(
                i.span(),
                "properties and events go inside a widget's braces: `Widget { name: value, event => { .. } }`",
            ));
        }
        if c.peek(0).is_some_and(is_str_lit) {
            return Err(syn::Error::new(
                at,
                "text goes inside a text widget: `Text { \"..\" }`",
            ));
        }
        out.push(parse_node(&mut c)?);
        c.skip_comma();
    }
    Ok(out)
}

/// A node at the cursor (widget, `if`, `for` or `{expr}`).
fn parse_node(c: &mut Cursor) -> Result<Node> {
    let at = c.span();
    if c.is_ident(0, "if") {
        c.i += 1;
        return parse_if(c, at).map(Node::If);
    }
    if c.is_ident(0, "for") {
        c.i += 1;
        let mut pat = TokenStream::new();
        while !c.done() && !c.is_ident(0, "in") {
            pat.extend([c.next().expect("not done")]);
        }
        if pat.is_empty() {
            return Err(syn::Error::new(
                at,
                "expected a pattern after `for`: `for item in items() { key: item.id, .. }`",
            ));
        }
        if c.done() {
            return Err(syn::Error::new(
                at,
                "expected `in` in `for pattern in items { .. }`",
            ));
        }
        c.i += 1;
        let iter = c.until_brace();
        let body = c.brace("after the `for` items")?;
        let mut b = Cursor::new(body.stream(), body.span_close());
        if !(b.is_ident(0, "key") && b.is_punct_alone(1, ':')) {
            return Err(syn::Error::new(
                body.span_open(),
                "a `for` in `ui!` needs a key first: `for item in items() { key: item.id, .. }`",
            ));
        }
        b.i += 2;
        let key = b.until_comma();
        if key.is_empty() {
            return Err(syn::Error::new(
                b.span(),
                "expected the key expression after `key:`",
            ));
        }
        b.skip_comma();
        let rest: TokenStream = b.toks[b.i..].iter().cloned().collect();
        let body = parse_nodes(rest, body.span_close())?;
        return Ok(Node::For {
            pat,
            iter,
            key,
            body,
            span: at,
        });
    }
    if let Some(TokenTree::Group(g)) = c.peek(0)
        && g.delimiter() == Delimiter::Brace
    {
        let g = g.clone();
        c.i += 1;
        return Ok(Node::Embed(g.stream(), g.span()));
    }
    // A widget: a path followed by braces.
    let mut path = TokenStream::new();
    loop {
        match c.peek(0) {
            Some(TokenTree::Ident(_)) => path.extend([c.next().expect("peeked")]),
            Some(TokenTree::Punct(p)) if p.as_char() == ':' && c.is_punct(1, ':') => {
                path.extend([c.next().expect("peeked"), c.next().expect("peeked")]);
            }
            _ => break,
        }
    }
    if path.is_empty() {
        return Err(syn::Error::new(
            at,
            "expected a widget (`Card { .. }`), `if`, `for` or `{expr}`",
        ));
    }
    if c.is_punct_alone(0, ':') && path.to_string().contains("::") {
        return Err(syn::Error::new(
            at,
            format!(
                "`{}` is not a property: target attributes are `web::<name>` and `native::<name>`",
                path.to_string().replace(' ', "")
            ),
        ));
    }
    let body = c.brace("after the widget name (`Widget { .. }`)")?;
    let items = parse_items(body.stream(), body.span_close())?;
    Ok(Node::Widget {
        path,
        span: at,
        items,
    })
}

fn parse_if(c: &mut Cursor, at: Span) -> Result<IfNode> {
    let cond = c.until_brace();
    if cond.is_empty() {
        return Err(syn::Error::new(at, "expected a condition after `if`"));
    }
    let then = c.brace("after the `if` condition")?;
    let then = parse_nodes(then.stream(), then.span_close())?;
    let els = if c.is_ident(0, "else") {
        let at = c.span();
        c.i += 1;
        if c.is_ident(0, "if") {
            c.i += 1;
            Some(Box::new(Else::If(parse_if(c, at)?)))
        } else {
            let b = c.brace("after `else`")?;
            Some(Box::new(Else::Block(parse_nodes(
                b.stream(),
                b.span_close(),
            )?)))
        }
    } else {
        None
    };
    Ok(IfNode { cond, then, els })
}

/// The items inside a widget's braces.
fn parse_items(ts: TokenStream, end: Span) -> Result<Vec<Item>> {
    let mut c = Cursor::new(ts, end);
    let mut out = Vec::new();
    while !c.done() {
        if c.is_punct(0, ',') {
            c.i += 1;
            continue;
        }
        // Positional text.
        if let Some(TokenTree::Literal(l)) = c.peek(0)
            && is_str_lit(&TokenTree::Literal(l.clone()))
        {
            let lit: LitStr = syn::parse2(TokenTree::Literal(l.clone()).into())?;
            c.i += 1;
            out.push(Item::Child(Node::Text(lit)));
            c.skip_comma();
            continue;
        }
        if let Some(TokenTree::Ident(name)) = c.peek(0).cloned() {
            // name: value
            if c.is_punct_alone(1, ':') {
                c.i += 2;
                let value_span = c.span();
                let value = c.until_comma();
                if value.is_empty() {
                    return Err(syn::Error::new(
                        value_span,
                        format!("expected a value for `{name}`"),
                    ));
                }
                out.push(Item::Prop {
                    ns: None,
                    name,
                    value,
                    value_span,
                });
                c.skip_comma();
                continue;
            }
            // ns::name: value
            if (name == "web" || name == "native")
                && c.is_punct(1, ':')
                && c.is_punct(2, ':')
                && matches!(c.peek(3), Some(TokenTree::Ident(_)))
                && c.is_punct_alone(4, ':')
            {
                let Some(TokenTree::Ident(attr)) = c.peek(3).cloned() else {
                    unreachable!()
                };
                c.i += 5;
                let value_span = c.span();
                let value = c.until_comma();
                if value.is_empty() {
                    return Err(syn::Error::new(
                        value_span,
                        format!("expected a value for `{name}::{attr}`"),
                    ));
                }
                out.push(Item::Prop {
                    ns: Some(name),
                    name: attr,
                    value,
                    value_span,
                });
                c.skip_comma();
                continue;
            }
            // event => body | event(args) => body
            let with_args = c.is_group(1, Delimiter::Parenthesis) && c.is_fat_arrow(2);
            if c.is_fat_arrow(1) || with_args {
                c.i += 1;
                let mut args = Vec::new();
                if with_args {
                    let Some(TokenTree::Group(g)) = c.next() else {
                        unreachable!()
                    };
                    let mut a = Cursor::new(g.stream(), g.span_close());
                    while !a.done() {
                        let arg = a.until_comma();
                        if !arg.is_empty() {
                            args.push(arg);
                        }
                        a.skip_comma();
                    }
                }
                c.i += 2; // =>
                let is_async = c.is_ident(0, "async");
                let mut body = TokenStream::new();
                if is_async {
                    body.extend([c.next().expect("async")]);
                    if c.is_ident(0, "move") {
                        body.extend([c.next().expect("move")]);
                    }
                }
                let block = c.brace(if is_async {
                    "for the async handler body (`=> async move { .. }`)"
                } else {
                    "for the handler body (`=> { .. }`)"
                })?;
                body.extend([TokenTree::Group(block)]);
                out.push(Item::Event {
                    name,
                    args,
                    body,
                    is_async,
                });
                c.skip_comma();
                continue;
            }
        }
        out.push(Item::Child(parse_node(&mut c)?));
        c.skip_comma();
    }
    Ok(out)
}

// ---------------------------------------------------------------- codegen

/// `ui! { .. }`.
pub fn expand(input: TokenStream) -> Result<TokenStream> {
    let nodes = parse_nodes(input, Span::call_site())?;
    Ok(fragment(&nodes))
}

fn fragment(nodes: &[Node]) -> TokenStream {
    let parts: Vec<TokenStream> = nodes.iter().map(node).collect();
    if parts.len() == 1 {
        return parts.into_iter().next().expect("one");
    }
    quote! { ::serge::__private::fragment(::std::vec![#(#parts),*]) }
}

/// Where a node starts (for diagnostics).
fn node_span(n: &Node) -> Span {
    match n {
        Node::Widget { span, .. } | Node::For { span, .. } => *span,
        Node::Text(lit) => lit.span(),
        Node::Embed(_, span) => *span,
        Node::If(_) => Span::call_site(),
    }
}

fn node(n: &Node) -> TokenStream {
    match n {
        Node::Widget { path, span, items } => {
            let span = *span;
            let mut acc = quote_spanned! {span=> <#path>::__new() };
            for item in items {
                acc = match item {
                    // `child(builder, element)`: a builder without children
                    // fails the `Children` bound with a readable message.
                    Item::Child(n) if !matches!(n, Node::Text(_)) => {
                        let e = node(n);
                        let at = node_span(n);
                        quote_spanned! {at=> ::serge::__private::child(#acc, #e) }
                    }
                    _ => {
                        let call = item_call(item);
                        quote! { #acc #call }
                    }
                };
            }
            quote_spanned! {span=>
                ::serge::__private::element(move || {
                    #[allow(unused_imports)]
                    use ::serge::__private::Styled as _;
                    #acc
                })
            }
        }
        Node::Text(lit) => {
            let span = lit.span();
            quote_spanned! {span=> ::core::compile_error!("text goes inside a text widget: `Text { \"..\" }`") }
        }
        Node::If(i) => {
            let body = if_expr(i);
            quote! { ::serge::__private::region(move || #body) }
        }
        Node::For {
            pat,
            iter,
            key,
            body,
            span,
        } => {
            let body = fragment(body);
            let span = Span::call_site().located_at(*span);
            quote_spanned! {span=>
                ::serge::__private::keyed(
                    move || #iter,
                    move |#pat: &_| #key,
                    move |#pat| #body,
                )
            }
        }
        Node::Embed(e, span) => {
            quote_spanned! {*span=> {
                #[allow(unused_braces)]
                let __serge_embed = { #e };
                ::serge::__private::IntoElement::into_element(__serge_embed)
            } }
        }
    }
}

fn if_expr(i: &IfNode) -> TokenStream {
    let cond = &i.cond;
    let then = fragment(&i.then);
    let els = match i.els.as_deref() {
        None => quote! { ::serge::__private::Element::empty() },
        Some(Else::Block(b)) => fragment(b),
        Some(Else::If(e)) => if_expr(e),
    };
    quote! { if #cond { #then } else { #els } }
}

fn item_call(item: &Item) -> TokenStream {
    match item {
        Item::Prop {
            ns: None,
            name,
            value,
            value_span,
        } => {
            let v = prop_value(value, *value_span);
            quote_spanned! {name.span()=> .#name(#v) }
        }
        Item::Prop {
            ns: Some(ns),
            name,
            value,
            value_span,
        } => {
            let v = prop_value(value, *value_span);
            let method = Ident::new(&format!("__{ns}"), name.span());
            quote_spanned! {name.span()=> .#method(::serge::__private::#ns::#name(#v)) }
        }
        Item::Event {
            name,
            args,
            body,
            is_async,
        } => {
            // The method is the event's own name, so rustc's "similar name"
            // help for a typo suggests valid `ui!` syntax.
            let method = name.clone();
            let body = if *is_async {
                quote! { { ::serge::__private::spawn(#body); } }
            } else {
                body.clone()
            };
            quote_spanned! {name.span()=> .#method(move |#(#args),*| #body) }
        }
        Item::Child(Node::Text(lit)) => {
            let v = text_value(lit);
            quote_spanned! {lit.span()=> .__text(#v) }
        }
        Item::Child(_) => unreachable!("children are folded in `node`"),
    }
}

/// A unit literal (`16px`, `1.5rem`) as a `Length` constructor.
fn unit_literal(l: &Literal) -> Option<TokenStream> {
    let s = l.to_string();
    let (num, unit) = ["px", "rem"]
        .iter()
        .find_map(|u| s.strip_suffix(u).map(|n| (n.to_owned(), *u)))?;
    if num.is_empty()
        || !num
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == '_')
    {
        return None;
    }
    let num: f64 = num.replace('_', "").parse().ok()?;
    let f = Ident::new(unit, l.span());
    let lit = Literal::f32_suffixed(num as f32);
    Some(quote_spanned! {l.span()=> ::serge::__private::#f(#lit) })
}

/// A number with a suffix that is neither a unit nor a Rust numeric type
/// (`2vw`): the unit it names.
fn unknown_unit(l: &Literal) -> Option<String> {
    let s = l.to_string();
    if !s.starts_with(|c: char| c.is_ascii_digit())
        || s.starts_with("0x")
        || s.starts_with("0o")
        || s.starts_with("0b")
    {
        return None;
    }
    let suffix = s.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == '_');
    let exponent = suffix.starts_with(['e', 'E'])
        && suffix[1..].starts_with(|c: char| c.is_ascii_digit() || c == '+' || c == '-');
    const RUST: [&str; 14] = [
        "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64", "i128", "isize",
        "f32", "f64",
    ];
    (!suffix.is_empty() && !exponent && !RUST.contains(&suffix) && !["px", "rem"].contains(&suffix))
        .then(|| suffix.to_owned())
}

/// Rewrites unit literals anywhere in a property expression.
fn rewrite_units(ts: TokenStream) -> TokenStream {
    ts.into_iter()
        .map(|t| match t {
            TokenTree::Literal(l) => match unit_literal(&l) {
                Some(e) => TokenTree::Group(Group::new(Delimiter::Parenthesis, e)),
                None => match unknown_unit(&l) {
                    Some(u) => {
                        let msg = format!("unknown unit `{u}`: units are `px`, `rem` and `%`");
                        TokenTree::Group(Group::new(
                            Delimiter::Parenthesis,
                            quote_spanned! {l.span()=> ::core::compile_error!(#msg) },
                        ))
                    }
                    None => TokenTree::Literal(l),
                },
            },
            TokenTree::Group(g) => {
                let mut ng = Group::new(g.delimiter(), rewrite_units(g.stream()));
                ng.set_span(g.span());
                TokenTree::Group(ng)
            }
            other => other,
        })
        .collect()
}

/// A property value: `Lit(..)` (evaluated once) or `Dyn(move || ..)`
/// (reactive). Literals, unit literals, plain paths and field accesses are
/// fixed; anything else may read signals.
fn prop_value(value: &TokenStream, span: Span) -> TokenStream {
    let toks: Vec<TokenTree> = value.clone().into_iter().collect();
    // `50%`
    if let [TokenTree::Literal(l), TokenTree::Punct(p)] = toks.as_slice()
        && p.as_char() == '%'
        && l.to_string()
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.')
    {
        let n: f64 = l.to_string().parse().unwrap_or(0.0);
        let lit = Literal::f32_suffixed(n as f32);
        return quote_spanned! {span=> ::serge::__private::Lit(::serge::__private::pct(#lit)) };
    }
    if let [TokenTree::Literal(l)] = toks.as_slice() {
        if let Some(e) = unit_literal(l) {
            return quote_spanned! {span=> ::serge::__private::Lit(#e) };
        }
        if is_str_lit(&toks[0])
            && let Ok(lit) = syn::parse2::<LitStr>(value.clone())
        {
            return text_value(&lit);
        }
    }
    let rewritten = rewrite_units(value.clone());
    // Generated wrappers carry macro hygiene at the user's location: errors
    // point at the value, and lints treat the closure as macro output.
    let span = Span::call_site().located_at(span);
    match syn::parse2::<Expr>(rewritten.clone()) {
        Ok(e) if is_fixed(&e) => quote_spanned! {span=> ::serge::__private::Lit(#rewritten) },
        Ok(_) => quote_spanned! {span=> ::serge::__private::Dyn(move || #rewritten) },
        // Let rustc report the expression's own error at its span.
        Err(_) => quote_spanned! {span=> ::serge::__private::Dyn(move || { #rewritten }) },
    }
}

fn is_fixed(e: &Expr) -> bool {
    match e {
        Expr::Lit(_) | Expr::Path(_) => true,
        Expr::Field(f) => is_fixed(&f.base),
        Expr::Unary(u) => matches!(u.op, syn::UnOp::Neg(_)) && is_fixed(&u.expr),
        Expr::Range(r) => {
            r.start.as_deref().is_none_or(is_fixed) && r.end.as_deref().is_none_or(is_fixed)
        }
        Expr::Paren(p) => is_fixed(&p.expr),
        Expr::Group(g) => is_fixed(&g.expr),
        Expr::Tuple(t) => t.elems.iter().all(is_fixed),
        Expr::Array(a) => a.elems.iter().all(is_fixed),
        Expr::Reference(r) => is_fixed(&r.expr),
        _ => false,
    }
}

/// A string in text or property position: fixed, or `format!` when it
/// interpolates `{expr}` / `{expr:spec}`.
fn text_value(lit: &LitStr) -> TokenStream {
    let span = Span::call_site().located_at(lit.span());
    match interpolate(lit) {
        Ok(None) => quote_spanned! {span=> ::serge::__private::Lit(#lit) },
        Ok(Some((fmt, args))) => {
            let fmt = LitStr::new(&fmt, span);
            quote_spanned! {span=> ::serge::__private::Dyn(move || ::std::format!(#fmt #(, #args)*)) }
        }
        Err(e) => e.to_compile_error(),
    }
}

/// Splits `"Count: {count():.2}"` into a format string and its arguments
/// (`docs/04-open-questions.md` #4: the format spec starts after the last
/// top-level `:` that is not part of `::`; `{{` and `}}` are literal
/// braces).
fn interpolate(lit: &LitStr) -> Result<Option<(String, Vec<TokenStream>)>> {
    let s = lit.value();
    let chars: Vec<char> = s.chars().collect();
    let mut fmt = String::new();
    let mut args = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '{' if chars.get(i + 1) == Some(&'{') => {
                fmt.push_str("{{");
                i += 2;
            }
            '}' if chars.get(i + 1) == Some(&'}') => {
                fmt.push_str("}}");
                i += 2;
            }
            '}' => {
                return Err(syn::Error::new(
                    lit.span(),
                    "unmatched `}` in text; write `}}` for a literal brace",
                ));
            }
            '{' => {
                let start = i + 1;
                let mut open: Vec<char> = Vec::new();
                let mut j = start;
                let mut in_str: Option<char> = None;
                while j < chars.len() {
                    let ch = chars[j];
                    if let Some(q) = in_str {
                        if ch == '\\' {
                            j += 1;
                        } else if ch == q {
                            in_str = None;
                        }
                    } else {
                        match ch {
                            '"' => in_str = Some('"'),
                            '(' | '[' | '{' => open.push(ch),
                            ')' | ']' => {
                                open.pop();
                            }
                            '}' if open.is_empty() => break,
                            '}' if open.last() == Some(&'{') => {
                                open.pop();
                            }
                            '}' => {
                                let o = open.last().copied().unwrap_or('(');
                                return Err(syn::Error::new(
                                    lit.span(),
                                    format!("`{o}` is not closed inside `{{..}}` in this text"),
                                ));
                            }
                            _ => {}
                        }
                    }
                    j += 1;
                }
                if j >= chars.len() {
                    return Err(syn::Error::new(
                        lit.span(),
                        "unclosed `{` in text; write `{{` for a literal brace",
                    ));
                }
                let inner: String = chars[start..j].iter().collect();
                let (expr, spec) = split_spec(&inner);
                if expr.trim().is_empty() {
                    return Err(syn::Error::new(lit.span(), "empty `{}` in text"));
                }
                let parsed: Expr = syn::parse_str(expr)
                    .map_err(|e| syn::Error::new(lit.span(), format!("in `{{{inner}}}`: {e}")))?;
                args.push(respan(parsed.to_token_stream(), lit.span()));
                fmt.push('{');
                if let Some(spec) = spec {
                    fmt.push(':');
                    fmt.push_str(spec);
                }
                fmt.push('}');
                i = j + 1;
            }
            c => {
                fmt.push(c);
                i += 1;
            }
        }
    }
    Ok(if args.is_empty() {
        None
    } else {
        Some((fmt, args))
    })
}

/// `expr:spec` at the last top-level `:` that is not part of `::`.
fn split_spec(inner: &str) -> (&str, Option<&str>) {
    let b: Vec<char> = inner.chars().collect();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut at = None;
    let mut idx = 0;
    let mut byte = 0;
    let mut bytes = Vec::with_capacity(b.len());
    for ch in &b {
        bytes.push(byte);
        byte += ch.len_utf8();
    }
    while idx < b.len() {
        let ch = b[idx];
        if in_str {
            if ch == '\\' {
                idx += 1;
            } else if ch == '"' {
                in_str = false;
            }
        } else {
            match ch {
                '"' => in_str = true,
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                ':' if depth == 0 => {
                    let joined = (idx > 0 && b[idx - 1] == ':') || b.get(idx + 1) == Some(&':');
                    if !joined {
                        at = Some(bytes[idx]);
                    }
                }
                _ => {}
            }
        }
        idx += 1;
    }
    match at {
        Some(p) => (&inner[..p], Some(&inner[p + 1..])),
        None => (inner, None),
    }
}

fn respan(ts: TokenStream, span: Span) -> TokenStream {
    ts.into_iter()
        .map(|t| match t {
            TokenTree::Group(g) => {
                let mut ng = Group::new(g.delimiter(), respan(g.stream(), span));
                ng.set_span(span);
                TokenTree::Group(ng)
            }
            TokenTree::Ident(mut i) => {
                i.set_span(span);
                TokenTree::Ident(i)
            }
            TokenTree::Punct(mut p) => {
                p.set_span(span);
                TokenTree::Punct(p)
            }
            TokenTree::Literal(mut l) => {
                l.set_span(span);
                TokenTree::Literal(l)
            }
        })
        .collect()
}

#[allow(dead_code)]
fn punct(c: char) -> Punct {
    Punct::new(c, Spacing::Alone)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_specs_split_at_the_last_single_colon() {
        assert_eq!(split_spec("exposure():.2"), ("exposure()", Some(".2")));
        assert_eq!(split_spec("Foo::bar()"), ("Foo::bar()", None));
        assert_eq!(split_spec("Foo::bar():>8"), ("Foo::bar()", Some(">8")));
        assert_eq!(split_spec("m[\"a:b\"]"), ("m[\"a:b\"]", None));
        assert_eq!(split_spec("f(|x: u8| x)"), ("f(|x: u8| x)", None));
    }
}
