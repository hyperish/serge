use crate::invalid;
use hyve::{HyveResult, Ulid};
use serge_web_ssr::{VerifiedBundle, digest};
use std::collections::HashMap;

pub(crate) const CSP: &str = "default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self'; img-src 'self'; font-src 'self'; connect-src 'self'; frame-src 'none'; child-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

/// A hash-pinned HTML document and its immutable, verified manifest assets.
pub struct LocalDocument {
    pub(crate) build: Ulid,
    pub(crate) document: Vec<u8>,
    pub(crate) assets: HashMap<String, (String, Vec<u8>)>,
}
impl LocalDocument {
    /// Pin the HTML digest in the trusted native application. Each script or
    /// stylesheet must be referenced by its content-addressed manifest URL.
    pub fn from_bundle(
        bundle: &VerifiedBundle,
        page: &str,
        expected_sha256: &str,
    ) -> HyveResult<Self> {
        let html = bundle
            .pages
            .get(page)
            .filter(|html| digest(html) == expected_sha256)
            .ok_or_else(|| invalid("webview document integrity check failed"))?;
        let mut assets = HashMap::new();
        for asset in &bundle.manifest.assets {
            let bytes = bundle
                .assets
                .get(&asset.url)
                .ok_or_else(|| invalid("missing webview asset"))?;
            if digest(bytes) != asset.sha256
                || asset.url
                    != format!(
                        "/assets/{}/{}",
                        asset.sha256,
                        asset.url.rsplit('/').next().unwrap_or("")
                    )
            {
                return Err(invalid("invalid webview asset"));
            }
            assets.insert(
                asset.url.clone(),
                (asset.media_type.clone(), bytes.to_vec()),
            );
        }
        let build = bundle
            .manifest
            .build_id
            .parse()
            .map_err(|_| invalid("invalid webview build"))?;
        Ok(Self {
            build,
            document: html.to_vec(),
            assets,
        })
    }
}
