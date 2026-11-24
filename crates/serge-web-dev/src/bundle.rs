//! Building a web bundle: `cargo build --target wasm32-unknown-unknown`,
//! then `wasm-bindgen --target web`, then `index.html` and the app's
//! `web/` assets.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{Error, Package, Workspace};

/// How to build.
#[derive(Clone, Debug, Default)]
pub struct BuildOptions {
    /// `--release`.
    pub release: bool,
    /// Cargo features to enable.
    pub features: Vec<String>,
    /// Where to write the bundle (default `target/serge/<package>`).
    pub out_dir: Option<PathBuf>,
}

/// A built bundle.
#[derive(Clone, Debug)]
pub struct Bundle {
    /// The directory to serve.
    pub dir: PathBuf,
}

const INDEX: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
</head>
<body>
<div id="serge-root"></div>
<script type="module">
import init from "/app.js";
init({ module_or_path: "/app_bg.wasm" });
</script>
</body>
</html>
"#;

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let target = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&e.path(), &target)?;
        } else {
            std::fs::copy(e.path(), target)?;
        }
    }
    Ok(())
}

/// Builds `package` for the browser.
pub fn build_web(ws: &Workspace, package: &Package, opts: &BuildOptions) -> Result<Bundle, Error> {
    let lib = package.lib_name.as_deref().filter(|_| package.cdylib).ok_or_else(|| {
        Error::new(format!("`{}` has no `cdylib` library to run in the browser", package.name))
            .hint("add `[lib] crate-type = [\"cdylib\", \"rlib\"]` and a `#[wasm_bindgen(start)]` function")
    })?;

    // 1. The wasm module.
    let mut cargo = Command::new("cargo");
    cargo
        .args([
            "build",
            "--lib",
            "--target",
            "wasm32-unknown-unknown",
            "-p",
            &package.name,
        ])
        .current_dir(&ws.root);
    if opts.release {
        cargo.arg("--release");
    }
    if !opts.features.is_empty() {
        cargo.arg("--features").arg(opts.features.join(","));
    }
    let status = cargo
        .status()
        .map_err(|e| Error::new(format!("could not run cargo: {e}")))?;
    if !status.success() {
        return Err(
            Error::new(format!("building `{}` for the web failed", package.name))
                .hint("is the target installed? `rustup target add wasm32-unknown-unknown`"),
        );
    }
    let wasm = ws
        .target_dir
        .join("wasm32-unknown-unknown")
        .join(if opts.release { "release" } else { "debug" })
        .join(format!("{lib}.wasm"));

    // 2. JS bindings, with the exact wasm-bindgen of the lockfile.
    let want = ws.locked_wasm_bindgen();
    let install = format!(
        "cargo install wasm-bindgen-cli{}",
        want.as_deref()
            .map(|v| format!(" --version ={v}"))
            .unwrap_or_default()
    );
    let have = Command::new("wasm-bindgen")
        .arg("--version")
        .output()
        .map_err(|_| Error::new("`wasm-bindgen` is not installed").hint(install.clone()))?;
    let have = String::from_utf8_lossy(&have.stdout);
    if let Some(w) = &want
        && !have.split_whitespace().any(|t| t == w)
    {
        return Err(Error::new(format!(
            "this workspace uses wasm-bindgen {w}, but `wasm-bindgen --version` says {}",
            have.trim()
        ))
        .hint(install));
    }
    let dir = opts
        .out_dir
        .clone()
        .unwrap_or_else(|| ws.target_dir.join("serge").join(&package.name));
    std::fs::create_dir_all(&dir).map_err(|e| Error::new(format!("{}: {e}", dir.display())))?;
    let status = Command::new("wasm-bindgen")
        .args([
            "--target",
            "web",
            "--no-typescript",
            "--out-name",
            "app",
            "--out-dir",
        ])
        .arg(&dir)
        .arg(&wasm)
        .status()
        .map_err(|e| Error::new(format!("could not run wasm-bindgen: {e}")))?;
    if !status.success() {
        return Err(Error::new("wasm-bindgen failed"));
    }

    // 3. The page and assets: the app's `web/` folder wins.
    let web = package.dir.join("web");
    if web.is_dir() {
        copy_dir(&web, &dir).map_err(|e| Error::new(format!("copying {}: {e}", web.display())))?;
    }
    if !web.join("index.html").is_file() {
        std::fs::write(
            dir.join("index.html"),
            INDEX.replace("{title}", &package.name),
        )
        .map_err(|e| Error::new(format!("writing index.html: {e}")))?;
    }
    Ok(Bundle { dir })
}
