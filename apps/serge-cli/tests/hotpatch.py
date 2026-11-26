"""Exercise native render edits at real window frame boundaries (macOS arm64)."""
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[3]


def source(value, typ, revision):
    return f'''use serge::prelude::*;
#[::serge::render_value]
fn render_value() -> u32 {{ {value}u32 }}
#[component]
fn Counter() -> Element {{
    let mut count = dev_signal("hotpatch", "counter", || 0{typ});
    count += 1;
    let label = move || {{
        let n = render_value();
        println!("RENDER {{}} {revision} {{n}} {{}}", std::process::id(), count());
        format!("{{n}} / {{}}", count())
    }};
    ui! {{ Text {{ "{{label()}}" }} }}
}}
fn main() {{ SergeApp::new().feature(feature().route("/", Counter)).run(); }}
'''


def run(exe):
    with tempfile.TemporaryDirectory(prefix="serge-render-test-") as temp:
        scratch = Path(temp)
        app = scratch / "app"
        (app / "src").mkdir(parents=True)
        (app / "Cargo.toml").write_text(f'''[package]
name = "render-test"
version = "0.1.0"
edition = "2024"
[workspace]
[package.metadata.serge]
web = true
[dependencies]
serge = {{path = "{ROOT / 'crates/serge'}"}}
hyve = {{path = "{ROOT.parent / 'hyve/crates/hyve'}"}}
''')
        (app / "hyve.toml").write_text("[serge]\nnative_hotpatch = true\n")
        main = app / "src/main.rs"
        main.write_text(source(17, "u32", "first"))
        env = dict(os.environ, HYVE_STAGE_LIFELINE="stdin", HYPERISH_RENDER_BACKEND="cpu")
        log = scratch / "stage.log"
        with log.open("w") as out, subprocess.Popen([exe, "dev", "--native"], cwd=app, env=env,
                stdin=subprocess.PIPE, stdout=out, stderr=out) as proc:
            def wait_for(pattern, start=0):
                until = time.monotonic() + 600
                while proc.poll() is None and time.monotonic() < until:
                    data = log.read_text(errors="replace")[start:]
                    if "native render hotpatch needs" in data:
                        raise RuntimeError(data[:300])
                    result = re.search(pattern, data)
                    if result: return result
                    time.sleep(.2)
                raise RuntimeError(log.read_text(errors="replace")[-5000:])
            try:
                original = wait_for(r"RENDER (\d+) first 17 1")[1]
                main.write_text(source(29, "u32", "first"))
                edit = wait_for(r"RENDER (\d+) first 29 (\d+)")
                updated = edit[1]
                assert edit[2] == "1", f"render edit remounted the view: {edit.group(0)}"
                assert original == updated, f"render edit restarted the window: {original} -> {updated}"
                print("live render 17 -> 29, same PID", original, flush=True)
                main.write_text("compile_error!(\"incomplete edit\");\n")
                wait_for("could not compile|native build failed")
                os.kill(int(original),0)
                main.write_text(source(29, "u32", "next"))
                rebuilt = wait_for(r"RENDER (\d+) next 29 2")[1]
                assert rebuilt != original
                main.write_text(source(29, "u64", "typed"))
                typed = wait_for(r"RENDER (\d+) typed 29 1")[1]
                assert typed != rebuilt
                print("invalid edit retained the window; full rebuild restored the counter; type change reset it", flush=True)
            finally:
                proc.stdin.close()
                try: proc.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait()


if __name__ == "__main__":
    run(str(Path(sys.argv[1]).resolve()))
