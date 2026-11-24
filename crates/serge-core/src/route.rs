//! How component arguments that are not properties get their values
//! (`docs/01-dx-and-dsl.md` §1, 컴포넌트 인자 규칙): `Ctx<T>` from the
//! enclosing provides, `Path<T>` / `Query<T>` from the route that mounted
//! the screen.

use hyve::api::params::{from_params, parse_query};
use hyve::api::{Path, Query};
use hyve::serde::Json;
use hyve::validate::Schema;
use hyve::{Ctx, HyveError, HyveResult, Scope};

use crate::element::Element;

/// The parameters of the route that mounted the current screen (provided
/// by the router in the screen's scope).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RouteParams {
    /// Placeholder values by name (`/projects/{id}` → `("id", "7")`).
    pub path: Vec<(String, String)>,
    /// The raw query string (without `?`).
    pub query: String,
}

/// A component argument filled from the view rather than by the parent.
pub trait FromView: Sized {
    /// The value for the current view.
    fn from_view() -> HyveResult<Self>;
}

fn current<T: Send + Sync + 'static>() -> HyveResult<Ctx<T>> {
    let scope = Scope::current()
        .ok_or_else(|| HyveError::internal("components run inside a view scope"))?;
    scope.get::<T>().ok_or_else(|| {
        HyveError::internal(format!(
            "no `{}` is provided here; provide it with `.provide(..)` on the app or an enclosing feature",
            std::any::type_name::<T>()
        ))
    })
}

impl<T: Send + Sync + 'static> FromView for Ctx<T> {
    fn from_view() -> HyveResult<Self> {
        current::<T>()
    }
}

impl<T: Json + Schema> FromView for Path<T> {
    fn from_view() -> HyveResult<Self> {
        let p = current::<RouteParams>()?;
        from_params::<T>(&p.path, None, "path").map(Path)
    }
}

impl<T: Json + Schema> FromView for Query<T> {
    fn from_view() -> HyveResult<Self> {
        let p = current::<RouteParams>()?;
        let pairs = parse_query(&p.query)?;
        from_params::<T>(&pairs, None, "query").map(Query)
    }
}

/// A component a route can mount: a function taking only view-filled
/// arguments (`Ctx<T>`, `Path<T>`, `Query<T>`).
pub trait RouteComponent<M>: 'static {
    /// Builds the screen (an error when a parameter does not parse).
    fn render(&self) -> HyveResult<Element>;
}

impl<F: Fn() -> Element + 'static> RouteComponent<()> for F {
    fn render(&self) -> HyveResult<Element> {
        Ok(self())
    }
}

macro_rules! route_component {
    ($($a:ident),*) => {
        impl<F, $($a: FromView),*> RouteComponent<($($a,)*)> for F
        where
            F: Fn($($a),*) -> Element + 'static,
        {
            #[allow(non_snake_case)]
            fn render(&self) -> HyveResult<Element> {
                $(let $a = $a::from_view()?;)*
                Ok(self($($a),*))
            }
        }
    };
}

route_component!(A);
route_component!(A, B);
route_component!(A, B, C);
route_component!(A, B, C, D);
