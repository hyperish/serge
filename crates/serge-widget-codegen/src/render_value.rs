//! Fixed-signature leaf calculations accepted by the native development tool.
//! Both the proc macro and the change classifier use this grammar.
use syn::{BinOp, Expr, Item, ItemFn, Lit, Result, ReturnType, Stmt, Type, UnOp, spanned::Spanned};

/// Verifies the signature and body, rejecting calls, attributes, captured
/// variables, allocations and target-dependent arithmetic.
pub fn check(f: &ItemFn) -> Result<()> {
    let sig = &f.sig;
    let allowed = sig.constness.is_none()
        && sig.asyncness.is_none()
        && sig.unsafety.is_none()
        && sig.abi.is_none()
        && sig.variadic.is_none()
        && sig.inputs.is_empty()
        && sig.generics.params.is_empty()
        && sig.generics.where_clause.is_none()
        && matches!(&sig.output, ReturnType::Type(_,ty) if matches!(ty.as_ref(),Type::Path(p) if p.qself.is_none() && p.path.is_ident("u32")));
    if !allowed {
        return Err(syn::Error::new(
            sig.span(),
            "render_value requires fn name() -> u32",
        ));
    }
    let [Stmt::Expr(expr, None)] = &f.block.stmts[..] else {
        return Err(syn::Error::new(
            f.block.span(),
            "use one u32 expression of literals and operators",
        ));
    };
    fn pure(expr: &Expr) -> bool {
        match expr {
            Expr::Lit(e) => {
                e.attrs.is_empty()
                    && matches!(&e.lit,Lit::Int(i) if matches!(i.suffix(), ""|"u32") && i.base10_parse::<u32>().is_ok())
            }
            Expr::Paren(e) => e.attrs.is_empty() && pure(&e.expr),
            Expr::Unary(e) => e.attrs.is_empty() && matches!(e.op, UnOp::Not(_)) && pure(&e.expr),
            Expr::Binary(e) => {
                e.attrs.is_empty()
                    && matches!(
                        e.op,
                        BinOp::Add(_)
                            | BinOp::Sub(_)
                            | BinOp::Mul(_)
                            | BinOp::BitAnd(_)
                            | BinOp::BitOr(_)
                            | BinOp::BitXor(_)
                            | BinOp::Shl(_)
                            | BinOp::Shr(_)
                    )
                    && pure(&e.left)
                    && pure(&e.right)
            }
            _ => false,
        }
    }
    if !pure(expr) {
        return Err(syn::Error::new(
            expr.span(),
            "render_value accepts only u32 literals and arithmetic or bitwise operators",
        ));
    }
    Ok(())
}

/// Replaces only verified function bodies with a fixed marker. All other
/// source bytes, including signatures and attributes, remain in the identity.
/// Only top-level, absolutely qualified attributes are eligible.
pub fn identity(source: &str) -> Option<String> {
    let file = syn::parse_file(source).ok()?;
    let mut bytes = source.to_owned();
    let mut ranges = Vec::new();
    for item in &file.items {
        let Item::Fn(f) = item else { continue };
        let [attr] = f.attrs.as_slice() else { continue };
        let p = attr.path();
        if p.leading_colon.is_none()
            || p.segments.len() != 2
            || p.segments[0].ident != "serge"
            || p.segments[1].ident != "render_value"
        {
            continue;
        }
        check(f).ok()?;
        ranges.push(f.block.brace_token.span.join().byte_range());
    }
    if ranges.is_empty() {
        return None;
    }
    for range in ranges.into_iter().rev() {
        if !source.get(range.clone())?.starts_with('{') {
            return None;
        }
        bytes.replace_range(range, "{<serge-render-u32>}");
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_literal_render_bodies_share_an_identity() {
        let s = "#[::serge::render_value]\nfn size() -> u32 { 2+3 }\nfn main() {}";
        assert_eq!(identity(s), identity(&s.replace("2+3", "17 * 9")));
        assert!(identity(s).is_some());
        assert_ne!(
            identity(s),
            identity(&s.replace("fn main()", "async fn main()"))
        );
        for expr in ["SOME_STATE", "vec![1].len()", "size()", "{ unsafe { 5 } }"] {
            assert!(identity(&s.replace("2+3", expr)).is_none());
        }
    }
}
