# Per-file memory is bounded; the batch budget adapts to the machine

**Status:** accepted · supersedes ADR 0003 (slot budget only; category lanes are retained)

Peak RAM for one job must not grow with input file size. The document path currently holds the whole unpacked archive in memory (`~2.5 ×` uncompressed, `~8–12 ×` the compressed file), so a 2 GB PPTX needs roughly 20 GB. Inputs may be any size, so no fixed cap can make that safe. Instead, every processor streams: the largest in-memory unit is one archive entry, one image, or one engine read buffer. Parallelism is governed by a memory budget derived from the machine — `clamp(total_ram / 8, 300 MiB, 2 GiB)` — adaptive by default and overridable to a fixed ceiling. Engine children are never assumed free: each job carries a conservative estimate from a static per-category table, and the scheduler admits a job only if the estimate fits the remaining budget.

## Considered Options

- **Bounded shape + adaptive budget (chosen).** Safe on any machine and any file size; adaptivity only buys concurrency, never buffering.
- **Fixed slots (ADR 0003).** Blind to bytes: four small JPEGs and two 500 MB decks cost the same; the only defence is a hard file-size limit.
- **O(size) with a cap.** A cap is always exceedable by a larger input, and the machine does not fail cleanly — macOS compresses and swaps, so it crawls instead of erroring.
- **Hard OS memory caps on engine children.** Unreliable on macOS (`RLIMIT_AS` is not honoured the way Linux does) and produces opaque engine failures instead of a clean skip.

## Consequences

- Document compression trades RAM for temp disk. Temp usage grows; the ceiling does not.
- A file can no longer fail with "too big to process"; it can still be slow.
- Swap no longer causes machine-wide stalls during a batch.
- `maxConcurrent` as a slot count disappears from settings; the user-facing control becomes "Adaptive memory" on/off plus a fixed ceiling.
- Admission depends on estimates, not measurements. Estimates are conservative and logged; children are never killed for exceeding them.
