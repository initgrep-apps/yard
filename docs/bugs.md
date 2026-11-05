# Open bugs, known gaps, and deferred work

Open items only. Resolved investigations belong in git history or as a durable rule in `docs/ci-process.md`. Design decisions belong in `docs/adr/`.

## Bugs

1. **Windows kanban input unverified** — slice #20's wheel-to-horizontal panning, thin scrollbar styling, and `select-none` need a real Windows build check (WebView2); cua-driver cannot synthesize wheel events on WebKit on macOS. Trackpad scroll feel on the new history table likewise needs a human pass.
2. **kanban-03 run board human-check pending** — dev-build pass on the new stock-kanban board: drag overlay ghost (item + column variants), always-visible lane grip opacity, pinned-card focus affordance (stock inert sortable attributes), and live compressing re-pin jump. dnd-kit cannot be drag-tested in jsdom; unit-level drag semantics are test-pinned in `src/lib/boardState.test.ts`.

Items 3–8 come from the 2026-11 architecture review. Slice numbers point into `docs/superpowers/plans/2026-11-05-backend-remediation.md`.

3. **Images lose orientation and colour profile** — `image.rs:59-64` appends `strip` to every save and never applies EXIF orientation, so portrait photos are delivered sideways; ICC profiles are discarded on non-sRGB inputs, so colours shift. S3.
4. **Output is committed by direct copy** — `engine/processor.rs:162` copies straight to the final path, so a cancel or error mid-copy leaves a partial file in the user's output folder. S0.
5. **PDF page loss is undetected** — `pdf.rs:174-208` accepts any output starting with `%PDF-`; a gs pass that drops pages still counts as a success. S5.
6. **Release builds cannot contain a panic** — `panic = "abort"` makes `catch_unwind` (`queue.rs:585`) and `JoinError` handling (`scheduler.rs:88`) unreachable. One processor panic kills the app. S0, S7.
7. **Video silently drops extra streams** — no `-map` in `video.rs:242-298`, so ffmpeg's default selection keeps one video and one audio track and discards the rest. S4.
8. **Stderr drain is unbounded and outlives its timeout** — `engine/processor.rs:238-246` reads to EOF into a `Vec`, and the 5 s join at `278-288` detaches the thread instead of stopping it. S0.

No other open bugs; the known gaps and accepted limitations below still apply.

## Known gaps

| Gap | Impact |
|---|---|
| No row memoization or progress throttling | 1000-file history tables virtualize above 200 rows; compression views still re-render the active view on every `file:progress`; measurable webview CPU |
| File picker passes `filters: []` | Unsupported files are only rejected after selection |
| `history.jsonl` is re-folded on every `get_history` call | Acceptable while history stays small; cache the fold in `HistoryStore` keyed by `(mtime, len)` if history grows past ~10k records |
| Linux chrome unverified | Linux builds from source with native decorations while the shell renders non-macOS controls and drag surfaces; no packaging or acceptance run |
| `motion` dependency has zero product importers | Board rewrite landed on dnd-kit; `motionStubs.ts` still serves `RunHeader`/`CompressView` tests. Remove dep + stubs in one cleanup slice after confirming no other usage |
| Manual acceptance matrix not yet run on released builds | CI proves packages start and compress; the UI matrix in `docs/distribution-acceptance.md` is manual |

## Accepted limitations

- Output copies are normalized: image and PDF metadata is stripped, HEIC/HEIF becomes JPEG, MOV becomes MP4. Video container metadata is currently kept, despite `README.md` saying otherwise.
- macOS builds are ad-hoc signed and not notarized (ADR 0002); Windows builds are unsigned.
- Linux builds from source but is not packaged.
- No per-file target override, presets, HEIC output, EMF/WMF conversion, magic-byte detection, telemetry, or auto-update.

## Refinement Drift — 2026-11-05 backend remediation

**Plan:** `docs/superpowers/plans/2026-11-05-backend-remediation.md`

1. `AGENTS.md` says `zip` 8.x; `src-tauri/Cargo.lock:5072` is 9.2.1. The docs are wrong, the code is right. Plan uses the 9.x API (`SimpleFileOptions`).
2. `ARCHITECTURE.md` said panics are contained; `src-tauri/Cargo.toml:55` sets `panic = "abort"`, making `catch_unwind` (`queue.rs:585`) unreachable in release. Doc corrected.
3. `README.md` and `docs/bugs.md` said metadata is stripped; FFmpeg copies container metadata by default (`video.rs:242-298` has no `-map_metadata -1`). Doc corrected.
4. `image.rs:61` passes `colours=256` to `pngsave`; libvips 8.18.7 has no such option and silently ignores it. Dead argument, removed by Task 4.
5. `models::file_status_serde_matches_typescript_union` and `category_serde_matches_typescript_union` compare against hardcoded arrays and never read `src/types.ts`; they cannot detect a missing TS member. Task 9 replaces them with a file-reading test.

## CI follow-ups

1. **mv-shim cygpath tests are win32-skipped** — the freeze/unfreeze rollback tests drive a mock `mv` shim whose msys/cygpath behavior differs. POSIX coverage is complete. Next step: a real win32 shell session, then un-skip or port.
2. **Five lock-refusal tests are win32-skipped** — under msys bash, the pre-existing-lock path exits 1 with empty stdout+stderr, so refusal/recovery output cannot be asserted. Next step: observe `acquire_lock` under `bash -x` on Windows.
3. **SBOM does not attribute bundled DLL/dylib licenses** — `sbom.mjs` maps the five top-level engines only; the msys2 closure (freetype, glib2, harfbuzz, …) and mac dylib closure ship their own licenses. Next step: component-level license map, generated on the arm64 release row.
4. **`engines/*.staging-*` litter** — `prep-windows.mjs` work dirs survive a crashed prep and are not gitignored. Next step: ignore `src-tauri/binaries/engines/*.staging-*` and sweep stale dirs at prep start.
5. **Windows loader diagnostics still in `ci.yml`** — `cargo test --no-run`, PE-imports probe, and test-enumeration probe were added while the `0xc0000139` class was open. Decide: keep as canaries or remove; if kept, keep them non-flaky (they only print today).
6. **Release gate fails closed on a leftover draft** — re-running a partially failed tag requires `gh release delete <tag> --cleanup-tag --yes` first. Candidate: reuse an existing *draft* by database id, keep failing on a published release. Deferred because retries are rare and fail-closed is safer.
