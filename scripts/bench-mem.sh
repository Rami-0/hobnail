#!/bin/bash
# Memory benchmark (docs/plans/react-port.md 4.9): one fresh application
# container, no memory limit, sampled from the host's cgroup v2 files while
# load clients (in a second container on the host network) use it.
#   scripts/bench-mem.sh <image> <outdir> [config.toml]
#   scripts/bench-mem.sh hobnail-rp:m4 /var/tmp/bench-app config.toml
#   quick smoke: STARTUP_S=5 IDLE_S=5 LOAD_S=5 AFTER_S=5 CLIENTS="1 3" scripts/bench-mem.sh ...
#
# Phases: start-up STARTUP_S (10) s at 100 ms, counted from `docker start`
# (start -> first /healthz 200 is recorded too); idle IDLE_S (90) s at 500 ms;
# each client count in CLIENTS ("1 10 25") LOAD_S (60) s at 500 ms; after load
# AFTER_S (60) s at 500 ms. Each sample: memory.current, memory.peak,
# memory.stat (anon, file, kernel, sock, file_mapped, inactive_file), working
# set = current - inactive_file (what `docker stats` shows) and cpu.stat
# usage_usec, appended to <outdir>/samples.csv as it is taken. At each phase
# end RssAnon/RssFile/RssShmem/VmRSS of the app process go to <outdir>/rss.csv.
#
# A load client holds one event stream, reads state every 2 s and history
# every 5 s, and downloads every client asset at its start and every 20 s;
# it counts requests, errors, stream events and latency into
# <outdir>/clients/<phase>/client-<n>.json. The load uses /api/v1/stream,
# /api/v1/containers, /api/history/system and the embedded app assets.
# Each load process shares an authenticated session on the fresh instance.
# Other env: PORT (3421; 127.0.0.1:PORT -> 3200), TOOLS_IMAGE (an image with
# python3, default hobnail-bench-tools:latest), LABEL (written into the
# report; default "warm cache": the page cache is not dropped), HEALTH (1:
# the compose healthcheck runs every 30 s, as deployed; 0: none), BENCH_TMP
# (parent of the temp data dir; default /var/tmp so the database is on disk,
# not tmpfs).
#
# The app container mirrors deploy/compose.yaml: the caller's uid:gid plus
# the Docker socket's group, read-only Docker socket, /proc and /sys
# read-only at /host/proc and /host/sys, read-only root, 8 MiB /tmp tmpfs,
# no capabilities, no-new-privileges, RH_HOSTNAME=homelab; its data dir is a
# fresh temp dir holding only the given config.toml. Everything it starts and
# the temp data are removed on exit, also on Ctrl-C. The browser, the Docker
# daemon and the load clients are never in the measured cgroup.
set -euo pipefail

usage() { echo "usage: scripts/bench-mem.sh <image> <outdir> [config.toml]" >&2; exit 2; }
[ $# -ge 2 ] || usage
IMAGE=$1; OUTDIR=$2; CONFIG=${3:-}
PORT=${PORT:-3421}
CLIENTS=${CLIENTS:-"1 10 25"}
STARTUP_S=${STARTUP_S:-10}; IDLE_S=${IDLE_S:-90}; LOAD_S=${LOAD_S:-60}; AFTER_S=${AFTER_S:-60}
TOOLS_IMAGE=${TOOLS_IMAGE:-hobnail-bench-tools:latest}
LABEL=${LABEL:-"warm cache"}
HEALTH=${HEALTH:-1}
[ "$PORT" != 3200 ] || { echo "refusing port 3200: that is the live instance" >&2; exit 2; }
[ -z "$CONFIG" ] || [ -f "$CONFIG" ] || { echo "no such config: $CONFIG" >&2; exit 2; }
docker image inspect "$IMAGE" >/dev/null || { echo "no such image: $IMAGE" >&2; exit 2; }
docker image inspect "$TOOLS_IMAGE" >/dev/null || { echo "no such tools image: $TOOLS_IMAGE" >&2; exit 2; }
if curl -s -o /dev/null --max-time 0.5 "http://127.0.0.1:$PORT/" 2>/dev/null; then echo "port $PORT is already in use" >&2; exit 2; fi

mkdir -p "$OUTDIR"; OUTDIR=$(cd "$OUTDIR" && pwd)
[ ! -e "$OUTDIR/samples.csv" ] || { echo "$OUTDIR already holds a run (samples.csv): use a new outdir" >&2; exit 2; }
NAME=hobnail-p0-bench-$$
LOADNAME=$NAME-load
TMP=$(mktemp -d "${BENCH_TMP:-/var/tmp}/hobnail-p0-bench.XXXXXX")
mkdir -p "$TMP/data" "$TMP/bin" "$TMP/credentials" "$OUTDIR/clients"
[ -z "$CONFIG" ] || cp "$CONFIG" "$TMP/data/config.toml"

cleanup() {
  trap - EXIT INT TERM
  [ -n "${HZ_PID:-}" ] && kill "$HZ_PID" 2>/dev/null || true
  docker rm -f "$LOADNAME" >/dev/null 2>&1 || true
  docker rm -f "$NAME" >/dev/null 2>&1 || true
  rm -rf "$TMP"
}
trap cleanup EXIT
trap 'echo "interrupted" >&2; exit 130' INT TERM

now_us() { local t=${EPOCHREALTIME/./}; echo $((10#$t)); }

# ---- the load client and the report (python3 in TOOLS_IMAGE) ----
cat > "$TMP/bin/client.py" <<'PY'
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
PY

cat > "$TMP/bin/report.py" <<'PY'
import csv, glob, json, os, statistics, sys
out = sys.argv[1]
rows = list(csv.DictReader(open(os.path.join(out, "samples.csv"))))
meta = dict(l.rstrip("\n").split("=", 1) for l in open(os.path.join(out, "meta.txt")) if "=" in l)
MiB = 1048576
def med(xs): return statistics.median(xs) if xs else float("nan")
def pct(xs, p):
    if not xs: return float("nan")
    xs = sorted(xs); k = (len(xs) - 1) * p / 100; f = int(k); c = min(f + 1, len(xs) - 1)
    return xs[f] + (xs[c] - xs[f]) * (k - f)
phases = []
for r in rows:
    if not phases or phases[-1][0] != r["phase"]: phases.append((r["phase"], []))
    phases[-1][1].append(r)
print(f"### Memory benchmark: `{meta.get('image')}` ({meta.get('image_id', '')[:19]}), profile {meta.get('profile')}, {meta.get('label')}")
print()
print(f"{meta.get('date')}; config {meta.get('config')}; host {meta.get('host')}; start -> first /healthz 200: {meta.get('healthz_ms', 'n/a')} ms")
print()
print("| Phase | Samples | `memory.current` median / max | Working set median / max | `memory.peak` at end | anon / file / kernel / sock median | CPU % of one core | Requests / errors / events | p50 / p99 ms |")
print("|---|---|---|---|---|---|---|---|---|")
for name, rs in phases:
    cur = [int(r["memory_current"]) / MiB for r in rs]
    ws = [int(r["working_set"]) / MiB for r in rs]
    t = [int(r["t_us"]) for r in rs]; u = [int(r["cpu_usage_usec"]) for r in rs]
    cpu = (u[-1] - u[0]) / (t[-1] - t[0]) * 100 if len(rs) > 1 and t[-1] > t[0] else float("nan")
    st = " / ".join(f"{med([int(r[k]) / MiB for r in rs]):.2f}" for k in ("anon", "file", "kernel", "sock"))
    load = "–"; lat = "–"
    cdir = os.path.join(out, "clients", name)
    files = sorted(glob.glob(os.path.join(cdir, "client-*.json")))
    if files:
        cs = [json.load(open(f)) for f in files]
        L = [x for c in cs for x in c["latency_ms"]]
        load = f"{sum(c['requests'] for c in cs)} / {sum(c['errors'] for c in cs)} / {sum(c['events'] for c in cs)}"
        lat = f"{pct(L, 50):.1f} / {pct(L, 99):.1f}"
    print(f"| {name} | {len(rs)} | {med(cur):.2f} / {max(cur):.2f} | {med(ws):.2f} / {max(ws):.2f} | {int(rs[-1]['memory_peak']) / MiB:.2f} | {st} | {cpu:.2f} | {load} | {lat} |")
print()
print("Process RSS at phase ends (MiB, from /proc/<pid>/status):")
print()
print("| Phase | RssAnon | RssFile | RssShmem | VmRSS |\n|---|---|---|---|---|")
for r in csv.DictReader(open(os.path.join(out, "rss.csv"))):
    print(f"| {r['phase']} | " + " | ".join(f"{int(r[k]) / 1024:.2f}" for k in ("rss_anon_kb", "rss_file_kb", "rss_shmem_kb", "vm_rss_kb")) + " |")
print()
for name, _ in phases:
    for f in sorted(glob.glob(os.path.join(out, "clients", name, "client-*.json"))):
        c = json.load(open(f))
        for e in c.get("error_samples", []): print(f"- {name} client {c['client']}: {e}")
ep = os.path.join(out, "clients")
for d in sorted(glob.glob(os.path.join(ep, "*", "endpoints.json")))[:1]:
    e = json.load(open(d)); print(f"Endpoints: stream `{e['stream']}`, state `{e['state']}`, history `{e['history']}`, assets {len(e['assets'])}: " + ", ".join(f"`{a}`" for a in e["assets"]))
print()
print(f"Raw samples: {out}/samples.csv; RSS: {out}/rss.csv; clients: {out}/clients/")
PY

# ---- the app container ----
SOCK=/var/run/docker.sock
health=(--no-healthcheck)
[ "$HEALTH" = 1 ] && health=(--health-cmd "/hobnail healthcheck" --health-interval 30s --health-timeout 5s --health-retries 3 --health-start-period 5s)
docker create --name "$NAME" \
  -u "$(id -u):$(id -g)" --group-add "$(stat -c %g "$SOCK")" \
  -p "127.0.0.1:$PORT:3200" \
  -v "$SOCK:$SOCK:ro" -v "$TMP/data:/data" -v /proc:/host/proc:ro -v /sys:/host/sys:ro \
  -e RH_LISTEN=0.0.0.0:3200 -e RH_HOSTNAME=homelab \
  --read-only --tmpfs /tmp:size=8m --cap-drop ALL --security-opt no-new-privileges:true \
  "${health[@]}" "$IMAGE" >/dev/null

{
  echo "image=$IMAGE"
  echo "image_id=$(docker image inspect -f '{{.Id}}' "$IMAGE")"
  echo "profile=app"
  echo "label=$LABEL"
  echo "config=$([ -n "$CONFIG" ] && basename "$CONFIG" || echo none) ($([ -n "$CONFIG" ] && sha256sum "$CONFIG" | cut -c1-12 || echo empty))"
  echo "date=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "host=$(nproc) CPUs, $(awk '/MemTotal/ {printf "%.1f GiB", $2/1048576}' /proc/meminfo), kernel $(uname -r), Docker $(docker version -f '{{.Server.Version}}')"
  echo "phases=startup ${STARTUP_S}s/100ms idle ${IDLE_S}s/500ms load ${LOAD_S}s/500ms x ($CLIENTS) after ${AFTER_S}s/500ms"
  echo "healthcheck=$HEALTH"
} > "$OUTDIR/meta.txt"

CSV=$OUTDIR/samples.csv
echo "phase,clients,t_us,memory_current,memory_peak,anon,file,kernel,sock,file_mapped,inactive_file,working_set,cpu_usage_usec" > "$CSV"
echo "phase,t_us,pid,rss_anon_kb,rss_file_kb,rss_shmem_kb,vm_rss_kb" > "$OUTDIR/rss.csv"

CG=""; PID=""; T0=0
# One sample, builtins only (no fork per read).
sample() { # phase clients
  local cur peak k v anon=0 file=0 kernel=0 sock=0 fmap=0 inact=0 usage=0 t
  t=$(( $(now_us) - T0 ))
  read -r cur < "$CG/memory.current" || return 0
  read -r peak < "$CG/memory.peak" || peak=0
  while read -r k v; do
    case $k in anon) anon=$v ;; file) file=$v ;; kernel) kernel=$v ;; sock) sock=$v ;; file_mapped) fmap=$v ;; inactive_file) inact=$v ;; esac
  done < "$CG/memory.stat"
  while read -r k v; do [ "$k" = usage_usec ] && usage=$v; done < "$CG/cpu.stat"
  echo "$1,$2,$t,$cur,$peak,$anon,$file,$kernel,$sock,$fmap,$inact,$((cur - inact)),$usage" >> "$CSV"
}
# Samples every $3 ms for $4 s (fixed schedule, no drift).
run_phase() { # phase clients interval_ms seconds
  local start next end now
  start=$(now_us); end=$((start + $4 * 1000000)); next=$start
  while :; do
    now=$(now_us); [ "$now" -lt "$end" ] || break
    if [ "$now" -ge "$next" ]; then sample "$1" "$2"; next=$((next + $3 * 1000)); continue; fi
    sleep "$(printf '0.%06d' $(( next - now < 999999 ? next - now : 999999 )))"
  done
  rss "$1"
}
rss() { # phase
  local k v a=0 f=0 s=0 r=0
  while read -r k v _; do
    case $k in RssAnon:) a=$v ;; RssFile:) f=$v ;; RssShmem:) s=$v ;; VmRSS:) r=$v ;; esac
  done < "/proc/$PID/status"
  echo "$1,$(( $(now_us) - T0 )),$PID,$a,$f,$s,$r" >> "$OUTDIR/rss.csv"
  echo "  $1 done: memory.current $(awk '{printf "%.2f", $1/1048576}' "$CG/memory.current") MiB, RssAnon $(awk "BEGIN{printf \"%.2f\", $a/1024}") MiB" >&2
}

echo "bench: $IMAGE on 127.0.0.1:$PORT, container $NAME, out $OUTDIR" >&2
# Start -> first /healthz 200, polled from the host from before `docker start`.
T0=$(now_us)
( until curl -fs -o /dev/null --max-time 0.2 "http://127.0.0.1:$PORT/healthz" 2>/dev/null; do
    [ $(( $(now_us) - T0 )) -lt 30000000 ] || exit 1
  done
  echo $(( ($(now_us) - T0) / 1000 )) > "$TMP/healthz_ms" ) &
HZ_PID=$!
docker start "$NAME" >/dev/null
ID=$(docker inspect -f '{{.Id}}' "$NAME"); PID=$(docker inspect -f '{{.State.Pid}}' "$NAME")
CG=/sys/fs/cgroup/system.slice/docker-$ID.scope
[ -d "$CG" ] || CG=/sys/fs/cgroup$(cut -d: -f3- "/proc/$PID/cgroup" | head -1)
[ -r "$CG/memory.current" ] || { echo "cannot read the container's cgroup ($CG)" >&2; exit 1; }
echo "  cgroup $CG, pid $PID, docker start took $(( ($(now_us) - T0) / 1000 )) ms" >&2

# Start-up: 10 s from docker start at 100 ms (minus what docker start took).
remaining=$(( STARTUP_S * 1000000 - ($(now_us) - T0) ))
run_phase startup 0 100 $(( remaining > 0 ? (remaining + 999999) / 1000000 : 0 ))
wait "$HZ_PID" 2>/dev/null || true; HZ_PID=""
hz=$(cat "$TMP/healthz_ms" 2>/dev/null || echo "none within 30 s")
echo "healthz_ms=$hz" >> "$OUTDIR/meta.txt"
echo "  start -> first /healthz 200: $hz ms" >&2
curl -fs -o /dev/null "http://127.0.0.1:$PORT/healthz" || { echo "the app is not answering /healthz" >&2; docker logs --tail 20 "$NAME" >&2; exit 1; }

run_phase idle 0 500 "$IDLE_S"

for n in $CLIENTS; do
  ph=clients-$n; d=$OUTDIR/clients/$ph; mkdir -p "$d"
  docker run -d --name "$LOADNAME" --network host -u "$(id -u):$(id -g)" \
    -v "$TMP/bin:/bench:ro" -v "$TMP/credentials:/credentials" -v "$d:/out" --entrypoint python3 "$TOOLS_IMAGE" \
    -I /bench/client.py 127.0.0.1 "$PORT" "$n" "$LOAD_S" /out >/dev/null
  for _ in $(seq 150); do [ -e "$d/ready" ] && break; sleep 0.1; done
  [ -e "$d/ready" ] || { echo "the load clients did not start" >&2; docker logs "$LOADNAME" >&2; exit 1; }
  run_phase "$ph" "$n" 500 "$LOAD_S"
  rc=$(docker wait "$LOADNAME")
  [ "$rc" = 0 ] || { echo "load clients exited $rc" >&2; docker logs --tail 20 "$LOADNAME" >&2; }
  docker rm "$LOADNAME" >/dev/null
done

run_phase after 0 500 "$AFTER_S"

docker run --rm -u "$(id -u):$(id -g)" -v "$TMP/bin:/bench:ro" -v "$OUTDIR:/out" --entrypoint python3 "$TOOLS_IMAGE" \
  -I /bench/report.py /out | sed "s#/out#$OUTDIR#g" | tee "$OUTDIR/summary.md"
