#!/usr/bin/env bash
# verify-theme-tokens.sh — assert all 18 core shadcn oklch tokens present in
# src/index.css, plus :root --radius and exact neutral --primary values for both
# light and dark. Exits 0 on success, 1 on failure. Guards against theme token
# drift (Task 2 acceptance criterion: "full 18+ oklch tokens").
set -euo pipefail

CSS="src/index.css"

fail() { echo "verify-theme: FAIL — $*" >&2; exit 1; }

[ -f "$CSS" ] || fail "$CSS missing"

tokens="background foreground card card-foreground popover popover-foreground primary primary-foreground secondary secondary-foreground muted muted-foreground accent accent-foreground destructive border input ring"
for tok in $tokens; do
  grep -q -- "--$tok:" "$CSS" || fail "missing --$tok"
done

grep -q -- "--radius: 0.625rem" "$CSS" || fail "missing --radius"

grep -q -- "--primary: oklch(0.205 0 0)" "$CSS" || fail "light --primary wrong"
grep -q -- "--primary: oklch(0.922 0 0)" "$CSS" || fail "dark --primary wrong"

echo "verify-theme: PASS — all theme tokens present"
exit 0