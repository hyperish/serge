"""Exercise successful and failed rebuilds through the development CLI."""

import contextlib
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "apps/serge-playground/tests/browser"))
from projects import request


def until(action, timeout=150):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if result := action():
            return result
        time.sleep(0.15)
    raise RuntimeError("timed out waiting for a rebuild")


def code(revision, typ):
    return f'''use serge::prelude::*;
pub fn native() {{
    let mut count = dev_signal("counter", "value", || 0{typ});
    println!("CHECKPOINT {{}} {revision} {{}}", std::process::id(), count());
    count += 1;
    loop {{ std::thread::park(); }}
}}
#[component]
pub fn Counter() -> Element {{
    let mut count = dev_signal("counter", "value", || 0{typ});
    ui! {{ Card {{
        Text {{ "{revision} count={{count()}}" }}
        Button {{ text: "Increment", clicked => {{ count += 1; }} }}
    }} }}
}}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {{ SergeApp::new().feature(feature().route("/", Counter)).run(); }}
'''


@contextlib.contextmanager
def running(args, path, log, **extra):
    env = dict(os.environ, HYVE_STAGE_LIFELINE="stdin")
    env.setdefault("CARGO_TARGET_DIR", str(ROOT / "target"))
    with open(log, "w") as out, subprocess.Popen(args, cwd=path, env=env, stdin=subprocess.PIPE,
                                                 stdout=out, stderr=out, **extra) as proc:
        try:
            yield proc
        finally:
            proc.stdin.close()
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.terminate()
                proc.wait(timeout=10)


def exercise(target, path, exe):
    lib = path / "src/lib.rs"
    lib.write_text(code("first", "u32"))
    log = path / f"{target}.log"
    args = [exe, "dev", "--" + target]
    if target == "web":
        args += ["--port", "0"]
    with running(args, path, log) as stage:
        def output():
            value = log.read_text()
            if stage.poll() is not None:
                raise RuntimeError(value[-3000:])
            return value

        if target == "native":
            def ready(version, value):
                return until(lambda: re.search(rf"CHECKPOINT (\d+) {version} {value}\b", output()))
            first_pid = ready("first", 0)[1]
            lib.write_text("uncompilable fixture\n")
            until(lambda: "keeping the running window" in output())
            os.kill(int(first_pid), 0)
            lib.write_text(code("second", "u32"))
            second_pid = ready("second", 1)[1]
            assert first_pid != second_pid
            lib.write_text(code("third", "u64"))
            assert ready("third", 0)[1] != second_pid
            assert "fingerprint changed" in output()
            return

        address = until(lambda: re.search(r"serving .+ at (http://[^ ]+/)", output()))[1]
        driver_log = path / "driver.log"
        with running([os.environ.get("CHROMEDRIVER", "chromedriver"), "--port=0"], path, driver_log):
            port = until(lambda: re.search(r"started successfully on port (\d+)", driver_log.read_text()))[1]
            driver = f"http://127.0.0.1:{port}"
            options = {"args": ["--headless", "--disable-dev-shm-usage"]}
            if binary := os.environ.get("CHROME_BIN"):
                options["binary"] = binary
            caps = {"browserName": "chrome", "goog:chromeOptions": options}
            session = request(driver + "/session", "POST", {"capabilities": {"alwaysMatch": caps}})
            page = driver + "/session/" + session["sessionId"]
            try:
                request(page + "/url", "POST", {"url": address})
                def js(script):
                    return request(page + "/execute/sync", "POST", {"script": script, "args": []})
                def has(text):
                    return until(lambda: text in (js("return document.body?.innerText") or ""))
                has("first count=0")
                js("document.querySelector('button').click()")
                has("first count=1")
                lib.write_text("uncompilable fixture\n")
                until(lambda: "rebuild failed" in output())
                has("first count=1")
                lib.write_text(code("second", "u32"))
                has("second count=1")
                lib.write_text(code("third", "u64"))
                has("third count=0")
            finally:
                request(page, "DELETE")


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="serge-reload-") as scratch:
        path = Path(scratch)
        (path / "src").mkdir()
        (path / "Cargo.toml").write_text(f'''
[package]
name = "serge-reload-test"
version = "0.0.0"
edition = "2024"
[lib]
crate-type = ["cdylib", "rlib"]
[package.metadata.serge]
web = true
[dependencies]
serge = {{path = "{ROOT}/crates/serge"}}
hyve = {{path = "{ROOT.parent}/hyve/crates/hyve"}}
[target.'cfg(target_arch = "wasm32")'.dependencies]
wasm-bindgen = "=0.2.130"
[workspace]
''')
        (path / "src/main.rs").write_text("fn main() { serge_reload_test::native(); }\n")
        for target in ("native", "web"):
            exercise(target, path, sys.argv[1])
            print(f"{target} rebuild, last good build, and typed state restoration passed", flush=True)
