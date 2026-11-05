# Yard — Features

What shipped, in the order the slices were built. This replaces the per-slice specs and plans; design decisions behind a row live in `docs/adr/`, open work in `docs/bugs.md`.

| # | Slice | Shipped | Notes / decisions |
|---|---|---|---|
| 1 | App shell | Tauri + React shell, sidebar navigation, dark mode from OS preference, toast host | No router; views mount one at a time |
| 2 | File detection | Extension → category, canonicalise + dedup, per-category target resolved in Rust | Frontend never guesses category or target |
| 3 | Settings | Settings dialog: General, targets, quality floors, performance, history retention | Every field defaulted; auto-saves after 400 ms and flushes on close |
| 4 | Image compression | vips quality search then dimension reduction; JPG/PNG/WebP native, HEIC/HEIF → JPG | PNG uses palette compression; stops at the quality floor (ADR 0006) |
| 5 | Video compression | Two-pass libx264 bitrate targeting, resolution ladder, CRF fallback | Never `-fs`; see `ARCHITECTURE.md` §6 |
| 6 | PDF compression | qpdf lossless → qpdf image pass → gs `/screen` → gs dpi × jpegq grid | qpdf exit 2 accepted as warnings-only |
| 7 | Document compression | OOXML media recompression, rezip at deflate 9, best-effort | Only exact `word/`, `ppt/`, `xl/` media folders; broken media entries keep original bytes |
| 8 | Self-contained engines | Manifest → fetch → prep → verify → audit pipeline; macOS dylib collection, Windows engine folders | Offline by construction; see `GLOSSARY.md` |
| 9 | Parallel compression | Weighted lane scheduler, video = 2 slots, per-file cancel, cancel-all | ADR 0003 |
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

## Not shipped (deliberately)

Per-file target override · batch presets · HEIC output · EMF/WMF conversion · magic-byte file detection · telemetry · auto-update · Linux packages.

## Backlog

Approved and sliced: `docs/superpowers/plans/2026-11-backend-remediation.md` (ADRs 0010–0014). It absorbs the following items from the 2026-10-18 review: collapse the four ladders, move runner capabilities into the shared seam, one outcome sink, one classification source, an honest IPC seam, history orchestration out of `QueueState`, resolve sidecars once, and let the ladder own progress.

Still unscheduled:

- Scheduled upstream drift check (`binaries:refresh --dry-run` opens an issue).

## Known performance debt

No row memoization or progress-event throttling. History tables virtualize above 200 filtered rows; compression views still re-render on every `file:progress`, measurable on 1000-file batches (`docs/bugs.md`).
