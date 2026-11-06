# Yard — Features

What shipped, in the order the slices were built. This replaces the per-slice specs and plans; design decisions behind a row live in `docs/adr/`, open work in `docs/bugs.md`.

| # | Slice | Shipped | Notes / decisions |
|---|---|---|---|
| 1 | App shell | Tauri + React shell, sidebar navigation, dark mode from OS preference, toast host | No router; views mount one at a time |
| 2 | File detection | Extension → category, canonicalise + dedup, per-category target resolved in Rust | Frontend never guesses category or target |
| 3 | Settings | Settings dialog: General, targets, quality floors, performance, history retention | Every field defaulted; auto-saves after 400 ms and flushes on close. Performance is now adaptive memory + ceiling (row 22) |
| 4 | Image compression | vips quality search then dimension reduction; JPG/PNG/WebP native, HEIC/HEIF → JPG when compression runs | PNG uses palette compression; stops at the quality floor (ADR 0006). Colour/orientation corrected in row 23; under-target copies keep original bytes |
| 5 | Video compression | Two-pass libx264 bitrate targeting, resolution ladder, CRF fallback | Never `-fs`; current stream/validation rules in row 24 |
| 6 | PDF compression | qpdf lossless → qpdf image pass → gs dpi × jpegq grid | qpdf exit 3 accepted as warnings-only; page validation and rung order in row 25 |
| 7 | Document compression | OOXML media recompression, rezip at deflate 9, best-effort | Only exact `word/`, `ppt/`, `xl/` media folders; broken media entries keep original bytes. Streaming rewrite in row 22 |
| 8 | Self-contained engines | Manifest → fetch → prep → verify → audit pipeline; macOS dylib collection, Windows engine folders | Offline by construction; see `GLOSSARY.md` |
| 9 | Parallel compression | Lane scheduler, per-file cancel, cancel-all | ADR 0003; slot weights replaced by the memory budget in row 22 |
| 10 | Cancelled batch signal | `batch:complete` carries `cancelled`; UI shows the cancelled outcome instead of a success toast | |
| 11 | History audit trail | Append-only JSONL, session + file records, sidebar batch list, modal filter dialog, pagination, 90-day trim | ADR 0005; sort is `newest` / `oldest` only |
| 12 | Compress run view | Staging list ⇄ run board, per-category lanes, live cards, batch footer, "New batch" clears staging | Boards scroll horizontally; the sidebar stays manual at 760px |
| 13 | UI overhaul | Chip filters, status tones, lane-aware cards, reduced-motion-safe shimmer, compact bar | Design tokens live in `src/lib/tokens.ts` |
| 14 | Release workflows | PR gate, freeze, release with four native rows, release-please changelog | `docs/ci-process.md` |
| 15 | Snapshot pipelines | Freeze all four triples into `sidecars-<date>`; every consumer unfreezes and verifies | ADR 0001 |
| 16 | Release integrity | Package-content gate per release row, macOS library-validation entitlement, Windows engine-path fixes, no console windows | ADRs 0001, 0002; `scripts/ci/package-gate.mjs` |
| 17 | Yard rename | Product, package, crate, identifier and temp folder are all `yard` | Repo slug is `initgrep-apps/yard` |
| 18 | App shell redesign | Native window chrome (macOS overlay, frameless Windows), stock shadcn sidebar with history batch list, filter dialog, settings/About dialogs, pre-paint theme | ADR 0007; Linux chrome unverified (`docs/bugs.md`) |
| 19 | Lucide icons | Every icon from `lucide-react`; `@remixicon/react` dropped, `components.json` declares `iconLibrary: lucide` | ADR 0008; sonner keeps shadcn's `OctagonX`/`TriangleAlert`, brand is `FileLock2` |
| 20 | History batch dashboard | Four summary cards (linear size scale, Files outcome pills, Best effort, Failed), absolute batch header, 3-column sortable table (File · Size · Status), search/type/outcome filters, expandable 3-card row detail (size scale · target/method · timing) with reveal actions; virtualizes above 200 rows; stock kanban-03 board (drag within a lane, drag-to-reorder lanes, compressing card pinned undraggable at its lane top, live re-pin), wheel panning, 16rem fixed lanes, lucide category icon chips; clear history moved to Settings → History behind a destructive confirm | Under target uses the History muted tone; summary reflows to 2×2 at ≤860px content; Windows wheel/scrollbar check pending (`docs/bugs.md`); drag overlay ghost + pinned-card keyboard focus pending dev-build human check (`docs/bugs.md`) |
| 21 | Neutral preset | shadcn preset `b0` applied with a complete overwrite: `nova` component shapes, neutral oklch palette, Inter, default radius, next-themes-backed sonner | ADR 0009; complete overwrite restores local component edits (`tableClassName`, `ToggleGroup` generic, hardened `Spinner`) and licence headers |
| 22 | Bounded memory | Documents stream archive entries; adaptive batch budget (`clamp(total/8, 300 MiB, 2 GiB)`) with `adaptiveMemory` / `memoryCeilingMb`; disk preflight before history; `memory_stats` command and `memory:usage` event | ADR 0010; admission uses conservative estimates, RSS is display-only |
| 23 | Image fidelity | Every attempt is `vips thumbnail` with EXIF autorotation, sRGB output and `keep=icc`; dimension ladder is 0.9× steps floored at `minLongEdge` (default 1280 px) | ADR 0011; document media uses the same invocation shape; `minLongEdge` is stored under `qualityFloors.image`; under-target files bypass attempts; real-vips fixtures are `#[ignore]` |
| 24 | Video fidelity | One ABR two-pass rung, then CRF 23 → floor at the lower allowed resolutions; maps all audio/subtitles and chapters; AAC 128 kbps, `yuv420p`, `+faststart`; validates duration, video/audio/subtitle counts | ADR 0011; dropped input streams require consent |
| 25 | PDF integrity | qpdf page-count equality through `--show-npages`; qpdf image pass reclassified as lossy; every gs grid pass carries `-dPDFSETTINGS=/screen` plus explicit color/gray/mono image resolutions | ADR 0011; `-r` only rasterises vector content, so the grid down-samples embedded images through `-d*ImageResolution`; up to 16 gs cells, 8 at default floors; one failed pass continues the ladder |
| 26 | One file-type registry | Extension → category → output extension in `registry.rs`; detection, accepted-extension lists and output naming read the same rows | ADR 0012; TypeScript unions are checked by file-reading tests |
| 27 | Shared job context and ladder | `JobContext` resolves sidecars, temp, output and policy once; `Ladder` owns progress, best attempt, denial state and cleanup; job bodies run in `spawn_blocking`; release panics unwind | ADR 0013, 0014 |
| 28 | Capability matrix and consent | Seven change kinds × five categories; rung declarations checked before delivery; allowed / needs consent / denied verdicts; a denied rung is removed | ADR 0012; an earlier authorised attempt may still deliver as best effort; `needs_permission` only when none remains |
| 29 | Consent surfaces | HEIC pre-flight dialog when `heicToJpg` is off; inline per-file prompts with once/batch scope; two denials silence repeat prompts; `consent:required` event + `answer_consent` command | ADR 0011; 30-minute answer timeout; cancel observed while waiting |
| 30 | History truth | File records carry `changes` and `consentSource`; old JSONL lines default to empty/none; session outcome counts `needs_permission` | ADR 0005; history detail shows the granted changes and source |

## Not shipped (deliberately)

Per-file target override · batch presets · HEIC output · EMF/WMF conversion · magic-byte file detection · telemetry · auto-update · Linux packages.

## Backlog

No approved slice is queued. The 2026-11 backend remediation is shipped as rows 22–30 (ADRs 0010–0014) and absorbed the 2026-10-18 review items: the four ladders now share `JobContext`/`Ladder`, runner capabilities sit in the shared seam, delivery has one outcome path, file types have one registry, the IPC seam reports `needs_permission`, history orchestration no longer lives in `QueueState`, sidecars resolve once per job, and the ladder owns progress.

Still unscheduled:

- Scheduled upstream drift check (`binaries:refresh --dry-run` opens an issue).

## Known performance debt

No row memoization or progress-event throttling. History tables virtualize above 200 filtered rows; compression views still re-render on every `file:progress`, measurable on 1000-file batches (`docs/bugs.md`).
