//! `hyve-serge`: the stage protocol, usage errors, and a real web build of
//! the playground.
#![allow(missing_docs)]

use std::process::Command;

fn hyve_serge(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_hyve-serge"))
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap()
}

#[test]
fn stages_answer_hyve() {
    let out = hyve_serge(&["stages"]);
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<_> = text.lines().collect();
    assert_eq!(lines, [env!("CARGO_PKG_VERSION"), "build", "preview"]);
}

#[test]
fn usage_errors_exit_2() {
    for args in [
        &[][..],
        &["deploy"],
        &["build", "--port", "1"],
        &["preview", "--web", "--native"],
    ] {
        let out = hyve_serge(args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("usage:"),
            "{args:?}"
        );
    }
    let out = hyve_serge(&["build", "-p", "no-such-package"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("no package `no-such-package`"));
}

#[test]
fn builds_the_playground_for_the_browser() {
    let out_dir = std::env::temp_dir().join(format!("serge-cli-bundle-{}", std::process::id()));
    let out = hyve_serge(&[
        "build",
        "--debug",
        "-p",
        "serge-playground",
        "--out",
        out_dir.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    for f in ["index.html", "app.js", "app_bg.wasm"] {
        assert!(out_dir.join(f).is_file(), "{f} missing");
    }
    let wasm = std::fs::read(out_dir.join("app_bg.wasm")).unwrap();
    assert_eq!(&wasm[..4], b"\0asm");
    let html = std::fs::read_to_string(out_dir.join("index.html")).unwrap();
    assert!(html.contains("serge-root") && html.contains("/app.js"));
    // A client build: no server bodies or database code in the module.
    let text = String::from_utf8_lossy(&wasm);
    assert!(
        !text.contains("ProjectStore"),
        "the server-only store leaked into the web build"
    );
    let _ = std::fs::remove_dir_all(out_dir);
}
