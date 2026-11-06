# Open bugs, known gaps, and deferred work

Open items only. Resolved investigations belong in git history or as a durable rule in `docs/ci-process.md`. Design decisions belong in `docs/adr/`.

## Bugs

1. **Windows kanban input unverified** — slice #20's wheel-to-horizontal panning, thin scrollbar styling, and `select-none` need a real Windows build check (WebView2); cua-driver cannot synthesize wheel events on WebKit on macOS. Trackpad scroll feel on the new history table likewise needs a human pass.
2. **kanban-03 run board human-check pending** — dev-build pass on the new stock-kanban board: drag overlay ghost (item + column variants), always-visible lane grip opacity, pinned-card focus affordance (stock inert sortable attributes), and live compressing re-pin jump. dnd-kit cannot be drag-tested in jsdom; unit-level drag semantics are test-pinned in `src/lib/boardState.test.ts`.

No other open bugs; the known gaps and accepted limitations below still apply.

## Closed in the 2026-11 backend remediation

| Was | Shipped fix | Proof test |
|---|---|---|
| Images lost EXIF orientation and colour profiles | Every attempt uses `vips thumbnail` with autorotation, sRGB output and `keep=icc` | `jpeg_args_rotate_convert_to_srgb_and_keep_icc`; real-engine `orientation_table_1_to_8_is_applied` and `display_p3_is_transformed_and_tagged_srgb` (`#[ignore]`) |
| Output was committed by direct copy, leaving partial files | Delivery writes `name.yard-tmp`, checks cancel between chunks, then hard-links (or checked-renames) into place | `cancelled_copy_leaves_no_final_file`, `failed_copy_removes_temp_file`, `existing_destination_is_not_clobbered` |
| PDF page loss was undetected | Every attempt is probe-compared with qpdf `--show-npages`; a mismatch or probe failure rejects it | `gs_output_with_fewer_pages_is_rejected`, `page_count_probe_failure_rejects_the_attempt` |
| Release builds could not contain a panic | Release profile is `panic = "unwind"`; job bodies are caught and the batch continues | `release_profile_keeps_unwind_panics`, `panicking_job_does_not_kill_batch_or_sum` |
| Video silently dropped extra streams | All audio/subtitle tracks and chapters are mapped; duration and stream counts are validated; a dropped input stream needs consent | `stream_maps_preserve_all_audio_and_subtitles`, `audio_count_matches_input_unless_drop_is_granted`, `subtitle_count_must_match_input`, `duration_mismatch_rejects_attempt` |
| Stderr drain was unbounded and outlived its timeout | Stderr retains a 2 KiB tail with a 500-character error rendering; stdout capture is capped at 64 KiB; drain threads are joined after exit | `stderr_tail_retains_at_most_2_kib_but_drains_everything`, `capture_stdout_is_capped_and_stderr_stays_bounded`, `video_and_generic_paths_share_one_stderr_tail` |

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

- Output copies are normalized: images are re-encoded to sRGB with ICC kept and EXIF orientation applied; HEIC/HEIF becomes JPEG by default, MOV becomes MP4. PDFs are rewritten by qpdf/gs. Video container metadata and all audio/subtitle streams are kept.
- macOS builds are ad-hoc signed and not notarized (ADR 0002); Windows builds are unsigned.
- Linux builds from source but is not packaged.
- No per-file target override, presets, HEIC output, EMF/WMF conversion, magic-byte detection, telemetry, or auto-update.

## CI follow-ups

1. **mv-shim cygpath tests are win32-skipped** — the freeze/unfreeze rollback tests drive a mock `mv` shim whose msys/cygpath behavior differs. POSIX coverage is complete. Next step: a real win32 shell session, then un-skip or port.
2. **Five lock-refusal tests are win32-skipped** — under msys bash, the pre-existing-lock path exits 1 with empty stdout+stderr, so refusal/recovery output cannot be asserted. Next step: observe `acquire_lock` under `bash -x` on Windows.
3. **SBOM does not attribute bundled DLL/dylib licenses** — `sbom.mjs` maps the five top-level engines only; the msys2 closure (freetype, glib2, harfbuzz, …) and mac dylib closure ship their own licenses. Next step: component-level license map, generated on the arm64 release row.
4. **`engines/*.staging-*` litter** — `prep-windows.mjs` work dirs survive a crashed prep and are not gitignored. Next step: ignore `src-tauri/binaries/engines/*.staging-*` and sweep stale dirs at prep start.
5. **Windows loader diagnostics still in `ci.yml`** — `cargo test --no-run`, PE-imports probe, and test-enumeration probe were added while the `0xc0000139` class was open. Decide: keep as canaries or remove; if kept, keep them non-flaky (they only print today).
6. **Release gate fails closed on a leftover draft** — re-running a partially failed tag requires `gh release delete <tag> --cleanup-tag --yes` first. Candidate: reuse an existing *draft* by database id, keep failing on a published release. Deferred because retries are rare and fail-closed is safer.
