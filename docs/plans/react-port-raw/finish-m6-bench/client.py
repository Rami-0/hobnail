# One process, N clients: each has a stream thread and a poller thread.
import gzip, http.client, json, os, re, secrets, socket, sys, threading, time
from urllib.parse import urljoin
host, port, clients, duration, outdir = sys.argv[1:]
port, n, dur = int(port), int(clients), float(duration)
HDR = {"Accept-Encoding": "gzip", "User-Agent": "hobnail-bench"}

def get(path, timeout=10):
    c = http.client.HTTPConnection(host, port, timeout=timeout)
    try:
        c.request("GET", path, headers=HDR)
        r = c.getresponse(); body = r.read()
        # Embedded assets come gzip-encoded when asked; read them as the page.
        if r.getheader("Content-Encoding") == "gzip": body = gzip.decompress(body)
        return r.status, body
    finally:
        c.close()

def authenticate():
    # One session for every box: an account keeps at most 20 sessions, so 25
    # boxes signing in separately would revoke the first ones.
    cookie_file = "/credentials/cookie"
    if os.path.isfile(cookie_file):
        HDR["Cookie"] = open(cookie_file).read()
        return
    authenticate_once()
    fd = os.open(cookie_file, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w") as saved:
        saved.write(HDR["Cookie"])

def authenticate_once():
    status, session = get("/api/v1/session")
    if status != 200:
        raise RuntimeError(f"session probe returned {status}")
    password_file = "/credentials/password"
    if os.path.isfile(password_file):
        with open(password_file) as saved:
            password = saved.read()
    else:
        password = secrets.token_urlsafe(24)
        fd = os.open(password_file, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(fd, "w") as saved:
            saved.write(password)
    path = "/api/login" if json.loads(session)["owner_exists"] else "/api/v1/onboarding/owner"
    conn = http.client.HTTPConnection(host, port, timeout=10)
    try:
        conn.request("POST", path, json.dumps({"username": "bench", "password": password}),
                     {"Origin": f"http://{host}:{port}", "Content-Type": "application/json"})
        response = conn.getresponse()
        if response.status not in (200, 201):
            raise RuntimeError(f"benchmark sign-in returned {response.status}")
        HDR["Cookie"] = response.getheader("Set-Cookie").split(";", 1)[0]
    finally:
        conn.close()

def discover():
    hist = "/api/history/system?metric=cpu&range=1h"
    stream = "/api/v1/stream?topics=containers,stats,system"
    state = "/api/v1/containers"
    page = "/app"
    pat = r'''["'(](/assets/[^"')\s?#]+)'''
    st, body = get(page)
    # Follow the built graph, including dynamic-import preload maps. Reading
    # index.html alone omits lazy routes and charts from the asset workload.
    assets = set(re.findall(pat, body.decode("utf-8", "replace")))
    pending = list(assets)
    references = re.compile(r"[`\"'(]((?:\./|/assets/|assets/)[A-Za-z0-9_.-]+\.(?:js|css))[`\"')]")
    while pending:
        asset = pending.pop()
        status, source = get(asset)
        if status != 200:
            raise RuntimeError(f"asset discovery {asset} returned {status}")
        for reference in references.findall(source.decode("utf-8", "replace")):
            path = urljoin(asset if reference.startswith(".") else "/", reference)
            if path.startswith("/assets/") and path not in assets:
                assets.add(path)
                pending.append(path)
    assets = [page] + sorted(assets)
    return {"stream": stream, "state": state, "history": hist, "assets": assets, "page_status": st}

authenticate()
ep = discover()
json.dump(ep, open(os.path.join(outdir, "endpoints.json"), "w"), indent=1)

class Client:
    def __init__(self, i):
        self.i = i; self.req = 0; self.err = 0; self.events = 0; self.lat = []; self.errs = []
        self.lock = threading.Lock(); self.connected = threading.Event(); self.sock = None
    def note_err(self, what):
        with self.lock:
            self.err += 1
            if len(self.errs) < 20: self.errs.append(what)
    def timed(self, path):
        t = time.perf_counter()
        try:
            st, _ = get(path)
            ms = (time.perf_counter() - t) * 1000
            with self.lock: self.req += 1; self.lat.append(ms)
            if st >= 400: self.note_err(f"{path} {st}")
        except Exception as e:
            with self.lock: self.req += 1
            self.note_err(f"{path} {type(e).__name__}: {e}")
    def stream(self, stop):
        while not stop.is_set():
            try:
                c = http.client.HTTPConnection(host, port, timeout=None)
                c.request("GET", ep["stream"], headers={"Accept": "text/event-stream", **HDR})
                r = c.getresponse()
                with self.lock: self.req += 1
                if r.status != 200:
                    self.note_err(f"stream {r.status}"); self.connected.set(); stop.wait(1); continue
                self.sock = c.sock; self.connected.set()
                data = False
                while not stop.is_set():
                    line = r.readline()
                    if not line: break
                    if line.startswith(b"data:"): data = True
                    elif line in (b"\n", b"\r\n"):
                        if data:
                            with self.lock: self.events += 1
                        data = False
                if not stop.is_set(): self.note_err("stream closed by the server")
            except Exception as e:
                if not stop.is_set(): self.note_err(f"stream {type(e).__name__}: {e}")
                self.connected.set()
                stop.wait(1)
    def poll(self, stop, t0):
        nxt = {"state": t0, "history": t0, "assets": t0}
        every = {"state": 2.0, "history": 5.0, "assets": 20.0}
        while not stop.is_set():
            now = time.monotonic()
            for k in ("assets", "state", "history"):
                if now >= nxt[k]:
                    if k == "assets":
                        for a in ep["assets"]: self.timed(a)
                    else: self.timed(ep[k])
                    nxt[k] += every[k]
            stop.wait(max(0.0, min(nxt.values()) - time.monotonic()))

clients = [Client(i) for i in range(n)]
stop = threading.Event()
threads = []
for c in clients:
    th = threading.Thread(target=c.stream, args=(stop,), daemon=True); th.start(); threads.append(th)
for c in clients: c.connected.wait(10)
open(os.path.join(outdir, "ready"), "w").write(str(time.time()))
t0 = time.monotonic()
for c in clients:
    # Spread the clients over the 2 s state period.
    th = threading.Thread(target=c.poll, args=(stop, t0 + 2.0 * c.i / n), daemon=True); th.start(); threads.append(th)
time.sleep(max(0.0, dur - (time.monotonic() - t0)))
stop.set()
for c in clients:
    try:
        if c.sock: c.sock.shutdown(socket.SHUT_RDWR)
    except Exception: pass
for th in threads: th.join(15)
for c in clients:
    with c.lock:
        json.dump({"client": c.i, "seconds": dur, "requests": c.req, "errors": c.err, "events": c.events,
                   "latency_ms": [round(x, 3) for x in c.lat], "error_samples": c.errs},
                  open(os.path.join(outdir, f"client-{c.i}.json"), "w"))
