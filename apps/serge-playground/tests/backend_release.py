"""Exercise two release servers with a fresh SQLite database and a private Valkey instance."""
from concurrent.futures import ThreadPoolExecutor
from contextlib import ExitStack
import json
import math
import os
from pathlib import Path
import re
import secrets
import selectors
import signal
import sqlite3
import statistics
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request


def request(base, route, method, token=None, value=None, status=200):
    headers = {"Content-Type": "application/json"}
    if token:
        headers["Authorization"] = "Bearer " + token
    body = None if value is None else json.dumps(value).encode()
    req = urllib.request.Request(base + route, body, headers, method=method)
    try:
        response = urllib.request.urlopen(req, timeout=10)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        result = response.read()
        assert response.status == status, (method, route, response.status, result[:200])
        if not result:
            return None
        return json.loads(result)["data" if status < 300 else "error"]


def exercise(binary, scratch, cleanup):
    password = secrets.token_hex(24)
    docker_env = dict(os.environ, REDISCLI_AUTH=password)
    container = subprocess.check_output(["docker", "run", "--rm", "-d", "-p", "127.0.0.1::6379",
        "--tmpfs", "/data", "-e", "REDISCLI_AUTH", "valkey/valkey:8.1.3-alpine", "sh", "-c",
        'exec valkey-server --save "" --appendonly no --requirepass "$REDISCLI_AUTH"'],
        text=True, env=docker_env).strip()
    cleanup.callback(subprocess.run, ["docker", "stop", container], stdout=subprocess.DEVNULL, check=True)
    address = subprocess.check_output(["docker", "port", container, "6379/tcp"], text=True).strip()
    for _ in range(50):
        if subprocess.run(["docker", "exec", container, "valkey-cli", "ping"], capture_output=True).stdout.strip() == b"PONG":
            break
        time.sleep(.1)
    else:
        raise RuntimeError("The disposable Valkey instance did not answer PING")
    db = scratch / "projects.sqlite"
    env = dict(os.environ, SERGE_PLAYGROUND_ADDR="127.0.0.1:0", HYPERISH_ORBIC_REPLICAS="2",
               HYPERISH_ORBIC_DATABASE_URL=f"sqlite:{db}?mode=rwc", HYPERISH_ORBIC_CACHE_NAMESPACE="release-test",
               HYPERISH_ORBIC_CACHE_URL=f"redis://:{password}@{address}/0")

    def stop(process):
        if process.poll() is None:
            process.send_signal(signal.SIGINT)
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()

    def start():
        proc = subprocess.Popen([binary], cwd=scratch, env=env, text=True, stdout=subprocess.PIPE)
        cleanup.callback(stop, proc)
        with selectors.DefaultSelector() as events:
            events.register(proc.stdout, selectors.EVENT_READ)
            if not events.select(timeout=20):
                raise RuntimeError("The release server did not announce its listening address")
            line = proc.stdout.readline()
        url = re.search(r"http://127\.0\.0\.1:\d+/v1", line)
        if not url:
            raise RuntimeError(f"release server failed to start ({proc.poll()})")
        return proc, url[0]

    replicas = [start(), start()]
    urls = [url for _, url in replicas]
    assert request(urls[0], "/projects", "POST", value={"title": "guest"}, status=401)["code"] == "unauthenticated"
    token = request(urls[0], "/demo/login", "POST", value=False)
    assert isinstance(token, str) and token
    invalid = request(urls[1], "/projects", "POST", token, {"title": ""}, status=400)
    assert invalid["code"] == "invalid_request" and invalid["details"]["fields"][0]["path"] == "$.title"
    n = 256
    def write(i):
        begin = time.perf_counter_ns()
        title = f"release-{i}"
        row = request(urls[i % 2], "/projects", "POST", token, {"title": title})
        assert row["title"] == title
        return row["id"], (time.perf_counter_ns() - begin) / 1e6

    begin = time.perf_counter()
    with ThreadPoolExecutor(max_workers=8) as pool:
        rows = list(pool.map(write, range(n)))
    elapsed = time.perf_counter() - begin
    assert len({row[0] for row in rows}) == n
    latencies = sorted(row[1] for row in rows)
    expected = {f"release-{i}" for i in range(n)}

    def check_rows(url):
        listed = request(url, "/projects", "GET", token)
        assert len(listed) == n and {r["title"] for r in listed} == expected
        assert [r["id"] for r in listed] == sorted(row[0] for row in rows)

    for url in urls:
        check_rows(url)
    stop(replicas[0][0])
    check_rows(urls[1])
    _, replacement = start()
    check_rows(replacement)
    request(replacement, "/demo/logout", "POST", token, status=204)
    request(urls[1], "/projects", "GET", token, status=401)
    with sqlite3.connect(db) as connection:
        assert connection.execute("SELECT COUNT(*) FROM projects WHERE owner='demo'").fetchone() == (n,)
        assert {v for v, in connection.execute("SELECT title FROM projects")} == expected
    print(f"{n} authenticated writes, 8 clients, 2 replicas: {n/elapsed:.1f} writes/s; "
          f"p50 {statistics.median(latencies):.2f} ms, p95 {latencies[math.ceil(n*.95)-1]:.2f} ms. "
          "Restart preserved all IDs and rows; cross-replica logout denied the same session.")


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="serge-release-") as directory, ExitStack() as stack:
        exercise(Path(sys.argv[1]).resolve(), Path(directory), stack)
