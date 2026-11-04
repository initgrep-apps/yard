# Yard — Project Config

## Agent language

- invoke caveman skill to respond unless user specifically ask to explain

## Overview

Offline desktop app for batch-optimizing files to maximum size limits. Tauri 2 + React + TypeScript + Vite + shadcn/ui. Rust core with sidecar binaries. GPLv3.

## Orchestrator Config

- `PLAN_GLOB` — `docs/superpowers/plans/*.md` (working files only; delete after the slice merges — see Documentation lifecycle)
- `TECH_SPEC_PATH` — `docs/ARCHITECTURE.md`
- `PRD_PATH` — `docs/FEATURES.md`
- `GATE_COMMAND` — `yarn build && yarn test && cd src-tauri && cargo test`
- `GATE_E2E_COMMAND` — `none`
- `BOUNDARY_RULES_REF` — `docs/ARCHITECTURE.md → §3 Components`
- `BUGS_PATH` — `docs/bugs.md`
- `CODE_PATHS` — `src/`, `src-tauri/src/`

## Documentation lifecycle

Canonical, always-current docs: `README.md`, `docs/ARCHITECTURE.md`, `docs/FEATURES.md`, `GLOSSARY.md`, `docs/adr/`, `docs/bugs.md`, `docs/ci-process.md`, `docs/distribution-acceptance.md`.

Superpowers specs and plans are working artifacts. When a slice merges: digest the outcome into `docs/FEATURES.md`, record any decision that passes the ADR three-test filter in `docs/adr/`, move open work to `docs/bugs.md`, then delete the spec and plan. Resolved investigations leave no document behind.

## Tech Stack

- **Desktop:** Tauri 2.12 (Rust backend)
- **Frontend:** React 19.3 + TypeScript 7.0 + Vite 8.3 (Rolldown)
- **UI:** shadcn/ui 4.23 on Base UI + Tailwind CSS 4.3 (neutral oklch theme, preset `b0`), motion 14, sonner
- **Font:** Inter (`@fontsource-variable/inter` 5.3)
- **Plugin:** tauri-plugin-store 2.5, tauri-plugin-dialog 2.8, tauri-plugin-opener 2.7, tauri-plugin-os 2.4, tauri-plugin-window-state 2.5, tauri-plugin-single-instance 2.5 (no shell plugin)
- **Async:** Tokio 1.x + tokio-util 0.7 (via Tauri)
- **Image processing:** libvips 8.18.7 (`vips` sidecar; Intel macOS frozen at 8.18.6)
- **Video processing:** FFmpeg 9.0.2 + ffprobe (sidecars)
- **PDF processing:** Ghostscript 10.08.0 + qpdf 12.4.2 (sidecars; Intel macOS gs frozen at 10.07.1)
- **Document processing:** Rust `zip` 8.x crate + vips sidecar
- **Rust deps:** serde 1.0, uuid 1.26, dirs 7.0
- **Package manager:** yarn
- **License:** GPLv3

## Architecture

See `docs/ARCHITECTURE.md` for components, IPC, algorithms and limits. Key rule: frontend (`src/`) = UI only; Rust core (`src-tauri/src/`) = all compression logic. Sidecar binaries in `src-tauri/binaries/`.

## Setup

Scaffolded via `yarn create tauri-app` (React + TS + Vite). shadcn/ui via `yarn dlx shadcn@latest init --preset b0 --template vite`. Package manager: yarn.

## CI/CD

- `ci.yml` — PR/push-to-main gate: 2 rows (macos-15 aarch64, windows-2025 x64) bounded 60 min; each row unfreezes its triple from the newest `sidecars-*` release → (mac) sync-macos-frameworks → clean-room audit (mac rows need `BINARIES_ALLOW_INACTIVE_BREW=1`) → verbatim `yarn build && yarn test && cd src-tauri && cargo test`. Windows rows carry loader diagnostics (probe of the fixed 0xc0000139 class) pending the removal decision in `docs/bugs.md`. Unfreeze writes `src-tauri/.sidecars-restored.json`; verify skips the `stagedSha256`-vs-lock check while the marker governs; fetch/prep clear stale markers.
- `freeze.yml` — manual dispatch; ONE run freezes all four triples into published `sidecars-<date>` (8 assets; proven live 2026-10-17). Mac rows: homebrew-core pinned per arch (`scripts/ci/homebrew-core-pins.json`; Intel = last sonoma-bottled vips 8.18.6 / gs 10.07.1, never bump without human bottle re-verification; `HOMEBREW_NO_INSTALL_FROM_API=1`, no `brew update` anywhere in CI). Windows rows: URL+sha256 manifest engines; ffmpeg/ffprobe static (flat exe only); windows gs = ROM model — msys2 closure via `extraPackages` (25 x64 + 23 arm64 = 48 pinned rows, no `gs-resources` staged on windows; `tauri.windows.conf.json` RFC 7396 null-merge drops it). Finalize verifies the exact-8 asset set live (`--json assets --jq`), retries 4×10 s through asset-index lag, and idempotently reuses leftover drafts. Matrix rows bounded 40 min.
- `release.yml` — `v*` tags (or dispatch): 4 native rows (macos-15, macos-15-intel, windows-2025, windows-11-arm — no cross-compile while macos-15-intel exists) unfreeze → audit → gate → tauri-action draft upload (numeric `releaseId`); per-row app-asset check; SBOM + third-party notices from the aarch64 row; sidecars preflight requires all 8 bundle assets; rows bounded 90 min.
- Windows-target compile gate before any push touching `src-tauri/src/`: `cargo check --all-targets --target aarch64-pc-windows-msvc` — verbatim command, placeholder staging, and bash 3.2/git-bash failure classes live in `docs/ci-process.md`. Open CI follow-ups: `docs/bugs.md`.
- Freeze-before-tag rule (ADR 0001): bumping any engine version requires running `freeze.yml` before tagging a release. Brew runs only inside freeze.yml mac rows — consumers never stage engines themselves. GitHub-hosted runners only; the local machine is development only.

## Skills & AI Docs

When working with a library, use its official AI docs/skill if available. Fetch via Context7 MCP or llms.txt URLs below.

| Library | Version | Skill / AI Docs |
|---------|---------|----------------|
| Tauri 2 | 2.12 | Context7: `/tauri-apps/tauri-docs` · llms.txt: `https://v2.tauri.app/llms.txt` |
| shadcn/ui | 4.21 | Skill: `shadcn` (already installed). llms.txt: `https://ui.shadcn.com/llms.txt` · MCP server available |
| React | 19.3 | Context7: `/websites/react_dev` · llms.txt: `https://react.dev/llms.txt` |
| Vite | 8.3 | Context7: resolve `Vite` · llms.txt: `https://vite.dev/llms.txt` |
| Tailwind CSS | 4.3 | No llms.txt. Context7: resolve `Tailwind CSS` |
| tauri-plugin-store | 2.5 | Covered by Tauri llms.txt — `https://v2.tauri.app/plugin/store` |
| tauri-plugin-dialog | 2.8 | Covered by Tauri llms.txt — `https://v2.tauri.app/plugin/dialog` |
| tauri-plugin-shell | 2.4 | Covered by Tauri llms.txt — `https://v2.tauri.app/plugin/shell` |

**Rules:**
- shadcn/ui components → use `shadcn` skill before adding/building components. Ask which shadcn block to use.
- Tauri APIs/plugins → fetch from Context7 `/tauri-apps/tauri-docs` before writing Tauri code.
- React patterns → fetch from Context7 or react.dev/llms.txt.
- Rust crates (tokio, serde, uuid, zip, dirs) → Context7 or crates.io docs. No llms.txt.

## Compatibility Notes

- shadcn/ui 4.x requires **Tailwind CSS v4** (CSS-based config, `@tailwindcss/vite` plugin — no `tailwind.config.js`)
- Vite 8.x uses **Rolldown** (Rust-based bundler) as default
- Tauri 2.12 requires Rust stable + Node.js 22 (jsdom's engine gate rejects other majors)
- Tauri 3 alpha exists — do NOT use, stay on 2.x stable

## Conventions

- Conventional commits: `feat(scope):`, `fix(scope):`, `chore:`, `refactor(scope):`
- Rust: `#[serde(rename_all = "camelCase")]` for IPC structs, `#[serde(rename_all = "lowercase")]` for frontend-facing enums
- TypeScript: types in `src/types.ts`, matching Rust models
- React: hooks in `src/hooks/`, views in `src/views/`, components in `src/components/`
- shadcn: use existing blocks — ask which block to use. Do not build custom when a block exists.
- Theme: neutral oklch tokens from preset. Do not approximate with hex.
- No comments in code unless explicitly requested.