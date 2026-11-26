"""Build the deployable playground, mount it with Orbic, and use it in Chrome."""
import json
import os
from pathlib import Path
import re
import sqlite3
import subprocess
import tempfile
import urllib.error
import urllib.request
from projects import main as exercise_projects

ROOT = Path(__file__).resolve().parents[4]

def main():
    meta = json.loads(subprocess.check_output(["cargo", "metadata", "--format-version", "1", "--no-deps"], cwd=ROOT))
    debug = Path(meta["target_directory"]) / "debug"
    subprocess.run(["cargo", "build", "--locked", "-p", "serge-cli"], cwd=ROOT, check=True)
    subprocess.run(["cargo", "build", "--locked", "-p", "serge-playground", "--features", "web-host",
                    "--bin", "playground-server"], cwd=ROOT, check=True)
    with tempfile.TemporaryDirectory(prefix="serge-site-") as scratch:
        bundle = Path(scratch) / "site"
        db = Path(scratch) / "projects.sqlite"
        subprocess.run([debug / "hyve-serge", "build", "--debug", "-p", "serge-playground", "--out", bundle],
                       cwd=ROOT, check=True)
        env = dict(os.environ, SERGE_BUNDLE_DIR=str(bundle), SERGE_PLAYGROUND_ADDR="127.0.0.1:0",
                   HYPERISH_ORBIC_DATABASE_URL=f"sqlite:{db}?mode=rwc")
        with subprocess.Popen([debug / "playground-server"], cwd=ROOT, env=env, stdout=subprocess.PIPE, text=True) as server:
            try:
                address = re.search(r"http://127.0.0.1:\d+", server.stdout.readline())[0]
                with urllib.request.urlopen(address + "/", timeout=5) as response:
                    html = response.read().decode()
                    assert response.headers["Cache-Control"] == "no-cache"
                    assert "data-serge-prerendered" in html and "Counter — the README example" in html
                try:
                    urllib.request.urlopen(address + "/projects/saved", timeout=5)
                    raise AssertionError("protected HTML was served without a session")
                except urllib.error.HTTPError as error:
                    assert error.code == 401
                exercise_projects(address + "/", prerendered=True)
                with sqlite3.connect(db) as conn:
                    assert conn.execute("select owner, title from projects").fetchall() == [("demo", "Browser project")]
                print("Generated SSG, hashed JS/Wasm, Chrome UI, server guard, and SQLite passed.")
            finally:
                server.terminate()

if __name__ == "__main__":
    main()
