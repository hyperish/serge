use super::*;
use std::{collections::BTreeSet, os::unix::fs::MetadataExt};

/// Cargo path sources are mutable even when their package versions do not
/// change. Registry and git sources are pinned by Cargo.lock.
pub(super) struct Inputs {
    roots: Vec<PathBuf>,
    fixed: Vec<PathBuf>,
    pub tip: PathBuf,
    target: PathBuf,
}
pub(super) struct Snapshot {
    pub exact: [u8; 32],
    pub shape: Option<[u8; 32]>,
}
impl Inputs {
    pub fn resolve(ws: &Workspace, package: &str, bin: &str) -> Result<Self, Fail> {
        let response = Command::new("cargo")
            .current_dir(&ws.root)
            .args(["metadata", "--format-version", "1"])
            .output()
            .map_err(dev_error)?;
        if !response.status.success() {
            return Err(dev_error("could not resolve native build inputs"));
        }
        let meta: Value = serde_json::from_slice(&response.stdout).map_err(dev_error)?;
        let pkgs = meta["packages"]
            .as_array()
            .ok_or_else(|| dev_error("missing Cargo packages"))?;
        let mut roots = Vec::new();
        let mut fixed = BTreeSet::new();
        let mut tip = None;
        for pkg in pkgs.iter().filter(|p| p["source"].is_null()) {
            let path = Path::new(
                pkg["manifest_path"]
                    .as_str()
                    .ok_or_else(|| dev_error("invalid Cargo manifest"))?,
            );
            let dir = path.parent().unwrap();
            roots.push(dir.to_owned());
            // Include ancestor workspace dependency declarations and Cargo configuration.
            for a in dir.ancestors() {
                for name in [
                    "Cargo.toml",
                    "Cargo.lock",
                    ".cargo/config",
                    ".cargo/config.toml",
                    "rust-toolchain",
                    "rust-toolchain.toml",
                ] {
                    fixed.insert(a.join(name));
                }
            }
            if pkg["name"] == package {
                for t in pkg["targets"].as_array().into_iter().flatten() {
                    if t["name"] == bin && t["kind"][0] == "bin" {
                        tip = t["src_path"].as_str().map(PathBuf::from);
                    }
                }
            }
        }
        fixed.insert(ws.root.join("hyve.toml"));
        if let Some(home) = std::env::var_os("CARGO_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".cargo")))
        {
            for name in ["config", "config.toml"] {
                fixed.insert(home.join(name));
            }
        }
        roots.sort();
        roots.dedup();
        let mut top = Vec::<PathBuf>::new();
        for root in roots {
            if !top.iter().any(|a| root.starts_with(a)) {
                top.push(root);
            }
        }
        Ok(Self {
            roots: top,
            fixed: fixed.into_iter().collect(),
            tip: tip.ok_or_else(|| dev_error("cannot identify native binary source"))?,
            target: ws.target_dir.clone(),
        })
    }
    fn files(&self) -> io::Result<BTreeSet<PathBuf>> {
        let mut files = BTreeSet::new();
        let mut todo = self.roots.clone();
        let mut visited = BTreeSet::new();
        while let Some(dir) = todo.pop() {
            let dir = fs::canonicalize(dir)?;
            if dir == self.target || !visited.insert(dir.clone()) {
                continue;
            }

            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let p = entry.path();
                let ty = entry.file_type()?;
                if ty.is_dir() {
                    if p != self.target
                        && !matches!(entry.file_name().to_str(), Some(".git" | "target" | ".dx"))
                    {
                        todo.push(p);
                    }
                } else if ty.is_symlink() {
                    if p.is_dir() {
                        todo.push(p.clone());
                    }
                    files.insert(p);
                } else if ty.is_file() {
                    files.insert(p);
                }
            }
        }
        files.extend(self.fixed.iter().filter(|p| p.is_file()).cloned());
        Ok(files)
    }
    pub fn stamp(&self) -> io::Result<[u8; 32]> {
        let mut hash = Sha256::new();
        for file in self.files()? {
            let m = file.symlink_metadata()?;
            if m.is_symlink() && file.is_file() {
                let target = file.metadata()?;
                for n in [
                    target.ino(),
                    target.len(),
                    target.mtime() as u64,
                    target.mtime_nsec() as u64,
                    target.ctime_nsec() as u64,
                ] {
                    hash.update(n.to_le_bytes());
                }
            }
            hash.update(file.as_os_str().as_encoded_bytes());
            hash.update([0]);
            for n in [
                m.ino(),
                m.len(),
                m.mtime() as u64,
                m.mtime_nsec() as u64,
                m.ctime_nsec() as u64,
            ] {
                hash.update(n.to_le_bytes());
            }
        }
        Ok(hash.finalize().into())
    }
    pub fn snapshot(&self, ws: &Workspace, features: &[String]) -> Result<Snapshot, Fail> {
        let mut raw = Sha256::new();
        let mut shape = Sha256::new();
        let mut checked = false;
        let mut symlink = false;
        let compiler = Command::new("rustc")
            .arg("-vV")
            .current_dir(&ws.root)
            .output()
            .map_err(dev_error)?;
        if !compiler.status.success() {
            return Err(dev_error("could not identify the Rust compiler"));
        }
        for h in [&mut raw, &mut shape] {
            h.update(&compiler.stdout);
            h.update(b"aarch64-apple-darwin:dx-0.7.10:desktop-dev");
            for f in features {
                h.update([0]);
                h.update(f.as_bytes());
            }
        }
        for file in self.files().map_err(dev_error)? {
            let meta = file.symlink_metadata().map_err(dev_error)?;
            if meta.is_symlink() {
                symlink = true;
                raw.update(file.as_os_str().as_encoded_bytes());
                raw.update([0]);
                raw.update(
                    fs::read_link(&file)
                        .map_err(dev_error)?
                        .as_os_str()
                        .as_encoded_bytes(),
                );
                if file.is_file() {
                    raw.update(Sha256::digest(fs::read(&file).map_err(dev_error)?));
                }
                continue;
            }
            let data = fs::read(&file).map_err(dev_error)?;
            let exact = Sha256::digest(&data);
            let normalized = if file == self.tip {
                std::str::from_utf8(&data)
                    .ok()
                    .and_then(serge_widget_codegen::render_value::identity)
                    .map(|s| {
                        checked = true;
                        Sha256::digest(s.as_bytes())
                    })
            } else {
                None
            };
            for (h, digest) in [(&mut raw, exact), (&mut shape, normalized.unwrap_or(exact))] {
                h.update(file.as_os_str().as_encoded_bytes());
                h.update([0]);
                h.update(digest);
            }
        }
        Ok(Snapshot {
            exact: raw.finalize().into(),
            shape: (checked && !symlink).then(|| shape.finalize().into()),
        })
    }
}
