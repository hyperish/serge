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

/// Under `hyve preview` (`HYVE_STAGE_LIFELINE=stdin`) the preview stops when
/// its stdin closes, and its port is free again.
#[test]
fn preview_stops_when_hyve_closes_its_stdin() {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;
    use std::time::{Duration, Instant};
    let port = 18_000 + (std::process::id() % 1_000) as u16 * 20;
    let mut child = Command::new(env!("CARGO_BIN_EXE_hyve-serge"))
        .args(["preview", "--web", "--no-watch", "-p", "serge-playground"])
        .args(["--port", &port.to_string()])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("HYVE_STAGE_LIFELINE", "stdin")
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Read stderr until the tool exits (a closed pipe would fail its
    // later prints); the served address comes over a channel.
    let err = BufReader::new(child.stderr.take().unwrap());
    let (tx, rx) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut said = String::new();
        for line in err.lines().map_while(Result::ok) {
            if let Some(rest) = line.split(" at http://").nth(1) {
                let _ = tx.send(rest.trim_end_matches('/').to_owned());
            }
            said.push_str(&line);
            said.push('\n');
        }
        said
    });
    let addr = match rx.recv_timeout(Duration::from_secs(600)) {
        Ok(addr) => addr,
        Err(_) => {
            let _ = child.kill();
            panic!("never served:\n{}", reader.join().unwrap());
        }
    };
    assert!(std::net::TcpStream::connect(&addr).is_ok());

    drop(child.stdin.take());
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        assert!(Instant::now() < deadline, "the preview did not stop");
        std::thread::sleep(Duration::from_millis(50));
    };
    let said = reader.join().unwrap();
    assert!(status.success(), "{status}\n{said}");
    assert!(
        std::net::TcpStream::connect(&addr).is_err(),
        "{addr} still served"
    );
}

/// `preview` is offered only where there is an app to preview.
#[test]
fn preview_is_offered_only_with_an_app() {
    let dir = std::env::temp_dir().join(format!("serge-cli-noapp-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"lib-only\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/lib.rs"), "").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_hyve-serge"))
        .arg("stages")
        .current_dir(&dir)
        .env("CARGO_NET_OFFLINE", "true")
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        text.lines().collect::<Vec<_>>(),
        [env!("CARGO_PKG_VERSION"), "build"]
    );
    let _ = std::fs::remove_dir_all(dir);
}
