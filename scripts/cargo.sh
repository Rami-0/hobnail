#!/bin/sh
# Run cargo inside the dev container (no toolchain on the host).
#   scripts/cargo.sh test
#   scripts/cargo.sh clippy --all-targets -- -D warnings
# Env: RH_TARGET_VOL  name of the docker volume for target/ (default
#                    rh-target-<first 12 hex of sha256 of the checkout's absolute path>,
#                    so two checkouts with the same folder name never share one),
#      RH_JOBS        cargo -j (default 2).
set -e
dir=$(cd "$(dirname "$0")/.." && pwd)
vol=${RH_TARGET_VOL:-rh-target-$(printf %s "$dir" | sha256sum | cut -c1-12)}
docker image inspect rh-dev >/dev/null 2>&1 || docker build -q -t rh-dev "$dir/deploy/dev" >/dev/null
docker volume inspect "$vol" >/dev/null 2>&1 || {
  docker volume create "$vol" >/dev/null
  docker run --rm -v "$vol":/t alpine:3.20 chown "$(id -u):$(id -g)" /t
}
exec docker run --rm -i \
  -u "$(id -u):$(id -g)" \
  -e CARGO_HOME=/cargo -e CARGO_BUILD_JOBS="${RH_JOBS:-2}" -e CARGO_TARGET_DIR=/target \
  ${RH_UPDATE_TYPES:+-e RH_UPDATE_TYPES} ${RH_WEB_DIST:+-e RH_WEB_DIST} \
  -v rh-cargo-home:/cargo -v "$vol":/target \
  -v "$dir":/src -w /src \
  rh-dev cargo "$@"
