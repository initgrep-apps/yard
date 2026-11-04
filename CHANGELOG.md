# Changelog

## 1.0.0 (2026-11-04)

First stable release. Yard compresses images, video, PDF, and Office documents
offline, to per-category size limits, without uploading files.

### Highlights

- **Category targets** — images, video, PDF, and documents each get a size limit;
  defaults are editable in Settings.
- **Bundled engines** — libvips, FFmpeg, qpdf, and Ghostscript ship with the app.
  No downloads, accounts, or network access at runtime.
- **Originals are never modified** — compressed copies go to a separate output
  folder; metadata is stripped and HEIC/HEIF becomes JPEG, MOV becomes MP4.
- **Batch workflow** — parallel compression with per-file progress, per-file
  cancel, and a history audit trail with 90-day retention by default.
- **Native installers** — macOS arm64/Intel DMG and Windows x64/arm64
  NSIS installers, built and gated on native CI runners.
- **Offline by design** — no telemetry, no auto-update, no cloud processing.

### Platforms

- macOS 11+ (Apple Silicon and Intel)
- Windows 10+ (x64 and arm64)

### Known limitations

- macOS builds are ad-hoc signed and not notarized; first launch needs the
  documented quarantine removal.
- Windows builds are unsigned, so SmartScreen warns on first launch.
- Linux builds from source but is not packaged.

## 0.1.0-alpha.1 (2026-10-17)

First development release, built and verified by the new CI/CD pipelines.

### Features

- 2x2 native installer matrix: macOS (arm + intel) and Windows (x64 + arm), each row unfreezing its
  sidecar engines from the `sidecars-20261017` snapshot release and gating on the full build + test suite
  before tauri-action uploads to the draft ([release run 39065895512]: 4/4 green, 10 draft assets).
- Snapshot-once engine pipeline: `freeze.yml` pins homebrew-core per arch, stages and audits all four
  triples on GitHub-hosted runners, and publishes the exact-8-asset `sidecars-*` release that CI and
  the release rows consume; the local machine stays development-only.

### Fixes

- Release version check accepts semver prerelease tags beyond `-rcN` (alpha/beta).
- Intel mac runner test portability: mac fixtures are arch-explicit, bundle scripts are triple-safe,
  and the rustc-derived triple in `bundle-pdf` staging-gate tests is stubbed to the fixture triple.
