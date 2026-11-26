"""Create an isolated Android MIDI device for a framework SDK probe."""
import os
from pathlib import Path
import shutil
import subprocess
import zipfile


def build(root, sdk):
    src = Path(__file__).with_name("midi_fixture")
    jar = sdk / "platforms/android-35/android.jar"
    tools = sdk / "build-tools/35.0.0"
    jdk = Path(os.environ["JAVA_HOME"]) / "bin"
    classes = root / "classes"
    res = root / "res/xml"
    classes.mkdir(parents=True)
    res.mkdir(parents=True)
    shutil.copy(src / "device.xml", res)
    def run(*cmd):
        subprocess.run(cmd, cwd=root, check=True, stdout=subprocess.DEVNULL)
    run(jdk / "javac", "--release", "8", "-classpath", jar, "-d", classes, *sorted(src.glob("*.java")))
    run(jdk / "java", "-cp", tools / "lib/d8.jar", "com.android.tools.r8.D8", "--min-api", "29",
        "--lib", jar, "--output", root, *sorted(classes.rglob("*.class")))
    run(tools / "aapt", "package", "-f", "-M", src / "AndroidManifest.xml", "-S", root / "res",
        "-I", jar, "-F", "unsigned.apk")
    with zipfile.ZipFile(root / "unsigned.apk", "a") as apk:
        apk.write(root / "classes.dex", "classes.dex")
    run(tools / "zipalign", "-f", "4", "unsigned.apk", "aligned.apk")
    run(jdk / "keytool", "-genkeypair", "-keyalg", "RSA", "-keystore", "test.p12", "-storepass",
        "android", "-keypass", "android", "-validity", "1", "-alias", "fixture", "-dname", "CN=Serge disposable test")
    run(tools / "apksigner", "sign", "--ks", root / "test.p12", "--ks-pass", "pass:android",
        "--out", "fixture.apk", "aligned.apk")
    return root / "fixture.apk"
