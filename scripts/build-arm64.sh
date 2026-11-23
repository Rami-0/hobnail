#!/bin/sh
# Build the linux/arm64 image from deploy/arm64.Dockerfile (Rust
# cross-compiled on this machine) and load it into the local Docker as
# hobnail:arm64-<sha>. It builds and tags only; nothing is deployed.
#   scripts/build-arm64.sh
# Env: RH_BUILDER  buildx builder to use (default hobnail-arm64, created with
#                  the docker-container driver if missing; its BuildKit
#                  emulates the two short arm64 steps, so the host needs no
#                  binfmt registration),
#      TAG         image tag (default hobnail:arm64-<short sha>).
# About 10 minutes on a 4-core amd64 host. Run the amd64 production build
# (scripts/deploy.sh, or docker build .) for fmt, clippy and the tests.
set -eu
cd "$(dirname "$0")/.."
sha=$(git rev-parse --short HEAD 2>/dev/null || echo nogit)
git diff --quiet HEAD -- src web build.rs Cargo.toml Cargo.lock deploy/arm64.Dockerfile .dockerignore 2>/dev/null || sha="$sha-dirty"
built=$(date -u +%Y-%m-%dT%H:%MZ)
builder=${RH_BUILDER:-hobnail-arm64}
tag=${TAG:-hobnail:arm64-$sha}
case $tag in hobnail:latest|hobnail) echo "build-arm64: refusing to overwrite $tag" >&2; exit 2 ;; esac
docker buildx inspect "$builder" >/dev/null 2>&1 \
  || docker buildx create --name "$builder" --driver docker-container >/dev/null
echo "==> building $tag (linux/arm64) with builder $builder"
docker buildx build --builder "$builder" --platform linux/arm64 \
  -f deploy/arm64.Dockerfile \
  --build-arg RH_GIT_SHA="$sha" --build-arg RH_BUILD_TIME="$built" \
  -t "$tag" --load .
echo "==> $(docker image inspect -f '{{.Os}}/{{.Architecture}}' "$tag") $tag"
