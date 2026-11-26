//! Builds in an isolated staging directory. Content-addressed files and pages
//! for a unique build are installed before the manifest is atomically replaced.
use crate::{BuildOptions, Bundle, Error, Package, Workspace};
use serge_web_ssr::{
    Pattern, SergeAsset, SergeBundleManifest, SergeRoute, VerifiedBundle, digest, media_type,
};
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command,
};

fn failed(e: impl std::fmt::Display) -> Error {
    Error::new(format!("building production bundle: {e}"))
}

fn files(dir: &Path, relative: &Path, out: &mut BTreeMap<String, Vec<u8>>) -> io::Result<()> {
    for entry in fs::read_dir(dir.join(relative))? {
        let entry = entry?;
        let path = relative.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            files(dir, &path, out)?;
        } else if kind.is_file() {
            let name = path
                .to_str()
                .ok_or_else(|| io::Error::other("non-UTF-8 asset name"))?
                .replace('\\', "/");
            out.insert(name, fs::read(entry.path())?);
        } else {
            return Err(io::Error::other(
                "bundle entries must be files or directories",
            ));
        }
    }
    Ok(())
}

fn relative_path(from: &str, to: &str) -> String {
    let from = from.rsplit_once('/').map_or("", |p| p.0);
    let dirs: Vec<_> = from.split('/').filter(|p| !p.is_empty()).collect();
    let dest: Vec<_> = to.split('/').collect();
    let common = dirs.iter().zip(&dest).take_while(|(a, b)| a == b).count();
    let mut parts = vec![".."; dirs.len() - common];
    parts.extend(&dest[common..]);
    parts.join("/")
}

// Resolve exact URL literals in HTML, ECMAScript imports/URL constructors,
// and CSS url(). Untouched external URLs keep their original origin.
fn references(source: &str, from: &str, to: &str) -> Vec<(String, String, String)> {
    let relative = relative_path(from, to);
    let mut found = Vec::new();
    for path in [format!("/{to}"), format!("./{relative}"), relative] {
        for (left, right) in [("\"", "\""), ("'", "'"), ("url(", ")")] {
            let needle = format!("{left}{path}{right}");
            if source.contains(&needle) {
                found.push((needle, left.into(), right.into()));
            }
        }
    }
    found
}

struct Assets {
    raw: BTreeMap<String, Vec<u8>>,
    output: BTreeMap<String, SergeAsset>,
    active: HashSet<String>,
    stage: PathBuf,
}
impl Assets {
    fn rewrite(&mut self, source: &str, name: &str) -> Result<String, Error> {
        let mut out = source.to_owned();
        for dep in self.raw.keys().cloned().collect::<Vec<_>>() {
            let refs = references(source, name, &dep);
            if !refs.is_empty() {
                let url = self.write(&dep)?;
                for (needle, left, right) in refs {
                    out = out.replace(&needle, &format!("{left}{url}{right}"));
                }
            }
        }
        Ok(out)
    }

    fn write(&mut self, name: &str) -> Result<String, Error> {
        if let Some(asset) = self.output.get(name) {
            return Ok(asset.url.clone());
        }
        if !self.active.insert(name.to_owned()) {
            return Err(failed(format!(
                "cyclic asset URL imports at {name}; bundle the module cycle into a single entry"
            )));
        }
        let filename = name.rsplit('/').next().unwrap_or(name);
        let ct = media_type(name);
        let raw = self.raw.get(name).expect("enumerated asset");
        let bytes = if matches!(
            ct,
            "text/javascript" | "text/css" | "application/manifest+json"
        ) {
            let text = std::str::from_utf8(raw).map_err(failed)?.to_owned();
            self.rewrite(&text, name)?.into_bytes()
        } else {
            raw.clone()
        };
        let sha = digest(&bytes);
        let file = format!("assets/{sha}/{filename}");
        let path = self.stage.join(&file);
        fs::create_dir_all(path.parent().unwrap()).map_err(failed)?;
        fs::write(path, &bytes).map_err(failed)?;
        let url = format!("/{file}");
        self.output.insert(
            name.to_owned(),
            SergeAsset {
                url: url.clone(),
                file,
                media_type: ct.into(),
                bytes: bytes.len() as u64,
                sha256: sha,
            },
        );
        self.active.remove(name);
        Ok(url)
    }
}

fn strings(package: &Package, key: &str) -> Result<Vec<String>, Error> {
    let Some(value) = package.delivery.get(key) else {
        return Ok(Vec::new());
    };
    value
        .as_array()
        .ok_or_else(|| failed(format!("{key} must be an array of paths")))?
        .iter()
        .map(|s| {
            s.as_str()
                .map(str::to_owned)
                .ok_or_else(|| failed(format!("invalid {key} path")))
        })
        .collect()
}

fn export(
    binary: &Path,
    package: &Package,
    path: Option<&str>,
    work: &Path,
) -> Result<String, Error> {
    let file = tempfile::NamedTempFile::new_in(work).map_err(failed)?;
    let mut cmd = Command::new(binary);
    cmd.current_dir(&package.dir)
        .env("SERGE_EXPORT_OUTPUT", file.path())
        .env_remove("SERGE_EXPORT_PATH");
    if let Some(p) = path {
        cmd.env("SERGE_EXPORT_PATH", p);
    }
    if !cmd.status().map_err(failed)?.success() {
        return Err(failed("app page exporter failed"));
    }
    let content = fs::read_to_string(file.path()).map_err(failed)?;
    if content.is_empty() && path.is_none() {
        return Err(failed("app did not export its routes"));
    }
    Ok(content)
}

fn routes(
    ws: &Workspace,
    package: &Package,
    opts: &BuildOptions,
    stage: &Path,
    shell: &str,
    id: &str,
) -> Result<Vec<SergeRoute>, Error> {
    let ssg = strings(package, "ssg")?;
    let ssr = strings(package, "ssr")?;
    let shell_file = format!("pages/{id}/shell.html");
    fs::create_dir_all(stage.join("pages").join(id)).map_err(failed)?;
    fs::write(stage.join(&shell_file), shell).map_err(failed)?;
    let Some(bin) = package.delivery.get("export") else {
        if !ssg.is_empty() || !ssr.is_empty() {
            return Err(failed("ssg/ssr routes require the app's export binary"));
        }
        return Ok(vec![SergeRoute::Csr {
            pattern: "/".into(),
            file: shell_file,
        }]);
    };
    let bin = bin
        .as_str()
        .filter(|b| package.bins.iter().any(|n| n == b))
        .ok_or_else(|| failed("metadata.serge.export must name an app binary"))?;
    let mut cmd = Command::new("cargo");
    cmd.current_dir(&ws.root)
        .args(["build", "-p", &package.name, "--bin", bin]);
    if opts.release {
        cmd.arg("--release");
    }
    if !opts.features.is_empty() {
        cmd.arg("--features").arg(opts.features.join(","));
    }
    if !cmd.status().map_err(failed)?.success() {
        return Err(failed("could not build page exporter"));
    }
    let binary = ws
        .target_dir
        .join(if opts.release { "release" } else { "debug" })
        .join(format!("{bin}{}", std::env::consts::EXE_SUFFIX));
    let declared: Vec<(String, bool)> =
        serde_json::from_str(&export(&binary, package, None, stage)?).map_err(failed)?;
    let mut out = Vec::new();
    let mut matched = HashSet::new();
    for (pattern, protected) in &declared {
        let matcher = Pattern::parse(pattern).map_err(failed)?;
        let server_rendered = ssr.contains(pattern);
        let mut concrete = false;
        for path in ssg.iter().filter(|p| matcher.matches(p).is_some()) {
            if !matched.insert(path) {
                continue;
            }
            if *protected || server_rendered {
                return Err(failed(format!(
                    "cannot generate protected or SSR route {path} as static HTML"
                )));
            }
            let html = export(&binary, package, Some(path), stage)?;
            let file = format!("pages/{id}/{}.html", out.len());
            let marker = "<div id=\"serge-root\"></div>";
            if !shell.contains(marker) {
                return Err(failed(
                    "the static page shell needs an empty serge-root div",
                ));
            }
            fs::write(
                stage.join(&file),
                shell.replacen(
                    marker,
                    &format!("<div id=\"serge-root\" data-serge-prerendered>{html}</div>"),
                    1,
                ),
            )
            .map_err(failed)?;
            out.push(SergeRoute::Ssg {
                pattern: path.clone(),
                file,
            });
            concrete |= path == pattern;
        }
        if !concrete {
            out.push(if server_rendered {
                SergeRoute::Ssr {
                    pattern: pattern.clone(),
                }
            } else {
                SergeRoute::Csr {
                    pattern: pattern.clone(),
                    file: shell_file.clone(),
                }
            });
        }
    }
    if ssg.iter().any(|p| !matched.contains(p))
        || ssr.iter().any(|p| !declared.iter().any(|r| &r.0 == p))
    {
        return Err(failed(
            "ssg or ssr contains a path without a matching app route",
        ));
    }
    Ok(out)
}

fn atomic_file(path: &Path, data: &[u8]) -> Result<(), Error> {
    let dir = path.parent().unwrap();
    fs::create_dir_all(dir).map_err(failed)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(failed)?;
    tmp.write_all(data).map_err(failed)?;
    tmp.persist(path).map_err(failed)?;
    Ok(())
}

/// Builds and verifies the production manifest, immutable assets and pages.
/// The previous manifest stays usable throughout a failed or concurrent build.
pub fn build_production(
    ws: &Workspace,
    package: &Package,
    opts: &BuildOptions,
) -> Result<Bundle, Error> {
    let dir = opts
        .out_dir
        .clone()
        .unwrap_or_else(|| ws.target_dir.join("serge").join(&package.name));
    fs::create_dir_all(&dir).map_err(failed)?;
    let stage = tempfile::tempdir_in(&dir).map_err(failed)?;
    let raw = tempfile::tempdir_in(stage.path()).map_err(failed)?;
    let id = hyve::Ulid::new().to_string();
    crate::bundle::build_wasm(
        ws,
        package,
        &BuildOptions {
            out_dir: Some(raw.path().into()),
            ..opts.clone()
        },
        Some(&id),
    )?;
    let mut data = BTreeMap::new();
    files(raw.path(), Path::new(""), &mut data).map_err(failed)?;
    let mut template = String::from_utf8(
        data.remove("index.html")
            .ok_or_else(|| failed("missing index.html"))?,
    )
    .map_err(failed)?;
    let marker = "<script type=\"module\">";
    let mut index = 0;
    while let Some(start) = template.find(marker) {
        let body = start + marker.len();
        let end = template[body..]
            .find("</script>")
            .map(|i| i + body)
            .ok_or_else(|| failed("unclosed module script in app shell"))?;
        let name = format!("serge-bootstrap-{index}.mjs");
        if data
            .insert(name.clone(), template.as_bytes()[body..end].to_vec())
            .is_some()
        {
            return Err(failed("bootstrap filename collides with a web asset"));
        }
        template.replace_range(
            start..end + "</script>".len(),
            &format!("<script type=\"module\" src=\"/{name}\"></script>"),
        );
        index += 1;
    }
    data.retain(|name, _| !name.to_ascii_lowercase().ends_with(".html"));
    let mut assets = Assets {
        raw: data,
        output: BTreeMap::new(),
        active: HashSet::new(),
        stage: stage.path().into(),
    };
    for name in assets.raw.keys().cloned().collect::<Vec<_>>() {
        assets.write(&name)?;
    }
    let mut shell = assets.rewrite(&template, "index.html")?;
    if !shell.contains("</head>") {
        return Err(failed("index.html needs a head element"));
    }
    shell = shell.replacen(
        "</head>",
        &format!("<meta name=\"serge-build\" content=\"{id}\">\n</head>"),
        1,
    );
    let routes = routes(ws, package, opts, stage.path(), &shell, &id)?;
    let m = SergeBundleManifest {
        version: 1,
        build_id: id,
        assets: assets.output.into_values().collect(),
        routes,
    };
    let json = serde_json::to_vec_pretty(&m).map_err(failed)?;
    fs::write(stage.path().join("serge-bundle.json"), &json).map_err(failed)?;
    let checked = VerifiedBundle::open(
        stage.path(),
        &["/v1", "/health", "/metrics", "/openapi.json"],
    )
    .map_err(failed)?;
    for a in &m.assets {
        atomic_file(&dir.join(&a.file), &checked.assets[&a.url])?;
    }
    for (file, data) in &checked.pages {
        atomic_file(&dir.join(file), data)?;
    }
    if !opts.release {
        // Keep the loose debug files used by existing local tools.
        for (file, bytes) in &assets.raw {
            atomic_file(&dir.join(file), bytes)?;
        }
    }
    let index = m
        .routes
        .iter()
        .find(|r| r.pattern() == "/")
        .and_then(SergeRoute::file)
        .and_then(|f| checked.pages.get(f));
    atomic_file(
        &dir.join("index.html"),
        index.map_or(shell.as_bytes(), |b| b.as_ref()),
    )?;
    atomic_file(&dir.join("serge-bundle.json"), &json)?;
    Ok(Bundle { dir })
}
