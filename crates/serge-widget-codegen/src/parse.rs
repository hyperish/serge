use super::{Field, Node, Part, Platform, Widget};
use syn::{
    Expr, Ident, LitInt, LitStr, Macro, Result, Token, braced,
    parse::{Parse, ParseStream},
    spanned::Spanned,
};

struct Raw {
    kind: Ident,
    body: proc_macro2::TokenStream,
}
impl Parse for Raw {
    fn parse(input: ParseStream) -> Result<Self> {
        let kind = input.parse()?;
        let body;
        braced!(body in input);
        Ok(Self {
            kind,
            body: body.parse()?,
        })
    }
}
struct Children {
    gap: u32,
    children: Vec<Raw>,
}
impl Parse for Children {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut gap = None;
        let mut children = Vec::new();
        while !input.is_empty() {
            let fork = input.fork();
            let key: Ident = fork.parse()?;
            if fork.peek(Token![:]) {
                if key != "gap" || gap.is_some() {
                    return Err(syn::Error::new(
                        key.span(),
                        "the only stack property is a single constant gap",
                    ));
                }
                input.parse::<Ident>()?;
                input.parse::<Token![:]>()?;
                let value: LitInt = input.parse()?;
                if value.suffix() != "px" {
                    return Err(syn::Error::new(value.span(), "expected an integer px gap"));
                }
                gap = Some(value.base10_parse()?);
                input.parse::<Token![,]>()?;
            } else {
                children.push(input.parse()?);
                if input.peek(Token![,]) {
                    input.parse::<Token![,]>()?;
                }
            }
        }
        Ok(Self {
            gap: gap.unwrap_or(0),
            children,
        })
    }
}
struct Section(Ident, Macro);
impl Parse for Section {
    fn parse(input: ParseStream) -> Result<Self> {
        let name = input.parse()?;
        input.parse::<Token![:]>()?;
        Ok(Self(name, input.parse()?))
    }
}
struct Progress(Expr);
impl Parse for Progress {
    fn parse(input: ParseStream) -> Result<Self> {
        let name: Ident = input.parse()?;
        if name != "value" {
            return Err(syn::Error::new(name.span(), "expected value: state.field"));
        }
        input.parse::<Token![:]>()?;
        let expr = input.parse()?;
        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
        }
        Ok(Self(expr))
    }
}
impl Widget {
    pub(super) fn parse_sections(&mut self, m: &Macro) -> Result<()> {
        match self.platform {
            Platform::Glance | Platform::WidgetKit => {
                let node = self.ui(m)?;
                self.sections.insert("body".into(), node);
            }
            Platform::Ongoing => {
                if !m.path.is_ident("ongoing_notification") {
                    return Err(syn::Error::new_spanned(
                        &m.path,
                        "expected ongoing_notification!",
                    ));
                }
                let sections = m.parse_body_with(
                    syn::punctuated::Punctuated::<Section, Token![,]>::parse_terminated,
                )?;
                for Section(name, ui) in sections {
                    let key = name.to_string();
                    let node = self.ui(&ui)?;
                    if !matches!(
                        (key.as_str(), &node),
                        ("title" | "body", Node::Text(_)) | ("progress", Node::Progress(_))
                    ) {
                        return Err(syn::Error::new(
                            name.span(),
                            "use Text for title/body or ProgressBar for progress",
                        ));
                    }
                    if self.sections.insert(key, node).is_some() {
                        return Err(syn::Error::new(
                            name.span(),
                            "duplicate notification section",
                        ));
                    }
                }
                if !self.sections.contains_key("title") {
                    return Err(syn::Error::new(
                        m.span(),
                        "an ongoing notification needs a title",
                    ));
                }
            }
            Platform::LiveActivity => {
                if !m.path.is_ident("live_activity") {
                    return Err(syn::Error::new_spanned(&m.path, "expected live_activity!"));
                }
                let required = ["compact_leading", "compact_trailing", "expanded"];
                let sections = m.parse_body_with(
                    syn::punctuated::Punctuated::<Section, Token![,]>::parse_terminated,
                )?;
                for Section(name, ui) in sections {
                    let k = name.to_string();
                    if !required.contains(&k.as_str()) || self.sections.contains_key(&k) {
                        return Err(syn::Error::new(
                            name.span(),
                            "unknown or duplicate live activity region",
                        ));
                    }
                    let node = self.ui(&ui)?;
                    self.sections.insert(k, node);
                }
                if required.iter().any(|k| !self.sections.contains_key(*k)) {
                    return Err(syn::Error::new(
                        m.span(),
                        "expected compact_leading, compact_trailing and expanded",
                    ));
                }
            }
        }
        Ok(())
    }
    fn ui(&mut self, m: &Macro) -> Result<Node> {
        if !m.path.is_ident("ui") {
            return Err(syn::Error::new_spanned(
                &m.path,
                "expected a static ui! widget",
            ));
        }
        self.node(m.parse_body()?)
    }
    fn node(&mut self, raw: Raw) -> Result<Node> {
        match raw.kind.to_string().as_str() {
            "Text" => {
                let lit: LitStr = syn::parse2(raw.body)?;
                Ok(Node::Text(self.text(&lit)?))
            }
            "Icon" => {
                let icon: LitStr = syn::parse2(raw.body)?;
                Ok(Node::Icon(icon.value()))
            }
            "ProgressBar" => {
                let Progress(expr) = syn::parse2(raw.body)?;
                Ok(Node::Progress(self.field(&expr, Field::Number)?))
            }
            "Stack" | "Row" | "Column" => {
                let body: Children = syn::parse2(raw.body)?;
                let children = body
                    .children
                    .into_iter()
                    .map(|r| self.node(r))
                    .collect::<Result<_>>()?;
                Ok(Node::Stack {
                    horizontal: raw.kind == "Row",
                    gap: body.gap,
                    children,
                })
            }
            _ => Err(syn::Error::new(
                raw.kind.span(),
                "platform widgets support Text, Icon, ProgressBar, Stack, Row and Column",
            )),
        }
    }
    fn field(&mut self, expr: &Expr, kind: Field) -> Result<String> {
        if let Expr::Field(e) = expr
            && let Expr::Path(base) = &*e.base
            && base.path.is_ident(&self.argument)
            && let syn::Member::Named(name) = &e.member
        {
            let key = name.to_string();
            if key.strip_prefix("r#").is_some() || !key.is_ascii() {
                return Err(syn::Error::new_spanned(
                    name,
                    "platform field names must be ASCII identifiers",
                ));
            }
            if self
                .fields
                .insert(key.clone(), kind)
                .is_some_and(|old| old != kind)
            {
                return Err(syn::Error::new_spanned(
                    name,
                    "widget field used as both text and number",
                ));
            }
            return Ok(key);
        }
        Err(syn::Error::new_spanned(
            expr,
            "expected a direct state field",
        ))
    }
    fn interpolation(&mut self, expr: &Expr) -> Result<Part> {
        if let Expr::Cast(cast) = expr
            && let syn::Type::Path(ty) = &*cast.ty
            && ty.path.is_ident("u32")
            && let Expr::Paren(group) = &*cast.expr
            && let Expr::Binary(binary) = &*group.expr
            && matches!(binary.op, syn::BinOp::Mul(_))
            && let Expr::Lit(lit) = &*binary.right
            && let syn::Lit::Float(f) = &lit.lit
            && f.base10_parse::<f64>()? == 100.0
        {
            return Ok(Part::Percent(self.field(&binary.left, Field::Number)?));
        }
        Ok(Part::Field(self.field(expr, Field::Text)?))
    }
    fn text(&mut self, lit: &LitStr) -> Result<Vec<Part>> {
        let text = lit.value();
        let mut chars = text.chars().peekable();
        let mut literal = String::new();
        let mut parts = Vec::new();
        while let Some(c) = chars.next() {
            match c {
                '{' | '}' if chars.peek() == Some(&c) => {
                    chars.next();
                    literal.push(c);
                }
                '{' => {
                    if !literal.is_empty() {
                        parts.push(Part::Literal(std::mem::take(&mut literal)));
                    }
                    let mut expr = String::new();
                    loop {
                        match chars.next() {
                            Some('}') => break,
                            Some('{') | None => {
                                return Err(syn::Error::new(
                                    lit.span(),
                                    "unclosed widget interpolation",
                                ));
                            }
                            Some(c) => expr.push(c),
                        }
                    }
                    let expr = syn::parse_str::<Expr>(&expr)
                        .map_err(|e| syn::Error::new(lit.span(), e))?;
                    parts.push(
                        self.interpolation(&expr)
                            .map_err(|e| syn::Error::new(lit.span(), e))?,
                    );
                }
                '}' => {
                    return Err(syn::Error::new(
                        lit.span(),
                        "unmatched closing brace in widget text",
                    ));
                }
                c => literal.push(c),
            }
        }
        if !literal.is_empty() || parts.is_empty() {
            parts.push(Part::Literal(literal));
        }
        Ok(parts)
    }
}
