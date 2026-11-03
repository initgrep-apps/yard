#!/usr/bin/env bash
# verify-dev.sh — smoke-check that `yarn dev` serves the app shell and that
# tauri.conf.json declares the intentionally untitled window (macOS overlay
# chrome). Exits 0 on success, 1 on failure. Does NOT launch the Tauri webview
# (impractical in CI); instead validates the dev server payload + Tauri
# manifest, which is the actionable surface for the dev smoke check.
set -euo pipefail

DEV_URL="http://localhost:5173"
TAURI_CONF="src-tauri/tauri.conf.json"
EXPECTED_TITLE=""

fail() { echo "verify-dev: FAIL — $*" >&2; exit 1; }

# 1. tauri.conf.json must exist and declare the intentionally untitled window
# (macOS overlay chrome).
[ -f "$TAURI_CONF" ] || fail "$TAURI_CONF missing"
node -e "
const fs = require('fs');
const cfg = JSON.parse(fs.readFileSync('$TAURI_CONF', 'utf8'));
const win = (cfg.app && cfg.app.windows && cfg.app.windows[0]) || {};
if (win.title !== '$EXPECTED_TITLE') {
  process.stderr.write('verify-dev: window must be intentionally untitled for macOS overlay chrome, got \"' + String(win.title) + '\"\\n');
  process.exit(1);
}
const csp = (cfg.app && cfg.app.security && cfg.app.security.csp) || null;
if (!csp || csp === 'null') {
  process.stderr.write('verify-dev: CSP must be set\\n');
  process.exit(1);
}
console.log('verify-dev: tauri.conf.json ok (intentionally untitled for overlay chrome, csp set)');
" || fail "tauri.conf.json validation failed"

# 2. Start vite dev server in background, curl root, kill, exit 0/1.
echo "verify-dev: starting vite dev server..."
# start vite in background, writing its pid to a file so we can clean it up
yarn dev > /tmp/verify-dev.log 2>&1 &
DEV_PID=$!
# give vite a moment to bind
for _ in $(seq 1 30); do
  if curl -fsS "$DEV_URL" > /dev/null 2>&1; then break; fi
  sleep 0.5
done

cleanup() {
  # vite spawns a tree; kill the process group
  pkill -P "$DEV_PID" 2>/dev/null || true
  kill "$DEV_PID" 2>/dev/null || true
}
trap cleanup EXIT

BODY="$(curl -fsS "$DEV_URL" 2>/dev/null || true)"
[ -n "$BODY" ] || fail "no response from $DEV_URL (vite did not start)"

echo "$BODY" | grep -q '<div id="root">' || fail "response missing <div id=\"root\">"
echo "$BODY" | grep -q '/src/main.tsx' || fail "response missing /src/main.tsx script reference"

# 3. Served source modules: App.tsx renders the app shell, index.css has the
# neutral oklch primary token (guards against font/theme regression).
APP_SRC="$(curl -fsS "$DEV_URL/src/App.tsx" 2>/dev/null || true)"
[ -n "$APP_SRC" ] || fail "no response from $DEV_URL/src/App.tsx"
echo "$APP_SRC" | grep -q 'Sidebar' || fail "App.tsx missing Sidebar reference"

CSS_SRC="$(curl -fsS "$DEV_URL/src/index.css" 2>/dev/null || true)"
[ -n "$CSS_SRC" ] || fail "no response from $DEV_URL/src/index.css"
echo "$CSS_SRC" | grep -q '0.205 0 0' || fail "index.css missing neutral light --primary oklch value"

echo "verify-dev: PASS — dev server serves root div + main.tsx; App.tsx has Sidebar; index.css has neutral primary; tauri.conf.json valid"
exit 0