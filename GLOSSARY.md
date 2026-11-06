# Yard — Language

Offline desktop app: users batch-compress files to size targets; all compression engines ship inside the app. This glossary fixes the vocabulary used across code, scripts, docs, and CI.

## Language

**Engine**:
One of the five offline compression tools shipped inside the app: vips, ffmpeg, ffprobe, qpdf, gs.
_Avoid_: binary (ambiguous), tool, dependency

**Sidecar**:
An engine invoked by the Rust core as a separate OS process, not linked into the app binary.
_Avoid_: subprocess, CLI tool

**Target triple**:
A Rust-style platform identifier (e.g. `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`) naming one build target. Four are supported; Linux has none.
_Avoid_: arch, platform (ambiguous)

**Manifest**:
`scripts/binaries.manifest.json` — the version-pinned download recipe stating, per target triple, where each engine comes from, its expected version, and its checksums.
_Avoid_: binary config

**Lock**:
`scripts/binaries.lock.json` — the provenance record of what was actually staged (versions, sha256 of downloaded artifacts and staged files).
_Avoid_: changelog

**Staged binaries**:
Engine files placed under `src-tauri/` (binaries, engines, frameworks, gs-resources) by the fetch/prep pipeline — the inputs `tauri build` consumes. Gitignored; re-staged per machine.
_Avoid_: built binaries

**Offline**:
The app property that compression works with no network and no package manager on the user's machine, because engines and their runtime files (frameworks, gs-resources) ship inside the package.
_Avoid_: embedded, self-contained

**Engines folder**:
The Windows staging layout `src-tauri/binaries/engines/<engine>/` holding each engine's exe with its DLLs; shipped as `resources/engines/` on Windows. macOS engines ship as externalBin instead.
_Avoid_: DLL dump

**Frameworks folder**:
The macOS dylib tree `src-tauri/frameworks/` collected from Homebrew by the bundle scripts and shipped in the .app's Frameworks; rewritten to `@rpath`.

**gs-resources**:
The staged Ghostscript resource tree shipped with the app so gs resolves fonts and ICC profiles locally instead of a Homebrew prefix.

**Bundle scripts**:
`scripts/bundle-vips-macos.sh` and `scripts/bundle-pdf-macos.sh` — make macOS engines self-contained: dylib collection, `@rpath` rewrite, rpath injection, ad-hoc signing.
_Avoid_: packaging script

**Freeze bundle**:
A version-pinned archive of one target triple's staged binaries, produced once and consumed by CI release builds instead of re-staging from Homebrew.
_Avoid_: cache, artifact (generic)

**Sidecars release**:
A dedicated GitHub Release (tag prefix `sidecars-`) holding freeze bundles for all target triples.

**Gate**:
The required pre-build test command (`yarn build && yarn test && cd src-tauri && cargo test`) that must pass on every platform.
_Avoid_: tests, checks

**Shell**:
The persistent window frame around the views: sidebar, top bar, dialogs, and the providers that own their state. It never unmounts; only the active view changes.
_Avoid_: layout, chrome (window chrome is the OS frame — ADR 0007)

**Top bar**:
The 48px row across the content area: breadcrumb, theme toggle, ⋯ menu, and — on Windows/Linux — the window controls. While the sidebar is hidden it also carries the expand trigger and ☰. It doubles as the drag surface.
_Avoid_: header, title bar (the title bar belongs to the OS or the macOS overlay)

**Sidebar header**:
The 48px row atop the sidebar: the brand on non-macOS and mobile, and the collapse trigger at the right edge. Hidden with the sidebar when it collapses; the top bar takes over its actions.
_Avoid_: sidebar bar, sidebar toolbar

**Batch list**:
The scrollable list of history sessions inside the sidebar. Selecting a row focuses that batch's detail and adds its label to the breadcrumb.
_Avoid_: history list, sidebar list

**Batch**:
One Compress action: the files staged when the user presses Compress, processed as one unit. Batches never overlap.
_Avoid_: run, job, queue (a job is one file inside a batch)

**Session**:
A batch as recorded in history: a start record, one record per file, an end record. A session with no end record is `interrupted`.
_Avoid_: batch (in history context), log

**Staging list**:
The pre-batch file list the user builds by dropping or browsing files. Cleared by "New batch".
_Avoid_: queue, pending list

**Lane**:
The per-category queue inside a batch (image, video, pdf, document, other). One job runs per lane at a time.
_Avoid_: queue, pool

**Attempt**:
One complete try at producing an output: one ladder rung. It may run one engine command (two for a two-pass encode) and, for documents, one vips run per recompressible media entry.
_Avoid_: pass, run, iteration

**Rung**:
One step of a ladder. A rung declares the command, the capabilities it needs, and how its output is validated.
_Avoid_: step, stage

**Ladder**:
The ordered list of rungs a job may try, from least to most destructive.
_Avoid_: pipeline, chain

**Lossless pass**:
A rung that changes encoding or container without discarding pages, tracks, pixels or profile data.
_Avoid_: safe mode, compression

**Remux**:
Rewriting the container without re-encoding the streams. Lossless, and it does not reduce size.
_Avoid_: convert, rewrap

**Lossy encode**:
Re-encoding that lowers fidelity while keeping the same contents: every page, track and frame, in the same format.
_Avoid_: destructive change, compression

**Fidelity ladder**:
The shipped attempt order: copy when already under target, lossless pass, lossy encode inside the quality floors, destructive change only with consent, then best effort or `needs_permission`. Each rung declares the changes it can make; the ladder rejects an undeclared change.
_Avoid_: fallback chain, retry loop

**Destructive change**:
A change the user can notice in the result: fewer pixels, a missing track or page, a different format, rasterised pages. Requires consent unless a setting grants it.
_Avoid_: lossy, degradation

**Batch budget**:
The total RAM a running batch may use, derived from the machine's RAM (`clamp(total/8, 300 MiB, 2 GiB)`) or fixed by `memoryCeilingMb` and clamped the same way. Replaces the old slot count.
_Avoid_: slot, concurrency limit, thread pool

**Job estimate**:
The conservative peak RAM attributed to one job for admission: image 4 × input, PDF 3 × input, document 4 MiB, video input + 400 MiB (width-scaled when known). An estimate, never a measurement, and never a kill threshold.
_Avoid_: memory limit, job ceiling, engine memory

**Registry**:
`registry.rs` — the single file-type table mapping extension → category → output extension. Detection, each processor's accepted extensions, output naming and the capability matrix's category columns all read it.
_Avoid_: type map, extension list

**Target**:
The per-category size cap a batch aims for (image 200 KB, video 10 MB, PDF 2 MB, document 1 MB by default).
_Avoid_: limit, max size

**Quality floors**:
The user's minimum acceptable quality per category (image Q30, video CRF 28 at 480p, PDF 72 dpi / JPEGQ 15, document media Q40). The compressor stops here rather than degrading further.
_Avoid_: limits, thresholds

**Resolution floor**:
The smallest size the user accepts. A downscale below it is denied; any downscale still needs consent. Video 480p, image long edge 1280 px.
_Avoid_: minimum size, scale limit

**Free zone**:
The degradation the user already authorised through the quality floors. Lossy re-encoding inside the floors needs no consent; downscaling above the resolution floor still does, and below the floor it is denied.
_Avoid_: allowed loss, safe range

**Capability**:
One kind of change a rung can make: lossy encode, downscale, drop stream, format change, metadata strip, rasterise, page drop.
_Avoid_: permission, flag

**Capability matrix**:
The 7 × 5 table in `policy.rs` that maps every capability and category to allowed, needs consent, or denied.
_Avoid_: rules engine, policy config

**Consent**:
An explicit user decision permitting a specific destructive change for a stated scope: once, this batch, or the global allow-destructive setting.
_Avoid_: approval, confirmation

**Consent source**:
Where an authorisation came from: `default` (allowed with no grant, including the built-in HEIC → JPEG permission), `global`, `category` (policy-level, no shipped UI control), `batch`, or `once`. History records the source only when it is not `default`.
_Avoid_: consent type, scope

**Pre-flight consent**:
One prompt before a batch for destructive changes that can be predicted from the file type alone, such as HEIC to JPG when the default permission is off.
_Avoid_: batch warning, upfront dialog

**Needs permission**:
The `needs_permission` status of a file whose required destructive change was denied or left unanswered. Nothing is delivered, and any delivered file is removed.
_Avoid_: failed, blocked, best effort

**Best effort**:
The outcome of a file where every required change was authorised but the target still could not be reached. The smallest attempt is delivered.
_Avoid_: needs permission, failed
