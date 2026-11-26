"""Exercise the compiled Serge room client and the public JSON rail in Chrome."""
import functools
import http.server
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import threading

from projects import request

ROOT = Path(__file__).resolve().parents[4]


def drive():
    fixture = json.load(sys.stdin)
    handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=os.environ["SERGE_ROOM_BUNDLE"])
    with http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler) as server:
        threading.Thread(target=server.serve_forever, daemon=True).start()
        driver = subprocess.Popen([os.environ.get("CHROMEDRIVER", "chromedriver"), "--port=0"],
                                  stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        try:
            for line in driver.stdout:
                found = re.search(r"started successfully on port (\d+)", line)
                if found:
                    break
            else:
                raise RuntimeError("ChromeDriver did not start")
            root = "http://127.0.0.1:" + found[1]
            options = {"args": ["--headless", "--disable-dev-shm-usage"]}
            if os.environ.get("CHROME_BIN"):
                options["binary"] = os.environ["CHROME_BIN"]
            session = request(root + "/session", "POST", {"capabilities": {"alwaysMatch": {
                "browserName": "chrome", "goog:chromeOptions": options}}})
            api = root + "/session/" + session["sessionId"]
            try:
                request(api + "/url", "POST", {"url": f"http://127.0.0.1:{server.server_port}/"})
                result = request(api + "/execute/async", "POST", {"args": [fixture], "script": """
                    const [config, done] = arguments;
                    (async () => {
                        const mod = await import('/room_browser.js');
                        await mod.default();
                        const result = await mod.exercise_room(config.udp, config.blocked, config.ws,
                            Uint8Array.from(config.pin), config.tickets.slice(0,3));
                        const values = await new Promise((resolve, reject) => {
                            const ws = new WebSocket(config.ws + '?ticket=' + config.tickets[3], 'hyve.room.json.v1');
                            const values = [];
                            ws.onerror = () => reject(Error('JSON room upgrade failed'));
                            ws.onclose = () => reject(Error('JSON room closed early'));
                            ws.onmessage = ({data}) => {
                                const msg = JSON.parse(data);
                                if (msg.type === 'hello') ws.send(JSON.stringify({type:'room.resume', id:'browser', payload:{lastSeq:0}}));
                                if (msg.type === 'counter.changed') values.push([msg.seq, msg.payload]);
                                if (msg.type === 'room.ready') { resolve(values); ws.close(); }
                            };
                        });
                        return {result, values};
                    })().then(done, e => done({error: String(e)}));
                """})
                assert result == {"result": "WebTransport, WebSocket, two commits", "values": [[1, 1], [2, 2]]}, result
                print("Chrome verified WebTransport, blocked-UDP binary WebSocket, and JSON replay.")
            finally:
                request(api, "DELETE")
        finally:
            server.shutdown()
            driver.terminate()
            driver.wait()


if __name__ == "__main__":
    if sys.argv[1:] == ["--drive"]:
        drive()
    else:
        with tempfile.TemporaryDirectory(prefix="serge-room-") as bundle:
            meta = json.loads(subprocess.check_output(["cargo", "metadata", "--no-deps", "--format-version", "1"], cwd=ROOT))
            subprocess.run(["cargo", "build", "--locked", "-p", "serge", "--features", "realtime", "--example",
                            "room_browser", "--target", "wasm32-unknown-unknown"], cwd=ROOT, check=True)
            wasm = Path(meta["target_directory"]) / "wasm32-unknown-unknown/debug/examples/room_browser.wasm"
            subprocess.run(["wasm-bindgen", "--target", "web", "--out-dir", bundle, wasm], check=True)
            Path(bundle, "index.html").write_text('<!doctype html><title>Room integration</title><link rel="icon" href="data:,">')
            subprocess.run(["cargo", "test", "--locked", "-p", "serge-playground", "--features", "realtime",
                            "--test", "realtime", "browser_room_transports", "--", "--ignored", "--nocapture"],
                           cwd=ROOT, check=True, env=dict(os.environ, SERGE_ROOM_BUNDLE=bundle))
