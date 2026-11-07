# Yard — Features

What shipped, in the order the slices were built. Decisions live in `docs/adr/`.

| # | Slice | Shipped | Notes / decisions |
|---|---|---|---|
| 1 | App shell | Tauri + React shell, sidebar navigation, dark mode from OS preference, toast host | No router; views mount one at a time |
| 2 | File detection | Extension → category, canonicalise + dedup, per-category target resolved in Rust | Frontend never guesses category or target |
| 3 | Settings | Settings dialog: General, targets, consent, quality limits, resource usage, history | Every field defaulted; auto-saves after 400 ms and flushes on close |
| 4 | Image compression | vips quality search then dimension reduction; JPG/PNG/WebP native, HEIC/HEIF → JPG when compression runs | PNG uses palette compression; stops at the quality floor (ADR 0006); under-target copies keep original bytes |
| 5 | Video compression | Two-pass libx264 bitrate targeting, resolution ladder, CRF fallback | Never `-fs`; current rules in row 24 |
| 6 | PDF compression | qpdf lossless → qpdf image pass → gs dpi × jpegq grid | qpdf exit 3 accepted as warnings; page validation and rung order in row 25 |
| 7 | Document compression | OOXML media recompression, rezip at deflate 9, best effort | Only exact `word/`, `ppt/`, `xl/` media folders; broken media entries keep original bytes |
| 8 | Self-contained engines | Manifest → fetch → prep → verify → audit pipeline; macOS dylib collection, Windows engine folders | Offline by construction |
| 9 | Parallel compression | Lane scheduler, per-file cancel, cancel-all | ADR 0003; slot weights replaced by the memory budget in row 22 |
| 10 | Cancelled batch signal | `batch:complete` carries `cancelled`; UI shows the cancelled outcome instead of a success toast | |
| 11 | History audit trail | Append-only JSONL, session + file records, sidebar batch list, filter dialog, pagination, 90-day trim | ADR 0005; sort is `newest` / `oldest` only |
| 12 | Compress run view | Staging list ⇄ run board, per-category lanes, live cards, batch footer, "New batch" clears staging | Boards scroll horizontally |
| 13 | UI overhaul | Chip filters, status tones, lane-aware cards, reduced-motion-safe shimmer, compact bar | Design tokens live in `src/lib/tokens.ts` |
| 14 | Release workflows | PR gate, freeze, release with four native rows, release-please changelog | `docs/cicd-process.md` |
| 15 | Snapshot pipelines | Freeze all four triples into `sidecars-<date>`; every consumer unfreezes and verifies | ADR 0001 |
| 16 | Release integrity | Package-content gate per release row, macOS library-validation entitlement, Windows engine-path fixes, no console windows | ADRs 0001, 0002; `scripts/ci/package-gate.mjs` |
| 17 | Yard rename | Product, package, crate, identifier and temp folder are all `yard` | Repo slug is `initgrep-apps/yard` |
| 18 | App shell redesign | Native window chrome (macOS overlay, frameless Windows), stock shadcn sidebar with history batch list, filter dialog, settings/About dialogs, pre-paint theme | ADR 0007 |
| 19 | Lucide icons | Every icon from `lucide-react`; `@remixicon/react` dropped, `components.json` declares `iconLibrary: lucide` | ADR 0008; sonner keeps shadcn's `OctagonX`/`TriangleAlert` |
| 20 | History batch dashboard | Summary cards, batch header, sortable table, search/type/outcome filters, expandable row detail with reveal actions; virtualizes above 200 rows; kanban board with drag/reorder/pin, wheel panning, auto-fit lanes | Under-target uses the History muted tone; summary reflows to 2×2 at ≤860px; Windows UI checks in `docs/cicd-process.md` |
| 21 | Neutral preset | shadcn preset `b0`: `nova` component shapes, neutral oklch palette, Inter, default radius, next-themes-backed sonner | ADR 0009 |
| 22 | Bounded memory | Documents stream archive entries; adaptive batch budget (`clamp(total/8, 300 MiB, 2 GiB)`), disk preflight before history, `memory_stats` and `memory:usage` | ADR 0010; admission uses conservative estimates, RSS is display-only |
| 23 | Image fidelity | Every attempt is `vips thumbnail` with EXIF autorotation, sRGB output and `keep=icc`; dimension ladder is 0.9× steps floored at `minLongEdge` (default 1280 px) | ADR 0011; under-target files bypass attempts; real-vips fixtures are `#[ignore]` |
| 24 | Video fidelity | One ABR two-pass rung, then CRF 23 → floor at the lower allowed resolutions; maps all audio/subtitles and chapters; AAC 128 kbps, `yuv420p`, `+faststart`; validates duration and stream counts | ADR 0011; dropped streams require consent; an unknown input width still declares `Downscale` under a scale cap |
| 25 | PDF integrity | qpdf page-count equality through `--show-npages`; qpdf image pass reclassified as lossy; every gs grid pass carries `-dPDFSETTINGS=/screen` plus explicit color/gray/mono image resolutions | ADR 0011; up to 16 gs cells, 8 at default floors; one failed pass continues the ladder |
| 26 | One file-type registry | Extension → category → output extension in `registry.rs`; detection, accepted-extension lists and output naming read the same rows | ADR 0012; TypeScript unions are checked by file-reading tests |
| 27 | Shared job context and ladder | `JobContext` resolves sidecars, temp, output and policy once; `Ladder` owns progress, best attempt, denial state and cleanup; job bodies run in `spawn_blocking`; release panics unwind | ADR 0013, 0014 |
| 28 | Capability matrix and consent | Seven change kinds × five categories; rung declarations checked before delivery; allowed / needs consent / denied verdicts; a denied rung is removed | ADR 0012; an earlier authorised attempt may still deliver as best effort |
| 29 | Consent surfaces | Settings → Consent toggles for destructive changes and HEIC→JPEG; HEIC pre-flight dialog only when needed; inline once/batch prompts; two denials silence repeats; `consent:required` + `answer_consent` | ADR 0011; 30-minute timeout; cancel observed while waiting |
| 30 | History truth | File records carry `changes` and `consentSource`; old JSONL lines default to empty/none; session outcome counts `needs_permission` | ADR 0005; history detail shows the granted changes and source |
