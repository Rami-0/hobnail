import argparse
from collections import Counter, defaultdict
import csv
import gzip
import hashlib
import http.client
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import secrets
import socket
import sqlite3
import ssl
import statistics
import subprocess
import tempfile
import threading
import time
from urllib.parse import urlsplit, parse_qs

args = argparse.ArgumentParser()
args.add_argument("image")
args.add_argument("out")
args.add_argument("--mode", choices=["auth", "full", "setup", "probe"], default="full")
args.add_argument("--hard", action="store_true")
args.add_argument("--port", default=3396, type=int, choices=range(3395, 3400))
args.add_argument("--repeat", type=int, default=1)
args.add_argument("--long", type=int, default=60)
args.add_argument("--legacy-image")
args.add_argument("--maintenance-trace", action="store_true")
args.add_argument("--steady-history", action="store_true")
args.add_argument("--cold-db", action="store_true")
opts = args.parse_args()
root = Path(__file__).parent
out = root / opts.out
out.mkdir()
pwd = secrets.token_urlsafe(24)
credential = dict(username="bench-owner", password=pwd)
port = opts.port
origin = f"http://127.0.0.1:{port}"
network = "host"
container = f"hobnail-m5-run-{port}"
node = Path("Dockerfile").read_text().splitlines()
node = next(line.split()[2] for line in node if line.startswith("FROM --platform") and "node:" in line)
lock = threading.Lock()
metrics = defaultdict(list)
upstream = Counter()
fake_errors = Counter()
sse_events = Counter()
sse_sockets = {}
sse_connections = Counter()
widget_status = Counter()
state = dict(phase="startup", interval=.5)
sessions = ("", "")
last_cgroup = None

def append(filename, record):
    with lock, (out / filename).open("a") as f:
        f.write(json.dumps(record) + "\n")
        f.flush()
        os.fsync(f.fileno())

def run(*cmd, **kw):
    return subprocess.check_output(cmd, text=True, **kw).strip()

def pairs(path):
    return {key: int(value) for key, value in (line.split() for line in path.read_text().splitlines())}

def sample(cg):
    stats = pairs(cg / "memory.stat")
    events = pairs(cg / "memory.events")
    return dict(ts=time.monotonic(), phase=state["phase"],
                current=int((cg / "memory.current").read_text()),
                peak=int((cg / "memory.peak").read_text()),
                cpu_us=pairs(cg / "cpu.stat")["usage_usec"], **events,
                **{k: stats[k] for k in ("anon", "file", "kernel", "sock", "file_mapped", "inactive_file")})

def record_sample(cg):
    row = sample(cg)
    row["ws"] = max(0, row["current"] - row["inactive_file"])
    if opts.maintenance_trace:
        for key, suffix in (("db_bytes", ""), ("wal_bytes", "-wal"), ("shm_bytes", "-shm")):
            try:
                row[key] = (scratch/"data"/("hobnail.db"+suffix)).stat().st_size
            except FileNotFoundError:
                row[key] = 0
    append("samples.jsonl", row)
    return row

def sampler(stop, cg):
    while not stop.is_set():
        try:
            record_sample(cg)
        except FileNotFoundError:
            return
        stop.wait(state["interval"])

def request(method, path, data=None, expected=200, connection=None, auth=True, check=True):
    c = connection or http.client.HTTPConnection("127.0.0.1", port, timeout=8)
    headers = {"Origin": origin, "Content-Type": "application/json", "Accept-Encoding": "gzip"}
    if auth and sessions[0]:
        headers.update({"Cookie": sessions[0], "X-RH-CSRF": sessions[1]})
    phase = state["phase"]
    t0 = time.monotonic()
    try:
        c.request(method, path, json.dumps(data) if data is not None else None, headers)
        r = c.getresponse()
        body = r.read()
        if body and r.getheader("Content-Encoding") == "gzip":
            body = gzip.decompress(body)
        value = json.loads(body) if body and r.getheader("Content-Type", "").startswith("application/json") else None
        status = r.status
    except (OSError, http.client.HTTPException) as e:
        r, value, status = None, None, type(e).__name__
    finally:
        if connection is None:
            c.close()
    row = dict(phase=phase, method=method, path=path.split("?")[0],
               status=status, ok=status == expected, ms=(time.monotonic()-t0)*1000)
    if status != expected and isinstance(value, dict):
        e = value.get("error")
        row["error_code"] = e.get("code") if isinstance(e, dict) else None
        row["error_field"] = e.get("field") if isinstance(e, dict) else None
        row["storage_unavailable"] = isinstance(e, dict) and e.get("message") == "dashboard storage unavailable"
    if path.startswith("/api/logs/stream") and status == expected:
        row["log_events"] = dict(Counter(line[7:].decode() for line in body.splitlines() if line.startswith(b"event: ")))
        row["ok"] = row["log_events"].get("err", 0) == 0 and row["log_events"].get("done", 0) == 1
        row["bytes_received"] = len(body)
    metrics[phase].append(row)
    append("requests.jsonl", row)
    if check and status != expected:
        raise RuntimeError(f"{method} {path.split('?')[0]} returned {status}")
    return r, value, status

def provider_input(kind, suffix, options=None, keys=(), *, tls=False, fingerprint=None):
    url = (f"https://{gateway}:{tls_port}" if tls else f"http://{gateway}:{fake_port}")
    if suffix:
        url += f"/{suffix}"
    obj = dict(kind=kind, name=kind, base_url=url, enabled=True, options=options or {})
    if keys:
        obj["credentials"] = dict.fromkeys(keys, "benchmark-fixture")
    if fingerprint:
        obj["fingerprint"] = fingerprint
    return obj

def fixture(providers_path, query, data):
    answers = {
        "pi/api/auth": {"session": {"valid": True}},
        "pi/api/stats/summary": {"queries": {"total": 42, "blocked": 12, "percent_blocked": 28.6}},
        "pi/api/dns/blocking": {"blocking": "enabled"},
        "kuma/api/status-page/home": {"publicGroupList": [{"monitorList": [{"id": 1, "name": "Web"}, {"id": 2, "name": "DB"}]}]},
        "kuma/api/status-page/heartbeat/home": {"heartbeatList": {"1": [{"status": 1}], "2": [{"status": 1}]}, "uptimeList": {"1_24": .995, "2_24": 1}},
        "jelly/System/Info": {"ServerName": "Fixture"},
        "jelly/Sessions": [{"Id": "one", "Client": "Web", "NowPlayingItem": {"Name": "Sample", "Type": "Movie"}}],
        "jelly/Items/Counts": {"MovieCount": 42, "SeriesCount": 3, "EpisodeCount": 105, "AlbumCount": 6, "SongCount": 72, "MusicVideoCount": 0, "BoxSetCount": 0, "BookCount": 4},
        "jelly/Items": {"Items": [{"Name": "New episode", "Type": "Episode"}]},
        "qb/api/v2/transfer/info": {"dl_info_speed": 1024, "up_info_speed": 1024, "connection_status": "connected"},
        "qb/api/v2/sync/maindata": {"rid": 1, "full_update": True, "server_state": {"free_space_on_disk": 2147483648}, "torrents": {"hash1": {"state": "queuedDL"}}},
        "qb/api/v2/torrents/info": [{"hash": "hash1", "state": "uploading"}],
        "ha/api/": {"message": "API running."},
        "ha/api/states/light.hall": {"entity_id": "light.hall", "state": "on", "attributes": {}},
        "ha/api/states/sensor.temp": {"entity_id": "sensor.temp", "state": "21", "attributes": {"unit_of_measurement": "°C"}},
        "pve/api2/json/version": {"data": {"version": "9.0.1"}},
        "pve/api2/json/cluster/resources": {"data": [{"node": "pve", "status": "online", "cpu": .125, "mem": 1048576, "maxmem": 2097152}] if query.get("type") == ["node"] else [{"type": "qemu", "status": "running"}, {"type": "lxc", "status": "running"}]},
        "generic": {"reading": 42, "load": 2, "ok": True},
        "reach": {"ok": True}
    }
    if providers_path in answers:
        return 200, {}, json.dumps(answers[providers_path]).encode()
    if providers_path == "qb/api/v2/auth/login":
        return 200, {"Set-Cookie": "SID=fixture; HttpOnly"}, b"Ok."
    if providers_path.startswith(("sonarr/", "radarr/")):
        series = providers_path.startswith("sonarr/")
        path = providers_path.split("/", 1)[1]
        arr = {"api/v3/health": [], "api/v3/queue/status": {"totalCount": 4},
               "api/v3/wanted/missing": {"totalRecords": 8}, "api/v3/system/status": {"version": "4.0.0"},
               "api/v3/calendar": [{"id": 1, "title": "Sample release", **(
                   {"airDateUtc": "2026-11-23T02:00:00Z", "series": {"title": "Sample series"}} if series else
                   {"digitalRelease": "2026-11-23T02:00:00Z"})}]}
        if path in arr:
            return 200, {}, json.dumps(arr[path]).encode()
    if providers_path == "tx/transmission/rpc":
        if not data[0]:
            return 409, {"X-Transmission-Session-Id": "fixture"}, b""
        method = json.loads(data[1]).get("method")
        rpc_args = {"downloadSpeed": 1048576, "uploadSpeed": 1024, "activeTorrentCount": 2} if method == "session-stats" else {"torrents": [{"name": "Sample", "status": 4}]}
        return 200, {}, json.dumps(dict(result="success", arguments=rpc_args)).encode()
    if providers_path == "rss":
        return 200, {"Content-Type": "application/rss+xml"}, b'<rss version="2.0"><channel><title>Fixture</title><item><title>Item one</title><link>https://example.test/story</link><description>One story</description></item></channel></rss>'
    if providers_path == "":
        return 200, {}, b"{}"
    fake_errors[providers_path] += 1
    return 404, {}, b"{}"

class FakeServer(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass
    def do_GET(self):
        uri = urlsplit(self.path)
        route = uri.path.lstrip("/")
        if self.server.is_tls:
            upstream["ssl_expiry/http"] += 1
        else:
            upstream[route] += 1
        code, headers, body = fixture(route, parse_qs(uri.query), (self.headers.get("X-Transmission-Session-Id"), self.rfile.read(int(self.headers.get("Content-Length", 0)))))
        self.send_response(code)
        for k,v in headers.items():
            self.send_header(k,v)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        try:
            self.wfile.write(body)
        except (OSError, ssl.SSLError):
            pass
    do_POST = do_GET
    do_HEAD = do_GET

class FixtureHTTPServer(ThreadingHTTPServer):
    def handle_error(self, request, address):
        import sys
        if not isinstance(sys.exception(), OSError):
            super().handle_error(request, address)
    def get_request(self):
        conn, addr = super().get_request()
        if self.is_tls:
            upstream["ssl_expiry/handshake"] += 1
        return conn, addr

def start_fixture(tls=False):
    server = FixtureHTTPServer((gateway, 0), FakeServer)
    server.is_tls = tls
    fingerprint = None
    if tls:
        key, cert = scratch / "key.pem", scratch / "cert.pem"
        subprocess.run(["openssl", "req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1",
                        "-keyout", key, "-out", cert, "-days", "30", "-nodes", "-subj", "/CN=hobnail fixture",
                        "-addext", f"subjectAltName=IP:{gateway}"], check=True, capture_output=True)
        fingerprint = hashlib.sha256(ssl.PEM_cert_to_DER_cert(cert.read_text())).hexdigest()
        ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        ctx.load_cert_chain(cert, key)
        server.socket = ctx.wrap_socket(server.socket, server_side=True)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server.server_port, fingerprint

def auth(rounds, onboard=True):
    global sessions
    state.update(phase="owner+login", interval=.005 if opts.mode in ("auth", "setup") else .02)
    if onboard:
        r, v, status = request("POST", "/api/v1/onboarding/owner", credential, 201, auth=False, check=False)
        if status != 201:
            return False
    for _ in range(rounds):
        r, v, status = request("POST", "/api/login", credential, auth=False, check=False)
        if status != 200:
            return False
        time.sleep(.10)
    sessions = (r.getheader("Set-Cookie").split(";", 1)[0], v["csrf"])
    state["interval"] = .5
    return True

def setup_board():
    state["phase"] = "guided-setup"
    draft_url = "/api/v1/onboarding/draft"
    _, draft, _ = request("GET", draft_url)
    request("GET", "/api/v1/onboarding/discovery")
    a, b = secrets.token_hex(16), secrets.token_hex(16)
    sample_web = provider_input("http", "generic", {"paths": ["reading"]})
    items = [
        dict(id=a, included=True, name="Sample webpage", group="Services",
             target=dict(kind="board", payload=dict(kind="link", title="Sample webpage", url="https://example.test/"))),
        dict(id=b, included=True, name="Fixture reading", group="Services",
             target=dict(kind="provider", provider="http", base_url=sample_web["base_url"], enabled=True, options={})),
    ]
    # Follow the same draft transitions as the browser.
    for step in ("appearance", "discover", "select", "connections", "review"):
        _, draft, _ = request("PUT", draft_url, dict(revision=draft["revision"],
                              document=dict(step=step, theme="light", items=items if step != "appearance" else []),
                              credentials=[]))
        if step == "connections":
            _, verdict, _ = request("POST", "/api/v1/onboarding/test", dict(revision=draft["revision"], item_id=b))
            if not verdict["ok"]:
                raise RuntimeError(f"guided connection test: {verdict['class']}")
    _, result, _ = request("POST", "/api/v1/onboarding/commit", dict(revision=draft["revision"]))
    _, replay, _ = request("POST", "/api/v1/onboarding/commit", dict(revision=draft["revision"]))
    assert result == replay
    return result["board_id"]

def install(board):
    state["phase"] = "providers"
    catalog = [
        (provider_input("pihole", "pi"), [("dns", {})]),
        (provider_input("uptime-kuma", "kuma", {"slug": "home"}), [("monitors", {})]),
        (provider_input("jellyfin", "jelly", keys=("api_key",)), [("streams", {}), ("library", {}), ("latest", {})]),
        (provider_input("qbittorrent", "qb", keys=("username", "password")), [("transfers", {})]),
        (provider_input("sonarr", "sonarr", keys=("api_key",)), [("overview", {}), ("upcoming", {})]),
        (provider_input("radarr", "radarr", keys=("api_key",)), [("overview", {}), ("upcoming", {})]),
        (provider_input("transmission", "tx"), [("activity", {})]),
        (provider_input("home-assistant", "ha", keys=("token",)), [("entities", {"entities": ["light.hall", "sensor.temp"]})]),
        (provider_input("proxmox-ve", "pve", {"token_id": "bench@pve!test"}, ("token_secret",)), [("cluster", {})]),
        (provider_input("http", "generic"), [("json", {"paths": ["reading", "load"]}), ("status", {})]),
        (provider_input("feed", "rss"), [("items", {})]),
        (provider_input("ssl_expiry", "", tls=True, fingerprint=tls_fingerprint), [("expiry", {})]),
        (provider_input("reachability", "reach", {"probe": "HTTP"}), [("status", {})]),
    ]
    if opts.mode == "probe" and "m4-b2" in opts.image:
        _, descriptors, _ = request("GET", "/api/v1/providers")
        present = {p["id"] for p in descriptors}
        catalog = [entry for entry in catalog if entry[0]["kind"] in present]
    expected = 0
    for integration, specs in catalog:
        widgets = [dict(id=kind, options={"refresh": 15, **fields}) for kind, fields in specs]
        expected += len(widgets)
        request("POST", "/api/v1/providers/install", dict(integration=integration, board_id=board, widgets=widgets), 201)
    _, integrations, _ = request("GET", "/api/v1/integrations")
    append("fixture.jsonl", dict(providers=[i["kind"] for i,_ in catalog], expected_widgets=expected))
    return expected, integrations

def seed_history(db_path):
    state["phase"] = "history-seed"
    # Seed the complete raw and rolled retention windows using a separate process/cgroup.
    now = int(time.time()) // 15 * 15
    with sqlite3.connect(db_path, timeout=20) as db:
        db.execute("PRAGMA foreign_keys=ON")
        def values(ts):
            return (1700 + ts % 137, 43, 5800, 100, 4800, 1000 + ts % 125, 600, 4500)
        db.executemany("INSERT OR IGNORE INTO sys_samples VALUES (?,?,?,?,?,?,?,?,?)",
                       ((ts, *values(ts)) for ts in range(now-21600, now, 15)))
        keeps = ((60, now-86400*7, now), (600, now-86400*90, now))
        for res, start, end in keeps:
            db.executemany("INSERT OR IGNORE INTO sys_roll VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
                           ((res, ts, *(v for x in values(ts) for v in (x, x+1)), res//15)
                            for ts in range((start+res-1)//res*res, end//res*res, res)))
        names = [f"svc{i:03}" for i in range(1, 151)]
        db.executemany("INSERT OR IGNORE INTO ctr_names(name) VALUES (?)", ((name,) for name in names))
        ids = list(db.execute("SELECT id FROM ctr_names"))
        for (cid,) in ids:
            db.executemany("INSERT OR IGNORE INTO ctr_samples VALUES (?,?,?,?)",
                           ((cid, ts, (ts+cid)%830, 1024*1024*cid) for ts in range(now-21600, now, 15)))
            # A cohort spans the complete long-lived retention windows.
            if cid <= 40:
                for res, start, end in keeps:
                    db.executemany("INSERT OR IGNORE INTO ctr_roll VALUES (?,?,?,?,?,?,?,?)",
                                   ((cid, res, ts, (ts+cid)%830, (ts+cid)%830+1, 1048576*cid, 1048576*cid, res//15)
                                    for ts in range((start+res-1)//res*res, end//res*res, res)))
        for key in ("cpu.busy", "mem.used", "mem.total", "mem.cached", "load.1", "swap.used", "net.eth0.rx", "net.eth0.tx"):
            db.execute("INSERT OR IGNORE INTO hostx_keys(key) VALUES (?)", (key,))
        for (kid,) in db.execute("SELECT id FROM hostx_keys"):
            db.executemany("INSERT OR IGNORE INTO hostx_samples VALUES (?,?,?)",
                           ((kid, ts, 1000+ts%140) for ts in range(now-21600, now, 15)))
            for res, start, end in keeps:
                db.executemany("INSERT OR IGNORE INTO hostx_roll VALUES (?,?,?,?,?,?)",
                               ((kid, res, ts, res//15, 1050, 1140)
                                for ts in range((start+res-1)//res*res, end//res*res, res)))
        for (cid,) in ids:
            if cid > 40:
                continue
            db.executemany("INSERT OR IGNORE INTO ctrx_samples VALUES (" + ",".join("?"*12) + ")",
                           ((cid, ts, *(cid*31+ts%137+i for i in range(10)))
                            for ts in range(now-21600, now, 15)))
            for res, start, end in keeps:
                db.executemany("INSERT OR IGNORE INTO ctrx_roll VALUES (" + ",".join("?"*24) + ")",
                               ((cid, res, ts, res//15, *(v for i in range(10) for v in (cid*31+i, cid*31+i+1)))
                                for ts in range((start+res-1)//res*res, end//res*res, res)))
                db.executemany("INSERT OR IGNORE INTO log_counts VALUES (?,?,?,?,?,?,?,?)",
                               ((cid, res, ts, 0, 1, 20, 0, 0)
                                for ts in range((start+res-1)//res*res, end//res*res, res)))
        db.executemany("INSERT OR REPLACE INTO kv(key,value) VALUES (?,?)",
                       ((f"{prefix}.w{res}", str(now//res*res))
                        for prefix in ("hist", "hostx", "ctrx") for res in (60, 600)))
        db.executemany("INSERT INTO events(ts,kind,level,subject,detail) VALUES (?,?,?,?,?)",
                       ((now-i*7, "container.start", "info", f"svc{i%150+1:03}", "Benchmark fixture")
                        for i in reversed(range(100000))))
        counts = {table: db.execute(f"SELECT count(*) FROM {table}").fetchone()[0] for table in
                  ("sys_samples","sys_roll","ctr_samples","ctr_roll","hostx_samples","hostx_roll","ctrx_samples","ctrx_roll","log_counts","events")}
        if opts.steady_history:
            db.execute("ANALYZE")
            db.execute("INSERT OR REPLACE INTO kv(key,value) VALUES ('hist.vacuum', ?)", (str(now),))
        db.commit()
        db.execute("PRAGMA wal_checkpoint(TRUNCATE)")
    append("fixture.jsonl", dict(history=counts, db_bytes=db_path.stat().st_size))

def health(cg):
    t0 = time.monotonic()
    while time.monotonic()-t0 < 12:
        try:
            c = http.client.HTTPConnection("127.0.0.1", port, timeout=.2)
            c.request("GET", "/healthz")
            r = c.getresponse()
            r.read()
            if r.status == 200:
                append("meta.jsonl", dict(health_after_discovery_ms=(time.monotonic()-t0)*1000,
                                          pre_health_sample=record_sample(cg)))
                return
        except (OSError, http.client.HTTPException):
            pass
        finally:
            c.close()
        time.sleep(.025)
    raise RuntimeError("health did not become ready")

def sse(board, stop, phase):
    c = http.client.HTTPConnection("127.0.0.1", port, timeout=3)
    try:
        t0 = time.monotonic()
        c.request("GET", f"/api/v1/stream?topics=containers,stats,system,widget&board={board}", headers={"Cookie": sessions[0]})
        r = c.getresponse()
        sse_connections[phase] += 1
        append("requests.jsonl", dict(phase=phase, method="GET", path="/api/v1/stream", status=r.status, ok=r.status==200, ms=(time.monotonic()-t0)*1000))
        if r.status != 200:
            sse_events[(phase, "error")] += 1
            return
        c.sock.settimeout(10)
        with lock:
            sse_sockets[threading.get_ident()] = c.sock
        while not stop.is_set():
            line = r.readline()
            if not line:
                if not stop.is_set():
                    sse_events[(phase, "error")] += 1
                break
            if line.startswith(b"event: "):
                name = line[7:].strip().decode()
                sse_events[(phase, name)] += 1
    except (OSError, http.client.HTTPException):
        if not stop.is_set():
            sse_events[(phase, "error")] += 1
    finally:
        with lock:
            sse_sockets.pop(threading.get_ident(), None)
        c.close()

def assets():
    return ["/app"] + sorted("/assets/" + p.name for p in Path("web/dist/assets").iterdir() if not p.name.endswith(".gz"))

def viewer(board, phase, stop, offset, paths):
    def get(path):
        _, result, code = request("GET", path, connection=c, check=False)
        if path == board_api and code == 200:
            with lock:
                widget_status.update(i["widget"]["status"] for i in result["resolved"] if i["widget"])
    if stop.wait(offset):
        return
    c = http.client.HTTPConnection("127.0.0.1", port, timeout=8)
    board_api = f"/api/v1/boards/{board}"
    try:
        t = time.monotonic()
        scheduled = [[t, 20, paths], [t, 2, [board_api, "/api/v1/containers"]],
                     [t, 5, ["/api/history/system?metric=cpu&range=1h"]],
                     [t, 30, ["/api/v1/bootstrap"]]]
        while not stop.is_set():
            now = time.monotonic()
            for job in scheduled:
                if job[0] <= now:
                    for path in job[2]:
                        if stop.is_set():
                            return
                        get(path)
                    job[0] += job[1]
            stop.wait(max(0, min(job[0] for job in scheduled)-time.monotonic()))
    finally:
        c.close()

def close_streams(stop):
    stop.set()
    with lock:
        for conn in sse_sockets.values():
            try:
                conn.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass

def clients(board, count, seconds):
    phase = f"clients-{count}"
    state["phase"] = phase
    stop = threading.Event()
    paths = assets()
    th = [threading.Thread(target=sse, args=(board,stop,phase)) for _ in range(count)]
    th += [threading.Thread(target=viewer, args=(board,phase,stop,2.0*i/count,paths)) for i in range(count)]
    for worker in th:
        worker.start()
    time.sleep(seconds)
    close_streams(stop)
    for worker in th:
        worker.join()
    append("sse.jsonl", dict(phase=phase, requests=sse_connections[phase], events={k:v for (p,k),v in sse_events.items() if p==phase}))

def wait_widgets(board, n):
    state["phase"] = "poll-warmup"
    stop = threading.Event()
    stream = threading.Thread(target=sse, args=(board, stop, "poll-warmup"))
    stream.start()
    try:
        return check_widgets(board, n)
    finally:
        close_streams(stop)
        stream.join()
        append("sse.jsonl", dict(phase="poll-warmup", requests=sse_connections["poll-warmup"],
                                 events={k:v for (p,k),v in sse_events.items() if p=="poll-warmup"}))

def check_widgets(board, n):
    deadline = time.monotonic()+35
    while time.monotonic() < deadline:
        _, view, _ = request("GET", f"/api/v1/boards/{board}")
        widgets = [i["widget"] for i in view["resolved"] if i["widget"]]
        if len(widgets) >= n and sum(w["status"] == "ready" for w in widgets) >= n:
            append("fixture.jsonl", dict(widgets_good=n, upstream=dict(upstream)))
            return
        time.sleep(1)
    safe = [(w["id"], w["status"], w["error"]) for w in widgets]
    append("fixture.jsonl", dict(widget_diagnostics=safe, upstream=dict(upstream), fake_errors=dict(fake_errors)))
    raise RuntimeError("provider widgets did not all report OK")

def operations(board, integrations, repeats):
    state["phase"] = "config-writes"
    api = "/api/v1/boards/" + board
    ids = [i["id"] for i in integrations if i["kind"] == "http"]
    ident = ids[-1]
    for i in range(repeats):
        _, board_view, _ = request("GET", api)
        doc = board_view["document"]
        doc["sections"][0]["title"] = f"Services {i % 2}"
        request("PUT", api, doc)
        request("PATCH", "/api/v1/settings", {"default_theme": "dark" if i % 2 else "light"})
        inp = provider_input("http", "generic")
        inp["name"] = f"Generic HTTP {i % 2}"
        request("PUT", f"/api/v1/integrations/{ident}", inp)
        _, test, _ = request("POST", f"/api/v1/integrations/{ident}/test", {})
        if not test["ok"]:
            raise RuntimeError(f"integration test returned {test['class']}")
        time.sleep(3.5)

def logs():
    state["phase"] = "log-buffers"
    paths = [f"/api/logs/stream?c=svc{i:03}&tail=5000&follow=0" for i in (1, 11, 21, 31)]
    for _ in range(3):
        workers = [threading.Thread(target=request, args=("GET", path), kwargs={"check": False}) for path in paths]
        for worker in workers:
            worker.start()
        for worker in workers:
            worker.join()
        time.sleep(.5)

def histories():
    state["phase"] = "full-history"
    for _ in range(10):
        for path in ("/api/v1/history?range=24h&cmp=1&events=all", "/api/v1/history?range=30d&events=all", "/api/v1/host/history?range=24h", "/api/history/container/svc001?metric=mem&range=30d"):
            _, value, _ = request("GET", path)
            if isinstance(value, dict) and "metrics" in value:
                append("fixture.jsonl", dict(history_path=path, points=[len(m["points"]) for m in value["metrics"]]))
        time.sleep(.5)

def mark(cg, phase, seconds):
    state["phase"] = phase
    record_sample(cg)
    time.sleep(seconds)

def one(ordinal):
    global scratch, gateway, fake_port, tls_port, tls_fingerprint, sessions
    sessions = ("","")
    state.update(phase="startup", interval=.5)
    scratch = Path(tempfile.mkdtemp(prefix="hobnail-m5-", dir="/var/tmp"))
    data = scratch / "data"
    data.mkdir()
    data_path = data / "hobnail.db"
    fake = f"hobnail-m5-docker-{port}"
    created = []
    fake_mounts = []
    stop = threading.Event()
    cg = None
    thread = None
    if opts.mode == "auth":
        gateway = None
    else:
        gateway = "127.0.0.1"
        fake_port, _ = start_fixture()
        tls_port, tls_fingerprint = start_fixture(True)
        (data / "config.toml").write_text(f'[http]\nallow_hosts = ["{gateway}"]\n')
        fake_data = scratch / "fake"
        fake_data.mkdir()
        fake_mounts = ["-v", f"{fake_data}:/fake:ro", "-e", "RH_DOCKER_SOCKET=/fake/docker.sock", "-e", "RH_HOST_SYS=/fake/sys"]
        run("docker", "run", "-d", "--name", fake, "--user", f"{os.getuid()}:{os.getgid()}", "--network", network,
            "-v", f"{Path.cwd()}:/repo:ro", "-v", f"{fake_data}:/fake", node, "node",
            "/repo/web/e2e/fake-docker.mjs", "/fake/docker.sock", "--sys", "/fake/sys", "--quiet")
        created.append(fake)
        for _ in range(100):
            if (fake_data / "docker.sock").exists():
                break
            time.sleep(.05)
        assert (fake_data / "docker.sock").exists()
    common = ["-u", f"{os.getuid()}:{os.getgid()}", 
              "--network", network, "--read-only", "--tmpfs", "/tmp:size=8m", "--cap-drop", "ALL", "--security-opt", "no-new-privileges",
              "--health-cmd", "/hobnail healthcheck", "--health-interval", "30s", "--health-timeout", "5s",
              "-v", f"{data}:/data", "-v", "/proc:/host/proc:ro", "-e", f"RH_LISTEN=127.0.0.1:{port}", "-e", "RH_MAX_CONNECTIONS_PER_PEER=" + os.environ.get("BENCH_PER_PEER", "12"), *(["-e", "RH_MAX_CONNECTIONS=" + os.environ["BENCH_MAX_CONN"]] if os.environ.get("BENCH_MAX_CONN") else []), *fake_mounts]
    try:
        if opts.mode in ("full", "probe"):
            initial = container + "-seed-schema"
            # Only the time-series data is prepopulated. The measured cgroup starts
            # independently, with no owner or session and an empty dashboard.
            run("docker", "run", "-d", "--name", initial, *common, opts.image)
            created.append(initial)
            for _ in range(100):
                c = http.client.HTTPConnection("127.0.0.1", port, timeout=.2)
                try:
                    c.request("GET", "/healthz")
                    if c.getresponse().status == 200:
                        break
                except OSError:
                    pass
                finally:
                    c.close()
                time.sleep(.05)
            run("docker", "stop", initial)
            seed_history(data_path)
            if opts.cold_db:
                with data_path.open("rb") as infile:
                    os.posix_fadvise(infile.fileno(), 0, 0, os.POSIX_FADV_DONTNEED)
            state["phase"] = "startup"
        if opts.legacy_image:
            old = container + "-legacy"
            run("docker", "run", "-d", "--name", old, *common, opts.legacy_image)
            created.append(old)
            for _ in range(100):
                if request("GET", "/healthz", auth=False, check=False)[2] == 200:
                    break
                time.sleep(.05)
            request("POST", "/api/v1/onboarding/owner", credential, 201, auth=False)
            run("docker", "stop", old)
        limit = ["--memory", "20m", "--memory-swap", "20m"] if opts.hard else []
        cid = run("docker", "create", "--name", container, *common, *limit, opts.image)
        created.append(container)
        run("docker", "start", container)
        cg = Path("/sys/fs/cgroup/system.slice/docker-" + cid + ".scope")
        if not cg.exists():
            pid = run("docker", "inspect", "-f", "{{.State.Pid}}", container)
            cg = Path("/sys/fs/cgroup" + Path(f"/proc/{pid}/cgroup").read_text().strip().split(":", 2)[2])
        append("meta.jsonl", dict(ordinal=ordinal, image=opts.image, id=run("docker", "image", "inspect", "-f", "{{.Id}}", opts.image),
                                  memory_max=(cg/"memory.max").read_text().strip(), swap_max=(cg/"memory.swap.max").read_text().strip(), cold_db=opts.cold_db, kernel=os.uname().release))
        thread = threading.Thread(target=sampler, args=(stop, cg))
        thread.start()
        health(cg)
        time.sleep(2 if opts.mode != "full" else 10)
        if not auth(7 if opts.mode == "auth" else 1, not opts.legacy_image):
            if opts.hard:
                try:
                    run("docker", "wait", container, timeout=1)
                except subprocess.TimeoutExpired:
                    pass
            append("meta.jsonl", dict(auth_completed=False))
            return
        if opts.mode == "auth":
            mark(cg, "after-login", 2)
            return
        board = setup_board()
        record_sample(cg)
        if opts.mode == "setup":
            return
        expected, integrations = install(board)
        record_sample(cg)
        wait_widgets(board, expected)
        mark(cg, "idle", opts.long if opts.mode == "full" or opts.maintenance_trace else 3)
        if opts.mode == "full":
            for n in (1, 10, 25):
                clients(board, n, opts.long)
                record_sample(cg)
        histories()
        record_sample(cg)
        logs()
        record_sample(cg)
        operations(board, integrations, 10 if opts.mode == "full" else 2)
        record_sample(cg)
        mark(cg, "cleanup", opts.long if opts.mode == "full" else 3)
    finally:
        stop.set()
        if thread:
            thread.join()
        if cg:
            try:
                final_cgroup = record_sample(cg)
            except FileNotFoundError:
                final_cgroup = None
            append("meta.jsonl", dict(final_cgroup=final_cgroup, upstream=dict(upstream),
                                      fake_errors=dict(fake_errors), widget_status=dict(widget_status)))
            append("meta.jsonl", dict(oom_killed=run("docker", "inspect", "-f", "{{.State.OOMKilled}}", container)))
            # Sanitized diagnostics: retain only relevant fixed log messages and numeric error metadata.
            app_logs = subprocess.run(["docker", "logs", container], capture_output=True, text=True)
            for line in (app_logs.stdout+app_logs.stderr).splitlines():
                try:
                    row = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if row.get("msg") in {"database error", "dashboard storage error"}:
                    append("diagnostics.jsonl", {k: row[k] for k in ("msg", "op", "code", "extended_code") if k in row})
        for name in reversed(created):
            run("docker", "rm", "-f", name)
        import shutil
        shutil.rmtree(scratch)

try:
    for i in range(opts.repeat):
        one(i+1)
finally:
    print(out, flush=True)
