"""Exercise native Wayland windows and GTK children on the same private Weston desktop."""
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time


def check(runtime, codec):
    env = {**os.environ, "XDG_RUNTIME_DIR": str(runtime), "WAYLAND_DISPLAY": "serge-wayland",
           "XDG_SESSION_TYPE": "wayland", "HYPERISH_RENDER_BACKEND": "cpu"}
    for inherited in ("DISPLAY", "WAYLAND_SOCKET", "GDK_BACKEND"):
        env.pop(inherited, None)
    log = runtime / "weston.log"
    help_text = subprocess.check_output(["weston", "--help"], text=True)
    renderer = "--renderer=pixman" if "--renderer=" in help_text else "--use-pixman"
    # The surrounding Xvfb display provides a pointer and keyboard to the compositor.
    # The application receives the private Wayland socket and Weston's own XWayland display.
    parent = {**env, "DISPLAY": os.environ["DISPLAY"]}
    with subprocess.Popen(["weston", "--backend=x11-backend.so", "--shell=kiosk-shell.so",
                           renderer, "--xwayland", "--width=1200", "--height=900",
                           "--no-config", "--idle-time=0", "--socket=" + env["WAYLAND_DISPLAY"],
                           "--log=" + str(log)], env=parent) as compositor:
        try:
            deadline = time.monotonic() + 15
            while True:
                output = log.read_text(errors="replace") if log.exists() else ""
                display = re.search(r"listening on display (:\d+)", output, re.IGNORECASE)
                if display and (runtime / env["WAYLAND_DISPLAY"]).is_socket():
                    break
                if compositor.poll() is not None or time.monotonic() > deadline:
                    raise RuntimeError(output or "Weston did not start")
                time.sleep(0.05)
            screen = subprocess.check_output(["xdotool", "search", "--sync", "--onlyvisible",
                                              "--class", "weston"], env=parent, text=True,
                                             timeout=10).splitlines()[0]
            subprocess.run(["xdotool", "windowfocus", screen, "mousemove", "--window", screen,
                            "150", "150", "click", "1"], env=parent, check=True)
            cargo = ["cargo", "test", "--locked", "-p", "serge-playground"]
            system = [*cargo, "--features", "desktop-system", "--test"]
            subprocess.run([*system, "windows"], check=True,
                           env={**env, "SERGE_TEST_WINDOWS": "1"})
            subprocess.run([*system, "status_notifier"],
                           check=True, env={**env, "SERGE_TEST_STATUS_NOTIFIER": "1"})
            subprocess.run([*system, "system_x11"], check=True,
                           env={**env, "GDK_BACKEND": "wayland", "SERGE_TEST_WAYLAND_MENU": "1"})
            env["DISPLAY"] = display[1]
            subprocess.run([*system, "system_x11", "--", "--menus-only"], check=True,
                           env={**env, "SERGE_TEST_X11": "1", "SERGE_TEST_WESTON_DISPLAY": parent["DISPLAY"],
                                "SERGE_TEST_WESTON_WINDOW": screen})
            for rail in ({}, {"SERGE_TEST_WEBVIEW_WASM": str(codec)}):
                subprocess.run([*cargo, "--features", "webview", "--test", "webview"],
                               check=True, env={**env, "SERGE_TEST_WEBVIEW": "1", **rail})
        except Exception:
            if log.exists():
                print(log.read_text(errors="replace"), file=sys.stderr)
            raise
        finally:
            compositor.terminate()


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="serge-wayland-") as runtime:
        check(Path(runtime), Path(sys.argv[1]).resolve(strict=True))
