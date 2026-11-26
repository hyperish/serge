"""Build the generated Android library into an app and observe OS notification and widget updates."""
from pathlib import Path
import os
import shutil
import subprocess
import sys
import tempfile
import time

HERE = Path(__file__).resolve().parent
PACKAGE = "org.hyperish.widgetprobe"


def main(serial, generation):
    adb = [str(Path(os.environ["ANDROID_HOME"]) / "platform-tools/adb"), "-s", serial]

    def output(*args):
        return subprocess.check_output([*adb, *args], text=True).strip()

    assert serial.startswith("emulator-") and output("shell", "getprop", "ro.kernel.qemu") == "1"
    user = output("shell", "am", "get-current-user")
    assert user.isdigit()
    assert not output("shell", "pm", "list", "packages", PACKAGE)
    with tempfile.TemporaryDirectory(prefix="serge-android-widgets-") as scratch:
        project = Path(scratch)
        if (generation / "current").is_file():
            generation = generation / (generation / "current").read_text().strip()
        widget = project / "widgets"
        shutil.copytree(generation / "android", widget, ignore=shutil.ignore_patterns("build", ".gradle"))
        settings = (widget / "settings.gradle.kts").read_text()
        (project / "settings.gradle.kts").write_text(settings + '\ninclude(":widgets")\n')
        shutil.copy(widget / "gradle.properties", project)
        shutil.copy(HERE / "widget_host/build.gradle.kts", project)
        src = project / "src/main/java/org/hyperish/widgetprobe"
        src.mkdir(parents=True)
        shutil.copy(HERE / "widget_host/Probe.kt", src)
        (project / "src/main/AndroidManifest.xml").write_text(
            '<manifest xmlns:android="http://schemas.android.com/apk/res/android">'
            '<application android:label="Serge widget test"><activity android:name=".Probe" '
            'android:exported="true" /></application></manifest>')
        subprocess.run([os.environ.get("SERGE_ANDROID_GRADLE", "gradle"), "--no-daemon",
                        "--max-workers=2", ":assembleDebug"], cwd=project, check=True)
        apk, = (project / "build/outputs/apk/debug").glob("*.apk")
        subprocess.run([*adb, "install", apk], check=True)
        try:
            output("shell", "appwidget", "grantbind", "--package", PACKAGE, "--user", user)
            for expected in ["denied", "updated"]:
                output("shell", "run-as", PACKAGE, "rm", "-f", "files/result")
                if expected == "updated":
                    output("shell", "pm", "grant", PACKAGE, "android.permission.POST_NOTIFICATIONS")
                output("shell", "am", "start", "-W", "-n", PACKAGE + "/.Probe",
                       "--ez", "denied", str(expected == "denied").lower())
                deadline = time.monotonic() + 55
                while time.monotonic() < deadline:
                    result = subprocess.run([*adb, "shell", "run-as", PACKAGE, "cat", "files/result"],
                                            text=True, capture_output=True)
                    if result.returncode == 0 and result.stdout.strip():
                        assert result.stdout.strip() == expected, result.stdout
                        break
                    time.sleep(0.2)
                else:
                    raise RuntimeError(f"widget probe timed out waiting for {expected}")
        finally:
            subprocess.run([*adb, "shell", "appwidget", "revokebind", "--package", PACKAGE, "--user", user])
            subprocess.run([*adb, "uninstall", PACKAGE], check=True)
    print("Notification permission, native progress update/cancel and two visible Glance widget updates passed.")


if __name__ == "__main__":
    main(sys.argv[1], Path(sys.argv[2]).resolve())
