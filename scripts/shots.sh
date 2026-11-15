#!/bin/bash
# Screenshot matrix for visual review (headless Chromium; not stored in the repo).
#   scripts/shots.sh OUTDIR [BASE_URL] [path ...]
# Default paths: / /containers /history /alerts /login /settings
# Widths 1440, 900, 390; dark and light. Needs the playwright headless shell.
set -u
out=${1:?outdir}; base=${2:-http://127.0.0.1:3200}; shift 2 2>/dev/null || shift $#
paths=("$@"); [ ${#paths[@]} -eq 0 ] && paths=(/ /containers /history /alerts /login /settings)
CH=${CH:-$HOME/.cache/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-linux64/chrome-headless-shell}
mkdir -p "$out"
for p in "${paths[@]}"; do
  name=$(echo "$p" | tr '/?=&' '____' | sed 's/^_$/_overview/')
  for w in 1440 900 390; do
    h=$([ $w = 390 ] && echo 844 || echo 900)
    for scheme in dark light; do
      v=$([ $scheme = dark ] && echo 0 || echo 1)
      timeout 40 "$CH" --no-sandbox --disable-gpu --hide-scrollbars --window-size=$w,$h \
        --timeout=4000 --blink-settings=preferredColorScheme=$v \
        --screenshot="$out/${name}-${w}-${scheme}.png" "$base$p" >/dev/null 2>&1 || echo "FAILED $p $w $scheme"
    done
  done
done
ls "$out" | wc -l
