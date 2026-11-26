"""Run native iOS services and tap the corresponding system UI on a private simulator."""
import json
import os
from pathlib import Path
import plistlib
import re
import shutil
import subprocess
import tempfile
import time

from ui_runner import project_for
from webview import HYVE, SERGE, installed_app, sim

HERE = Path(__file__).resolve().parent


def prepare(root, cli):
    app = root / "ios-sdk"
    subprocess.run([cli, "init", app.name, "--template", "serge-app"], cwd=root, check=True)
    manifest = app / "Cargo.toml"
    deps = f"""[dependencies]
hyve = {{ path = {json.dumps(str(HYVE / "crates/hyve"))} }}
serge = {{ path = {json.dumps(str(SERGE / "crates/serge"))}, features = ["native-system"] }}
raw-window-handle = "=0.6.2"
objc2-ui-kit = {{ version = "=0.3.2", default-features = false, features = ["std","UIView","UIWindow","UIResponder"] }}
"""
    source, count = re.subn(r"(?ms)^\[dependencies\]\n.*?(?=^\[)", lambda _: deps + "\n", manifest.read_text())
    assert count == 1
    manifest.write_text(source.replace("[package.metadata.serge]",
                                       '[package.metadata.serge]\nwidgets = ["src/platform_widgets.rs"]\n'
                                       'android_package = "org.example.iossdk"'))
    shutil.copyfile(SERGE / "apps/serge-playground/src/platform_widgets.rs", app / "src/platform_widgets.rs")
    shutil.copyfile(HERE / "sdk_fixture.rs", app / "src/main.rs")
    shutil.copyfile(HERE / "IntentProbe.swift", app / "ios/IntentProbe.swift")
    build = app / "ios/build-rust.sh"
    command = '"$HYVE_WIDGET_BRIDGE" -o "$object"'
    script = build.read_text()
    assert script.count(command) == 1
    build.write_text(script.replace(command, '"$HYVE_WIDGET_BRIDGE" "$SRCROOT/IntentProbe.swift" -o "$object"'))
    project = app / "ios/App.xcodeproj/project.pbxproj"
    project.write_text(project.read_text().replace("GENERATE_INFOPLIST_FILE = YES;",
        "GENERATE_INFOPLIST_FILE = YES; INFOPLIST_KEY_UIFileSharingEnabled = YES; "
        "INFOPLIST_KEY_LSSupportsOpeningDocumentsInPlace = YES; "
        'INFOPLIST_KEY_NSFaceIDUsageDescription = "Unlock a test project";'))
    return app


def receipt(data, key, timeout=25):
    path = data / "tmp" / f"serge-{key}.txt"
    until = time.monotonic() + timeout
    while not path.exists() and time.monotonic() < until:
        time.sleep(.1)
    if not path.is_file():
        raise RuntimeError(f"no application result for {key}")
    value = path.read_text()
    if value.startswith("error:"):
        raise RuntimeError(value)
    return value


def run_shortcut(root, udid):
    root.mkdir()
    sim("openurl", udid, "shortcuts://create-shortcut")
    project, args = project_for(root, HERE / "ShortcutsTest.swift", udid, root / "Shortcuts.xcresult")
    subprocess.run(args, cwd=project, check=True)
    expected = {"project_name": "Saved project", "progress": .75}
    until = time.monotonic() + 8
    while time.monotonic() < until:
        value = sim("pbpaste", udid)
        if value.startswith("{") and json.loads(value) == expected:
            return
        time.sleep(.2)
    raise AssertionError(f"Unexpected result copied by Shortcuts: {value!r}")


def verify_lock_screen(root, bundle):
    root.mkdir()
    with installed_app(bundle, match_xcode=True) as (udid, package, data, runtime):
        def launch(mode):
            sim("launch", udid, package, env=dict(os.environ, SIMCTL_CHILD_SERGE_IOS_PROBE=mode,
                                                 SIMCTL_CHILD_HYPERISH_RENDER_BACKEND="cpu"))

        launch("widgets")
        assert receipt(data, "widgets") == "published"
        project, args = project_for(root, HERE / "LockWidget.swift", udid, root / "LockWidget.xcresult")
        subprocess.run(args, cwd=project, check=True)
        for _ in range(5):
            subprocess.run(["xcrun", "simctl", "terminate", udid, package], capture_output=True)
            (data / "tmp/serge-widget-families.txt").unlink(missing_ok=True)
            launch("widget-families")
            families = json.loads(receipt(data, "widget-families"))
            if "accessoryRectangular" in families:
                print(f"PASS {runtime} WidgetKit reports ExportHome on the Lock Screen", flush=True)
                return
            time.sleep(1)
        raise AssertionError(f"WidgetKit did not report a Lock Screen widget: {families}")


def ui_suite(root, udid, package, data):
    results = Path(os.environ.get("SERGE_XCRESULT", root / "Platform.xcresult")).resolve()
    project, args = project_for(root, HERE / "PlatformTests.swift", udid, results)
    payload = project / "test.apns"
    payload.write_text(json.dumps({"aps": {"alert": {"title": "Serge export ready", "body": "Choose a project action"},
                                           "category": "project", "sound": "default"}}))
    with subprocess.Popen(args, cwd=project) as run:
        sent = False
        last_face = 0
        until = time.monotonic() + 720
        while run.poll() is None:
            if time.monotonic() > until:
                run.terminate()
                raise TimeoutError("iOS UI test suite exceeded 12 minutes")
            grant = data / "tmp/serge-push.txt"
            if not sent and grant.exists() and grant.read_text() == "allowed":
                sim("push", udid, package, str(payload))
                sent = True
            # The first scan can occur while the system permission sheet is still visible.
            if (data / "tmp/serge-biometric-ready.txt").exists() and not (data / "tmp/serge-biometric.txt").exists() and time.monotonic() - last_face > 1:
                subprocess.run(["applesimutils", "--byId", udid, "--biometricMatch"], check=True, stdout=subprocess.DEVNULL)
                last_face = time.monotonic()
            time.sleep(.2)
        return run.returncode


def verify(root):
    cli = os.environ.get("HYVE", str(HYVE / "target/debug/hyve"))
    app = prepare(root, cli)
    subprocess.run([cli, "build", "--debug", "--format", "ios-sim"], cwd=app, check=True)
    bundle = app / "target/debug/ios/ios-sdk/simulator/ios-sdk.app"
    info = plistlib.loads((bundle / "Info.plist").read_bytes())
    failures = []
    with installed_app(bundle, match_xcode=True) as (udid, package, data, runtime):
        def check(name, operation):
            try:
                operation()
            except (OSError, ValueError, RuntimeError, AssertionError, subprocess.SubprocessError) as exc:
                failures.append(f"{name}: {exc!r}")
                print(f"FAIL {name}: {exc!r}", flush=True)
            else:
                print(f"PASS {runtime} {name}", flush=True)

        def native(mode):
            subprocess.run(["xcrun", "simctl", "terminate", udid, package], capture_output=True)
            sim("launch", udid, package, env=dict(os.environ, SIMCTL_CHILD_SERGE_IOS_PROBE=mode,
                                                  SIMCTL_CHILD_HYPERISH_RENDER_BACKEND="cpu"))
            if mode == "widgets":
                assert receipt(data, mode) == "published"
                group = Path(sim("get_app_container", udid, package, info["SergeWidgetAppGroup"]))
                state = json.loads((group / "serge-widget-ExportHome.json").read_text())
                assert state == {"project_name": "Saved project", "progress": .75}, state
            elif mode == "background":
                assert receipt(data, "background-ready") == "active"
                sim("launch", udid, "com.apple.Preferences")
                assert receipt(data, mode) == "saved after entering background"
            elif mode == "activity":
                outcome = receipt(data, mode)
                assert outcome.startswith("ended ") and outcome[6:].strip(), outcome
            elif mode == "intent":
                assert json.loads(receipt(data, mode)) == {"project_name": "Saved project", "progress": .75}
            elif mode == "widget-families":
                families = json.loads(receipt(data, mode))
                assert "systemSmall" in families, families
            elif mode == "clipboard":
                assert receipt(data, mode) == "clipboard and haptic request"
                assert sim("pbpaste", udid) == "Serge system clipboard"
            elif mode == "biometric-none":
                assert receipt(data, mode) == "unavailable"

        for mode in ["clipboard", "widgets", "intent", "activity", "background", "biometric-none"]:
            check(mode, lambda m=mode: native(m))
        subprocess.run(["xcrun", "simctl", "terminate", udid, package], capture_output=True)
        def run_ui():
            subprocess.run(["applesimutils", "--byId", udid, "--biometricEnrollment", "YES"], check=True)
            code = ui_suite(root, udid, package, data)
            assert code == 0, f"xcodebuild test exited {code}"
        check("XCUITest", run_ui)
        check("installed Home widget family", lambda: native("widget-families"))
        # Shortcuts indexes the intent metadata of the installed widget extension.
        check("Shortcuts editor and clipboard output", lambda: run_shortcut(root / "shortcuts", udid))
        for mode, expected in {"action": r"Index\(0\)", "date": r'Selected\("\d{4}-\d\d-\d\d"\)',
                               "time": "Cancelled", "share": "presented", "export": "serge-export.txt",
                               "import": "Serge document bytes", "import-cancel": "cancelled", "read-shared": "Serge shared project",
                               "push": "allowed", "push-action": "archive", "biometric": "matched"}.items():
            def result(key=mode, pattern=expected):
                value = receipt(data, key, timeout=1)
                assert re.fullmatch(pattern, value), value
            check(mode, result)
    if failures:
        raise RuntimeError("\n".join(failures))
    verify_lock_screen(root / "lock-screen", bundle)


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="serge-ios-sdk-") as scratch:
        verify(Path(scratch))
