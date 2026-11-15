#!/bin/bash
# Measure hobnail against GOALS.md section 1. Prints a markdown block.
#   scripts/measure.sh            (uses the running hobnail container; URL=http://host:3200 to override)
# Cold start restarts *our* container only (5 runs, median).
set -eu
C=hobnail
URL=${URL:-http://127.0.0.1:3200}
TS=${TS:-$URL}
mib() { awk '{ v=$1; u=$1; gsub(/[0-9.]/,"",u); gsub(/[^0-9.]/,"",v);
  if (u=="KiB") v/=1024; else if (u=="GiB") v*=1024; else if (u=="B") v/=1048576; printf "%.2f", v }'; }
rss() { docker stats --no-stream --format '{{.MemUsage}}' $C | cut -d/ -f1 | mib; }
cpu() { docker stats --no-stream --format '{{.CPUPerc}}' $C; }

sha=$(curl -fs $URL/api/state | sed -n 's/.*"sha": "\([^"]*\)".*/\1/p' | head -1)
img=$(docker image inspect hobnail:latest --format '{{.Size}}')
disk=$(docker images hobnail:latest --format '{{.Size}}' | head -1)

# Cold start: process start (State.StartedAt) -> first successful /healthz.
runs=()
for i in 1 2 3 4 5; do
  docker stop -t 5 $C >/dev/null
  docker start $C >/dev/null
  until curl -fs -o /dev/null --max-time 0.2 $URL/healthz 2>/dev/null; do :; done
  end=$(date +%s.%N)
  started=$(date -d "$(docker inspect -f '{{.State.StartedAt}}' $C)" +%s.%N)
  runs+=("$(echo "($end - $started) * 1000" | bc -l | xargs printf '%.0f')")
done
cold=$(printf '%s\n' "${runs[@]}" | sort -n | sed -n 3p)

# Idle: nobody connected, give it 20s to settle.
sleep 20
idle=$(rss); idle_cpu=$(cpu)

# Page load from this host over the LAN address: TTFB and total for '/',
# plus CSS+JS (first paint needs HTML + CSS). 10 runs, median.
pl=(); for i in $(seq 10); do pl+=("$(curl -s -o /dev/null -w '%{time_total}' $URL/)"); done
page=$(printf '%s\n' "${pl[@]}" | sort -n | sed -n 5p | awk '{printf "%.1f", $1*1000}')
ttfb=$(curl -s -o /dev/null -w '%{time_starttransfer}' $URL/ | awk '{printf "%.1f", $1*1000}')
bytes=$(curl -s $URL/ | wc -c)
assets=$(curl -s -o /dev/null -w '%{time_total}' "$URL/static/app.css?v=$sha" | awk '{printf "%.1f", $1*1000}')

# Sampler cost: CPU time of the app over 120 s with nobody viewing. With no
# viewer the only periodic work is the 15 s history sampler (+ docker events,
# alert checks), so this is an upper bound for the sampler's cost.
pid=$(docker inspect -f '{{.State.Pid}}' $C)
tck=$(getconf CLK_TCK)
t0=$(awk '{print $14+$15}' /proc/$pid/stat); sleep 120; t1=$(awk '{print $14+$15}' /proc/$pid/stat)
cpu_ms=$(( (t1 - t0) * 1000 / tck ))
samp_pct=$(echo "scale=3; $cpu_ms / 1200" | bc)            # % of one core
per_sample=$(echo "scale=1; $cpu_ms / 8" | bc)              # 8 samples in 120 s

# History database.
db=/opt/stacks/hobnail/data/hobnail.db
dbsize=$(du -cb $db $db-wal 2>/dev/null | tail -1 | cut -f1)
first=$(curl -s "$URL/api/history/system?metric=cpu&range=30d" | sed -n 's/.*"points":\[\[\([0-9]*\),.*/\1/p')
age_h=$(echo "scale=2; ($(date +%s) - ${first:-$(date +%s)}) / 3600" | bc)

# Other pages (median of 5, curl).
pages=""
for p in /containers /history /alerts; do
  pl=(); for i in $(seq 5); do pl+=("$(curl -s -o /dev/null -w '%{time_total}' $URL$p)"); done
  pages="$pages $p $(printf '%s\n' "${pl[@]}" | sort -n | sed -n 3p | awk '{printf "%.1f", $1*1000}') ms;"
done

# One active viewer: an SSE stream open for 30s (refresh loop running).
curl -sN $TS/api/stream >/dev/null & v=$!
sleep 30
one=$(rss); one_cpu=$(cpu)
kill $v 2>/dev/null || true

cat <<OUT
### $(date -u +%Y-%m-%dT%H:%MZ) — build \`$sha\`

| Metric | Measured | Target |
|---|---|---|
| Idle RAM (no viewers, 20 s after start) | **${idle} MiB** (CPU ${idle_cpu}) | ≤ 15 MiB |
| RAM with 1 active viewer (SSE open 30 s) | **${one} MiB** (CPU ${one_cpu}) | ≤ 25 MiB |
| Image size | **${disk}** unpacked, $(echo "$img" | awk '{printf "%.2f MB", $1/1e6}') compressed | ≤ 20 MB |
| Cold start → first request (median of 5: ${runs[*]} ms) | **${cold} ms** | < 200 ms |
| Page load \`/\` (median of 10, LAN, curl) | **${page} ms** total, TTFB ${ttfb} ms, ${bytes} B; app.css ${assets} ms | < 150 ms |
| Other pages (median of 5) |${pages} | — |
| App CPU with no viewer, 120 s (sampler + events + alerts) | **${samp_pct} %** of one core, ~${per_sample} ms per 15 s sample | ~0 % |
| History DB | **$(echo "$dbsize" | awk '{printf "%.2f MiB", $1/1048576}')** after ${age_h} h of data | < 50 MB steady |
OUT
