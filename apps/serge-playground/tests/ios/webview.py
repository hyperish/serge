"""Install a fresh app and run the JSON and archive slot scenarios in iOS WKWebView."""
import json
from contextlib import contextmanager
import os
import plistlib
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time

TESTS = Path(__file__).resolve().parents[1]
SERGE = TESTS.parents[2]
HYVE = SERGE.parent / "hyve"
sys.path.insert(0, str(HYVE / "apps/hyve-cli/tests"))
from ios_simulator import installed_app, sim
from ui_runner import project_for, signals


@contextmanager
def confirming_links(root, udid):
    project, args = project_for(root, TESTS / "ios/LinkConfirmation.swift", udid, root / "Links.xcresult",
                                host=TESTS / "ios/LinkHost.swift")
    runner = None
    with subprocess.Popen(args, cwd=project) as driver:
        try:
            # XCTest builds, signs, installs and starts the runner on a fresh
            # simulator before its first test can signal readiness.
            until = time.monotonic() + 600
            while time.monotonic() < until and driver.poll() is None:
                runner = signals(udid, "links-ready")
                if runner is not None:
                    break
                time.sleep(1)
            else:
                raise RuntimeError(f"the link UI test did not start (xcodebuild status {driver.poll()})")
            yield runner
            (runner / "links-finished").touch()
            if driver.wait(timeout=180):
                raise RuntimeError(f"the link UI test exited {driver.returncode}")
        finally:
            if driver.poll() is None:
                driver.terminate()
                try:
                    driver.wait(timeout=20)
                except subprocess.TimeoutExpired:
                    driver.kill()


def prepare(directory, cli):
    app = directory / "ios-slot"
    subprocess.run([cli, "init", app.name, "--template", "serge-app"], cwd=directory, check=True)
    manifest = app / "Cargo.toml"
    dependencies = f"""[dependencies]
hyve = {{ path = {json.dumps(str(HYVE / "crates/hyve"))} }}
serge = {{ path = {json.dumps(str(SERGE / "crates/serge"))}, features = ["webview", "native-system"] }}
serge-web-ssr = {{ path = {json.dumps(str(SERGE / "crates/serge-web-ssr"))} }}
tempfile = "3"
serde_json = "1"
"""
    updated, count = re.subn(r"(?ms)^\[dependencies\]\n.*?(?=^\[)", lambda _: dependencies + "\n",
                             manifest.read_text())
    assert count == 1
    manifest.write_text(updated)
    shutil.copyfile(TESTS / "webview.rs", app / "src/main.rs")
    for path in ["support/webview_document.rs", "browser/webview.js"]:
        dest = app / "src" / path
        dest.parent.mkdir(exist_ok=True)
        shutil.copyfile(TESTS / path, dest)
    # Custom schemes use the normal Xcode Info.plist setting.
    (app / "ios/Links.plist").write_bytes(plistlib.dumps({"CFBundleURLTypes": [
        {"CFBundleURLName": "org.example.ios-slot", "CFBundleURLSchemes": ["serge-slot"]}
    ]}))
    project = app / "ios/App.xcodeproj/project.pbxproj"
    old = 'INFOPLIST_FILE = "$(HYVE_WIDGET_HOST_PLIST)";'
    source = project.read_text()
    assert source.count(old) == 2
    project.write_text(source.replace(old, 'INFOPLIST_FILE = "Links.plist";'))
    return app


def verify(codec):
    cli = os.environ.get("HYVE", str(HYVE / "target/debug/hyve"))
    with tempfile.TemporaryDirectory(prefix="serge-ios-slot-") as directory:
        app = prepare(Path(directory), cli)
        subprocess.run([cli, "build", "--debug", "--format", "ios-sim"], cwd=app, check=True)
        bundle = app / "target/debug/ios/ios-slot/simulator/ios-slot.app"
        with installed_app(bundle, match_xcode=True) as (udid, package, data, runtime), confirming_links(Path(directory), udid) as runner:
            assets = data / "tmp/slot-codec"
            assets.mkdir()
            for name in ["webview_codec.js", "webview_codec_bg.wasm"]:
                shutil.copyfile(codec / name, assets / name)
            missing_links = []
            for rail in ["JSON", "archive"]:
                receipt = data / "tmp" / rail
                env = dict(os.environ, SIMCTL_CHILD_HYPERISH_RENDER_BACKEND="cpu",
                           SIMCTL_CHILD_SERGE_TEST_WEBVIEW="1", SIMCTL_CHILD_SERGE_TEST_WEBVIEW_RECEIPT=str(receipt))
                if rail == "archive":
                    env["SIMCTL_CHILD_SERGE_TEST_WEBVIEW_WASM"] = str(assets)
                stderr = data / "tmp" / f"webview-{rail}.log"
                sim("launch", f"--stderr={stderr}", udid, package, env=env)
                until = time.monotonic() + 45
                while time.monotonic() < until:
                    if receipt.exists() and receipt.read_bytes() == b"saved-once resized restored denied":
                        break
                    time.sleep(.2)
                else:
                    if stderr.is_file():
                        print(stderr.read_text(errors="replace")[-6000:], flush=True)
                    raise RuntimeError(f"iOS WKWebView did not complete the {rail} scenario")
                print(f"{runtime} WKWebView resized and recovered the guarded edit over {rail}", flush=True)
                link = f"serge-slot://projects/42?mode=edit&rail={rail}"
                pending = runner / "link.pending"
                pending.write_text(link)
                pending.replace(runner / f"open-{rail}")
                delivered = receipt.with_suffix(".url")
                # The foreground sender opens this URL from a button. UIKit
                # must resume the receiving app and deliver the original URL.
                until = time.monotonic() + 60
                while time.monotonic() < until:
                    if delivered.exists() and delivered.read_text() == link:
                        break
                    time.sleep(.1)
                else:
                    missing_links.append(rail)
                    if stderr.is_file():
                        print(stderr.read_text(errors="replace")[-4000:], flush=True)
                    predicate = 'process IN {"SpringBoard","lsd","ios-slot","frontboardd"} AND ' \
                                '(eventMessage CONTAINS[c] "openURL" OR eventMessage CONTAINS[c] "serge-slot" OR eventMessage CONTAINS[c] "open application")'
                    events = sim("spawn", udid, "log", "show", "--last", "1m", "--style", "compact",
                                 "--info", "--predicate", predicate)
                    print(events[-5000:], flush=True)
                    if artifacts := os.environ.get("SERGE_SIM_DIAGNOSTICS"):
                        target = Path(artifacts)
                        target.mkdir(parents=True, exist_ok=True)
                        sim("io", udid, "screenshot", str(target / f"link-{rail}.png"))
                sim("terminate", udid, package)
            if missing_links:
                raise RuntimeError("UIKit did not deliver the registered project URL over " + ", ".join(missing_links))


if __name__ == "__main__":
    verify(Path(sys.argv[1]).resolve())
