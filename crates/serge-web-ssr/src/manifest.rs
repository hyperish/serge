use crate::Pattern;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::io;
use std::path::Path;

/// Manifest written as `serge-bundle.json`. Unknown extension fields are ignored.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SergeBundleManifest {
    /// Wire version, currently 1.
    pub version: u16,
    /// Unique canonical ULID of this build.
    pub build_id: String,
    /// Immutable public assets.
    pub assets: Vec<SergeAsset>,
    /// UI routes in declaration order.
    pub routes: Vec<SergeRoute>,
}
/// A publicly served content-addressed file.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SergeAsset {
    /// Absolute public URL.
    pub url: String,
    /// Relative path inside the bundle.
    pub file: String,
    /// HTTP content type.
    pub media_type: String,
    /// Exact byte length.
    pub bytes: u64,
    /// Lowercase hexadecimal SHA-256 of the file.
    pub sha256: String,
}
/// Delivery mode and data for one UI route.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
#[allow(missing_docs)] // All variants use the same pattern and relative HTML file fields.
pub enum SergeRoute {
    Csr { pattern: String, file: String },
    Ssg { pattern: String, file: String },
    Ssr { pattern: String },
}
impl SergeRoute {
    /// Whole-segment UI route, or concrete SSG path.
    pub fn pattern(&self) -> &str {
        match self {
            Self::Csr { pattern, .. } | Self::Ssg { pattern, .. } | Self::Ssr { pattern } => {
                pattern
            }
        }
    }
    /// Relative path to the HTML, if this route uses a static document.
    pub fn file(&self) -> Option<&str> {
        match self {
            Self::Csr { file, .. } | Self::Ssg { file, .. } => Some(file),
            Self::Ssr { .. } => None,
        }
    }
}

/// The bytes used for a mount are immutable after verification. A request
/// never traverses the filesystem and an old response can retain its build.
pub struct VerifiedBundle {
    /// Parsed manifest and its build identity.
    pub manifest: SergeBundleManifest,
    /// Public assets indexed by the exact declared URL.
    pub assets: BTreeMap<String, Bytes>,
    /// HTML indexed by its relative manifest filename.
    pub pages: BTreeMap<String, Bytes>,
    /// Parsed matchers in declaration order.
    pub patterns: Vec<Pattern>,
}

fn invalid(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}
/// Lowercase SHA-256.
pub fn digest(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        write!(out, "{byte:02x}").expect("formatting into String");
    }
    out
}
/// Content type associated with a named public asset.
pub fn media_type(name: &str) -> &'static str {
    match name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "js" | "mjs" => "text/javascript",
        "wasm" => "application/wasm",
        "css" => "text/css",
        "json" | "map" => "application/json",
        "png" => "image/png",
        "jpeg" | "jpg" => "image/jpeg",
        "svg" => "image/svg+xml",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "txt" => "text/plain",
        "pdf" => "application/pdf",
        "webmanifest" => "application/manifest+json",
        _ => "application/octet-stream",
    }
}

fn read(root: &Path, name: &str) -> io::Result<Bytes> {
    let mut path = root.to_path_buf();
    if name.contains(['\\', ':', '\0']) {
        return Err(invalid("invalid relative bundle path"));
    }
    for part in name.split('/') {
        if part.is_empty() || matches!(part, "." | "..") {
            return Err(invalid("bundle files must be normal relative paths"));
        }
        path.push(part);
        if std::fs::symlink_metadata(&path)?.is_symlink() {
            return Err(invalid("symbolic link inside bundle"));
        }
    }
    if !path.is_file() {
        return Err(invalid("bundle entry must be a regular file"));
    }
    Ok(std::fs::read(path)?.into())
}

impl VerifiedBundle {
    /// Reads and verifies a complete candidate before a host publishes it.
    /// Reserved prefixes include the app's API and management endpoints.
    pub fn open(root: &Path, reserved: &[&str]) -> io::Result<Self> {
        let json: serde_json::Value = serde_json::from_slice(&read(root, "serge-bundle.json")?)
            .map_err(|e| invalid(e.to_string()))?;
        if json["routes"].as_array().is_some_and(|routes| {
            routes
                .iter()
                .any(|r| r["mode"] == "ssr" && r.get("file").is_some())
        }) {
            return Err(invalid("SSR routes cannot contain a file"));
        }
        let m: SergeBundleManifest =
            serde_json::from_value(json).map_err(|e| invalid(e.to_string()))?;
        if m.version != 1
            || !m
                .build_id
                .parse::<hyve::Ulid>()
                .is_ok_and(|id| id.to_string() == m.build_id)
        {
            return Err(invalid("unsupported bundle version or invalid buildId"));
        }
        let mut assets = BTreeMap::new();
        let mut pages = BTreeMap::new();
        let mut patterns = Vec::new();
        let mut files = HashSet::new();
        let mut asset_hashes = HashSet::new();
        for asset in &m.assets {
            let name = asset.url.rsplit('/').next().unwrap_or("");
            if name.is_empty()
                || matches!(name, "." | "..")
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._~-".contains(&b))
                || asset.url != format!("/assets/{}/{name}", asset.sha256)
                || asset.media_type != media_type(name)
                || asset.bytes > 9_007_199_254_740_991
                || !files.insert(asset.file.as_str())
            {
                return Err(invalid(format!("invalid public asset {}", asset.url)));
            }
            let bytes = read(root, &asset.file)?;
            if bytes.len() as u64 != asset.bytes
                || digest(&bytes) != asset.sha256
                || assets.insert(asset.url.clone(), bytes).is_some()
            {
                return Err(invalid(format!(
                    "invalid content or duplicate URL for {}",
                    asset.url
                )));
            }
            asset_hashes.insert(asset.sha256.as_str());
        }
        let mut names = HashSet::new();
        for route in &m.routes {
            let p = route.pattern();
            let first = p.split('/').nth(1).unwrap_or("");
            if !p.starts_with('/')
                || (p.len() > 1 && p.ends_with('/'))
                || p.contains("//")
                || p.contains(['?', '#', '\\', '%'])
                || p.chars().any(char::is_control)
                || p.split('/').any(|s| matches!(s, "." | ".."))
                || !names.insert(p)
                || reserved.iter().chain(std::iter::once(&"/assets")).any(|r| {
                    !first.is_empty()
                        && (p == *r || p.strip_prefix(*r).is_some_and(|rest| rest.starts_with('/')))
                })
                || (matches!(route, SergeRoute::Ssg { .. }) && p.contains('{'))
            {
                return Err(invalid(format!(
                    "invalid, duplicate, or reserved UI route {p}"
                )));
            }
            patterns.push(Pattern::parse(p).map_err(invalid)?);
            if let Some(file) = route.file() {
                if files.contains(file) {
                    return Err(invalid("an HTML file is also a public asset"));
                }
                if !pages.contains_key(file) {
                    let bytes = read(root, file)?;
                    std::str::from_utf8(&bytes).map_err(|_| invalid("HTML must be UTF-8"))?;
                    if asset_hashes.contains(digest(&bytes).as_str()) {
                        return Err(invalid("public asset aliases route HTML"));
                    }
                    pages.insert(file.to_owned(), bytes);
                }
            }
        }
        Ok(Self {
            manifest: m,
            assets,
            pages,
            patterns,
        })
    }
}
