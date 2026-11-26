//! Navigation hook: `Link` and `navigate()` ask the router installed for
//! this UI thread to open a path.

use std::cell::RefCell;
use std::rc::Rc;

type Navigator = Rc<dyn Fn(&str)>;

thread_local! {
    static NAVIGATOR: RefCell<Option<Navigator>> = const { RefCell::new(None) };
}

/// Installs the router's navigation function (the router does this when the
/// app starts).
pub fn set_navigator(f: Rc<dyn Fn(&str)>) {
    NAVIGATOR.with(|n| *n.borrow_mut() = Some(f));
}

/// Opens `path` (`/projects/7?tab=a`) in the app's router. Without a router
/// (no routes) it does nothing.
pub fn navigate(path: &str) {
    let f = NAVIGATOR.with(|n| n.borrow().clone());
    if let Some(f) = f {
        f(path);
    }
}
