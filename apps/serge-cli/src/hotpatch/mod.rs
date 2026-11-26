//! Development-only, macOS arm64 hot reload for macro-checked leaf code.
//! DX compiles our own child binary. A private directory ties each response
//! to a single build and PID, with polling only at a UI frame boundary.
use super::{Fail, Opts, Window, Workspace, dev_error, native_app};
use hyve::parser::{TomlValue, parse_toml};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs,
    io::{self, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use tempfile::TempDir;
mod inputs;
use inputs::{Inputs, Snapshot};

pub(super) fn enabled(ws: &Workspace) -> Result<bool, Fail> {
    let config = match fs::read_to_string(ws.root.join("hyve.toml")) {
        Ok(s) => s,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(dev_error(e)),
    };
    let table = parse_toml(&config).map_err(|_| dev_error("invalid hyve.toml"))?;
    let Some(TomlValue::Table(table)) = table.get("serge").map(|v| &v.value) else {
        return Ok(false);
    };
    match table.get("native_hotpatch").map(|v| &v.value) {
        None | Some(TomlValue::Bool(false)) => Ok(false),
        Some(TomlValue::Bool(true)) => Ok(true),
        _ => Err(dev_error("serge.native_hotpatch must be a boolean")),
    }
}
struct Builder<'a> {
    ws: &'a Workspace,
    opts: &'a Opts,
    package: String,
    bin: String,
    dx: OsString,
    build_dir: TempDir,
}
impl Builder<'_> {
    fn compile(&self, base: Option<(&Value, u64)>) -> Result<Value, Fail> {
        let mut cmd = Command::new(&self.dx);
        cmd.current_dir(&self.ws.root)
            .env("CARGO_TARGET_DIR", self.build_dir.path())
            .arg("--json-output");
        if let Some((_, reference)) = base {
            cmd.args([
                "tools",
                "hotpatch",
                "--aslr-reference",
                &reference.to_string(),
            ]);
        } else {
            cmd.args(["build", "--fat-binary"]);
        }
        let mut features = self.opts.features.clone();
        features.push("serge/native-hotpatch".into());
        cmd.args([
            "--platform",
            "macos",
            "--renderer",
            "native",
            "-p",
            &self.package,
            "--bin",
            &self.bin,
            "--features",
            &features.join(","),
        ]);
        let mut child = cmd
            .stdin(if base.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .spawn()
            .map_err(dev_error)?;
        if let Some((artifacts, _)) = base {
            let mut input = child.stdin.take().unwrap();
            serde_json::to_writer(&mut input, artifacts).map_err(dev_error)?;
            input.flush().map_err(dev_error)?;
            drop(input);
        }
        let result = child.wait_with_output().map_err(dev_error)?;
        // Structured DX metadata includes linker/environment arguments. Do not
        // print it. Keep compiler diagnostics and a single selected result.
        let key = if base.is_some() {
            "Hotpatch"
        } else {
            "BuildsFinished"
        };
        let mut payload = None;
        for line in result.stdout.split(|&b| b == b'\n') {
            let Ok(log) = serde_json::from_slice::<Value>(line) else {
                continue;
            };
            if let Some(data) = log["json"]
                .as_str()
                .and_then(|s| serde_json::from_str::<Value>(s).ok())
                && !data[key].is_null()
            {
                payload = Some(data[key].clone());
            }
            if matches!(log["level"].as_str(), Some("ERROR" | "error"))
                && let Some(msg) = log["message"].as_str()
            {
                eprintln!("hyve-serge: {msg}");
            }
        }
        if !result.status.success() {
            return Err(dev_error("native render build failed"));
        }
        payload.ok_or_else(|| dev_error("DX returned no native build result"))
    }
    fn own_output(&self, file: &Value) -> Result<PathBuf, Fail> {
        let path = Path::new(
            file.as_str()
                .ok_or_else(|| dev_error("missing DX native artifact"))?,
        )
        .canonicalize()
        .map_err(dev_error)?;
        if !path.starts_with(self.build_dir.path().canonicalize().map_err(dev_error)?) {
            return Err(dev_error("unexpected DX native artifact directory"));
        }
        Ok(path)
    }
    fn start(&self, window: &Window) -> Result<Generation, Fail> {
        let output = self.compile(None)?;
        let mut artifacts = output["client"].clone();
        // DX replays the fat compiler environment after preparing the thin
        // command. Its temporary linker files belong to the completed build.
        // Let the new invocation install its own linker and capture paths.
        if let Some(vars) = artifacts["rustc_envs"].as_array_mut() {
            vars.retain(|pair| {
                pair[0].as_str().is_none_or(|key| {
                    !key.starts_with("DX_LINK")
                        && !matches!(
                            key,
                            "DX_RUSTC" | "RUSTC_WRAPPER" | "RUSTC_WORKSPACE_WRAPPER"
                        )
                })
            });
        }
        let exe = self.own_output(&artifacts["exe"])?;
        let dir = tempdir(&window.session_dir().map_err(dev_error)?, "render-session-")?;
        let mut cmd = Command::new(exe);
        cmd.current_dir(&self.ws.root)
            .env("SERGE_RENDER_SESSION", dir.path())
            .stdin(Stdio::null());
        window.start(cmd).map_err(dev_error)?;
        let pid = window
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .child
            .as_ref()
            .map(|c| c.id())
            .ok_or_else(|| dev_error("development window exited"))?;
        Ok(Generation {
            dir,
            pid,
            artifacts,
        })
    }
    fn try_start(&self, window: &Window) -> Option<Generation> {
        match self.start(window) {
            Ok(g) => Some(g),
            Err(Fail::Error(e)) => {
                eprintln!("hyve-serge: {}", e.message);
                None
            }
            Err(_) => None,
        }
    }
    fn patch(&self, g: &Generation, inputs: &Inputs, expected: &Snapshot) -> Result<(), Fail> {
        let ready: Value =
            serde_json::from_slice(&fs::read(g.dir.path().join("ready")).map_err(dev_error)?)
                .map_err(dev_error)?;
        let session = g.dir.path().file_name().and_then(|s| s.to_str());
        if ready["pid"].as_u64() != Some(u64::from(g.pid))
            || ready["generation"].as_str() != session
        {
            return Err(dev_error("native window changed"));
        }
        let reference = ready["reference"]
            .as_u64()
            .ok_or_else(|| dev_error("window has no render registration"))?;
        let result = self.compile(Some((&g.artifacts, reference)))?;
        let table = &result["jump_table"];
        let dylib = self.own_output(&table["lib"])?;
        let fresh = inputs.snapshot(self.ws, &self.opts.features)?;
        if fresh.exact != expected.exact {
            return Err(dev_error("sources changed during patch compilation"));
        }
        let dir = g.dir.path();
        let bytes = fs::read(dylib).map_err(dev_error)?;
        let digest = hex::encode(Sha256::digest(&bytes));
        fs::write(dir.join(format!("{digest}.dylib")), bytes).map_err(dev_error)?;
        let message = json!({"generation":session,"pid":g.pid,"sha256":digest,"reference":reference,"table":table});
        let _ = fs::remove_file(dir.join("ack"));
        fs::write(dir.join("patch.tmp"), message.to_string())
            .and_then(|_| fs::rename(dir.join("patch.tmp"), dir.join("patch")))
            .map_err(dev_error)?;
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Ok(answer) = fs::read_to_string(dir.join("ack")) {
                return if answer == "applied" {
                    Ok(())
                } else {
                    Err(dev_error("window requested a full render rebuild"))
                };
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        Err(dev_error("window did not confirm the render patch"))
    }
}
fn tempdir(parent: &Path, prefix: &str) -> Result<TempDir, Fail> {
    tempfile::Builder::new()
        .prefix(prefix)
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(parent)
        .map_err(dev_error)
}
struct Generation {
    dir: TempDir,
    pid: u32,
    artifacts: Value,
}
pub(super) fn serve(ws: &Workspace, opts: &Opts, window: &Window) -> Result<(), Fail> {
    let app = ws.web_app(opts.package.as_deref())?;
    let bin = if app.bins.contains(&app.name) {
        &app.name
    } else if let [one] = app.bins.as_slice() {
        one
    } else {
        return Err(dev_error("select a package with a native binary"));
    };
    let dx = std::env::var_os("HYVE_DX").unwrap_or_else(|| "dx".into());
    let tool_ok = Command::new(&dx).arg("--version").output().is_ok_and(|o| {
        o.status.success()
            && String::from_utf8_lossy(&o.stdout).split_whitespace().nth(1) == Some("0.7.10")
    });
    if !tool_ok {
        eprintln!(
            "hyve-serge: native render hotpatch needs dx 0.7.10; using state-preserving rebuilds"
        );
        return super::dev_native_restart(ws, opts, window);
    }
    let builder = Builder {
        ws,
        opts,
        package: app.name.clone(),
        bin: bin.clone(),
        dx,
        build_dir: tempdir(&window.session_dir().map_err(dev_error)?, "render-build-")?,
    };
    let mut inputs = Inputs::resolve(ws, &builder.package, &builder.bin)?;
    let mut stamp = inputs.stamp().map_err(dev_error)?;
    let mut snapshot = inputs.snapshot(ws, &opts.features)?;
    let mut generation = builder.try_start(window);
    if generation.is_none() {
        window.start(native_app(opts)?).map_err(dev_error)?;
    }
    // A build spanning two input revisions cannot be a hotpatch baseline.
    let mut rebuild = inputs.stamp().ok() != Some(stamp);
    loop {
        if let Some(status) = window.status().map_err(dev_error)? {
            return if status.success() {
                Ok(())
            } else {
                Err(dev_error(format!("native window exited with {status}")))
            };
        }
        std::thread::sleep(Duration::from_millis(250));
        let next_stamp = match inputs.stamp() {
            Ok(s) => s,
            Err(_) => continue,
        };
        if !rebuild && next_stamp == stamp {
            continue;
        }
        let next = match inputs.snapshot(ws, &opts.features) {
            Ok(s) => s,
            Err(_) => continue,
        };
        stamp = next_stamp;
        let must_rebuild = std::mem::take(&mut rebuild);
        if !must_rebuild && next.exact == snapshot.exact {
            continue;
        }
        if !must_rebuild
            && let Some(g) = &generation
            && snapshot.shape.is_some()
            && snapshot.shape == next.shape
        {
            match builder.patch(g, &inputs, &next) {
                Ok(()) => {
                    eprintln!("hyve-serge: applied render edit in PID {}", g.pid);
                    snapshot = next;
                    continue;
                }
                Err(Fail::Error(error)) => eprintln!(
                    "hyve-serge: rebuilding the native window: {}",
                    error.message
                ),
                Err(error) => return Err(error),
            }
        }
        // Validate first. A broken edit leaves the last successful window live.
        match native_app(opts) {
            Ok(fallback) => {
                let Ok(resolved) = Inputs::resolve(ws, &builder.package, &builder.bin) else {
                    eprintln!(
                        "hyve-serge: Cargo metadata changed during rebuild; keeping the current window"
                    );
                    rebuild = true;
                    continue;
                };
                let (Ok(before), Ok(current), Ok(hotpatch)) = (
                    resolved.stamp(),
                    resolved.snapshot(ws, &opts.features),
                    enabled(ws),
                ) else {
                    continue;
                };
                inputs = resolved;
                snapshot = current;
                generation = if hotpatch {
                    builder.try_start(window)
                } else {
                    None
                };
                if generation.is_none() {
                    window.start(fallback).map_err(dev_error)?;
                }
                stamp = before;
                rebuild = inputs.stamp().ok() != Some(before);
            }
            Err(super::Fail::Error(err)) => eprintln!("hyve-serge: {}", err.message),
            Err(e) => return Err(e),
        }
    }
}
