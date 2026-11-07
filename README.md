<h1 align="center">Yard</h1>

<p align="center">
  Offline batch file compression for macOS and Windows.
  <br>
  Images, video, PDF, and Office documents — compressed to target sizes, never uploaded.
</p>

<p align="center">
  <a href="https://github.com/initgrep-apps/yard/actions/workflows/ci.yml"><img src="https://github.com/initgrep-apps/yard/actions/workflows/ci.yml/badge.svg" alt="PR gate"></a>
  <a href="https://github.com/initgrep-apps/yard/releases/latest"><img src="https://img.shields.io/github/v/release/initgrep-apps/yard?display_name=tag&sort=semver" alt="Latest release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/initgrep-apps/yard" alt="License: GPL-3.0"></a>
</p>

Yard batch-compresses images, video, PDFs, and Office documents to per-category
size targets. The engines are bundled and all processing stays on your machine:
no telemetry, account, or network access. Originals are never modified, and
destructive changes need your consent.

## Download

| Platform | Installer |
|---|---|
| macOS 11+ · Apple Silicon | [Yard-macOS-arm64.dmg](https://github.com/initgrep-apps/yard/releases/latest/download/Yard-macOS-arm64.dmg) |
| macOS 11+ · Intel | [Yard-macOS-x64.dmg](https://github.com/initgrep-apps/yard/releases/latest/download/Yard-macOS-x64.dmg) |
| Windows 10+ · x64 | [Yard-Windows-x64-setup.exe](https://github.com/initgrep-apps/yard/releases/latest/download/Yard-Windows-x64-setup.exe) |
| Windows 10+ · arm64 | [Yard-Windows-arm64-setup.exe](https://github.com/initgrep-apps/yard/releases/latest/download/Yard-Windows-arm64-setup.exe) |

All installers, checksums, SBOM, and third-party notices are on the
[Releases page](https://github.com/initgrep-apps/yard/releases/latest).

macOS builds are ad-hoc signed and not notarized. Copy the app to
`/Applications`, then run `xattr -dr com.apple.quarantine /Applications/yard.app`
(or use **Open Anyway** after the first launch attempt). Windows builds are
unsigned; choose **More info → Run anyway** when SmartScreen warns.

## Usage

1. Drop files onto the window, or click **Browse your files**.
2. Press **Compress**. Progress is per file; cancel one file or the whole batch.
3. Open the output folder from the result row, or browse the batch in **History**.

Targets, quality floors, memory ceiling, output folder, and retention live in
**Settings**.

## Supported formats

| Type | Input | Output |
|---|---|---|
| Images | JPEG, PNG, WebP, HEIC/HEIF | JPEG, PNG, WebP (HEIC/HEIF → JPEG when compression runs) |
| Video | MP4, MOV | MP4 (MOV → MP4 when compression runs) |
| PDF | PDF | PDF |
| Documents | DOCX, XLSX, PPTX | Same format, embedded media recompressed |

A file already under its target is copied byte-for-byte under the mapped
extension, so an under-target HEIC can remain HEIC bytes inside a `.jpg`.

## Requirements

| | Minimum | Recommended |
|---|---|---|
| OS | macOS 11+ / Windows 10+ | Same |
| CPU | 2 cores | 6–8 logical cores |
| RAM | 4 GB | 16 GB |
| Disk | SSD, room for the largest file plus its attempts | SSD, 30 GB free |

Memory use is bounded per file, and parallel jobs are admitted against an
adaptive budget. Large batches also need temp disk for attempts.

## Security

Yard processes files locally and sends nothing over the network. Report
vulnerabilities through
[GitHub private vulnerability reporting](https://github.com/initgrep-apps/yard/security/advisories/new);
see [`SECURITY.md`](SECURITY.md). Do not open a public issue.

## Development

Requirements: Node.js 22, yarn, Rust stable, and the
[Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/). Setup, the
local gate, and PR rules are in [`CONTRIBUTING.md`](CONTRIBUTING.md).

## Documentation

- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — components, IPC, limits
- [`docs/FEATURES.md`](docs/FEATURES.md) — shipped capabilities
- [`docs/adr/`](docs/adr/) — decisions and rejected alternatives
- [`docs/cicd-process.md`](docs/cicd-process.md) — CI/CD model, release runbook, manual acceptance
- [`docs/bugs.md`](docs/bugs.md) — open bugs
- [`GLOSSARY.md`](GLOSSARY.md) — project vocabulary

## License

GPL-3.0 — see [`LICENSE`](LICENSE). Bundled engines keep their own licenses:
FFmpeg GPL-2.0-or-later, Ghostscript AGPL-3.0, libvips LGPL-3.0, qpdf
Apache-2.0. See [`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md).
