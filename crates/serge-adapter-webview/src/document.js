(() => {
  "use strict";
  if (window !== window.top) return;
  const key = "__SERGE_SLOT_KEY__";
  const send = data => window.ipc.postMessage(key + "/" + data);
  const storageKey = "serge.webview.pending.v1";
  const recoveryKey = "serge.webview.recovery.v1";
  let app, hello, rail, baseline, stateSeq, eventSeq, pending, timer, retries, stopped;
  const notify = (name, ...args) => app?.[name]?.(...args);
  const status = name => notify("onStatus", name);
  const decimal = s => typeof s === "string" && /^(0|[1-9][0-9]{0,19})$/.test(s) && BigInt(s) <= 18446744073709551615n;
  const ulid = () => {
    let bits = BigInt(Date.now());
    for (const b of crypto.getRandomValues(new Uint8Array(10))) bits = (bits << 8n) | BigInt(b);
    let s = "";
    for (let i = 0; i < 26; i++) {
      s = "0123456789ABCDEFGHJKMNPQRSTVWXYZ"[Number(bits & 31n)] + s;
      bits >>= 5n;
    }
    return s;
  };
  function stop(reason) {
    clearTimeout(timer);
    stopped = true;
    status(reason);
  }
  function reconnect() {
    if (sessionStorage.getItem(recoveryKey) === hello.session) return stop("disconnected");
    sessionStorage.setItem(recoveryKey, hello.session);
    stop("reconnecting");
    send("reload");
  }
  function raw(message) {
    const json = JSON.stringify({session: hello.session, generation: hello.generation, ...message});
    const data = rail === "json" ? json : app.codec.encode(json, app.fingerprint);
    if ((rail === "json" ? new TextEncoder().encode(data).length : data.length) > (rail === "json" ? 65536 : 87384)) {
      throw new Error("webview frame exceeds 64 KiB");
    }
    return data;
  }
  function transmit(message) { send(raw(message)); }
  function retry(task) {
    clearTimeout(timer);
    timer = setTimeout(() => {
      if (++retries > 3) return reconnect();
      task();
    }, 1000);
  }
  function submitPending() {
    transmit({kind: "event", eventSeq: pending.eventSeq, id: pending.id, payload: pending.payload});
    retry(submitPending);
  }
  function ready() {
    if (!app || !hello || stopped) return;
    if (app.buildId !== hello.buildId) return reconnect();
    if (hello.version !== 1) return stop("incompatible");
    rail = app.codec && app.fingerprint === hello.fingerprint ? "rkyv" : "json";
    const fingerprint = app.codec ? app.fingerprint : (hello.fingerprint === "0000000000000000" ? "0000000000000001" : "0000000000000000");
    send(JSON.stringify({...hello, kind: "ready", fingerprint, rail}));
    retry(ready);
  }
  function result(receipt) {
    if (!pending || receipt.eventSeq !== pending.eventSeq || receipt.id !== pending.id) return;
    clearTimeout(timer);
    if (receipt.outcome.kind === "pending") {
      retries = 0;
      retry(submitPending);
      return;
    }
    eventSeq = receipt.eventSeq;
    const finished = pending;
    sessionStorage.removeItem(storageKey);
    pending = undefined;
    status("ready");
    notify("onResult", finished.id, receipt.outcome);
  }
  function receive(data) {
    if (typeof data !== "string" || data.length > 87384 || stopped) return;
    let m = data.startsWith("{") ? JSON.parse(data) : undefined;
    if (m?.kind === "hello") {
      clearTimeout(timer);
      hello = m;
      baseline = true; stateSeq = "0"; eventSeq = "0"; retries = 0;
      const record = sessionStorage.getItem(storageKey);
      pending = record ? JSON.parse(record) : undefined;
      if (pending && pending.session !== hello.session) {
        notify("onUncertain", pending.id);
        sessionStorage.removeItem(storageKey);
        pending = undefined;
      }
      ready();
      return;
    }
    if (!hello || !rail) return;
    if (rail === "rkyv") m = JSON.parse(app.codec.decode(data, app.fingerprint));
    if (!m || m.session !== hello.session || m.generation !== hello.generation) return;
    if (m.kind === "eventResult" && !baseline) return result(m);
    if (m.kind !== "snapshot" || !decimal(m.stateSeq) || m.stateSeq === "0") return;
    const n = BigInt(m.stateSeq);
    if (!baseline && n <= BigInt(stateSeq)) return transmit({kind: "ackState", stateSeq});
    if (!baseline && n !== BigInt(stateSeq) + 1n) {
      baseline = true;
      return transmit({kind: "resync", lastStateSeq: stateSeq});
    }
    const restoring = baseline;
    notify("onState", m.state);
    stateSeq = m.stateSeq; baseline = false;
    transmit({kind: "ackState", stateSeq});
    if (!restoring) return;
    clearTimeout(timer);
    retries = 0;
    eventSeq = m.lastEvent?.eventSeq ?? "0";
    if (!pending) {
      sessionStorage.removeItem(recoveryKey);
      return status("ready");
    }
    status("busy");
    if (m.lastEvent?.id === pending.id && eventSeq === pending.eventSeq) {
      result(m.lastEvent);
    } else if (BigInt(pending.eventSeq) === BigInt(eventSeq) + 1n) {
      submitPending();
    } else {
      stop("uncertain");
    }
  }
  const api = Object.freeze({
    connect(options) {
      if (app) throw new Error("webview already connected");
      if (!options || !/^[0-7][0-9A-HJKMNP-TV-Z]{25}$/.test(options.buildId)) throw new Error("expected a build ULID");
      if (options.codec && !/^[0-9a-f]{16}$/.test(options.fingerprint)) throw new Error("expected a codec fingerprint");
      app = options; status("connecting"); ready();
    },
    submit(payload) {
      if (stopped || !hello || baseline || pending || !rail) throw new Error("webview is not ready for an event");
      const id = ulid(), next = (BigInt(eventSeq) + 1n).toString();
      if (!decimal(next)) throw new Error("webview session exhausted");
      const record = {session: hello.session, eventSeq: next, id, payload};
      const wire = raw({kind: "event", eventSeq: next, id, payload});
      sessionStorage.setItem(storageKey, JSON.stringify(record));
      pending = record; retries = 0;
      status("busy"); send(wire); retry(submitPending);
      return id;
    }
  });
  const heartbeat = setInterval(() => { if (rail && !stopped) send("tick"); }, 1000);
  Object.defineProperty(window, "sergeWebview", {value: api});
  Object.defineProperty(window, "__sergeWebview", {value: Object.freeze({receive, disconnect: () => stop("disconnected")})});
  window.addEventListener("pagehide", () => { clearInterval(heartbeat); clearTimeout(timer); stopped = true; });
})();
