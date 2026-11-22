#!/bin/sh
# The cross-browser matrix: Chromium, Firefox and WebKit in the official
# Playwright image, against throwaway Hobnail instances on a fake Docker
# (web/e2e/fake-instance.sh) with fake provider servers. No Node and no
# browsers on the host; nothing is added to web/package.json.
#   scripts/e2e-matrix.sh <hobnail image>
# Environment (all optional):
#   ENGINES   "chromium firefox webkit"
#   PLAN      a file of "unit profile theme motion" lines (default: below)
#   FLOWS     comma-separated flow names to run (default: all)
#   OUT       output dir (default: a new dir under ${TMPDIR:-/tmp})
#   PW_REBUILD=1  reinstall the test tools volume
# Each engine gets its own port (3395, 3396, 3397) and runs its units in
# order; the engines run in parallel. Every unit starts a fresh instance.
# Results: $OUT/<engine>-<profile>-<unit>-<theme>.json, $OUT/<engine>.log,
# $OUT/summary.txt; screenshots of failing states in $OUT/shots.
# See web/e2e/matrix/README.md.
set -eu
[ $# -eq 1 ] || { echo "usage: scripts/e2e-matrix.sh <hobnail image>" >&2; exit 2; }
image=$1
dir=$(cd "$(dirname "$0")/.." && pwd)
PW_IMAGE=mcr.microsoft.com/playwright:v1.64.1-noble
VOL=hobnail-m6-pw
ENGINES=${ENGINES:-chromium firefox webkit}
docker image inspect "$image" >/dev/null || { echo "matrix: no image $image" >&2; exit 2; }
case $image in hobnail:*|hobnail) echo "matrix: refusing the live image name $image; tag a copy" >&2; exit 2 ;; esac

# Test tools (playwright-core, axe-core) from web/e2e/matrix/package-lock.json
# into a named volume, with install scripts off; reinstalled when the lock
# file changes.
lock=$(sha256sum "$dir/web/e2e/matrix/package-lock.json" | cut -c1-16)
have=$(docker run --rm -v "$VOL":/pw "$PW_IMAGE" cat /pw/.lock 2>/dev/null || true)
if [ -n "${PW_REBUILD:-}" ] || [ "$have" != "$lock" ]; then
  echo "matrix: installing test tools into volume $VOL" >&2
  docker run --rm -v "$VOL":/pw -v "$dir/web/e2e/matrix":/src:ro -w /pw -e HOME=/tmp "$PW_IMAGE" sh -c \
    "rm -rf /pw/node_modules && cp /src/package.json /src/package-lock.json /src/.npmrc /pw/ && npm ci --ignore-scripts --no-audit --no-fund --loglevel=error && echo $lock > /pw/.lock" >&2
fi

OUT=${OUT:-$(mktemp -d "${TMPDIR:-/tmp}/hobnail-matrix.XXXXXX")}
mkdir -p "$OUT"; OUT=$(cd "$OUT" && pwd)
# A random fixture password for the owner and invited users; never printed.
HB_PASSWORD=$(head -c 24 /dev/urandom | base64 | tr -d '/+=' | cut -c1-24)
export HB_PASSWORD

if [ -n "${PLAN:-}" ]; then plan=$(cat "$PLAN"); else
plan=$(cat <<'PLAN'
flows 390 dark reduce
flows tablet-p light no-preference
flows tablet-l dark no-preference
flows 1440 light no-preference
skip 390 light no-preference
skip 1440 dark no-preference
sweep 360 light no-preference
sweep 390 light no-preference
sweep 412 dark reduce
sweep tablet-p dark reduce
sweep tablet-l light no-preference
sweep 1280 dark reduce
sweep 1440 dark reduce
sweep 1920 light no-preference
sweep 2560 dark no-preference
sweep zoom200 light no-preference
sweep 360 dark reduce
sweep 390 dark reduce
sweep 412 light no-preference
sweep tablet-p light no-preference
sweep tablet-l dark reduce
sweep 1280 light no-preference
sweep 1440 light no-preference
sweep 1920 dark reduce
sweep 2560 light no-preference
sweep zoom200 dark reduce
PLAN
)
fi

engine_run() {
  engine=$1; port=$2
  [ "$port" != 3200 ] || exit 2
  log="$OUT/$engine.log"; : >"$log"
  # The SSE flow drops the network: the browser goes offline and this
  # watcher restarts the throwaway server (never the container "hobnail"),
  # which cuts the open stream; see web/e2e/matrix/README.md.
  ctl="$OUT/ctl-$port"; rm -f "$ctl".*
  ( while [ ! -f "$ctl.stop" ]; do
      if [ -f "$ctl.restart" ]; then rm -f "$ctl.restart"; docker restart -t 1 "hobnail-p0-h-$port" >/dev/null 2>&1; touch "$ctl.done"; fi
      sleep 0.2
    done ) &
  echo "$plan" | while read -r unit profile theme motion; do
    [ -n "$unit" ] || continue
    case $unit in \#*) continue ;; esac
    TMPDIR=${TMPDIR:-/tmp} "$dir/web/e2e/fake-instance.sh" start "$image" "$port" --m2-fixtures --log-fixtures --churn 30 >>"$log" 2>&1 || { echo "FAIL $engine $unit $profile: instance did not start" >>"$log"; continue; }
    control="http://$(docker port "hobnail-p0-h-fd-$port" 8081/tcp | head -1)"
    echo "== $engine $unit $profile $theme $motion" >>"$log"
    docker run --rm --init --network host --shm-size 1g -u "$(id -u):$(id -g)" -e HOME=/tmp \
      -v "$VOL":/pw:ro -v "$dir":/repo:ro -w /repo -v "$OUT":/out -e OUT=/out \
      -e HB_PASSWORD -e ENGINE="$engine" -e PROFILE="$profile" -e UNIT="$unit" -e THEME="$theme" -e MOTION="$motion" -e FLOWS="${FLOWS:-}" \
      "$PW_IMAGE" node /repo/web/e2e/matrix/run.mjs "http://127.0.0.1:$port" "$control" >>"$log" 2>&1 || true
    "$dir/web/e2e/fake-instance.sh" stop "$port" >/dev/null 2>&1 || true
  done
  touch "$ctl.stop"
}

i=0; pids=""
for engine in $ENGINES; do
  engine_run "$engine" $((3395 + i)) & pids="$pids $!"
  i=$((i + 1))
done
trap 'for p in $pids; do kill $p 2>/dev/null; done; for n in 0 1 2; do "$dir/web/e2e/fake-instance.sh" stop $((3395 + n)) >/dev/null 2>&1; done' INT TERM
for p in $pids; do wait "$p" || true; done

docker run --rm -u "$(id -u):$(id -g)" -v "$dir":/repo:ro -v "$OUT":/out "$PW_IMAGE" node /repo/web/e2e/matrix/summary.mjs /out >"$OUT/summary.txt" 2>&1 || true
cat "$OUT/summary.txt"
echo "matrix: output in $OUT" >&2
! grep -q '^RESULT: FAIL' "$OUT/summary.txt"
