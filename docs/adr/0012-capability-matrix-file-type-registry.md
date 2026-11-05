# One file-type registry and one capability matrix

Extension knowledge currently lives in three places — `detection.rs`, each processor's `supported_extensions`, and the output-name rewrite in `queue.rs` — so adding a file type means three edits and a silent failure if one is missed. Replace them with one registry table: extension → category → output extension → processor → declared capabilities. Consent rules become a second table, `(category, capability) → allowed | needs consent | denied`, where capability is one of a closed set of seven change kinds: lossy encode, downscale, drop stream, format change, metadata strip, rasterise, page drop. Each ladder rung declares the capabilities it needs; one policy lookup returns the verdict. There is no rule engine, no config file and no scripting surface — the tables are Rust constants.

## Considered Options

- **Two constant tables (chosen).** Adding a file type is one row; adding a capability is one enum variant and one column; policy is testable as data.
- **Scattered conditionals.** The current pattern: `if category == … && rung >= … && !settings.…`, copied per processor.
- **Generic rules engine / config file.** A second language to maintain in an offline app, for a matrix that fits on one screen.
- **Per-processor consent checks.** Correct until one processor drifts, then the same file type behaves differently by path.

## Consequences

- Detection, the processor registry, output naming and the capability matrix all read the same rows.
- A rung cannot change something it did not declare; the policy lookup is the only place that decides.
- `MetadataStrip` is not a consent row: orientation and ICC are never stripped, and bulky non-visual data is stripped silently and reported.
- A new file type still needs a processor, but no other file needs editing.
