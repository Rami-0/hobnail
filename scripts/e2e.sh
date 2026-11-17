#!/bin/sh
# Run a browser check (a raw-DevTools .mjs script) inside a container: no Node
# and no Chromium on the host.
#   scripts/e2e.sh <check.mjs, relative to the repo> [args...]
# e.g.
#   HB_PASSWORD=... scripts/e2e.sh web/e2e/app-check.mjs http://127.0.0.1:3420
#   HB_PASSWORD=... scripts/e2e.sh scripts/a11y-check.mjs http://127.0.0.1:3420 <container>
# The first run builds a small local image, hobnail-e2e:p0 (node:26-alpine
# pinned by digest + Alpine's chromium + fonts), later runs reuse it.
# `docker rmi hobnail-e2e:p0` removes it; E2E_REBUILD=1 rebuilds it.
#
# The container: --network host (so 127.0.0.1 instances are reachable), the
# repo mounted read-only at /repo (cwd), the calling uid:gid, HOME=/tmp,
# CH=<chromium binary>. Passed through when set: HB_PASSWORD, HB_COOKIE,
# WIDTHS, REPORT, plus any names listed in E2E_ENV (space separated, e.g.
# E2E_ENV="CPU_SECONDS PARTS"). OUT (default: a fresh directory under
# ${TMPDIR:-/tmp}) is mounted read-write at /out and the check sees OUT=/out;
# the host path is printed at the end. Exit status is the check's.
set -eu
[ $# -ge 1 ] || { echo "usage: scripts/e2e.sh <check.mjs relative to the repo> [args...]" >&2; exit 2; }
dir=$(cd "$(dirname "$0")/.." && pwd)
check=$1; shift
case $check in /*) echo "e2e: give the check relative to the repo, not $check" >&2; exit 2 ;; esac
[ -f "$dir/$check" ] || { echo "e2e: no such file: $dir/$check" >&2; exit 2; }

IMG=hobnail-e2e:p0
REV=2 # bump when the Dockerfile below changes: an older image is rebuilt
have=$(docker image inspect -f '{{ index .Config.Labels "io.hobnail.e2e.rev" }}' "$IMG" 2>/dev/null || true)
if [ -n "${E2E_REBUILD:-}" ] || [ "$have" != "$REV" ]; then
  echo "e2e: building $IMG (once)" >&2
  docker build -q --build-arg REV=$REV -t "$IMG" - >&2 <<'DOCKERFILE'
FROM node:26-alpine@sha256:6bd9ab43c0ec0190ca7b2d193906cb8b7edf6e7f4afa43ddeccd6452710bffb5
RUN apk add --no-cache chromium font-noto font-dejavu \
 && ln -sf "$(command -v chromium || command -v chromium-browser)" /usr/local/bin/e2e-chromium \
 && /usr/local/bin/e2e-chromium --version
# The existing checks start $CH without --headless (they expect Playwright's
# headless shell); Alpine's chromium needs it, and a small /dev/shm is fine
# with --disable-dev-shm-usage. The launcher execs, so a kill reaches Chromium.
RUN printf '#!/bin/sh\nexec /usr/lib/chromium/chromium-launcher.sh --headless --disable-dev-shm-usage "$@"\n' > /usr/local/bin/e2e-headless \
 && chmod 755 /usr/local/bin/e2e-headless
ARG REV
LABEL io.hobnail.e2e.rev=$REV
ENV CH=/usr/local/bin/e2e-headless HOME=/tmp
DOCKERFILE
fi

OUT=${OUT:-$(mktemp -d "${TMPDIR:-/tmp}/hobnail-e2e.XXXXXX")}
mkdir -p "$OUT"
OUT=$(cd "$OUT" && pwd)

envs=""
for v in HB_PASSWORD HB_COOKIE WIDTHS REPORT ${E2E_ENV:-}; do
  # Only names that are set; the value itself never goes on the command line.
  if eval "[ -n \"\${$v+x}\" ]"; then envs="$envs -e $v"; fi
done

tty=; [ -t 0 ] && [ -t 1 ] && tty=-t
set +e
# shellcheck disable=SC2086
docker run --rm -i $tty --init \
  --network host --shm-size 512m \
  -u "$(id -u):$(id -g)" \
  -v "$dir":/repo:ro -w /repo \
  -v "$OUT":/out -e OUT=/out \
  $envs \
  "$IMG" node "/repo/$check" "$@"
rc=$?
set -e
echo "e2e: exit $rc; output dir $OUT" >&2
exit $rc
