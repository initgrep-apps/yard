# Best effort at the quality floor, never worse

**Status:** superseded by ADR 0011 — silent best-effort delivery is replaced by a consent-gated ladder.

Every category has a user-editable quality floor. When the target cannot be met at the floor, the compressor stops, delivers the smallest attempt, and reports `met_target: false` with status `best_effort`. Continuing to degrade until the number fits was rejected: it silently destroys the quality the user said was unacceptable, and a size target is advisory, not a contract.

## Considered Options

- **Stop at the floor, deliver best effort (chosen)** — the floors are the real guarantee; the user sees a visible "target not met" result.
- **Keep degrading until the file fits** — reaches the byte target by violating the only quality signal the user configured.
- **Fail the job** — wastes completed work and hides a usable file behind an error.

## Consequences

- A `best_effort` result can exceed its target; the UI and history must show that plainly.
- Default floors: image Q30, video CRF 28 at 480p minimum, PDF 72 dpi / JPEGQ 15, document media Q40.
- One failed pass never aborts a ladder; the next rung still runs.
