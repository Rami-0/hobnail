#!/bin/bash
# Mixed guard bench: app container on the default bridge; 25 clients from 5
# containers (5 each, distinct IPs) and attackers from their own containers,
# all addressing the app's bridge IP. Samples memory.current and the app's
# open fds every 500 ms.   bench-guards.sh <image> <outdir> <phase-spec>...
# phase spec: name:seconds:attackers where attackers is a comma list of
#   mode/conns/containers (e.g. header/50/1,body/50/1) or "none".
set -euo pipefail
IMAGE=$1; mkdir -p "$2"; OUT=$(cd "$2" && pwd); shift 2
D=$(cd "$(dirname "$0")" && pwd); TOOLS=hobnail-bench-tools:latest
NAME=hobnail-p0-m6-bench-$$; mkdir -p "$OUT"
TMP=$(mktemp -d /var/tmp/hobnail-p0-m6b.XXXXXX); mkdir -p "$TMP/data" "$TMP/cred"
cleanup() { docker ps -aq --filter "name=$NAME" | xargs -r docker rm -f >/dev/null; rm -rf "$TMP"; }
trap cleanup EXIT
SOCK=/var/run/docker.sock
docker run -d --name "$NAME" -u "$(id -u):$(id -g)" --group-add "$(stat -c %g $SOCK)" \
  -v "$SOCK:$SOCK:ro" -v "$TMP/data:/data" -v /proc:/host/proc:ro -v /sys:/host/sys:ro \
  -e RH_LISTEN=0.0.0.0:3200 -e RH_HOSTNAME=homelab ${APP_ENV:-} \
  --read-only --tmpfs /tmp:size=8m --cap-drop ALL --security-opt no-new-privileges:true \
  --no-healthcheck "$IMAGE" >/dev/null
IP=$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "$NAME")
PID=$(docker inspect -f '{{.State.Pid}}' "$NAME"); ID=$(docker inspect -f '{{.Id}}' "$NAME")
CG=/sys/fs/cgroup/system.slice/docker-$ID.scope
for _ in $(seq 50); do curl -fs -o /dev/null "http://$IP:3200/healthz" && break; sleep 0.1; done
ASSET=$(curl -s "http://$IP:3200/app" | grep -o '/assets/index-[^"]*\.js' | head -1)
echo "app $IP pid $PID asset $ASSET" >&2
echo "phase,t,memory_current,fds,anon,kernel,sock" > "$OUT/samples.csv"
run() { # name seconds
  local end=$(( $(date +%s) + $2 ))
  while [ "$(date +%s)" -lt "$end" ]; do
    echo "$1,$(date +%s.%N),$(cat $CG/memory.current),$(ls /proc/$PID/fd | wc -l),$(awk '$1=="anon"||$1=="sock"||$1=="kernel"{printf "%s ",$2}' $CG/memory.stat | tr ' ' ',' | sed 's/,$//')" >> "$OUT/samples.csv"; sleep ${SAMPLE:-0.5}
  done
}
start_clients() { # name seconds
  local d=$OUT/$1; mkdir -p "$d"
  local boxes=${CLIENT_BOXES:-5}
  for i in $(seq "$boxes"); do
    mkdir -p "$d/c$i"
    docker run -d --name "$NAME-c$i" -u "$(id -u):$(id -g)" -v "$D/client.py:/bench/client.py:ro" \
      -v "$TMP/cred:/credentials" -v "$d/c$i:/out" --entrypoint python3 "$TOOLS" -I /bench/client.py "$IP" 3200 $((25 / boxes)) "$2" /out >/dev/null
    for _ in $(seq 100); do [ -e "$d/c$i/ready" ] && break; sleep 0.1; done
  done
}
start_attack() { # name seconds spec
  local d=$OUT/$1 k=0; [ "$3" = none ] && return 0
  IFS=, read -ra parts <<< "$3"
  for p in "${parts[@]}"; do IFS=/ read -r mode conns boxes <<< "$p"
    for _ in $(seq "$boxes"); do k=$((k+1)); mkdir -p "$d/a$k"
      docker run -d --name "$NAME-a$k" -u "$(id -u):$(id -g)" -v "$D/attack2.py:/bench/attack2.py:ro" -v "$d/a$k:/out" \
        --entrypoint python3 "$TOOLS" -I /bench/attack2.py "$IP" 3200 "$mode" "$conns" "$2" "$ASSET" /out >/dev/null
    done
  done
}
[ -z "${TIGHT:-}" ] || python3 "$D/tight.py" "$CG" "$TIGHT" "$OUT/tight.txt" &
run idle 20
for spec in "$@"; do
  IFS=: read -r name secs attackers <<< "$spec"
  start_attack "$name" "$secs" "$attackers"; sleep 2; start_clients "$name" "$secs"
  run "$name" "$secs"
  for c in $(docker ps -aq --filter "name=$NAME-"); do docker wait "$c" >/dev/null; docker rm "$c" >/dev/null; done
  run "$name-after" 20
  echo "== $name" >&2
done
echo "memory.peak $(cat $CG/memory.peak)" > "$OUT/peak.txt"
python3 - "$OUT" <<'PY'
import csv, glob, json, os, sys, statistics as S
out = sys.argv[1]; rows = list(csv.DictReader(open(f"{out}/samples.csv"))); M = 1048576
ph = {}
for r in rows: ph.setdefault(r["phase"], []).append(r)
print("| phase | memory.current median / max MiB | app fds median / max | client requests / errors / events | latency p50 / p95 / p99 ms | attackers |")
print("|---|---|---|---|---|---|")
for name, rs in ph.items():
    m = [int(r["memory_current"]) / M for r in rs]; f = [int(r["fds"]) for r in rs]
    cs = [json.load(open(p)) for p in glob.glob(f"{out}/{name}/c*/client-*.json")]
    load = f"{sum(c['requests'] for c in cs)} / {sum(c['errors'] for c in cs)} / {sum(c['events'] for c in cs)}" if cs else "-"
    at = [json.load(open(p)) for p in sorted(glob.glob(f"{out}/{name}/a*/attack.json"))]
    agg = {}
    for a in at:
        g = agg.setdefault(a["mode"], {"boxes": 0, "connects": 0, "closed_by_server": 0, "refused_503": 0, "answered_408": 0, "held_median_s": []})
        g["boxes"] += 1
        for k in ("connects", "closed_by_server", "refused_503", "answered_408"): g[k] += a[k]
        g["held_median_s"].append(a["held_median_s"])
    att = "; ".join(f"{k}x{v['boxes']}: {v['connects']} conns, {v['closed_by_server']} closed by server ({v['refused_503']} 503, {v['answered_408']} 408), held median {v['held_median_s']} s" for k, v in agg.items()) or "-"
    lat = sorted(x for c in cs for x in c.get("latency_ms", []))
    q = lambda p: lat[min(len(lat) - 1, int(p * len(lat)))]
    lat_s = f"{q(.5):.1f} / {q(.95):.1f} / {q(.99):.1f}" if lat else "-"
    print(f"| {name} | {S.median(m):.2f} / {max(m):.2f} | {S.median(f):.0f} / {max(f)} | {load} | {lat_s} | {att} |")
    errs = [e for c in cs for e in c.get("error_samples", [])][:5]
    for e in errs: print(f"  - {name}: {e}")
print(open(f"{out}/peak.txt").read().strip(), f"({int(open(f'{out}/peak.txt').read().split()[1]) / M:.2f} MiB)")
PY
