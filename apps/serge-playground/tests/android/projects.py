"""Playground acceptance on a selected Android emulator, using the OS accessibility tree and touch."""
import os
from pathlib import Path
import re
import sqlite3
import subprocess
import sys
import tempfile
import time
import xml.etree.ElementTree as ET

PKG = "org.hyperish.playground"
ACTIVITY = PKG + "/android.app.NativeActivity"


def exercise(adb, server_binary, apk):
    def device(*args):
        return subprocess.check_output([*adb, *args], stderr=subprocess.STDOUT)

    def launch():
        device("shell", "am", "start", "-W", "-a", "android.intent.action.MAIN",
               "-c", "android.intent.category.LAUNCHER", "-f", "0x10200000", "-n", ACTIVITY)

    dump = f"/data/local/tmp/serge-{os.getpid()}.xml"

    def center(item):
        left, top, right, bottom = map(int, re.findall(r"-?\d+", item.get("bounds")))
        if right > left and bottom > top:
            return (left + right) // 2, (top + bottom) // 2

    def node(label=None, widget=None):
        until = time.monotonic() + 45
        seen, foreground = [], []
        while time.monotonic() < until:
            # On a cold activity UIAutomator can report a missing root or an
            # idle timeout with exit status zero and leave the XML unwritten.
            report = device("shell", "uiautomator", "dump", dump).decode(errors="replace").strip()
            if f"dumped to: {dump}" not in report:
                time.sleep(.2)
                continue
            tree = ET.fromstring(device("shell", "cat", dump))
            foreground = sorted({(item.get("package", ""), item.get("text") or item.get("content-desc") or "")
                                 for item in tree.iter("node") if item.get("package") != PKG})
            # A cold Google APIs image can cover the activity with its launcher's ANR dialog.
            if ("android", "Pixel Launcher isn't responding") in foreground:
                wait = tree.find(".//node[@package='android'][@text='Wait']")
                if wait is not None and (point := center(wait)):
                    print("Waiting for Pixel Launcher to recover on the cold emulator", flush=True)
                    device("shell", "input", "tap", *map(str, point))
                    continue
            seen = []
            for item in tree.iter("node"):
                if item.get("package") != PKG:
                    continue
                name = " ".join((item.get("content-desc") or item.get("text") or "").split())
                seen.append((name, item.get("bounds")))
                if label is not None and label != name:
                    continue
                if widget is not None and item.get("class") != widget:
                    continue
                if point := center(item):
                    return point
        raise AssertionError(f"missing {label!r} {widget!r} in {seen}, system UI {foreground}, {report}")

    def tap(label=None, widget=None):
        x, y = node(label, widget)
        device("shell", "input", "tap", str(x), str(y))

    def type_title(title):
        tap(widget="android.widget.EditText")
        until = time.monotonic() + 30
        while time.monotonic() < until:
            windows = device("shell", "dumpsys", "window", "displays")
            if re.search(rb"\btype=ime\b[^\n]*\bvisible=true\b", windows):
                break
            time.sleep(.2)
        else:
            raise AssertionError("Android did not show the keyboard for the focused project title")
        device("shell", "input", "text", title)
        node(title, "android.widget.EditText")

    def start(db, port=0):
        env = dict(os.environ, HYPERISH_ORBIC_DATABASE_URL=f"sqlite:{db}?mode=rwc",
                   SERGE_PLAYGROUND_ADDR=f"127.0.0.1:{port}")
        server = subprocess.Popen([server_binary], stdout=subprocess.PIPE, text=True, env=env)
        line = server.stdout.readline()
        match = re.search(r"127\.0\.0\.1:(\d+)", line)
        if not match:
            server.terminate()
            server.wait()
            raise RuntimeError(line or "playground server failed to start")
        return server, int(match[1])

    # Use a dedicated emulator. Install this debug APK and reserve only the app's loopback port.
    with tempfile.TemporaryDirectory(prefix="serge-android-projects-") as tmp:
        db = Path(tmp) / "projects.db"
        server, port = start(db)
        reverse = False
        try:
            device("reverse", "--no-rebind", "tcp:3000", f"tcp:{port}")
            reverse = True
            device("install", "--no-incremental", "-r", apk)
            device("shell", "am", "force-stop", PKG)
            device("shell", "input", "keyevent", "KEYCODE_WAKEUP")
            device("shell", "wm", "dismiss-keyguard")
            device("shell", "am", "wait-for-broadcast-idle")
            launch()
            tap("Counter")
            tap("Increment", "android.widget.Button")
            node("Count: 1")
            pid = device("shell", "pidof", PKG)
            device("shell", "input", "keyevent", "KEYCODE_HOME")
            launch()
            node("Count: 1")
            assert device("shell", "pidof", PKG) == pid
            tap("Saved")
            tap("Sign in", "android.widget.Button")
            tap("Saved")
            node("No projects yet.")
            type_title("AndroidProject")
            tap("Save", "android.widget.Button")
            node("Saved #1 on the server.")
            node("#1 AndroidProject")
            tap("Home")
            tap("Saved")
            node("#1 AndroidProject")
            # A fresh backend retains the DB and has a new in-memory session store.
            server.terminate()
            server.wait(timeout=10)
            server, _ = start(db, port)
            type_title("RefusedProject")
            tap("Save", "android.widget.Button")
            node("The server refused: sign in to continue (unauthenticated)")
            with sqlite3.connect(db) as cx:
                assert cx.execute("SELECT owner,title FROM projects").fetchall() == [("demo", "AndroidProject")]
            print("Android touch, accessibility, IME, suspend/resume, guarded save, reload, refusal and SQLite passed.")
        except Exception as error:
            if isinstance(error, subprocess.CalledProcessError) and error.output:
                print(error.output.decode(errors="replace"), file=sys.stderr)
            for args in [("shell", "dumpsys", "activity", "exit-info", PKG),
                         ("logcat", "-d", "-s", "RustStdoutStderr", "AndroidRuntime", "libc", "DEBUG")]:
                log = subprocess.run([*adb, *args], capture_output=True, text=True, errors="replace")
                print("\n".join(log.stdout.splitlines()[-80:]), file=sys.stderr)
            raise
        finally:
            server.terminate()
            server.wait()
            if reverse:
                device("reverse", "--remove", "tcp:3000")
            device("shell", "rm", "-f", dump)


if __name__ == "__main__":
    if len(sys.argv) != 4 or not sys.argv[1].startswith("emulator-"):
        raise SystemExit("usage: projects.py emulator-SERIAL playground-server playground.apk")
    exercise([os.environ.get("ADB", "adb"), "-s", sys.argv[1]], sys.argv[2], sys.argv[3])
