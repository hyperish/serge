"""Rebuild the optional Android BLE adapter from the checksum-verified published crate."""
import hashlib
import io
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import urllib.request

root = Path(__file__).resolve().parent
sdk = Path(os.environ["ANDROID_HOME"])
jdk = Path(os.environ["JAVA_HOME"]) / "bin"
jar = sdk / "platforms/android-35/android.jar"
version = "0.13.5"
digest = "3d836502de731602ea87cbb307065e8feaed4fd10f0855a75b0b87944ee011dc"
cached = list((Path(os.environ.get("CARGO_HOME", Path.home()/".cargo"))/"registry/cache").glob(f"*/btleplug-{version}.crate"))
archive = cached[0].read_bytes() if cached else urllib.request.urlopen(
    f"https://static.crates.io/crates/btleplug/btleplug-{version}.crate", timeout=30).read()
assert hashlib.sha256(archive).hexdigest() == digest, "unexpected btleplug archive"
with tempfile.TemporaryDirectory() as work:
    work = Path(work)
    src, out = work/"src", work/"classes"
    src.mkdir()
    out.mkdir()
    prefix = f"btleplug-{version}/src/droidplug/java/src/main/java/"
    sources = [root/"BleBootstrap.java"]
    with tarfile.open(fileobj=io.BytesIO(archive)) as package:
        for f in package:
            if f.isfile() and f.name.startswith(prefix) and f.name.endswith(".java"):
                path = src/f.name[len(prefix):]
                path.parent.mkdir(parents=True,exist_ok=True)
                path.write_bytes(package.extractfile(f).read())
                sources.append(path)
    subprocess.run([jdk/"javac","--release","8","-g:none","-classpath",jar,"-d",out,*sorted(sources)],check=True)
    subprocess.run([jdk/"java","-cp",sdk/"build-tools/35.0.0/lib/d8.jar","com.android.tools.r8.D8","--release",
                    "--min-api","26","--lib",jar,"--output",work,*sorted(out.rglob("*.class"))],check=True)
    (root/"adapter.dex").write_bytes((work/"classes.dex").read_bytes())
