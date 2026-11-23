#!/bin/sh
# A throwaway Hobnail instance on a fake Docker (web/e2e/fake-docker.mjs):
# 150 containers in 13 stacks with moving CPU / memory, for the browser and
# performance checks. Everything runs in containers; nothing on the host
# but a temp dir.
#   web/e2e/fake-instance.sh start <hobnail image> <port> [fake-docker options...]
#   web/e2e/fake-instance.sh stop <port>
# e.g.
#   web/e2e/fake-instance.sh start hobnail-p0:6d7d101 3440 --churn 20
#   ... checks against http://127.0.0.1:3440 ...
#   web/e2e/fake-instance.sh stop 3440
# start: makes a temp dir (data/ and fake/), runs the fake Docker in
# node:26-alpine as container hobnail-p0-h-fd-<port> (socket and cgroup
# files in fake/, the repo mounted read-only), then runs the image as
# hobnail-p0-h-<port> on 127.0.0.1:<port> with RH_DOCKER_SOCKET and
# RH_HOST_SYS pointing at the fake. The checks onboard the owner or sign in.
# stop: removes both containers and the temp dir. Refuses port 3200.
set -eu
dir=$(cd "$(dirname "$0")/../.." && pwd)
cmd=${1:-}; port=${3:-${2:-}}
usage() { echo "usage: $0 start <hobnail image> <port> [fake-docker options]  |  $0 stop <port>" >&2; exit 2; }
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
m2=0; socket=/fake/docker.sock
for option in "$@"; do
  [ "$option" != --m2-fixtures ] || m2=1
  [ "$option" != --docker-unavailable ] || socket=/fake/unavailable.sock
done
tmp=$(mktemp -d "${TMPDIR:-/tmp}/hobnail-fake.XXXXXX")
mkdir -p "$tmp/data" "$tmp/fake"
uid="$(id -u):$(id -g)"
fixture_port=""
if [ "$m2" = 1 ]; then
  fixture_port="-p 127.0.0.1::8081"
  cat >"$tmp/data/config.toml" <<'TOML'
[http]
allow_hosts = ["127.0.0.1"]

[[widgets]]
id = "m2-reading"
type = "custom-api"
title = "Fixture reading"
url = "http://127.0.0.1:8081/fixture/m2/reading"
refresh = "1s"
template = "{{reading}}"
fields = { reading = "$.reading" }
status = { field = "_status", ok = [200] }
TOML
fi
docker run -d --name "$fd" --label "io.hobnail.e2e.dir=$tmp" -u "$uid" --init \
  -p "127.0.0.1:$port:8080" $fixture_port \
  -v "$dir":/repo:ro -v "$tmp/fake":/fake \
  node:26-alpine node /repo/web/e2e/fake-docker.mjs /fake/docker.sock --sys /fake/sys --quiet "$@" >/dev/null
i=0; until [ -S "$tmp/fake/docker.sock" ]; do i=$((i+1)); [ $i -lt 100 ] || { docker logs "$fd" >&2; stop; echo "fake docker did not start" >&2; exit 1; }; sleep 0.1; done
docker logs "$fd"
docker run -d --name "$hb" -u "$uid" --network "container:$fd" \
  -v "$tmp/data":/data -v "$tmp/fake":/fake:ro \
  -e RH_LISTEN=0.0.0.0:8080 -e RH_HOSTNAME=homelab -e RH_DATA_DIR=/data \
  -e RH_DOCKER_SOCKET="$socket" -e RH_HOST_SYS=/fake/sys -e RH_HOST_PROC=/proc \
  "$image" serve >/dev/null
i=0; until curl -fsS "http://127.0.0.1:$port/healthz" >/dev/null 2>&1; do i=$((i+1)); [ $i -lt 100 ] || { docker logs "$hb" >&2; echo "hobnail did not start" >&2; exit 1; }; sleep 0.1; done

echo "hobnail $image on http://127.0.0.1:$port (fake docker $fd, files in $tmp)"

if [ "$m2" = 1 ]; then
  echo "M2 fixture control: http://$(docker port "$fd" 8081/tcp)"
fi
