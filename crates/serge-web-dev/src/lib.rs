//! `serge-web-dev` — what `hyve-serge build --web` and `hyve-serge preview
//! --web` do: build a serge app for the browser (`cargo` for
//! `wasm32-unknown-unknown`, then `wasm-bindgen --target web`) into
//! `target/serge/<package>/`, and serve it locally with SPA fallback, an API
//! proxy to an orbic server, and live reload on source changes.
//!
//! Phase 1 prototype: live reload reloads the page (state-preserving HMR is
//! planned), the server speaks plain HTTP/1.1 on loopback, and templates
//! are not cloned yet. A web app opts in with
//! `[package.metadata.serge] web = true` in its manifest.

mod bundle;
mod server;
mod workspace;

pub use bundle::{BuildOptions, Bundle, build_web};
pub use server::{DevServer, Proxy, Reload};
pub use workspace::{Package, Workspace};

/// A failure with a hint for the user.
#[derive(Debug)]
pub struct Error {
    /// What went wrong.
    pub message: String,
    /// What to do about it.
    pub hint: Option<String>,
}

impl Error {
    pub(crate) fn new(message: impl Into<String>) -> Error {
        Error {
            message: message.into(),
            hint: None,
        }
    }

    pub(crate) fn hint(mut self, hint: impl Into<String>) -> Error {
        self.hint = Some(hint.into());
        self
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)?;
        if let Some(h) = &self.hint {
            write!(f, "\n  hint: {h}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {}
