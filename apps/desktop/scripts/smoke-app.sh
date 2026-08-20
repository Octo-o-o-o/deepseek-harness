#!/bin/sh
# Three-gate smoke of the bundled sidecar inside dshd.app.
# Usage: apps/desktop/scripts/smoke-app.sh [path-to-dshd.app]
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
app=${1:-"$(node "$root/scripts/pack-sidecar.mjs" app-path | head -n 1)"}
node="$app/Contents/Resources/bin/node"
bin="$app/Contents/Resources/app/lib/bin.js"
if [ ! -x "$node" ] || [ ! -f "$bin" ]; then
  echo "smoke-app: missing bundled runtime under $app" >&2
  exit 1
fi
home=$(mktemp -d "${TMPDIR:-/tmp}/dshd-smoke.XXXXXX")
cleanup() {
  if [ -n "${pid:-}" ]; then
    kill -TERM "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
  fi
  rm -rf "$home"
}
trap cleanup EXIT INT TERM
PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH DSH_HOME="$home" HOME="$home"
out="$home/out.log"
: >"$out"
"$node" "$bin" web --port 0 --host 127.0.0.1 --no-open >"$out" 2>&1 &
pid=$!
port=""
i=0
while [ "$i" -lt 15 ]; do
  if grep -E 'dsh web: http://127\.0\.0\.1:[0-9]+' "$out" >/dev/null 2>&1; then
    port=$(sed -n 's/.*dsh web: http:\/\/127\.0\.0\.1:\([0-9][0-9]*\).*/\1/p' "$out" | tail -n 1)
    break
  fi
  i=$((i + 1))
  sleep 1
done
if [ -z "$port" ]; then
  echo "smoke-app: no ready line" >&2
  tail -n 40 "$out" >&2 || true
  exit 1
fi
echo "smoke-app: ready http://127.0.0.1:$port"
code=$(curl -sS -o "$home/index.html" -w '%{http_code}' "http://127.0.0.1:$port/")
if [ "$code" != "200" ] || ! grep -q '__DSH_BOOT__' "$home/index.html"; then
  echo "smoke-app: GET / failed HTTP $code" >&2
  exit 1
fi
echo "smoke-app: GET / 200 __DSH_BOOT__"
kill -TERM "$pid"
wait "$pid"
status=$?
pid=""
if [ "$status" -ne 0 ]; then
  echo "smoke-app: SIGTERM exit $status" >&2
  exit 1
fi
echo "smoke-app: SIGTERM 0"
echo "smoke-app: ok"
