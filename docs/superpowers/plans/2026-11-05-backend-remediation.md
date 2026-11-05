# Backend Remediation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bound per-file memory, replace silent degradation with a consent-gated fidelity ladder, and collapse the duplicated processor orchestration into one ladder state object.

**Architecture:** Streaming processors with a machine-derived memory budget (ADRs 0010, 0013), a fidelity ladder whose destructive rungs are gated by a capability matrix (ADRs 0011, 0012), and blocking work moved off the tokio runtime with unwind-safe panic containment (ADR 0014).

**Tech Stack:** Tauri 2.12, Rust 1.x, tokio (adds `time`), `zip` 9.2.1, `sysinfo` 0.39.x, libvips 8.18.7, FFmpeg 9.0.2, qpdf 12.4.2, Ghostscript 10.08.0, React 19 + TypeScript 7 + vitest.

**Spec:** `docs/adr/0010-bounded-memory-adaptive-budget.md`, `0011-fidelity-ladder-consent.md`, `0012-capability-matrix-file-type-registry.md`, `0013-attempt-ladder-job-context.md`, `0014-blocking-work-off-runtime.md`; current behaviour in `docs/ARCHITECTURE.md`.

## Global Constraints

- Gate after every task: `yarn build && yarn test && cd src-tauri && cargo test`.
- Windows compile gate after every task touching `src-tauri/src/`: `cd src-tauri && cargo check --all-targets --target aarch64-pc-windows-msvc`.
- No new engines and no new file types. `sysinfo` and tokio `time` are the only new dependencies.
- Originals are never modified. Outputs are written to the user's output folder only by atomic rename.
- Release profile uses `panic = "unwind"`; `panic = "abort"` must not reappear.
- `FileStatus` values and the TypeScript union change in the same task; the file-reading union test added in Task 9 is the guard.
- Every rung declares the changes it makes; no processor reads consent settings directly.
- Do not delete the `#[ignore]` real-engine tests; they are the only check against real binaries.

## Review Focus

Failure modes the spec implies but no current test exercises, ordered by how badly they bite a user. Each gets a test in the owning task.

1. **A partial or clobbered file at the user's output name** (rating 9). No test interrupts delivery today, and the existing `output_was_free` guard disappears when direct copy becomes rename. A file the app did not create must never be replaced, and a cancelled job must leave nothing behind.
2. **A destructive change delivered without consent** (rating 9). A HEIC conversion, a downscale, or a dropped subtitle track must be impossible unless the policy says `Allowed`. Nothing tests declared-vs-actual capabilities today.
3. **Settings migration wipes the file** (rating 8). `parse_settings_value` falls back to `Settings::default()` on any deserialize error. An old `maxConcurrent` key must not cost the user their output folder and targets.
4. **An orphaned consent request stalls the batch** (rating 8). Cancel, timeout and batch-scope grants must resolve every pending request; a test that resolves one request can pass while others hang.
5. **A Windows engine spawn failure** (rating 8). Every engine fake is `#[cfg(unix)]`; the known `0xc0000139` loader class lives on the untested platform. One Windows fake-engine smoke is the minimum, and it must fail loudly if the binary is missing.

---

## Verified Facts

Checked on 2026-11-05 against the pinned tool versions on this machine. Implementers must not re-derive these.

| Fact | Evidence |
|---|---|
| `zip` is locked at 9.2.1, not 8.x | `src-tauri/Cargo.lock:5072`; `git log` shows the 9.2.1 bump |
| `zip` API: `SimpleFileOptions::default().compression_method(CompressionMethod::Stored)`, `by_index` returns `Read`, `std::io::copy` streams | Context7 `/zip-rs/zip2` |
| Entry metadata: `size()`, `compressed_size()`, `compression()`, `is_dir()`, `enclosed_name()` | Context7 `/zip-rs/zip2` |
| `sysinfo` 0.39.x: all memory getters return **bytes**; `Process::memory()` is RSS; `Disk::available_space()` is bytes | Context7 `/guillaumegomez/sysinfo` |
| `sysinfo` is not currently in `Cargo.lock` — new dependency | `grep` on `Cargo.lock` |
| tokio features are `sync, rt, macros`; `timeout` needs `time` | `src-tauri/Cargo.toml:30` |
| `vips thumbnail in out WIDTH` exists; rotation is **on by default** (`no-rotate` default false); `fail-on` default `none` | `vips thumbnail` on libvips 8.18.7 |
| Saver options take the form `out.jpg[Q=80,keep=icc,optimize-coding]` | `vips jpegsave`, `vips webpsave` |
| `keep` flags: `none, exif, xmp, iptc, icc, other, gainmap, all`; default is all | `vips jpegsave` |
| `--output-profile srgb` transforms and the output is tagged `srgb` | run against libvips 8.18.7; `vipsheader` reported `srgb` |
| WebP knobs are `effort` 0–6 (default 4), `smart-subsample`, `alpha-q` (not `alpha-quality`), `target-size` | `vips webpsave` |
| PNG knobs are `compression`, `palette`, `Q`, `effort`, `keep` — there is no `colours` option; existing `colours=256` is ignored | `vips pngsave` |
| Bundled FFmpeg 9.0.2 supports `+faststart`, `mov_text`, and libx264 `yuv420p` | `src-tauri/binaries/ffmpeg-aarch64-apple-darwin -h` |
| Local Homebrew ffmpeg is broken (missing `libx265`); all ffmpeg checks must use the bundled binary or CI | `ffmpeg -version` dyld error |
| `qpdf --show-npages file.pdf` prints the count on its own line | qpdf 12.4.2 on a generated 2-page PDF |
| `panic = "abort"` is at `src-tauri/Cargo.toml:55`; `catch_unwind` already uses `AssertUnwindSafe` | `queue.rs:585-599` |
| `app.emit` already runs from blocking threads via `ProgressSender` | `engine/progress.rs:31-39` |
| `std::fs::rename` replaces an existing file on Windows | Rust std `fs::rename` contract |

---

## File Structure

**Create**

| File | Responsibility |
|---|---|
| `src-tauri/src/memory.rs` | total RAM, budget derivation, job estimates, free disk |
| `src-tauri/src/registry.rs` | the single extension table and output naming |
| `src-tauri/src/policy.rs` | capability enum, matrix, consent resolution |
| `src-tauri/src/engine/context.rs` | `JobContext` |
| `src-tauri/src/engine/ladder.rs` | `Ladder` state object |
| `src-tauri/src/bin/fake_engine.rs` | spawnable test engine for Windows and Unix |
| `src/hooks/useConsent.ts` | frontend consent request state |
| `src/hooks/useMemory.ts` | memory budget and live usage |
| `src/components/run/ConsentPrompt.tsx` | inline card prompt |
| `src/components/run/PreflightDialog.tsx` | one-shot batch warning |

**Modify (production ranges)**

| File | Task |
|---|---|
| `src-tauri/Cargo.toml:30,55` | 1, 3 |
| `src-tauri/src/engine/processor.rs:153-172,218-302` | 1 |
| `src-tauri/src/processors/document.rs:68-73,112-197,323-536` | 2 |
| `src-tauri/src/settings.rs:87-99,135-172` | 3, 9 |
| `src-tauri/src/scheduler.rs:28-98` | 3 |
| `src-tauri/src/processors/image.rs:45-64,131-355` | 4 |
| `src-tauri/src/processors/video.rs:45-101,200-298,686-917` | 5 |
| `src-tauri/src/processors/pdf.rs:34-45,72-138,220-368` | 6 |
| `src-tauri/src/detection.rs:19-32`, `src-tauri/src/processors/mod.rs:27-60`, `src-tauri/src/queue.rs:50-69` | 7 |
| `src-tauri/src/queue.rs:127-134,435-708`, `src-tauri/src/engine/processor.rs:322-338` | 8 |
| `src-tauri/src/models.rs:29-40`, `src/types.ts:23-31`, `src-tauri/src/engine/result.rs:21-28` | 9 |
| `src-tauri/src/queue.rs:82-121,797-829`, `src-tauri/src/history.rs:32-128` | 10 |
| `src/views/CompressView.tsx`, `src/components/SettingsDialog.tsx`, `src/components/history/*`, `src/views/HistoryView.tsx` | 3, 10 |

---

### Task 1: Safety floor

**Files:**
- Modify: `src-tauri/Cargo.toml:55`
- Modify: `src-tauri/src/engine/processor.rs:153-172, 218-302`
- Test: inline modules in `engine/processor.rs` and `queue.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces: `fn deliver_output(..., cancel: &CancellationToken) -> Result<CompressionResult, CompressionError>` (adds the token); `fn commit_output(temp: &Path, dest: &Path) -> std::io::Result<()>`; `struct StderrTail` with `fn push(&mut self, chunk: &[u8])` and `fn bytes_seen(&self) -> u64`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn cancelled_copy_leaves_no_final_file() { /* token set after a partial .yard-tmp exists, assert neither file survives */ }

#[test]
fn failed_copy_removes_temp_file() { /* read-only dest dir, assert no .yard-tmp */ }

#[test]
fn existing_destination_is_not_clobbered() { /* dest created after resolution -> OutputFileError, dest bytes unchanged */ }

#[test]
fn stderr_tail_retains_at_most_2_kib_but_drains_everything() { /* push 10 MiB, assert tail <= 2048 && bytes_seen == 10 MiB */ }

#[test]
fn panic_is_caught_and_reported_as_file_error() { /* processor closure panics, job returns Err */ }

#[test]
fn release_profile_keeps_unwind_panics() { /* read Cargo.toml, assert panic = "unwind" */ }
```

- [ ] **Step 2: Run them and confirm the expected failures**

Run: `cd src-tauri && cargo test engine::processor queue::tests`
Expected: FAIL — no temp file is created, stderr is unbounded, the manifest still says `abort`.

**Note on `cargo test --release`:** Cargo forces `unwind` for test targets regardless of the profile, so a release test run cannot prove the shipped setting. The manifest test above is the real guard; keep the behaviour test in dev.

- [ ] **Step 3: Change the release profile**

`src-tauri/Cargo.toml`: `panic = "unwind"`. Nothing else in the profile changes.

- [ ] **Step 4: Commit by rename**

`deliver_output` copies to `<dest-stem>.yard-tmp` in the **same directory** as `dest`, checks the cancel token, then `fs::rename(temp, dest)`. On any error or cancellation, remove the temp first. `dest` is never opened directly. If `dest` exists at rename time (created by another process after name resolution), delete the temp and return `OutputFileError` — never replace a file the app did not create.

- [ ] **Step 5: Bound the stderr drain**

Replace `read_to_end` with a loop over a 4 KiB buffer feeding `StderrTail`. Keep draining until EOF so the child cannot block. Delete the 5 s detach path; the reader thread ends when the pipe closes after kill.

- [ ] **Step 6: Update the two stderr assertions and run the gate**

`image::wait_sidecar_reports_truncated_stderr_tail_on_failure` and `pdf::pdf_sidecar_stderr_tail_in_error_message` assume a buffer smaller than 2 KiB; make their fakes emit more than 2 KiB of filler so `HEAD_START` still falls outside the retained tail. Message truncation stays at 500 chars.

Run: `cd src-tauri && cargo test && cd .. && yarn test`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/src/engine/processor.rs
git commit -m "fix(engine): atomic output commit, bounded stderr, unwind panics"
```

**Acceptance Criteria:**
- A cancelled or failed job leaves no file at the final output path and no `.yard-tmp`.
- A processor panic is reported as a file error and the batch continues; the shipped profile keeps `panic = "unwind"`.
- Retained stderr never exceeds 2 KiB.
- Gate passes clean: `yarn build && yarn test && cd src-tauri && cargo test`.
- Boundary rules enforced per `AGENTS.md` (frontend renders, Rust compresses).

**Required Tests:**
- `cancelled_copy_leaves_no_final_file` — cancellation during delivery, after a partial temp exists.
- `failed_copy_removes_temp_file` — write failure cleanup.
- `existing_destination_is_not_clobbered` — no data loss under a name race.
- `stderr_tail_retains_at_most_2_kib_but_drains_everything` — bounded retention, unbounded drain.
- `panic_is_caught_and_reported_as_file_error` — containment behaviour.
- `release_profile_keeps_unwind_panics` — the shipped profile cannot regress to abort.

---

### Task 2: Document streaming rewrite

**Files:**
- Modify: `src-tauri/src/processors/document.rs:68-73, 112-197, 323-536`
- Modify: `src-tauri/src/temp.rs` only if a new sweep helper is needed
- Test: inline module in `document.rs`

**Interfaces:**
- Consumes: `commit_output` from Task 1.
- Produces:
  - `struct EntryMeta { index: usize, name: String, is_dir: bool, compression: zip::CompressionMethod, size: u64 }`
  - `fn read_index(input: &Path) -> Result<Vec<EntryMeta>, CompressionError>`
  - `fn extract_media(input: &Path, metas: &[EntryMeta], temp: &Path) -> Result<HashMap<usize, PathBuf>, CompressionError>`
  - `fn write_archive(input: &Path, metas: &[EntryMeta], media: &HashMap<usize, PathBuf>, out: &Path) -> Result<(), CompressionError>`
  - `const MAX_UNCOMPRESSED_BYTES: u64 = 4 * 1024 * 1024 * 1024;`
  - `const MAX_COMPRESSION_RATIO: u64 = 200;`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn streaming_never_holds_more_than_one_media_file() { /* fake vips inspects temp dir at spawn time, assert 1 media file */ }

#[test]
fn non_media_entries_are_byte_identical() { /* XML part bytes equal after extraction */ }

#[test]
fn already_compressed_media_is_stored_not_deflated() { /* jpg, jpeg, PNG -> Stored; XML -> Deflated */ }

#[test]
fn media_never_grows() { /* fake vips emits a larger file -> original bytes written */ }

#[test]
fn zip_bomb_ratio_is_rejected() { /* ratio > 200 -> InputFileError */ }

#[test]
fn declared_uncompressed_total_is_rejected() { /* declared total > 4 GiB -> InputFileError */ }

#[test]
fn archive_is_written_once_per_accepted_quality() { /* counting writer records one finish per result */ }
```

- [ ] **Step 2: Run them and confirm they fail**

Run: `cd src-tauri && cargo test document::tests`
Expected: FAIL — `ZipEntry` has no streaming counterpart.

- [ ] **Step 3: Delete the snapshot type**

Remove `ZipEntry { content: Vec<u8> }` and `snapshot_archive`. `read_index` opens `ZipArchive<File>`, reads metadata only, and uses `enclosed_name()` for safety.

- [ ] **Step 4: Extract one media entry at a time**

`extract_media` iterates `by_index`, writes each media entry to `temp/media-{index}.{ext}` with `std::io::copy`, and drops the `ZipFile` before the next iteration. Non-media entries are not extracted.

- [ ] **Step 5: Measure the fixed part, then search quality**

Phase A writes every non-media entry into the destination `ZipWriter` and records `fixed_len`. Track it with a `CountingWriter<File>` wrapping the destination (a 20-line `Write` impl) rather than trusting the crate's buffering. Phase B runs the quality search against the extracted media files only; the predicted size is `fixed_len + sum(media file sizes) + 1 KiB` for the central directory. The archive is **not** rebuilt during the search.

- [ ] **Step 6: Write the media and finish**

Write the chosen media files `Stored` (they are already compressed), falling back to the original bytes when a recompressed entry is larger. `finish()` then `commit_output`. If the finished size still exceeds the target despite the 1 KiB margin, step one quality level down and rebuild once; never more than one retry.

- [ ] **Step 7: Update the tests whose premise changed, then run the gate**

- `document_rezip_uses_deflate_level_9` asserts `Deflated` for **every** entry; change it to XML `Deflated`, media `Stored`.
- The three `document_progress_*` and four `document_cancel_*` tests assume the old eight-pass rezip window; re-derive the expected bounds from the two-phase model.
- `document_cancel_during_rezip_preserves_preexisting_output_file` no longer proves ownership; replace it with the Task 1 no-clobber test at this layer.

Run: `cd src-tauri && cargo test document && cargo test`
Expected: PASS; the ignored large-file smoke remains ignored.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/processors/document.rs
git commit -m "perf(document): stream archive entries instead of snapshotting to RAM"
```

**Acceptance Criteria:**
- No path reads a whole archive entry into a `Vec`.
- At most one extracted media file exists at a time.
- The quality search never rebuilds the archive; at most one rebuild follows a failed size check.
- Entry set and non-media decompressed bytes are identical to the input.
- Archive ratio above 200:1 or declared total above 4 GiB is rejected before extraction.
- Gate passes clean: `yarn build && yarn test && cd src-tauri && cargo test`.
- Boundary rules enforced per `AGENTS.md`.

**Required Tests:**
- `streaming_never_holds_more_than_one_media_file` — memory shape, observed at engine spawn.
- `non_media_entries_are_byte_identical` — no silent document damage.
- `already_compressed_media_is_stored_not_deflated` — wasted CPU removed, per extension.
- `media_never_grows` — no entry larger than its original.
- `zip_bomb_ratio_is_rejected` — ratio guard.
- `declared_uncompressed_total_is_rejected` — size guard.
- `archive_is_written_once_per_accepted_quality` — no rebuild during search.
- `#[ignore] document_streams_a_256_mib_archive` — large-file smoke.

---

### Task 3: Memory budget, settings, disk preflight

**Files:**
- Create: `src-tauri/src/memory.rs`
- Modify: `src-tauri/Cargo.toml:30` (tokio `time`), dependencies (`sysinfo`)
- Modify: `src-tauri/src/settings.rs:87-99, 159-172`
- Modify: `src-tauri/src/scheduler.rs:28-98`
- Modify: `src-tauri/src/queue.rs:309-321`
- Modify: `src-tauri/src/lib.rs` (module list, register `memory:usage` emitter)
- Modify: `src/types.ts:60-64`, `src/components/SettingsDialog.tsx`, `src/hooks/useSettings.ts`
- Test: inline modules plus `src/components/SettingsDialog.test.tsx`

**Interfaces:**
- Produces:
  - `pub const BUDGET_FLOOR_BYTES: u64 = 300 * 1024 * 1024;`
  - `pub const BUDGET_CEILING_BYTES: u64 = 2 * 1024 * 1024 * 1024;`
  - `pub fn total_ram() -> u64`
  - `pub fn derive_budget_from_total_ram(adaptive: bool, ceiling_mb: u32, total_ram: u64) -> u64` (pure seam; `derive_budget` only reads `total_ram()` and delegates)
  - `pub fn estimate_job(category: Category, input_size: u64, width: Option<u32>) -> u64`
  - `pub fn available_space(path: &Path) -> Option<u64>`
  - `pub struct Performance { pub adaptive_memory: bool, pub memory_ceiling_mb: u32 }` with `#[serde(default)]` on the struct
  - `pub async fn run_budgeted_lanes<J, F, Fut, S, C, E>(lanes: Vec<LaneSpec<J>>, budget: u64, should_stop: S, run_job: F, on_skipped: C, estimate: E) -> u64` where `E: Fn(&J) -> u64`
  - `pub struct LaneSpec<J> { pub key: String, pub jobs: VecDeque<J> }` — `weight` is deleted

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn budget_clamps_between_300_mib_and_2_gib() { /* 1/4/8/16/32/64 GiB literal table */ }

#[test]
fn adaptive_off_uses_the_configured_ceiling() { /* ceiling 512 MiB -> 512 MiB; ceiling 4 GiB -> clamped 2 GiB */ }

#[test]
fn estimate_grows_with_input_size() { /* 1 MiB < 1 GiB */ }

#[test]
fn scheduler_waits_when_estimate_exceeds_budget() { /* 200 MiB + 200 MiB in a 300 MiB budget -> one at a time */ }

#[test]
fn oversize_job_runs_alone_instead_of_deadlocking() { /* 2 GiB estimate, 300 MiB budget, nothing running */ }

#[test]
fn old_settings_with_max_concurrent_still_load() { /* custom outputFolder + targets survive, new fields default */ }

#[test]
fn batch_fails_when_output_disk_is_short() { /* available_space stubbed below need */ }
```

- [ ] **Step 2: Run them and confirm they fail**

Run: `cd src-tauri && cargo test memory::tests scheduler::tests settings::tests`
Expected: FAIL — module does not exist and the scheduler has no budget argument.

- [ ] **Step 3: Implement `memory.rs`**

`total_ram()` reads `System::total_memory()` once at startup. `derive_budget_from_total_ram` returns `if adaptive { (total/8).clamp(FLOOR, CEILING) } else { (ceiling_mb * MiB).clamp(FLOOR, CEILING) }`. `estimate_job` uses one conservative constant per category: image `input * 4`, pdf `input * 3`, document `4 MiB` (bounded by Task 2), video `input + 400 MiB` at 1080p, scaled by width. Log the estimate once per job.

- [ ] **Step 4: Replace slot weights with the budget**

`scheduler.rs` takes `budget` and `estimate`, admits when `used + estimate <= budget`, and keeps one job per lane. **A job whose estimate alone exceeds the budget is admitted when nothing is running, with one warning** — otherwise a 2 GiB video on a 300 MiB budget deadlocks. Delete `weight` from `LaneSpec` and `lane_weight` (`queue.rs:868-870`). Delete the `max_concurrent <= 1` sequential branch (`queue.rs:333-361`); the budget scheduler already runs one job at a time when the budget is small, and the one-slot parity test moves to a one-job-at-a-time budget test.

- [ ] **Step 5: Migrate settings without resetting the file**

`Performance` gains `adaptive_memory` (default true) and `memory_ceiling_mb` (default 2048), and gets `#[serde(default)]` at the struct level. Without that attribute an old `{ "performance": { "maxConcurrent": "3" } }` fails deserialization and `parse_settings_value` (`settings.rs:266-274`) silently replaces **every** setting with defaults. Delete `max_concurrent` and `resolve_max_concurrent*` (`settings.rs:90, 96, 240-263`). Update call sites `queue.rs:323, 333, 399, 868, 893`, tests `settings.rs:387-428` and `queue.rs:1292-1305`, `src/types.ts:60-64`, `SettingsDialog.tsx:354-359`, and the fixtures in `App.test.tsx:99`, `SettingsDialog.test.tsx:61`, `useSettings.test.ts:44`.

- [ ] **Step 6: Add disk preflight and the usage event**

Before the first job, require `available_space(output_dir) >= largest_output * 1.1` and `available_space(temp_dir) >= largest_estimate * 0.5`. Emit `memory:usage` every 2 s while a batch runs: `{ budgetBytes, usedBytes, processRssBytes }`.

- [ ] **Step 7: Migrate the slot-era tests, then run the gate**

All 11 `scheduler.rs` tests and the 19 `queue.rs` tests that call `setup_app("1"|"2"|"3")` depend on the weight API. Rewrite them against explicit estimates and budgets; keep the invariants (one job per lane, stop-hook, panic isolation, output-name suffixes) and replace the `n1` sequential-order assertion with a one-job-at-a-time budget assertion.

Run: `cd src-tauri && cargo test memory scheduler settings queue && cd .. && yarn test`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/memory.rs src-tauri/src/scheduler.rs src-tauri/src/settings.rs src-tauri/src/queue.rs src-tauri/src/lib.rs src/types.ts src/components/SettingsDialog.tsx src/hooks/useSettings.ts
git commit -m "feat(memory): adaptive batch budget replaces fixed slots"
```

**Acceptance Criteria:**
- Budget stays within 300 MiB–2 GiB.
- No job starts when it does not fit the remaining budget, except a single oversize job running alone.
- Adaptive off pins the budget to the configured ceiling.
- Old settings files load without error and without resetting other fields.
- A disk shortfall fails the batch before any history record is written.
- Settings → Performance shows total RAM, budget, and live app RSS.
- Gate passes clean: `yarn build && yarn test && cd src-tauri && cargo test`.

**Required Tests:**
- `budget_clamps_between_300_mib_and_2_gib` — pure clamp table, both boundaries.
- `adaptive_off_uses_the_configured_ceiling` — manual mode.
- `estimate_grows_with_input_size` — monotonic estimate.
- `scheduler_waits_when_estimate_exceeds_budget` — admission.
- `oversize_job_runs_alone_instead_of_deadlocking` — budget deadlock guard.
- `old_settings_with_max_concurrent_still_load` — migration preserves unrelated fields.
- `batch_fails_when_output_disk_is_short` — preflight.

---

### Task 4: Image fidelity

**Files:**
- Modify: `src-tauri/src/processors/image.rs:45-64, 131-355`
- Test: inline module in `image.rs`

**Interfaces:**
- Produces:
  - `fn save_options(extension: &str, quality: u32) -> String`
  - `fn attempt_args(input: &Path, output: &Path, width: Option<u32>, quality: u32) -> Vec<String>`
- Consumes: `JobContext` arrives in Task 8; keep the current signature until then.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn jpeg_args_rotate_convert_to_srgb_and_keep_icc() { /* args contain --output-profile srgb, keep=icc, no strip */ }

#[test]
fn thumbnail_is_used_for_every_attempt() { /* first arg is "thumbnail", --size down, --fail-on error */ }

#[test]
fn webp_args_use_alpha_q_and_effort() { /* alpha-q=80, effort=6, smart-subsample */ }

#[test]
fn png_args_drop_the_ignored_colours_option() { /* no "colours" in args */ }

#[test]
fn icc_failure_fails_the_attempt_instead_of_flattening() { /* fake vips errors -> attempt failed, not delivered */ }
```

- [ ] **Step 2: Run them and confirm they fail**

Run: `cd src-tauri && cargo test image::tests`
Expected: FAIL — current args contain `strip` and no thumbnail invocation.

- [ ] **Step 3: Replace the save option strings and teach the fake `thumbnail`**

JPEG: `[Q={q},keep=icc,optimize-coding]`. PNG: `[compression=9,palette,Q=100,keep=icc]`. WebP: `[Q={q},effort=6,smart-subsample,alpha-q=80,keep=icc]`. No `strip`, no `colours`. Extend `write_fake_vips` to handle the `thumbnail` operation — today it implements only `copy|resize`, so the new first attempt writes nothing and six existing image tests fail with a zero-byte rejection.

- [ ] **Step 4: Use `thumbnail` for every attempt**

One invocation shape for all rungs:

```text
vips thumbnail <in> <out>[opts] <width> --size down --output-profile srgb --fail-on error
```

Rotation is on by default (`no-rotate` default false). Quality-only rungs pass `width = 100000`; `--size down` guarantees no upscale, so the image keeps its dimensions while rotation and the sRGB transform still apply. Downscale rungs pass the target long edge. `vips copy` is deleted from the processor.

- [ ] **Step 5: Run the tests and the gate**

Run: `cd src-tauri && cargo test image`
Expected: PASS. Then run the real-engine fixtures manually against the staged vips: `cargo test image::tests -- --ignored` — they are not part of the gate.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/processors/image.rs
git commit -m "fix(image): apply orientation, manage colour, use shrink-on-load"
```

**Acceptance Criteria:**
- Portrait JPEG (orientation 6) stays portrait and carries no orientation tag.
- Display-P3 input is transformed to sRGB and tagged `srgb`.
- No output is written without a profile.
- Downscale attempts use shrink-on-load.
- Gate passes clean: `yarn build && yarn test && cd src-tauri && cargo test`.

**Required Tests:**
- `jpeg_args_rotate_convert_to_srgb_and_keep_icc` — colour and orientation flags.
- `thumbnail_is_used_for_every_attempt` — no full decode.
- `webp_args_use_alpha_q_and_effort` — correct option names.
- `png_args_drop_the_ignored_colours_option` — dead option removed.
- `#[ignore] orientation_table_1_to_8_is_applied` — real vips fixtures, all eight tags.
- `#[ignore] display_p3_is_transformed_and_tagged_srgb` — real vips fixture; assert the output profile is sRGB, not "any profile".
- `icc_failure_fails_the_attempt_instead_of_flattening` — no silent colour shift.

---

### Task 5: Video fidelity and ladder arithmetic

**Files:**
- Modify: `src-tauri/src/processors/video.rs:45-101, 200-298, 686-917`
- Test: inline module in `video.rs`

**Interfaces:**
- Produces:
  - `fn video_bitrate_kbps(target_bytes: u64, duration_secs: f64, audio_kbps: u32) -> u32`
  - `fn stream_map_args() -> Vec<&'static str>`
  - `struct StreamInfo { duration_secs: f64, video: u32, audio: u32, subtitle: u32 }`
  - `fn probe_streams(path: &Path, ffprobe: &str) -> Result<StreamInfo, CompressionError>` — extends the ffprobe args with `-show_entries format=duration:stream=codec_type`
- Constants: `AUDIO_KBPS = 128`, `CONTAINER_OVERHEAD = 0.02`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn bitrate_budget_subtracts_audio_and_overhead() { /* literal kbps values; a missing subtraction fails */ }

#[test]
fn stream_maps_preserve_all_audio_and_subtitles() { /* args contain -map 0:a? and -map 0:s? */ }

#[test]
fn final_attempt_forces_8bit_faststart() { /* -pix_fmt yuv420p, -movflags +faststart */ }

#[test]
fn duration_mismatch_rejects_attempt() { /* output duration 10% short -> error */ }

#[test]
fn subtitle_encoder_is_explicit() { /* -c:s mov_text */ }

#[test]
fn audio_count_matches_input_unless_drop_is_granted() { /* 2 in -> 2 out; 1 out only with DropStream */ }
```

- [ ] **Step 2: Run them and confirm they fail**

Run: `cd src-tauri && cargo test video::tests`
Expected: FAIL — no mapping, no `pix_fmt`, no duration validation.

- [ ] **Step 3: Fix the bitrate budget**

`kbps = (target_bytes * 8 * 0.95 / duration) - AUDIO_KBPS`, then subtract 2 % container overhead. Clamp at 50 kbps and fail the attempt below it.

- [ ] **Step 4: Remove the duplicate-bitrate rungs**

Encode ABR once at the highest allowed resolution. Then run the CRF ladder at 720p and 480p, where the size actually changes. Keep the resolution floor filter.

- [ ] **Step 5: Add mapping and output flags**

Pass `-map 0:v:0 -map 0:a? -map 0:s? -map_chapters 0 -map_metadata 0`, `-c:a aac -b:a 128k`, `-c:s mov_text`, `-pix_fmt yuv420p`, `-preset medium`, `-movflags +faststart`. A subtitle conversion failure rejects the attempt and the ladder moves on. Task 10 adds the authorised path that drops the subtitle track instead.

- [ ] **Step 6: Validate the output and make the fake path-aware**

Probe the **output** with `probe_streams`. Require duration within 1 %, one video stream, and audio count not below the input count unless `DropStream` is granted (Task 10 wires the grant; until then the input count is the floor). Replace the constant-returning `fake_ffprobe` with one that reads its arguments and emits different values for input and output paths — otherwise every validation test passes vacuously.

- [ ] **Step 7: Run the tests and the gate**

Run: `cd src-tauri && cargo test video && cargo test`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/processors/video.rs
git commit -m "fix(video): truthful bitrate budget, stream preservation, 8-bit faststart output"
```

**Acceptance Criteria:**
- Output is `yuv420p` and `moov` precedes `mdat`.
- Duration is preserved within 1 %.
- All audio tracks survive unless dropping was authorised.
- No two encodes share a bitrate across resolutions.
- Gate passes clean: `yarn build && yarn test && cd src-tauri && cargo test`.
- Windows compile gate passes.

**Required Tests:**
- `bitrate_budget_subtracts_audio_and_overhead` — literal size math.
- `stream_maps_preserve_all_audio_and_subtitles` — no silent loss.
- `final_attempt_forces_8bit_faststart` — playback compatibility.
- `duration_mismatch_rejects_attempt` — output content validation, path-aware fake.
- `subtitle_encoder_is_explicit` — mp4 has no default subtitle encoder (verified).
- `audio_count_matches_input_unless_drop_is_granted` — stream parity.
- Existing `video_two_pass_commands_constructed_correctly`, `video_bitrate_formula_matches_spec`, `video_over_target_steps_resolution_1080_720_480`, `video_crf_floor_28_best_effort` are updated to the new argv and rung set, not deleted.

---

### Task 6: PDF fidelity

**Files:**
- Modify: `src-tauri/src/processors/pdf.rs:34-45, 72-138, 220-368`
- Modify: `src-tauri/src/engine/processor.rs` (stdout capture variant)
- Test: inline module in `pdf.rs`

**Interfaces:**
- Produces:
  - `fn run_sidecar_capture_stdout(...) -> Result<(ExitStatus, String), CompressionError>` in `engine/processor.rs` — the existing runner sends stdout to `Stdio::null()`, so page counting needs a variant; cap captured stdout at 64 KiB
  - `fn page_count(path: &Path, qpdf: &str) -> Result<u32, CompressionError>`
  - `fn validate_pdf(input: &Path, output: &Path, qpdf: &str) -> Result<(), CompressionError>`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn gs_output_with_fewer_pages_is_rejected() { /* fake qpdf returns 2 then 1 */ }

#[test]
fn rungs_are_ordered_by_aggressiveness() { /* strict dpi desc then jpegq desc */ }

#[test]
fn image_optimisation_is_marked_lossy() { /* strategy reports lossy encode */ }

#[test]
fn missing_qpdf_and_missing_gs_fail_fast() { /* both engines -> SidecarNotFound */ }

#[test]
fn page_count_probe_failure_rejects_the_attempt() { /* unparsable stdout */ }
```

- [ ] **Step 2: Run them and confirm they fail**

Run: `cd src-tauri && cargo test pdf::tests`
Expected: FAIL — no page-count check exists.

- [ ] **Step 3: Add stdout capture and page-count validation**

Add `run_sidecar_capture_stdout` (stderr still bounded, stdout piped and capped). Run `qpdf --show-npages <file>` and parse the single line. Compare input and output after every qpdf and gs attempt; reject on mismatch. Treat unparsable output as a failed attempt. Teach `fake_qpdf` to answer `--show-npages` with a count keyed by path; without that every existing PDF test stalls on a probe the fake does not understand.

- [ ] **Step 4: Reclassify, drop the duplicate rung, reorder**

Mark `qpdf-image-opt` as a lossy rung. Delete the separate `/screen` pass: it is a 72 dpi cell with different defaults, it duplicates the grid, and it makes the order non-monotonic. The ladder becomes lossless → image-opt → `for dpi in [100,72,50,36] { for jpegq in [60,40,25,15] }`, filtered by floors. `gs_args` always passes explicit `-r` and `-dJPEGQ`. Keep qpdf exit 2 tolerated.

- [ ] **Step 5: Update the PDF suite, then run the gate**

Every PDF integration test now sees extra `--show-npages` invocations and a stdout pipe. Update the call-order assertions (`pdf_dpi_jpegq_grid_walks_descending`, `pdf_pass3_gs_screen_stops_early`, `pdf_floor_above_screen_rung_skips_gs_screen_pass`) and the invocation counts; do not delete the behavioural assertions.

Run: `cd src-tauri && cargo test pdf && cargo test`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/processors/pdf.rs
git commit -m "fix(pdf): reject page loss, reclassify lossy pass, order rungs by aggressiveness"
```

**Acceptance Criteria:**
- A page-count mismatch is never delivered.
- Rungs run in strictly decreasing dpi, then decreasing JPEG quality; no duplicate 72 dpi rung exists.
- A failed rung does not stop the ladder.
- Gate passes clean: `yarn build && yarn test && cd src-tauri && cargo test`.

**Required Tests:**
- `gs_output_with_fewer_pages_is_rejected` — silent page loss, path-aware fake.
- `rungs_are_ordered_by_aggressiveness` — attempt order.
- `image_optimisation_is_marked_lossy` — honest reporting.
- `page_count_probe_failure_rejects_the_attempt` — unparsable stdout is a failed rung, not a pass.
- `missing_qpdf_and_missing_gs_fail_fast` — both engines, not just gs.

---

### Task 7: One file-type registry

**Files:**
- Create: `src-tauri/src/registry.rs`
- Modify: `src-tauri/src/detection.rs:19-32`, `src-tauri/src/processors/mod.rs:27-60`, `src-tauri/src/queue.rs:50-69`, `src-tauri/src/lib.rs` (module list)
- Test: inline module in `registry.rs`

**Interfaces:**
- Produces:
  - `pub struct FileType { pub ext: &'static str, pub category: Category, pub output_ext: &'static str }`
  - `pub const FILE_TYPES: &[FileType]`
  - `pub fn by_name(name: &str) -> Option<&'static FileType>`
  - `pub fn output_file_name(name: &str) -> String`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn accepted_extensions_match_the_previous_lists() { /* frozen per-category list */ }

#[test]
fn no_duplicate_extension_is_registered() { /* set length == const length */ }

#[test]
fn mov_and_heic_keep_their_output_extension_rules() { /* clip.mov -> clip.mp4, a.heic -> a.jpg */ }

#[test]
fn unknown_extension_stays_other_and_keeps_its_name() { /* a.xyz -> Other, a.xyz */ }

#[test]
fn every_row_agrees_across_detection_registry_and_naming() { /* one loop over FILE_TYPES, three sources agree */ }
```

- [ ] **Step 2: Run them and confirm they fail**

Run: `cd src-tauri && cargo test registry::tests`
Expected: FAIL — module does not exist.

- [ ] **Step 3: Build the table from today's behaviour**

Rows exactly as today: `jpg, jpeg, png, webp, heic, heif` (image); `mp4, mov` (video); `pdf`; `docx, xlsx, pptx` (document). Output ext: `heic|heif -> jpg`, `mov -> mp4`, everything else same as input. No `m4v` — it is not accepted today.

- [ ] **Step 4: Route the three call sites through it**

`detection::detect_category`, each processor's `supported_extensions`, and `queue::output_file_name` all read the table. Delete the three local lists. `image::get_output_extension` (`image.rs:45-54`) is deleted and replaced by `registry::output_file_name`; the three `get_output_extension_*` tests move to `registry.rs` with their assertions intact.

- [ ] **Step 5: Run the tests and the gate**

Run: `cd src-tauri && cargo test registry detection processors queue && cargo test`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/registry.rs src-tauri/src/detection.rs src-tauri/src/processors/mod.rs src-tauri/src/queue.rs src-tauri/src/lib.rs
git commit -m "refactor: single file-type registry for detection and output naming"
```

**Acceptance Criteria:**
- Accepted extension set per category is identical to today.
- Output naming is identical, including case-insensitivity.
- Adding a row is the only edit needed for detection and naming.
- Gate passes clean: `yarn build && yarn test && cd src-tauri && cargo test`.

**Required Tests:**
- `accepted_extensions_match_the_previous_lists` — no behaviour drift.
- `no_duplicate_extension_is_registered` — table integrity.
- `mov_and_heic_keep_their_output_extension_rules` — naming parity.
- `unknown_extension_stays_other_and_keeps_its_name` — fallback.
- `every_row_agrees_across_detection_registry_and_naming` — proves there is one source of truth, not three agreeing by luck.

---

### Task 8: `JobContext`, `Ladder`, real async

**Files:**
- Create: `src-tauri/src/engine/context.rs`, `src-tauri/src/engine/ladder.rs`, `src-tauri/src/bin/fake_engine.rs`
- Modify: `src-tauri/src/engine/processor.rs:322-338`, `src-tauri/src/engine/mod.rs`, all four processors, `src-tauri/src/processors/mod.rs`, `src-tauri/src/queue.rs:127-134,435-708`, `src-tauri/src/scheduler.rs`, `src-tauri/Cargo.toml`
- Test: inline modules and `src-tauri/tests/`

**Interfaces:**
- Produces:
  - `pub struct JobContext<'a> { pub input: &'a Path, pub output: &'a Path, pub target_bytes: u64, pub floors: &'a QualityFloors, pub cancel: CancellationToken, pub progress: ProgressSender, pub sidecars: &'a SidecarPaths, pub temp_dir: &'a Path }` — Task 9 adds the `policy` field.
  - `pub trait CompressionProcessor { fn category(&self) -> Category; fn supported_extensions(&self) -> &[&str]; fn compress(&self, ctx: &JobContext<'_>) -> Result<CompressionResult, CompressionError>; }`
  - `pub struct Ladder { /* best, attempts, temp_paths */ }`
  - `impl Ladder { pub fn new() -> Self; pub fn shortcut(&self, ctx: &JobContext<'_>, input_size: u64) -> Option<CompressionResult>; pub fn record(&mut self, ctx: &JobContext<'_>, outcome: AttemptOutcome) -> AttemptDecision; pub fn finish(self, ctx: &JobContext<'_>) -> Result<CompressionResult, CompressionError>; pub fn abandon(self); }`
  - `pub enum AttemptOutcome { Produced { path: PathBuf, size: u64, quality: u32, strategy: String }, Failed(CompressionError) }` — Task 9 extends `Produced` with `changes` and `consent`.
  - `pub enum AttemptDecision { Continue, Stop(CompressionResult) }`

- [ ] **Step 1: Write the failing tests**

```rust
#[tokio::test]
async fn job_body_runs_in_spawn_blocking() { /* assert runtime worker is not blocked during a sleeping fake engine */ }

#[test]
fn ladder_keeps_the_smallest_attempt() { /* 3 outcomes, strict smallest wins, ties keep earlier */ }

#[test]
fn ladder_cleans_attempts_on_cancel() { /* temp files removed, Cancelled returned */ }

#[test]
fn ladder_short_circuits_under_target() { /* copy strategy, no engine call */ }

#[test]
fn no_processor_contains_a_per_attempt_outcome_match() { /* source scan for the old pattern */ }

#[test]
fn job_context_carries_the_resolved_sidecars_temp_and_progress() { /* values arrive unchanged */ }

#[test]
fn progress_has_one_scale_across_attempts() { /* collector sees monotonic 0..=100, no double scaling */ }
```

- [ ] **Step 2: Run them and confirm they fail**

Run: `cd src-tauri && cargo test engine::ladder`
Expected: FAIL — module does not exist.

- [ ] **Step 3: Add `JobContext` and change the trait**

Delete the eight-argument method. Each processor reads everything from `ctx`. `ProcessorRegistry` is built once in `lib.rs` and stored in managed state.

- [ ] **Step 4: Implement `Ladder`**

`shortcut` copies when `input_size <= target`. `record` keeps the smallest strictly smaller result, deletes failed attempt files, checks cancellation, and returns `Stop` when the target is met. `finish` delivers the best attempt or the documented error. `abandon` removes attempt files.

- [ ] **Step 5: Move the job body to `spawn_blocking`**

`process_single_job` becomes a thin async wrapper: `tokio::task::spawn_blocking(move || run_job(...)).await`. Add tokio `time` for the consent wait in Task 10. Scheduler uses `JoinSet`.

- [ ] **Step 6: Add the cross-platform fake engine**

`src-tauri/src/bin/fake_engine.rs` is a small binary that reads `FAKE_ENGINE_MODE` and writes output files. Tests locate it via `std::env::current_exe()?.parent()?.join("fake_engine")`. Port one image test and one video test to run on Windows.

- [ ] **Step 7: Move the existing processor tests onto `Ladder` before deleting duplication**

Run the full processor suites with the new context. Only then delete the four duplicated `Ok/Err/Cancelled` blocks and the second binary search. The API change is a mechanical compile break across 93 processor tests (document 24, image 20, pdf 23, video 26); status, strategy, size and attempt-count assertions must survive unchanged, and only the call shape may change.

- [ ] **Step 8: Run the tests and the gate**

Run: `cd src-tauri && cargo test && cd .. && yarn test`
Expected: PASS. Behavioural assertions (status, strategy, size, attempt count) are unchanged; only the invocation shape moved to `JobContext`.

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src/engine src-tauri/src/processors src-tauri/src/queue.rs src-tauri/src/scheduler.rs src-tauri/src/lib.rs src-tauri/src/bin/fake_engine.rs src-tauri/Cargo.toml
git commit -m "refactor(engine): job context, ladder state, spawn_blocking job bodies"
```

**Acceptance Criteria:**
- No processor contains a per-attempt `Ok/Err/Cancelled` match.
- Cancellation, cleanup, best tracking and recording exist in one place.
- Existing processor tests keep their behavioural assertions; only the call shape changes.
- A Windows test spawns a fake engine and compresses a file.
- Release builds contain panics within a job.
- Gate passes clean: `yarn build && yarn test && cd src-tauri && cargo test`.

**Required Tests:**
- `job_body_runs_in_spawn_blocking` — runtime not parked.
- `ladder_keeps_the_smallest_attempt` — selection rule.
- `ladder_cleans_attempts_on_cancel` — cleanup.
- `ladder_short_circuits_under_target` — copy path.
- `no_processor_contains_a_per_attempt_outcome_match` — the duplication is actually gone.
- `job_context_carries_the_resolved_sidecars_temp_and_progress` — no field is re-derived differently.
- `progress_has_one_scale_across_attempts` — no double scaling after the move.
- `windows_fake_engine_compresses_a_file` — platform coverage; assert the binary exists rather than early-returning when missing.

---

### Task 9: Capability matrix and policy

**Files:**
- Create: `src-tauri/src/policy.rs`
- Modify: `src-tauri/src/settings.rs`, `src-tauri/src/lib.rs` (module list), `src-tauri/src/engine/result.rs:21-28`, `src-tauri/src/models.rs:29-40`, `src/types.ts:23-31`, `src-tauri/src/queue.rs:71-80`, `src-tauri/src/engine/context.rs`, `src-tauri/src/engine/ladder.rs`
- Test: inline modules in `policy.rs`, `models.rs`

**Interfaces:**
- Produces:
  - `pub enum Change { LossyEncode, Downscale, DropStream, FormatChange, MetadataStrip, Rasterize, PageDrop }`
  - `pub enum Verdict { Allowed, NeedsConsent, Denied }`
  - `pub fn verdict(category: Category, change: Change) -> Verdict`
  - `pub struct Policy { /* global + per-category + batch grants */ }`
  - `impl Policy { pub fn check(&self, category: Category, changes: &[Change]) -> Verdict }`
  - `pub enum ConsentSource { Default, Global, Category, Batch, Once }`
  - `FileStatus::NeedsPermission` serialized as `needs_permission`
  - `CompressionResult { changes: Vec<Change>, consent: ConsentSource }`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn matrix_matches_the_agreed_table() { /* literal 7x5 table written in the test, not read from the constant */ }

#[test]
fn denied_change_returns_needs_permission_and_no_output() { /* PageDrop denied */ }

#[test]
fn batch_grant_overrides_a_needs_consent_verdict() { /* Downscale granted for batch */ }

#[test]
fn best_effort_requires_every_change_granted() { /* missing grant -> NeedsPermission */ }

#[test]
fn status_union_matches_typescript() { /* read CARGO_MANIFEST_DIR/../src/types.ts, assert every FileStatus literal appears */ }

#[test]
fn declared_capability_mismatch_is_rejected() { /* rung declares [] but requests DropStream -> error */ }
```

The existing `models::file_status_serde_matches_typescript_union` is tautological: it compares against a hardcoded array and never opens `src/types.ts`. Replace it with a test that reads `../src/types.ts` from `CARGO_MANIFEST_DIR` so a missing TS member actually fails. The old-history deserialization test belongs to Task 10, where the fields are added.

- [ ] **Step 2: Run them and confirm they fail**

Run: `cd src-tauri && cargo test policy::tests models::tests`
Expected: FAIL — no policy module, no `NeedsPermission` variant.

- [ ] **Step 3: Add the enum, matrix, and the context extension**

Seven changes; matrix exactly as agreed in ADR 0012. `MetadataStrip` is `Allowed` for bulky non-visual data only; orientation, ICC and gain maps are never stripped and are not a matrix row that can be granted. Add `policy: &'a Policy` to `JobContext` and `changes: Vec<Change>` + `consent: ConsentSource` to `AttemptOutcome::Produced`; fill them in every processor and in `Ladder::record`.

- [ ] **Step 4: Resolve consent**

`Policy::check` returns `Denied` if any change is denied, `NeedsConsent` if any needs consent and no grant covers it, otherwise `Allowed`. Grants are checked per category and per batch.

- [ ] **Step 5: Add the status and result fields**

`NeedsPermission` in Rust and TypeScript, `changes` and `consent` on `CompressionResult`. `decide_result_status` uses them. Add `needs_permission` to `history::Counts` (`history.rs:71-80`) and `needsPermission` to the TypeScript `SessionOutcome` (`src/types.ts:94-101`); update every exhaustive match, including `history.rs:225-242` and the summary cards.

- [ ] **Step 6: Update every exhaustive status consumer, then run the gate**

Rust: `models.rs` exhaustive match and `[String; 8]`, `queue::status_key` (`all.len() == 8`), `queue::history_counts_of`, `history::Counts` and `record_schema_keys_are_pinned`. Frontend: `src/lib/tokens.ts` `STATUS_TONES`, `src/lib/tokens.test.ts` `STATUS_KEYS`, `src/lib/historyTable.ts` `HISTORY_STATUS_LABELS`, `src/lib/grouping.ts`, `RunCard`, `RunBoard`, `HistoryFilesTable`, `HistorySummaryCards`. Each is a `Record<FileStatus, …>` and fails typecheck until extended.

Run: `cd src-tauri && cargo test policy models engine history queue && cd .. && yarn test && yarn build`
Expected: PASS, including the file-reading union test.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/policy.rs src-tauri/src/settings.rs src-tauri/src/engine/result.rs src-tauri/src/models.rs src/types.ts src-tauri/src/queue.rs
git commit -m "feat(policy): capability matrix and consent resolution"
```

**Acceptance Criteria:**
- A denied capability produces `needs_permission`, no output file, and a reason.
- `best_effort` is reachable only when every required change was granted.
- Metadata that affects appearance cannot be granted away.
- Rust and TypeScript status unions stay in lockstep.
- Gate passes clean: `yarn build && yarn test && cd src-tauri && cargo test`.

**Required Tests:**
- `matrix_matches_the_agreed_table` — full table snapshot.
- `denied_change_returns_needs_permission_and_no_output` — denial path.
- `batch_grant_overrides_a_needs_consent_verdict` — scope precedence.
- `best_effort_requires_every_change_granted` — outcome gating.
- `status_union_matches_typescript` — contract drift, read from the real TS file.
- `declared_capability_mismatch_is_rejected` — a rung cannot change what it did not declare.

---

### Task 10: Consent surfaces and history

**Files:**
- Modify: `src-tauri/src/queue.rs:82-121, 797-829`, `src-tauri/src/history.rs:41-59, 90-106`
- Create: `src/hooks/useConsent.ts`, `src/components/run/ConsentPrompt.tsx`, `src/components/run/PreflightDialog.tsx`
- Modify: `src/components/run/RunCard.tsx`, `src/components/run/RunBoard.tsx`, `src/components/run/RunHeader.tsx`, `src/views/CompressView.tsx`, `src/components/history/HistoryFileDetail.tsx`, `src/components/history/HistorySummaryCards.tsx`, `src/views/HistoryView.tsx`, `src/types.ts`
- Test: `src/hooks/useConsent.test.ts`, `src/components/run/ConsentPrompt.test.tsx`, inline Rust tests

**Interfaces:**
- Produces:
  - Event `consent:required` payload `{ requestId, fileId, changes, detail }`
  - Command `answer_consent { requestId, allow, scope }` where `scope` is `once | batch`
  - `FileResultRecord` and `HistoryFile` gain `changes: Vec<Change>` and `consent_source: Option<ConsentSource>`; `FileCompletePayload` gains the same two fields so the live card can show them
  - `ConsentSource` serialized `default | global | category | batch | once`

- [ ] **Step 1: Write the failing tests**

```rust
#[tokio::test]
async fn consent_request_resolves_with_the_answer() { /* oneshot round-trip */ }

#[tokio::test]
async fn consent_timeout_denies() { /* paused clock, 30 min -> deny */ }

#[tokio::test]
async fn batch_grant_resolves_future_requests() { /* second file does not emit */ }

#[tokio::test]
async fn cancel_while_waiting_ends_the_job() { /* every pending request resolves, Cancelled, no output */ }

#[tokio::test]
async fn batch_grant_is_cleared_when_the_batch_ends() { /* next batch prompts again */ }

#[tokio::test]
async fn late_answer_after_timeout_cannot_flip_to_allow() { /* request id already retired */ }

#[tokio::test]
async fn denying_one_capability_does_not_silence_another() { /* streak is per capability */ }

#[test]
fn old_history_lines_without_changes_still_load() { /* serde(default) on both new fields */ }

#[test]
fn history_records_changes_and_consent_source() { /* populated values survive fold + serialize */ }
```

```tsx
it('renders an inline prompt, never a modal', () => { /* role="dialog" absent, buttons present */ });
it('fifty heic files produce one preflight prompt', () => { /* useConsent reducer */ });
it('apply to rest resolves requests already waiting', () => { /* not only future ones */ });
it('a batch with no destructive change never prompts', () => { /* zero requests */ });
```

- [ ] **Step 2: Run them and confirm they fail**

Run: `cd src-tauri && cargo test consent && cd .. && yarn test consent`
Expected: FAIL — no event, no command, no hook.

- [ ] **Step 3: Add the Rust consent registry**

`Mutex<HashMap<String, oneshot::Sender<ConsentAnswer>>>` on `QueueState`. Emit `consent:required`, wait with `tokio::time::timeout(Duration::from_secs(1800))`, and default to deny. Cancel while waiting resolves as `Cancelled`.

- [ ] **Step 4: Add pre-flight classification**

Before the first job, ask the registry which extensions imply a `FormatChange` (today only `heic|heif`). If any do and the policy needs consent, emit one `consent:required` with `fileCount` and wait once.

- [ ] **Step 5: Build the inline UI**

The file card flips to `waiting` with Allow/Skip and "apply to the rest of this batch". The batch header shows a waiting count. No modal, no focus steal. Pre-flight uses a single dialog because it is one decision for the whole batch.

- [ ] **Step 6: Silence repeated denials**

Two denials of the same capability in one batch suppress further prompts for that capability and report the count at the end.

- [ ] **Step 7: Record consent in history**

Append `changes` and `consentSource` on the file record; show them in the history detail card. Update `src/types.ts` and the history tests.

- [ ] **Step 8: Run the tests and the gate**

Run: `cd src-tauri && cargo test consent history && cd .. && yarn test && yarn build`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src/queue.rs src-tauri/src/history.rs src/hooks/useConsent.ts src/components src/views src/types.ts
git commit -m "feat(ui): consent preflight, inline prompts, and consent history"
```

**Acceptance Criteria:**
- A 50-HEIC batch produces one pre-flight prompt.
- Waiting never opens a modal and never blocks the UI thread.
- Timeout and cancellation resolve every pending request.
- History explains why a file was downscaled or lost a track.
- Gate passes clean: `yarn build && yarn test && cd src-tauri && cargo test`.

**Required Tests:**
- `consent_request_resolves_with_the_answer` — round-trip.
- `consent_timeout_denies` — unattended default.
- `batch_grant_resolves_future_requests` — scope.
- `batch_grant_is_cleared_when_the_batch_ends` — no consent leak into the next batch.
- `cancel_while_waiting_ends_the_job` — every pending request resolves, batch does not stall.
- `late_answer_after_timeout_cannot_flip_to_allow` — no stale-id confusion.
- `denying_one_capability_does_not_silence_another` — streak scope.
- `renders_an_inline_prompt, never a modal` — UX contract.
- `fifty_heic_files_produce_one_preflight_prompt` — volume control.
- `apply_to_rest_resolves_requests_already_waiting` — batch scope covers the queue, not just the future.
- `a_batch_with_no_destructive_change_never_prompts` — no noise.
- `history_records_changes_and_consent_source` — audit trail; assert populated values, not just the keys.
- `old_history_lines_without_changes_still_load` — existing history survives the schema addition.

---

### Task 11: Documentation truth pass

**Files:**
- Modify: `README.md`, `docs/ARCHITECTURE.md`, `docs/FEATURES.md`, `GLOSSARY.md`, `docs/bugs.md`, `CHANGELOG.md`
- Delete: this plan, after the final slice merges

**Interfaces:**
- Consumes: shipped behaviour from Tasks 1–10.
- Produces: documentation that matches the code.

- [ ] **Step 1: Diff every documented claim against the code**

Check the memory model, the status list, the compression table, the settings schema and the consent model.

- [ ] **Step 2: Update the memory and compression sections**

`ARCHITECTURE.md` §6 and §10 get the streaming document path, the budget, and the consent ladder. The stale `panic = "abort"` note is deleted.

- [ ] **Step 3: Update the glossary and README**

Add any term that shipped differently. Remove the "metadata is stripped" claim where it is no longer true.

- [ ] **Step 4: Update `bugs.md`**

Close the six items fixed by Tasks 1–9. Keep what remains.

- [ ] **Step 5: Run the gate one final time**

Run: `yarn build && yarn test && cd src-tauri && cargo test`
Expected: PASS.

- [ ] **Step 6: Commit and delete the plan**

```bash
git add README.md docs GLOSSARY.md CHANGELOG.md
git commit -m "docs: record bounded memory and consent ladder"
git rm docs/superpowers/plans/2026-11-05-backend-remediation.md
git commit -m "docs: remove completed remediation plan"
```

**Acceptance Criteria:**
- No documented claim contradicts the code.
- Every closed bug in `bugs.md` is verified closed by a named test.
- Gate passes clean: `yarn build && yarn test && cd src-tauri && cargo test`.
- Boundary rules enforced per `AGENTS.md`.

**Required Tests:**
- No new tests; the task is verified by the full gate plus a manual claim-by-claim diff.
