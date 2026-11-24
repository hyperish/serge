//! The dependency boundary (`docs/02-architecture.md` §2): the web build
//! links no native stack, the native build no browser bindings, and a
//! client build of an app links no server runtime.
#![cfg(not(target_arch = "wasm32"))]
#![allow(missing_docs)]

use std::collections::BTreeSet;
use std::process::Command;

fn tree(package: &str, target: Option<&str>) -> BTreeSet<String> {
    let mut cmd = Command::new(env!("CARGO"));
    cmd.args([
        "tree", "-q", "-p", package, "-e", "normal", "--prefix", "none",
    ])
    .current_dir(env!("CARGO_MANIFEST_DIR"));
    if let Some(t) = target {
        cmd.args(["--target", t]);
    }
    let out = cmd.output().expect("cargo tree");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.split_whitespace().next().map(str::to_owned))
        .collect()
}

fn none_of(set: &BTreeSet<String>, banned: &[&str], what: &str) {
    let found: Vec<_> = banned.iter().filter(|b| set.contains(**b)).collect();
    assert!(found.is_empty(), "{what} links {found:?}");
}

#[test]
fn targets_link_only_their_stack() {
    let web = tree("serge", Some("wasm32-unknown-unknown"));
    assert!(web.contains("serge-web-dom"));
    none_of(
        &web,
        &[
            "taffy",
            "parley",
            "vello_cpu",
            "winit",
            "softbuffer",
            "wgpu",
            "tokio",
            "hyper",
            "serge-native",
            "serge-platform-desktop",
        ],
        "the web build",
    );
    let native = tree("serge", None);
    assert!(native.contains("serge-native") && native.contains("winit"));
    none_of(
        &native,
        &["serge-web-dom", "web-sys", "wasm-bindgen", "js-sys"],
        "the native build",
    );
    let client = tree("serge-playground", Some("wasm32-unknown-unknown"));
    none_of(
        &client,
        &["orbic", "orbic-core", "orbic-db", "tokio"],
        "the playground's web build",
    );
}
