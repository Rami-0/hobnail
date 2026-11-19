#!/bin/sh
# Build the image (fmt + clippy + tests run inside the build) and (re)deploy
# the compose stack at /opt/stacks/hobnail. Requires explicit confirmation.
set -eu
STACK=${STACK:-/opt/stacks/hobnail}
printf '==> will replace image hobnail:latest, container hobnail, stack %s\n' "$STACK"
if [ "${DEPLOY_YES:-}" != 1 ]; then
  if [ ! -t 0 ]; then
    echo 'Refusing non-interactive deployment; set DEPLOY_YES=1 to confirm.' >&2
    exit 1
  fi
  printf 'Replace this instance? Type yes: '
  read -r answer
  [ "$answer" = yes ] || exit 1
fi
cd "$(dirname "$0")/.."
sha=$(git rev-parse --short HEAD 2>/dev/null || echo nogit)
git diff --quiet HEAD -- src web build.rs Cargo.toml Cargo.lock Dockerfile .dockerignore 2>/dev/null || sha="$sha-dirty"
built=$(date -u +%Y-%m-%dT%H:%MZ)

echo "==> building hobnail $sha"
DOCKER_BUILDKIT=1 docker build \
  --build-arg RH_GIT_SHA="$sha" --build-arg RH_BUILD_TIME="$built" \
  -t hobnail:latest -t "hobnail:$sha" .
# The version line carries the content hash the build verified (stale-binary guard).
echo "==> $(docker run --rm --entrypoint /hobnail "hobnail:$sha" version)"

echo "==> deploying to $STACK"
mkdir -p "$STACK/data"
cp deploy/compose.yaml "$STACK/compose.yaml"
set -- -f "$STACK/compose.yaml"
# Machine-specific overrides (published addresses, host name), kept out of git.
if [ -f deploy/compose.local.yaml ]; then
  cp deploy/compose.local.yaml "$STACK/compose.local.yaml"
  set -- "$@" -f "$STACK/compose.local.yaml"
fi
docker compose "$@" up -d --remove-orphans hobnail

printf "==> waiting for healthy"
i=0
while [ $i -lt 60 ]; do
  s=$(docker inspect -f '{{.State.Health.Status}}' hobnail 2>/dev/null || echo none)
  [ "$s" = healthy ] && break
  printf "."; sleep 1; i=$((i+1))
done
echo " $s"
for addr in $(docker port hobnail 3200/tcp); do
  printf "    http://%s/healthz -> " "$addr"
  curl -fsS -m 3 "http://$addr/healthz" || echo FAILED
done

# Clean up: keep the 3 newest sha tags, remove dangling images from our builds only.
docker images hobnail --format '{{.CreatedAt}}\t{{.Tag}}' | sort -r | cut -f2 \
  | grep -v '^latest$' | tail -n +4 | while read -r t; do docker rmi -f "hobnail:$t" >/dev/null || true; done
docker image prune -f --filter label=io.hobnail.build=1 >/dev/null
echo "==> done: $sha"
