//! Native development runtime for the local Serge launcher.
#![cfg(all(debug_assertions, target_os = "macos", target_arch = "aarch64"))]
use hyve::state::{Signal, signal};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    fs, io,
    path::{Path, PathBuf},
};

struct Session {
    dir: PathBuf,
    epoch: Signal<u32>,
}
thread_local! { static ACTIVE: RefCell<Option<Session>> = const {RefCell::new(None)}; }

/// Registers the launched process before its views mount. No listener or
/// network connection is opened. The launcher owns the private session directory.
pub fn initialize() {
    let Some(dir) = std::env::var_os("SERGE_RENDER_SESSION").map(PathBuf::from) else {
        return;
    };
    let state = serde_json::json!({"pid":std::process::id(),"generation":dir.file_name().and_then(|n|n.to_str()),"reference":subsecond::aslr_reference()});
    if atomic(&dir, "ready", state.to_string().as_bytes()).is_ok() {
        ACTIVE.with(|s| {
            *s.borrow_mut() = Some(Session {
                dir,
                epoch: signal(0),
            })
        });
    }
}
fn atomic(dir: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
    let temp = dir.join(format!("{name}.tmp"));
    fs::write(&temp, bytes)?;
    fs::rename(temp, dir.join(name))
}
/// Runs a macro-checked leaf and tracks the current render generation.
#[inline(always)]
pub fn evaluate(f: impl Fn() -> u32 + Copy + 'static) -> u32 {
    if let Some(epoch) = ACTIVE.with(|s| s.borrow().as_ref().map(|s| s.epoch)) {
        let _ = epoch.get();
        subsecond::call(f)
    } else {
        f()
    }
}

/// Checks for one trusted compiler result between frames. Returns true while
/// the development UI should keep polling for edits.
pub fn between_frames() -> bool {
    let Some((dir, epoch)) = ACTIVE.with(|s| s.borrow().as_ref().map(|s| (s.dir.clone(), s.epoch)))
    else {
        return false;
    };
    let file = dir.join("patch");
    if !file.is_file() {
        return true;
    }
    let apply = || -> Result<(), Box<dyn std::error::Error>> {
        let msg: serde_json::Value = serde_json::from_slice(&fs::read(&file)?)?;
        if msg["pid"].as_u64() != Some(u64::from(std::process::id()))
            || msg["generation"].as_str() != dir.file_name().and_then(|s| s.to_str())
        {
            return Err("stale build session".into());
        }
        let mut table: subsecond::JumpTable = serde_json::from_value(msg["table"].clone())?;
        let name = msg["sha256"].as_str().ok_or("missing artifact digest")?;
        if name.len() != 64 || !name.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("invalid artifact digest".into());
        }
        table.lib = dir.join(format!("{name}.dylib"));
        if msg["reference"].as_u64() != Some(subsecond::aslr_reference() as u64)
            || hex::encode(Sha256::digest(fs::read(&table.lib)?)) != name
        {
            return Err("incompatible local render build".into());
        }
        // SAFETY: the launching CLI generated this table for this exact live
        // binary, target, compiler and process, after accepting only literal
        // edits to fixed-signature leaf bodies. The dylib comes exclusively
        // from its private per-generation directory and is content checked.
        #[allow(unsafe_code)]
        unsafe { subsecond::apply_patch(table) }?;
        Ok(())
    };
    let result = apply();
    if let Err(e) = &result {
        eprintln!("serge: render edit requires a restart: {e}");
    }
    let _ = fs::remove_file(&file);
    let message = if result.is_ok() { "applied" } else { "restart" };
    let _ = atomic(&dir, "ack", message.as_bytes());
    if result.is_ok() {
        epoch.update(|n| *n = n.wrapping_add(1));
    }
    true
}
