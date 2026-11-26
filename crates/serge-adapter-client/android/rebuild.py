"""Rebuild the embedded verifier with JDK 17, Android SDK 35 and build-tools 35.0.0."""
from hashlib import sha256
from pathlib import Path
import os
import subprocess
import tempfile
from urllib.request import urlopen
from zipfile import ZipFile

ROOT = Path(__file__).resolve().parent
SDK = Path(os.environ["ANDROID_HOME"])
JAVA = Path(os.environ["JAVA_HOME"]) / "bin/java"
SOURCES = {
    "verifier.aar": (
        "https://raw.githubusercontent.com/rustls/rustls-platform-verifier/1aa691352a5e0c215210dfc9a3c2f2078639dc91/android-release-support/maven/org/rustls/rustls-platform-verifier/0.2.0/rustls-platform-verifier-0.2.0.aar",
        "aa021794230fbc2f0be355999e2cf67398dd563de066a2e17001ffbd0b69101b"),
    "kotlin.jar": (
        "https://repo.maven.apache.org/maven2/org/jetbrains/kotlin/kotlin-stdlib/2.2.10/kotlin-stdlib-2.2.10.jar",
        "9c67cc79efd6b9215b49d2a4308f5f3433537376c7c88e89bdd6729bd096e61a"),
}
with tempfile.TemporaryDirectory() as tmp:
    stage = Path(tmp)
    for name, (url, digest) in SOURCES.items():
        with urlopen(url, timeout=30) as source:
            data = source.read()
        if sha256(data).hexdigest() != digest:
            raise ValueError("source checksum mismatch for " + name)
        (stage / name).write_bytes(data)
    with ZipFile(stage / "verifier.aar") as archive:
        archive.extract("classes.jar", stage)
    out = stage / "dex"
    out.mkdir()
    subprocess.run([JAVA, "-cp", SDK / "build-tools/35.0.0/lib/d8.jar",
                    "com.android.tools.r8.R8", "--release", "--min-api", "26", "--lib",
                    SDK / "platforms/android-35/android.jar", "--pg-conf", ROOT / "keep.pro",
                    "--output", out, stage / "classes.jar", stage / "kotlin.jar"], check=True)
    (ROOT / "platform-verifier.dex").write_bytes((out / "classes.dex").read_bytes())
