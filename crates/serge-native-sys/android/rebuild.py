"""Compile NativeServices for API 26+ with JDK 17 and Android build-tools 35.0.0."""
from pathlib import Path
import os
import subprocess
import tempfile

root = Path(__file__).resolve().parent
sdk = Path(os.environ["ANDROID_HOME"])
jdk = Path(os.environ["JAVA_HOME"]) / "bin"
android = sdk / "platforms/android-35/android.jar"
with tempfile.TemporaryDirectory() as work:
    work = Path(work)
    subprocess.run([jdk / "javac", "--release", "8", "-g:none", "-classpath", android, "-d", work, *sorted(root.glob("*.java"))], check=True)
    classes = sorted(work.rglob("*.class"))
    subprocess.run([jdk / "java", "-cp", sdk / "build-tools/35.0.0/lib/d8.jar", "com.android.tools.r8.D8",
                    "--release", "--min-api", "26", "--lib", android, "--output", work, *classes], check=True)
    (root / "services.dex").write_bytes((work / "classes.dex").read_bytes())
