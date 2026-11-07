# Changelog

## Unreleased

### Added

- **Memory budget** — batches are admitted against `clamp(total RAM / 8, 300 MiB,
  2 GiB)` instead of a slot count. Settings → Performance exposes adaptive memory
  and a fixed ceiling; `memory_stats` and `memory:usage` report budget and RSS.
- **Consent gate** — destructive changes (downscale, dropped stream, format change,
  rasterise) are checked before delivery and ask when the matrix allows consent.
  Inline prompts grant once or for the batch; the pre-flight dialog covers HEIC → JPEG
  when its default permission is off. A denied or unanswered rung is removed; the file
  ends as `needs_permission` only when no authorised attempt remains. An earlier
  authorised over-target attempt may still be delivered as best effort.
- **History truth** — file records carry `changes` and `consentSource`; the batch
  outcome counts `needs_permission`.
- **Image long-edge floor** — `qualityFloors.image.minLongEdge` (default 1280 px)
  bounds the dimension ladder.

### Changed

- **Documents stream** — archive entries are copied one at a time instead of
  snapshotting the whole archive. Temp disk grows; RAM no longer does.
- **Under-target copies** — an input already under target is copied byte-for-byte under
  the mapped extension. HEIC/MOV bytes and the original image colour/orientation remain
  until compression actually runs.
- **Images** — every compression attempt uses `vips thumbnail` with EXIF autorotation,
  sRGB output and `keep=icc`. Metadata is no longer blanket-stripped.
- **Video** — one ABR rung then CRF at the lower allowed resolutions; all audio/subtitle
  tracks and chapters are mapped; output is `yuv420p` with `+faststart`; duration and
  stream counts are validated.
- **PDF** — page counts are compared through qpdf; the qpdf image pass is lossy;
  every gs grid pass carries `-dPDFSETTINGS=/screen`. The stale "2 qpdf + 17 gs"
  bound is now "2 qpdf + up to 16 gs cells, plus page probes".
- **One file-type table** — `registry.rs` drives detection, accepted extensions
  and output naming.
- **Job bodies run in `spawn_blocking`** — engine polling no longer parks tokio
  runtime workers; release builds keep `panic = "unwind"` so one file's panic is
  contained.

### Fixed

- Portrait photos keep their orientation, and non-sRGB inputs are transformed and
  tagged sRGB instead of losing their profile.
- Cancelled or failed copies leave no partial file in the output folder.
- PDFs that lose pages are rejected instead of reported as done.
- Video no longer silently drops extra audio or subtitle tracks.
- A processor panic no longer kills the app in a release build.
- Sidecar stderr and stdout drains are bounded and joined.

## [1.1.0](https://github.com/initgrep-apps/yard/compare/v1.0.0...v1.1.0) (2026-11-07)


### Features

* bounded memory, consent-gated fidelity, and backend remediation ([#224](https://github.com/initgrep-apps/yard/issues/224)) ([602c283](https://github.com/initgrep-apps/yard/commit/602c2831176f2df78d3f344ff11cff905f4618f1))


### Bug Fixes

* **settings:** plain-language labels and section copy ([#227](https://github.com/initgrep-apps/yard/issues/227)) ([087679a](https://github.com/initgrep-apps/yard/commit/087679a2ab1704dd12c7089dc6d496fdac4ac03e))
* **settings:** sidebar-style tabs and destination directory ([#228](https://github.com/initgrep-apps/yard/issues/228)) ([2a6d279](https://github.com/initgrep-apps/yard/commit/2a6d279aff52984700f6eb41071aaeb9edb96e43))
* **ui:** plain-language settings, editable manual ceiling, responsive board ([#226](https://github.com/initgrep-apps/yard/issues/226)) ([3a26ad5](https://github.com/initgrep-apps/yard/commit/3a26ad5f12513ada496522e3a16b76f4360de5e3))

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
