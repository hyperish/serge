"""Exercise system trust, sensor/PCM rings and OS services in a disposable API 35 emulator."""
from contextlib import ExitStack
import midi_fixture
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import os
import json
import re
import xml.etree.ElementTree as ET
import select
import socket
import socketserver
import shutil
import ssl
import subprocess
import sys
import tempfile
import threading
import time
import uuid

SERGE = Path(__file__).resolve().parents[4]
PACKAGE = "org.hyperish.sdkprobe"
SDK = Path(os.environ["ANDROID_HOME"])
ADB = os.environ.get("ADB", str(SDK / "platform-tools/adb"))
ARCH = {"arm64-v8a": "aarch64-linux-android", "x86_64": "x86_64-linux-android"}


def run(*args, **kwargs):
    subprocess.run(args, check=True, **kwargs)


class Service(BaseHTTPRequestHandler):
    def do_GET(self):
        self.server.reads += 1
        self.send_response(200)
        self.end_headers()
        self.wfile.write(b"local fixture")

    def log_message(self, *args):
        pass


def relay(host, cleanup):
    class Forward(socketserver.BaseRequestHandler):
        def handle(self):
            with socket.create_connection((host, 443), timeout=8) as remote:
                sockets = [self.request, remote]
                while True:
                    ready, _, _ = select.select(sockets, [], [], 10)
                    if not ready:
                        return
                    for source in ready:
                        data = source.recv(16384)
                        if not data:
                            return
                        (remote if source is self.request else self.request).sendall(data)
    server = socketserver.ThreadingTCPServer(("127.0.0.1", 0), Forward)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    cleanup.callback(server.server_close)
    cleanup.callback(server.shutdown)
    return server.server_address[1]


def endpoint(directory, name, cleanup):
    cert, key = directory / (name + ".pem"), directory / (name + ".key")
    ca, ca_key = directory / (name + "-ca.pem"), directory / (name + "-ca.key")
    quiet = dict(stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    run("openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
        "-subj", "/CN=Serge fixture", "-addext", "basicConstraints=critical,CA:TRUE",
        "-keyout", ca_key, "-out", ca, **quiet)
    csr = directory / (name + ".csr")
    run("openssl", "req", "-new", "-newkey", "rsa:2048", "-nodes", "-subj", "/CN=localhost",
        "-keyout", key, "-out", csr, **quiet)
    extensions = directory / "server.ext"
    extensions.write_text("basicConstraints=CA:FALSE\nextendedKeyUsage=serverAuth\nsubjectAltName=DNS:localhost\n")
    run("openssl", "x509", "-req", "-in", csr, "-CA", ca, "-CAkey", ca_key, "-CAcreateserial",
        "-days", "1", "-extfile", extensions, "-out", cert, **quiet)
    server = ThreadingHTTPServer(("127.0.0.1", 0), Service)
    server.reads = 0
    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    ctx.load_cert_chain(cert, key)
    server.socket = ctx.wrap_socket(server.socket, server_side=True)
    worker = threading.Thread(target=server.serve_forever, daemon=True)
    worker.start()
    cleanup.callback(server.server_close)
    cleanup.callback(server.shutdown)
    return ca, server


def accepted(serial, public_host=None):
    def device(*args):
        return subprocess.check_output([ADB, "-s", serial, *args], stderr=subprocess.STDOUT).decode().strip()
    if device("shell", "getprop", "ro.kernel.qemu") != "1":
        raise ValueError("the SDK acceptance fixture requires an emulator")
    if device("shell", "pm", "list", "packages", PACKAGE):
        raise ValueError("this emulator already has the SDK fixture installed")
    abi = device("shell", "getprop", "ro.product.cpu.abi")
    with tempfile.TemporaryDirectory(prefix="serge-device-sdk-") as temp, ExitStack() as cleanup:
        root = Path(temp)
        cert, good = endpoint(root, "trusted", cleanup)
        _, bad = endpoint(root, "untrusted", cleanup)
        filename = "serge-" + uuid.uuid4().hex + ".txt"
        extra = {"SERGE_PROBE_DOCUMENT": filename}
        finger = os.environ.get("SERGE_ANDROID_FINGERPRINT")
        if finger:
            if finger not in {str(n) for n in range(1, 11)}:
                raise ValueError("SERGE_ANDROID_FINGERPRINT must name an enrolled emulator finger (1-10)")
            def fingerprint_counts():
                for line in device("shell", "dumpsys", "fingerprint").splitlines():
                    if line.startswith("{"):
                        for user in json.loads(line).get("prints", []):
                            if user["id"] == 0:
                                return user
                raise ValueError("missing user 0 fingerprint sensor stats")
            before = fingerprint_counts()
            if before["count"] != 1:
                raise ValueError("the fixture needs exactly one enrolled test fingerprint")
            extra["SERGE_PROBE_FINGERPRINT"] = finger
        peer = os.environ.get("SERGE_ANDROID_BLE_PEER")
        if peer:
            if peer == serial or subprocess.check_output([ADB, "-s", peer, "shell", "getprop", "ro.kernel.qemu"]).strip() != b"1":
                raise ValueError("the GATT fixture requires a second independent emulator")
            extra["SERGE_PROBE_GATT"] = str(uuid.uuid4())
        foldable = "HALF_OPENED" in device("shell", "cmd", "device_state", "print-states")
        if foldable:
            original = device("emu", "sensor", "get", "hinge-angle0").splitlines()[0].split("=")[1].strip()
            device("emu", "sensor", "set", "hinge-angle0", "180")
            cleanup.callback(device, "emu", "sensor", "set", "hinge-angle0", original)
            extra["SERGE_PROBE_FOLD"] = "1"
        if public_host:
            port = relay(public_host, cleanup)
            mapping = f"tcp:{port}"
            device("reverse", "--no-rebind", mapping, mapping)
            cleanup.callback(device, "reverse", "--remove", mapping)
            extra["SERGE_PROBE_PUBLIC"] = f"{public_host}:{port}"
        ports = []
        for server in [good, bad]:
            port = server.server_port
            mapping = f"tcp:{port}"
            device("reverse", "--no-rebind", mapping, mapping)
            cleanup.callback(device, "reverse", "--remove", mapping)
            ports.append(str(port))
        app = root / "app"
        (app / "src").mkdir(parents=True)
        shutil.copyfile(Path(__file__).with_name("sdk_probe.rs"), app / "src/lib.rs")
        shutil.copyfile(Path(__file__).with_name("camera.c"), app / "camera.c")
        shutil.copyfile(Path(__file__).with_name("vulkan_probe.rs"), app / "src/vulkan_probe.rs")
        tests = Path(__file__).resolve().parent.parent
        shutil.copyfile(tests / "android/webview_probe.rs", app / "src/webview_probe.rs")
        shutil.copyfile(tests / "support/webview_document.rs", app / "src/webview_document.rs")
        shutil.copyfile(tests / "browser/webview.js", app / "src/webview.js")
        codec = os.environ.get("SERGE_TEST_WEBVIEW_WASM")
        for name in ["webview_codec.js", "webview_codec_bg.wasm"]:
            (app / "src" / name).write_bytes((Path(codec) / name).read_bytes() if codec else b"")
        shader = next((Path(os.environ["ANDROID_NDK_ROOT"]) / "shader-tools").glob("*/glslc"))
        run(shader, "--target-env=vulkan1.1", Path(__file__).with_name("sample_camera.comp"), "-o", app / "src/sample_camera.spv")
        android_ns = "http://schemas.android.com/apk/res/android"
        ET.register_namespace("android", android_ns)
        overlay = ET.parse(SERGE / "apps/serge-playground/android/AndroidManifest.xml")
        activity = overlay.find("./application/activity")
        link = ET.SubElement(activity, "intent-filter")
        for tag, name in [("action", "android.intent.action.VIEW"), ("category", "android.intent.category.DEFAULT"),
                          ("category", "android.intent.category.BROWSABLE")]:
            ET.SubElement(link, tag, {f"{{{android_ns}}}name": name})
        ET.SubElement(link, "data", {f"{{{android_ns}}}scheme": "serge-probe", f"{{{android_ns}}}host": "projects"})
        ET.SubElement(overlay.getroot(), "uses-permission", {
            f"{{{android_ns}}}name": "android.permission.BLUETOOTH_SCAN",
            f"{{{android_ns}}}usesPermissionFlags": "neverForLocation",
        })
        overlay.write(app / "overlay.xml", encoding="utf-8")
        (app / "build.rs").write_text('fn main(){ cc::Build::new().file("camera.c").compile("camera_probe"); println!("cargo:rustc-link-lib=EGL"); println!("cargo:rustc-link-lib=GLESv2"); }')
        run("openssl", "x509", "-in", cert, "-outform", "DER", "-out", app / "src/ca.der")
        (app / "Cargo.toml").write_text(f'''[package]
name="sdk-probe"
version="0.1.0"
edition="2024"
[workspace]
[build-dependencies]
cc="1"
[lib]
crate-type=["cdylib"]
[dependencies]
hyve={{path="{(SERGE.parent / "hyve/crates/hyve").as_posix()}"}}
serge-web-ssr={{path="{(SERGE / "crates/serge-web-ssr").as_posix()}"}}
serge-adapter-webview={{path="{(SERGE / "crates/serge-adapter-webview").as_posix()}",features=["native-host"]}}
serde_json="1"
serge-native-sys={{path="{(SERGE / "crates/serge-native-sys").as_posix()}",features=["midi","bluetooth","vulkan-camera"]}}
ash={{version="=0.38.0",default-features=false,features=["std","loaded","debug"]}}
tokio={{version="1",features=["rt","time"]}}
futures-util="0.3"
uuid="1"
serge-adapter-client={{path="{(SERGE / "crates/serge-adapter-client").as_posix()}"}}
android-activity={{version="=0.6.1",features=["native-activity"]}}
rustls={{version="=0.23.46",default-features=false,features=["std","ring","tls12"]}}
rustls-platform-verifier="=0.7.1"
[package.metadata.android]
package="{PACKAGE}"
build_targets=["{ARCH[abi]}"]
manifest_overlay="overlay.xml"
window_layout=true
document_picker=true
background_task=true
notifications=true
webview=true
[package.metadata.android.sdk]
min_sdk_version=29
target_sdk_version=35
[package.metadata.android.application]
label="Serge SDK acceptance"
[[package.metadata.android.uses_permission]]
name="android.permission.INTERNET"
[[package.metadata.android.uses_permission]]
name="android.permission.RECORD_AUDIO"
[[package.metadata.android.uses_permission]]
name="android.permission.VIBRATE"
[[package.metadata.android.uses_permission]]
name="android.permission.BLUETOOTH_SCAN"
[[package.metadata.android.uses_permission]]
name="android.permission.BLUETOOTH_CONNECT"
[[package.metadata.android.uses_permission]]
name="android.permission.CAMERA"
[[package.metadata.android.uses_permission]]
name="android.permission.USE_BIOMETRIC"
''')
        # A uniquely named fixture has its own staging directory, even during other APK builds.
        target = Path(os.environ.get("SERGE_ANDROID_PROBE_TARGET", root / "target")).resolve()
        run(os.environ.get("HYVE", "hyve"), "build", "--debug", "--format", "apk", cwd=app, env=dict(
            os.environ, **extra, CARGO_TARGET_DIR=str(target), SERGE_PROBE_GOOD=ports[0], SERGE_PROBE_BAD=ports[1]))
        built = target / "debug/apk"
        apk = built / "sdk-probe.apk"
        fixture = "org.hyperish.midifixture"
        assert not device("shell", "pm", "list", "packages", fixture).strip()
        fixture_apk = midi_fixture.build(root / "midi", SDK)
        device("install", "--no-incremental", fixture_apk)
        cleanup.callback(device, "uninstall", fixture)
        if peer:
            def remote(*args):
                return subprocess.check_output([ADB, "-s", peer, *args], text=True).strip()
            if remote("shell", "pm", "list", "packages", fixture):
                raise ValueError("the peer already has the fixture installed")
            remote("install", "--no-incremental", "-g", fixture_apk)
            cleanup.callback(remote, "uninstall", fixture)
            remote("shell", "am", "start", "-W", "-n", fixture + "/.GattFixture", "--es", "service", extra["SERGE_PROBE_GATT"])
        device("install", "--no-incremental", "-g", apk)
        cleanup.callback(device, "uninstall", PACKAGE)
        device("shell", "am", "start", "-W", "-a", "android.intent.action.VIEW", "-d",
               "serge-probe://projects/17?tab=notes", "-p", PACKAGE)
        dump = "/data/local/tmp/serge-sdk-" + str(os.getpid()) + ".xml"
        cleanup.callback(device, "shell", "rm", "-f", dump)
        pending_buttons = ["Second", "OK", "OK", "OK"]
        doc_steps = ["CREATE", "OPEN", "CANCEL"]
        # The disposable provider initially opens Downloads; remove only this run's unique fixture.
        cleanup.callback(device, "shell", "rm", "-f", "/sdcard/Download/" + filename)
        moved_hinge = False
        pen_sent = False
        shade_open = False
        action_clicked = False
        wrong_finger = matched_finger = biometric_cancelled = False
        sent_home = False
        resumed = False
        end = time.monotonic() + 135
        pid = device("shell", "pidof", PACKAGE)
        while time.monotonic() < end:
            log = device("logcat", "--pid", pid, "-d")
            if finger and not biometric_cancelled and "SERGE_BIOMETRIC_WAIT" in log:
                device("shell", "uiautomator", "dump", dump)
                tree = ET.fromstring(device("shell", "cat", dump))
                titles = {n.get("text") for n in tree.iter("node") if n.get("package") == "com.android.systemui"}
                if "Unlock project" in titles:
                    if not wrong_finger:
                        device("emu", "finger", "touch", "2" if finger == "1" else "1")
                        device("emu", "finger", "remove")
                        wrong_finger = True
                    elif not matched_finger and fingerprint_counts()["reject"] > before["reject"]:
                        device("emu", "finger", "touch", finger)
                        device("emu", "finger", "remove")
                        matched_finger = True
                if "SERGE_BIOMETRIC_CANCEL" in log and "Cancel verification" in titles:
                    device("shell", "input", "keyevent", "KEYCODE_BACK")
                    biometric_cancelled = True
            if not pen_sent and "SERGE_PEN_READY" in log:
                result = device("shell", "am", "instrument", "-w", fixture + "/.PenFixture")
                assert "pen=delivered" in result, result
                pen_sent = True
            if not action_clicked and "SERGE_NOTIFICATION_READY" in log:
                if not shade_open:
                    device("shell", "cmd", "statusbar", "expand-notifications")
                    shade_open = True
                device("shell", "uiautomator", "dump", dump)
                tree = ET.fromstring(device("shell", "cat", dump))
                for node in tree.iter("node"):
                    if node.get("text", "").casefold() == "archive project":
                        left, top, right, bottom = map(int, re.findall(r"-?\d+", node.get("bounds")))
                        device("shell", "input", "tap", str((left + right)//2), str((top + bottom)//2))
                        action_clicked = True
                        device("shell", "cmd", "statusbar", "collapse")
                        break
                else:
                    # A collapsed SystemUI row hides its action buttons. Expand this request only.
                    parents = {child: parent for parent in tree.iter() for child in parent}
                    row = next((n for n in tree.iter("node") if n.get("text") == "Serge project ready"), None)
                    while row is not None:
                        expander = next((n for n in row.iter("node") if n.get("resource-id", "").endswith("/expand_button")), None)
                        if expander is not None:
                            left, top, right, bottom = map(int, re.findall(r"-?\d+", expander.get("bounds")))
                            device("shell", "input", "tap", str((left + right)//2), str((top + bottom)//2))
                            break
                        row = parents.get(row)
            if not sent_home and "SERGE_BACKGROUND_READY" in log:
                service = device("shell", "dumpsys", "activity", "services", PACKAGE)
                assert "org.hyperish.serge.background.WorkService" in service and "isForeground=true" in service, service
                device("shell", "input", "keyevent", "KEYCODE_HOME")
                sent_home = True
            saved = re.search(r"SERGE_BACKGROUND_SAVED=(/[^\s]+)", log)
            if sent_home and saved and not resumed:
                assert device("shell", "run-as", PACKAGE, "cat", saved[1]) == "saved while the activity was stopped"
                device("shell", "am", "start", "-W", "--activity-reorder-to-front", "-n", PACKAGE + "/android.app.NativeActivity")
                resumed = True
            if foldable and not moved_hinge and "SERGE_PROBE_FOLD_WAIT" in log:
                device("emu", "sensor", "set", "hinge-angle0", "90")
                moved_hinge = True
            if "SERGE_PROBE_DIALOG" in log and pending_buttons:
                device("shell", "uiautomator", "dump", dump)
                tree = ET.fromstring(device("shell", "cat", dump))
                for button in tree.iter("node"):
                    if button.get("package") == PACKAGE and button.get("text") == pending_buttons[0]:
                        x, y, r, b = map(int, re.findall(r"-?\d+", button.get("bounds")))
                        device("shell", "input", "tap", str((x+r)//2), str((y+b)//2))
                        pending_buttons.pop(0)
                        break
            if doc_steps and "SERGE_PROBE_DOCUMENT_" + doc_steps[0] in log:
                device("shell", "uiautomator", "dump", dump)
                nodes = list(ET.fromstring(device("shell", "cat", dump)).iter("node"))
                provider = [n for n in nodes if n.get("package", "").endswith(".documentsui")]
                step = doc_steps[0]
                if step == "CANCEL" and provider:
                    device("shell", "input", "keyevent", "KEYCODE_BACK")
                    doc_steps.pop(0)
                else:
                    for item in provider:
                        if (step == "CREATE" and item.get("resource-id") == "android:id/button1") or (
                            step == "OPEN" and item.get("text") == filename
                        ):
                            x, y, right, bottom = map(int, re.findall(r"-?\d+", item.get("bounds")))
                            device("shell", "input", "tap", str((x+right)//2), str((y+bottom)//2))
                            doc_steps.pop(0)
                            break
            if "SERGE_SDK_PROBE_OK" in log:
                assert not pending_buttons and not doc_steps and good.reads == 1 and bad.reads == 0
                assert sent_home and resumed
                assert pen_sent and "SERGE_PEN_ACCEPTED" in log
                assert action_clicked and "SERGE_NOTIFICATION_ACCEPTED" in log
                if finger:
                    assert matched_finger and biometric_cancelled and "SERGE_BIOMETRIC_ACCEPTED" in log
                    assert fingerprint_counts()["accept"] > before["accept"]
                    print("Android SystemUI rejected an unregistered finger, authenticated the enrolled finger and returned cancellation for Back.")
                print("Android SystemUI displayed the replacement notification and delivered its selected command exactly once.")
                print("Android delivered pen and eraser strokes with position, pressure, tilt, lift and cancellation, and excluded finger input.")
                assert "SERGE_WEBVIEW_ACCEPTED_JSON" in log
                if codec:
                    assert "SERGE_WEBVIEW_ACCEPTED_ARCHIVE" in log
                print("Android System WebView restored the committed edit after reopening, rendered the new state, enforced the guard, and blocked inline scripts on " + ("JSON and WASM/rkyv." if codec else "JSON."))
                assert "ServiceRecord{" not in device("shell", "dumpsys", "activity", "services", PACKAGE)
                print("Android promoted the short service, expired an overlapping request, saved the file after the activity stopped, and removed the service when its last handle closed.")
                print("Android trusted the configured local CA, refused an unknown issuer, and delivered sensor, microphone and clipboard data.")
                print("Android sampled two Camera2 frames as external GPU textures, started/stopped a filtered BLE scan, and exchanged MIDI notes through a separate OS service.")
                if "SERGE_VULKAN_CAMERA_SAMPLED" in log:
                    print("Vulkan sampled two acquired Camera2 hardware buffers with a compute shader and read back the opaque, varying pixels.")
                if peer:
                    print("GATT discovered the other emulator's private service, read, subscribed, wrote, received the same bytes, unsubscribed and disconnected.")
                print("Android DocumentsUI created a selected document, reopened its exact bytes and reported a cancelled picker.")
                print("AndroidKeyStore signed with a non-exportable P-256 identity, verified it after reopening, rejected a changed message and preserved its replacement.")
                print("AndroidKeyStore replace/read/delete, action selection, alert, date and time pickers passed.")
                print("Android resolved the registered project link and the activity received its URI; the vibrator accepted a three-step waveform on supported hardware.")
                print("Jetpack delivered a window layout snapshot and Android confirmed the foreground NativeActivity entered picture-in-picture.")
                if foldable:
                    assert moved_hinge
                    print("Changing the virtual hinge from 180° to 90° produced a separating half-open window and a 90° NDK sensor sample.")
                if public_host:
                    print(f"Android verified {public_host} with the OS trust store.")
                return
            if "SERGE_SDK_PROBE_FAILED" in log or "panicked at" in log:
                break
            time.sleep(.3)
        raise RuntimeError(log[-5000:])


if __name__ == "__main__":
    accepted(*sys.argv[1:])
