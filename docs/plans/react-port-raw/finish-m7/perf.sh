#!/bin/bash
# Post-gate browser budgets: the M5 gate procedure (perf-check, app + history,
# two runs) on the final image, throwaway port 3396.
set -u -o pipefail
M=$(cd "$(dirname "$0")" && pwd); cd /home/dev/projects/rust-homepage
export TMPDIR="$M" HB_PASSWORD="$(head -c 24 /dev/urandom | base64 | tr -d '/+=' | cut -c1-24)"
image=$1; port=3396; url=http://127.0.0.1:$port
trap 'web/e2e/fake-instance.sh stop "$port"' EXIT
web/e2e/fake-instance.sh start "$image" "$port" --log-fixtures || exit 1
for run in 1 2; do
  cat /proc/pressure/cpu
  OUT="$M/perf-$run" E2E_ENV=TARGETS TARGETS="app history" scripts/e2e.sh web/e2e/perf-check.mjs "$url" > "$M/perf-$run.log" 2>&1
  echo "perf $run exit $?"
done
