//! The dev server: index with the reload hook, SPA fallback, 404s for
//! missing files, MIME types, no escaping the bundle directory, the API
//! proxy, and reload events.
#![allow(missing_docs)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

use serge_web_dev::{DevServer, Proxy};

fn get(addr: &str, req: &str) -> (u16, String, String) {
    let mut s = TcpStream::connect(addr).unwrap();
    s.write_all(req.as_bytes()).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    let (head, body) = out.split_once("\r\n\r\n").unwrap_or((&out, ""));
    let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, head.to_owned(), body.to_owned())
}

fn bundle() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("serge-web-dev-test-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(
        dir.join("index.html"),
        "<html><body><div id=\"serge-root\"></div></body></html>",
    )
    .unwrap();
    std::fs::write(dir.join("app.js"), "export default 1;").unwrap();
    std::fs::write(dir.join("app_bg.wasm"), b"\0asm").unwrap();
    std::fs::write(dir.join("assets/logo.svg"), "<svg/>").unwrap();
    dir
}

/// An upstream API answering with what it received.
fn upstream() -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap().to_string();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            let mut len = 0;
            loop {
                let mut h = String::new();
                r.read_line(&mut h).unwrap();
                if h.trim().is_empty() {
                    break;
                }
                if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; len];
            r.read_exact(&mut body).unwrap();
            let echo = format!("{} {}", line.trim(), String::from_utf8_lossy(&body));
            let mut s = s;
            write!(s, "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{echo}", echo.len()).unwrap();
        }
    });
    addr
}

#[test]
fn serves_the_bundle_like_a_single_page_app() {
    let api = upstream();
    let server = DevServer::start(
        "127.0.0.1:0",
        bundle(),
        Some(Proxy::new("/v1", &format!("http://{api}"))),
    )
    .unwrap();
    let a = server.addr().to_string();

    let (st, _, body) = get(&a, "GET / HTTP/1.1\r\n\r\n");
    assert_eq!(st, 200);
    assert!(
        body.contains("serge-root") && body.contains("/__serge/events"),
        "{body}"
    );

    // App routes fall back to the page; missing files are 404s.
    let (st, _, body) = get(&a, "GET /projects/7?tab=a HTTP/1.1\r\n\r\n");
    assert_eq!(st, 200);
    assert!(body.contains("serge-root"));
    assert_eq!(get(&a, "GET /missing.png HTTP/1.1\r\n\r\n").0, 404);
    assert_eq!(get(&a, "GET /../Cargo.toml HTTP/1.1\r\n\r\n").0, 404);

    let (st, head, _) = get(&a, "GET /app_bg.wasm HTTP/1.1\r\n\r\n");
    assert_eq!(st, 200);
    assert!(head.contains("application/wasm"), "{head}");
    let (_, head, body) = get(&a, "GET /assets/logo.svg HTTP/1.1\r\n\r\n");
    assert!(head.contains("image/svg+xml") && body == "<svg/>");

    // The API prefix goes to the server, body included.
    let (st, _, body) = get(
        &a,
        "POST /v1/projects HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}",
    );
    assert_eq!(st, 200);
    assert_eq!(body, "POST /v1/projects HTTP/1.1 {}");
    // Only the prefix: `/v10` is an app route.
    assert!(
        get(&a, "GET /v10 HTTP/1.1\r\n\r\n")
            .2
            .contains("serge-root")
    );
}

#[test]
fn an_unreachable_api_is_a_502_and_pages_get_reload_events() {
    let dead = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .to_string();
    let server = DevServer::start("127.0.0.1:0", bundle(), Some(Proxy::new("v1", &dead))).unwrap();
    let a = server.addr().to_string();
    let (st, _, body) = get(&a, "GET /v1/x HTTP/1.1\r\n\r\n");
    assert_eq!(st, 502);
    assert!(body.contains("not reachable"), "{body}");

    let mut page = TcpStream::connect(&a).unwrap();
    page.write_all(b"GET /__serge/events HTTP/1.1\r\n\r\n")
        .unwrap();
    page.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    for _ in 0..100 {
        if server.reload().listeners() == 1 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    server.reload().reload();
    let mut r = BufReader::new(page);
    let mut seen = String::new();
    while !seen.contains("data: reload") {
        let mut line = String::new();
        if r.read_line(&mut line).unwrap() == 0 {
            break;
        }
        seen.push_str(&line);
    }
    assert!(
        seen.contains("text/event-stream") && seen.contains("data: reload"),
        "{seen}"
    );
}

#[test]
fn proxy_urls_name_their_prefix() {
    let p = Proxy::from_url("http://127.0.0.1:3000/v1");
    assert_eq!(
        (p.prefix.as_str(), p.target.as_str()),
        ("/v1", "127.0.0.1:3000")
    );
    let p = Proxy::from_url("http://localhost:9000/api/v2/");
    assert_eq!(
        (p.prefix.as_str(), p.target.as_str()),
        ("/api/v2", "localhost:9000")
    );
    let p = Proxy::from_url("127.0.0.1:3000");
    assert_eq!(
        (p.prefix.as_str(), p.target.as_str()),
        ("/v1", "127.0.0.1:3000")
    );
}
