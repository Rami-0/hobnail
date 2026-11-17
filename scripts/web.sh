#!/bin/sh
# Run npm/node for the React client (web/) inside a Node container: no Node
# on the host. The release-age gate and the no-scripts rule come from
# web/.npmrc and apply to every command.
#   scripts/web.sh ci                 install exactly what web/package-lock.json pins
#   scripts/web.sh run check          lint + type check
#   scripts/web.sh run test           client unit tests
#   scripts/web.sh run build          vite build + gzip copies -> web/dist
#   scripts/web.sh run dev -- --port 5173   (with RH_WEB_PUBLISH=127.0.0.1:5173:5173)
# Env: RH_WEB_IMAGE    the Node image (default: the digest the Dockerfile pins),
#      RH_WEB_PUBLISH  a -p value for the dev server (optional),
#      RH_WEB_NET      a --network value (optional, e.g. host for the dev proxy),
#      RH_DEV_BACKEND  the Hobnail the dev server proxies /api to (e.g. http://127.0.0.1:3401).
set -e
dir=$(cd "$(dirname "$0")/.." && pwd)
img=${RH_WEB_IMAGE:-$(sed -n 's/^FROM --platform=\$BUILDPLATFORM \(node:[^ ]*\) AS web$/\1/p' "$dir/Dockerfile")}
[ -n "$img" ] || img=node:26-alpine
tty=; [ -t 0 ] && tty=-t
set -- npm "$@"
[ "$1 $2" = "npm node" ] && shift  # scripts/web.sh node file.mjs
exec docker run --rm -i $tty \
  -u "$(id -u):$(id -g)" \
  -e HOME=/tmp -e npm_config_cache=/npm-cache ${RH_DEV_BACKEND:+-e RH_DEV_BACKEND} \
  -v rh-npm-cache:/npm-cache \
  ${RH_WEB_PUBLISH:+-p "$RH_WEB_PUBLISH"} ${RH_WEB_NET:+--network "$RH_WEB_NET"} \
  -v "$dir/web":/web -w /web \
  "$img" "$@"
