//! The cargo workspace as `cargo metadata` describes it.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::Error;

/// A workspace package.
#[derive(Clone, Debug)]
pub struct Package {
    /// Production export settings from the package manifest.
    pub delivery: serde_json::Value,
    /// The package name.
    pub name: String,
    /// Its directory.
    pub dir: PathBuf,
    /// The library target's crate name (`serge_playground`), if any.
    pub lib_name: Option<String>,
    /// Whether the library is a `cdylib` (needed for the web).
    pub cdylib: bool,
    /// `[package.metadata.serge] web = true`.
    pub web: bool,
    /// Binary target names.
    pub bins: Vec<String>,
    /// Feature names.
    pub features: Vec<String>,
}

/// The workspace.
#[derive(Clone, Debug)]
pub struct Workspace {
    /// The workspace root.
    pub root: PathBuf,
    /// The target directory.
    pub target_dir: PathBuf,
    /// Member packages.
    pub packages: Vec<Package>,
}

impl Workspace {
    /// Reads the workspace containing `dir`.
    pub fn load(dir: &Path) -> Result<Workspace, Error> {
        let out = Command::new("cargo")
            .args(["metadata", "--format-version", "1", "--no-deps"])
            .current_dir(dir)
            .output()
            .map_err(|e| {
                Error::new(format!("could not run cargo: {e}"))
                    .hint("install Rust with rustup: https://rustup.rs")
            })?;
        if !out.status.success() {
            return Err(Error::new(format!(
                "`cargo metadata` failed:\n{}",
                String::from_utf8_lossy(&out.stderr)
            )));
        }
        let v: Value = serde_json::from_slice(&out.stdout)
            .map_err(|e| Error::new(format!("unreadable `cargo metadata` output: {e}")))?;
        let s = |v: &Value| v.as_str().unwrap_or_default().to_owned();
        let packages = v["packages"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| {
                let targets = p["targets"].as_array().cloned().unwrap_or_default();
                let kinds = |t: &Value| -> Vec<String> {
                    t["kind"].as_array().into_iter().flatten().map(s).collect()
                };
                let lib = targets.iter().find(|t| {
                    kinds(t)
                        .iter()
                        .any(|k| k == "lib" || k == "cdylib" || k == "rlib")
                });
                Package {
                    delivery: p["metadata"]["serge"].clone(),
                    name: s(&p["name"]),
                    dir: PathBuf::from(s(&p["manifest_path"]))
                        .parent()
                        .map(Path::to_path_buf)
                        .unwrap_or_default(),
                    lib_name: lib.map(|t| s(&t["name"]).replace('-', "_")),
                    cdylib: lib.is_some_and(|t| kinds(t).iter().any(|k| k == "cdylib")),
                    web: p["metadata"]["serge"]["web"].as_bool().unwrap_or(false),
                    bins: targets
                        .iter()
                        .filter(|t| kinds(t).iter().any(|k| k == "bin"))
                        .map(|t| s(&t["name"]))
                        .collect(),
                    features: p["features"]
                        .as_object()
                        .map(|m| m.keys().cloned().collect())
                        .unwrap_or_default(),
                }
            })
            .collect();
        Ok(Workspace {
            root: PathBuf::from(s(&v["workspace_root"])),
            target_dir: PathBuf::from(s(&v["target_directory"])),
            packages,
        })
    }

    /// The web app to build: `name` if given, else the only package with
    /// `[package.metadata.serge] web = true`.
    pub fn web_app(&self, name: Option<&str>) -> Result<&Package, Error> {
        if let Some(n) = name {
            return self
                .packages
                .iter()
                .find(|p| p.name == n)
                .ok_or_else(|| Error::new(format!("no package `{n}` in this workspace")));
        }
        let apps: Vec<_> = self.packages.iter().filter(|p| p.web).collect();
        match apps.as_slice() {
            [one] => Ok(one),
            [] => Err(Error::new("no web app in this workspace").hint(
                "add `[package.metadata.serge] web = true` to the app's Cargo.toml, or pass `-p <package>`",
            )),
            many => Err(Error::new(format!(
                "several web apps: {}",
                many.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ")
            ))
            .hint("choose one with `-p <package>`")),
        }
    }

    /// The `wasm-bindgen` version in `Cargo.lock`.
    pub fn locked_wasm_bindgen(&self) -> Option<String> {
        let lock = std::fs::read_to_string(self.root.join("Cargo.lock")).ok()?;
        let mut lines = lock.lines();
        while let Some(l) = lines.next() {
            if l.trim() == "name = \"wasm-bindgen\"" {
                let v = lines.next()?.trim();
                return v
                    .strip_prefix("version = \"")
                    .and_then(|v| v.strip_suffix('"'))
                    .map(str::to_owned);
            }
        }
        None
    }

    /// `api_proxy` of the `[serge]` table in the workspace's `hyve.toml`
    /// (`api_proxy = "http://127.0.0.1:3000/v1"`).
    pub fn configured_api_proxy(&self) -> Option<String> {
        let text = std::fs::read_to_string(self.root.join("hyve.toml")).ok()?;
        let mut in_serge = false;
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.starts_with('[') {
                in_serge = line == "[serge]";
            } else if in_serge
                && let Some((k, v)) = line.split_once('=')
                && k.trim() == "api_proxy"
            {
                return Some(v.trim().trim_matches('"').to_owned());
            }
        }
        None
    }

    /// Source directories to watch: every member's `src`, plus the app's
    /// `web/` assets.
    pub fn watch_dirs(&self) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = self
            .packages
            .iter()
            .flat_map(|p| [p.dir.join("src"), p.dir.join("web")])
            .filter(|d| d.is_dir())
            .collect();
        dirs.sort();
        dirs.dedup();
        dirs
    }
}
