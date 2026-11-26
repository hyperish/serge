"""Drive the real playground with ChromeDriver's WebDriver HTTP API."""

import json
import os
import re
import subprocess
import sys
from urllib.error import HTTPError
from urllib.request import Request, urlopen


def request(url, method, payload=None):
    data = None if payload is None else json.dumps(payload).encode()
    req = Request(url, data, {"Content-Type": "application/json"}, method=method)
    try:
        with urlopen(req, timeout=30) as response:
            return json.load(response)["value"]
    except HTTPError as error:
        raise RuntimeError(error.read().decode()) from error


def exercise(driver, page, prerendered=False):
    options = {"args": ["--headless", "--disable-dev-shm-usage"]}
    if binary := os.environ.get("CHROME_BIN"):
        options["binary"] = binary
    caps = {"browserName": "chrome", "goog:chromeOptions": options}
    session = request(driver + "/session", "POST", {"capabilities": {"alwaysMatch": caps}})
    url = driver + "/session/" + session["sessionId"]

    def post(path, data):
        return request(url + path, "POST", data)

    def element(xpath):
        found = post("/element", {"using": "xpath", "value": xpath})
        return "/element/" + found["element-6066-11e4-a52e-4f735466cecf"]

    def click(tag, label):
        post(element(f"//{tag}[normalize-space(.)='{label}']") + "/click", {})

    def text(label):
        element(f"//div[@class='serge-text' and normalize-space(.)='{label}']")

    def save(title):
        post(element("//input[@placeholder='Project title']") + "/value", {"text": title})
        click("button", "Save")

    try:
        post("/timeouts", {"implicit": 10000, "pageLoad": 20000})
        if prerendered:
            # Capture a real server-rendered node before the Wasm module starts.
            post("/goog/cdp/execute", {"cmd": "Page.addScriptToEvaluateOnNewDocument", "params": {
                "source": """new MutationObserver((_, obs) => {
                    const link = document.querySelector('a[data-serge-node]');
                    if (link) { window.firstServerLink = link; obs.disconnect(); }
                }).observe(document, {subtree:true, childList:true});"""
            }})
        post("/url", {"url": page})
        if prerendered:
            element("//div[@id='serge-root' and not(@data-serge-prerendered)]")
            assert post("/execute/sync", {"script": "return window.firstServerLink?.isConnected === true", "args": []})

        click("a", "Saved")
        click("button", "Sign in")
        # Wait for the authenticated screen before following another link.
        element("//button[normalize-space(.)='좋아요']")
        click("a", "Saved")
        text("No projects yet.")
        save("Browser project")
        text("Saved #1 on the server.")
        text("#1 Browser project")

        click("a", "Home")
        click("a", "Saved")
        text("#1 Browser project")

        # The UI is still signed in, but the backend no longer receives a
        # session cookie. A second save must fail before it writes to SQLite.
        request(url + "/cookie", "DELETE")
        save("Refused project")
        element("//div[@class='serge-text' and contains(., '(unauthenticated)')]")
    finally:
        request(url, "DELETE")


def main(page, prerendered=False):
    executable = os.environ.get("CHROMEDRIVER") or "chromedriver"
    with subprocess.Popen([executable, "--port=0"], stdout=subprocess.PIPE,
                          stderr=subprocess.STDOUT, text=True) as driver:
        try:
            for line in driver.stdout:
                ready = re.search(r"started successfully on port (\d+)", line)
                if ready:
                    exercise("http://127.0.0.1:" + ready[1], page, prerendered)
                    return
            raise RuntimeError("ChromeDriver exited before it announced its port")
        finally:
            driver.terminate()


if __name__ == "__main__":
    main(sys.argv[1])
