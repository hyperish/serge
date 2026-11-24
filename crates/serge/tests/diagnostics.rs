//! `ui!` and `#[component]` misuse ends in a spanned compile error, in
//! finite time (`docs/05-implementation-plan.md` §3, 문법).
#![cfg(not(target_arch = "wasm32"))]
#![allow(missing_docs)]

#[test]
fn misuse_is_a_compile_error() {
    trybuild::TestCases::new().compile_fail("tests/ui/*.rs");
}
