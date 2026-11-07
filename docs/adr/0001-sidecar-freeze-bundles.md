# Sidecar freeze bundles instead of staging engines on CI runner hosts

CI release builds cannot stage vips and gs from Homebrew deterministically: the manifest pins exact formula versions, and runner brew revisions drift, so every consumer run risks a loud break. We freeze the staged binaries once per manifest bump into per-triple archives on a `sidecars-*` GitHub Release; consumers download, verify, and unstage that bundle instead of staging engines themselves.

## Considered Options

- **Freeze bundles (chosen)** — deterministic release builds, fast CI; only the manual freeze run can hit brew drift, and it fails loudly there. Cost: a version bump requires a freeze before the next tag.
- **Stage on every CI run** — at the mercy of runner host state. October 2026: eight red runs, stale runner indexes resolving old formulas, Intel closures falling into hours-long source builds (llvm included), one six-hour hang.

## Ruling (2026-10-16): one freeze captures all four triples

One `freeze.yml` run stages and bundles `aarch64-apple-darwin`, `x86_64-apple-darwin`, and both Windows triples, then publishes 8 assets (4 tarballs + 4 digests). Every consumer — PR gate and release rows — restores its own triple by unfreezing; no consumer fetches or stages engines itself, and brew runs only inside `freeze.yml`, only on mac rows.

Windows rows ship binaries-only roots: `binaries/engines/<engine>/` (exe + recursive DLL tree, DLL presence enforced) for vips/qpdf/gs, plus flat `binaries/<dest>-<triple>.exe` for every entry. ffmpeg/ffprobe are static and ship as the flat `externalBin` pair only. msys2's Ghostscript carries no Resource tree, so Windows gs runs on its compiled-in resources (ROM model): no `resources` key, no `gs-resources` mapping (`tauri.windows.conf.json` null-merge), and its DLL closure is pinned as 48 `extraPackages` rows.

## Ruling (2026-10-17): GitHub-hosted runners only

The local machine is development only. Each triple builds natively on its matching hosted label: `macos-15`, `macos-15-intel`, `windows-2025`, `windows-11-arm`. Cross-compiling Intel macOS on silicon is rejected while `macos-15-intel` exists (supported until August 2027; revisit at EOL by replacing the Intel path wholesale, not by moving its pin forward).

## The homebrew-core pin (mac rows only, per arch)

Mac freeze rows install vips and ghostscript from `homebrew-core` at the commit recorded for their triple in `scripts/ci/homebrew-core-pins.json`; no CI step runs `brew update`, so the formula set is fixed by the pins. A pin must satisfy: formula versions equal that triple's manifest `expectVersion`, and bottles exist for the runner's tier (brew's fallback reaches newer macOS from older bottles only). Bumping a pin is a deliberate act tied to an `expectVersion` change — never a drift fix. Arm and Intel pins move independently.

Intel is frozen at vips 8.18.6 / gs 10.07.1: homebrew-core stopped publishing x86_64 (sonoma) bottles between 2026-09-10 and 09-11, and vips 8.18.7 arrived 16 days later, so no commit can ever supply 8.18.7 Intel bottles. Never bump Intel engines without a human re-verifying the whole closure's sonoma bottles. Operator steps: `docs/cicd-process.md` §4.

## Consequences

- A version bump requires a manual freeze before the next tag; a missing `sidecars-*` release fails the release preflight loudly.
- Freeze jobs are bounded at 40 minutes so the brew-source-build class dies as a bounded failure instead of a runaway job.
- Same-day `sidecars-*` collisions are manual recovery; finalize reuses a leftover draft idempotently and never publishes short of the exact-8 asset set.
