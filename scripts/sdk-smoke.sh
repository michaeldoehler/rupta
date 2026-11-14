#!/usr/bin/env bash
# End-to-end check with the official open-source SDKs: starts a rupta
# server on a free port with a temporary data directory, seeds a project
# and key, runs the example programs in scripts/sdk-smoke/ (Node, PHP,
# Python, and the browser SDK in headless Chrome when Chrome is found),
# stops the server and checks what was stored.
#
# Requirements: a built server (cargo build -p rupta-server), node + npm,
# php + composer, python3. Missing language toolchains are skipped with a
# warning unless SMOKE_REQUIRE_ALL=1.
#
# Environment:
#   RUPTA_BIN            server binary (default target/debug/rupta)
#   SMOKE_SDK_DIR        where SDKs are installed (default: a temp dir;
#                        set it to reuse installs between runs)
#   SMOKE_CAPTURE_DIR    also record every request (via scripts/sdk-smoke/capture.py)
#   SMOKE_REQUIRE_ALL=1  fail instead of skipping a missing toolchain
#   SMOKE_KEEP=1         keep the temporary directory
#   NODE_SDK_VERSION, BROWSER_SDK_VERSION, PHP_SDK_VERSION, PYTHON_SDK_VERSION
#                        package versions (default: latest)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
EXAMPLES="$ROOT/scripts/sdk-smoke"
BIN="${RUPTA_BIN:-$ROOT/target/debug/rupta}"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/rupta-smoke.XXXXXX")"
SDK_DIR="${SMOKE_SDK_DIR:-$WORK/sdk}"
SERVER_PID=""
PROXY_PID=""
STATIC_PID=""

log() { printf '\033[1m[smoke]\033[0m %s\n' "$*"; }
fail() { printf '\033[31m[smoke] FAIL:\033[0m %s\n' "$*" >&2; exit 1; }

cleanup() {
  for pid in "$STATIC_PID" "$PROXY_PID" "$SERVER_PID"; do
    [ -n "$pid" ] && kill "$pid" 2>/dev/null || true
  done
  if [ "${SMOKE_KEEP:-0}" = "1" ]; then log "kept $WORK"; else rm -rf "$WORK"; fi
}
trap cleanup EXIT

free_port() { python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])'; }

wait_http() {
  for _ in $(seq 1 100); do
    if curl -fs -o /dev/null "$1" 2>/dev/null; then return 0; fi
    sleep 0.1
  done
  return 1
}

skip_or_fail() {
  if [ "${SMOKE_REQUIRE_ALL:-0}" = "1" ]; then fail "$1"; fi
  log "skipping: $1"
}

[ -x "$BIN" ] || fail "server binary $BIN not found; run cargo build -p rupta-server"
log "building the seeding helper"
(cd "$ROOT" && cargo build -q -p rupta-ingest --example ingest_admin)
ADMIN="$ROOT/target/debug/examples/ingest_admin"

DATA="$WORK/data"
KEY="$(python3 -c 'import uuid; print(uuid.uuid4().hex)')"
PROJECT="$("$ADMIN" seed "$DATA" "$KEY")"
PORT="$(free_port)"
log "starting rupta on 127.0.0.1:$PORT (project $PROJECT)"
RUPTA_SECRET_KEY="smoke-test-secret-key-0123456789abcdef" \
RUPTA_DATA_DIR="$DATA" \
RUPTA_LISTEN="127.0.0.1:$PORT" \
RUPTA_PUBLIC_URL="http://127.0.0.1:$PORT" \
RUPTA_DURABILITY=safe \
RUPTA_LOG="${RUPTA_LOG:-warn}" \
  "$BIN" serve >"$WORK/server.log" 2>&1 &
SERVER_PID=$!
wait_http "http://127.0.0.1:$PORT/healthz" || { cat "$WORK/server.log"; fail "server did not start"; }

TARGET_PORT="$PORT"
if [ -n "${SMOKE_CAPTURE_DIR:-}" ]; then
  TARGET_PORT="$(free_port)"
  python3 "$EXAMPLES/capture.py" "$TARGET_PORT" "http://127.0.0.1:$PORT" "$SMOKE_CAPTURE_DIR" &
  PROXY_PID=$!
  sleep 0.5
  log "recording requests to $SMOKE_CAPTURE_DIR"
fi
DSN="http://$KEY@127.0.0.1:$TARGET_PORT/$PROJECT"
export SMOKE_DSN="$DSN"

RAN=()
mkdir -p "$SDK_DIR"

# --- Node and browser ---------------------------------------------------
if command -v node >/dev/null && command -v npm >/dev/null; then
  mkdir -p "$SDK_DIR/js"
  if [ ! -d "$SDK_DIR/js/node_modules/@sentry/node" ]; then
    log "installing the JavaScript SDKs"
    (cd "$SDK_DIR/js" && npm init -y >/dev/null && npm install --silent --no-audit --no-fund \
      "@sentry/node@${NODE_SDK_VERSION:-latest}" "@sentry/browser@${BROWSER_SDK_VERSION:-latest}" esbuild puppeteer-core >/dev/null)
  fi
  cp "$EXAMPLES/node.mjs" "$EXAMPLES/browser.js" "$EXAMPLES/run-browser.mjs" "$SDK_DIR/js/"
  log "running the Node SDK example"
  (cd "$SDK_DIR/js" && node node.mjs) || fail "Node example failed"
  RAN+=(node)

  CHROME="${CHROME:-$(command -v google-chrome || command -v chromium || command -v chromium-browser || true)}"
  if [ -n "$CHROME" ]; then
    WEB="$WORK/web"
    mkdir -p "$WEB"
    (cd "$SDK_DIR/js" && npx esbuild browser.js --bundle --format=iife --outfile="$WEB/bundle.js" --log-level=warning)
    cp "$EXAMPLES/browser.html" "$WEB/index.html"
    echo "window.SMOKE_DSN = \"$DSN\";" >"$WEB/config.js"
    WEB_PORT="$(free_port)"
    python3 -m http.server "$WEB_PORT" --bind 127.0.0.1 --directory "$WEB" >/dev/null 2>&1 &
    STATIC_PID=$!
    wait_http "http://127.0.0.1:$WEB_PORT/index.html" || fail "static server did not start"
    log "running the browser SDK example in headless Chrome"
    # Chrome needs a short temporary path for its profile socket.
    (cd "$SDK_DIR/js" && TMPDIR=/tmp node run-browser.mjs "$CHROME" "http://127.0.0.1:$WEB_PORT/index.html") || fail "browser example failed"
    RAN+=(browser)
  else
    skip_or_fail "no Chrome or Chromium found for the browser SDK"
  fi
else
  skip_or_fail "node/npm not installed"
fi

# --- PHP ------------------------------------------------------------------
if command -v php >/dev/null && command -v composer >/dev/null; then
  mkdir -p "$SDK_DIR/php"
  if [ ! -d "$SDK_DIR/php/vendor/sentry/sentry" ]; then
    log "installing the PHP SDK"
    (cd "$SDK_DIR/php" && composer require --quiet --no-interaction "sentry/sentry:${PHP_SDK_VERSION:-*}")
  fi
  cp "$EXAMPLES/php.php" "$SDK_DIR/php/"
  log "running the PHP SDK example"
  (cd "$SDK_DIR/php" && php php.php) || fail "PHP example failed"
  RAN+=(php)
else
  skip_or_fail "php/composer not installed"
fi

# --- Python ---------------------------------------------------------------
if command -v python3 >/dev/null; then
  if [ ! -x "$SDK_DIR/py/venv/bin/python" ]; then
    log "installing the Python SDK"
    python3 -m venv "$SDK_DIR/py/venv"
    "$SDK_DIR/py/venv/bin/pip" install --quiet --disable-pip-version-check "sentry-sdk${PYTHON_SDK_VERSION:+==$PYTHON_SDK_VERSION}"
  fi
  log "running the Python SDK example"
  "$SDK_DIR/py/venv/bin/python" "$EXAMPLES/python.py" || fail "Python example failed"
  RAN+=(python)
fi

# --- Results ----------------------------------------------------------------
curl -fs "http://127.0.0.1:$PORT/metrics" >"$WORK/metrics.txt" || true
log "stopping the server"
kill -TERM "$SERVER_PID"
wait "$SERVER_PID" || true
SERVER_PID=""
"$ADMIN" summary "$DATA" >"$WORK/summary.json"
cat "$WORK/summary.json"

python3 - "$WORK/summary.json" "${RAN[@]}" <<'PY'
import json, sys
s = json.load(open(sys.argv[1]))
ran = set(sys.argv[2:])
sdk_names = {"node": "sentry.javascript.node", "browser": "sentry.javascript.browser", "php": "sentry.php", "python": "sentry.python"}
by_sdk = dict(s["events_by_sdk"])
errors = []
for lang in ran:
    # Every SDK sent an exception and a message.
    if by_sdk.get(sdk_names[lang], 0) < 2:
        errors.append(f"{lang}: expected at least 2 events, got {by_sdk.get(sdk_names[lang], 0)}")
servers = ran & {"node", "php", "python"}
def at_least(key, n, what):
    if s[key] < n:
        errors.append(f"expected at least {n} {what}, got {s[key]}")
at_least("transactions", len(servers) + (1 if "browser" in ran else 0), "transactions")
at_least("monitors", len(servers), "monitors")
at_least("checkins", len(servers), "check-ins")
at_least("attachments", len(servers), "attachments")
at_least("feedback", len(ran & {"node", "browser"}), "feedback entries")
at_least("feedback_linked", len(ran & {"node", "browser"}), "linked feedback entries")
if ran & {"node", "browser", "python"}:
    at_least("sessions", 1, "sessions")
bad = [o for o in s["outcomes"] if o[1] == "invalid"]
if bad:
    errors.append(f"invalid items: {bad}")
if errors:
    print("\n".join("FAIL: " + e for e in errors))
    sys.exit(1)
print("smoke test passed for: " + ", ".join(sorted(ran)))
PY
