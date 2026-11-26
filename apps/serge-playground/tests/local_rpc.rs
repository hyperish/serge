//! Generated calls over a real, separately launched local Orbic server.
#![cfg(all(feature = "ipc", any(unix, windows), not(target_arch = "wasm32")))]
#[allow(dead_code)]
mod common;
use common::{run, scope_with, signed_in};
use serge::RemoteTransport;
use serge_playground::{
    CreateProject, PlaygroundSession, ReportRequest, build_report, create_project_rpc,
    list_projects,
};
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn socket_server_enforces_guards_and_persists_binary_calls() {
    let root = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    let endpoint = {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        root.path().join("rpc")
    };
    #[cfg(windows)]
    let endpoint = std::path::PathBuf::from(format!(r"\\.\pipe\serge-{}", hyve::Ulid::new()));
    let db = root.path().join("state.sqlite");
    let mut server = Server(
        Command::new(env!("CARGO_BIN_EXE_playground-server"))
            .env("SERGE_PLAYGROUND_IPC", &endpoint)
            .env(
                "HYPERISH_ORBIC_DATABASE_URL",
                format!("sqlite:{}?mode=rwc", db.display()),
            )
            .env_remove("HYPERISH_ORBIC_CACHE_URL")
            .env_remove("SERGE_BUNDLE_DIR")
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let stdout = server.0.stdout.take().unwrap();
    let (send, recv) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = send.send(BufReader::new(stdout).lines().next());
    });
    let ready = recv
        .recv_timeout(Duration::from_secs(15))
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(ready.contains("local IPC"), "{ready}");
    let local = RemoteTransport::local(endpoint, "/v1");
    let guest = scope_with(local.clone().into_arc());
    let create = |title: &str| {
        create_project_rpc(CreateProject {
            title: title.into(),
        })
    };
    assert_eq!(
        run(&guest, || create("denied")).unwrap_err().code(),
        "unauthenticated"
    );
    let caller = signed_in(PlaygroundSession::remote(local.clone()));
    assert_eq!(
        run(&caller, || create("IPC project")).unwrap().title,
        "IPC project"
    );
    assert_eq!(
        run(&caller, || create("")).unwrap_err().code(),
        "invalid_request"
    );
    let fallback = signed_in(PlaygroundSession::remote(
        local.clone().claim_fingerprint(0),
    ));
    let titles: Vec<_> = run(&fallback, list_projects)
        .unwrap()
        .into_iter()
        .map(|p| p.title)
        .collect();
    assert_eq!(titles, ["IPC project"]);
    let cancel = scope_with(local.into_arc());
    let unmount = cancel.clone();
    serge::runtime().spawn(async move {
        tokio::time::sleep(Duration::from_millis(80)).await;
        unmount.dispose();
    });
    let start = Instant::now();
    assert_eq!(
        run(&cancel, || build_report(ReportRequest { millis: 5000 }))
            .unwrap_err()
            .code(),
        "cancelled"
    );
    assert!(start.elapsed() < Duration::from_secs(2));
}
