//! The dev server: static files with SPA fallback, a proxy for the API
//! prefix, and live reload (server-sent events) — std only, one thread per
//! connection, HTTP/1.1 with `Connection: close`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const RELOAD_SCRIPT: &str = r#"<script>
(() => {
  const es = new EventSource("/__serge/events");
  es.onmessage = (e) => { if (e.data === "reload") location.reload(); };
})();
</script>"#;

/// Forwards requests under `prefix` (`/v1`) to `target` (`127.0.0.1:3000`).
#[derive(Clone, Debug)]
pub struct Proxy {
    /// The path prefix.
    pub prefix: String,
    /// `host:port` of the API server.
    pub target: String,
}

impl Proxy {
    /// `http://127.0.0.1:3000/v1`: the URL's path is the prefix proxied to
    /// its origin (`/v1` when the URL has no path).
    pub fn from_url(url: &str) -> Proxy {
        let rest = url.trim_start_matches("http://").trim_end_matches('/');
        match rest.split_once('/') {
            Some((host, path)) => Proxy::new(path, host),
            None => Proxy::new("/v1", rest),
        }
    }

    /// Parses `http://host:port` (or `host:port`) for `prefix`.
    pub fn new(prefix: &str, target: &str) -> Proxy {
        let t = target.trim_start_matches("http://").trim_end_matches('/');
        Proxy {
            prefix: format!("/{}", prefix.trim_matches('/')),
            target: t.split('/').next().unwrap_or(t).to_owned(),
        }
    }
}

/// Connected pages to reload.
#[derive(Clone, Default, Debug)]
pub struct Reload(Arc<Mutex<Vec<TcpStream>>>);

impl Reload {
    /// Tells every connected page to reload.
    pub fn reload(&self) {
        let mut v = self.0.lock().unwrap_or_else(|e| e.into_inner());
        v.retain_mut(|s| {
            s.write_all(b"data: reload\n\n")
                .and_then(|()| s.flush())
                .is_ok()
        });
    }

    /// How many pages listen.
    pub fn listeners(&self) -> usize {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}

/// A running dev server.
#[derive(Debug)]
pub struct DevServer {
    addr: SocketAddr,
    reload: Reload,
    files: Arc<Mutex<Arc<Files>>>,
}

#[derive(Debug)]
enum Files {
    Directory(PathBuf),
    Build(tempfile::TempDir),
}

impl Files {
    fn path(&self) -> &Path {
        match self {
            Self::Directory(dir) => dir,
            Self::Build(dir) => dir.path(),
        }
    }
}

impl DevServer {
    /// Serves `dir` on `addr` (port 0 picks one) in background threads.
    pub fn start(addr: &str, dir: PathBuf, proxy: Option<Proxy>) -> std::io::Result<DevServer> {
        use std::hash::{DefaultHasher, Hash, Hasher};
        let mut key = DefaultHasher::new();
        dir.canonicalize()?.hash(&mut key);
        Self::listen(
            TcpListener::bind(addr)?,
            Files::Directory(dir),
            proxy,
            key.finish(),
        )
    }

    /// Serves a finished development build. The namespace identifies the
    /// workspace and app even when the output directory changes on rebuild.
    pub fn development(
        listener: TcpListener,
        build: tempfile::TempDir,
        proxy: Option<Proxy>,
        namespace: u64,
    ) -> std::io::Result<Self> {
        Self::listen(listener, Files::Build(build), proxy, namespace)
    }

    fn listen(
        listener: TcpListener,
        initial: Files,
        proxy: Option<Proxy>,
        namespace: u64,
    ) -> std::io::Result<Self> {
        let addr = listener.local_addr()?;
        let reload = Reload::default();
        let r = reload.clone();
        let files = Arc::new(Mutex::new(Arc::new(initial)));
        let current = files.clone();
        let proxy = Arc::new(proxy);
        std::thread::Builder::new()
            .name("serge-dev-server".into())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    let dir = current.lock().unwrap_or_else(|e| e.into_inner()).clone();
                    let (proxy, r) = (proxy.clone(), r.clone());
                    std::thread::spawn(move || {
                        let _ = handle(stream, dir.path(), proxy.as_ref().as_ref(), &r, namespace);
                    });
                }
            })?;
        Ok(DevServer {
            addr,
            reload,
            files,
        })
    }

    /// Activates one successful build. Readers of the previous directory keep
    /// it alive until their response completes.
    pub fn replace(&self, build: tempfile::TempDir) {
        *self.files.lock().unwrap_or_else(|e| e.into_inner()) = Arc::new(Files::Build(build));
        self.reload.reload();
    }

    /// The bound address.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The live-reload hub.
    pub fn reload(&self) -> &Reload {
        &self.reload
    }
}

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn read_request(stream: &TcpStream) -> std::io::Result<Request> {
    let mut r = BufReader::new(stream);
    let mut line = String::new();
    r.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_owned();
    let path = parts.next().unwrap_or("/").to_owned();
    let mut headers = Vec::new();
    loop {
        let mut h = String::new();
        if r.read_line(&mut h)? == 0 || h.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.push((k.trim().to_owned(), v.trim().to_owned()));
        }
    }
    let len = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0; len];
    r.read_exact(&mut body)?;
    Ok(Request {
        method,
        path,
        headers,
        body,
    })
}

fn respond(
    mut s: &TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> std::io::Result<()> {
    write!(
        s,
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    s.write_all(body)?;
    s.flush()
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "wasm" => "application/wasm",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// `dir` + the request path, refusing to leave `dir`.
fn file_for(dir: &Path, url_path: &str) -> Option<PathBuf> {
    let path = url_path.split(['?', '#']).next().unwrap_or("/");
    let mut out = dir.to_path_buf();
    for c in Path::new(path.trim_start_matches('/')).components() {
        match c {
            Component::Normal(p) => out.push(p),
            Component::CurDir => {}
            _ => return None,
        }
    }
    if out.is_dir() {
        out.push("index.html");
    }
    out.is_file().then_some(out)
}

fn serve_index(s: &TcpStream, dir: &Path, namespace: u64) -> std::io::Result<()> {
    match std::fs::read_to_string(dir.join("index.html")) {
        Ok(html) => {
            let marker =
                format!(r#"<meta name="serge-dev-session" content="dev-{namespace:016x}">"#);
            let html = match html.rfind("</body>") {
                Some(i) => format!("{}{marker}{RELOAD_SCRIPT}\n{}", &html[..i], &html[i..]),
                None => format!("{html}{marker}{RELOAD_SCRIPT}"),
            };
            respond(s, "200 OK", "text/html; charset=utf-8", html.as_bytes())
        }
        Err(_) => respond(
            s,
            "503 Service Unavailable",
            "text/plain",
            b"the bundle is not built yet",
        ),
    }
}

fn handle(
    stream: TcpStream,
    dir: &Path,
    proxy: Option<&Proxy>,
    reload: &Reload,
    namespace: u64,
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    let req = read_request(&stream)?;
    let path = req.path.split('?').next().unwrap_or("/").to_owned();
    if path == "/__serge/events" {
        let mut s = stream.try_clone()?;
        s.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: keep-alive\r\n\r\n: connected\n\n")?;
        s.flush()?;
        reload.0.lock().unwrap_or_else(|e| e.into_inner()).push(s);
        return Ok(());
    }
    if let Some(p) = proxy
        && (path == p.prefix || path.starts_with(&format!("{}/", p.prefix)))
    {
        return forward(&stream, &req, p);
    }
    if req.method != "GET" && req.method != "HEAD" {
        return respond(&stream, "405 Method Not Allowed", "text/plain", b"");
    }
    if path == "/" || path == "/index.html" {
        return serve_index(&stream, dir, namespace);
    }
    match file_for(dir, &path) {
        Some(f) => {
            let body = std::fs::read(&f)?;
            respond(&stream, "200 OK", content_type(&f), &body)
        }
        // A file-like path that does not exist is a 404; anything else is a
        // route of the app (SPA fallback).
        None if Path::new(&path).extension().is_some() => {
            respond(&stream, "404 Not Found", "text/plain", b"not found")
        }
        None => serve_index(&stream, dir, namespace),
    }
}

fn forward(client: &TcpStream, req: &Request, p: &Proxy) -> std::io::Result<()> {
    let mut up = match TcpStream::connect(&p.target) {
        Ok(s) => s,
        Err(e) => {
            let msg = format!("the API server at {} is not reachable: {e}", p.target);
            return respond(
                client,
                "502 Bad Gateway",
                "text/plain; charset=utf-8",
                msg.as_bytes(),
            );
        }
    };
    write!(up, "{} {} HTTP/1.1\r\n", req.method, req.path)?;
    for (k, v) in &req.headers {
        let skip = [
            "host",
            "connection",
            "keep-alive",
            "content-length",
            "accept-encoding",
        ];
        if !skip.iter().any(|s| k.eq_ignore_ascii_case(s)) {
            write!(up, "{k}: {v}\r\n")?;
        }
    }
    write!(
        up,
        "Host: {}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
        p.target,
        req.body.len()
    )?;
    up.write_all(&req.body)?;
    up.flush()?;
    let mut c = client.try_clone()?;
    std::io::copy(&mut up, &mut c)?;
    c.flush()
}
