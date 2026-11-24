//! `hyve-serge` — the serge stages `hyve` runs (`docs/03-adapters-and-pipeline.md`
//! §2, 도구 단계), also usable directly:
//!
//! ```text
//! hyve-serge stages
//! hyve-serge build [--debug] [-p <package>] [--features <a,b>] [--out <dir>]
//! hyve-serge preview [--web | --native] [-p <package>] [--port <n>]
//!                    [--api-proxy <url>] [--features <a,b>] [--no-watch] [--release]
//! hyve-serge preview --native [-p <package>] [--features <a,b>] [--release]
//! ```
//!
//! `stages` answers `hyve` (first line: serge's version, then one stage per
//! line; `preview` only when the workspace has an app). `build` bundles every web app (`[package.metadata.serge] web =
//! true`) for the browser into `target/serge/<package>/`, release unless
//! `--debug`. `preview --web` builds the app, serves it with SPA fallback,
//! proxies the API prefix to an orbic server (`--api-proxy
//! http://127.0.0.1:3000/v1`, or `api_proxy` in `hyve.toml`'s `[serge]`
//! table: the URL's path is the proxied prefix), and rebuilds and reloads
//! the page when sources change. `preview --native` builds the app's binary
//! and runs it in a window; `preview` without either runs both. Under `hyve
//! preview` (`HYVE_STAGE_LIFELINE=stdin`) the preview stops, window
//! included, when stdin closes. Exit codes: 0 success, 1 failure, 2 usage.

use std::path::PathBuf;
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use serge_web_dev::{BuildOptions, DevServer, Error, Proxy, Workspace};

const USAGE: &str = "usage:
  hyve-serge stages
  hyve-serge build [--debug] [-p <package>] [--features <a,b>] [--out <dir>]
  hyve-serge preview [--web | --native] [-p <package>] [--port <n>] [--api-proxy <url>] [--features <a,b>] [--no-watch] [--release]";

enum Fail {
    Usage(String),
    Error(Error),
}

impl From<Error> for Fail {
    fn from(e: Error) -> Fail {
        Fail::Error(e)
    }
}

#[derive(Clone, Default)]
struct Opts {
    web: bool,
    native: bool,
    debug: bool,
    release: bool,
    package: Option<String>,
    features: Vec<String>,
    out: Option<PathBuf>,
    port: Option<u16>,
    api_proxy: Option<String>,
    no_watch: bool,
}

fn parse(cmd: &str, args: &[String], allowed: &[&str]) -> Result<Opts, Fail> {
    let mut o = Opts::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let flag = a.as_str();
        if !allowed.contains(&flag) {
            return Err(Fail::Usage(format!("`{cmd}` does not take `{a}`")));
        }
        let mut value = || {
            it.next()
                .cloned()
                .ok_or_else(|| Fail::Usage(format!("`{a}` needs a value")))
        };
        match flag {
            "--web" => o.web = true,
            "--native" => o.native = true,
            "--debug" => o.debug = true,
            "--release" => o.release = true,
            "--no-watch" => o.no_watch = true,
            "-p" | "--package" => o.package = Some(value()?),
            "--features" => o.features.extend(
                value()?
                    .split(',')
                    .map(|s| s.trim().to_owned())
                    .filter(|s| !s.is_empty()),
            ),
            "--out" => o.out = Some(PathBuf::from(value()?)),
            "--port" => {
                let v = value()?;
                o.port = Some(
                    v.parse()
                        .map_err(|_| Fail::Usage(format!("`--port {v}` is not a port")))?,
                );
            }
            "--api-proxy" => o.api_proxy = Some(value()?),
            _ => unreachable!("checked above"),
        }
    }
    Ok(o)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.split_first() {
        Some((s, rest)) if s == "stages" && rest.is_empty() => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            println!("build");
            // `hyve preview` ends when any stage ends, so `preview` is
            // offered only when there is an app to preview.
            if workspace().is_ok_and(|ws| ws.packages.iter().any(|p| p.web)) {
                println!("preview");
            }
            Ok(())
        }
        Some((s, rest)) if s == "build" => build(rest),
        Some((s, rest)) if s == "preview" => preview(rest),
        Some((s, _)) if s == "--help" || s == "-h" => {
            println!("{USAGE}");
            Ok(())
        }
        Some((s, _)) => Err(Fail::Usage(format!("unknown stage `{s}`"))),
        None => Err(Fail::Usage("no stage given".into())),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(Fail::Usage(m)) => {
            eprintln!("error: {m}\n{USAGE}");
            ExitCode::from(2)
        }
        Err(Fail::Error(e)) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn workspace() -> Result<Workspace, Fail> {
    let dir = std::env::current_dir().map_err(|e| {
        Fail::Error(Error {
            message: format!("no current directory: {e}"),
            hint: None,
        })
    })?;
    Ok(Workspace::load(&dir)?)
}

fn build(args: &[String]) -> Result<(), Fail> {
    let o = parse(
        "build",
        args,
        &["--web", "--debug", "-p", "--package", "--features", "--out"],
    )?;
    let ws = workspace()?;
    let apps: Vec<_> = match &o.package {
        Some(p) => vec![ws.web_app(Some(p))?.clone()],
        None => ws.packages.iter().filter(|p| p.web).cloned().collect(),
    };
    if apps.is_empty() {
        eprintln!("hyve-serge: no web apps to bundle (`[package.metadata.serge] web = true`)");
        return Ok(());
    }
    for app in apps {
        let opts = BuildOptions {
            release: !o.debug,
            features: o.features.clone(),
            out_dir: o.out.clone(),
        };
        let b = serge_web_dev::build_web(&ws, &app, &opts)?;
        eprintln!("hyve-serge: {} → {}", app.name, b.dir.display());
    }
    Ok(())
}

fn preview(args: &[String]) -> Result<(), Fail> {
    let o = parse(
        "preview",
        args,
        &[
            "--web",
            "--native",
            "-p",
            "--package",
            "--features",
            "--port",
            "--api-proxy",
            "--no-watch",
            "--release",
        ],
    )?;
    match (o.web, o.native) {
        (true, true) => Err(Fail::Usage(
            "choose `--web` or `--native` (neither runs both)".into(),
        )),
        (false, true) => {
            let window = Window::default();
            window.lifeline();
            window.start(native_app(&o)?).map_err(|e| {
                Fail::Error(Error {
                    message: format!("the app could not start: {e}"),
                    hint: None,
                })
            })?;
            match window.wait() {
                Some(status) if !status.success() => Err(Fail::Error(Error {
                    message: format!("the app exited with {status}"),
                    hint: None,
                })),
                _ => Ok(()),
            }
        }
        (true, false) => preview_web(&o, None),
        // Neither: every target of the app — the window next to the page.
        (false, false) => preview_web(&o, Some(())),
    }
}

/// The native window this tool started, if any. Under `hyve preview`
/// (`HYVE_STAGE_LIFELINE=stdin`) it is stopped, and the tool exits, when
/// stdin closes (`hyve/docs/01-dx-and-dsl.md` §2). Starting and stopping
/// hold the same lock, so a window never starts after the stop.
#[derive(Clone, Default)]
struct Window(Arc<Mutex<(Option<Child>, bool)>>);

impl Window {
    fn lifeline(&self) {
        if std::env::var("HYVE_STAGE_LIFELINE").as_deref() != Ok("stdin") {
            return;
        }
        let window = self.clone();
        std::thread::spawn(move || {
            use std::io::Read;
            let mut sink = [0u8; 256];
            let mut stdin = std::io::stdin();
            while matches!(stdin.read(&mut sink), Ok(n) if n > 0) {}
            let mut w = window.0.lock().unwrap_or_else(|e| e.into_inner());
            w.1 = true;
            if let Some(mut c) = w.0.take() {
                let _ = c.kill();
                let _ = c.wait();
            }
            std::process::exit(0);
        });
    }

    /// Starts the window unless the tool is stopping.
    fn start(&self, mut cmd: Command) -> std::io::Result<()> {
        let mut w = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if !w.1 {
            w.0 = Some(cmd.spawn()?);
        }
        Ok(())
    }

    /// Waits for the window to close.
    fn wait(&self) -> Option<std::process::ExitStatus> {
        loop {
            {
                let mut w = self.0.lock().unwrap_or_else(|e| e.into_inner());
                match w.0.as_mut().map(Child::try_wait) {
                    Some(Ok(Some(status))) => return Some(status),
                    Some(Ok(None)) => {}
                    Some(Err(_)) | None => return None,
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

/// Builds the app's native binary and returns the command that runs it
/// directly (stopping it stops the app; `cargo run` would leave it behind).
fn native_app(o: &Opts) -> Result<Command, Fail> {
    let ws = workspace()?;
    let (bin, mut build) = native_build(&ws, o)?;
    let status = build.status().map_err(|e| {
        Fail::Error(Error {
            message: format!("could not run cargo: {e}"),
            hint: None,
        })
    })?;
    if !status.success() {
        return Err(Fail::Error(Error {
            message: "the native build failed".into(),
            hint: None,
        }));
    }
    let mut cmd = Command::new(bin);
    cmd.current_dir(&ws.root).stdin(Stdio::null());
    Ok(cmd)
}

/// `cargo build` of the app's native binary, and where the binary lands.
fn native_build(ws: &Workspace, o: &Opts) -> Result<(PathBuf, Command), Fail> {
    let app = ws.web_app(o.package.as_deref())?;
    let bin = if app.bins.contains(&app.name) {
        app.name.clone()
    } else if let [one] = app.bins.as_slice() {
        one.clone()
    } else {
        return Err(Fail::Error(Error {
            message: format!(
                "`{}` has no binary named after it to run natively",
                app.name
            ),
            hint: Some("add `src/main.rs` that calls `SergeApp::run()`".into()),
        }));
    };
    let mut cmd = Command::new("cargo");
    cmd.args(["build", "-p", &app.name, "--bin", &bin])
        .current_dir(&ws.root)
        .stdin(Stdio::null());
    if o.release {
        cmd.arg("--release");
    }
    if !o.features.is_empty() {
        cmd.arg("--features").arg(o.features.join(","));
    }
    let path = ws
        .target_dir
        .join(if o.release { "release" } else { "debug" })
        .join(format!("{bin}{}", std::env::consts::EXE_SUFFIX));
    Ok((path, cmd))
}

/// A cheap fingerprint of the watched sources: newest mtime and file count.
fn sources_stamp(dirs: &[PathBuf]) -> (Option<SystemTime>, usize) {
    fn walk(dir: &std::path::Path, newest: &mut Option<SystemTime>, n: &mut usize) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, newest, n);
            } else if let Ok(m) = e.metadata().and_then(|m| m.modified()) {
                *n += 1;
                if newest.is_none_or(|t| m > t) {
                    *newest = Some(m);
                }
            }
        }
    }
    let (mut newest, mut n) = (None, 0);
    for d in dirs {
        walk(d, &mut newest, &mut n);
    }
    (newest, n)
}

fn preview_web(o: &Opts, native: Option<()>) -> Result<(), Fail> {
    let window = Window::default();
    window.lifeline();
    let ws = workspace()?;
    let app = ws.web_app(o.package.as_deref())?.clone();
    let opts = BuildOptions {
        release: o.release,
        features: o.features.clone(),
        out_dir: None,
    };
    let started = Instant::now();
    let bundle = serge_web_dev::build_web(&ws, &app, &opts)?;
    eprintln!(
        "hyve-serge: built {} in {:.1}s",
        app.name,
        started.elapsed().as_secs_f32()
    );

    let proxy = o
        .api_proxy
        .clone()
        .or_else(|| ws.configured_api_proxy())
        .map(|u| Proxy::from_url(&u));
    let port = o.port.unwrap_or(8080);
    let server = (0..20)
        .find_map(|i| {
            DevServer::start(
                &format!("127.0.0.1:{}", port + i),
                bundle.dir.clone(),
                proxy.clone(),
            )
            .ok()
        })
        .ok_or_else(|| {
            Fail::Error(Error {
                message: format!("no free port from {port}"),
                hint: Some("pass `--port <n>`".into()),
            })
        })?;
    eprintln!(
        "hyve-serge: serving {} at http://{}/",
        app.name,
        server.addr()
    );
    if let Some(p) = &proxy {
        eprintln!("hyve-serge: {}/* → http://{}", p.prefix, p.target);
    }
    // The window starts once the page is served (built in the background
    // after the web build); the page keeps being served after it closes.
    if native.is_some() {
        eprintln!("hyve-serge: building the native window next to the page");
        let (o, window) = (o.clone(), window.clone());
        std::thread::spawn(move || {
            match native_app(&o).and_then(|cmd| {
                window.start(cmd).map_err(|e| {
                    Fail::Error(Error {
                        message: format!("{e}"),
                        hint: None,
                    })
                })
            }) {
                Ok(()) => {}
                Err(Fail::Error(e)) => {
                    eprintln!(
                        "hyve-serge: the native window could not start: {}",
                        e.message
                    )
                }
                Err(Fail::Usage(e)) => {
                    eprintln!("hyve-serge: the native window could not start: {e}")
                }
            }
        });
    }
    if o.no_watch {
        eprintln!("hyve-serge: Ctrl-C stops");
        loop {
            std::thread::park();
        }
    }
    eprintln!(
        "hyve-serge: watching sources; saving a file rebuilds and reloads the page (Ctrl-C stops)"
    );
    let dirs = ws.watch_dirs();
    let mut stamp = sources_stamp(&dirs);
    loop {
        std::thread::sleep(Duration::from_millis(400));
        let now = sources_stamp(&dirs);
        if now == stamp {
            continue;
        }
        // Let a burst of saves settle.
        std::thread::sleep(Duration::from_millis(150));
        stamp = sources_stamp(&dirs);
        let t = Instant::now();
        match serge_web_dev::build_web(&ws, &app, &opts) {
            Ok(_) => {
                eprintln!(
                    "hyve-serge: rebuilt in {:.1}s, reloading {} page(s)",
                    t.elapsed().as_secs_f32(),
                    server.reload().listeners()
                );
                server.reload().reload();
            }
            Err(e) => eprintln!("hyve-serge: rebuild failed (the page keeps the last build): {e}"),
        }
    }
}
