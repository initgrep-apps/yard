# Category lanes with a weighted slot budget

**Status:** superseded by ADR 0010 — the weighted slot budget is replaced by a memory budget; category lanes are retained.

Compression runs one FIFO lane per category with at most one job per lane, inside a global slot budget of `clamp(cores / 2, 1, 4)`; a video costs 2 slots, everything else 1. A free-for-all pool was rejected because two concurrent x264 encodes each use every core and two image jobs multiply memory spikes without finishing sooner, while strict sequential (v1) left the machine idle behind a single long video. Weighting video at 2 is what stops back-to-back encodes even when the budget is 4.

## Considered Options

- **Lanes + weighted slots (chosen)** — category serialization is cheap to reason about, cross-category work still overlaps, and video gets protection without a special-case scheduler.
- **Strict sequential** — correct but underuses the machine; images and PDFs lost minutes waiting behind video.
- **Unbounded pool** — no protection against CPU/RAM spikes; not viable with engines that saturate cores by default.
- **Per-job thread pool without lanes** — allows two videos at once and makes cancel/cleanup ordering hard to test.

## Consequences

- `auto` on 8 cores means 4 slots; on 2 cores it means 1, which bypasses the scheduler and replays list order.
- Two videos never run at once, even at 4 slots; images never run in parallel with each other.
- The scheduler is a standalone generic function with no Tauri dependency, pinned by a parity test at one slot and by cross-lane/cancel tests in `queue.rs`.
