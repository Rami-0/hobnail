#!/bin/sh
# A throwaway Hobnail instance on a fake Docker (web/e2e/fake-docker.mjs):
# 150 containers in 13 stacks with moving CPU / memory, for the browser and
# performance checks. Everything runs in containers; nothing on the host
# but a temp dir.
#   HB_PASSWORD=... web/e2e/fake-instance.sh start <hobnail image> <port> [fake-docker options...]
#   web/e2e/fake-instance.sh stop <port>
# e.g.
#   HB_PASSWORD=... web/e2e/fake-instance.sh start hobnail-p0:6d7d101 3440 --churn 20
#   ... checks against http://127.0.0.1:3440 ...
#   web/e2e/fake-instance.sh stop 3440
# start: makes a temp dir (data/ and fake/), runs the fake Docker in
# node:26-alpine as container hobnail-p0-h-fd-<port> (socket and cgroup
# files in fake/, the repo mounted read-only), sets the password (from
# HB_PASSWORD; without it the instance has none), then runs the image as
# hobnail-p0-h-<port> on 127.0.0.1:<port> with RH_DOCKER_SOCKET and
# RH_HOST_SYS pointing at the fake (RH_HOST_PROC stays the container's own
# /proc). Waits for /healthz and for the first container list.
# stop: removes both containers and the temp dir. Refuses port 3200.
set -eu
dir=$(cd "$(dirname "$0")/../.." && pwd)
cmd=${1:-}; port=${3:-${2:-}}
usage() { echo "usage: HB_PASSWORD=... $0 start <hobnail image> <port> [fake-docker options]  |  $0 stop <port>" >&2; exit 2; }
case $cmd in
  start) [ $# -ge 3 ] || usage; image=$2; port=$3; shift 3 ;;
  stop) [ $# -eq 2 ] || usage; port=$2 ;;
  *) usage ;;
esac
case $port in ''|*[!0-9]*) usage ;; esac
[ "$port" != 3200 ] || { echo "refusing port 3200: that is the live instance" >&2; exit 2; }
fd=hobnail-p0-h-fd-$port; hb=hobnail-p0-h-$port

stop() {
  tmp=$(docker inspect -f '{{ index .Config.Labels "io.hobnail.e2e.dir" }}' "$fd" 2>/dev/null || true)
  docker rm -f "$hb" "$fd" >/dev/null 2>&1 || true
  # The fake wrote its files as the calling user; the dir is ours to remove.
  case $tmp in */hobnail-fake.*) rm -rf "$tmp" ;; esac
}
if [ "$cmd" = stop ]; then stop; echo "stopped $hb, $fd"; exit 0; fi

stop
tmp=$(mktemp -d "${TMPDIR:-/tmp}/hobnail-fake.XXXXXX")
mkdir -p "$tmp/data" "$tmp/fake"
uid="$(id -u):$(id -g)"
docker run -d --name "$fd" --label "io.hobnail.e2e.dir=$tmp" -u "$uid" --init \
  -v "$dir":/repo:ro -v "$tmp/fake":/fake \
  node:26-alpine node /repo/web/e2e/fake-docker.mjs /fake/docker.sock --sys /fake/sys --quiet "$@" >/dev/null
i=0; until [ -S "$tmp/fake/docker.sock" ]; do i=$((i+1)); [ $i -lt 100 ] || { docker logs "$fd" >&2; stop; echo "fake docker did not start" >&2; exit 1; }; sleep 0.1; done
docker logs "$fd"
if [ -n "${HB_PASSWORD:-}" ]; then
  printf '%s\n' "$HB_PASSWORD" | docker run --rm -i -u "$uid" -v "$tmp/data":/data -e RH_DATA_DIR=/data "$image" set-password >/dev/null
fi
docker run -d --name "$hb" -u "$uid" -p "127.0.0.1:$port:3200" \
  -v "$tmp/data":/data -v "$tmp/fake":/fake:ro \
  -e RH_LISTEN=0.0.0.0:3200 -e RH_HOSTNAME=homelab -e RH_DATA_DIR=/data \
  -e RH_DOCKER_SOCKET=/fake/docker.sock -e RH_HOST_SYS=/fake/sys -e RH_HOST_PROC=/proc \
  "$image" serve >/dev/null
i=0; until curl -fsS "http://127.0.0.1:$port/healthz" >/dev/null 2>&1; do i=$((i+1)); [ $i -lt 100 ] || { docker logs "$hb" >&2; echo "hobnail did not start" >&2; exit 1; }; sleep 0.1; done
i=0; until curl -fsS "http://127.0.0.1:$port/api/state" 2>/dev/null | grep -q '"svc001"'; do i=$((i+1)); [ $i -lt 100 ] || { echo "warning: /api/state does not list svc001 yet" >&2; break; }; sleep 0.1; done
echo "hobnail $image on http://127.0.0.1:$port (fake docker $fd, files in $tmp)"
